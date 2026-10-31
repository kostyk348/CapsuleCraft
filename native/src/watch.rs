//! Live vault folder watching.
//!
//! The Tauri build watches the vault so files edited outside the app appear
//! without a restart; the native shell needs the same courtesy. The watcher
//! runs on its own thread and only reports *that* something changed — the
//! shell decides when a reload is safe (never while there are unsaved edits,
//! and never immediately after its own writes).

use std::path::Path;
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use notify::{RecursiveMode, Watcher};

/// A running watcher over one vault folder.
pub struct VaultWatch {
    receiver: Receiver<()>,
    /// Dropping this stops the watcher thread.
    _watcher: notify::RecommendedWatcher,
}

impl VaultWatch {
    /// Watch `dir` for `.eml` changes. `None` when the platform refuses to
    /// watch the folder (network shares, permission problems) — the app then
    /// behaves exactly as before, with the manual Reload button.
    pub fn start(dir: &Path) -> Option<Self> {
        let (sender, receiver) = channel();
        let mut watcher =
            notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                let Ok(event) = result else {
                    return;
                };
                if event.paths.iter().any(|path| is_eml(path)) {
                    let _ = sender.send(());
                }
            })
            .ok()?;
        watcher.watch(dir, RecursiveMode::NonRecursive).ok()?;
        Some(Self {
            receiver,
            _watcher: watcher,
        })
    }

    /// Drain pending events; `true` when at least one `.eml` changed.
    pub fn changed(&self) -> bool {
        let mut changed = false;
        loop {
            match self.receiver.try_recv() {
                Ok(()) => changed = true,
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        changed
    }
}

/// Only `.eml` files matter; editor temp files and `.DS_Store` do not.
pub fn is_eml(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("eml"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_eml_files_count() {
        assert!(is_eml(Path::new("C:/vault/page.eml")));
        assert!(is_eml(Path::new("C:/vault/PAGE.EML")));
        assert!(!is_eml(Path::new("C:/vault/page.txt")));
        assert!(!is_eml(Path::new("C:/vault/.DS_Store")));
        assert!(!is_eml(Path::new("C:/vault/page")));
    }

    #[test]
    fn reports_a_file_written_after_start() {
        let dir = std::env::temp_dir().join(format!("capsuleos-watch-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let Some(watcher) = VaultWatch::start(&dir) else {
            // Some CI containers cannot watch the filesystem; the fallback is
            // the manual reload, so this is not a failure.
            return;
        };
        assert!(!watcher.changed());
        std::fs::write(dir.join("new-page.eml"), b"Subject: test\r\n").expect("write");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut seen = false;
        while std::time::Instant::now() < deadline {
            if watcher.changed() {
                seen = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(seen, "the watcher never reported the new .eml");
    }
}
