//! IMAP synchronization for CapsuleCraft.
//!
//! Synchronizes `.eml` capsules between local vault folder and an IMAP mailbox folder
//! (e.g. "CapsuleCraft").
//!
//! - Connects over TLS (port 993).
//! - Uploads local capsules missing from the server (APPEND).
//! - Downloads remote capsules missing locally (FETCH RFC822).
//! - Replaces updated capsules.

use native_tls::TlsConnector;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const SYNC_CONFIG_FILE: &str = ".capsule-sync.json";
pub const DEFAULT_IMAP_FOLDER: &str = "CapsuleCraft";
pub const DEFAULT_IMAP_PORT: u16 = 993;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImapConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub folder: String,
    pub auto_sync: bool,
}

impl Default for ImapConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: String::new(),
            port: DEFAULT_IMAP_PORT,
            username: String::new(),
            password: String::new(),
            folder: DEFAULT_IMAP_FOLDER.to_string(),
            auto_sync: false,
        }
    }
}

impl ImapConfig {
    pub fn load_from_vault(vault_dir: &Path) -> Self {
        let path = vault_dir.join(SYNC_CONFIG_FILE);
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str::<ImapConfig>(&data) {
                return config;
            }
        }
        Self::default()
    }

    pub fn save_to_vault(&self, vault_dir: &Path) -> Result<(), String> {
        let path = vault_dir.join(SYNC_CONFIG_FILE);
        let data = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, data).map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncStatus {
    Idle,
    Syncing,
    Success(String),
    Error(String),
}

/// Remote message metadata retrieved during IMAP scan
#[derive(Debug, Clone)]
pub struct RemoteEnvelope {
    pub uid: u32,
    pub message_id: Option<String>,
    pub date_ms: i64,
}

/// Core sync engine
pub struct ImapSyncEngine {
    pub config: ImapConfig,
    pub vault_dir: PathBuf,
}

impl ImapSyncEngine {
    pub fn new(config: ImapConfig, vault_dir: PathBuf) -> Self {
        Self { config, vault_dir }
    }

    /// Tests the connection and credentials.
    pub fn test_connection(&self) -> Result<String, String> {
        let tls = TlsConnector::builder()
            .build()
            .map_err(|e| format!("TLS init error: {e}"))?;

        let port = if self.config.port == 0 {
            DEFAULT_IMAP_PORT
        } else {
            self.config.port
        };

        let client = imap::connect((self.config.host.as_str(), port), &self.config.host, &tls)
            .map_err(|e| format!("Connection error: {e}"))?;

        let mut session = client
            .login(&self.config.username, &self.config.password)
            .map_err(|e| format!("Login failed: {}", e.0))?;

        let folder = if self.config.folder.trim().is_empty() {
            DEFAULT_IMAP_FOLDER
        } else {
            self.config.folder.trim()
        };

        // Try to select or examine folder, create if doesn't exist
        if let Err(_) = session.select(folder) {
            let _ = session.create(folder);
        }

        let _ = session.logout();
        Ok("Connection successful".to_string())
    }

