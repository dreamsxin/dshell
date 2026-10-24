use std::{collections::HashSet, env, fs, io::ErrorKind, path::PathBuf};

use serde::{Deserialize, Serialize};

const DOCUMENT_VERSION: u8 = 1;
pub const MAX_SNIPPETS: usize = 200;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSnippet {
    pub name: String,
    pub command: String,
    #[serde(default = "default_send_enter")]
    pub send_enter: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SnippetDocument {
    version: u8,
    snippets: Vec<CommandSnippet>,
}

pub struct SnippetStore {
    path: PathBuf,
}

impl SnippetStore {
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
            path: config_root.join("dshell").join("snippets.json"),
        })
    }

    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<Vec<CommandSnippet>, String> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(format!(
                    "Could not read command snippets at {}: {error}",
                    self.path.display()
                ));
            }
        };
        let document: SnippetDocument = serde_json::from_str(&contents).map_err(|error| {
            format!(
                "Could not parse command snippets at {}: {error}",
                self.path.display()
            )
        })?;
        if document.version != DOCUMENT_VERSION {
            return Err(format!(
                "Command snippet file version {} is not supported.",
                document.version
            ));
        }
        validate_snippets(&document.snippets)?;
        Ok(document.snippets)
    }

    pub fn save(&self, snippets: &[CommandSnippet]) -> Result<(), String> {
        validate_snippets(snippets)?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "The command snippet path has no parent directory.".to_owned())?;
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Could not create command snippet directory {}: {error}",
                parent.display()
            )
        })?;
        let contents = serde_json::to_vec_pretty(&SnippetDocument {
            version: DOCUMENT_VERSION,
            snippets: snippets.to_vec(),
        })
        .map_err(|error| format!("Could not serialize command snippets: {error}"))?;
        fs::write(&self.path, contents).map_err(|error| {
            format!(
                "Could not save command snippets at {}: {error}",
                self.path.display()
            )
        })
    }
}

fn default_send_enter() -> bool {
    true
}

fn validate_snippets(snippets: &[CommandSnippet]) -> Result<(), String> {
    if snippets.len() > MAX_SNIPPETS {
        return Err(format!(
            "Command snippets cannot exceed {MAX_SNIPPETS} entries."
        ));
    }
    let mut names = HashSet::with_capacity(snippets.len());
    for snippet in snippets {
        if snippet.name.trim().is_empty() || snippet.command.trim().is_empty() {
            return Err("Command snippets require a name and a command.".to_owned());
        }
        if !names.insert(snippet.name.to_lowercase()) {
            return Err(format!(
                "Command snippet names must be unique: {}",
                snippet.name
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippets_round_trip_and_default_to_pressing_enter() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("snippets.json");
        fs::write(
            &path,
            r#"{
                "version": 1,
                "snippets": [
                    { "name": "uptime", "command": "uptime" }
                ]
            }"#,
        )
        .unwrap();
        let store = SnippetStore::at(path.clone());
        let snippets = store.load().unwrap();
        assert_eq!(snippets.len(), 1);
        assert!(snippets[0].send_enter);

        store.save(&snippets).unwrap();
        assert_eq!(store.load().unwrap(), snippets);
    }

    #[test]
    fn missing_snippet_file_is_an_empty_list() {
        let directory = tempfile::tempdir().unwrap();
        let store = SnippetStore::at(directory.path().join("missing.json"));
        assert!(store.load().unwrap().is_empty());
    }

    #[test]
    fn duplicate_snippet_names_are_rejected() {
        let snippet = CommandSnippet {
            name: "deploy".to_owned(),
            command: "git pull".to_owned(),
            send_enter: true,
        };
        let duplicate = CommandSnippet {
            name: "DEPLOY".to_owned(),
            ..snippet.clone()
        };
        assert!(validate_snippets(&[snippet, duplicate]).is_err());
    }
}
