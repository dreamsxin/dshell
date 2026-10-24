use std::collections::HashSet;

use crate::terminal::TerminalFrame;

pub const MAX_TRIGGER_RULES: usize = 32;
pub const MAX_TRIGGER_KEYWORD_LENGTH: usize = 128;
pub const MAX_TRIGGER_TEXT_LENGTH: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerActionKind {
    Highlight,
    Notify,
    SendText,
}

impl TriggerActionKind {
    pub const ALL: [Self; 3] = [Self::Highlight, Self::Notify, Self::SendText];

    pub fn label(self) -> &'static str {
        match self {
            Self::Highlight => "Highlight",
            Self::Notify => "Notify",
            Self::SendText => "Send text",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TriggerAction {
    Highlight,
    Notify,
    SendText { text: String, send_enter: bool },
}

impl TriggerAction {
    pub fn kind(&self) -> TriggerActionKind {
        match self {
            Self::Highlight => TriggerActionKind::Highlight,
            Self::Notify => TriggerActionKind::Notify,
            Self::SendText { .. } => TriggerActionKind::SendText,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TriggerRule {
    pub keyword: String,
    pub action: TriggerAction,
    pub enabled: bool,
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
}
