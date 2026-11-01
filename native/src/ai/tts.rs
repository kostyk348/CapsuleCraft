//! Russian text-to-speech via the sherpa-onnx shared library (piper VITS
//! voices, downloaded as components).
//!
//! The `sherpa-onnx` C API is loaded at runtime with `libloading` — no build
//!-time linking. `libonnxruntime` is loaded first with `RTLD_GLOBAL` so the
//! c-api dependency resolves from the component directory without touching
//! the system library search path.
//!
//! Struct layouts mirror `sherpa-onnx/c-api/c-api.h` at tag **v1.12.40**
//! exactly; `vits` is the only model type we initialize, everything else is
//! zeroed. Verified by the `tts` e2e smoke test in CI.

use std::ffi::{c_char, c_float, CString};
use std::path::Path;

use libloading::{Library, Symbol};

use crate::ai::pcm_to_wav;
use crate::components;

/// Opaque handles from sherpa-onnx.
#[repr(C)]
struct SherpaOnnxOfflineTts {
    _private: [u8; 0],
}

#[repr(C)]
struct VitsModelConfig {
    model: *const c_char,
    lexicon: *const c_char,
    tokens: *const c_char,
    data_dir: *const c_char,
    noise_scale: c_float,
    noise_scale_w: c_float,
    length_scale: c_float,
    dict_dir: *const c_char,
}

#[repr(C)]
struct MatchaModelConfig {
    acoustic_model: *const c_char,
    vocoder: *const c_char,
    lexicon: *const c_char,
    tokens: *const c_char,
    data_dir: *const c_char,
    noise_scale: c_float,
    length_scale: c_float,
    dict_dir: *const c_char,
}

#[repr(C)]
struct KokoroModelConfig {
    model: *const c_char,
    voices: *const c_char,
    tokens: *const c_char,
    data_dir: *const c_char,
    length_scale: c_float,
    dict_dir: *const c_char,
    lexicon: *const c_char,
    lang: *const c_char,
}

#[repr(C)]
struct KittenModelConfig {
    model: *const c_char,
    voices: *const c_char,
    tokens: *const c_char,
    data_dir: *const c_char,
    length_scale: c_float,
}

#[repr(C)]
struct ZipvoiceModelConfig {
    tokens: *const c_char,
    encoder: *const c_char,
    decoder: *const c_char,
    vocoder: *const c_char,
    data_dir: *const c_char,
    lexicon: *const c_char,
    feat_scale: c_float,
    t_shift: c_float,
    target_rms: c_float,
    guidance_scale: c_float,
}

#[repr(C)]
struct PocketModelConfig {
    lm_flow: *const c_char,
    lm_main: *const c_char,
    encoder: *const c_char,
    decoder: *const c_char,
    text_conditioner: *const c_char,
    vocab_json: *const c_char,
    token_scores_json: *const c_char,
    voice_embedding_cache_capacity: i32,
}

#[repr(C)]
struct SupertonicModelConfig {
    duration_predictor: *const c_char,
    text_encoder: *const c_char,
    vector_estimator: *const c_char,
    vocoder: *const c_char,
    tts_json: *const c_char,
    unicode_indexer: *const c_char,
    voice_style: *const c_char,
}

#[repr(C)]
struct ModelConfig {
    vits: VitsModelConfig,
    num_threads: i32,
    debug: i32,
    provider: *const c_char,
    matcha: MatchaModelConfig,
    kokoro: KokoroModelConfig,
    kitten: KittenModelConfig,
    zipvoice: ZipvoiceModelConfig,
    pocket: PocketModelConfig,
    supertonic: SupertonicModelConfig,
}

#[repr(C)]
struct TtsConfig {
    model: ModelConfig,
    rule_fsts: *const c_char,
    max_num_sentences: i32,
    rule_fars: *const c_char,
    silence_scale: c_float,
}

#[repr(C)]
struct GeneratedAudio {
    samples: *const c_float,
    n: i32,
    sample_rate: i32,
}

type CreateTtsFn = unsafe extern "C" fn(config: *const TtsConfig) -> *const SherpaOnnxOfflineTts;
type DestroyTtsFn = unsafe extern "C" fn(tts: *const SherpaOnnxOfflineTts);
type GenerateFn = unsafe extern "C" fn(
    tts: *const SherpaOnnxOfflineTts,
    text: *const c_char,
    sid: i32,
    speed: c_float,
) -> *const GeneratedAudio;
type DestroyAudioFn = unsafe extern "C" fn(audio: *const GeneratedAudio);

/// Strip a path to a `CString` usable by the C API.
fn path_cstring(path: &Path) -> Result<CString, String> {
    CString::new(
        path.to_str()
            .ok_or_else(|| "path is not valid UTF-8".to_string())?,
    )
    .map_err(|_| "path contains a NUL byte".to_string())
}

