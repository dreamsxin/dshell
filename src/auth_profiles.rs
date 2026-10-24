//! Reusable authentication templates.
//!
//! Templates store only the authentication method, an optional identity file
//! path, an optional jump-host reference and a keep-alive interval. Passwords,
//! passphrases and key material never enter this structure or its JSON file.

use std::{env, fs, io::ErrorKind, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::profiles::AuthMethod;

const DOCUMENT_VERSION: u8 = 1;
pub const MAX_AUTH_PROFILES: usize = 50;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthProfile {
    pub name: String,
    #[serde(default)]
    pub auth_method: AuthMethod,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub identity_file: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proxy_jump: String,
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub keepalive_seconds: u16,
}

impl AuthProfile {
    pub fn normalized(mut self) -> Result<Self, String> {
        self.name = self.name.trim().to_owned();
        self.identity_file = self.identity_file.trim().to_owned();
        self.proxy_jump = self.proxy_jump.trim().to_owned();
        if self.name.is_empty() {
            return Err("An authentication profile name is required.".to_owned());
        }
        if self.name.chars().count() > 80 {
            return Err("Authentication profile names are limited to 80 characters.".to_owned());
        }
        if self.auth_method == AuthMethod::PublicKey && self.identity_file.is_empty() {
            return Err("A public-key profile requires an identity file path.".to_owned());
        }
        if self.keepalive_seconds > 3600 {
            return Err("Keep-alive must be between 0 and 3600 seconds.".to_owned());
        }
        Ok(self)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AuthProfileDocument {
    version: u8,
    profiles: Vec<AuthProfile>,
}

pub struct AuthProfileStore {
    path: PathBuf,
}

impl AuthProfileStore {
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
            path: config_root.join("dshell").join("auth_profiles.json"),
        })
    }

    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<Vec<AuthProfile>, String> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(format!(
                    "Could not read authentication profiles at {}: {error}",
                    self.path.display()
                ));
            }
        };
        let document: AuthProfileDocument = serde_json::from_str(&contents).map_err(|error| {
            format!(
                "Could not parse authentication profiles at {}: {error}",
                self.path.display()
            )
        })?;
        if document.version != DOCUMENT_VERSION {
            return Err(format!(
                "Authentication profile file version {} is not supported.",
                document.version
            ));
        }
        let mut names = std::collections::HashSet::new();
        let mut profiles = Vec::with_capacity(document.profiles.len());
        for profile in document.profiles {
            let profile = profile.normalized()?;
            if !names.insert(profile.name.to_lowercase()) {
                return Err(format!(
                    "Authentication profile {} appears more than once.",
                    profile.name
                ));
            }
            profiles.push(profile);
        }
        if profiles.len() > MAX_AUTH_PROFILES {
            return Err(format!(
                "At most {MAX_AUTH_PROFILES} authentication profiles are supported."
            ));
        }
        profiles.sort_by_key(|profile| profile.name.to_lowercase());
        Ok(profiles)
    }

    pub fn save(&self, profiles: &[AuthProfile]) -> Result<(), String> {
        if profiles.len() > MAX_AUTH_PROFILES {
            return Err(format!(
                "At most {MAX_AUTH_PROFILES} authentication profiles are supported."
            ));
        }
        let mut names = std::collections::HashSet::new();
        let mut normalized = Vec::with_capacity(profiles.len());
        for profile in profiles {
            let profile = profile.clone().normalized()?;
            if !names.insert(profile.name.to_lowercase()) {
                return Err(format!(
                    "Authentication profile {} already exists.",
                    profile.name
                ));
            }
            normalized.push(profile);
        }
        normalized.sort_by_key(|profile| profile.name.to_lowercase());
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "The authentication profile path has no parent directory.".to_owned())?;
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Could not create authentication profile directory {}: {error}",
                parent.display()
            )
        })?;
        let contents = serde_json::to_vec_pretty(&AuthProfileDocument {
            version: DOCUMENT_VERSION,
            profiles: normalized,
        })
        .map_err(|error| format!("Could not serialize authentication profiles: {error}"))?;
        fs::write(&self.path, contents).map_err(|error| {
            format!(
                "Could not save authentication profiles at {}: {error}",
                self.path.display()
            )
        })
    }
}

fn is_zero_u16(value: &u16) -> bool {
    *value == 0
}

#[cfg(test)]
mod tests {
    use super::{AuthProfile, AuthProfileStore, MAX_AUTH_PROFILES};
    use crate::profiles::AuthMethod;

    fn profile(name: &str, method: AuthMethod) -> AuthProfile {
        AuthProfile {
            name: name.to_owned(),
            auth_method: method,
            identity_file: if method == AuthMethod::PublicKey {
                "~/.ssh/id_ed25519".to_owned()
            } else {
                String::new()
            },
            proxy_jump: String::new(),
            keepalive_seconds: 30,
        }
    }

    #[test]
    fn missing_file_loads_no_profiles() {
        let directory = tempfile::tempdir().unwrap();
        let store = AuthProfileStore::at(directory.path().join("auth_profiles.json"));
        assert!(store.load().unwrap().is_empty());
    }

    #[test]
    fn profiles_round_trip_without_secret_fields() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("auth_profiles.json");
        let store = AuthProfileStore::at(path.clone());
        let profiles = vec![
            profile("agent", AuthMethod::Agent),
            profile("key", AuthMethod::PublicKey),
        ];
        store.save(&profiles).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded, profiles);

        let contents = std::fs::read_to_string(&path).unwrap();
        let lower = contents.to_lowercase();
        assert!(!lower.contains("password"));
        assert!(!lower.contains("passphrase"));
        assert!(!lower.contains("private_key"));
    }

    #[test]
    fn duplicate_names_and_incomplete_keys_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("auth_profiles.json");
        let store = AuthProfileStore::at(path);
        let duplicate = vec![
            profile("agent", AuthMethod::Agent),
            profile("AGENT", AuthMethod::Agent),
        ];
        assert!(store.save(&duplicate).is_err());

        let incomplete = vec![AuthProfile {
            name: "key".to_owned(),
            auth_method: AuthMethod::PublicKey,
            identity_file: String::new(),
            proxy_jump: String::new(),
            keepalive_seconds: 0,
        }];
        assert!(store.save(&incomplete).is_err());
    }

    #[test]
    fn profile_count_is_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let store = AuthProfileStore::at(directory.path().join("auth_profiles.json"));
        let profiles = (0..=MAX_AUTH_PROFILES)
            .map(|index| profile(&format!("profile-{index}"), AuthMethod::Agent))
            .collect::<Vec<_>>();
        assert!(store.save(&profiles).is_err());
    }
}
