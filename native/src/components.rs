//! Downloadable integrations: the components catalog and download manager.
//!
//! Every local AI/asset engine in CapsuleCraft ships as a component the user
//! downloads from the Integrations view: the OCR engine (mistral.rs, pure
//! Rust) + the Xiaomi-OCR-0 model, whisper STT models (run in-process on
//! candle), the sherpa-onnx TTS runtime + Russian voices, and the pdfium PDF
//! engine. Nothing is bundled with the binary — the vault stays zero-dep and
//! the components appear only after an explicit download.
//!
//! Layout: `%APPDATA%/com.capsuleos.app/components/<id>/…` outside the vault,
//! mirroring `ui_state.rs`. Install markers live in `components.json`.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};

use crate::i18n::{t, Key, Locale};
use crate::vault;

// ---------------------------------------------------------------------------
// Platform
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    LinuxX64,
    MacArm64,
    MacX64,
    WinX64,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Platform::WinX64
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") {
                Platform::MacArm64
            } else {
                Platform::MacX64
            }
        } else {
            Platform::LinuxX64
        }
    }

    /// CI asset slug used by our own release artifacts.
    pub fn slug(self) -> &'static str {
        match self {
            Platform::LinuxX64 => "linux-x64",
            Platform::MacArm64 => "macos-arm64",
            Platform::MacX64 => "macos-x64",
            Platform::WinX64 => "windows-x64",
        }
    }
}

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComponentId {
    OcrEngine,
    OcrModel,
    SttWhisperTiny,
    SttWhisperBase,
    SttWhisperSmall,
    TtsRuntime,
    TtsVoiceIrina,
    TtsVoiceDmitri,
    TtsVoiceAmy,
    PdfEngine,
}

/// How the downloaded bytes must be unpacked into the component directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveKind {
    /// Plain files stored as-is next to each other.
    None,
    TarGz,
    TarBz2,
}

pub struct FileSpec {
    pub url: String,
    pub name: String,
}

/// One downloadable integration.
pub struct ComponentSpec {
    pub id: ComponentId,
    pub version: &'static str,
    /// Approximate advertised size in bytes, for the catalog card.
    pub size_hint: u64,
    pub archive: ArchiveKind,
    /// Components that must be installed before this one can be used.
    pub requires: &'static [ComponentId],
}

