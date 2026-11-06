//! OCR via Xiaomi-OCR-0 served by mistral.rs — a pure-Rust engine downloaded
//! as a component. The app spawns `mistralrs` (OpenAI-compatible server) on
//! localhost and posts base64 images to `/v1/chat/completions`.
//!
//! Progress is streamed back as [`OcrEvent`]s so the UI can show "starting →
//! ready → done" instead of a single toast and minutes of silence (v1.12.4).

use std::fs;
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::{json, Value};

pub const DEFAULT_PORT: u16 = 8_471;

/// Command-line shape for the downloaded engine. Kept pure so the spawn is
/// unit-testable without the 1.7 GB model.
///
/// v2.1.0: no `-a qwen3_5`. The architecture flag only accepts the exact
/// loader names of the engine build, so a renamed/absent arch makes the
/// process die instantly ("invalid value for --arch") and OCR never starts —
/// while the app could only report "engine exited before readiness". The
/// engine reads `model_type` from the model's own `config.json` when `-a` is
/// absent, which is both version-proof and correct for the OCR checkpoint.
/// `--no-ui` keeps the bundled web chat out of the local port.
pub fn serve_args(port: u16, model_dir: &Path) -> Vec<String> {
    serve_args_with(port, model_dir, isq_from_env())
}

