//! Local AI engines: OCR (mistral.rs), STT (whisper on candle), TTS
//! (sherpa-onnx) — all backed by downloadable components from
//! `crate::components`.

pub mod ocr;
pub mod stt;
pub mod tts;
pub mod whisper;

/// Encode mono f32 samples (−1..1) as a 16-bit PCM WAV file.
pub fn pcm_to_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        out.extend_from_slice(&((clamped * 32_767.0) as i16).to_le_bytes());
    }
    out
}

/// Linear resample of mono samples between two rates.
pub fn resample_linear(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() || from == 0 {
        return samples.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let out_len = ((samples.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let position = i as f64 * ratio;
        let index = position.floor() as usize;
        let frac = (position - index as f64) as f32;
        let a = samples[index];
        let b = samples.get(index + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * frac);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_and_length() {
        let wav = pcm_to_wav(&[0.0, 0.5, -0.5], 16_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 6);
        let data_len = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as usize;
        assert_eq!(data_len, 6);
    }

    #[test]
    fn resample_identity_when_same_rate() {
        let samples = vec![0.1, 0.2, 0.3];
        assert_eq!(resample_linear(&samples, 16_000, 16_000), samples);
    }

    #[test]
    fn resample_downscales_length() {
        // 4 samples at 48k → 16k should give ~1/3 the samples.
        let samples = vec![0.0f32, 1.0, 0.0, 1.0];
        let out = resample_linear(&samples, 48_000, 16_000);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn resample_interpolates() {
        let out = resample_linear(&[0.0f32, 1.0], 2, 4);
        // Positions 0.0, 0.5, 1.0, 1.5(clamped) → 0.0, 0.5, 1.0, 1.0
        assert_eq!(out.len(), 4);
        assert!((out[1] - 0.5).abs() < 1e-6);
        assert!((out[2] - 1.0).abs() < 1e-6);
    }
}
