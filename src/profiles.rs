use std::{
    collections::HashSet,
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Deserializer, Serialize, de::IgnoredAny};

use crate::forward::ForwardRuleSpec;
use crate::groups::normalize_group_path;

const DOCUMENT_VERSION: u8 = 7;
const SUPPORTED_DOCUMENT_VERSIONS: [u8; 7] = [1, 2, 3, 4, 5, 6, 7];
const EXPORT_DOCUMENT_VERSION: u8 = 3;
const SUPPORTED_EXPORT_DOCUMENT_VERSIONS: [u8; 3] = [1, 2, EXPORT_DOCUMENT_VERSION];
pub const MAX_RECENT_CONNECTIONS: usize = 20;
pub const MAX_FORWARD_RULES: usize = 32;
pub const MAX_TAB_LABEL_CHARS: usize = 80;
const MAX_RECENT_DIRECTORY_BYTES: usize = 4096;
pub const MIN_RECONNECT_INTERVAL_SECONDS: u16 = 1;
pub const MAX_RECONNECT_INTERVAL_SECONDS: u16 = 300;
pub const MAX_RECONNECT_ATTEMPTS: u8 = 20;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    #[default]
    Password,
    PublicKey,
    KeyboardInteractive,
    Agent,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TabColor {
    #[default]
    Default,
    Blue,
    Green,
    Amber,
    Red,
    Cyan,
    Teal,
    Pink,
    Violet,
}

impl TabColor {
    pub const ALL: [Self; 9] = [
        Self::Default,
        Self::Blue,
        Self::Green,
        Self::Amber,
        Self::Red,
        Self::Cyan,
        Self::Teal,
        Self::Pink,
        Self::Violet,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Blue => "Blue",
            Self::Green => "Green",
            Self::Amber => "Amber",
            Self::Red => "Red",
            Self::Cyan => "Cyan",
            Self::Teal => "Teal",
            Self::Pink => "Pink",
            Self::Violet => "Violet",
        }
    }

    const fn is_default(value: &Self) -> bool {
        matches!(value, Self::Default)
    }
}

impl AuthMethod {
    pub const ALL: [Self; 4] = [
        Self::Password,
        Self::PublicKey,
        Self::KeyboardInteractive,
        Self::Agent,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Password => "Password",
            Self::PublicKey => "Public key",
            Self::KeyboardInteractive => "Keyboard interactive",
            Self::Agent => "SSH agent",
        }
    }

    pub const fn uses_password_input(self) -> bool {
        matches!(self, Self::Password | Self::KeyboardInteractive)
    }

    fn is_password(&self) -> bool {
        matches!(self, Self::Password)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReconnectSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_reconnect_interval")]
    pub interval_seconds: u16,
    #[serde(default = "default_reconnect_attempts")]
    pub max_attempts: u8,
}

impl Default for ReconnectSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_seconds: default_reconnect_interval(),
            max_attempts: default_reconnect_attempts(),
        }
    }
}

impl ReconnectSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(MIN_RECONNECT_INTERVAL_SECONDS..=MAX_RECONNECT_INTERVAL_SECONDS)
            .contains(&self.interval_seconds)
        {
            return Err(format!(
                "Reconnect interval must be between {MIN_RECONNECT_INTERVAL_SECONDS} and {MAX_RECONNECT_INTERVAL_SECONDS} seconds."
            ));
        }
        if !(1..=MAX_RECONNECT_ATTEMPTS).contains(&self.max_attempts) {
            return Err(format!(
                "Reconnect attempts must be between 1 and {MAX_RECONNECT_ATTEMPTS}."
            ));
        }
        Ok(())
    }
}

const fn default_reconnect_interval() -> u16 {
    5
}

