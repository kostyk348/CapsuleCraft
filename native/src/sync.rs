//! Folder sync: two-way merge between the vault and any folder another
//! device keeps in step (Dropbox, Nextcloud, Syncthing, a USB drive).
//!
//! The vault is a folder of `.eml` pages — one file per capsule — so the
//! unit of sync is the file. The plan is last-writer-wins per file by
//! modification time; a file present on one side only is copied to the
//! other. No daemon, no server, no account: the merge is a pure function
//! over two directory listings, applied by plain file copies.

use std::path::{Path, PathBuf};

/// One synced page: the file name inside the vault plus the facts the merge
/// needs. `mtime_ms` comes from the filesystem; two devices disagree about
/// clocks, so a second of slack is baked into the conflict rule below.
#[derive(Clone, Debug, PartialEq)]
pub struct SyncRecord {
    pub file: String,
    pub mtime_ms: u64,
    pub size: u64,
}

/// What to do with one file after comparing the two listings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncDirection {
    /// Copy local → remote.
    Push,
    /// Copy remote → local.
    Pull,
    /// Both sides agree; nothing to move.
    Skip,
}

/// A full merge plan; `push`/`pull` name the files to copy in each direction.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncPlan {
    pub push: Vec<String>,
    pub pull: Vec<String>,
}

impl SyncPlan {
    pub fn is_empty(&self) -> bool {
        self.push.is_empty() && self.pull.is_empty()
    }
}

/// True for vault pages; ignore everything else (notes, temp files).
pub fn is_eml(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".eml")
}

/// Lists the `.eml` pages of a folder with mtime/size facts. A missing or
/// unreadable folder is an empty listing — the merge then simply pushes or
/// pulls everything, which is also the correct behavior for a fresh remote.
pub fn scan_dir(dir: &Path) -> Vec<SyncRecord> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut records = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !is_eml(&name) {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if !meta.is_file() {
                continue;
            }
            records.push(SyncRecord {
                file: name,
                mtime_ms: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
                size: meta.len(),
            });
        }
    }
    records.sort_by(|a, b| a.file.cmp(&b.file));
    records
}

/// The merge rule. Two-sided files go by mtime (last writer wins; a tie or
/// a ≤2 s disagreement counts as "the same edit" and stays untouched);
/// one-sided files move toward the side missing them.
pub fn plan_sync(local: &[SyncRecord], remote: &[SyncRecord]) -> SyncPlan {
    const CLOCK_SLACK_MS: u64 = 2_000;
    let mut plan = SyncPlan::default();
    for local_record in local {
        match remote.iter().find(|r| r.file == local_record.file) {
            None => plan.push.push(local_record.file.clone()),
            Some(remote_record) => {
                if local_record.mtime_ms > remote_record.mtime_ms + CLOCK_SLACK_MS {
                    plan.push.push(local_record.file.clone());
                } else if remote_record.mtime_ms > local_record.mtime_ms + CLOCK_SLACK_MS {
                    plan.pull.push(remote_record.file.clone());
                }
            }
        }
    }
    for remote_record in remote {
        if !local.iter().any(|r| r.file == remote_record.file) {
            plan.pull.push(remote_record.file.clone());
        }
    }
    plan
}

/// Applies a plan with real copies. The vault wins ties on disk by copying
/// first and touching mtimes through the copy itself. Returns the number of
/// files moved in each direction.
pub fn apply_plan(vault_dir: &Path, remote_dir: &Path, plan: &SyncPlan) -> (usize, usize) {
    let _ = std::fs::create_dir_all(remote_dir);
    let _ = std::fs::create_dir_all(vault_dir);
    let mut pushed = 0;
    let mut pulled = 0;
    for file in &plan.push {
        if copy_file(&vault_dir.join(file), &remote_dir.join(file)) {
            pushed += 1;
        }
    }
    for file in &plan.pull {
        if copy_file(&remote_dir.join(file), &vault_dir.join(file)) {
            pulled += 1;
        }
    }
    (pushed, pulled)
}