/// Optional in-situ quantization, e.g. `CAPSULEOS_OCR_ISQ=8`. The CPU path
/// loads the 1.75 GB bf16 weights raw; ISQ 4/8 cuts RAM and load time at a
/// small quality cost. `None` keeps the stock full-precision behaviour.
pub fn serve_args_with(port: u16, model_dir: &Path, isq: Option<u8>) -> Vec<String> {
    let mut args = vec![
        "serve".to_string(),
        "--port".to_string(),
        port.to_string(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--no-ui".to_string(),
    ];
    args.push("multimodal".to_string());
    args.push("-m".to_string());
    args.push(model_dir.display().to_string());
    // ISQ belongs to the model-argument group (same struct as `-m`), which
    // sits *after* the subcommand in mistralrs-cli.
    if let Some(q) = isq {
        args.push("--isq".to_string());
        args.push(q.to_string());
    }
    args
}

fn isq_from_env() -> Option<u8> {
    isq_from_env_checked(&std::env::var("CAPSULEOS_OCR_ISQ").unwrap_or_default())
}

fn isq_from_env_checked(raw: &str) -> Option<u8> {
    raw.trim().parse::<u8>().ok().filter(|q| (2..=8).contains(q))
}

/// Cheap pre-flight so a broken install fails in milliseconds with a
/// sentence instead of a silent engine exit after a minute of loading.
/// The vision preprocessor files are optional (mistral.rs falls back to
/// defaults); the weights, tokenizer and a parsable config are not.
pub fn preflight(model_dir: &Path) -> Result<(), String> {
    if !model_dir.is_dir() {
        return Err(format!(
            "OCR model folder is missing: {} — reinstall the OCR model component",
            model_dir.display()
        ));
    }
    for name in ["model.safetensors", "tokenizer.json", "config.json"] {
        let path = model_dir.join(name);
        match std::fs::metadata(&path) {
            Ok(meta) if meta.len() > 0 => {}
            _ => {
                return Err(format!(
                    "OCR model folder is incomplete: {name} is missing or empty in {} — reinstall the OCR model component",
                    model_dir.display()
                ));
            }
        }
    }
    let config = std::fs::read_to_string(model_dir.join("config.json"))
        .map_err(|e| format!("cannot read OCR config.json: {e}"))?;
    let doc: serde_json::Value =
        serde_json::from_str(&config).map_err(|e| format!("OCR config.json is not valid JSON: {e}"))?;
    let model_type = doc["model_type"].as_str().unwrap_or("<none>").to_string();
    let _ = writeln!(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(engine_log_path(model_dir))
            .unwrap_or_else(|_| std::fs::File::create(std::env::temp_dir().join("capsuleos-mistralrs.log")).expect("temp log")),
        "==== preflight {} ====: model_type = {model_type}",
        chrono::Utc::now().to_rfc3339()
    );
    Ok(())
}

/// Last `lines` non-empty lines of the engine log, formatted for an error message.
fn read_log_tail(path: &Path, lines: usize) -> String {
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let collected: Vec<&str> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    if collected.is_empty() {
        return String::new();
    }
    let start = collected.len().saturating_sub(lines);
    let tail = collected[start..].join(" | ");
    format!(" — last log lines: {tail}")
}

/// Stderr log lives beside the model dir (the components root) so it
/// survives across runs and is easy to find next to the model itself.
pub fn engine_log_path(model_dir: &Path) -> PathBuf {
    model_dir
        .parent()
        .map(|p| p.join("mistralrs.log"))
        .unwrap_or_else(|| std::env::temp_dir().join("capsuleos-mistralrs.log"))
}

/// A running local engine process.
pub struct OcrEngine {
    child: Child,
    pub port: u16,
    /// Where the engine's stderr is being written.
    pub log_path: PathBuf,
    /// Model id advertised by `/v1/models`; the chat endpoint rejects an
    /// invented name ("model \"ocr\" not found"), which is how a perfectly
    /// healthy engine looked broken.
    pub model: String,
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
    ///
    /// The pipes are deliberately `null()`: nothing in the app ever reads
    /// them, so a chatty engine would fill the 64 KB pipe buffer and block
    /// mid-load — the exact "инициализация движка… и тишина" hang of
    /// v1.12.3. mistralrs logs to the console; losing that log is fine, a
    /// deadlocked engine is not.
    pub fn spawn(engine_bin: &Path, model_dir: &Path, port: u16) -> Result<Self, String> {
        preflight(model_dir)?;
        let args = serve_args(port, model_dir);
        let mut command = Command::new(engine_bin);
        command.args(&args).stdin(Stdio::null()).stdout(Stdio::null());
        // The engine log lands next to the model dir (the components root).
        // v2.1.0 discarded stderr entirely, so an engine that died during the
        // cold load left only "exited before readiness" with no reason.
        let log_path = engine_log_path(model_dir);
        if let Ok(mut log) = fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            let _ = writeln!(log, "==== spawn {} ====", chrono::Utc::now().to_rfc3339());
            command.stderr(Stdio::from(log));
        }
        let child = command.spawn().map_err(|e| format!("spawn engine: {e}"))?;
        let mut engine = Self {
            child,
            port,
            model: String::new(),
            log_path,
        };
        // v2.1.0: 15 minutes. A cold start on a laptop loads 1.75 GB of
        // weights plus the vision tower; the old 5-minute ceiling expired
        // *after* the engine had already come up on slower machines, which
        // looked exactly like a broken OCR.
        engine.wait_ready(Duration::from_secs(900))?;
        Ok(engine)
    }

    /// Poll `/v1/models` until the server is ready or the timeout expires.
    /// A process that exits early must fail immediately — burning five
    /// minutes of a silent progress bar is the bug, not the remedy.
    ///
    /// On success the advertised model id is recorded: the chat endpoint only
    /// accepts *that* id, and the old hard-coded `"model": "ocr"` made a
    /// healthy engine answer `model "ocr" not found` — the actual v2.0.0 OCR
    /// failure.
    pub fn wait_ready(&mut self, timeout: Duration) -> Result<(), String> {
        let url = format!("http://127.0.0.1:{}/v1/models", self.port);
        let deadline = Instant::now() + timeout;
        let mut last_error = String::from("no attempts");
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    let tail = read_log_tail(&self.log_path, 12);
                    return Err(format!(
                        "engine exited ({status}) before readiness: {last_error} — stderr log: {}{}",
                        self.log_path.display(),
                        tail
                    ));
                }
                Ok(None) => {}
                // A failed try_wait is retried on the next poll; the ureq
                // attempt below always refreshes `last_error` anyway.
                Err(_) => {}
            }
            match ureq::get(&url).timeout(Duration::from_secs(2)).call() {
                Ok(response) => match response.into_string() {
                    Ok(body) => {
                        if let Some(model) = parse_model_id(&body) {
                            self.model = model;
                            return Ok(());
                        }
                        last_error = body.chars().take(400).collect();
                    }
                    Err(error) => last_error = error.to_string(),
                },
                Err(error) => last_error = error.to_string(),
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Err(format!("engine not ready in time: {last_error}"))
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Recognize text in an image (PNG/JPEG bytes) via chat completions.
    /// `model` is the id returned by `/v1/models` — never a guess.
    pub fn recognize(
        port: u16,
        model: &str,
        image: &[u8],
        mime: &str,
        prompt: &str,
    ) -> Result<String, String> {
        let url = format!("http://127.0.0.1:{port}/v1/chat/completions");
        let data_url = format!("data:{mime};base64,{}", BASE64.encode(image));
        let body = json!({
            "model": model,
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
            .timeout(Duration::from_secs(900))
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

/// First model id advertised by an OpenAI-style `/v1/models` body:
/// `{"object":"list","data":[{"id":"...","object":"model"}]}`.
pub fn parse_model_id(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    value
        .get("data")
        .and_then(Value::as_array)
        .and_then(|models| models.first())
        .and_then(|model| model.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|id| !id.is_empty())
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

/// Resolve the engine binary + model dir from installed components. A custom
/// OCR model folder replaces the downloaded checkpoint; the engine binary is
/// still the component (it is the runtime, not the model).
pub fn resolve_paths(
    components: &crate::components::Manager,
    custom_model: Option<&str>,
) -> Option<(PathBuf, PathBuf)> {
    let custom = crate::ai::custom::resolve(crate::ui_state::CustomModelKind::Ocr, custom_model);
    if !components.ready(crate::components::ComponentId::OcrEngine) {
        return None;
    }
    if custom.is_none() && !components.ready(crate::components::ComponentId::OcrModel) {
        return None;
    }
    let engine_dir = components.dir(crate::components::ComponentId::OcrEngine);
    let engine_name = if cfg!(target_os = "windows") {
        "mistralrs.exe"
    } else {
        "mistralrs"
    };
    let engine_bin = crate::components::find_file(&engine_dir, Path::new(engine_name))?;
    let model_dir = custom.unwrap_or_else(|| components.dir(crate::components::ComponentId::OcrModel));
    Some((engine_bin, model_dir))
}

/// Progress of one recognition, streamed back through the request channel.
#[derive(Debug, Clone)]
pub enum OcrEvent {
    /// The engine process is starting; a cold model load can take minutes.
    Starting,
    /// The engine answered `/v1/models` — inference begins.
    Ready,
    /// The image was recognized.
    Done(String),
    /// The engine or the request failed.
    Failed(String),
}

/// One OCR request, answered through its own event channel.
pub struct OcrRequest {
    pub image: Vec<u8>,
    pub mime: String,
    pub prompt: String,
    pub reply: std::sync::mpsc::Sender<OcrEvent>,
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
                let _ = request.reply.send(OcrEvent::Starting);
                // An engine without an advertised model id cannot answer the
                // chat endpoint, so it counts as dead and gets respawned.
                let healthy = engine
                    .as_mut()
                    .map(|engine| engine.is_alive() && !engine.model.is_empty())
                    .unwrap_or(false);
                if !healthy {
                    engine = None;
                    match OcrEngine::spawn(&engine_bin, &model_dir, port) {
                        Ok(spawned) => engine = Some(spawned),
                        Err(message) => {
                            let _ = request.reply.send(OcrEvent::Failed(message));
                            continue;
                        }
                    }
                }
                let Some(active) = engine.as_ref() else {
                    continue;
                };
                let model = active.model.clone();
                let _ = request.reply.send(OcrEvent::Ready);
                let result = OcrEngine::recognize(
                    port,
                    &model,
                    &request.image,
                    &request.mime,
                    &request.prompt,
                );
                let _ = request.reply.send(match result {
                    Ok(text) => OcrEvent::Done(text),
                    Err(message) => OcrEvent::Failed(message),
                });
            }
            // rx dropped → engine (and its child process) dropped here.
        });
        Self { tx }
    }

    /// Queue a recognition; `None` when the actor is gone. Progress arrives
    /// as [`OcrEvent`]s on the returned channel.
    pub fn recognize(
        &self,
        image: Vec<u8>,
        mime: String,
        prompt: String,
    ) -> Option<std::sync::mpsc::Receiver<OcrEvent>> {
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
    fn isq_flag_optional_and_ordered() {
        let plain = serve_args_with(8_471, Path::new("/models/ocr"), None);
        assert!(!plain.contains(&"--isq".to_string()));
        let quantized = serve_args_with(8_471, Path::new("/models/ocr"), Some(8));
        let isq = quantized.iter().position(|a| a == "--isq").unwrap();
        let m = quantized.iter().position(|a| a == "-m").unwrap();
        let sub = quantized.iter().position(|a| a == "multimodal").unwrap();
        assert_eq!(quantized[isq + 1], "8");
        assert!(sub < isq && isq > m, "isq belongs to the model arg group after the subcommand");
        // Out-of-range values are ignored at the env layer.
        assert_eq!(isq_from_env_checked("12"), None);
        assert_eq!(isq_from_env_checked("0"), None);
        assert_eq!(isq_from_env_checked("4"), Some(4));
    }

    #[test]
    fn serve_args_shape() {
        let args = serve_args(8_471, Path::new("/models/ocr"));
        assert_eq!(args[0], "serve");
        assert!(args.windows(2).any(|w| w == ["--port", "8471"]));
        assert!(args.contains(&"multimodal".to_string()));
        assert!(args.contains(&"/models/ocr".to_string()));
        assert!(args.contains(&"127.0.0.1".to_string()));
        assert!(args.contains(&"--no-ui".to_string()));
        // The arch flag is what killed the engine on v2.0.0: the loader name
        // is build-specific and an unknown value aborts the process at once.
        assert!(!args.contains(&"-a".to_string()), "{args:?}");
        assert!(!args.iter().any(|arg| arg.contains("qwen")), "{args:?}");
        // Order matters: parent `serve` options must precede the subcommand.
        let sub = args.iter().position(|arg| arg == "multimodal").unwrap();
        let port = args.iter().position(|arg| arg == "--port").unwrap();
        assert!(port < sub, "serve options must precede the subcommand");
    }

    #[test]
    fn reads_the_advertised_model_id() {
        let body = serde_json::json!({
            "object": "list",
            "data": [
                { "id": "org/ocr-model", "object": "model" },
                { "id": "other", "object": "model" }
            ]
        })
        .to_string();
        assert_eq!(parse_model_id(&body).as_deref(), Some("org/ocr-model"));
        // A body that merely mentions "data" is not ready.
        assert_eq!(parse_model_id("{\"detail\":\"loading\"}"), None);
        assert_eq!(parse_model_id("{\"data\":[]}"), None);
        assert_eq!(parse_model_id("not json"), None);
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
    fn preflight_rejects_incomplete_model_dirs() {
        let tmp = std::env::temp_dir().join(format!("capsule-preflight-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        // Missing dir entirely.
        assert!(preflight(&tmp.join("nope")).is_err());
        // Empty dir.
        assert!(preflight(&tmp).is_err());
        // Stub files, broken JSON.
        std::fs::write(tmp.join("model.safetensors"), b"stub").unwrap();
        std::fs::write(tmp.join("tokenizer.json"), b"{}").unwrap();
        std::fs::write(tmp.join("config.json"), b"{not json").unwrap();
        assert!(preflight(&tmp).is_err());
        // Valid config passes.
        std::fs::write(
            tmp.join("config.json"),
            br#"{"model_type": "qwen3_5"}"#
        )
        .unwrap();
        assert!(preflight(&tmp).is_ok());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn log_tail_joins_last_lines() {
        let tmp = std::env::temp_dir().join(format!("capsule-logtail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("log.txt");
        std::fs::write(&path, b"line one\n\nline two\nline three\n").unwrap();
        let tail = read_log_tail(&path, 2);
        assert!(tail.contains("line two"));
        assert!(tail.contains("line three"));
        assert!(!tail.contains("line one"));
        assert_eq!(read_log_tail(&tmp.join("absent"), 3), "");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_paths_requires_both_components() {
        let manager = crate::components::Manager::load_for_tests();
        assert!(resolve_paths(&manager, None).is_none());
        // An invalid custom folder must not smuggle a model in.
        assert!(resolve_paths(&manager, Some("/nope/ocr")).is_none());
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
