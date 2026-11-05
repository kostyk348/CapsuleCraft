//! User-supplied model directories ("подключить свои модели").
//!
//! The components catalog downloads a fixed set of engines; this module lets a
//! folder already on disk take its place. Every slot is validated before use,
//! so pointing at the wrong folder degrades to the catalog instead of failing
//! at inference time with an unreadable error.

use std::path::{Path, PathBuf};

use crate::components;
use crate::ui_state::CustomModelKind;

/// What a folder must contain to be accepted for `kind`.
pub fn validate(kind: CustomModelKind, dir: &Path) -> Result<PathBuf, String> {
    if !dir.is_dir() {
        return Err(format!("{}: not a directory", dir.display()));
    }
    let has = |name: &str| components::find_file(dir, Path::new(name)).is_some();
    let ok = match kind {
        // whisper checkpoints: config.json + weights (candle loader layout).
        CustomModelKind::Stt => has("config.json") && has("model.safetensors"),
        // piper/vits voices: any .onnx + the phoneme token table.
        CustomModelKind::Tts => components::find_onnx(dir).is_some() && has("tokens.txt"),
        // mistral.rs multimodal checkpoint: config.json + weights.
        CustomModelKind::Ocr => has("config.json") && has("model.safetensors"),
    };
    if ok {
        Ok(dir.to_path_buf())
    } else {
        Err(format!("{}: missing model files", dir.display()))
    }
}

/// Validate an optional settings string; `None` when unset or invalid.
pub fn resolve(kind: CustomModelKind, path: Option<&str>) -> Option<PathBuf> {
    let path = path?.trim();
    if path.is_empty() {
        return None;
    }
    validate(kind, Path::new(path)).ok()
}

/// True when the folder holds a whisper checkpoint this build can run.
pub fn is_whisper_dir(dir: &Path) -> bool {
    validate(CustomModelKind::Stt, dir).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cc-custom-model-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn rejects_a_folder_without_weights() {
        let dir = temp_dir("empty");
        assert!(validate(CustomModelKind::Stt, &dir).is_err());
        assert!(resolve(CustomModelKind::Stt, Some(dir.to_str().unwrap())).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn accepts_a_whisper_checkpoint() {
        let dir = temp_dir("whisper");
        std::fs::write(dir.join("config.json"), "{}").unwrap();
        std::fs::write(dir.join("model.safetensors"), b"x").unwrap();
        assert!(validate(CustomModelKind::Stt, &dir).is_ok());
        // The same folder is not a piper voice: no tokens.txt, no .onnx.
        assert!(validate(CustomModelKind::Tts, &dir).is_err());
        assert!(is_whisper_dir(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn accepts_a_piper_voice() {
        let dir = temp_dir("voice");
        std::fs::write(dir.join("en_US-amy-medium.onnx"), b"x").unwrap();
        std::fs::write(dir.join("tokens.txt"), b"x").unwrap();
        assert!(validate(CustomModelKind::Tts, &dir).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unset_slot_resolves_to_nothing() {
        assert!(resolve(CustomModelKind::Ocr, None).is_none());
        assert!(resolve(CustomModelKind::Ocr, Some("   ")).is_none());
    }
}
