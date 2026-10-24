mod forward;
mod keygen;
mod openssh_config;
mod profiles;
mod settings;
mod sftp;
mod shortcuts;
mod snippets;
mod ssh;
mod terminal;
mod ui;

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
    io::{Read as _, Seek as _, SeekFrom},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use forward::{ForwardCommand, ForwardEvent, ForwardKind, ForwardRuleSpec, ForwardRuleUiState};
use gpui::{AnyElement, StatefulInteractiveElement as _, prelude::*, *};
use gpui_component::{
    IconName, PixelsExt, Root, StyledExt,
    button::{Button, ButtonVariants},
    input::{InputState, TextInput},
    resizable::{ResizableState, h_resizable, resizable_panel, v_resizable},
};
use openssh_config::import_openssh_config;
use profiles::{
    AuthMethod, HostData, HostProfile, MAX_FORWARD_RULES, ProfileStore, RecentConnection,
    record_recent,
};
use settings::{AppSettings, DEFAULT_FONT_FAMILY, MAX_FONT_SIZE, MIN_FONT_SIZE, SettingsStore};
use sftp::{RemoteEntry, SftpCommand, SftpEvent, TransferKind, parent_remote_path, remote_join};
use shortcuts::{KeyShortcut, ShortcutAction, ShortcutSettings};
use snippets::{CommandSnippet, MAX_SNIPPETS, SnippetStore};
use ssh::{AuthPromptInfo, Authentication, ConnectionSpec, JumpSpec, UiEvent};
use terminal::{
    CursorStyle, MouseTrackingMode, SearchDirection, TerminalEmulator, TerminalFrame,
    TerminalKeyInput, TerminalMouseButton, TerminalMouseEvent, TerminalMouseModifiers,
    TerminalPosition, TerminalRequest, TerminalRun, TerminalSearchResult, TerminalSize,
    TerminalTheme,
};
use tokio::sync::{mpsc, oneshot, watch};
use ui::{
    components::{self, ButtonKind, ButtonSize, Tone},
    theme::{ThemeTokens, UiTheme},
    tokens as ui_tokens,
};
use zeroize::Zeroizing;

const TERMINAL_PANEL_HEADER_PX: f32 = ui_tokens::PANEL_HEADER_HEIGHT;
const TERMINAL_CARD_BORDER_PX: f32 = 2.0;
const MAX_AUTH_PROMPTS: usize = 8;
const MAX_LOG_REPLAY_BYTES: u64 = 16 * 1024 * 1024;