impl ComponentId {
    pub const ALL: [ComponentId; 10] = [
        ComponentId::OcrEngine,
        ComponentId::OcrModel,
        ComponentId::SttWhisperTiny,
        ComponentId::SttWhisperBase,
        ComponentId::SttWhisperSmall,
        ComponentId::TtsRuntime,
        ComponentId::TtsVoiceIrina,
        ComponentId::TtsVoiceDmitri,
        ComponentId::TtsVoiceAmy,
        ComponentId::PdfEngine,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            ComponentId::OcrEngine => "ocr-engine",
            ComponentId::OcrModel => "ocr-model-xiaomi-ocr-0",
            ComponentId::SttWhisperTiny => "stt-whisper-tiny",
            ComponentId::SttWhisperBase => "stt-whisper-base",
            ComponentId::SttWhisperSmall => "stt-whisper-small",
            ComponentId::TtsRuntime => "tts-runtime",
            ComponentId::TtsVoiceIrina => "tts-voice-irina",
            ComponentId::TtsVoiceDmitri => "tts-voice-dmitri",
            ComponentId::TtsVoiceAmy => "tts-voice-amy",
            ComponentId::PdfEngine => "pdf-engine",
        }
    }

    pub fn spec(self) -> ComponentSpec {
        match self {
            ComponentId::OcrEngine => ComponentSpec {
                id: self,
                version: "0.9.5",
                size_hint: 180_000_000,
                archive: ArchiveKind::TarGz,
                requires: &[],
            },
            ComponentId::OcrModel => ComponentSpec {
                id: self,
                version: "1.0",
                size_hint: 1_766_800_000,
                archive: ArchiveKind::None,
                requires: &[ComponentId::OcrEngine],
            },
            ComponentId::SttWhisperTiny => whisper_spec(self, "tiny", 153_600_000),
            ComponentId::SttWhisperBase => whisper_spec(self, "base", 293_000_000),
            ComponentId::SttWhisperSmall => whisper_spec(self, "small", 970_000_000),
            ComponentId::TtsRuntime => ComponentSpec {
                id: self,
                version: "2.0.1",
                size_hint: 11_000_000,
                archive: ArchiveKind::TarBz2,
                requires: &[],
            },
            ComponentId::TtsVoiceIrina => voice_spec(self, "ru_RU-irina", 67_200_000),
            ComponentId::TtsVoiceDmitri => voice_spec(self, "ru_RU-dmitri", 67_200_000),
            ComponentId::TtsVoiceAmy => voice_spec(self, "en_US-amy", 67_200_000),
            ComponentId::PdfEngine => ComponentSpec {
                id: self,
                version: "8125",
                size_hint: 5_000_000,
                archive: ArchiveKind::TarGz,
                requires: &[],
            },
        }
    }

    pub fn title(self, locale: Locale) -> String {
        t(locale, self.key()).to_string()
    }

    pub fn key(self) -> Key {
        match self {
            ComponentId::OcrEngine => Key::CompOcrEngine,
            ComponentId::OcrModel => Key::CompOcrModel,
            ComponentId::SttWhisperTiny => Key::CompSttTiny,
            ComponentId::SttWhisperBase => Key::CompSttBase,
            ComponentId::SttWhisperSmall => Key::CompSttSmall,
            ComponentId::TtsRuntime => Key::CompTtsRuntime,
            ComponentId::TtsVoiceIrina => Key::CompTtsIrina,
            ComponentId::TtsVoiceDmitri => Key::CompTtsDmitri,
            ComponentId::TtsVoiceAmy => Key::CompTtsAmy,
            ComponentId::PdfEngine => Key::CompPdfEngine,
        }
    }

    /// Files to download for `platform`. Every URL was probed at release time;
    /// our own engine artifacts use the `releases/latest` redirect so the URL
    /// survives tag bumps without a code change.
    pub fn files(self, platform: Platform) -> Vec<FileSpec> {
        let hf = |repo: &str, name: &str| {
            format!("https://huggingface.co/{}/resolve/main/{}", repo, name)
        };
        match self {
            ComponentId::OcrEngine => vec![FileSpec {
                url: format!(
                    "https://github.com/kostyk348/CapsuleCraft/releases/latest/download/mistralrs-engine-{}.tar.gz",
                    platform.slug()
                ),
                name: "engine.tar.gz".to_string(),
            }],
            ComponentId::OcrModel => ["model.safetensors", "tokenizer.json", "config.json", "processor_config.json", "chat_template.jinja", "tokenizer_config.json"]
                .iter()
                .map(|name| FileSpec {
                    url: hf("xiaomi-research/Xiaomi-OCR-0", name),
                    name: name.to_string(),
                })
                .collect(),
            ComponentId::SttWhisperTiny => whisper_files("tiny"),
            ComponentId::SttWhisperBase => whisper_files("base"),
            ComponentId::SttWhisperSmall => whisper_files("small"),
            ComponentId::TtsRuntime => vec![FileSpec {
                // MUST stay pinned to v1.12.40: the FFI struct layouts in
                // ai/tts.rs mirror c-api.h at exactly this tag (v2.0.1 dropped
                // the `data_dir` phonemization argument and shifts layouts).
                url: match platform {
                    Platform::LinuxX64 => "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.12.40/sherpa-onnx-v1.12.40-linux-x64-shared-lib.tar.bz2",
                    Platform::MacArm64 => "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.12.40/sherpa-onnx-v1.12.40-osx-arm64-shared-lib.tar.bz2",
                    Platform::MacX64 => "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.12.40/sherpa-onnx-v1.12.40-osx-x64-shared-lib.tar.bz2",
                    Platform::WinX64 => "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.12.40/sherpa-onnx-v1.12.40-win-x64-shared-MD-Release-lib.tar.bz2",
                }
                .to_string(),
                name: "runtime.tar.bz2".to_string(),
            }],
            ComponentId::TtsVoiceIrina => voice_files("ru_RU-irina"),
            ComponentId::TtsVoiceDmitri => voice_files("ru_RU-dmitri"),
            ComponentId::TtsVoiceAmy => voice_files("en_US-amy"),
            ComponentId::PdfEngine => vec![FileSpec {
                url: format!(
                    "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F8125/pdfium-{}.tgz",
                    match platform {
                        Platform::LinuxX64 => "linux-x64",
                        Platform::MacArm64 => "mac-arm64",
                        Platform::MacX64 => "mac-x64",
                        Platform::WinX64 => "win-x64",
                    }
                ),
                name: "pdfium.tgz".to_string(),
            }],
        }
    }

    /// The file whose presence proves the component is fully installed.
    pub fn sentinel(self) -> &'static str {
        match self {
            ComponentId::OcrEngine => "mistralrs",
            ComponentId::OcrModel => "model.safetensors",
            ComponentId::SttWhisperTiny
            | ComponentId::SttWhisperBase
            | ComponentId::SttWhisperSmall => "model.safetensors",
            ComponentId::TtsRuntime => TTS_C_API_BASE,
            ComponentId::TtsVoiceIrina => "ru_RU-irina-medium.onnx",
            ComponentId::TtsVoiceDmitri => "ru_RU-dmitri-medium.onnx",
            ComponentId::TtsVoiceAmy => "en_US-amy-medium.onnx",
            ComponentId::PdfEngine => PDFIUM_BASE,
        }
    }
}

