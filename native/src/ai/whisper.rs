//! Whisper speech-to-text running in-process on candle (pure Rust, CPU).
//!
//! Ported from `candle-examples/examples/whisper` (Apache-2.0, Copyright
//! Hugging Face / Laurent Mazare): the streaming decoder with OpenAI's
//! timestamp rules is kept verbatim where possible; model files come from the
//! downloadable components directory instead of the hub, audio decoding uses
//! the built-in WAV parser instead of symphonia, and the language is forced
//! to Russian (models tiny/base/small are multilingual).

use std::path::Path;

use candle_core as candle;
use candle_core::{Device, IndexOp, Tensor};
use candle_nn::{
    ops::{log_softmax, softmax},
    VarBuilder,
};
use candle_transformers::models::whisper::{self as m, audio, Config};
use rand09::distr::weighted::WeightedIndex;
use rand09::distr::Distribution;
use rand09::SeedableRng;
use tokenizers::Tokenizer;

use crate::ai::resample_linear;

pub const SAMPLE_RATE: u32 = m::SAMPLE_RATE as u32;

enum Model {
    Normal(m::model::Whisper),
}

impl Model {
    fn config(&self) -> &Config {
        match self {
            Model::Normal(m) => &m.config,
        }
    }

    fn encoder_forward(&mut self, x: &Tensor, flush: bool) -> candle::Result<Tensor> {
        match self {
            Model::Normal(m) => m.encoder.forward(x, flush),
        }
    }

    fn decoder_forward(&mut self, x: &Tensor, xa: &Tensor, flush: bool) -> candle::Result<Tensor> {
        match self {
            Model::Normal(m) => m.decoder.forward(x, xa, flush),
        }
    }

    fn decoder_final_linear(&self, x: &Tensor) -> candle::Result<Tensor> {
        match self {
            Model::Normal(m) => m.decoder.final_linear(x),
        }
    }
}

struct DecodingResult {
    tokens: Vec<u32>,
    avg_logprob: f64,
    no_speech_prob: f64,
}

struct Decoder {
    model: Model,
    rng: rand09::rngs::StdRng,
    tokenizer: Tokenizer,
    suppress_tokens: Tensor,
    sot_token: u32,
    transcribe_token: u32,
    eot_token: u32,
    no_speech_token: u32,
    no_timestamps_token: u32,
    language_token: Option<u32>,
}

impl Decoder {
    fn new(
        model: Model,
        tokenizer: Tokenizer,
        seed: u64,
        device: &Device,
        language_token: Option<u32>,
    ) -> Result<Self, String> {
        let no_timestamps_token = token_id(&tokenizer, m::NO_TIMESTAMPS_TOKEN)?;
        let suppress_tokens: Vec<f32> = (0..model.config().vocab_size as u32)
            .map(|i| {
                if model.config().suppress_tokens.contains(&i) || i == no_timestamps_token {
                    f32::NEG_INFINITY
                } else {
                    0f32
                }
            })
            .collect();
        let suppress_tokens = Tensor::new(suppress_tokens.as_slice(), device).map_err(e2s)?;
        let sot_token = token_id(&tokenizer, m::SOT_TOKEN)?;
        let transcribe_token = token_id(&tokenizer, m::TRANSCRIBE_TOKEN)?;
        let eot_token = token_id(&tokenizer, m::EOT_TOKEN)?;
        let no_speech_token = m::NO_SPEECH_TOKENS
            .iter()
            .find_map(|token| token_id(&tokenizer, token).ok())
            .ok_or_else(|| "unable to find any non-speech token".to_string())?;
        Ok(Self {
            model,
            rng: rand09::rngs::StdRng::seed_from_u64(seed),
            tokenizer,
            suppress_tokens,
            sot_token,
            transcribe_token,
            eot_token,
            no_speech_token,
            no_timestamps_token,
            language_token,
        })
    }

