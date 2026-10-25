use std::{collections::BTreeSet, env, fs, io::ErrorKind, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    shortcuts::ShortcutSettings,
    terminal::{CursorStyle, TerminalTheme},
    ui::{
        i18n::Language,
        theme::UiTheme,
        tokens::{
            INSPECTOR_MAX_WIDTH, INSPECTOR_MIN_WIDTH, INSPECTOR_WIDTH, SIDEBAR_MAX_WIDTH,
            SIDEBAR_MIN_WIDTH, SIDEBAR_WIDTH,
        },
    },
};

const DOCUMENT_VERSION: u8 = 1;
pub const MIN_FONT_SIZE: f32 = 9.0;
pub const MAX_FONT_SIZE: f32 = 32.0;
pub const DEFAULT_FONT_SIZE: f32 = 13.0;
pub const DEFAULT_FONT_FAMILY: &str = "monospace";
pub const DEFAULT_ALERT_CPU_PERCENT: u8 = 80;
pub const DEFAULT_ALERT_MEMORY_PERCENT: u8 = 80;
pub const DEFAULT_ALERT_DISK_PERCENT: u8 = 90;
const MAX_COLLAPSED_GROUPS: usize = 128;
pub const MAX_GROUP_NAME_CHARS: usize = 120;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppSettings {
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default)]
    pub theme: TerminalTheme,
    #[serde(default)]
    pub cursor_style: CursorStyle,
    #[serde(default)]
    pub shortcuts: ShortcutSettings,
    #[serde(default)]
    pub ui_theme: UiTheme,
    #[serde(default)]
    pub language: Language,
    #[serde(default)]
    pub sidebar_collapsed: bool,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: f32,
    #[serde(default = "default_inspector_width")]
    pub inspector_width: f32,
    #[serde(default)]
    pub collapsed_groups: Vec<String>,
    #[serde(default)]
    pub group_order: Vec<String>,
    #[serde(default)]
    pub group_names: Vec<String>,
    #[serde(default = "default_alert_cpu_percent")]
    pub alert_cpu_percent: u8,
    #[serde(default = "default_alert_memory_percent")]
    pub alert_memory_percent: u8,
    #[serde(default = "default_alert_disk_percent")]
    pub alert_disk_percent: u8,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            font_size: DEFAULT_FONT_SIZE,
            font_family: DEFAULT_FONT_FAMILY.to_owned(),
            theme: TerminalTheme::Default,
            cursor_style: CursorStyle::Block,
            shortcuts: ShortcutSettings::default(),
            ui_theme: UiTheme::DEFAULT,
            language: Language::default(),
            sidebar_collapsed: false,
            sidebar_width: SIDEBAR_WIDTH,
            inspector_width: INSPECTOR_WIDTH,
            collapsed_groups: Vec::new(),
            group_order: Vec::new(),
            group_names: Vec::new(),
            alert_cpu_percent: DEFAULT_ALERT_CPU_PERCENT,
            alert_memory_percent: DEFAULT_ALERT_MEMORY_PERCENT,
            alert_disk_percent: DEFAULT_ALERT_DISK_PERCENT,
        }
    }
}