fn whisper_spec(id: ComponentId, _model: &'static str, size: u64) -> ComponentSpec {
    ComponentSpec {
        id,
        version: "openai",
        size_hint: size,
        archive: ArchiveKind::None,
        requires: &[],
    }
}

fn whisper_files(model: &str) -> Vec<FileSpec> {
    ["model.safetensors", "config.json", "tokenizer.json"]
        .iter()
        .map(|name| FileSpec {
            url: format!("https://huggingface.co/openai/whisper-{model}/resolve/main/{name}"),
            name: name.to_string(),
        })
        .collect()
}

fn voice_spec(id: ComponentId, _voice: &'static str, size: u64) -> ComponentSpec {
    ComponentSpec {
        id,
        version: "piper-medium",
        size_hint: size,
        archive: ArchiveKind::TarBz2,
        requires: &[ComponentId::TtsRuntime],
    }
}

/// `stem` is the full piper voice id, e.g. `ru_RU-irina` or `en_US-amy`;
/// the tarball is `vits-piper-<stem>-medium.tar.bz2` in sherpa-onnx's
/// `tts-models` release and unpacks into `vits-piper-<stem>-medium/`.
fn voice_files(stem: &str) -> Vec<FileSpec> {
    vec![FileSpec {
        url: format!(
            "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/vits-piper-{stem}-medium.tar.bz2"
        ),
        name: format!("voice-{stem}.tar.bz2"),
    }]
}

const TTS_C_API_BASE: &str = "sherpa-onnx-c-api";
const PDFIUM_BASE: &str = "pdfium";