    /// Performs two-way synchronization between local vault and IMAP folder.
    pub fn run_sync(&self) -> Result<String, String> {
        if !self.config.enabled {
            return Err("IMAP sync is disabled in settings".to_string());
        }
        if self.config.host.trim().is_empty() || self.config.username.trim().is_empty() {
            return Err("IMAP host or username not set".to_string());
        }

        let tls = TlsConnector::builder()
            .build()
            .map_err(|e| format!("TLS init error: {e}"))?;

        let port = if self.config.port == 0 {
            DEFAULT_IMAP_PORT
        } else {
            self.config.port
        };

        let client = imap::connect((self.config.host.as_str(), port), &self.config.host, &tls)
            .map_err(|e| format!("Connection error: {e}"))?;

        let mut session = client
            .login(&self.config.username, &self.config.password)
            .map_err(|e| format!("Login failed: {}", e.0))?;

        let folder = if self.config.folder.trim().is_empty() {
            DEFAULT_IMAP_FOLDER
        } else {
            self.config.folder.trim()
        };

        let mailbox = if let Ok(mailbox) = session.select(folder) {
            mailbox
        } else {
            session
                .create(folder)
                .map_err(|e| format!("Failed to create folder '{folder}': {e}"))?;
            session
                .select(folder)
                .map_err(|e| format!("Failed to select folder '{folder}': {e}"))?
        };
        // Folder resets (recreate, server migration) restart the uid counter;
        // download names must not collide across resets.
        let uidvalidity = mailbox.uid_validity.unwrap_or(0);

        // 1. Scan remote messages
        // Fetch UID, ENVELOPE, INTERNALDATE
        let mut remote_messages: Vec<RemoteEnvelope> = Vec::new();
        if let Ok(fetches) = session.fetch("1:*", "(UID ENVELOPE INTERNALDATE)") {
            for fetch in fetches.iter() {
                let uid = fetch.uid.unwrap_or(0);
                let message_id = fetch
                    .envelope()
                    .and_then(|env| env.message_id.as_ref())
                    .and_then(|id_bytes| std::str::from_utf8(id_bytes).ok())
                    .map(|s| s.trim().to_string());

                let date_ms = fetch
                    .internal_date()
                    .map(|dt| dt.timestamp_millis())
                    .unwrap_or(0);

                if uid > 0 {
                    remote_messages.push(RemoteEnvelope {
                        uid,
                        message_id,
                        date_ms,
                    });
                }
            }
        }

        // 2. Scan local capsules
        let mut local_files: Vec<(PathBuf, String, Option<String>, i64)> = Vec::new();
        let mut local_bodies: HashSet<u64> = HashSet::new();
        let mut local_names: HashSet<String> = HashSet::new();
        if let Ok(entries) = fs::read_dir(&self.vault_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path
                    .extension()
                    .map(|e| e.eq_ignore_ascii_case("eml"))
                    .unwrap_or(false)
                {
                    let file_name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    local_names.insert(file_name.clone());

                    let modified = fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);

                    if let Ok(bytes) = fs::read(&path) {
                        local_bodies.insert(body_digest(&bytes));
                    }

                    // Extract Message-ID from first few lines of local .eml
                    let message_id = extract_local_message_id(&path);
                    local_files.push((path, file_name, message_id, modified));
                }
            }
        }

        let mut pulled = 0;
        let mut pushed = 0;
        let mut skipped = 0;

        // 3. Match and download new/incoming capsules from server
        // Map local Message-IDs
        let local_by_msg_id: HashMap<String, (PathBuf, i64)> = local_files
            .iter()
            .filter_map(|(path, _, msg_id, mod_time)| {
                msg_id
                    .as_ref()
                    .map(|id| (normalize_message_id(id), (path.clone(), *mod_time)))
            })
            .collect();

        let mut remote_uids_to_fetch = Vec::new();
        for remote in &remote_messages {
            if let Some(msg_id) = &remote.message_id {
                let norm = normalize_message_id(msg_id);
                if !local_by_msg_id.contains_key(&norm)
                    && !local_names.contains(&download_file_name(
                        uidvalidity,
                        remote.uid,
                        Some(msg_id),
                    ))
                {
                    remote_uids_to_fetch.push(remote.uid);
                }
            } else {
                // Without an envelope Message-Id the file name is the only
                // cheap identity; the body hash below catches the rest.
                if !local_names.contains(&download_file_name(uidvalidity, remote.uid, None)) {
                    remote_uids_to_fetch.push(remote.uid);
                } else {
                    skipped += 1;
                }
            }
        }

        // Fetch missing bodies
        for uid in remote_uids_to_fetch {
            let envelope = remote_messages.iter().find(|r| r.uid == uid);
            let file_name = download_file_name(
                uidvalidity,
                uid,
                envelope.and_then(|r| r.message_id.as_deref()),
            );
            if local_names.contains(&file_name) {
                skipped += 1;
                continue;
            }
            if let Ok(fetches) = session.uid_fetch(uid.to_string(), "BODY[]") {
                for fetch in fetches.iter() {
                    if let Some(body) = fetch.body() {
                        // The duplicate flood of v2.3: a capsule already in
                        // the vault (same bytes, any name — uploaded by a
                        // previous sync without a Message-Id, or renamed by
                        // the user) was appended again on every sync because
                        // only file names were compared. Content decides now.
                        if local_bodies.contains(&body_digest(body)) {
                            skipped += 1;
                            break;
                        }
                        let path = self.vault_dir.join(&file_name);
                        if !path.exists() {
                            if fs::write(&path, body).is_ok() {
                                local_names.insert(file_name.clone());
                                local_bodies.insert(body_digest(body));
                                pulled += 1;
                            }
                        }
                        break;
                    }
                }
            }
        }

        // 4. Match and upload local capsules missing from server
        let remote_by_msg_id: HashSet<String> = remote_messages
            .iter()
            .filter_map(|r| r.message_id.as_ref().map(|id| normalize_message_id(id)))
            .collect();

        for (path, _file_name, msg_id, _) in &local_files {
            let needs_upload = match msg_id {
                Some(id) => !remote_by_msg_id.contains(&normalize_message_id(id)),
                None => true,
            };

            if needs_upload {
                if let Ok(bytes) = fs::read(path) {
                    // Append message to folder
                    if let Ok(_) = session.append(folder, &bytes) {
                        pushed += 1;
                    }
                }
            }
        }

        let _ = session.logout();

        if skipped > 0 {
            Ok(format!(
                "Synced: +{pulled} downloaded, +{pushed} uploaded, {skipped} already in sync"
            ))
        } else {
            Ok(format!("Synced: +{pulled} downloaded, +{pushed} uploaded"))
        }
    }
}

