use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutAction {
    NewSession,
    CloseSession,
    ToggleFiles,
    ToggleSnippets,
    ToggleSettings,
    ToggleSidebar,
    ToggleCompose,
    Disconnect,
    ReplayLog,
    Find,
}

impl ShortcutAction {
    pub const ALL: [Self; 10] = [
        Self::NewSession,
        Self::CloseSession,
        Self::ToggleFiles,
        Self::ToggleSnippets,
        Self::ToggleSettings,
        Self::ToggleSidebar,
        Self::ToggleCompose,
        Self::Disconnect,
        Self::ReplayLog,
        Self::Find,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::NewSession => "New session",
            Self::CloseSession => "Close session",
            Self::ToggleFiles => "Toggle files",
            Self::ToggleSnippets => "Toggle snippets",
            Self::ToggleSettings => "Toggle settings",
            Self::ToggleSidebar => "Toggle sidebar",
            Self::ToggleCompose => "Toggle compose",
            Self::Disconnect => "Disconnect",
            Self::ReplayLog => "Replay log",
            Self::Find => "Find",
        }
    }

    pub const fn default_shortcut(self) -> &'static str {
        match self {
            Self::NewSession => "ctrl+shift+t",
            Self::CloseSession => "ctrl+shift+w",
            Self::ToggleFiles => "ctrl+shift+f",
            Self::ToggleSnippets => "ctrl+shift+s",
            Self::ToggleSettings => "ctrl+shift+o",
            Self::ToggleSidebar => "ctrl+shift+b",
            Self::ToggleCompose => "ctrl+shift+m",
            Self::Disconnect => "ctrl+shift+d",
            Self::ReplayLog => "ctrl+shift+r",
            Self::Find => "ctrl+f",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShortcutSettings {
    pub new_session: String,
    pub close_session: String,
    pub toggle_files: String,
    pub toggle_snippets: String,
    pub toggle_settings: String,
    pub toggle_sidebar: String,
    pub toggle_compose: String,
    pub disconnect: String,
    pub replay_log: String,
    pub find: String,
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        let mut settings = Self {
            new_session: String::new(),
            close_session: String::new(),
            toggle_files: String::new(),
            toggle_snippets: String::new(),
            toggle_settings: String::new(),
            toggle_sidebar: String::new(),
            toggle_compose: String::new(),
            disconnect: String::new(),
            replay_log: String::new(),
            find: String::new(),
        };
        for action in ShortcutAction::ALL {
            settings.set(action, action.default_shortcut());
        }
        settings
    }
}

impl ShortcutSettings {
    pub fn shortcut(&self, action: ShortcutAction) -> &str {
        match action {
            ShortcutAction::NewSession => &self.new_session,
            ShortcutAction::CloseSession => &self.close_session,
            ShortcutAction::ToggleFiles => &self.toggle_files,
            ShortcutAction::ToggleSnippets => &self.toggle_snippets,
            ShortcutAction::ToggleSettings => &self.toggle_settings,
            ShortcutAction::ToggleSidebar => &self.toggle_sidebar,
            ShortcutAction::ToggleCompose => &self.toggle_compose,
            ShortcutAction::Disconnect => &self.disconnect,
            ShortcutAction::ReplayLog => &self.replay_log,
            ShortcutAction::Find => &self.find,
        }
    }

    pub fn set(&mut self, action: ShortcutAction, value: impl Into<String>) {
        let value = value.into();
        match action {
            ShortcutAction::NewSession => self.new_session = value,
            ShortcutAction::CloseSession => self.close_session = value,
            ShortcutAction::ToggleFiles => self.toggle_files = value,
            ShortcutAction::ToggleSnippets => self.toggle_snippets = value,
            ShortcutAction::ToggleSettings => self.toggle_settings = value,
            ShortcutAction::ToggleSidebar => self.toggle_sidebar = value,
            ShortcutAction::ToggleCompose => self.toggle_compose = value,
            ShortcutAction::Disconnect => self.disconnect = value,
            ShortcutAction::ReplayLog => self.replay_log = value,
            ShortcutAction::Find => self.find = value,
        }
    }