/// Resolved (per-OS) name of a shared library inside a component directory:
/// `libsherpa-onnx-c-api` may be installed as `libsherpa-onnx-c-api.so`,
/// `.dylib` or `sherpa-onnx-c-api.dll`.
pub fn resolve_lib(dir: &Path, base: &str) -> Option<PathBuf> {
    let candidates: Vec<String> = if cfg!(target_os = "windows") {
        vec![format!("{base}.dll")]
    } else if cfg!(target_os = "macos") {
        vec![format!("lib{base}.dylib")]
    } else {
        vec![format!("lib{base}.so"), format!("{base}.so")]
    };
    for candidate in &candidates {
        let candidate_path = Path::new(candidate);
        if let Some(path) = find_file(dir, candidate_path) {
            return Some(path);
        }
    }
    None
}

/// Depth-first search for a file or directory by exact name inside `dir`.
pub fn find_file(dir: &Path, name: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name() == Some(name.as_os_str()) {
            return Some(path);
        }
        if path.is_dir() {
            subdirs.push(path);
        }
    }
    for sub in subdirs {
        if let Some(found) = find_file(&sub, name) {
            return Some(found);
        }
    }
    None
}

/// Depth-first search for any file with the given extension (`onnx`), used
/// where the model file keeps its vendor name (`en_US-amy-medium.onnx`).
pub fn find_onnx(dir: &Path) -> Option<PathBuf> {
    find_with_extension(dir, "onnx")
}

pub fn find_with_extension(dir: &Path, extension: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some(extension) {
            return Some(path);
        }
        if path.is_dir() {
            subdirs.push(path);
        }
    }
    for sub in subdirs {
        if let Some(found) = find_with_extension(&sub, extension) {
            return Some(found);
        }
    }
    None
}

/// Reject archive entries that escape the install root (zip/tar-slip guard):
/// any `..` or Windows prefix is refused outright, absolute paths are rooted,
/// and empty/dot-only names yield nothing.
pub fn safe_entry_path(root: &Path, entry: &Path) -> Option<PathBuf> {
    let mut has_content = false;
    for component in entry.components() {
        match component {
            std::path::Component::Normal(_) => {
                has_content = true;
            }
            std::path::Component::CurDir => {}
            std::path::Component::RootDir => {}
            std::path::Component::Prefix(_) | std::path::Component::ParentDir => return None,
        }
    }
    if !has_content {
        return None;
    }
    let stripped: PathBuf = entry
        .components()
        .filter(|c| {
            !matches!(
                c,
                std::path::Component::RootDir | std::path::Component::Prefix(_)
            )
        })
        .collect();
    Some(root.join(stripped))
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct InstalledMap {
    /// slug → {"version": …, "installed_at": …}
    components: HashMap<String, InstalledInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct InstalledInfo {
    version: String,
    installed_at: String,
}

/// Live phase of a download job, shared between the worker thread and the UI.
#[derive(Clone, Debug, PartialEq)]
pub enum JobPhase {
    Queued,
    Fetching { got: u64, total: Option<u64> },
    Extracting,
    Done,
    Failed(String),
}

impl JobPhase {
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            JobPhase::Queued | JobPhase::Fetching { .. } | JobPhase::Extracting
        )
    }

    /// 0.0..=1.0 progress for the current job, if measurable.
    pub fn progress(&self) -> Option<f32> {
        match self {
            JobPhase::Fetching {
                got,
                total: Some(total),
            } if *total > 0 => Some((*got as f32 / *total as f32).clamp(0.0, 1.0)),
            JobPhase::Done => Some(1.0),
            _ => None,
        }
    }
}

struct ManagerInner {
    installed: InstalledMap,
    jobs: HashMap<String, JobPhase>,
}

pub struct Manager {
    root: PathBuf,
    inner: Arc<Mutex<ManagerInner>>,
}