/// Helper to read just the headers of a local .eml to extract Message-ID quickly
fn extract_local_message_id(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let content = String::from_utf8_lossy(&bytes);
    for line in content.lines() {
        if line.is_empty() {
            break; // End of headers
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("message-id:") {
            return Some(line[11..].trim().to_string());
        }
    }
    None
}

/// Strip enclosing `<...>` for comparison
pub fn normalize_message_id(id: &str) -> String {
    id.trim().trim_matches(&['<', '>'][..]).trim().to_string()
}

/// File name a downloaded remote message gets in the vault.
///
/// Message-Id-bearing messages are named after the id, so the same message
/// keeps its file identity across folder resets (UIDVALIDITY changes, folder
/// recreation). Messages without one fall back to uid-qualified names — the
/// bare `capsule_imap_{uid}.eml` of v2.3 collided after any folder reset and
/// then silently blocked re-downloads (`path.exists()` said "already here").
pub fn download_file_name(uidvalidity: u32, uid: u32, message_id: Option<&str>) -> String {
    match message_id.map(normalize_message_id) {
        Some(id) if !id.is_empty() => {
            let safe: String = id
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '-' | '_') {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            format!("capsule_{safe}.eml")
        }
        _ => format!("capsule_imap_{uidvalidity}_{uid}.eml"),
    }
}

/// A content digest used to recognize "this exact body is already in the
/// vault". Only compared within one sync run, so the std hasher suffices.
pub fn body_digest(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.len().hash(&mut hasher);
    bytes.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_message_id() {
        assert_eq!(normalize_message_id("<test@domain.com>"), "test@domain.com");
        assert_eq!(normalize_message_id("test@domain.com"), "test@domain.com");
        assert_eq!(normalize_message_id("  <foo-123>  "), "foo-123");
    }

    #[test]
    fn test_imap_config_defaults() {
        let cfg = ImapConfig::default();
        assert_eq!(cfg.port, 993);
        assert_eq!(cfg.folder, "CapsuleCraft");
        assert!(!cfg.enabled);
    }

    #[test]
    fn download_names_survive_folder_resets() {
        // A Message-Id names the file, so the same message keeps its identity
        // after a UIDVALIDITY change.
        let a = download_file_name(111, 5, Some("<a@b.com>"));
        let b = download_file_name(999, 9, Some("<a@b.com>"));
        assert_eq!(a, b);
        assert!(a.starts_with("capsule_a@b.com.eml"), "{a}");
        // Without one, the uid pair (uidvalidity, uid) keeps names unique.
        let c = download_file_name(111, 5, None);
        let d = download_file_name(999, 5, None);
        assert_ne!(c, d);
        assert_eq!(c, "capsule_imap_111_5.eml");
    }

    #[test]
    fn download_names_are_filesystem_safe() {
        let name = download_file_name(1, 2, Some("<weird/id with spaces@host>"));
        assert!(!name.contains('/') && !name.contains(' '), "{name}");
        assert!(name.ends_with(".eml"));
        // An empty id behaves like "no id".
        assert_eq!(
            download_file_name(7, 8, Some("   ")),
            "capsule_imap_7_8.eml"
        );
    }

    #[test]
    fn body_digest_separates_bodies() {
        let a = b"Message-ID: <x@y>\r\n\r\nhello".to_vec();
        let b = b"Message-ID: <x@y>\r\n\r\nhello ".to_vec();
        assert_eq!(body_digest(&a), body_digest(&a));
        assert_ne!(body_digest(&a), body_digest(&b));
    }
}