    pub fn normalized(self) -> Self {
        let mut settings = Self::default();
        for action in ShortcutAction::ALL {
            let value = KeyShortcut::parse(self.shortcut(action)).map_or_else(
                || action.default_shortcut().to_owned(),
                |shortcut| shortcut.canonical(),
            );
            settings.set(action, value);
        }
        settings
    }

    pub fn conflict(&self) -> Option<(ShortcutAction, ShortcutAction)> {
        let actions = ShortcutAction::ALL;
        for (index, left) in actions.iter().enumerate() {
            let Some(left_shortcut) = KeyShortcut::parse(self.shortcut(*left)) else {
                continue;
            };
            for right in &actions[index + 1..] {
                if KeyShortcut::parse(self.shortcut(*right))
                    .is_some_and(|right_shortcut| right_shortcut == left_shortcut)
                {
                    return Some((*left, *right));
                }
            }
        }
        None
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyShortcut {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: String,
}

impl KeyShortcut {
    pub fn parse(value: &str) -> Option<Self> {
        let mut control = false;
        let mut shift = false;
        let mut alt = false;
        let mut key = None;
        for part in value
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
        {
            if key.is_some() {
                return None;
            }
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" | "platform" => control = true,
                "shift" => shift = true,
                "alt" | "option" => alt = true,
                "enter" | "return" | "tab" | "space" | "escape" | "esc" | "backspace"
                | "delete" | "insert" | "home" | "end" | "pageup" | "pagedown" | "up" | "down"
                | "left" | "right" => key = Some(part.to_ascii_lowercase()),
                candidate if candidate.len() == 1 => key = Some(candidate.to_owned()),
                candidate if is_function_key(candidate) => key = Some(candidate.to_owned()),
                _ => return None,
            }
        }
        if !control && !shift && !alt {
            return None;
        }
        Some(Self {
            control,
            shift,
            alt,
            key: key?,
        })
    }

    pub fn matches(&self, key: &str, control: bool, shift: bool, alt: bool) -> bool {
        self.control == control
            && self.shift == shift
            && self.alt == alt
            && self.key.eq_ignore_ascii_case(key)
    }

    pub fn canonical(&self) -> String {
        let mut value = String::new();
        if self.control {
            value.push_str("ctrl+");
        }
        if self.alt {
            value.push_str("alt+");
        }
        if self.shift {
            value.push_str("shift+");
        }
        value.push_str(&self.key);
        value
    }
}

fn is_function_key(value: &str) -> bool {
    value
        .strip_prefix('f')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=24).contains(&number))
}

#[cfg(test)]
mod tests {
    use super::{KeyShortcut, ShortcutAction, ShortcutSettings};

    #[test]
    fn parses_and_canonicalizes_key_shortcuts() {
        let shortcut = KeyShortcut::parse("Shift + CTRL + T").unwrap();
        assert!(shortcut.control);
        assert!(shortcut.shift);
        assert!(!shortcut.alt);
        assert_eq!(shortcut.key, "t");
        assert_eq!(shortcut.canonical(), "ctrl+shift+t");
        assert!(shortcut.matches("t", true, true, false));
        assert!(!shortcut.matches("t", true, false, false));

        assert!(
            KeyShortcut::parse("alt+F12")
                .unwrap()
                .matches("f12", false, false, true)
        );
        assert!(KeyShortcut::parse("cmd+space").is_some());
    }

    #[test]
    fn rejects_incomplete_or_unknown_shortcuts() {
        assert!(KeyShortcut::parse("").is_none());
        assert!(KeyShortcut::parse("ctrl").is_none());
        assert!(KeyShortcut::parse("ctrl+shift+not-a-key").is_none());
        assert!(KeyShortcut::parse("ctrl+a+b").is_none());
        assert!(KeyShortcut::parse("t").is_none());
    }

    #[test]
    fn settings_normalize_invalid_values_and_detect_conflicts() {
        let mut settings = ShortcutSettings::default();
        assert_eq!(settings.conflict(), None);
        settings.set(ShortcutAction::CloseSession, "ctrl+shift+t");
        assert_eq!(
            settings.conflict(),
            Some((ShortcutAction::NewSession, ShortcutAction::CloseSession))
        );
        settings.set(ShortcutAction::CloseSession, "");
        let normalized = settings.clone().normalized();
        assert_eq!(
            normalized.close_session,
            ShortcutAction::CloseSession.default_shortcut()
        );
    }
}