impl Manager {
    pub fn load() -> Self {
        let root = vault::app_data_dir().join("components");
        let _ = fs::create_dir_all(&root);
        let installed = fs::read_to_string(root.join("components.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            root,
            inner: Arc::new(Mutex::new(ManagerInner {
                installed,
                jobs: HashMap::new(),
            })),
        }
    }

    pub fn dir(&self, id: ComponentId) -> PathBuf {
        self.root.join(id.slug())
    }

    /// Test-only instance rooted in a throwaway directory.
    #[cfg(test)]
    pub fn load_for_tests() -> Self {
        let root = std::env::temp_dir().join(format!(
            "cc_components_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&root).unwrap();
        Self {
            root,
            inner: Arc::new(Mutex::new(ManagerInner {
                installed: InstalledMap::default(),
                jobs: HashMap::new(),
            })),
        }
    }

    /// True when the sentinel file exists on disk (the source of truth — the
    /// marker only caches metadata). Archive components nest their payload in
    /// a top-level folder, so fall back to a recursive search.
    pub fn is_installed(&self, id: ComponentId) -> bool {
        let dir = self.dir(id);
        if dir.join(id.sentinel()).is_file() {
            return true;
        }
        if matches!(id, ComponentId::OcrEngine)
            && cfg!(target_os = "windows")
            && dir.join("mistralrs.exe").is_file()
        {
            return true;
        }
        match id {
            ComponentId::TtsRuntime | ComponentId::PdfEngine => {
                resolve_lib(&dir, id.sentinel()).is_some()
            }
            _ => find_file(&dir, Path::new(id.sentinel())).is_some(),
        }
    }

    pub fn installed_version(&self, id: ComponentId) -> Option<String> {
        self.inner
            .lock()
            .ok()?
            .installed
            .components
            .get(id.slug())
            .map(|info| info.version.clone())
    }

    pub fn job(&self, id: ComponentId) -> Option<JobPhase> {
        self.inner.lock().ok()?.jobs.get(id.slug()).cloned()
    }

    /// A component is usable when it and all of its requirements are installed.
    pub fn ready(&self, id: ComponentId) -> bool {
        self.is_installed(id) && id.spec().requires.iter().all(|dep| self.is_installed(*dep))
    }

    pub fn missing_requirements(&self, id: ComponentId) -> Vec<ComponentId> {
        id.spec()
            .requires
            .iter()
            .copied()
            .filter(|dep| !self.is_installed(*dep))
            .collect()
    }

    /// Kick off a background download + install. Refuses to double-run.
    pub fn start(&self, id: ComponentId) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|_| "lock poisoned")?;
        if let Some(phase) = inner.jobs.get(id.slug()) {
            if phase.is_active() {
                return Err("already running".to_string());
            }
        }
        inner.jobs.insert(id.slug().to_string(), JobPhase::Queued);
        drop(inner);

        let root = self.root.clone();
        let dir = self.dir(id);
        let inner_shared = Arc::clone(&self.inner);
        let spec = id.spec();
        let files = id.files(Platform::current());
        thread::spawn(move || {
            let set = |phase: JobPhase| {
                if let Ok(mut guard) = inner_shared.lock() {
                    guard.jobs.insert(spec.id.slug().to_string(), phase);
                }
            };
            let result = install_component(&root, &dir, &spec, &files, &set);
            match result {
                Ok(()) => {
                    let mut installed = {
                        match inner_shared.lock() {
                            Ok(guard) => guard.installed.clone(),
                            Err(_) => InstalledMap::default(),
                        }
                    };
                    installed.components.insert(
                        spec.id.slug().to_string(),
                        InstalledInfo {
                            version: spec.version.to_string(),
                            installed_at: chrono::Utc::now().to_rfc3339(),
                        },
                    );
                    set(JobPhase::Done);
                    if let Ok(guard) = inner_shared.lock() {
                        Self::persist_static(&root, &guard.installed);
                    }
                }
                Err(message) => set(JobPhase::Failed(message)),
            }
        });
        Ok(())
    }

    fn persist_static(root: &Path, installed: &InstalledMap) {
        if let Ok(json) = serde_json::to_string_pretty(installed) {
            let _ = fs::write(root.join("components.json"), json);
        }
    }

    /// Delete an installed component directory and its marker.
    pub fn remove(&self, id: ComponentId) {
        let _ = fs::remove_dir_all(self.dir(id));
        if let Ok(mut guard) = self.inner.lock() {
            guard.installed.components.remove(id.slug());
            let snapshot = guard.installed.clone();
            drop(guard);
            Self::persist_static(&self.root, &snapshot);
        }
    }

    /// Resolve a library inside a component directory (`tts-runtime`, …).
    pub fn lib_path(&self, id: ComponentId, base: &str) -> Option<PathBuf> {
        if !self.is_installed(id) {
            return None;
        }
        resolve_lib(&self.dir(id), base)
    }
}

/// Download every file of `spec` into `dir`, unpack archives, finish cleanly.
fn install_component(
    root: &Path,
    dir: &Path,
    spec: &ComponentSpec,
    files: &[FileSpec],
    set: &dyn Fn(JobPhase),
) -> Result<(), String> {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).map_err(|e| format!("create dir: {e}"))?;
    let staging = dir.join(".staging");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| format!("staging: {e}"))?;

    let total_expected: u64 = if spec.archive == ArchiveKind::None {
        spec.size_hint
    } else {
        0
    };
    for file in files {
        fetch_file(&staging, file, total_expected, &set)?;
    }

    set(JobPhase::Extracting);
    match spec.archive {
        ArchiveKind::None => {
            // Plain files: move them out of staging into the component root.
            for file in files {
                fs::rename(staging.join(&file.name), dir.join(&file.name))
                    .map_err(|e| format!("finalize {}: {e}", file.name))?;
            }
        }
        ArchiveKind::TarGz => {
            let archive_path = staging.join(&files[0].name);
            unpack_tar_gz(&archive_path, dir)?;
            let _ = fs::remove_file(&archive_path);
        }
        ArchiveKind::TarBz2 => {
            let archive_path = staging.join(&files[0].name);
            unpack_tar_bz2(&archive_path, dir)?;
            let _ = fs::remove_file(&archive_path);
        }
    }
    let _ = fs::remove_dir_all(&staging);
    let _ = fs::write(root.join(".last-install"), chrono::Utc::now().to_rfc3339());
    Ok(())
}

