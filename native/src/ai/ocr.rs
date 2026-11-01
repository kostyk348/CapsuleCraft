//! OCR via Xiaomi-OCR-0 served by mistral.rs — a pure-Rust engine downloaded
//! as a component. The app spawns `mistralrs` (OpenAI-compatible server) on
//! localhost and posts base64 images to `/v1/chat/completions`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::{json, Value};

pub const DEFAULT_PORT: u16 = 8_471;

/// Command-line shape for the downloaded engine. Kept pure so the spawn is
/// unit-testable without the 1.7 GB model.
pub fn serve_args(port: u16, model_dir: &Path) -> Vec<String> {
    vec![
        "serve".to_string(),
        "--port".to_string(),
        port.to_string(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "multimodal".to_string(),
        "-m".to_string(),
        model_dir.display().to_string(),
        "-a".to_string(),
        "qwen3_5".to_string(),
    ]
}

/// A running local engine process.
pub struct OcrEngine {
    child: Child,
    pub port: u16,
}

impl Drop for OcrEngine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl OcrEngine {
    /// Spawn the engine bound to the model directory and wait until it
    /// answers `/v1/models` (cold load of the 1.75 GB weights takes a while).
    pub fn spawn(engine_bin: &Path, model_dir: &Path, port: u16) -> Result<Self, String> {
        let args = serve_args(port, model_dir);
        let mut command = Command::new(engine_bin);
        command
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = command.spawn().map_err(|e| format!("spawn engine: {e}"))?;
        let engine = Self { child, port };
        engine.wait_ready(Duration::from_secs(300))?;
        Ok(engine)
    }

    /// Poll `/v1/models` until the server is ready or the timeout expires.
    pub fn wait_ready(&self, timeout: Duration) -> Result<(), String> {
        let url = format!("http://127.0.0.1:{}/v1/models", self.port);
        let deadline = Instant::now() + timeout;
        let mut last_error = String::from("no attempts");
        while Instant::now() < deadline {
            if let Ok(response) = ureq::get(&url).timeout(Duration::from_secs(2)).call() {
                match response.into_string() {
                    Ok(body) => {
                        if body.contains("\"data\"") {
                            return Ok(());
                        }
                        last_error = body;
                    }
                    Err(error) => last_error = error.to_string(),
                }
            } else if let Err(error) = ureq::get(&url).timeout(Duration::from_secs(2)).call() {
                last_error = error.to_string();
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Err(format!("engine not ready in time: {last_error}"))
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Recognize text in an image (PNG/JPEG bytes) via chat completions.
    pub fn recognize(port: u16, image: &[u8], mime: &str, prompt: &str) -> Result<String, String> {
        let url = format!("http://127.0.0.1:{port}/v1/chat/completions");
        let data_url = format!("data:{mime};base64,{}", BASE64.encode(image));
        let body = json!({
            "model": "ocr",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image_url", "image_url": { "url": data_url } },
                    { "type": "text", "text": prompt }
                ]
            }],
            "max_tokens": 4096,
            "temperature": 0.0,
        });
        let response = ureq::post(&url)
            .timeout(Duration::from_secs(600))
            .send_json(body)
            .map_err(|e| format!("ocr request: {e}"))?;
        let mut text = String::new();
        response
            .into_reader()
            .read_to_string(&mut text)
            .map_err(|e| format!("ocr read: {e}"))?;
        parse_completion(&text)
    }
}

/// Extract `choices[0].message.content` from an OpenAI-style response.
pub fn parse_completion(body: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(body).map_err(|e| format!("bad json: {e}"))?;
    let message = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .ok_or_else(|| format!("unexpected completion shape: {body}"))?;
    Ok(message.trim().to_string())
}

/// Resolve the engine binary + model dir from installed components.
pub fn resolve_paths(components: &crate::components::Manager) -> Option<(PathBuf, PathBuf)> {
    if !components.ready(crate::components::ComponentId::OcrEngine)
        || !components.ready(crate::components::ComponentId::OcrModel)
    {
        return None;
    }
    let engine_dir = components.dir(crate::components::ComponentId::OcrEngine);
    let engine_name = if cfg!(target_os = "windows") {
        "mistralrs.exe"
    } else {
        "mistralrs"
    };
    let engine_bin = crate::components::find_file(&engine_dir, Path::new(engine_name))?;
    let model_dir = components.dir(crate::components::ComponentId::OcrModel);
    Some((engine_bin, model_dir))
}

/// One OCR request, answered through its own channel.
pub struct OcrRequest {
    pub image: Vec<u8>,
    pub mime: String,
    pub prompt: String,
    pub reply: std::sync::mpsc::Sender<Result<String, String>>,
}

/// Background actor owning the engine process. The cold model load blocks for
/// tens of seconds, so it must never run on the UI thread; requests are
/// serialized and the process stays warm between them. The actor ends — and
/// kills the engine — when every sender is dropped.
pub struct OcrActor {
    tx: std::sync::mpsc::Sender<OcrRequest>,
}

impl OcrActor {
    pub fn start(engine_bin: PathBuf, model_dir: PathBuf, port: u16) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<OcrRequest>();
        std::thread::spawn(move || {
            let mut engine: Option<OcrEngine> = None;
            for request in rx.iter() {
                let alive = engine
                    .as_mut()
                    .map(|engine| engine.is_alive())
                    .unwrap_or(false);
                if !alive {
                    engine = None;
                    match OcrEngine::spawn(&engine_bin, &model_dir, port) {
                        Ok(spawned) => engine = Some(spawned),
                        Err(message) => {
                            let _ = request.reply.send(Err(message));
                            continue;
                        }
                    }
                }
                let result =
                    OcrEngine::recognize(port, &request.image, &request.mime, &request.prompt);
                let _ = request.reply.send(result);
            }
            // rx dropped → engine (and its child process) dropped here.
        });
        Self { tx }
    }

    /// Queue a recognition; `false` when another one is already in flight.
    pub fn recognize(
        &self,
        image: Vec<u8>,
        mime: String,
        prompt: String,
    ) -> Option<std::sync::mpsc::Receiver<Result<String, String>>> {
        let (reply, rx) = std::sync::mpsc::channel();
        self.tx
            .send(OcrRequest {
                image,
                mime,
                prompt,
                reply,
            })
            .ok()
            .map(|_| rx)
    }
}