    fn decode(&mut self, mel: &Tensor, t: f64) -> Result<DecodingResult, String> {
        let audio_features = self.model.encoder_forward(mel, true).map_err(e2s)?;
        let sample_len = self.model.config().max_target_positions / 2;
        let mut sum_logprob = 0f64;
        let mut no_speech_prob = f64::NAN;
        let mut tokens = vec![self.sot_token];
        if let Some(language_token) = self.language_token {
            tokens.push(language_token);
        }
        tokens.push(self.transcribe_token);
        for i in 0..sample_len {
            let tokens_t = Tensor::new(tokens.as_slice(), mel.device())
                .map_err(e2s)?
                .unsqueeze(0)
                .map_err(e2s)?;
            let ys = self
                .model
                .decoder_forward(&tokens_t, &audio_features, i == 0)
                .map_err(e2s)?;

            if i == 0 {
                let logits = self
                    .model
                    .decoder_final_linear(&ys.i(..1).map_err(e2s)?)
                    .map_err(e2s)?
                    .i(0)
                    .map_err(e2s)?
                    .i(0)
                    .map_err(e2s)?;
                no_speech_prob = softmax(&logits, 0)
                    .map_err(e2s)?
                    .i(self.no_speech_token as usize)
                    .map_err(e2s)?
                    .to_scalar::<f32>()
                    .map_err(e2s)? as f64;
            }

            let (_, seq_len, _) = ys.dims3().map_err(e2s)?;
            let logits = self
                .model
                .decoder_final_linear(&ys.i((..1, seq_len - 1..)).map_err(e2s)?)
                .map_err(e2s)?
                .i(0)
                .map_err(e2s)?
                .i(0)
                .map_err(e2s)?;
            let logits = self.apply_timestamp_rules(&logits, &tokens)?;
            let logits = logits.broadcast_add(&self.suppress_tokens).map_err(e2s)?;
            let next_token = if t > 0f64 {
                let prs = softmax(&(&logits / t).map_err(e2s)?, 0).map_err(e2s)?;
                let logits_v: Vec<f32> = prs.to_vec1().map_err(e2s)?;
                let distr = WeightedIndex::new(&logits_v).map_err(e2s)?;
                distr.sample(&mut self.rng) as u32
            } else {
                let logits_v: Vec<f32> = logits.to_vec1().map_err(e2s)?;
                logits_v
                    .iter()
                    .enumerate()
                    .max_by(|(_, u), (_, v)| u.total_cmp(v))
                    .map(|(i, _)| i as u32)
                    .unwrap_or(0)
            };
            tokens.push(next_token);
            let prob = softmax(&logits, candle::D::Minus1)
                .map_err(e2s)?
                .i(next_token as usize)
                .map_err(e2s)?
                .to_scalar::<f32>()
                .map_err(e2s)? as f64;
            if next_token == self.eot_token
                || tokens.len() > self.model.config().max_target_positions
            {
                break;
            }
            sum_logprob += prob.ln();
        }
        let avg_logprob = sum_logprob / tokens.len() as f64;
        Ok(DecodingResult {
            tokens,
            avg_logprob,
            no_speech_prob,
        })
    }

    fn decode_with_fallback(&mut self, segment: &Tensor) -> Result<DecodingResult, String> {
        for (i, &t) in m::TEMPERATURES.iter().enumerate() {
            let dr = self.decode(segment, t);
            if i == m::TEMPERATURES.len() - 1 {
                return dr;
            }
            match dr {
                Ok(dr) => {
                    let needs_fallback = dr.avg_logprob < m::LOGPROB_THRESHOLD;
                    if !needs_fallback || dr.no_speech_prob > m::NO_SPEECH_THRESHOLD {
                        return Ok(dr);
                    }
                }
                Err(_err) => continue,
            }
        }
        unreachable!()
    }