/// Stream one URL to `dir/<name>` reporting progress through `set`.
fn fetch_file(
    dir: &Path,
    file: &FileSpec,
    _expected: u64,
    set: &dyn Fn(JobPhase),
) -> Result<(), String> {
    let response = ureq::get(&file.url)
        .call()
        .map_err(|e| format!("{}: {e}", file.name))?;
    let total = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok());
    let mut reader = response.into_reader();
    let mut out =
        fs::File::create(dir.join(&file.name)).map_err(|e| format!("create {}: {e}", file.name))?;
    let mut buffer = [0u8; 64 * 1024];
    let mut got: u64 = 0;
    loop {
        let read = reader.read(&mut buffer).map_err(|e| format!("read: {e}"))?;
        if read == 0 {
            break;
        }
        out.write_all(&buffer[..read])
            .map_err(|e| format!("write: {e}"))?;
        got += read as u64;
        if total.is_some() {
            set(JobPhase::Fetching { got, total });
        }
    }
    if let Some(total) = total {
        if got != total {
            return Err(format!("{}: got {got} of {total} bytes", file.name));
        }
    }
    Ok(())
}

fn unpack_tar_gz(path: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(path).map_err(|e| format!("open: {e}"))?;
    let decoder = flate2::read::GzDecoder::new(file);
    unpack_tar(decoder, dest)
}

fn unpack_tar_bz2(path: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(path).map_err(|e| format!("open: {e}"))?;
    let decoder = bzip2_rs::DecoderReader::new(file);
    unpack_tar(decoder, dest)
}

