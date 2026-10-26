use std::{collections::HashSet, env, fs, io::ErrorKind, path::PathBuf};

use crate::terminal::TerminalFrame;
use serde::{Deserialize, Serialize};

const DOCUMENT_VERSION: u8 = 1;
pub const MAX_TRIGGER_RULES: usize = 32;
pub const MAX_TRIGGER_KEYWORD_LENGTH: usize = 128;
pub const MAX_TRIGGER_TEXT_LENGTH: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerActionKind {
    Highlight,
    Notify,
    SendText,
    PromptResponse,
}

impl TriggerActionKind {
    pub const ALL: [Self; 4] = [
        Self::Highlight,
        Self::Notify,
        Self::SendText,
        Self::PromptResponse,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Highlight => "Highlight",
            Self::Notify => "Notify",
            Self::SendText => "Send text",
            Self::PromptResponse => "Ask and send",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TriggerAction {
    Highlight,
    Notify,
    SendText { text: String, send_enter: bool },
    PromptResponse { label: String },
}

impl TriggerAction {
    pub fn kind(&self) -> TriggerActionKind {
        match self {
            Self::Highlight => TriggerActionKind::Highlight,
            Self::Notify => TriggerActionKind::Notify,
            Self::SendText { .. } => TriggerActionKind::SendText,
            Self::PromptResponse { .. } => TriggerActionKind::PromptResponse,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerRule {
    pub keyword: String,
    pub action: TriggerAction,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TriggerSettings {
    pub engine_enabled: bool,
    pub rules: Vec<TriggerRule>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TriggerDocument {
    version: u8,
    engine_enabled: bool,
    rules: Vec<TriggerRule>,
}

pub struct TriggerStore {
    path: PathBuf,
}

impl TriggerStore {
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
            path: config_root.join("dshell").join("triggers.json"),
        })
    }

    #[cfg(test)]
    fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<TriggerSettings, String> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(TriggerSettings::default());
            }
            Err(error) => {
                return Err(format!(
                    "Could not read trigger rules at {}: {error}",
                    self.path.display()
                ));
            }
        };
        let document: TriggerDocument = serde_json::from_str(&contents)
            .map_err(|error| format!("Could not parse trigger rules: {error}"))?;
        if document.version != DOCUMENT_VERSION {
            return Err(format!(
                "Trigger rules version {} is not supported.",
                document.version
            ));
        }
        validate_persisted_rules(&document.rules)?;
        Ok(TriggerSettings {
            engine_enabled: document.engine_enabled,
            rules: document.rules,
        })
    }

    pub fn save(&self, settings: &TriggerSettings) -> Result<(), String> {
        let rules: Vec<_> = settings
            .rules
            .iter()
            .filter(|rule| !matches!(rule.action, TriggerAction::SendText { .. }))
            .cloned()
            .collect();
        validate_persisted_rules(&rules)?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "The trigger rules path has no parent directory.".to_owned())?;
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Could not create trigger rules directory {}: {error}",
                parent.display()
            )
        })?;
        let contents = serde_json::to_vec_pretty(&TriggerDocument {
            version: DOCUMENT_VERSION,
            engine_enabled: settings.engine_enabled,
            rules,
        })
        .map_err(|error| format!("Could not serialize trigger rules: {error}"))?;
        fs::write(&self.path, contents).map_err(|error| {
            format!(
                "Could not save trigger rules at {}: {error}",
                self.path.display()
            )
        })
    }
}

fn validate_persisted_rules(rules: &[TriggerRule]) -> Result<(), String> {
    if rules.len() > MAX_TRIGGER_RULES {
        return Err(format!(
            "Trigger rules cannot exceed {MAX_TRIGGER_RULES} entries."
        ));
    }
    let mut keywords = HashSet::with_capacity(rules.len());
    for rule in rules {
        let keyword = normalize_keyword(&rule.keyword).map_err(str::to_owned)?;
        if keyword != rule.keyword || !keywords.insert(rule.match_key()) {
            return Err("Trigger rules contain an invalid or duplicate keyword.".to_owned());
        }
        match &rule.action {
            TriggerAction::Highlight | TriggerAction::Notify => {}
            TriggerAction::PromptResponse { label } => {
                let normalized = normalize_prompt_label(label).map_err(str::to_owned)?;
                if normalized != *label {
                    return Err("Trigger response labels are invalid.".to_owned());
                }
            }
            TriggerAction::SendText { .. } => {
                return Err("Preset text triggers cannot be stored.".to_owned());
            }
        }
    }
    Ok(())
}

impl TriggerRule {
    pub fn match_key(&self) -> String {
        self.keyword.to_lowercase()
    }
}