    fn apply_timestamp_rules(
        &self,
        input_logits: &Tensor,
        tokens: &[u32],
    ) -> Result<Tensor, String> {
        let device = input_logits.device().clone();
        let timestamp_begin = self.no_timestamps_token + 1;
        let vocab_size = self.model.config().vocab_size as u32;

        let sample_begin = if self.language_token.is_some() { 3 } else { 2 };
        let sampled_tokens = if tokens.len() > sample_begin {
            &tokens[sample_begin..]
        } else {
            &[]
        };

        let mut masks = Vec::new();
        let mut mask_buffer = vec![0.0f32; vocab_size as usize];

        // Rule 1: timestamps must come in pairs, except directly before EOT.
        if !sampled_tokens.is_empty() {
            let last_was_timestamp = sampled_tokens
                .last()
                .map(|&t| t >= timestamp_begin)
                .unwrap_or(false);
            let penultimate_was_timestamp = if sampled_tokens.len() >= 2 {
                sampled_tokens[sampled_tokens.len() - 2] >= timestamp_begin
            } else {
                false
            };

            if last_was_timestamp {
                if penultimate_was_timestamp {
                    for (i, slot) in mask_buffer.iter_mut().enumerate() {
                        *slot = if i as u32 >= timestamp_begin {
                            f32::NEG_INFINITY
                        } else {
                            0.0
                        };
                    }
                    masks.push(Tensor::new(mask_buffer.as_slice(), &device).map_err(e2s)?);
                } else {
                    for (i, slot) in mask_buffer.iter_mut().enumerate() {
                        *slot = if (i as u32) < self.eot_token {
                            f32::NEG_INFINITY
                        } else {
                            0.0
                        };
                    }
                    masks.push(Tensor::new(mask_buffer.as_slice(), &device).map_err(e2s)?);
                }
            }

            // Rule 2: timestamps should not decrease.
            let timestamp_tokens: Vec<u32> = sampled_tokens
                .iter()
                .filter(|&&t| t >= timestamp_begin)
                .copied()
                .collect();
            if !timestamp_tokens.is_empty() {
                let timestamp_last = if last_was_timestamp && !penultimate_was_timestamp {
                    *timestamp_tokens.last().unwrap()
                } else {
                    timestamp_tokens.last().unwrap() + 1
                };
                for (i, slot) in mask_buffer.iter_mut().enumerate() {
                    *slot = if i as u32 >= timestamp_begin && (i as u32) < timestamp_last {
                        f32::NEG_INFINITY
                    } else {
                        0.0
                    };
                }
                masks.push(Tensor::new(mask_buffer.as_slice(), &device).map_err(e2s)?);
            }
        }

        // Rule 3: force an initial timestamp.
        if tokens.len() == sample_begin {
            for slot in mask_buffer.iter_mut() {
                *slot = f32::NEG_INFINITY;
            }
            for slot in mask_buffer.iter_mut().skip(timestamp_begin as usize) {
                *slot = 0.0;
            }
            masks.push(Tensor::new(mask_buffer.as_slice(), &device).map_err(e2s)?);
        }

        let mut logits = input_logits.clone();
        for mask in masks {
            logits = logits.broadcast_add(&mask).map_err(e2s)?;
        }

        // Rule 4: if the probability mass of the timestamps exceeds any text
        // token, only timestamps may be sampled.
        let log_probs = log_softmax(&logits, 0).map_err(e2s)?;
        let timestamp_log_probs = log_probs
            .narrow(
                0,
                timestamp_begin as usize,
                vocab_size as usize - timestamp_begin as usize,
            )
            .map_err(e2s)?;
        let text_log_probs = log_probs
            .narrow(0, 0, timestamp_begin as usize)
            .map_err(e2s)?;

        let timestamp_logprob = {
            let max_val = timestamp_log_probs.max(0).map_err(e2s)?;
            let shifted = timestamp_log_probs.broadcast_sub(&max_val).map_err(e2s)?;
            let exp_shifted = shifted.exp().map_err(e2s)?;
            let sum_exp = exp_shifted.sum(0).map_err(e2s)?;
            let log_sum = sum_exp.log().map_err(e2s)?;
            max_val
                .broadcast_add(&log_sum)
                .map_err(e2s)?
                .to_scalar::<f32>()
                .map_err(e2s)?
        };
        let max_text_token_logprob: f32 = text_log_probs
            .max(0)
            .map_err(e2s)?
            .to_scalar::<f32>()
            .map_err(e2s)?;

        if timestamp_logprob > max_text_token_logprob {
            for (i, slot) in mask_buffer.iter_mut().enumerate() {
                *slot = if (i as u32) < timestamp_begin {
                    f32::NEG_INFINITY
                } else {
                    0.0
                };
            }
            let mask_tensor = Tensor::new(mask_buffer.as_slice(), &device).map_err(e2s)?;
            logits = logits.broadcast_add(&mask_tensor).map_err(e2s)?;
        }

        Ok(logits)
    }

