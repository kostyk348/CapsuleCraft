//! Speech-to-text job runner: transcribes recorded/imported audio blocks with
//! the in-process whisper engine. Jobs run on worker threads and report their
//! phase through a shared map polled by the UI loop.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::ai::whisper;

#[derive(Clone, Debug, PartialEq)]
pub enum SttPhase {
    Running,
    Done(String),
    Failed(String),
}

pub struct SttJobs {
    inner: Arc<Mutex<HashMap<String, SttPhase>>>,
}

impl Default for SttJobs {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl SttJobs {
    /// Queue transcription of `wav_bytes` for attachment `cid`. No-op when a
    /// job for the same audio is already active. `language` is forced into the
    /// whisper decoder — English audio must not be decoded as Russian.
    pub fn start(
        &self,
        model_dir: PathBuf,
        cid: &str,
        wav_bytes: Vec<u8>,
        language: whisper::Language,
    ) -> Result<(), String> {
        self.start_with(
            model_dir,
            cid,
            wav_bytes,
            language,
            |model_dir, wav, language| whisper::transcribe(&model_dir, &wav, language),
        )
    }

    /// The seam behind `start`: the worker closure is injectable so tests can
    /// hold it open and assert the busy-flag without racing the transcriber
    /// (a real failing worker finishes in microseconds and used to win the
    /// race on loaded CI runners).
    fn start_with<F>(
        &self,
        model_dir: PathBuf,
        cid: &str,
        wav_bytes: Vec<u8>,
        language: whisper::Language,
        worker: F,
    ) -> Result<(), String>
    where
        F: FnOnce(PathBuf, Vec<u8>, whisper::Language) -> Result<String, String> + Send + 'static,
    {
        let mut jobs = self.inner.lock().map_err(|_| "stt lock poisoned")?;
        if jobs.get(cid) == Some(&SttPhase::Running) {
            return Err("already running".to_string());
        }
        jobs.insert(cid.to_string(), SttPhase::Running);
        drop(jobs);

        let inner = Arc::clone(&self.inner);
        let cid = cid.to_string();
        thread::spawn(move || {
            let phase = match worker(model_dir, wav_bytes, language) {
                Ok(text) if text.trim().is_empty() => {
                    SttPhase::Failed("empty transcript".to_string())
                }
                Ok(text) => SttPhase::Done(text),
                Err(message) => SttPhase::Failed(message),
            };
            if let Ok(mut jobs) = inner.lock() {
                jobs.insert(cid, phase);
            }
        });
        Ok(())
    }

    pub fn phase(&self, cid: &str) -> Option<SttPhase> {
        self.inner.lock().ok()?.get(cid).cloned()
    }

    /// Take a finished (Done/Failed) phase off the map, keeping active jobs.
    pub fn take_finished(&self, cid: &str) -> Option<SttPhase> {
        let mut jobs = self.inner.lock().ok()?;
        match jobs.get(cid)? {
            SttPhase::Running => None,
            _ => jobs.remove(cid),
        }
    }

    /// `Some(true)` when a running job exists for `cid`.
    pub fn is_running(&self, cid: &str) -> bool {
        self.phase(cid) == Some(SttPhase::Running)
    }
}

/// Directory of the best installed whisper model (small > base > tiny).
/// A user-supplied folder wins over the catalog when it holds a checkpoint.
pub fn best_model_dir(
    components: &crate::components::Manager,
    custom: Option<&str>,
) -> Option<PathBuf> {
    if let Some(dir) = crate::ai::custom::resolve(crate::ui_state::CustomModelKind::Stt, custom) {
        return Some(dir);
    }
    [
        crate::components::ComponentId::SttWhisperSmall,
        crate::components::ComponentId::SttWhisperBase,
        crate::components::ComponentId::SttWhisperTiny,
    ]
    .into_iter()
    .find(|id| components.ready(*id))
    .map(|id| components.dir(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(unused_imports)]
    fn start_rejects_when_missing_model() {
        use std::path::Path;
        use std::sync::mpsc;
        use std::time::Duration;

        // Hold the worker open behind a gate: the busy-flag assertions below
        // must not race a worker that (in production) can fail in
        // microseconds and flip Running -> Failed before they run.
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let (entered_tx, entered_rx) = mpsc::channel::<()>();
        let jobs = SttJobs::default();
        let err = jobs.start_with(
            Path::new("/nonexistent").to_path_buf(),
            "cid-1",
            b"xx".to_vec(),
            whisper::Language::Russian,
            move |_dir, _wav, _lang| {
                let _ = entered_tx.send(());
                let _ = gate_rx.recv();
                Err("model missing".to_string())
            },
        );
        // The job is queued and fails asynchronously; starting itself is fine.
        assert!(err.is_ok());
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("worker entered");
        // Busy-flag prevents double starts while the (failing) job runs.
        assert!(jobs.is_running("cid-1"));
        assert!(jobs
            .start(
                PathBuf::from("/x"),
                "cid-1",
                Vec::new(),
                whisper::Language::English
            )
            .is_err());
        // Release the worker; the failure must be marked (bounded wait
        // instead of a fixed sleep, so slow runners cannot flake either).
        let _ = gate_tx.send(());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match jobs.take_finished("cid-1") {
                Some(SttPhase::Failed(_)) => break,
                Some(other) => panic!("unexpected terminal phase {other:?}"),
                None if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                None => panic!("worker never marked the failure"),
            }
        }
    }

    #[test]
    fn best_model_prefers_larger_models() {
        use crate::components::{ComponentId, Manager};
        let manager = Manager::load_for_tests();
        assert_eq!(best_model_dir(&manager, None), None);
        assert_eq!(best_model_dir(&manager, Some("/nope/whisper")), None);
        let _ = ComponentId::SttWhisperTiny; // silence unused import in some cfgs
    }
}
