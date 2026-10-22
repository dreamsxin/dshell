mod profiles;
mod ssh;
mod terminal;

use std::{cell::RefCell, rc::Rc};

use gpui::{StatefulInteractiveElement as _, prelude::*, *};
use gpui_component::{
    Root,
    button::{Button, ButtonVariants},
    input::{InputState, TextInput},
};
use profiles::{HostProfile, ProfileStore};
use ssh::{ConnectionSpec, UiEvent};
use terminal::{
    CELL_HEIGHT_PX, CELL_WIDTH_PX, MouseTrackingMode, TerminalFrame, TerminalKeyInput,
    TerminalMouseButton, TerminalMouseEvent, TerminalMouseModifiers, TerminalPosition,
    TerminalRequest, TerminalRun, TerminalSize,
};
use tokio::sync::{mpsc, watch};
use zeroize::Zeroizing;

const HOSTS_SIDEBAR_WIDTH_PX: f32 = 228.0;
const EXTRA_WORKSPACE_CHROME_PX: f32 = 172.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionState {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
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
    state: SessionState,
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
                self.state = SessionState::Disconnected;
            }
            UiEvent::Disconnected => {
                self.clear_terminal(
                    "Disconnected",
                    "Disconnected. Verify the host details and connect again.",
                );
                self.command_tx = None;
                self.cancel_tx = None;
                self.state = SessionState::Disconnected;
            }
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
        MouseTrackingMode, SessionState, SshSession, TerminalFrame, TerminalRequest, TerminalSize,
        UiEvent, apply_session_event, enqueue_session_request, mpsc, remove_session, watch,
    };

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
}

struct AppView {
    profile_name: Entity<InputState>,
    profile_search: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    status: String,
    sessions: Vec<SshSession>,
    active_session_id: Option<u64>,
    next_session_id: u64,
    terminal_size: TerminalSize,
    terminal_bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
    terminal_focus: FocusHandle,
    profiles: Vec<HostProfile>,
    profile_store: Option<ProfileStore>,
    profiles_ready: bool,
    selected_profile: Option<String>,
    confirm_delete: bool,
    profile_message: String,
    _bounds_subscription: Subscription,
}