    fn run(&mut self, mel: &Tensor) -> Result<String, String> {
        let (_, _, content_frames) = mel.dims3().map_err(e2s)?;
        let mut seek = 0;
        let mut texts: Vec<String> = Vec::new();
        while seek < content_frames {
            let segment_size = usize::min(content_frames - seek, m::N_FRAMES);
            let mel_segment = mel.narrow(2, seek, segment_size).map_err(e2s)?;
            let dr = self.decode_with_fallback(&mel_segment)?;
            seek += segment_size;
            if dr.no_speech_prob > m::NO_SPEECH_THRESHOLD && dr.avg_logprob < m::LOGPROB_THRESHOLD {
                continue;
            }
            // Timestamped tokens decode as separate utterances.
            let mut tokens_to_decode: Vec<u32> = Vec::new();
            for &token in dr.tokens.iter() {
                if token == self.sot_token || token == self.eot_token {
                    continue;
                }
                if token > self.no_timestamps_token {
                    if !tokens_to_decode.is_empty() {
                        let text = self
                            .tokenizer
                            .decode(&tokens_to_decode, true)
                            .map_err(e2s)?;
                        if !text.trim().is_empty() {
                            texts.push(text);
                        }
                        tokens_to_decode.clear();
                    }
                } else {
                    tokens_to_decode.push(token);
                }
            }
            if !tokens_to_decode.is_empty() {
                let text = self
                    .tokenizer
                    .decode(&tokens_to_decode, true)
                    .map_err(e2s)?;
                if !text.trim().is_empty() {
                    texts.push(text);
                }
            }
        }
        Ok(texts.join(" ").trim().to_string())
    }
}

fn token_id(tokenizer: &Tokenizer, token: &str) -> Result<u32, String> {
    tokenizer
        .token_to_id(token)
        .ok_or_else(|| format!("no token-id for {token}"))
}

fn e2s<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

