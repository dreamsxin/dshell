use std::{collections::HashSet, env, fs, io::ErrorKind, path::PathBuf};

use serde::{Deserialize, Serialize};

const DOCUMENT_VERSION: u8 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostProfile {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProfileDocument {
    version: u8,
    profiles: Vec<HostProfile>,
}

pub struct ProfileStore {
    path: PathBuf,
}

impl ProfileStore {
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
            path: config_root.join("dshell").join("hosts.json"),
        })
    }

    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<Vec<HostProfile>, String> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(format!(
                    "Could not read host profiles at {}: {error}",
                    self.path.display()
                ));
            }
        };
        let document: ProfileDocument = serde_json::from_str(&contents).map_err(|error| {
            format!(
                "Could not parse host profiles at {}: {error}",
                self.path.display()
            )
        })?;
        if document.version != DOCUMENT_VERSION {
            return Err(format!(
                "Host profile file version {} is not supported.",
                document.version
            ));
        }
        validate_profiles(&document.profiles)?;
        Ok(document.profiles)
    }

    pub fn save(&self, profiles: &[HostProfile]) -> Result<(), String> {
        validate_profiles(profiles)?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "The host profile path has no parent directory.".to_owned())?;
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Could not create host profile directory {}: {error}",
                parent.display()
            )
        })?;
        let document = ProfileDocument {
            version: DOCUMENT_VERSION,
            profiles: profiles.to_vec(),
        };
        let contents = serde_json::to_vec_pretty(&document)
            .map_err(|error| format!("Could not serialize host profiles: {error}"))?;
        fs::write(&self.path, contents).map_err(|error| {
            format!(
                "Could not save host profiles at {}: {error}",
                self.path.display()
            )
        })
    }
}

fn validate_profiles(profiles: &[HostProfile]) -> Result<(), String> {
    let mut names = HashSet::with_capacity(profiles.len());
    for profile in profiles {
        if profile.name.trim().is_empty()
            || profile.host.trim().is_empty()
            || profile.username.trim().is_empty()
            || profile.port == 0
        {
            return Err("Host profiles require a name, host, username, and valid port.".to_owned());
        }
        if !names.insert(profile.name.to_lowercase()) {
            return Err(format!(
                "Host profile names must be unique: {}",
                profile.name
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_round_trip_without_credentials() {
        let directory = tempfile::tempdir().unwrap();
        let store = ProfileStore::at(directory.path().join("hosts.json"));
        let profiles = vec![HostProfile {
            name: "staging".to_owned(),
            host: "server.example.com".to_owned(),
            port: 2222,
            username: "operator".to_owned(),
        }];

        store.save(&profiles).unwrap();
        assert_eq!(store.load().unwrap(), profiles);
        let contents = fs::read_to_string(&store.path).unwrap();
        assert!(!contents.to_lowercase().contains("password"));
    }

    #[test]
    fn missing_profile_file_is_an_empty_list() {
        let directory = tempfile::tempdir().unwrap();
        let store = ProfileStore::at(directory.path().join("missing.json"));

        assert!(store.load().unwrap().is_empty());
    }

    #[test]
    fn duplicate_profile_names_are_rejected() {
        let profile = HostProfile {
            name: "prod".to_owned(),
            host: "one.example.com".to_owned(),
            port: 22,
            username: "operator".to_owned(),
        };
        let duplicate = HostProfile {
            name: "PROD".to_owned(),
            host: "two.example.com".to_owned(),
            ..profile.clone()
        };

        assert!(validate_profiles(&[profile, duplicate]).is_err());
    }
}