impl AppView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let profile_name = cx.new(|cx| InputState::new(window, cx).placeholder("Production"));
        let profile_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search saved hosts"));
        let host = cx.new(|cx| InputState::new(window, cx).placeholder("server.example.com"));
        let port = cx.new(|cx| InputState::new(window, cx).default_value("22"));
        let username = cx.new(|cx| InputState::new(window, cx).placeholder("user"));
        let password = cx.new(|cx| InputState::new(window, cx).placeholder("Password"));
        password.update(cx, |state, cx| state.set_masked(true, window, cx));
        let terminal_size = Self::size_for_window(window);
        let (profile_store, profiles, profiles_ready, profile_message) =
            match ProfileStore::discover() {
                Ok(store) => match store.load() {
                    Ok(profiles) => (Some(store), profiles, true, String::new()),
                    Err(error) => (Some(store), Vec::new(), false, error),
                },
                Err(error) => (None, Vec::new(), false, error),
            };
        let bounds_subscription = cx.observe_window_bounds(window, |this, window, cx| {
            this.resize_terminal(window, cx);
        });

        Self {
            profile_name,
            profile_search,
            host,
            port,
            username,
            password,
            status: "Ready".to_owned(),
            sessions: Vec::new(),
            active_session_id: None,
            next_session_id: 1,
            terminal_size,
            terminal_bounds: Rc::default(),
            terminal_focus: cx.focus_handle(),
            profiles,
            profile_store,
            profiles_ready,
            selected_profile: None,
            confirm_delete: false,
            profile_message,
            _bounds_subscription: bounds_subscription,
        }
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

    fn size_for_window(window: &Window) -> TerminalSize {
        let bounds = window.bounds();
        TerminalSize::for_window(
            bounds.size.width / px(1.0) - HOSTS_SIDEBAR_WIDTH_PX,
            bounds.size.height / px(1.0) - EXTRA_WORKSPACE_CHROME_PX,
        )
    }

    fn resize_terminal(&mut self, window: &Window, cx: &mut Context<Self>) {
        let size = Self::size_for_window(window);
        self.terminal_size = size;
        let mut resize_session_ids = Vec::new();
        for session in &mut self.sessions {
            if session.terminal_size == size {
                continue;
            }
            session.terminal_size = size;
            if session.command_tx.is_some() {
                resize_session_ids.push(session.id);
            }
        }
        for id in resize_session_ids {
            if let Err(error) =
                enqueue_session_request(&self.sessions, id, TerminalRequest::Resize(size))
                && let Some(session) = self.session_mut(id)
            {
                session.status = format!("Terminal resize queue is unavailable: {error}");
            }
        }
        cx.notify();
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

    fn new_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        Self::set_input_value(&self.profile_name, "", window, cx);
        Self::set_input_value(&self.host, "", window, cx);
        Self::set_input_value(&self.port, "22", window, cx);
        Self::set_input_value(&self.username, "", window, cx);
        Self::set_input_value(&self.password, "", window, cx);
        self.selected_profile = None;
        self.confirm_delete = false;
        self.profile_message.clear();
        cx.notify();
    }

    fn load_profile(&mut self, profile: &HostProfile, window: &mut Window, cx: &mut Context<Self>) {
        Self::set_input_value(&self.profile_name, &profile.name, window, cx);
        Self::set_input_value(&self.host, &profile.host, window, cx);
        Self::set_input_value(&self.port, &profile.port.to_string(), window, cx);
        Self::set_input_value(&self.username, &profile.username, window, cx);
        Self::set_input_value(&self.password, "", window, cx);
        self.selected_profile = Some(profile.name.clone());
        self.confirm_delete = false;
        self.profile_message = format!("Loaded {}. Enter the password to connect.", profile.name);
        cx.notify();
    }

    fn save_profile(&mut self, cx: &mut Context<Self>) {
        if !self.profiles_ready {
            self.profile_message =
                "Host profiles could not be loaded; fix the profile file before saving.".to_owned();
            cx.notify();
            return;
        }

        let Some(store) = self.profile_store.as_ref() else {
            self.profile_message = "Host profile storage is unavailable.".to_owned();
            cx.notify();
            return;
        };
        let name = self.profile_name.read(cx).value().trim().to_owned();
        let host = self.host.read(cx).value().trim().to_owned();
        let port = self.port.read(cx).value().parse::<u16>();
        let username = self.username.read(cx).value().trim().to_owned();
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
        };
        let mut profiles = self.profiles.clone();
        if let Some(selected) = &self.selected_profile {
            profiles.retain(|profile| !profile.name.eq_ignore_ascii_case(selected));
        }
        profiles.push(profile);
        profiles.sort_by_key(|profile| profile.name.to_lowercase());
        if let Err(error) = store.save(&profiles) {
            self.profile_message = error;
            cx.notify();
            return;
        }

        self.profiles = profiles;
        self.selected_profile = Some(name.clone());
        self.confirm_delete = false;
        self.profile_message = format!("Saved {name}.");
        cx.notify();
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
        let Some(store) = self.profile_store.as_ref() else {
            self.profile_message = "Host profile storage is unavailable.".to_owned();
            cx.notify();
            return;
        };
        let mut profiles = self.profiles.clone();
        profiles.retain(|profile| !profile.name.eq_ignore_ascii_case(&selected));
        if let Err(error) = store.save(&profiles) {
            self.profile_message = error;
            cx.notify();
            return;
        }

        self.profiles = profiles;
        self.new_profile(window, cx);
        self.profile_message = format!("Deleted {selected}.");
        cx.notify();
    }

    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let host = self.host.read(cx).value().trim().to_owned();
        let port = self.port.read(cx).value().parse::<u16>();
        let username = self.username.read(cx).value().trim().to_owned();
        let requested_title = self.profile_name.read(cx).value().trim().to_owned();
        let password = Zeroizing::new(self.password.read(cx).text().to_string());
        self.password
            .update(cx, |state, cx| state.set_value("", window, cx));

        let port = match port {
            Ok(port) if port > 0 => port,
            _ => {
                self.profile_message = "Port must be between 1 and 65535.".to_owned();
                cx.notify();
                return;
            }
        };
        if host.trim().is_empty() || username.trim().is_empty() || password.is_empty() {
            self.profile_message = "Host, user, and password are required.".to_owned();
            cx.notify();
            return;
        }

        let id = self.next_session_id;
        self.next_session_id = self.next_session_id.saturating_add(1);
        let title = if requested_title.is_empty() {
            format!("{username}@{host}")
        } else {
            requested_title
        };
        let status = format!("Checking host key and connecting to {host}:{port}...");
        let spec = ConnectionSpec {
            host,
            port,
            username: username.clone(),
            password,
            terminal_size: self.terminal_size,
        };
        let (event_tx, mut event_rx) = mpsc::channel(16);
        match ssh::spawn_connection(spec, event_tx) {
            Ok((command_tx, cancel_tx)) => {
                self.sessions.push(SshSession {
                    id,
                    title,
                    host: self.host.read(cx).value().trim().to_owned(),
                    port,
                    username,
                    status,
                    frame: None,
                    terminal_message: "Connecting...".to_owned(),
                    terminal_size: self.terminal_size,
                    scroll_remainder: 0.0,
                    mouse_position: None,
                    command_tx: Some(command_tx),
                    cancel_tx: Some(cancel_tx),
                    state: SessionState::Connecting,
                });
                self.active_session_id = Some(id);
            }
            Err(error) => {
                self.profile_message = format!("Could not start SSH runtime: {error}");
                cx.notify();
                return;
            }
        }

        cx.spawn(async move |this, cx| {
            while let Some(event) = event_rx.recv().await {
                let result = this.update(cx, |view, cx| {
                    if apply_session_event(&mut view.sessions, id, event) {
                        cx.notify();
                    }
                });
                if result.is_err() {
                    break;
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
        self.profile_message.clear();
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn select_session(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.session(id).is_none() {
            return;
        }
        self.active_session_id = Some(id);
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn disconnect_session(&mut self, id: u64, cx: &mut Context<Self>) {
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
        if !remove_session(&mut self.sessions, &mut self.active_session_id, id) {
            return;
        }
        if self.active_session_id.is_none() {
            self.status = "Ready".to_owned();
        }
        window.focus(&self.terminal_focus);
        cx.notify();
    }

    fn send_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(session_id) = self.active_session_id else {
            return;
        };
        if self
            .session(session_id)
            .is_none_or(|session| session.state != SessionState::Connected)
        {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        let key_name = event.keystroke.key.to_ascii_lowercase();
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

    fn terminal_position(&self, position: Point<Pixels>) -> Option<TerminalPosition> {
        let bounds = self.terminal_bounds.borrow().clone()?;
        let size = self
            .active_session()
            .map_or(self.terminal_size, |session| session.terminal_size);
        let column = (((position.x - bounds.origin.x) / px(1.0)) / CELL_WIDTH_PX)
            .floor()
            .clamp(0.0, f32::from(size.columns.saturating_sub(1))) as u16;
        let row = (((position.y - bounds.origin.y) / px(1.0)) / CELL_HEIGHT_PX)
            .floor()
            .clamp(0.0, f32::from(size.rows.saturating_sub(1))) as u16;
        Some(TerminalPosition { row, column })
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.terminal_focus);
        let Some(session_id) = self.active_session_id else {
            return;
        };
        if self.session(session_id).is_none_or(|session| {
            session.state != SessionState::Connected || session.frame.is_none()
        }) {
            return;
        }
        let Some(button) = terminal_mouse_button(event.button) else {
            return;
        };
        let Some(position) = self.terminal_position(event.position) else {
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

    fn mouse_up(&mut self, event: &gpui::MouseUpEvent, cx: &mut Context<Self>) {
        let Some(session_id) = self.active_session_id else {
            return;
        };
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
        let Some(position) = self.terminal_position(event.position).or(previous_position) else {
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

    fn mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(session_id) = self.active_session_id else {
            return;
        };
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
        let Some(position) = self.terminal_position(event.position) else {
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

    fn scroll_terminal(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let Some(session_id) = self.active_session_id else {
            return;
        };
        let Some(session) = self.session_mut(session_id) else {
            return;
        };
        if session.state != SessionState::Connected || session.command_tx.is_none() {
            return;
        }
        let delta_y = match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => delta.y / px(CELL_HEIGHT_PX),
        };
        let accumulated = session.scroll_remainder - delta_y;
        let rows = accumulated.trunc() as i32;
        session.scroll_remainder = accumulated - rows as f32;
        cx.stop_propagation();
        if rows == 0 {
            return;
        }
        let Some(position) = self.terminal_position(event.position) else {
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

fn render_terminal_run(run: &TerminalRun) -> impl IntoElement {
    let mut cell_run = div()
        .flex_shrink_0()
        .w(px(CELL_WIDTH_PX * run.width_cells as f32))
        .h(px(CELL_HEIGHT_PX))
        .font_family("monospace")
        .text_size(px(13.))
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

fn labeled_input(
    label: &'static str,
    input: &Entity<InputState>,
    masked: bool,
) -> impl IntoElement {
    let input = if masked {
        TextInput::new(input).mask_toggle()
    } else {
        TextInput::new(input)
    };
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgb(0x5b646b))
                .child(label),
        )
        .child(input)
}

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let search = self.profile_search.read(cx).value().trim().to_lowercase();
        let mut profile_list = div()
            .id("host-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(4.))
            .overflow_y_scroll();
        let mut matching_profiles = 0;
        for (index, profile) in self.profiles.iter().enumerate() {
            let search_text = format!(
                "{} {} {} {}",
                profile.name, profile.username, profile.host, profile.port
            )
            .to_lowercase();
            if !search_text.contains(&search) {
                continue;
            }
            matching_profiles += 1;
            let is_selected = self.selected_profile.as_deref() == Some(&profile.name);
            let selected_profile = profile.clone();
            let button = Button::new(("saved-host", index)).label(profile.name.clone());
            let button = if is_selected {
                button.primary()
            } else {
                button
            };
            profile_list = profile_list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(button.on_click(cx.listener(move |this, _, window, cx| {
                        this.load_profile(&selected_profile, window, cx);
                    })))
                    .child(
                        div()
                            .px(px(10.))
                            .text_size(px(11.))
                            .text_color(rgb(0x667078))
                            .child(format!(
                                "{}@{}:{}",
                                profile.username, profile.host, profile.port
                            )),
                    ),
            );
        }
        if matching_profiles == 0 {
            let empty_message = if self.profiles.is_empty() {
                "No saved hosts".to_owned()
            } else {
                "No matching hosts".to_owned()
            };
            profile_list = profile_list.child(
                div()
                    .p(px(8.))
                    .text_size(px(12.))
                    .text_color(rgb(0x667078))
                    .child(empty_message),
            );
        }

        let mut session_tabs = div()
            .id("session-tabs")
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(px(4.))
            .overflow_x_scroll();
        for session in &self.sessions {
            let id = session.id;
            let title = session.title.clone();
            let selected = self.active_session_id == Some(id);
            let tab = Button::new(("session-tab", id)).label(title);
            let tab = if selected { tab.primary() } else { tab };
            session_tabs =
                session_tabs.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .child(tab.on_click(cx.listener(move |this, _, window, cx| {
                            this.select_session(id, window, cx);
                        })))
                        .child(Button::new(("close-session", id)).label("x").on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.close_session(id, window, cx);
                            }),
                        )),
                );
        }
        if self.sessions.is_empty() {
            session_tabs = session_tabs.child(
                div()
                    .px(px(8.))
                    .text_size(px(12.))
                    .text_color(rgb(0x667078))
                    .child("No active sessions"),
            );
        }

        let terminal_bounds = Rc::clone(&self.terminal_bounds);
        let mut terminal_view = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .font_family("monospace")
            .text_size(px(13.))
            .text_color(rgb(0xe1e5e8))
            .on_children_prepainted(move |children, _, _| {
                *terminal_bounds.borrow_mut() = children.first().cloned();
            })
            .on_key_down(cx.listener(|this, event, _, cx| this.send_key(event, cx)))
            .on_scroll_wheel(cx.listener(|this, event, _, cx| this.scroll_terminal(event, cx)))
            .on_any_mouse_down(cx.listener(|this, event, window, cx| {
                this.mouse_down(event, window, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event, _, cx| this.mouse_up(event, cx)),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|this, event, _, cx| this.mouse_up(event, cx)),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, event, _, cx| this.mouse_up(event, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, event, _, cx| this.mouse_up(event, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Middle,
                cx.listener(|this, event, _, cx| this.mouse_up(event, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Right,
                cx.listener(|this, event, _, cx| this.mouse_up(event, cx)),
            )
            .on_mouse_move(cx.listener(|this, event, _, cx| this.mouse_move(event, cx)));

        let active_frame = self
            .active_session()
            .and_then(|session| session.frame.as_ref());
        if let Some(frame) = active_frame {
            let mut grid = div().flex().flex_col().flex_1().min_h_0();
            for row in &frame.rows {
                let mut rendered_row = div().flex().h(px(CELL_HEIGHT_PX)).flex_shrink_0();
                for run in row {
                    rendered_row = rendered_row.child(render_terminal_run(run));
                }
                grid = grid.child(rendered_row);
            }
            terminal_view = terminal_view.child(grid);
        } else {
            let message = self
                .active_session()
                .map(|session| session.terminal_message.clone())
                .unwrap_or_default();
            terminal_view = terminal_view.child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .text_color(rgb(0x859099))
                    .child(message),
            );
        }

        let active_session = self.active_session();
        let status_color = match active_session.map(|session| session.state) {
            Some(SessionState::Disconnected) | None => rgb(0x5b646b),
            Some(SessionState::Connecting) => rgb(0x986c12),
            Some(SessionState::Connected) => rgb(0x28744d),
            Some(SessionState::Disconnecting) => rgb(0x9a5a24),
        };
        let status_text = active_session
            .map(|session| session.status.clone())
            .unwrap_or_else(|| self.status.clone());
        let identity = active_session
            .map(|session| format!("{}@{}:{}", session.username, session.host, session.port))
            .unwrap_or_default();
        let terminal_dimensions = active_session
            .map(|session| {
                format!(
                    "{} x {}",
                    session.terminal_size.columns, session.terminal_size.rows
                )
            })
            .unwrap_or_default();

        let mut actions = div().flex().items_center().gap(px(6.)).child(
            Button::new("save-profile")
                .label("Save")
                .on_click(cx.listener(|this, _, _, cx| this.save_profile(cx))),
        );
        if self.selected_profile.is_some() {
            if self.confirm_delete {
                actions = actions
                    .child(
                        Button::new("confirm-delete-profile")
                            .label("Confirm")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.delete_profile(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("cancel-delete-profile")
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirm_delete = false;
                                cx.notify();
                            })),
                    );
            } else {
                actions = actions.child(
                    Button::new("delete-profile")
                        .label("Delete")
                        .on_click(cx.listener(|this, _, _, cx| this.request_profile_delete(cx))),
                );
            }
        }
        if let Some(session) = active_session
            && matches!(
                session.state,
                SessionState::Connecting | SessionState::Connected
            )
        {
            let active_id = session.id;
            let label = if session.state == SessionState::Connecting {
                "Cancel"
            } else {
                "Disconnect"
            };
            actions = actions.child(Button::new("disconnect-active").label(label).on_click(
                cx.listener(move |this, _, _, cx| {
                    this.disconnect_session(active_id, cx);
                }),
            ));
        }
        actions = actions.child(Button::new("connect").primary().label("Connect").on_click(
            cx.listener(|this, _, window, cx| {
                this.connect(window, cx);
            }),
        ));

        div()
            .flex()
            .size_full()
            .bg(rgb(0xf3f5f6))
            .text_color(rgb(0x20262b))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(HOSTS_SIDEBAR_WIDTH_PX))
                    .h_full()
                    .gap(px(8.))
                    .p(px(10.))
                    .bg(rgb(0xe8ebed))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .h(px(32.))
                            .child(div().text_size(px(16.)).child("dshell"))
                            .child(Button::new("new-host-profile").label("New").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.new_profile(window, cx);
                                }),
                            )),
                    )
                    .child(
                        div()
                            .pt(px(4.))
                            .text_size(px(11.))
                            .text_color(rgb(0x59636a))
                            .child("SAVED HOSTS"),
                    )
                    .child(TextInput::new(&self.profile_search))
                    .child(profile_list),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .gap(px(0.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .px(px(10.))
                            .py(px(8.))
                            .bg(rgb(0xf3f5f6))
                            .child(
                                div()
                                    .flex()
                                    .items_end()
                                    .gap(px(6.))
                                    .child(div().w(px(150.)).child(labeled_input(
                                        "Name",
                                        &self.profile_name,
                                        false,
                                    )))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(140.))
                                            .child(labeled_input("Host", &self.host, false)),
                                    )
                                    .child(
                                        div()
                                            .w(px(76.))
                                            .child(labeled_input("Port", &self.port, false)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_end()
                                    .gap(px(6.))
                                    .child(div().w(px(150.)).child(labeled_input(
                                        "Username",
                                        &self.username,
                                        false,
                                    )))
                                    .child(div().flex_1().min_w(px(130.)).child(labeled_input(
                                        "Password",
                                        &self.password,
                                        true,
                                    )))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(11.))
                                            .text_color(rgb(0x59636a))
                                            .child(self.profile_message.clone()),
                                    )
                                    .child(actions),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .h(px(38.))
                            .gap(px(4.))
                            .px(px(8.))
                            .bg(rgb(0xe4e7e9))
                            .child(session_tabs),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .bg(rgb(0x0b0f13))
                            .p(px(8.))
                            .child(terminal_view.track_focus(&self.terminal_focus).size_full()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .h(px(26.))
                            .px(px(10.))
                            .bg(rgb(0xdfe3e5))
                            .text_size(px(11.))
                            .child(div().text_color(status_color).child(status_text))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .text_color(rgb(0x59636a))
                                    .child(identity)
                                    .child(terminal_dimensions),
                            ),
                    ),
            )
    }
}

fn main() {
    Application::new().run(|cx| {
        gpui_component::init(cx);
        cx.open_window(WindowOptions::default(), |window, cx| {
            let view = cx.new(|cx| AppView::new(window, cx));
            cx.new(|cx| Root::new(view.into(), window, cx))
        })
        .expect("failed to open dshell window");
        cx.activate(true);
    });
}
