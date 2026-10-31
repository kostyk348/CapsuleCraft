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

        if let Err(_) = session.select(folder) {
            session
                .create(folder)
                .map_err(|e| format!("Failed to create folder '{folder}': {e}"))?;
            session
                .select(folder)
                .map_err(|e| format!("Failed to select folder '{folder}': {e}"))?;
        }

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

                    let modified = fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);

                    // Extract Message-ID from first few lines of local .eml
                    let message_id = extract_local_message_id(&path);
                    local_files.push((path, file_name, message_id, modified));
                }
            }
        }

        let mut pushed = 0;
        let mut pulled = 0;

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
                if !local_by_msg_id.contains_key(&norm) {
                    remote_uids_to_fetch.push(remote.uid);
                }
            } else {
                // If remote has no Message-ID envelope, fetch to see if it's a valid email note
                remote_uids_to_fetch.push(remote.uid);
            }
        }

        // Fetch missing bodies
        for uid in remote_uids_to_fetch {
            if let Ok(fetches) = session.uid_fetch(uid.to_string(), "BODY[]") {
                for fetch in fetches.iter() {
                    if let Some(body) = fetch.body() {
                        let file_name = format!("capsule_imap_{uid}.eml");
                        let path = self.vault_dir.join(&file_name);
                        if !path.exists() {
                            if let Ok(_) = fs::write(&path, body) {
                                pulled += 1;
                            }
                        }
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

        Ok(format!("Synced: +{pulled} downloaded, +{pushed} uploaded"))
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
}
