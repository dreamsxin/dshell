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

const HOSTS_SIDEBAR_WIDTH_PX: f32 = 236.0;
const EXTRA_WORKSPACE_CHROME_PX: f32 = 88.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionState {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
}

struct AppView {
    profile_name: Entity<InputState>,
    profile_search: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    status: String,
    frame: Option<TerminalFrame>,
    terminal_message: String,
    terminal_size: TerminalSize,
    scroll_remainder: f32,
    terminal_bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
    mouse_position: Option<TerminalPosition>,
    terminal_focus: FocusHandle,
    command_tx: Option<mpsc::Sender<TerminalRequest>>,
    cancel_tx: Option<watch::Sender<bool>>,
    session_state: SessionState,
    session_generation: u64,
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
            status: "Disconnected".to_owned(),
            frame: None,
            terminal_message: "Connect to a server with a verified host key.".to_owned(),
            terminal_size,
            scroll_remainder: 0.0,
            terminal_bounds: Rc::default(),
            mouse_position: None,
            terminal_focus: cx.focus_handle(),
            command_tx: None,
            cancel_tx: None,
            session_state: SessionState::Disconnected,
            session_generation: 0,
            profiles,
            profile_store,
            profiles_ready,
            selected_profile: None,
            confirm_delete: false,
            profile_message,
            _bounds_subscription: bounds_subscription,
        }
    }

    fn clear_terminal_session(&mut self, status: impl Into<String>, message: impl Into<String>) {
        self.status = status.into();
        self.frame = None;
        self.terminal_message = message.into();
        self.mouse_position = None;
        self.scroll_remainder = 0.0;
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
        if size == self.terminal_size {
            return;
        }

        self.terminal_size = size;
        if let Some(command_tx) = &self.command_tx
            && let Err(error) = command_tx.try_send(TerminalRequest::Resize(size))
        {
            self.status = format!("Terminal resize queue is unavailable: {error}");
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

    fn require_disconnected(&mut self, cx: &mut Context<Self>) -> bool {
        if self.session_state == SessionState::Disconnected {
            return true;
        }
        self.profile_message =
            "Disconnect the active session before changing host profiles.".to_owned();
        cx.notify();
        false
    }

    fn new_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.require_disconnected(cx) {
            return;
        }
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
        if !self.require_disconnected(cx) {
            return;
        }
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
        if !self.require_disconnected(cx) {
            return;
        }
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
        if self.require_disconnected(cx) && self.selected_profile.is_some() {
            self.confirm_delete = true;
            self.profile_message.clear();
            cx.notify();
        }
    }

    fn delete_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.require_disconnected(cx) {
            return;
        }
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
        if self.session_state != SessionState::Disconnected {
            self.status = "A session is already active.".to_owned();
            cx.notify();
            return;
        }

        let host = self.host.read(cx).value().to_string();
        let port = self.port.read(cx).value().parse::<u16>();
        let username = self.username.read(cx).value().to_string();
        let password = Zeroizing::new(self.password.read(cx).text().to_string());
        self.mouse_position = None;
        self.password
            .update(cx, |state, cx| state.set_value("", window, cx));

        let port = match port {
            Ok(port) if port > 0 => port,
            _ => {
                self.status = "Port must be between 1 and 65535.".to_owned();
                cx.notify();
                return;
            }
        };
        if host.trim().is_empty() || username.trim().is_empty() || password.is_empty() {
            self.status = "Host, user, and password are required.".to_owned();
            cx.notify();
            return;
        }

        self.status = format!("Checking host key and connecting to {host}:{port}...");
        self.frame = None;
        self.terminal_message.clear();
        let spec = ConnectionSpec {
            host,
            port,
            username,
            password,
            terminal_size: self.terminal_size,
        };
        let (event_tx, mut event_rx) = mpsc::channel(16);
        match ssh::spawn_connection(spec, event_tx) {
            Ok((command_tx, cancel_tx)) => {
                self.command_tx = Some(command_tx);
                self.cancel_tx = Some(cancel_tx);
                self.session_state = SessionState::Connecting;
            }
            Err(error) => {
                self.status = format!("Could not start SSH runtime: {error}");
                cx.notify();
                return;
            }
        }

        self.session_generation = self.session_generation.wrapping_add(1);
        let session_generation = self.session_generation;
        cx.spawn(async move |this, cx| {
            while let Some(event) = event_rx.recv().await {
                let result = this.update(cx, |view, cx| {
                    if view.session_generation != session_generation {
                        return;
                    }
                    if view.session_state == SessionState::Disconnecting
                        && matches!(&event, UiEvent::Status(_) | UiEvent::Screen(_))
                    {
                        return;
                    }
                    match event {
                        UiEvent::Status(status) => {
                            view.status = status;
                            view.session_state = SessionState::Connected;
                        }
                        UiEvent::Screen(frame) => {
                            view.frame = Some(frame);
                            view.terminal_message.clear();
                            view.session_state = SessionState::Connected;
                        }
                        UiEvent::Failed(error) => {
                            view.clear_terminal_session(
                                error,
                                "The SSH session ended. Verify the status and connect again.",
                            );
                            view.command_tx = None;
                            view.cancel_tx = None;
                            view.session_state = SessionState::Disconnected;
                        }
                        UiEvent::Disconnected => {
                            view.clear_terminal_session(
                                "Disconnected",
                                "Disconnected. Verify the host details and connect again.",
                            );
                            view.command_tx = None;
                            view.cancel_tx = None;
                            view.session_state = SessionState::Disconnected;
                        }
                    }
                    cx.notify();
                });
                if result.is_err() {
                    break;
                }
            }
            let _ = this.update(cx, |view, cx| {
                if view.session_generation == session_generation
                    && view.session_state != SessionState::Disconnected
                {
                    view.clear_terminal_session(
                        "Session event stream closed",
                        "The SSH session ended unexpectedly. Connect again.",
                    );
                    view.command_tx = None;
                    view.cancel_tx = None;
                    view.session_state = SessionState::Disconnected;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn disconnect(&mut self, cx: &mut Context<Self>) {
        if self.session_state == SessionState::Disconnected
            || self.session_state == SessionState::Disconnecting
        {
            return;
        }

        if let Some(cancel_tx) = &self.cancel_tx {
            cancel_tx.send_replace(true);
        }
        self.session_state = SessionState::Disconnecting;
        self.frame = None;
        self.terminal_message = "Disconnecting from the SSH session...".to_owned();
        self.mouse_position = None;
        self.scroll_remainder = 0.0;
        self.status = "Disconnecting...".to_owned();
        cx.notify();
    }

    fn send_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if self.session_state != SessionState::Connected {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        let key_name = event.keystroke.key.to_ascii_lowercase();
        if self.command_tx.is_some() && modifiers.shift && (modifiers.control || modifiers.platform)
        {
            match key_name.as_str() {
                "c" => {
                    cx.stop_propagation();
                    if let Some(text) = self
                        .frame
                        .as_ref()
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

        let Some(command_tx) = &self.command_tx else {
            return;
        };
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

        match command_tx.try_send(TerminalRequest::Key(key)) {
            Ok(()) => cx.stop_propagation(),
            Err(error) => {
                self.status = format!("Terminal input queue is unavailable: {error}");
                cx.notify();
            }
        }
    }

    fn terminal_position(&self, position: Point<Pixels>) -> Option<TerminalPosition> {
        let bounds = self.terminal_bounds.borrow().clone()?;
        let column = (((position.x - bounds.origin.x) / px(1.0)) / CELL_WIDTH_PX)
            .floor()
            .clamp(0.0, f32::from(self.terminal_size.columns.saturating_sub(1)))
            as u16;
        let row = (((position.y - bounds.origin.y) / px(1.0)) / CELL_HEIGHT_PX)
            .floor()
            .clamp(0.0, f32::from(self.terminal_size.rows.saturating_sub(1)))
            as u16;
        Some(TerminalPosition { row, column })
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.terminal_focus);
        if self.session_state != SessionState::Connected || self.frame.is_none() {
            return;
        }
        let Some(button) = terminal_mouse_button(event.button) else {
            return;
        };
        let Some(position) = self.terminal_position(event.position) else {
            return;
        };
        self.mouse_position = Some(position);
        self.send_terminal_request(
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
        if self.session_state != SessionState::Connected {
            return;
        }
        let Some(button) = terminal_mouse_button(event.button) else {
            return;
        };
        let Some(position) = self
            .terminal_position(event.position)
            .or(self.mouse_position)
        else {
            return;
        };
        self.mouse_position = Some(position);
        self.send_terminal_request(
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
        if self.session_state != SessionState::Connected {
            return;
        }
        let tracking_mode = self
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
        if self.mouse_position == Some(position) {
            return;
        }
        self.mouse_position = Some(position);
        self.send_terminal_request(
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
        request: TerminalRequest,
        action: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(command_tx) = &self.command_tx
            && let Err(error) = command_tx.try_send(request)
        {
            self.status = format!("{action} queue is unavailable: {error}");
            cx.notify();
        }
    }

    fn scroll_terminal(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if self.session_state != SessionState::Connected {
            return;
        }
        let Some(command_tx) = &self.command_tx else {
            return;
        };
        let delta_y = match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => delta.y / px(CELL_HEIGHT_PX),
        };
        let accumulated = self.scroll_remainder - delta_y;
        let rows = accumulated.trunc() as i32;
        self.scroll_remainder = accumulated - rows as f32;
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
        if let Err(error) = command_tx.try_send(request) {
            self.status = format!("Terminal scroll queue is unavailable: {error}");
            cx.notify();
        }
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
                .text_color(rgb(0xaeb7c0))
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
            .gap(px(6.))
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
                    .gap(px(2.))
                    .child(button.on_click(cx.listener(move |this, _, window, cx| {
                        this.load_profile(&selected_profile, window, cx);
                    })))
                    .child(
                        div()
                            .px(px(8.))
                            .text_size(px(11.))
                            .text_color(rgb(0x9ba6b0))
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
                    .text_color(rgb(0x9ba6b0))
                    .child(empty_message),
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

        if let Some(frame) = &self.frame {
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
            terminal_view = terminal_view.child(self.terminal_message.clone());
        }

        let status_color = match self.session_state {
            SessionState::Disconnected => rgb(0xaeb7c0),
            SessionState::Connecting => rgb(0xe6c36a),
            SessionState::Connected => rgb(0x77c89a),
            SessionState::Disconnecting => rgb(0xe6a76a),
        };
        let mut actions = div().flex().items_center().gap(px(8.)).child(
            Button::new("save-profile")
                .label("Save Host")
                .on_click(cx.listener(|this, _, _, cx| this.save_profile(cx))),
        );
        if self.selected_profile.is_some() {
            if self.confirm_delete {
                actions = actions
                    .child(
                        Button::new("confirm-delete-profile")
                            .label("Confirm Delete")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.delete_profile(window, cx)),
                            ),
                    )
                    .child(Button::new("cancel-delete-profile").label("Keep").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.confirm_delete = false;
                            cx.notify();
                        }),
                    ));
            } else {
                actions = actions.child(
                    Button::new("delete-profile")
                        .label("Delete Host")
                        .on_click(cx.listener(|this, _, _, cx| this.request_profile_delete(cx))),
                );
            }
        }
        actions = actions.child(match self.session_state {
            SessionState::Disconnected => Button::new("connect")
                .primary()
                .label("Connect")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.connect(window, cx);
                })),
            SessionState::Connecting => Button::new("cancel-connection")
                .label("Cancel")
                .on_click(cx.listener(|this, _, _, cx| this.disconnect(cx))),
            SessionState::Connected => Button::new("disconnect")
                .label("Disconnect")
                .on_click(cx.listener(|this, _, _, cx| this.disconnect(cx))),
            SessionState::Disconnecting => Button::new("disconnecting").label("Disconnecting..."),
        });

        div()
            .flex()
            .size_full()
            .bg(rgb(0x151a20))
            .text_color(rgb(0xe1e5e9))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(HOSTS_SIDEBAR_WIDTH_PX))
                    .h_full()
                    .gap(px(10.))
                    .p(px(12.))
                    .bg(rgb(0x20262d))
                    .child(div().text_size(px(18.)).child("dshell"))
                    .child(
                        Button::new("new-host-profile")
                            .label("New Host")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.new_profile(window, cx);
                            })),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(0xaeb7c0))
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
                    .gap(px(8.))
                    .p(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .h(px(24.))
                            .child(div().text_size(px(15.)).child("Connection"))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(status_color)
                                    .child(self.status.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_end()
                                    .gap(px(8.))
                                    .child(div().flex_1().min_w(px(110.)).child(labeled_input(
                                        "Profile",
                                        &self.profile_name,
                                        false,
                                    )))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(170.))
                                            .child(labeled_input("Host", &self.host, false)),
                                    )
                                    .child(
                                        div()
                                            .w(px(78.))
                                            .child(labeled_input("Port", &self.port, false)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_end()
                                    .gap(px(8.))
                                    .child(div().flex_1().min_w(px(120.)).child(labeled_input(
                                        "Username",
                                        &self.username,
                                        false,
                                    )))
                                    .child(div().flex_1().min_w(px(160.)).child(labeled_input(
                                        "Password",
                                        &self.password,
                                        true,
                                    ))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(11.))
                                            .text_color(rgb(0xaeb7c0))
                                            .child(self.profile_message.clone()),
                                    )
                                    .child(actions),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .bg(rgb(0x0b0f13))
                            .p(px(12.))
                            .child(terminal_view.track_focus(&self.terminal_focus).size_full()),
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
