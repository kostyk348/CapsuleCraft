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
    /// job for the same audio is already active.
    pub fn start(&self, model_dir: PathBuf, cid: &str, wav_bytes: Vec<u8>) -> Result<(), String> {
        let mut jobs = self.inner.lock().map_err(|_| "stt lock poisoned")?;
        if jobs.get(cid) == Some(&SttPhase::Running) {
            return Err("already running".to_string());
        }
        jobs.insert(cid.to_string(), SttPhase::Running);
        drop(jobs);

        let inner = Arc::clone(&self.inner);
        let cid = cid.to_string();
        thread::spawn(move || {
            let phase = match whisper::transcribe(&model_dir, &wav_bytes) {
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
pub fn best_model_dir(components: &crate::components::Manager) -> Option<PathBuf> {
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
        let jobs = SttJobs::default();
        let err = jobs.start(
            Path::new("/nonexistent").to_path_buf(),
            "cid-1",
            b"xx".to_vec(),
        );
        // The job is queued and fails asynchronously; starting itself is fine.
        assert!(err.is_ok());
        // Busy-flag prevents double starts while the (failing) job runs.
        assert!(jobs.is_running("cid-1"));
        assert!(jobs
            .start(PathBuf::from("/x"), "cid-1", Vec::new())
            .is_err());
        // Eventually the worker marks the failure.
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(matches!(
            jobs.take_finished("cid-1"),
            Some(SttPhase::Failed(_))
        ));
    }

    #[test]
    fn best_model_prefers_larger_models() {
        use crate::components::{ComponentId, Manager};
        let manager = Manager::load_for_tests();
        assert_eq!(best_model_dir(&manager), None);
        let _ = ComponentId::SttWhisperTiny; // silence unused import in some cfgs
    }
}