/// Parse a RIFF/WAVE file into mono f32 samples plus its sample rate.
/// Handles 8/16/24/32-bit integer PCM and 32-bit float, any channel count.
pub fn decode_wav(bytes: &[u8]) -> Result<(Vec<f32>, u32), String> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".to_string());
    }
    let mut pos = 12usize;
    let mut sample_rate = 0u32;
    let mut channels = 0u16;
    let mut bits = 0u16;
    let mut format = 1u16;
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= bytes.len() {
        let chunk_id = &bytes[pos..pos + 4];
        let chunk_len = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let body = &bytes[pos + 8..(pos + 8 + chunk_len).min(bytes.len())];
        match chunk_id {
            b"fmt " => {
                if body.len() >= 16 {
                    format = u16::from_le_bytes([body[0], body[1]]);
                    channels = u16::from_le_bytes([body[2], body[3]]);
                    sample_rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
                    bits = u16::from_le_bytes([body[14], body[15]]);
                }
            }
            b"data" => data = Some(body),
            _ => {}
        }
        pos += 8 + chunk_len + (chunk_len & 1);
    }
    let Some(body) = data else {
        return Err("WAV has no data chunk".to_string());
    };
    if sample_rate == 0 || channels == 0 {
        return Err("WAV header incomplete".to_string());
    }
    let bytes_per_sample = (bits / 8) as usize;
    if bytes_per_sample == 0 {
        return Err(format!("unsupported bit depth {bits}"));
    }
    let frame_size = bytes_per_sample * channels as usize;
    let mut mono: Vec<f32> = Vec::with_capacity(body.len() / frame_size.max(1));
    for frame in body.chunks_exact(frame_size) {
        let mut acc = 0f32;
        for channel in 0..channels as usize {
            let offset = channel * bytes_per_sample;
            let sample_bytes = &frame[offset..offset + bytes_per_sample];
            let value = match (format, bits) {
                (1, 8) => (sample_bytes[0] as f32 - 128.0) / 128.0,
                (1, 16) => i16::from_le_bytes([sample_bytes[0], sample_bytes[1]]) as f32 / 32_768.0,
                (1, 24) => {
                    let raw = (sample_bytes[0] as i32)
                        | ((sample_bytes[1] as i32) << 8)
                        | ((sample_bytes[2] as i32) << 16);
                    let extended = if raw & 0x80_0000 != 0 {
                        raw | !0xFF_FFFF
                    } else {
                        raw
                    };
                    extended as f32 / 8_388_608.0
                }
                (1, 32) => {
                    i32::from_le_bytes([
                        sample_bytes[0],
                        sample_bytes[1],
                        sample_bytes[2],
                        sample_bytes[3],
                    ]) as f32
                        / 2_147_483_648.0
                }
                (3, 32) => f32::from_le_bytes([
                    sample_bytes[0],
                    sample_bytes[1],
                    sample_bytes[2],
                    sample_bytes[3],
                ]),
                _ => return Err(format!("unsupported WAV format {format} at {bits} bits")),
            };
            acc += value;
        }
        mono.push(acc / channels as f32);
    }
    Ok((mono, sample_rate))
}

/// Spoken language forced into the whisper decoder. Multilingual whisper
/// checkpoints need an explicit prompt language; v2.0.0 hard-coded `<|ru|>`,
/// so English audio came out as Russian-sounding gibberish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Russian,
    English,
}

impl Language {
    /// The special token that tells whisper which language it hears.
    pub fn token(self) -> &'static str {
        match self {
            Language::Russian => "<|ru|>",
            Language::English => "<|en|>",
        }
    }

    /// Follow the UI locale: an English app transcribes English.
    pub fn from_locale(locale: crate::i18n::Locale) -> Self {
        match locale {
            crate::i18n::Locale::Ru => Language::Russian,
            crate::i18n::Locale::En => Language::English,
        }
    }
}