const fn default_reconnect_attempts() -> u8 {
    5
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostProfile {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tab_label: String,
    #[serde(default, skip_serializing_if = "TabColor::is_default")]
    pub tab_color: TabColor,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub favorite: bool,
    #[serde(default, skip_serializing_if = "AuthMethod::is_password")]
    pub auth_method: AuthMethod,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub identity_file: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proxy_jump: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forwards: Vec<ForwardRuleSpec>,
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub keepalive_seconds: u16,
    #[serde(default)]
    pub reconnect: ReconnectSettings,
    #[serde(
        default,
        skip_serializing,
        deserialize_with = "ignore_legacy_credential"
    )]
    pub password: String,
    #[serde(
        default,
        skip_serializing,
        deserialize_with = "ignore_legacy_credential"
    )]
    pub passphrase: String,
    #[serde(
        default,
        skip_serializing,
        deserialize_with = "ignore_legacy_credential"
    )]
    pub jump_password: String,
    #[serde(
        default,
        skip_serializing,
        deserialize_with = "ignore_legacy_credential"
    )]
    pub jump_passphrase: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecentConnection {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub last_used_unix: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_local_directory: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_remote_directory: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HostData {
    pub profiles: Vec<HostProfile>,
    pub recent: Vec<RecentConnection>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProfileDocument {
    version: u8,
    profiles: Vec<HostProfile>,
    #[serde(default)]
    recent: Vec<RecentConnection>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProfileExportDocument {
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

    pub fn load(&self) -> Result<HostData, String> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(HostData::default()),
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
        if !SUPPORTED_DOCUMENT_VERSIONS.contains(&document.version) {
            return Err(format!(
                "Host profile file version {} is not supported.",
                document.version
            ));
        }
        let requires_migration = document.version < DOCUMENT_VERSION;
        validate_profiles(&document.profiles)?;
        validate_recent(&document.recent)?;
        let data = HostData {
            profiles: document.profiles,
            recent: document.recent,
        };
        if requires_migration {
            self.save(&data)?;
        }
        Ok(data)
    }

    pub fn save(&self, data: &HostData) -> Result<(), String> {
        validate_profiles(&data.profiles)?;
        validate_recent(&data.recent)?;
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
            profiles: data.profiles.clone(),
            recent: data.recent.clone(),
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

/// Export host profiles without recent-connection history or any credentials.
pub fn export_profiles(path: &Path, profiles: &[HostProfile]) -> Result<(), String> {
    validate_profiles(profiles)?;
    let parent = path
        .parent()
        .ok_or_else(|| "The host profile export path has no parent directory.".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "Could not create host profile export directory {}: {error}",
            parent.display()
        )
    })?;
    let profiles = profiles
        .iter()
        .cloned()
        .map(|mut profile| {
            profile.password.clear();
            profile.passphrase.clear();
            profile.jump_password.clear();
            profile.jump_passphrase.clear();
            profile
        })
        .collect();
    let contents = serde_json::to_vec_pretty(&ProfileExportDocument {
        version: EXPORT_DOCUMENT_VERSION,
        profiles,
    })
    .map_err(|error| format!("Could not serialize host profile export: {error}"))?;
    fs::write(path, contents).map_err(|error| {
        format!(
            "Could not write host profile export at {}: {error}",
            path.display()
        )
    })
}

/// Import host profiles from the versioned, credential-free export format.
pub fn import_profiles(path: &Path) -> Result<Vec<HostProfile>, String> {
    let contents = fs::read_to_string(path).map_err(|error| {
        format!(
            "Could not read host profile export at {}: {error}",
            path.display()
        )
    })?;
    let document: ProfileExportDocument = serde_json::from_str(&contents).map_err(|error| {
        format!(
            "Could not parse host profile export at {}: {error}",
            path.display()
        )
    })?;
    if !SUPPORTED_EXPORT_DOCUMENT_VERSIONS.contains(&document.version) {
        return Err(format!(
            "Host profile export version {} is not supported.",
            document.version
        ));
    }
    validate_profiles(&document.profiles)?;
    Ok(document.profiles)
}

pub fn record_recent(recent: &mut Vec<RecentConnection>, entry: RecentConnection) {
    recent.retain(|candidate| {
        !(candidate.host.eq_ignore_ascii_case(&entry.host)
            && candidate.port == entry.port
            && candidate.username == entry.username)
    });
    recent.insert(0, entry);
    recent.truncate(MAX_RECENT_CONNECTIONS);
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn ignore_legacy_credential<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    IgnoredAny::deserialize(deserializer)?;
    Ok(String::new())
}

fn is_zero_u16(value: &u16) -> bool {
    *value == 0
}

fn validate_forward_rules(profile: &HostProfile) -> Result<(), String> {
    if profile.forwards.len() > MAX_FORWARD_RULES {
        return Err(format!(
            "Host profile {} cannot have more than {MAX_FORWARD_RULES} forwarding rules.",
            profile.name
        ));
    }
    if profile.keepalive_seconds > 3600 {
        return Err(format!(
            "Host profile {} has a keep-alive interval above one hour.",
            profile.name
        ));
    }
    for rule in &profile.forwards {
        if rule.listen_host.trim().is_empty() {
            return Err(format!(
                "Host profile {} has a forwarding rule without a listen address.",
                profile.name
            ));
        }
        if rule.kind.needs_target() && (rule.target_host.trim().is_empty() || rule.target_port == 0)
        {
            return Err(format!(
                "Host profile {} has a forwarding rule without a valid target.",
                profile.name
            ));
        }
    }
    Ok(())
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
        if profile.tab_label.chars().count() > MAX_TAB_LABEL_CHARS
            || profile.tab_label.chars().any(char::is_control)
        {
            return Err(format!(
                "Host profile {} has an invalid tab label.",
                profile.name
            ));
        }
        if !names.insert(profile.name.to_lowercase()) {
            return Err(format!(
                "Host profile names must be unique: {}",
                profile.name
            ));
        }
        if profile.group.contains('/') {
            normalize_group_path(&profile.group).map_err(|error| {
                format!(
                    "Host profile {} has an invalid group path: {error}",
                    profile.name
                )
            })?;
        }
        if profile.auth_method == AuthMethod::PublicKey && profile.identity_file.trim().is_empty() {
            return Err(format!(
                "Host profile {} uses public key authentication but has no identity file.",
                profile.name
            ));
        }
        profile
            .reconnect
            .validate()
            .map_err(|error| format!("Host profile {}: {error}", profile.name))?;
        validate_forward_rules(profile)?;
    }
    Ok(())
}

fn validate_recent(recent: &[RecentConnection]) -> Result<(), String> {
    if recent.len() > MAX_RECENT_CONNECTIONS {
        return Err(format!(
            "Recent connections cannot exceed {MAX_RECENT_CONNECTIONS} entries."
        ));
    }
    if recent.iter().any(|entry| {
        entry.host.trim().is_empty() || entry.username.trim().is_empty() || entry.port == 0
    }) {
        return Err("Recent connections require a host, username, and valid port.".to_owned());
    }
    if recent.iter().any(|entry| {
        [&entry.last_local_directory, &entry.last_remote_directory]
            .into_iter()
            .any(|path| {
                path.len() > MAX_RECENT_DIRECTORY_BYTES || path.chars().any(char::is_control)
            })
    }) {
        return Err("Recent SFTP directories are invalid or too long.".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forward::{ForwardKind, ForwardRuleSpec};

    fn profile(name: &str) -> HostProfile {
        HostProfile {
            name: name.to_owned(),
            host: "server.example.com".to_owned(),
            port: 2222,
            username: "operator".to_owned(),
            tab_label: String::new(),
            tab_color: TabColor::Default,
            group: String::new(),
            favorite: false,
            auth_method: AuthMethod::Password,
            identity_file: String::new(),
            proxy_jump: String::new(),
            forwards: Vec::new(),
            keepalive_seconds: 0,
            reconnect: ReconnectSettings::default(),
            password: String::new(),
            passphrase: String::new(),
            jump_password: String::new(),
            jump_passphrase: String::new(),
        }
    }

    fn recent(host: &str, port: u16) -> RecentConnection {
        RecentConnection {
            name: "recent".to_owned(),
            host: host.to_owned(),
            port,
            username: "operator".to_owned(),
            last_used_unix: 1_700_000_000,
            last_local_directory: String::new(),
            last_remote_directory: String::new(),
        }
    }

    #[test]
    fn legacy_profile_credentials_are_ignored_and_removed_from_disk() {
        let directory = tempfile::tempdir().unwrap();
        let store = ProfileStore::at(directory.path().join("hosts.json"));
        let mut legacy_profile = serde_json::to_value(HostProfile {
            group: "Production".to_owned(),
            favorite: true,
            proxy_jump: "bastion".to_owned(),
            forwards: vec![ForwardRuleSpec {
                kind: ForwardKind::Local,
                listen_host: "127.0.0.1".to_owned(),
                listen_port: 15432,
                target_host: "db.internal".to_owned(),
                target_port: 5432,
            }],
            keepalive_seconds: 30,
            ..profile("staging")
        })
        .unwrap();
        legacy_profile["password"] = serde_json::json!("secret");
        legacy_profile["passphrase"] = serde_json::json!("key-secret");
        legacy_profile["jump_password"] = serde_json::json!("jump-secret");
        legacy_profile["jump_passphrase"] = serde_json::json!("jump-key-secret");
        let legacy_document = serde_json::json!({
            "version": DOCUMENT_VERSION - 1,
            "profiles": [legacy_profile],
            "recent": [recent("server.example.com", 2222)],
        });
        fs::write(
            &store.path,
            serde_json::to_vec_pretty(&legacy_document).unwrap(),
        )
        .unwrap();

        let data = store.load().unwrap();
        let loaded = &data.profiles[0];
        assert_eq!(loaded.group, "Production");
        assert_eq!(loaded.proxy_jump, "bastion");
        assert_eq!(loaded.forwards.len(), 1);
        assert!(loaded.password.is_empty());
        assert!(loaded.passphrase.is_empty());
        assert!(loaded.jump_password.is_empty());
        assert!(loaded.jump_passphrase.is_empty());

        let contents = fs::read_to_string(&store.path).unwrap();
        for secret in ["secret", "key-secret", "jump-secret", "jump-key-secret"] {
            assert!(!contents.contains(secret));
        }
        let saved: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(saved["version"].as_u64(), Some(u64::from(DOCUMENT_VERSION)));
        let saved_profile = saved["profiles"][0].as_object().unwrap();
        for field in ["password", "passphrase", "jump_password", "jump_passphrase"] {
            assert!(!saved_profile.contains_key(field));
        }
    }

    #[test]
    fn profile_exports_round_trip_without_recent_history_or_credentials() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("export.json");
        let profiles = vec![HostProfile {
            tab_label: "API west".to_owned(),
            tab_color: TabColor::Teal,
            group: "Production".to_owned(),
            favorite: true,
            proxy_jump: "bastion".to_owned(),
            forwards: vec![ForwardRuleSpec {
                kind: ForwardKind::Local,
                listen_host: "127.0.0.1".to_owned(),
                listen_port: 15432,
                target_host: "db.internal".to_owned(),
                target_port: 5432,
            }],
            keepalive_seconds: 30,
            password: "secret".to_owned(),
            passphrase: "key-secret".to_owned(),
            jump_password: "jump-secret".to_owned(),
            jump_passphrase: "jump-key-secret".to_owned(),
            ..profile("staging")
        }];

        export_profiles(&path, &profiles).unwrap();
        let mut expected = profiles.clone();
        expected[0].password.clear();
        expected[0].passphrase.clear();
        expected[0].jump_password.clear();
        expected[0].jump_passphrase.clear();
        assert_eq!(import_profiles(&path).unwrap(), expected);
        let contents = fs::read_to_string(path).unwrap();
        assert!(!contents.contains("recent"));
        assert!(!contents.to_lowercase().contains("password"));
        assert!(!contents.to_lowercase().contains("passphrase"));
        assert!(!contents.to_lowercase().contains("private_key"));
        assert!(!contents.contains("secret"));
    }

    #[test]
    fn profile_exports_reject_unknown_versions_and_duplicate_names() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("export.json");
        fs::write(
            &path,
            r#"{
                "version": 99,
                "profiles": []
            }"#,
        )
        .unwrap();
        assert!(import_profiles(&path).is_err());

        let duplicate = HostProfile {
            name: "STAGING".to_owned(),
            ..profile("staging")
        };
        assert!(export_profiles(&path, &[profile("staging"), duplicate]).is_err());
    }

    #[test]
    fn missing_profile_file_is_an_empty_list() {
        let directory = tempfile::tempdir().unwrap();
        let store = ProfileStore::at(directory.path().join("missing.json"));

        assert_eq!(store.load().unwrap(), HostData::default());
    }

    #[test]
    fn version_one_files_load_with_workspace_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hosts.json");
        fs::write(
            &path,
            r#"{
                "version": 1,
                "profiles": [
                    {
                        "name": "prod",
                        "host": "prod.example.com",
                        "port": 22,
                        "username": "deploy"
                    }
                ]
            }"#,
        )
        .unwrap();
        let store = ProfileStore::at(path);

        let data = store.load().unwrap();
        assert_eq!(
            data.profiles,
            vec![HostProfile {
                name: "prod".to_owned(),
                host: "prod.example.com".to_owned(),
                port: 22,
                username: "deploy".to_owned(),
                tab_label: String::new(),
                tab_color: TabColor::Default,
                group: String::new(),
                favorite: false,
                auth_method: AuthMethod::Password,
                identity_file: String::new(),
                proxy_jump: String::new(),
                forwards: Vec::new(),
                keepalive_seconds: 0,
                reconnect: ReconnectSettings::default(),
                password: String::new(),
                passphrase: String::new(),
                jump_password: String::new(),
                jump_passphrase: String::new(),
            }]
        );
        assert!(data.recent.is_empty());
    }

    #[test]
    fn version_four_recent_connections_migrate_with_empty_sftp_directories() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hosts.json");
        fs::write(
            &path,
            r#"{
                "version": 4,
                "profiles": [],
                "recent": [{
                    "name": "prod",
                    "host": "prod.example.com",
                    "port": 22,
                    "username": "deploy",
                    "last_used_unix": 1700000000
                }]
            }"#,
        )
        .unwrap();
        let store = ProfileStore::at(path);

        let data = store.load().unwrap();
        assert!(data.recent[0].last_local_directory.is_empty());
        assert!(data.recent[0].last_remote_directory.is_empty());
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&store.path).unwrap()).unwrap();
        assert_eq!(saved["version"].as_u64(), Some(u64::from(DOCUMENT_VERSION)));
    }

    #[test]
    fn recent_sftp_directories_round_trip_and_validate() {
        let directory = tempfile::tempdir().unwrap();
        let store = ProfileStore::at(directory.path().join("hosts.json"));
        let mut entry = recent("prod.example.com", 22);
        entry.last_local_directory = r"C:\Users\operator\Downloads".to_owned();
        entry.last_remote_directory = "/srv/releases".to_owned();
        let data = HostData {
            profiles: Vec::new(),
            recent: vec![entry.clone()],
        };

        store.save(&data).unwrap();
        assert_eq!(store.load().unwrap().recent, vec![entry.clone()]);

        entry.last_remote_directory.push('\n');
        assert!(validate_recent(&[entry]).is_err());
        let oversized = RecentConnection {
            last_local_directory: "x".repeat(MAX_RECENT_DIRECTORY_BYTES + 1),
            ..recent("prod.example.com", 22)
        };
        assert!(validate_recent(&[oversized]).is_err());
    }

    #[test]
    fn version_five_profiles_migrate_with_auto_reconnect_disabled() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hosts.json");
        fs::write(
            &path,
            r#"{
                "version": 5,
                "profiles": [{
                    "name": "prod",
                    "host": "prod.example.com",
                    "port": 22,
                    "username": "deploy"
                }]
            }"#,
        )
        .unwrap();
        let store = ProfileStore::at(path);

        let data = store.load().unwrap();
        assert_eq!(data.profiles[0].reconnect, ReconnectSettings::default());
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&store.path).unwrap()).unwrap();
        assert_eq!(saved["version"].as_u64(), Some(u64::from(DOCUMENT_VERSION)));
    }

    #[test]
    fn version_six_profiles_migrate_with_default_tab_appearance() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hosts.json");
        fs::write(
            &path,
            r#"{
                "version": 6,
                "profiles": [{
                    "name": "prod",
                    "host": "prod.example.com",
                    "port": 22,
                    "username": "deploy"
                }]
            }"#,
        )
        .unwrap();
        let store = ProfileStore::at(path);

        let data = store.load().unwrap();
        assert!(data.profiles[0].tab_label.is_empty());
        assert_eq!(data.profiles[0].tab_color, TabColor::Default);
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&store.path).unwrap()).unwrap();
        assert_eq!(saved["version"].as_u64(), Some(u64::from(DOCUMENT_VERSION)));
    }

    #[test]
    fn tab_labels_are_bounded_and_reject_control_characters() {
        let mut host = profile("prod");
        host.tab_label = "x".repeat(MAX_TAB_LABEL_CHARS + 1);
        assert!(validate_profiles(&[host.clone()]).is_err());

        host.tab_label = "production\nblue".to_owned();
        assert!(validate_profiles(&[host]).is_err());
    }

    #[test]
    fn reconnect_settings_enforce_interval_and_attempt_bounds() {
        let mut settings = ReconnectSettings::default();
        settings.enabled = true;
        assert!(settings.validate().is_ok());

        settings.interval_seconds = MIN_RECONNECT_INTERVAL_SECONDS - 1;
        assert!(settings.validate().is_err());
        settings.interval_seconds = MAX_RECONNECT_INTERVAL_SECONDS + 1;
        assert!(settings.validate().is_err());
        settings.interval_seconds = default_reconnect_interval();
        settings.max_attempts = MAX_RECONNECT_ATTEMPTS + 1;
        assert!(settings.validate().is_err());
    }

    #[test]
    fn duplicate_profile_names_are_rejected() {
        let profile = profile("prod");
        let duplicate = HostProfile {
            name: "PROD".to_owned(),
            host: "two.example.com".to_owned(),
            ..profile.clone()
        };

        assert!(validate_profiles(&[profile, duplicate]).is_err());
    }

    #[test]
    fn host_profiles_accept_nested_groups_and_reject_invalid_paths() {
        let mut profile = profile("prod");
        profile.group = "Production/Databases/Primary".to_owned();
        assert!(validate_profiles(std::slice::from_ref(&profile)).is_ok());

        profile.group = "Production//Primary".to_owned();
        assert!(validate_profiles(&[profile]).is_err());
    }

    #[test]
    fn public_key_profiles_require_an_identity_file() {
        let missing_identity = HostProfile {
            auth_method: AuthMethod::PublicKey,
            ..profile("key-host")
        };
        assert!(validate_profiles(&[missing_identity]).is_err());

        let with_identity = HostProfile {
            identity_file: "~/.ssh/id_ed25519".to_owned(),
            auth_method: AuthMethod::PublicKey,
            ..profile("key-host")
        };
        assert!(validate_profiles(&[with_identity]).is_ok());
    }

    #[test]
    fn forwarding_rules_are_validated() {
        let invalid = HostProfile {
            forwards: vec![ForwardRuleSpec {
                kind: ForwardKind::Local,
                listen_host: String::new(),
                listen_port: 0,
                target_host: "db.internal".to_owned(),
                target_port: 5432,
            }],
            ..profile("bad-forward")
        };
        assert!(validate_profiles(&[invalid]).is_err());

        let dynamic = HostProfile {
            forwards: vec![ForwardRuleSpec {
                kind: ForwardKind::Dynamic,
                listen_host: "127.0.0.1".to_owned(),
                listen_port: 0,
                target_host: String::new(),
                target_port: 0,
            }],
            keepalive_seconds: 30,
            ..profile("socks")
        };
        assert!(validate_profiles(&[dynamic]).is_ok());
    }

    #[test]
    fn recent_connections_are_deduplicated_and_capped() {
        let mut entries = Vec::new();
        record_recent(&mut entries, recent("one.example.com", 22));
        record_recent(&mut entries, recent("two.example.com", 22));
        record_recent(&mut entries, recent("ONE.example.com", 22));

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].host, "ONE.example.com");
        assert_eq!(entries[1].host, "two.example.com");

        for index in 0..MAX_RECENT_CONNECTIONS + 5 {
            record_recent(
                &mut entries,
                recent(&format!("host{index}.example.com"), 22),
            );
        }
        assert_eq!(entries.len(), MAX_RECENT_CONNECTIONS);
    }
}