impl AppSettings {
    pub fn normalized(self) -> Self {
        let font_family = self.font_family.trim();
        Self {
            font_size: self.font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE),
            font_family: if font_family.is_empty() {
                DEFAULT_FONT_FAMILY.to_owned()
            } else {
                font_family.chars().take(120).collect()
            },
            theme: self.theme,
            cursor_style: self.cursor_style,
            shortcuts: self.shortcuts.normalized(),
            ui_theme: self.ui_theme,
            language: self.language,
            sidebar_collapsed: self.sidebar_collapsed,
            sidebar_width: normalize_width(
                self.sidebar_width,
                SIDEBAR_WIDTH,
                SIDEBAR_MIN_WIDTH,
                SIDEBAR_MAX_WIDTH,
            ),
            inspector_width: normalize_width(
                self.inspector_width,
                INSPECTOR_WIDTH,
                INSPECTOR_MIN_WIDTH,
                INSPECTOR_MAX_WIDTH,
            ),
            collapsed_groups: normalize_collapsed_groups(self.collapsed_groups),
            group_order: normalize_group_order(self.group_order),
            group_names: normalize_group_names(self.group_names),
            alert_cpu_percent: normalize_alert_percent(
                self.alert_cpu_percent,
                DEFAULT_ALERT_CPU_PERCENT,
            ),
            alert_memory_percent: normalize_alert_percent(
                self.alert_memory_percent,
                DEFAULT_ALERT_MEMORY_PERCENT,
            ),
            alert_disk_percent: normalize_alert_percent(
                self.alert_disk_percent,
                DEFAULT_ALERT_DISK_PERCENT,
            ),
        }
    }

    pub fn cell_width(&self) -> f32 {
        self.font_size * (8.0 / DEFAULT_FONT_SIZE)
    }

    pub fn cell_height(&self) -> f32 {
        self.font_size * (18.0 / DEFAULT_FONT_SIZE)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SettingsDocument {
    version: u8,
    #[serde(default)]
    font_size: f32,
    #[serde(default = "default_font_family")]
    font_family: String,
    #[serde(default)]
    theme: TerminalTheme,
    #[serde(default)]
    cursor_style: CursorStyle,
    #[serde(default)]
    shortcuts: ShortcutSettings,
    #[serde(default)]
    ui_theme: UiTheme,
    #[serde(default)]
    language: Language,
    #[serde(default)]
    sidebar_collapsed: bool,
    #[serde(default = "default_sidebar_width")]
    sidebar_width: f32,
    #[serde(default = "default_inspector_width")]
    inspector_width: f32,
    #[serde(default)]
    collapsed_groups: Vec<String>,
    #[serde(default)]
    group_order: Vec<String>,
    #[serde(default)]
    group_names: Vec<String>,
    #[serde(default = "default_alert_cpu_percent")]
    alert_cpu_percent: u8,
    #[serde(default = "default_alert_memory_percent")]
    alert_memory_percent: u8,
    #[serde(default = "default_alert_disk_percent")]
    alert_disk_percent: u8,
}

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
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
            path: config_root.join("dshell").join("settings.json"),
        })
    }

    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<AppSettings, String> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(AppSettings::default());
            }
            Err(error) => {
                return Err(format!(
                    "Could not read settings at {}: {error}",
                    self.path.display()
                ));
            }
        };
        let document: SettingsDocument = serde_json::from_str(&contents).map_err(|error| {
            format!(
                "Could not parse settings at {}: {error}",
                self.path.display()
            )
        })?;
        if document.version != DOCUMENT_VERSION {
            return Err(format!(
                "Settings file version {} is not supported.",
                document.version
            ));
        }
        Ok(AppSettings {
            font_size: document.font_size,
            font_family: document.font_family,
            theme: document.theme,
            cursor_style: document.cursor_style,
            shortcuts: document.shortcuts,
            ui_theme: document.ui_theme,
            language: document.language,
            sidebar_collapsed: document.sidebar_collapsed,
            sidebar_width: document.sidebar_width,
            inspector_width: document.inspector_width,
            collapsed_groups: document.collapsed_groups,
            group_order: document.group_order,
            group_names: document.group_names,
            alert_cpu_percent: document.alert_cpu_percent,
            alert_memory_percent: document.alert_memory_percent,
            alert_disk_percent: document.alert_disk_percent,
        }
        .normalized())
    }

    pub fn save(&self, settings: &AppSettings) -> Result<(), String> {
        let settings = settings.clone().normalized();
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "The settings path has no parent directory.".to_owned())?;
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Could not create settings directory {}: {error}",
                parent.display()
            )
        })?;
        let contents = serde_json::to_vec_pretty(&SettingsDocument {
            version: DOCUMENT_VERSION,
            font_size: settings.font_size,
            font_family: settings.font_family,
            theme: settings.theme,
            cursor_style: settings.cursor_style,
            shortcuts: settings.shortcuts.clone(),
            ui_theme: settings.ui_theme,
            language: settings.language,
            sidebar_collapsed: settings.sidebar_collapsed,
            sidebar_width: settings.sidebar_width,
            inspector_width: settings.inspector_width,
            collapsed_groups: settings.collapsed_groups,
            group_order: settings.group_order,
            group_names: settings.group_names,
            alert_cpu_percent: settings.alert_cpu_percent,
            alert_memory_percent: settings.alert_memory_percent,
            alert_disk_percent: settings.alert_disk_percent,
        })
        .map_err(|error| format!("Could not serialize settings: {error}"))?;
        fs::write(&self.path, contents).map_err(|error| {
            format!(
                "Could not save settings at {}: {error}",
                self.path.display()
            )
        })
    }
}