/// Load onnxruntime globally, then the c-api from the same directory. The
/// handle is cached for the process lifetime: sherpa-onnx keeps internal
/// onnxruntime state, so one shared instance serves every synthesis call.
fn load_libraries(lib_dir: &Path, c_api: &Path) -> Result<&'static Library, String> {
    static CACHE: std::sync::Mutex<Option<Option<&'static Library>>> =
        std::sync::Mutex::new(Some(None));
    let mut cache = CACHE.lock().map_err(|_| "tts cache poisoned")?;
    if cache.is_none() {
        return Err("tts already failed to load".to_string());
    }
    if let Some(Some(handle)) = *cache {
        return Ok(handle);
    }
    let mut order: Vec<std::path::PathBuf> = Vec::new();
    if let Some(path) = components::resolve_lib(lib_dir, "onnxruntime") {
        order.push(path);
    }
    order.push(c_api.to_path_buf());
    for path in &order[..order.len() - 1] {
        let library = unsafe { Library::new(path) }.map_err(|e| format!("load {path:?}: {e}"))?;
        // Leak the handle on purpose: dependency libraries must stay mapped
        // for the whole process, or the cached c-api would dangle.
        std::mem::forget(library);
    }
    let c_api_lib: &'static Library = Box::leak(Box::new(
        unsafe { Library::new(c_api) }.map_err(|e| format!("load {c_api:?}: {e}"))?,
    ));
    *cache = Some(Some(c_api_lib));
    Ok(c_api_lib)
}

/// Synthesize `text` to WAV bytes using the piper voice in `voice_dir`
/// (containing `*.onnx`, `tokens.txt`, `espeak-ng-data/`).
pub fn synthesize(
    c_api_lib: &Path,
    voice_dir: &Path,
    text: &str,
    speed: f32,
) -> Result<Vec<u8>, String> {
    if text.trim().is_empty() {
        return Err("empty text".to_string());
    }
    let model = components::find_file(voice_dir, Path::new("model.onnx"))
        .or_else(|| {
            // Piper voices keep their original file name; take any .onnx.
            components::find_file(voice_dir, Path::new("ru_RU-irina-medium.onnx"))
                .or_else(|| components::find_file(voice_dir, Path::new("ru_RU-dmitri-medium.onnx")))
        })
        .ok_or_else(|| "voice .onnx not found".to_string())?;
    let tokens = components::find_file(voice_dir, Path::new("tokens.txt"))
        .ok_or_else(|| "tokens.txt not found".to_string())?;
    let data_dir = components::find_file(voice_dir, Path::new("espeak-ng-data"))
        .filter(|path| path.is_dir())
        .ok_or_else(|| "espeak-ng-data not found".to_string())?;

    let lib_dir = c_api_lib
        .parent()
        .ok_or_else(|| "c-api path has no parent".to_string())?;
    let library = load_libraries(lib_dir, c_api_lib)?;

    let model_c = path_cstring(&model)?;
    let empty = CString::default();
    let tokens_c = path_cstring(&tokens)?;
    let data_dir_c = path_cstring(&data_dir)?;
    let provider = CString::new("cpu").unwrap();

    let config = TtsConfig {
        model: ModelConfig {
            vits: VitsModelConfig {
                model: model_c.as_ptr(),
                lexicon: empty.as_ptr(),
                tokens: tokens_c.as_ptr(),
                data_dir: data_dir_c.as_ptr(),
                noise_scale: 0.667,
                noise_scale_w: 0.8,
                length_scale: 1.0,
                dict_dir: empty.as_ptr(),
            },
            num_threads: 2,
            debug: 0,
            provider: provider.as_ptr(),
            matcha: zeroed_matcha(),
            kokoro: zeroed_kokoro(),
            kitten: zeroed_kitten(),
            zipvoice: zeroed_zipvoice(),
            pocket: zeroed_pocket(),
            supertonic: zeroed_supertonic(),
        },
        rule_fsts: empty.as_ptr(),
        max_num_sentences: 2,
        rule_fars: empty.as_ptr(),
        silence_scale: 0.2,
    };

    unsafe {
        let create: Symbol<CreateTtsFn> = library
            .get(b"SherpaOnnxCreateOfflineTts\0")
            .map_err(|e| format!("symbol create: {e}"))?;
        let tts = create(&config);
        if tts.is_null() {
            return Err("SherpaOnnxCreateOfflineTts returned null".to_string());
        }
        let destroy_tts: Symbol<DestroyTtsFn> = library
            .get(b"SherpaOnnxDestroyOfflineTts\0")
            .map_err(|e| format!("symbol destroy: {e}"))?;
        let generate: Symbol<GenerateFn> = library
            .get(b"SherpaOnnxOfflineTtsGenerate\0")
            .map_err(|e| format!("symbol generate: {e}"))?;
        let destroy_audio: Symbol<DestroyAudioFn> = library
            .get(b"SherpaOnnxDestroyOfflineTtsGeneratedAudio\0")
            .map_err(|e| format!("symbol destroy audio: {e}"))?;

        let text_c = CString::new(text).map_err(|_| "text contains NUL".to_string())?;
        let audio = generate(tts, text_c.as_ptr(), 0, speed);
        if audio.is_null() {
            destroy_tts(tts);
            return Err("generation returned null".to_string());
        }
        let samples = std::slice::from_raw_parts((*audio).samples, (*audio).n.max(0) as usize);
        let sample_rate = (*audio).sample_rate.max(1) as u32;
        let wav = pcm_to_wav(samples, sample_rate);
        let non_silent = samples.iter().any(|s| s.abs() > 0.001);
        destroy_audio(audio);
        destroy_tts(tts);
        if !non_silent {
            return Err("generated audio is silent".to_string());
        }
        Ok(wav)
    }
}

