use std::{
    collections::{BTreeSet, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

use crate::{
    forward::{ForwardKind, ForwardRuleSpec},
    profiles::{AuthMethod, HostProfile, MAX_FORWARD_RULES},
};

const MAX_INCLUDE_DEPTH: usize = 8;
const MAX_CONFIG_LINES: usize = 20_000;

const SUPPORTED_OPTIONS: &[&str] = &[
    "hostname",
    "user",
    "port",
    "identityfile",
    "localforward",
    "remoteforward",
    "dynamicforward",
    "serveraliveinterval",
];

const REJECTED_OPTIONS: &[&str] = &[
    "match",
    "proxyjump",
    "proxycommand",
    "proxyusefdpass",
    "hostkeyalias",
    "canonicalizehostname",
    "canonicaldomains",
    "canonicalizepermittedcnames",
    "stricthostkeychecking",
    "userknownhostsfile",
    "globalknownhostsfile",
    "updatehostkeys",
    "verifyhostkeydns",
    "pubkeyauthentication",
    "passwordauthentication",
    "kbdinteractiveauthentication",
    "preferredauthentications",
    "identitiesonly",
    "identityagent",
    "certificatefile",
    "securitykeyprovider",
    "forwardagent",
    "forwardx11",
    "forwardx11trusted",
    "remotecommand",
    "permitlocalcommand",
    "localcommand",
    "controlmaster",
    "controlpath",
    "controlpersist",
    "bindinterface",
    "addressfamily",
    "gatewayports",
    "exitonforwardfailure",
    "clearallforwardings",
    "setenv",
    "sendenv",
];

pub struct ImportedConfig {
    pub profiles: Vec<HostProfile>,
    pub warnings: Vec<String>,
}

#[derive(Clone)]
struct ConfigLine {
    keyword: String,
    value: String,
    location: String,
}

struct HostBlock {
    aliases: Vec<String>,
    options: Vec<ConfigLine>,
    location: String,
}

pub fn import_openssh_config(path: &Path) -> Result<ImportedConfig, String> {
    let mut warnings = Vec::new();
    let mut visited = HashSet::new();
    let lines = load_lines(path, 0, &mut visited, &mut warnings)?;
    parse_lines(&lines, warnings)
}

fn load_lines(
    path: &Path,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
    warnings: &mut Vec<String>,
) -> Result<Vec<ConfigLine>, String> {
    if depth > MAX_INCLUDE_DEPTH {
        warnings.push(format!(
            "Stopped expanding includes deeper than {MAX_INCLUDE_DEPTH} levels."
        ));
        return Ok(Vec::new());
    }
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(canonical.clone()) {
        warnings.push(format!("Ignored recursive include of {}.", path.display()));
        return Ok(Vec::new());
    }
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if depth == 0 => {
            return Err(format!("Could not read {}: {error}", path.display()));
        }
        Err(error) => {
            warnings.push(format!(
                "Could not read included file {}: {error}",
                path.display()
            ));
            return Ok(Vec::new());
        }
    };

    let mut lines = Vec::new();
    for (index, raw) in contents.lines().enumerate() {
        if lines.len() >= MAX_CONFIG_LINES {
            warnings.push(format!(
                "Stopped reading {} after {MAX_CONFIG_LINES} effective lines.",
                path.display()
            ));
            break;
        }
        let Some((keyword, value)) = split_keyword(strip_comment(raw)) else {
            continue;
        };
        if keyword.eq_ignore_ascii_case("include") {
            for pattern in split_tokens(&value) {
                if pattern.contains(['*', '?', '[']) {
                    warnings.push(format!(
                        "Ignored glob include {pattern:?} at {}:{}; import it explicitly.",
                        path.display(),
                        index + 1
                    ));
                    continue;
                }
                let included = resolve_include_path(&pattern, path);
                let nested = load_lines(&included, depth + 1, visited, warnings)?;
                lines.extend(nested);
            }
            continue;
        }
        lines.push(ConfigLine {
            keyword: keyword.to_lowercase(),
            value,
            location: format!("{}:{}", path.display(), index + 1),
        });
    }
    Ok(lines)
}

fn parse_lines(lines: &[ConfigLine], mut warnings: Vec<String>) -> Result<ImportedConfig, String> {
    let mut defaults = Vec::new();
    let mut blocks: Vec<HostBlock> = Vec::new();
    let mut current: Option<HostBlock> = None;
    let mut ignored = BTreeSet::new();

    for line in lines {
        match line.keyword.as_str() {
            "host" => {
                if let Some(block) = current.take() {
                    blocks.push(block);
                }
                let aliases = split_tokens(&line.value);
                if aliases.is_empty() {
                    warnings.push(format!("Ignored empty Host at {}.", line.location));
                    current = None;
                    continue;
                }
                current = Some(HostBlock {
                    aliases,
                    options: Vec::new(),
                    location: line.location.clone(),
                });
            }
            "match" => {
                return Err(format!(
                    "Match blocks are not supported; import refused at {}.",
                    line.location
                ));
            }
            _ => {
                if let Some(block) = current.as_mut() {
                    block.options.push(line.clone());
                } else {
                    if REJECTED_OPTIONS.contains(&line.keyword.as_str()) {
                        return Err(format!(
                            "Global option {} at {} is not supported; import refused.",
                            line.keyword, line.location
                        ));
                    }
                    if !SUPPORTED_OPTIONS.contains(&line.keyword.as_str()) {
                        ignored.insert(line.keyword.clone());
                    }
                    defaults.push(line.clone());
                }
            }
        }
    }
    if let Some(block) = current {
        blocks.push(block);
    }
    if !ignored.is_empty() {
        warnings.push(format!(
            "Ignored unsupported options: {}.",
            ignored.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }

    let mut profiles = Vec::new();
    let mut imported_names = HashSet::new();
    for block in blocks {
        let concrete = block
            .aliases
            .iter()
            .filter(|alias| !is_wildcard_pattern(alias))
            .cloned()
            .collect::<Vec<_>>();
        if concrete.is_empty() {
            warnings.push(format!(
                "Skipped wildcard Host block {} at {}; wildcard rules are not applied.",
                block.aliases.join(" "),
                block.location
            ));
            continue;
        }
        if let Some(rejected) = block
            .options
            .iter()
            .find(|option| REJECTED_OPTIONS.contains(&option.keyword.as_str()))
        {
            warnings.push(format!(
                "Skipped {} because option {} at {} is not supported.",
                concrete.join(", "),
                rejected.keyword,
                rejected.location
            ));
            continue;
        }
        if block.aliases.len() != concrete.len() {
            warnings.push(format!(
                "Skipped wildcard aliases in Host block at {}; concrete hosts still imported.",
                block.location
            ));
        }

        for alias in concrete {
            match build_profile(&alias, &defaults, &block.options, &mut warnings) {
                Ok(profile) => {
                    if imported_names.insert(profile.name.to_lowercase()) {
                        profiles.push(profile);
                    } else {
                        warnings.push(format!("Skipped duplicate imported host {}.", profile.name));
                    }
                }
                Err(error) => warnings.push(format!("Skipped {alias}: {error}")),
            }
        }
    }
    Ok(ImportedConfig { profiles, warnings })
}

fn build_profile(
    alias: &str,
    defaults: &[ConfigLine],
    options: &[ConfigLine],
    warnings: &mut Vec<String>,
) -> Result<HostProfile, String> {
    let combined = defaults.iter().chain(options.iter()).collect::<Vec<_>>();
    let username =
        last_value(&combined, "user").ok_or_else(|| "no User is configured".to_owned())?;
    if username.is_empty() {
        return Err("User is empty".to_owned());
    }
    let host = last_value(&combined, "hostname").unwrap_or_else(|| alias.to_owned());
    let port = match last_value(&combined, "port") {
        Some(value) => value
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
            .ok_or_else(|| format!("Port {value:?} is invalid"))?,
        None => 22,
    };

    let identity_files = combined
        .iter()
        .filter(|line| line.keyword == "identityfile")
        .flat_map(|line| split_tokens(&line.value))
        .collect::<Vec<_>>();
    let identity_file = identity_files.first().cloned().unwrap_or_default();
    if identity_files.len() > 1 {
        warnings.push(format!(
            "Host {alias} has multiple IdentityFile entries; only {} is imported.",
            identity_file
        ));
    }

    let keepalive_seconds = match last_value(&combined, "serveraliveinterval") {
        Some(value) => match value.parse::<u64>() {
            Ok(seconds) if seconds == 0 => 0,
            Ok(seconds) if seconds <= 3600 => seconds as u16,
            Ok(seconds) => {
                warnings.push(format!(
                    "Host {alias} ServerAliveInterval {seconds} exceeds one hour; capped at 3600."
                ));
                3600
            }
            Err(_) => {
                warnings.push(format!(
                    "Host {alias} has an invalid ServerAliveInterval; keep-alive disabled."
                ));
                0
            }
        },
        None => 0,
    };

    let mut forwards = Vec::new();
    for line in &combined {
        let kind = match line.keyword.as_str() {
            "localforward" => ForwardKind::Local,
            "remoteforward" => ForwardKind::Remote,
            "dynamicforward" => ForwardKind::Dynamic,
            _ => continue,
        };
        match parse_forward_rule(kind, &line.value) {
            Ok(rule) => forwards.push(rule),
            Err(error) => {
                return Err(format!(
                    "{} at {} could not be imported: {error}",
                    line.keyword, line.location
                ));
            }
        }
    }
    if forwards.len() > MAX_FORWARD_RULES {
        warnings.push(format!(
            "Host {alias} has more than {MAX_FORWARD_RULES} forwarding rules; the rest were dropped."
        ));
        forwards.truncate(MAX_FORWARD_RULES);
    }

    Ok(HostProfile {
        name: alias.to_owned(),
        host,
        port,
        username,
        tab_label: String::new(),
        tab_color: crate::profiles::TabColor::Default,
        terminal_theme: None,
        group: "Imported".to_owned(),
        favorite: false,
        auth_method: if identity_file.is_empty() {
            AuthMethod::Password
        } else {
            AuthMethod::PublicKey
        },
        identity_file,
        proxy_jump: String::new(),
        forwards,
        keepalive_seconds,
        reconnect: crate::profiles::ReconnectSettings::default(),
        login_commands: crate::profiles::LoginCommandSettings::default(),
        password: String::new(),
        passphrase: String::new(),
        jump_password: String::new(),
        jump_passphrase: String::new(),
    })
}

fn parse_forward_rule(kind: ForwardKind, value: &str) -> Result<ForwardRuleSpec, String> {
    let tokens = split_tokens(value);
    let (listen_host, listen_port) = tokens
        .first()
        .ok_or_else(|| "missing listen address".to_owned())
        .and_then(|token| parse_bind_address(token))?;
    if kind == ForwardKind::Dynamic {
        if tokens.len() > 1 {
            return Err("dynamic forwarding accepts only a listen address".to_owned());
        }
        return Ok(ForwardRuleSpec {
            kind,
            listen_host,
            listen_port,
            target_host: String::new(),
            target_port: 0,
        });
    }
    let target = tokens
        .get(1)
        .ok_or_else(|| "missing forwarding target".to_owned())?;
    let (target_host, target_port) = parse_target_address(target)?;
    Ok(ForwardRuleSpec {
        kind,
        listen_host,
        listen_port,
        target_host,
        target_port,
    })
}

fn parse_bind_address(value: &str) -> Result<(String, u16), String> {
    if let Some(rest) = value.strip_prefix('[') {
        let (address, rest) = rest
            .split_once(']')
            .ok_or_else(|| "unterminated IPv6 bind address".to_owned())?;
        let port = rest
            .strip_prefix(':')
            .ok_or_else(|| "missing port after IPv6 bind address".to_owned())?;
        return Ok((normalize_bind_host(address), parse_port(port)?));
    }
    if let Some((address, port)) = value.rsplit_once(':') {
        if port.chars().all(|character| character.is_ascii_digit()) {
            return Ok((normalize_bind_host(address), parse_port(port)?));
        }
    }
    Ok(("127.0.0.1".to_owned(), parse_port(value)?))
}

fn parse_target_address(value: &str) -> Result<(String, u16), String> {
    if let Some(rest) = value.strip_prefix('[') {
        let (host, rest) = rest
            .split_once(']')
            .ok_or_else(|| "unterminated IPv6 target address".to_owned())?;
        let port = rest
            .strip_prefix(':')
            .ok_or_else(|| "missing target port after IPv6 address".to_owned())?;
        return Ok((host.to_owned(), parse_port(port)?));
    }
    let (host, port) = value
        .rsplit_once(':')
        .ok_or_else(|| "target must be host:port".to_owned())?;
    if host.is_empty() {
        return Err("target host is empty".to_owned());
    }
    Ok((host.to_owned(), parse_port(port)?))
}

fn parse_port(value: &str) -> Result<u16, String> {
    value
        .parse::<u16>()
        .map_err(|_| format!("port {value:?} is invalid"))
}

fn normalize_bind_host(value: &str) -> String {
    match value {
        "" | "*" | "0.0.0.0" => "0.0.0.0".to_owned(),
        other => other.to_owned(),
    }
}

fn last_value(options: &[&ConfigLine], keyword: &str) -> Option<String> {
    options
        .iter()
        .rev()
        .find(|line| line.keyword == keyword)
        .map(|line| split_tokens(&line.value).join(" "))
        .filter(|value| !value.is_empty())
}

fn is_wildcard_pattern(value: &str) -> bool {
    value.contains(['*', '?', '[', ']', '!'])
}

fn resolve_include_path(value: &str, source: &Path) -> PathBuf {
    let expanded = if value == "~" {
        home_directory()
    } else if let Some(rest) = value.strip_prefix("~/") {
        home_directory().join(rest)
    } else if let Some(rest) = value.strip_prefix("%d/") {
        home_directory().join(rest)
    } else {
        PathBuf::from(value)
    };
    if expanded.is_absolute() {
        expanded
    } else {
        source
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(expanded)
    }
}

fn home_directory() -> PathBuf {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    for (index, character) in line.char_indices() {
        match character {
            '"' => quoted = !quoted,
            '#' if !quoted => return &line[..index],
            _ => {}
        }
    }
    line
}

fn split_keyword(line: &str) -> Option<(&str, String)> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let split = line.find(|character: char| character.is_whitespace() || character == '=');
    let Some(index) = split else {
        return Some((line, String::new()));
    };
    let keyword = &line[..index];
    let rest = line[index..]
        .trim_start_matches(|character: char| character.is_whitespace() || character == '=');
    if keyword.is_empty() {
        None
    } else {
        Some((keyword, rest.to_owned()))
    }
}

fn split_tokens(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in value.chars() {
        match character {
            '"' => quoted = !quoted,
            character if character.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            character => current.push(character),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_config(directory: &Path, name: &str, contents: &str) -> PathBuf {
        let path = directory.join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn imports_defaults_identity_and_forwards() {
        let directory = tempfile::tempdir().unwrap();
        let path = write_config(
            directory.path(),
            "config",
            r#"
User deploy
ServerAliveInterval 30

Host prod
    HostName prod.example.com
    Port 2222
    IdentityFile ~/.ssh/id_ed25519
    LocalForward 127.0.0.1:15432 db.internal:5432
    DynamicForward 1080

Host staging staging-alt
    HostName staging.example.com
"#,
        );
        let imported = import_openssh_config(&path).unwrap();
        assert_eq!(imported.profiles.len(), 3);
        let prod = imported
            .profiles
            .iter()
            .find(|profile| profile.name == "prod")
            .unwrap();
        assert_eq!(prod.host, "prod.example.com");
        assert_eq!(prod.port, 2222);
        assert_eq!(prod.username, "deploy");
        assert_eq!(prod.auth_method, AuthMethod::PublicKey);
        assert_eq!(prod.identity_file, "~/.ssh/id_ed25519");
        assert_eq!(prod.keepalive_seconds, 30);
        assert_eq!(
            prod.forwards,
            vec![
                ForwardRuleSpec {
                    kind: ForwardKind::Local,
                    listen_host: "127.0.0.1".to_owned(),
                    listen_port: 15432,
                    target_host: "db.internal".to_owned(),
                    target_port: 5432,
                },
                ForwardRuleSpec {
                    kind: ForwardKind::Dynamic,
                    listen_host: "127.0.0.1".to_owned(),
                    listen_port: 1080,
                    target_host: String::new(),
                    target_port: 0,
                },
            ]
        );
        assert!(
            imported
                .profiles
                .iter()
                .any(|profile| profile.name == "staging")
        );
        assert!(
            imported
                .profiles
                .iter()
                .any(|profile| profile.name == "staging-alt")
        );
    }

    #[test]
    fn expands_relative_and_quoted_include_files() {
        let directory = tempfile::tempdir().unwrap();
        write_config(
            directory.path(),
            "extra.conf",
            "Host included\n    HostName included.example.com\n    User ops\n",
        );
        let path = write_config(
            directory.path(),
            "config",
            "Include \"extra.conf\"\nHost direct\n    HostName direct.example.com\n    User me\n",
        );
        let imported = import_openssh_config(&path).unwrap();
        assert_eq!(imported.profiles.len(), 2);
        assert!(
            imported
                .profiles
                .iter()
                .any(|profile| profile.name == "included")
        );
        assert!(
            imported
                .profiles
                .iter()
                .any(|profile| profile.name == "direct")
        );
    }

    #[test]
    fn wildcard_blocks_are_reported_and_skipped() {
        let directory = tempfile::tempdir().unwrap();
        let path = write_config(
            directory.path(),
            "config",
            "Host *\n    User everyone\nHost concrete\n    HostName concrete.example.com\n    User ops\n",
        );
        let imported = import_openssh_config(&path).unwrap();
        assert_eq!(imported.profiles.len(), 1);
        assert_eq!(imported.profiles[0].name, "concrete");
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("wildcard Host block"))
        );
    }

    #[test]
    fn security_relevant_options_skip_the_host() {
        let directory = tempfile::tempdir().unwrap();
        let path = write_config(
            directory.path(),
            "config",
            "Host unsafe\n    HostName unsafe.example.com\n    User root\n    StrictHostKeyChecking no\nHost safe\n    HostName safe.example.com\n    User ops\n",
        );
        let imported = import_openssh_config(&path).unwrap();
        assert_eq!(imported.profiles.len(), 1);
        assert_eq!(imported.profiles[0].name, "safe");
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.to_lowercase().contains("stricthostkeychecking"))
        );
    }

    #[test]
    fn match_blocks_refuse_the_import() {
        let directory = tempfile::tempdir().unwrap();
        let path = write_config(
            directory.path(),
            "config",
            "Host one\n    User ops\nMatch host one\n    User other\n",
        );
        assert!(import_openssh_config(&path).is_err());
    }

    #[test]
    fn keyword_equals_and_inline_comments_are_supported() {
        let directory = tempfile::tempdir().unwrap();
        let path = write_config(
            directory.path(),
            "config",
            "Host=eq\n  HostName=eq.example.com # comment\n  User=ops\n",
        );
        let imported = import_openssh_config(&path).unwrap();
        assert_eq!(imported.profiles.len(), 1);
        assert_eq!(imported.profiles[0].name, "eq");
        assert_eq!(imported.profiles[0].host, "eq.example.com");
    }
}