fn default_font_size() -> f32 {
    DEFAULT_FONT_SIZE
}

fn default_alert_cpu_percent() -> u8 {
    DEFAULT_ALERT_CPU_PERCENT
}

fn default_alert_memory_percent() -> u8 {
    DEFAULT_ALERT_MEMORY_PERCENT
}

fn default_alert_disk_percent() -> u8 {
    DEFAULT_ALERT_DISK_PERCENT
}

fn normalize_alert_percent(value: u8, default: u8) -> u8 {
    if value == 0 {
        default
    } else {
        value.clamp(1, 100)
    }
}

fn default_font_family() -> String {
    DEFAULT_FONT_FAMILY.to_owned()
}

fn default_sidebar_width() -> f32 {
    SIDEBAR_WIDTH
}

fn default_inspector_width() -> f32 {
    INSPECTOR_WIDTH
}

fn normalize_width(value: f32, default: f32, minimum: f32, maximum: f32) -> f32 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        default
    }
}

fn normalize_collapsed_groups(groups: Vec<String>) -> Vec<String> {
    let mut normalized = BTreeSet::new();
    for group in groups {
        let key = group.trim().to_lowercase();
        if key.chars().count() <= MAX_GROUP_NAME_CHARS {
            normalized.insert(key);
        } else {
            normalized.insert(key.chars().take(MAX_GROUP_NAME_CHARS).collect());
        }
    }
    normalized.into_iter().take(MAX_COLLAPSED_GROUPS).collect()
}

fn normalize_group_order(groups: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::new();
    for group in groups {
        let key = group.trim().to_lowercase();
        let key = if key.chars().count() <= MAX_GROUP_NAME_CHARS {
            key
        } else {
            key.chars().take(MAX_GROUP_NAME_CHARS).collect()
        };
        if seen.insert(key.clone()) {
            normalized.push(key);
            if normalized.len() == MAX_COLLAPSED_GROUPS {
                break;
            }
        }
    }
    normalized
}