fn unpack_tar<R: Read>(reader: R, dest: &Path) -> Result<(), String> {
    let mut archive = tar::Archive::new(reader);
    archive.set_overwrite(true);
    for entry in archive.entries().map_err(|e| format!("tar: {e}"))? {
        let mut entry = entry.map_err(|e| format!("tar entry: {e}"))?;
        let target = safe_entry_path(dest, &entry.path().map_err(|e| format!("path: {e}"))?)
            .ok_or_else(|| "unsafe archive entry".to_string())?;
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&target).map_err(|e| format!("mkdir: {e}"))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
        }
        // sherpa-onnx and pdfium ship versioned soname symlinks; recreate them
        // so dlopen by the plain name keeps working.
        if entry.header().entry_type().is_symlink() {
            if let Some(link) = entry.link_name().map_err(|e| format!("link: {e}"))? {
                let _ = fs::remove_file(&target);
                #[cfg(unix)]
                std::os::unix::fs::symlink(&link, &target).map_err(|e| format!("symlink: {e}"))?;
                #[cfg(not(unix))]
                fs::copy(dest.join(&link), &target).map_err(|e| format!("link copy: {e}"))?;
            }
            continue;
        }
        let mut out = fs::File::create(&target).map_err(|e| format!("create: {e}"))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("extract: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(mode) = entry.header().mode() {
                let _ = fs::set_permissions(&target, fs::Permissions::from_mode(mode));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_resolves_for_every_platform() {
        for id in ComponentId::ALL {
            for platform in [
                Platform::LinuxX64,
                Platform::MacArm64,
                Platform::MacX64,
                Platform::WinX64,
            ] {
                let files = id.files(platform);
                assert!(!files.is_empty(), "{:?}/{:?}", id, platform);
                for file in &files {
                    assert!(file.url.starts_with("https://"), "{}", file.url);
                    assert!(!file.name.is_empty());
                }
            }
        }
    }

    #[test]
    fn engine_urls_point_at_our_release() {
        for platform in [
            Platform::LinuxX64,
            Platform::MacArm64,
            Platform::MacX64,
            Platform::WinX64,
        ] {
            let files = ComponentId::OcrEngine.files(platform);
            assert_eq!(files.len(), 1);
            // The `latest` redirect keeps the URL valid across tag bumps.
            assert!(
                files[0].url.starts_with(
                    "https://github.com/kostyk348/CapsuleCraft/releases/latest/download/mistralrs-engine-"
                ),
                "{}",
                files[0].url
            );
        }
    }

    #[test]
    fn tts_runtime_url_matches_ffi_pin() {
        // ai/tts.rs mirrors c-api.h at v1.12.40 — the runtime URL must never
        // drift from that tag, or the hand-verified struct layouts break.
        for platform in [
            Platform::LinuxX64,
            Platform::MacArm64,
            Platform::MacX64,
            Platform::WinX64,
        ] {
            let files = ComponentId::TtsRuntime.files(platform);
            assert_eq!(files.len(), 1);
            assert!(
                files[0].url.contains("/v1.12.40/sherpa-onnx-v1.12.40-"),
                "{}",
                files[0].url
            );
        }
    }

    #[test]
    fn pdfium_url_matches_platform() {
        let files = ComponentId::PdfEngine.files(Platform::LinuxX64);
        assert!(files[0].url.contains("pdfium-linux-x64.tgz"));
        let files = ComponentId::PdfEngine.files(Platform::WinX64);
        assert!(files[0].url.contains("pdfium-win-x64.tgz"));
    }

    #[test]
    fn model_requires_engine() {
        let spec = ComponentId::OcrModel.spec();
        assert_eq!(spec.requires, &[ComponentId::OcrEngine]);
        let voice = ComponentId::TtsVoiceIrina.spec();
        assert_eq!(voice.requires, &[ComponentId::TtsRuntime]);
    }

    #[test]
    fn voice_urls_cover_both_languages() {
        // v2.1.0: English voice. The piper tarball is `vits-piper-<stem>-medium`
        // and the sentinel is the vendor .onnx name — both must match the real
        // release, or the download "succeeds" into an unreadable voice.
        let amy = ComponentId::TtsVoiceAmy;
        let files = amy.files(Platform::LinuxX64);
        assert!(
            files[0]
                .url
                .ends_with("vits-piper-en_US-amy-medium.tar.bz2"),
            "{}",
            files[0].url
        );
        assert_eq!(amy.sentinel(), "en_US-amy-medium.onnx");
        assert_eq!(amy.slug(), "tts-voice-amy");
        assert_eq!(amy.spec().requires, &[ComponentId::TtsRuntime]);

        for voice in [
            ComponentId::TtsVoiceIrina,
            ComponentId::TtsVoiceDmitri,
            ComponentId::TtsVoiceAmy,
        ] {
            let url = &voice.files(Platform::LinuxX64)[0].url;
            assert!(url.contains("/tts-models/vits-piper-"), "{url}");
        }
    }

    #[test]
    fn slugs_are_unique() {
        let mut slugs: Vec<_> = ComponentId::ALL.iter().map(|id| id.slug()).collect();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), ComponentId::ALL.len());
    }

    #[test]
    fn safe_entry_rejects_escape() {
        let root = Path::new("/tmp/install");
        assert!(safe_entry_path(root, Path::new("../evil")).is_none());
        assert!(safe_entry_path(root, Path::new("/abs/path")).is_some());
        assert!(safe_entry_path(root, Path::new("lib/libx.so")).is_some());
        let joined = safe_entry_path(root, Path::new("/abs/path")).unwrap();
        assert!(joined.starts_with(root));
        assert!(!joined.starts_with("/abs"));
    }

    #[test]
    fn safe_entry_rejects_empty() {
        let root = Path::new("/tmp/install");
        assert!(safe_entry_path(root, Path::new("./")).is_none());
    }

    #[test]
    fn job_phase_progress_math() {
        let phase = JobPhase::Fetching {
            got: 500,
            total: Some(1000),
        };
        assert!((phase.progress().unwrap() - 0.5).abs() < 1e-6);
        assert!(JobPhase::Queued.progress().is_none());
        assert!(JobPhase::Fetching {
            got: 1,
            total: None
        }
        .progress()
        .is_none());
        assert_eq!(JobPhase::Done.progress(), Some(1.0));
        assert!(!JobPhase::Failed("x".into()).is_active());
    }

    #[test]
    fn resolve_lib_finds_platform_variant() {
        let temp = std::env::temp_dir().join(format!("cc_lib_test_{}", std::process::id()));
        let lib_dir = temp.join("lib");
        fs::create_dir_all(&lib_dir).unwrap();
        let name = if cfg!(target_os = "windows") {
            "sherpa-onnx-c-api.dll"
        } else if cfg!(target_os = "macos") {
            "libsherpa-onnx-c-api.dylib"
        } else {
            "libsherpa-onnx-c-api.so"
        };
        fs::write(lib_dir.join(name), b"stub").unwrap();
        let found = resolve_lib(&temp, "sherpa-onnx-c-api");
        assert!(found.is_some(), "expected {name} under {temp:?}");
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn resolve_lib_missing_is_none() {
        let temp = std::env::temp_dir().join(format!("cc_lib_none_{}", std::process::id()));
        fs::create_dir_all(&temp).unwrap();
        assert!(resolve_lib(&temp, "sherpa-onnx-c-api").is_none());
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn find_file_searches_recursively() {
        let temp = std::env::temp_dir().join(format!("cc_find_{}", std::process::id()));
        fs::create_dir_all(temp.join("a/b")).unwrap();
        fs::write(temp.join("a/b/target.bin"), b"x").unwrap();
        let found = find_file(&temp, Path::new("target.bin"));
        assert!(found.unwrap().ends_with("a/b/target.bin"));
        let _ = fs::remove_dir_all(&temp);
    }
}