fn copy_file(from: &PathBuf, to: &PathBuf) -> bool {
    match std::fs::copy(from, to) {
        Ok(_) => true,
        Err(_) => false,
    }
}

/// Convenience used by the shell: scan both sides, apply, report.
pub fn sync_now(vault_dir: &Path, remote_dir: &Path) -> Result<(usize, usize), String> {
    let plan = plan_sync(&scan_dir(vault_dir), &scan_dir(remote_dir));
    if plan.is_empty() {
        return Ok((0, 0));
    }
    Ok(apply_plan(vault_dir, remote_dir, &plan))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(file: &str, mtime_ms: u64, size: u64) -> SyncRecord {
        SyncRecord {
            file: file.to_string(),
            mtime_ms,
            size,
        }
    }

    #[test]
    fn one_sided_files_move_toward_the_missing_side() {
        let local = vec![record("a.eml", 100, 10), record("b.eml", 100, 10)];
        let remote = vec![record("b.eml", 100, 10), record("c.eml", 100, 10)];
        let plan = plan_sync(&local, &remote);
        assert_eq!(plan.push, vec!["a.eml".to_string()]);
        assert_eq!(plan.pull, vec!["c.eml".to_string()]);
    }

    #[test]
    fn newer_mtime_wins_with_clock_slack() {
        let local = vec![record("a.eml", 10_000, 10)];
        let remote_newer = vec![record("a.eml", 15_000, 10)];
        assert_eq!(plan_sync(&local, &remote_newer).pull, vec!["a.eml"]);
        let local_newer = vec![record("a.eml", 15_000, 10)];
        let remote = vec![record("a.eml", 10_000, 10)];
        assert_eq!(plan_sync(&local_newer, &remote).push, vec!["a.eml"]);
        // A 1 s disagreement is clock noise, not a newer edit.
        let noise_a = vec![record("a.eml", 10_000, 10)];
        let noise_b = vec![record("a.eml", 11_000, 10)];
        assert!(plan_sync(&noise_a, &noise_b).is_empty());
    }

    #[test]
    fn identical_sides_plan_nothing() {
        let side = vec![record("a.eml", 100, 10), record("b.eml", 200, 20)];
        assert!(plan_sync(&side, &side).is_empty());
    }

    #[test]
    fn apply_plan_copies_files_both_ways() {
        let tmp = std::env::temp_dir().join(format!("cc-sync-test-{}", std::process::id()));
        let vault = tmp.join("vault");
        let remote = tmp.join("remote");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::create_dir_all(&remote).unwrap();
        std::fs::write(vault.join("a.eml"), "local only").unwrap();
        std::fs::write(remote.join("c.eml"), "remote only").unwrap();

        let plan = plan_sync(&scan_dir(&vault), &scan_dir(&remote));
        let (pushed, pulled) = apply_plan(&vault, &remote, &plan);
        assert_eq!((pushed, pulled), (1, 1));
        assert_eq!(
            std::fs::read_to_string(remote.join("a.eml")).unwrap(),
            "local only"
        );
        assert_eq!(
            std::fs::read_to_string(vault.join("c.eml")).unwrap(),
            "remote only"
        );

        // Second run: both sides agree, nothing moves.
        let plan = plan_sync(&scan_dir(&vault), &scan_dir(&remote));
        assert!(plan.is_empty());
        assert_eq!(sync_now(&vault, &remote).unwrap(), (0, 0));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_dir_ignores_non_eml_and_missing_folders() {
        let tmp = std::env::temp_dir().join(format!("cc-scan-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("page.eml"), "x").unwrap();
        std::fs::write(tmp.join("notes.txt"), "y").unwrap();
        let records = scan_dir(&tmp);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].file, "page.eml");
        assert!(scan_dir(&tmp.join("does-not-exist")).is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