fn normalize_group_names(groups: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::new();
    for group in groups {
        let name = group.trim();
        if name.is_empty() {
            continue;
        }
        let name = name.chars().take(MAX_GROUP_NAME_CHARS).collect::<String>();
        let key = name.to_lowercase();
        if seen.insert(key) {
            normalized.push(name);
            if normalized.len() == MAX_COLLAPSED_GROUPS {
                break;
            }
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::ShortcutAction;

    #[test]
    fn missing_settings_file_uses_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let store = SettingsStore::at(directory.path().join("settings.json"));
        assert_eq!(store.load().unwrap(), AppSettings::default());
    }

    #[test]
    fn settings_round_trip_and_clamp_invalid_values() {
        let directory = tempfile::tempdir().unwrap();
        let store = SettingsStore::at(directory.path().join("settings.json"));
        let mut shortcuts = ShortcutSettings::default();
        shortcuts.set(ShortcutAction::ReplayLog, "alt+f9");
        let settings = AppSettings {
            font_size: 18.0,
            font_family: "Cascadia Mono".to_owned(),
            theme: TerminalTheme::SolarizedLight,
            cursor_style: CursorStyle::Bar,
            shortcuts,
            ui_theme: UiTheme::Light,
            language: Language::SimplifiedChinese,
            sidebar_collapsed: true,
            sidebar_width: 312.0,
            inspector_width: 460.0,
            collapsed_groups: vec!["Production".to_owned(), "production".to_owned()],
            group_order: vec![" Gamma ".to_owned(), "alpha".to_owned(), "gamma".to_owned()],
            group_names: vec![
                " Production ".to_owned(),
                "production".to_owned(),
                "QA".to_owned(),
            ],
            alert_cpu_percent: 75,
            alert_memory_percent: 82,
            alert_disk_percent: 91,
        };
        store.save(&settings).unwrap();
        assert_eq!(
            store.load().unwrap().collapsed_groups,
            vec!["production".to_owned()]
        );
        assert_eq!(
            store.load().unwrap().group_order,
            vec!["gamma".to_owned(), "alpha".to_owned()]
        );
        assert_eq!(
            store.load().unwrap().group_names,
            vec!["Production".to_owned(), "QA".to_owned()]
        );
        let loaded = store.load().unwrap();
        assert_eq!(loaded.alert_cpu_percent, 75);
        assert_eq!(loaded.alert_memory_percent, 82);
        assert_eq!(loaded.alert_disk_percent, 91);

        store
            .save(&AppSettings {
                font_size: 200.0,
                font_family: "  ".to_owned(),
                theme: TerminalTheme::Default,
                cursor_style: CursorStyle::Underline,
                shortcuts: ShortcutSettings::default(),
                ui_theme: UiTheme::Dark,
                language: Language::English,
                sidebar_collapsed: false,
                sidebar_width: 10.0,
                inspector_width: 10_000.0,
                collapsed_groups: Vec::new(),
                group_order: Vec::new(),
                group_names: Vec::new(),
                alert_cpu_percent: 0,
                alert_memory_percent: 101,
                alert_disk_percent: 0,
            })
            .unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.font_size, MAX_FONT_SIZE);
        assert_eq!(loaded.font_family, DEFAULT_FONT_FAMILY);
        assert_eq!(loaded.sidebar_width, SIDEBAR_MIN_WIDTH);
        assert_eq!(loaded.inspector_width, INSPECTOR_MAX_WIDTH);
        assert_eq!(loaded.alert_cpu_percent, DEFAULT_ALERT_CPU_PERCENT);
        assert_eq!(loaded.alert_memory_percent, 100);
        assert_eq!(loaded.alert_disk_percent, DEFAULT_ALERT_DISK_PERCENT);
    }

    #[test]
    fn version_one_files_without_ui_preferences_use_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"version":1,"font_size":14,"font_family":"monospace"}"#,
        )
        .unwrap();
        let loaded = SettingsStore::at(path).load().unwrap();
        assert_eq!(loaded.font_size, 14.0);
        assert_eq!(loaded.ui_theme, UiTheme::Dark);
        assert_eq!(loaded.language, Language::English);
        assert!(!loaded.sidebar_collapsed);
        assert_eq!(loaded.sidebar_width, SIDEBAR_WIDTH);
        assert_eq!(loaded.inspector_width, INSPECTOR_WIDTH);
        assert_eq!(loaded.shortcuts, ShortcutSettings::default());
        assert!(loaded.collapsed_groups.is_empty());
        assert!(loaded.group_order.is_empty());
        assert!(loaded.group_names.is_empty());
        assert_eq!(loaded.alert_cpu_percent, DEFAULT_ALERT_CPU_PERCENT);
        assert_eq!(loaded.alert_memory_percent, DEFAULT_ALERT_MEMORY_PERCENT);
        assert_eq!(loaded.alert_disk_percent, DEFAULT_ALERT_DISK_PERCENT);
    }

    #[test]
    fn cell_metrics_scale_with_the_font_size() {
        let settings = AppSettings {
            font_size: 26.0,
            font_family: "monospace".to_owned(),
            theme: TerminalTheme::Default,
            cursor_style: CursorStyle::Block,
            shortcuts: ShortcutSettings::default(),
            ui_theme: UiTheme::Dark,
            language: Language::English,
            sidebar_collapsed: false,
            sidebar_width: SIDEBAR_WIDTH,
            inspector_width: INSPECTOR_WIDTH,
            collapsed_groups: Vec::new(),
            group_order: Vec::new(),
            group_names: Vec::new(),
            alert_cpu_percent: DEFAULT_ALERT_CPU_PERCENT,
            alert_memory_percent: DEFAULT_ALERT_MEMORY_PERCENT,
            alert_disk_percent: DEFAULT_ALERT_DISK_PERCENT,
        };
        assert!((settings.cell_width() - 16.0).abs() < 0.001);
        assert!((settings.cell_height() - 36.0).abs() < 0.001);
    }
}