/// Sniff the image MIME from magic bytes (PNG/JPEG/GIF/WebP); anything else
/// reports as JPEG so the data URL stays valid.
pub fn sniff_image_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_args_shape() {
        let args = serve_args(8_471, Path::new("/models/ocr"));
        assert_eq!(args[0], "serve");
        assert!(args.windows(2).any(|w| w == ["--port", "8471"]));
        assert!(args.contains(&"multimodal".to_string()));
        assert!(args.contains(&"/models/ocr".to_string()));
        assert!(args.contains(&"qwen3_5".to_string()));
        assert!(args.contains(&"127.0.0.1".to_string()));
    }

    #[test]
    fn parses_completion_content() {
        let body = serde_json::json!({
            "choices": [{ "message": { "content": "  ЧЕК №123\nИТОГО: 450 " } }]
        })
        .to_string();
        assert_eq!(parse_completion(&body).unwrap(), "ЧЕК №123\nИТОГО: 450");
    }

    #[test]
    fn rejects_malformed_completion() {
        assert!(parse_completion("{\"error\": true}").is_err());
        assert!(parse_completion("not json").is_err());
    }

    #[test]
    fn resolve_paths_requires_both_components() {
        let manager = crate::components::Manager::load_for_tests();
        assert!(resolve_paths(&manager).is_none());
    }

    #[test]
    fn sniffs_image_mime() {
        assert_eq!(
            sniff_image_mime(&[0x89, b'P', b'N', b'G', 1, 2]),
            "image/png"
        );
        assert_eq!(sniff_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), "image/jpeg");
        assert_eq!(sniff_image_mime(b"GIF89a"), "image/gif");
        let mut webp = b"RIFF\x00\x00\x00\x00WEBPVP8 ".to_vec();
        assert_eq!(sniff_image_mime(&webp), "image/webp");
        webp[0] = b'X';
        assert_eq!(sniff_image_mime(&webp), "image/jpeg");
    }
}
