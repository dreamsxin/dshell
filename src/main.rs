mod ssh;
mod terminal;

use gpui::{prelude::*, *};
use gpui_component::{
    Root,
    button::{Button, ButtonVariants},
    input::{InputState, TextInput},
};
use ssh::{ConnectionSpec, UiEvent};
use terminal::{
    CELL_HEIGHT_PX, CELL_WIDTH_PX, TerminalFrame, TerminalKeyInput, TerminalRequest, TerminalRun,
    TerminalSize,
};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

struct AppView {
    host: Entity<InputState>,
    port: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    status: String,
    frame: Option<TerminalFrame>,
    terminal_message: String,
    terminal_size: TerminalSize,
    scroll_remainder: f32,
    terminal_focus: FocusHandle,
    command_tx: Option<mpsc::Sender<TerminalRequest>>,
    _bounds_subscription: Subscription,
}

impl AppView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let host = cx.new(|cx| InputState::new(window, cx).placeholder("server.example.com"));
        let port = cx.new(|cx| InputState::new(window, cx).default_value("22"));
        let username = cx.new(|cx| InputState::new(window, cx).placeholder("user"));
        let password = cx.new(|cx| InputState::new(window, cx).placeholder("Password"));
        password.update(cx, |state, cx| state.set_masked(true, window, cx));
        let terminal_size = Self::size_for_window(window);
        let bounds_subscription = cx.observe_window_bounds(window, |this, window, cx| {
            this.resize_terminal(window, cx);
        });

        Self {
            host,
            port,
            username,
            password,
            status: "Disconnected".to_owned(),
            frame: None,
            terminal_message: "Connect to a server with a verified host key.".to_owned(),
            terminal_size,
            scroll_remainder: 0.0,
            terminal_focus: cx.focus_handle(),
            command_tx: None,
            _bounds_subscription: bounds_subscription,
        }
    }

    fn size_for_window(window: &Window) -> TerminalSize {
        let bounds = window.bounds();
        TerminalSize::for_window(bounds.size.width / px(1.0), bounds.size.height / px(1.0))
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

    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_tx.is_some() {
            self.status = "A session is already active.".to_owned();
            cx.notify();
            return;
        }

        let host = self.host.read(cx).value().to_string();
        let port = self.port.read(cx).value().parse::<u16>();
        let username = self.username.read(cx).value().to_string();
        let password = Zeroizing::new(self.password.read(cx).text().to_string());
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
            Ok(command_tx) => self.command_tx = Some(command_tx),
            Err(error) => {
                self.status = format!("Could not start SSH runtime: {error}");
                cx.notify();
                return;
            }
        }

        cx.spawn(async move |this, cx| {
            while let Some(event) = event_rx.recv().await {
                let result = this.update(cx, |view, cx| {
                    match event {
                        UiEvent::Status(status) => view.status = status,
                        UiEvent::Screen(frame) => view.frame = Some(frame),
                        UiEvent::Failed(error) => {
                            view.status = error;
                            view.command_tx = None;
                        }
                        UiEvent::Disconnected => {
                            view.status = "Disconnected".to_owned();
                            view.command_tx = None;
                        }
                    }
                    cx.notify();
                });
                if result.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn send_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(command_tx) = &self.command_tx else {
            return;
        };
        let modifiers = event.keystroke.modifiers;
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

    fn scroll_terminal(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
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

        if let Err(error) = command_tx.try_send(TerminalRequest::Scroll(rows)) {
            self.status = format!("Terminal scroll queue is unavailable: {error}");
            cx.notify();
        }
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

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut terminal_view = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .font_family("monospace")
            .text_size(px(13.))
            .on_key_down(cx.listener(|this, event, _, cx| this.send_key(event, cx)))
            .on_scroll_wheel(cx.listener(|this, event, _, cx| this.scroll_terminal(event, cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, _| {
                    window.focus(&this.terminal_focus);
                }),
            );

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

        div()
            .flex()
            .flex_col()
            .size_full()
            .gap(px(12.))
            .p(px(16.))
            .bg(rgb(0x151a20))
            .text_color(rgb(0xe1e5e9))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(18.)).child("dshell"))
                    .child(div().text_size(px(12.)).child(self.status.clone())),
            )
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(8.))
                    .h(px(48.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .flex_1()
                            .min_w(px(160.))
                            .child("Host")
                            .child(TextInput::new(&self.host)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .w(px(72.))
                            .child("Port")
                            .child(TextInput::new(&self.port)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .flex_1()
                            .min_w(px(110.))
                            .child("User")
                            .child(TextInput::new(&self.username)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .flex_1()
                            .min_w(px(110.))
                            .child("Password")
                            .child(TextInput::new(&self.password).mask_toggle()),
                    )
                    .child(Button::new("connect").primary().label("Connect").on_click(
                        cx.listener(|this, _, window, cx| {
                            this.connect(window, cx);
                        }),
                    )),
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
