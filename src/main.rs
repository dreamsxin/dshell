mod ssh;
mod terminal;

use gpui::{prelude::*, *};
use gpui_component::{
    Root,
    button::{Button, ButtonVariants},
    input::{InputState, TextInput},
};
use ssh::{ConnectionSpec, UiEvent};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

struct AppView {
    host: Entity<InputState>,
    port: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    command: Entity<InputState>,
    status: String,
    screen: String,
    command_tx: Option<mpsc::Sender<String>>,
}

impl AppView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let host = cx.new(|cx| InputState::new(window, cx).placeholder("server.example.com"));
        let port = cx.new(|cx| InputState::new(window, cx).default_value("22"));
        let username = cx.new(|cx| InputState::new(window, cx).placeholder("user"));
        let password = cx.new(|cx| InputState::new(window, cx).placeholder("Password"));
        password.update(cx, |state, cx| state.set_masked(true, window, cx));
        let command = cx.new(|cx| InputState::new(window, cx).placeholder("Enter a command"));

        Self {
            host,
            port,
            username,
            password,
            command,
            status: "Disconnected".to_owned(),
            screen: "Connect to a server with a verified host key.".to_owned(),
            command_tx: None,
        }
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
        self.screen.clear();
        let spec = ConnectionSpec {
            host,
            port,
            username,
            password,
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
                        UiEvent::Screen(screen) => view.screen = screen,
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

    fn send_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(command_tx) = &self.command_tx else {
            self.status = "Connect before sending a command.".to_owned();
            cx.notify();
            return;
        };
        let mut command = self.command.read(cx).value().to_string();
        if command.is_empty() {
            return;
        }
        command.push('\r');
        match command_tx.try_send(command) {
            Ok(()) => {
                self.command
                    .update(cx, |state, cx| state.set_value("", window, cx));
            }
            Err(error) => self.status = format!("Input queue is unavailable: {error}"),
        }
        cx.notify();
    }
}

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .w(px(240.))
                            .child("Host")
                            .child(TextInput::new(&self.host)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .w(px(76.))
                            .child("Port")
                            .child(TextInput::new(&self.port)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .w(px(150.))
                            .child("User")
                            .child(TextInput::new(&self.username)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .w(px(180.))
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
                    .gap(px(8.))
                    .bg(rgb(0x0b0f13))
                    .p(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .font_family("monospace")
                            .text_size(px(13.))
                            .child(self.screen.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().flex_1().child(TextInput::new(&self.command)))
                            .child(Button::new("send").label("Send").on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.send_command(window, cx);
                                },
                            ))),
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