fn sidebar_width(collapsed: bool) -> f32 {
    if collapsed {
        ui_tokens::SIDEBAR_COLLAPSED_WIDTH
    } else {
        ui_tokens::SIDEBAR_WIDTH
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
        ui_tokens::INSPECTOR_WIDTH + ui_tokens::SPACE_2
    } else {
        0.0
    };
    let width = width
        - sidebar_width(sidebar_collapsed)
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

struct LogReplayState {
    path: PathBuf,
    terminal: TerminalEmulator,
    frame: TerminalFrame,
    message: String,
    scroll_remainder: f32,
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
enum SessionEditorSection {
    Connection,
    Authentication,
    Security,
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

fn sort_profiles(profiles: &mut [HostProfile]) {
    profiles.sort_by(|left, right| {
        left.group
            .to_lowercase()
            .cmp(&right.group.to_lowercase())
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
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
    state: SessionState,
}

#[derive(Clone, Debug)]
struct TransferProgressUi {
    kind: TransferKind,
    name: String,
    transferred: u64,
    total: u64,
}

#[derive(Clone, Debug)]
struct OverwritePrompt {
    kind: TransferKind,
    local: PathBuf,
    remote: String,
    is_directory: bool,
}

#[derive(Default)]
struct SftpViewState {
    visible: bool,
    path: String,
    entries: Vec<RemoteEntry>,
    message: String,
    transfer: Option<TransferProgressUi>,
    overwrite: Option<OverwritePrompt>,
    rename_from: Option<String>,
    pending_delete: Option<(String, bool)>,
}

#[derive(Default)]
struct ForwardViewState {
    visible: bool,
    rules: Vec<(u64, ForwardRuleUiState)>,
    message: String,
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
                self.sftp.overwrite = None;
                self.forward_tx = None;
                self.forwards.rules.clear();
                self.log_path = None;
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
                self.sftp.overwrite = None;
                self.forward_tx = None;
                self.forwards.rules.clear();
                self.log_path = None;
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
        AppShortcut, ForwardViewState, MouseTrackingMode, SessionEditorEscape, SessionEditorKey,
        SessionSplit, SessionState, SftpViewState, SplitOrientation, SshSession, TerminalFrame,
        TerminalRequest, TerminalSize, UiEvent, app_shortcut, apply_session_event,
        enqueue_session_request, mpsc, read_log_for_replay, remove_session, search_result_message,
        session_editor_escape, session_editor_key, session_terminal_size_for_split,
        split_panel_terminal_size, terminal_size_for_viewport, watch,
    };
    use crate::forward::{ForwardCommand, ForwardKind, ForwardRuleSpec};
    use crate::settings::AppSettings;
    use crate::shortcuts::{ShortcutAction, ShortcutSettings};
    use crate::terminal::{CELL_HEIGHT_PX, CELL_WIDTH_PX};

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
            read_log_for_replay(&path, 4).unwrap(),
            (b"6789".to_vec(), true)
        );
        assert_eq!(
            read_log_for_replay(&path, 100).unwrap(),
            (b"0123456789".to_vec(), false)
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
    profile_name: Entity<InputState>,
    profile_search: Entity<InputState>,
    profile_group: Entity<InputState>,
    profile_favorite: bool,
    auth_method: AuthMethod,
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
    side_by_side_state: Entity<ResizableState>,
    stacked_state: Entity<ResizableState>,
    profiles: Vec<HostProfile>,
    recent: Vec<RecentConnection>,
    profile_store: Option<ProfileStore>,
    profiles_ready: bool,
    selected_profile: Option<String>,
    session_editor_open: bool,
    session_editor_section: SessionEditorSection,
    confirm_delete: bool,
    profile_message: String,
    snippets: Vec<CommandSnippet>,
    snippet_store: Option<SnippetStore>,
    snippet_name: Entity<InputState>,
    snippet_command: Entity<InputState>,
    snippet_send_enter: bool,
    editing_snippet: Option<String>,
    snippet_message: String,
    snippets_visible: bool,
    snippet_targets: HashSet<u64>,
    settings: AppSettings,
    settings_store: Option<SettingsStore>,
    settings_font_family: Entity<InputState>,
    settings_font_size: Entity<InputState>,
    settings_theme: TerminalTheme,
    settings_cursor: CursorStyle,
    shortcut_inputs: Vec<Entity<InputState>>,
    settings_message: String,
    settings_visible: bool,
    keys_visible: bool,
    key_path: Entity<InputState>,
    new_key_passphrase: Entity<InputState>,
    key_comment: Entity<InputState>,
    key_message: String,
    key_public: Option<String>,
    auth_prompt: Option<AuthPromptState>,
    auth_prompt_inputs: Vec<Entity<InputState>>,
    auth_prompt_plain_inputs: Vec<Entity<InputState>>,
    auth_prompt_needs_clear: bool,
    log_replay: Option<LogReplayState>,
    search_input: Entity<InputState>,
    search_visible: bool,
    search_message: String,
    search_target: Option<SearchTarget>,
    _bounds_subscription: Subscription,
    _side_by_side_subscription: Subscription,
    _stacked_subscription: Subscription,
}

impl AppView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let profile_name = cx.new(|cx| InputState::new(window, cx).placeholder("Production"));
        let profile_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search saved hosts"));
        let profile_group = cx.new(|cx| InputState::new(window, cx).placeholder("Ungrouped"));
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
        let snippet_name = cx.new(|cx| InputState::new(window, cx).placeholder("Snippet name"));
        let snippet_command = cx.new(|cx| InputState::new(window, cx).placeholder("uptime"));
        let (settings_store, settings, settings_message) = match SettingsStore::discover() {
            Ok(store) => match store.load() {
                Ok(settings) => (Some(store), settings, String::new()),
                Err(error) => (Some(store), AppSettings::default(), error),
            },
            Err(error) => (None, AppSettings::default(), error),
        };
        ui::theme::install(cx, settings.ui_theme);
        let settings_font_family =
            cx.new(|cx| InputState::new(window, cx).default_value(settings.font_family.clone()));
        let settings_font_size =
            cx.new(|cx| InputState::new(window, cx).default_value(settings.font_size.to_string()));
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

        Self {
            ui_theme: settings.ui_theme,
            sidebar_collapsed: settings.sidebar_collapsed,
            inspector_visible: false,
            inspector_transfers_open: true,
            inspector_forwards_open: true,
            inspector_info_open: true,
            profile_name,
            profile_search,
            profile_group,
            profile_favorite: false,
            auth_method: AuthMethod::Password,
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
            side_by_side_state,
            stacked_state,
            profiles: data.profiles,
            recent: data.recent,
            profile_store,
            profiles_ready,
            selected_profile: None,
            session_editor_open: false,
            session_editor_section: SessionEditorSection::Connection,
            confirm_delete: false,
            profile_message,
            snippets,
            snippet_store,
            snippet_name,
            snippet_command,
            snippet_send_enter: true,
            editing_snippet: None,
            snippet_message,
            snippets_visible: false,
            snippet_targets: HashSet::new(),
            settings_theme: settings.theme,
            settings_cursor: settings.cursor_style,
            shortcut_inputs,
            settings,
            settings_store,
            settings_font_family,
            settings_font_size,
            settings_message,
            settings_visible: false,
            keys_visible: false,
            key_path,
            new_key_passphrase,
            key_comment,
            key_message: String::new(),
            key_public: None,
            auth_prompt: None,
            auth_prompt_inputs,
            auth_prompt_plain_inputs,
            auth_prompt_needs_clear: false,
            log_replay: None,
            search_input,
            search_visible: false,
            search_message: String::new(),
            search_target: None,
            _bounds_subscription: bounds_subscription,
            _side_by_side_subscription: side_by_side_subscription,
            _stacked_subscription: stacked_subscription,
        }
    }

    fn tokens(&self) -> ThemeTokens {
        self.ui_theme.tokens()
    }

    fn request_overlay_focus(&mut self) {
        self.overlay_focus_pending = true;
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
                    .background_spawn(async move {
                        read_log_for_replay(&read_path, MAX_LOG_REPLAY_BYTES)
                    })
                    .await;
                let _ = this.update(cx, |view, cx| view.apply_log_replay(path, read, cx));
            }
        })
        .detach();
    }

    fn apply_log_replay(
        &mut self,
        path: PathBuf,
        read: Result<(Vec<u8>, bool), String>,
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
        let message = match read {
            Ok((bytes, truncated)) => {
                terminal.feed(&bytes);
                if truncated {
                    format!(
                        "{} bytes replayed from the end of the file (limit {} MiB).",
                        bytes.len(),
                        MAX_LOG_REPLAY_BYTES / (1024 * 1024)
                    )
                } else {
                    format!("{} bytes replayed.", bytes.len())
                }
            }
            Err(error) => format!("Could not read the log: {error}"),
        };
        let frame = terminal.frame();
        self.log_replay = Some(LogReplayState {
            path,
            terminal,
            frame,
            message,
            scroll_remainder: 0.0,
        });
        self.request_overlay_focus();
        cx.notify();
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
            "Log replay: {}",
            replay.path.file_name().map_or_else(
                || replay.path.display().to_string(),
                |name| name.to_string_lossy().into_owned()
            )
        );
        let message = replay.message.clone();
        let frame = replay.frame.clone();
        let palette = self.settings.theme.palette();
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
                            .child(render_terminal_grid(&frame, &self.settings)),
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
        let mut names = profiles
            .iter()
            .map(|profile| profile.name.to_lowercase())
            .collect::<HashSet<_>>();
        let mut added = 0;
        let mut skipped = 0;
        for profile in imported.profiles {
            if names.insert(profile.name.to_lowercase()) {
                profiles.push(profile);
                added += 1;
            } else {
                skipped += 1;
            }
        }
        if added > 0 {
            sort_profiles(&mut profiles);
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
        self.selected_profile = None;
        self.session_editor_section = SessionEditorSection::Connection;
        self.session_editor_open = true;
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
        Self::set_input_value(&self.password, "", window, cx);
        Self::set_input_value(&self.key_passphrase, "", window, cx);
        Self::set_input_value(&self.identity_file, &profile.identity_file, window, cx);
        Self::set_input_value(&self.jump_profile, &profile.proxy_jump, window, cx);
        Self::set_input_value(&self.jump_password, "", window, cx);
        Self::set_input_value(&self.jump_passphrase, "", window, cx);
        Self::set_input_value(
            &self.keepalive_seconds,
            &profile.keepalive_seconds.to_string(),
            window,
            cx,
        );
        self.profile_favorite = profile.favorite;
        self.auth_method = profile.auth_method;
        self.selected_profile = Some(profile.name.clone());
        self.session_editor_section = SessionEditorSection::Connection;
        self.session_editor_open = true;
        self.request_overlay_focus();
        self.confirm_delete = false;
        self.profile_message =
            format!("Loaded {}. Enter the credentials to connect.", profile.name);
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
            self.profile_message = format!("Loaded {} from recent connections.", profile.name);
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
        self.selected_profile = None;
        self.session_editor_section = SessionEditorSection::Connection;
        self.session_editor_open = true;
        self.request_overlay_focus();
        self.confirm_delete = false;
        self.profile_message =
            "Loaded a recent connection. Save it as a host profile if you want to keep it."
                .to_owned();
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
                    format!("Added {name} to favorites.")
                } else {
                    format!("Removed {name} from favorites.")
                };
            }
            Err(error) => self.profile_message = error,
        }
        cx.notify();
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
                "Host profiles could not be loaded; fix the profile file before saving.".to_owned();
            cx.notify();
            return;
        }

        if self.profile_store.is_none() {
            self.profile_message = "Host profile storage is unavailable.".to_owned();
            cx.notify();
            return;
        }
        let name = self.profile_name.read(cx).value().trim().to_owned();
        let group = self.profile_group.read(cx).value().trim().to_owned();
        let host = self.host.read(cx).value().trim().to_owned();
        let port = self.port.read(cx).value().parse::<u16>();
        let username = self.username.read(cx).value().trim().to_owned();
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
                    "Keep-alive must be between 0 and 3600 seconds (0 disables it).".to_owned();
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
            self.profile_message = "Port must be between 1 and 65535.".to_owned();
            cx.notify();
            return;
        };
        if name.is_empty() || host.is_empty() || username.is_empty() || port == 0 {
            self.profile_message =
                "Profile name, host, username, and a valid port are required.".to_owned();
            cx.notify();
            return;
        }
        if self.auth_method == AuthMethod::PublicKey && identity_file.is_empty() {
            self.profile_message =
                "Public key authentication requires an identity file path.".to_owned();
            cx.notify();
            return;
        }
        if !proxy_jump.is_empty() {
            if proxy_jump.eq_ignore_ascii_case(&name) {
                self.profile_message = "A host cannot use itself as its jump host.".to_owned();
                cx.notify();
                return;
            }
            if !self.profiles.iter().any(|profile| {
                profile.name.eq_ignore_ascii_case(&proxy_jump)
                    && !profile.name.eq_ignore_ascii_case(&name)
            }) {
                self.profile_message = format!("Jump host {proxy_jump} is not another saved host.");
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
            self.profile_message = format!("A host named {name} already exists.");
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
        self.profile_message = format!("Saved {name}.");
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
            self.profile_message = "Host profile storage is unavailable.".to_owned();
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

        self.new_profile(window, cx);
        self.session_editor_open = false;
        self.overlay_focus_pending = false;
        window.focus(&self.terminal_focus);
        self.profile_message = format!("Deleted {selected}.");
        self.show_toast(self.profile_message.clone(), Tone::Success, cx);
    }

    fn cancel_session_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        Self::set_input_value(&self.password, "", window, cx);
        Self::set_input_value(&self.key_passphrase, "", window, cx);
        Self::set_input_value(&self.jump_password, "", window, cx);
        Self::set_input_value(&self.jump_passphrase, "", window, cx);
        self.confirm_delete = false;
        self.session_editor_open = false;
        self.overlay_focus_pending = false;
        window.focus(&self.terminal_focus);
        cx.notify();
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
        Self::set_input_value(&self.password, "", window, cx);
        Self::set_input_value(&self.key_passphrase, "", window, cx);
        Self::set_input_value(&self.jump_password, "", window, cx);
        Self::set_input_value(&self.jump_passphrase, "", window, cx);

        let port = match port {
            Ok(port) if port > 0 => port,
            _ => {
                self.profile_message = "Port must be between 1 and 65535.".to_owned();
                cx.notify();
                return;
            }
        };
        if host.trim().is_empty() || username.trim().is_empty() {
            self.profile_message = "Host and user are required.".to_owned();
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
                    "Keep-alive must be between 0 and 3600 seconds (0 disables it).".to_owned();
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
                    self.profile_message = error;
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
                self.profile_message = format!("Jump host {jump_name} is not a saved host.");
                cx.notify();
                return;
            };
            if !profile.proxy_jump.trim().is_empty() {
                self.profile_message = format!(
                    "Jump host {} already uses jump host {}. Nested jump hosts are not supported yet.",
                    profile.name, profile.proxy_jump
                );
                cx.notify();
                return;
            }
            if profile.host.eq_ignore_ascii_case(&host)
                && profile.port == port
                && profile.username == username
            {
                self.profile_message =
                    "The jump host resolves to the same target; choose a different jump host."
                        .to_owned();
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
                    self.profile_message = format!("Jump host {}: {error}", profile.name);
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
        let status = format!("Checking host keys and connecting to {host}:{port}{route}...");
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
                    terminal_message: "Connecting...".to_owned(),
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
                    state: SessionState::Connecting,
                });
                self.split = None;
                self.terminal_bounds.borrow_mut().clear();
                self.active_session_id = Some(id);
                self.session_editor_open = false;
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
                    self.profile_message =
                        format!("Connected, but recent connections could not be saved: {error}");
                }
                self.resize_sessions(cx);
                cx.spawn(async move |this, cx| {
                    while let Some(event) = sftp_events.recv().await {
                        let result = this.update(cx, |view, cx| {
                            if view.apply_sftp_event(id, event) {
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
                self.profile_message = format!("Could not start SSH runtime: {error}");
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
                            if apply_session_event(&mut view.sessions, id, event) {
                                cx.notify();
                            }
                            if view
                                .session(id)
                                .is_some_and(|session| session.state == SessionState::Disconnected)
                                && view
                                    .auth_prompt
                                    .as_ref()
                                    .is_some_and(|prompt| prompt.session_id == id)
                            {
                                view.auth_prompt = None;
                                view.auth_prompt_needs_clear = true;
                                cx.notify();
                            }
                        });
                        if result.is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = this.update(cx, |view, cx| {
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
                    cx.notify();
                }
            });
        })
        .detach();
        window.focus(&self.terminal_focus);
        cx.notify();
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
        let closes_split_pane = self.split.is_some_and(|split| split.contains(id));
        if !remove_session(&mut self.sessions, &mut self.active_session_id, id) {
            return;
        }
        self.terminal_bounds.borrow_mut().remove(&id);
        self.snippet_targets.remove(&id);
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
                        self.send_terminal_request(
                            session_id,
                            TerminalRequest::Paste(text),
                            "Terminal paste",
                            cx,
                        );
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

        match enqueue_session_request(&self.sessions, session_id, TerminalRequest::Key(key)) {
            Ok(()) => cx.stop_propagation(),
            Err(error) => {
                if let Some(session) = self.session_mut(session_id) {
                    session.status = format!("Terminal input queue is unavailable: {error}");
                }
                cx.notify();
            }
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
            self.send_terminal_request(
                session_id,
                TerminalRequest::Paste(text),
                "Terminal paste",
                cx,
            );
            self.show_toast("Pasted from clipboard.", Tone::Success, cx);
        }
    }

    fn terminal_surface(&mut self, session_id: u64, cx: &mut Context<Self>) -> AnyElement {
        let terminal_bounds = Rc::clone(&self.terminal_bounds);
        let palette = self.settings.theme.palette();
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
            terminal_view = terminal_view.child(render_terminal_grid(frame, &self.settings));
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
                        session_state_label(state),
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
                section.label(),
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
                    "Leave empty for a direct connection.".to_owned()
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
                    "Session name",
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
                                "Host",
                                &self.host,
                                false,
                            )),
                        )
                        .child(
                            div()
                                .w(px(ui_tokens::PORT_FIELD_WIDTH))
                                .child(components::field(
                                    &tokens,
                                    "Port",
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
                                "Group",
                                &self.profile_group,
                                false,
                            )),
                        )
                        .child(components::button(
                            &tokens,
                            "toggle-profile-favorite",
                            if self.profile_favorite {
                                "Favorite"
                            } else {
                                "Not favorite"
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
                    "Hosts with the same group are listed under one folder in the sidebar.",
                ))
                .child(components::field(
                    &tokens,
                    "Jump host",
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
                    "Keep-alive (seconds, 0 = off)",
                    &self.keepalive_seconds,
                    false,
                ))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(ui_tokens::SPACE_1))
                        .child(components::label_text(&tokens, "Protocol"))
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
                        method.label(),
                        kind,
                        ButtonSize::Regular,
                    );
                    methods = methods.child(button.on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.auth_method = method;
                            cx.notify();
                        },
                    )));
                }
                div()
                    .flex()
                    .flex_col()
                    .gap(px(ui_tokens::SPACE_3))
                    .child(components::field(
                        &tokens,
                        "Username",
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
                                "Authentication method",
                            ))
                            .child(methods),
                    )
                    .when(self.auth_method.uses_password_input(), |panel| {
                        panel
                            .child(components::field(
                                &tokens,
                                "Password",
                                &self.password,
                                true,
                            ))
                            .child(components::help_text(
                                &tokens,
                                if self.auth_method == AuthMethod::KeyboardInteractive {
                                    "Hidden keyboard-interactive prompts are answered with this password. One-time codes and visible prompts are not supported yet."
                                } else {
                                    "Passwords are used for a connection and are never saved with host profiles."
                                },
                            ))
                    })
                    .when(self.auth_method == AuthMethod::PublicKey, |panel| {
                        panel
                            .child(components::field(
                                &tokens,
                                "Identity file",
                                &self.identity_file,
                                false,
                            ))
                            .child(components::field(
                                &tokens,
                                "Key passphrase",
                                &self.key_passphrase,
                                true,
                            ))
                            .child(components::help_text(
                                &tokens,
                                "The identity file path is saved with the host. Passphrases and key material are never saved.",
                            ))
                    })
                    .when(self.auth_method == AuthMethod::Agent, |panel| {
                        panel.child(components::help_text(
                            &tokens,
                            "Uses the Windows OpenSSH agent named pipe or Pageant. Load the key into the agent before connecting.",
                        ))
                    })
                    .when(!jump_name.is_empty(), |panel| {
                        panel
                            .child(components::subheading(&tokens, "Jump host"))
                            .child(components::help_text(&tokens, jump_summary))
                            .when(jump_method == Some(AuthMethod::Password), |panel| {
                                panel.child(components::field(
                                    &tokens,
                                    "Jump password",
                                    &self.jump_password,
                                    true,
                                ))
                            })
                            .when(jump_method == Some(AuthMethod::PublicKey), |panel| {
                                panel.child(components::field(
                                    &tokens,
                                    "Jump key passphrase",
                                    &self.jump_passphrase,
                                    true,
                                ))
                            })
                            .child(components::help_text(
                                &tokens,
                                "Jump credentials are used only for the jump host and are never saved.",
                            ))
                    })
            }
            SessionEditorSection::Security => div()
                .flex()
                .flex_col()
                .gap(px(ui_tokens::SPACE_3))
                .child(components::subheading(&tokens, "Server identity"))
                .child(components::help_text(
                    &tokens,
                    "Host keys must already match an entry in the user's OpenSSH known_hosts file. Unknown and changed keys are rejected. When a jump host is used, both the jump host and the target are verified independently.",
                ))
                .child(components::subheading(&tokens, "Credential handling"))
                .child(components::help_text(
                    &tokens,
                    "Password and key passphrase fields are cleared when you connect or cancel. Saved host profiles contain connection details, an optional identity file path, and a jump host reference, never passwords, passphrases, or key material.",
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
                            "Keep",
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
                            "Delete",
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
                        "Delete",
                        ButtonKind::Danger,
                        ButtonSize::Regular,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.request_profile_delete(cx))),
                );
            }
        }

        let title = if self.selected_profile.is_some() {
            "Session Properties"
        } else {
            "New SSH Session"
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
                                    "Close",
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
                                        .child(self.session_editor_section.label()),
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
                                    "Cancel",
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
                                    "Save",
                                    ButtonKind::Secondary,
                                    ButtonSize::Regular,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.save_profile(cx))),
                            )
                            .child(
                                components::button(
                                    &tokens,
                                    "connect",
                                    "Connect",
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

    fn saved_host_row(
        &self,
        tokens: &ThemeTokens,
        prefix: &'static str,
        index: usize,
        profile: &HostProfile,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_selected = self.selected_profile.as_deref() == Some(&profile.name);
        let favorite = profile.favorite;
        let favorite_name = profile.name.clone();
        let selected_profile = profile.clone();
        let edit_profile = profile.clone();
        let actions = div()
            .flex()
            .items_center()
            .gap(px(2.0))
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
                    "Edit host",
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
            this.load_profile(&selected_profile, window, cx);
        }))
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
            "Load into the connection form",
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
            this.load_recent(&row_entry, window, cx);
        }))
        .into_any_element()
    }

    fn apply_sftp_event(&mut self, id: u64, event: SftpEvent) -> bool {
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
                    let _ = sftp_tx.try_send(SftpCommand::List(target));
                }
            }
            SftpEvent::Entries { path, entries } => {
                session.sftp.path = path;
                session.sftp.entries = entries;
                session.sftp.message.clear();
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
                });
                session.sftp.message = format!("{} started: {name}", kind.label());
            }
            SftpEvent::TransferProgress {
                kind,
                name,
                transferred,
                total,
            } => {
                session.sftp.transfer = Some(TransferProgressUi {
                    kind,
                    name,
                    transferred,
                    total,
                });
            }
            SftpEvent::TransferFinished { kind, name } => {
                session.sftp.transfer = None;
                session.sftp.message = format!("{} finished: {name}", kind.label());
                if kind == TransferKind::Upload {
                    let path = session.sftp.path.clone();
                    if let Some(sftp_tx) = &session.sftp_tx {
                        let _ = sftp_tx.try_send(SftpCommand::List(path));
                    }
                }
            }
            SftpEvent::TransferFailed { kind, name, error } => {
                session.sftp.transfer = None;
                session.sftp.message = format!("{} failed for {name}: {error}", kind.label());
            }
            SftpEvent::TransferCancelled { kind, name } => {
                session.sftp.transfer = None;
                session.sftp.message = format!("{} cancelled: {name}", kind.label());
            }
            SftpEvent::OperationFinished { action } => {
                session.sftp.message = action;
                let path = session.sftp.path.clone();
                if let Some(sftp_tx) = &session.sftp_tx {
                    let _ = sftp_tx.try_send(SftpCommand::List(path));
                }
            }
        }
        true
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
            multiple: false,
            prompt: Some("Upload file".into()),
        });
        cx.spawn(async move |this, cx| match receiver.await {
            Ok(Ok(Some(mut paths))) => {
                if let Some(local) = paths.pop() {
                    let _ = this.update(cx, |view, cx| view.queue_upload(session_id, local, cx));
                }
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
        self.send_sftp_command(
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
                        view.send_sftp_command(
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
        self.send_sftp_command(
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
                        view.send_sftp_command(
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
            session.sftp.message = "Transfer cancelled.".to_owned();
        }
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
        let overwrite = session.sftp.overwrite.clone();
        let rename_from = session.sftp.rename_from.clone();
        let pending_delete = session.sftp.pending_delete.clone();

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
                    .text_color(rgb(0x667078))
                    .child("No entries"),
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
                            .label("Download")
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
                            .text_color(rgb(0x667078))
                            .child(size),
                    )
                    .child(
                        Button::new(("sftp-download", index))
                            .label("Download")
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
                    Button::new(("sftp-rename", index))
                        .label("Rename")
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
                        .label("Delete")
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
                    .label("Up")
                    .on_click(cx.listener(move |this, _, _, cx| this.sftp_go_up(session_id, cx))),
            )
            .child(
                Button::new("sftp-refresh")
                    .label("Refresh")
                    .on_click(cx.listener(move |this, _, _, cx| this.refresh_sftp(session_id, cx))),
            )
            .child(
                Button::new("sftp-upload")
                    .label("Upload")
                    .on_click(cx.listener(move |this, _, _, cx| this.begin_upload(session_id, cx))),
            )
            .child(
                Button::new("sftp-upload-dir")
                    .label("Folder")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.begin_upload_directory(session_id, cx);
                    })),
            )
            .child(
                Button::new("sftp-close")
                    .label("Close")
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
            .border_color(rgb(0x343c42))
            .bg(rgb(0x11161b))
            .text_color(rgb(0xd4dade))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(TERMINAL_PANEL_HEADER_PX))
                    .flex_shrink_0()
                    .px(px(8.))
                    .bg(rgb(0x171d22))
                    .child(div().text_size(px(11.)).child("SFTP"))
                    .child(controls),
            )
            .child(
                div()
                    .px(px(8.))
                    .py(px(4.))
                    .text_size(px(10.))
                    .text_color(rgb(0x8a949b))
                    .overflow_hidden()
                    .child(if path.is_empty() {
                        "Waiting for the SFTP subsystem...".to_owned()
                    } else {
                        path
                    }),
            );
        let mut operations = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(8.))
            .py(px(4.))
            .child(div().flex_1().child(components::field(
                &tokens,
                "Remote name",
                &self.sftp_name,
                false,
            )));
        if rename_from.is_some() {
            operations =
                operations
                    .child(
                        Button::new("sftp-rename-apply")
                            .label("Rename")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.apply_sftp_rename(session_id, window, cx);
                            })),
                    )
                    .child(Button::new("sftp-rename-cancel").label("Cancel").on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.cancel_sftp_rename(session_id, cx);
                        }),
                    ));
        } else {
            operations = operations.child(Button::new("sftp-mkdir").label("New folder").on_click(
                cx.listener(move |this, _, _, cx| {
                    this.create_sftp_directory(session_id, cx);
                }),
            ));
        }
        panel = panel.child(operations).child(list);

        if let Some((delete_path, is_dir)) = pending_delete {
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .p(px(8.))
                    .border_t_1()
                    .border_color(rgb(0x2a3238))
                    .text_size(px(11.))
                    .child(format!(
                        "Delete {} {delete_path}?",
                        if is_dir { "directory" } else { "file" }
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
                                    "Cancel",
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
                                    "Delete",
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
                    .border_color(rgb(0x2a3238))
                    .text_size(px(10.))
                    .child(format!(
                        "{} {} · {percent}% · {} / {}",
                        progress.kind.label(),
                        progress.name,
                        format_size(progress.transferred),
                        format_size(progress.total)
                    ))
                    .child(
                        Button::new("sftp-cancel-transfer")
                            .label("Cancel transfer")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.cancel_transfer(session_id, cx);
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
                    .border_color(rgb(0x2a3238))
                    .text_size(px(11.))
                    .child(format!(
                        "{} target already exists: {target}",
                        overwrite.kind.label()
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                components::button(
                                    &tokens,
                                    "sftp-keep",
                                    "Keep existing",
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
                                    "Resume",
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
                                    "Overwrite",
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
                    .border_color(rgb(0x2a3238))
                    .text_size(px(10.))
                    .text_color(rgb(0x9aa4ab))
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
                session.forwards.message = "Forwarding rule stopped.".to_owned();
                persist_remove = profile_name.clone().zip(removed);
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
            let button = Button::new(("forward-kind", index)).label(candidate.label());
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
                "Listen host",
                &self.forward_listen_host,
                false,
            ))
            .child(components::field(
                &tokens,
                "Listen port (0 = auto)",
                &self.forward_listen_port,
                false,
            ));
        if kind.needs_target() {
            form = form
                .child(components::field(
                    &tokens,
                    "Target host",
                    &self.forward_target_host,
                    false,
                ))
                .child(components::field(
                    &tokens,
                    "Target port",
                    &self.forward_target_port,
                    false,
                ));
        } else {
            form = form.child(
                div()
                    .text_size(px(10.))
                    .text_color(rgb(0x8a949b))
                    .child("SOCKS5 clients choose the target host and port."),
            );
        }
        form = form.child(
            Button::new("forward-add")
                .label("Add tunnel")
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
                    .text_color(rgb(0x667078))
                    .child("No forwarding rules"),
            );
        }
        for (rule_id, rule) in rules {
            let spec = rule.spec.clone();
            let summary = if spec.kind == ForwardKind::Dynamic {
                format!(
                    "{} {}:{} · {} connection{}",
                    spec.kind.label(),
                    spec.listen_host,
                    rule.bound_port,
                    rule.connections,
                    if rule.connections == 1 { "" } else { "s" }
                )
            } else {
                format!(
                    "{} {}:{} -> {}:{} · {} connection{}",
                    spec.kind.label(),
                    spec.listen_host,
                    rule.bound_port,
                    spec.target_host,
                    spec.target_port,
                    rule.connections,
                    if rule.connections == 1 { "" } else { "s" }
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
                            .label("Stop")
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
            .border_color(rgb(0x343c42))
            .bg(rgb(0x11161b))
            .text_color(rgb(0xd4dade))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(TERMINAL_PANEL_HEADER_PX))
                    .flex_shrink_0()
                    .px(px(8.))
                    .bg(rgb(0x171d22))
                    .child(div().text_size(px(11.)).child("Tunnels"))
                    .child(Button::new("forward-close").label("Close").on_click(
                        cx.listener(move |this, _, _, cx| this.toggle_tunnels(session_id, cx)),
                    )),
            )
            .child(form)
            .child(
                div()
                    .px(px(8.))
                    .text_size(px(11.))
                    .text_color(rgb(0x9aa4ab))
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
        self.snippet_message = format!("Editing {}.", snippet.name);
        cx.notify();
    }

    fn save_snippet(&mut self, cx: &mut Context<Self>) {
        let name = self.snippet_name.read(cx).value().trim().to_owned();
        let command = self.snippet_command.read(cx).value().trim().to_owned();
        if name.is_empty() || command.is_empty() {
            self.snippet_message = "Snippet name and command are required.".to_owned();
            cx.notify();
            return;
        }
        let Some(store) = self.snippet_store.as_ref() else {
            self.snippet_message = "Command snippet storage is unavailable.".to_owned();
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
            self.snippet_message = format!("A snippet named {name} already exists.");
            cx.notify();
            return;
        }
        let mut snippets = self.snippets.clone();
        if let Some(editing) = &self.editing_snippet {
            snippets.retain(|snippet| !snippet.name.eq_ignore_ascii_case(editing));
        }
        if snippets.len() >= MAX_SNIPPETS {
            self.snippet_message = format!("Cannot have more than {MAX_SNIPPETS} snippets.");
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
        self.snippet_message = format!("Saved {name}.");
        cx.notify();
    }

    fn delete_snippet(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = self.snippet_store.as_ref() else {
            self.snippet_message = "Command snippet storage is unavailable.".to_owned();
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
        self.snippet_message = format!("Deleted {name}.");
        cx.notify();
    }

    fn toggle_snippets(&mut self, cx: &mut Context<Self>) {
        self.snippets_visible = !self.snippets_visible;
        if self.snippets_visible {
            self.settings_visible = false;
            self.keys_visible = false;
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

    fn toggle_snippet_target(&mut self, session_id: u64, cx: &mut Context<Self>) {
        if !self.snippet_targets.remove(&session_id) {
            self.snippet_targets.insert(session_id);
        }
        cx.notify();
    }

    fn send_snippet_to_active(&mut self, snippet: &CommandSnippet, cx: &mut Context<Self>) {
        let Some(session_id) = self.active_session_id else {
            self.snippet_message = "There is no active session.".to_owned();
            cx.notify();
            return;
        };
        if self.send_command_to_session(session_id, &snippet.command, snippet.send_enter, cx) {
            self.snippet_message = format!("Sent {} to the active session.", snippet.name);
        } else {
            self.snippet_message = "The active session cannot accept commands.".to_owned();
        }
        cx.notify();
    }

    fn broadcast_snippet(&mut self, cx: &mut Context<Self>) {
        let command = self.snippet_command.read(cx).value().trim().to_owned();
        if command.is_empty() {
            self.snippet_message = "Enter a command before broadcasting.".to_owned();
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
            self.snippet_message = "Select at least one connected target session.".to_owned();
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
            "No selected session could accept the command.".to_owned()
        } else {
            format!("Sent to: {}.", sent_to.join(", "))
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
            self.snippet_message = format!("Could not send the command: {error}");
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
                self.snippet_message = format!("Could not send the command: {error}");
                return false;
            }
        }
        cx.notify();
        true
    }

    fn snippets_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.tokens();
        let snippets = self.snippets.clone();
        let message = self.snippet_message.clone();
        let targets = self.snippet_targets.clone();
        let mut list = div()
            .id("snippet-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(4.))
            .overflow_y_scroll();
        if snippets.is_empty() {
            list = list.child(
                div()
                    .p(px(8.))
                    .text_size(px(11.))
                    .text_color(rgb(0x667078))
                    .child("No command snippets"),
            );
        }
        for (index, snippet) in snippets.into_iter().enumerate() {
            let load = snippet.clone();
            let send = snippet.clone();
            let delete_name = snippet.name.clone();
            list =
                list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            Button::new(("snippet-load", index))
                                .label(snippet.name.clone())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.load_snippet(&load, window, cx);
                                })),
                        )
                        .child(Button::new(("snippet-send", index)).label("Send").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.send_snippet_to_active(&send, cx);
                            }),
                        ))
                        .child(Button::new(("snippet-delete", index)).label("x").on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.delete_snippet(&delete_name, window, cx);
                            }),
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_size(px(10.))
                                .text_color(rgb(0x7b858c))
                                .child(snippet.command),
                        ),
                );
        }

        let mut target_list = div().flex().flex_col().gap(px(3.));
        let connected_sessions = self
            .sessions
            .iter()
            .filter(|session| session.state == SessionState::Connected)
            .map(|session| (session.id, session.title.clone()))
            .collect::<Vec<_>>();
        if connected_sessions.is_empty() {
            target_list = target_list.child(
                div()
                    .text_size(px(10.))
                    .text_color(rgb(0x667078))
                    .child("No connected sessions"),
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
            .w(px(360.))
            .flex_shrink_0()
            .min_h_0()
            .ml(px(6.))
            .border_1()
            .border_color(rgb(0x343c42))
            .bg(rgb(0x11161b))
            .text_color(rgb(0xd4dade))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(TERMINAL_PANEL_HEADER_PX))
                    .flex_shrink_0()
                    .px(px(8.))
                    .bg(rgb(0x171d22))
                    .child(div().text_size(px(11.)).child("Snippets"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(Button::new("snippet-new").label("New").on_click(
                                cx.listener(|this, _, window, cx| this.new_snippet(window, cx)),
                            ))
                            .child(
                                Button::new("snippet-close").label("Close").on_click(
                                    cx.listener(|this, _, _, cx| this.toggle_snippets(cx)),
                                ),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .p(px(8.))
                    .child(components::field(
                        &tokens,
                        "Name",
                        &self.snippet_name,
                        false,
                    ))
                    .child(components::field(
                        &tokens,
                        "Command",
                        &self.snippet_command,
                        false,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                Button::new("snippet-enter")
                                    .label(if self.snippet_send_enter {
                                        "Press Enter"
                                    } else {
                                        "No Enter"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.snippet_send_enter = !this.snippet_send_enter;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("snippet-save")
                                    .label("Save")
                                    .on_click(cx.listener(|this, _, _, cx| this.save_snippet(cx))),
                            ),
                    ),
            )
            .child(
                div()
                    .px(px(8.))
                    .text_size(px(11.))
                    .text_color(rgb(0x9aa4ab))
                    .child(message),
            )
            .child(list)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .p(px(8.))
                    .border_t_1()
                    .border_color(rgb(0x2a3238))
                    .child(div().text_size(px(10.)).child("Send to selected sessions"))
                    .child(target_list)
                    .child(
                        Button::new("snippet-broadcast")
                            .label(format!("Send to {} selected", self.snippet_targets.len()))
                            .on_click(cx.listener(|this, _, _, cx| this.broadcast_snippet(cx))),
                    ),
            )
            .into_any_element()
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

    fn toggle_ui_theme(&mut self, cx: &mut Context<Self>) {
        self.ui_theme = self.ui_theme.toggled();
        ui::theme::install(cx, self.ui_theme);
        self.persist_ui_preferences(cx);
        cx.notify();
    }

    fn persist_ui_preferences(&mut self, cx: &mut Context<Self>) {
        self.settings.ui_theme = self.ui_theme;
        self.settings.sidebar_collapsed = self.sidebar_collapsed;
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
            self.settings_visible = false;
            self.snippets_visible = false;
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
        Self::set_input_value(&self.new_key_passphrase, "", window, cx);
        self.key_message = "Generating Ed25519 key...".to_owned();
        self.key_public = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let read_path = path.clone();
            let result = cx
                .background_spawn(async move {
                    keygen::generate_ed25519(&read_path, passphrase.as_str(), &comment)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(generated) => {
                        view.key_message = format!(
                            "Created {} and {}.",
                            generated.private_path.display(),
                            generated.public_path.display()
                        );
                        view.key_public = Some(generated.public_key);
                        view.show_toast("Ed25519 key pair generated.", Tone::Success, cx);
                    }
                    Err(error) => {
                        view.key_message = format!("{error:#}");
                        view.key_public = None;
                        view.show_toast("Key generation failed.", Tone::Danger, cx);
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
        let mut body = div()
            .id("keys-body")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap(px(ui_tokens::SPACE_3))
            .p(px(ui_tokens::SPACE_4))
            .child(components::field(
                &tokens,
                "Private key path",
                &self.key_path,
                false,
            ))
            .child(components::field(
                &tokens,
                "Passphrase (optional)",
                &self.new_key_passphrase,
                true,
            ))
            .child(components::field(&tokens, "Comment", &self.key_comment, false))
            .child(
                components::button(
                    &tokens,
                    "generate-key",
                    "Generate Ed25519 key",
                    ButtonKind::Primary,
                    ButtonSize::Regular,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.generate_key(window, cx);
                })),
            )
            .child(components::help_text(
                &tokens,
                "Creates an OpenSSH private key and a .pub file. Existing files are never overwritten. The passphrase is not stored; Unix key files use mode 0600.",
            ));
        if !message.is_empty() {
            body = body.child(components::help_text(&tokens, message));
        }
        if let Some(public_key) = public_key {
            body = body
                .child(components::section_label(&tokens, "Public key"))
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
                        "Copy public key",
                        ButtonKind::Secondary,
                        ButtonSize::Small,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(key) = this.key_public.clone() {
                            cx.write_to_clipboard(ClipboardItem::new_string(key));
                            this.show_toast("Public key copied.", Tone::Success, cx);
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
                        components::modal_header(&tokens, "User keys").child(
                            components::icon_button(
                                &tokens,
                                "keys-close",
                                IconName::Close,
                                ButtonKind::Ghost,
                                ButtonSize::Small,
                                "Close",
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
            sidebar_collapsed: self.sidebar_collapsed,
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
        self.show_toast(self.settings_message.clone(), Tone::Success, cx);
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
        self.settings = settings;
        self.settings_theme = self.settings.theme;
        self.settings_cursor = self.settings.cursor_style;
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
        self.show_toast(self.settings_message.clone(), Tone::Success, cx);
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
            if let Some(session) = self.session_mut(session_id) {
                session.status = format!(
                    "The server requested {} keyboard-interactive prompts; at most {MAX_AUTH_PROMPTS} are supported.",
                    prompts.len()
                );
            }
            return;
        }
        if let Some(session) = self.session_mut(session_id) {
            session.status = if name.trim().is_empty() {
                "Waiting for keyboard-interactive input...".to_owned()
            } else {
                format!("Keyboard-interactive authentication: {name}")
            };
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
        if let Some(session) = self.session_mut(prompt.session_id) {
            session.status = "Submitting authentication response...".to_owned();
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
            "Keyboard-interactive authentication".to_owned()
        } else {
            format!("Keyboard-interactive authentication: {}", prompt.name)
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
            "Responses are sent only to the SSH server and are not stored.",
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
                                    "Cancel",
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
                                    "Continue",
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
                theme.label(),
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
                style.label(),
                kind,
                ButtonSize::Small,
            );
            cursor_selector =
                cursor_selector.child(button.on_click(cx.listener(move |this, _, _, cx| {
                    this.settings_cursor = style;
                    cx.notify();
                })));
        }

        let mut shortcut_fields = div().flex().flex_col().gap(px(ui_tokens::SPACE_3));
        for (index, action) in ShortcutAction::ALL.iter().enumerate() {
            shortcut_fields = shortcut_fields.child(components::field(
                &tokens,
                action.label(),
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
                        components::modal_header(&tokens, "Settings").child(
                            components::button(
                                &tokens,
                                "settings-close",
                                "Close",
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
                        "Font family",
                        &self.settings_font_family,
                        false,
                    ))
                    .child(components::field(
                        &tokens,
                        "Font size",
                        &self.settings_font_size,
                        false,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(ui_tokens::SPACE_2))
                            .child(components::section_label(&tokens, "Terminal theme"))
                            .child(theme_selector),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(ui_tokens::SPACE_2))
                            .child(components::section_label(&tokens, "Cursor style"))
                            .child(cursor_selector),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(ui_tokens::SPACE_2))
                            .child(components::section_label(&tokens, "Workspace shortcuts"))
                            .child(shortcut_fields),
                    )
                    .child(components::help_text(
                        &tokens,
                        "Format: ctrl+shift+t, alt+F12 or cmd+space. Alt+1..9 always switches tabs.",
                    ))
                    .child(components::help_text(
                        &tokens,
                        format!(
                            "Allowed range: {MIN_FONT_SIZE:.0}-{MAX_FONT_SIZE:.0} px. Cell size and PTY dimensions follow the font size."
                        ),
                    ))
                    .child(components::help_text(&tokens, message)),
            )
            .child(
                components::modal_footer(&tokens)
                    .child(
                        components::button(
                            &tokens,
                            "settings-reset",
                            "Reset",
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
                            "Apply",
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
                    "Expand sidebar",
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
                    "New host",
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
                    "Import OpenSSH config",
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
                profile_list = profile_list.child(components::section_header(tokens, "FAVORITES"));
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

            let mut groups: BTreeMap<String, (String, Vec<usize>)> = BTreeMap::new();
            for (index, profile) in self.profiles.iter().enumerate() {
                let key = profile.group.to_lowercase();
                let display = if profile.group.is_empty() {
                    "UNGROUPED".to_owned()
                } else {
                    profile.group.clone()
                };
                groups
                    .entry(key)
                    .or_insert_with(|| (display, Vec::new()))
                    .1
                    .push(index);
            }
            for (_, (display, mut indexes)) in groups {
                indexes.sort_by_key(|index| self.profiles[*index].name.to_lowercase());
                profile_list = profile_list.child(components::section_header(tokens, display));
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
                matching_profiles += 1;
            }

            if !self.recent.is_empty() {
                profile_list = profile_list.child(components::section_header(tokens, "RECENT"));
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
                        "No saved hosts",
                        "Import an OpenSSH config or create a host to get started.",
                    )
                    .child(
                        components::button(
                            tokens,
                            "empty-new-host",
                            "New host",
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
                        "No matching hosts",
                        "Try a different name, group, user or port.",
                    )
                    .child(
                        components::button(
                            tokens,
                            "clear-host-search",
                            "Clear search",
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
            .w(px(ui_tokens::SIDEBAR_WIDTH))
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
                            "Collapse sidebar (Ctrl+Shift+B)",
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
                        components::button(
                            tokens,
                            "import-openssh-config",
                            "Import",
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
                            "New",
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
                        .child("split"),
                );
            }
            tab = tab.child(
                components::icon_button(
                    tokens,
                    ("close-session", id),
                    IconName::Close,
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    "Close session",
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
                    .child(components::help_text(tokens, "No active sessions")),
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
                    "new-session-tab",
                    IconName::Plus,
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    "New session",
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
                        "Side by side",
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
                        "Stacked",
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
                        "Close split",
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
            .unwrap_or_else(|| "SSH workspace".to_owned());
        let state = active.map(|session| session.state);

        let mut left = div()
            .flex()
            .items_center()
            .gap(px(ui_tokens::SPACE_1))
            .flex_shrink_0()
            .child(
                components::button(
                    tokens,
                    "new-session",
                    "New session",
                    ButtonKind::Primary,
                    ButtonSize::Small,
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
                    "Search saved hosts",
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.focus_profile_search(window, cx);
                })),
            );
        if self.selected_profile.is_some() {
            left = left.child(
                components::button(
                    tokens,
                    "edit-session-profile",
                    "Properties",
                    ButtonKind::Secondary,
                    ButtonSize::Small,
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
                components::button(
                    tokens,
                    "split-active-session",
                    "Split",
                    ButtonKind::Secondary,
                    ButtonSize::Small,
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
                    session_state_label(state),
                    session_tone(state),
                ))
            });

        let mut right = div()
            .flex()
            .items_center()
            .gap(px(ui_tokens::SPACE_1))
            .flex_shrink_0();
        if let Some(session) = active {
            let active_id = session.id;
            let files_label = if session.sftp.visible {
                "Close files"
            } else {
                "Files"
            };
            let tunnels_label = if session.forwards.visible {
                "Close tunnels"
            } else {
                "Tunnels"
            };
            let log_label = if session.log_path.is_some() {
                "Stop log"
            } else {
                "Log"
            };
            right = right
                .child(
                    components::button(
                        tokens,
                        "toggle-sftp",
                        files_label,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_sftp(active_id, cx);
                    })),
                )
                .child(
                    components::button(
                        tokens,
                        "toggle-tunnels",
                        tunnels_label,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_tunnels(active_id, cx);
                    })),
                )
                .child(
                    components::button(
                        tokens,
                        "toggle-log",
                        log_label,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_log(active_id, cx);
                    })),
                )
                .child(
                    components::button(
                        tokens,
                        "replay-log",
                        "Replay",
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.begin_log_replay(cx))),
                )
                .child(
                    components::button(
                        tokens,
                        "find-terminal",
                        "Find",
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_search(active_id, window, cx);
                    })),
                );
            if matches!(
                session.state,
                SessionState::Connecting | SessionState::Connected
            ) {
                let label = if session.state == SessionState::Connecting {
                    "Cancel"
                } else {
                    "Disconnect"
                };
                right = right.child(
                    components::button(
                        tokens,
                        "disconnect-active",
                        label,
                        ButtonKind::Secondary,
                        ButtonSize::Small,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.disconnect_session(active_id, cx);
                    })),
                );
            }
        }
        right = right
            .child(
                components::button(
                    tokens,
                    "toggle-snippets",
                    "Snippets",
                    if self.snippets_visible {
                        ButtonKind::Secondary
                    } else {
                        ButtonKind::Ghost
                    },
                    ButtonSize::Small,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_snippets(cx))),
            )
            .child(
                components::button(
                    tokens,
                    "toggle-keys",
                    "Keys",
                    if self.keys_visible {
                        ButtonKind::Secondary
                    } else {
                        ButtonKind::Ghost
                    },
                    ButtonSize::Small,
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_keys(window, cx))),
            )
            .child(
                components::icon_button(
                    tokens,
                    "toggle-ui-theme",
                    IconName::Palette,
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    format!("Switch to {} theme", self.ui_theme.toggled().label()),
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
                    "Settings",
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_settings(window, cx))),
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
            None => (Tone::Neutral, "dshell".to_owned(), self.status.clone()),
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
                    .when(!identity.is_empty(), |right| right.child(identity))
                    .when(!dimensions.is_empty(), |right| right.child(dimensions)),
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
        let forward_message = session.forwards.message.clone();
        let forwards_visible = session.forwards.visible;
        let username = session.username.clone();
        let host = session.host.clone();
        let port = session.port;
        let state = session.state;
        let terminal_size = session.terminal_size;
        let log_active = session.log_path.is_some();
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
                "Transfers",
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
                                    .child(transfer.kind.label()),
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
                                "{} / {}",
                                format_size(transfer.transferred),
                                format_size(transfer.total)
                            ))
                            .child(format!("{:.0}%", ratio * 100.0)),
                    );
            } else {
                group = group.child(components::help_text(
                    tokens,
                    if transfer_message.is_empty() {
                        "No active transfer.".to_owned()
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
                        "Close files"
                    } else {
                        "Open files"
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
                "Forwards",
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
            if rules.is_empty() {
                group = group.child(components::help_text(tokens, "No forwarding rules."));
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
                                                rule.spec.kind.label(),
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
                                            "Stop rule",
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
                                    .child(format!("{} active", rule.connections)),
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
                        "Close tunnels"
                    } else {
                        "Manage tunnels"
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
                "Connection",
                self.inspector_info_open,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.inspector_info_open = !this.inspector_info_open;
                cx.notify();
            })),
        );
        if self.inspector_info_open {
            let auth = profile
                .as_ref()
                .map_or("Quick connect", |profile| profile.auth_method.label());
            let profile_name = profile
                .as_ref()
                .map_or_else(|| "Unsaved".to_owned(), |profile| profile.name.clone());
            let jump = profile
                .as_ref()
                .map(|profile| profile.proxy_jump.clone())
                .filter(|jump| !jump.is_empty())
                .unwrap_or_else(|| "None".to_owned());
            let keepalive = profile
                .as_ref()
                .map(|profile| profile.keepalive_seconds)
                .unwrap_or(0);
            body = body
                .child(components::info_row(
                    tokens,
                    "Host",
                    format!("{username}@{host}:{port}"),
                ))
                .child(components::info_row(
                    tokens,
                    "State",
                    session_state_label(state),
                ))
                .child(components::info_row(
                    tokens,
                    "Terminal",
                    format!("{} x {}", terminal_size.columns, terminal_size.rows),
                ))
                .child(components::info_row(tokens, "Auth", auth))
                .child(components::info_row(tokens, "Profile", profile_name))
                .child(components::info_row(tokens, "Jump host", jump))
                .child(components::info_row(
                    tokens,
                    "Keep-alive",
                    if keepalive == 0 {
                        "Off".to_owned()
                    } else {
                        format!("{keepalive}s")
                    },
                ))
                .child(components::info_row(
                    tokens,
                    "Logging",
                    if log_active { "Recording" } else { "Off" },
                ));
        }

        components::panel(tokens)
            .id("inspector")
            .w(px(ui_tokens::INSPECTOR_WIDTH))
            .ml(px(ui_tokens::SPACE_2))
            .border_1()
            .border_color(tokens.border_subtle)
            .rounded(px(ui_tokens::RADIUS_PANEL))
            .overflow_hidden()
            .child(
                components::panel_header(tokens, "Inspector").child(
                    components::icon_button(
                        tokens,
                        "close-inspector",
                        IconName::Close,
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                        "Close inspector",
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

fn render_terminal_grid(frame: &TerminalFrame, settings: &AppSettings) -> AnyElement {
    let mut grid = div().flex().flex_col().flex_1().min_h_0();
    for (row_index, row) in frame.rows.iter().enumerate() {
        let mut rendered_row = div()
            .flex()
            .relative()
            .h(px(settings.cell_height()))
            .flex_shrink_0();
        for run in row {
            rendered_row = rendered_row.child(render_terminal_run(run, settings));
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

fn render_terminal_run(run: &TerminalRun, settings: &AppSettings) -> impl IntoElement {
    let mut cell_run = div()
        .flex_shrink_0()
        .w(px(settings.cell_width() * run.width_cells as f32))
        .h(px(settings.cell_height()))
        .font_family(settings.font_family.clone())
        .text_size(px(settings.font_size))
        .text_color(rgb(run.foreground))
        .bg(rgb(run.background));
    if run.selected {
        cell_run = cell_run.text_color(rgb(0xf4f6f8)).bg(rgb(0x315b80));
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

fn read_log_for_replay(path: &Path, limit: u64) -> Result<(Vec<u8>, bool), String> {
    let limit = limit.max(1);
    let mut file =
        File::open(path).map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    let length = file
        .metadata()
        .map_err(|error| format!("Could not read {} metadata: {error}", path.display()))?
        .len();
    let truncated = length > limit;
    if truncated {
        file.seek(SeekFrom::Start(length - limit))
            .map_err(|error| format!("Could not seek {}: {error}", path.display()))?;
    }
    let mut bytes = Vec::with_capacity(length.min(limit) as usize);
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    Ok((bytes, truncated))
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
                        "No active terminal",
                        "Create a session or open a saved host from the sidebar.",
                    )),
            );
        }
        if self.inspector_visible
            && let Some(session_id) = self.active_session_id
        {
            terminal_workspace = terminal_workspace.child(self.inspector(session_id, &tokens, cx));
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

        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(tokens.bg_canvas)
            .text_color(tokens.text_primary)
            .child(command_bar)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(sidebar)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(tab_bar)
                            .child(terminal_workspace),
                    ),
            )
            .child(status_bar);
        if self.keys_visible {
            root = root.child(self.keys_panel(cx));
        }
        if self.settings_visible {
            root = root.child(self.settings_panel(cx));
        }
        if self.session_editor_open {
            root = root.child(self.session_editor(cx));
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
    Application::new().run(|cx| {
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
