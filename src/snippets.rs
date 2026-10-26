use std::{
    collections::{BTreeMap, HashSet},
    env,
    fs::{self, File},
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const DOCUMENT_VERSION: u8 = 2;
const SUPPORTED_DOCUMENT_VERSIONS: [u8; 2] = [1, DOCUMENT_VERSION];
pub const MAX_SNIPPETS: usize = 200;
pub const MAX_SNIPPET_GROUP_CHARS: usize = 48;
const MAX_SNIPPET_BUNDLE_BYTES: u64 = 1_048_576;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSnippet {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    pub command: String,
    #[serde(default = "default_send_enter")]
    pub send_enter: bool,
}

pub fn expand_session_variables(command: &str, host: &str, port: u16, username: &str) -> String {
    let port = port.to_string();
    let variables = [
        ("{{host}}", shell_quote(host)),
        ("{{port}}", shell_quote(&port)),
        ("{{user}}", shell_quote(username)),
    ];
    let mut rendered = String::with_capacity(command.len());
    let mut remaining = command;

    loop {
        let next = variables
            .iter()
            .filter_map(|(token, value)| remaining.find(token).map(|index| (index, *token, value)))
            .min_by_key(|(index, _, _)| *index);
        let Some((index, token, value)) = next else {
            rendered.push_str(remaining);
            break;
        };
        rendered.push_str(&remaining[..index]);
        rendered.push_str(value);
        remaining = &remaining[index + token.len()..];
    }

    rendered
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
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
        if !SUPPORTED_DOCUMENT_VERSIONS.contains(&document.version) {
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

pub fn export_bundle(path: &Path, snippets: &[CommandSnippet]) -> Result<(), String> {
    validate_snippets(snippets)?;
    let contents = serde_json::to_vec_pretty(&SnippetDocument {
        version: DOCUMENT_VERSION,
        snippets: snippets.to_vec(),
    })
    .map_err(|error| format!("Could not serialize command snippets: {error}"))?;
    if contents.len() as u64 > MAX_SNIPPET_BUNDLE_BYTES {
        return Err("The command snippet bundle exceeds the 1 MiB size limit.".to_owned());
    }
    fs::write(path, contents).map_err(|error| {
        format!(
            "Could not export command snippets to {}: {error}",
            path.display()
        )
    })
}

pub fn import_bundle(path: &Path) -> Result<Vec<CommandSnippet>, String> {
    let metadata = fs::metadata(path).map_err(|error| {
        format!(
            "Could not inspect command snippet bundle {}: {error}",
            path.display()
        )
    })?;
    if !metadata.is_file() {
        return Err("The selected command snippet bundle is not a regular file.".to_owned());
    }
    if metadata.len() > MAX_SNIPPET_BUNDLE_BYTES {
        return Err("The command snippet bundle exceeds the 1 MiB size limit.".to_owned());
    }

    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| {
            format!(
                "Could not open command snippet bundle {}: {error}",
                path.display()
            )
        })?
        .take(MAX_SNIPPET_BUNDLE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read command snippet bundle: {error}"))?;
    if bytes.len() as u64 > MAX_SNIPPET_BUNDLE_BYTES {
        return Err("The command snippet bundle exceeds the 1 MiB size limit.".to_owned());
    }
    let contents = String::from_utf8(bytes)
        .map_err(|error| format!("The command snippet bundle is not valid UTF-8: {error}"))?;
    let document: SnippetDocument = serde_json::from_str(&contents)
        .map_err(|error| format!("Could not parse command snippet bundle: {error}"))?;
    if !SUPPORTED_DOCUMENT_VERSIONS.contains(&document.version) {
        return Err(format!(
            "Command snippet bundle version {} is not supported.",
            document.version
        ));
    }
    validate_snippets(&document.snippets)?;
    Ok(document.snippets)
}

pub fn merge_unique(
    existing: &[CommandSnippet],
    imported: Vec<CommandSnippet>,
) -> Result<(Vec<CommandSnippet>, usize), String> {
    validate_snippets(existing)?;
    validate_snippets(&imported)?;

    let mut merged = existing.to_vec();
    let mut names = existing
        .iter()
        .map(|snippet| snippet.name.to_lowercase())
        .collect::<HashSet<_>>();
    let mut skipped = 0;
    for snippet in imported {
        if names.insert(snippet.name.to_lowercase()) {
            merged.push(snippet);
        } else {
            skipped += 1;
        }
    }
    if merged.len() > MAX_SNIPPETS {
        return Err(format!(
            "Importing this bundle would exceed the {MAX_SNIPPETS} command limit."
        ));
    }
    merged.sort_by_key(|snippet| snippet.name.to_lowercase());
    Ok((merged, skipped))
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
        validate_group_name(&snippet.group).map_err(str::to_owned)?;
        if !names.insert(snippet.name.to_lowercase()) {
            return Err(format!(
                "Command snippet names must be unique: {}",
                snippet.name
            ));
        }
    }
    Ok(())
}

pub fn validate_group_name(group: &str) -> Result<(), &'static str> {
    if group.trim() != group
        || group.chars().count() > MAX_SNIPPET_GROUP_CHARS
        || group.chars().any(char::is_control)
    {
        return Err(
            "Command group names must be trimmed, at most 48 characters, and contain no control characters.",
        );
    }
    Ok(())
}

pub fn ordered_group_indices(snippets: &[CommandSnippet]) -> Vec<(String, Vec<usize>)> {
    let mut groups = BTreeMap::<String, (String, Vec<usize>)>::new();
    for (index, snippet) in snippets.iter().enumerate() {
        let display_name = snippet.group.trim();
        let key = display_name.to_lowercase();
        groups
            .entry(key)
            .or_insert_with(|| (display_name.to_owned(), Vec::new()))
            .1
            .push(index);
    }
    groups.into_values().collect()
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
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["version"].as_u64(), Some(u64::from(DOCUMENT_VERSION)));
        assert!(saved["snippets"][0].get("group").is_none());
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
            group: String::new(),
            command: "git pull".to_owned(),
            send_enter: true,
        };
        let duplicate = CommandSnippet {
            name: "DEPLOY".to_owned(),
            ..snippet.clone()
        };
        assert!(validate_snippets(&[snippet, duplicate]).is_err());
    }

    #[test]
    fn command_groups_are_bounded_trimmed_and_control_free() {
        assert!(validate_group_name("").is_ok());
        assert!(validate_group_name("运维工具").is_ok());
        assert!(validate_group_name(&"x".repeat(MAX_SNIPPET_GROUP_CHARS)).is_ok());
        assert!(validate_group_name(&"x".repeat(MAX_SNIPPET_GROUP_CHARS + 1)).is_err());
        assert!(validate_group_name(" operations").is_err());
        assert!(validate_group_name("operations\nprod").is_err());
    }

    #[test]
    fn command_groups_are_case_insensitive_sorted_and_keep_original_order() {
        let snippets = [
            CommandSnippet {
                name: "b".to_owned(),
                group: "Network".to_owned(),
                command: "true".to_owned(),
                send_enter: true,
            },
            CommandSnippet {
                name: "a".to_owned(),
                group: String::new(),
                command: "true".to_owned(),
                send_enter: true,
            },
            CommandSnippet {
                name: "c".to_owned(),
                group: "diagnostics".to_owned(),
                command: "true".to_owned(),
                send_enter: true,
            },
            CommandSnippet {
                name: "d".to_owned(),
                group: "network".to_owned(),
                command: "true".to_owned(),
                send_enter: true,
            },
        ];

        assert_eq!(
            ordered_group_indices(&snippets),
            vec![
                (String::new(), vec![1]),
                ("diagnostics".to_owned(), vec![2]),
                ("Network".to_owned(), vec![0, 3]),
            ]
        );
    }

    #[test]
    fn session_variables_expand_per_target_with_posix_shell_quoting() {
        assert_eq!(
            expand_session_variables(
                "inspect {{host}} {{port}} {{user}}",
                "db'test",
                2222,
                "ops $(id)",
            ),
            "inspect 'db'\\''test' '2222' 'ops $(id)'"
        );
    }

    #[test]
    fn session_variable_expansion_leaves_unknown_and_missing_tokens_untouched() {
        assert_eq!(
            expand_session_variables("echo {{host}} {{unknown}} {{port", "host", 22, "user"),
            "echo 'host' {{unknown}} {{port"
        );
        assert_eq!(
            expand_session_variables("uptime", "host", 22, "user"),
            "uptime"
        );
    }

    #[test]
    fn session_variable_values_are_not_expanded_recursively() {
        assert_eq!(
            expand_session_variables("{{user}} {{host}}", "host{{user}}", 22, "user{{host}}"),
            "'user{{host}}' 'host{{user}}'"
        );
    }

    #[test]
    fn command_bundle_round_trips_and_defaults_send_enter() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("team-commands.json");
        let snippets = vec![CommandSnippet {
            name: "uptime".to_owned(),
            group: "Diagnostics".to_owned(),
            command: "uptime".to_owned(),
            send_enter: false,
        }];

        export_bundle(&path, &snippets).unwrap();
        assert_eq!(import_bundle(&path).unwrap(), snippets);

        fs::write(
            &path,
            r#"{"version":1,"snippets":[{"name":"disk","command":"df -h"}]}"#,
        )
        .unwrap();
        let legacy = import_bundle(&path).unwrap().remove(0);
        assert!(legacy.send_enter);
        assert!(legacy.group.is_empty());
    }

    #[test]
    fn command_bundle_rejects_unsupported_or_oversized_documents() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("commands.json");
        fs::write(&path, r#"{"version":3,"snippets":[]}"#).unwrap();
        assert!(import_bundle(&path).unwrap_err().contains("version 3"));

        fs::write(&path, vec![b' '; MAX_SNIPPET_BUNDLE_BYTES as usize + 1]).unwrap();
        assert!(import_bundle(&path).unwrap_err().contains("1 MiB"));
    }

    #[test]
    fn merge_skips_existing_names_and_rejects_overflow_without_mutating_existing() {
        let existing = vec![CommandSnippet {
            name: "uptime".to_owned(),
            group: String::new(),
            command: "uptime".to_owned(),
            send_enter: true,
        }];
        let imported = vec![
            CommandSnippet {
                name: "UPTIME".to_owned(),
                group: "Other".to_owned(),
                command: "uptime -p".to_owned(),
                send_enter: true,
            },
            CommandSnippet {
                name: "disk".to_owned(),
                group: "Diagnostics".to_owned(),
                command: "df -h".to_owned(),
                send_enter: false,
            },
        ];
        let (merged, skipped) = merge_unique(&existing, imported).unwrap();
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[1].name, "uptime");
        assert_eq!(skipped, 1);

        let full = (0..MAX_SNIPPETS)
            .map(|index| CommandSnippet {
                name: format!("command-{index}"),
                group: String::new(),
                command: "true".to_owned(),
                send_enter: true,
            })
            .collect::<Vec<_>>();
        let incoming = vec![CommandSnippet {
            name: "overflow".to_owned(),
            group: String::new(),
            command: "true".to_owned(),
            send_enter: true,
        }];
        assert!(merge_unique(&full, incoming).is_err());
        assert_eq!(full.len(), MAX_SNIPPETS);
    }
}
