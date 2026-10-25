mod auth_profiles;
mod forward;
mod keygen;
mod log;
mod openssh_config;
mod profiles;
mod settings;
mod sftp;
mod shortcuts;
mod snippets;
mod ssh;
mod terminal;
mod triggers;
mod ui;

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use auth_profiles::{AuthProfile, AuthProfileStore};
use forward::{ForwardCommand, ForwardEvent, ForwardKind, ForwardRuleSpec, ForwardRuleUiState};
use gpui::{AnyElement, StatefulInteractiveElement as _, prelude::*, *};
use gpui_component::{
    Disableable as _, Icon, IconName, PixelsExt, Root, Sizable as _, Size, StyledExt,
    button::{Button, ButtonVariants},
    input::{InputState, TextInput},
    resizable::{ResizableState, h_resizable, resizable_panel, v_resizable},
};
use openssh_config::import_openssh_config;
use profiles::{
    AuthMethod, HostData, HostProfile, MAX_FORWARD_RULES, ProfileStore, RecentConnection,
    export_profiles as export_host_profiles, import_profiles as import_host_profiles,
    record_recent,
};
use settings::{
    AppSettings, DEFAULT_FONT_FAMILY, MAX_FONT_SIZE, MAX_GROUP_NAME_CHARS, MIN_FONT_SIZE,
    SettingsStore,
};
use sftp::{
    RemoteEntry, RemoteFilesystemInfo, SftpCommand, SftpEvent, TransferKind,
    format_permission_mode, parent_remote_path, parse_permission_mode, parse_unix_timestamp,
    remote_join,
};
use shortcuts::{KeyShortcut, ShortcutAction, ShortcutSettings};
use snippets::{CommandSnippet, MAX_SNIPPETS, SnippetStore};
use ssh::{
    AuthPromptInfo, Authentication, ConnectionInfo, ConnectionSpec, JumpSpec, ServerData,
    ServerDataError, ServerListener, ServerMetrics, ServerMount, ServerNetworkInterface,
    ServerPowerAction, ServerProcess, ServerService, ServerServiceLogs, ServiceAction, UiEvent,
};
use terminal::{
    CursorStyle, MouseTrackingMode, SearchDirection, TerminalEmulator, TerminalFrame,
    TerminalKeyInput, TerminalMouseButton, TerminalMouseEvent, TerminalMouseModifiers,
    TerminalPalette, TerminalPosition, TerminalRequest, TerminalRun, TerminalSearchResult,
    TerminalSize, TerminalTheme,
};
use tokio::sync::{mpsc, oneshot, watch};
use triggers::{
    MAX_TRIGGER_RULES, TriggerAction, TriggerActionKind, TriggerRule, matching_keys,
    normalize_keyword, normalize_trigger_text,
};
use ui::{
    assets::AppAssets,
    components::{self, ButtonKind, ButtonSize, Tone},
    i18n::{self, Language},
    theme::{ThemeTokens, UiTheme},
    tokens as ui_tokens,
};
use zeroize::Zeroizing;

const TERMINAL_PANEL_HEADER_PX: f32 = ui_tokens::PANEL_HEADER_HEIGHT;
const TERMINAL_CARD_BORDER_PX: f32 = 2.0;
const LAYOUT_DIVIDER_PX: f32 = 4.0;
const MAX_AUTH_PROMPTS: usize = 8;
const MAX_LOG_REPLAY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_KEYWORD_HIGHLIGHTS: usize = 32;
const MAX_SERVER_ALERT_HISTORY: usize = 32;

fn sidebar_width(settings: &AppSettings, collapsed: bool) -> f32 {
    if collapsed {
        ui_tokens::SIDEBAR_COLLAPSED_WIDTH
    } else {
        settings.sidebar_width
    }
}

fn terminal_size_for_viewport(
    width: f32,
    height: f32,
    settings: &AppSettings,
    sidebar_collapsed: bool,
    inspector_visible: bool,
) -> TerminalSize {
    let inspector = if inspector_visible {
        settings.inspector_width + ui_tokens::SPACE_2 + LAYOUT_DIVIDER_PX
    } else {
        0.0
    };
    let sidebar_divider = if sidebar_collapsed {
        0.0
    } else {
        LAYOUT_DIVIDER_PX
    };
    let width = width
        - sidebar_width(settings, sidebar_collapsed)
        - sidebar_divider
        - ui_tokens::SPACE_4
        - TERMINAL_CARD_BORDER_PX
        - inspector;
    let height = height
        - ui_tokens::COMMAND_BAR_HEIGHT
        - ui_tokens::TAB_BAR_HEIGHT
        - ui_tokens::STATUS_BAR_HEIGHT
        - ui_tokens::SPACE_4
        - TERMINAL_PANEL_HEADER_PX
        - TERMINAL_CARD_BORDER_PX;
    TerminalSize::for_window_with(width, height, settings.cell_width(), settings.cell_height())
}

#[derive(Clone, Debug)]
struct ToastState {
    message: String,
    tone: Tone,
}

#[derive(Clone, Debug)]
struct TabDragData {
    session_id: u64,
    title: SharedString,
}

#[derive(Clone, Debug)]
enum SidebarDragData {
    Host { name: String, title: SharedString },
    Group { key: String, title: SharedString },
}

struct DragPreview {
    title: SharedString,
    tokens: ThemeTokens,
}

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(ui_tokens::SPACE_3))
            .py(px(ui_tokens::SPACE_1))
            .rounded(px(ui_tokens::RADIUS_CONTROL))
            .border_1()
            .border_color(self.tokens.border_strong)
            .bg(self.tokens.surface_overlay)
            .text_size(px(ui_tokens::FONT_BODY))
            .text_color(self.tokens.text_primary)
            .child(self.title.clone())
    }
}

fn reorder_session_ids(ids: &mut Vec<u64>, dragged: u64, target: u64) -> bool {
    if dragged == target {
        return false;
    }
    let Some(from) = ids.iter().position(|id| *id == dragged) else {
        return false;
    };
    let Some(original_target) = ids.iter().position(|id| *id == target) else {
        return false;
    };
    let moving_right = from < original_target;
    let session = ids.remove(from);
    let Some(to) = ids.iter().position(|id| *id == target) else {
        ids.insert(from, session);
        return false;
    };
    let insert_at = if moving_right { to + 1 } else { to };
    ids.insert(insert_at.min(ids.len()), session);
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayoutDragTarget {
    Sidebar,
    Inspector,
}

#[derive(Clone, Copy, Debug)]
struct LayoutDrag {
    target: LayoutDragTarget,
    start_x: f32,
    start_width: f32,
}

fn dragged_panel_width(target: LayoutDragTarget, start_width: f32, delta: f32) -> f32 {
    match target {
        LayoutDragTarget::Sidebar => {
            (start_width + delta).clamp(ui_tokens::SIDEBAR_MIN_WIDTH, ui_tokens::SIDEBAR_MAX_WIDTH)
        }
        LayoutDragTarget::Inspector => (start_width - delta).clamp(
            ui_tokens::INSPECTOR_MIN_WIDTH,
            ui_tokens::INSPECTOR_MAX_WIDTH,
        ),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionState {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
}

fn session_tone(state: SessionState) -> Tone {
    match state {
        SessionState::Connected => Tone::Success,
        SessionState::Connecting | SessionState::Disconnecting => Tone::Warning,
        SessionState::Disconnected => Tone::Neutral,
    }
}

fn session_state_label(state: SessionState) -> &'static str {
    match state {
        SessionState::Disconnected => "Disconnected",
        SessionState::Connecting => "Connecting",
        SessionState::Connected => "Connected",
        SessionState::Disconnecting => "Disconnecting",
    }
}

struct AuthPromptState {
    session_id: u64,
    name: String,
    instructions: String,
    prompts: Vec<AuthPromptInfo>,
    respond_to: Option<oneshot::Sender<Vec<Zeroizing<String>>>>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CredentialCacheKey {
    host: String,
    port: u16,
    username: String,
    auth_method: AuthMethod,
    identity_file: String,
    jump_profile: String,
}

impl CredentialCacheKey {
    fn from_profile(profile: &HostProfile) -> Self {
        Self {
            host: profile.host.trim().to_owned(),
            port: profile.port,
            username: profile.username.trim().to_owned(),
            auth_method: profile.auth_method,
            identity_file: profile.identity_file.trim().to_owned(),
            jump_profile: profile.proxy_jump.trim().to_owned(),
        }
    }

    fn from_connection(
        host: &str,
        port: u16,
        username: &str,
        auth_method: AuthMethod,
        identity_file: &str,
        jump_profile: &str,
    ) -> Self {
        Self {
            host: host.trim().to_owned(),
            port,
            username: username.trim().to_owned(),
            auth_method,
            identity_file: identity_file.trim().to_owned(),
            jump_profile: jump_profile.trim().to_owned(),
        }
    }
}

#[derive(Clone)]
struct CachedCredentials {
    password: Zeroizing<String>,
    passphrase: Zeroizing<String>,
    jump_password: Zeroizing<String>,
    jump_passphrase: Zeroizing<String>,
}

impl CachedCredentials {
    fn from_inputs(
        password: &Zeroizing<String>,
        passphrase: &Zeroizing<String>,
        jump_password: &Zeroizing<String>,
        jump_passphrase: &Zeroizing<String>,
    ) -> Self {
        Self {
            password: Zeroizing::new(password.as_str().to_owned()),
            passphrase: Zeroizing::new(passphrase.as_str().to_owned()),
            jump_password: Zeroizing::new(jump_password.as_str().to_owned()),
            jump_passphrase: Zeroizing::new(jump_passphrase.as_str().to_owned()),
        }
    }

    fn contains_secret(&self) -> bool {
        !self.password.is_empty()
            || !self.passphrase.is_empty()
            || !self.jump_password.is_empty()
            || !self.jump_passphrase.is_empty()
    }
}

struct LogReplayState {
    path: PathBuf,
    terminal: TerminalEmulator,
    frame: TerminalFrame,
    message: String,
    scroll_remainder: f32,
    replay_size: TerminalSize,
    bytes: Vec<u8>,
    timeline: Vec<log::LogEntry>,
    timeline_index: Option<usize>,
    timestamped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchTarget {
    Session(u64),
    LogReplay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SplitOrientation {
    SideBySide,
    Stacked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayoutPreset {
    Focus,
    Standard,
    Inspector,
    Split,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionEditorSection {
    Connection,
    Authentication,
    Security,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupEditorMode {
    Create,
    Rename,
    Delete,
}

impl SessionEditorSection {
    const ALL: [Self; 3] = [Self::Connection, Self::Authentication, Self::Security];

    const fn label(self) -> &'static str {
        match self {
            Self::Connection => "Connection",
            Self::Authentication => "Authentication",
            Self::Security => "Security",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SessionSplit {
    first_session_id: u64,
    second_session_id: u64,
    orientation: SplitOrientation,
}

impl SessionSplit {
    fn contains(self, session_id: u64) -> bool {
        self.first_session_id == session_id || self.second_session_id == session_id
    }
}

fn split_panel_terminal_size(
    full_size: TerminalSize,
    orientation: SplitOrientation,
    panel_extent: Option<f32>,
    panel_index: usize,
    cell_width: f32,
    cell_height: f32,
) -> TerminalSize {
    match orientation {
        SplitOrientation::SideBySide => {
            let columns = panel_extent.map_or_else(
                || {
                    if panel_index == 0 {
                        full_size.columns.saturating_add(1) / 2
                    } else {
                        (full_size.columns / 2).max(1)
                    }
                },
                |width| {
                    ((width - TERMINAL_CARD_BORDER_PX).max(0.0) / cell_width)
                        .floor()
                        .clamp(1.0, 240.0) as u16
                },
            );
            TerminalSize {
                columns,
                rows: full_size.rows,
            }
        }
        SplitOrientation::Stacked => {
            let rows = panel_extent.map_or_else(
                || {
                    if panel_index == 0 {
                        full_size.rows.saturating_add(1) / 2
                    } else {
                        (full_size.rows / 2).max(1)
                    }
                },
                |height| {
                    ((height - TERMINAL_PANEL_HEADER_PX - TERMINAL_CARD_BORDER_PX).max(0.0)
                        / cell_height)
                        .floor()
                        .clamp(1.0, 120.0) as u16
                },
            );
            TerminalSize {
                columns: full_size.columns,
                rows,
            }
        }
    }
}

fn session_terminal_size_for_split(
    session_id: u64,
    full_size: TerminalSize,
    split_sizes: Option<(SessionSplit, TerminalSize, TerminalSize)>,
) -> TerminalSize {
    match split_sizes {
        Some((split, first, _)) if session_id == split.first_session_id => first,
        Some((split, _, second)) if session_id == split.second_session_id => second,
        _ => full_size,
    }
}

fn move_profile_to_group(profiles: &mut [HostProfile], name: &str, group: &str) -> bool {
    let Some(profile) = profiles
        .iter_mut()
        .find(|profile| profile.name.eq_ignore_ascii_case(name))
    else {
        return false;
    };
    if profile.group == group {
        return false;
    }
    profile.group = group.to_owned();
    sort_profiles(profiles);
    true
}

fn rename_profile_group(profiles: &mut [HostProfile], old_group: &str, new_group: &str) -> usize {
    let old_key = collapsed_group_key(old_group);
    let mut renamed = 0;
    for profile in &mut *profiles {
        if collapsed_group_key(&profile.group) == old_key {
            profile.group = new_group.to_owned();
            renamed += 1;
        }
    }
    if renamed > 0 {
        sort_profiles(profiles);
    }
    renamed
}

fn remove_profile_group(profiles: &mut [HostProfile], group: &str) -> usize {
    let group_key = collapsed_group_key(group);
    let mut removed = 0;
    for profile in &mut *profiles {
        if collapsed_group_key(&profile.group) == group_key {
            profile.group.clear();
            removed += 1;
        }
    }
    if removed > 0 {
        sort_profiles(profiles);
    }
    removed
}

fn reorder_group_order(order: &mut Vec<String>, dragged: &str, target: &str) -> bool {
    let dragged = collapsed_group_key(dragged);
    let target = collapsed_group_key(target);
    if dragged == target {
        return false;
    }
    if !order.iter().any(|entry| entry == &dragged) {
        order.push(dragged.clone());
    }
    if !order.iter().any(|entry| entry == &target) {
        order.push(target.clone());
    }
    let Some(index) = order.iter().position(|entry| entry == &dragged) else {
        return false;
    };
    let value = order.remove(index);
    let Some(target_index) = order.iter().position(|entry| entry == &target) else {
        return false;
    };
    order.insert(target_index, value);
    true
}

fn collapsed_group_key(group: &str) -> String {
    group.trim().to_lowercase()
}

fn sort_profiles(profiles: &mut [HostProfile]) {
    profiles.sort_by(|left, right| {
        left.group
            .to_lowercase()
            .cmp(&right.group.to_lowercase())
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
}

fn merge_unique_profiles(
    profiles: &mut Vec<HostProfile>,
    imported: impl IntoIterator<Item = HostProfile>,
) -> (usize, usize) {
    let mut names = profiles
        .iter()
        .map(|profile| profile.name.to_lowercase())
        .collect::<HashSet<_>>();
    let mut added = 0;
    let mut skipped = 0;
    for profile in imported {
        if names.insert(profile.name.to_lowercase()) {
            profiles.push(profile);
            added += 1;
        } else {
            skipped += 1;
        }
    }
    if added > 0 {
        sort_profiles(profiles);
    }
    (added, skipped)
}

fn build_authentication(
    method: AuthMethod,
    identity_file: &str,
    password: Zeroizing<String>,
    passphrase: Zeroizing<String>,
) -> Result<Authentication, String> {
    match method {
        AuthMethod::Password => {
            if password.is_empty() {
                return Err("A password is required for password authentication.".to_owned());
            }
            Ok(Authentication::Password(password))
        }
        AuthMethod::KeyboardInteractive => {
            if password.is_empty() {
                return Err(
                    "A password is required for keyboard-interactive authentication.".to_owned(),
                );
            }
            Ok(Authentication::KeyboardInteractive {
                password,
                interactive: true,
            })
        }
        AuthMethod::PublicKey => {
            if identity_file.trim().is_empty() {
                return Err(
                    "An identity file path is required for public key authentication.".to_owned(),
                );
            }
            Ok(Authentication::PublicKey {
                identity_file: PathBuf::from(identity_file),
                passphrase,
            })
        }
        AuthMethod::Agent => Ok(Authentication::Agent),
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn home_directory() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn expand_local_path(value: &str) -> PathBuf {
    if value == "~" {
        return home_directory();
    }
    if let Some(rest) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        return home_directory().join(rest);
    }
    PathBuf::from(value)
}

fn valid_remote_entry_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\\')
}

fn format_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{bytes:.0} B")
    }
}

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds / 3_600) % 24;
    let minutes = (seconds / 60) % 60;
    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

fn format_kib(kib: Option<u64>) -> String {
    kib.map_or_else(
        || "Unavailable".to_owned(),
        |value| format_size(value.saturating_mul(1024)),
    )
}

fn format_storage_kib(kib: u64) -> String {
    format_size(kib.saturating_mul(1024))
}

fn memory_usage_percent(total_kib: Option<u64>, available_kib: Option<u64>) -> Option<u8> {
    let (total, available) = (total_kib?, available_kib?);
    if total == 0 {
        return Some(0);
    }
    Some(
        (total
            .saturating_sub(available.min(total))
            .saturating_mul(100)
            / total)
            .min(100) as u8,
    )
}

fn server_alert_observations(
    metrics: Option<&ServerMetrics>,
    processes: &[ServerProcess],
    cpu_threshold: u8,
    memory_threshold: u8,
    disk_threshold: u8,
) -> Vec<ServerAlertObservation> {
    let mut alerts = Vec::new();
    if let Some(max_cpu) = processes
        .iter()
        .map(|process| process.cpu_percent)
        .filter(|value| value.is_finite())
        .reduce(f32::max)
        && max_cpu >= f32::from(cpu_threshold)
    {
        alerts.push(ServerAlertObservation {
            kind: ServerAlertKind::Cpu,
            value: format!("{max_cpu:.1}%"),
            threshold: cpu_threshold,
        });
    }
    if let Some(memory_percent) = metrics.and_then(|metrics| {
        memory_usage_percent(metrics.memory_total_kib, metrics.memory_available_kib)
    }) && memory_percent >= memory_threshold
    {
        alerts.push(ServerAlertObservation {
            kind: ServerAlertKind::Memory,
            value: format!("{memory_percent}%"),
            threshold: memory_threshold,
        });
    }
    if let Some(disk_percent) = metrics.and_then(|metrics| metrics.disk_use_percent)
        && disk_percent >= disk_threshold
    {
        alerts.push(ServerAlertObservation {
            kind: ServerAlertKind::Disk,
            value: format!("{disk_percent}%"),
            threshold: disk_threshold,
        });
    }
    alerts
}

fn format_alert_time(seconds: u64) -> String {
    let days = seconds / 86_400;
    let within_day = seconds % 86_400;
    let hours = within_day / 3_600;
    let minutes = (within_day / 60) % 60;
    let seconds = within_day % 60;
    format!("{days}d {hours:02}:{minutes:02}:{seconds:02} UTC")
}

fn parse_alert_percent(value: &str) -> Option<u8> {
    value
        .trim()
        .parse::<u8>()
        .ok()
        .filter(|value| (1..=100).contains(value))
}

fn format_unix_timestamp(value: Option<u32>) -> String {
    value.map_or_else(String::new, |value| value.to_string())
}

fn filesystem_usage_percent(total_bytes: u64, available_bytes: u64) -> u8 {
    if total_bytes == 0 {
        return 0;
    }
    let used_bytes = total_bytes.saturating_sub(available_bytes.min(total_bytes));
    ((used_bytes as f64 * 100.0 / total_bytes as f64).round() as u8).min(100)
}

fn format_rate(bytes_per_second: f64) -> String {
    if !bytes_per_second.is_finite() || bytes_per_second <= 0.5 {
        "0 B/s".to_owned()
    } else {
        format!("{}/s", format_size(bytes_per_second.round() as u64))
    }
}

fn remember_compose_history(history: &mut Vec<String>, text: String) {
    history.retain(|item| item != &text);
    history.insert(0, text);
    history.truncate(20);
}

fn recall_compose_history(history: &[String], recall: usize) -> Option<(String, usize)> {
    if history.is_empty() {
        return None;
    }
    let index = recall % history.len();
    Some((history[index].clone(), (index + 1) % history.len()))
}

fn synchronized_targets(
    active_session_id: u64,
    enabled: bool,
    selected: &HashSet<u64>,
    connected: impl Fn(u64) -> bool,
) -> Vec<u64> {
    let mut targets = vec![active_session_id];
    if enabled {
        let mut mirrored = selected
            .iter()
            .copied()
            .filter(|id| *id != active_session_id && connected(*id))
            .collect::<Vec<_>>();
        mirrored.sort_unstable();
        targets.extend(mirrored);
    }
    targets
}

fn menu_item(
    tokens: &ThemeTokens,
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    shortcut: &'static str,
    active: bool,
    enabled: bool,
) -> Button {
    let button = Button::new(id)
        .h(px(32.0))
        .w_full()
        .px(px(ui_tokens::SPACE_2))
        .rounded(px(ui_tokens::RADIUS_CONTROL))
        .disabled(!enabled)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(ui_tokens::SPACE_2))
                .w_full()
                .child(
                    Icon::new(icon)
                        .with_size(Size::Small)
                        .text_color(tokens.text_secondary),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_size(px(ui_tokens::FONT_BODY))
                        .text_color(tokens.text_primary)
                        .child(label.into()),
                )
                .when(!shortcut.is_empty(), |row| {
                    row.child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(ui_tokens::FONT_CAPTION))
                            .text_color(tokens.text_disabled)
                            .child(shortcut),
                    )
                }),
        );
    if active { button } else { button.ghost() }
}

fn layout_preset_item(
    tokens: &ThemeTokens,
    id: impl Into<ElementId>,
    icon: IconName,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    selected: bool,
    enabled: bool,
) -> Button {
    Button::new(id)
        .w_full()
        .h(px(ui_tokens::LAYOUT_PRESET_HEIGHT))
        .p(px(ui_tokens::SPACE_2))
        .rounded(px(ui_tokens::RADIUS_CARD))
        .border_1()
        .border_color(if selected {
            tokens.accent
        } else {
            tokens.border_subtle
        })
        .bg(if selected {
            tokens.surface_overlay
        } else {
            tokens.surface
        })
        .disabled(!enabled)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(ui_tokens::SPACE_2))
                .w_full()
                .child(
                    Icon::new(icon)
                        .with_size(Size::Medium)
                        .text_color(if selected {
                            tokens.accent
                        } else {
                            tokens.text_secondary
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(ui_tokens::SPACE_1))
                        .min_w_0()
                        .items_start()
                        .child(
                            div()
                                .text_size(px(ui_tokens::FONT_BODY))
                                .font_semibold()
                                .text_color(tokens.text_primary)
                                .child(title.into()),
                        )
                        .child(
                            div()
                                .text_size(px(ui_tokens::FONT_CAPTION))
                                .text_color(tokens.text_secondary)
                                .child(description.into()),
                        ),
                ),
        )
}

fn format_log_time(milliseconds: u64) -> String {
    let hours = milliseconds / 3_600_000;
    let minutes = (milliseconds / 60_000) % 60;
    let seconds = (milliseconds / 1_000) % 60;
    let millis = milliseconds % 1_000;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}.{millis:03}")
    } else {
        format!("{minutes:02}:{seconds:02}.{millis:03}")
    }
}

struct SshSession {
    id: u64,
    title: String,
    host: String,
    port: u16,
    username: String,
    status: String,
    frame: Option<TerminalFrame>,
    terminal_message: String,
    terminal_size: TerminalSize,
    scroll_remainder: f32,
    mouse_position: Option<TerminalPosition>,
    command_tx: Option<mpsc::Sender<TerminalRequest>>,
    cancel_tx: Option<watch::Sender<bool>>,
    sftp_tx: Option<mpsc::Sender<SftpCommand>>,
    sftp_cancel: Option<Arc<AtomicBool>>,
    sftp: SftpViewState,
    forward_tx: Option<mpsc::Sender<ForwardCommand>>,
    forwards: ForwardViewState,
    profile_name: Option<String>,
    pending_forwards: Vec<ForwardRuleSpec>,
    log_path: Option<PathBuf>,
    connection_info: Option<ConnectionInfo>,
    server_metrics: ServerMetricsViewState,
    credential_key: Option<CredentialCacheKey>,
    state: SessionState,
}

#[derive(Clone, Debug, Default)]
struct ServerMetricsViewState {
    metrics: Option<ServerMetrics>,
    loading: bool,
    message: String,
    power_action_loading: bool,
    power_action_message: String,
    processes: Vec<ServerProcess>,
    processes_loading: bool,
    processes_message: String,
    services: Vec<ServerService>,
    services_loading: bool,
    services_message: String,
    network: Vec<ServerNetworkInterface>,
    network_loading: bool,
    network_message: String,
    storage: Vec<ServerMount>,
    storage_loading: bool,
    storage_message: String,
    listeners: Vec<ServerListener>,
    listeners_loading: bool,
    listeners_message: String,
    service_logs: Option<ServerServiceLogs>,
    service_logs_loading: bool,
    service_logs_message: String,
    alert_history: Vec<ServerAlertRecord>,
    active_alerts: Vec<ServerAlertObservation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServerAlertKind {
    Cpu,
    Memory,
    Disk,
}

impl ServerAlertKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU alert",
            Self::Memory => "Memory alert",
            Self::Disk => "Disk alert",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ServerAlertObservation {
    kind: ServerAlertKind,
    value: String,
    threshold: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ServerAlertRecord {
    kind: ServerAlertKind,
    value: String,
    threshold: u8,
    observed_at: u64,
}

#[derive(Clone, Debug)]
struct TransferProgressUi {
    kind: TransferKind,
    name: String,
    transferred: u64,
    total: u64,
    last_transferred: u64,
    last_sample: Instant,
    rate_bytes_per_second: f64,
}

#[derive(Clone, Debug)]
struct OverwritePrompt {
    kind: TransferKind,
    local: PathBuf,
    remote: String,
    is_directory: bool,
}

#[derive(Clone, Debug)]
struct SftpMetadataEdit {
    path: String,
    name: String,
}

#[derive(Default)]
struct SftpViewState {
    visible: bool,
    path: String,
    entries: Vec<RemoteEntry>,
    message: String,
    transfer: Option<TransferProgressUi>,
    active_transfer: Option<SftpCommand>,
    transfer_queue: VecDeque<SftpCommand>,
    overwrite: Option<OverwritePrompt>,
    rename_from: Option<String>,
    pending_delete: Option<(String, bool)>,
    metadata_edit: Option<SftpMetadataEdit>,
    filesystem: Option<RemoteFilesystemInfo>,
    filesystem_supported: Option<bool>,
}

#[derive(Default)]
struct ForwardViewState {
    visible: bool,
    rules: Vec<(u64, ForwardRuleUiState)>,
    stopped: Vec<ForwardRuleSpec>,
    message: String,
}

fn remember_stopped_forward(state: &mut ForwardViewState, spec: ForwardRuleSpec) {
    if !state.stopped.contains(&spec) {
        state.stopped.push(spec);
    }
}

fn forget_stopped_forward(state: &mut ForwardViewState, spec: &ForwardRuleSpec) {
    state.stopped.retain(|stopped| stopped != spec);
}

impl SshSession {
    fn clear_terminal(&mut self, status: impl Into<String>, message: impl Into<String>) {
        self.status = status.into();
        self.frame = None;
        self.terminal_message = message.into();
        self.mouse_position = None;
        self.scroll_remainder = 0.0;
    }

    fn apply_event(&mut self, event: UiEvent) -> bool {
        if self.state == SessionState::Disconnecting
            && matches!(&event, UiEvent::Status(_) | UiEvent::Screen(_))
        {
            return false;
        }
        match event {
            UiEvent::Status(status) => {
                self.status = status;
                self.state = SessionState::Connected;
                let pending = std::mem::take(&mut self.pending_forwards);
                if let Some(forward_tx) = &self.forward_tx {
                    for rule in pending {
                        let _ = forward_tx.try_send(ForwardCommand::Add(rule));
                    }
                }
            }
            UiEvent::LogState(path) => {
                self.log_path = path;
            }
            UiEvent::ConnectionInfo(info) => {
                self.connection_info = Some(info);
            }
            UiEvent::ServerMetrics(ServerData::Metrics(metrics)) => {
                self.server_metrics.metrics = Some(metrics);
                self.server_metrics.loading = false;
                self.server_metrics.message.clear();
            }
            UiEvent::ServerMetrics(ServerData::Processes(processes)) => {
                self.server_metrics.processes = processes;
                self.server_metrics.processes_loading = false;
                self.server_metrics.processes_message.clear();
            }
            UiEvent::ServerMetrics(ServerData::Network(network)) => {
                self.server_metrics.network = network;
                self.server_metrics.network_loading = false;
                self.server_metrics.network_message.clear();
            }
            UiEvent::ServerMetrics(ServerData::Storage(storage)) => {
                self.server_metrics.storage = storage;
                self.server_metrics.storage_loading = false;
                self.server_metrics.storage_message.clear();
            }
            UiEvent::ServerMetrics(ServerData::Listeners(listeners)) => {
                self.server_metrics.listeners = listeners;
                self.server_metrics.listeners_loading = false;
                self.server_metrics.listeners_message.clear();
            }
            UiEvent::ServerMetrics(ServerData::ProcessAction(result)) => {
                self.server_metrics.processes_loading = false;
                self.server_metrics.processes_message = result.message;
            }
            UiEvent::ServerMetrics(ServerData::Services(services)) => {
                self.server_metrics.services = services;
                self.server_metrics.services_loading = false;
                self.server_metrics.services_message.clear();
            }
            UiEvent::ServerMetrics(ServerData::ServiceAction(result)) => {
                self.server_metrics.services_message = result.message;
            }
            UiEvent::ServerMetrics(ServerData::ServiceLogs(logs)) => {
                self.server_metrics.service_logs = Some(logs);
                self.server_metrics.service_logs_loading = false;
                self.server_metrics.service_logs_message.clear();
            }
            UiEvent::ServerMetrics(ServerData::PowerAction(result)) => {
                self.server_metrics.power_action_loading = false;
                self.server_metrics.power_action_message = result.message;
            }
            UiEvent::ServerMetricsError(ServerDataError::Metrics(error)) => {
                self.server_metrics.loading = false;
                self.server_metrics.message = error;
            }
            UiEvent::ServerMetricsError(ServerDataError::Processes(error)) => {
                self.server_metrics.processes_loading = false;
                self.server_metrics.processes_message = error;
            }
            UiEvent::ServerMetricsError(ServerDataError::Network(error)) => {
                self.server_metrics.network_loading = false;
                self.server_metrics.network_message = error;
            }
            UiEvent::ServerMetricsError(ServerDataError::Storage(error)) => {
                self.server_metrics.storage_loading = false;
                self.server_metrics.storage_message = error;
            }
            UiEvent::ServerMetricsError(ServerDataError::Listeners(error)) => {
                self.server_metrics.listeners_loading = false;
                self.server_metrics.listeners_message = error;
            }
            UiEvent::ServerMetricsError(ServerDataError::Services(error)) => {
                self.server_metrics.services_loading = false;
                self.server_metrics.services_message = error;
            }
            UiEvent::ServerMetricsError(ServerDataError::ServiceLogs(error)) => {
                self.server_metrics.service_logs_loading = false;
                self.server_metrics.service_logs_message = error;
            }
            UiEvent::Screen(frame) => {
                self.frame = Some(frame);
                self.terminal_message.clear();
                self.state = SessionState::Connected;
            }
            UiEvent::Failed(error) => {
                self.clear_terminal(
                    error,
                    "The SSH session ended. Verify the status and connect again.",
                );
                self.command_tx = None;
                self.cancel_tx = None;
                self.sftp_tx = None;
                self.sftp_cancel = None;
                self.sftp.transfer = None;
                self.sftp.active_transfer = None;
                self.sftp.transfer_queue.clear();
                self.sftp.overwrite = None;
                self.sftp.metadata_edit = None;
                self.sftp.filesystem = None;
                self.sftp.filesystem_supported = None;
                self.forward_tx = None;
                self.forwards.rules.clear();
                self.forwards.stopped.clear();
                self.log_path = None;
                self.connection_info = None;
                self.server_metrics = ServerMetricsViewState::default();
                self.state = SessionState::Disconnected;
            }
            UiEvent::Disconnected => {
                self.clear_terminal(
                    "Disconnected",
                    "Disconnected. Verify the host details and connect again.",
                );
                self.command_tx = None;
                self.cancel_tx = None;
                self.sftp_tx = None;
                self.sftp_cancel = None;
                self.sftp.transfer = None;
                self.sftp.active_transfer = None;
                self.sftp.transfer_queue.clear();
                self.sftp.overwrite = None;
                self.sftp.metadata_edit = None;
                self.sftp.filesystem = None;
                self.sftp.filesystem_supported = None;
                self.forward_tx = None;
                self.forwards.rules.clear();
                self.forwards.stopped.clear();
                self.log_path = None;
                self.connection_info = None;
                self.server_metrics = ServerMetricsViewState::default();
                self.state = SessionState::Disconnected;
            }
            UiEvent::AuthPrompt { .. } => {}
            UiEvent::SearchResult(_) => {}
        }
        true
    }
}

fn apply_session_event(sessions: &mut [SshSession], id: u64, event: UiEvent) -> bool {
    sessions
        .iter_mut()
        .find(|session| session.id == id)
        .is_some_and(|session| session.apply_event(event))
}

fn enqueue_session_request(
    sessions: &[SshSession],
    id: u64,
    request: TerminalRequest,
) -> Result<(), &'static str> {
    let Some(session) = sessions.iter().find(|session| session.id == id) else {
        return Err("SSH session is no longer available");
    };
    let Some(command_tx) = &session.command_tx else {
        return Err("SSH session is not accepting input");
    };
    command_tx.try_send(request).map_err(|error| match error {
        mpsc::error::TrySendError::Full(_) => "Terminal request queue is full",
        mpsc::error::TrySendError::Closed(_) => "SSH session is not accepting input",
    })
}

fn keyword_matches(text: &str, keywords: &[String]) -> bool {
    let text = text.to_lowercase();
    keywords.iter().any(|keyword| {
        let keyword = keyword.trim();
        !keyword.is_empty() && text.contains(&keyword.to_lowercase())
    })
}

fn session_matches_target(session: &SshSession, host: &str, port: u16, username: &str) -> bool {
    session.state != SessionState::Disconnected
        && session.host.eq_ignore_ascii_case(host)
        && session.port == port
        && session.username == username
}

fn is_authentication_failure(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("authentication")
        || message.contains("password")
        || message.contains("passphrase")
        || message.contains("private key")
}

fn remove_session(
    sessions: &mut Vec<SshSession>,
    active_session_id: &mut Option<u64>,
    id: u64,
) -> bool {
    let Some(index) = sessions.iter().position(|session| session.id == id) else {
        return false;
    };
    if let Some(cancel_tx) = &sessions[index].cancel_tx {
        cancel_tx.send_replace(true);
    }
    sessions.remove(index);
    if *active_session_id == Some(id) {
        *active_session_id = sessions
            .get(index)
            .or_else(|| sessions.last())
            .map(|session| session.id);
    }
    true
}

#[cfg(test)]
mod session_tests {
    use super::{
        AppShortcut, CachedCredentials, CredentialCacheKey, ForwardViewState, LayoutDragTarget,
        MouseTrackingMode, SessionEditorEscape, SessionEditorKey, SessionSplit, SessionState,
        SftpViewState, SplitOrientation, SshSession, TerminalFrame, TerminalRequest, TerminalSize,
        UiEvent, app_shortcut, apply_session_event, collapsed_group_key, dragged_panel_width,
        enqueue_session_request, filesystem_usage_percent, forget_stopped_forward,
        format_alert_time, format_log_time, format_rate, format_storage_kib,
        is_authentication_failure, is_sftp_transfer_command, keyword_matches, memory_usage_percent,
        merge_unique_profiles, move_profile_to_group, mpsc, parse_alert_percent,
        recall_compose_history, remember_compose_history, remember_stopped_forward,
        remove_profile_group, remove_session, rename_profile_group, reorder_group_order,
        reorder_session_ids, search_result_message, server_alert_observations,
        session_editor_escape, session_editor_key, session_matches_target,
        session_terminal_size_for_split, split_panel_terminal_size, synchronized_targets,
        take_next_sftp_transfer, terminal_size_for_viewport, watch,
    };
    use crate::forward::{ForwardCommand, ForwardKind, ForwardRuleSpec};
    use crate::log;
    use crate::profiles::{AuthMethod, HostProfile};
    use crate::settings::AppSettings;
    use crate::sftp::SftpCommand;
    use crate::shortcuts::{ShortcutAction, ShortcutSettings};
    use crate::ssh::{ServerMetrics, ServerProcess};
    use crate::terminal::{CELL_HEIGHT_PX, CELL_WIDTH_PX};
    use crate::ui::tokens as ui_tokens;
    use std::collections::{HashSet, VecDeque};
    use zeroize::Zeroizing;

    fn test_session(
        id: u64,
    ) -> (
        SshSession,
        mpsc::Receiver<TerminalRequest>,
        watch::Receiver<bool>,
    ) {
        let (command_tx, command_rx) = mpsc::channel(4);
        let (cancel_tx, cancel_rx) = watch::channel(false);
        (
            SshSession {
                id,
                title: format!("session {id}"),
                host: "127.0.0.1".to_owned(),
                port: 22,
                username: "test".to_owned(),
                status: format!("Session {id}"),
                frame: None,
                terminal_message: String::new(),
                terminal_size: TerminalSize {
                    columns: 80,
                    rows: 24,
                },
                scroll_remainder: 0.0,
                mouse_position: None,
                command_tx: Some(command_tx),
                cancel_tx: Some(cancel_tx),
                sftp_tx: None,
                sftp_cancel: None,
                sftp: SftpViewState::default(),
                forward_tx: None,
                forwards: ForwardViewState::default(),
                profile_name: None,
                pending_forwards: Vec::new(),
                log_path: None,
                connection_info: None,
                server_metrics: crate::ServerMetricsViewState::default(),
                credential_key: None,
                state: SessionState::Connected,
            },
            command_rx,
            cancel_rx,
        )
    }

    #[test]
    fn ssh_output_updates_only_the_target_session() {
        let (mut first, _, _) = test_session(1);
        first.state = SessionState::Connecting;
        let (second, _, _) = test_session(2);
        let mut sessions = vec![first, second];
        let frame = TerminalFrame {
            rows: Vec::new(),
            selection_text: None,
            mouse_tracking_mode: MouseTrackingMode::Disabled,
            foreground: 0xe1e5e9,
            background: 0x0b0f13,
            cursor: None,
        };

        assert!(apply_session_event(
            &mut sessions,
            2,
            UiEvent::Screen(frame)
        ));
        assert!(sessions[0].frame.is_none());
        assert!(sessions[1].frame.is_some());
        assert!(matches!(sessions[0].state, SessionState::Connecting));
        assert!(matches!(sessions[1].state, SessionState::Connected));
        assert!(!apply_session_event(
            &mut sessions,
            99,
            UiEvent::Disconnected
        ));
    }

    #[test]
    fn host_rows_reuse_only_live_sessions_for_the_same_target() {
        let (mut session, _, _) = test_session(1);
        assert!(session_matches_target(&session, "127.0.0.1", 22, "test"));
        assert!(!session_matches_target(&session, "127.0.0.1", 22, "TEST"));
        assert!(!session_matches_target(&session, "127.0.0.1", 2222, "test"));

        session.state = SessionState::Disconnected;
        assert!(!session_matches_target(&session, "127.0.0.1", 22, "test"));
    }

    #[test]
    fn authentication_failures_reopen_the_credential_flow() {
        assert!(is_authentication_failure(
            "SSH password authentication was rejected"
        ));
        assert!(is_authentication_failure(
            "private key passphrase is invalid"
        ));
        assert!(!is_authentication_failure(
            "Server key is not present in known_hosts"
        ));
    }

    #[test]
    fn alert_thresholds_accept_only_integer_percentages() {
        assert_eq!(parse_alert_percent("80"), Some(80));
        assert_eq!(parse_alert_percent(" 1 "), Some(1));
        assert_eq!(parse_alert_percent("100"), Some(100));
        assert_eq!(parse_alert_percent("0"), None);
        assert_eq!(parse_alert_percent("101"), None);
        assert_eq!(parse_alert_percent("80.5"), None);
    }

    #[test]
    fn credential_cache_keys_normalize_profile_target_fields() {
        let first = CredentialCacheKey::from_connection(
            "  example.com ",
            22,
            " ops ",
            AuthMethod::Password,
            " ~/.ssh/id_ed25519 ",
            " jump ",
        );
        let second = CredentialCacheKey::from_connection(
            "example.com",
            22,
            "ops",
            AuthMethod::Password,
            "~/.ssh/id_ed25519",
            "jump",
        );
        assert_eq!(first, second);
    }

    #[test]
    fn cached_credentials_track_only_non_empty_secret_values() {
        let empty = CachedCredentials::from_inputs(
            &Zeroizing::new(String::new()),
            &Zeroizing::new(String::new()),
            &Zeroizing::new(String::new()),
            &Zeroizing::new(String::new()),
        );
        assert!(!empty.contains_secret());

        let password = Zeroizing::new("secret".to_owned());
        let cached = CachedCredentials::from_inputs(
            &password,
            &Zeroizing::new(String::new()),
            &Zeroizing::new(String::new()),
            &Zeroizing::new(String::new()),
        );
        assert!(cached.contains_secret());
    }

    #[test]
    fn terminal_input_is_enqueued_only_for_the_target_session() {
        let (first, mut first_rx, _) = test_session(1);
        let (second, mut second_rx, _) = test_session(2);
        let sessions = vec![first, second];

        assert!(
            enqueue_session_request(
                &sessions,
                2,
                TerminalRequest::Paste("target only".to_owned()),
            )
            .is_ok()
        );
        assert!(first_rx.try_recv().is_err());
        assert!(matches!(
            second_rx.try_recv(),
            Ok(TerminalRequest::Paste(text)) if text == "target only"
        ));
    }

    #[test]
    fn log_replay_reads_the_tail_with_a_size_limit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.log");
        std::fs::write(&path, b"0123456789").unwrap();

        assert_eq!(
            log::read_replay(&path, 4).unwrap(),
            log::ReplayLog {
                bytes: b"6789".to_vec(),
                timeline: Vec::new(),
                truncated: true,
                timestamped: false,
            }
        );
        assert_eq!(
            log::read_replay(&path, 100).unwrap(),
            log::ReplayLog {
                bytes: b"0123456789".to_vec(),
                timeline: Vec::new(),
                truncated: false,
                timestamped: false,
            }
        );
    }

    #[test]
    fn search_result_messages_show_position_and_wrapping() {
        use crate::terminal::TerminalSearchResult;
        assert_eq!(
            search_result_message(&TerminalSearchResult {
                found: false,
                total: 0,
                current: 0,
                wrapped: false,
            }),
            "No matches"
        );
        assert_eq!(
            search_result_message(&TerminalSearchResult {
                found: true,
                total: 5,
                current: 1,
                wrapped: false,
            }),
            "2/5 matches"
        );
        assert_eq!(
            search_result_message(&TerminalSearchResult {
                found: true,
                total: 5,
                current: 4,
                wrapped: true,
            }),
            "5/5 matches (wrapped)"
        );
    }

    #[test]
    fn workspace_shortcuts_are_distinct_from_terminal_copy_and_paste() {
        let defaults = ShortcutSettings::default();
        assert_eq!(
            app_shortcut(&defaults, "t", true, true, false),
            Some(AppShortcut::NewSession)
        );
        assert_eq!(
            app_shortcut(&defaults, "w", true, true, false),
            Some(AppShortcut::CloseSession)
        );
        assert_eq!(
            app_shortcut(&defaults, "f", true, true, false),
            Some(AppShortcut::ToggleFiles)
        );
        assert_eq!(
            app_shortcut(&defaults, "s", true, true, false),
            Some(AppShortcut::ToggleSnippets)
        );
        assert_eq!(
            app_shortcut(&defaults, "o", true, true, false),
            Some(AppShortcut::ToggleSettings)
        );
        assert_eq!(
            app_shortcut(&defaults, "m", true, true, false),
            Some(AppShortcut::ToggleCompose)
        );
        assert_eq!(
            app_shortcut(&defaults, "b", true, true, false),
            Some(AppShortcut::ToggleSidebar)
        );
        assert_eq!(app_shortcut(&defaults, "c", true, true, false), None);
        assert_eq!(app_shortcut(&defaults, "v", true, true, false), None);
        assert_eq!(
            app_shortcut(&defaults, "d", true, true, false),
            Some(AppShortcut::Disconnect)
        );
        assert_eq!(
            app_shortcut(&defaults, "r", true, true, false),
            Some(AppShortcut::ReplayLog)
        );
        assert_eq!(
            app_shortcut(&defaults, "f", true, false, false),
            Some(AppShortcut::Find)
        );
        assert_eq!(
            app_shortcut(&defaults, "1", false, false, true),
            Some(AppShortcut::SelectSession(0))
        );
        assert_eq!(
            app_shortcut(&defaults, "9", false, false, true),
            Some(AppShortcut::SelectSession(8))
        );
        assert_eq!(app_shortcut(&defaults, "0", false, false, true), None);
        assert_eq!(app_shortcut(&defaults, "c", true, true, false), None);
        assert_eq!(app_shortcut(&defaults, "v", true, true, false), None);
        assert_eq!(app_shortcut(&defaults, "t", true, false, false), None);

        let mut custom = ShortcutSettings::default();
        custom.set(ShortcutAction::NewSession, "ctrl+alt+n");
        assert_eq!(
            app_shortcut(&custom, "n", true, false, true),
            Some(AppShortcut::NewSession)
        );
        assert_eq!(app_shortcut(&custom, "t", true, true, false), None);
    }

    #[test]
    fn keyword_matching_is_literal_and_case_insensitive() {
        let keywords = vec!["error".to_owned(), "timeout".to_owned()];
        assert!(keyword_matches("Remote ERROR: timeout", &keywords));
        assert!(keyword_matches("request timed out", &["timed".to_owned()]));
        assert!(!keyword_matches("connected", &keywords));
        assert!(!keyword_matches("error", &[String::new()]));
    }

    #[test]
    fn closing_a_session_cancels_only_it_and_selects_a_neighbor() {
        let (first, _, first_cancel_rx) = test_session(1);
        let (second, _, second_cancel_rx) = test_session(2);
        let (third, _, third_cancel_rx) = test_session(3);
        let mut sessions = vec![first, second, third];
        let mut active_session_id = Some(2);

        assert!(remove_session(&mut sessions, &mut active_session_id, 2));
        assert_eq!(
            sessions
                .iter()
                .map(|session| session.id)
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(active_session_id, Some(3));
        assert!(*second_cancel_rx.borrow());
        assert!(!*first_cancel_rx.borrow());
        assert!(!*third_cancel_rx.borrow());
        assert!(!remove_session(&mut sessions, &mut active_session_id, 99));
    }

    #[test]
    fn persisted_forwards_start_after_the_session_connects() {
        let (mut session, _, _) = test_session(1);
        session.state = SessionState::Connecting;
        let (forward_tx, mut forward_rx) = mpsc::channel(4);
        session.forward_tx = Some(forward_tx);
        let rule = ForwardRuleSpec {
            kind: ForwardKind::Local,
            listen_host: "127.0.0.1".to_owned(),
            listen_port: 15432,
            target_host: "db.internal".to_owned(),
            target_port: 5432,
        };
        session.pending_forwards = vec![rule.clone()];
        let mut sessions = vec![session];

        assert!(apply_session_event(
            &mut sessions,
            1,
            UiEvent::Status("Connected to db.internal:22".to_owned())
        ));
        assert!(matches!(
            forward_rx.try_recv(),
            Ok(ForwardCommand::Add(added)) if added == rule
        ));
        assert!(sessions[0].pending_forwards.is_empty());
    }

    #[test]
    fn session_editor_escape_and_ctrl_enter_are_keyboard_actions() {
        assert_eq!(
            session_editor_key("Escape", false, false),
            Some(SessionEditorKey::Cancel)
        );
        assert_eq!(
            session_editor_key("enter", true, false),
            Some(SessionEditorKey::Save)
        );
        assert_eq!(
            session_editor_key("Enter", false, true),
            Some(SessionEditorKey::Save)
        );
        assert_eq!(session_editor_key("enter", false, false), None);
        assert_eq!(session_editor_key("tab", false, false), None);
    }

    #[test]
    fn escape_cancels_a_pending_delete_before_closing_the_editor() {
        assert_eq!(
            session_editor_escape(true),
            SessionEditorEscape::CancelDelete
        );
        assert_eq!(
            session_editor_escape(false),
            SessionEditorEscape::CloseEditor
        );
    }

    #[test]
    fn terminal_layout_stays_usable_across_dpi_and_font_scales() {
        let mut settings = AppSettings::default();
        let mut previous_columns = u16::MAX;
        for font_size in [9.0, 13.0, 24.0, 32.0] {
            settings.font_size = font_size;
            let size = terminal_size_for_viewport(1280.0, 800.0, &settings, false, false);
            assert!(size.columns > 0 && size.rows > 0, "font size {font_size}");
            assert!(
                size.columns < previous_columns,
                "larger fonts must reduce the column count"
            );
            previous_columns = size.columns;
        }

        // The same physical display at 100%, 150% and 200% DPI maps to smaller
        // logical viewports; the layout must stay positive at every step.
        settings.font_size = 13.0;
        for (width, height) in [(1920.0, 1080.0), (1280.0, 720.0), (960.0, 540.0)] {
            let size = terminal_size_for_viewport(width, height, &settings, false, false);
            assert!(size.columns > 0 && size.rows > 0, "{width}x{height}");
        }
    }

    #[test]
    fn panel_drag_clamps_to_the_configured_range() {
        assert_eq!(
            dragged_panel_width(LayoutDragTarget::Sidebar, 248.0, 1000.0),
            ui_tokens::SIDEBAR_MAX_WIDTH
        );
        assert_eq!(
            dragged_panel_width(LayoutDragTarget::Sidebar, 248.0, -1000.0),
            ui_tokens::SIDEBAR_MIN_WIDTH
        );
        assert_eq!(
            dragged_panel_width(LayoutDragTarget::Inspector, 380.0, 1000.0),
            ui_tokens::INSPECTOR_MIN_WIDTH
        );
        assert_eq!(
            dragged_panel_width(LayoutDragTarget::Inspector, 380.0, -1000.0),
            ui_tokens::INSPECTOR_MAX_WIDTH
        );
    }

    #[test]
    fn custom_panel_widths_reduce_terminal_columns_without_changing_rows() {
        let mut settings = AppSettings::default();
        let default_size = terminal_size_for_viewport(1280.0, 800.0, &settings, false, true);
        settings.sidebar_width = ui_tokens::SIDEBAR_MAX_WIDTH;
        settings.inspector_width = ui_tokens::INSPECTOR_MAX_WIDTH;
        let narrow = terminal_size_for_viewport(1280.0, 800.0, &settings, false, true);
        assert!(narrow.columns < default_size.columns);
        assert_eq!(narrow.rows, default_size.rows);
    }

    #[test]
    fn stopped_forward_rules_stay_available_for_restart() {
        let spec = ForwardRuleSpec {
            kind: ForwardKind::Local,
            listen_host: "127.0.0.1".to_owned(),
            listen_port: 0,
            target_host: "db.internal".to_owned(),
            target_port: 5432,
        };
        let mut state = ForwardViewState::default();
        remember_stopped_forward(&mut state, spec.clone());
        remember_stopped_forward(&mut state, spec.clone());
        assert_eq!(state.stopped.len(), 1);
        forget_stopped_forward(&mut state, &spec);
        assert!(state.stopped.is_empty());
    }

    #[test]
    fn transfer_rates_are_formatted_for_the_inspector() {
        assert_eq!(format_rate(0.0), "0 B/s");
        assert_eq!(format_rate(f64::NAN), "0 B/s");
        assert!(format_rate(1536.0).ends_with("/s"));
    }

    #[test]
    fn storage_sizes_are_formatted_from_kib() {
        assert_eq!(format_storage_kib(0), "0 B");
        assert_eq!(format_storage_kib(1024), "1.0 MiB");
    }

    #[test]
    fn filesystem_usage_handles_empty_and_clamped_capacity_values() {
        assert_eq!(filesystem_usage_percent(0, 0), 0);
        assert_eq!(filesystem_usage_percent(100, 100), 0);
        assert_eq!(filesystem_usage_percent(100, 25), 75);
        assert_eq!(filesystem_usage_percent(100, 200), 0);
    }

    #[test]
    fn memory_usage_handles_missing_and_clamped_capacity_values() {
        assert_eq!(memory_usage_percent(None, Some(10)), None);
        assert_eq!(memory_usage_percent(Some(0), Some(0)), Some(0));
        assert_eq!(memory_usage_percent(Some(100), Some(25)), Some(75));
        assert_eq!(memory_usage_percent(Some(100), Some(200)), Some(0));
    }

    #[test]
    fn server_alert_observations_capture_cpu_memory_and_disk_thresholds() {
        let metrics = ServerMetrics {
            os: "Linux".to_owned(),
            kernel: "6.8".to_owned(),
            load_1m: None,
            load_5m: None,
            load_15m: None,
            memory_total_kib: Some(100),
            memory_available_kib: Some(10),
            disk_total_kib: Some(100),
            disk_used_kib: Some(95),
            disk_available_kib: Some(5),
            disk_use_percent: Some(95),
            uptime_seconds: None,
        };
        let processes = vec![ServerProcess {
            pid: 42,
            user: "root".to_owned(),
            cpu_percent: 91.25,
            memory_percent: 1.0,
            state: "R".to_owned(),
            command: "busy".to_owned(),
        }];
        let alerts = server_alert_observations(Some(&metrics), &processes, 80, 80, 90);
        assert_eq!(alerts.len(), 3);
        assert_eq!(alerts[0].value, "91.2%");
        assert_eq!(alerts[1].value, "90%");
        assert_eq!(alerts[2].value, "95%");
        assert_eq!(format_alert_time(86_401), "1d 00:00:01 UTC");
    }

    #[test]
    fn server_alert_observations_ignore_missing_metrics_and_safe_cpu_values() {
        let processes = vec![ServerProcess {
            pid: 42,
            user: "root".to_owned(),
            cpu_percent: 79.9,
            memory_percent: 0.0,
            state: "S".to_owned(),
            command: "idle".to_owned(),
        }];
        assert!(server_alert_observations(None, &processes, 80, 80, 90).is_empty());
    }

    #[test]
    fn sftp_transfer_queue_ignores_non_transfer_commands() {
        let upload = SftpCommand::Upload {
            local: "local.txt".into(),
            remote: "/tmp/local.txt".to_owned(),
            overwrite: false,
            resume: false,
        };
        let list = SftpCommand::List("/tmp".to_owned());
        assert!(is_sftp_transfer_command(&upload));
        assert!(!is_sftp_transfer_command(&list));
    }

    #[test]
    fn sftp_transfer_queue_is_fifo_and_pauses_for_active_work() {
        let first = SftpCommand::Upload {
            local: "first.txt".into(),
            remote: "/tmp/first.txt".to_owned(),
            overwrite: false,
            resume: false,
        };
        let second = SftpCommand::Download {
            remote: "/tmp/second.txt".to_owned(),
            local: "second.txt".into(),
            overwrite: false,
            resume: false,
        };
        let mut queue = VecDeque::from([first, second]);
        assert!(take_next_sftp_transfer(true, false, &mut queue).is_none());
        assert!(take_next_sftp_transfer(false, true, &mut queue).is_none());
        assert!(matches!(
            take_next_sftp_transfer(false, false, &mut queue),
            Some(SftpCommand::Upload { .. })
        ));
        assert!(matches!(
            take_next_sftp_transfer(false, false, &mut queue),
            Some(SftpCommand::Download { .. })
        ));
        assert!(queue.is_empty());
    }

    #[test]
    fn tab_drag_reorders_sessions_without_changing_their_identity() {
        let mut ids = vec![1, 2, 3, 4];
        assert!(reorder_session_ids(&mut ids, 1, 3));
        assert_eq!(ids, vec![2, 3, 1, 4]);
        assert!(reorder_session_ids(&mut ids, 4, 2));
        assert_eq!(ids, vec![4, 2, 3, 1]);
        assert!(!reorder_session_ids(&mut ids, 2, 2));
        assert!(!reorder_session_ids(&mut ids, 9, 2));
        assert_eq!(ids, vec![4, 2, 3, 1]);
    }

    #[test]
    fn log_timeline_times_are_formatted() {
        assert_eq!(format_log_time(0), "00:00.000");
        assert_eq!(format_log_time(61_234), "01:01.234");
        assert_eq!(format_log_time(3_661_005), "1:01:01.005");
    }

    #[test]
    fn dragging_a_host_moves_it_between_groups() {
        fn profile(name: &str, group: &str) -> HostProfile {
            HostProfile {
                name: name.to_owned(),
                host: "example.com".to_owned(),
                port: 22,
                username: "ops".to_owned(),
                group: group.to_owned(),
                favorite: false,
                auth_method: AuthMethod::Password,
                identity_file: String::new(),
                proxy_jump: String::new(),
                forwards: Vec::new(),
                keepalive_seconds: 0,
                password: String::new(),
                passphrase: String::new(),
                jump_password: String::new(),
                jump_passphrase: String::new(),
            }
        }

        let mut profiles = vec![profile("web", ""), profile("db", "Prod")];
        assert!(move_profile_to_group(&mut profiles, "web", "Prod"));
        assert_eq!(
            profiles
                .iter()
                .find(|profile| profile.name == "web")
                .unwrap()
                .group,
            "Prod"
        );
        assert!(!move_profile_to_group(&mut profiles, "web", "Prod"));
        assert!(move_profile_to_group(&mut profiles, "db", ""));
        assert!(
            profiles
                .iter()
                .find(|profile| profile.name == "db")
                .unwrap()
                .group
                .is_empty()
        );
        assert!(!move_profile_to_group(&mut profiles, "missing", "Prod"));
    }

    #[test]
    fn merging_imported_profiles_skips_duplicate_names_and_sorts_new_hosts() {
        fn profile(name: &str, group: &str) -> HostProfile {
            HostProfile {
                name: name.to_owned(),
                host: format!("{name}.example.com"),
                port: 22,
                username: "ops".to_owned(),
                group: group.to_owned(),
                favorite: false,
                auth_method: AuthMethod::Password,
                identity_file: String::new(),
                proxy_jump: String::new(),
                forwards: Vec::new(),
                keepalive_seconds: 0,
                password: String::new(),
                passphrase: String::new(),
                jump_password: String::new(),
                jump_passphrase: String::new(),
            }
        }

        let mut profiles = vec![profile("existing", "Production")];
        let (added, skipped) = merge_unique_profiles(
            &mut profiles,
            vec![profile("EXISTING", "Other"), profile("new", "Development")],
        );
        assert_eq!((added, skipped), (1, 1));
        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile.name.as_str())
                .collect::<Vec<_>>(),
            vec!["new", "existing"]
        );
    }

    #[test]
    fn dragging_a_group_reorders_persisted_group_order() {
        let mut order = vec!["alpha".to_owned(), "beta".to_owned(), "gamma".to_owned()];
        assert!(reorder_group_order(&mut order, " GAMMA ", " alpha "));
        assert_eq!(order, vec!["gamma", "alpha", "beta"]);
        assert!(!reorder_group_order(&mut order, "gamma", "GAMMA"));
        assert!(!reorder_group_order(&mut order, "missing", "missing"));
    }

    #[test]
    fn renaming_a_group_updates_matching_profiles_case_insensitively() {
        let mut profiles = vec![
            HostProfile {
                name: "prod-a".to_owned(),
                host: "a.example.com".to_owned(),
                port: 22,
                username: "ops".to_owned(),
                group: "Production".to_owned(),
                favorite: false,
                auth_method: AuthMethod::Password,
                identity_file: String::new(),
                proxy_jump: String::new(),
                forwards: Vec::new(),
                keepalive_seconds: 0,
                password: String::new(),
                passphrase: String::new(),
                jump_password: String::new(),
                jump_passphrase: String::new(),
            },
            HostProfile {
                name: "prod-b".to_owned(),
                host: "b.example.com".to_owned(),
                port: 22,
                username: "ops".to_owned(),
                group: "production".to_owned(),
                favorite: false,
                auth_method: AuthMethod::Password,
                identity_file: String::new(),
                proxy_jump: String::new(),
                forwards: Vec::new(),
                keepalive_seconds: 0,
                password: String::new(),
                passphrase: String::new(),
                jump_password: String::new(),
                jump_passphrase: String::new(),
            },
        ];
        assert_eq!(
            rename_profile_group(&mut profiles, " PRODUCTION ", "Live"),
            2
        );
        assert!(profiles.iter().all(|profile| profile.group == "Live"));
    }

    #[test]
    fn deleting_a_group_moves_profiles_to_ungrouped() {
        let mut profiles = vec![HostProfile {
            name: "prod".to_owned(),
            host: "prod.example.com".to_owned(),
            port: 22,
            username: "ops".to_owned(),
            group: "Production".to_owned(),
            favorite: false,
            auth_method: AuthMethod::Password,
            identity_file: String::new(),
            proxy_jump: String::new(),
            forwards: Vec::new(),
            keepalive_seconds: 0,
            password: String::new(),
            passphrase: String::new(),
            jump_password: String::new(),
            jump_passphrase: String::new(),
        }];
        assert_eq!(remove_profile_group(&mut profiles, "production"), 1);
        assert!(profiles[0].group.is_empty());
        assert_eq!(remove_profile_group(&mut profiles, "production"), 0);
    }

    #[test]
    fn collapsed_group_keys_are_case_insensitive_and_trimmed() {
        assert_eq!(collapsed_group_key("  Production "), "production");
        assert_eq!(collapsed_group_key(""), "");
    }

    #[test]
    fn compose_history_is_deduplicated_bounded_and_recalled_in_order() {
        let mut history = Vec::new();
        remember_compose_history(&mut history, "one".to_owned());
        remember_compose_history(&mut history, "two".to_owned());
        remember_compose_history(&mut history, "one".to_owned());
        assert_eq!(history, vec!["one".to_owned(), "two".to_owned()]);
        assert_eq!(
            recall_compose_history(&history, 0),
            Some(("one".to_owned(), 1))
        );
        assert_eq!(
            recall_compose_history(&history, 1),
            Some(("two".to_owned(), 0))
        );
        for index in 0..25 {
            remember_compose_history(&mut history, format!("cmd{index}"));
        }
        assert_eq!(history.len(), 20);
        assert_eq!(history[0], "cmd24");
        assert!(recall_compose_history(&[], 0).is_none());
    }

    #[test]
    fn synchronized_input_targets_only_selected_connected_sessions() {
        let selected: HashSet<u64> = [1, 2, 3, 9].into_iter().collect();
        assert_eq!(synchronized_targets(1, false, &selected, |_| true), vec![1]);
        assert_eq!(
            synchronized_targets(1, true, &selected, |id| matches!(id, 2 | 3)),
            vec![1, 2, 3]
        );
        assert_eq!(
            synchronized_targets(2, true, &selected, |id| matches!(id, 1 | 2)),
            vec![2, 1]
        );
        assert_eq!(
            synchronized_targets(1, true, &HashSet::new(), |_| true),
            vec![1]
        );
    }

    #[test]
    fn shell_viewport_keeps_the_terminal_usable_at_common_window_sizes() {
        let settings = AppSettings::default();
        let minimum = terminal_size_for_viewport(1280.0, 800.0, &settings, false, false);
        assert!(
            minimum.columns >= 80,
            "1280x800 must keep at least 80 columns, got {}",
            minimum.columns
        );
        assert!(minimum.rows >= 24);

        let expanded = terminal_size_for_viewport(1920.0, 1080.0, &settings, false, false);
        assert!(expanded.columns > minimum.columns);
        assert!(expanded.rows > minimum.rows);
    }

    #[test]
    fn collapsing_the_sidebar_widens_the_terminal() {
        let settings = AppSettings::default();
        let expanded = terminal_size_for_viewport(1280.0, 800.0, &settings, false, false);
        let collapsed = terminal_size_for_viewport(1280.0, 800.0, &settings, true, false);
        assert!(collapsed.columns > expanded.columns);
        assert_eq!(collapsed.rows, expanded.rows);
    }

    #[test]
    fn opening_the_inspector_reduces_terminal_columns_only() {
        let settings = AppSettings::default();
        let hidden = terminal_size_for_viewport(1280.0, 800.0, &settings, false, false);
        let shown = terminal_size_for_viewport(1280.0, 800.0, &settings, false, true);
        assert!(shown.columns < hidden.columns);
        assert_eq!(shown.rows, hidden.rows);
    }

    #[test]
    fn split_resize_updates_the_axis_dimension_for_each_pane() {
        let full_size = TerminalSize {
            columns: 100,
            rows: 40,
        };

        let left = split_panel_terminal_size(
            full_size,
            SplitOrientation::SideBySide,
            Some(400.0),
            0,
            CELL_WIDTH_PX,
            CELL_HEIGHT_PX,
        );
        let right = split_panel_terminal_size(
            full_size,
            SplitOrientation::SideBySide,
            Some(320.0),
            1,
            CELL_WIDTH_PX,
            CELL_HEIGHT_PX,
        );
        assert_eq!(
            left,
            TerminalSize {
                columns: 49,
                rows: 40
            }
        );
        assert_eq!(
            right,
            TerminalSize {
                columns: 39,
                rows: 40
            }
        );

        let top = split_panel_terminal_size(
            full_size,
            SplitOrientation::Stacked,
            Some(208.0),
            0,
            CELL_WIDTH_PX,
            CELL_HEIGHT_PX,
        );
        let bottom = split_panel_terminal_size(
            full_size,
            SplitOrientation::Stacked,
            Some(190.0),
            1,
            CELL_WIDTH_PX,
            CELL_HEIGHT_PX,
        );
        assert_eq!(
            top,
            TerminalSize {
                columns: 100,
                rows: 9
            }
        );
        assert_eq!(
            bottom,
            TerminalSize {
                columns: 100,
                rows: 8
            }
        );
    }

    #[test]
    fn split_layout_contains_exactly_its_two_sessions() {
        let split = SessionSplit {
            first_session_id: 11,
            second_session_id: 22,
            orientation: SplitOrientation::SideBySide,
        };

        assert!(split.contains(11));
        assert!(split.contains(22));
        assert!(!split.contains(33));
    }

    #[test]
    fn split_resize_is_scoped_to_its_two_sessions() {
        let full_size = TerminalSize {
            columns: 100,
            rows: 40,
        };
        let first_size = TerminalSize {
            columns: 50,
            rows: 40,
        };
        let second_size = TerminalSize {
            columns: 50,
            rows: 40,
        };
        let split = SessionSplit {
            first_session_id: 11,
            second_session_id: 22,
            orientation: SplitOrientation::SideBySide,
        };
        let split_sizes = Some((split, first_size, second_size));

        assert_eq!(
            session_terminal_size_for_split(11, full_size, split_sizes),
            first_size
        );
        assert_eq!(
            session_terminal_size_for_split(22, full_size, split_sizes),
            second_size
        );
        assert_eq!(
            session_terminal_size_for_split(33, full_size, split_sizes),
            full_size
        );
    }
}

struct AppView {
    ui_theme: UiTheme,
    sidebar_collapsed: bool,
    inspector_visible: bool,
    inspector_transfers_open: bool,
    inspector_forwards_open: bool,
    inspector_info_open: bool,
    inspector_server_open: bool,
    inspector_processes_open: bool,
    inspector_network_open: bool,
    inspector_storage_open: bool,
    inspector_listeners_open: bool,
    pending_process_termination: Option<(u64, u32)>,
    inspector_services_open: bool,
    pending_service_action: Option<(u64, String, ServiceAction)>,
    pending_power_action: Option<(u64, ServerPowerAction)>,
    inspector_log_open: bool,
    profile_name: Entity<InputState>,
    profile_search: Entity<InputState>,
    profile_group: Entity<InputState>,
    profile_favorite: bool,
    auth_method: AuthMethod,
    auth_profiles: Vec<AuthProfile>,
    auth_profile_store: Option<AuthProfileStore>,
    auth_profile_name: Entity<InputState>,
    auth_profile_message: String,
    applied_auth_profile: Option<String>,
    identity_file: Entity<InputState>,
    key_passphrase: Entity<InputState>,
    jump_profile: Entity<InputState>,
    jump_password: Entity<InputState>,
    jump_passphrase: Entity<InputState>,
    keepalive_seconds: Entity<InputState>,
    forward_kind: ForwardKind,
    forward_listen_host: Entity<InputState>,
    forward_listen_port: Entity<InputState>,
    forward_target_host: Entity<InputState>,
    forward_target_port: Entity<InputState>,
    sftp_name: Entity<InputState>,
    sftp_permissions: Entity<InputState>,
    sftp_atime: Entity<InputState>,
    sftp_mtime: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    status: String,
    sessions: Vec<SshSession>,
    active_session_id: Option<u64>,
    split: Option<SessionSplit>,
    next_session_id: u64,
    terminal_size: TerminalSize,
    terminal_bounds: Rc<RefCell<HashMap<u64, Bounds<Pixels>>>>,
    terminal_focus: FocusHandle,
    overlay_focus: FocusHandle,
    overlay_focus_pending: bool,
    toast: Option<ToastState>,
    toast_generation: u64,
    layout_drag: Option<LayoutDrag>,
    side_by_side_state: Entity<ResizableState>,
    stacked_state: Entity<ResizableState>,
    profiles: Vec<HostProfile>,
    recent: Vec<RecentConnection>,
    collapsed_groups: HashSet<String>,
    group_editor_name: Entity<InputState>,
    group_editor_open: bool,
    group_editor_mode: GroupEditorMode,
    group_editor_source: Option<String>,
    group_editor_message: String,
    profile_store: Option<ProfileStore>,
    profiles_ready: bool,
    selected_profile: Option<String>,
    session_editor_open: bool,
    credential_prompt_open: bool,
    credential_prompt_session_id: Option<u64>,
    session_editor_section: SessionEditorSection,
    confirm_delete: bool,
    profile_message: String,
    credential_focus_pending: bool,
    credential_cache: HashMap<CredentialCacheKey, CachedCredentials>,
    pending_credentials: HashMap<u64, (CredentialCacheKey, CachedCredentials)>,
    snippets: Vec<CommandSnippet>,
    snippet_store: Option<SnippetStore>,
    snippet_name: Entity<InputState>,
    snippet_command: Entity<InputState>,
    snippet_send_enter: bool,
    editing_snippet: Option<String>,
    snippet_message: String,
    snippets_visible: bool,
    snippet_targets: HashSet<u64>,
    keyword_input: Entity<InputState>,
    keyword_highlights: Vec<String>,
    keyword_highlighting_enabled: bool,
    keyword_message: String,
    keywords_visible: bool,
    trigger_keyword_input: Entity<InputState>,
    trigger_text_input: Entity<InputState>,
    trigger_rules: Vec<TriggerRule>,
    trigger_action_kind: TriggerActionKind,
    trigger_send_enter: bool,
    trigger_engine_enabled: bool,
    trigger_message: String,
    triggers_visible: bool,
    trigger_latches: HashMap<u64, HashSet<String>>,
    settings: AppSettings,
    settings_store: Option<SettingsStore>,
    settings_font_family: Entity<InputState>,
    settings_font_size: Entity<InputState>,
    settings_alert_cpu: Entity<InputState>,
    settings_alert_memory: Entity<InputState>,
    settings_alert_disk: Entity<InputState>,
    settings_theme: TerminalTheme,
    settings_cursor: CursorStyle,
    settings_language: Language,
    shortcut_inputs: Vec<Entity<InputState>>,
    settings_message: String,
    settings_visible: bool,
    keys_visible: bool,
    key_path: Entity<InputState>,
    new_key_passphrase: Entity<InputState>,
    key_comment: Entity<InputState>,
    key_message: String,
    key_public: Option<String>,
    key_algorithm: keygen::KeyAlgorithm,
    existing_keys: Vec<keygen::ExistingKey>,
    key_discovery_message: String,
    key_change_path: Option<PathBuf>,
    key_change_old_passphrase: Entity<InputState>,
    key_change_new_passphrase: Entity<InputState>,
    key_change_message: String,
    key_change_visible: bool,
    key_delete_path: Option<PathBuf>,
    key_delete_message: String,
    key_delete_visible: bool,
    auth_prompt: Option<AuthPromptState>,
    auth_prompt_inputs: Vec<Entity<InputState>>,
    auth_prompt_plain_inputs: Vec<Entity<InputState>>,
    auth_prompt_needs_clear: bool,
    log_replay: Option<LogReplayState>,
    search_input: Entity<InputState>,
    compose_input: Entity<InputState>,
    compose_visible: bool,
    compose_history: Vec<String>,
    compose_recall: usize,
    sync_input_enabled: bool,
    sync_targets: HashSet<u64>,
    sync_visible: bool,
    menu_visible: bool,
    layout_menu_visible: bool,
    tab_menu_session: Option<u64>,
    search_visible: bool,
    search_message: String,
    search_target: Option<SearchTarget>,
    _bounds_subscription: Subscription,
    _side_by_side_subscription: Subscription,
    _stacked_subscription: Subscription,
}

impl AppView {
    fn tr(&self, key: &str) -> String {
        i18n::translate(self.settings.language, key).into_owned()
    }

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let profile_name = cx.new(|cx| InputState::new(window, cx).placeholder("Production"));
        let profile_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search saved hosts"));
        let profile_group = cx.new(|cx| InputState::new(window, cx).placeholder("Ungrouped"));
        let group_editor_name = cx.new(|cx| InputState::new(window, cx).placeholder("Group name"));
        let identity_file =
            cx.new(|cx| InputState::new(window, cx).placeholder("~/.ssh/id_ed25519"));
        let key_passphrase = cx.new(|cx| InputState::new(window, cx).placeholder("Key passphrase"));
        key_passphrase.update(cx, |state, cx| state.set_masked(true, window, cx));
        let jump_profile =
            cx.new(|cx| InputState::new(window, cx).placeholder("Saved jump host name"));
        let jump_password =
            cx.new(|cx| InputState::new(window, cx).placeholder("Jump host password"));
        jump_password.update(cx, |state, cx| state.set_masked(true, window, cx));
        let jump_passphrase =
            cx.new(|cx| InputState::new(window, cx).placeholder("Jump key passphrase"));
        jump_passphrase.update(cx, |state, cx| state.set_masked(true, window, cx));
        let keepalive_seconds = cx.new(|cx| InputState::new(window, cx).default_value("0"));
        let forward_listen_host =
            cx.new(|cx| InputState::new(window, cx).default_value("127.0.0.1"));
        let forward_listen_port = cx.new(|cx| InputState::new(window, cx).default_value("0"));
        let forward_target_host =
            cx.new(|cx| InputState::new(window, cx).placeholder("target.internal"));
        let forward_target_port = cx.new(|cx| InputState::new(window, cx).default_value("22"));
        let sftp_name = cx.new(|cx| InputState::new(window, cx).placeholder("name"));
        let sftp_permissions = cx.new(|cx| InputState::new(window, cx).placeholder("0755"));
        let sftp_atime = cx.new(|cx| InputState::new(window, cx).placeholder("Unix seconds"));
        let sftp_mtime = cx.new(|cx| InputState::new(window, cx).placeholder("Unix seconds"));
        let snippet_name = cx.new(|cx| InputState::new(window, cx).placeholder("Snippet name"));
        let snippet_command = cx.new(|cx| InputState::new(window, cx).placeholder("uptime"));
        let keyword_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("ERROR or timeout"));
        let trigger_keyword_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("ERROR or timeout"));
        let trigger_text_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("preset command, for example: systemctl status ssh")
        });
        let (settings_store, settings, settings_message) = match SettingsStore::discover() {
            Ok(store) => match store.load() {
                Ok(settings) => (Some(store), settings, String::new()),
                Err(error) => (Some(store), AppSettings::default(), error),
            },
            Err(error) => (None, AppSettings::default(), error),
        };
        let collapsed_groups = settings.collapsed_groups.iter().cloned().collect();
        ui::theme::install(cx, settings.ui_theme);
        let settings_font_family =
            cx.new(|cx| InputState::new(window, cx).default_value(settings.font_family.clone()));
        let settings_font_size =
            cx.new(|cx| InputState::new(window, cx).default_value(settings.font_size.to_string()));
        let settings_alert_cpu = cx.new(|cx| {
            InputState::new(window, cx).default_value(settings.alert_cpu_percent.to_string())
        });
        let settings_alert_memory = cx.new(|cx| {
            InputState::new(window, cx).default_value(settings.alert_memory_percent.to_string())
        });
        let settings_alert_disk = cx.new(|cx| {
            InputState::new(window, cx).default_value(settings.alert_disk_percent.to_string())
        });
        let auth_prompt_inputs: Vec<Entity<InputState>> = (0..MAX_AUTH_PROMPTS)
            .map(|index| {
                let input = cx.new(|cx| {
                    InputState::new(window, cx).placeholder(format!("Response {}", index + 1))
                });
                input.update(cx, |state, cx| state.set_masked(true, window, cx));
                input
            })
            .collect();
        let auth_prompt_plain_inputs: Vec<Entity<InputState>> = (0..MAX_AUTH_PROMPTS)
            .map(|index| {
                cx.new(|cx| {
                    InputState::new(window, cx).placeholder(format!("Response {}", index + 1))
                })
            })
            .collect();
        let shortcut_inputs: Vec<Entity<InputState>> = ShortcutAction::ALL
            .iter()
            .map(|action| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(settings.shortcuts.shortcut(*action).to_owned())
                })
            })
            .collect();
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search terminal"));
        let compose_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line()
                .rows(3)
                .placeholder("Compose a multi-line command, then press Ctrl+Enter")
        });
        let default_key_path = home_directory()
            .join(".ssh")
            .join("dshell_ed25519")
            .display()
            .to_string();
        let key_path = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(default_key_path)
                .placeholder("Path for the new private key")
        });
        let new_key_passphrase =
            cx.new(|cx| InputState::new(window, cx).placeholder("Optional passphrase"));
        new_key_passphrase.update(cx, |state, cx| state.set_masked(true, window, cx));
        let key_comment = cx.new(|cx| InputState::new(window, cx).default_value("dshell"));
        let key_change_old_passphrase =
            cx.new(|cx| InputState::new(window, cx).placeholder("Current passphrase"));
        key_change_old_passphrase.update(cx, |state, cx| state.set_masked(true, window, cx));
        let key_change_new_passphrase =
            cx.new(|cx| InputState::new(window, cx).placeholder("New passphrase (optional)"));
        key_change_new_passphrase.update(cx, |state, cx| state.set_masked(true, window, cx));
        let host = cx.new(|cx| InputState::new(window, cx).placeholder("server.example.com"));
        let port = cx.new(|cx| InputState::new(window, cx).default_value("22"));
        let username = cx.new(|cx| InputState::new(window, cx).placeholder("user"));
        let password = cx.new(|cx| InputState::new(window, cx).placeholder("Password"));
        password.update(cx, |state, cx| state.set_masked(true, window, cx));
        let terminal_size =
            Self::size_for_window(window, &settings, settings.sidebar_collapsed, false);
        let (profile_store, data, profiles_ready, profile_message) = match ProfileStore::discover()
        {
            Ok(store) => match store.load() {
                Ok(data) => (Some(store), data, true, String::new()),
                Err(error) => (Some(store), HostData::default(), false, error),
            },
            Err(error) => (None, HostData::default(), false, error),
        };
        let (snippet_store, snippets, snippet_message) = match SnippetStore::discover() {
            Ok(store) => match store.load() {
                Ok(snippets) => (Some(store), snippets, String::new()),
                Err(error) => (Some(store), Vec::new(), error),
            },
            Err(error) => (None, Vec::new(), error),
        };
        let bounds_subscription = cx.observe_window_bounds(window, |this, window, cx| {
            this.resize_terminal(window, cx);
        });
        let side_by_side_state = ResizableState::new(cx);
        let stacked_state = ResizableState::new(cx);
        let side_by_side_subscription = cx.observe(&side_by_side_state, |this, _, cx| {
            this.resize_sessions(cx);
        });
        let stacked_subscription = cx.observe(&stacked_state, |this, _, cx| {
            this.resize_sessions(cx);
        });
        let auth_profile_name =
            cx.new(|cx| InputState::new(window, cx).placeholder("Template name"));
        let (auth_profile_store, auth_profiles, auth_profile_message) =
            match AuthProfileStore::discover() {
                Ok(store) => match store.load() {
                    Ok(profiles) => (Some(store), profiles, String::new()),
                    Err(error) => (Some(store), Vec::new(), error),
                },
                Err(error) => (None, Vec::new(), error),
            };

        let mut view = Self {
            ui_theme: settings.ui_theme,
            sidebar_collapsed: settings.sidebar_collapsed,
            inspector_visible: false,
            inspector_transfers_open: true,
            inspector_forwards_open: true,
            inspector_info_open: true,
            inspector_server_open: true,
            inspector_processes_open: true,
            inspector_network_open: true,
            inspector_storage_open: true,
            inspector_listeners_open: true,
            pending_process_termination: None,
            inspector_services_open: true,
            pending_service_action: None,
            pending_power_action: None,
            inspector_log_open: true,
            profile_name,
            profile_search,
            profile_group,
            profile_favorite: false,
            auth_method: AuthMethod::Password,
            auth_profiles,
            auth_profile_store,
            auth_profile_name,
            auth_profile_message,
            applied_auth_profile: None,
            identity_file,
            key_passphrase,
            jump_profile,
            jump_password,
            jump_passphrase,
            keepalive_seconds,
            forward_kind: ForwardKind::Local,
            forward_listen_host,
            forward_listen_port,
            forward_target_host,
            forward_target_port,
            sftp_name,
            sftp_permissions,
            sftp_atime,
            sftp_mtime,
            host,
            port,
            username,
            password,
            status: "Ready".to_owned(),
            sessions: Vec::new(),
            active_session_id: None,
            split: None,
            next_session_id: 1,
            terminal_size,
            terminal_bounds: Rc::default(),
            terminal_focus: cx.focus_handle(),
            overlay_focus: cx.focus_handle(),
            overlay_focus_pending: false,
            toast: None,
            toast_generation: 0,
            layout_drag: None,
            side_by_side_state,
            stacked_state,
            profiles: data.profiles,
            recent: data.recent,
            collapsed_groups,
            group_editor_name,
            group_editor_open: false,
            group_editor_mode: GroupEditorMode::Create,
            group_editor_source: None,
            group_editor_message: String::new(),
            profile_store,
            profiles_ready,
            selected_profile: None,
            session_editor_open: false,
            credential_prompt_open: false,
            credential_prompt_session_id: None,
            session_editor_section: SessionEditorSection::Connection,
            confirm_delete: false,
            profile_message,
            credential_focus_pending: false,
            credential_cache: HashMap::new(),
            pending_credentials: HashMap::new(),
            snippets,
            snippet_store,
            snippet_name,
            snippet_command,
            snippet_send_enter: true,
            editing_snippet: None,
            snippet_message,
            snippets_visible: false,
            snippet_targets: HashSet::new(),
            keyword_input,
            keyword_highlights: Vec::new(),
            keyword_highlighting_enabled: false,
            keyword_message: String::new(),
            keywords_visible: false,
            trigger_keyword_input,
            trigger_text_input,
            trigger_rules: Vec::new(),
            trigger_action_kind: TriggerActionKind::Notify,
            trigger_send_enter: false,
            trigger_engine_enabled: false,
            trigger_message: String::new(),
            triggers_visible: false,
            trigger_latches: HashMap::new(),
            settings_theme: settings.theme,
            settings_cursor: settings.cursor_style,
            settings_language: settings.language,
            shortcut_inputs,
            settings,
            settings_store,
            settings_font_family,
            settings_font_size,
            settings_alert_cpu,
            settings_alert_memory,
            settings_alert_disk,
            settings_message,
            settings_visible: false,
            keys_visible: false,
            key_path,
            new_key_passphrase,
            key_comment,
            key_message: String::new(),
            key_public: None,
            key_algorithm: keygen::KeyAlgorithm::default(),
            existing_keys: Vec::new(),
            key_discovery_message: String::new(),
            key_change_path: None,
            key_change_old_passphrase,
            key_change_new_passphrase,
            key_change_message: String::new(),
            key_change_visible: false,
            key_delete_path: None,
            key_delete_message: String::new(),
            key_delete_visible: false,
            auth_prompt: None,
            auth_prompt_inputs,
            auth_prompt_plain_inputs,
            auth_prompt_needs_clear: false,
            log_replay: None,
            search_input,
            compose_input,
            compose_visible: false,
            compose_history: Vec::new(),
            compose_recall: 0,
            sync_input_enabled: false,
            sync_targets: HashSet::new(),
            sync_visible: false,
            menu_visible: false,
            layout_menu_visible: false,
            tab_menu_session: None,
            search_visible: false,
            search_message: String::new(),
            search_target: None,
            _bounds_subscription: bounds_subscription,
            _side_by_side_subscription: side_by_side_subscription,
            _stacked_subscription: stacked_subscription,
        };
        view.set_language_placeholders(window, cx);
        view.refresh_existing_keys();
        view
    }

    fn tokens(&self) -> ThemeTokens {
        self.ui_theme.tokens()
    }

    fn set_language_placeholders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholders = [
            (&self.profile_name, "Production"),
            (&self.profile_search, "Search saved hosts"),
            (&self.profile_group, "Ungrouped"),
            (&self.group_editor_name, "Group name"),
            (&self.key_passphrase, "Key passphrase"),
            (&self.jump_profile, "Saved jump host name"),
            (&self.jump_password, "Jump host password"),
            (&self.jump_passphrase, "Jump key passphrase"),
            (&self.snippet_name, "Snippet name"),
            (&self.keyword_input, "ERROR or timeout"),
            (&self.trigger_keyword_input, "ERROR or timeout"),
            (
                &self.trigger_text_input,
                "preset command, for example: systemctl status ssh",
            ),
            (&self.search_input, "Search terminal"),
            (&self.key_path, "Path for the new private key"),
            (&self.new_key_passphrase, "Optional passphrase"),
            (&self.key_change_old_passphrase, "Current passphrase"),
            (&self.key_change_new_passphrase, "New passphrase (optional)"),
            (&self.host, "server.example.com"),
            (&self.username, "user"),
            (&self.password, "Password"),
            (&self.auth_profile_name, "Template name"),
        ];
        for (input, placeholder) in placeholders {
            let localized = self.tr(placeholder);
            input.update(cx, |state, cx| state.set_placeholder(localized, window, cx));
        }
        for inputs in [&self.auth_prompt_inputs, &self.auth_prompt_plain_inputs] {
            for (index, input) in inputs.iter().enumerate() {
                let localized = format!("{} {}", self.tr("Response"), index + 1);
                input.update(cx, |state, cx| state.set_placeholder(localized, window, cx));
            }
        }
        for (input, placeholder) in [
            (&self.sftp_name, "name"),
            (&self.sftp_atime, "Unix seconds"),
            (&self.sftp_mtime, "Unix seconds"),
        ] {
            let localized = self.tr(placeholder);
            input.update(cx, |state, cx| state.set_placeholder(localized, window, cx));
        }
        let compose_placeholder = self.tr("Compose a multi-line command, then press Ctrl+Enter");
        self.compose_input.update(cx, |state, cx| {
            state.set_placeholder(compose_placeholder, window, cx)
        });
    }

    fn request_overlay_focus(&mut self) {
        self.overlay_focus_pending = true;
    }

    fn begin_layout_drag(
        &mut self,
        target: LayoutDragTarget,
        position_x: f32,
        cx: &mut Context<Self>,
    ) {
        let start_width = match target {
            LayoutDragTarget::Sidebar => self.settings.sidebar_width,
            LayoutDragTarget::Inspector => self.settings.inspector_width,
        };
        self.layout_drag = Some(LayoutDrag {
            target,
            start_x: position_x,
            start_width,
        });
        cx.notify();
    }

    fn update_layout_drag(&mut self, position_x: f32, cx: &mut Context<Self>) {
        let Some(drag) = self.layout_drag else {
            return;
        };
        let delta = position_x - drag.start_x;
        match drag.target {
            LayoutDragTarget::Sidebar => {
                self.settings.sidebar_width =
                    dragged_panel_width(drag.target, drag.start_width, delta);
            }
            LayoutDragTarget::Inspector => {
                self.settings.inspector_width =
                    dragged_panel_width(drag.target, drag.start_width, delta);
            }
        }
        cx.notify();
    }

    fn finish_layout_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.layout_drag.take().is_some() {
            self.persist_ui_preferences(cx);
            self.resize_terminal(window, cx);
        }
    }

    fn layout_divider(
        &mut self,
        id: &'static str,
        target: LayoutDragTarget,
        tokens: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .occlude()
            .flex_shrink_0()
            .w(px(LAYOUT_DIVIDER_PX))
            .h_full()
            .cursor_col_resize()
            .bg(tokens.border_subtle.opacity(0.45))
            .hover(|style| style.bg(tokens.accent.opacity(0.6)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.begin_layout_drag(target, event.position.x.as_f32(), cx);
                }),
            )
            .into_any_element()
    }

    fn dismiss_toast(&mut self, cx: &mut Context<Self>) {
        self.toast = None;
        self.toast_generation = self.toast_generation.wrapping_add(1);
        cx.notify();
    }

    fn show_toast(&mut self, message: impl Into<String>, tone: Tone, cx: &mut Context<Self>) {
        self.toast = Some(ToastState {
            message: message.into(),
            tone,
        });
        self.toast_generation = self.toast_generation.wrapping_add(1);
        let generation = self.toast_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(4)).await;
            let _ = this.update(cx, |view, cx| {
                if view.toast_generation == generation {
                    view.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn toast_overlay(&mut self, toast: ToastState, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        div()
            .absolute()
            .bottom(px(ui_tokens::STATUS_BAR_HEIGHT + ui_tokens::SPACE_3))
            .right(px(ui_tokens::SPACE_4))
            .occlude()
            .child(
                components::toast(&tokens, toast.tone, toast.message).child(
                    components::icon_button(
                        &tokens,
                        "toast-close",
                        IconName::Close,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        "Dismiss",
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.dismiss_toast(cx))),
                ),
            )
            .into_any_element()
    }

    fn session(&self, id: u64) -> Option<&SshSession> {
        self.sessions.iter().find(|session| session.id == id)
    }

    fn session_mut(&mut self, id: u64) -> Option<&mut SshSession> {
        self.sessions.iter_mut().find(|session| session.id == id)
    }

    fn active_session(&self) -> Option<&SshSession> {
        self.active_session_id.and_then(|id| self.session(id))
    }

    fn session_for_target(&self, host: &str, port: u16, username: &str) -> Option<u64> {
        self.sessions
            .iter()
            .find(|session| session_matches_target(session, host, port, username))
            .map(|session| session.id)
    }

    fn saved_jump_profile(&self, name: &str) -> Option<&HostProfile> {
        self.profiles
            .iter()
            .find(|profile| profile.name.eq_ignore_ascii_case(name.trim()))
    }

    fn profile_needs_credential_prompt(&self, profile: &HostProfile) -> bool {
        let target_needs = matches!(
            profile.auth_method,
            AuthMethod::Password | AuthMethod::KeyboardInteractive
        ) && profile.password.is_empty();
        let jump_needs = self
            .saved_jump_profile(&profile.proxy_jump)
            .is_some_and(|jump| {
                matches!(
                    jump.auth_method,
                    AuthMethod::Password | AuthMethod::KeyboardInteractive
                ) && profile.jump_password.is_empty()
                    && jump.password.is_empty()
            });
        target_needs || jump_needs
    }

    fn apply_cached_credentials(
        &self,
        credentials: &CachedCredentials,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Self::set_input_value(&self.password, credentials.password.as_str(), window, cx);
        Self::set_input_value(
            &self.key_passphrase,
            credentials.passphrase.as_str(),
            window,
            cx,
        );
        Self::set_input_value(
            &self.jump_password,
            credentials.jump_password.as_str(),
            window,
            cx,
        );
        Self::set_input_value(
            &self.jump_passphrase,
            credentials.jump_passphrase.as_str(),
            window,
            cx,
        );
    }

    fn open_credential_prompt(&mut self, message: String, cx: &mut Context<Self>) {
        self.session_editor_open = false;
        self.credential_prompt_open = true;
        self.credential_prompt_session_id = None;
        self.session_editor_section = SessionEditorSection::Authentication;
        self.confirm_delete = false;
        self.profile_message = message;
        self.overlay_focus_pending = true;
        let jump_name = self.jump_profile.read(cx).value();
        self.credential_focus_pending = self.auth_method != AuthMethod::Agent
            || self
                .saved_jump_profile(&jump_name)
                .is_some_and(|profile| profile.auth_method != AuthMethod::Agent);
        cx.notify();
    }

    fn reopen_authentication_after_failure(
        &mut self,
        id: u64,
        error: String,
        cx: &mut Context<Self>,
    ) {
        if !is_authentication_failure(&error) {
            return;
        }
        let credential_key = self
            .session(id)
            .and_then(|session| session.credential_key.clone());
        self.pending_credentials.remove(&id);
        if let Some(key) = credential_key {
            self.credential_cache.remove(&key);
        }
        let profile_name = self
            .session(id)
            .and_then(|session| session.profile_name.clone());
        self.selected_profile = profile_name;
        self.credential_prompt_session_id = Some(id);
        self.profile_message = self.tr(&error);
        self.session_editor_section = SessionEditorSection::Authentication;
        self.session_editor_open = false;
        self.credential_prompt_open = true;
        self.confirm_delete = false;
        self.applied_auth_profile = None;
        self.overlay_focus_pending = true;
        let jump_name = self.jump_profile.read(cx).value();
        self.credential_focus_pending = self.auth_method != AuthMethod::Agent
            || self
                .saved_jump_profile(&jump_name)
                .is_some_and(|profile| profile.auth_method != AuthMethod::Agent);
        cx.notify();
    }

    fn activate_recent_connection(
        &mut self,
        entry: &RecentConnection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.session_for_target(&entry.host, entry.port, &entry.username) {
            self.session_editor_open = false;
            self.select_session(id, window, cx);
        } else {
            self.load_recent(entry, window, cx);
        }
    }

    fn connect_saved_profile(
        &mut self,
        profile: &HostProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.session_for_target(&profile.host, profile.port, &profile.username) {
            self.selected_profile = Some(profile.name.clone());
            self.session_editor_open = false;
            self.select_session(id, window, cx);
            return;
        }

        self.load_profile(profile, window, cx);
        let key = CredentialCacheKey::from_profile(profile);
        if let Some(credentials) = self.credential_cache.get(&key).cloned() {
            self.apply_cached_credentials(&credentials, window, cx);
            self.connect(window, cx);
        } else if self.profile_needs_credential_prompt(profile) {
            let message = self
                .tr("Enter credentials for {name}. They will be saved with this host profile after a successful connection.")
                .replace("{name}", &profile.name);
            self.open_credential_prompt(message, cx);
        } else {
            self.connect(window, cx);
        }
    }

    fn size_for_window(
        window: &Window,
        settings: &AppSettings,
        sidebar_collapsed: bool,
        inspector_visible: bool,
    ) -> TerminalSize {
        let bounds = window.bounds();
        terminal_size_for_viewport(
            bounds.size.width / px(1.0),
            bounds.size.height / px(1.0),
            settings,
            sidebar_collapsed,
            inspector_visible,
        )
    }

    fn resize_terminal(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.terminal_size = Self::size_for_window(
            window,
            &self.settings,
            self.sidebar_collapsed,
            self.inspector_visible,
        );
        self.resize_sessions(cx);
        cx.notify();
    }

    fn resize_sessions(&mut self, cx: &mut Context<Self>) {
        let split_sizes = self.split.map(|split| {
            let state = match split.orientation {
                SplitOrientation::SideBySide => &self.side_by_side_state,
                SplitOrientation::Stacked => &self.stacked_state,
            };
            let panel_sizes = state.read(cx).sizes();
            let first = split_panel_terminal_size(
                self.terminal_size,
                split.orientation,
                panel_sizes.first().map(|size| size.as_f32()),
                0,
                self.settings.cell_width(),
                self.settings.cell_height(),
            );
            let second = split_panel_terminal_size(
                self.terminal_size,
                split.orientation,
                panel_sizes.get(1).map(|size| size.as_f32()),
                1,
                self.settings.cell_width(),
                self.settings.cell_height(),
            );
            (split, first, second)
        });

        let mut resize_session_ids = Vec::new();
        let mut changed = false;
        for session in &mut self.sessions {
            let size = session_terminal_size_for_split(session.id, self.terminal_size, split_sizes);
            if session.terminal_size == size {
                continue;
            }
            session.terminal_size = size;
            changed = true;
            if session.command_tx.is_some() {
                resize_session_ids.push((session.id, size));
            }
        }
        for (id, size) in resize_session_ids {
            if let Err(error) =
                enqueue_session_request(&self.sessions, id, TerminalRequest::Resize(size))
                && let Some(session) = self.session_mut(id)
            {
                session.status = format!("Terminal resize queue is unavailable: {error}");
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
    }

    fn set_split(
        &mut self,
        second_session_id: u64,
        orientation: SplitOrientation,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(first_session_id) = self.active_session_id else {
            return;
        };
        if first_session_id == second_session_id || self.session(second_session_id).is_none() {
            return;
        }
        self.split = Some(SessionSplit {
            first_session_id,
            second_session_id,
            orientation,
        });
        self.terminal_bounds.borrow_mut().clear();
        self.resize_terminal(window, cx);
    }

    fn set_split_orientation(
        &mut self,
        orientation: SplitOrientation,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(split) = &mut self.split else {
            return;
        };
        if split.orientation == orientation {
            return;
        }
        split.orientation = orientation;
        self.terminal_bounds.borrow_mut().clear();
        self.resize_terminal(window, cx);
    }

    fn close_split(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.split.take().is_none() {
            return;
        }
        self.terminal_bounds.borrow_mut().clear();
        self.resize_terminal(window, cx);
    }

    fn set_input_value(
        input: &Entity<InputState>,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        input.update(cx, |state, cx| {
            state.set_value(value.to_owned(), window, cx)
        });
    }

    fn open_search(&mut self, session_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.search_target = Some(if self.log_replay.is_some() {
            SearchTarget::LogReplay
        } else {
            SearchTarget::Session(session_id)
        });
        self.search_visible = true;
        self.search_message.clear();
        let focus = self.search_input.read(cx).focus_handle(cx);
        window.focus(&focus);
        cx.notify();
    }

    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_visible = false;
        self.search_target = None;
        self.search_message.clear();
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn run_search(&mut self, direction: SearchDirection, cx: &mut Context<Self>) {
        let query = self.search_input.read(cx).value().trim().to_owned();
        if query.is_empty() {
            self.search_message = "Enter a search term.".to_owned();
            cx.notify();
            return;
        }
        match self.search_target {
            Some(SearchTarget::Session(session_id)) => {
                if self.session(session_id).is_none() {
                    self.search_message = "The session is no longer available.".to_owned();
                } else {
                    self.search_message = "Searching...".to_owned();
                    self.send_terminal_request(
                        session_id,
                        TerminalRequest::Search { query, direction },
                        "Terminal search",
                        cx,
                    );
                }
            }
            Some(SearchTarget::LogReplay) => {
                if let Some(replay) = self.log_replay.as_mut() {
                    let result = replay.terminal.search(&query, direction);
                    replay.frame = replay.terminal.frame();
                    self.search_message = search_result_message(&result);
                } else {
                    self.search_message = "Log replay is closed.".to_owned();
                }
            }
            None => {
                self.search_message = "No terminal to search.".to_owned();
            }
        }
        cx.notify();
    }

    fn search_bar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let message = self.search_message.clone();
        div()
            .id("terminal-search")
            .flex()
            .flex_col()
            .gap(px(ui_tokens::SPACE_1))
            .p(px(ui_tokens::SPACE_2))
            .w_full()
            .border_1()
            .border_color(tokens.border_strong)
            .rounded(px(ui_tokens::RADIUS_CARD))
            .shadow_lg()
            .bg(tokens.surface_overlay)
            .text_color(tokens.text_primary)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.to_ascii_lowercase();
                if key == "escape" {
                    cx.stop_propagation();
                    this.close_search(window, cx);
                } else if key == "enter" {
                    cx.stop_propagation();
                    let direction = if event.keystroke.modifiers.shift {
                        SearchDirection::Backward
                    } else {
                        SearchDirection::Forward
                    };
                    this.run_search(direction, cx);
                }
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(ui_tokens::SPACE_1))
                    .child(div().flex_1().min_w_0().child(
                        TextInput::new(&self.search_input).h(px(ui_tokens::CONTROL_HEIGHT_SM)),
                    ))
                    .child(
                        components::button(
                            &tokens,
                            "search-previous",
                            "Prev",
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.run_search(SearchDirection::Backward, cx);
                        })),
                    )
                    .child(
                        components::button(
                            &tokens,
                            "search-next",
                            "Next",
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.run_search(SearchDirection::Forward, cx);
                        })),
                    )
                    .child(
                        components::icon_button(
                            &tokens,
                            "search-close",
                            IconName::Close,
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                            "Close search",
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_search(window, cx);
                        })),
                    ),
            )
            .when(!message.is_empty(), |bar| {
                bar.child(components::help_text(&tokens, message))
            })
            .into_any_element()
    }

    fn begin_log_replay(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open session log".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(mut paths))) = receiver.await
                && let Some(path) = paths.pop()
            {
                let read_path = path.clone();
                let read = cx
                    .background_spawn(
                        async move { log::read_replay(&read_path, MAX_LOG_REPLAY_BYTES) },
                    )
                    .await;
                let _ = this.update(cx, |view, cx| view.apply_log_replay(path, read, cx));
            }
        })
        .detach();
    }

    fn apply_log_replay(
        &mut self,
        path: PathBuf,
        read: Result<log::ReplayLog, String>,
        cx: &mut Context<Self>,
    ) {
        let replay_size = TerminalSize {
            columns: (1000.0 / self.settings.cell_width())
                .floor()
                .clamp(20.0, 240.0) as u16,
            rows: (620.0 / self.settings.cell_height())
                .floor()
                .clamp(5.0, 120.0) as u16,
        };
        let mut terminal = TerminalEmulator::new_with_theme(replay_size, self.settings.theme);
        terminal.set_cursor_style(self.settings.cursor_style);
        let mut bytes = Vec::new();
        let mut timeline = Vec::new();
        let mut timestamped = false;
        let message = match read {
            Ok(replay) => {
                timestamped = replay.timestamped;
                terminal.feed(&replay.bytes);
                let size = if replay.truncated {
                    format!(
                        "{} bytes replayed from the end of the file (limit {} MiB)",
                        replay.bytes.len(),
                        MAX_LOG_REPLAY_BYTES / (1024 * 1024)
                    )
                } else {
                    format!("{} bytes replayed", replay.bytes.len())
                };
                let detail = if replay.timestamped {
                    format!("{} timestamped record(s)", replay.timeline.len())
                } else {
                    "legacy raw log without a timeline".to_owned()
                };
                bytes = replay.bytes;
                timeline = replay.timeline;
                format!("{size}; {detail}.")
            }
            Err(error) => format!("Could not read the log: {error}"),
        };
        let frame = terminal.frame();
        let timeline_index = timeline.len().checked_sub(1);
        self.log_replay = Some(LogReplayState {
            path,
            terminal,
            frame,
            message,
            scroll_remainder: 0.0,
            replay_size,
            bytes,
            timeline,
            timeline_index,
            timestamped,
        });
        self.request_overlay_focus();
        cx.notify();
    }

    fn jump_log_replay_timeline(&mut self, index: usize, cx: &mut Context<Self>) {
        let theme = self.settings.theme;
        let cursor_style = self.settings.cursor_style;
        let Some(replay) = self.log_replay.as_mut() else {
            return;
        };
        let Some(entry) = replay.timeline.get(index) else {
            return;
        };
        let (offset, length, timestamp) = (entry.offset, entry.length, entry.timestamp_ms);
        let end = offset.saturating_add(length).min(replay.bytes.len());
        let first_timestamp = replay
            .timeline
            .first()
            .map_or(timestamp, |entry| entry.timestamp_ms);
        let mut terminal = TerminalEmulator::new_with_theme(replay.replay_size, theme);
        terminal.set_cursor_style(cursor_style);
        terminal.feed(&replay.bytes[..end]);
        replay.frame = terminal.frame();
        replay.terminal = terminal;
        replay.timeline_index = Some(index);
        replay.message = format!(
            "Timeline {}/{} · {} · {} bytes replayed.",
            index + 1,
            replay.timeline.len(),
            format_log_time(timestamp.saturating_sub(first_timestamp)),
            end
        );
        cx.notify();
    }

    fn step_log_replay_timeline(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(replay) = self.log_replay.as_ref() else {
            return;
        };
        if replay.timeline.is_empty() {
            return;
        }
        let last = replay.timeline.len() - 1;
        let current = replay.timeline_index.unwrap_or(last);
        let next = if forward {
            (current + 1).min(last)
        } else {
            current.saturating_sub(1)
        };
        self.jump_log_replay_timeline(next, cx);
    }

    fn scroll_log_replay(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let cell_height = self.settings.cell_height();
        let delta_y = match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => delta.y / px(cell_height),
        };
        let Some(replay) = self.log_replay.as_mut() else {
            return;
        };
        let accumulated = replay.scroll_remainder - delta_y;
        let rows = accumulated.trunc() as i32;
        replay.scroll_remainder = accumulated - rows as f32;
        if rows == 0 {
            return;
        }
        replay.terminal.scroll_display(rows);
        replay.frame = replay.terminal.frame();
        cx.stop_propagation();
        cx.notify();
    }

    fn scroll_log_replay_to(&mut self, top: bool, cx: &mut Context<Self>) {
        if let Some(replay) = self.log_replay.as_mut() {
            replay
                .terminal
                .scroll_display(if top { 1_000_000 } else { -1_000_000 });
            replay.frame = replay.terminal.frame();
            cx.notify();
        }
    }

    fn close_log_replay(&mut self, cx: &mut Context<Self>) {
        self.log_replay = None;
        if self.search_target == Some(SearchTarget::LogReplay) {
            self.search_visible = false;
            self.search_target = None;
        }
        cx.notify();
    }

    fn log_replay_overlay(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let Some(replay) = self.log_replay.as_ref() else {
            return div().into_any_element();
        };
        let title = format!(
            "Log replay{}: {}",
            if replay.timestamped {
                " (timeline)"
            } else {
                ""
            },
            replay.path.file_name().map_or_else(
                || replay.path.display().to_string(),
                |name| name.to_string_lossy().into_owned()
            )
        );
        let message = replay.message.clone();
        let frame = replay.frame.clone();
        let palette = self.settings.theme.palette();
        let mut keyword_highlights = if self.keyword_highlighting_enabled {
            self.keyword_highlights.clone()
        } else {
            Vec::new()
        };
        for keyword in self.trigger_highlight_keywords() {
            if !keyword_highlights
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&keyword))
            {
                keyword_highlights.push(keyword);
            }
        }
        let has_timeline = !replay.timeline.is_empty();
        let timeline_label = replay.timeline_index.and_then(|index| {
            replay.timeline.get(index).map(|entry| {
                let first = replay
                    .timeline
                    .first()
                    .map_or(entry.timestamp_ms, |entry| entry.timestamp_ms);
                format!(
                    "{} · {}/{}",
                    format_log_time(entry.timestamp_ms.saturating_sub(first)),
                    index + 1,
                    replay.timeline.len()
                )
            })
        });
        let search_overlay =
            if self.search_visible && self.search_target == Some(SearchTarget::LogReplay) {
                Some(
                    div()
                        .absolute()
                        .top(px(ui_tokens::MODAL_HEADER_HEIGHT + ui_tokens::SPACE_2))
                        .left(px(ui_tokens::SPACE_4))
                        .right(px(ui_tokens::SPACE_4))
                        .child(self.search_bar(cx)),
                )
            } else {
                None
            };
        components::modal_overlay(&tokens, "log-replay-overlay")
            .track_focus(&self.overlay_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    if this.search_visible && this.search_target == Some(SearchTarget::LogReplay) {
                        this.close_search(window, cx);
                    } else {
                        this.close_log_replay(cx);
                        window.focus(&this.terminal_focus);
                    }
                }
            }))
            .child(
                components::modal_panel(&tokens, "log-replay")
                    .relative()
                    .w_full()
                    .h_full()
                    .max_w(px(1100.))
                    .max_h(px(760.))
                    .child(
                        components::modal_header(&tokens, title).child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(ui_tokens::SPACE_1))
                                .when(has_timeline, |actions| {
                                    actions
                                        .child(
                                            components::button(
                                                &tokens,
                                                "log-replay-timeline-prev",
                                                "Prev event",
                                                ButtonKind::Ghost,
                                                ButtonSize::Small,
                                            )
                                            .on_click(
                                                cx.listener(|this, _, _, cx| {
                                                    this.step_log_replay_timeline(false, cx);
                                                }),
                                            ),
                                        )
                                        .child(
                                            components::button(
                                                &tokens,
                                                "log-replay-timeline-next",
                                                "Next event",
                                                ButtonKind::Ghost,
                                                ButtonSize::Small,
                                            )
                                            .on_click(
                                                cx.listener(|this, _, _, cx| {
                                                    this.step_log_replay_timeline(true, cx);
                                                }),
                                            ),
                                        )
                                        .when_some(timeline_label.clone(), |actions, label| {
                                            actions.child(components::help_text(&tokens, label))
                                        })
                                })
                                .child(
                                    components::button(
                                        &tokens,
                                        "log-replay-find",
                                        "Find",
                                        ButtonKind::Ghost,
                                        ButtonSize::Small,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, window, cx| {
                                            this.search_target = Some(SearchTarget::LogReplay);
                                            this.search_visible = true;
                                            let focus = this.search_input.read(cx).focus_handle(cx);
                                            window.focus(&focus);
                                            cx.notify();
                                        },
                                    )),
                                )
                                .child(
                                    components::button(
                                        &tokens,
                                        "log-replay-top",
                                        "Top",
                                        ButtonKind::Ghost,
                                        ButtonSize::Small,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.scroll_log_replay_to(true, cx);
                                        },
                                    )),
                                )
                                .child(
                                    components::button(
                                        &tokens,
                                        "log-replay-bottom",
                                        "Bottom",
                                        ButtonKind::Ghost,
                                        ButtonSize::Small,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.scroll_log_replay_to(false, cx);
                                        },
                                    )),
                                )
                                .child(
                                    components::button(
                                        &tokens,
                                        "log-replay-close",
                                        "Close",
                                        ButtonKind::Secondary,
                                        ButtonSize::Small,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, window, cx| {
                                            this.close_log_replay(cx);
                                            window.focus(&this.terminal_focus);
                                        },
                                    )),
                                ),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .bg(rgb(palette.background))
                            .on_scroll_wheel(cx.listener(|this, event, _, cx| {
                                this.scroll_log_replay(event, cx);
                            }))
                            .child(render_terminal_grid(
                                &frame,
                                &self.settings,
                                palette,
                                &keyword_highlights,
                            )),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .px(px(ui_tokens::SPACE_3))
                            .py(px(ui_tokens::SPACE_1))
                            .border_t_1()
                            .border_color(tokens.border_subtle)
                            .bg(tokens.surface)
                            .child(components::help_text(&tokens, message)),
                    )
                    .when(search_overlay.is_some(), |panel| {
                        panel.child(search_overlay.expect("search overlay is present"))
                    }),
            )
            .into_any_element()
    }

    fn begin_config_import(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import OpenSSH config".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(mut paths))) = receiver.await
                && let Some(path) = paths.pop()
            {
                let _ = this.update(cx, |view, cx| view.import_config(&path, cx));
            }
        })
        .detach();
    }

    fn begin_profile_export(&mut self, cx: &mut Context<Self>) {
        if !self.profiles_ready {
            self.profile_message = self.tr("Host profiles are unavailable for export.");
            self.show_toast(self.profile_message.clone(), Tone::Danger, cx);
            return;
        }
        let directory = home_directory();
        let receiver = cx.prompt_for_new_path(&directory, Some("dshell-hosts.json"));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = receiver.await {
                let _ = this.update(cx, |view, cx| view.export_profiles(&path, cx));
            }
        })
        .detach();
    }

    fn export_profiles(&mut self, path: &Path, cx: &mut Context<Self>) {
        if let Err(error) = export_host_profiles(path, &self.profiles) {
            self.profile_message = error;
            self.show_toast(self.tr("Could not export host profiles."), Tone::Danger, cx);
            return;
        }
        self.profile_message = self
            .tr("Exported {count} host profile(s) to {path}.")
            .replace("{count}", &self.profiles.len().to_string())
            .replace("{path}", &path.display().to_string());
        self.show_toast(self.profile_message.clone(), Tone::Success, cx);
    }

    fn begin_profile_import(&mut self, cx: &mut Context<Self>) {
        let prompt: SharedString = self.tr("Import dshell host profiles").into();
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(prompt),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(mut paths))) = receiver.await
                && let Some(path) = paths.pop()
            {
                let _ = this.update(cx, |view, cx| view.import_host_profiles(&path, cx));
            }
        })
        .detach();
    }

    fn import_host_profiles(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !self.profiles_ready {
            self.profile_message = self
                .tr("Host profiles could not be loaded; fix the profile file before importing.");
            self.show_toast(self.profile_message.clone(), Tone::Danger, cx);
            return;
        }
        let imported = match import_host_profiles(path) {
            Ok(imported) => imported,
            Err(error) => {
                self.profile_message = error;
                self.show_toast(self.tr("Could not import host profiles."), Tone::Danger, cx);
                return;
            }
        };
        let mut profiles = self.profiles.clone();
        let (added, skipped) = merge_unique_profiles(&mut profiles, imported);
        if added > 0 {
            if let Err(error) = self.save_workspace(profiles, self.recent.clone()) {
                self.profile_message = error;
                self.show_toast(self.tr("Could not import host profiles."), Tone::Danger, cx);
                return;
            }
        }
        self.profile_message = self
            .tr("Imported {added} host profile(s) from {path}; skipped {skipped} duplicate(s).")
            .replace("{added}", &added.to_string())
            .replace("{path}", &path.display().to_string())
            .replace("{skipped}", &skipped.to_string());
        let tone = if skipped > 0 {
            Tone::Warning
        } else {
            Tone::Success
        };
        self.show_toast(self.profile_message.clone(), tone, cx);
    }

    fn import_config(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !self.profiles_ready {
            self.profile_message =
                "Host profiles could not be loaded; fix the profile file before importing."
                    .to_owned();
            cx.notify();
            return;
        }
        let imported = match import_openssh_config(path) {
            Ok(imported) => imported,
            Err(error) => {
                self.profile_message = error;
                cx.notify();
                return;
            }
        };
        let mut profiles = self.profiles.clone();
        let (added, skipped) = merge_unique_profiles(&mut profiles, imported.profiles);
        if added > 0 {
            if let Err(error) = self.save_workspace(profiles, self.recent.clone()) {
                self.profile_message = error;
                cx.notify();
                return;
            }
        }
        let mut summary = format!("Imported {added} host(s) from {}", path.display());
        if skipped > 0 {
            summary.push_str(&format!("; skipped {skipped} existing name(s)"));
        }
        if !imported.warnings.is_empty() {
            let visible = imported
                .warnings
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>();
            summary.push_str(&format!("; {}", visible.join("; ")));
            if imported.warnings.len() > 3 {
                summary.push_str(&format!(
                    "; and {} more warning(s)",
                    imported.warnings.len() - 3
                ));
            }
        }
        let tone = if added == 0 || skipped > 0 || !imported.warnings.is_empty() {
            Tone::Warning
        } else {
            Tone::Success
        };
        self.profile_message = summary;
        self.show_toast(self.profile_message.clone(), tone, cx);
    }

    fn new_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        Self::set_input_value(&self.profile_name, "", window, cx);
        Self::set_input_value(&self.profile_group, "", window, cx);
        Self::set_input_value(&self.host, "", window, cx);
        Self::set_input_value(&self.port, "22", window, cx);
        Self::set_input_value(&self.username, "", window, cx);
        Self::set_input_value(&self.password, "", window, cx);
        Self::set_input_value(&self.key_passphrase, "", window, cx);
        Self::set_input_value(&self.identity_file, "", window, cx);
        Self::set_input_value(&self.jump_profile, "", window, cx);
        Self::set_input_value(&self.jump_password, "", window, cx);
        Self::set_input_value(&self.jump_passphrase, "", window, cx);
        Self::set_input_value(&self.keepalive_seconds, "0", window, cx);
        self.profile_favorite = false;
        self.auth_method = AuthMethod::Password;
        self.applied_auth_profile = None;
        self.selected_profile = None;
        self.session_editor_section = SessionEditorSection::Connection;
        self.session_editor_open = true;
        self.credential_prompt_open = false;
        self.credential_prompt_session_id = None;
        self.request_overlay_focus();
        self.confirm_delete = false;
        self.profile_message.clear();
        cx.notify();
    }

    fn load_profile(&mut self, profile: &HostProfile, window: &mut Window, cx: &mut Context<Self>) {
        Self::set_input_value(&self.profile_name, &profile.name, window, cx);
        Self::set_input_value(&self.profile_group, &profile.group, window, cx);
        Self::set_input_value(&self.host, &profile.host, window, cx);
        Self::set_input_value(&self.port, &profile.port.to_string(), window, cx);
        Self::set_input_value(&self.username, &profile.username, window, cx);
        Self::set_input_value(&self.password, &profile.password, window, cx);
        Self::set_input_value(&self.key_passphrase, &profile.passphrase, window, cx);
        Self::set_input_value(&self.identity_file, &profile.identity_file, window, cx);
        Self::set_input_value(&self.jump_profile, &profile.proxy_jump, window, cx);
        let jump_profile = self.saved_jump_profile(&profile.proxy_jump);
        let jump_password = if profile.jump_password.is_empty() {
            jump_profile.map_or("", |jump| jump.password.as_str())
        } else {
            profile.jump_password.as_str()
        };
        let jump_passphrase = if profile.jump_passphrase.is_empty() {
            jump_profile.map_or("", |jump| jump.passphrase.as_str())
        } else {
            profile.jump_passphrase.as_str()
        };
        Self::set_input_value(&self.jump_password, jump_password, window, cx);
        Self::set_input_value(&self.jump_passphrase, jump_passphrase, window, cx);
        Self::set_input_value(
            &self.keepalive_seconds,
            &profile.keepalive_seconds.to_string(),
            window,
            cx,
        );
        self.profile_favorite = profile.favorite;
        self.auth_method = profile.auth_method;
        self.applied_auth_profile = None;
        self.selected_profile = Some(profile.name.clone());
        self.session_editor_section = SessionEditorSection::Connection;
        self.session_editor_open = true;
        self.credential_prompt_open = false;
        self.credential_prompt_session_id = None;
        self.request_overlay_focus();
        self.confirm_delete = false;
        self.profile_message = self
            .tr("Loaded {name}. Saved credentials will be used when you connect.")
            .replace("{name}", &profile.name);
        cx.notify();
    }

    fn load_recent(
        &mut self,
        entry: &RecentConnection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let saved = self.profiles.iter().find(|profile| {
            profile.host.eq_ignore_ascii_case(&entry.host)
                && profile.port == entry.port
                && profile.username == entry.username
        });
        if let Some(profile) = saved.cloned() {
            self.load_profile(&profile, window, cx);
            self.profile_message = self
                .tr("Loaded {name} from recent connections.")
                .replace("{name}", &profile.name);
            cx.notify();
            return;
        }

        Self::set_input_value(&self.profile_name, &entry.name, window, cx);
        Self::set_input_value(&self.profile_group, "", window, cx);
        Self::set_input_value(&self.host, &entry.host, window, cx);
        Self::set_input_value(&self.port, &entry.port.to_string(), window, cx);
        Self::set_input_value(&self.username, &entry.username, window, cx);
        Self::set_input_value(&self.password, "", window, cx);
        Self::set_input_value(&self.key_passphrase, "", window, cx);
        Self::set_input_value(&self.identity_file, "", window, cx);
        Self::set_input_value(&self.jump_profile, "", window, cx);
        Self::set_input_value(&self.jump_password, "", window, cx);
        Self::set_input_value(&self.jump_passphrase, "", window, cx);
        Self::set_input_value(&self.keepalive_seconds, "0", window, cx);
        self.profile_favorite = false;
        self.auth_method = AuthMethod::Password;
        self.applied_auth_profile = None;
        self.selected_profile = None;
        self.session_editor_section = SessionEditorSection::Connection;
        self.session_editor_open = true;
        self.credential_prompt_open = false;
        self.credential_prompt_session_id = None;
        self.request_overlay_focus();
        self.confirm_delete = false;
        self.profile_message = self
            .tr("Loaded a recent connection. Save it as a host profile if you want to keep it.");
        cx.notify();
    }

    fn toggle_favorite(&mut self, name: &str, cx: &mut Context<Self>) {
        let mut profiles = self.profiles.clone();
        let Some(profile) = profiles
            .iter_mut()
            .find(|profile| profile.name.eq_ignore_ascii_case(name))
        else {
            return;
        };
        profile.favorite = !profile.favorite;
        let favorite = profile.favorite;
        if self.selected_profile.as_deref() == Some(name) {
            self.profile_favorite = favorite;
        }
        match self.save_workspace(profiles, self.recent.clone()) {
            Ok(()) => {
                self.profile_message = if favorite {
                    self.tr("Added {name} to favorites.")
                        .replace("{name}", name)
                } else {
                    self.tr("Removed {name} from favorites.")
                        .replace("{name}", name)
                };
            }
            Err(error) => self.profile_message = error,
        }
        cx.notify();
    }

    fn move_host_to_group(&mut self, name: &str, group: &str, cx: &mut Context<Self>) {
        let mut profiles = self.profiles.clone();
        if !move_profile_to_group(&mut profiles, name, group) {
            return;
        }
        match self.save_workspace(profiles.clone(), self.recent.clone()) {
            Ok(()) => {
                self.profiles = profiles;
                self.profile_message = if group.is_empty() {
                    self.tr("Moved {name} to Ungrouped.")
                        .replace("{name}", name)
                } else {
                    self.tr("Moved {name} to {group}.")
                        .replace("{name}", name)
                        .replace("{group}", group)
                };
                self.show_toast(self.profile_message.clone(), Tone::Success, cx);
            }
            Err(error) => {
                self.profile_message = error;
                self.show_toast(self.tr("Could not move the host."), Tone::Danger, cx);
            }
        }
    }

    fn open_group_editor(
        &mut self,
        mode: GroupEditorMode,
        group: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.group_editor_mode = mode;
        self.group_editor_source = group.map(str::to_owned);
        Self::set_input_value(
            &self.group_editor_name,
            group.unwrap_or_default(),
            window,
            cx,
        );
        self.group_editor_message.clear();
        self.group_editor_open = true;
        self.request_overlay_focus();
        cx.notify();
    }

    fn close_group_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.group_editor_open = false;
        self.group_editor_source = None;
        self.group_editor_message.clear();
        self.overlay_focus_pending = false;
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn validate_group_name(&self, raw: &str, source: Option<&str>) -> Result<String, String> {
        let name = raw.trim();
        if name.is_empty() {
            return Err(self.tr("Group name is required."));
        }
        if name.chars().count() > MAX_GROUP_NAME_CHARS {
            return Err(self.tr("Group name is too long."));
        }
        let key = collapsed_group_key(name);
        if key == collapsed_group_key("UNGROUPED") {
            return Err(self.tr("UNGROUPED is reserved."));
        }
        let source_key = source.map(collapsed_group_key);
        let duplicate = self.settings.group_names.iter().any(|existing| {
            collapsed_group_key(existing) == key && source_key.as_deref() != Some(key.as_str())
        }) || self.profiles.iter().any(|profile| {
            collapsed_group_key(&profile.group) == key
                && source_key.as_deref() != Some(key.as_str())
        });
        if duplicate {
            return Err(self.tr("A group with this name already exists."));
        }
        Ok(name.to_owned())
    }

    fn submit_group_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.group_editor_mode == GroupEditorMode::Delete {
            self.delete_group(window, cx);
            return;
        }
        let raw = self.group_editor_name.read(cx).value();
        let source = self.group_editor_source.clone();
        let name = match self.validate_group_name(&raw, source.as_deref()) {
            Ok(name) => name,
            Err(error) => {
                self.group_editor_message = error;
                cx.notify();
                return;
            }
        };
        let key = collapsed_group_key(&name);
        match self.group_editor_mode {
            GroupEditorMode::Create => {
                self.settings.group_names.push(name.clone());
                self.settings.group_order.push(key);
                self.persist_ui_preferences(cx);
                self.close_group_editor(window, cx);
                self.show_toast(self.tr("Group created."), Tone::Success, cx);
            }
            GroupEditorMode::Rename => {
                let Some(source) = self.group_editor_source.clone() else {
                    self.group_editor_message = self.tr("The group to rename is unavailable.");
                    cx.notify();
                    return;
                };
                let old_key = collapsed_group_key(&source);
                let mut profiles = self.profiles.clone();
                if rename_profile_group(&mut profiles, &source, &name) > 0
                    && let Err(error) = self.save_workspace(profiles, self.recent.clone())
                {
                    self.group_editor_message = error;
                    cx.notify();
                    return;
                }
                let mut found = false;
                for existing in &mut self.settings.group_names {
                    if collapsed_group_key(existing) == old_key {
                        *existing = name.clone();
                        found = true;
                    }
                }
                if !found {
                    self.settings.group_names.push(name.clone());
                }
                for existing in &mut self.settings.group_order {
                    if existing == &old_key {
                        *existing = key.clone();
                    }
                }
                if !self.settings.group_order.iter().any(|entry| entry == &key) {
                    self.settings.group_order.push(key.clone());
                }
                if self.collapsed_groups.remove(&old_key) {
                    self.collapsed_groups.insert(key);
                }
                self.persist_ui_preferences(cx);
                self.close_group_editor(window, cx);
                self.show_toast(self.tr("Group renamed."), Tone::Success, cx);
            }
            GroupEditorMode::Delete => unreachable!(),
        }
    }

    fn delete_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(source) = self.group_editor_source.clone() else {
            self.group_editor_message = self.tr("The group to delete is unavailable.");
            cx.notify();
            return;
        };
        let mut profiles = self.profiles.clone();
        if remove_profile_group(&mut profiles, &source) > 0
            && let Err(error) = self.save_workspace(profiles, self.recent.clone())
        {
            self.group_editor_message = error;
            cx.notify();
            return;
        }
        let key = collapsed_group_key(&source);
        self.settings
            .group_names
            .retain(|group| collapsed_group_key(group) != key);
        self.settings.group_order.retain(|group| group != &key);
        self.collapsed_groups.remove(&key);
        self.persist_ui_preferences(cx);
        self.close_group_editor(window, cx);
        self.show_toast(
            self.tr("Group deleted; hosts moved to Ungrouped."),
            Tone::Success,
            cx,
        );
    }

    fn toggle_group_collapsed(&mut self, group: &str, cx: &mut Context<Self>) {
        let key = collapsed_group_key(group);
        if !self.collapsed_groups.insert(key.clone()) {
            self.collapsed_groups.remove(&key);
        }
        self.persist_ui_preferences(cx);
    }

    fn reorder_groups_before(&mut self, dragged: &str, target: &str, cx: &mut Context<Self>) {
        let mut order = self.settings.group_order.clone();
        let mut missing = self
            .profiles
            .iter()
            .map(|profile| collapsed_group_key(&profile.group))
            .filter(|key| !order.iter().any(|entry| entry == key))
            .collect::<Vec<_>>();
        missing.sort();
        missing.dedup();
        order.extend(missing);
        if !reorder_group_order(&mut order, dragged, target) {
            return;
        }
        self.settings.group_order = order;
        self.persist_ui_preferences(cx);
        self.show_toast(self.tr("Group order updated."), Tone::Success, cx);
    }

    fn group_drop_header(
        &self,
        tokens: &ThemeTokens,
        label: impl Into<SharedString>,
        group: String,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label: SharedString = label.into();
        let drop_id: SharedString = format!("host-group-drop-{group}").into();
        let toggle_group = group.clone();
        let target_group = collapsed_group_key(&group);
        let host_group = group.clone();
        let preview_tokens = *tokens;
        let mut actions = div().flex().items_center().gap(px(ui_tokens::SPACE_1));
        if !group.is_empty() {
            let rename_group = group.clone();
            let delete_group = group.clone();
            actions = actions
                .child(
                    components::icon_button(
                        tokens,
                        SharedString::from(format!("rename-group-{target_group}")),
                        IconName::Settings2,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        self.tr("Rename group"),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open_group_editor(
                            GroupEditorMode::Rename,
                            Some(&rename_group),
                            window,
                            cx,
                        );
                    })),
                )
                .child(
                    components::icon_button(
                        tokens,
                        SharedString::from(format!("delete-group-{target_group}")),
                        IconName::Delete,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        self.tr("Delete group"),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open_group_editor(
                            GroupEditorMode::Delete,
                            Some(&delete_group),
                            window,
                            cx,
                        );
                    })),
                );
        }
        components::collapsible_header_with_actions(tokens, drop_id, label.clone(), open, actions)
            .rounded(px(ui_tokens::RADIUS_CONTROL))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_group_collapsed(&toggle_group, cx);
            }))
            .on_drag(
                SidebarDragData::Group {
                    key: target_group.clone(),
                    title: label,
                },
                move |data: &SidebarDragData, _, _, cx| {
                    let title = match data {
                        SidebarDragData::Host { title, .. }
                        | SidebarDragData::Group { title, .. } => title.clone(),
                    };
                    cx.new(|_| DragPreview {
                        title,
                        tokens: preview_tokens,
                    })
                },
            )
            .on_drop(
                cx.listener(move |this, data: &SidebarDragData, _, cx| match data {
                    SidebarDragData::Host { name, .. } => {
                        this.move_host_to_group(name, &host_group, cx);
                    }
                    SidebarDragData::Group { key, .. } => {
                        this.reorder_groups_before(key, &target_group, cx);
                    }
                }),
            )
            .hover(|style| style.bg(tokens.surface_overlay))
            .into_any_element()
    }

    fn group_editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let mode = self.group_editor_mode;
        let source = self.group_editor_source.clone().unwrap_or_default();
        let title = match mode {
            GroupEditorMode::Create => self.tr("New group"),
            GroupEditorMode::Rename => self.tr("Rename group"),
            GroupEditorMode::Delete => self.tr("Delete group"),
        };
        let mut body = div()
            .id("group-editor-body")
            .flex()
            .flex_col()
            .gap(px(ui_tokens::SPACE_3))
            .p(px(ui_tokens::SPACE_4));
        match mode {
            GroupEditorMode::Delete => {
                body = body
                    .child(components::help_text(
                        &tokens,
                        format!("{}: {source}", self.tr("Group")),
                    ))
                    .child(components::warning_text(
                        &tokens,
                        self.tr("The group will be removed and its hosts moved to Ungrouped."),
                    ));
            }
            GroupEditorMode::Create | GroupEditorMode::Rename => {
                body = body.child(components::field(
                    &tokens,
                    self.tr("Group name"),
                    &self.group_editor_name,
                    false,
                ));
            }
        }
        if !self.group_editor_message.is_empty() {
            body = body.child(components::warning_text(
                &tokens,
                self.group_editor_message.clone(),
            ));
        }
        let action_label = match mode {
            GroupEditorMode::Delete => self.tr("Delete"),
            GroupEditorMode::Create | GroupEditorMode::Rename => self.tr("Save"),
        };
        let action_kind = if mode == GroupEditorMode::Delete {
            ButtonKind::Danger
        } else {
            ButtonKind::Primary
        };
        components::modal_overlay(&tokens, "group-editor-overlay")
            .track_focus(&self.overlay_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.close_group_editor(window, cx);
                } else if event.keystroke.key.eq_ignore_ascii_case("enter")
                    && (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
                {
                    cx.stop_propagation();
                    this.submit_group_editor(window, cx);
                }
            }))
            .child(
                components::modal_panel(&tokens, "group-editor-panel")
                    .w(px(ui_tokens::MODAL_WIDTH_KEYGEN))
                    .child(
                        components::modal_header(&tokens, title).child(
                            components::icon_button(
                                &tokens,
                                "group-editor-close",
                                IconName::Close,
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                                self.tr("Close"),
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.close_group_editor(window, cx);
                                },
                            )),
                        ),
                    )
                    .child(body)
                    .child(
                        components::modal_footer(&tokens)
                            .child(
                                components::button(
                                    &tokens,
                                    "group-editor-cancel",
                                    self.tr("Cancel"),
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.close_group_editor(window, cx);
                                    },
                                )),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "group-editor-submit",
                                    action_label,
                                    action_kind,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.submit_group_editor(window, cx);
                                    },
                                )),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn apply_auth_profile(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(profile) = self
            .auth_profiles
            .iter()
            .find(|profile| profile.name.eq_ignore_ascii_case(name))
            .cloned()
        else {
            self.auth_profile_message = self
                .tr("Authentication template {name} was not found.")
                .replace("{name}", name);
            cx.notify();
            return;
        };
        self.auth_method = profile.auth_method;
        self.applied_auth_profile = None;
        Self::set_input_value(&self.identity_file, &profile.identity_file, window, cx);
        Self::set_input_value(&self.jump_profile, &profile.proxy_jump, window, cx);
        Self::set_input_value(
            &self.keepalive_seconds,
            &profile.keepalive_seconds.to_string(),
            window,
            cx,
        );
        Self::set_input_value(&self.password, "", window, cx);
        Self::set_input_value(&self.key_passphrase, "", window, cx);
        Self::set_input_value(&self.jump_password, "", window, cx);
        Self::set_input_value(&self.jump_passphrase, "", window, cx);
        Self::set_input_value(&self.auth_profile_name, &profile.name, window, cx);
        self.applied_auth_profile = Some(profile.name.clone());
        self.auth_profile_message = self
            .tr("Applied authentication template {name}. Re-enter any password or passphrase.")
            .replace("{name}", &profile.name);
        self.show_toast(
            self.tr("Applied authentication template {name}.")
                .replace("{name}", &profile.name),
            Tone::Success,
            cx,
        );
    }

    fn save_auth_profile(&mut self, cx: &mut Context<Self>) {
        let name = self.auth_profile_name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            self.auth_profile_message = self.tr("Enter a template name.");
            cx.notify();
            return;
        }
        let keepalive = self.keepalive_seconds.read(cx).value().trim().to_owned();
        let keepalive_seconds = if keepalive.is_empty() {
            0
        } else {
            match keepalive.parse::<u16>() {
                Ok(value) => value,
                Err(_) => {
                    self.auth_profile_message =
                        self.tr("Keep-alive must be a whole number of seconds.");
                    cx.notify();
                    return;
                }
            }
        };
        let profile = AuthProfile {
            name: name.clone(),
            auth_method: self.auth_method,
            identity_file: self.identity_file.read(cx).value().trim().to_owned(),
            proxy_jump: self.jump_profile.read(cx).value().trim().to_owned(),
            keepalive_seconds,
        };
        let profile = match profile.normalized() {
            Ok(profile) => profile,
            Err(error) => {
                self.auth_profile_message = error;
                cx.notify();
                return;
            }
        };
        let mut profiles = self.auth_profiles.clone();
        if let Some(existing) = profiles
            .iter_mut()
            .find(|existing| existing.name.eq_ignore_ascii_case(&profile.name))
        {
            *existing = profile.clone();
        } else {
            profiles.push(profile.clone());
        }
        profiles.sort_by_key(|profile| profile.name.to_lowercase());
        let Some(store) = self.auth_profile_store.as_ref() else {
            self.auth_profile_message = self.tr("Authentication profile storage is unavailable.");
            cx.notify();
            return;
        };
        if let Err(error) = store.save(&profiles) {
            self.auth_profile_message = error;
            self.show_toast(
                self.tr("Could not save the authentication template."),
                Tone::Danger,
                cx,
            );
            return;
        }
        self.auth_profiles = profiles;
        self.applied_auth_profile = Some(profile.name.clone());
        self.auth_profile_message = self
            .tr("Saved authentication template {name}.")
            .replace("{name}", &profile.name);
        self.show_toast(
            self.tr("Saved authentication template {name}.")
                .replace("{name}", &profile.name),
            Tone::Success,
            cx,
        );
    }

    fn delete_auth_profile(&mut self, cx: &mut Context<Self>) {
        let mut name = self.auth_profile_name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            name = self.applied_auth_profile.clone().unwrap_or_default();
        }
        if name.is_empty() {
            self.auth_profile_message = self.tr("Select or name a template to delete.");
            cx.notify();
            return;
        }
        let mut profiles = self.auth_profiles.clone();
        let before = profiles.len();
        profiles.retain(|profile| !profile.name.eq_ignore_ascii_case(&name));
        if profiles.len() == before {
            self.auth_profile_message = self
                .tr("Authentication template {name} was not found.")
                .replace("{name}", &name);
            cx.notify();
            return;
        }
        let Some(store) = self.auth_profile_store.as_ref() else {
            self.auth_profile_message = self.tr("Authentication profile storage is unavailable.");
            cx.notify();
            return;
        };
        if let Err(error) = store.save(&profiles) {
            self.auth_profile_message = error;
            self.show_toast(
                self.tr("Could not delete the authentication template."),
                Tone::Danger,
                cx,
            );
            return;
        }
        self.auth_profiles = profiles;
        self.applied_auth_profile = None;
        self.auth_profile_message = self
            .tr("Deleted authentication template {name}.")
            .replace("{name}", &name);
        self.show_toast(
            self.tr("Deleted authentication template {name}.")
                .replace("{name}", &name),
            Tone::Success,
            cx,
        );
    }

    fn save_workspace(
        &mut self,
        profiles: Vec<HostProfile>,
        recent: Vec<RecentConnection>,
    ) -> Result<(), String> {
        let Some(store) = self.profile_store.as_ref() else {
            return Err("Host profile storage is unavailable.".to_owned());
        };
        store.save(&HostData {
            profiles: profiles.clone(),
            recent: recent.clone(),
        })?;
        self.profiles = profiles;
        self.recent = recent;
        Ok(())
    }

    fn persist_forward_rule(
        &mut self,
        profile_name: &str,
        add: Option<&ForwardRuleSpec>,
        remove: Option<&ForwardRuleSpec>,
    ) {
        let mut profiles = self.profiles.clone();
        let Some(profile) = profiles
            .iter_mut()
            .find(|profile| profile.name.eq_ignore_ascii_case(profile_name))
        else {
            return;
        };
        if let Some(rule) = add {
            if !profile.forwards.iter().any(|existing| existing == rule) {
                if profile.forwards.len() >= MAX_FORWARD_RULES {
                    self.profile_message = format!(
                        "Host profile {profile_name} cannot have more than {MAX_FORWARD_RULES} forwarding rules."
                    );
                    return;
                }
                profile.forwards.push(rule.clone());
            }
        }
        if let Some(rule) = remove {
            profile.forwards.retain(|existing| existing != rule);
        }
        if let Err(error) = self.save_workspace(profiles, self.recent.clone()) {
            self.profile_message = error;
        }
    }

    fn save_profile(&mut self, cx: &mut Context<Self>) {
        if !self.profiles_ready {
            self.profile_message =
                self.tr("Host profiles could not be loaded; fix the profile file before saving.");
            cx.notify();
            return;
        }

        if self.profile_store.is_none() {
            self.profile_message = self.tr("Host profile storage is unavailable.");
            cx.notify();
            return;
        }
        let name = self.profile_name.read(cx).value().trim().to_owned();
        let group = self.profile_group.read(cx).value().trim().to_owned();
        let host = self.host.read(cx).value().trim().to_owned();
        let port = self.port.read(cx).value().parse::<u16>();
        let username = self.username.read(cx).value().trim().to_owned();
        let password = Zeroizing::new(self.password.read(cx).text().to_string());
        let passphrase = Zeroizing::new(self.key_passphrase.read(cx).text().to_string());
        let jump_password = Zeroizing::new(self.jump_password.read(cx).text().to_string());
        let jump_passphrase = Zeroizing::new(self.jump_passphrase.read(cx).text().to_string());
        let identity_file = self.identity_file.read(cx).value().trim().to_owned();
        let proxy_jump = self.jump_profile.read(cx).value().trim().to_owned();
        let keepalive_seconds = self
            .keepalive_seconds
            .read(cx)
            .value()
            .trim()
            .parse::<u16>();
        let keepalive_seconds = match keepalive_seconds {
            Ok(seconds) if seconds <= 3600 => seconds,
            _ => {
                self.profile_message =
                    self.tr("Keep-alive must be between 0 and 3600 seconds (0 disables it).");
                cx.notify();
                return;
            }
        };
        let existing_forwards = self
            .selected_profile
            .as_ref()
            .and_then(|selected| {
                self.profiles
                    .iter()
                    .find(|profile| profile.name.eq_ignore_ascii_case(selected))
            })
            .map(|profile| profile.forwards.clone())
            .unwrap_or_default();
        let Ok(port) = port else {
            self.profile_message = self.tr("Port must be between 1 and 65535.");
            cx.notify();
            return;
        };
        if name.is_empty() || host.is_empty() || username.is_empty() || port == 0 {
            self.profile_message =
                self.tr("Profile name, host, username, and a valid port are required.");
            cx.notify();
            return;
        }
        if self.auth_method == AuthMethod::PublicKey && identity_file.is_empty() {
            self.profile_message =
                self.tr("Public key authentication requires an identity file path.");
            cx.notify();
            return;
        }
        if !proxy_jump.is_empty() {
            if proxy_jump.eq_ignore_ascii_case(&name) {
                self.profile_message = self.tr("A host cannot use itself as its jump host.");
                cx.notify();
                return;
            }
            if !self.profiles.iter().any(|profile| {
                profile.name.eq_ignore_ascii_case(&proxy_jump)
                    && !profile.name.eq_ignore_ascii_case(&name)
            }) {
                self.profile_message = self
                    .tr("Jump host {name} is not another saved host.")
                    .replace("{name}", &proxy_jump);
                cx.notify();
                return;
            }
        }

        let replaces_selected = |profile: &HostProfile| {
            self.selected_profile
                .as_ref()
                .is_some_and(|selected| selected.eq_ignore_ascii_case(&profile.name))
        };
        if self
            .profiles
            .iter()
            .any(|profile| profile.name.eq_ignore_ascii_case(&name) && !replaces_selected(profile))
        {
            self.profile_message = self
                .tr("A host named {name} already exists.")
                .replace("{name}", &name);
            cx.notify();
            return;
        }

        let profile = HostProfile {
            name: name.clone(),
            host,
            port,
            username,
            group,
            favorite: self.profile_favorite,
            auth_method: self.auth_method,
            identity_file: if self.auth_method == AuthMethod::PublicKey {
                identity_file
            } else {
                String::new()
            },
            proxy_jump,
            forwards: existing_forwards,
            keepalive_seconds,
            password: password.to_string(),
            passphrase: passphrase.to_string(),
            jump_password: jump_password.to_string(),
            jump_passphrase: jump_passphrase.to_string(),
        };
        let mut profiles = self.profiles.clone();
        if let Some(selected) = &self.selected_profile {
            profiles.retain(|profile| !profile.name.eq_ignore_ascii_case(selected));
        }
        profiles.push(profile);
        sort_profiles(&mut profiles);
        if let Err(error) = self.save_workspace(profiles, self.recent.clone()) {
            self.profile_message = error;
            cx.notify();
            return;
        }

        self.selected_profile = Some(name.clone());
        self.confirm_delete = false;
        self.profile_message = self.tr("Saved {name}.").replace("{name}", &name);
        self.show_toast(self.profile_message.clone(), Tone::Success, cx);
    }

    fn request_profile_delete(&mut self, cx: &mut Context<Self>) {
        if self.selected_profile.is_some() {
            self.confirm_delete = true;
            self.profile_message.clear();
            cx.notify();
        }
    }

    fn delete_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.selected_profile.clone() else {
            return;
        };
        if self.profile_store.is_none() {
            self.profile_message = self.tr("Host profile storage is unavailable.");
            cx.notify();
            return;
        }
        let mut profiles = self.profiles.clone();
        profiles.retain(|profile| !profile.name.eq_ignore_ascii_case(&selected));
        if let Err(error) = self.save_workspace(profiles, self.recent.clone()) {
            self.profile_message = error;
            cx.notify();
            return;
        }
        if let Some(profile) = self
            .profiles
            .iter()
            .find(|profile| profile.name.eq_ignore_ascii_case(&selected))
        {
            self.credential_cache
                .remove(&CredentialCacheKey::from_profile(profile));
        }

        self.new_profile(window, cx);
        self.session_editor_open = false;
        self.overlay_focus_pending = false;
        window.focus(&self.terminal_focus);
        self.profile_message = self.tr("Deleted {name}.").replace("{name}", &selected);
        self.show_toast(self.profile_message.clone(), Tone::Success, cx);
    }

    fn cancel_session_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let credential_session_id = self.credential_prompt_session_id.take();
        Self::set_input_value(&self.password, "", window, cx);
        Self::set_input_value(&self.key_passphrase, "", window, cx);
        Self::set_input_value(&self.jump_password, "", window, cx);
        Self::set_input_value(&self.jump_passphrase, "", window, cx);
        self.confirm_delete = false;
        self.session_editor_open = false;
        self.credential_prompt_open = false;
        self.overlay_focus_pending = false;
        if let Some(id) = credential_session_id {
            self.close_session(id, window, cx);
            return;
        }
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn persist_credentials_for_session(
        &mut self,
        session_id: u64,
        credentials: &CachedCredentials,
        cx: &mut Context<Self>,
    ) {
        let Some(profile_name) = self
            .session(session_id)
            .and_then(|session| session.profile_name.clone())
        else {
            return;
        };
        let mut profiles = self.profiles.clone();
        let Some(profile) = profiles
            .iter_mut()
            .find(|profile| profile.name.eq_ignore_ascii_case(&profile_name))
        else {
            return;
        };
        profile.password = credentials.password.to_string();
        profile.passphrase = credentials.passphrase.to_string();
        profile.jump_password = credentials.jump_password.to_string();
        profile.jump_passphrase = credentials.jump_passphrase.to_string();
        if let Err(error) = self.save_workspace(profiles, self.recent.clone()) {
            self.profile_message = self
                .tr("Connected, but credentials could not be saved: {error}")
                .replace("{error}", &error);
            self.show_toast(self.profile_message.clone(), Tone::Warning, cx);
        }
    }

    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let host = self.host.read(cx).value().trim().to_owned();
        let port = self.port.read(cx).value().parse::<u16>();
        let username = self.username.read(cx).value().trim().to_owned();
        let requested_title = self.profile_name.read(cx).value().trim().to_owned();
        let identity_file = self.identity_file.read(cx).value().trim().to_owned();
        let jump_name = self.jump_profile.read(cx).value().trim().to_owned();
        let password = Zeroizing::new(self.password.read(cx).text().to_string());
        let passphrase = Zeroizing::new(self.key_passphrase.read(cx).text().to_string());
        let jump_password = Zeroizing::new(self.jump_password.read(cx).text().to_string());
        let jump_passphrase = Zeroizing::new(self.jump_passphrase.read(cx).text().to_string());
        let cached_credentials = CachedCredentials::from_inputs(
            &password,
            &passphrase,
            &jump_password,
            &jump_passphrase,
        );
        Self::set_input_value(&self.password, "", window, cx);
        Self::set_input_value(&self.key_passphrase, "", window, cx);
        Self::set_input_value(&self.jump_password, "", window, cx);
        Self::set_input_value(&self.jump_passphrase, "", window, cx);

        let port = match port {
            Ok(port) if port > 0 => port,
            _ => {
                self.profile_message = self.tr("Port must be between 1 and 65535.");
                cx.notify();
                return;
            }
        };
        if host.trim().is_empty() || username.trim().is_empty() {
            self.profile_message = self.tr("Host and user are required.");
            cx.notify();
            return;
        }
        let keepalive_seconds = match self
            .keepalive_seconds
            .read(cx)
            .value()
            .trim()
            .parse::<u16>()
        {
            Ok(seconds) if seconds <= 3600 => seconds,
            _ => {
                self.profile_message =
                    self.tr("Keep-alive must be between 0 and 3600 seconds (0 disables it).");
                cx.notify();
                return;
            }
        };
        let saved_profile = if requested_title.is_empty() {
            None
        } else {
            self.profiles
                .iter()
                .find(|profile| profile.name.eq_ignore_ascii_case(&requested_title))
                .cloned()
        };
        let saved_profile_name = saved_profile.as_ref().map(|profile| profile.name.clone());
        let saved_forwards = saved_profile
            .as_ref()
            .map(|profile| profile.forwards.clone())
            .unwrap_or_default();
        let authentication =
            match build_authentication(self.auth_method, &identity_file, password, passphrase) {
                Ok(authentication) => authentication,
                Err(error) => {
                    self.profile_message = self.tr(&error);
                    cx.notify();
                    return;
                }
            };
        let jump = if jump_name.is_empty() {
            None
        } else {
            let Some(profile) = self
                .profiles
                .iter()
                .find(|profile| profile.name.eq_ignore_ascii_case(&jump_name))
                .cloned()
            else {
                self.profile_message = self
                    .tr("Jump host {name} is not a saved host.")
                    .replace("{name}", &jump_name);
                cx.notify();
                return;
            };
            if !profile.proxy_jump.trim().is_empty() {
                self.profile_message = self
                    .tr("Jump host {name} already uses jump host {jump}. Nested jump hosts are not supported yet.")
                    .replace("{name}", &profile.name)
                    .replace("{jump}", &profile.proxy_jump);
                cx.notify();
                return;
            }
            if profile.host.eq_ignore_ascii_case(&host)
                && profile.port == port
                && profile.username == username
            {
                self.profile_message = self
                    .tr("The jump host resolves to the same target; choose a different jump host.");
                cx.notify();
                return;
            }
            let jump_authentication = match build_authentication(
                profile.auth_method,
                &profile.identity_file,
                jump_password,
                jump_passphrase,
            ) {
                Ok(authentication) => authentication,
                Err(error) => {
                    self.profile_message = self
                        .tr("Jump host {name}: {error}")
                        .replace("{name}", &profile.name)
                        .replace("{error}", &error);
                    cx.notify();
                    return;
                }
            };
            Some(JumpSpec {
                host: profile.host.clone(),
                port: profile.port,
                username: profile.username.clone(),
                authentication: jump_authentication,
            })
        };

        let id = self.next_session_id;
        self.next_session_id = self.next_session_id.saturating_add(1);
        let title = if requested_title.is_empty() {
            format!("{username}@{host}")
        } else {
            requested_title
        };
        let route = if jump.is_some() {
            format!(" via {jump_name}")
        } else {
            String::new()
        };
        let credential_key = CredentialCacheKey::from_connection(
            &host,
            port,
            &username,
            self.auth_method,
            &identity_file,
            &jump_name,
        );
        let status = self
            .tr("Checking host keys and connecting to {host}:{port}{route}...")
            .replace("{host}", &host)
            .replace("{port}", &port.to_string())
            .replace("{route}", &route);
        let terminal_size = self
            .active_session()
            .map_or(self.terminal_size, |session| session.terminal_size);
        let recent_name = title.clone();
        let recent_host = host.clone();
        let spec = ConnectionSpec {
            host,
            port,
            username: username.clone(),
            authentication,
            terminal_size,
            jump,
            keepalive_seconds,
            theme: self.settings.theme,
            cursor_style: self.settings.cursor_style,
        };
        let (event_tx, mut event_rx) = mpsc::channel(16);
        self.profile_message.clear();
        match ssh::spawn_connection(spec, event_tx) {
            Ok(handles) => {
                let ssh::ConnectionHandles {
                    terminal,
                    sftp,
                    cancellation,
                    mut sftp_events,
                    sftp_cancel,
                    forwards,
                    mut forward_events,
                } = handles;
                self.sessions.push(SshSession {
                    id,
                    title,
                    host: recent_host.clone(),
                    port,
                    username: username.clone(),
                    status,
                    frame: None,
                    terminal_message: self.tr("Connecting..."),
                    terminal_size,
                    scroll_remainder: 0.0,
                    mouse_position: None,
                    command_tx: Some(terminal),
                    cancel_tx: Some(cancellation),
                    sftp_tx: Some(sftp),
                    sftp_cancel: Some(sftp_cancel),
                    sftp: SftpViewState::default(),
                    forward_tx: Some(forwards),
                    forwards: ForwardViewState::default(),
                    profile_name: saved_profile_name.clone(),
                    pending_forwards: saved_forwards.clone(),
                    log_path: None,
                    connection_info: None,
                    server_metrics: ServerMetricsViewState::default(),
                    credential_key: Some(credential_key.clone()),
                    state: SessionState::Connecting,
                });
                if cached_credentials.contains_secret() {
                    self.pending_credentials
                        .insert(id, (credential_key, cached_credentials));
                }
                self.split = None;
                self.terminal_bounds.borrow_mut().clear();
                self.active_session_id = Some(id);
                self.session_editor_open = false;
                self.credential_prompt_open = false;
                self.credential_prompt_session_id = None;
                self.overlay_focus_pending = false;
                let mut recent = self.recent.clone();
                record_recent(
                    &mut recent,
                    RecentConnection {
                        name: recent_name,
                        host: recent_host,
                        port,
                        username,
                        last_used_unix: unix_now(),
                    },
                );
                if let Err(error) = self.save_workspace(self.profiles.clone(), recent) {
                    self.profile_message = self
                        .tr("Connected, but recent connections could not be saved: {error}")
                        .replace("{error}", &error);
                }
                self.resize_sessions(cx);
                cx.spawn(async move |this, cx| {
                    while let Some(event) = sftp_events.recv().await {
                        let result = this.update(cx, |view, cx| {
                            if view.apply_sftp_event(id, event, cx) {
                                cx.notify();
                            }
                        });
                        if result.is_err() {
                            break;
                        }
                    }
                })
                .detach();
                cx.spawn(async move |this, cx| {
                    while let Some(event) = forward_events.recv().await {
                        let result = this.update(cx, |view, cx| {
                            if view.apply_forward_event(id, event) {
                                cx.notify();
                            }
                        });
                        if result.is_err() {
                            break;
                        }
                    }
                })
                .detach();
            }
            Err(error) => {
                self.pending_credentials.remove(&id);
                self.profile_message = self
                    .tr("Could not start SSH runtime: {error}")
                    .replace("{error}", &error.to_string());
                cx.notify();
                return;
            }
        }

        cx.spawn(async move |this, cx| {
            while let Some(event) = event_rx.recv().await {
                match event {
                    UiEvent::AuthPrompt {
                        name,
                        instructions,
                        prompts,
                        respond_to,
                    } => {
                        let result = this.update(cx, |view, cx| {
                            view.begin_auth_prompt(id, name, instructions, prompts, respond_to, cx);
                            cx.notify();
                        });
                        if result.is_err() {
                            break;
                        }
                    }
                    UiEvent::SearchResult(result) => {
                        let result = this.update(cx, |view, cx| {
                            view.search_message = search_result_message(&result);
                            cx.notify();
                        });
                        if result.is_err() {
                            break;
                        }
                    }
                    event => {
                        let result = this.update(cx, |view, cx| {
                            let server_alerts_changed = matches!(
                                &event,
                                UiEvent::ServerMetrics(ServerData::Metrics(_))
                                    | UiEvent::ServerMetrics(ServerData::Processes(_))
                            );
                            let connection_established = matches!(
                                &event,
                                UiEvent::Status(status) if status.starts_with("Connected to ")
                            );
                            let screen = match &event {
                                UiEvent::Screen(frame) => Some(frame.clone()),
                                _ => None,
                            };
                            let failure = match &event {
                                UiEvent::Failed(error) => Some(error.clone()),
                                _ => None,
                            };
                            let service_action = match &event {
                                UiEvent::ServerMetrics(ServerData::ServiceAction(result)) => {
                                    Some(result.clone())
                                }
                                _ => None,
                            };
                            let process_action = match &event {
                                UiEvent::ServerMetrics(ServerData::ProcessAction(result)) => {
                                    Some(result.clone())
                                }
                                _ => None,
                            };
                            if apply_session_event(&mut view.sessions, id, event) {
                                cx.notify();
                            }
                            if server_alerts_changed && view.record_server_alerts(id) {
                                cx.notify();
                            }
                            if let Some(result) = process_action {
                                if result.success {
                                    view.refresh_server_processes(id, cx);
                                } else if let Some(session) = view.session_mut(id) {
                                    session.server_metrics.processes_loading = false;
                                }
                                cx.notify();
                            }
                            if let Some(result) = service_action {
                                if result.success {
                                    view.refresh_server_services(id, cx);
                                } else if let Some(session) = view.session_mut(id) {
                                    session.server_metrics.services_loading = false;
                                }
                                cx.notify();
                            }
                            if connection_established
                                && view
                                    .session(id)
                                    .is_some_and(|session| session.state == SessionState::Connected)
                                && let Some((key, credentials)) =
                                    view.pending_credentials.remove(&id)
                                && credentials.contains_secret()
                            {
                                view.persist_credentials_for_session(id, &credentials, cx);
                                view.credential_cache.insert(key, credentials);
                            }
                            if failure.is_some() {
                                view.pending_credentials.remove(&id);
                            }
                            if let Some(frame) = screen {
                                view.process_triggers(id, &frame, cx);
                            }
                            if view
                                .session(id)
                                .is_some_and(|session| session.state == SessionState::Disconnected)
                            {
                                let had_auth_prompt = view
                                    .auth_prompt
                                    .as_ref()
                                    .is_some_and(|prompt| prompt.session_id == id);
                                let had_trigger_latch = view.trigger_latches.remove(&id).is_some();
                                if had_auth_prompt {
                                    view.auth_prompt = None;
                                    view.auth_prompt_needs_clear = true;
                                }
                                if had_auth_prompt || had_trigger_latch {
                                    cx.notify();
                                }
                                if let Some(error) = failure {
                                    view.reopen_authentication_after_failure(id, error, cx);
                                }
                            }
                        });
                        if result.is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = this.update(cx, |view, cx| {
                view.pending_credentials.remove(&id);
                if view
                    .session(id)
                    .is_some_and(|session| session.state != SessionState::Disconnected)
                    && apply_session_event(
                        &mut view.sessions,
                        id,
                        UiEvent::Failed("Session event stream closed".to_owned()),
                    )
                {
                    if let Some(session) = view.session_mut(id) {
                        session.terminal_message =
                            "The SSH session ended unexpectedly. Connect again.".to_owned();
                    }
                    view.trigger_latches.remove(&id);
                    if view
                        .auth_prompt
                        .as_ref()
                        .is_some_and(|prompt| prompt.session_id == id)
                    {
                        view.auth_prompt = None;
                        view.auth_prompt_needs_clear = true;
                    }
                    cx.notify();
                }
            });
        })
        .detach();
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn reorder_sessions_before(&mut self, dragged_id: u64, target_id: u64) -> bool {
        let mut ids = self.sessions.iter().map(|session| session.id).collect();
        if !reorder_session_ids(&mut ids, dragged_id, target_id) {
            return false;
        }
        let mut sessions: HashMap<u64, SshSession> = self
            .sessions
            .drain(..)
            .map(|session| (session.id, session))
            .collect();
        self.sessions = ids
            .into_iter()
            .filter_map(|id| sessions.remove(&id))
            .collect();
        true
    }

    fn select_session(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.session(id).is_none() {
            return;
        }
        if self.split.is_some_and(|split| !split.contains(id)) {
            self.split = None;
            self.terminal_bounds.borrow_mut().clear();
            self.resize_sessions(cx);
        }
        self.active_session_id = Some(id);
        if matches!(self.search_target, Some(SearchTarget::Session(_))) {
            self.search_target = Some(SearchTarget::Session(id));
            self.search_message.clear();
        }
        if self.search_visible {
            let focus = self.search_input.read(cx).focus_handle(cx);
            window.focus(&focus);
        } else {
            window.focus(&self.terminal_focus);
        }
        cx.notify();
    }

    fn disconnect_session(&mut self, id: u64, cx: &mut Context<Self>) {
        self.pending_credentials.remove(&id);
        if self
            .auth_prompt
            .as_ref()
            .is_some_and(|prompt| prompt.session_id == id)
        {
            self.auth_prompt = None;
            self.auth_prompt_needs_clear = true;
        }
        if self.search_target == Some(SearchTarget::Session(id)) {
            self.search_visible = false;
            self.search_target = None;
            self.search_message.clear();
        }
        self.trigger_latches.remove(&id);
        let Some(session) = self.session_mut(id) else {
            return;
        };
        if session.state == SessionState::Disconnected
            || session.state == SessionState::Disconnecting
        {
            return;
        }

        if let Some(cancel_tx) = &session.cancel_tx {
            cancel_tx.send_replace(true);
        }
        session.state = SessionState::Disconnecting;
        session.frame = None;
        session.terminal_message = "Disconnecting from the SSH session...".to_owned();
        session.mouse_position = None;
        session.scroll_remainder = 0.0;
        session.status = "Disconnecting...".to_owned();
        cx.notify();
    }

    fn close_session(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.pending_credentials.remove(&id);
        let closes_split_pane = self.split.is_some_and(|split| split.contains(id));
        if !remove_session(&mut self.sessions, &mut self.active_session_id, id) {
            return;
        }
        if self.tab_menu_session == Some(id) {
            self.tab_menu_session = None;
        }
        self.terminal_bounds.borrow_mut().remove(&id);
        self.trigger_latches.remove(&id);
        self.snippet_targets.remove(&id);
        if self.sync_targets.remove(&id) && self.sync_targets.is_empty() {
            self.sync_input_enabled = false;
        }
        if self.search_target == Some(SearchTarget::Session(id)) {
            self.search_visible = false;
            self.search_target = None;
            self.search_message.clear();
        }
        if self
            .auth_prompt
            .as_ref()
            .is_some_and(|prompt| prompt.session_id == id)
        {
            self.auth_prompt = None;
            self.auth_prompt_needs_clear = true;
        }
        if closes_split_pane {
            self.split = None;
            self.terminal_bounds.borrow_mut().clear();
        }
        if self.active_session_id.is_none() {
            self.status = "Ready".to_owned();
        }
        if closes_split_pane {
            self.resize_terminal(window, cx);
        }
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn send_key(
        &mut self,
        session_id: u64,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.session(session_id).is_none() {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        let key_name = event.keystroke.key.to_ascii_lowercase();
        if let Some(shortcut) = app_shortcut(
            &self.settings.shortcuts,
            &key_name,
            modifiers.control || modifiers.platform,
            modifiers.shift,
            modifiers.alt,
        ) {
            cx.stop_propagation();
            match shortcut {
                AppShortcut::NewSession => self.new_profile(window, cx),
                AppShortcut::CloseSession => self.close_session(session_id, window, cx),
                AppShortcut::ToggleFiles => self.toggle_sftp(session_id, cx),
                AppShortcut::ToggleSnippets => self.toggle_snippets(cx),
                AppShortcut::ToggleSettings => self.toggle_settings(window, cx),
                AppShortcut::ToggleSidebar => self.toggle_sidebar(window, cx),
                AppShortcut::ToggleCompose => self.toggle_compose(window, cx),
                AppShortcut::Disconnect => self.disconnect_session(session_id, cx),
                AppShortcut::ReplayLog => self.begin_log_replay(cx),
                AppShortcut::Find => self.open_search(session_id, window, cx),
                AppShortcut::SelectSession(index) => {
                    if let Some(target) = self.sessions.get(index).map(|session| session.id) {
                        self.select_session(target, window, cx);
                    }
                }
            }
            return;
        }
        if self
            .session(session_id)
            .is_none_or(|session| session.state != SessionState::Connected)
        {
            return;
        }
        if modifiers.shift && (modifiers.control || modifiers.platform) {
            match key_name.as_str() {
                "c" => {
                    cx.stop_propagation();
                    if let Some(text) = self
                        .session(session_id)
                        .and_then(|session| session.frame.as_ref())
                        .and_then(|frame| frame.selection_text.clone())
                        .filter(|text| !text.is_empty())
                    {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
                    return;
                }
                "v" => {
                    cx.stop_propagation();
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        let delivered = self.send_input_to_targets(
                            session_id,
                            || TerminalRequest::Paste(text.clone()),
                            "Terminal paste",
                            cx,
                        );
                        if delivered > 1 {
                            self.show_toast(
                                format!("Pasted into {delivered} sessions."),
                                Tone::Success,
                                cx,
                            );
                        }
                    }
                    return;
                }
                _ => {}
            }
        }

        let key = TerminalKeyInput {
            key: event.keystroke.key.clone(),
            text: event.keystroke.key_char.clone(),
            control: modifiers.control,
            alt: modifiers.alt,
            shift: modifiers.shift,
            platform: modifiers.platform,
            function: modifiers.function,
        };
        if key.encode(false).is_none() {
            return;
        }

        match self.send_input_to_targets(
            session_id,
            || TerminalRequest::Key(key.clone()),
            "Terminal input",
            cx,
        ) {
            0 => {}
            _ => cx.stop_propagation(),
        }
    }

    fn activate_session(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.session(id).is_none() {
            return;
        }
        let changed = self.active_session_id != Some(id);
        self.active_session_id = Some(id);
        window.focus(&self.terminal_focus);
        if changed {
            cx.notify();
        }
    }

    fn terminal_position(
        &self,
        session_id: u64,
        position: Point<Pixels>,
    ) -> Option<TerminalPosition> {
        let bounds = self.terminal_bounds.borrow().get(&session_id).cloned()?;
        let size = self
            .session(session_id)
            .map_or(self.terminal_size, |session| session.terminal_size);
        let column = (((position.x - bounds.origin.x) / px(1.0)) / self.settings.cell_width())
            .floor()
            .clamp(0.0, f32::from(size.columns.saturating_sub(1))) as u16;
        let row = (((position.y - bounds.origin.y) / px(1.0)) / self.settings.cell_height())
            .floor()
            .clamp(0.0, f32::from(size.rows.saturating_sub(1))) as u16;
        Some(TerminalPosition { row, column })
    }

    fn mouse_down(
        &mut self,
        session_id: u64,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_session(session_id, window, cx);
        if self.session(session_id).is_none_or(|session| {
            session.state != SessionState::Connected || session.frame.is_none()
        }) {
            return;
        }
        let Some(button) = terminal_mouse_button(event.button) else {
            return;
        };
        let Some(position) = self.terminal_position(session_id, event.position) else {
            return;
        };
        if let Some(session) = self.session_mut(session_id) {
            session.mouse_position = Some(position);
        }
        self.send_terminal_request(
            session_id,
            TerminalRequest::Mouse(TerminalMouseEvent::Press {
                button,
                position,
                modifiers: terminal_mouse_modifiers(
                    event.modifiers.shift,
                    event.modifiers.alt,
                    event.modifiers.control,
                ),
            }),
            "Terminal mouse input",
            cx,
        );
    }

    fn mouse_up(&mut self, session_id: u64, event: &gpui::MouseUpEvent, cx: &mut Context<Self>) {
        if self
            .session(session_id)
            .is_none_or(|session| session.state != SessionState::Connected)
        {
            return;
        }
        let Some(button) = terminal_mouse_button(event.button) else {
            return;
        };
        let previous_position = self
            .session(session_id)
            .and_then(|session| session.mouse_position);
        let Some(position) = self
            .terminal_position(session_id, event.position)
            .or(previous_position)
        else {
            return;
        };
        if let Some(session) = self.session_mut(session_id) {
            session.mouse_position = Some(position);
        }
        self.send_terminal_request(
            session_id,
            TerminalRequest::Mouse(TerminalMouseEvent::Release {
                button,
                position,
                modifiers: terminal_mouse_modifiers(
                    event.modifiers.shift,
                    event.modifiers.alt,
                    event.modifiers.control,
                ),
            }),
            "Terminal mouse input",
            cx,
        );
    }

    fn mouse_move(&mut self, session_id: u64, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(session) = self.session(session_id) else {
            return;
        };
        if session.state != SessionState::Connected {
            return;
        }
        let tracking_mode = session
            .frame
            .as_ref()
            .map_or(MouseTrackingMode::Disabled, |frame| {
                frame.mouse_tracking_mode
            });
        let button = event.pressed_button.and_then(terminal_mouse_button);
        if event.pressed_button.is_some() && button.is_none() {
            return;
        }
        let local_selection_drag = tracking_mode == MouseTrackingMode::Disabled
            && button == Some(TerminalMouseButton::Left);
        if !local_selection_drag && !tracking_mode.reports_motion(button.is_some()) {
            return;
        }
        let Some(position) = self.terminal_position(session_id, event.position) else {
            return;
        };
        if self
            .session(session_id)
            .is_some_and(|session| session.mouse_position == Some(position))
        {
            return;
        }
        if let Some(session) = self.session_mut(session_id) {
            session.mouse_position = Some(position);
        }
        self.send_terminal_request(
            session_id,
            TerminalRequest::Mouse(TerminalMouseEvent::Move {
                button,
                position,
                modifiers: terminal_mouse_modifiers(
                    event.modifiers.shift,
                    event.modifiers.alt,
                    event.modifiers.control,
                ),
            }),
            "Terminal mouse input",
            cx,
        );
    }

    fn send_terminal_request(
        &mut self,
        session_id: u64,
        request: TerminalRequest,
        action: &str,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = enqueue_session_request(&self.sessions, session_id, request) {
            if let Some(session) = self.session_mut(session_id) {
                session.status = format!("{action} queue is unavailable: {error}");
            }
            cx.notify();
        }
    }

    fn sync_target_count(&self) -> usize {
        if !self.sync_input_enabled {
            return 0;
        }
        self.sync_targets
            .iter()
            .filter(|id| {
                self.session(**id)
                    .is_some_and(|session| session.state == SessionState::Connected)
            })
            .count()
    }

    fn input_targets(&self, active_session_id: u64) -> Vec<u64> {
        synchronized_targets(
            active_session_id,
            self.sync_input_enabled,
            &self.sync_targets,
            |id| {
                self.session(id)
                    .is_some_and(|session| session.state == SessionState::Connected)
            },
        )
    }

    fn send_input_to_targets(
        &mut self,
        active_session_id: u64,
        request: impl Fn() -> TerminalRequest,
        action: &str,
        cx: &mut Context<Self>,
    ) -> usize {
        let targets = self.input_targets(active_session_id);
        let mut delivered = 0;
        for target in targets {
            match enqueue_session_request(&self.sessions, target, request()) {
                Ok(()) => delivered += 1,
                Err(error) => {
                    if let Some(session) = self.session_mut(target) {
                        session.status = format!("{action} queue is unavailable: {error}");
                    }
                }
            }
        }
        if delivered == 0 {
            cx.notify();
        }
        delivered
    }

    fn toggle_sync_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_visible = !self.sync_visible;
        if self.sync_visible {
            self.request_overlay_focus();
        } else {
            self.overlay_focus_pending = false;
            window.focus(&self.terminal_focus);
        }
        cx.notify();
    }

    fn toggle_sync_target(&mut self, session_id: u64, cx: &mut Context<Self>) {
        if self
            .session(session_id)
            .is_none_or(|session| session.state != SessionState::Connected)
        {
            return;
        }
        if self.sync_targets.remove(&session_id) {
            if self.sync_targets.is_empty() {
                self.sync_input_enabled = false;
            }
        } else {
            self.sync_targets.insert(session_id);
            self.sync_input_enabled = true;
        }
        cx.notify();
    }

    fn clear_sync_targets(&mut self, cx: &mut Context<Self>) {
        self.sync_targets.clear();
        self.sync_input_enabled = false;
        cx.notify();
    }

    fn toggle_main_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu_visible = !self.menu_visible;
        if self.menu_visible {
            self.layout_menu_visible = false;
            self.tab_menu_session = None;
            self.request_overlay_focus();
        } else {
            self.overlay_focus_pending = false;
            window.focus(&self.terminal_focus);
        }
        cx.notify();
    }

    fn close_main_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu_visible {
            self.menu_visible = false;
            self.overlay_focus_pending = false;
            window.focus(&self.terminal_focus);
            cx.notify();
        }
    }

    fn toggle_layout_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layout_menu_visible = !self.layout_menu_visible;
        if self.layout_menu_visible {
            self.menu_visible = false;
            self.tab_menu_session = None;
            self.request_overlay_focus();
        } else {
            self.overlay_focus_pending = false;
            window.focus(&self.terminal_focus);
        }
        cx.notify();
    }

    fn close_layout_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.layout_menu_visible {
            self.layout_menu_visible = false;
            self.overlay_focus_pending = false;
            window.focus(&self.terminal_focus);
            cx.notify();
        }
    }

    fn open_tab_menu(&mut self, session_id: u64, cx: &mut Context<Self>) {
        if self.session(session_id).is_none() {
            return;
        }
        self.tab_menu_session = Some(session_id);
        self.menu_visible = false;
        self.layout_menu_visible = false;
        self.request_overlay_focus();
        cx.notify();
    }

    fn close_tab_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab_menu_session.take().is_some() {
            self.overlay_focus_pending = false;
            window.focus(&self.terminal_focus);
            cx.notify();
        }
    }

    fn close_other_sessions(
        &mut self,
        keep_session_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ids = self
            .sessions
            .iter()
            .map(|session| session.id)
            .filter(|id| *id != keep_session_id)
            .collect::<Vec<_>>();
        for id in ids {
            self.close_session(id, window, cx);
        }
        self.tab_menu_session = None;
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn close_sessions_to_right(
        &mut self,
        session_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .sessions
            .iter()
            .position(|session| session.id == session_id)
        else {
            return;
        };
        let ids = self
            .sessions
            .iter()
            .skip(index + 1)
            .map(|session| session.id)
            .collect::<Vec<_>>();
        for id in ids {
            self.close_session(id, window, cx);
        }
        self.tab_menu_session = None;
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn apply_layout_preset(
        &mut self,
        preset: LayoutPreset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if preset == LayoutPreset::Split {
            let active = self.active_session_id;
            let other = active.and_then(|active| {
                self.sessions
                    .iter()
                    .map(|session| session.id)
                    .find(|id| *id != active)
            });
            if let Some(other) = other {
                self.sidebar_collapsed = false;
                self.inspector_visible = false;
                self.compose_visible = false;
                self.set_split(other, SplitOrientation::SideBySide, window, cx);
            } else {
                self.show_toast("Open two sessions to use split layout.", Tone::Warning, cx);
            }
        } else {
            self.compose_visible = false;
            self.inspector_visible = matches!(preset, LayoutPreset::Inspector);
            self.sidebar_collapsed = matches!(preset, LayoutPreset::Focus);
            self.split = None;
            self.terminal_bounds.borrow_mut().clear();
            self.persist_ui_preferences(cx);
            self.resize_terminal(window, cx);
        }
        self.layout_menu_visible = false;
        self.tab_menu_session = None;
        self.overlay_focus_pending = false;
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn apply_split_orientation(
        &mut self,
        orientation: SplitOrientation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(active) = self.active_session_id else {
            self.show_toast("Open two sessions to use split layout.", Tone::Warning, cx);
            return;
        };
        let Some(other) = self
            .sessions
            .iter()
            .map(|session| session.id)
            .find(|id| *id != active)
        else {
            self.show_toast("Open two sessions to use split layout.", Tone::Warning, cx);
            return;
        };
        self.sidebar_collapsed = false;
        self.inspector_visible = false;
        self.compose_visible = false;
        let active_is_split = self.split.is_some_and(|split| split.contains(active));
        if active_is_split {
            self.set_split_orientation(orientation, window, cx);
        } else {
            if self.split.take().is_some() {
                self.terminal_bounds.borrow_mut().clear();
            }
            self.set_split(other, orientation, window, cx);
        }
        self.layout_menu_visible = false;
        self.tab_menu_session = None;
        self.overlay_focus_pending = false;
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn copy_terminal_selection(&mut self, session_id: u64, cx: &mut Context<Self>) {
        if let Some(text) = self
            .session(session_id)
            .and_then(|session| session.frame.as_ref())
            .and_then(|frame| frame.selection_text.clone())
            .filter(|text| !text.is_empty())
        {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.show_toast("Copied selection.", Tone::Success, cx);
        }
    }

    fn paste_into_terminal(&mut self, session_id: u64, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let delivered = self.send_input_to_targets(
                session_id,
                || TerminalRequest::Paste(text.clone()),
                "Terminal paste",
                cx,
            );
            let message = if delivered > 1 {
                format!("Pasted into {delivered} sessions.")
            } else {
                "Pasted from clipboard.".to_owned()
            };
            self.show_toast(message, Tone::Success, cx);
        }
    }

    fn terminal_surface(&mut self, session_id: u64, cx: &mut Context<Self>) -> AnyElement {
        let terminal_bounds = Rc::clone(&self.terminal_bounds);
        let palette = self.settings.theme.palette();
        let mut keyword_highlights = if self.keyword_highlighting_enabled {
            self.keyword_highlights.clone()
        } else {
            Vec::new()
        };
        for keyword in self.trigger_highlight_keywords() {
            if !keyword_highlights
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&keyword))
            {
                keyword_highlights.push(keyword);
            }
        }
        let mut terminal_view = div()
            .flex()
            .flex_col()
            .relative()
            .flex_1()
            .min_h_0()
            .font_family(self.settings.font_family.clone())
            .text_size(px(self.settings.font_size))
            .text_color(rgb(palette.foreground))
            .bg(rgb(palette.background))
            .on_children_prepainted(move |children, _, _| {
                if let Some(bounds) = children.first() {
                    terminal_bounds
                        .borrow_mut()
                        .insert(session_id, bounds.clone());
                }
            })
            .on_key_down(cx.listener(move |this, event, window, cx| {
                this.send_key(session_id, event, window, cx);
            }))
            .on_scroll_wheel(cx.listener(move |this, event, window, cx| {
                this.scroll_terminal(session_id, event, window, cx);
            }))
            .on_any_mouse_down(cx.listener(move |this, event, window, cx| {
                this.mouse_down(session_id, event, window, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, event, _, cx| this.mouse_up(session_id, event, cx)),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |this, event, _, cx| this.mouse_up(session_id, event, cx)),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(move |this, event, _, cx| this.mouse_up(session_id, event, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(move |this, event, _, cx| this.mouse_up(session_id, event, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Middle,
                cx.listener(move |this, event, _, cx| this.mouse_up(session_id, event, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Right,
                cx.listener(move |this, event, _, cx| this.mouse_up(session_id, event, cx)),
            )
            .on_mouse_move(cx.listener(move |this, event, _, cx| {
                this.mouse_move(session_id, event, cx);
            }))
            .track_focus(&self.terminal_focus);

        if let Some(frame) = self
            .session(session_id)
            .and_then(|session| session.frame.as_ref())
        {
            terminal_view = terminal_view.child(render_terminal_grid(
                frame,
                &self.settings,
                palette,
                &keyword_highlights,
            ));
        } else {
            let message = self
                .session(session_id)
                .map(|session| session.terminal_message.clone())
                .unwrap_or_default();
            terminal_view = terminal_view.child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .text_color(rgb(palette.dim_foreground))
                    .child(message),
            );
        }
        if self.search_visible && self.search_target == Some(SearchTarget::Session(session_id)) {
            terminal_view = terminal_view.child(
                div()
                    .absolute()
                    .top(px(ui_tokens::SPACE_2))
                    .left(px(ui_tokens::SPACE_4))
                    .right(px(ui_tokens::SPACE_4))
                    .child(self.search_bar(cx)),
            );
        }
        terminal_view.into_any_element()
    }

    fn terminal_panel(&mut self, session_id: u64, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let Some(session) = self.session(session_id) else {
            return div().flex_1().min_w_0().min_h_0().into_any_element();
        };
        let title = session.title.clone();
        let state = session.state;
        let focused = self.active_session_id == Some(session_id);
        let surface = self.terminal_surface(session_id, cx);
        let card_group: SharedString = format!("terminal-card-{session_id}").into();
        div()
            .id(("terminal-panel", session_id))
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .relative()
            .group(card_group.clone())
            .border_1()
            .border_color(if focused {
                tokens.border_strong
            } else {
                tokens.border_subtle
            })
            .rounded(px(ui_tokens::RADIUS_PANEL))
            .overflow_hidden()
            .bg(tokens.surface_raised)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(TERMINAL_PANEL_HEADER_PX))
                    .flex_shrink_0()
                    .px(px(ui_tokens::SPACE_3))
                    .border_b_1()
                    .border_color(tokens.border_subtle)
                    .bg(tokens.surface)
                    .text_size(px(ui_tokens::FONT_CAPTION))
                    .on_any_mouse_down(cx.listener(move |this, _, window, cx| {
                        this.activate_session(session_id, window, cx);
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_color(tokens.text_primary)
                            .child(title),
                    )
                    .child(components::pill(
                        &tokens,
                        self.tr(session_state_label(state)),
                        session_tone(state),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .top(px(TERMINAL_PANEL_HEADER_PX + ui_tokens::SPACE_1))
                    .right(px(ui_tokens::SPACE_2))
                    .occlude()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .p(px(2.0))
                    .rounded(px(ui_tokens::RADIUS_CONTROL))
                    .border_1()
                    .border_color(tokens.border_subtle)
                    .bg(tokens.surface_overlay.opacity(0.94))
                    .opacity(0.4)
                    .group_hover(card_group, |style| style.opacity(1.0))
                    .child(
                        components::icon_button(
                            &tokens,
                            ("terminal-find", session_id),
                            IconName::Search,
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                            "Find in terminal",
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.open_search(session_id, window, cx);
                            },
                        )),
                    )
                    .child(
                        components::icon_button(
                            &tokens,
                            ("terminal-copy", session_id),
                            IconName::Copy,
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                            "Copy selection",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.copy_terminal_selection(session_id, cx);
                        })),
                    )
                    .child(
                        components::icon_button(
                            &tokens,
                            ("terminal-paste", session_id),
                            IconName::Replace,
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                            "Paste",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.paste_into_terminal(session_id, cx);
                        })),
                    )
                    .child(
                        components::icon_button(
                            &tokens,
                            ("terminal-replay", session_id),
                            IconName::BookOpen,
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                            "Replay log",
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.begin_log_replay(cx))),
                    )
                    .child(
                        components::icon_button(
                            &tokens,
                            ("terminal-upload", session_id),
                            IconName::ArrowUp,
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                            "Send file",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.begin_upload(session_id, cx);
                        })),
                    )
                    .child(
                        components::icon_button(
                            &tokens,
                            ("terminal-compose", session_id),
                            IconName::PanelBottom,
                            if self.compose_visible {
                                ButtonKind::Secondary
                            } else {
                                ButtonKind::Ghost
                            },
                            ButtonSize::Small,
                            "Compose (Ctrl+Shift+M)",
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_compose(window, cx);
                        })),
                    )
                    .child(
                        components::icon_button(
                            &tokens,
                            ("toggle-inspector", session_id),
                            IconName::Ellipsis,
                            if self.inspector_visible {
                                ButtonKind::Secondary
                            } else {
                                ButtonKind::Ghost
                            },
                            ButtonSize::Small,
                            "Toggle inspector",
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.toggle_inspector(window, cx)),
                        ),
                    ),
            )
            .child(surface)
            .into_any_element()
    }

    fn session_editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let mut navigation = div()
            .flex()
            .flex_col()
            .w(px(ui_tokens::SESSION_NAV_WIDTH))
            .flex_shrink_0()
            .gap(px(ui_tokens::SPACE_1))
            .p(px(ui_tokens::SPACE_2))
            .border_r_1()
            .border_color(tokens.border_subtle)
            .bg(tokens.surface);
        for (index, section) in SessionEditorSection::ALL.into_iter().enumerate() {
            let kind = if self.session_editor_section == section {
                ButtonKind::Primary
            } else {
                ButtonKind::Ghost
            };
            let button = components::button(
                &tokens,
                ("session-editor-section", index),
                self.tr(section.label()),
                kind,
                ButtonSize::Regular,
            );
            navigation =
                navigation.child(button.w_full().on_click(cx.listener(move |this, _, _, cx| {
                    this.session_editor_section = section;
                    cx.notify();
                })));
        }

        let jump_name = self.jump_profile.read(cx).value().trim().to_owned();
        let jump_profile = self
            .profiles
            .iter()
            .find(|profile| profile.name.eq_ignore_ascii_case(&jump_name))
            .cloned();
        let jump_method = jump_profile.as_ref().map(|profile| profile.auth_method);
        let jump_summary = jump_profile.as_ref().map_or_else(
            || {
                if jump_name.is_empty() {
                    self.tr("Leave empty for a direct connection.")
                } else {
                    format!("{jump_name} is not a saved host.")
                }
            },
            |profile| {
                format!(
                    "{}@{}:{} · {}",
                    profile.username,
                    profile.host,
                    profile.port,
                    profile.auth_method.label()
                )
            },
        );

        let section_content = match self.session_editor_section {
            SessionEditorSection::Connection => div()
                .flex()
                .flex_col()
                .gap(px(ui_tokens::SPACE_3))
                .child(components::field(
                    &tokens,
                    self.tr("Session name"),
                    &self.profile_name,
                    false,
                ))
                .child(
                    div()
                        .flex()
                        .gap(px(ui_tokens::SPACE_3))
                        .child(
                            div().flex_1().child(components::field(
                                &tokens,
                                self.tr("Host"),
                                &self.host,
                                false,
                            )),
                        )
                        .child(
                            div()
                                .w(px(ui_tokens::PORT_FIELD_WIDTH))
                                .child(components::field(
                                    &tokens,
                                    self.tr("Port"),
                                    &self.port,
                                    false,
                                )),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_end()
                        .gap(px(ui_tokens::SPACE_3))
                        .child(
                            div().flex_1().child(components::field(
                                &tokens,
                                self.tr("Group"),
                                &self.profile_group,
                                false,
                            )),
                        )
                        .child(components::button(
                            &tokens,
                            "toggle-profile-favorite",
                            if self.profile_favorite {
                                self.tr("Favorite")
                            } else {
                                self.tr("Not favorite")
                            },
                            ButtonKind::Secondary,
                            ButtonSize::Regular,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.profile_favorite = !this.profile_favorite;
                            cx.notify();
                        }))),
                )
                .child(components::help_text(
                    &tokens,
                    self.tr("Hosts with the same group are listed under one folder in the sidebar."),
                ))
                .child(components::field(
                    &tokens,
                    self.tr("Jump host"),
                    &self.jump_profile,
                    false,
                ))
                .child(if jump_profile.is_some() {
                    components::help_text(&tokens, jump_summary)
                } else {
                    components::warning_text(&tokens, jump_summary)
                })
                .child(components::field(
                    &tokens,
                    self.tr("Keep-alive (seconds, 0 = off)"),
                    &self.keepalive_seconds,
                    false,
                ))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(ui_tokens::SPACE_1))
                        .child(components::label_text(&tokens, self.tr("Protocol")))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .h(px(ui_tokens::CONTROL_HEIGHT))
                                .px(px(ui_tokens::SPACE_3))
                                .border_1()
                                .border_color(tokens.border_strong)
                                .rounded(px(ui_tokens::RADIUS_CONTROL))
                                .bg(tokens.surface)
                                .text_size(px(ui_tokens::FONT_CAPTION))
                                .text_color(tokens.text_secondary)
                                .child("SSH"),
                        ),
                ),
            SessionEditorSection::Authentication => {
                let mut methods = div().flex().items_center().gap(px(ui_tokens::SPACE_2));
                for (index, method) in AuthMethod::ALL.into_iter().enumerate() {
                    let kind = if self.auth_method == method {
                        ButtonKind::Primary
                    } else {
                        ButtonKind::Ghost
                    };
                    let button = components::button(
                        &tokens,
                        ("auth-method", index),
                        self.tr(method.label()),
                        kind,
                        ButtonSize::Regular,
                    );
                    methods = methods.child(button.on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.auth_method = method;
                            this.applied_auth_profile = None;
                            cx.notify();
                        },
                    )));
                }
                let mut template_buttons =
                    div().flex().flex_wrap().gap(px(ui_tokens::SPACE_2));
                for (index, profile) in self.auth_profiles.iter().enumerate() {
                    let name = profile.name.clone();
                    let apply_name = name.clone();
                    let selected =
                        self.applied_auth_profile.as_deref() == Some(profile.name.as_str());
                    template_buttons = template_buttons.child(
                        components::button(
                            &tokens,
                            ("auth-template", index),
                            name,
                            if selected {
                                ButtonKind::Primary
                            } else {
                                ButtonKind::Secondary
                            },
                            ButtonSize::Small,
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.apply_auth_profile(&apply_name, window, cx);
                        })),
                    );
                }
                let templates = div()
                    .flex()
                    .flex_col()
                    .gap(px(ui_tokens::SPACE_2))
                    .child(components::section_label(
                        &tokens,
                        self.tr("Authentication templates"),
                    ))
                    .when(self.auth_profiles.is_empty(), |panel| {
                        panel.child(components::help_text(
                            &tokens,
                            self.tr("No saved templates. Save the current method as a reusable template."),
                        ))
                    })
                    .when(!self.auth_profiles.is_empty(), |panel| {
                        panel.child(template_buttons)
                    })
                    .child(components::field(
                        &tokens,
                        self.tr("Template name"),
                        &self.auth_profile_name,
                        false,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(ui_tokens::SPACE_2))
                            .child(
                                components::button(
                                    &tokens,
                                    "save-auth-template",
                                    self.tr("Save template"),
                                    ButtonKind::Secondary,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.save_auth_profile(cx);
                                })),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "delete-auth-template",
                                    self.tr("Delete"),
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.delete_auth_profile(cx);
                                })),
                            ),
                    )
                    .child(components::help_text(
                        &tokens,
                        self.tr("Templates store the method, identity file path, jump-host reference and keep-alive only. Applying a template clears secret inputs; saved host profiles keep their own credentials."),
                    ))
                    .when(!self.auth_profile_message.is_empty(), |panel| {
                        panel.child(components::help_text(
                            &tokens,
                            self.auth_profile_message.clone(),
                        ))
                    });
                div()
                    .flex()
                    .flex_col()
                    .gap(px(ui_tokens::SPACE_3))
                    .child(templates)
                    .child(components::field(
                        &tokens,
                        self.tr("Username"),
                        &self.username,
                        false,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(ui_tokens::SPACE_1))
                            .child(components::label_text(
                                &tokens,
                                self.tr("Authentication method"),
                            ))
                            .child(methods),
                    )
                    .when(self.auth_method.uses_password_input(), |panel| {
                        panel
                            .child(components::field(
                                &tokens,
                                self.tr("Password"),
                                &self.password,
                                true,
                            ))
                            .child(components::help_text(
                                &tokens,
                                if self.auth_method == AuthMethod::KeyboardInteractive {
                                    self.tr("Hidden keyboard-interactive prompts are answered with this password. One-time codes and visible prompts are not supported yet.")
                                } else {
                        self.tr("Passwords and passphrases are saved with host profiles and reused on the next connection.")
                                },
                            ))
                    })
                    .when(self.auth_method == AuthMethod::PublicKey, |panel| {
                        panel
                            .child(components::field(
                                &tokens,
                                self.tr("Identity file"),
                                &self.identity_file,
                                false,
                            ))
                            .child(components::field(
                                &tokens,
                                self.tr("Key passphrase"),
                                &self.key_passphrase,
                                true,
                            ))
                            .child(components::help_text(
                                &tokens,
                                self.tr("The identity file path and passphrase are saved with the host. Private key material is never copied."),
                            ))
                    })
                    .when(self.auth_method == AuthMethod::Agent, |panel| {
                        panel.child(components::help_text(
                            &tokens,
                            self.tr("Uses the Windows OpenSSH agent named pipe or Pageant. Load the key into the agent before connecting."),
                        ))
                    })
                    .when(!jump_name.is_empty(), |panel| {
                        panel
                            .child(components::subheading(&tokens, self.tr("Jump host")))
                            .child(components::help_text(&tokens, jump_summary))
                            .when(jump_method == Some(AuthMethod::Password), |panel| {
                                panel.child(components::field(
                                    &tokens,
                                    self.tr("Jump password"),
                                    &self.jump_password,
                                    true,
                                ))
                            })
                            .when(jump_method == Some(AuthMethod::PublicKey), |panel| {
                                panel.child(components::field(
                                    &tokens,
                                    self.tr("Jump key passphrase"),
                                    &self.jump_passphrase,
                                    true,
                                ))
                            })
                            .child(components::help_text(
                                &tokens,
                                    self.tr("Jump credentials are saved with this host profile and reused for its jump host."),
                            ))
                    })
            }
            SessionEditorSection::Security => div()
                .flex()
                .flex_col()
                .gap(px(ui_tokens::SPACE_3))
                .child(components::subheading(&tokens, self.tr("Server identity")))
                .child(components::help_text(
                    &tokens,
                    self.tr("Host keys must already match an entry in the user's OpenSSH known_hosts file. Unknown and changed keys are rejected. When a jump host is used, both the jump host and the target are verified independently."),
                ))
                .child(components::subheading(&tokens, self.tr("Credential handling")))
                .child(components::help_text(
                    &tokens,
                    self.tr("Password and key passphrase fields are cleared when you connect or cancel. Saved host profiles also keep the credentials needed for the next connection; private key material is never copied."),
                )),
        };

        let mut profile_actions = div().flex().items_center().gap(px(ui_tokens::SPACE_2));
        if self.selected_profile.is_some() {
            if self.confirm_delete {
                profile_actions = profile_actions
                    .child(
                        components::button(
                            &tokens,
                            "cancel-delete-profile",
                            self.tr("Keep"),
                            ButtonKind::Ghost,
                            ButtonSize::Regular,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_delete = false;
                            cx.notify();
                        })),
                    )
                    .child(
                        components::button(
                            &tokens,
                            "confirm-delete-profile",
                            self.tr("Delete"),
                            ButtonKind::Danger,
                            ButtonSize::Regular,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.delete_profile(window, cx)),
                        ),
                    );
            } else {
                profile_actions = profile_actions.child(
                    components::button(
                        &tokens,
                        "delete-profile",
                        self.tr("Delete"),
                        ButtonKind::Danger,
                        ButtonSize::Regular,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.request_profile_delete(cx))),
                );
            }
        }

        let title = if self.selected_profile.is_some() {
            self.tr("Session Properties")
        } else {
            self.tr("New SSH Session")
        };
        components::modal_overlay(&tokens, "session-editor-overlay")
            .track_focus(&self.overlay_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let modifiers = event.keystroke.modifiers;
                match session_editor_key(
                    &event.keystroke.key,
                    modifiers.control,
                    modifiers.platform,
                ) {
                    Some(SessionEditorKey::Cancel) => {
                        cx.stop_propagation();
                        match session_editor_escape(this.confirm_delete) {
                            SessionEditorEscape::CancelDelete => {
                                this.confirm_delete = false;
                                cx.notify();
                            }
                            SessionEditorEscape::CloseEditor => {
                                this.cancel_session_editor(window, cx);
                            }
                        }
                    }
                    Some(SessionEditorKey::Save) => {
                        cx.stop_propagation();
                        this.save_profile(cx);
                    }
                    None => {}
                }
            }))
            .child(
                components::modal_panel(&tokens, "session-editor")
                    .w_full()
                    .max_w(px(ui_tokens::MODAL_WIDTH_SESSION))
                    .h_full()
                    .max_h(px(ui_tokens::MODAL_HEIGHT_SESSION))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .h(px(ui_tokens::MODAL_HEADER_HEIGHT))
                            .flex_shrink_0()
                            .px(px(ui_tokens::SPACE_4))
                            .border_b_1()
                            .border_color(tokens.border_subtle)
                            .child(
                                div()
                                    .text_size(px(ui_tokens::FONT_TITLE))
                                    .font_semibold()
                                    .child(title),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "close-session-editor",
                                    self.tr("Close"),
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.cancel_session_editor(window, cx);
                                    },
                                )),
                            ),
                    )
                    .child(
                        div().flex().flex_1().min_h_0().child(navigation).child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .gap(px(ui_tokens::SPACE_4))
                                .p(px(ui_tokens::SPACE_4))
                                .child(
                                    div()
                                        .text_size(px(ui_tokens::FONT_TITLE))
                                        .font_semibold()
                                        .child(self.tr(self.session_editor_section.label())),
                                )
                                .child(section_content),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .h(px(ui_tokens::MODAL_FOOTER_HEIGHT))
                            .flex_shrink_0()
                            .gap(px(ui_tokens::SPACE_3))
                            .px(px(ui_tokens::SPACE_4))
                            .border_t_1()
                            .border_color(tokens.border_subtle)
                            .bg(tokens.surface)
                            .child(profile_actions)
                            .child(div().flex_1().min_w_0().overflow_hidden().child(
                                components::help_text(&tokens, self.profile_message.clone()),
                            ))
                            .child(
                                components::button(
                                    &tokens,
                                    "cancel-session-editor",
                                    self.tr("Cancel"),
                                    ButtonKind::Ghost,
                                    ButtonSize::Regular,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.cancel_session_editor(window, cx);
                                    },
                                )),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "save-profile",
                                    self.tr("Save"),
                                    ButtonKind::Secondary,
                                    ButtonSize::Regular,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.save_profile(cx))),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "connect",
                                    self.tr("Connect"),
                                    ButtonKind::Primary,
                                    ButtonSize::Large,
                                )
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.connect(window, cx)),
                                ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn credential_prompt_overlay(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let host = self.host.read(cx).value().trim().to_owned();
        let port = self.port.read(cx).value().trim().to_owned();
        let username = self.username.read(cx).value().trim().to_owned();
        let jump_name = self.jump_profile.read(cx).value().trim().to_owned();
        let jump_profile = self.saved_jump_profile(&jump_name).cloned();
        let target = if host.is_empty() {
            self.tr("SSH connection")
        } else {
            format!("{username}@{host}:{port}")
        };
        let title = self
            .selected_profile
            .as_ref()
            .map(|name| self.tr("Authenticate {name}").replace("{name}", name))
            .unwrap_or_else(|| {
                self.tr("Authenticate {target}")
                    .replace("{target}", &target)
            });

        let mut fields = div().flex().flex_col().gap(px(ui_tokens::SPACE_3));
        if self.auth_method.uses_password_input() {
            fields = fields
                .child(components::field(
                    &tokens,
                    self.tr("Password"),
                    &self.password,
                    true,
                ))
                .child(components::help_text(
                    &tokens,
                    if self.auth_method == AuthMethod::KeyboardInteractive {
                        self.tr("Hidden keyboard-interactive prompts are answered with this password. One-time codes and visible prompts are not supported yet.")
                    } else {
                        self.tr("The password is saved with this host profile after a successful connection and reused next time.")
                    },
                ));
        }
        if self.auth_method == AuthMethod::PublicKey {
            fields = fields
                .child(components::field(
                    &tokens,
                    self.tr("Key passphrase"),
                    &self.key_passphrase,
                    true,
                ))
                .child(components::help_text(
                    &tokens,
                    self.tr("Leave the passphrase empty when the private key is not encrypted. A supplied passphrase is saved with this host profile."),
                ));
        }
        if self.auth_method == AuthMethod::Agent {
            fields = fields.child(components::help_text(
                &tokens,
                self.tr("Uses the SSH agent; no password is required."),
            ));
        }
        if !jump_name.is_empty() {
            fields = fields.child(components::subheading(&tokens, self.tr("Jump host")));
            if let Some(profile) = jump_profile {
                fields = fields.child(components::help_text(
                    &tokens,
                    format!(
                        "{}@{}:{} · {}",
                        profile.username,
                        profile.host,
                        profile.port,
                        self.tr(profile.auth_method.label())
                    ),
                ));
                match profile.auth_method {
                    AuthMethod::Password | AuthMethod::KeyboardInteractive => {
                        fields = fields.child(components::field(
                            &tokens,
                            self.tr("Jump password"),
                            &self.jump_password,
                            true,
                        ));
                    }
                    AuthMethod::PublicKey => {
                        fields = fields.child(components::field(
                            &tokens,
                            self.tr("Jump key passphrase"),
                            &self.jump_passphrase,
                            true,
                        ));
                    }
                    AuthMethod::Agent => {}
                }
            } else {
                fields = fields.child(components::warning_text(
                    &tokens,
                    self.tr("Jump host {name} is not a saved host.")
                        .replace("{name}", &jump_name),
                ));
            }
        }

        let mut body = div()
            .id("credential-prompt-body")
            .flex()
            .flex_col()
            .gap(px(ui_tokens::SPACE_3))
            .p(px(ui_tokens::SPACE_4))
            .min_h_0()
            .overflow_y_scroll()
            .child(components::help_text(
                &tokens,
                self.tr("Enter the authentication information and confirm to connect."),
            ))
            .child(fields);
        if !self.profile_message.is_empty() {
            body = body.child(components::warning_text(
                &tokens,
                self.profile_message.clone(),
            ));
        }
        body = body.child(components::help_text(
            &tokens,
            self.tr("Credentials are cleared from the form after connecting or cancelling. A successful connection saves them with the host profile for the next connection."),
        ));

        components::modal_overlay(&tokens, "credential-prompt-overlay")
            .track_focus(&self.overlay_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.cancel_session_editor(window, cx);
                }
            }))
            .child(
                components::modal_panel(&tokens, "credential-prompt")
                    .w_full()
                    .max_w(px(ui_tokens::MODAL_WIDTH_AUTH))
                    .max_h(px(640.))
                    .child(components::modal_header(&tokens, title))
                    .child(body)
                    .child(
                        components::modal_footer(&tokens)
                            .child(
                                components::button(
                                    &tokens,
                                    "credential-prompt-cancel",
                                    self.tr("Cancel"),
                                    ButtonKind::Ghost,
                                    ButtonSize::Regular,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.cancel_session_editor(window, cx);
                                    },
                                )),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "credential-prompt-connect",
                                    self.tr("Connect"),
                                    ButtonKind::Primary,
                                    ButtonSize::Large,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.connect(window, cx);
                                    },
                                )),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn saved_host_row(
        &self,
        tokens: &ThemeTokens,
        prefix: &'static str,
        index: usize,
        profile: &HostProfile,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_selected = self.selected_profile.as_deref() == Some(&profile.name)
            || self
                .session_for_target(&profile.host, profile.port, &profile.username)
                .is_some_and(|id| self.active_session_id == Some(id));
        let favorite = profile.favorite;
        let preview_tokens = *tokens;
        let drag_name = profile.name.clone();
        let drag_title: SharedString = profile.name.clone().into();
        let favorite_name = profile.name.clone();
        let connect_profile = profile.clone();
        let row_connect_profile = connect_profile.clone();
        let edit_profile = profile.clone();
        let actions = div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .child(
                components::icon_button(
                    tokens,
                    (ElementId::from((prefix, index)), "connect"),
                    IconName::ArrowRight,
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    self.tr("Connect to host"),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.connect_saved_profile(&connect_profile, window, cx);
                })),
            )
            .child(
                components::icon_button(
                    tokens,
                    (ElementId::from((prefix, index)), "favorite"),
                    IconName::Star,
                    if favorite {
                        ButtonKind::Primary
                    } else {
                        ButtonKind::Ghost
                    },
                    ButtonSize::Small,
                    if favorite {
                        "Remove favorite"
                    } else {
                        "Add favorite"
                    },
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.toggle_favorite(&favorite_name, cx);
                })),
            )
            .child(
                components::icon_button(
                    tokens,
                    (ElementId::from((prefix, index)), "edit"),
                    IconName::Settings2,
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    self.tr("Edit host"),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.load_profile(&edit_profile, window, cx);
                })),
            );
        components::SidebarRow::new(
            tokens,
            (prefix, index),
            profile.name.clone(),
            format!("{}@{}:{}", profile.username, profile.host, profile.port),
        )
        .selected(is_selected)
        .actions(actions)
        .into_row()
        .on_click(cx.listener(move |this, _, window, cx| {
            this.connect_saved_profile(&row_connect_profile, window, cx);
        }))
        .on_drag(
            SidebarDragData::Host {
                name: drag_name,
                title: drag_title,
            },
            move |data: &SidebarDragData, _, _, cx| {
                let title = match data {
                    SidebarDragData::Host { title, .. } | SidebarDragData::Group { title, .. } => {
                        title.clone()
                    }
                };
                cx.new(|_| DragPreview {
                    title: title.clone(),
                    tokens: preview_tokens,
                })
            },
        )
        .into_any_element()
    }

    fn recent_host_row(
        &self,
        tokens: &ThemeTokens,
        index: usize,
        entry: &RecentConnection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected_entry = entry.clone();
        let row_entry = entry.clone();
        let actions = components::icon_button(
            tokens,
            ("recent-host-action", index),
            IconName::ArrowRight,
            ButtonKind::Ghost,
            ButtonSize::Small,
            self.tr("Load into the connection form"),
        )
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            this.load_recent(&selected_entry, window, cx);
        }));
        components::SidebarRow::new(
            tokens,
            ("recent-host", index),
            entry.name.clone(),
            format!("{}@{}:{}", entry.username, entry.host, entry.port),
        )
        .actions(actions)
        .into_row()
        .on_click(cx.listener(move |this, _, window, cx| {
            this.activate_recent_connection(&row_entry, window, cx);
        }))
        .into_any_element()
    }

    fn apply_sftp_event(&mut self, id: u64, event: SftpEvent, cx: &mut Context<Self>) -> bool {
        let mut dispatch_next = false;
        {
            let Some(session) = self.session_mut(id) else {
                return false;
            };
            match event {
                SftpEvent::Ready { path } => {
                    if session.sftp.path.is_empty() {
                        session.sftp.path = path;
                    }
                    session.sftp.message.clear();
                    let target = session.sftp.path.clone();
                    if let Some(sftp_tx) = &session.sftp_tx {
                        let _ = sftp_tx.try_send(SftpCommand::List(target.clone()));
                        let _ = sftp_tx.try_send(SftpCommand::FilesystemInfo(target));
                    }
                }
                SftpEvent::Entries { path, entries } => {
                    session.sftp.path = path;
                    session.sftp.entries = entries;
                    session.sftp.message.clear();
                    let target = session.sftp.path.clone();
                    if let Some(sftp_tx) = &session.sftp_tx {
                        let _ = sftp_tx.try_send(SftpCommand::FilesystemInfo(target));
                    }
                }
                SftpEvent::FilesystemInfo(info) => {
                    session.sftp.filesystem = Some(info);
                    session.sftp.filesystem_supported = Some(true);
                }
                SftpEvent::FilesystemUnavailable { path } => {
                    if path == session.sftp.path {
                        session.sftp.filesystem = None;
                        session.sftp.filesystem_supported = Some(false);
                    }
                }
                SftpEvent::Error(error) => {
                    session.sftp.message = error;
                }
                SftpEvent::NeedsOverwrite {
                    kind,
                    local,
                    remote,
                    is_directory,
                } => {
                    session.sftp.message = format!(
                        "{} target {} already exists.",
                        kind.label(),
                        if kind == TransferKind::Upload {
                            remote.clone()
                        } else {
                            local.display().to_string()
                        }
                    );
                    session.sftp.overwrite = Some(OverwritePrompt {
                        kind,
                        local,
                        remote,
                        is_directory,
                    });
                }
                SftpEvent::TransferStarted { kind, name, total } => {
                    session.sftp.transfer = Some(TransferProgressUi {
                        kind,
                        name: name.clone(),
                        transferred: 0,
                        total,
                        last_transferred: 0,
                        last_sample: Instant::now(),
                        rate_bytes_per_second: 0.0,
                    });
                    session.sftp.message = format!("{} started: {name}", kind.label());
                }
                SftpEvent::TransferProgress {
                    kind,
                    name,
                    transferred,
                    total,
                } => {
                    let (rate, last_sample) = session.sftp.transfer.as_ref().map_or_else(
                        || (0.0, Instant::now()),
                        |current| {
                            let elapsed = current.last_sample.elapsed().as_secs_f64();
                            let rate = if elapsed > 0.0 {
                                transferred.saturating_sub(current.last_transferred) as f64
                                    / elapsed
                            } else {
                                current.rate_bytes_per_second
                            };
                            (rate, Instant::now())
                        },
                    );
                    session.sftp.transfer = Some(TransferProgressUi {
                        kind,
                        name,
                        transferred,
                        total,
                        last_transferred: transferred,
                        last_sample,
                        rate_bytes_per_second: rate,
                    });
                }
                SftpEvent::TransferFinished { kind, name } => {
                    session.sftp.transfer = None;
                    session.sftp.active_transfer = None;
                    session.sftp.message = format!("{} finished: {name}", kind.label());
                    if kind == TransferKind::Upload {
                        let path = session.sftp.path.clone();
                        if let Some(sftp_tx) = &session.sftp_tx {
                            let _ = sftp_tx.try_send(SftpCommand::List(path));
                        }
                    }
                    dispatch_next = true;
                }
                SftpEvent::TransferFailed { kind, name, error } => {
                    session.sftp.transfer = None;
                    session.sftp.active_transfer = None;
                    session.sftp.message = format!("{} failed for {name}: {error}", kind.label());
                    dispatch_next = true;
                }
                SftpEvent::TransferCancelled { kind, name } => {
                    session.sftp.transfer = None;
                    session.sftp.active_transfer = None;
                    session.sftp.message = format!("{} cancelled: {name}", kind.label());
                    dispatch_next = true;
                }
                SftpEvent::OperationFinished { action } => {
                    session.sftp.message = action;
                    let path = session.sftp.path.clone();
                    if let Some(sftp_tx) = &session.sftp_tx {
                        let _ = sftp_tx.try_send(SftpCommand::List(path));
                    }
                }
            }
        }
        if dispatch_next {
            self.dispatch_next_sftp_transfer(id, cx);
        }
        true
    }

    fn enqueue_sftp_transfer(
        &mut self,
        session_id: u64,
        command: SftpCommand,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session_mut(session_id) else {
            return;
        };
        debug_assert!(is_sftp_transfer_command(&command));
        session.sftp.transfer_queue.push_back(command);
        self.dispatch_next_sftp_transfer(session_id, cx);
        cx.notify();
    }

    fn dispatch_next_sftp_transfer(&mut self, session_id: u64, _cx: &mut Context<Self>) {
        let (sftp_tx, command) = {
            let Some(session) = self.session_mut(session_id) else {
                return;
            };
            if session.sftp.active_transfer.is_some() || session.sftp.overwrite.is_some() {
                return;
            }
            let Some(command) = take_next_sftp_transfer(
                session.sftp.active_transfer.is_some(),
                session.sftp.overwrite.is_some(),
                &mut session.sftp.transfer_queue,
            ) else {
                return;
            };
            let Some(sftp_tx) = session.sftp_tx.clone() else {
                session.sftp.message = "SFTP is not available for this session.".to_owned();
                return;
            };
            session.sftp.active_transfer = Some(command.clone());
            (sftp_tx, command)
        };
        let error = match sftp_tx.try_send(command) {
            Ok(()) => return,
            Err(error) => error,
        };
        let error_message = format!("SFTP request queue is unavailable: {error:?}");
        let command = match error {
            mpsc::error::TrySendError::Full(command)
            | mpsc::error::TrySendError::Closed(command) => command,
        };
        if let Some(session) = self.session_mut(session_id) {
            session.sftp.active_transfer = None;
            session.sftp.transfer_queue.push_front(command);
            session.sftp.message = error_message;
        }
    }

    fn clear_sftp_transfer_queue(&mut self, session_id: u64, cx: &mut Context<Self>) {
        if let Some(session) = self.session_mut(session_id) {
            let cleared = session.sftp.transfer_queue.len();
            session.sftp.transfer_queue.clear();
            session.sftp.message = if cleared == 0 {
                "No queued transfers.".to_owned()
            } else {
                format!("Cleared {cleared} queued transfer(s).")
            };
        }
        cx.notify();
    }

    fn send_sftp_command(&mut self, session_id: u64, command: SftpCommand, cx: &mut Context<Self>) {
        let Some(session) = self.session_mut(session_id) else {
            return;
        };
        let Some(sftp_tx) = &session.sftp_tx else {
            session.sftp.message = "SFTP is not available for this session.".to_owned();
            cx.notify();
            return;
        };
        if let Err(error) = sftp_tx.try_send(command) {
            session.sftp.message = format!("SFTP request queue is unavailable: {error}");
            cx.notify();
        }
    }

    fn toggle_sftp(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let (list_request, opening) = {
            let Some(session) = self.session_mut(session_id) else {
                return;
            };
            session.sftp.visible = !session.sftp.visible;
            if session.sftp.visible {
                session.forwards.visible = false;
            }
            let list_request = if session.sftp.visible && !session.sftp.path.is_empty() {
                Some((session.sftp_tx.clone(), session.sftp.path.clone()))
            } else {
                None
            };
            (list_request, session.sftp.visible)
        };
        if opening {
            self.snippets_visible = false;
            self.settings_visible = false;
            self.keys_visible = false;
            self.keywords_visible = false;
            self.triggers_visible = false;
        }
        if let Some((Some(sftp_tx), path)) = list_request {
            let _ = sftp_tx.try_send(SftpCommand::List(path));
        }
        cx.notify();
    }

    fn open_sftp_directory(&mut self, session_id: u64, path: String, cx: &mut Context<Self>) {
        if let Some(session) = self.session_mut(session_id) {
            session.sftp.path = path.clone();
            session.sftp.message = format!("Loading {path}...");
        }
        self.send_sftp_command(session_id, SftpCommand::List(path), cx);
        cx.notify();
    }

    fn refresh_sftp(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let path = self
            .session(session_id)
            .map(|session| session.sftp.path.clone())
            .unwrap_or_default();
        if !path.is_empty() {
            self.send_sftp_command(session_id, SftpCommand::List(path), cx);
        }
    }

    fn sftp_go_up(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let path = self
            .session(session_id)
            .map(|session| parent_remote_path(&session.sftp.path))
            .unwrap_or_else(|| "/".to_owned());
        self.open_sftp_directory(session_id, path, cx);
    }

    fn begin_upload(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Upload file".into()),
        });
        cx.spawn(async move |this, cx| match receiver.await {
            Ok(Ok(Some(paths))) => {
                let _ = this.update(cx, |view, cx| {
                    for local in paths {
                        view.queue_upload(session_id, local, cx);
                    }
                });
            }
            Ok(Err(error)) => {
                let _ = this.update(cx, |view, cx| {
                    if let Some(session) = view.session_mut(session_id) {
                        session.sftp.message = format!("Could not open a file picker: {error}");
                    }
                    cx.notify();
                });
            }
            _ => {}
        })
        .detach();
    }

    fn queue_upload(&mut self, session_id: u64, local: PathBuf, cx: &mut Context<Self>) {
        let Some(name) = local
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            if let Some(session) = self.session_mut(session_id) {
                session.sftp.message = "The selected file has no name.".to_owned();
            }
            cx.notify();
            return;
        };
        let Some(base) = self
            .session(session_id)
            .map(|session| session.sftp.path.clone())
        else {
            return;
        };
        let remote = remote_join(&base, &name);
        self.enqueue_sftp_transfer(
            session_id,
            SftpCommand::Upload {
                local,
                remote,
                overwrite: false,
                resume: false,
            },
            cx,
        );
    }

    fn begin_download(
        &mut self,
        session_id: u64,
        remote: String,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let Some(local_name) = Path::new(&name)
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
        else {
            if let Some(session) = self.session_mut(session_id) {
                session.sftp.message =
                    "The remote file name cannot be used as a local file name.".to_owned();
            }
            cx.notify();
            return;
        };
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose download folder".into()),
        });
        cx.spawn(async move |this, cx| match receiver.await {
            Ok(Ok(Some(mut paths))) => {
                if let Some(directory) = paths.pop() {
                    let local = directory.join(&local_name);
                    let _ = this.update(cx, |view, cx| {
                        view.enqueue_sftp_transfer(
                            session_id,
                            SftpCommand::Download {
                                remote,
                                local,
                                overwrite: false,
                                resume: false,
                            },
                            cx,
                        );
                    });
                }
            }
            Ok(Err(error)) => {
                let _ = this.update(cx, |view, cx| {
                    if let Some(session) = view.session_mut(session_id) {
                        session.sftp.message = format!("Could not open a folder picker: {error}");
                    }
                    cx.notify();
                });
            }
            _ => {}
        })
        .detach();
    }

    fn begin_upload_directory(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Upload folder".into()),
        });
        cx.spawn(async move |this, cx| match receiver.await {
            Ok(Ok(Some(mut paths))) => {
                if let Some(local) = paths.pop() {
                    let _ = this.update(cx, |view, cx| {
                        view.queue_upload_directory(session_id, local, cx);
                    });
                }
            }
            Ok(Err(error)) => {
                let _ = this.update(cx, |view, cx| {
                    view.set_sftp_message(
                        session_id,
                        format!("Could not open a folder picker: {error}"),
                        cx,
                    );
                });
            }
            _ => {}
        })
        .detach();
    }

    fn queue_upload_directory(&mut self, session_id: u64, local: PathBuf, cx: &mut Context<Self>) {
        let Some(name) = local
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            self.set_sftp_message(session_id, "The selected folder has no name.", cx);
            return;
        };
        let Some(base) = self
            .session(session_id)
            .map(|session| session.sftp.path.clone())
        else {
            return;
        };
        let remote = remote_join(&base, &name);
        self.enqueue_sftp_transfer(
            session_id,
            SftpCommand::UploadDirectory {
                local,
                remote,
                overwrite: false,
                resume: false,
            },
            cx,
        );
    }

    fn begin_download_directory(
        &mut self,
        session_id: u64,
        remote: String,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let Some(local_name) = Path::new(&name)
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
        else {
            self.set_sftp_message(session_id, "The remote folder name is invalid.", cx);
            return;
        };
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose download folder".into()),
        });
        cx.spawn(async move |this, cx| match receiver.await {
            Ok(Ok(Some(mut paths))) => {
                if let Some(directory) = paths.pop() {
                    let local = directory.join(&local_name);
                    let _ = this.update(cx, |view, cx| {
                        view.enqueue_sftp_transfer(
                            session_id,
                            SftpCommand::DownloadDirectory {
                                remote,
                                local,
                                overwrite: false,
                                resume: false,
                            },
                            cx,
                        );
                    });
                }
            }
            Ok(Err(error)) => {
                let _ = this.update(cx, |view, cx| {
                    view.set_sftp_message(
                        session_id,
                        format!("Could not open a folder picker: {error}"),
                        cx,
                    );
                });
            }
            _ => {}
        })
        .detach();
    }

    fn cancel_transfer(&mut self, session_id: u64, cx: &mut Context<Self>) {
        if let Some(session) = self.session_mut(session_id) {
            if let Some(cancellation) = &session.sftp_cancel {
                cancellation.store(true, Ordering::SeqCst);
                session.sftp.message = "Cancelling transfer...".to_owned();
            }
        }
        cx.notify();
    }

    fn confirm_sftp_overwrite(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let command = self
            .session_mut(session_id)
            .and_then(|session| session.sftp.overwrite.take())
            .map(|prompt| sftp_transfer_command(prompt, true, false));
        if let Some(command) = command {
            self.send_sftp_command(session_id, command, cx);
        }
        cx.notify();
    }

    fn confirm_sftp_resume(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let command = self
            .session_mut(session_id)
            .and_then(|session| session.sftp.overwrite.take())
            .map(|prompt| sftp_transfer_command(prompt, false, true));
        if let Some(command) = command {
            self.send_sftp_command(session_id, command, cx);
        }
        cx.notify();
    }

    fn dismiss_sftp_overwrite(&mut self, session_id: u64, cx: &mut Context<Self>) {
        if let Some(session) = self.session_mut(session_id) {
            session.sftp.overwrite = None;
            session.sftp.active_transfer = None;
            session.sftp.message = "Transfer cancelled.".to_owned();
        }
        self.dispatch_next_sftp_transfer(session_id, cx);
        cx.notify();
    }

    fn sftp_panel(&mut self, session_id: u64, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let Some(session) = self.session(session_id) else {
            return div().into_any_element();
        };
        let path = session.sftp.path.clone();
        let entries = session.sftp.entries.clone();
        let message = session.sftp.message.clone();
        let transfer = session.sftp.transfer.clone();
        let queued_transfers = session.sftp.transfer_queue.len();
        let overwrite = session.sftp.overwrite.clone();
        let rename_from = session.sftp.rename_from.clone();
        let pending_delete = session.sftp.pending_delete.clone();
        let metadata_edit = session.sftp.metadata_edit.clone();
        let filesystem = session.sftp.filesystem.clone();
        let filesystem_supported = session.sftp.filesystem_supported;

        let mut list = div()
            .id("sftp-entries")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(1.))
            .overflow_y_scroll();
        if entries.is_empty() {
            list = list.child(
                div()
                    .p(px(8.))
                    .text_size(px(11.))
                    .text_color(tokens.text_disabled)
                    .child(self.tr("No entries")),
            );
        }
        for (index, entry) in entries.into_iter().enumerate() {
            let name = entry.name.clone();
            let entry_path = entry.path.clone();
            let is_dir = entry.is_dir;
            let size = if is_dir {
                String::new()
            } else {
                format_size(entry.size)
            };
            let rename_path = entry.path.clone();
            let rename_name = entry.name.clone();
            let delete_path = entry.path.clone();
            let metadata_path = entry.path.clone();
            let metadata_name = entry.name.clone();
            let metadata_permissions = entry.permissions;
            let metadata_atime = entry.atime;
            let metadata_mtime = entry.mtime;
            let metadata_summary = format!(
                "{} · {}",
                format_permission_mode(entry.permissions),
                entry
                    .mtime
                    .map_or_else(|| "-".to_owned(), |value| value.to_string())
            );
            let mut row = div().flex().items_center().gap(px(4.)).min_w_0();
            if is_dir {
                let download_path = entry.path.clone();
                let download_name = entry.name.clone();
                row = row
                    .child(
                        Button::new(("sftp-dir", index))
                            .label(format!("{name}/"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_sftp_directory(session_id, entry_path.clone(), cx);
                            })),
                    )
                    .child(
                        Button::new(("sftp-download-dir", index))
                            .label(self.tr("Download"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.begin_download_directory(
                                    session_id,
                                    download_path.clone(),
                                    download_name.clone(),
                                    cx,
                                );
                            })),
                    );
            } else {
                row = row
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_size(px(12.))
                            .child(name.clone()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(10.))
                            .text_color(tokens.text_disabled)
                            .child(size),
                    )
                    .child(
                        Button::new(("sftp-download", index))
                            .label(self.tr("Download"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.begin_download(
                                    session_id,
                                    entry_path.clone(),
                                    name.clone(),
                                    cx,
                                );
                            })),
                    );
            }
            row = row
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(9.))
                        .text_color(tokens.text_disabled)
                        .child(metadata_summary),
                )
                .child(
                    components::icon_button(
                        &tokens,
                        ("sftp-metadata", index),
                        IconName::Settings2,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        self.tr("Edit metadata"),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.begin_sftp_metadata(
                            session_id,
                            metadata_path.clone(),
                            metadata_name.clone(),
                            metadata_permissions,
                            metadata_atime,
                            metadata_mtime,
                            window,
                            cx,
                        );
                    })),
                )
                .child(
                    Button::new(("sftp-rename", index))
                        .label(self.tr("Rename"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.begin_sftp_rename(
                                session_id,
                                rename_path.clone(),
                                rename_name.clone(),
                                window,
                                cx,
                            );
                        })),
                )
                .child(
                    Button::new(("sftp-delete", index))
                        .label(self.tr("Delete"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.request_sftp_delete(session_id, delete_path.clone(), is_dir, cx);
                        })),
                );
            list = list.child(row);
        }

        let mut controls = div().flex().items_center().gap(px(4.));
        controls = controls
            .child(
                Button::new("sftp-up")
                    .label(self.tr("Up"))
                    .on_click(cx.listener(move |this, _, _, cx| this.sftp_go_up(session_id, cx))),
            )
            .child(
                Button::new("sftp-refresh")
                    .label(self.tr("Refresh"))
                    .on_click(cx.listener(move |this, _, _, cx| this.refresh_sftp(session_id, cx))),
            )
            .child(
                Button::new("sftp-upload")
                    .label(self.tr("Upload"))
                    .on_click(cx.listener(move |this, _, _, cx| this.begin_upload(session_id, cx))),
            )
            .child(
                Button::new("sftp-upload-dir")
                    .label(self.tr("Folder"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.begin_upload_directory(session_id, cx);
                    })),
            )
            .child(
                components::icon_button(
                    &tokens,
                    "sftp-filesystem-info",
                    IconName::Info,
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    self.tr("Show disk usage"),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.refresh_sftp(session_id, cx);
                })),
            )
            .child(
                Button::new("sftp-close")
                    .label(self.tr("Close"))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_sftp(session_id, cx))),
            );

        let mut panel = div()
            .id("sftp-panel")
            .flex()
            .flex_col()
            .w(px(360.))
            .flex_shrink_0()
            .min_h_0()
            .ml(px(6.))
            .border_1()
            .border_color(tokens.border_strong)
            .bg(tokens.surface_raised)
            .text_color(tokens.text_primary)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(TERMINAL_PANEL_HEADER_PX))
                    .flex_shrink_0()
                    .px(px(8.))
                    .bg(tokens.surface)
                    .child(div().text_size(px(11.)).child("SFTP"))
                    .child(controls),
            )
            .child(
                div()
                    .px(px(8.))
                    .py(px(4.))
                    .text_size(px(10.))
                    .text_color(tokens.text_secondary)
                    .overflow_hidden()
                    .child(if path.is_empty() {
                        self.tr("Waiting for the SFTP subsystem...")
                    } else {
                        path
                    }),
            );
        if let Some(info) = filesystem {
            let used_bytes = info.total_bytes.saturating_sub(info.available_bytes);
            let usage = filesystem_usage_percent(info.total_bytes, info.available_bytes);
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .px(px(8.))
                    .py(px(5.))
                    .border_b_1()
                    .border_color(tokens.border_subtle)
                    .text_size(px(10.))
                    .child(format!(
                        "{}: {}% · {} / {}",
                        self.tr("Disk usage"),
                        usage,
                        format_size(used_bytes),
                        format_size(info.total_bytes)
                    ))
                    .child(format!(
                        "{}: {} · {}: {}",
                        self.tr("Available"),
                        format_size(info.available_bytes),
                        self.tr("Free space"),
                        format_size(info.free_bytes)
                    ))
                    .child(format!(
                        "{}: {} · {}: {}",
                        self.tr("Block size"),
                        format_size(info.block_size),
                        self.tr("Max filename length"),
                        info.name_max
                    )),
            );
        } else if filesystem_supported == Some(false) {
            panel = panel.child(
                div()
                    .px(px(8.))
                    .py(px(5.))
                    .border_b_1()
                    .border_color(tokens.border_subtle)
                    .text_size(px(10.))
                    .text_color(tokens.text_secondary)
                    .child(self.tr("Filesystem statistics unavailable.")),
            );
        }
        if metadata_edit.is_none() {
            let mut operations = div()
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(8.))
                .py(px(4.))
                .child(div().flex_1().child(components::field(
                    &tokens,
                    self.tr("Remote name"),
                    &self.sftp_name,
                    false,
                )));
            if rename_from.is_some() {
                operations = operations
                    .child(
                        Button::new("sftp-rename-apply")
                            .label(self.tr("Rename"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.apply_sftp_rename(session_id, window, cx);
                            })),
                    )
                    .child(
                        Button::new("sftp-rename-cancel")
                            .label(self.tr("Cancel"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.cancel_sftp_rename(session_id, cx);
                            })),
                    );
            } else {
                operations = operations.child(
                    Button::new("sftp-mkdir")
                        .label(self.tr("New folder"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.create_sftp_directory(session_id, cx);
                        })),
                );
            }
            panel = panel.child(operations);
        } else if let Some(metadata) = metadata_edit {
            let metadata_name = metadata.name;
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .px(px(8.))
                    .py(px(6.))
                    .border_b_1()
                    .border_color(tokens.border_subtle)
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_semibold()
                            .child(format!("{}: {metadata_name}", self.tr("Metadata"))),
                    )
                    .child(
                        div().flex().gap(px(4.)).child(
                            components::field(
                                &tokens,
                                self.tr("Permissions (octal)"),
                                &self.sftp_permissions,
                                false,
                            )
                            .flex_1(),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(
                                components::field(
                                    &tokens,
                                    self.tr("Access time (Unix)"),
                                    &self.sftp_atime,
                                    false,
                                )
                                .flex_1(),
                            )
                            .child(
                                components::field(
                                    &tokens,
                                    self.tr("Modified time (Unix)"),
                                    &self.sftp_mtime,
                                    false,
                                )
                                .flex_1(),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                Button::new("sftp-metadata-save")
                                    .label(self.tr("Save metadata"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.apply_sftp_metadata(session_id, window, cx);
                                    })),
                            )
                            .child(
                                Button::new("sftp-metadata-cancel")
                                    .label(self.tr("Cancel"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.cancel_sftp_metadata(session_id, window, cx);
                                    })),
                            ),
                    ),
            );
        }
        panel = panel.child(list);

        if let Some((delete_path, is_dir)) = pending_delete {
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .p(px(8.))
                    .border_t_1()
                    .border_color(tokens.border_subtle)
                    .text_size(px(11.))
                    .child(format!(
                        "Delete {} {delete_path}?",
                        if is_dir {
                            self.tr("directory")
                        } else {
                            self.tr("file")
                        }
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                components::button(
                                    &tokens,
                                    "sftp-delete-cancel",
                                    self.tr("Cancel"),
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.cancel_sftp_delete(session_id, cx);
                                    },
                                )),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "sftp-delete-confirm",
                                    self.tr("Delete"),
                                    ButtonKind::Danger,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.confirm_sftp_delete(session_id, cx);
                                    },
                                )),
                            ),
                    ),
            );
        }

        if let Some(progress) = transfer {
            let percent = if progress.total == 0 {
                0
            } else {
                (progress.transferred.saturating_mul(100) / progress.total).min(100)
            };
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .p(px(8.))
                    .border_t_1()
                    .border_color(tokens.border_subtle)
                    .text_size(px(10.))
                    .child(format!(
                        "{} {} · {percent}% · {} / {}",
                        self.tr(progress.kind.label()),
                        progress.name,
                        format_size(progress.transferred),
                        format_size(progress.total)
                    ))
                    .child(
                        Button::new("sftp-cancel-transfer")
                            .label(self.tr("Cancel transfer"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.cancel_transfer(session_id, cx);
                            })),
                    ),
            );
        }

        if queued_transfers > 0 {
            panel = panel.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(4.))
                    .p(px(8.))
                    .border_t_1()
                    .border_color(tokens.border_subtle)
                    .text_size(px(10.))
                    .child(format!(
                        "{}: {queued_transfers}",
                        self.tr("Queued transfers")
                    ))
                    .child(
                        Button::new("sftp-clear-queue")
                            .label(self.tr("Clear queue"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.clear_sftp_transfer_queue(session_id, cx);
                            })),
                    ),
            );
        }

        if let Some(overwrite) = overwrite {
            let target = if overwrite.kind == TransferKind::Upload {
                overwrite.remote.clone()
            } else {
                overwrite.local.display().to_string()
            };
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .p(px(8.))
                    .border_t_1()
                    .border_color(tokens.border_subtle)
                    .text_size(px(11.))
                    .child(
                        self.tr("{kind} target already exists: {target}")
                            .replace("{kind}", &self.tr(overwrite.kind.label()))
                            .replace("{target}", &target),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                components::button(
                                    &tokens,
                                    "sftp-keep",
                                    self.tr("Keep existing"),
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.dismiss_sftp_overwrite(session_id, cx);
                                    },
                                )),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "sftp-resume",
                                    self.tr("Resume"),
                                    ButtonKind::Secondary,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.confirm_sftp_resume(session_id, cx);
                                    },
                                )),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "sftp-overwrite",
                                    self.tr("Overwrite"),
                                    ButtonKind::Danger,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.confirm_sftp_overwrite(session_id, cx);
                                    },
                                )),
                            ),
                    ),
            );
        }

        if !message.is_empty() {
            panel = panel.child(
                div()
                    .p(px(8.))
                    .border_t_1()
                    .border_color(tokens.border_subtle)
                    .text_size(px(10.))
                    .text_color(tokens.text_secondary)
                    .child(message),
            );
        }

        panel.into_any_element()
    }

    fn apply_forward_event(&mut self, id: u64, event: ForwardEvent) -> bool {
        let Some(session) = self.session_mut(id) else {
            return false;
        };
        let profile_name = session.profile_name.clone();
        let mut persist_add = None;
        let mut persist_remove = None;
        match event {
            ForwardEvent::RuleAdded {
                id: rule_id,
                spec,
                bound_port,
            } => {
                session
                    .forwards
                    .rules
                    .retain(|(existing, _)| *existing != rule_id);
                forget_stopped_forward(&mut session.forwards, &spec);
                session.forwards.rules.push((
                    rule_id,
                    ForwardRuleUiState {
                        spec: spec.clone(),
                        bound_port,
                        connections: 0,
                    },
                ));
                session.forwards.message = if spec.kind == ForwardKind::Dynamic {
                    format!(
                        "Dynamic SOCKS5 proxy listening on {}:{}",
                        spec.listen_host, bound_port
                    )
                } else {
                    format!(
                        "{} forward {}:{} -> {}:{}",
                        spec.kind.label(),
                        spec.listen_host,
                        bound_port,
                        spec.target_host,
                        spec.target_port
                    )
                };
                session.forwards.visible = true;
                persist_add = profile_name.clone().map(|name| (name, spec.clone()));
            }
            ForwardEvent::RuleRemoved { id: rule_id } => {
                let removed = session
                    .forwards
                    .rules
                    .iter()
                    .find(|(existing, _)| *existing == rule_id)
                    .map(|(_, rule)| rule.spec.clone());
                session
                    .forwards
                    .rules
                    .retain(|(existing, _)| *existing != rule_id);
                if let Some(removed) = removed {
                    remember_stopped_forward(&mut session.forwards, removed.clone());
                    persist_remove = profile_name.clone().zip(Some(removed));
                }
                session.forwards.message = "Forwarding rule stopped.".to_owned();
            }
            ForwardEvent::Error { id: rule_id, error } => {
                session.forwards.message = match rule_id {
                    Some(rule_id) => format!("Forwarding rule {rule_id}: {error}"),
                    None => error,
                };
            }
            ForwardEvent::ConnectionOpened {
                id: rule_id,
                peer,
                target,
            } => {
                if let Some((_, rule)) = session
                    .forwards
                    .rules
                    .iter_mut()
                    .find(|(existing, _)| *existing == rule_id)
                {
                    rule.connections = rule.connections.saturating_add(1);
                }
                session.forwards.message = format!("Tunnel {peer} -> {target} opened.");
            }
            ForwardEvent::ConnectionClosed { id: rule_id, peer } => {
                if let Some((_, rule)) = session
                    .forwards
                    .rules
                    .iter_mut()
                    .find(|(existing, _)| *existing == rule_id)
                {
                    rule.connections = rule.connections.saturating_sub(1);
                }
                session.forwards.message = format!("Tunnel {peer} closed.");
            }
        }
        if let Some((name, spec)) = persist_add {
            self.persist_forward_rule(&name, Some(&spec), None);
        }
        if let Some((name, spec)) = persist_remove {
            self.persist_forward_rule(&name, None, Some(&spec));
        }
        true
    }

    fn send_forward_command(
        &mut self,
        session_id: u64,
        command: ForwardCommand,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session_mut(session_id) else {
            return;
        };
        let Some(forward_tx) = &session.forward_tx else {
            session.forwards.message =
                "Port forwarding is not available for this session.".to_owned();
            cx.notify();
            return;
        };
        if let Err(error) = forward_tx.try_send(command) {
            session.forwards.message = format!("Port forwarding queue is unavailable: {error}");
            cx.notify();
        }
    }

    fn toggle_tunnels(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let mut opening = false;
        if let Some(session) = self.session_mut(session_id) {
            session.forwards.visible = !session.forwards.visible;
            if session.forwards.visible {
                session.sftp.visible = false;
                opening = true;
            }
        }
        if opening {
            self.snippets_visible = false;
            self.settings_visible = false;
            self.keys_visible = false;
            self.keywords_visible = false;
            self.triggers_visible = false;
        }
        cx.notify();
    }

    fn add_forward_rule(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let listen_host = self.forward_listen_host.read(cx).value().trim().to_owned();
        let listen_port = self.forward_listen_port.read(cx).value().parse::<u16>();
        let target_host = self.forward_target_host.read(cx).value().trim().to_owned();
        let target_port = self.forward_target_port.read(cx).value().parse::<u16>();
        let Ok(listen_port) = listen_port else {
            self.set_forward_message(session_id, "Listen port must be between 0 and 65535.", cx);
            return;
        };
        if listen_host.is_empty() {
            self.set_forward_message(session_id, "A listen address is required.", cx);
            return;
        }
        let (target_host, target_port) = if self.forward_kind.needs_target() {
            let Ok(target_port) = target_port else {
                self.set_forward_message(
                    session_id,
                    "Target port must be between 1 and 65535.",
                    cx,
                );
                return;
            };
            if target_host.is_empty() || target_port == 0 {
                self.set_forward_message(
                    session_id,
                    "A target host and valid target port are required.",
                    cx,
                );
                return;
            }
            (target_host, target_port)
        } else {
            (String::new(), 0)
        };
        let kind = self.forward_kind;
        self.send_forward_command(
            session_id,
            ForwardCommand::Add(ForwardRuleSpec {
                kind,
                listen_host,
                listen_port,
                target_host,
                target_port,
            }),
            cx,
        );
    }

    fn remove_forward_rule(&mut self, session_id: u64, rule_id: u64, cx: &mut Context<Self>) {
        self.send_forward_command(session_id, ForwardCommand::Remove(rule_id), cx);
    }

    fn restart_forward_rule(&mut self, session_id: u64, index: usize, cx: &mut Context<Self>) {
        let spec = self.session_mut(session_id).and_then(|session| {
            (index < session.forwards.stopped.len()).then(|| session.forwards.stopped.remove(index))
        });
        if let Some(spec) = spec {
            self.send_forward_command(session_id, ForwardCommand::Add(spec), cx);
        }
    }

    fn set_forward_message(
        &mut self,
        session_id: u64,
        message: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.session_mut(session_id) {
            session.forwards.message = message.into();
        }
        cx.notify();
    }

    fn forward_panel(&mut self, session_id: u64, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let Some(session) = self.session(session_id) else {
            return div().into_any_element();
        };
        let kind = self.forward_kind;
        let rules = session.forwards.rules.clone();
        let message = session.forwards.message.clone();

        let mut kind_buttons = div().flex().items_center().gap(px(4.));
        for (index, candidate) in ForwardKind::ALL.into_iter().enumerate() {
            let button = Button::new(("forward-kind", index)).label(self.tr(candidate.label()));
            let button = if kind == candidate {
                button.primary()
            } else {
                button
            };
            kind_buttons =
                kind_buttons.child(button.on_click(cx.listener(move |this, _, _, cx| {
                    this.forward_kind = candidate;
                    cx.notify();
                })));
        }

        let mut form = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(8.))
            .child(kind_buttons)
            .child(components::field(
                &tokens,
                self.tr("Listen host"),
                &self.forward_listen_host,
                false,
            ))
            .child(components::field(
                &tokens,
                self.tr("Listen port (0 = auto)"),
                &self.forward_listen_port,
                false,
            ));
        if kind.needs_target() {
            form = form
                .child(components::field(
                    &tokens,
                    self.tr("Target host"),
                    &self.forward_target_host,
                    false,
                ))
                .child(components::field(
                    &tokens,
                    self.tr("Target port"),
                    &self.forward_target_port,
                    false,
                ));
        } else {
            form = form.child(
                div()
                    .text_size(px(10.))
                    .text_color(tokens.text_secondary)
                    .child(self.tr("SOCKS5 clients choose the target host and port.")),
            );
        }
        form = form.child(
            Button::new("forward-add")
                .label(self.tr("Add tunnel"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.add_forward_rule(session_id, cx);
                })),
        );

        let mut rule_list = div()
            .id("forward-rules")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(4.))
            .overflow_y_scroll();
        if rules.is_empty() {
            rule_list = rule_list.child(
                div()
                    .p(px(8.))
                    .text_size(px(11.))
                    .text_color(tokens.text_disabled)
                    .child(self.tr("No forwarding rules")),
            );
        }
        for (rule_id, rule) in rules {
            let spec = rule.spec.clone();
            let connection_word = if rule.connections == 1 {
                self.tr("connection")
            } else {
                self.tr("connections")
            };
            let summary = if spec.kind == ForwardKind::Dynamic {
                format!(
                    "{} {}:{} · {} {}",
                    self.tr(spec.kind.label()),
                    spec.listen_host,
                    rule.bound_port,
                    rule.connections,
                    connection_word,
                )
            } else {
                format!(
                    "{} {}:{} -> {}:{} · {} {}",
                    self.tr(spec.kind.label()),
                    spec.listen_host,
                    rule.bound_port,
                    spec.target_host,
                    spec.target_port,
                    rule.connections,
                    connection_word,
                )
            };
            rule_list = rule_list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(div().flex_1().min_w_0().text_size(px(11.)).child(summary))
                    .child(
                        Button::new(("forward-remove", rule_id))
                            .label(self.tr("Stop"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_forward_rule(session_id, rule_id, cx);
                            })),
                    ),
            );
        }

        div()
            .id("forward-panel")
            .flex()
            .flex_col()
            .w(px(340.))
            .flex_shrink_0()
            .min_h_0()
            .ml(px(6.))
            .border_1()
            .border_color(tokens.border_strong)
            .bg(tokens.surface_raised)
            .text_color(tokens.text_primary)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(TERMINAL_PANEL_HEADER_PX))
                    .flex_shrink_0()
                    .px(px(8.))
                    .bg(tokens.surface)
                    .child(div().text_size(px(11.)).child(self.tr("Tunnels")))
                    .child(
                        Button::new("forward-close")
                            .label(self.tr("Close"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_tunnels(session_id, cx)
                            })),
                    ),
            )
            .child(form)
            .child(
                div()
                    .px(px(8.))
                    .text_size(px(11.))
                    .text_color(tokens.text_secondary)
                    .child(message),
            )
            .child(rule_list)
            .into_any_element()
    }

    fn toggle_log(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let Some(session) = self.session(session_id) else {
            return;
        };
        if session.state != SessionState::Connected {
            return;
        }
        if session.log_path.is_some() {
            self.send_terminal_request(
                session_id,
                TerminalRequest::StopLog,
                "Terminal logging",
                cx,
            );
            return;
        }
        let suggested = format!(
            "{}.log",
            session
                .title
                .chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                        character
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
        );
        let directory = home_directory();
        let receiver = cx.prompt_for_new_path(&directory, Some(&suggested));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = receiver.await {
                this.update(cx, |view, cx| {
                    view.send_terminal_request(
                        session_id,
                        TerminalRequest::StartLog(path),
                        "Terminal logging",
                        cx,
                    );
                })
                .ok();
            }
        })
        .detach();
    }

    fn new_snippet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        Self::set_input_value(&self.snippet_name, "", window, cx);
        Self::set_input_value(&self.snippet_command, "", window, cx);
        self.snippet_send_enter = true;
        self.editing_snippet = None;
        self.snippet_message.clear();
        cx.notify();
    }

    fn load_snippet(
        &mut self,
        snippet: &CommandSnippet,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Self::set_input_value(&self.snippet_name, &snippet.name, window, cx);
        Self::set_input_value(&self.snippet_command, &snippet.command, window, cx);
        self.snippet_send_enter = snippet.send_enter;
        self.editing_snippet = Some(snippet.name.clone());
        self.snippet_message = self.tr("Editing {name}.").replace("{name}", &snippet.name);
        cx.notify();
    }

    fn save_snippet(&mut self, cx: &mut Context<Self>) {
        let name = self.snippet_name.read(cx).value().trim().to_owned();
        let command = self.snippet_command.read(cx).value().trim().to_owned();
        if name.is_empty() || command.is_empty() {
            self.snippet_message = self.tr("Snippet name and command are required.");
            cx.notify();
            return;
        }
        let Some(store) = self.snippet_store.as_ref() else {
            self.snippet_message = self.tr("Command snippet storage is unavailable.");
            cx.notify();
            return;
        };
        if self.snippets.iter().any(|snippet| {
            snippet.name.eq_ignore_ascii_case(&name)
                && self
                    .editing_snippet
                    .as_ref()
                    .is_none_or(|editing| !editing.eq_ignore_ascii_case(&snippet.name))
        }) {
            self.snippet_message = self
                .tr("A snippet named {name} already exists.")
                .replace("{name}", &name);
            cx.notify();
            return;
        }
        let mut snippets = self.snippets.clone();
        if let Some(editing) = &self.editing_snippet {
            snippets.retain(|snippet| !snippet.name.eq_ignore_ascii_case(editing));
        }
        if snippets.len() >= MAX_SNIPPETS {
            self.snippet_message = self
                .tr("Cannot have more than {count} snippets.")
                .replace("{count}", &MAX_SNIPPETS.to_string());
            cx.notify();
            return;
        }
        snippets.push(CommandSnippet {
            name: name.clone(),
            command,
            send_enter: self.snippet_send_enter,
        });
        snippets.sort_by_key(|snippet| snippet.name.to_lowercase());
        if let Err(error) = store.save(&snippets) {
            self.snippet_message = error;
            cx.notify();
            return;
        }
        self.snippets = snippets;
        self.editing_snippet = Some(name.clone());
        self.snippet_message = self.tr("Saved {name}.").replace("{name}", &name);
        cx.notify();
    }

    fn delete_snippet(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = self.snippet_store.as_ref() else {
            self.snippet_message = self.tr("Command snippet storage is unavailable.");
            cx.notify();
            return;
        };
        let mut snippets = self.snippets.clone();
        snippets.retain(|snippet| !snippet.name.eq_ignore_ascii_case(name));
        if let Err(error) = store.save(&snippets) {
            self.snippet_message = error;
            cx.notify();
            return;
        }
        self.snippets = snippets;
        if self
            .editing_snippet
            .as_ref()
            .is_some_and(|editing| editing.eq_ignore_ascii_case(name))
        {
            self.new_snippet(window, cx);
        }
        self.snippet_message = self.tr("Deleted {name}.").replace("{name}", name);
        cx.notify();
    }

    fn toggle_snippets(&mut self, cx: &mut Context<Self>) {
        self.snippets_visible = !self.snippets_visible;
        if self.snippets_visible {
            self.settings_visible = false;
            self.keys_visible = false;
            self.keywords_visible = false;
            self.triggers_visible = false;
        }
        if self.snippets_visible
            && let Some(session_id) = self.active_session_id
            && let Some(session) = self.session_mut(session_id)
        {
            session.sftp.visible = false;
            session.forwards.visible = false;
        }
        cx.notify();
    }

    fn toggle_keywords(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.keywords_visible = !self.keywords_visible;
        if self.keywords_visible {
            self.snippets_visible = false;
            self.settings_visible = false;
            self.keys_visible = false;
            self.triggers_visible = false;
            if let Some(session_id) = self.active_session_id
                && let Some(session) = self.session_mut(session_id)
            {
                session.sftp.visible = false;
                session.forwards.visible = false;
            }
            let focus = self.keyword_input.read(cx).focus_handle(cx);
            window.focus(&focus);
        } else {
            window.focus(&self.terminal_focus);
        }
        cx.notify();
    }

    fn toggle_triggers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.triggers_visible = !self.triggers_visible;
        if self.triggers_visible {
            self.keywords_visible = false;
            self.snippets_visible = false;
            self.settings_visible = false;
            self.keys_visible = false;
            if let Some(session_id) = self.active_session_id
                && let Some(session) = self.session_mut(session_id)
            {
                session.sftp.visible = false;
                session.forwards.visible = false;
            }
            let focus = self.trigger_keyword_input.read(cx).focus_handle(cx);
            window.focus(&focus);
        } else {
            window.focus(&self.terminal_focus);
        }
        cx.notify();
    }

    fn toggle_trigger_engine(&mut self, cx: &mut Context<Self>) {
        self.trigger_engine_enabled = !self.trigger_engine_enabled;
        if !self.trigger_engine_enabled {
            self.trigger_latches.clear();
        }
        self.trigger_message = if self.trigger_engine_enabled {
            self.tr("Trigger engine enabled.")
        } else {
            self.tr("Trigger engine disabled.")
        };
        cx.notify();
    }

    fn set_trigger_action_kind(&mut self, kind: TriggerActionKind, cx: &mut Context<Self>) {
        self.trigger_action_kind = kind;
        cx.notify();
    }

    fn toggle_trigger_send_enter(&mut self, cx: &mut Context<Self>) {
        self.trigger_send_enter = !self.trigger_send_enter;
        cx.notify();
    }

    fn add_trigger(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let keyword = match normalize_keyword(self.trigger_keyword_input.read(cx).value().as_str())
        {
            Ok(keyword) => keyword,
            Err(error) => {
                self.trigger_message = self.tr(error);
                cx.notify();
                return;
            }
        };
        if self.trigger_rules.len() >= MAX_TRIGGER_RULES {
            self.trigger_message = self.tr("Triggers are limited to 32 rules.");
            cx.notify();
            return;
        }
        if self
            .trigger_rules
            .iter()
            .any(|rule| rule.keyword.eq_ignore_ascii_case(&keyword))
        {
            self.trigger_message = format!("{} {keyword}", self.tr("Trigger is already listed:"));
            cx.notify();
            return;
        }
        let action = match self.trigger_action_kind {
            TriggerActionKind::Highlight => TriggerAction::Highlight,
            TriggerActionKind::Notify => TriggerAction::Notify,
            TriggerActionKind::SendText => {
                let text =
                    match normalize_trigger_text(self.trigger_text_input.read(cx).value().as_str())
                    {
                        Ok(text) => text,
                        Err(error) => {
                            self.trigger_message = self.tr(error);
                            cx.notify();
                            return;
                        }
                    };
                TriggerAction::SendText {
                    text,
                    send_enter: self.trigger_send_enter,
                }
            }
        };
        self.trigger_rules.push(TriggerRule {
            keyword: keyword.clone(),
            action,
            enabled: false,
        });
        self.trigger_message = format!("{} {keyword}", self.tr("Added disabled trigger:"));
        Self::set_input_value(&self.trigger_keyword_input, "", window, cx);
        Self::set_input_value(&self.trigger_text_input, "", window, cx);
        cx.notify();
    }

    fn toggle_trigger_rule(&mut self, keyword: &str, cx: &mut Context<Self>) {
        let Some(index) = self
            .trigger_rules
            .iter()
            .position(|rule| rule.keyword.eq_ignore_ascii_case(keyword))
        else {
            cx.notify();
            return;
        };
        let (enabled, rule_keyword) = {
            let rule = &mut self.trigger_rules[index];
            rule.enabled = !rule.enabled;
            let key = rule.match_key();
            for latched in self.trigger_latches.values_mut() {
                latched.remove(&key);
            }
            (rule.enabled, rule.keyword.clone())
        };
        self.trigger_message = if enabled {
            format!("{} {rule_keyword}", self.tr("Enabled trigger:"))
        } else {
            format!("{} {rule_keyword}", self.tr("Disabled trigger:"))
        };
        cx.notify();
    }

    fn remove_trigger(&mut self, keyword: &str, cx: &mut Context<Self>) {
        self.trigger_rules
            .retain(|rule| !rule.keyword.eq_ignore_ascii_case(keyword));
        let key = keyword.to_lowercase();
        for latched in self.trigger_latches.values_mut() {
            latched.remove(&key);
        }
        self.trigger_message = format!("{} {keyword}", self.tr("Removed trigger:"));
        cx.notify();
    }

    fn process_triggers(&mut self, session_id: u64, frame: &TerminalFrame, cx: &mut Context<Self>) {
        if !self.trigger_engine_enabled {
            self.trigger_latches.remove(&session_id);
            return;
        }
        let matches = matching_keys(&triggers::frame_text(frame), &self.trigger_rules);
        let newly_matched = {
            let previous = self.trigger_latches.entry(session_id).or_default();
            let newly_matched = self
                .trigger_rules
                .iter()
                .filter(|rule| {
                    let key = rule.match_key();
                    matches.contains(&key) && !previous.contains(&key)
                })
                .cloned()
                .collect::<Vec<_>>();
            previous.retain(|key| matches.contains(key));
            previous.extend(matches.iter().cloned());
            newly_matched
        };
        for rule in newly_matched {
            self.fire_trigger(session_id, &rule, cx);
        }
    }

    fn fire_trigger(&mut self, session_id: u64, rule: &TriggerRule, cx: &mut Context<Self>) {
        match &rule.action {
            TriggerAction::Highlight => {}
            TriggerAction::Notify => {
                self.show_toast(
                    format!("{} {}", self.tr("Trigger matched:"), rule.keyword),
                    Tone::Warning,
                    cx,
                );
            }
            TriggerAction::SendText { text, send_enter } => {
                if self
                    .session(session_id)
                    .is_none_or(|session| session.state != SessionState::Connected)
                {
                    return;
                }
                if let Err(error) = enqueue_session_request(
                    &self.sessions,
                    session_id,
                    TerminalRequest::Paste(text.clone()),
                ) {
                    self.show_toast(
                        format!("{} {error}", self.tr("Trigger could not send:")),
                        Tone::Danger,
                        cx,
                    );
                    return;
                }
                if *send_enter {
                    let key = TerminalKeyInput {
                        key: "enter".to_owned(),
                        text: None,
                        control: false,
                        alt: false,
                        shift: false,
                        platform: false,
                        function: false,
                    };
                    if let Err(error) = enqueue_session_request(
                        &self.sessions,
                        session_id,
                        TerminalRequest::Key(key),
                    ) {
                        self.show_toast(
                            format!("{} {error}", self.tr("Trigger could not send Enter:")),
                            Tone::Danger,
                            cx,
                        );
                        return;
                    }
                }
                self.show_toast(
                    format!(
                        "{} {}",
                        self.tr("Trigger sent preset text for:"),
                        rule.keyword
                    ),
                    Tone::Accent,
                    cx,
                );
            }
        }
    }

    fn trigger_highlight_keywords(&self) -> Vec<String> {
        if !self.trigger_engine_enabled {
            return Vec::new();
        }
        self.trigger_rules
            .iter()
            .filter(|rule| rule.enabled && matches!(&rule.action, TriggerAction::Highlight))
            .map(|rule| rule.keyword.clone())
            .collect()
    }

    fn toggle_keyword_highlighting(&mut self, cx: &mut Context<Self>) {
        self.keyword_highlighting_enabled = !self.keyword_highlighting_enabled;
        self.keyword_message = if self.keyword_highlighting_enabled {
            self.tr("Keyword highlighting enabled.")
        } else {
            self.tr("Keyword highlighting disabled.")
        };
        cx.notify();
    }

    fn add_keyword(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let keyword = self.keyword_input.read(cx).value().trim().to_owned();
        if keyword.is_empty() {
            self.keyword_message = self.tr("Enter a keyword to highlight.");
            cx.notify();
            return;
        }
        if self.keyword_highlights.len() >= MAX_KEYWORD_HIGHLIGHTS {
            self.keyword_message = self
                .tr("Keyword highlights are limited to {count} entries.")
                .replace("{count}", &MAX_KEYWORD_HIGHLIGHTS.to_string());
            cx.notify();
            return;
        }
        if self
            .keyword_highlights
            .iter()
            .any(|item| item.eq_ignore_ascii_case(&keyword))
        {
            self.keyword_message = self
                .tr("Keyword {keyword} is already listed.")
                .replace("{keyword}", &keyword);
            cx.notify();
            return;
        }
        self.keyword_highlights.push(keyword.clone());
        self.keyword_message = self.tr("Added {keyword}.").replace("{keyword}", &keyword);
        Self::set_input_value(&self.keyword_input, "", window, cx);
        cx.notify();
    }

    fn remove_keyword(&mut self, keyword: &str, cx: &mut Context<Self>) {
        self.keyword_highlights
            .retain(|item| !item.eq_ignore_ascii_case(keyword));
        if self.keyword_highlights.is_empty() {
            self.keyword_highlighting_enabled = false;
        }
        self.keyword_message = self.tr("Removed {keyword}.").replace("{keyword}", keyword);
        cx.notify();
    }

    fn toggle_snippet_target(&mut self, session_id: u64, cx: &mut Context<Self>) {
        if !self.snippet_targets.remove(&session_id) {
            self.snippet_targets.insert(session_id);
        }
        cx.notify();
    }

    fn send_snippet_to_active(&mut self, snippet: &CommandSnippet, cx: &mut Context<Self>) {
        let Some(session_id) = self.active_session_id else {
            self.snippet_message = self.tr("There is no active session.");
            cx.notify();
            return;
        };
        if self.send_command_to_session(session_id, &snippet.command, snippet.send_enter, cx) {
            self.snippet_message = self
                .tr("Sent {name} to the active session.")
                .replace("{name}", &snippet.name);
        } else {
            self.snippet_message = self.tr("The active session cannot accept commands.");
        }
        cx.notify();
    }

    fn broadcast_snippet(&mut self, cx: &mut Context<Self>) {
        let command = self.snippet_command.read(cx).value().trim().to_owned();
        if command.is_empty() {
            self.snippet_message = self.tr("Enter a command before broadcasting.");
            cx.notify();
            return;
        }
        let send_enter = self.snippet_send_enter;
        let targets = self
            .snippet_targets
            .iter()
            .copied()
            .filter(|id| {
                self.session(*id)
                    .is_some_and(|session| session.state == SessionState::Connected)
            })
            .collect::<Vec<_>>();
        if targets.is_empty() {
            self.snippet_message = self.tr("Select at least one connected target session.");
            cx.notify();
            return;
        }
        let mut sent_to = Vec::new();
        for session_id in targets {
            let title = self
                .session(session_id)
                .map(|session| session.title.clone())
                .unwrap_or_else(|| session_id.to_string());
            if self.send_command_to_session(session_id, &command, send_enter, cx) {
                sent_to.push(title);
            }
        }
        self.snippet_message = if sent_to.is_empty() {
            self.tr("No selected session could accept the command.")
        } else {
            self.tr("Sent to: {targets}.")
                .replace("{targets}", &sent_to.join(", "))
        };
        cx.notify();
    }

    fn send_command_to_session(
        &mut self,
        session_id: u64,
        command: &str,
        send_enter: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if self
            .session(session_id)
            .is_none_or(|session| session.state != SessionState::Connected)
        {
            return false;
        }
        if let Err(error) = enqueue_session_request(
            &self.sessions,
            session_id,
            TerminalRequest::Paste(command.to_owned()),
        ) {
            self.snippet_message = self
                .tr("Could not send the command: {error}")
                .replace("{error}", &error.to_string());
            return false;
        }
        if send_enter {
            let key = TerminalKeyInput {
                key: "enter".to_owned(),
                text: None,
                control: false,
                alt: false,
                shift: false,
                platform: false,
                function: false,
            };
            if let Err(error) =
                enqueue_session_request(&self.sessions, session_id, TerminalRequest::Key(key))
            {
                self.snippet_message = self
                    .tr("Could not send the command: {error}")
                    .replace("{error}", &error.to_string());
                return false;
            }
        }
        cx.notify();
        true
    }

    fn refresh_server_metrics(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let not_connected = self.tr("Connect to the server before refreshing metrics.");
        let not_accepting = self.tr("The SSH session is not accepting requests.");
        let queue_full = self.tr("The SSH request queue is full.");
        let Some(session) = self.session_mut(session_id) else {
            return;
        };
        if session.state != SessionState::Connected {
            session.server_metrics.message = not_connected;
            cx.notify();
            return;
        }
        let Some(command_tx) = &session.command_tx else {
            session.server_metrics.message = not_accepting;
            cx.notify();
            return;
        };
        if let Err(error) = command_tx.try_send(TerminalRequest::RefreshServerMetrics) {
            session.server_metrics.message = match error {
                mpsc::error::TrySendError::Full(_) => queue_full,
                mpsc::error::TrySendError::Closed(_) => not_accepting,
            };
            session.server_metrics.loading = false;
        } else {
            session.server_metrics.loading = true;
            session.server_metrics.message.clear();
        }
        cx.notify();
    }

    fn record_server_alerts(&mut self, session_id: u64) -> bool {
        let observations = {
            let Some(session) = self.session(session_id) else {
                return false;
            };
            server_alert_observations(
                session.server_metrics.metrics.as_ref(),
                &session.server_metrics.processes,
                self.settings.alert_cpu_percent,
                self.settings.alert_memory_percent,
                self.settings.alert_disk_percent,
            )
        };
        let Some(session) = self.session_mut(session_id) else {
            return false;
        };
        let previous = std::mem::replace(
            &mut session.server_metrics.active_alerts,
            observations.clone(),
        );
        let mut changed = previous != observations;
        for observation in observations {
            if previous.contains(&observation) {
                continue;
            }
            session
                .server_metrics
                .alert_history
                .push(ServerAlertRecord {
                    kind: observation.kind,
                    value: observation.value,
                    threshold: observation.threshold,
                    observed_at: unix_now(),
                });
            changed = true;
        }
        if session.server_metrics.alert_history.len() > MAX_SERVER_ALERT_HISTORY {
            let excess = session.server_metrics.alert_history.len() - MAX_SERVER_ALERT_HISTORY;
            session.server_metrics.alert_history.drain(0..excess);
            changed = true;
        }
        changed
    }

    fn clear_server_alert_history(&mut self, session_id: u64, cx: &mut Context<Self>) {
        if let Some(session) = self.session_mut(session_id) {
            session.server_metrics.alert_history.clear();
            session.server_metrics.active_alerts.clear();
            cx.notify();
        }
    }

    fn refresh_server_processes(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let not_connected = self.tr("Connect to the server before refreshing processes.");
        let not_accepting = self.tr("The SSH session is not accepting requests.");
        let queue_full = self.tr("The SSH request queue is full.");
        let Some(session) = self.session_mut(session_id) else {
            return;
        };
        if session.state != SessionState::Connected {
            session.server_metrics.processes_message = not_connected;
            cx.notify();
            return;
        }
        let Some(command_tx) = &session.command_tx else {
            session.server_metrics.processes_message = not_accepting;
            cx.notify();
            return;
        };
        if let Err(error) = command_tx.try_send(TerminalRequest::RefreshServerProcesses) {
            session.server_metrics.processes_message = match error {
                mpsc::error::TrySendError::Full(_) => queue_full,
                mpsc::error::TrySendError::Closed(_) => not_accepting,
            };
            session.server_metrics.processes_loading = false;
        } else {
            session.server_metrics.processes_loading = true;
            session.server_metrics.processes_message.clear();
        }
        cx.notify();
    }

    fn refresh_server_network(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let not_connected = self.tr("Connect to the server before refreshing network information.");
        let not_accepting = self.tr("The SSH session is not accepting requests.");
        let queue_full = self.tr("The SSH request queue is full.");
        let Some(session) = self.session_mut(session_id) else {
            return;
        };
        if session.state != SessionState::Connected {
            session.server_metrics.network_message = not_connected;
            cx.notify();
            return;
        }
        let Some(command_tx) = &session.command_tx else {
            session.server_metrics.network_message = not_accepting;
            cx.notify();
            return;
        };
        if let Err(error) = command_tx.try_send(TerminalRequest::RefreshServerNetwork) {
            session.server_metrics.network_message = match error {
                mpsc::error::TrySendError::Full(_) => queue_full,
                mpsc::error::TrySendError::Closed(_) => not_accepting,
            };
            session.server_metrics.network_loading = false;
        } else {
            session.server_metrics.network_loading = true;
            session.server_metrics.network_message.clear();
        }
        cx.notify();
    }

    fn refresh_server_storage(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let not_connected = self.tr("Connect to the server before refreshing storage information.");
        let not_accepting = self.tr("The SSH session is not accepting requests.");
        let queue_full = self.tr("The SSH request queue is full.");
        let Some(session) = self.session_mut(session_id) else {
            return;
        };
        if session.state != SessionState::Connected {
            session.server_metrics.storage_message = not_connected;
            cx.notify();
            return;
        }
        let Some(command_tx) = &session.command_tx else {
            session.server_metrics.storage_message = not_accepting;
            cx.notify();
            return;
        };
        if let Err(error) = command_tx.try_send(TerminalRequest::RefreshServerStorage) {
            session.server_metrics.storage_message = match error {
                mpsc::error::TrySendError::Full(_) => queue_full,
                mpsc::error::TrySendError::Closed(_) => not_accepting,
            };
            session.server_metrics.storage_loading = false;
        } else {
            session.server_metrics.storage_loading = true;
            session.server_metrics.storage_message.clear();
        }
        cx.notify();
    }

    fn refresh_server_listeners(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let not_connected = self.tr("Connect to the server before refreshing listening ports.");
        let not_accepting = self.tr("The SSH session is not accepting requests.");
        let queue_full = self.tr("The SSH request queue is full.");
        let Some(session) = self.session_mut(session_id) else {
            return;
        };
        if session.state != SessionState::Connected {
            session.server_metrics.listeners_message = not_connected;
            cx.notify();
            return;
        }
        let Some(command_tx) = &session.command_tx else {
            session.server_metrics.listeners_message = not_accepting;
            cx.notify();
            return;
        };
        if let Err(error) = command_tx.try_send(TerminalRequest::RefreshServerListeners) {
            session.server_metrics.listeners_message = match error {
                mpsc::error::TrySendError::Full(_) => queue_full,
                mpsc::error::TrySendError::Closed(_) => not_accepting,
            };
            session.server_metrics.listeners_loading = false;
        } else {
            session.server_metrics.listeners_loading = true;
            session.server_metrics.listeners_message.clear();
        }
        cx.notify();
    }

    fn request_process_termination(&mut self, session_id: u64, pid: u32, cx: &mut Context<Self>) {
        let not_connected = self.tr("Connect to the server before terminating a process.");
        if self
            .session(session_id)
            .is_none_or(|session| session.state != SessionState::Connected)
        {
            if let Some(session) = self.session_mut(session_id) {
                session.server_metrics.processes_message = not_connected;
            }
            cx.notify();
            return;
        }
        self.pending_process_termination = Some((session_id, pid));
        self.request_overlay_focus();
        cx.notify();
    }

    fn cancel_process_termination(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pending_process_termination = None;
        self.overlay_focus_pending = false;
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn submit_process_termination(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((session_id, pid)) = self.pending_process_termination.take() else {
            return;
        };
        let not_connected = self.tr("Connect to the server before terminating a process.");
        let not_accepting = self.tr("The SSH session is not accepting requests.");
        let queue_full = self.tr("The SSH request queue is full.");
        let applying = self.tr("Requesting process termination...");
        let Some(session) = self.session_mut(session_id) else {
            self.overlay_focus_pending = false;
            window.focus(&self.terminal_focus);
            return;
        };
        if session.state != SessionState::Connected {
            session.server_metrics.processes_message = not_connected;
        } else if let Some(command_tx) = &session.command_tx {
            match command_tx.try_send(TerminalRequest::TerminateServerProcess { pid }) {
                Ok(()) => {
                    session.server_metrics.processes_loading = true;
                    session.server_metrics.processes_message = applying;
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    session.server_metrics.processes_message = queue_full;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    session.server_metrics.processes_message = not_accepting;
                }
            }
        } else {
            session.server_metrics.processes_message = not_accepting;
        }
        self.overlay_focus_pending = false;
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn process_termination_overlay(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let Some((_, pid)) = self.pending_process_termination else {
            return div().into_any_element();
        };
        components::modal_overlay(&tokens, "process-termination-overlay")
            .track_focus(&self.overlay_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.cancel_process_termination(window, cx);
                } else if event.keystroke.key.eq_ignore_ascii_case("enter") {
                    cx.stop_propagation();
                    this.submit_process_termination(window, cx);
                }
            }))
            .child(
                components::modal_panel(&tokens, "process-termination-panel")
                    .w(px(ui_tokens::MODAL_WIDTH_KEYGEN))
                    .child(
                        components::modal_header(&tokens, self.tr("Terminate process")).child(
                            components::icon_button(
                                &tokens,
                                "process-termination-close",
                                IconName::Close,
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                                self.tr("Close"),
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.cancel_process_termination(window, cx);
                                },
                            )),
                        ),
                    )
                    .child(components::warning_text(
                        &tokens,
                        self.tr(
                            "This sends SIGTERM to the remote process. Confirm before continuing.",
                        ),
                    ))
                    .child(components::info_row(
                        &tokens,
                        self.tr("PID"),
                        pid.to_string(),
                    ))
                    .child(
                        components::modal_footer(&tokens)
                            .child(
                                components::button(
                                    &tokens,
                                    "process-termination-cancel",
                                    self.tr("Cancel"),
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.cancel_process_termination(window, cx);
                                    },
                                )),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "process-termination-confirm",
                                    self.tr("Terminate"),
                                    ButtonKind::Danger,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.submit_process_termination(window, cx);
                                    },
                                )),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn refresh_server_services(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let not_connected = self.tr("Connect to the server before refreshing services.");
        let not_accepting = self.tr("The SSH session is not accepting requests.");
        let queue_full = self.tr("The SSH request queue is full.");
        let Some(session) = self.session_mut(session_id) else {
            return;
        };
        if session.state != SessionState::Connected {
            session.server_metrics.services_message = not_connected;
            cx.notify();
            return;
        }
        let Some(command_tx) = &session.command_tx else {
            session.server_metrics.services_message = not_accepting;
            cx.notify();
            return;
        };
        if let Err(error) = command_tx.try_send(TerminalRequest::RefreshServerServices) {
            session.server_metrics.services_message = match error {
                mpsc::error::TrySendError::Full(_) => queue_full,
                mpsc::error::TrySendError::Closed(_) => not_accepting,
            };
            session.server_metrics.services_loading = false;
        } else {
            session.server_metrics.services_loading = true;
            session.server_metrics.services_message.clear();
        }
        cx.notify();
    }

    fn read_server_service_logs(&mut self, session_id: u64, name: String, cx: &mut Context<Self>) {
        let not_connected = self.tr("Connect to the server before reading service logs.");
        let not_accepting = self.tr("The SSH session is not accepting requests.");
        let queue_full = self.tr("The SSH request queue is full.");
        let Some(session) = self.session_mut(session_id) else {
            return;
        };
        if session.state != SessionState::Connected {
            session.server_metrics.service_logs_message = not_connected;
            cx.notify();
            return;
        }
        let Some(command_tx) = &session.command_tx else {
            session.server_metrics.service_logs_message = not_accepting;
            cx.notify();
            return;
        };
        match command_tx.try_send(TerminalRequest::ReadServerServiceLogs { name }) {
            Ok(()) => {
                session.server_metrics.service_logs_loading = true;
                session.server_metrics.service_logs = None;
                session.server_metrics.service_logs_message.clear();
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                session.server_metrics.service_logs_message = queue_full;
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                session.server_metrics.service_logs_message = not_accepting;
            }
        }
        cx.notify();
    }

    fn request_service_action(
        &mut self,
        session_id: u64,
        name: String,
        action: ServiceAction,
        cx: &mut Context<Self>,
    ) {
        let not_connected = self.tr("Connect to the server before changing services.");
        if self
            .session(session_id)
            .is_none_or(|session| session.state != SessionState::Connected)
        {
            if let Some(session) = self.session_mut(session_id) {
                session.server_metrics.services_message = not_connected;
            }
            cx.notify();
            return;
        }
        self.pending_service_action = Some((session_id, name, action));
        self.request_overlay_focus();
        cx.notify();
    }

    fn cancel_service_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pending_service_action = None;
        self.overlay_focus_pending = false;
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn submit_service_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((session_id, name, action)) = self.pending_service_action.take() else {
            return;
        };
        let not_connected = self.tr("Connect to the server before changing services.");
        let not_accepting = self.tr("The SSH session is not accepting requests.");
        let queue_full = self.tr("The SSH request queue is full.");
        let applying = self.tr("Applying service action...");
        let Some(session) = self.session_mut(session_id) else {
            self.overlay_focus_pending = false;
            window.focus(&self.terminal_focus);
            return;
        };
        if session.state != SessionState::Connected {
            session.server_metrics.services_message = not_connected;
        } else if let Some(command_tx) = &session.command_tx {
            match command_tx.try_send(TerminalRequest::ControlServerService { name, action }) {
                Ok(()) => {
                    session.server_metrics.services_loading = true;
                    session.server_metrics.services_message = applying;
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    session.server_metrics.services_message = queue_full;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    session.server_metrics.services_message = not_accepting;
                }
            }
        } else {
            session.server_metrics.services_message = not_accepting;
        }
        self.overlay_focus_pending = false;
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn service_action_overlay(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let Some((_, name, action)) = self.pending_service_action.clone() else {
            return div().into_any_element();
        };
        let action_label = self.tr(action.label());
        components::modal_overlay(&tokens, "service-action-overlay")
            .track_focus(&self.overlay_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.cancel_service_action(window, cx);
                } else if event.keystroke.key.eq_ignore_ascii_case("enter") {
                    cx.stop_propagation();
                    this.submit_service_action(window, cx);
                }
            }))
            .child(
                components::modal_panel(&tokens, "service-action-panel")
                    .w(px(ui_tokens::MODAL_WIDTH_KEYGEN))
                    .child(components::modal_header(&tokens, action_label.clone()).child(
                        components::icon_button(
                            &tokens,
                            "service-action-close",
                            IconName::Close,
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                            self.tr("Close"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.cancel_service_action(window, cx);
                        })),
                    ))
                    .child(components::warning_text(
                        &tokens,
                        self.tr("Changing a service affects the remote server immediately. Confirm before continuing."),
                    ))
                    .child(components::info_row(&tokens, self.tr("Service"), name))
                    .child(
                        components::modal_footer(&tokens)
                            .child(
                                components::button(
                                    &tokens,
                                    "service-action-cancel",
                                    self.tr("Cancel"),
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.cancel_service_action(window, cx);
                                })),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "service-action-confirm",
                                    action_label,
                                    if action == ServiceAction::Stop {
                                        ButtonKind::Danger
                                    } else {
                                        ButtonKind::Primary
                                    },
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.submit_service_action(window, cx);
                                })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn request_power_action(
        &mut self,
        session_id: u64,
        action: ServerPowerAction,
        cx: &mut Context<Self>,
    ) {
        let not_connected = self.tr("Connect to the server before changing power state.");
        if self
            .session(session_id)
            .is_none_or(|session| session.state != SessionState::Connected)
        {
            if let Some(session) = self.session_mut(session_id) {
                session.server_metrics.power_action_message = not_connected;
            }
            cx.notify();
            return;
        }
        self.pending_power_action = Some((session_id, action));
        self.request_overlay_focus();
        cx.notify();
    }

    fn cancel_power_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pending_power_action = None;
        self.overlay_focus_pending = false;
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn submit_power_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((session_id, action)) = self.pending_power_action.take() else {
            return;
        };
        let not_connected = self.tr("Connect to the server before changing power state.");
        let not_accepting = self.tr("The SSH session is not accepting requests.");
        let queue_full = self.tr("The SSH request queue is full.");
        let applying = self.tr("Applying server power action...");
        let Some(session) = self.session_mut(session_id) else {
            self.overlay_focus_pending = false;
            window.focus(&self.terminal_focus);
            return;
        };
        if session.state != SessionState::Connected {
            session.server_metrics.power_action_message = not_connected;
        } else if let Some(command_tx) = &session.command_tx {
            match command_tx.try_send(TerminalRequest::ControlServerPower { action }) {
                Ok(()) => {
                    session.server_metrics.power_action_loading = true;
                    session.server_metrics.power_action_message = applying;
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    session.server_metrics.power_action_message = queue_full;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    session.server_metrics.power_action_message = not_accepting;
                }
            }
        } else {
            session.server_metrics.power_action_message = not_accepting;
        }
        self.overlay_focus_pending = false;
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn power_action_overlay(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let Some((_, action)) = self.pending_power_action else {
            return div().into_any_element();
        };
        let action_label = self.tr(action.label());
        components::modal_overlay(&tokens, "power-action-overlay")
            .track_focus(&self.overlay_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.cancel_power_action(window, cx);
                } else if event.keystroke.key.eq_ignore_ascii_case("enter") {
                    cx.stop_propagation();
                    this.submit_power_action(window, cx);
                }
            }))
            .child(
                components::modal_panel(&tokens, "power-action-panel")
                    .w(px(ui_tokens::MODAL_WIDTH_KEYGEN))
                    .child(components::modal_header(&tokens, action_label.clone()).child(
                        components::icon_button(
                            &tokens,
                            "power-action-close",
                            IconName::Close,
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                            self.tr("Close"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.cancel_power_action(window, cx);
                        })),
                    ))
                    .child(components::warning_text(
                        &tokens,
                        self.tr(
                            "This changes the remote server power state immediately and may disconnect the session. Confirm before continuing.",
                        ),
                    ))
                    .child(
                        components::modal_footer(&tokens)
                            .child(
                                components::button(
                                    &tokens,
                                    "power-action-cancel",
                                    self.tr("Cancel"),
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.cancel_power_action(window, cx);
                                })),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "power-action-confirm",
                                    action_label,
                                    ButtonKind::Danger,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.submit_power_action(window, cx);
                                })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn keywords_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let keywords = self.keyword_highlights.clone();
        let message = self.keyword_message.clone();
        let mut list = div()
            .id("keyword-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(ui_tokens::SPACE_1))
            .overflow_y_scroll();
        if keywords.is_empty() {
            list = list.child(components::empty_state(
                &tokens,
                self.tr("No highlighted keywords"),
                self.tr("Add literal words such as error, warning or timeout."),
            ));
        } else {
            for (index, keyword) in keywords.into_iter().enumerate() {
                let delete_keyword = keyword.clone();
                list = list.child(
                    div()
                        .id(("keyword-row", index))
                        .flex()
                        .items_center()
                        .gap(px(ui_tokens::SPACE_2))
                        .p(px(ui_tokens::SPACE_2))
                        .rounded(px(ui_tokens::RADIUS_CONTROL))
                        .border_1()
                        .border_color(tokens.border_subtle)
                        .bg(tokens.surface)
                        .child(
                            Icon::new(IconName::CaseSensitive)
                                .with_size(Size::Small)
                                .text_color(tokens.accent),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_size(px(ui_tokens::FONT_BODY))
                                .text_color(tokens.text_primary)
                                .child(keyword),
                        )
                        .child(
                            components::icon_button(
                                &tokens,
                                ("keyword-delete", index),
                                IconName::Delete,
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                                self.tr("Remove keyword"),
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.remove_keyword(&delete_keyword, cx);
                                },
                            )),
                        ),
                );
            }
        }

        div()
            .id("keywords-panel")
            .flex()
            .flex_col()
            .w(px(ui_tokens::QUICK_COMMAND_PANEL_WIDTH))
            .flex_shrink_0()
            .min_h_0()
            .ml(px(ui_tokens::SPACE_2))
            .border_1()
            .border_color(tokens.border_strong)
            .bg(tokens.surface_raised)
            .text_color(tokens.text_primary)
            .child(
                components::panel_header(&tokens, self.tr("Keyword highlights")).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(ui_tokens::SPACE_1))
                        .child(
                            components::icon_button(
                                &tokens,
                                "keyword-toggle",
                                if self.keyword_highlighting_enabled {
                                    IconName::Eye
                                } else {
                                    IconName::EyeOff
                                },
                                if self.keyword_highlighting_enabled {
                                    ButtonKind::Secondary
                                } else {
                                    ButtonKind::Ghost
                                },
                                ButtonSize::Small,
                                if self.keyword_highlighting_enabled {
                                    self.tr("Disable keyword highlighting")
                                } else {
                                    self.tr("Enable keyword highlighting")
                                },
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_keyword_highlighting(cx);
                            })),
                        )
                        .child(
                            components::icon_button(
                                &tokens,
                                "keyword-close",
                                IconName::Close,
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                                self.tr("Close keyword highlights"),
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.toggle_keywords(window, cx);
                                },
                            )),
                        ),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(ui_tokens::SPACE_1))
                    .p(px(ui_tokens::SPACE_2))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key.eq_ignore_ascii_case("enter") {
                            cx.stop_propagation();
                            this.add_keyword(window, cx);
                        }
                    }))
                    .child(div().flex_1().min_w_0().child(
                        TextInput::new(&self.keyword_input).h(px(ui_tokens::CONTROL_HEIGHT_SM)),
                    ))
                    .child(
                        components::icon_button(
                            &tokens,
                            "keyword-add",
                            IconName::Plus,
                            ButtonKind::Primary,
                            ButtonSize::Small,
                            self.tr("Add keyword"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.add_keyword(window, cx);
                        })),
                    ),
            )
            .child(
                div()
                    .px(px(ui_tokens::SPACE_2))
                    .pb(px(ui_tokens::SPACE_2))
                    .text_size(px(ui_tokens::FONT_CAPTION))
                    .text_color(tokens.text_secondary)
                    .child(if self.keyword_highlighting_enabled {
                        self.tr("Literal matches are highlighted in the terminal.")
                    } else {
                        self.tr("Highlighting is off until you enable it.")
                    })
                    .when(!message.is_empty(), |row| {
                        row.child(
                            div()
                                .pt(px(ui_tokens::SPACE_1))
                                .text_color(tokens.accent_text)
                                .child(message),
                        )
                    }),
            )
            .child(list)
            .into_any_element()
    }

    fn triggers_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let rules = self.trigger_rules.clone();
        let message = self.trigger_message.clone();
        let mut list = div()
            .id("trigger-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(ui_tokens::SPACE_1))
            .overflow_y_scroll();
        if rules.is_empty() {
            list = list.child(components::empty_state(
                &tokens,
                self.tr("No triggers"),
                self.tr("Add a disabled rule, then enable it when you are ready."),
            ));
        } else {
            for (index, rule) in rules.into_iter().enumerate() {
                let toggle_keyword = rule.keyword.clone();
                let delete_keyword = rule.keyword.clone();
                let action_label = match rule.action.kind() {
                    TriggerActionKind::Highlight => self.tr("Highlight"),
                    TriggerActionKind::Notify => self.tr("Notify"),
                    TriggerActionKind::SendText => match &rule.action {
                        TriggerAction::SendText {
                            send_enter: true, ..
                        } => self.tr("Send text + Enter"),
                        TriggerAction::SendText { .. } => self.tr("Send text"),
                        _ => unreachable!("trigger action kind must match action"),
                    },
                };
                list = list.child(
                    div()
                        .id(("trigger-row", index))
                        .flex()
                        .items_center()
                        .gap(px(ui_tokens::SPACE_2))
                        .p(px(ui_tokens::SPACE_2))
                        .rounded(px(ui_tokens::RADIUS_CONTROL))
                        .border_1()
                        .border_color(tokens.border_subtle)
                        .bg(tokens.surface)
                        .child(
                            components::icon_button(
                                &tokens,
                                ("trigger-toggle", index),
                                if rule.enabled {
                                    IconName::Eye
                                } else {
                                    IconName::EyeOff
                                },
                                if rule.enabled {
                                    ButtonKind::Secondary
                                } else {
                                    ButtonKind::Ghost
                                },
                                ButtonSize::Small,
                                if rule.enabled {
                                    self.tr("Disable trigger")
                                } else {
                                    self.tr("Enable trigger")
                                },
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.toggle_trigger_rule(&toggle_keyword, cx);
                                },
                            )),
                        )
                        .child(Icon::new(IconName::Bell).with_size(Size::Small).text_color(
                            if rule.enabled {
                                tokens.warning_text
                            } else {
                                tokens.text_disabled
                            },
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(ui_tokens::FONT_BODY))
                                        .text_color(tokens.text_primary)
                                        .child(rule.keyword),
                                )
                                .child(
                                    div()
                                        .text_size(px(ui_tokens::FONT_CAPTION))
                                        .text_color(tokens.text_secondary)
                                        .child(action_label),
                                ),
                        )
                        .child(
                            components::icon_button(
                                &tokens,
                                ("trigger-delete", index),
                                IconName::Delete,
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                                self.tr("Remove trigger"),
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.remove_trigger(&delete_keyword, cx);
                                },
                            )),
                        ),
                );
            }
        }

        let mut action_modes = div().flex().items_center().gap(px(ui_tokens::SPACE_1));
        for (index, kind) in TriggerActionKind::ALL.into_iter().enumerate() {
            let selected = self.trigger_action_kind == kind;
            action_modes = action_modes.child(
                components::button(
                    &tokens,
                    ("trigger-action", index),
                    self.tr(kind.label()),
                    if selected {
                        ButtonKind::Primary
                    } else {
                        ButtonKind::Ghost
                    },
                    ButtonSize::Small,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_trigger_action_kind(kind, cx);
                })),
            );
        }

        let mut editor = div()
            .flex()
            .flex_col()
            .gap(px(ui_tokens::SPACE_1))
            .p(px(ui_tokens::SPACE_2))
            .border_b_1()
            .border_color(tokens.border_subtle)
            .child(components::label_text(&tokens, self.tr("Trigger keyword")))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(ui_tokens::SPACE_1))
                    .child(
                        div().flex_1().min_w_0().child(
                            TextInput::new(&self.trigger_keyword_input)
                                .h(px(ui_tokens::CONTROL_HEIGHT_SM)),
                        ),
                    )
                    .child(
                        components::icon_button(
                            &tokens,
                            "trigger-add",
                            IconName::Plus,
                            ButtonKind::Primary,
                            ButtonSize::Small,
                            self.tr("Add trigger"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.add_trigger(window, cx);
                        })),
                    ),
            )
            .child(components::label_text(&tokens, self.tr("Action")))
            .child(action_modes);
        if self.trigger_action_kind == TriggerActionKind::SendText {
            editor = editor
                .child(components::label_text(&tokens, self.tr("Preset text")))
                .child(TextInput::new(&self.trigger_text_input).h(px(ui_tokens::CONTROL_HEIGHT_SM)))
                .child(
                    components::button(
                        &tokens,
                        "trigger-send-mode",
                        if self.trigger_send_enter {
                            self.tr("Send + Enter")
                        } else {
                            self.tr("Paste only")
                        },
                        if self.trigger_send_enter {
                            ButtonKind::Secondary
                        } else {
                            ButtonKind::Ghost
                        },
                        ButtonSize::Small,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_trigger_send_enter(cx);
                    })),
                );
        }

        div()
            .id("triggers-panel")
            .flex()
            .flex_col()
            .w(px(ui_tokens::QUICK_COMMAND_PANEL_WIDTH))
            .flex_shrink_0()
            .min_h_0()
            .ml(px(ui_tokens::SPACE_2))
            .border_1()
            .border_color(tokens.border_strong)
            .bg(tokens.surface_raised)
            .text_color(tokens.text_primary)
            .child(
                components::panel_header(&tokens, self.tr("Triggers")).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(ui_tokens::SPACE_1))
                        .child(
                            components::icon_button(
                                &tokens,
                                "trigger-engine-toggle",
                                if self.trigger_engine_enabled {
                                    IconName::Eye
                                } else {
                                    IconName::EyeOff
                                },
                                if self.trigger_engine_enabled {
                                    ButtonKind::Secondary
                                } else {
                                    ButtonKind::Ghost
                                },
                                ButtonSize::Small,
                                if self.trigger_engine_enabled {
                                    self.tr("Disable trigger engine")
                                } else {
                                    self.tr("Enable trigger engine")
                                },
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_trigger_engine(cx);
                            })),
                        )
                        .child(
                            components::icon_button(
                                &tokens,
                                "trigger-close",
                                IconName::Close,
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                                self.tr("Close triggers"),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_triggers(window, cx);
                            })),
                        ),
                ),
            )
            .child(components::help_text(
                &tokens,
                self.tr("Rules are disabled by default. Actions are limited to highlighting, a toast, or preset text."),
            ))
            .child(editor)
            .child(
                div()
                    .px(px(ui_tokens::SPACE_2))
                    .pb(px(ui_tokens::SPACE_2))
                    .text_size(px(ui_tokens::FONT_CAPTION))
                    .text_color(tokens.text_secondary)
                    .child(if self.trigger_engine_enabled {
                        self.tr("Enable individual rules to react to new terminal output.")
                    } else {
                        self.tr("The trigger engine is off until you enable it.")
                    })
                    .when(!message.is_empty(), |row| {
                        row.child(
                            div()
                                .pt(px(ui_tokens::SPACE_1))
                                .text_color(tokens.accent_text)
                                .child(message),
                        )
                    }),
            )
            .child(list)
            .into_any_element()
    }

    fn snippets_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let snippets = self.snippets.clone();
        let message = self.snippet_message.clone();
        let targets = self.snippet_targets.clone();
        let active_session = self.active_session().map(|session| session.title.clone());
        let mut quick_grid = div()
            .id("quick-command-grid")
            .flex()
            .flex_wrap()
            .flex_1()
            .min_h_0()
            .content_start()
            .gap(px(ui_tokens::SPACE_2))
            .overflow_y_scroll();
        if snippets.is_empty() {
            quick_grid = quick_grid.child(
                components::empty_state(
                    &tokens,
                    self.tr("No quick commands"),
                    self.tr("Create a command tile for the active SSH session."),
                )
                .w_full(),
            );
        }
        for (index, snippet) in snippets.into_iter().enumerate() {
            let load = snippet.clone();
            let send = snippet.clone();
            let delete_name = snippet.name.clone();
            let editing = self
                .editing_snippet
                .as_ref()
                .is_some_and(|name| name.eq_ignore_ascii_case(&snippet.name));
            let run_button = Button::new(("quick-command-run", index))
                .w(px(ui_tokens::QUICK_COMMAND_WIDTH))
                .h(px(ui_tokens::QUICK_COMMAND_HEIGHT))
                .p(px(ui_tokens::SPACE_2))
                .rounded(px(ui_tokens::RADIUS_CARD))
                .border_1()
                .border_color(if editing {
                    tokens.accent
                } else {
                    tokens.border_subtle
                })
                .bg(tokens.surface)
                .hover(|style| style.bg(tokens.surface_overlay))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(ui_tokens::SPACE_1))
                        .w_full()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(ui_tokens::SPACE_1))
                                .child(
                                    Icon::new(IconName::SquareTerminal)
                                        .with_size(Size::Small)
                                        .text_color(tokens.accent),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_size(px(ui_tokens::FONT_BODY))
                                        .font_semibold()
                                        .text_color(tokens.text_primary)
                                        .child(snippet.name.clone()),
                                ),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_size(px(ui_tokens::FONT_CAPTION))
                                .text_color(tokens.text_secondary)
                                .child(snippet.command.clone()),
                        ),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.send_snippet_to_active(&send, cx);
                }));
            quick_grid = quick_grid.child(
                div()
                    .w(px(ui_tokens::QUICK_COMMAND_WIDTH))
                    .flex()
                    .flex_col()
                    .gap(px(ui_tokens::SPACE_1))
                    .child(run_button)
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(ui_tokens::SPACE_1))
                            .child(
                                components::icon_button(
                                    &tokens,
                                    ("quick-command-edit", index),
                                    IconName::Settings2,
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                    self.tr("Edit quick command"),
                                )
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.load_snippet(&load, window, cx);
                                    },
                                )),
                            )
                            .child(
                                components::icon_button(
                                    &tokens,
                                    ("quick-command-delete", index),
                                    IconName::Delete,
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                    self.tr("Delete quick command"),
                                )
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.delete_snippet(&delete_name, window, cx);
                                    },
                                )),
                            ),
                    ),
            );
        }

        let mut target_list = div().flex().flex_wrap().gap(px(ui_tokens::SPACE_1));
        let connected_sessions = self
            .sessions
            .iter()
            .filter(|session| session.state == SessionState::Connected)
            .map(|session| (session.id, session.title.clone()))
            .collect::<Vec<_>>();
        if connected_sessions.is_empty() {
            target_list = target_list.child(
                div()
                    .text_size(px(ui_tokens::FONT_CAPTION))
                    .text_color(tokens.text_disabled)
                    .child(self.tr("No connected sessions")),
            );
        }
        for (session_id, title) in connected_sessions {
            let selected = targets.contains(&session_id);
            let button = Button::new(("snippet-target", session_id)).label(title);
            let button = if selected { button.primary() } else { button };
            target_list = target_list.child(button.on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_snippet_target(session_id, cx);
            })));
        }

        div()
            .id("snippets-panel")
            .flex()
            .flex_col()
            .w(px(ui_tokens::QUICK_COMMAND_PANEL_WIDTH))
            .flex_shrink_0()
            .min_h_0()
            .ml(px(ui_tokens::SPACE_2))
            .border_1()
            .border_color(tokens.border_strong)
            .bg(tokens.surface_raised)
            .text_color(tokens.text_primary)
            .child(
                components::panel_header(&tokens, self.tr("Quick commands")).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(ui_tokens::SPACE_1))
                        .child(
                            components::icon_button(
                                &tokens,
                                "quick-command-new",
                                IconName::Plus,
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                                self.tr("New quick command"),
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.new_snippet(window, cx);
                                },
                            )),
                        )
                        .child(
                            components::icon_button(
                                &tokens,
                                "quick-command-close",
                                IconName::Close,
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                                self.tr("Close quick commands"),
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_snippets(cx))),
                        ),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(ui_tokens::SPACE_2))
                    .p(px(ui_tokens::SPACE_2))
                    .child(components::field(
                        &tokens,
                        self.tr("Name"),
                        &self.snippet_name,
                        false,
                    ))
                    .child(components::field(
                        &tokens,
                        self.tr("Command"),
                        &self.snippet_command,
                        false,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(ui_tokens::SPACE_1))
                            .child(
                                components::button(
                                    &tokens,
                                    "quick-command-enter",
                                    if self.snippet_send_enter {
                                        self.tr("Send + Enter")
                                    } else {
                                        self.tr("Paste only")
                                    },
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.snippet_send_enter = !this.snippet_send_enter;
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "quick-command-save",
                                    self.tr("Save command"),
                                    ButtonKind::Primary,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.save_snippet(cx))),
                            ),
                    ),
            )
            .child(
                div()
                    .px(px(ui_tokens::SPACE_2))
                    .text_size(px(ui_tokens::FONT_CAPTION))
                    .text_color(tokens.text_secondary)
                    .child(if let Some(title) = active_session {
                        self.tr("Send to {title}").replace("{title}", &title)
                    } else {
                        self.tr("Open a connected session to run a command.")
                    })
                    .when(!message.is_empty(), |row| {
                        row.child(
                            div()
                                .pt(px(ui_tokens::SPACE_1))
                                .text_color(tokens.accent_text)
                                .child(message),
                        )
                    }),
            )
            .child(quick_grid)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(ui_tokens::SPACE_2))
                    .p(px(ui_tokens::SPACE_2))
                    .border_t_1()
                    .border_color(tokens.border_subtle)
                    .child(
                        div()
                            .text_size(px(ui_tokens::FONT_CAPTION))
                            .font_semibold()
                            .text_color(tokens.text_secondary)
                            .child(self.tr("Broadcast targets")),
                    )
                    .child(target_list)
                    .child(
                        components::button(
                            &tokens,
                            "quick-command-broadcast",
                            self.tr("Send to {count} selected")
                                .replace("{count}", &self.snippet_targets.len().to_string()),
                            ButtonKind::Secondary,
                            ButtonSize::Small,
                        )
                        .w_full()
                        .on_click(cx.listener(|this, _, _, cx| this.broadcast_snippet(cx))),
                    ),
            )
            .into_any_element()
    }

    fn begin_sftp_metadata(
        &mut self,
        session_id: u64,
        path: String,
        name: String,
        permissions: Option<u32>,
        atime: Option<u32>,
        mtime: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.session_mut(session_id) {
            session.sftp.metadata_edit = Some(SftpMetadataEdit { path, name });
            session.sftp.rename_from = None;
            session.sftp.pending_delete = None;
        }
        Self::set_input_value(
            &self.sftp_permissions,
            &format_permission_mode(permissions),
            window,
            cx,
        );
        Self::set_input_value(&self.sftp_atime, &format_unix_timestamp(atime), window, cx);
        Self::set_input_value(&self.sftp_mtime, &format_unix_timestamp(mtime), window, cx);
        cx.notify();
    }

    fn apply_sftp_metadata(
        &mut self,
        session_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mode = match parse_permission_mode(&self.sftp_permissions.read(cx).value()) {
            Ok(mode) => mode,
            Err(_) => {
                let message = self.tr("Invalid permission mode.");
                self.set_sftp_message(session_id, message, cx);
                return;
            }
        };
        let atime = match parse_unix_timestamp(&self.sftp_atime.read(cx).value()) {
            Ok(value) => value,
            Err(_) => {
                let message = self.tr("Invalid access time.");
                self.set_sftp_message(session_id, message, cx);
                return;
            }
        };
        let mtime = match parse_unix_timestamp(&self.sftp_mtime.read(cx).value()) {
            Ok(value) => value,
            Err(_) => {
                let message = self.tr("Invalid modified time.");
                self.set_sftp_message(session_id, message, cx);
                return;
            }
        };
        let Some((path, current_permissions)) = self.session(session_id).and_then(|session| {
            session.sftp.metadata_edit.as_ref().map(|edit| {
                (
                    edit.path.clone(),
                    session
                        .sftp
                        .entries
                        .iter()
                        .find(|entry| entry.path == edit.path)
                        .and_then(|entry| entry.permissions),
                )
            })
        }) else {
            return;
        };
        let permissions = Some((current_permissions.unwrap_or_default() & !0o7777) | mode);
        self.send_sftp_command(
            session_id,
            SftpCommand::SetMetadata {
                path,
                permissions,
                atime,
                mtime,
            },
            cx,
        );
        if let Some(session) = self.session_mut(session_id) {
            session.sftp.metadata_edit = None;
        }
        Self::set_input_value(&self.sftp_permissions, "", window, cx);
        Self::set_input_value(&self.sftp_atime, "", window, cx);
        Self::set_input_value(&self.sftp_mtime, "", window, cx);
        cx.notify();
    }

    fn cancel_sftp_metadata(
        &mut self,
        session_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.session_mut(session_id) {
            session.sftp.metadata_edit = None;
        }
        Self::set_input_value(&self.sftp_permissions, "", window, cx);
        Self::set_input_value(&self.sftp_atime, "", window, cx);
        Self::set_input_value(&self.sftp_mtime, "", window, cx);
        cx.notify();
    }

    fn begin_sftp_rename(
        &mut self,
        session_id: u64,
        path: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.session_mut(session_id) {
            session.sftp.rename_from = Some(path);
            session.sftp.pending_delete = None;
            session.sftp.metadata_edit = None;
        }
        Self::set_input_value(&self.sftp_name, &name, window, cx);
        cx.notify();
    }

    fn apply_sftp_rename(&mut self, session_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.sftp_name.read(cx).value().trim().to_owned();
        if !valid_remote_entry_name(&name) {
            self.set_sftp_message(session_id, "A valid new name is required.", cx);
            return;
        }
        let Some((from, current)) = self.session(session_id).and_then(|session| {
            session
                .sftp
                .rename_from
                .clone()
                .map(|from| (from, session.sftp.path.clone()))
        }) else {
            return;
        };
        let to = remote_join(&current, &name);
        self.send_sftp_command(session_id, SftpCommand::Rename { from, to }, cx);
        if let Some(session) = self.session_mut(session_id) {
            session.sftp.rename_from = None;
            session.sftp.metadata_edit = None;
        }
        Self::set_input_value(&self.sftp_name, "", window, cx);
        cx.notify();
    }

    fn cancel_sftp_rename(&mut self, session_id: u64, cx: &mut Context<Self>) {
        if let Some(session) = self.session_mut(session_id) {
            session.sftp.rename_from = None;
        }
        cx.notify();
    }

    fn create_sftp_directory(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let name = self.sftp_name.read(cx).value().trim().to_owned();
        if !valid_remote_entry_name(&name) {
            self.set_sftp_message(session_id, "A valid directory name is required.", cx);
            return;
        }
        let Some(current) = self
            .session(session_id)
            .map(|session| session.sftp.path.clone())
        else {
            return;
        };
        self.send_sftp_command(
            session_id,
            SftpCommand::CreateDirectory(remote_join(&current, &name)),
            cx,
        );
        cx.notify();
    }

    fn request_sftp_delete(
        &mut self,
        session_id: u64,
        path: String,
        is_dir: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.session_mut(session_id) {
            session.sftp.pending_delete = Some((path, is_dir));
            session.sftp.rename_from = None;
            session.sftp.metadata_edit = None;
        }
        cx.notify();
    }

    fn confirm_sftp_delete(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let Some((path, is_dir)) = self
            .session_mut(session_id)
            .and_then(|session| session.sftp.pending_delete.take())
        else {
            return;
        };
        let command = if is_dir {
            SftpCommand::RemoveDirectory(path)
        } else {
            SftpCommand::RemoveFile(path)
        };
        self.send_sftp_command(session_id, command, cx);
        cx.notify();
    }

    fn cancel_sftp_delete(&mut self, session_id: u64, cx: &mut Context<Self>) {
        if let Some(session) = self.session_mut(session_id) {
            session.sftp.pending_delete = None;
        }
        cx.notify();
    }

    fn set_sftp_message(
        &mut self,
        session_id: u64,
        message: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.session_mut(session_id) {
            session.sftp.message = message.into();
        }
        cx.notify();
    }

    fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_visible = !self.settings_visible;
        if self.settings_visible {
            self.snippets_visible = false;
            self.keys_visible = false;
            self.keywords_visible = false;
            self.triggers_visible = false;
            if let Some(session_id) = self.active_session_id
                && let Some(session) = self.session_mut(session_id)
            {
                session.sftp.visible = false;
                session.forwards.visible = false;
            }
            self.request_overlay_focus();
        } else {
            window.focus(&self.terminal_focus);
        }
        cx.notify();
    }

    fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        self.persist_ui_preferences(cx);
        self.resize_terminal(window, cx);
        cx.notify();
    }

    fn focus_profile_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_collapsed {
            self.sidebar_collapsed = false;
            self.persist_ui_preferences(cx);
            self.resize_terminal(window, cx);
        }
        self.profile_search
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    fn toggle_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.inspector_visible = !self.inspector_visible;
        self.resize_terminal(window, cx);
        cx.notify();
    }

    fn toggle_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.compose_visible = !self.compose_visible;
        if self.compose_visible {
            let focus = self.compose_input.read(cx).focus_handle(cx);
            window.focus(&focus);
        } else {
            window.focus(&self.terminal_focus);
        }
        cx.notify();
    }

    fn recall_compose_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((text, next)) = recall_compose_history(&self.compose_history, self.compose_recall)
        else {
            return;
        };
        Self::set_input_value(&self.compose_input, &text, window, cx);
        self.compose_recall = next;
        let focus = self.compose_input.read(cx).focus_handle(cx);
        window.focus(&focus);
        cx.notify();
    }

    fn send_compose(&mut self, session_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.compose_input.read(cx).text().to_string();
        if text.trim().is_empty() {
            return;
        }
        self.send_input_to_targets(
            session_id,
            || TerminalRequest::Paste(text.clone()),
            "Compose",
            cx,
        );
        remember_compose_history(&mut self.compose_history, text);
        self.compose_recall = 0;
        Self::set_input_value(&self.compose_input, "", window, cx);
        let focus = self.compose_input.read(cx).focus_handle(cx);
        window.focus(&focus);
        cx.notify();
    }

    fn compose_bar(
        &mut self,
        session_id: u64,
        tokens: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let history_label = if self.compose_history.is_empty() {
            self.tr("History")
        } else {
            self.tr("History {current}/{total}")
                .replace(
                    "{current}",
                    &(self.compose_recall % self.compose_history.len() + 1).to_string(),
                )
                .replace("{total}", &self.compose_history.len().to_string())
        };
        div()
            .id("compose-bar")
            .flex_shrink_0()
            .mx(px(ui_tokens::SPACE_2))
            .mb(px(ui_tokens::SPACE_2))
            .h(px(96.0))
            .flex()
            .flex_col()
            .gap(px(ui_tokens::SPACE_1))
            .p(px(ui_tokens::SPACE_2))
            .border_1()
            .border_color(tokens.border_strong)
            .rounded(px(ui_tokens::RADIUS_CARD))
            .bg(tokens.surface_raised)
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.to_ascii_lowercase();
                let modifiers = event.keystroke.modifiers;
                if key == "escape" {
                    cx.stop_propagation();
                    this.toggle_compose(window, cx);
                } else if key == "enter" && (modifiers.control || modifiers.platform) {
                    cx.stop_propagation();
                    this.send_compose(session_id, window, cx);
                }
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(ui_tokens::SPACE_2))
                    .child(components::help_text(
                        tokens,
                        self.tr("Compose · Ctrl+Enter sends · Esc closes"),
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(ui_tokens::SPACE_1))
                            .child(
                                components::button(
                                    tokens,
                                    "compose-history",
                                    history_label,
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.recall_compose_history(window, cx);
                                    },
                                )),
                            )
                            .child(
                                components::button(
                                    tokens,
                                    "compose-send",
                                    self.tr("Send"),
                                    ButtonKind::Primary,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.send_compose(session_id, window, cx);
                                    },
                                )),
                            )
                            .child(
                                components::icon_button(
                                    tokens,
                                    "compose-close",
                                    IconName::Close,
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                    self.tr("Close compose"),
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.toggle_compose(window, cx);
                                    },
                                )),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(TextInput::new(&self.compose_input).h_full()),
            )
            .into_any_element()
    }

    fn toggle_ui_theme(&mut self, cx: &mut Context<Self>) {
        self.ui_theme = self.ui_theme.toggled();
        ui::theme::install(cx, self.ui_theme);
        self.persist_ui_preferences(cx);
        cx.notify();
    }

    fn persist_ui_preferences(&mut self, cx: &mut Context<Self>) {
        self.settings.ui_theme = self.ui_theme;
        self.settings.sidebar_collapsed = self.sidebar_collapsed;
        self.settings.sidebar_width = self
            .settings
            .sidebar_width
            .clamp(ui_tokens::SIDEBAR_MIN_WIDTH, ui_tokens::SIDEBAR_MAX_WIDTH);
        self.settings.inspector_width = self.settings.inspector_width.clamp(
            ui_tokens::INSPECTOR_MIN_WIDTH,
            ui_tokens::INSPECTOR_MAX_WIDTH,
        );
        self.settings.collapsed_groups = self.collapsed_groups.iter().cloned().collect();
        self.settings.collapsed_groups.sort();
        if let Some(store) = self.settings_store.as_ref()
            && let Err(error) = store.save(&self.settings)
        {
            self.status = format!("Could not save UI preferences: {error}");
        }
        cx.notify();
    }

    fn toggle_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.keys_visible = !self.keys_visible;
        if self.keys_visible {
            self.refresh_existing_keys();
            self.key_change_visible = false;
            self.key_change_path = None;
            self.key_change_message.clear();
            self.key_delete_visible = false;
            self.key_delete_path = None;
            self.key_delete_message.clear();
            self.settings_visible = false;
            self.snippets_visible = false;
            self.keywords_visible = false;
            self.triggers_visible = false;
            if let Some(session_id) = self.active_session_id
                && let Some(session) = self.session_mut(session_id)
            {
                session.sftp.visible = false;
                session.forwards.visible = false;
            }
            self.request_overlay_focus();
        } else {
            self.key_change_visible = false;
            self.key_change_path = None;
            self.key_change_message.clear();
            self.key_delete_visible = false;
            self.key_delete_path = None;
            self.key_delete_message.clear();
            Self::set_input_value(&self.key_change_old_passphrase, "", window, cx);
            Self::set_input_value(&self.key_change_new_passphrase, "", window, cx);
            window.focus(&self.terminal_focus);
        }
        cx.notify();
    }

    fn refresh_existing_keys(&mut self) {
        let directory = home_directory().join(".ssh");
        match keygen::discover(&directory) {
            Ok(keys) => {
                self.existing_keys = keys;
                self.key_discovery_message.clear();
            }
            Err(error) => {
                self.existing_keys.clear();
                self.key_discovery_message = format!(
                    "{} {}: {error:#}",
                    self.tr("Could not scan"),
                    directory.display()
                );
            }
        }
    }

    fn begin_key_import(&mut self, cx: &mut Context<Self>) {
        let prompt: SharedString = self.tr("Import OpenSSH private key").into();
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(prompt),
        });
        let directory = home_directory().join(".ssh");
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(mut paths))) = receiver.await
                && let Some(path) = paths.pop()
            {
                let _ = this.update(cx, |view, cx| {
                    view.import_key_from_path(&path, &directory, cx);
                });
            }
        })
        .detach();
    }

    fn import_key_from_path(&mut self, source: &Path, directory: &Path, cx: &mut Context<Self>) {
        let source = source.to_owned();
        let directory = directory.to_owned();
        self.key_message = self.tr("Importing key...");
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { keygen::import_key(&source, &directory) })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(imported) => {
                        view.key_message = format!(
                            "{} {}.",
                            view.tr("Imported key"),
                            imported.private_path.display()
                        );
                        view.refresh_existing_keys();
                        view.show_toast(view.tr("Key imported."), Tone::Success, cx);
                    }
                    Err(error) => {
                        view.key_message = format!("{error:#}");
                        view.show_toast(view.tr("Key import failed."), Tone::Danger, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn begin_key_export(&mut self, key: &keygen::ExistingKey, cx: &mut Context<Self>) {
        let source = key.private_path.clone();
        let suggested = source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("id_ed25519")
            .to_owned();
        let directory = home_directory();
        let receiver = cx.prompt_for_new_path(&directory, Some(&suggested));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = receiver.await {
                let _ = this.update(cx, |view, cx| {
                    view.export_key_to_path(&source, &path, cx);
                });
            }
        })
        .detach();
    }

    fn export_key_to_path(&mut self, source: &Path, destination: &Path, cx: &mut Context<Self>) {
        let source = source.to_owned();
        let destination = destination.to_owned();
        self.key_message = self.tr("Exporting key...");
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { keygen::export_key(&source, &destination) })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(exported) => {
                        view.key_message = format!(
                            "{} {}.",
                            view.tr("Exported key"),
                            exported.private_path.display()
                        );
                        view.show_toast(view.tr("Key exported."), Tone::Success, cx);
                    }
                    Err(error) => {
                        view.key_message = format!("{error:#}");
                        view.show_toast(view.tr("Key export failed."), Tone::Danger, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn request_key_delete(&mut self, key: &keygen::ExistingKey, cx: &mut Context<Self>) {
        self.key_delete_path = Some(key.private_path.clone());
        self.key_delete_message.clear();
        self.key_delete_visible = true;
        self.request_overlay_focus();
        cx.notify();
    }

    fn cancel_key_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.key_delete_visible = false;
        self.key_delete_path = None;
        self.key_delete_message.clear();
        if self.keys_visible {
            self.request_overlay_focus();
        } else {
            window.focus(&self.terminal_focus);
        }
        cx.notify();
    }

    fn confirm_key_delete(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.key_delete_path.clone() else {
            return;
        };
        let path_for_ui = path.clone();
        self.key_delete_message = self.tr("Deleting key...");
        self.key_delete_visible = false;
        self.key_delete_path = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { keygen::delete_key_pair(&path) })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(_) => {
                        view.key_delete_message.clear();
                        view.refresh_existing_keys();
                        view.show_toast(view.tr("Key deleted."), Tone::Success, cx);
                    }
                    Err(error) => {
                        view.key_delete_path = Some(path_for_ui.clone());
                        view.key_delete_visible = true;
                        view.key_delete_message = format!("{error:#}");
                        view.request_overlay_focus();
                        view.show_toast(view.tr("Key deletion failed."), Tone::Danger, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn use_existing_key(
        &mut self,
        key: &keygen::ExistingKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = key.private_path.display().to_string();
        Self::set_input_value(&self.identity_file, &path, window, cx);
        self.key_public = key.public_key.clone();
        self.key_message = format!(
            "{} {}",
            key.private_path.display(),
            self.tr("Selected for the next SSH connection.")
        );
        self.show_toast(self.tr("Identity file selected."), Tone::Success, cx);
        cx.notify();
    }

    fn open_key_passphrase(
        &mut self,
        key: &keygen::ExistingKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.key_change_path = Some(key.private_path.clone());
        self.key_change_message.clear();
        Self::set_input_value(&self.key_change_old_passphrase, "", window, cx);
        Self::set_input_value(&self.key_change_new_passphrase, "", window, cx);
        self.key_change_visible = true;
        self.request_overlay_focus();
        cx.notify();
    }

    fn close_key_passphrase(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.key_change_visible = false;
        self.key_change_path = None;
        self.key_change_message.clear();
        Self::set_input_value(&self.key_change_old_passphrase, "", window, cx);
        Self::set_input_value(&self.key_change_new_passphrase, "", window, cx);
        if self.keys_visible {
            self.request_overlay_focus();
        } else {
            window.focus(&self.terminal_focus);
        }
        cx.notify();
    }

    fn change_key_passphrase(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.key_change_path.clone() else {
            self.key_change_message = self.tr("No key selected.");
            cx.notify();
            return;
        };
        let old_passphrase =
            Zeroizing::new(self.key_change_old_passphrase.read(cx).text().to_string());
        let new_passphrase =
            Zeroizing::new(self.key_change_new_passphrase.read(cx).text().to_string());
        Self::set_input_value(&self.key_change_old_passphrase, "", window, cx);
        Self::set_input_value(&self.key_change_new_passphrase, "", window, cx);
        self.key_change_message = self.tr("Changing passphrase...");
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    keygen::change_passphrase(
                        &path,
                        old_passphrase.as_str(),
                        new_passphrase.as_str(),
                    )
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(()) => {
                        view.key_change_visible = false;
                        view.key_change_path = None;
                        view.key_change_message.clear();
                        view.refresh_existing_keys();
                        view.request_overlay_focus();
                        view.show_toast(view.tr("Passphrase updated."), Tone::Success, cx);
                    }
                    Err(error) => {
                        view.key_change_message = format!("{error:#}");
                        view.show_toast(view.tr("Passphrase update failed."), Tone::Danger, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn generate_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.key_path.read(cx).value().trim().to_owned();
        if path.is_empty() {
            self.key_message = "Enter a private key path.".to_owned();
            cx.notify();
            return;
        }
        let path = expand_local_path(&path);
        let passphrase = Zeroizing::new(self.new_key_passphrase.read(cx).value().to_string());
        let comment = self.key_comment.read(cx).value().trim().to_owned();
        let algorithm = self.key_algorithm;
        Self::set_input_value(&self.new_key_passphrase, "", window, cx);
        self.key_message = format!(
            "{} {} {}...",
            self.tr("Generating"),
            algorithm.label(),
            self.tr("key")
        );
        self.key_public = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let read_path = path.clone();
            let result = cx
                .background_spawn(async move {
                    keygen::generate(&read_path, passphrase.as_str(), &comment, algorithm)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(generated) => {
                        view.key_message = format!(
                            "{} {} and {}.",
                            view.tr("Created"),
                            generated.private_path.display(),
                            generated.public_path.display()
                        );
                        view.key_public = Some(generated.public_key);
                        view.refresh_existing_keys();
                        view.show_toast(
                            format!("{} {}.", algorithm.label(), view.tr("key pair generated")),
                            Tone::Success,
                            cx,
                        );
                    }
                    Err(error) => {
                        view.key_message = format!("{error:#}");
                        view.key_public = None;
                        view.show_toast(view.tr("Key generation failed."), Tone::Danger, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn keys_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let message = self.key_message.clone();
        let public_key = self.key_public.clone();
        let existing_keys = self.existing_keys.clone();
        let discovery_message = self.key_discovery_message.clone();
        let selected_algorithm = self.key_algorithm;
        let mut algorithm_selector = div().flex().flex_wrap().gap(px(ui_tokens::SPACE_2));
        for (index, algorithm) in keygen::KeyAlgorithm::ALL.into_iter().enumerate() {
            let kind = if selected_algorithm == algorithm {
                ButtonKind::Primary
            } else {
                ButtonKind::Secondary
            };
            algorithm_selector = algorithm_selector.child(
                components::button(
                    &tokens,
                    ("key-algorithm", index),
                    algorithm.label(),
                    kind,
                    ButtonSize::Small,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.key_algorithm = algorithm;
                    this.key_public = None;
                    cx.notify();
                })),
            );
        }
        let mut existing_keys_panel = div().flex().flex_col().gap(px(ui_tokens::SPACE_2));
        let existing_keys_header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(ui_tokens::SPACE_2))
            .child(components::section_label(&tokens, self.tr("Existing keys")))
            .child(
                components::icon_button(
                    &tokens,
                    "import-key",
                    IconName::FolderOpen,
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    self.tr("Import key"),
                )
                .on_click(cx.listener(|this, _, _, cx| this.begin_key_import(cx))),
            );
        existing_keys_panel = existing_keys_panel.child(existing_keys_header);
        if existing_keys.is_empty() {
            existing_keys_panel = existing_keys_panel.child(components::help_text(
                &tokens,
                self.tr("No existing OpenSSH keys found in ~/.ssh."),
            ));
        } else {
            for (index, key) in existing_keys.iter().enumerate() {
                let private_path = key.private_path.clone();
                let file_name = private_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("identity")
                    .to_owned();
                let algorithm = key.algorithm.clone();
                let public_key = key.public_key.clone();
                let path_summary = private_path.display().to_string();
                let use_tooltip = self.tr("Use for SSH");
                let change_tooltip = self.tr("Change passphrase");
                let copy_tooltip = self.tr("Copy public key");
                let export_tooltip = self.tr("Export key");
                let delete_tooltip = self.tr("Delete key");
                let mut actions = div().flex().items_center().gap(px(ui_tokens::SPACE_1));
                actions = actions.child(
                    components::icon_button(
                        &tokens,
                        ("existing-key-use", index),
                        IconName::ArrowRight,
                        ButtonKind::Secondary,
                        ButtonSize::Small,
                        use_tooltip,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(key) = this.existing_keys.get(index).cloned() {
                            this.use_existing_key(&key, window, cx);
                        }
                    })),
                );
                let key_for_change = key.clone();
                actions = actions.child(
                    components::icon_button(
                        &tokens,
                        ("existing-key-change-passphrase", index),
                        IconName::Replace,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        change_tooltip,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_key_passphrase(&key_for_change, window, cx);
                    })),
                );
                if let Some(public_key) = public_key {
                    actions = actions.child(
                        components::icon_button(
                            &tokens,
                            ("existing-key-copy", index),
                            IconName::Copy,
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                            copy_tooltip,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(public_key.clone()));
                            this.show_toast(this.tr("Public key copied."), Tone::Success, cx);
                        })),
                    );
                }
                let key_for_export = key.clone();
                actions = actions.child(
                    components::icon_button(
                        &tokens,
                        ("existing-key-export", index),
                        IconName::ArrowUp,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        export_tooltip,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.begin_key_export(&key_for_export, cx);
                    })),
                );
                let key_for_delete = key.clone();
                actions = actions.child(
                    components::icon_button(
                        &tokens,
                        ("existing-key-delete", index),
                        IconName::Delete,
                        ButtonKind::Danger,
                        ButtonSize::Small,
                        delete_tooltip,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.request_key_delete(&key_for_delete, cx);
                    })),
                );
                existing_keys_panel = existing_keys_panel.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(ui_tokens::SPACE_2))
                        .p(px(ui_tokens::SPACE_2))
                        .rounded(px(ui_tokens::RADIUS_CONTROL))
                        .border_1()
                        .border_color(tokens.border_subtle)
                        .bg(tokens.surface)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .min_w_0()
                                .gap(px(ui_tokens::SPACE_1))
                                .child(
                                    div()
                                        .text_size(px(ui_tokens::FONT_BODY))
                                        .text_color(tokens.text_primary)
                                        .child(file_name),
                                )
                                .child(components::help_text(
                                    &tokens,
                                    format!("{algorithm} · {path_summary}"),
                                )),
                        )
                        .child(actions),
                );
            }
        }
        if !discovery_message.is_empty() {
            existing_keys_panel =
                existing_keys_panel.child(components::help_text(&tokens, discovery_message));
        }
        let mut body = div()
            .id("keys-body")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap(px(ui_tokens::SPACE_3))
            .p(px(ui_tokens::SPACE_4))
            .child(existing_keys_panel)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(ui_tokens::SPACE_2))
                    .child(components::section_label(&tokens, self.tr("Algorithm")))
                    .child(algorithm_selector)
                    .child(components::help_text(
                        &tokens,
                        self.tr(selected_algorithm.description()),
                    ))
                    .child(components::help_text(
                        &tokens,
                        format!(
                            "{}: {}",
                            self.tr("Suggested private key path"),
                            selected_algorithm.path_hint()
                        ),
                    )),
            )
            .child(components::field(
                &tokens,
                self.tr("Private key path"),
                &self.key_path,
                false,
            ))
            .child(components::field(
                &tokens,
                self.tr("Passphrase (optional)"),
                &self.new_key_passphrase,
                true,
            ))
            .child(components::field(
                &tokens,
                self.tr("Comment"),
                &self.key_comment,
                false,
            ))
            .child(
                components::button(
                    &tokens,
                    "generate-key",
                    format!(
                        "{} {} {}",
                        self.tr("Generate"),
                        selected_algorithm.label(),
                        self.tr("key")
                    ),
                    ButtonKind::Primary,
                    ButtonSize::Regular,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.generate_key(window, cx);
                })),
            )
            .child(components::help_text(
                &tokens,
                self.tr("Creates an OpenSSH private key and a .pub file. Existing files are never overwritten. The passphrase is not stored; Unix key files use mode 0600."),
            ));
        if !message.is_empty() {
            body = body.child(components::help_text(&tokens, message));
        }
        if let Some(public_key) = public_key {
            body = body
                .child(components::section_label(&tokens, self.tr("Public key")))
                .child(
                    div()
                        .p(px(ui_tokens::SPACE_2))
                        .rounded(px(ui_tokens::RADIUS_CONTROL))
                        .border_1()
                        .border_color(tokens.border_subtle)
                        .bg(tokens.surface)
                        .text_size(px(ui_tokens::FONT_CAPTION))
                        .text_color(tokens.text_primary)
                        .child(public_key),
                )
                .child(
                    components::button(
                        &tokens,
                        "copy-public-key",
                        self.tr("Copy public key"),
                        ButtonKind::Secondary,
                        ButtonSize::Small,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(key) = this.key_public.clone() {
                            cx.write_to_clipboard(ClipboardItem::new_string(key));
                            this.show_toast(this.tr("Public key copied."), Tone::Success, cx);
                        }
                    })),
                );
        }

        components::modal_overlay(&tokens, "keys-overlay")
            .track_focus(&self.overlay_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.toggle_keys(window, cx);
                }
            }))
            .child(
                components::modal_panel(&tokens, "keys-panel")
                    .w(px(ui_tokens::MODAL_WIDTH_KEYGEN))
                    .max_h(px(640.))
                    .child(
                        components::modal_header(&tokens, self.tr("User keys")).child(
                            components::icon_button(
                                &tokens,
                                "keys-close",
                                IconName::Close,
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                                self.tr("Close"),
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| this.toggle_keys(window, cx)),
                            ),
                        ),
                    )
                    .child(body),
            )
            .into_any_element()
    }

    fn key_change_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let path = self
            .key_change_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| self.tr("No key selected."));
        let message = self.key_change_message.clone();
        let mut body = div()
            .id("key-change-body")
            .flex()
            .flex_col()
            .gap(px(ui_tokens::SPACE_3))
            .p(px(ui_tokens::SPACE_4))
            .child(components::help_text(
                &tokens,
                format!("{}: {path}", self.tr("Private key path")),
            ))
            .child(components::field(
                &tokens,
                self.tr("Current passphrase"),
                &self.key_change_old_passphrase,
                true,
            ))
            .child(components::field(
                &tokens,
                self.tr("New passphrase (optional)"),
                &self.key_change_new_passphrase,
                true,
            ))
            .child(components::help_text(
                &tokens,
                self.tr("Leave the new passphrase empty to remove encryption."),
            ));
        if !message.is_empty() {
            body = body.child(components::help_text(&tokens, message));
        }
        components::modal_overlay(&tokens, "key-change-overlay")
            .track_focus(&self.overlay_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.close_key_passphrase(window, cx);
                }
            }))
            .child(
                components::modal_panel(&tokens, "key-change-panel")
                    .w(px(ui_tokens::MODAL_WIDTH_KEYGEN))
                    .child(
                        components::modal_header(&tokens, self.tr("Change key passphrase")).child(
                            components::icon_button(
                                &tokens,
                                "key-change-close",
                                IconName::Close,
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                                self.tr("Close"),
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.close_key_passphrase(window, cx);
                                },
                            )),
                        ),
                    )
                    .child(body)
                    .child(
                        components::modal_footer(&tokens)
                            .child(
                                components::button(
                                    &tokens,
                                    "key-change-cancel",
                                    self.tr("Cancel"),
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.close_key_passphrase(window, cx);
                                    },
                                )),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "key-change-save",
                                    self.tr("Update passphrase"),
                                    ButtonKind::Primary,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.change_key_passphrase(window, cx);
                                    },
                                )),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn key_delete_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let path = self
            .key_delete_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| self.tr("No key selected."));
        let mut body = div()
            .id("key-delete-body")
            .flex()
            .flex_col()
            .gap(px(ui_tokens::SPACE_3))
            .p(px(ui_tokens::SPACE_4))
            .child(components::help_text(
                &tokens,
                format!("{}: {path}", self.tr("Private key path")),
            ))
            .child(components::warning_text(
                &tokens,
                self.tr(
                    "The private key and its .pub companion will be permanently removed from disk.",
                ),
            ));
        if !self.key_delete_message.is_empty() {
            body = body.child(components::warning_text(
                &tokens,
                self.key_delete_message.clone(),
            ));
        }
        components::modal_overlay(&tokens, "key-delete-overlay")
            .track_focus(&self.overlay_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.cancel_key_delete(window, cx);
                }
            }))
            .child(
                components::modal_panel(&tokens, "key-delete-panel")
                    .w(px(ui_tokens::MODAL_WIDTH_KEYGEN))
                    .child(
                        components::modal_header(&tokens, self.tr("Delete key")).child(
                            components::icon_button(
                                &tokens,
                                "key-delete-close",
                                IconName::Close,
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                                self.tr("Close"),
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.cancel_key_delete(window, cx);
                                },
                            )),
                        ),
                    )
                    .child(body)
                    .child(
                        components::modal_footer(&tokens)
                            .child(
                                components::button(
                                    &tokens,
                                    "key-delete-cancel",
                                    self.tr("Keep key"),
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.cancel_key_delete(window, cx);
                                    },
                                )),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "key-delete-confirm",
                                    self.tr("Delete key"),
                                    ButtonKind::Danger,
                                    ButtonSize::Small,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.confirm_key_delete(cx);
                                    },
                                )),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn apply_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let size = self
            .settings_font_size
            .read(cx)
            .value()
            .trim()
            .parse::<f32>();
        let Ok(size) = size else {
            self.settings_message = "Font size must be a number between 9 and 32.".to_owned();
            cx.notify();
            return;
        };
        let Some(alert_cpu_percent) =
            parse_alert_percent(&self.settings_alert_cpu.read(cx).value())
        else {
            self.settings_message =
                "Alert thresholds must be whole percentages from 1 to 100.".to_owned();
            cx.notify();
            return;
        };
        let Some(alert_memory_percent) =
            parse_alert_percent(&self.settings_alert_memory.read(cx).value())
        else {
            self.settings_message =
                "Alert thresholds must be whole percentages from 1 to 100.".to_owned();
            cx.notify();
            return;
        };
        let Some(alert_disk_percent) =
            parse_alert_percent(&self.settings_alert_disk.read(cx).value())
        else {
            self.settings_message =
                "Alert thresholds must be whole percentages from 1 to 100.".to_owned();
            cx.notify();
            return;
        };
        let mut shortcuts = ShortcutSettings::default();
        for (index, action) in ShortcutAction::ALL.iter().enumerate() {
            let action = *action;
            let value = self.shortcut_inputs[index]
                .read(cx)
                .value()
                .trim()
                .to_owned();
            let Some(shortcut) = KeyShortcut::parse(&value) else {
                self.settings_message = format!(
                    "Invalid shortcut for {}: use modifiers plus one key, for example ctrl+shift+t.",
                    action.label()
                );
                cx.notify();
                return;
            };
            let shortcut = shortcut.canonical();
            if shortcut == "ctrl+shift+c" || shortcut == "ctrl+shift+v" {
                self.settings_message = format!(
                    "{} cannot use ctrl+shift+c or ctrl+shift+v because those keys copy and paste.",
                    action.label()
                );
                cx.notify();
                return;
            }
            shortcuts.set(action, shortcut);
        }
        if let Some((left, right)) = shortcuts.conflict() {
            self.settings_message = format!(
                "{} and {} use the same shortcut.",
                left.label(),
                right.label()
            );
            cx.notify();
            return;
        }
        let settings = AppSettings {
            font_size: size,
            font_family: self.settings_font_family.read(cx).value().trim().to_owned(),
            theme: self.settings_theme,
            cursor_style: self.settings_cursor,
            shortcuts,
            ui_theme: self.ui_theme,
            language: self.settings_language,
            sidebar_collapsed: self.sidebar_collapsed,
            sidebar_width: self.settings.sidebar_width,
            inspector_width: self.settings.inspector_width,
            collapsed_groups: self.settings.collapsed_groups.clone(),
            group_order: self.settings.group_order.clone(),
            group_names: self.settings.group_names.clone(),
            alert_cpu_percent,
            alert_memory_percent,
            alert_disk_percent,
        }
        .normalized();
        let Some(store) = self.settings_store.as_ref() else {
            self.settings_message = "Settings storage is unavailable.".to_owned();
            cx.notify();
            return;
        };
        if let Err(error) = store.save(&settings) {
            self.settings_message = error;
            cx.notify();
            return;
        }
        self.settings = settings;
        self.set_language_placeholders(window, cx);
        for session in &self.sessions {
            if session.command_tx.is_some() {
                let _ = enqueue_session_request(
                    &self.sessions,
                    session.id,
                    TerminalRequest::SetTheme(self.settings.theme),
                );
                let _ = enqueue_session_request(
                    &self.sessions,
                    session.id,
                    TerminalRequest::SetCursorStyle(self.settings.cursor_style),
                );
            }
        }
        Self::set_input_value(
            &self.settings_font_family,
            &self.settings.font_family,
            window,
            cx,
        );
        Self::set_input_value(
            &self.settings_font_size,
            &self.settings.font_size.to_string(),
            window,
            cx,
        );
        Self::set_input_value(
            &self.settings_alert_cpu,
            &self.settings.alert_cpu_percent.to_string(),
            window,
            cx,
        );
        Self::set_input_value(
            &self.settings_alert_memory,
            &self.settings.alert_memory_percent.to_string(),
            window,
            cx,
        );
        Self::set_input_value(
            &self.settings_alert_disk,
            &self.settings.alert_disk_percent.to_string(),
            window,
            cx,
        );
        for (index, action) in ShortcutAction::ALL.iter().enumerate() {
            Self::set_input_value(
                &self.shortcut_inputs[index],
                self.settings.shortcuts.shortcut(*action),
                window,
                cx,
            );
        }
        self.settings_message = "Appearance and shortcuts applied.".to_owned();
        self.resize_terminal(window, cx);
        self.show_toast(self.tr(&self.settings_message), Tone::Success, cx);
    }

    fn reset_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut settings = AppSettings::default();
        settings.sidebar_collapsed = self.sidebar_collapsed;
        if let Some(store) = self.settings_store.as_ref()
            && let Err(error) = store.save(&settings)
        {
            self.settings_message = error;
            cx.notify();
            return;
        }
        self.collapsed_groups.clear();
        self.settings = settings;
        self.settings_theme = self.settings.theme;
        self.settings_cursor = self.settings.cursor_style;
        self.settings_language = self.settings.language;
        self.set_language_placeholders(window, cx);
        self.ui_theme = self.settings.ui_theme;
        ui::theme::install(cx, self.ui_theme);
        for session in &self.sessions {
            if session.command_tx.is_some() {
                let _ = enqueue_session_request(
                    &self.sessions,
                    session.id,
                    TerminalRequest::SetTheme(self.settings.theme),
                );
                let _ = enqueue_session_request(
                    &self.sessions,
                    session.id,
                    TerminalRequest::SetCursorStyle(self.settings.cursor_style),
                );
            }
        }
        Self::set_input_value(&self.settings_font_family, DEFAULT_FONT_FAMILY, window, cx);
        Self::set_input_value(
            &self.settings_font_size,
            &self.settings.font_size.to_string(),
            window,
            cx,
        );
        Self::set_input_value(
            &self.settings_alert_cpu,
            &self.settings.alert_cpu_percent.to_string(),
            window,
            cx,
        );
        Self::set_input_value(
            &self.settings_alert_memory,
            &self.settings.alert_memory_percent.to_string(),
            window,
            cx,
        );
        Self::set_input_value(
            &self.settings_alert_disk,
            &self.settings.alert_disk_percent.to_string(),
            window,
            cx,
        );
        for (index, action) in ShortcutAction::ALL.iter().enumerate() {
            Self::set_input_value(
                &self.shortcut_inputs[index],
                action.default_shortcut(),
                window,
                cx,
            );
        }
        self.settings_message = "Appearance and shortcuts reset to defaults.".to_owned();
        self.resize_terminal(window, cx);
        self.show_toast(self.tr(&self.settings_message), Tone::Success, cx);
    }

    fn begin_auth_prompt(
        &mut self,
        session_id: u64,
        name: String,
        instructions: String,
        prompts: Vec<AuthPromptInfo>,
        respond_to: oneshot::Sender<Vec<Zeroizing<String>>>,
        _cx: &mut Context<Self>,
    ) {
        if prompts.len() > MAX_AUTH_PROMPTS {
            let _ = respond_to.send(vec![Zeroizing::new(String::new())]);
            let status = self
                .tr("The server requested {count} keyboard-interactive prompts; at most {max} are supported.")
                .replace("{count}", &prompts.len().to_string())
                .replace("{max}", &MAX_AUTH_PROMPTS.to_string());
            if let Some(session) = self.session_mut(session_id) {
                session.status = status;
            }
            return;
        }
        let status = if name.trim().is_empty() {
            self.tr("Waiting for keyboard-interactive input...")
        } else {
            self.tr("Keyboard-interactive authentication: {name}")
                .replace("{name}", &name)
        };
        if let Some(session) = self.session_mut(session_id) {
            session.status = status;
        }
        self.auth_prompt = Some(AuthPromptState {
            session_id,
            name,
            instructions,
            prompts,
            respond_to: Some(respond_to),
        });
        self.request_overlay_focus();
    }

    fn submit_auth_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.auth_prompt.take() else {
            return;
        };
        let mut responses = Vec::with_capacity(prompt.prompts.len());
        for (index, info) in prompt.prompts.iter().enumerate() {
            let input = if info.echo {
                &self.auth_prompt_plain_inputs[index]
            } else {
                &self.auth_prompt_inputs[index]
            };
            responses.push(Zeroizing::new(input.read(cx).value().to_string()));
        }
        if let Some(respond_to) = prompt.respond_to {
            let _ = respond_to.send(responses);
        }
        self.clear_auth_prompt_inputs(window, cx);
        self.auth_prompt_needs_clear = false;
        let status = self.tr("Submitting authentication response...");
        if let Some(session) = self.session_mut(prompt.session_id) {
            session.status = status;
        }
        self.overlay_focus_pending = false;
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn cancel_auth_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.auth_prompt.take() else {
            return;
        };
        if let Some(respond_to) = prompt.respond_to {
            let _ = respond_to.send(vec![Zeroizing::new(String::new()); prompt.prompts.len()]);
        }
        self.clear_auth_prompt_inputs(window, cx);
        self.auth_prompt_needs_clear = false;
        self.disconnect_session(prompt.session_id, cx);
        self.overlay_focus_pending = false;
        window.focus(&self.terminal_focus);
    }

    fn clear_auth_prompt_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
        for input in self
            .auth_prompt_inputs
            .iter()
            .chain(self.auth_prompt_plain_inputs.iter())
        {
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
    }

    fn auth_prompt_overlay(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let Some(prompt) = self.auth_prompt.as_ref() else {
            return div().into_any_element();
        };
        let title = if prompt.name.trim().is_empty() {
            self.tr("Keyboard-interactive authentication")
        } else {
            self.tr("Keyboard-interactive authentication: {name}")
                .replace("{name}", &prompt.name)
        };
        let instructions = prompt.instructions.trim().to_owned();
        let prompts = prompt.prompts.clone();
        let mut fields = div().flex().flex_col().gap(px(ui_tokens::SPACE_3));
        for (index, info) in prompts.iter().enumerate() {
            let input = if info.echo {
                &self.auth_prompt_plain_inputs[index]
            } else {
                &self.auth_prompt_inputs[index]
            };
            fields = fields.child(components::field(
                &tokens,
                info.prompt.clone(),
                input,
                !info.echo,
            ));
        }
        let mut body = div()
            .id("auth-prompt-body")
            .flex()
            .flex_col()
            .gap(px(ui_tokens::SPACE_3))
            .p(px(ui_tokens::SPACE_4))
            .min_h_0()
            .overflow_y_scroll()
            .child(fields);
        if !instructions.is_empty() {
            body = body.child(components::help_text(&tokens, instructions));
        }
        body = body.child(components::help_text(
            &tokens,
            self.tr("Responses are sent only to the SSH server and are not stored."),
        ));

        components::modal_overlay(&tokens, "auth-prompt-overlay")
            .track_focus(&self.overlay_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.cancel_auth_prompt(window, cx);
                }
            }))
            .child(
                components::modal_panel(&tokens, "auth-prompt")
                    .w_full()
                    .max_w(px(ui_tokens::MODAL_WIDTH_AUTH))
                    .max_h(px(640.))
                    .child(components::modal_header(&tokens, title))
                    .child(body)
                    .child(
                        components::modal_footer(&tokens)
                            .child(
                                components::button(
                                    &tokens,
                                    "auth-prompt-cancel",
                                    self.tr("Cancel"),
                                    ButtonKind::Ghost,
                                    ButtonSize::Regular,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.cancel_auth_prompt(window, cx);
                                    },
                                )),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "auth-prompt-submit",
                                    self.tr("Continue"),
                                    ButtonKind::Primary,
                                    ButtonSize::Regular,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.submit_auth_prompt(window, cx);
                                    },
                                )),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn settings_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let message = self.settings_message.clone();

        let mut theme_selector = div().flex().flex_wrap().gap(px(ui_tokens::SPACE_2));
        for (index, theme) in TerminalTheme::ALL.into_iter().enumerate() {
            let kind = if self.settings_theme == theme {
                ButtonKind::Primary
            } else {
                ButtonKind::Secondary
            };
            let button = components::button(
                &tokens,
                ("settings-theme", index),
                self.tr(theme.label()),
                kind,
                ButtonSize::Small,
            );
            theme_selector =
                theme_selector.child(button.on_click(cx.listener(move |this, _, _, cx| {
                    this.settings_theme = theme;
                    cx.notify();
                })));
        }

        let mut cursor_selector = div().flex().flex_wrap().gap(px(ui_tokens::SPACE_2));
        for (index, style) in CursorStyle::ALL.into_iter().enumerate() {
            let kind = if self.settings_cursor == style {
                ButtonKind::Primary
            } else {
                ButtonKind::Secondary
            };
            let button = components::button(
                &tokens,
                ("settings-cursor", index),
                self.tr(style.label()),
                kind,
                ButtonSize::Small,
            );
            cursor_selector =
                cursor_selector.child(button.on_click(cx.listener(move |this, _, _, cx| {
                    this.settings_cursor = style;
                    cx.notify();
                })));
        }

        let mut language_selector = div().flex().flex_wrap().gap(px(ui_tokens::SPACE_2));
        for (index, language) in Language::ALL.into_iter().enumerate() {
            let kind = if self.settings_language == language {
                ButtonKind::Primary
            } else {
                ButtonKind::Secondary
            };
            let button = components::button(
                &tokens,
                ("settings-language", index),
                language.label(),
                kind,
                ButtonSize::Small,
            );
            language_selector =
                language_selector.child(button.on_click(cx.listener(move |this, _, _, cx| {
                    this.settings_language = language;
                    cx.notify();
                })));
        }

        let mut shortcut_fields = div().flex().flex_col().gap(px(ui_tokens::SPACE_3));
        for (index, action) in ShortcutAction::ALL.iter().enumerate() {
            shortcut_fields = shortcut_fields.child(components::field(
                &tokens,
                self.tr(action.label()),
                &self.shortcut_inputs[index],
                false,
            ));
        }

        components::modal_overlay(&tokens, "settings-overlay")
            .track_focus(&self.overlay_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.toggle_settings(window, cx);
                }
            }))
            .child(
                components::modal_panel(&tokens, "settings-panel")
                    .w_full()
                    .max_w(px(ui_tokens::MODAL_WIDTH_SETTINGS))
                    .h_full()
                    .max_h(px(ui_tokens::MODAL_HEIGHT_SETTINGS))
                    .child(
                        components::modal_header(&tokens, self.tr("Settings")).child(
                            components::button(
                                &tokens,
                                "settings-close",
                                self.tr("Close"),
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| this.toggle_settings(window, cx)),
                            ),
                        ),
                    )
                    .child(
                        div()
                            .id("settings-body")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                    .gap(px(ui_tokens::SPACE_4))
                    .p(px(ui_tokens::SPACE_4))
                    .child(components::field(
                        &tokens,
                        self.tr("Font family"),
                        &self.settings_font_family,
                        false,
                    ))
                    .child(components::field(
                        &tokens,
                        self.tr("Font size"),
                        &self.settings_font_size,
                        false,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(ui_tokens::SPACE_2))
                            .child(components::section_label(
                                &tokens,
                                self.tr("Server alerts"),
                            ))
                            .child(components::field(
                                &tokens,
                                self.tr("CPU alert threshold (%)"),
                                &self.settings_alert_cpu,
                                false,
                            ))
                            .child(components::field(
                                &tokens,
                                self.tr("Memory alert threshold (%)"),
                                &self.settings_alert_memory,
                                false,
                            ))
                            .child(components::field(
                                &tokens,
                                self.tr("Disk alert threshold (%)"),
                                &self.settings_alert_disk,
                                false,
                            ))
                            .child(components::help_text(
                                &tokens,
                                self.tr(
                                    "Values at or above a threshold show a warning in the server inspector.",
                                ),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(ui_tokens::SPACE_2))
                            .child(components::section_label(&tokens, self.tr("Terminal theme")))
                            .child(theme_selector),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(ui_tokens::SPACE_2))
                            .child(components::section_label(&tokens, self.tr("Cursor style")))
                            .child(cursor_selector),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(ui_tokens::SPACE_2))
                            .child(components::section_label(
                                &tokens,
                                self.tr("Workspace shortcuts"),
                            ))
                            .child(shortcut_fields),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(ui_tokens::SPACE_2))
                            .child(components::section_label(&tokens, self.tr("Language")))
                            .child(language_selector),
                    )
                    .child(components::help_text(
                        &tokens,
                        self.tr(
                            "Format: ctrl+shift+t, alt+F12 or cmd+space. Alt+1..9 always switches tabs.",
                        ),
                    ))
                    .child(components::help_text(
                        &tokens,
                        self.tr(&format!(
                            "Allowed range: {MIN_FONT_SIZE:.0}-{MAX_FONT_SIZE:.0} px. Cell size and PTY dimensions follow the font size."
                        )),
                    ))
                    .child(components::help_text(&tokens, self.tr(&message))),
            )
            .child(
                components::modal_footer(&tokens)
                    .child(
                        components::button(
                            &tokens,
                            "settings-reset",
                            self.tr("Reset"),
                            ButtonKind::Secondary,
                            ButtonSize::Regular,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.reset_settings(window, cx);
                        })),
                    )
                    .child(
                        components::button(
                            &tokens,
                            "settings-apply",
                            self.tr("Apply"),
                            ButtonKind::Primary,
                            ButtonSize::Regular,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_settings(window, cx);
                        })),
                    ),
            )
            )
            .into_any_element()
    }

    fn scroll_terminal(
        &mut self,
        session_id: u64,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_session(session_id, window, cx);
        let cell_height = self.settings.cell_height();
        let Some(session) = self.session_mut(session_id) else {
            return;
        };
        if session.state != SessionState::Connected || session.command_tx.is_none() {
            return;
        }
        let delta_y = match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => delta.y / px(cell_height),
        };
        let accumulated = session.scroll_remainder - delta_y;
        let rows = accumulated.trunc() as i32;
        session.scroll_remainder = accumulated - rows as f32;
        cx.stop_propagation();
        if rows == 0 {
            return;
        }
        let Some(position) = self.terminal_position(session_id, event.position) else {
            return;
        };

        let request = TerminalRequest::Mouse(TerminalMouseEvent::Wheel {
            rows,
            position,
            modifiers: terminal_mouse_modifiers(
                event.modifiers.shift,
                event.modifiers.alt,
                event.modifiers.control,
            ),
        });
        self.send_terminal_request(session_id, request, "Terminal scroll", cx);
    }

    fn sidebar(&self, tokens: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        if self.sidebar_collapsed {
            let mut rail = div()
                .flex()
                .flex_col()
                .items_center()
                .flex_shrink_0()
                .w(px(ui_tokens::SIDEBAR_COLLAPSED_WIDTH))
                .h_full()
                .gap(px(ui_tokens::SPACE_2))
                .py(px(ui_tokens::SPACE_2))
                .bg(tokens.surface)
                .border_r_1()
                .border_color(tokens.border_subtle);
            rail = rail.child(
                components::icon_button(
                    tokens,
                    "expand-sidebar",
                    IconName::PanelLeftOpen,
                    ButtonKind::Ghost,
                    ButtonSize::Regular,
                    self.tr("Expand sidebar"),
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx))),
            );
            rail = rail.child(
                components::icon_button(
                    tokens,
                    "new-host-rail",
                    IconName::Plus,
                    ButtonKind::Primary,
                    ButtonSize::Regular,
                    self.tr("New host"),
                )
                .on_click(cx.listener(|this, _, window, cx| this.new_profile(window, cx))),
            );
            rail = rail.child(
                components::icon_button(
                    tokens,
                    "import-config-rail",
                    IconName::FolderOpen,
                    ButtonKind::Ghost,
                    ButtonSize::Regular,
                    self.tr("Import OpenSSH config"),
                )
                .on_click(cx.listener(|this, _, _, cx| this.begin_config_import(cx))),
            );
            return rail.into_any_element();
        }

        let search = self.profile_search.read(cx).value().trim().to_lowercase();
        let mut profile_list = div()
            .id("host-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(ui_tokens::SPACE_1))
            .px(px(ui_tokens::SPACE_2))
            .overflow_y_scroll();
        let mut matching_profiles = 0;
        if search.is_empty() {
            let favorite_indexes = self
                .profiles
                .iter()
                .enumerate()
                .filter_map(|(index, profile)| profile.favorite.then_some(index))
                .collect::<Vec<_>>();
            if !favorite_indexes.is_empty() {
                profile_list =
                    profile_list.child(components::section_header(tokens, self.tr("FAVORITES")));
                for index in favorite_indexes {
                    let profile = &self.profiles[index];
                    profile_list = profile_list.child(self.saved_host_row(
                        tokens,
                        "favorite-host",
                        index,
                        profile,
                        cx,
                    ));
                }
                matching_profiles += 1;
            }

            let managed_group_names = self
                .settings
                .group_names
                .iter()
                .map(|name| (collapsed_group_key(name), name.clone()))
                .collect::<BTreeMap<_, _>>();
            let mut groups: BTreeMap<String, (String, Vec<usize>)> = BTreeMap::new();
            for (index, profile) in self.profiles.iter().enumerate() {
                let key = collapsed_group_key(&profile.group);
                let display = if profile.group.is_empty() {
                    self.tr("UNGROUPED")
                } else {
                    managed_group_names
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| profile.group.clone())
                };
                groups
                    .entry(key)
                    .or_insert_with(|| (display, Vec::new()))
                    .1
                    .push(index);
            }
            for name in &self.settings.group_names {
                let key = collapsed_group_key(name);
                groups
                    .entry(key)
                    .or_insert_with(|| (name.clone(), Vec::new()));
            }
            if !self.profiles.is_empty() {
                groups
                    .entry(String::new())
                    .or_insert_with(|| (self.tr("UNGROUPED"), Vec::new()));
            }
            let mut group_keys = groups.keys().cloned().collect::<Vec<_>>();
            let preferred_order = &self.settings.group_order;
            group_keys.sort_by_key(|key| {
                preferred_order
                    .iter()
                    .position(|preferred| preferred == key)
                    .unwrap_or(usize::MAX)
            });
            for key in group_keys {
                let Some((display, mut indexes)) = groups.get(&key).cloned() else {
                    continue;
                };
                indexes.sort_by_key(|index| self.profiles[*index].name.to_lowercase());
                let group_value = if key.is_empty() {
                    String::new()
                } else {
                    display.clone()
                };
                let collapsed = self
                    .collapsed_groups
                    .contains(&collapsed_group_key(&group_value));
                profile_list = profile_list.child(self.group_drop_header(
                    tokens,
                    display,
                    group_value,
                    !collapsed,
                    cx,
                ));
                if !collapsed {
                    for index in indexes {
                        let profile = &self.profiles[index];
                        profile_list = profile_list.child(self.saved_host_row(
                            tokens,
                            "group-host",
                            index,
                            profile,
                            cx,
                        ));
                    }
                }
                matching_profiles += 1;
            }

            if !self.recent.is_empty() {
                profile_list =
                    profile_list.child(components::section_header(tokens, self.tr("RECENT")));
                for (index, entry) in self.recent.iter().enumerate() {
                    profile_list =
                        profile_list.child(self.recent_host_row(tokens, index, entry, cx));
                }
                matching_profiles += 1;
            }
        } else {
            for (index, profile) in self.profiles.iter().enumerate() {
                let search_text = format!(
                    "{} {} {} {} {}",
                    profile.name, profile.username, profile.host, profile.port, profile.group
                )
                .to_lowercase();
                if !search_text.contains(&search) {
                    continue;
                }
                matching_profiles += 1;
                profile_list = profile_list.child(self.saved_host_row(
                    tokens,
                    "search-host",
                    index,
                    profile,
                    cx,
                ));
            }
        }
        if matching_profiles == 0 {
            if self.profiles.is_empty() {
                profile_list = profile_list.child(
                    components::empty_state(
                        tokens,
                        self.tr("No saved hosts"),
                        self.tr("Import an OpenSSH config or create a host to get started."),
                    )
                    .child(
                        components::button(
                            tokens,
                            "empty-new-host",
                            self.tr("New host"),
                            ButtonKind::Primary,
                            ButtonSize::Small,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.new_profile(window, cx);
                        })),
                    ),
                );
            } else {
                profile_list = profile_list.child(
                    components::empty_state(
                        tokens,
                        self.tr("No matching hosts"),
                        self.tr("Try a different name, group, user or port."),
                    )
                    .child(
                        components::button(
                            tokens,
                            "clear-host-search",
                            self.tr("Clear search"),
                            ButtonKind::Secondary,
                            ButtonSize::Small,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            Self::set_input_value(&this.profile_search, "", window, cx);
                            cx.notify();
                        })),
                    ),
                );
            }
        }

        div()
            .w(px(self.settings.sidebar_width))
            .h_full()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .min_h_0()
            .bg(tokens.surface)
            .border_r_1()
            .border_color(tokens.border_subtle)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(ui_tokens::SIDEBAR_HEADER_HEIGHT))
                    .flex_shrink_0()
                    .px(px(ui_tokens::SPACE_3))
                    .child(
                        div()
                            .text_size(px(ui_tokens::FONT_TITLE))
                            .font_semibold()
                            .text_color(tokens.text_primary)
                            .child("dshell"),
                    )
                    .child(
                        components::icon_button(
                            tokens,
                            "collapse-sidebar",
                            IconName::PanelLeftClose,
                            ButtonKind::Ghost,
                            ButtonSize::Regular,
                            self.tr("Collapse sidebar (Ctrl+Shift+B)"),
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx)),
                        ),
                    ),
            )
            .child(
                div()
                    .px(px(ui_tokens::SPACE_2))
                    .pb(px(ui_tokens::SPACE_2))
                    .child(
                        TextInput::new(&self.profile_search)
                            .h(px(ui_tokens::CONTROL_HEIGHT_SM))
                            .w_full(),
                    ),
            )
            .when(!self.profile_message.is_empty(), |sidebar| {
                sidebar.child(
                    div()
                        .mx(px(ui_tokens::SPACE_2))
                        .mb(px(ui_tokens::SPACE_2))
                        .p(px(ui_tokens::SPACE_2))
                        .rounded(px(ui_tokens::RADIUS_CONTROL))
                        .bg(tokens.surface_overlay)
                        .child(components::warning_text(
                            tokens,
                            self.profile_message.clone(),
                        )),
                )
            })
            .child(profile_list)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(ui_tokens::SPACE_2))
                    .p(px(ui_tokens::SPACE_2))
                    .border_t_1()
                    .border_color(tokens.border_subtle)
                    .child(
                        components::icon_button(
                            tokens,
                            "new-group",
                            IconName::Folder,
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                            self.tr("New group"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_group_editor(GroupEditorMode::Create, None, window, cx);
                        })),
                    )
                    .child(
                        components::button(
                            tokens,
                            "import-openssh-config",
                            self.tr("Import"),
                            ButtonKind::Secondary,
                            ButtonSize::Small,
                        )
                        .flex_1()
                        .on_click(cx.listener(|this, _, _, cx| this.begin_config_import(cx))),
                    )
                    .child(
                        components::button(
                            tokens,
                            "new-host-profile",
                            self.tr("New"),
                            ButtonKind::Primary,
                            ButtonSize::Small,
                        )
                        .flex_1()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.new_profile(window, cx);
                        })),
                    ),
            )
            .into_any_element()
    }

    fn session_tabs(&self, tokens: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let preview_tokens = *tokens;
        let mut tabs = div()
            .id("session-tabs")
            .flex()
            .items_center()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_x_scroll();
        for session in &self.sessions {
            let id = session.id;
            let selected = self.active_session_id == Some(id);
            let in_split = self.split.is_some_and(|split| split.contains(id));
            let mut tab = div()
                .id(("session-tab", id))
                .flex()
                .items_center()
                .gap(px(ui_tokens::SPACE_2))
                .flex_shrink_0()
                .h_full()
                .pl(px(ui_tokens::SPACE_3))
                .pr(px(ui_tokens::SPACE_1))
                .border_t_2()
                .border_color(if selected {
                    tokens.accent
                } else {
                    tokens.surface
                })
                .bg(if selected {
                    tokens.surface_raised
                } else {
                    tokens.surface
                })
                .text_color(if selected {
                    tokens.text_primary
                } else {
                    tokens.text_secondary
                })
                .hover(|style| style.bg(tokens.surface_raised))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_session(id, window, cx);
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.open_tab_menu(id, cx);
                    }),
                )
                .on_drag(
                    TabDragData {
                        session_id: id,
                        title: session.title.clone().into(),
                    },
                    move |data: &TabDragData, _, _, cx| {
                        let title = data.title.clone();
                        cx.new(|_| DragPreview {
                            title: title.clone(),
                            tokens: preview_tokens,
                        })
                    },
                )
                .on_drop(cx.listener(move |this, data: &TabDragData, _, cx| {
                    if this.reorder_sessions_before(data.session_id, id) {
                        cx.notify();
                    }
                }))
                .child(components::status_dot(tokens, session_tone(session.state)))
                .child(
                    div()
                        .max_w(px(180.0))
                        .overflow_hidden()
                        .text_size(px(ui_tokens::FONT_BODY))
                        .child(session.title.clone()),
                );
            if in_split {
                tab = tab.child(
                    div()
                        .text_size(px(ui_tokens::FONT_CAPTION))
                        .text_color(tokens.text_secondary)
                        .child(self.tr("split")),
                );
            }
            tab = tab.child(
                components::icon_button(
                    tokens,
                    ("close-session", id),
                    IconName::Close,
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    self.tr("Close session"),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.close_session(id, window, cx);
                })),
            );
            tabs = tabs.child(tab);
        }
        if self.sessions.is_empty() {
            tabs = tabs.child(
                div()
                    .px(px(ui_tokens::SPACE_3))
                    .child(components::help_text(tokens, self.tr("No active sessions"))),
            );
        }

        let mut right = div()
            .flex()
            .items_center()
            .gap(px(ui_tokens::SPACE_1))
            .px(px(ui_tokens::SPACE_2))
            .flex_shrink_0()
            .child(
                components::icon_button(
                    tokens,
                    "session-tab-menu",
                    IconName::Ellipsis,
                    if self.tab_menu_session.is_some() {
                        ButtonKind::Secondary
                    } else {
                        ButtonKind::Ghost
                    },
                    ButtonSize::Small,
                    self.tr("Session tab actions"),
                )
                .disabled(self.active_session_id.is_none())
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(id) = this.active_session_id {
                        this.open_tab_menu(id, cx);
                    }
                })),
            )
            .child(
                components::icon_button(
                    tokens,
                    "new-session-tab",
                    IconName::Plus,
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    self.tr("New session"),
                )
                .on_click(cx.listener(|this, _, window, cx| this.new_profile(window, cx))),
            );
        if let Some(split) = self.split {
            let side_by_side = if split.orientation == SplitOrientation::SideBySide {
                ButtonKind::Primary
            } else {
                ButtonKind::Ghost
            };
            let stacked = if split.orientation == SplitOrientation::Stacked {
                ButtonKind::Primary
            } else {
                ButtonKind::Ghost
            };
            right = right
                .child(
                    components::icon_button(
                        tokens,
                        "split-side-by-side",
                        IconName::PanelRight,
                        side_by_side,
                        ButtonSize::Small,
                        self.tr("Side by side"),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_split_orientation(SplitOrientation::SideBySide, window, cx);
                    })),
                )
                .child(
                    components::icon_button(
                        tokens,
                        "split-stacked",
                        IconName::PanelBottom,
                        stacked,
                        ButtonSize::Small,
                        self.tr("Stacked"),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_split_orientation(SplitOrientation::Stacked, window, cx);
                    })),
                )
                .child(
                    components::icon_button(
                        tokens,
                        "close-split",
                        IconName::Close,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        self.tr("Close split"),
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.close_split(window, cx))),
                );
        }

        div()
            .h(px(ui_tokens::TAB_BAR_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .bg(tokens.surface)
            .border_b_1()
            .border_color(tokens.border_subtle)
            .child(tabs)
            .child(right)
            .into_any_element()
    }

    fn command_bar(&self, tokens: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let active = self.active_session();
        let title = active
            .map(|session| session.title.clone())
            .unwrap_or_else(|| self.tr("SSH workspace"));
        let state = active.map(|session| session.state);

        let mut left = div()
            .flex()
            .items_center()
            .gap(px(ui_tokens::SPACE_1))
            .flex_shrink_0()
            .child(
                components::icon_button(
                    tokens,
                    "new-session",
                    IconName::Plus,
                    ButtonKind::Primary,
                    ButtonSize::Small,
                    self.tr("New session (Ctrl+Shift+T)"),
                )
                .on_click(cx.listener(|this, _, window, cx| this.new_profile(window, cx))),
            )
            .child(
                components::icon_button(
                    tokens,
                    "focus-session-search",
                    IconName::Search,
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    self.tr("Search saved hosts"),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.focus_profile_search(window, cx);
                })),
            );
        if self.selected_profile.is_some() {
            left = left.child(
                components::icon_button(
                    tokens,
                    "edit-session-profile",
                    IconName::Settings2,
                    ButtonKind::Secondary,
                    ButtonSize::Small,
                    self.tr("Session properties"),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.session_editor_section = SessionEditorSection::Connection;
                    this.session_editor_open = true;
                    this.request_overlay_focus();
                    cx.notify();
                })),
            );
        }
        if self.sessions.len() > 1 && self.active_session_id.is_some() {
            left = left.child(
                components::icon_button(
                    tokens,
                    "split-active-session",
                    IconName::PanelRight,
                    ButtonKind::Secondary,
                    ButtonSize::Small,
                    self.tr("Split with another session"),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    let Some(active_id) = this.active_session_id else {
                        return;
                    };
                    if let Some(other) = this
                        .sessions
                        .iter()
                        .map(|session| session.id)
                        .find(|id| *id != active_id)
                    {
                        this.set_split(other, SplitOrientation::SideBySide, window, cx);
                    }
                })),
            );
        }

        let middle = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .justify_center()
            .gap(px(ui_tokens::SPACE_2))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_size(px(ui_tokens::FONT_SUBTITLE))
                    .font_semibold()
                    .child(title),
            )
            .when_some(state, |middle, state| {
                middle.child(components::pill(
                    tokens,
                    self.tr(session_state_label(state)),
                    session_tone(state),
                ))
            });

        let mut right = div()
            .flex()
            .items_center()
            .gap(px(ui_tokens::SPACE_1))
            .flex_shrink_0();
        right = right
            .child(
                components::icon_button(
                    tokens,
                    "toggle-layout-menu",
                    IconName::LayoutDashboard,
                    if self.layout_menu_visible || self.inspector_visible {
                        ButtonKind::Secondary
                    } else {
                        ButtonKind::Ghost
                    },
                    ButtonSize::Small,
                    self.tr("Workspace layout"),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_layout_menu(window, cx);
                })),
            )
            .child(
                components::icon_button(
                    tokens,
                    "toggle-layout-sidebar",
                    if self.sidebar_collapsed {
                        IconName::PanelLeftOpen
                    } else {
                        IconName::PanelLeftClose
                    },
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    if self.sidebar_collapsed {
                        self.tr("Show sidebar")
                    } else {
                        self.tr("Hide sidebar")
                    },
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_sidebar(window, cx);
                })),
            );
        if let Some(session) = active {
            let active_id = session.id;
            right = right
                .child(
                    components::icon_button(
                        tokens,
                        "toggle-sftp",
                        IconName::Folder,
                        if session.sftp.visible {
                            ButtonKind::Secondary
                        } else {
                            ButtonKind::Ghost
                        },
                        ButtonSize::Small,
                        self.tr("Files (Ctrl+Shift+F)"),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_sftp(active_id, cx);
                    })),
                )
                .child(
                    components::icon_button(
                        tokens,
                        "toggle-tunnels",
                        IconName::Globe,
                        if session.forwards.visible {
                            ButtonKind::Secondary
                        } else {
                            ButtonKind::Ghost
                        },
                        ButtonSize::Small,
                        self.tr("Tunnels"),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_tunnels(active_id, cx);
                    })),
                )
                .child(
                    components::icon_button(
                        tokens,
                        "toggle-log",
                        if session.log_path.is_some() {
                            IconName::CircleCheck
                        } else {
                            IconName::Dash
                        },
                        if session.log_path.is_some() {
                            ButtonKind::Secondary
                        } else {
                            ButtonKind::Ghost
                        },
                        ButtonSize::Small,
                        if session.log_path.is_some() {
                            self.tr("Stop logging")
                        } else {
                            self.tr("Start logging")
                        },
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_log(active_id, cx);
                    })),
                )
                .child(
                    components::icon_button(
                        tokens,
                        "replay-log",
                        IconName::BookOpen,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        self.tr("Replay log (Ctrl+Shift+R)"),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.begin_log_replay(cx))),
                )
                .child(
                    components::icon_button(
                        tokens,
                        "find-terminal",
                        IconName::Search,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        self.tr("Find (Ctrl+F)"),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_search(active_id, window, cx);
                    })),
                );
            if matches!(
                session.state,
                SessionState::Connecting | SessionState::Connected
            ) {
                let connecting = session.state == SessionState::Connecting;
                right = right.child(
                    components::icon_button(
                        tokens,
                        "disconnect-active",
                        IconName::CircleX,
                        ButtonKind::Secondary,
                        ButtonSize::Small,
                        if connecting {
                            self.tr("Cancel")
                        } else {
                            self.tr("Disconnect")
                        },
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.disconnect_session(active_id, cx);
                    })),
                );
            }
        }
        right = right
            .child(
                components::icon_button(
                    tokens,
                    "toggle-keywords",
                    IconName::CaseSensitive,
                    if self.keywords_visible || self.keyword_highlighting_enabled {
                        ButtonKind::Secondary
                    } else {
                        ButtonKind::Ghost
                    },
                    ButtonSize::Small,
                    self.tr("Keyword highlights"),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_keywords(window, cx);
                })),
            )
            .child(
                components::icon_button(
                    tokens,
                    "toggle-triggers",
                    IconName::Bell,
                    if self.triggers_visible || self.trigger_engine_enabled {
                        ButtonKind::Secondary
                    } else {
                        ButtonKind::Ghost
                    },
                    ButtonSize::Small,
                    self.tr("Triggers"),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_triggers(window, cx);
                })),
            )
            .child(
                components::icon_button(
                    tokens,
                    "toggle-snippets",
                    IconName::Asterisk,
                    if self.snippets_visible {
                        ButtonKind::Secondary
                    } else {
                        ButtonKind::Ghost
                    },
                    ButtonSize::Small,
                    self.tr("Quick commands (Ctrl+Shift+S)"),
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_snippets(cx))),
            )
            .child(
                components::icon_button(
                    tokens,
                    "toggle-keys",
                    IconName::User,
                    if self.keys_visible {
                        ButtonKind::Secondary
                    } else {
                        ButtonKind::Ghost
                    },
                    ButtonSize::Small,
                    self.tr("Keys"),
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_keys(window, cx))),
            )
            .child(
                components::icon_button(
                    tokens,
                    "toggle-sync",
                    IconName::ChevronsUpDown,
                    if self.sync_visible || self.sync_target_count() > 0 {
                        ButtonKind::Secondary
                    } else {
                        ButtonKind::Ghost
                    },
                    ButtonSize::Small,
                    format!(
                        "{} ({} {})",
                        self.tr("Synchronized input"),
                        self.sync_target_count(),
                        self.tr("target"),
                    ),
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_sync_modal(window, cx))),
            )
            .child(
                components::icon_button(
                    tokens,
                    "toggle-ui-theme",
                    IconName::Palette,
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    self.tr(&format!(
                        "Switch to {} theme",
                        self.ui_theme.toggled().label()
                    )),
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_ui_theme(cx))),
            )
            .child(
                components::icon_button(
                    tokens,
                    "toggle-settings",
                    IconName::Settings,
                    if self.settings_visible {
                        ButtonKind::Secondary
                    } else {
                        ButtonKind::Ghost
                    },
                    ButtonSize::Small,
                    self.tr("Settings (Ctrl+Shift+O)"),
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_settings(window, cx))),
            )
            .child(
                components::icon_button(
                    tokens,
                    "main-menu",
                    IconName::Ellipsis,
                    if self.menu_visible {
                        ButtonKind::Secondary
                    } else {
                        ButtonKind::Ghost
                    },
                    ButtonSize::Small,
                    self.tr("Menu"),
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_main_menu(window, cx))),
            );

        div()
            .h(px(ui_tokens::COMMAND_BAR_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(ui_tokens::SPACE_2))
            .px(px(ui_tokens::SPACE_3))
            .bg(tokens.surface)
            .border_b_1()
            .border_color(tokens.border_subtle)
            .child(left)
            .child(middle)
            .child(right)
            .into_any_element()
    }

    fn status_bar(&self, tokens: &ThemeTokens) -> AnyElement {
        let active = self.active_session();
        let (tone, name, status) = match active {
            Some(session) => (
                session_tone(session.state),
                session.title.clone(),
                session.status.clone(),
            ),
            None => (Tone::Neutral, "dshell".to_owned(), self.tr(&self.status)),
        };
        let identity = active
            .map(|session| format!("{}@{}:{}", session.username, session.host, session.port))
            .unwrap_or_default();
        let dimensions = active
            .map(|session| {
                format!(
                    "{} x {}",
                    session.terminal_size.columns, session.terminal_size.rows
                )
            })
            .unwrap_or_default();
        let sync_count = self.sync_target_count();

        div()
            .h(px(ui_tokens::STATUS_BAR_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(ui_tokens::SPACE_3))
            .px(px(ui_tokens::SPACE_3))
            .bg(tokens.surface)
            .border_t_1()
            .border_color(tokens.border_subtle)
            .text_size(px(ui_tokens::FONT_CAPTION))
            .text_color(tokens.text_secondary)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(ui_tokens::SPACE_2))
                    .min_w_0()
                    .child(components::status_dot(tokens, tone))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(tokens.text_primary)
                            .child(name),
                    )
                    .child(div().min_w_0().overflow_hidden().child(status)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(ui_tokens::SPACE_3))
                    .flex_shrink_0()
                    .when(sync_count > 0, |right| {
                        right.child(components::pill(
                            tokens,
                            format!("{} {sync_count}", self.tr("Sync")),
                            Tone::Accent,
                        ))
                    })
                    .when(!identity.is_empty(), |right| right.child(identity))
                    .when(!dimensions.is_empty(), |right| right.child(dimensions)),
            )
            .into_any_element()
    }

    fn sync_overlay(&mut self, tokens: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let connected = self
            .sessions
            .iter()
            .filter(|session| session.state == SessionState::Connected)
            .map(|session| {
                (
                    session.id,
                    session.title.clone(),
                    format!("{}@{}:{}", session.username, session.host, session.port),
                    session.state,
                )
            })
            .collect::<Vec<_>>();
        let sync_count = self.sync_target_count();
        let mut list = div()
            .id("sync-targets")
            .flex()
            .flex_col()
            .gap(px(ui_tokens::SPACE_1))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(ui_tokens::SPACE_4));
        if connected.is_empty() {
            list = list.child(components::empty_state(
                tokens,
                self.tr("No connected sessions"),
                self.tr("Connect at least one session before enabling synchronized input."),
            ));
        } else {
            for (id, title, identity, state) in connected {
                let selected = self.sync_targets.contains(&id);
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(ui_tokens::SPACE_2))
                        .p(px(ui_tokens::SPACE_2))
                        .rounded(px(ui_tokens::RADIUS_CONTROL))
                        .border_1()
                        .border_color(tokens.border_subtle)
                        .bg(tokens.surface)
                        .child(components::status_dot(tokens, session_tone(state)))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .overflow_hidden()
                                        .text_size(px(ui_tokens::FONT_BODY))
                                        .text_color(tokens.text_primary)
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .overflow_hidden()
                                        .text_size(px(ui_tokens::FONT_CAPTION))
                                        .text_color(tokens.text_secondary)
                                        .child(identity),
                                ),
                        )
                        .child(
                            components::button(
                                tokens,
                                ("sync-target", id),
                                if selected {
                                    self.tr("Syncing")
                                } else {
                                    self.tr("Add")
                                },
                                if selected {
                                    ButtonKind::Secondary
                                } else {
                                    ButtonKind::Ghost
                                },
                                ButtonSize::Small,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.toggle_sync_target(id, cx);
                                },
                            )),
                        ),
                );
            }
        }

        components::modal_overlay(tokens, "sync-overlay")
            .track_focus(&self.overlay_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.toggle_sync_modal(window, cx);
                }
            }))
            .child(
                components::modal_panel(tokens, "sync-panel")
                    .w_full()
                    .max_w(px(480.))
                    .max_h(px(560.))
                    .child(
                        components::modal_header(tokens, self.tr("Synchronized input")).child(
                            components::help_text(
                                tokens,
                                self.tr("{count} target(s) selected")
                                    .replace("{count}", &sync_count.to_string()),
                            ),
                        ),
                    )
                    .child(list)
                    .child(
                        div()
                            .px(px(ui_tokens::SPACE_4))
                            .pb(px(ui_tokens::SPACE_2))
                            .child(components::help_text(
                                tokens,
                                self.tr("Keyboard, paste and compose are mirrored; mouse, scrolling and resize stay per-session."),
                            )),
                    )
                    .child(
                        components::modal_footer(tokens)
                            .child(
                                components::button(
                                    tokens,
                                    "clear-sync-targets",
                                    self.tr("Clear"),
                                    ButtonKind::Ghost,
                                    ButtonSize::Regular,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.clear_sync_targets(cx);
                                })),
                            )
                            .child(
                                components::button(
                                    tokens,
                                    "close-sync-panel",
                                    self.tr("Done"),
                                    ButtonKind::Primary,
                                    ButtonSize::Regular,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_sync_modal(window, cx);
                                })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn tab_menu_overlay(&mut self, tokens: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let Some(session_id) = self.tab_menu_session else {
            return div().into_any_element();
        };
        let Some(session) = self.session(session_id) else {
            return div().into_any_element();
        };
        let title = session.title.clone();
        let active = self.active_session_id == Some(session_id);
        let connected = matches!(
            session.state,
            SessionState::Connecting | SessionState::Connected
        );
        let session_index = self
            .sessions
            .iter()
            .position(|session| session.id == session_id)
            .unwrap_or_default();
        let has_other = self.sessions.iter().any(|session| session.id != session_id);
        let has_right = session_index + 1 < self.sessions.len();
        let menu = div()
            .id("session-tab-menu")
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .flex()
            .flex_col()
            .gap(px(ui_tokens::SPACE_1))
            .w(px(ui_tokens::MENU_WIDTH))
            .p(px(ui_tokens::SPACE_2))
            .border_1()
            .border_color(tokens.border_strong)
            .rounded(px(ui_tokens::RADIUS_PANEL))
            .shadow_lg()
            .bg(tokens.surface_overlay)
            .text_color(tokens.text_primary)
            .child(
                components::panel_header(tokens, title).child(
                    components::icon_button(
                        tokens,
                        "session-tab-menu-close",
                        IconName::Close,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        self.tr("Close tab actions"),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_tab_menu(window, cx);
                    })),
                ),
            )
            .child(
                menu_item(
                    tokens,
                    "tab-menu-properties",
                    IconName::Settings2,
                    self.tr("Session properties"),
                    "",
                    active,
                    true,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.tab_menu_session = None;
                    this.select_session(session_id, window, cx);
                    this.session_editor_section = SessionEditorSection::Connection;
                    this.session_editor_open = true;
                    this.request_overlay_focus();
                    cx.notify();
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "tab-menu-split-side",
                    IconName::PanelRight,
                    self.tr("Split side by side"),
                    "",
                    self.split.is_some_and(|split| {
                        split.orientation == SplitOrientation::SideBySide
                            && split.contains(session_id)
                    }),
                    has_other,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.active_session_id = Some(session_id);
                    this.apply_split_orientation(SplitOrientation::SideBySide, window, cx);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "tab-menu-split-stack",
                    IconName::PanelBottom,
                    self.tr("Split stacked"),
                    "",
                    self.split.is_some_and(|split| {
                        split.orientation == SplitOrientation::Stacked && split.contains(session_id)
                    }),
                    has_other,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.active_session_id = Some(session_id);
                    this.apply_split_orientation(SplitOrientation::Stacked, window, cx);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "tab-menu-disconnect",
                    IconName::CircleX,
                    if session.state == SessionState::Connecting {
                        self.tr("Cancel connection")
                    } else {
                        self.tr("Disconnect")
                    },
                    "",
                    false,
                    connected,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.tab_menu_session = None;
                    this.disconnect_session(session_id, cx);
                    window.focus(&this.terminal_focus);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "tab-menu-close",
                    IconName::Close,
                    self.tr("Close tab"),
                    "Ctrl+Shift+W",
                    active,
                    true,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.tab_menu_session = None;
                    this.close_session(session_id, window, cx);
                })),
            )
            .child(components::section_label(tokens, self.tr("TABS")))
            .child(
                menu_item(
                    tokens,
                    "tab-menu-close-others",
                    IconName::PanelLeft,
                    self.tr("Close other tabs"),
                    "",
                    false,
                    has_other,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.close_other_sessions(session_id, window, cx);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "tab-menu-close-right",
                    IconName::ArrowRight,
                    self.tr("Close tabs to the right"),
                    "",
                    false,
                    has_right,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.close_sessions_to_right(session_id, window, cx);
                })),
            );

        div()
            .id("session-tab-menu-overlay")
            .absolute()
            .size_full()
            .occlude()
            .track_focus(&self.overlay_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.close_tab_menu(window, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.close_tab_menu(window, cx);
                }),
            )
            .child(
                div()
                    .absolute()
                    .top(px(ui_tokens::COMMAND_BAR_HEIGHT + ui_tokens::TAB_BAR_HEIGHT))
                    .right(px(ui_tokens::SPACE_2))
                    .child(menu),
            )
            .into_any_element()
    }

    fn layout_menu_overlay(&mut self, tokens: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let focus_selected =
            self.split.is_none() && self.sidebar_collapsed && !self.inspector_visible;
        let standard_selected =
            self.split.is_none() && !self.sidebar_collapsed && !self.inspector_visible;
        let inspector_selected =
            self.split.is_none() && !self.sidebar_collapsed && self.inspector_visible;
        let split_selected = self.split.is_some();
        let has_other_session = self
            .active_session_id
            .is_some_and(|active| self.sessions.iter().any(|session| session.id != active));
        let mut menu = div()
            .id("layout-menu")
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .flex()
            .flex_col()
            .gap(px(ui_tokens::SPACE_2))
            .w(px(ui_tokens::LAYOUT_MENU_WIDTH))
            .p(px(ui_tokens::SPACE_2))
            .border_1()
            .border_color(tokens.border_strong)
            .rounded(px(ui_tokens::RADIUS_PANEL))
            .shadow_lg()
            .bg(tokens.surface_overlay)
            .text_color(tokens.text_primary);
        menu = menu
            .child(
                components::panel_header(tokens, self.tr("Workspace layout")).child(
                    components::icon_button(
                        tokens,
                        "layout-menu-close",
                        IconName::Close,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        self.tr("Close"),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_layout_menu(window, cx);
                    })),
                ),
            )
            .child(components::help_text(
                tokens,
                self.tr("Choose a workspace preset. Divider sizes remain adjustable."),
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(ui_tokens::SPACE_1))
                    .child(
                        layout_preset_item(
                            tokens,
                            "layout-preset-focus",
                            IconName::PanelLeftClose,
                            self.tr("Focus"),
                            self.tr("Focus workspace"),
                            focus_selected,
                            true,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_layout_preset(LayoutPreset::Focus, window, cx);
                        })),
                    )
                    .child(
                        layout_preset_item(
                            tokens,
                            "layout-preset-standard",
                            IconName::LayoutDashboard,
                            self.tr("Standard"),
                            self.tr("Standard workspace"),
                            standard_selected,
                            true,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_layout_preset(LayoutPreset::Standard, window, cx);
                        })),
                    )
                    .child(
                        layout_preset_item(
                            tokens,
                            "layout-preset-inspector",
                            IconName::Inspector,
                            self.tr("Inspector"),
                            self.tr("Workspace with inspector"),
                            inspector_selected,
                            true,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_layout_preset(LayoutPreset::Inspector, window, cx);
                        })),
                    )
                    .child(
                        layout_preset_item(
                            tokens,
                            "layout-preset-split",
                            IconName::PanelRight,
                            self.tr("Split"),
                            self.tr("Split active session"),
                            split_selected,
                            has_other_session,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_layout_preset(LayoutPreset::Split, window, cx);
                        })),
                    ),
            )
            .child(components::section_label(
                tokens,
                self.tr("SPLIT ORIENTATION"),
            ))
            .child(
                div()
                    .flex()
                    .gap(px(ui_tokens::SPACE_1))
                    .child(
                        components::icon_button(
                            tokens,
                            "layout-split-side",
                            IconName::PanelRight,
                            if self.split.is_some_and(|split| {
                                split.orientation == SplitOrientation::SideBySide
                            }) {
                                ButtonKind::Primary
                            } else {
                                ButtonKind::Ghost
                            },
                            ButtonSize::Small,
                            self.tr("Split side by side"),
                        )
                        .disabled(!has_other_session)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_split_orientation(SplitOrientation::SideBySide, window, cx);
                        })),
                    )
                    .child(
                        components::icon_button(
                            tokens,
                            "layout-split-stack",
                            IconName::PanelBottom,
                            if self
                                .split
                                .is_some_and(|split| split.orientation == SplitOrientation::Stacked)
                            {
                                ButtonKind::Primary
                            } else {
                                ButtonKind::Ghost
                            },
                            ButtonSize::Small,
                            self.tr("Split stacked"),
                        )
                        .disabled(!has_other_session)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_split_orientation(SplitOrientation::Stacked, window, cx);
                        })),
                    )
                    .child(
                        components::icon_button(
                            tokens,
                            "layout-close-split",
                            IconName::Close,
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                            self.tr("Close"),
                        )
                        .disabled(self.split.is_none())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_split(window, cx);
                            this.close_layout_menu(window, cx);
                        })),
                    ),
            )
            .child(components::help_text(
                tokens,
                self.tr("Use the divider handles to fine-tune widths after choosing a preset."),
            ));

        div()
            .id("layout-menu-overlay")
            .absolute()
            .size_full()
            .occlude()
            .track_focus(&self.overlay_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.close_layout_menu(window, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.close_layout_menu(window, cx);
                }),
            )
            .child(
                div()
                    .absolute()
                    .top(px(ui_tokens::COMMAND_BAR_HEIGHT + ui_tokens::SPACE_1))
                    .right(px(ui_tokens::SPACE_2))
                    .child(menu),
            )
            .into_any_element()
    }

    fn main_menu_overlay(&mut self, tokens: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let active_id = self.active_session_id;
        let active_connected = active_id.is_some_and(|id| {
            self.session(id).is_some_and(|session| {
                matches!(
                    session.state,
                    SessionState::Connecting | SessionState::Connected
                )
            })
        });
        let other_session = active_id.and_then(|active| {
            self.sessions
                .iter()
                .map(|session| session.id)
                .find(|id| *id != active)
        });
        let log_active = active_id
            .and_then(|id| self.session(id))
            .is_some_and(|session| session.log_path.is_some());
        let mut menu = div()
            .id("main-menu")
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .flex()
            .flex_col()
            .gap(px(ui_tokens::SPACE_1))
            .w(px(ui_tokens::MENU_WIDTH))
            .max_h(px(ui_tokens::MENU_MAX_HEIGHT))
            .overflow_y_scroll()
            .p(px(ui_tokens::SPACE_2))
            .border_1()
            .border_color(tokens.border_strong)
            .rounded(px(ui_tokens::RADIUS_PANEL))
            .shadow_lg()
            .bg(tokens.surface_overlay)
            .text_color(tokens.text_primary);
        menu = menu
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(ui_tokens::SPACE_1))
                    .pb(px(ui_tokens::SPACE_2))
                    .border_b_1()
                    .border_color(tokens.border_subtle)
                    .child(
                        components::icon_button(
                            tokens,
                            "menu-quick-new-session",
                            IconName::Plus,
                            ButtonKind::Primary,
                            ButtonSize::Small,
                            self.tr("New session"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.menu_visible = false;
                            window.focus(&this.terminal_focus);
                            this.new_profile(window, cx);
                        })),
                    )
                    .child(
                        components::icon_button(
                            tokens,
                            "menu-quick-search",
                            IconName::Search,
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                            self.tr("Search saved hosts"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.menu_visible = false;
                            this.focus_profile_search(window, cx);
                        })),
                    )
                    .child(
                        components::icon_button(
                            tokens,
                            "menu-quick-layout",
                            IconName::LayoutDashboard,
                            if self.inspector_visible {
                                ButtonKind::Secondary
                            } else {
                                ButtonKind::Ghost
                            },
                            ButtonSize::Small,
                            self.tr("Workspace layout"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.menu_visible = false;
                            this.toggle_layout_menu(window, cx);
                        })),
                    )
                    .child(
                        components::icon_button(
                            tokens,
                            "menu-quick-settings",
                            IconName::Settings,
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                            self.tr("Settings"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.menu_visible = false;
                            this.toggle_settings(window, cx);
                        })),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(ui_tokens::FONT_CAPTION))
                            .text_color(tokens.text_secondary)
                            .child(self.tr("Quick actions")),
                    ),
            )
            .child(components::section_label(tokens, self.tr("SESSION")))
            .child(
                menu_item(
                    tokens,
                    "menu-new-session",
                    IconName::Plus,
                    self.tr("New session"),
                    "Ctrl+Shift+T",
                    false,
                    true,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    window.focus(&this.terminal_focus);
                    this.new_profile(window, cx);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-session-properties",
                    IconName::Settings2,
                    self.tr("Session properties"),
                    "",
                    false,
                    active_id.is_some(),
                )
                .on_click(cx.listener(|this, _, _window, cx| {
                    this.menu_visible = false;
                    this.session_editor_section = SessionEditorSection::Connection;
                    this.session_editor_open = true;
                    this.request_overlay_focus();
                    cx.notify();
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-import-config",
                    IconName::FolderOpen,
                    self.tr("Import OpenSSH config"),
                    "",
                    false,
                    true,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    window.focus(&this.terminal_focus);
                    this.begin_config_import(cx);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-import-host-profiles",
                    IconName::FolderOpen,
                    self.tr("Import dshell host profiles"),
                    "",
                    false,
                    true,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    window.focus(&this.terminal_focus);
                    this.begin_profile_import(cx);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-export-host-profiles",
                    IconName::ArrowUp,
                    self.tr("Export host profiles"),
                    "",
                    false,
                    !self.profiles.is_empty(),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    window.focus(&this.terminal_focus);
                    this.begin_profile_export(cx);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-close-tab",
                    IconName::Close,
                    self.tr("Close tab"),
                    "Ctrl+Shift+W",
                    false,
                    active_id.is_some(),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    if let Some(id) = this.active_session_id {
                        this.close_session(id, window, cx);
                    }
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-disconnect",
                    IconName::CircleX,
                    self.tr("Disconnect"),
                    "Ctrl+Shift+D",
                    false,
                    active_connected,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    if let Some(id) = this.active_session_id {
                        this.disconnect_session(id, cx);
                    }
                    window.focus(&this.terminal_focus);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-split-side",
                    IconName::PanelRight,
                    self.tr("Split side by side"),
                    "",
                    false,
                    other_session.is_some(),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    let active = this.active_session_id;
                    if let Some(other) = this
                        .sessions
                        .iter()
                        .map(|session| session.id)
                        .find(|id| Some(*id) != active)
                    {
                        this.set_split(other, SplitOrientation::SideBySide, window, cx);
                    }
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-split-stack",
                    IconName::PanelBottom,
                    self.tr("Split stacked"),
                    "",
                    false,
                    other_session.is_some(),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    let active = this.active_session_id;
                    if let Some(other) = this
                        .sessions
                        .iter()
                        .map(|session| session.id)
                        .find(|id| Some(*id) != active)
                    {
                        this.set_split(other, SplitOrientation::Stacked, window, cx);
                    }
                })),
            )
            .child(components::section_label(tokens, self.tr("VIEW")))
            .child(
                menu_item(
                    tokens,
                    "menu-layout",
                    IconName::LayoutDashboard,
                    self.tr("Workspace layout"),
                    "",
                    self.layout_menu_visible,
                    true,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    this.toggle_layout_menu(window, cx);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-sidebar",
                    IconName::PanelLeft,
                    if self.sidebar_collapsed {
                        self.tr("Show sidebar")
                    } else {
                        self.tr("Hide sidebar")
                    },
                    "Ctrl+Shift+B",
                    !self.sidebar_collapsed,
                    true,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    this.toggle_sidebar(window, cx);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-inspector",
                    IconName::Inspector,
                    self.tr("Toggle inspector"),
                    "",
                    self.inspector_visible,
                    true,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    this.toggle_inspector(window, cx);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-compose",
                    IconName::PanelBottom,
                    self.tr("Toggle compose"),
                    "Ctrl+Shift+M",
                    self.compose_visible,
                    true,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    this.toggle_compose(window, cx);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-sync",
                    IconName::ChevronsUpDown,
                    self.tr("Synchronized input"),
                    "",
                    self.sync_visible || self.sync_target_count() > 0,
                    true,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    this.toggle_sync_modal(window, cx);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-theme",
                    IconName::Palette,
                    self.tr(&format!(
                        "Switch to {} theme",
                        self.ui_theme.toggled().label()
                    )),
                    "",
                    false,
                    true,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.menu_visible = false;
                    this.toggle_ui_theme(cx);
                })),
            )
            .child(components::section_label(tokens, self.tr("TOOLS")))
            .child(
                menu_item(
                    tokens,
                    "menu-files",
                    IconName::Folder,
                    self.tr("Files"),
                    "Ctrl+Shift+F",
                    false,
                    active_id.is_some(),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    if let Some(id) = this.active_session_id {
                        this.toggle_sftp(id, cx);
                    }
                    window.focus(&this.terminal_focus);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-tunnels",
                    IconName::Globe,
                    self.tr("Tunnels"),
                    "",
                    false,
                    active_id.is_some(),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    if let Some(id) = this.active_session_id {
                        this.toggle_tunnels(id, cx);
                    }
                    window.focus(&this.terminal_focus);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-log",
                    if log_active {
                        IconName::CircleCheck
                    } else {
                        IconName::Dash
                    },
                    if log_active {
                        self.tr("Stop logging")
                    } else {
                        self.tr("Start logging")
                    },
                    "",
                    log_active,
                    active_id.is_some(),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    if let Some(id) = this.active_session_id {
                        this.toggle_log(id, cx);
                    }
                    window.focus(&this.terminal_focus);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-replay",
                    IconName::BookOpen,
                    self.tr("Replay log"),
                    "Ctrl+Shift+R",
                    false,
                    true,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    this.begin_log_replay(cx);
                    window.focus(&this.terminal_focus);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-find",
                    IconName::Search,
                    self.tr("Find"),
                    "Ctrl+F",
                    false,
                    active_id.is_some(),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    if let Some(id) = this.active_session_id {
                        this.open_search(id, window, cx);
                    }
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-keywords",
                    IconName::CaseSensitive,
                    self.tr("Keyword highlights"),
                    "",
                    self.keywords_visible || self.keyword_highlighting_enabled,
                    true,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    this.toggle_keywords(window, cx);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-triggers",
                    IconName::Bell,
                    self.tr("Triggers"),
                    "",
                    self.triggers_visible || self.trigger_engine_enabled,
                    true,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    this.toggle_triggers(window, cx);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-snippets",
                    IconName::Asterisk,
                    self.tr("Quick commands"),
                    "Ctrl+Shift+S",
                    self.snippets_visible,
                    true,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.menu_visible = false;
                    this.toggle_snippets(cx);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-keys",
                    IconName::User,
                    self.tr("Keys"),
                    "",
                    self.keys_visible,
                    true,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    this.toggle_keys(window, cx);
                })),
            )
            .child(
                menu_item(
                    tokens,
                    "menu-settings",
                    IconName::Settings,
                    self.tr("Settings"),
                    "Ctrl+Shift+O",
                    self.settings_visible,
                    true,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.menu_visible = false;
                    this.toggle_settings(window, cx);
                })),
            )
            .child(
                div()
                    .pt(px(ui_tokens::SPACE_2))
                    .px(px(ui_tokens::SPACE_2))
                    .child(components::help_text(
                        tokens,
                        self.tr("dshell {version} · native GPUI · Esc closes")
                            .replace("{version}", env!("CARGO_PKG_VERSION")),
                    )),
            );

        div()
            .id("main-menu-overlay")
            .absolute()
            .size_full()
            .occlude()
            .track_focus(&self.overlay_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    this.close_main_menu(window, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.close_main_menu(window, cx);
                }),
            )
            .child(
                div()
                    .absolute()
                    .top(px(ui_tokens::COMMAND_BAR_HEIGHT + 4.0))
                    .right(px(ui_tokens::SPACE_2))
                    .child(menu),
            )
            .into_any_element()
    }

    fn inspector(
        &mut self,
        session_id: u64,
        tokens: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session) = self.session(session_id) else {
            return div().into_any_element();
        };
        let transfer = session.sftp.transfer.clone();
        let transfer_message = session.sftp.message.clone();
        let sftp_visible = session.sftp.visible;
        let rules = session.forwards.rules.clone();
        let stopped_rules = session.forwards.stopped.clone();
        let forward_message = session.forwards.message.clone();
        let forwards_visible = session.forwards.visible;
        let username = session.username.clone();
        let host = session.host.clone();
        let port = session.port;
        let state = session.state;
        let terminal_size = session.terminal_size;
        let log_active = session.log_path.is_some();
        let log_path = session.log_path.clone();
        let connection_info = session.connection_info.clone();
        let server_metrics = session.server_metrics.clone();
        let power_action_loading = server_metrics.power_action_loading;
        let power_action_message = server_metrics.power_action_message.clone();
        let server_processes = server_metrics.processes.clone();
        let processes_loading = server_metrics.processes_loading;
        let processes_message = server_metrics.processes_message.clone();
        let network = server_metrics.network.clone();
        let network_loading = server_metrics.network_loading;
        let network_message = server_metrics.network_message.clone();
        let storage = server_metrics.storage.clone();
        let storage_loading = server_metrics.storage_loading;
        let storage_message = server_metrics.storage_message.clone();
        let listeners = server_metrics.listeners.clone();
        let listeners_loading = server_metrics.listeners_loading;
        let listeners_message = server_metrics.listeners_message.clone();
        let server_services = server_metrics.services.clone();
        let services_loading = server_metrics.services_loading;
        let services_message = server_metrics.services_message.clone();
        let service_logs = server_metrics.service_logs.clone();
        let service_logs_loading = server_metrics.service_logs_loading;
        let service_logs_message = server_metrics.service_logs_message.clone();
        let alert_history = server_metrics.alert_history.clone();
        let alert_cpu_percent = self.settings.alert_cpu_percent;
        let alert_memory_percent = self.settings.alert_memory_percent;
        let alert_disk_percent = self.settings.alert_disk_percent;
        let profile = session
            .profile_name
            .as_ref()
            .and_then(|name| {
                self.profiles
                    .iter()
                    .find(|profile| profile.name.eq_ignore_ascii_case(name))
            })
            .cloned();

        let mut body = div()
            .id("inspector-body")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();

        body = body.child(
            components::collapsible_header(
                tokens,
                "inspector-transfers",
                self.tr("Transfers"),
                self.inspector_transfers_open,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.inspector_transfers_open = !this.inspector_transfers_open;
                cx.notify();
            })),
        );
        if self.inspector_transfers_open {
            let mut group = div()
                .flex()
                .flex_col()
                .gap(px(ui_tokens::SPACE_2))
                .p(px(ui_tokens::SPACE_3))
                .border_b_1()
                .border_color(tokens.border_subtle);
            if let Some(transfer) = transfer {
                let ratio = if transfer.total > 0 {
                    transfer.transferred as f32 / transfer.total as f32
                } else {
                    0.0
                };
                group = group
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(ui_tokens::SPACE_2))
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_size(px(ui_tokens::FONT_CAPTION))
                                    .text_color(tokens.text_primary)
                                    .child(transfer.name.clone()),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(ui_tokens::FONT_CAPTION))
                                    .text_color(tokens.text_secondary)
                                    .child(self.tr(transfer.kind.label())),
                            ),
                    )
                    .child(components::progress_bar(tokens, ratio))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(ui_tokens::FONT_CAPTION))
                            .text_color(tokens.text_secondary)
                            .child(format!(
                                "{} / {} · {}",
                                format_size(transfer.transferred),
                                format_size(transfer.total),
                                format_rate(transfer.rate_bytes_per_second)
                            ))
                            .child(format!("{:.0}%", ratio * 100.0)),
                    )
                    .child(
                        components::button(
                            tokens,
                            ("inspector-cancel-transfer", session_id),
                            self.tr("Cancel transfer"),
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.cancel_transfer(session_id, cx);
                        })),
                    );
            } else {
                group = group.child(components::help_text(
                    tokens,
                    if transfer_message.is_empty() {
                        self.tr("No active transfer.")
                    } else {
                        transfer_message.clone()
                    },
                ));
            }
            group = group.child(
                components::button(
                    tokens,
                    "inspector-files",
                    if sftp_visible {
                        self.tr("Close files")
                    } else {
                        self.tr("Open files")
                    },
                    ButtonKind::Secondary,
                    ButtonSize::Small,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_sftp(session_id, cx);
                })),
            );
            body = body.child(group);
        }

        body = body.child(
            components::collapsible_header(
                tokens,
                "inspector-forwards",
                self.tr("Forwards"),
                self.inspector_forwards_open,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.inspector_forwards_open = !this.inspector_forwards_open;
                cx.notify();
            })),
        );
        if self.inspector_forwards_open {
            let mut group = div()
                .flex()
                .flex_col()
                .gap(px(ui_tokens::SPACE_2))
                .p(px(ui_tokens::SPACE_3))
                .border_b_1()
                .border_color(tokens.border_subtle);
            if rules.is_empty() && stopped_rules.is_empty() {
                group = group.child(components::help_text(
                    tokens,
                    self.tr("No forwarding rules."),
                ));
            } else {
                for (rule_id, rule) in rules {
                    let target = if rule.spec.kind.needs_target() {
                        format!("{}:{}", rule.spec.target_host, rule.spec.target_port)
                    } else {
                        "SOCKS5".to_owned()
                    };
                    group = group.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .p(px(ui_tokens::SPACE_2))
                            .rounded(px(ui_tokens::RADIUS_CONTROL))
                            .bg(tokens.surface)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(ui_tokens::SPACE_2))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .text_size(px(ui_tokens::FONT_CAPTION))
                                            .text_color(tokens.text_primary)
                                            .child(format!(
                                                "{} {}:{}",
                                                self.tr(rule.spec.kind.label()),
                                                rule.spec.listen_host,
                                                rule.bound_port
                                            )),
                                    )
                                    .child(
                                        components::icon_button(
                                            tokens,
                                            ("inspector-stop-forward", rule_id),
                                            IconName::Close,
                                            ButtonKind::Ghost,
                                            ButtonSize::Small,
                                            self.tr("Stop rule"),
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.remove_forward_rule(session_id, rule_id, cx);
                                            }),
                                        ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .text_size(px(ui_tokens::FONT_CAPTION))
                                    .text_color(tokens.text_secondary)
                                    .child(target)
                                    .child(format!("{} {}", rule.connections, self.tr("active"))),
                            ),
                    );
                }
                for (index, spec) in stopped_rules.iter().enumerate() {
                    let target = if spec.kind.needs_target() {
                        format!("{}:{}", spec.target_host, spec.target_port)
                    } else {
                        "SOCKS5".to_owned()
                    };
                    let listen_port = if spec.listen_port == 0 {
                        "auto".to_owned()
                    } else {
                        spec.listen_port.to_string()
                    };
                    group = group.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .p(px(ui_tokens::SPACE_2))
                            .rounded(px(ui_tokens::RADIUS_CONTROL))
                            .bg(tokens.surface)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(ui_tokens::SPACE_2))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .text_size(px(ui_tokens::FONT_CAPTION))
                                            .text_color(tokens.text_disabled)
                                            .child(format!(
                                                "{} {}:{}",
                                                self.tr(spec.kind.label()),
                                                spec.listen_host,
                                                listen_port
                                            )),
                                    )
                                    .child(
                                        components::icon_button(
                                            tokens,
                                            ("inspector-start-forward", index),
                                            IconName::ArrowRight,
                                            ButtonKind::Ghost,
                                            ButtonSize::Small,
                                            self.tr("Start rule"),
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.restart_forward_rule(session_id, index, cx);
                                            }),
                                        ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .text_size(px(ui_tokens::FONT_CAPTION))
                                    .text_color(tokens.text_secondary)
                                    .child(target)
                                    .child(self.tr("Stopped")),
                            ),
                    );
                }
            }
            if !forward_message.is_empty() {
                group = group.child(components::help_text(tokens, forward_message));
            }
            group = group.child(
                components::button(
                    tokens,
                    "inspector-tunnels",
                    if forwards_visible {
                        self.tr("Close tunnels")
                    } else {
                        self.tr("Manage tunnels")
                    },
                    ButtonKind::Secondary,
                    ButtonSize::Small,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_tunnels(session_id, cx);
                })),
            );
            body = body.child(group);
        }

        body = body.child(
            components::collapsible_header(
                tokens,
                "inspector-connection",
                self.tr("Connection"),
                self.inspector_info_open,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.inspector_info_open = !this.inspector_info_open;
                cx.notify();
            })),
        );
        if self.inspector_info_open {
            let auth = profile.as_ref().map_or_else(
                || self.tr("Quick connect"),
                |profile| self.tr(profile.auth_method.label()),
            );
            let profile_name = profile
                .as_ref()
                .map_or_else(|| "Unsaved".to_owned(), |profile| profile.name.clone());
            let jump = profile
                .as_ref()
                .map(|profile| profile.proxy_jump.clone())
                .filter(|jump| !jump.is_empty())
                .unwrap_or_else(|| self.tr("None"));
            let keepalive = profile
                .as_ref()
                .map(|profile| profile.keepalive_seconds)
                .unwrap_or(0);
            body = body
                .child(components::info_row(
                    tokens,
                    self.tr("Host"),
                    format!("{username}@{host}:{port}"),
                ))
                .child(components::info_row(
                    tokens,
                    self.tr("State"),
                    self.tr(session_state_label(state)),
                ))
                .child(components::info_row(
                    tokens,
                    self.tr("Terminal"),
                    format!("{} x {}", terminal_size.columns, terminal_size.rows),
                ))
                .child(components::info_row(tokens, self.tr("Auth"), auth))
                .child(components::info_row(
                    tokens,
                    self.tr("Host key"),
                    connection_info
                        .as_ref()
                        .map(|info| info.host_key_algorithm.clone())
                        .unwrap_or_else(|| self.tr("Pending")),
                ))
                .child(components::info_row(
                    tokens,
                    self.tr("Fingerprint"),
                    connection_info
                        .as_ref()
                        .map(|info| info.host_key_fingerprint.clone())
                        .unwrap_or_else(|| self.tr("Not reported")),
                ))
                .child(components::info_row(
                    tokens,
                    self.tr("Profile"),
                    profile_name,
                ))
                .child(components::info_row(tokens, self.tr("Jump host"), jump))
                .child(components::info_row(
                    tokens,
                    self.tr("Keep-alive"),
                    if keepalive == 0 {
                        self.tr("Off")
                    } else {
                        format!("{keepalive}s")
                    },
                ))
                .child(components::info_row(
                    tokens,
                    self.tr("Logging"),
                    if log_active {
                        self.tr("Recording")
                    } else {
                        self.tr("Off")
                    },
                ));
        }

        body = body.child(
            components::collapsible_header(
                tokens,
                "inspector-server",
                self.tr("Server health"),
                self.inspector_server_open,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.inspector_server_open = !this.inspector_server_open;
                cx.notify();
            })),
        );
        if self.inspector_server_open {
            let mut group = div()
                .flex()
                .flex_col()
                .gap(px(ui_tokens::SPACE_2))
                .p(px(ui_tokens::SPACE_3))
                .border_b_1()
                .border_color(tokens.border_subtle);
            let refresh_label = if server_metrics.loading {
                self.tr("Refreshing...")
            } else {
                self.tr("Refresh")
            };
            group = group.child(
                components::button(
                    tokens,
                    ("inspector-refresh-server", session_id),
                    refresh_label,
                    ButtonKind::Secondary,
                    ButtonSize::Small,
                )
                .disabled(server_metrics.loading || state != SessionState::Connected)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.refresh_server_metrics(session_id, cx);
                })),
            );
            let power_actions = div()
                .flex()
                .items_center()
                .gap(px(ui_tokens::SPACE_1))
                .child(
                    components::icon_button(
                        tokens,
                        ("server-reboot", session_id),
                        IconName::Replace,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        self.tr("Reboot server"),
                    )
                    .disabled(power_action_loading || state != SessionState::Connected)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.request_power_action(session_id, ServerPowerAction::Reboot, cx);
                    })),
                )
                .child(
                    components::icon_button(
                        tokens,
                        ("server-shutdown", session_id),
                        IconName::CircleX,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        self.tr("Shut down server"),
                    )
                    .disabled(power_action_loading || state != SessionState::Connected)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.request_power_action(session_id, ServerPowerAction::Shutdown, cx);
                    })),
                );
            group = group.child(power_actions);
            if power_action_loading || !power_action_message.is_empty() {
                group = group.child(components::help_text(
                    tokens,
                    if power_action_loading {
                        self.tr("Applying server power action...")
                    } else {
                        self.tr(&power_action_message)
                    },
                ));
            }
            if let Some(metrics) = server_metrics.metrics {
                group = group
                    .child(components::info_row(
                        tokens,
                        self.tr("Operating system"),
                        metrics.os,
                    ))
                    .child(components::info_row(
                        tokens,
                        self.tr("Kernel"),
                        metrics.kernel,
                    ))
                    .child(components::info_row(
                        tokens,
                        self.tr("Load average"),
                        match (metrics.load_1m, metrics.load_5m, metrics.load_15m) {
                            (Some(one), Some(five), Some(fifteen)) => {
                                format!("{one:.2} / {five:.2} / {fifteen:.2}")
                            }
                            _ => self.tr("Unavailable"),
                        },
                    ))
                    .child(components::info_row(
                        tokens,
                        self.tr("Memory"),
                        match (metrics.memory_total_kib, metrics.memory_available_kib) {
                            (Some(total), Some(available)) => format!(
                                "{} total · {} available · {}% used",
                                format_kib(Some(total)),
                                format_kib(Some(available)),
                                memory_usage_percent(Some(total), Some(available)).unwrap_or(0)
                            ),
                            _ => self.tr("Unavailable"),
                        },
                    ))
                    .child(components::info_row(
                        tokens,
                        self.tr("Root disk"),
                        match (
                            metrics.disk_total_kib,
                            metrics.disk_used_kib,
                            metrics.disk_available_kib,
                        ) {
                            (Some(total), Some(used), Some(available)) => format!(
                                "{} total · {} used · {} available · {}%",
                                format_kib(Some(total)),
                                format_kib(Some(used)),
                                format_kib(Some(available)),
                                metrics.disk_use_percent.unwrap_or(0)
                            ),
                            _ => self.tr("Unavailable"),
                        },
                    ))
                    .child(components::info_row(
                        tokens,
                        self.tr("Uptime"),
                        metrics
                            .uptime_seconds
                            .map(format_uptime)
                            .unwrap_or_else(|| self.tr("Unavailable")),
                    ));
                let memory_percent =
                    memory_usage_percent(metrics.memory_total_kib, metrics.memory_available_kib);
                let mut alerts = Vec::new();
                if memory_percent.is_some_and(|value| value >= alert_memory_percent) {
                    alerts.push(self.tr("Memory threshold exceeded."));
                }
                if metrics
                    .disk_use_percent
                    .is_some_and(|value| value >= alert_disk_percent)
                {
                    alerts.push(self.tr("Disk threshold exceeded."));
                }
                if let Some(max_cpu) = server_processes
                    .iter()
                    .map(|process| process.cpu_percent)
                    .reduce(f32::max)
                    && max_cpu >= f32::from(alert_cpu_percent)
                {
                    alerts.push(self.tr("CPU threshold exceeded."));
                }
                for alert in alerts {
                    group = group.child(components::warning_text(tokens, alert));
                }
                if !alert_history.is_empty() {
                    let clear_alerts = components::icon_button(
                        tokens,
                        ("server-alerts-clear", session_id),
                        IconName::Delete,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        self.tr("Clear alert history"),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.clear_server_alert_history(session_id, cx);
                    }));
                    let mut history_group = div()
                        .flex()
                        .flex_col()
                        .gap(px(ui_tokens::SPACE_1))
                        .p(px(ui_tokens::SPACE_2))
                        .rounded(px(ui_tokens::RADIUS_CONTROL))
                        .bg(tokens.surface)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(ui_tokens::FONT_CAPTION))
                                        .font_semibold()
                                        .text_color(tokens.text_primary)
                                        .child(self.tr("Alert history")),
                                )
                                .child(clear_alerts),
                        );
                    for record in alert_history.iter().rev().take(8) {
                        history_group = history_group.child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(ui_tokens::SPACE_2))
                                .text_size(px(ui_tokens::FONT_CAPTION))
                                .text_color(tokens.text_secondary)
                                .child(format!(
                                    "{} · {} ≥ {}",
                                    self.tr(record.kind.label()),
                                    record.value,
                                    format!("{}%", record.threshold)
                                ))
                                .child(format_alert_time(record.observed_at)),
                        );
                    }
                    group = group.child(history_group);
                }
            } else if server_metrics.loading {
                group = group.child(components::help_text(
                    tokens,
                    self.tr("Collecting server metrics..."),
                ));
            } else {
                group = group.child(components::help_text(
                    tokens,
                    if server_metrics.message.is_empty() {
                        self.tr("No server metrics collected yet.")
                    } else {
                        self.tr(&server_metrics.message)
                    },
                ));
            }
            body = body.child(group);
        }

        body = body.child(
            components::collapsible_header(
                tokens,
                "inspector-network",
                self.tr("Network"),
                self.inspector_network_open,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.inspector_network_open = !this.inspector_network_open;
                cx.notify();
            })),
        );
        if self.inspector_network_open {
            let mut group = div()
                .flex()
                .flex_col()
                .gap(px(ui_tokens::SPACE_2))
                .p(px(ui_tokens::SPACE_3))
                .border_b_1()
                .border_color(tokens.border_subtle);
            let refresh_label = if network_loading {
                self.tr("Refreshing...")
            } else {
                self.tr("Refresh")
            };
            group = group.child(
                components::button(
                    tokens,
                    ("inspector-refresh-network", session_id),
                    refresh_label,
                    ButtonKind::Secondary,
                    ButtonSize::Small,
                )
                .disabled(network_loading || state != SessionState::Connected)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.refresh_server_network(session_id, cx);
                })),
            );
            if network.is_empty() {
                group = group.child(components::help_text(
                    tokens,
                    if network_loading {
                        self.tr("Collecting network information...")
                    } else if network_message.is_empty() {
                        self.tr("No network interfaces reported yet.")
                    } else {
                        self.tr(&network_message)
                    },
                ));
            } else {
                for interface in network {
                    let status_tone = if interface.state.eq_ignore_ascii_case("up") {
                        Tone::Success
                    } else {
                        Tone::Warning
                    };
                    group = group.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(ui_tokens::SPACE_1))
                            .p(px(ui_tokens::SPACE_2))
                            .rounded(px(ui_tokens::RADIUS_CONTROL))
                            .bg(tokens.surface)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(ui_tokens::SPACE_2))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .text_size(px(ui_tokens::FONT_CAPTION))
                                            .text_color(tokens.text_primary)
                                            .child(interface.name),
                                    )
                                    .child(components::pill(tokens, interface.state, status_tone)),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_size(px(ui_tokens::FONT_CAPTION))
                                    .text_color(tokens.text_secondary)
                                    .child(if interface.addresses.is_empty() {
                                        self.tr("Addresses unavailable")
                                    } else {
                                        interface.addresses
                                    }),
                            ),
                    );
                }
            }
            body = body.child(group);
        }

        body = body.child(
            components::collapsible_header(
                tokens,
                "inspector-listeners",
                self.tr("Listening ports"),
                self.inspector_listeners_open,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.inspector_listeners_open = !this.inspector_listeners_open;
                cx.notify();
            })),
        );
        if self.inspector_listeners_open {
            let mut group = div()
                .flex()
                .flex_col()
                .gap(px(ui_tokens::SPACE_2))
                .p(px(ui_tokens::SPACE_3))
                .border_b_1()
                .border_color(tokens.border_subtle);
            let refresh_label = if listeners_loading {
                self.tr("Refreshing...")
            } else {
                self.tr("Refresh")
            };
            group = group.child(
                components::button(
                    tokens,
                    ("inspector-refresh-listeners", session_id),
                    refresh_label,
                    ButtonKind::Secondary,
                    ButtonSize::Small,
                )
                .disabled(listeners_loading || state != SessionState::Connected)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.refresh_server_listeners(session_id, cx);
                })),
            );
            if listeners.is_empty() {
                group = group.child(components::help_text(
                    tokens,
                    if listeners_loading {
                        self.tr("Collecting listening ports...")
                    } else if listeners_message.is_empty() {
                        self.tr("No listening ports reported yet.")
                    } else {
                        self.tr(&listeners_message)
                    },
                ));
            } else {
                for listener in listeners {
                    group = group.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(ui_tokens::SPACE_1))
                            .p(px(ui_tokens::SPACE_2))
                            .rounded(px(ui_tokens::RADIUS_CONTROL))
                            .bg(tokens.surface)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(ui_tokens::SPACE_2))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .text_size(px(ui_tokens::FONT_CAPTION))
                                            .text_color(tokens.text_primary)
                                            .child(listener.local_address),
                                    )
                                    .child(components::pill(
                                        tokens,
                                        listener.protocol,
                                        Tone::Neutral,
                                    )),
                            )
                            .child(components::help_text(tokens, listener.process)),
                    );
                }
            }
            body = body.child(group);
        }

        body = body.child(
            components::collapsible_header(
                tokens,
                "inspector-storage",
                self.tr("Storage"),
                self.inspector_storage_open,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.inspector_storage_open = !this.inspector_storage_open;
                cx.notify();
            })),
        );
        if self.inspector_storage_open {
            let mut group = div()
                .flex()
                .flex_col()
                .gap(px(ui_tokens::SPACE_2))
                .p(px(ui_tokens::SPACE_3))
                .border_b_1()
                .border_color(tokens.border_subtle);
            let refresh_label = if storage_loading {
                self.tr("Refreshing...")
            } else {
                self.tr("Refresh")
            };
            group = group.child(
                components::button(
                    tokens,
                    ("inspector-refresh-storage", session_id),
                    refresh_label,
                    ButtonKind::Secondary,
                    ButtonSize::Small,
                )
                .disabled(storage_loading || state != SessionState::Connected)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.refresh_server_storage(session_id, cx);
                })),
            );
            if storage.is_empty() {
                group = group.child(components::help_text(
                    tokens,
                    if storage_loading {
                        self.tr("Collecting storage information...")
                    } else if storage_message.is_empty() {
                        self.tr("No storage mounts reported yet.")
                    } else {
                        self.tr(&storage_message)
                    },
                ));
            } else {
                for mount in storage {
                    let tone = if mount.use_percent >= 90 {
                        Tone::Danger
                    } else if mount.use_percent >= 75 {
                        Tone::Warning
                    } else {
                        Tone::Success
                    };
                    group = group.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(ui_tokens::SPACE_1))
                            .p(px(ui_tokens::SPACE_2))
                            .rounded(px(ui_tokens::RADIUS_CONTROL))
                            .bg(tokens.surface)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(ui_tokens::SPACE_2))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .text_size(px(ui_tokens::FONT_CAPTION))
                                            .text_color(tokens.text_primary)
                                            .child(mount.mount_point),
                                    )
                                    .child(components::pill(
                                        tokens,
                                        format!("{}%", mount.use_percent),
                                        tone,
                                    )),
                            )
                            .child(components::help_text(
                                tokens,
                                format!(
                                    "{} · {}: {} / {} · {}: {}",
                                    mount.filesystem,
                                    self.tr("Used"),
                                    format_storage_kib(mount.used_kib),
                                    format_storage_kib(mount.total_kib),
                                    self.tr("Free"),
                                    format_storage_kib(mount.available_kib),
                                ),
                            )),
                    );
                }
            }
            body = body.child(group);
        }

        body = body.child(
            components::collapsible_header(
                tokens,
                "inspector-processes",
                self.tr("Processes"),
                self.inspector_processes_open,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.inspector_processes_open = !this.inspector_processes_open;
                cx.notify();
            })),
        );
        if self.inspector_processes_open {
            let mut group = div()
                .flex()
                .flex_col()
                .gap(px(ui_tokens::SPACE_2))
                .p(px(ui_tokens::SPACE_3))
                .border_b_1()
                .border_color(tokens.border_subtle);
            let refresh_label = if processes_loading {
                self.tr("Refreshing...")
            } else {
                self.tr("Refresh")
            };
            group = group.child(
                components::button(
                    tokens,
                    ("inspector-refresh-processes", session_id),
                    refresh_label,
                    ButtonKind::Secondary,
                    ButtonSize::Small,
                )
                .disabled(processes_loading || state != SessionState::Connected)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.refresh_server_processes(session_id, cx);
                })),
            );
            if server_processes.is_empty() {
                group = group.child(components::help_text(
                    tokens,
                    if processes_loading {
                        self.tr("Collecting process list...")
                    } else if processes_message.is_empty() {
                        self.tr("No processes collected yet.")
                    } else {
                        self.tr(&processes_message)
                    },
                ));
            } else {
                group = group.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(px(ui_tokens::FONT_CAPTION))
                        .text_color(tokens.text_secondary)
                        .child(format!(
                            "{} {}",
                            server_processes.len(),
                            self.tr("processes"),
                        ))
                        .child(self.tr("Top CPU")),
                );
                for (process_index, process) in server_processes.into_iter().enumerate() {
                    let pid = process.pid;
                    let terminate_button = components::icon_button(
                        tokens,
                        ("process-terminate", process_index),
                        IconName::CircleX,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        self.tr("Terminate process"),
                    )
                    .disabled(state != SessionState::Connected || processes_loading)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.request_process_termination(session_id, pid, cx);
                    }));
                    group = group.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(ui_tokens::SPACE_1))
                            .p(px(ui_tokens::SPACE_2))
                            .rounded(px(ui_tokens::RADIUS_CONTROL))
                            .bg(tokens.surface)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(ui_tokens::SPACE_2))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .text_size(px(ui_tokens::FONT_CAPTION))
                                            .text_color(tokens.text_primary)
                                            .child(format!(
                                                "{} · {} · {}",
                                                process.pid, process.user, process.state
                                            )),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(ui_tokens::SPACE_1))
                                            .child(
                                                div()
                                                    .flex_shrink_0()
                                                    .text_size(px(ui_tokens::FONT_CAPTION))
                                                    .text_color(tokens.text_secondary)
                                                    .child(format!(
                                                        "CPU {:.1}% · {} {:.1}%",
                                                        process.cpu_percent,
                                                        self.tr("Memory"),
                                                        process.memory_percent
                                                    )),
                                            )
                                            .child(terminate_button),
                                    ),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_size(px(ui_tokens::FONT_CAPTION))
                                    .text_color(tokens.text_secondary)
                                    .child(process.command),
                            ),
                    );
                }
            }
            body = body.child(group);
        }

        body = body.child(
            components::collapsible_header(
                tokens,
                "inspector-services",
                self.tr("Services"),
                self.inspector_services_open,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.inspector_services_open = !this.inspector_services_open;
                cx.notify();
            })),
        );
        if self.inspector_services_open {
            let mut group = div()
                .flex()
                .flex_col()
                .gap(px(ui_tokens::SPACE_2))
                .p(px(ui_tokens::SPACE_3))
                .border_b_1()
                .border_color(tokens.border_subtle);
            let refresh_label = if services_loading {
                self.tr("Refreshing...")
            } else {
                self.tr("Refresh")
            };
            group = group.child(
                components::button(
                    tokens,
                    ("inspector-refresh-services", session_id),
                    refresh_label,
                    ButtonKind::Secondary,
                    ButtonSize::Small,
                )
                .disabled(services_loading || state != SessionState::Connected)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.refresh_server_services(session_id, cx);
                })),
            );
            if server_services.is_empty() {
                group = group.child(components::help_text(
                    tokens,
                    if services_loading {
                        self.tr("Collecting service status...")
                    } else if services_message.is_empty() {
                        self.tr("No services reported yet.")
                    } else {
                        self.tr(&services_message)
                    },
                ));
            } else {
                for (service_index, service) in server_services.into_iter().enumerate() {
                    let status = format!("{} / {}", service.active, service.sub);
                    let service_name = service.name.clone();
                    let start_name = service.name.clone();
                    let stop_name = service.name.clone();
                    let restart_name = service.name.clone();
                    let logs_name = service.name.clone();
                    let service_actions = div()
                        .flex()
                        .items_center()
                        .gap(px(ui_tokens::SPACE_1))
                        .child(
                            components::icon_button(
                                tokens,
                                ("service-logs", service_index),
                                IconName::File,
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                                self.tr("View service logs"),
                            )
                            .disabled(service_logs_loading || state != SessionState::Connected)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.read_server_service_logs(
                                        session_id,
                                        logs_name.clone(),
                                        cx,
                                    );
                                },
                            )),
                        )
                        .child(
                            components::icon_button(
                                tokens,
                                ("service-start", service_index),
                                IconName::ArrowRight,
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                                self.tr("Start service"),
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.request_service_action(
                                        session_id,
                                        start_name.clone(),
                                        ServiceAction::Start,
                                        cx,
                                    );
                                },
                            )),
                        )
                        .child(
                            components::icon_button(
                                tokens,
                                ("service-stop", service_index),
                                IconName::CircleX,
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                                self.tr("Stop service"),
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.request_service_action(
                                        session_id,
                                        stop_name.clone(),
                                        ServiceAction::Stop,
                                        cx,
                                    );
                                },
                            )),
                        )
                        .child(
                            components::icon_button(
                                tokens,
                                ("service-restart", service_index),
                                IconName::Replace,
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                                self.tr("Restart service"),
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.request_service_action(
                                        session_id,
                                        restart_name.clone(),
                                        ServiceAction::Restart,
                                        cx,
                                    );
                                },
                            )),
                        );
                    group = group.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(ui_tokens::SPACE_1))
                            .p(px(ui_tokens::SPACE_2))
                            .rounded(px(ui_tokens::RADIUS_CONTROL))
                            .bg(tokens.surface)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(ui_tokens::SPACE_2))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .text_size(px(ui_tokens::FONT_CAPTION))
                                            .text_color(tokens.text_primary)
                                            .child(service_name),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(ui_tokens::SPACE_1))
                                            .child(components::pill(
                                                tokens,
                                                status,
                                                if service.active == "active" {
                                                    Tone::Success
                                                } else {
                                                    Tone::Warning
                                                },
                                            ))
                                            .child(service_actions),
                                    ),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_size(px(ui_tokens::FONT_CAPTION))
                                    .text_color(tokens.text_secondary)
                                    .child(service.description),
                            ),
                    );
                }
            }
            if service_logs_loading || !service_logs_message.is_empty() {
                group = group.child(components::help_text(
                    tokens,
                    if service_logs_loading {
                        self.tr("Reading service logs...")
                    } else {
                        self.tr(&service_logs_message)
                    },
                ));
            }
            if let Some(logs) = service_logs {
                let mut logs_group = div()
                    .flex()
                    .flex_col()
                    .gap(px(ui_tokens::SPACE_1))
                    .p(px(ui_tokens::SPACE_2))
                    .rounded(px(ui_tokens::RADIUS_CONTROL))
                    .bg(tokens.surface)
                    .child(
                        div()
                            .text_size(px(ui_tokens::FONT_CAPTION))
                            .font_semibold()
                            .text_color(tokens.text_primary)
                            .child(format!("{} · {}", self.tr("Service logs"), logs.name)),
                    );
                if logs.lines.is_empty() {
                    logs_group = logs_group.child(components::help_text(
                        tokens,
                        self.tr("No service log lines were returned."),
                    ));
                } else {
                    for line in logs.lines {
                        logs_group = logs_group.child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_size(px(ui_tokens::FONT_CAPTION))
                                .text_color(tokens.text_secondary)
                                .child(line),
                        );
                    }
                }
                group = group.child(logs_group);
            }
            body = body.child(group);
        }

        body = body.child(
            components::collapsible_header(
                tokens,
                "inspector-log",
                self.tr("Log"),
                self.inspector_log_open,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.inspector_log_open = !this.inspector_log_open;
                cx.notify();
            })),
        );
        if self.inspector_log_open {
            let mut group = div()
                .flex()
                .flex_col()
                .gap(px(ui_tokens::SPACE_2))
                .p(px(ui_tokens::SPACE_3))
                .border_b_1()
                .border_color(tokens.border_subtle)
                .child(components::info_row(
                    tokens,
                    self.tr("State"),
                    if log_active {
                        self.tr("Recording")
                    } else {
                        self.tr("Off")
                    },
                ));
            if let Some(path) = &log_path {
                group = group.child(components::info_row(
                    tokens,
                    self.tr("File"),
                    path.display().to_string(),
                ));
            }
            group = group
                .child(components::help_text(
                    tokens,
                    self.tr("New logs use timestamped records with a replay timeline. Appending to an existing legacy raw log keeps the raw format. Logs may contain sensitive terminal output."),
                ))
                .child(
                    components::button(
                        tokens,
                        "inspector-toggle-log",
                        if log_active {
                            self.tr("Stop logging")
                        } else {
                            self.tr("Start logging")
                        },
                        ButtonKind::Secondary,
                        ButtonSize::Small,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_log(session_id, cx);
                    })),
                )
                .child(
                    components::button(
                        tokens,
                        "inspector-open-replay",
                        self.tr("Open replay..."),
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.begin_log_replay(cx))),
                );
            body = body.child(group);
        }

        components::panel(tokens)
            .id("inspector")
            .w(px(self.settings.inspector_width))
            .ml(px(ui_tokens::SPACE_2))
            .border_1()
            .border_color(tokens.border_subtle)
            .rounded(px(ui_tokens::RADIUS_PANEL))
            .overflow_hidden()
            .child(
                components::panel_header(tokens, self.tr("Inspector")).child(
                    components::icon_button(
                        tokens,
                        "close-inspector",
                        IconName::Close,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        self.tr("Close inspector"),
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_inspector(window, cx))),
                ),
            )
            .child(body)
            .into_any_element()
    }
}

fn terminal_mouse_button(button: MouseButton) -> Option<TerminalMouseButton> {
    match button {
        MouseButton::Left => Some(TerminalMouseButton::Left),
        MouseButton::Middle => Some(TerminalMouseButton::Middle),
        MouseButton::Right => Some(TerminalMouseButton::Right),
        MouseButton::Navigate(_) => None,
    }
}

fn terminal_mouse_modifiers(shift: bool, alt: bool, control: bool) -> TerminalMouseModifiers {
    TerminalMouseModifiers {
        shift,
        alt,
        control,
    }
}

impl Focusable for AppView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.terminal_focus.clone()
    }
}

fn render_terminal_grid(
    frame: &TerminalFrame,
    settings: &AppSettings,
    palette: TerminalPalette,
    keyword_highlights: &[String],
) -> AnyElement {
    let mut grid = div().flex().flex_col().flex_1().min_h_0();
    for (row_index, row) in frame.rows.iter().enumerate() {
        let mut rendered_row = div()
            .flex()
            .relative()
            .h(px(settings.cell_height()))
            .flex_shrink_0();
        for run in row {
            rendered_row = rendered_row.child(render_terminal_run(
                run,
                settings,
                palette,
                keyword_matches(&run.text, keyword_highlights),
            ));
        }
        if let Some(cursor) = frame.cursor
            && cursor.row as usize == row_index
            && cursor.style != CursorStyle::Block
        {
            let left = px(cursor.column as f32 * settings.cell_width());
            let cursor_mark = match cursor.style {
                CursorStyle::Underline => div()
                    .absolute()
                    .left(left)
                    .top(px(settings.cell_height() - 2.))
                    .w(px(settings.cell_width()))
                    .h(px(2.))
                    .bg(rgb(frame.foreground)),
                CursorStyle::Bar => div()
                    .absolute()
                    .left(left)
                    .top(px(0.))
                    .w(px(2.))
                    .h(px(settings.cell_height()))
                    .bg(rgb(frame.foreground)),
                CursorStyle::Block => div(),
            };
            rendered_row = rendered_row.child(cursor_mark);
        }
        grid = grid.child(rendered_row);
    }
    grid.into_any_element()
}

fn render_terminal_run(
    run: &TerminalRun,
    settings: &AppSettings,
    palette: TerminalPalette,
    keyword_highlighted: bool,
) -> impl IntoElement {
    let mut cell_run = div()
        .flex_shrink_0()
        .w(px(settings.cell_width() * run.width_cells as f32))
        .h(px(settings.cell_height()))
        .font_family(settings.font_family.clone())
        .text_size(px(settings.font_size))
        .text_color(rgb(run.foreground))
        .bg(rgb(run.background));
    if keyword_highlighted && !run.selected {
        cell_run = cell_run.bg(rgb(palette.selection_background));
    }
    if run.selected {
        cell_run = cell_run
            .text_color(rgb(palette.selection_foreground))
            .bg(rgb(palette.selection_background));
    }
    if run.bold {
        cell_run = cell_run.font_weight(FontWeight::BOLD);
    }
    if run.italic {
        cell_run = cell_run.italic();
    }
    if run.underline {
        cell_run = cell_run.underline();
    }
    cell_run.child(run.text.clone())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionEditorKey {
    Cancel,
    Save,
}

fn session_editor_key(key: &str, control: bool, platform: bool) -> Option<SessionEditorKey> {
    match key.to_ascii_lowercase().as_str() {
        "escape" => Some(SessionEditorKey::Cancel),
        "enter" if control || platform => Some(SessionEditorKey::Save),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionEditorEscape {
    CancelDelete,
    CloseEditor,
}

fn session_editor_escape(confirm_delete: bool) -> SessionEditorEscape {
    if confirm_delete {
        SessionEditorEscape::CancelDelete
    } else {
        SessionEditorEscape::CloseEditor
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppShortcut {
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
    SelectSession(usize),
}

fn search_result_message(result: &TerminalSearchResult) -> String {
    if result.found {
        let wrapped = if result.wrapped { " (wrapped)" } else { "" };
        format!("{}/{} matches{wrapped}", result.current + 1, result.total)
    } else {
        "No matches".to_owned()
    }
}

fn app_shortcut(
    settings: &ShortcutSettings,
    key: &str,
    control: bool,
    shift: bool,
    alt: bool,
) -> Option<AppShortcut> {
    for action in ShortcutAction::ALL {
        if KeyShortcut::parse(settings.shortcut(action))
            .is_some_and(|shortcut| shortcut.matches(key, control, shift, alt))
        {
            return Some(match action {
                ShortcutAction::NewSession => AppShortcut::NewSession,
                ShortcutAction::CloseSession => AppShortcut::CloseSession,
                ShortcutAction::ToggleFiles => AppShortcut::ToggleFiles,
                ShortcutAction::ToggleSnippets => AppShortcut::ToggleSnippets,
                ShortcutAction::ToggleSettings => AppShortcut::ToggleSettings,
                ShortcutAction::ToggleSidebar => AppShortcut::ToggleSidebar,
                ShortcutAction::ToggleCompose => AppShortcut::ToggleCompose,
                ShortcutAction::Disconnect => AppShortcut::Disconnect,
                ShortcutAction::ReplayLog => AppShortcut::ReplayLog,
                ShortcutAction::Find => AppShortcut::Find,
            });
        }
    }
    if alt && !control && !shift {
        if let Ok(index) = key.parse::<usize>()
            && (1..=9).contains(&index)
        {
            return Some(AppShortcut::SelectSession(index - 1));
        }
    }
    None
}

fn is_sftp_transfer_command(command: &SftpCommand) -> bool {
    matches!(
        command,
        SftpCommand::Upload { .. }
            | SftpCommand::Download { .. }
            | SftpCommand::UploadDirectory { .. }
            | SftpCommand::DownloadDirectory { .. }
    )
}

fn take_next_sftp_transfer(
    active: bool,
    waiting_for_overwrite: bool,
    queue: &mut VecDeque<SftpCommand>,
) -> Option<SftpCommand> {
    if active || waiting_for_overwrite {
        return None;
    }
    queue.pop_front()
}

fn sftp_transfer_command(prompt: OverwritePrompt, overwrite: bool, resume: bool) -> SftpCommand {
    match (prompt.kind, prompt.is_directory) {
        (TransferKind::Upload, false) => SftpCommand::Upload {
            local: prompt.local,
            remote: prompt.remote,
            overwrite,
            resume,
        },
        (TransferKind::Upload, true) => SftpCommand::UploadDirectory {
            local: prompt.local,
            remote: prompt.remote,
            overwrite,
            resume,
        },
        (TransferKind::Download, false) => SftpCommand::Download {
            remote: prompt.remote,
            local: prompt.local,
            overwrite,
            resume,
        },
        (TransferKind::Download, true) => SftpCommand::DownloadDirectory {
            remote: prompt.remote,
            local: prompt.local,
            overwrite,
            resume,
        },
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.auth_prompt_needs_clear {
            self.clear_auth_prompt_inputs(window, cx);
            self.auth_prompt_needs_clear = false;
        }
        if std::mem::take(&mut self.overlay_focus_pending) {
            window.focus(&self.overlay_focus);
        }
        if std::mem::take(&mut self.credential_focus_pending)
            && (self.session_editor_open || self.credential_prompt_open)
        {
            let focus = if self.auth_method == AuthMethod::PublicKey {
                self.key_passphrase.read(cx).focus_handle(cx)
            } else {
                self.password.read(cx).focus_handle(cx)
            };
            window.focus(&focus);
        }
        let tokens = self.tokens();
        let command_bar = self.command_bar(&tokens, cx);
        let sidebar = self.sidebar(&tokens, cx);
        let tab_bar = self.session_tabs(&tokens, cx);
        let status_bar = self.status_bar(&tokens);

        let mut terminal_workspace = div()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(tokens.bg_canvas)
            .p(px(ui_tokens::SPACE_2));
        if let Some(split) = self.split {
            let first_panel = self.terminal_panel(split.first_session_id, cx);
            let second_panel = self.terminal_panel(split.second_session_id, cx);
            let panel_minimum = px(240.)..Pixels::MAX;
            let group = match split.orientation {
                SplitOrientation::SideBySide => h_resizable(
                    "terminal-split-side-by-side",
                    self.side_by_side_state.clone(),
                )
                .child(
                    resizable_panel()
                        .size_range(panel_minimum.clone())
                        .child(first_panel),
                )
                .child(
                    resizable_panel()
                        .size_range(panel_minimum)
                        .child(second_panel),
                ),
                SplitOrientation::Stacked => {
                    v_resizable("terminal-split-stacked", self.stacked_state.clone())
                        .child(
                            resizable_panel()
                                .size_range(panel_minimum.clone())
                                .child(first_panel),
                        )
                        .child(
                            resizable_panel()
                                .size_range(panel_minimum)
                                .child(second_panel),
                        )
                }
            };
            terminal_workspace = terminal_workspace.child(group);
        } else if let Some(session_id) = self.active_session_id {
            terminal_workspace = terminal_workspace.child(self.terminal_panel(session_id, cx));
        } else {
            terminal_workspace = terminal_workspace.child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .border_1()
                    .border_color(tokens.border_subtle)
                    .rounded(px(ui_tokens::RADIUS_PANEL))
                    .overflow_hidden()
                    .bg(tokens.surface_raised)
                    .child(components::empty_state(
                        &tokens,
                        self.tr("No active terminal"),
                        self.tr("Create a session or open a saved host from the sidebar."),
                    )),
            );
        }
        if self.inspector_visible
            && let Some(session_id) = self.active_session_id
        {
            terminal_workspace = terminal_workspace
                .child(self.layout_divider(
                    "inspector-resize",
                    LayoutDragTarget::Inspector,
                    &tokens,
                    cx,
                ))
                .child(self.inspector(session_id, &tokens, cx));
        }
        if let Some(session_id) = self.active_session_id
            && self
                .session(session_id)
                .is_some_and(|session| session.sftp.visible)
        {
            terminal_workspace = terminal_workspace.child(self.sftp_panel(session_id, cx));
        }
        if let Some(session_id) = self.active_session_id
            && self
                .session(session_id)
                .is_some_and(|session| session.forwards.visible)
        {
            terminal_workspace = terminal_workspace.child(self.forward_panel(session_id, cx));
        }
        if self.snippets_visible {
            terminal_workspace = terminal_workspace.child(self.snippets_panel(cx));
        }
        if self.keywords_visible {
            terminal_workspace = terminal_workspace.child(self.keywords_panel(cx));
        }
        if self.triggers_visible {
            terminal_workspace = terminal_workspace.child(self.triggers_panel(cx));
        }

        let mut content_row = div().flex().flex_1().min_h_0().min_w_0().child(sidebar);
        if !self.sidebar_collapsed {
            content_row = content_row.child(self.layout_divider(
                "sidebar-resize",
                LayoutDragTarget::Sidebar,
                &tokens,
                cx,
            ));
        }
        let mut content_column = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(tab_bar)
            .child(terminal_workspace);
        if self.compose_visible
            && let Some(session_id) = self.active_session_id
        {
            content_column = content_column.child(self.compose_bar(session_id, &tokens, cx));
        }
        content_row = content_row.child(content_column);

        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(tokens.bg_canvas)
            .text_color(tokens.text_primary)
            .child(command_bar)
            .child(content_row)
            .child(status_bar);
        if self.layout_drag.is_some() {
            root = root.child(
                div()
                    .id("layout-drag-overlay")
                    .absolute()
                    .size_full()
                    .occlude()
                    .cursor_col_resize()
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        this.update_layout_drag(event.position.x.as_f32(), cx);
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.finish_layout_drag(window, cx)),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.finish_layout_drag(window, cx)),
                    ),
            );
        }
        if self.menu_visible {
            root = root.child(self.main_menu_overlay(&tokens, cx));
        }
        if self.layout_menu_visible {
            root = root.child(self.layout_menu_overlay(&tokens, cx));
        }
        if self.tab_menu_session.is_some() {
            root = root.child(self.tab_menu_overlay(&tokens, cx));
        }
        if self.sync_visible {
            root = root.child(self.sync_overlay(&tokens, cx));
        }
        if self.keys_visible {
            root = root.child(self.keys_panel(cx));
        }
        if self.keys_visible && self.key_change_visible {
            root = root.child(self.key_change_panel(cx));
        }
        if self.keys_visible && self.key_delete_visible {
            root = root.child(self.key_delete_panel(cx));
        }
        if self.settings_visible {
            root = root.child(self.settings_panel(cx));
        }
        if self.session_editor_open {
            root = root.child(self.session_editor(cx));
        }
        if self.credential_prompt_open {
            root = root.child(self.credential_prompt_overlay(cx));
        }
        if self.pending_service_action.is_some() {
            root = root.child(self.service_action_overlay(cx));
        }
        if self.pending_power_action.is_some() {
            root = root.child(self.power_action_overlay(cx));
        }
        if self.pending_process_termination.is_some() {
            root = root.child(self.process_termination_overlay(cx));
        }
        if self.group_editor_open {
            root = root.child(self.group_editor(cx));
        }
        if self.auth_prompt.is_some() {
            root = root.child(self.auth_prompt_overlay(cx));
        }
        if self.log_replay.is_some() {
            root = root.child(self.log_replay_overlay(cx));
        }
        if let Some(toast) = self.toast.clone() {
            root = root.child(self.toast_overlay(toast, cx));
        }
        root
    }
}

fn main() {
    Application::new().with_assets(AppAssets).run(|cx| {
        gpui_component::init(cx);
        ui::theme::install(cx, UiTheme::DEFAULT);
        cx.open_window(WindowOptions::default(), |window, cx| {
            let view = cx.new(|cx| AppView::new(window, cx));
            cx.new(|cx| Root::new(view.into(), window, cx))
        })
        .expect("failed to open dshell window");
        cx.activate(true);
    });
}