pub fn normalize_keyword(value: &str) -> Result<String, &'static str> {
    let keyword = value.trim();
    if keyword.is_empty() {
        return Err("Enter a keyword for the trigger.");
    }
    if keyword.chars().count() > MAX_TRIGGER_KEYWORD_LENGTH {
        return Err("Trigger keywords are limited to 128 characters.");
    }
    Ok(keyword.to_owned())
}

pub fn normalize_trigger_text(value: &str) -> Result<String, &'static str> {
    let text = value.trim();
    if text.is_empty() {
        return Err("Enter preset text for the trigger.");
    }
    if text.chars().count() > MAX_TRIGGER_TEXT_LENGTH {
        return Err("Preset trigger text is limited to 1024 characters.");
    }
    Ok(text.to_owned())
}

pub fn normalize_prompt_label(value: &str) -> Result<String, &'static str> {
    let label = value.trim();
    if label.is_empty() {
        return Err("Enter a prompt label for the trigger.");
    }
    if label.chars().count() > MAX_TRIGGER_TEXT_LENGTH {
        return Err("Trigger prompt labels are limited to 1024 characters.");
    }
    Ok(label.to_owned())
}

pub fn frame_text(frame: &TerminalFrame) -> String {
    frame
        .rows
        .iter()
        .map(|row| row.iter().map(|run| run.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn matching_keys(text: &str, rules: &[TriggerRule]) -> HashSet<String> {
    let text = text.to_lowercase();
    rules
        .iter()
        .filter(|rule| rule.enabled)
        .filter_map(|rule| {
            let key = rule.match_key();
            (!key.is_empty() && text.contains(&key)).then_some(key)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(keyword: &str, action: TriggerAction, enabled: bool) -> TriggerRule {
        TriggerRule {
            keyword: keyword.to_owned(),
            action,
            enabled,
        }
    }

    #[test]
    fn keyword_matching_is_literal_case_insensitive_and_respects_enabled_state() {
        let rules = vec![
            rule("ERROR", TriggerAction::Notify, true),
            rule("timeout", TriggerAction::Highlight, false),
            rule("disk full", TriggerAction::Highlight, true),
        ];
        let matches = matching_keys("remote error: disk full", &rules);
        assert!(matches.contains("error"));
        assert!(matches.contains("disk full"));
        assert!(!matches.contains("timeout"));
    }

    #[test]
    fn normalization_rejects_empty_and_overlong_values() {
        assert_eq!(normalize_keyword("  timeout  ").unwrap(), "timeout");
        assert!(normalize_keyword(" ").is_err());
        assert!(normalize_keyword(&"x".repeat(MAX_TRIGGER_KEYWORD_LENGTH + 1)).is_err());
        assert!(normalize_trigger_text(" ").is_err());
        assert!(normalize_trigger_text(&"x".repeat(MAX_TRIGGER_TEXT_LENGTH + 1)).is_err());
        assert_eq!(
            normalize_prompt_label("  sudo password  ").unwrap(),
            "sudo password"
        );
        assert!(normalize_prompt_label(" ").is_err());
        assert!(normalize_prompt_label(&"x".repeat(MAX_TRIGGER_TEXT_LENGTH + 1)).is_err());
    }

    #[test]
    fn send_text_action_keeps_explicit_enter_choice() {
        let action = TriggerAction::SendText {
            text: "systemctl status ssh".to_owned(),
            send_enter: true,
        };
        assert_eq!(action.kind(), TriggerActionKind::SendText);
        assert!(matches!(
            action,
            TriggerAction::SendText {
                text,
                send_enter: true
            } if text == "systemctl status ssh"
        ));
    }

    #[test]
    fn trigger_store_persists_prompt_rules_but_omits_preset_text() {
        let directory = tempfile::tempdir().unwrap();
        let store = TriggerStore::at(directory.path().join("triggers.json"));
        let settings = TriggerSettings {
            engine_enabled: true,
            rules: vec![
                rule("Password:", TriggerAction::Notify, true),
                rule(
                    "sudo password:",
                    TriggerAction::PromptResponse {
                        label: "Enter the sudo password".to_owned(),
                    },
                    false,
                ),
                rule(
                    "second factor",
                    TriggerAction::SendText {
                        text: "secret-must-not-be-saved".to_owned(),
                        send_enter: true,
                    },
                    true,
                ),
            ],
        };

        store.save(&settings).unwrap();
        let serialized = fs::read_to_string(&store.path).unwrap();
        assert!(!serialized.contains("secret-must-not-be-saved"));
        let loaded = store.load().unwrap();
        assert!(loaded.engine_enabled);
        assert_eq!(loaded.rules.len(), 2);
        assert!(matches!(
            &loaded.rules[1].action,
            TriggerAction::PromptResponse { .. }
        ));
    }

    #[test]
    fn missing_trigger_store_uses_disabled_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let store = TriggerStore::at(directory.path().join("missing.json"));

        assert_eq!(store.load().unwrap(), TriggerSettings::default());
    }
}
