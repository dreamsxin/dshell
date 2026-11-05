//! Persisted session restore state.
//!
//! Records the set of connected SSH sessions (connection identity only —
//! never credentials) so dshell can offer to restore them on the next
//! launch. The store is rewritten whenever a session connects or closes;
//! the file is small and bounded by the number of active sessions.

use std::path::PathBuf;
use std::{env, fs};

use serde::{Deserialize, Serialize};

/// One restore entry: the saved connection identity for a session that was
/// active when dshell last ran.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RestoreEntry {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct RestoreDocument {
    version: u32,
    sessions: Vec<RestoreEntry>,
}

impl Default for RestoreDocument {
    fn default() -> Self {
        Self {
            version: RESTORE_DOCUMENT_VERSION,
            sessions: Vec::new(),
        }
    }
}

pub const RESTORE_DOCUMENT_VERSION: u32 = 1;
const SUPPORTED_RESTORE_VERSIONS: [u32; 1] = [1];

pub struct SessionRestoreStore {
    path: PathBuf,
}

impl SessionRestoreStore {
    pub fn discover() -> Result<Self, String> {
        let config_root = if cfg!(windows) {
            env::var_os("APPDATA")
                .or_else(|| {
                    env::var_os("USERPROFILE").map(|path| {
                        PathBuf::from(path)
                            .join("AppData")
                            .join("Roaming")
                            .into_os_string()
                    })
                })
                .map(PathBuf::from)
        } else {
            env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| env::var_os("HOME").map(|path| PathBuf::from(path).join(".config")))
        }
        .ok_or_else(|| "Could not locate the user configuration directory.".to_owned())?;

        Ok(Self {
            path: config_root.join("dshell").join("restore.json"),
        })
    }

    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<Vec<RestoreEntry>, String> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => {
                return Err(format!("Could not read the session restore file: {error}"));
            }
        };
        let document: RestoreDocument = serde_json::from_str(&contents)
            .map_err(|error| format!("Could not parse the session restore file: {error}"))?;
        if !SUPPORTED_RESTORE_VERSIONS.contains(&document.version) {
            return Ok(Vec::new());
        }
        Ok(document.sessions)
    }

    pub fn save(&self, sessions: &[RestoreEntry]) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Could not create the session restore directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        let document = RestoreDocument {
            version: RESTORE_DOCUMENT_VERSION,
            sessions: sessions.to_vec(),
        };
        let contents = serde_json::to_vec_pretty(&document)
            .map_err(|error| format!("Could not serialize session restore state: {error}"))?;
        fs::write(&self.path, contents).map_err(|error| {
            format!(
                "Could not write the session restore file {}: {error}",
                self.path.display()
            )
        })
    }

    /// Remove the restore file so the next launch does not offer stale
    /// sessions (used when the last session closes).
    pub fn clear(&self) -> Result<(), String> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("Could not clear the session restore file: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> RestoreEntry {
        RestoreEntry {
            name: name.to_owned(),
            host: "server.example.com".to_owned(),
            port: 22,
            username: "operator".to_owned(),
        }
    }

    #[test]
    fn restore_store_round_trips_entries_and_never_persists_credentials() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionRestoreStore::at(directory.path().join("restore.json"));
        assert_eq!(store.load().unwrap().len(), 0);

        store.save(&[entry("prod"), entry("staging")]).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "prod");
        assert_eq!(loaded[1].host, "server.example.com");

        store.clear().unwrap();
        assert_eq!(store.load().unwrap().len(), 0);
        // Clearing twice is idempotent.
        store.clear().unwrap();
    }

    #[test]
    fn restore_store_ignores_unknown_versions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("restore.json");
        std::fs::write(
            &path,
            r#"{"version":99,"sessions":[{"name":"x","host":"h","port":1,"username":"u"}]}"#,
        )
        .unwrap();
        let store = SessionRestoreStore::at(path);
        assert_eq!(store.load().unwrap().len(), 0);
    }
}