/// Transcribe `wav_bytes` (any PCM WAV) with the whisper model in `model_dir`.
/// The audio is resampled to 16 kHz mono and decoded with `language` forced.
pub fn transcribe(
    model_dir: &Path,
    wav_bytes: &[u8],
    language: Language,
) -> Result<String, String> {
    let config_path = model_dir.join("config.json");
    let weights_path = model_dir.join("model.safetensors");
    let tokenizer_path = model_dir.join("tokenizer.json");
    let config: Config = serde_json::from_str(
        &std::fs::read_to_string(&config_path).map_err(|e| format!("read config: {e}"))?,
    )
    .map_err(|e| format!("parse config: {e}"))?;
    let tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(|e| e.to_string())?;

    let mel_bytes: &[u8] = match config.num_mel_bins {
        80 => include_bytes!("../../assets/whisper/mel_filters_80.bytes"),
        128 => include_bytes!("../../assets/whisper/mel_filters_128.bytes"),
        other => return Err(format!("unexpected num_mel_bins {other}")),
    };
    let mut mel_filters = vec![0f32; mel_bytes.len() / 4];
    <byteorder_like::LittleEndian as byteorder_like::ByteOrder>::read_f32_into(
        mel_bytes,
        &mut mel_filters,
    );

    let (pcm, rate) = decode_wav(wav_bytes)?;
    let pcm = resample_linear(&pcm, rate, SAMPLE_RATE);
    if pcm.is_empty() {
        return Err("no audio samples".to_string());
    }
    let mel_data = audio::pcm_to_mel(&config, &pcm, &mel_filters);
    let mel_len = mel_data.len();
    let device = Device::Cpu;
    let mel = Tensor::from_vec(
        mel_data,
        (1, config.num_mel_bins, mel_len / config.num_mel_bins),
        &device,
    )
    .map_err(e2s)?;

    let vb = unsafe {
        VarBuilder::from_mmaped_safetensors(&[weights_path], m::DTYPE, &device).map_err(e2s)?
    };
    let model = Model::Normal(m::model::Whisper::load(&vb, config).map_err(e2s)?);

    let language_token = Some(token_id(&tokenizer, language.token())?);
    let mut decoder = Decoder::new(model, tokenizer, 299_792_458, &device, language_token)?;
    decoder.run(&mel)
}

/// Minimal little-endian float reader (avoids adding the byteorder crate).
mod byteorder_like {
    pub struct LittleEndian;

    pub trait ByteOrder {
        fn read_f32_into(src: &[u8], dst: &mut [f32]);
    }

    impl ByteOrder for LittleEndian {
        fn read_f32_into(src: &[u8], dst: &mut [f32]) {
            for (index, out) in dst.iter_mut().enumerate() {
                let at = index * 4;
                if at + 4 > src.len() {
                    break;
                }
                *out = f32::from_le_bytes([src[at], src[at + 1], src[at + 2], src[at + 3]]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav_bytes(samples: &[i16], rate: u32) -> Vec<u8> {
        crate::ai::pcm_to_wav(
            &samples
                .iter()
                .map(|s| *s as f32 / 32_768.0)
                .collect::<Vec<_>>(),
            rate,
        )
    }

    #[test]
    fn decodes_16bit_mono() {
        let bytes = wav_bytes(&[0, 16_384, -16_384], 16_000);
        let (pcm, rate) = decode_wav(&bytes).unwrap();
        assert_eq!(rate, 16_000);
        assert_eq!(pcm.len(), 3);
        assert!((pcm[1] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn decodes_16bit_stereo_to_mono() {
        // Hand-build a stereo file: header + one interleaved L/R frame.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        let data: [u8; 4] = [&0i16.to_le_bytes()[..], &32_767i16.to_le_bytes()[..]]
            .concat()
            .try_into()
            .unwrap();
        bytes.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&2u16.to_le_bytes()); // stereo
        bytes.extend_from_slice(&16_000u32.to_le_bytes());
        bytes.extend_from_slice(&(16_000u32 * 4).to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&data);
        let (pcm, _) = decode_wav(&bytes).unwrap();
        assert_eq!(pcm.len(), 1);
        assert!((pcm[0] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode_wav(b"nope").is_err());
    }

    #[test]
    fn transcribe_requires_model_dir() {
        let bytes = wav_bytes(&[0; 1600], 16_000);
        let err = transcribe(Path::new("/nonexistent-model"), &bytes, Language::Russian);
        assert!(err.is_err());
    }

    #[test]
    fn language_tokens_match_the_whisper_vocabulary() {
        assert_eq!(Language::Russian.token(), "<|ru|>");
        assert_eq!(Language::English.token(), "<|en|>");
        assert_eq!(
            Language::from_locale(crate::i18n::Locale::En),
            Language::English
        );
    }
}