fn zeroed_matcha() -> MatchaModelConfig {
    MatchaModelConfig {
        acoustic_model: std::ptr::null(),
        vocoder: std::ptr::null(),
        lexicon: std::ptr::null(),
        tokens: std::ptr::null(),
        data_dir: std::ptr::null(),
        noise_scale: 0.0,
        length_scale: 0.0,
        dict_dir: std::ptr::null(),
    }
}

fn zeroed_kokoro() -> KokoroModelConfig {
    KokoroModelConfig {
        model: std::ptr::null(),
        voices: std::ptr::null(),
        tokens: std::ptr::null(),
        data_dir: std::ptr::null(),
        length_scale: 0.0,
        dict_dir: std::ptr::null(),
        lexicon: std::ptr::null(),
        lang: std::ptr::null(),
    }
}

fn zeroed_kitten() -> KittenModelConfig {
    KittenModelConfig {
        model: std::ptr::null(),
        voices: std::ptr::null(),
        tokens: std::ptr::null(),
        data_dir: std::ptr::null(),
        length_scale: 0.0,
    }
}

fn zeroed_zipvoice() -> ZipvoiceModelConfig {
    ZipvoiceModelConfig {
        tokens: std::ptr::null(),
        encoder: std::ptr::null(),
        decoder: std::ptr::null(),
        vocoder: std::ptr::null(),
        data_dir: std::ptr::null(),
        lexicon: std::ptr::null(),
        feat_scale: 0.0,
        t_shift: 0.0,
        target_rms: 0.0,
        guidance_scale: 0.0,
    }
}

fn zeroed_pocket() -> PocketModelConfig {
    PocketModelConfig {
        lm_flow: std::ptr::null(),
        lm_main: std::ptr::null(),
        encoder: std::ptr::null(),
        decoder: std::ptr::null(),
        text_conditioner: std::ptr::null(),
        vocab_json: std::ptr::null(),
        token_scores_json: std::ptr::null(),
        voice_embedding_cache_capacity: 0,
    }
}

fn zeroed_supertonic() -> SupertonicModelConfig {
    SupertonicModelConfig {
        duration_predictor: std::ptr::null(),
        text_encoder: std::ptr::null(),
        vector_estimator: std::ptr::null(),
        vocoder: std::ptr::null(),
        tts_json: std::ptr::null(),
        unicode_indexer: std::ptr::null(),
        voice_style: std::ptr::null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_is_rejected() {
        let err = synthesize(
            Path::new("/nonexistent"),
            Path::new("/nonexistent"),
            "  ",
            1.0,
        );
        assert!(err.is_err());
    }

    /// End-to-end smoke against the real sherpa-onnx shared library and a
    /// piper voice. Runs only when both env vars point at downloaded
    /// components (kept out of CI: 90 MB of binaries):
    ///   CC_SHERPA_LIB=…/libsherpa-onnx-c-api.so CC_VOICE_DIR=…/vits-piper-ru_RU-irina-medium \
    ///   cargo test tts_smoke_with_real_libraries -- --ignored
    #[test]
    #[ignore = "needs CC_SHERPA_LIB and CC_VOICE_DIR pointing at real components"]
    fn tts_smoke_with_real_libraries() {
        let lib = std::env::var("CC_SHERPA_LIB").expect("CC_SHERPA_LIB not set");
        let voice = std::env::var("CC_VOICE_DIR").expect("CC_VOICE_DIR not set");
        let (lib, voice) = (Path::new(&lib), Path::new(&voice));
        let started = std::time::Instant::now();
        let wav = synthesize(lib, voice, "Привет! Это проверка синтеза речи.", 1.0)
            .expect("synthesis must succeed");
        assert!(
            wav.len() > 44 * 100,
            "wav suspiciously small: {}",
            wav.len()
        );
        assert_eq!(&wav[0..4], b"RIFF");
        let samples = (wav.len() - 44) / 2;
        eprintln!(
            "tts smoke: {samples} samples (~{}s) in {:?}",
            samples / 16_000,
            started.elapsed()
        );
    }
}
