use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::AtomicBool},
    thread,
    time::Duration,
};

use anyhow::{Context as _, Result, anyhow, bail};
use russh::{
    ChannelMsg, Disconnect,
    client::{self, AuthResult, Handler, KeyboardInteractiveAuthResponse, Prompt},
    keys::{
        HashAlg, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate, check_known_hosts_path,
        load_secret_key,
    },
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    sync::{mpsc, watch},
    task::JoinHandle,
    time::{MissedTickBehavior, interval, timeout},
};
use zeroize::Zeroizing;

use crate::forward::{
    self, AcceptedConnection, ForwardCommand, ForwardEvent, ForwardKind, ForwardRuleSpec,
    RemoteTarget, RemoteTargetMap,
};
use crate::log;
use crate::sftp::{self, SftpCommand, SftpEvent};
use crate::terminal::{
    CursorStyle, MouseTrackingMode, TerminalEmulator, TerminalFrame, TerminalMouseButton,
    TerminalMouseEvent, TerminalRequest, TerminalSearchResult, TerminalSize, TerminalTheme,
};

pub enum Authentication {
    Password(Zeroizing<String>),
    KeyboardInteractive {
        password: Zeroizing<String>,
        interactive: bool,
    },
    PublicKey {
        identity_file: PathBuf,
        passphrase: Zeroizing<String>,
    },
    Agent,
}

pub struct JumpSpec {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub authentication: Authentication,
}

struct LocalForwardRuntime {
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

struct RemoteForwardRuntime {
    spec: ForwardRuleSpec,
    bound_port: u16,
}

pub struct ConnectionSpec {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub authentication: Authentication,
    pub terminal_size: TerminalSize,
    pub jump: Option<JumpSpec>,
    pub keepalive_seconds: u16,
    pub theme: TerminalTheme,
    pub cursor_style: CursorStyle,
}

#[derive(Clone, Debug)]
pub struct AuthPromptInfo {
    pub prompt: String,
    pub echo: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionInfo {
    pub host_key_algorithm: String,
    pub host_key_fingerprint: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServerMetrics {
    pub os: String,
    pub kernel: String,
    pub load_1m: Option<f32>,
    pub load_5m: Option<f32>,
    pub load_15m: Option<f32>,
    pub memory_total_kib: Option<u64>,
    pub memory_available_kib: Option<u64>,
    pub disk_total_kib: Option<u64>,
    pub disk_used_kib: Option<u64>,
    pub disk_available_kib: Option<u64>,
    pub disk_use_percent: Option<u8>,
    pub uptime_seconds: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServerProcess {
    pub pid: u32,
    pub user: String,
    pub cpu_percent: f32,
    pub memory_percent: f32,
    pub state: String,
    pub command: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServerProcessActionResult {
    pub pid: u32,
    pub success: bool,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServerService {
    pub name: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    pub description: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
}

impl ServiceAction {
    pub const fn command(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Start => "Start service",
            Self::Stop => "Stop service",
            Self::Restart => "Restart service",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServerServiceActionResult {
    pub name: String,
    pub action: ServiceAction,
    pub success: bool,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServerServiceLogs {
    pub name: String,
    pub lines: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerPowerAction {
    Reboot,
    Shutdown,
}

impl ServerPowerAction {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Reboot => "Reboot server",
            Self::Shutdown => "Shut down server",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServerPowerActionResult {
    pub action: ServerPowerAction,
    pub success: bool,
    pub message: String,
}

pub enum ServerData {
    Metrics(ServerMetrics),
    Processes(Vec<ServerProcess>),
    ProcessAction(ServerProcessActionResult),
    Services(Vec<ServerService>),
    ServiceAction(ServerServiceActionResult),
    ServiceLogs(ServerServiceLogs),
    PowerAction(ServerPowerActionResult),
}

pub enum ServerDataError {
    Metrics(String),
    Processes(String),
    Services(String),
    ServiceLogs(String),
}

fn connection_info_for_key(key: &PublicKey) -> ConnectionInfo {
    ConnectionInfo {
        host_key_algorithm: key.algorithm().as_str().to_owned(),
        host_key_fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
    }
}

pub enum UiEvent {
    Status(String),
    Screen(TerminalFrame),
    Failed(String),
    Disconnected,
    LogState(Option<PathBuf>),
    ConnectionInfo(ConnectionInfo),
    ServerMetrics(ServerData),
    ServerMetricsError(ServerDataError),
    AuthPrompt {
        name: String,
        instructions: String,
        prompts: Vec<AuthPromptInfo>,
        respond_to: tokio::sync::oneshot::Sender<Vec<Zeroizing<String>>>,
    },
    SearchResult(TerminalSearchResult),
}

struct KnownHostsHandler {
    host: String,
    port: u16,
    path: PathBuf,
    remote_forwards: RemoteTargetMap,
    forward_events: Option<mpsc::UnboundedSender<ForwardEvent>>,
    events: Option<mpsc::Sender<UiEvent>>,
}

impl Handler for KnownHostsHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key = server_public_key.public_key();
        match is_known_host_key(&self.host, self.port, &key, &self.path)? {
            true => {
                if let Some(events) = &self.events {
                    let _ = events.try_send(UiEvent::ConnectionInfo(connection_info_for_key(&key)));
                }
                Ok(true)
            }
            false => Err(anyhow!(
                "Server key for {}:{} is not present in {}",
                self.host,
                self.port,
                self.path.display()
            )),
        }
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        _connected_address: &str,
        connected_port: u32,
        originator_address: &str,
        originator_port: u32,
        reply: russh::client::ChannelOpenHandle,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        let target = {
            let targets = self
                .remote_forwards
                .lock()
                .map_err(|_| anyhow!("The remote forwarding registry is poisoned"))?;
            targets.get(&connected_port).cloned()
        };
        let Some(target) = target else {
            return Ok(());
        };
        reply.accept().await;
        let peer = format!("{originator_address}:{originator_port}");
        let events = self.forward_events.clone();
        tokio::spawn(async move {
            let mut remote = channel.into_stream();
            match tokio::net::TcpStream::connect((target.host.as_str(), target.port)).await {
                Ok(mut local) => {
                    if let Some(events) = &events {
                        let _ = events.send(ForwardEvent::ConnectionOpened {
                            id: target.rule_id,
                            peer: peer.clone(),
                            target: format!("{}:{}", target.host, target.port),
                        });
                    }
                    let _ = tokio::io::copy_bidirectional(&mut local, &mut remote).await;
                    if let Some(events) = events {
                        let _ = events.send(ForwardEvent::ConnectionClosed {
                            id: target.rule_id,
                            peer,
                        });
                    }
                }
                Err(error) => {
                    if let Some(events) = events {
                        let _ = events.send(ForwardEvent::Error {
                            id: Some(target.rule_id),
                            error: format!(
                                "Remote forwarding could not reach {}:{}: {error}",
                                target.host, target.port
                            ),
                        });
                    }
                }
            }
        });
        Ok(())
    }
}

fn default_known_hosts_path() -> Result<PathBuf> {
    let home = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .context("could not determine the current user's home directory")?;
    Ok(PathBuf::from(home).join(".ssh").join("known_hosts"))
}

fn is_known_host_key(host: &str, port: u16, key: &PublicKey, path: &Path) -> Result<bool> {
    check_known_hosts_path(host, port, key, path).map_err(Into::into)
}

pub struct ConnectionHandles {
    pub terminal: mpsc::Sender<TerminalRequest>,
    pub sftp: mpsc::Sender<SftpCommand>,
    pub cancellation: watch::Sender<bool>,
    pub sftp_events: mpsc::Receiver<SftpEvent>,
    pub sftp_cancel: Arc<AtomicBool>,
    pub forwards: mpsc::Sender<ForwardCommand>,
    pub forward_events: mpsc::UnboundedReceiver<ForwardEvent>,
}

pub fn spawn_connection(
    spec: ConnectionSpec,
    events: mpsc::Sender<UiEvent>,
) -> Result<ConnectionHandles> {
    let (command_tx, command_rx) = mpsc::channel(128);
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let (sftp_command_tx, sftp_command_rx) = mpsc::channel(16);
    let (sftp_event_tx, sftp_event_rx) = mpsc::channel(32);
    let (forward_command_tx, forward_command_rx) = mpsc::channel(16);
    let (forward_event_tx, forward_event_rx) = mpsc::unbounded_channel();
    let sftp_cancel = Arc::new(AtomicBool::new(false));
    let thread_sftp_cancel = Arc::clone(&sftp_cancel);
    let error_events = events.clone();

    thread::Builder::new()
        .name("dshell-ssh-runtime".to_owned())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = error_events.blocking_send(UiEvent::Failed(format!(
                        "Could not create SSH runtime: {error}"
                    )));
                    return;
                }
            };

            if let Err(error) = runtime.block_on(run_connection(
                spec,
                command_rx,
                events.clone(),
                sftp_command_rx,
                sftp_event_tx,
                thread_sftp_cancel,
                forward_command_rx,
                forward_event_tx,
                cancel_rx,
            )) {
                let _ =
                    events.blocking_send(UiEvent::Failed(format!("Connection failed: {error}")));
            }
        })
        .context("could not start the SSH runtime thread")?;

    Ok(ConnectionHandles {
        terminal: command_tx,
        sftp: sftp_command_tx,
        cancellation: cancel_tx,
        sftp_events: sftp_event_rx,
        sftp_cancel,
        forwards: forward_command_tx,
        forward_events: forward_event_rx,
    })
}

async fn run_connection(
    spec: ConnectionSpec,
    commands: mpsc::Receiver<TerminalRequest>,
    events: mpsc::Sender<UiEvent>,
    sftp_commands: mpsc::Receiver<SftpCommand>,
    sftp_events: mpsc::Sender<SftpEvent>,
    sftp_cancel: Arc<AtomicBool>,
    forward_commands: mpsc::Receiver<ForwardCommand>,
    forward_events: mpsc::UnboundedSender<ForwardEvent>,
    cancellation: watch::Receiver<bool>,
) -> Result<()> {
    let known_hosts_path = default_known_hosts_path()?;
    run_connection_with_known_hosts(
        spec,
        commands,
        events,
        sftp_commands,
        sftp_events,
        sftp_cancel,
        forward_commands,
        forward_events,
        known_hosts_path,
        cancellation,
    )
    .await
}

async fn run_connection_with_known_hosts(
    spec: ConnectionSpec,
    mut commands: mpsc::Receiver<TerminalRequest>,
    events: mpsc::Sender<UiEvent>,
    sftp_commands: mpsc::Receiver<SftpCommand>,
    sftp_events: mpsc::Sender<SftpEvent>,
    sftp_cancel: Arc<AtomicBool>,
    mut forward_commands: mpsc::Receiver<ForwardCommand>,
    forward_events: mpsc::UnboundedSender<ForwardEvent>,
    known_hosts_path: PathBuf,
    mut cancellation: watch::Receiver<bool>,
) -> Result<()> {
    let ConnectionSpec {
        host,
        port,
        username,
        authentication,
        terminal_size,
        jump,
        keepalive_seconds,
        theme,
        cursor_style,
    } = spec;
    let remote_targets: RemoteTargetMap = Arc::new(Mutex::new(HashMap::new()));
    let (accepted_tx, mut accepted_connections) = mpsc::unbounded_channel();
    let mut local_forwards: HashMap<u64, LocalForwardRuntime> = HashMap::new();
    let mut remote_forwards: HashMap<u64, RemoteForwardRuntime> = HashMap::new();
    let mut next_forward_id = 1u64;
    let setup = async {
        let mut config = client::Config::default();
        if keepalive_seconds > 0 {
            config.keepalive_interval = Some(Duration::from_secs(u64::from(keepalive_seconds)));
        }
        let config = Arc::new(config);
        let mut session;
        let mut jump_session = None;
        if let Some(jump) = jump {
            let jump_handler = KnownHostsHandler {
                host: jump.host.clone(),
                port: jump.port,
                path: known_hosts_path.clone(),
                remote_forwards: Arc::new(Mutex::new(HashMap::new())),
                forward_events: None,
                events: None,
            };
            let mut jump_handle = client::connect(
                Arc::clone(&config),
                (jump.host.as_str(), jump.port),
                jump_handler,
            )
            .await
            .context("SSH jump host handshake failed")?;
            authenticate(
                &mut jump_handle,
                &jump.username,
                jump.authentication,
                &events,
            )
            .await
            .context("SSH jump host authentication failed")?;
            let channel = jump_handle
                .channel_open_direct_tcpip(host.clone(), u32::from(port), "127.0.0.1", 0)
                .await
                .context("The jump host could not connect to the target host")?;
            let target_handler = KnownHostsHandler {
                host: host.clone(),
                port,
                path: known_hosts_path.clone(),
                remote_forwards: Arc::clone(&remote_targets),
                forward_events: Some(forward_events.clone()),
                events: Some(events.clone()),
            };
            session =
                client::connect_stream(Arc::clone(&config), channel.into_stream(), target_handler)
                    .await
                    .context("SSH target handshake through the jump host failed")?;
            jump_session = Some(jump_handle);
        } else {
            let handler = KnownHostsHandler {
                host: host.clone(),
                port,
                path: known_hosts_path,
                remote_forwards: Arc::clone(&remote_targets),
                forward_events: Some(forward_events.clone()),
                events: Some(events.clone()),
            };
            session = client::connect(config, (host.as_str(), port), handler)
                .await
                .context("SSH handshake failed")?;
        }

        authenticate(&mut session, &username, authentication, &events).await?;

        let sftp_setup = tokio::time::timeout(Duration::from_secs(5), async {
            let channel = session.channel_open_session().await?;
            channel.request_subsystem(true, "sftp").await?;
            Ok::<_, russh::Error>(channel)
        })
        .await;
        match sftp_setup {
            Ok(Ok(channel)) => {
                tokio::spawn(sftp::run(
                    channel.into_stream(),
                    sftp_commands,
                    sftp_events,
                    sftp_cancel,
                ));
            }
            Ok(Err(error)) => {
                let _ = sftp_events
                    .send(SftpEvent::Error(format!(
                        "Could not open the SFTP subsystem: {error}"
                    )))
                    .await;
            }
            Err(_) => {
                let _ = sftp_events
                    .send(SftpEvent::Error(
                        "Timed out opening the SFTP subsystem.".to_owned(),
                    ))
                    .await;
            }
        }

        let channel = session.channel_open_session().await?;
        channel
            .request_pty(
                true,
                "xterm-256color",
                terminal_size.columns.into(),
                terminal_size.rows.into(),
                u32::from(terminal_size.columns) * 8,
                u32::from(terminal_size.rows) * 18,
                &[],
            )
            .await?;
        channel.request_shell(true).await?;
        Ok::<_, anyhow::Error>((session, channel, jump_session))
    };
    tokio::pin!(setup);
    let (mut session, channel, jump_session) = tokio::select! {
        result = &mut setup => result?,
        _ = wait_for_cancellation(&mut cancellation) => {
            let _ = events.send(UiEvent::Disconnected).await;
            return Ok(());
        }
    };
    events
        .send(UiEvent::Status(format!(
            "Connected to {}:{} as {}",
            host, port, username
        )))
        .await
        .context("UI closed while connecting")?;

    let (mut reader, writer) = channel.split();
    let mut terminal = TerminalEmulator::new_with_theme(terminal_size, theme);
    terminal.set_cursor_style(cursor_style);
    let mut screen_dirty = false;
    let mut log_file: Option<tokio::fs::File> = None;
    let mut log_buffer: Vec<u8> = Vec::new();
    let mut log_framed = true;
    let mut render_tick = interval(Duration::from_millis(16));
    render_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            request = commands.recv() => {
                match request {
                    Some(TerminalRequest::Key(key)) => {
                        if let Some(bytes) = key.encode(terminal.application_cursor_mode()) {
                            writer.data_bytes(bytes).await?;
                        }
                    }
                    Some(TerminalRequest::Resize(size)) => {
                        terminal.resize(size);
                        writer.window_change(
                            size.columns.into(),
                            size.rows.into(),
                            u32::from(size.columns) * 8,
                            u32::from(size.rows) * 18,
                        ).await?;
                        events.send(UiEvent::Screen(terminal.frame())).await
                            .context("UI closed during terminal resize")?;
                        screen_dirty = false;
                    }
                    Some(TerminalRequest::Mouse(event)) => {
                        let tracking_mode = terminal.mouse_tracking_mode();
                        if let Some(bytes) = terminal.mouse_report_bytes(event) {
                            writer.data_bytes(bytes).await?;
                        } else if tracking_mode == MouseTrackingMode::Disabled {
                            match event {
                                TerminalMouseEvent::Press {
                                    button: TerminalMouseButton::Left,
                                    position,
                                    ..
                                } => {
                                    terminal.start_selection(position);
                                    screen_dirty = true;
                                }
                                TerminalMouseEvent::Move {
                                    button: Some(TerminalMouseButton::Left),
                                    position,
                                    ..
                                } => {
                                    terminal.update_selection(position);
                                    screen_dirty = true;
                                }
                                TerminalMouseEvent::Wheel { rows, .. } => {
                                    terminal.scroll_display(rows);
                                    events.send(UiEvent::Screen(terminal.frame())).await
                                        .context("UI closed during terminal scroll")?;
                                    screen_dirty = false;
                                }
                                TerminalMouseEvent::Press { .. }
                                | TerminalMouseEvent::Release { .. }
                                | TerminalMouseEvent::Move { .. } => {}
                            }
                        }
                    }
                    Some(TerminalRequest::Paste(text)) => {
                        writer.data_bytes(terminal.paste_bytes(&text)).await?;
                    }
                    Some(TerminalRequest::StartLog(path)) => {
                        match open_log_file(&path).await {
                            Ok((file, framed)) => {
                                log_file = Some(file);
                                log_framed = framed;
                                log_buffer.clear();
                                events
                                    .send(UiEvent::LogState(Some(path)))
                                    .await
                                    .context("UI closed while starting terminal logging")?;
                            }
                            Err(error) => {
                                let _ = events.send(UiEvent::LogState(None)).await;
                                let _ = events
                                    .send(UiEvent::Status(format!(
                                        "Could not start terminal logging: {error:#}"
                                    )))
                                    .await;
                            }
                        }
                    }
                    Some(TerminalRequest::StopLog) => {
                        if let Err(error) =
                            flush_log(&mut log_file, &mut log_buffer, log_framed).await
                        {
                            let _ = events
                                .send(UiEvent::Status(format!(
                                    "Could not finish the terminal log: {error:#}"
                                )))
                                .await;
                            log_file = None;
                            log_buffer.clear();
                        }
                        let _ = events.send(UiEvent::LogState(None)).await;
                    }
                    Some(TerminalRequest::SetTheme(theme)) => {
                        terminal.set_theme(theme);
                        events
                            .send(UiEvent::Screen(terminal.frame()))
                            .await
                            .context("UI closed while changing the terminal theme")?;
                    }
                    Some(TerminalRequest::SetCursorStyle(cursor_style)) => {
                        terminal.set_cursor_style(cursor_style);
                        events
                            .send(UiEvent::Screen(terminal.frame()))
                            .await
                            .context("UI closed while changing the terminal cursor")?;
                    }
                    Some(TerminalRequest::Search { query, direction }) => {
                        let result = terminal.search(&query, direction);
                        events
                            .send(UiEvent::SearchResult(result))
                            .await
                            .context("UI closed while searching the terminal")?;
                        events
                            .send(UiEvent::Screen(terminal.frame()))
                            .await
                            .context("UI closed while showing the search match")?;
                    }
                    Some(TerminalRequest::RefreshServerMetrics) => {
                        refresh_server_metrics(&mut session, &events).await;
                    }
                    Some(TerminalRequest::RefreshServerProcesses) => {
                        refresh_server_processes(&mut session, &events).await;
                    }
                    Some(TerminalRequest::TerminateServerProcess { pid }) => {
                        terminate_server_process(&mut session, &events, pid).await;
                    }
                    Some(TerminalRequest::RefreshServerServices) => {
                        refresh_server_services(&mut session, &events).await;
                    }
                    Some(TerminalRequest::ControlServerService { name, action }) => {
                        control_server_service(&mut session, &events, name, action).await;
                    }
                    Some(TerminalRequest::ReadServerServiceLogs { name }) => {
                        read_server_service_logs(&mut session, &events, name).await;
                    }
                    Some(TerminalRequest::ControlServerPower { action }) => {
                        control_server_power(&mut session, &events, action).await;
                    }
                    None => break,
                }
            }
            _ = wait_for_cancellation(&mut cancellation) => break,
            command = forward_commands.recv() => {
                match command {
                    Some(ForwardCommand::Add(spec)) => {
                        let id = next_forward_id;
                        next_forward_id = next_forward_id.saturating_add(1);
                        if spec.kind == ForwardKind::Local || spec.kind == ForwardKind::Dynamic {
                            match forward::bind_local_rule(
                                id,
                                spec.clone(),
                                accepted_tx.clone(),
                                forward_events.clone(),
                            )
                            .await
                            {
                                Ok((bound_port, stop, task)) => {
                                    local_forwards.insert(
                                        id,
                                        LocalForwardRuntime { stop, task },
                                    );
                                    let _ = forward_events.send(ForwardEvent::RuleAdded {
                                        id,
                                        spec,
                                        bound_port,
                                    });
                                }
                                Err(error) => {
                                    let _ = forward_events.send(ForwardEvent::Error {
                                        id: Some(id),
                                        error: format!("{error:#}"),
                                    });
                                }
                            }
                        } else {
                            match session
                                .tcpip_forward(
                                    spec.listen_host.clone(),
                                    u32::from(spec.listen_port),
                                )
                                .await
                            {
                                Ok(port) => {
                                    let bound_port = if port == 0 {
                                        spec.listen_port
                                    } else {
                                        u16::try_from(port).unwrap_or(0)
                                    };
                                    if bound_port == 0 {
                                        let _ = forward_events.send(ForwardEvent::Error {
                                            id: Some(id),
                                            error: format!(
                                                "The server did not provide a usable forwarding port (requested {}, returned {port})",
                                                spec.listen_port
                                            ),
                                        });
                                    } else {
                                        if let Ok(mut targets) = remote_targets.lock() {
                                            targets.insert(
                                                u32::from(bound_port),
                                                RemoteTarget {
                                                    rule_id: id,
                                                    host: spec.target_host.clone(),
                                                    port: spec.target_port,
                                                },
                                            );
                                        }
                                        remote_forwards.insert(
                                            id,
                                            RemoteForwardRuntime {
                                                spec: spec.clone(),
                                                bound_port,
                                            },
                                        );
                                        let _ = forward_events.send(ForwardEvent::RuleAdded {
                                            id,
                                            spec,
                                            bound_port,
                                        });
                                    }
                                }
                                Err(error) => {
                                    let _ = forward_events.send(ForwardEvent::Error {
                                        id: Some(id),
                                        error: format!(
                                            "Could not request remote forwarding on {}:{}: {error}",
                                            spec.listen_host, spec.listen_port
                                        ),
                                    });
                                }
                            }
                        }
                    }
                    Some(ForwardCommand::Remove(id)) => {
                        if let Some(runtime) = local_forwards.remove(&id) {
                            let LocalForwardRuntime { stop, task } = runtime;
                            let _ = stop.send(true);
                            task.abort();
                            let _ = task.await;
                            let _ = forward_events.send(ForwardEvent::RuleRemoved { id });
                        } else if let Some(runtime) = remote_forwards.remove(&id) {
                            if let Ok(mut targets) = remote_targets.lock() {
                                targets.remove(&u32::from(runtime.bound_port));
                            }
                            let _ = session
                                .cancel_tcpip_forward(
                                    runtime.spec.listen_host,
                                    u32::from(runtime.bound_port),
                                )
                                .await;
                            let _ = forward_events.send(ForwardEvent::RuleRemoved { id });
                        } else {
                            let _ = forward_events.send(ForwardEvent::Error {
                                id: Some(id),
                                error: "The forwarding rule no longer exists.".to_owned(),
                            });
                        }
                    }
                    None => {}
                }
            }
            accepted = accepted_connections.recv() => {
                if let Some(AcceptedConnection {
                    rule_id,
                    mut stream,
                    peer,
                    target_host,
                    target_port,
                    socks5,
                }) = accepted
                {
                    match session
                        .channel_open_direct_tcpip(
                            target_host.clone(),
                            u32::from(target_port),
                            peer.ip().to_string(),
                            u32::from(peer.port()),
                        )
                        .await
                    {
                        Ok(channel) => {
                            if socks5 {
                                let _ = forward::socks5_accept(&mut stream).await;
                            }
                            let _ = forward_events.send(ForwardEvent::ConnectionOpened {
                                id: rule_id,
                                peer: peer.to_string(),
                                target: format!("{target_host}:{target_port}"),
                            });
                            let mut remote = channel.into_stream();
                            let events = forward_events.clone();
                            let peer_label = peer.to_string();
                            tokio::spawn(async move {
                                let _ =
                                    tokio::io::copy_bidirectional(&mut stream, &mut remote).await;
                                let _ = events.send(ForwardEvent::ConnectionClosed {
                                    id: rule_id,
                                    peer: peer_label,
                                });
                            });
                        }
                        Err(error) => {
                            if socks5 {
                                let _ = forward::socks5_reject(&mut stream, 4).await;
                            }
                            let _ = forward_events.send(ForwardEvent::Error {
                                id: Some(rule_id),
                                error: format!(
                                    "Could not open a forwarded connection to {target_host}:{target_port}: {error}"
                                ),
                            });
                        }
                    }
                }
            }
            _ = render_tick.tick(), if screen_dirty => {
                if let Err(error) = flush_log(&mut log_file, &mut log_buffer, log_framed).await {
                    log_file = None;
                    log_buffer.clear();
                    let _ = events.send(UiEvent::LogState(None)).await;
                    let _ = events
                        .send(UiEvent::Status(format!(
                            "Terminal logging stopped: {error:#}"
                        )))
                        .await;
                }
                events.send(UiEvent::Screen(terminal.frame())).await
                    .context("UI closed during terminal output")?;
                screen_dirty = false;
            }
            message = reader.wait() => {
                match message {
                    Some(ChannelMsg::Data { data }) => {
                        terminal.feed(&data);
                        if log_file.is_some() {
                            log_buffer.extend_from_slice(&data);
                        }
                        screen_dirty = true;
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        terminal.feed(&data);
                        if log_file.is_some() {
                            log_buffer.extend_from_slice(&data);
                        }
                        screen_dirty = true;
                    }
                    Some(ChannelMsg::ExitStatus { .. }) | None => break,
                    Some(_) => {}
                }
            }
        }
    }

    let _ = flush_log(&mut log_file, &mut log_buffer, log_framed).await;
    for (_, runtime) in local_forwards.drain() {
        let _ = runtime.stop.send(true);
        runtime.task.abort();
    }
    for (_, runtime) in remote_forwards.drain() {
        let _ = session
            .cancel_tcpip_forward(runtime.spec.listen_host, u32::from(runtime.bound_port))
            .await;
    }
    if let Ok(mut targets) = remote_targets.lock() {
        targets.clear();
    }
    let _ = session
        .disconnect(Disconnect::ByApplication, "client closed", "en")
        .await;
    if let Some(jump_session) = jump_session {
        let _ = jump_session
            .disconnect(Disconnect::ByApplication, "target session closed", "en")
            .await;
    }
    let _ = events.send(UiEvent::Disconnected).await;
    Ok(())
}

const SERVER_METRICS_COMMAND: &str = r##"printf '%s\n' 'DSHELL_METRICS_V1'; printf '%s\n' "OS=$(uname -s 2>/dev/null || printf unknown)"; printf '%s\n' "KERNEL=$(uname -r 2>/dev/null || printf unknown)"; printf '%s\n' "LOAD1=$(awk '{print $1}' /proc/loadavg 2>/dev/null)"; printf '%s\n' "LOAD5=$(awk '{print $2}' /proc/loadavg 2>/dev/null)"; printf '%s\n' "LOAD15=$(awk '{print $3}' /proc/loadavg 2>/dev/null)"; printf '%s\n' "MEM_TOTAL_KIB=$(awk '/^MemTotal:/{print $2; exit}' /proc/meminfo 2>/dev/null)"; printf '%s\n' "MEM_AVAILABLE_KIB=$(awk '/^MemAvailable:/{print $2; exit}' /proc/meminfo 2>/dev/null)"; printf '%s\n' "UPTIME_SECONDS=$(awk '{print int($1)}' /proc/uptime 2>/dev/null)"; printf '%s\n' "DISK_TOTAL_KIB=$(df -Pk / 2>/dev/null | awk 'NR==2 {print $2; exit}')"; printf '%s\n' "DISK_USED_KIB=$(df -Pk / 2>/dev/null | awk 'NR==2 {print $3; exit}')"; printf '%s\n' "DISK_AVAILABLE_KIB=$(df -Pk / 2>/dev/null | awk 'NR==2 {print $4; exit}')"; printf '%s\n' "DISK_USE_PERCENT=$(df -Pk / 2>/dev/null | awk 'NR==2 {gsub(/%/,"",$5); print $5; exit}')"##;

const SERVER_PROCESSES_COMMAND: &str = r##"printf 'DSHELL_PROCESSES_V1\n'; ps -eo pid=,user=,pcpu=,pmem=,stat=,args= --sort=-pcpu 2>/dev/null | head -n 50 | awk '{pid=$1; user=$2; cpu=$3; mem=$4; state=$5; $1=$2=$3=$4=$5=""; sub(/^[ \t]+/, ""); print "PROC\t" pid "\t" user "\t" cpu "\t" mem "\t" state "\t" $0}'"##;
const MAX_SERVER_PROCESSES: usize = 50;

const SERVER_SERVICES_COMMAND: &str = r##"if command -v systemctl >/dev/null 2>&1; then services=$(systemctl list-units --type=service --all --no-legend --no-pager 2>/dev/null); if [ $? -eq 0 ] && [ -n "$services" ]; then printf 'DSHELL_SERVICES_V1\n'; printf '%s\n' "$services" | head -n 50 | awk '{unit=$1; load=$2; active=$3; sub=$4; $1=$2=$3=$4=""; sub(/^[ \t]+/, ""); print "SERVICE\t" unit "\t" load "\t" active "\t" sub "\t" $0}'; exit 0; fi; fi; if command -v service >/dev/null 2>&1; then printf 'DSHELL_SERVICES_V1\n'; service --status-all 2>&1 | head -n 50 | awk '$2 == "+" || $2 == "-" || $2 == "?" {mark=$2; name=$4; active=(mark == "+") ? "active" : ((mark == "-") ? "inactive" : "unknown"); print "SERVICE\t" name "\tloaded\t" active "\t" active "\t" $0}'; exit 0; fi; printf 'DSHELL_SERVICES_V1\n'"##;
const SERVER_SERVICE_LOG_LINES: usize = 80;

fn metric_value<'a>(lines: &'a str, key: &str) -> Option<&'a str> {
    lines
        .lines()
        .find_map(|line| {
            line.strip_prefix(key)
                .and_then(|value| value.strip_prefix('='))
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn metric_u64(lines: &str, key: &str) -> Option<u64> {
    metric_value(lines, key).and_then(|value| value.parse().ok())
}

fn metric_f32(lines: &str, key: &str) -> Option<f32> {
    metric_value(lines, key)
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite())
}

fn parse_server_metrics(output: &str) -> Result<ServerMetrics> {
    if !output.contains("DSHELL_METRICS_V1") {
        bail!("The remote host did not return a supported metrics response.");
    }
    let os = metric_value(output, "OS").unwrap_or("Unknown").to_owned();
    let kernel = metric_value(output, "KERNEL")
        .unwrap_or("Unknown")
        .to_owned();
    let disk_use_percent = metric_u64(output, "DISK_USE_PERCENT").map(|value| value.min(100) as u8);
    if os == "Unknown"
        && kernel == "Unknown"
        && metric_f32(output, "LOAD1").is_none()
        && metric_u64(output, "MEM_TOTAL_KIB").is_none()
        && metric_u64(output, "DISK_TOTAL_KIB").is_none()
    {
        bail!("The remote host did not expose usable system metrics.");
    }
    Ok(ServerMetrics {
        os,
        kernel,
        load_1m: metric_f32(output, "LOAD1"),
        load_5m: metric_f32(output, "LOAD5"),
        load_15m: metric_f32(output, "LOAD15"),
        memory_total_kib: metric_u64(output, "MEM_TOTAL_KIB"),
        memory_available_kib: metric_u64(output, "MEM_AVAILABLE_KIB"),
        disk_total_kib: metric_u64(output, "DISK_TOTAL_KIB"),
        disk_used_kib: metric_u64(output, "DISK_USED_KIB"),
        disk_available_kib: metric_u64(output, "DISK_AVAILABLE_KIB"),
        disk_use_percent,
        uptime_seconds: metric_u64(output, "UPTIME_SECONDS"),
    })
}

async fn refresh_server_metrics(
    session: &mut client::Handle<KnownHostsHandler>,
    events: &mpsc::Sender<UiEvent>,
) {
    let result = timeout(Duration::from_secs(3), async {
        let channel = session
            .channel_open_session()
            .await
            .context("could not open a server metrics channel")?;
        channel
            .exec(true, SERVER_METRICS_COMMAND)
            .await
            .context("could not start the server metrics command")?;
        let mut output = Vec::new();
        let mut stream = channel.into_stream();
        stream
            .read_to_end(&mut output)
            .await
            .context("could not read the server metrics response")?;
        if output.len() > 64 * 1024 {
            bail!("The server metrics response was too large.");
        }
        let output = String::from_utf8_lossy(&output);
        let metrics = parse_server_metrics(&output)?;
        Ok::<ServerMetrics, anyhow::Error>(metrics)
    })
    .await;

    match result {
        Ok(Ok(metrics)) => {
            let _ = events
                .send(UiEvent::ServerMetrics(ServerData::Metrics(metrics)))
                .await;
        }
        Ok(Err(error)) => {
            let _ = events
                .send(UiEvent::ServerMetricsError(ServerDataError::Metrics(
                    format!("{error:#}"),
                )))
                .await;
        }
        Err(_) => {
            let _ = events
                .send(UiEvent::ServerMetricsError(ServerDataError::Metrics(
                    "Timed out waiting for server metrics.".to_owned(),
                )))
                .await;
        }
    }
}

fn parse_server_processes(output: &str) -> Result<Vec<ServerProcess>> {
    if !output
        .lines()
        .any(|line| line.trim() == "DSHELL_PROCESSES_V1")
    {
        bail!("The remote host did not return a supported process response.");
    }
    let mut processes = Vec::new();
    for line in output.lines() {
        let Some(fields) = line.strip_prefix("PROC\t") else {
            continue;
        };
        let fields = fields.splitn(6, '\t').collect::<Vec<_>>();
        if fields.len() != 6 {
            bail!("The remote host returned a malformed process row.");
        }
        let pid = fields[0]
            .parse::<u32>()
            .context("The remote host returned an invalid process ID.")?;
        let cpu_percent = fields[2]
            .parse::<f32>()
            .context("The remote host returned an invalid process CPU value.")?;
        let memory_percent = fields[3]
            .parse::<f32>()
            .context("The remote host returned an invalid process memory value.")?;
        if pid == 0
            || fields[1].trim().is_empty()
            || !cpu_percent.is_finite()
            || cpu_percent < 0.0
            || !memory_percent.is_finite()
            || memory_percent < 0.0
            || fields[4].trim().is_empty()
            || fields[5].trim().is_empty()
        {
            bail!("The remote host returned an invalid process row.");
        }
        if processes.len() >= MAX_SERVER_PROCESSES {
            bail!("The remote host returned too many process rows.");
        }
        processes.push(ServerProcess {
            pid,
            user: fields[1].trim().to_owned(),
            cpu_percent,
            memory_percent,
            state: fields[4].trim().to_owned(),
            command: fields[5].trim().to_owned(),
        });
    }
    if processes.is_empty() {
        bail!("The remote host did not expose any process rows.");
    }
    Ok(processes)
}

fn parse_server_services(output: &str) -> Result<Vec<ServerService>> {
    if !output
        .lines()
        .any(|line| line.trim() == "DSHELL_SERVICES_V1")
    {
        bail!("The remote host did not return a supported service response.");
    }
    let mut services = Vec::new();
    for line in output.lines() {
        let Some(fields) = line.strip_prefix("SERVICE\t") else {
            continue;
        };
        let fields = fields.splitn(5, '\t').collect::<Vec<_>>();
        if fields.len() != 5 {
            bail!("The remote host returned a malformed service row.");
        }
        if fields[..4].iter().any(|field| field.trim().is_empty()) {
            bail!("The remote host returned an invalid service row.");
        }
        if services.len() >= 50 {
            bail!("The remote host returned too many service rows.");
        }
        services.push(ServerService {
            name: fields[0].trim().to_owned(),
            load: fields[1].trim().to_owned(),
            active: fields[2].trim().to_owned(),
            sub: fields[3].trim().to_owned(),
            description: fields[4].trim().to_owned(),
        });
    }
    Ok(services)
}

fn service_logs_command(name: &str) -> String {
    format!(
        "printf '%s\\n' 'DSHELL_SERVICE_LOGS_V1'; if command -v journalctl >/dev/null 2>&1; then journalctl -u {} -n {} --no-pager -o short-iso 2>&1; status=$?; elif command -v service >/dev/null 2>&1; then service {} status 2>&1; status=$?; else status=127; fi; printf 'DSHELL_SERVICE_LOGS_END\\t%d\\n' \"$status\"",
        name, SERVER_SERVICE_LOG_LINES, name,
    )
}

fn parse_service_logs(output: &str, name: &str) -> Result<ServerServiceLogs> {
    if !output
        .lines()
        .any(|line| line.trim() == "DSHELL_SERVICE_LOGS_V1")
    {
        bail!("The remote host did not return a supported service logs response.");
    }
    let (body, status) = output
        .rsplit_once("DSHELL_SERVICE_LOGS_END\t")
        .ok_or_else(|| anyhow!("The remote host returned an incomplete service logs response."))?;
    let status = status
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .parse::<u8>()
        .context("The remote host returned an invalid service logs status.")?;
    let lines = body
        .lines()
        .skip_while(|line| line.trim() != "DSHELL_SERVICE_LOGS_V1")
        .skip(1)
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .take(SERVER_SERVICE_LOG_LINES)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if status != 0 {
        let detail = lines
            .iter()
            .take(3)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("; ");
        if detail.is_empty() {
            bail!("The remote host could not read service logs.");
        }
        bail!("The remote host could not read service logs: {detail}");
    }
    Ok(ServerServiceLogs {
        name: name.to_owned(),
        lines,
    })
}

async fn refresh_server_processes(
    session: &mut client::Handle<KnownHostsHandler>,
    events: &mpsc::Sender<UiEvent>,
) {
    let result = timeout(Duration::from_secs(3), async {
        let channel = session
            .channel_open_session()
            .await
            .context("could not open a server process channel")?;
        channel
            .exec(true, SERVER_PROCESSES_COMMAND)
            .await
            .context("could not start the server process command")?;
        let mut output = Vec::new();
        let mut stream = channel.into_stream();
        stream
            .read_to_end(&mut output)
            .await
            .context("could not read the server process response")?;
        if output.len() > 64 * 1024 {
            bail!("The server process response was too large.");
        }
        let output = String::from_utf8_lossy(&output);
        parse_server_processes(&output)
    })
    .await;

    match result {
        Ok(Ok(processes)) => {
            let _ = events
                .send(UiEvent::ServerMetrics(ServerData::Processes(processes)))
                .await;
        }
        Ok(Err(error)) => {
            let _ = events
                .send(UiEvent::ServerMetricsError(ServerDataError::Processes(
                    format!("{error:#}"),
                )))
                .await;
        }
        Err(_) => {
            let _ = events
                .send(UiEvent::ServerMetricsError(ServerDataError::Processes(
                    "Timed out waiting for server processes.".to_owned(),
                )))
                .await;
        }
    }
}

fn valid_process_pid(pid: u32) -> bool {
    pid > 1
}

fn process_termination_command(pid: u32) -> String {
    format!("kill -TERM {pid}; status=$?; printf 'DSHELL_PROCESS_ACTION_V1\\t%d\\n' \"$status\"")
}

fn parse_process_action(output: &str) -> Result<(bool, String)> {
    let marker = output
        .lines()
        .find_map(|line| line.strip_prefix("DSHELL_PROCESS_ACTION_V1\t"))
        .ok_or_else(|| {
            anyhow!("The remote host did not return a supported process action response.")
        })?;
    let status = marker
        .trim()
        .parse::<u8>()
        .context("The remote host returned an invalid process action status.")?;
    let detail = output
        .lines()
        .filter(|line| !line.starts_with("DSHELL_PROCESS_ACTION_V1\t"))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join("; ");
    Ok((status == 0, detail))
}

async fn terminate_server_process(
    session: &mut client::Handle<KnownHostsHandler>,
    events: &mpsc::Sender<UiEvent>,
    pid: u32,
) {
    if !valid_process_pid(pid) {
        let _ = events
            .send(UiEvent::ServerMetrics(ServerData::ProcessAction(
                ServerProcessActionResult {
                    pid,
                    success: false,
                    message: "The process ID is not valid for termination.".to_owned(),
                },
            )))
            .await;
        return;
    }
    let result = timeout(Duration::from_secs(3), async {
        let channel = session
            .channel_open_session()
            .await
            .context("could not open a process action channel")?;
        channel
            .exec(true, process_termination_command(pid))
            .await
            .context("could not start the process termination command")?;
        let mut output = Vec::new();
        let mut stream = channel.into_stream();
        stream
            .read_to_end(&mut output)
            .await
            .context("could not read the process action response")?;
        if output.len() > 64 * 1024 {
            bail!("The process action response was too large.");
        }
        let output = String::from_utf8_lossy(&output);
        parse_process_action(&output)
    })
    .await;
    let (success, detail) = match result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => (false, format!("{error:#}")),
        Err(_) => (
            false,
            "Timed out waiting for the process action.".to_owned(),
        ),
    };
    let message = if success {
        if detail.is_empty() {
            "Process termination requested.".to_owned()
        } else {
            detail
        }
    } else if detail.is_empty() {
        "Process termination failed.".to_owned()
    } else {
        detail
    };
    let _ = events
        .send(UiEvent::ServerMetrics(ServerData::ProcessAction(
            ServerProcessActionResult {
                pid,
                success,
                message,
            },
        )))
        .await;
}

async fn refresh_server_services(
    session: &mut client::Handle<KnownHostsHandler>,
    events: &mpsc::Sender<UiEvent>,
) {
    let result = timeout(Duration::from_secs(3), async {
        let channel = session
            .channel_open_session()
            .await
            .context("could not open a server service channel")?;
        channel
            .exec(true, SERVER_SERVICES_COMMAND)
            .await
            .context("could not start the server service command")?;
        let mut output = Vec::new();
        let mut stream = channel.into_stream();
        stream
            .read_to_end(&mut output)
            .await
            .context("could not read the server service response")?;
        if output.len() > 64 * 1024 {
            bail!("The server service response was too large.");
        }
        let output = String::from_utf8_lossy(&output);
        parse_server_services(&output)
    })
    .await;

    match result {
        Ok(Ok(services)) => {
            let _ = events
                .send(UiEvent::ServerMetrics(ServerData::Services(services)))
                .await;
        }
        Ok(Err(error)) => {
            let _ = events
                .send(UiEvent::ServerMetricsError(ServerDataError::Services(
                    format!("{error:#}"),
                )))
                .await;
        }
        Err(_) => {
            let _ = events
                .send(UiEvent::ServerMetricsError(ServerDataError::Services(
                    "Timed out waiting for server services.".to_owned(),
                )))
                .await;
        }
    }
}

async fn read_server_service_logs(
    session: &mut client::Handle<KnownHostsHandler>,
    events: &mpsc::Sender<UiEvent>,
    name: String,
) {
    let name = name.trim().to_owned();
    if !valid_service_name(&name) {
        let _ = events
            .send(UiEvent::ServerMetricsError(ServerDataError::ServiceLogs(
                "The service name contains unsupported characters.".to_owned(),
            )))
            .await;
        return;
    }
    let result = timeout(Duration::from_secs(3), async {
        let channel = session
            .channel_open_session()
            .await
            .context("could not open a service logs channel")?;
        channel
            .exec(true, service_logs_command(&name))
            .await
            .context("could not start the service logs command")?;
        let mut output = Vec::new();
        let mut stream = channel.into_stream();
        stream
            .read_to_end(&mut output)
            .await
            .context("could not read the service logs response")?;
        if output.len() > 64 * 1024 {
            bail!("The server service logs response was too large.");
        }
        let output = String::from_utf8_lossy(&output);
        parse_service_logs(&output, &name)
    })
    .await;
    match result {
        Ok(Ok(logs)) => {
            let _ = events
                .send(UiEvent::ServerMetrics(ServerData::ServiceLogs(logs)))
                .await;
        }
        Ok(Err(error)) => {
            let _ = events
                .send(UiEvent::ServerMetricsError(ServerDataError::ServiceLogs(
                    format!("{error:#}"),
                )))
                .await;
        }
        Err(_) => {
            let _ = events
                .send(UiEvent::ServerMetricsError(ServerDataError::ServiceLogs(
                    "Timed out waiting for service logs.".to_owned(),
                )))
                .await;
        }
    }
}

fn valid_service_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'@' | b':' | b'+' | b'-')
        })
}

fn service_action_command(name: &str, action: ServiceAction) -> String {
    format!(
        "if command -v systemctl >/dev/null 2>&1; then systemctl {} {}; status=$?; else if command -v service >/dev/null 2>&1; then service {} {}; status=$?; else status=127; fi; fi; printf 'DSHELL_SERVICE_ACTION_V1\\t%d\\n' \"$status\"",
        action.command(),
        name,
        name,
        action.command(),
    )
}

fn parse_service_action(output: &str) -> Result<(bool, String)> {
    let marker = output
        .lines()
        .find_map(|line| line.strip_prefix("DSHELL_SERVICE_ACTION_V1\t"))
        .ok_or_else(|| {
            anyhow!("The remote host did not return a supported service action response.")
        })?;
    let status = marker
        .trim()
        .parse::<u8>()
        .context("The remote host returned an invalid service action status.")?;
    let detail = output
        .lines()
        .filter(|line| !line.starts_with("DSHELL_SERVICE_ACTION_V1\t"))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join("; ");
    Ok((status == 0, detail))
}

async fn control_server_service(
    session: &mut client::Handle<KnownHostsHandler>,
    events: &mpsc::Sender<UiEvent>,
    name: String,
    action: ServiceAction,
) {
    let name = name.trim().to_owned();
    if !valid_service_name(&name) {
        let _ = events
            .send(UiEvent::ServerMetrics(ServerData::ServiceAction(
                ServerServiceActionResult {
                    name,
                    action,
                    success: false,
                    message: "The service name contains unsupported characters.".to_owned(),
                },
            )))
            .await;
        return;
    }
    let result = timeout(Duration::from_secs(3), async {
        let channel = session
            .channel_open_session()
            .await
            .context("could not open a service action channel")?;
        channel
            .exec(true, service_action_command(&name, action))
            .await
            .context("could not start the service action command")?;
        let mut output = Vec::new();
        let mut stream = channel.into_stream();
        stream
            .read_to_end(&mut output)
            .await
            .context("could not read the service action response")?;
        if output.len() > 64 * 1024 {
            bail!("The service action response was too large.");
        }
        let output = String::from_utf8_lossy(&output);
        parse_service_action(&output)
    })
    .await;
    let (success, detail) = match result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => (false, format!("{error:#}")),
        Err(_) => (
            false,
            "Timed out waiting for the service action.".to_owned(),
        ),
    };
    let message = if success {
        if detail.is_empty() {
            format!("{} completed.", action.label())
        } else {
            detail
        }
    } else if detail.is_empty() {
        format!("{} failed.", action.label())
    } else {
        detail
    };
    let _ = events
        .send(UiEvent::ServerMetrics(ServerData::ServiceAction(
            ServerServiceActionResult {
                name,
                action,
                success,
                message,
            },
        )))
        .await;
}

fn power_action_command(action: ServerPowerAction) -> &'static str {
    match action {
        ServerPowerAction::Reboot => {
            "if command -v systemctl >/dev/null 2>&1; then systemctl --no-block reboot; status=$?; else (sleep 1; reboot) >/dev/null 2>&1 & status=$?; fi; printf 'DSHELL_POWER_ACTION_V1\\t%d\\n' \"$status\""
        }
        ServerPowerAction::Shutdown => {
            "if command -v systemctl >/dev/null 2>&1; then systemctl --no-block poweroff; status=$?; else (sleep 1; shutdown -h now) >/dev/null 2>&1 & status=$?; fi; printf 'DSHELL_POWER_ACTION_V1\\t%d\\n' \"$status\""
        }
    }
}

fn parse_power_action(output: &str) -> Result<(bool, String)> {
    let marker = output
        .lines()
        .find_map(|line| line.strip_prefix("DSHELL_POWER_ACTION_V1\t"))
        .ok_or_else(|| {
            anyhow!("The remote host did not return a supported power action response.")
        })?;
    let status = marker
        .trim()
        .parse::<u8>()
        .context("The remote host returned an invalid power action status.")?;
    let detail = output
        .lines()
        .filter(|line| !line.starts_with("DSHELL_POWER_ACTION_V1\t"))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join("; ");
    Ok((status == 0, detail))
}

async fn control_server_power(
    session: &mut client::Handle<KnownHostsHandler>,
    events: &mpsc::Sender<UiEvent>,
    action: ServerPowerAction,
) {
    let result = timeout(Duration::from_secs(3), async {
        let channel = session
            .channel_open_session()
            .await
            .context("could not open a server power action channel")?;
        channel
            .exec(true, power_action_command(action))
            .await
            .context("could not start the server power action command")?;
        let mut output = Vec::new();
        let mut stream = channel.into_stream();
        stream
            .read_to_end(&mut output)
            .await
            .context("could not read the server power action response")?;
        if output.len() > 64 * 1024 {
            bail!("The server power action response was too large.");
        }
        let output = String::from_utf8_lossy(&output);
        parse_power_action(&output)
    })
    .await;
    let (success, detail) = match result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => (false, format!("{error:#}")),
        Err(_) => (
            false,
            "Timed out waiting for the server power action.".to_owned(),
        ),
    };
    let message = if success {
        if detail.is_empty() {
            format!("{} requested.", action.label())
        } else {
            detail
        }
    } else if detail.is_empty() {
        format!("{} failed.", action.label())
    } else {
        detail
    };
    let _ = events
        .send(UiEvent::ServerMetrics(ServerData::PowerAction(
            ServerPowerActionResult {
                action,
                success,
                message,
            },
        )))
        .await;
}

fn can_answer_with_password(prompts: &[Prompt], password_used: bool) -> bool {
    if prompts.is_empty() {
        return true;
    }
    !password_used
        && prompts.iter().all(|prompt| {
            let text = prompt.prompt.to_ascii_lowercase();
            !prompt.echo && (text.contains("password") || text.contains("passphrase"))
        })
}

async fn authenticate(
    session: &mut client::Handle<KnownHostsHandler>,
    username: &str,
    authentication: Authentication,
    events: &mpsc::Sender<UiEvent>,
) -> Result<()> {
    match authentication {
        Authentication::Password(mut password) => {
            let auth = session
                .authenticate_password(username.to_owned(), std::mem::take(&mut *password))
                .await
                .context("SSH password authentication failed")?;
            drop(password);
            if !matches!(auth, AuthResult::Success) {
                bail!("SSH password authentication was rejected");
            }
        }
        Authentication::KeyboardInteractive {
            password,
            interactive,
        } => {
            let mut password_used = false;
            let mut response = session
                .authenticate_keyboard_interactive_start(username.to_owned(), None::<String>)
                .await
                .context("SSH keyboard-interactive authentication failed")?;
            loop {
                match response {
                    KeyboardInteractiveAuthResponse::Success => break,
                    KeyboardInteractiveAuthResponse::Failure { .. } => {
                        bail!("SSH keyboard-interactive authentication was rejected");
                    }
                    KeyboardInteractiveAuthResponse::InfoRequest {
                        name,
                        instructions,
                        prompts,
                    } => {
                        let responses =
                            if interactive && !can_answer_with_password(&prompts, password_used) {
                                let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                                events
                                    .send(UiEvent::AuthPrompt {
                                        name,
                                        instructions,
                                        prompts: prompts
                                            .iter()
                                            .map(|prompt| AuthPromptInfo {
                                                prompt: prompt.prompt.clone(),
                                                echo: prompt.echo,
                                            })
                                            .collect(),
                                        respond_to: response_tx,
                                    })
                                    .await
                                    .context("UI closed while waiting for authentication input")?;
                                response_rx
                                    .await
                                    .context("Keyboard-interactive prompt was dismissed")?
                            } else {
                                prompts
                                    .iter()
                                    .map(|prompt| {
                                        if prompt.echo {
                                            Zeroizing::new(String::new())
                                        } else {
                                            password_used = true;
                                            Zeroizing::new(password.to_string())
                                        }
                                    })
                                    .collect()
                            };
                        let responses = responses
                            .iter()
                            .map(|value| value.to_string())
                            .collect::<Vec<_>>();
                        response = session
                            .authenticate_keyboard_interactive_respond(responses)
                            .await
                            .context("SSH keyboard-interactive authentication failed")?;
                    }
                }
            }
            drop(password);
        }
        Authentication::PublicKey {
            identity_file,
            passphrase,
        } => {
            let identity_file = expand_user_path(&identity_file);
            let key = {
                let passphrase = (!passphrase.is_empty()).then_some(passphrase.as_str());
                load_secret_key(&identity_file, passphrase).with_context(|| {
                    format!(
                        "Could not load SSH identity file {}",
                        identity_file.display()
                    )
                })?
            };
            drop(passphrase);
            let hash_alg = session.best_supported_rsa_hash().await?.flatten();
            let auth = session
                .authenticate_publickey(
                    username.to_owned(),
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg),
                )
                .await
                .context("SSH public key authentication failed")?;
            if !matches!(auth, AuthResult::Success) {
                bail!("SSH public key authentication was rejected");
            }
        }
        Authentication::Agent => {
            let mut agent = connect_agent().await?;
            let identities = agent
                .request_identities()
                .await
                .context("Could not list identities from the SSH agent")?;
            if identities.is_empty() {
                bail!("The SSH agent has no identities loaded");
            }
            let mut rejection = None;
            for identity in identities {
                let key = identity.public_key().into_owned();
                let hash_alg = if key.algorithm().is_rsa() {
                    session.best_supported_rsa_hash().await?.flatten()
                } else {
                    None
                };
                match session
                    .authenticate_publickey_with(username.to_owned(), key, hash_alg, &mut agent)
                    .await
                {
                    Ok(AuthResult::Success) => return Ok(()),
                    Ok(AuthResult::Failure { .. }) => {
                        rejection = Some("the server rejected an agent identity".to_owned());
                    }
                    Err(error) => {
                        rejection = Some(format!("agent signing failed: {error}"));
                    }
                }
            }
            bail!(
                "SSH agent authentication was rejected ({})",
                rejection.unwrap_or_else(|| "no usable identities".to_owned())
            );
        }
    }
    Ok(())
}

async fn connect_agent() -> Result<
    russh::keys::agent::client::AgentClient<
        Box<dyn russh::keys::agent::client::AgentStream + Send + Unpin>,
    >,
> {
    #[cfg(windows)]
    {
        let path = env::var_os("SSH_AUTH_SOCK")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"\\.\pipe\openssh-ssh-agent"));
        match russh::keys::agent::client::AgentClient::connect_named_pipe(&path).await {
            Ok(client) => return Ok(client.dynamic()),
            Err(pipe_error) => {
                match russh::keys::agent::client::AgentClient::connect_pageant().await {
                    Ok(client) => return Ok(client.dynamic()),
                    Err(pageant_error) => bail!(
                        "Could not connect to the OpenSSH agent at {} ({pipe_error}) or Pageant ({pageant_error})",
                        path.display()
                    ),
                }
            }
        }
    }
    #[cfg(unix)]
    {
        let client = russh::keys::agent::client::AgentClient::connect_env()
            .await
            .context("Could not connect to the SSH agent at SSH_AUTH_SOCK")?;
        Ok(client.dynamic())
    }
}

fn expand_user_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    let Some(rest) = text
        .strip_prefix("~/")
        .or_else(|| text.strip_prefix("~\\"))
        .or_else(|| text.strip_prefix("%d/"))
        .or_else(|| text.strip_prefix("%d\\"))
    else {
        return path.to_path_buf();
    };
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map_or_else(|| path.to_path_buf(), |home| PathBuf::from(home).join(rest))
}

async fn wait_for_cancellation(cancellation: &mut watch::Receiver<bool>) {
    if *cancellation.borrow() {
        return;
    }
    let _ = cancellation.changed().await;
}

async fn open_log_file(path: &Path) -> Result<(tokio::fs::File, bool)> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Could not create {}", parent.display()))?;
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)
        .await
        .with_context(|| format!("Could not open {}", path.display()))?;
    let length = file
        .metadata()
        .await
        .with_context(|| format!("Could not inspect {}", path.display()))?
        .len();
    let timestamped = if length == 0 {
        file.write_all(log::HEADER)
            .await
            .with_context(|| format!("Could not write {}", path.display()))?;
        file.flush()
            .await
            .with_context(|| format!("Could not flush {}", path.display()))?;
        true
    } else {
        let mut header = [0u8; log::HEADER.len()];
        match file.read_exact(&mut header).await {
            Ok(_) => log::is_timestamped_header(&header),
            Err(_) => false,
        }
    };
    Ok((file, timestamped))
}

async fn flush_log(
    file: &mut Option<tokio::fs::File>,
    buffer: &mut Vec<u8>,
    timestamped: bool,
) -> Result<()> {
    let Some(file) = file.as_mut() else {
        buffer.clear();
        return Ok(());
    };
    if buffer.is_empty() {
        return Ok(());
    }
    if timestamped {
        let record = log::frame(log::now_ms(), buffer);
        file.write_all(&record).await?;
    } else {
        file.write_all(buffer).await?;
    }
    file.flush().await?;
    buffer.clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Authentication, ConnectionSpec, JumpSpec, ServerPowerAction, ServiceAction, UiEvent,
        can_answer_with_password, connection_info_for_key, is_known_host_key, parse_power_action,
        parse_process_action, parse_server_metrics, parse_server_processes, parse_server_services,
        parse_service_action, parse_service_logs, power_action_command,
        process_termination_command, run_connection_with_known_hosts, service_action_command,
        service_logs_command, valid_process_pid, valid_service_name,
    };
    use crate::forward::{ForwardCommand, ForwardEvent, ForwardKind, ForwardRuleSpec};
    use crate::sftp::{SftpCommand, SftpEvent};
    use crate::terminal::{
        CursorStyle, SearchDirection, TerminalFrame, TerminalKeyInput, TerminalPosition,
        TerminalRequest, TerminalSize, TerminalTheme,
    };
    use anyhow::{Context, Result, bail};
    use russh::client::Prompt;
    use russh::keys::PublicKey;
    use std::{
        env,
        path::PathBuf,
        sync::{Arc, atomic::AtomicBool},
        time::Duration,
    };
    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::{TcpListener, TcpStream},
        sync::{mpsc, watch},
        time::timeout,
    };
    use zeroize::Zeroizing;

    fn forward_test_channels() -> (
        mpsc::Receiver<ForwardCommand>,
        mpsc::Sender<ForwardCommand>,
        mpsc::UnboundedSender<ForwardEvent>,
        mpsc::UnboundedReceiver<ForwardEvent>,
    ) {
        let (command_tx, command_rx) = mpsc::channel(8);
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        (command_rx, command_tx, event_tx, event_rx)
    }

    fn sftp_test_channels() -> (
        mpsc::Receiver<SftpCommand>,
        mpsc::Sender<SftpEvent>,
        Arc<AtomicBool>,
    ) {
        let (_command_tx, command_rx) = mpsc::channel(8);
        let (event_tx, _event_rx) = mpsc::channel(8);
        (command_rx, event_tx, Arc::new(AtomicBool::new(false)))
    }

    const KEY_A: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
    const KEY_B: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA6rWI3G2sz07DnfFlrouTcysQlj2P+jpNSOEWD9OJ3X";

    #[test]
    fn known_host_key_is_accepted() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("known_hosts");
        std::fs::write(&path, format!("[localhost]:13265 {KEY_A}\n")).unwrap();
        let key = PublicKey::from_openssh(KEY_A).unwrap();

        assert!(is_known_host_key("localhost", 13265, &key, &path).unwrap());
    }

    #[test]
    fn connection_info_reports_the_algorithm_and_fingerprint() {
        let key = PublicKey::from_openssh(KEY_A).unwrap();
        let info = connection_info_for_key(&key);
        assert_eq!(info.host_key_algorithm, "ssh-ed25519");
        assert!(
            info.host_key_fingerprint.starts_with("SHA256:"),
            "{}",
            info.host_key_fingerprint
        );
        assert!(!info.host_key_fingerprint.contains(KEY_A));
    }

    #[test]
    fn server_metrics_parser_reads_supported_fields() {
        let metrics = parse_server_metrics(
            "DSHELL_METRICS_V1\nOS=Linux\nKERNEL=6.8.0\nLOAD1=0.25\nLOAD5=0.50\nLOAD15=1.00\nMEM_TOTAL_KIB=4096\nMEM_AVAILABLE_KIB=1024\nUPTIME_SECONDS=3661\nDISK_TOTAL_KIB=8192\nDISK_USED_KIB=2048\nDISK_AVAILABLE_KIB=6144\nDISK_USE_PERCENT=25\n",
        )
        .unwrap();
        assert_eq!(metrics.os, "Linux");
        assert_eq!(metrics.kernel, "6.8.0");
        assert_eq!(metrics.memory_total_kib, Some(4096));
        assert_eq!(metrics.disk_use_percent, Some(25));
        assert_eq!(metrics.uptime_seconds, Some(3661));
        assert_eq!(metrics.load_1m, Some(0.25));
    }

    #[test]
    fn server_metrics_parser_rejects_unmarked_or_empty_output() {
        assert!(parse_server_metrics("OS=Linux\n").is_err());
        assert!(parse_server_metrics("DSHELL_METRICS_V1\n").is_err());
    }

    #[test]
    fn server_process_parser_reads_bounded_rows() {
        let processes = parse_server_processes(
            "DSHELL_PROCESSES_V1\nPROC\t42\troot\t12.5\t1.2\tR\t/usr/bin/example --ready\nPROC\t7\tapp\t0.0\t0.4\tS\tsshd: app@pts/0\n",
        )
        .unwrap();
        assert_eq!(processes.len(), 2);
        assert_eq!(processes[0].pid, 42);
        assert_eq!(processes[0].user, "root");
        assert_eq!(processes[0].command, "/usr/bin/example --ready");
        assert_eq!(processes[1].state, "S");
    }

    #[test]
    fn server_process_parser_rejects_missing_marker_and_bad_rows() {
        assert!(parse_server_processes("PROC\t42\troot\t1.0\t1.0\tS\tsh\n").is_err());
        assert!(
            parse_server_processes("DSHELL_PROCESSES_V1\nPROC\t0\troot\t1.0\t1.0\tS\tsh\n")
                .is_err()
        );
        assert!(
            parse_server_processes("DSHELL_PROCESSES_V1\nPROC\t42\troot\tNaN\t1.0\tS\tsh\n")
                .is_err()
        );
        let mut too_many = String::from("DSHELL_PROCESSES_V1\n");
        for pid in 1..=51 {
            too_many.push_str(&format!("PROC\t{pid}\troot\t0.0\t0.0\tS\tprocess-{pid}\n"));
        }
        assert!(parse_server_processes(&too_many).is_err());
    }

    #[test]
    fn server_service_parser_reads_systemd_and_legacy_rows() {
        let services = parse_server_services(
            "DSHELL_SERVICES_V1\nSERVICE\tssh.service\tloaded\tactive\trunning\tOpenSSH server daemon\nSERVICE\tcron\tloaded\tinactive\tinactive\tcron scheduler\n",
        )
        .unwrap();
        assert_eq!(services.len(), 2);
        assert_eq!(services[0].name, "ssh.service");
        assert_eq!(services[0].active, "active");
        assert_eq!(services[1].description, "cron scheduler");
    }

    #[test]
    fn server_service_parser_rejects_missing_marker_and_bad_rows() {
        assert!(parse_server_services("SERVICE\tssh\tloaded\tactive\trunning\tssh\n").is_err());
        assert!(
            parse_server_services("DSHELL_SERVICES_V1\nSERVICE\tssh\tloaded\tactive\n").is_err()
        );
        assert!(
            parse_server_services("DSHELL_SERVICES_V1\nSERVICE\t\tloaded\tactive\trunning\tssh\n")
                .is_err()
        );
    }

    #[test]
    fn service_logs_parser_reads_bounded_output_and_status() {
        let logs = parse_service_logs(
            "DSHELL_SERVICE_LOGS_V1\n2026-10-25T10:00:00+0000 sshd started\n2026-10-25T10:01:00+0000 sshd ready\nDSHELL_SERVICE_LOGS_END\t0\n",
            "sshd.service",
        )
        .unwrap();
        assert_eq!(logs.name, "sshd.service");
        assert_eq!(logs.lines.len(), 2);
        assert!(logs.lines[1].contains("ready"));
        assert!(service_logs_command("sshd.service").contains("journalctl -u sshd.service -n 80"));
    }

    #[test]
    fn service_logs_parser_rejects_missing_or_failed_response() {
        assert!(parse_service_logs("journal output", "ssh").is_err());
        assert!(
            parse_service_logs(
                "DSHELL_SERVICE_LOGS_V1\nfailed\nDSHELL_SERVICE_LOGS_END\t1\n",
                "ssh",
            )
            .is_err()
        );
    }

    #[test]
    fn service_actions_validate_names_and_parse_status() {
        assert!(valid_service_name("ssh.service"));
        assert!(valid_service_name("serial-getty@ttyS0"));
        assert!(!valid_service_name("ssh; reboot"));
        assert!(!valid_service_name(""));
        assert!(
            service_action_command("ssh.service", ServiceAction::Restart)
                .contains("systemctl restart ssh.service")
        );
        assert_eq!(
            parse_service_action("stopped\nDSHELL_SERVICE_ACTION_V1\t0\n").unwrap(),
            (true, "stopped".to_owned())
        );
        assert_eq!(
            parse_service_action("failed\nDSHELL_SERVICE_ACTION_V1\t1\n").unwrap(),
            (false, "failed".to_owned())
        );
        assert!(parse_service_action("no marker").is_err());
    }

    #[test]
    fn power_actions_use_fixed_commands_and_parse_status() {
        assert_eq!(ServerPowerAction::Reboot.label(), "Reboot server");
        assert_eq!(ServerPowerAction::Shutdown.label(), "Shut down server");
        assert!(
            power_action_command(ServerPowerAction::Reboot).contains("systemctl --no-block reboot")
        );
        assert!(
            power_action_command(ServerPowerAction::Shutdown)
                .contains("systemctl --no-block poweroff")
        );
        assert_eq!(
            parse_power_action("queued\nDSHELL_POWER_ACTION_V1\t0\n").unwrap(),
            (true, "queued".to_owned())
        );
        assert_eq!(
            parse_power_action("permission denied\nDSHELL_POWER_ACTION_V1\t1\n").unwrap(),
            (false, "permission denied".to_owned())
        );
        assert!(parse_power_action("no marker").is_err());
    }

    #[test]
    fn process_termination_validates_pid_and_parses_status() {
        assert!(valid_process_pid(2));
        assert!(!valid_process_pid(0));
        assert!(!valid_process_pid(1));
        assert_eq!(
            process_termination_command(42),
            "kill -TERM 42; status=$?; printf 'DSHELL_PROCESS_ACTION_V1\\t%d\\n' \"$status\""
        );
        assert_eq!(
            parse_process_action("terminated\nDSHELL_PROCESS_ACTION_V1\t0\n").unwrap(),
            (true, "terminated".to_owned())
        );
        assert_eq!(
            parse_process_action("permission denied\nDSHELL_PROCESS_ACTION_V1\t1\n").unwrap(),
            (false, "permission denied".to_owned())
        );
        assert!(parse_process_action("no marker").is_err());
    }

    #[test]
    fn unknown_host_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("known_hosts");
        std::fs::write(&path, format!("[localhost]:13265 {KEY_A}\n")).unwrap();
        let key = PublicKey::from_openssh(KEY_A).unwrap();

        assert!(!is_known_host_key("other.example", 13265, &key, &path).unwrap());
    }

    #[test]
    fn changed_key_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("known_hosts");
        std::fs::write(&path, format!("[localhost]:13265 {KEY_A}\n")).unwrap();
        let changed_key = PublicKey::from_openssh(KEY_B).unwrap();

        assert!(is_known_host_key("localhost", 13265, &changed_key, &path).is_err());
    }

    #[test]
    fn keyboard_interactive_prompts_choose_between_password_and_ui() {
        let prompt = |text: &str, echo| Prompt {
            prompt: text.to_owned(),
            echo,
        };
        assert!(can_answer_with_password(
            &[prompt("Password: ", false)],
            false
        ));
        assert!(!can_answer_with_password(
            &[prompt("Password: ", false)],
            true
        ));
        assert!(!can_answer_with_password(
            &[prompt("Verification code: ", false)],
            false
        ));
        assert!(!can_answer_with_password(
            &[prompt("Password: ", true)],
            false
        ));
        assert!(!can_answer_with_password(
            &[
                prompt("Password: ", false),
                prompt("Verification code: ", false),
            ],
            false
        ));
        assert!(can_answer_with_password(&[], false));
    }

    #[tokio::test]
    async fn cancellation_interrupts_ssh_handshake() {
        assert_handshake_cancellation(false).await;
    }

    #[tokio::test]
    async fn dropping_cancellation_sender_interrupts_ssh_handshake() {
        assert_handshake_cancellation(true).await;
    }

    async fn assert_handshake_cancellation(drop_sender: bool) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            let _ = accepted_tx.send(());
            tokio::time::sleep(Duration::from_secs(10)).await;
        });

        let directory = tempfile::tempdir().unwrap();
        let (command_tx, command_rx) = mpsc::channel(8);
        let (event_tx, mut event_rx) = mpsc::channel(8);
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let spec = ConnectionSpec {
            host: address.ip().to_string(),
            port: address.port(),
            username: "test".to_owned(),
            authentication: Authentication::Password(Zeroizing::new("secret".to_owned())),
            terminal_size: TerminalSize {
                columns: 80,
                rows: 24,
            },
            jump: None,
            keepalive_seconds: 0,
            theme: TerminalTheme::Default,
            cursor_style: CursorStyle::Block,
        };
        let (sftp_commands, sftp_events, sftp_cancel) = sftp_test_channels();
        let (forward_commands, _forward_command_tx, forward_events, _forward_event_rx) =
            forward_test_channels();
        let connection = run_connection_with_known_hosts(
            spec,
            command_rx,
            event_tx,
            sftp_commands,
            sftp_events,
            sftp_cancel,
            forward_commands,
            forward_events,
            directory.path().join("known_hosts"),
            cancel_rx,
        );
        tokio::pin!(connection);

        let exercise = async {
            accepted_rx.await.unwrap();
            if drop_sender {
                drop(cancel_tx);
            } else {
                cancel_tx.send_replace(true);
            }
            assert!(matches!(
                timeout(Duration::from_secs(1), event_rx.recv())
                    .await
                    .unwrap(),
                Some(UiEvent::Disconnected)
            ));
            Ok::<(), anyhow::Error>(())
        };
        let result = timeout(Duration::from_secs(2), async {
            let (exercise_result, connection_result) = tokio::join!(exercise, &mut connection);
            exercise_result?;
            connection_result?;
            Ok::<(), anyhow::Error>(())
        })
        .await;
        server.abort();

        assert!(
            result.is_ok(),
            "cancellation did not interrupt SSH setup: {result:?}"
        );
        result.unwrap().unwrap();
        drop(command_tx);
    }

    #[test]
    #[ignore = "requires scripts/test-docker-ssh.ps1 to provide an Ubuntu SSH server"]
    fn docker_ssh_terminal_round_trip() {
        let host = env::var("DSHELL_DOCKER_SSH_HOST").expect("test host is not configured");
        let port = env::var("DSHELL_DOCKER_SSH_PORT")
            .expect("test port is not configured")
            .parse::<u16>()
            .expect("test port must be numeric");
        let username = env::var("DSHELL_DOCKER_SSH_USER").expect("test username is not configured");
        let password =
            env::var("DSHELL_DOCKER_SSH_PASSWORD").expect("test password is not configured");
        let known_hosts_path = PathBuf::from(
            env::var_os("DSHELL_DOCKER_SSH_KNOWN_HOSTS")
                .expect("isolated known_hosts path is not configured"),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime
            .block_on(async move {
                let (command_tx, command_rx) = mpsc::channel(32);
                let (event_tx, event_rx) = mpsc::channel(32);
                let (cancel_tx, cancel_rx) = watch::channel(false);
                let spec = ConnectionSpec {
                    host,
                    port,
                    username,
                    authentication: Authentication::Password(Zeroizing::new(password)),
                    terminal_size: TerminalSize {
                        columns: 80,
                        rows: 24,
                    },
                    jump: None,
                    keepalive_seconds: 0,
                    theme: TerminalTheme::Default,
                    cursor_style: CursorStyle::Block,
                };
                let (sftp_commands, sftp_events, sftp_cancel) = sftp_test_channels();
                let (
                    forward_commands,
                    _forward_command_tx,
                    forward_events,
                    _forward_event_rx,
                ) = forward_test_channels();
                let connection = run_connection_with_known_hosts(
                    spec,
                    command_rx,
                    event_tx,
                    sftp_commands,
                    sftp_events,
                    sftp_cancel,
                    forward_commands,
                    forward_events,
                    known_hosts_path,
                    cancel_rx,
                );
                tokio::pin!(connection);
                let _cancel_tx = cancel_tx;

                let exercise = async {
                    let mut events = event_rx;
                    let mut connection_info = None;
                    loop {
                        match timeout(Duration::from_secs(20), events.recv())
                            .await
                            .context("timed out waiting for SSH connection")?
                            .context("SSH event stream closed during connection")?
                        {
                            UiEvent::Status(status) if status.starts_with("Connected to ") => break,
                            UiEvent::ConnectionInfo(info) => connection_info = Some(info),
                            UiEvent::Failed(error) => bail!("SSH connection failed: {error}"),
                            UiEvent::Disconnected => bail!("SSH disconnected during connection"),
                            UiEvent::Status(_) | UiEvent::LogState(_) | UiEvent::Screen(_) | UiEvent::AuthPrompt { .. } | UiEvent::SearchResult(_) | UiEvent::ServerMetrics(_) | UiEvent::ServerMetricsError(_) => {}
                        }
                    }
                    let info = connection_info
                        .context("the SSH session did not report its host key information")?;
                    assert!(!info.host_key_algorithm.is_empty());
                    assert!(
                        info.host_key_fingerprint.starts_with("SHA256:"),
                        "{}",
                        info.host_key_fingerprint
                    );

                    command_tx
                        .send(TerminalRequest::Resize(TerminalSize {
                            columns: 100,
                            rows: 30,
                        }))
                        .await?;
                    next_screen(&mut events, |_| true)
                        .await
                        .context("waiting for the initial terminal screen")?;

                    command_tx
                        .send(TerminalRequest::SetTheme(TerminalTheme::SolarizedLight))
                        .await?;
                    next_screen(&mut events, |frame| {
                        frame.background == TerminalTheme::SolarizedLight.palette().background
                    })
                    .await
                    .context("waiting for the Solarized light terminal theme")?;
                    command_tx
                        .send(TerminalRequest::SetTheme(TerminalTheme::Default))
                        .await?;
                    next_screen(&mut events, |frame| {
                        frame.background == TerminalTheme::Default.palette().background
                    })
                    .await
                    .context("waiting for the default terminal theme")?;
                    command_tx
                        .send(TerminalRequest::SetCursorStyle(CursorStyle::Bar))
                        .await?;
                    next_screen(&mut events, |frame| {
                        frame.cursor.is_some_and(|cursor| cursor.style == CursorStyle::Bar)
                    })
                    .await
                    .context("waiting for the bar terminal cursor")?;
                    command_tx
                        .send(TerminalRequest::SetCursorStyle(CursorStyle::Block))
                        .await?;
                    next_screen(&mut events, |frame| {
                        frame
                            .cursor
                            .is_some_and(|cursor| cursor.style == CursorStyle::Block)
                    })
                    .await
                    .context("waiting for the block terminal cursor")?;

                    command_tx
                        .send(TerminalRequest::Paste("stty -echo".to_owned()))
                        .await?;
                    command_tx.send(TerminalRequest::Key(enter_key())).await?;
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    command_tx
                        .send(TerminalRequest::Paste("stty size".to_owned()))
                        .await?;
                    command_tx.send(TerminalRequest::Key(enter_key())).await?;
                    next_screen(&mut events, |frame| frame_text(frame).contains("30 100"))
                        .await
                        .context("waiting for the resized terminal dimensions")?;

                    command_tx
                        .send(TerminalRequest::Paste(
                            r"printf 'DSHELL_SSH_READY\n'".to_owned(),
                        ))
                        .await?;
                    command_tx.send(TerminalRequest::Key(enter_key())).await?;
                    let ready = next_screen(&mut events, |frame| {
                        frame_text(frame).contains("DSHELL_SSH_READY")
                    })
                    .await
                    .context("waiting for the shell command marker")?;
                    command_tx
                        .send(TerminalRequest::Search {
                            query: "dshell_ssh_ready".to_owned(),
                            direction: SearchDirection::Forward,
                        })
                        .await?;
                    loop {
                        match timeout(Duration::from_secs(20), events.recv())
                            .await
                            .context("timed out waiting for the terminal search result")?
                            .context("SSH event stream closed during terminal search")?
                        {
                            UiEvent::SearchResult(result) => {
                                assert!(result.found, "terminal search did not find the marker");
                                assert!(result.total >= 1);
                                break;
                            }
                            UiEvent::Failed(error) => bail!("SSH session failed: {error}"),
                            UiEvent::Disconnected => {
                                bail!("SSH disconnected during terminal search")
                            }
                            UiEvent::Status(_)
                            | UiEvent::LogState(_)
                            | UiEvent::Screen(_)
                            | UiEvent::ConnectionInfo(_)
                            | UiEvent::ServerMetrics(_)
                            | UiEvent::ServerMetricsError(_)
                            | UiEvent::AuthPrompt { .. } => {}
                        }
                    }
                    next_screen(&mut events, |frame| {
                        frame
                            .selection_text
                            .as_deref()
                            .is_some_and(|text| text.eq_ignore_ascii_case("DSHELL_SSH_READY"))
                    })
                    .await
                    .context("waiting for the terminal search selection")?;
                    let position = find_text(&ready, "DSHELL_SSH_READY")
                        .context("SSH output marker was not found in the terminal grid")?;
                    command_tx
                        .send(TerminalRequest::Mouse(
                            crate::terminal::TerminalMouseEvent::Press {
                                button: crate::terminal::TerminalMouseButton::Left,
                                position,
                                modifiers: crate::terminal::TerminalMouseModifiers::default(),
                            },
                        ))
                        .await?;
                    command_tx
                        .send(TerminalRequest::Mouse(
                            crate::terminal::TerminalMouseEvent::Move {
                                button: Some(crate::terminal::TerminalMouseButton::Left),
                                position: TerminalPosition {
                                    row: position.row,
                                    column: position.column + "DSHELL_SSH_READY".len() as u16 - 1,
                                },
                                modifiers: crate::terminal::TerminalMouseModifiers::default(),
                            },
                        ))
                        .await?;
                    next_screen(&mut events, |frame| {
                        frame
                            .selection_text
                            .as_deref()
                            .is_some_and(|text| text.contains("DSHELL_SSH_READY"))
                    })
                    .await
                    .context("waiting for terminal text selection")?;

                    command_tx
                        .send(TerminalRequest::Paste(
                            r"stty -echo -icanon min 1 time 0; printf '\033[?1000h'; dd bs=1 count=6 2>/dev/null | od -An -tx1; printf '\033[?1000l'; stty echo icanon; printf 'DSHELL_MOUSE_READY\n'"
                                .to_owned(),
                        ))
                        .await?;
                    command_tx.send(TerminalRequest::Key(enter_key())).await?;
                    next_screen(&mut events, |frame| {
                        frame.mouse_tracking_mode == crate::terminal::MouseTrackingMode::Click
                    })
                    .await
                    .context("waiting for click mouse tracking mode")?;
                    command_tx
                        .send(TerminalRequest::Mouse(
                            crate::terminal::TerminalMouseEvent::Press {
                                button: crate::terminal::TerminalMouseButton::Left,
                                position: TerminalPosition { row: 5, column: 6 },
                                modifiers: crate::terminal::TerminalMouseModifiers::default(),
                            },
                        ))
                        .await?;
                    next_screen(&mut events, |frame| {
                        frame.mouse_tracking_mode == crate::terminal::MouseTrackingMode::Disabled
                            && frame_text(frame).contains("1b 5b 4d 20 27 26")
                    })
                    .await
                    .context("waiting for the terminal mouse report")?;

                    command_tx
                        .send(TerminalRequest::Paste(
                            r"printf 'DSHELL_PASTE_OK\n'".to_owned(),
                        ))
                        .await?;
                    command_tx.send(TerminalRequest::Key(enter_key())).await?;
                    next_screen(&mut events, |frame| {
                        frame_text(frame).contains("DSHELL_PASTE_OK")
                    })
                    .await
                    .context("waiting for the pasted shell command marker")?;

                    command_tx
                        .send(TerminalRequest::Paste(
                            "export PS1='DSHELL_SSH_PROMPT> '; printf 'DSHELL_SSH_PROMPT_READY\\n'"
                                .to_owned(),
                        ))
                        .await?;
                    command_tx.send(TerminalRequest::Key(enter_key())).await?;
                    next_screen(&mut events, |frame| {
                        frame_text(frame).contains("DSHELL_SSH_PROMPT_READY")
                    })
                    .await
                    .context("waiting for the test shell prompt setup")?;

                    let vim_text = "DSHELL_VIM_中文🙂e\u{301}";
                    command_tx
                        .send(TerminalRequest::Paste(
                            "vim -Nu NONE -n /tmp/dshell-vim-smoke".to_owned(),
                        ))
                        .await?;
                    command_tx.send(TerminalRequest::Key(enter_key())).await?;
                    next_screen(&mut events, |frame| {
                        let text = frame_text(frame);
                        text.contains("[New File]") || text.contains("[New]")
                    })
                    .await
                    .context("waiting for Vim to open the new file")?;
                    command_tx
                        .send(TerminalRequest::Key(text_key("i")))
                        .await?;
                    command_tx
                        .send(TerminalRequest::Key(text_key(vim_text)))
                        .await?;
                    command_tx
                        .send(TerminalRequest::Key(TerminalKeyInput {
                            key: "escape".to_owned(),
                            text: None,
                            control: false,
                            alt: false,
                            shift: false,
                            platform: false,
                            function: false,
                        }))
                        .await?;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    next_screen(&mut events, |frame| frame_text(frame).contains(vim_text))
                        .await
                        .context("waiting for Vim to display Unicode text")?;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    command_tx
                        .send(TerminalRequest::Key(text_key(":wq")))
                        .await?;
                    command_tx.send(TerminalRequest::Key(enter_key())).await?;
                    next_screen(&mut events, |frame| {
                        frame_text(frame).contains("DSHELL_SSH_PROMPT> ")
                    })
                    .await
                    .context("waiting for the shell prompt after Vim exits")?;
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    command_tx
                        .send(TerminalRequest::Paste(
                            "cat /tmp/dshell-vim-smoke; printf '\\nDSHELL_VIM_READBACK_DONE\\n'"
                                .to_owned(),
                        ))
                        .await?;
                    command_tx.send(TerminalRequest::Key(enter_key())).await?;
                    let readback = next_screen(&mut events, |frame| {
                        frame_has_output_line(frame, "DSHELL_VIM_READBACK_DONE")
                    })
                    .await
                    .context("waiting for Vim file readback")?;
                    let readback_text = frame_text(&readback);
                    if !readback_text.contains(vim_text) {
                        command_tx
                            .send(TerminalRequest::Paste(
                                "od -An -tx1 /tmp/dshell-vim-smoke; printf '\\nDSHELL_VIM_HEX_DONE\\n'"
                                    .to_owned(),
                            ))
                            .await?;
                        command_tx.send(TerminalRequest::Key(enter_key())).await?;
                        let hex = next_screen(&mut events, |frame| {
                            frame_has_output_line(frame, "DSHELL_VIM_HEX_DONE")
                        })
                        .await?;
                        bail!(
                            "Vim file readback did not preserve Unicode text; screen:\n{readback_text}\nfile bytes:\n{}",
                            frame_text(&hex)
                        );
                    }

                    command_tx
                        .send(TerminalRequest::Paste("exit".to_owned()))
                        .await?;
                    command_tx.send(TerminalRequest::Key(enter_key())).await?;
                    loop {
                        match timeout(Duration::from_secs(20), events.recv())
                            .await
                            .context("timed out waiting for SSH session to exit")?
                            .context("SSH event stream closed before disconnect")?
                        {
                            UiEvent::Disconnected => break,
                            UiEvent::Failed(error) => bail!("SSH session failed: {error}"),
                            UiEvent::Status(_) | UiEvent::LogState(_) | UiEvent::Screen(_) | UiEvent::AuthPrompt { .. } | UiEvent::SearchResult(_) | UiEvent::ConnectionInfo(_) | UiEvent::ServerMetrics(_) | UiEvent::ServerMetricsError(_) => {}
                        }
                    }
                    Ok::<(), anyhow::Error>(())
                };

                let result = timeout(Duration::from_secs(60), async {
                    tokio::select! {
                        result = exercise => {
                            result?;
                            connection.await
                        }
                        result = &mut connection => result,
                    }
                })
                .await
                .context("Docker SSH terminal test exceeded 60 seconds")?;
                result?;
                Ok::<(), anyhow::Error>(())
            })
            .unwrap();
    }

    #[test]
    #[ignore = "requires scripts/test-docker-ssh.ps1 to provide an Ubuntu SSH server"]
    fn docker_ssh_explicit_disconnect() {
        let host = env::var("DSHELL_DOCKER_SSH_HOST").expect("test host is not configured");
        let port = env::var("DSHELL_DOCKER_SSH_PORT")
            .expect("test port is not configured")
            .parse::<u16>()
            .expect("test port must be numeric");
        let username = env::var("DSHELL_DOCKER_SSH_USER").expect("test username is not configured");
        let password =
            env::var("DSHELL_DOCKER_SSH_PASSWORD").expect("test password is not configured");
        let known_hosts_path = PathBuf::from(
            env::var_os("DSHELL_DOCKER_SSH_KNOWN_HOSTS")
                .expect("isolated known_hosts path is not configured"),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime
            .block_on(async move {
                let (_command_tx, command_rx) = mpsc::channel(32);
                let (event_tx, mut event_rx) = mpsc::channel(32);
                let (cancel_tx, cancel_rx) = watch::channel(false);
                let spec = ConnectionSpec {
                    host,
                    port,
                    username,
                    authentication: Authentication::Password(Zeroizing::new(password)),
                    terminal_size: TerminalSize {
                        columns: 80,
                        rows: 24,
                    },
                    jump: None,
                    keepalive_seconds: 0,
                    theme: TerminalTheme::Default,
                    cursor_style: CursorStyle::Block,
                };
                let (sftp_commands, sftp_events, sftp_cancel) = sftp_test_channels();
                let (forward_commands, _forward_command_tx, forward_events, _forward_event_rx) =
                    forward_test_channels();
                let connection = run_connection_with_known_hosts(
                    spec,
                    command_rx,
                    event_tx,
                    sftp_commands,
                    sftp_events,
                    sftp_cancel,
                    forward_commands,
                    forward_events,
                    known_hosts_path,
                    cancel_rx,
                );
                tokio::pin!(connection);

                let exercise = async {
                    loop {
                        match timeout(Duration::from_secs(20), event_rx.recv())
                            .await
                            .context("timed out waiting for SSH connection")?
                            .context("SSH event stream closed during connection")?
                        {
                            UiEvent::Status(status) if status.starts_with("Connected to ") => break,
                            UiEvent::Failed(error) => bail!("SSH connection failed: {error}"),
                            UiEvent::Disconnected => bail!("SSH disconnected during connection"),
                            UiEvent::Status(_)
                            | UiEvent::LogState(_)
                            | UiEvent::Screen(_)
                            | UiEvent::AuthPrompt { .. }
                            | UiEvent::SearchResult(_)
                            | UiEvent::ConnectionInfo(_)
                            | UiEvent::ServerMetrics(_)
                            | UiEvent::ServerMetricsError(_) => {}
                        }
                    }

                    cancel_tx.send_replace(true);
                    loop {
                        match timeout(Duration::from_secs(20), event_rx.recv())
                            .await
                            .context("timed out waiting for explicit disconnect")?
                            .context("SSH event stream closed before disconnect")?
                        {
                            UiEvent::Disconnected => break,
                            UiEvent::Failed(error) => bail!("SSH session failed: {error}"),
                            UiEvent::Status(_)
                            | UiEvent::LogState(_)
                            | UiEvent::Screen(_)
                            | UiEvent::AuthPrompt { .. }
                            | UiEvent::SearchResult(_)
                            | UiEvent::ConnectionInfo(_)
                            | UiEvent::ServerMetrics(_)
                            | UiEvent::ServerMetricsError(_) => {}
                        }
                    }
                    Ok::<(), anyhow::Error>(())
                };

                let result = timeout(Duration::from_secs(45), async {
                    tokio::select! {
                        result = exercise => {
                            result?;
                            connection.await
                        }
                        result = &mut connection => result,
                    }
                })
                .await
                .context("Docker SSH explicit-disconnect test exceeded 45 seconds")?;
                result?;
                Ok::<(), anyhow::Error>(())
            })
            .unwrap();
    }

    #[test]
    #[ignore = "requires scripts/test-docker-ssh.ps1 to provide an Ubuntu SSH server"]
    fn docker_ssh_session_logging() {
        let host = env::var("DSHELL_DOCKER_SSH_HOST").expect("test host is not configured");
        let port = env::var("DSHELL_DOCKER_SSH_PORT")
            .expect("test port is not configured")
            .parse::<u16>()
            .expect("test port must be numeric");
        let username = env::var("DSHELL_DOCKER_SSH_USER").expect("test username is not configured");
        let password =
            env::var("DSHELL_DOCKER_SSH_PASSWORD").expect("test password is not configured");
        let known_hosts_path = PathBuf::from(
            env::var_os("DSHELL_DOCKER_SSH_KNOWN_HOSTS")
                .expect("isolated known_hosts path is not configured"),
        );
        let directory = tempfile::tempdir().unwrap();
        let log_path = directory.path().join("session.log");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime
            .block_on(async move {
                let (command_tx, command_rx) = mpsc::channel(32);
                let (event_tx, mut event_rx) = mpsc::channel(32);
                let (cancel_tx, cancel_rx) = watch::channel(false);
                let (sftp_commands, sftp_events, sftp_cancel) = sftp_test_channels();
                let (forward_commands, _forward_command_tx, forward_events, _forward_event_rx) =
                    forward_test_channels();
                let spec = ConnectionSpec {
                    host,
                    port,
                    username,
                    authentication: Authentication::Password(Zeroizing::new(password)),
                    terminal_size: TerminalSize {
                        columns: 80,
                        rows: 24,
                    },
                    jump: None,
                    keepalive_seconds: 0,
                    theme: TerminalTheme::Default,
                    cursor_style: CursorStyle::Block,
                };
                let connection = run_connection_with_known_hosts(
                    spec,
                    command_rx,
                    event_tx,
                    sftp_commands,
                    sftp_events,
                    sftp_cancel,
                    forward_commands,
                    forward_events,
                    known_hosts_path,
                    cancel_rx,
                );
                tokio::pin!(connection);

                let exercise = async {
                    loop {
                        match timeout(Duration::from_secs(20), event_rx.recv())
                            .await
                            .context("timed out waiting for the SSH session")?
                            .context("SSH event stream closed during connection")?
                        {
                            UiEvent::Status(status) if status.starts_with("Connected to ") => break,
                            UiEvent::Failed(error) => bail!("SSH connection failed: {error}"),
                            UiEvent::Disconnected => bail!("SSH disconnected during connection"),
                            UiEvent::Status(_)
                            | UiEvent::LogState(_)
                            | UiEvent::Screen(_)
                            | UiEvent::AuthPrompt { .. }
                            | UiEvent::SearchResult(_)
                            | UiEvent::ConnectionInfo(_)
                            | UiEvent::ServerMetrics(_)
                            | UiEvent::ServerMetricsError(_) => {}
                        }
                    }

                    command_tx
                        .send(TerminalRequest::StartLog(log_path.clone()))
                        .await?;
                    loop {
                        match timeout(Duration::from_secs(20), event_rx.recv())
                            .await
                            .context("timed out waiting for the logging state")?
                            .context("SSH event stream closed while starting logging")?
                        {
                            UiEvent::LogState(Some(path)) if path == log_path => break,
                            UiEvent::Failed(error) => bail!("SSH session failed: {error}"),
                            UiEvent::Disconnected => bail!("SSH disconnected while logging"),
                            UiEvent::Status(_)
                            | UiEvent::LogState(_)
                            | UiEvent::Screen(_)
                            | UiEvent::AuthPrompt { .. }
                            | UiEvent::SearchResult(_)
                            | UiEvent::ConnectionInfo(_)
                            | UiEvent::ServerMetrics(_)
                            | UiEvent::ServerMetricsError(_) => {}
                        }
                    }

                    command_tx
                        .send(TerminalRequest::Paste(
                            r"printf 'DSHELL_LOG_MARKER\n'".to_owned(),
                        ))
                        .await?;
                    command_tx.send(TerminalRequest::Key(enter_key())).await?;
                    next_screen(&mut event_rx, |frame| {
                        frame_text(frame).contains("DSHELL_LOG_MARKER")
                    })
                    .await
                    .context("waiting for the logged terminal output")?;

                    command_tx.send(TerminalRequest::StopLog).await?;
                    loop {
                        match timeout(Duration::from_secs(20), event_rx.recv())
                            .await
                            .context("timed out waiting for logging to stop")?
                            .context("SSH event stream closed while stopping logging")?
                        {
                            UiEvent::LogState(None) => break,
                            UiEvent::Failed(error) => bail!("SSH session failed: {error}"),
                            UiEvent::Disconnected => bail!("SSH disconnected while logging"),
                            UiEvent::Status(_)
                            | UiEvent::LogState(_)
                            | UiEvent::Screen(_)
                            | UiEvent::AuthPrompt { .. }
                            | UiEvent::SearchResult(_)
                            | UiEvent::ConnectionInfo(_)
                            | UiEvent::ServerMetrics(_)
                            | UiEvent::ServerMetricsError(_) => {}
                        }
                    }
                    let replay = crate::log::read_replay(&log_path, 16 * 1024 * 1024)
                        .map_err(anyhow::Error::msg)
                        .context("the terminal log file is unreadable")?;
                    assert!(replay.timestamped);
                    assert!(
                        !replay.timeline.is_empty(),
                        "the timestamped log did not contain a timeline"
                    );
                    assert!(
                        replay
                            .timeline
                            .windows(2)
                            .all(|pair| pair[0].timestamp_ms <= pair[1].timestamp_ms),
                        "log timestamps must not decrease"
                    );
                    assert!(
                        String::from_utf8_lossy(&replay.bytes).contains("DSHELL_LOG_MARKER"),
                        "the terminal log did not contain the remote output"
                    );

                    cancel_tx.send_replace(true);
                    loop {
                        match timeout(Duration::from_secs(20), event_rx.recv())
                            .await
                            .context("timed out waiting for the logging session cleanup")?
                            .context("SSH event stream closed before disconnect")?
                        {
                            UiEvent::Disconnected => break,
                            UiEvent::Failed(error) => bail!("SSH session failed: {error}"),
                            UiEvent::Status(_)
                            | UiEvent::LogState(_)
                            | UiEvent::Screen(_)
                            | UiEvent::AuthPrompt { .. }
                            | UiEvent::SearchResult(_)
                            | UiEvent::ConnectionInfo(_)
                            | UiEvent::ServerMetrics(_)
                            | UiEvent::ServerMetricsError(_) => {}
                        }
                    }
                    Ok::<(), anyhow::Error>(())
                };

                let result = timeout(Duration::from_secs(60), async {
                    tokio::select! {
                        result = exercise => {
                            result?;
                            connection.await
                        }
                        result = &mut connection => result,
                    }
                })
                .await
                .context("Docker terminal logging test exceeded 60 seconds")?;
                result?;
                Ok::<(), anyhow::Error>(())
            })
            .unwrap();
    }

    #[test]
    #[ignore = "requires scripts/test-docker-ssh.ps1 to provide an Ubuntu SSH server"]
    fn docker_ssh_jump_host_round_trip() {
        let jump_host = env::var("DSHELL_DOCKER_SSH_HOST").expect("test host is not configured");
        let jump_port = env::var("DSHELL_DOCKER_SSH_PORT")
            .expect("test port is not configured")
            .parse::<u16>()
            .expect("test port must be numeric");
        let username = env::var("DSHELL_DOCKER_SSH_USER").expect("test username is not configured");
        let password =
            env::var("DSHELL_DOCKER_SSH_PASSWORD").expect("test password is not configured");
        let known_hosts_path = PathBuf::from(
            env::var_os("DSHELL_DOCKER_SSH_KNOWN_HOSTS")
                .expect("isolated known_hosts path is not configured"),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime
            .block_on(async move {
                let (command_tx, command_rx) = mpsc::channel(32);
                let (event_tx, mut event_rx) = mpsc::channel(32);
                let (cancel_tx, cancel_rx) = watch::channel(false);
                let (sftp_commands, sftp_events, sftp_cancel) = sftp_test_channels();
                let (forward_commands, _forward_command_tx, forward_events, _forward_event_rx) =
                    forward_test_channels();
                let spec = ConnectionSpec {
                    host: "127.0.0.1".to_owned(),
                    port: 22,
                    username: username.clone(),
                    authentication: Authentication::Password(Zeroizing::new(password.clone())),
                    terminal_size: TerminalSize {
                        columns: 80,
                        rows: 24,
                    },
                    jump: Some(JumpSpec {
                        host: jump_host,
                        port: jump_port,
                        username,
                        authentication: Authentication::Password(Zeroizing::new(password)),
                    }),
                    keepalive_seconds: 0,
                    theme: TerminalTheme::Default,
                    cursor_style: CursorStyle::Block,
                };
                let connection = run_connection_with_known_hosts(
                    spec,
                    command_rx,
                    event_tx,
                    sftp_commands,
                    sftp_events,
                    sftp_cancel,
                    forward_commands,
                    forward_events,
                    known_hosts_path,
                    cancel_rx,
                );
                tokio::pin!(connection);

                let exercise = async {
                    loop {
                        match timeout(Duration::from_secs(20), event_rx.recv())
                            .await
                            .context("timed out waiting for the jump host connection")?
                            .context("SSH event stream closed during connection")?
                        {
                            UiEvent::Status(status) if status.starts_with("Connected to ") => break,
                            UiEvent::Failed(error) => bail!("SSH connection failed: {error}"),
                            UiEvent::Disconnected => bail!("SSH disconnected during connection"),
                            UiEvent::Status(_)
                            | UiEvent::LogState(_)
                            | UiEvent::Screen(_)
                            | UiEvent::AuthPrompt { .. }
                            | UiEvent::SearchResult(_)
                            | UiEvent::ConnectionInfo(_)
                            | UiEvent::ServerMetrics(_)
                            | UiEvent::ServerMetricsError(_) => {}
                        }
                    }

                    command_tx
                        .send(TerminalRequest::Paste(
                            r"printf 'DSHELL_JUMP_READY\n'".to_owned(),
                        ))
                        .await?;
                    command_tx.send(TerminalRequest::Key(enter_key())).await?;
                    next_screen(&mut event_rx, |frame| {
                        frame_text(frame).contains("DSHELL_JUMP_READY")
                    })
                    .await
                    .context("waiting for shell output through the jump host")?;

                    cancel_tx.send_replace(true);
                    loop {
                        match timeout(Duration::from_secs(20), event_rx.recv())
                            .await
                            .context("timed out waiting for jump host disconnect")?
                            .context("SSH event stream closed before disconnect")?
                        {
                            UiEvent::Disconnected => break,
                            UiEvent::Failed(error) => bail!("SSH session failed: {error}"),
                            UiEvent::Status(_)
                            | UiEvent::LogState(_)
                            | UiEvent::Screen(_)
                            | UiEvent::AuthPrompt { .. }
                            | UiEvent::SearchResult(_)
                            | UiEvent::ConnectionInfo(_)
                            | UiEvent::ServerMetrics(_)
                            | UiEvent::ServerMetricsError(_) => {}
                        }
                    }
                    Ok::<(), anyhow::Error>(())
                };

                let result = timeout(Duration::from_secs(60), async {
                    tokio::select! {
                        result = exercise => {
                            result?;
                            connection.await
                        }
                        result = &mut connection => result,
                    }
                })
                .await
                .context("Docker jump host test exceeded 60 seconds")?;
                result?;
                Ok::<(), anyhow::Error>(())
            })
            .unwrap();
    }

    #[test]
    #[ignore = "requires scripts/test-docker-ssh.ps1 to provide an Ubuntu SSH server"]
    fn docker_ssh_keyboard_interactive_authentication() {
        let host = env::var("DSHELL_DOCKER_SSH_HOST").expect("test host is not configured");
        let port = env::var("DSHELL_DOCKER_SSH_PORT")
            .expect("test port is not configured")
            .parse::<u16>()
            .expect("test port must be numeric");
        let username = env::var("DSHELL_DOCKER_SSH_USER").expect("test username is not configured");
        let password =
            env::var("DSHELL_DOCKER_SSH_PASSWORD").expect("test password is not configured");
        let known_hosts_path = PathBuf::from(
            env::var_os("DSHELL_DOCKER_SSH_KNOWN_HOSTS")
                .expect("isolated known_hosts path is not configured"),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime
            .block_on(async move {
                let (command_tx, command_rx) = mpsc::channel(32);
                let (event_tx, mut event_rx) = mpsc::channel(32);
                let (cancel_tx, cancel_rx) = watch::channel(false);
                let (sftp_commands, sftp_events, sftp_cancel) = sftp_test_channels();
                let (forward_commands, _forward_command_tx, forward_events, _forward_event_rx) =
                    forward_test_channels();
                let spec = ConnectionSpec {
                    host,
                    port,
                    username,
                    authentication: Authentication::KeyboardInteractive {
                        password: Zeroizing::new(password),
                        interactive: false,
                    },
                    terminal_size: TerminalSize {
                        columns: 80,
                        rows: 24,
                    },
                    jump: None,
                    keepalive_seconds: 0,
                    theme: TerminalTheme::Default,
                    cursor_style: CursorStyle::Block,
                };
                let connection = run_connection_with_known_hosts(
                    spec,
                    command_rx,
                    event_tx,
                    sftp_commands,
                    sftp_events,
                    sftp_cancel,
                    forward_commands,
                    forward_events,
                    known_hosts_path,
                    cancel_rx,
                );
                tokio::pin!(connection);

                let exercise = async {
                    loop {
                        match timeout(Duration::from_secs(20), event_rx.recv())
                            .await
                            .context("timed out waiting for keyboard-interactive authentication")?
                            .context("SSH event stream closed during authentication")?
                        {
                            UiEvent::Status(status) if status.starts_with("Connected to ") => break,
                            UiEvent::Failed(error) => bail!("SSH connection failed: {error}"),
                            UiEvent::Disconnected => bail!("SSH disconnected during connection"),
                            UiEvent::Status(_)
                            | UiEvent::LogState(_)
                            | UiEvent::Screen(_)
                            | UiEvent::AuthPrompt { .. }
                            | UiEvent::SearchResult(_)
                            | UiEvent::ConnectionInfo(_)
                            | UiEvent::ServerMetrics(_)
                            | UiEvent::ServerMetricsError(_) => {}
                        }
                    }

                    cancel_tx.send_replace(true);
                    loop {
                        match timeout(Duration::from_secs(20), event_rx.recv())
                            .await
                            .context("timed out waiting for keyboard-interactive session cleanup")?
                            .context("SSH event stream closed before disconnect")?
                        {
                            UiEvent::Disconnected => break,
                            UiEvent::Failed(error) => bail!("SSH session failed: {error}"),
                            UiEvent::Status(_)
                            | UiEvent::LogState(_)
                            | UiEvent::Screen(_)
                            | UiEvent::AuthPrompt { .. }
                            | UiEvent::SearchResult(_)
                            | UiEvent::ConnectionInfo(_)
                            | UiEvent::ServerMetrics(_)
                            | UiEvent::ServerMetricsError(_) => {}
                        }
                    }
                    Ok::<(), anyhow::Error>(())
                };

                let result = timeout(Duration::from_secs(45), async {
                    tokio::select! {
                        result = exercise => {
                            result?;
                            connection.await
                        }
                        result = &mut connection => result,
                    }
                })
                .await
                .context("Docker keyboard-interactive test exceeded 45 seconds")?;
                result?;
                drop(command_tx);
                Ok::<(), anyhow::Error>(())
            })
            .unwrap();
    }

    #[test]
    #[ignore = "requires scripts/test-docker-ssh.ps1 to provide an Ubuntu SSH server"]
    fn docker_ssh_public_key_authentication() {
        let (identity_file, passphrase) = docker_identity_settings();
        assert_public_key_connection(identity_file, passphrase);
    }

    #[test]
    #[ignore = "requires scripts/test-docker-ssh.ps1 to provide an Ubuntu SSH server"]
    fn docker_ssh_public_key_wrong_passphrase_is_rejected() {
        let (identity_file, _) = docker_identity_settings();
        let host = env::var("DSHELL_DOCKER_SSH_HOST").expect("test host is not configured");
        let port = env::var("DSHELL_DOCKER_SSH_PORT")
            .expect("test port is not configured")
            .parse::<u16>()
            .expect("test port must be numeric");
        let username = env::var("DSHELL_DOCKER_SSH_USER").expect("test username is not configured");
        let known_hosts_path = PathBuf::from(
            env::var_os("DSHELL_DOCKER_SSH_KNOWN_HOSTS")
                .expect("isolated known_hosts path is not configured"),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async move {
            let (_command_tx, command_rx) = mpsc::channel(8);
            let (event_tx, _event_rx) = mpsc::channel(8);
            let (cancel_tx, cancel_rx) = watch::channel(false);
            let _cancel_tx = cancel_tx;
            let spec = ConnectionSpec {
                host,
                port,
                username,
                authentication: Authentication::PublicKey {
                    identity_file,
                    passphrase: Zeroizing::new("definitely-the-wrong-passphrase".to_owned()),
                },
                terminal_size: TerminalSize {
                    columns: 80,
                    rows: 24,
                },
                jump: None,
                keepalive_seconds: 0,
                theme: TerminalTheme::Default,
                cursor_style: CursorStyle::Block,
            };
            let (sftp_commands, sftp_events, sftp_cancel) = sftp_test_channels();
            let (forward_commands, _forward_command_tx, forward_events, _forward_event_rx) =
                forward_test_channels();
            let error = run_connection_with_known_hosts(
                spec,
                command_rx,
                event_tx,
                sftp_commands,
                sftp_events,
                sftp_cancel,
                forward_commands,
                forward_events,
                known_hosts_path,
                cancel_rx,
            )
            .await
            .expect_err("a wrong key passphrase must fail the connection");
            let message = format!("{error:#}");
            assert!(
                message.contains("identity file"),
                "the failure did not identify the unreadable identity file: {message}"
            );
        });
    }

    fn docker_identity_settings() -> (PathBuf, String) {
        (
            PathBuf::from(
                env::var_os("DSHELL_DOCKER_SSH_IDENTITY")
                    .expect("test identity file is not configured"),
            ),
            env::var("DSHELL_DOCKER_SSH_KEY_PASSPHRASE")
                .expect("test identity passphrase is not configured"),
        )
    }

    fn assert_public_key_connection(identity_file: PathBuf, passphrase: String) {
        let host = env::var("DSHELL_DOCKER_SSH_HOST").expect("test host is not configured");
        let port = env::var("DSHELL_DOCKER_SSH_PORT")
            .expect("test port is not configured")
            .parse::<u16>()
            .expect("test port must be numeric");
        let username = env::var("DSHELL_DOCKER_SSH_USER").expect("test username is not configured");
        let known_hosts_path = PathBuf::from(
            env::var_os("DSHELL_DOCKER_SSH_KNOWN_HOSTS")
                .expect("isolated known_hosts path is not configured"),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime
            .block_on(async move {
                let (command_tx, command_rx) = mpsc::channel(32);
                let (event_tx, mut event_rx) = mpsc::channel(32);
                let (cancel_tx, cancel_rx) = watch::channel(false);
                let spec = ConnectionSpec {
                    host,
                    port,
                    username,
                    authentication: Authentication::PublicKey {
                        identity_file,
                        passphrase: Zeroizing::new(passphrase),
                    },
                    terminal_size: TerminalSize {
                        columns: 80,
                        rows: 24,
                    },
                    jump: None,
                    keepalive_seconds: 0,
                    theme: TerminalTheme::Default,
                    cursor_style: CursorStyle::Block,
                };
                let (sftp_commands, sftp_events, sftp_cancel) = sftp_test_channels();
                let (forward_commands, _forward_command_tx, forward_events, _forward_event_rx) =
                    forward_test_channels();
                let connection = run_connection_with_known_hosts(
                    spec,
                    command_rx,
                    event_tx,
                    sftp_commands,
                    sftp_events,
                    sftp_cancel,
                    forward_commands,
                    forward_events,
                    known_hosts_path,
                    cancel_rx,
                );
                tokio::pin!(connection);

                let exercise = async {
                    loop {
                        match timeout(Duration::from_secs(20), event_rx.recv())
                            .await
                            .context("timed out waiting for SSH public key authentication")?
                            .context("SSH event stream closed during authentication")?
                        {
                            UiEvent::Status(status) if status.starts_with("Connected to ") => break,
                            UiEvent::Failed(error) => bail!("SSH connection failed: {error}"),
                            UiEvent::Disconnected => bail!("SSH disconnected during connection"),
                            UiEvent::Status(_)
                            | UiEvent::LogState(_)
                            | UiEvent::Screen(_)
                            | UiEvent::AuthPrompt { .. }
                            | UiEvent::SearchResult(_)
                            | UiEvent::ConnectionInfo(_)
                            | UiEvent::ServerMetrics(_)
                            | UiEvent::ServerMetricsError(_) => {}
                        }
                    }

                    cancel_tx.send_replace(true);
                    loop {
                        match timeout(Duration::from_secs(20), event_rx.recv())
                            .await
                            .context("timed out waiting for public key session cleanup")?
                            .context("SSH event stream closed before disconnect")?
                        {
                            UiEvent::Disconnected => break,
                            UiEvent::Failed(error) => bail!("SSH session failed: {error}"),
                            UiEvent::Status(_)
                            | UiEvent::LogState(_)
                            | UiEvent::Screen(_)
                            | UiEvent::AuthPrompt { .. }
                            | UiEvent::SearchResult(_)
                            | UiEvent::ConnectionInfo(_)
                            | UiEvent::ServerMetrics(_)
                            | UiEvent::ServerMetricsError(_) => {}
                        }
                    }
                    Ok::<(), anyhow::Error>(())
                };

                let result = timeout(Duration::from_secs(45), async {
                    tokio::select! {
                        result = exercise => {
                            result?;
                            connection.await
                        }
                        result = &mut connection => result,
                    }
                })
                .await
                .context("Docker SSH public key test exceeded 45 seconds")?;
                result?;
                drop(command_tx);
                Ok::<(), anyhow::Error>(())
            })
            .unwrap();
    }

    #[test]
    #[ignore = "requires scripts/test-docker-ssh.ps1 to provide an Ubuntu SSH server"]
    fn docker_ssh_sftp_round_trip() {
        let host = env::var("DSHELL_DOCKER_SSH_HOST").expect("test host is not configured");
        let port = env::var("DSHELL_DOCKER_SSH_PORT")
            .expect("test port is not configured")
            .parse::<u16>()
            .expect("test port must be numeric");
        let username = env::var("DSHELL_DOCKER_SSH_USER").expect("test username is not configured");
        let password =
            env::var("DSHELL_DOCKER_SSH_PASSWORD").expect("test password is not configured");
        let known_hosts_path = PathBuf::from(
            env::var_os("DSHELL_DOCKER_SSH_KNOWN_HOSTS")
                .expect("isolated known_hosts path is not configured"),
        );
        let directory = tempfile::tempdir().unwrap();
        let upload_path = directory.path().join("upload.txt");
        let download_path = directory.path().join("download.txt");
        let failed_download_path = directory.path().join("unexpected.txt");
        let tree_upload = directory.path().join("tree-upload");
        let tree_download = directory.path().join("tree-download");
        let resume_remote = "dshell-sftp-resume.txt".to_owned();
        let resume_full_local = directory.path().join("resume-full.txt");
        let resume_partial_local = directory.path().join("resume-partial.txt");
        let resume_download = directory.path().join("resume-download.txt");
        let resume_content = (0..100_000u32)
            .map(|index| b'a' + (index % 26) as u8)
            .collect::<Vec<_>>();
        std::fs::write(&resume_full_local, &resume_content).unwrap();
        std::fs::write(&resume_partial_local, &resume_content[..40_000]).unwrap();
        std::fs::create_dir_all(tree_upload.join("nested")).unwrap();
        std::fs::write(tree_upload.join("nested/file.txt"), b"nested content").unwrap();
        let content = b"dshell sftp round trip\nUnicode: \xe4\xb8\xad\xe6\x96\x87\n";
        std::fs::write(&upload_path, content).unwrap();
        let remote = "dshell-sftp-round-trip.txt".to_owned();
        let remote_name = remote.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime
            .block_on(async move {
                let (_command_tx, command_rx) = mpsc::channel(32);
                let (event_tx, mut event_rx) = mpsc::channel(32);
                let (cancel_tx, cancel_rx) = watch::channel(false);
                let (sftp_command_tx, sftp_command_rx) = mpsc::channel(16);
                let (sftp_event_tx, mut sftp_event_rx) = mpsc::channel(32);
                let sftp_cancel = Arc::new(AtomicBool::new(false));
                let (forward_commands, _forward_command_tx, forward_events, _forward_event_rx) =
                    forward_test_channels();
                let spec = ConnectionSpec {
                    host,
                    port,
                    username,
                    authentication: Authentication::Password(Zeroizing::new(password)),
                    terminal_size: TerminalSize {
                        columns: 80,
                        rows: 24,
                    },
                    jump: None,
                    keepalive_seconds: 0,
                    theme: TerminalTheme::Default,
                    cursor_style: CursorStyle::Block,
                };
                let connection = run_connection_with_known_hosts(
                    spec,
                    command_rx,
                    event_tx,
                    sftp_command_rx,
                    sftp_event_tx,
                    sftp_cancel,
                    forward_commands,
                    forward_events,
                    known_hosts_path,
                    cancel_rx,
                );
                tokio::pin!(connection);

                let exercise = async {
                    loop {
                        match timeout(Duration::from_secs(20), event_rx.recv())
                            .await
                            .context("timed out waiting for the SSH session")?
                            .context("SSH event stream closed during connection")?
                        {
                            UiEvent::Status(status) if status.starts_with("Connected to ") => break,
                            UiEvent::Failed(error) => bail!("SSH connection failed: {error}"),
                            UiEvent::Disconnected => bail!("SSH disconnected during connection"),
                            UiEvent::Status(_) | UiEvent::LogState(_) | UiEvent::Screen(_) | UiEvent::AuthPrompt { .. } | UiEvent::SearchResult(_) | UiEvent::ConnectionInfo(_) | UiEvent::ServerMetrics(_) | UiEvent::ServerMetricsError(_) => {}
                        }
                    }

                    let home = match next_sftp_event(&mut sftp_event_rx, |event| {
                        matches!(event, SftpEvent::Ready { .. })
                    })
                    .await?
                    {
                        SftpEvent::Ready { path } => path,
                        _ => unreachable!(),
                    };
                    assert!(!home.is_empty());

                    sftp_command_tx
                        .send(SftpCommand::List(home.clone()))
                        .await?;
                    next_sftp_event(
                        &mut sftp_event_rx,
                        |event| matches!(event, SftpEvent::Entries { path, .. } if path == &home),
                    )
                    .await?;
                    sftp_command_tx
                        .send(SftpCommand::FilesystemInfo(home.clone()))
                        .await?;
                    let filesystem = next_sftp_event(&mut sftp_event_rx, |event| {
                        matches!(event, SftpEvent::FilesystemInfo(info) if info.path == home)
                    })
                    .await?;
                    let SftpEvent::FilesystemInfo(filesystem) = filesystem else {
                        unreachable!()
                    };
                    assert!(filesystem.total_bytes > 0);
                    assert!(filesystem.available_bytes <= filesystem.total_bytes);
                    assert!(filesystem.block_size > 0);

                    sftp_command_tx
                        .send(SftpCommand::Upload {
                            local: upload_path.clone(),
                            remote: remote.clone(),
                            overwrite: false,
                            resume: false,
                        })
                        .await?;
                    next_sftp_event(&mut sftp_event_rx, |event| {
                        matches!(
                            event,
                            SftpEvent::TransferFinished {
                                kind: crate::sftp::TransferKind::Upload,
                                ..
                            }
                        )
                    })
                    .await?;

                    sftp_command_tx
                        .send(SftpCommand::Upload {
                            local: upload_path.clone(),
                            remote: remote.clone(),
                            overwrite: false,
                            resume: false,
                        })
                        .await?;
                    next_sftp_event(&mut sftp_event_rx, |event| {
                        matches!(
                            event,
                            SftpEvent::NeedsOverwrite {
                                kind: crate::sftp::TransferKind::Upload,
                                ..
                            }
                        )
                    })
                    .await?;

                    sftp_command_tx
                        .send(SftpCommand::Upload {
                            local: upload_path.clone(),
                            remote: remote.clone(),
                            overwrite: true,
                            resume: false,
                        })
                        .await?;
                    next_sftp_event(&mut sftp_event_rx, |event| {
                        matches!(
                            event,
                            SftpEvent::TransferFinished {
                                kind: crate::sftp::TransferKind::Upload,
                                ..
                            }
                        )
                    })
                    .await?;

                    sftp_command_tx
                        .send(SftpCommand::List(home.clone()))
                        .await?;
                    let entries = next_sftp_event(
                        &mut sftp_event_rx,
                        |event| matches!(event, SftpEvent::Entries { path, .. } if path == &home),
                    )
                    .await?;
                    let SftpEvent::Entries { entries, .. } = entries else {
                        unreachable!()
                    };
                    assert!(
                        entries.iter().any(|entry| entry.name == remote_name),
                        "uploaded file is missing from the remote listing"
                    );

                    sftp_command_tx
                        .send(SftpCommand::SetMetadata {
                            path: remote.clone(),
                            permissions: Some(0o640),
                            atime: Some(1_700_000_000),
                            mtime: Some(1_700_000_123),
                        })
                        .await?;
                    next_sftp_action(&mut sftp_event_rx, "Updated metadata").await?;
                    sftp_command_tx
                        .send(SftpCommand::List(home.clone()))
                        .await?;
                    let entries = next_sftp_event(
                        &mut sftp_event_rx,
                        |event| matches!(event, SftpEvent::Entries { path, .. } if path == &home),
                    )
                    .await?;
                    let SftpEvent::Entries { entries, .. } = entries else {
                        unreachable!()
                    };
                    let metadata_entry = entries
                        .iter()
                        .find(|entry| entry.name == remote_name)
                        .context("metadata test file is missing from the listing")?;
                    assert_eq!(
                        metadata_entry.permissions.map(|permissions| permissions & 0o7777),
                        Some(0o640)
                    );
                    assert_eq!(metadata_entry.atime, Some(1_700_000_000));
                    assert_eq!(metadata_entry.mtime, Some(1_700_000_123));
                    sftp_command_tx
                        .send(SftpCommand::SetMetadata {
                            path: "dshell-sftp-missing-file.txt".to_owned(),
                            permissions: Some(0o600),
                            atime: None,
                            mtime: None,
                        })
                        .await?;
                    next_sftp_event(&mut sftp_event_rx, |event| {
                        matches!(event, SftpEvent::Error(error) if error.contains("Could not update metadata"))
                    })
                    .await?;

                    sftp_command_tx
                        .send(SftpCommand::Download {
                            remote: remote.clone(),
                            local: download_path.clone(),
                            overwrite: false,
                            resume: false,
                        })
                        .await?;
                    next_sftp_event(&mut sftp_event_rx, |event| {
                        matches!(
                            event,
                            SftpEvent::TransferFinished {
                                kind: crate::sftp::TransferKind::Download,
                                ..
                            }
                        )
                    })
                    .await?;
                    assert_eq!(
                        tokio::fs::read(&download_path)
                            .await
                            .context("downloaded file is unreadable")?,
                        content,
                        "downloaded file content differs from the uploaded file"
                    );

                    sftp_command_tx
                        .send(SftpCommand::Upload {
                            local: resume_partial_local.clone(),
                            remote: resume_remote.clone(),
                            overwrite: true,
                            resume: false,
                        })
                        .await?;
                    next_sftp_event(&mut sftp_event_rx, |event| {
                        matches!(
                            event,
                            SftpEvent::TransferFinished {
                                kind: crate::sftp::TransferKind::Upload,
                                ..
                            }
                        )
                    })
                    .await?;
                    sftp_command_tx
                        .send(SftpCommand::Upload {
                            local: resume_full_local.clone(),
                            remote: resume_remote.clone(),
                            overwrite: false,
                            resume: true,
                        })
                        .await?;
                    let resumed = next_sftp_event(&mut sftp_event_rx, |event| {
                        matches!(
                            event,
                            SftpEvent::TransferFinished {
                                kind: crate::sftp::TransferKind::Upload,
                                ..
                            }
                        )
                    })
                    .await?;
                    let SftpEvent::TransferFinished { .. } = resumed else {
                        unreachable!()
                    };
                    sftp_command_tx
                        .send(SftpCommand::Download {
                            remote: resume_remote.clone(),
                            local: resume_download.clone(),
                            overwrite: true,
                            resume: false,
                        })
                        .await?;
                    next_sftp_event(&mut sftp_event_rx, |event| {
                        matches!(
                            event,
                            SftpEvent::TransferFinished {
                                kind: crate::sftp::TransferKind::Download,
                                ..
                            }
                        )
                    })
                    .await?;
                    assert_eq!(
                        tokio::fs::read(&resume_download).await?,
                        resume_content,
                        "resumed upload content differs from the source file"
                    );
                    std::fs::write(&resume_download, &resume_content[..30_000]).unwrap();
                    sftp_command_tx
                        .send(SftpCommand::Download {
                            remote: resume_remote.clone(),
                            local: resume_download.clone(),
                            overwrite: false,
                            resume: true,
                        })
                        .await?;
                    next_sftp_event(&mut sftp_event_rx, |event| {
                        matches!(
                            event,
                            SftpEvent::TransferFinished {
                                kind: crate::sftp::TransferKind::Download,
                                ..
                            }
                        )
                    })
                    .await?;
                    assert_eq!(
                        tokio::fs::read(&resume_download).await?,
                        resume_content,
                        "resumed download content differs from the remote file"
                    );
                    sftp_command_tx
                        .send(SftpCommand::RemoveFile(resume_remote.clone()))
                        .await?;
                    next_sftp_action(&mut sftp_event_rx, "Deleted file").await?;

                    let renamed = "dshell-sftp-renamed.txt".to_owned();
                    sftp_command_tx
                        .send(SftpCommand::CreateDirectory("dshell-sftp-dir".to_owned()))
                        .await?;
                    next_sftp_action(&mut sftp_event_rx, "Created directory").await?;
                    sftp_command_tx
                        .send(SftpCommand::Rename {
                            from: remote.clone(),
                            to: renamed.clone(),
                        })
                        .await?;
                    next_sftp_action(&mut sftp_event_rx, "Renamed").await?;
                    sftp_command_tx
                        .send(SftpCommand::List(home.clone()))
                        .await?;
                    let entries = next_sftp_event(
                        &mut sftp_event_rx,
                        |event| matches!(event, SftpEvent::Entries { path, .. } if path == &home),
                    )
                    .await?;
                    let SftpEvent::Entries { entries, .. } = entries else {
                        unreachable!()
                    };
                    assert!(entries.iter().any(|entry| entry.name == renamed));
                    assert!(!entries.iter().any(|entry| entry.name == remote_name));
                    sftp_command_tx
                        .send(SftpCommand::RemoveFile(renamed.clone()))
                        .await?;
                    next_sftp_action(&mut sftp_event_rx, "Deleted file").await?;
                    sftp_command_tx
                        .send(SftpCommand::RemoveDirectory("dshell-sftp-dir".to_owned()))
                        .await?;
                    next_sftp_action(&mut sftp_event_rx, "Removed directory").await?;
                    sftp_command_tx
                        .send(SftpCommand::List(home.clone()))
                        .await?;
                    let entries = next_sftp_event(
                        &mut sftp_event_rx,
                        |event| matches!(event, SftpEvent::Entries { path, .. } if path == &home),
                    )
                    .await?;
                    let SftpEvent::Entries { entries, .. } = entries else {
                        unreachable!()
                    };
                    assert!(!entries.iter().any(|entry| entry.name == renamed));
                    assert!(!entries.iter().any(|entry| entry.name == "dshell-sftp-dir"));

                    let tree_remote = "dshell-sftp-tree".to_owned();
                    sftp_command_tx
                        .send(SftpCommand::UploadDirectory {
                            local: tree_upload.clone(),
                            remote: tree_remote.clone(),
                            overwrite: false,
                            resume: false,
                        })
                        .await?;
                    next_sftp_event(&mut sftp_event_rx, |event| {
                        matches!(
                            event,
                            SftpEvent::TransferFinished {
                                kind: crate::sftp::TransferKind::Upload,
                                ..
                            }
                        )
                    })
                    .await?;
                    sftp_command_tx
                        .send(SftpCommand::List(format!("{tree_remote}/nested")))
                        .await?;
                    let entries = next_sftp_event(&mut sftp_event_rx, |event| {
                        matches!(event, SftpEvent::Entries { path, .. } if path == &format!("{tree_remote}/nested"))
                    })
                    .await?;
                    let SftpEvent::Entries { entries, .. } = entries else {
                        unreachable!()
                    };
                    assert!(entries.iter().any(|entry| entry.name == "file.txt"));

                    sftp_command_tx
                        .send(SftpCommand::DownloadDirectory {
                            remote: tree_remote.clone(),
                            local: tree_download.clone(),
                            overwrite: false,
                            resume: false,
                        })
                        .await?;
                    next_sftp_event(&mut sftp_event_rx, |event| {
                        matches!(
                            event,
                            SftpEvent::TransferFinished {
                                kind: crate::sftp::TransferKind::Download,
                                ..
                            }
                        )
                    })
                    .await?;
                    assert_eq!(
                        tokio::fs::read(tree_download.join("nested/file.txt"))
                            .await
                            .context("downloaded directory content is unreadable")?,
                        b"nested content"
                    );

                    sftp_command_tx
                        .send(SftpCommand::RemoveFile(format!(
                            "{tree_remote}/nested/file.txt"
                        )))
                        .await?;
                    next_sftp_action(&mut sftp_event_rx, "Deleted file").await?;
                    sftp_command_tx
                        .send(SftpCommand::RemoveDirectory(format!(
                            "{tree_remote}/nested"
                        )))
                        .await?;
                    next_sftp_action(&mut sftp_event_rx, "Removed directory").await?;
                    sftp_command_tx
                        .send(SftpCommand::RemoveDirectory(tree_remote.clone()))
                        .await?;
                    next_sftp_action(&mut sftp_event_rx, "Removed directory").await?;

                    sftp_command_tx
                        .send(SftpCommand::Download {
                            remote: "dshell-sftp-missing-file.txt".to_owned(),
                            local: failed_download_path.clone(),
                            overwrite: true,
                            resume: false,
                        })
                        .await?;
                    next_sftp_event(&mut sftp_event_rx, |event| {
                        matches!(
                            event,
                            SftpEvent::TransferFailed {
                                kind: crate::sftp::TransferKind::Download,
                                ..
                            }
                        )
                    })
                    .await?;

                    cancel_tx.send_replace(true);
                    loop {
                        match timeout(Duration::from_secs(20), event_rx.recv())
                            .await
                            .context("timed out waiting for SFTP session cleanup")?
                            .context("SSH event stream closed before disconnect")?
                        {
                            UiEvent::Disconnected => break,
                            UiEvent::Failed(error) => bail!("SSH session failed: {error}"),
                            UiEvent::Status(_) | UiEvent::LogState(_) | UiEvent::Screen(_) | UiEvent::AuthPrompt { .. } | UiEvent::SearchResult(_) | UiEvent::ConnectionInfo(_) | UiEvent::ServerMetrics(_) | UiEvent::ServerMetricsError(_) => {}
                        }
                    }
                    Ok::<(), anyhow::Error>(())
                };

                let result = timeout(Duration::from_secs(60), async {
                    tokio::select! {
                        result = exercise => {
                            result?;
                            connection.await
                        }
                        result = &mut connection => result,
                    }
                })
                .await
                .context("Docker SFTP test exceeded 60 seconds")?;
                result?;
                Ok::<(), anyhow::Error>(())
            })
            .unwrap();
    }

    #[test]
    #[ignore = "requires scripts/test-docker-ssh.ps1 to provide an Ubuntu SSH server"]
    fn docker_ssh_port_forwarding() {
        let host = env::var("DSHELL_DOCKER_SSH_HOST").expect("test host is not configured");
        let port = env::var("DSHELL_DOCKER_SSH_PORT")
            .expect("test port is not configured")
            .parse::<u16>()
            .expect("test port must be numeric");
        let username = env::var("DSHELL_DOCKER_SSH_USER").expect("test username is not configured");
        let password =
            env::var("DSHELL_DOCKER_SSH_PASSWORD").expect("test password is not configured");
        let known_hosts_path = PathBuf::from(
            env::var_os("DSHELL_DOCKER_SSH_KNOWN_HOSTS")
                .expect("isolated known_hosts path is not configured"),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime
            .block_on(async move {
                let (command_tx, command_rx) = mpsc::channel(32);
                let (event_tx, mut event_rx) = mpsc::channel(32);
                let (cancel_tx, cancel_rx) = watch::channel(false);
                let (sftp_commands, sftp_events, sftp_cancel) = sftp_test_channels();
                let (
                    forward_commands,
                    forward_command_tx,
                    forward_events,
                    mut forward_event_rx,
                ) = forward_test_channels();
                let spec = ConnectionSpec {
                    host,
                    port,
                    username,
                    authentication: Authentication::Password(Zeroizing::new(password)),
                    terminal_size: TerminalSize {
                        columns: 80,
                        rows: 24,
                    },
                    jump: None,
                    keepalive_seconds: 0,
                    theme: TerminalTheme::Default,
                    cursor_style: CursorStyle::Block,
                };
                let connection = run_connection_with_known_hosts(
                    spec,
                    command_rx,
                    event_tx,
                    sftp_commands,
                    sftp_events,
                    sftp_cancel,
                    forward_commands,
                    forward_events,
                    known_hosts_path,
                    cancel_rx,
                );
                tokio::pin!(connection);

                let exercise = async {
                    loop {
                        match timeout(Duration::from_secs(20), event_rx.recv())
                            .await
                            .context("timed out waiting for the SSH session")?
                            .context("SSH event stream closed during connection")?
                        {
                            UiEvent::Status(status) if status.starts_with("Connected to ") => break,
                            UiEvent::Failed(error) => bail!("SSH connection failed: {error}"),
                            UiEvent::Disconnected => bail!("SSH disconnected during connection"),
                            UiEvent::Status(_) | UiEvent::LogState(_) | UiEvent::Screen(_) | UiEvent::AuthPrompt { .. } | UiEvent::SearchResult(_) | UiEvent::ConnectionInfo(_) | UiEvent::ServerMetrics(_) | UiEvent::ServerMetricsError(_) => {}
                        }
                    }

                    forward_command_tx
                        .send(ForwardCommand::Add(ForwardRuleSpec {
                            kind: ForwardKind::Local,
                            listen_host: "127.0.0.1".to_owned(),
                            listen_port: 0,
                            target_host: "127.0.0.1".to_owned(),
                            target_port: 22,
                        }))
                        .await?;
                    let (local_id, local_port) =
                        next_forward_rule(&mut forward_event_rx, ForwardKind::Local).await?;
                    let mut local_stream = TcpStream::connect(("127.0.0.1", local_port)).await?;
                    let mut banner = [0u8; 64];
                    let read = timeout(Duration::from_secs(10), local_stream.read(&mut banner))
                        .await
                        .context("timed out reading the local forwarding banner")??;
                    assert!(
                        String::from_utf8_lossy(&banner[..read]).starts_with("SSH-"),
                        "local forwarding did not reach the SSH server"
                    );
                    forward_command_tx
                        .send(ForwardCommand::Remove(local_id))
                        .await?;
                    next_forward_removed(&mut forward_event_rx, local_id).await?;
                    assert!(
                        TcpStream::connect(("127.0.0.1", local_port)).await.is_err(),
                        "the local forwarding listener still accepts connections after removal"
                    );

                    forward_command_tx
                        .send(ForwardCommand::Add(ForwardRuleSpec {
                            kind: ForwardKind::Dynamic,
                            listen_host: "127.0.0.1".to_owned(),
                            listen_port: 0,
                            target_host: String::new(),
                            target_port: 0,
                        }))
                        .await?;
                    let (_, socks_port) =
                        next_forward_rule(&mut forward_event_rx, ForwardKind::Dynamic).await?;
                    let mut socks = TcpStream::connect(("127.0.0.1", socks_port)).await?;
                    socks.write_all(&[5, 1, 0]).await?;
                    let mut method_reply = [0u8; 2];
                    socks.read_exact(&mut method_reply).await?;
                    assert_eq!(method_reply, [5, 0]);
                    socks
                        .write_all(&[5, 1, 0, 1, 127, 0, 0, 1, 0, 22])
                        .await?;
                    let mut success_reply = [0u8; 10];
                    socks.read_exact(&mut success_reply).await?;
                    assert_eq!(success_reply[1], 0);
                    let read = timeout(Duration::from_secs(10), socks.read(&mut banner))
                        .await
                        .context("timed out reading the SOCKS5 forwarding banner")??;
                    assert!(
                        String::from_utf8_lossy(&banner[..read]).starts_with("SSH-"),
                        "dynamic forwarding did not reach the SSH server"
                    );

                    let local_listener = TcpListener::bind(("127.0.0.1", 0)).await?;
                    let local_target_port = local_listener.local_addr()?.port();
                    let (payload_tx, payload_rx) = tokio::sync::oneshot::channel();
                    tokio::spawn(async move {
                        if let Ok((mut stream, _)) = local_listener.accept().await {
                            let mut buffer = [0u8; 128];
                            let read = stream.read(&mut buffer).await.unwrap_or(0);
                            let _ = payload_tx.send(
                                String::from_utf8_lossy(&buffer[..read]).into_owned(),
                            );
                        }
                    });
                    forward_command_tx
                        .send(ForwardCommand::Add(ForwardRuleSpec {
                            kind: ForwardKind::Remote,
                            listen_host: "127.0.0.1".to_owned(),
                            listen_port: 0,
                            target_host: "127.0.0.1".to_owned(),
                            target_port: local_target_port,
                        }))
                        .await?;
                    let (remote_id, remote_port) =
                        next_forward_rule(&mut forward_event_rx, ForwardKind::Remote).await?;
                    command_tx
                        .send(TerminalRequest::Paste(format!(
                            "exec 3<>/dev/tcp/127.0.0.1/{remote_port}; printf 'DSHELL_REMOTE_TUNNEL\\n' >&3; exec 3>&-; exec 3<&-"
                        )))
                        .await?;
                    command_tx.send(TerminalRequest::Key(enter_key())).await?;
                    let payload = timeout(Duration::from_secs(20), payload_rx)
                        .await
                        .context("timed out waiting for the remote forwarding payload")?
                        .context("the remote forwarding local listener closed")?;
                    assert!(
                        payload.contains("DSHELL_REMOTE_TUNNEL"),
                        "remote forwarding delivered unexpected payload: {payload:?}"
                    );
                    next_forward_opened(&mut forward_event_rx, remote_id).await?;
                    forward_command_tx
                        .send(ForwardCommand::Remove(remote_id))
                        .await?;
                    next_forward_removed(&mut forward_event_rx, remote_id).await?;

                    cancel_tx.send_replace(true);
                    loop {
                        match timeout(Duration::from_secs(20), event_rx.recv())
                            .await
                            .context("timed out waiting for forwarding session cleanup")?
                            .context("SSH event stream closed before disconnect")?
                        {
                            UiEvent::Disconnected => break,
                            UiEvent::Failed(error) => bail!("SSH session failed: {error}"),
                            UiEvent::Status(_) | UiEvent::LogState(_) | UiEvent::Screen(_) | UiEvent::AuthPrompt { .. } | UiEvent::SearchResult(_) | UiEvent::ConnectionInfo(_) | UiEvent::ServerMetrics(_) | UiEvent::ServerMetricsError(_) => {}
                        }
                    }
                    Ok::<(), anyhow::Error>(())
                };

                let result = timeout(Duration::from_secs(90), async {
                    tokio::select! {
                        result = exercise => {
                            result?;
                            connection.await
                        }
                        result = &mut connection => result,
                    }
                })
                .await
                .context("Docker port forwarding test exceeded 90 seconds")?;
                result?;
                Ok::<(), anyhow::Error>(())
            })
            .unwrap();
    }

    async fn next_forward_rule(
        events: &mut mpsc::UnboundedReceiver<ForwardEvent>,
        kind: ForwardKind,
    ) -> Result<(u64, u16)> {
        loop {
            let event = forward_event_timeout(events).await?;
            if let ForwardEvent::RuleAdded {
                id,
                spec,
                bound_port,
            } = event
                && spec.kind == kind
            {
                return Ok((id, bound_port));
            }
        }
    }

    async fn next_forward_removed(
        events: &mut mpsc::UnboundedReceiver<ForwardEvent>,
        rule_id: u64,
    ) -> Result<()> {
        loop {
            let event = forward_event_timeout(events).await?;
            if matches!(event, ForwardEvent::RuleRemoved { id } if id == rule_id) {
                return Ok(());
            }
        }
    }

    async fn next_forward_opened(
        events: &mut mpsc::UnboundedReceiver<ForwardEvent>,
        rule_id: u64,
    ) -> Result<()> {
        loop {
            let event = forward_event_timeout(events).await?;
            if matches!(event, ForwardEvent::ConnectionOpened { id, .. } if id == rule_id) {
                return Ok(());
            }
        }
    }

    async fn forward_event_timeout(
        events: &mut mpsc::UnboundedReceiver<ForwardEvent>,
    ) -> Result<ForwardEvent> {
        timeout(Duration::from_secs(20), events.recv())
            .await
            .context("timed out waiting for a forwarding event")?
            .context("the forwarding event stream closed")
    }

    async fn next_sftp_action(
        events: &mut mpsc::Receiver<SftpEvent>,
        action_prefix: &str,
    ) -> Result<()> {
        loop {
            let event = timeout(Duration::from_secs(20), events.recv())
                .await
                .context("timed out waiting for an SFTP operation")?
                .context("the SFTP event stream closed during an operation")?;
            match event {
                SftpEvent::OperationFinished { action, .. }
                    if action.starts_with(action_prefix) =>
                {
                    return Ok(());
                }
                SftpEvent::Error(error) => bail!("SFTP operation failed: {error}"),
                _ => {}
            }
        }
    }

    async fn next_sftp_event(
        events: &mut mpsc::Receiver<SftpEvent>,
        matches: impl Fn(&SftpEvent) -> bool,
    ) -> Result<SftpEvent> {
        let mut last_event = None;
        loop {
            let event = match timeout(Duration::from_secs(20), events.recv()).await {
                Ok(Some(event)) => event,
                Ok(None) => bail!("SFTP event stream closed unexpectedly"),
                Err(_) => bail!("timed out waiting for an SFTP event; last event: {last_event:?}"),
            };
            if matches(&event) {
                return Ok(event);
            }
            last_event = Some(format!("{event:?}"));
        }
    }

    async fn next_screen(
        events: &mut mpsc::Receiver<UiEvent>,
        matches: impl Fn(&TerminalFrame) -> bool,
    ) -> Result<TerminalFrame> {
        let mut last_screen = None;
        loop {
            let event = match timeout(Duration::from_secs(20), events.recv()).await {
                Ok(Some(event)) => event,
                Ok(None) => bail!("SSH event stream closed while waiting for terminal output"),
                Err(_) => {
                    let screen = last_screen
                        .as_ref()
                        .map(frame_text)
                        .unwrap_or_else(|| "<no terminal screen received>".to_owned());
                    bail!("timed out waiting for terminal output; last screen:\n{screen}");
                }
            };
            match event {
                UiEvent::Screen(frame) if matches(&frame) => return Ok(frame),
                UiEvent::Screen(frame) => last_screen = Some(frame),
                UiEvent::Failed(error) => bail!("SSH session failed: {error}"),
                UiEvent::Disconnected => {
                    bail!("SSH disconnected while waiting for terminal output")
                }
                UiEvent::Status(_)
                | UiEvent::LogState(_)
                | UiEvent::AuthPrompt { .. }
                | UiEvent::SearchResult(_)
                | UiEvent::ConnectionInfo(_)
                | UiEvent::ServerMetrics(_)
                | UiEvent::ServerMetricsError(_) => {}
            }
        }
    }

    fn enter_key() -> TerminalKeyInput {
        TerminalKeyInput {
            key: "enter".to_owned(),
            text: None,
            control: false,
            alt: false,
            shift: false,
            platform: false,
            function: false,
        }
    }

    fn text_key(text: &str) -> TerminalKeyInput {
        TerminalKeyInput {
            key: text.to_owned(),
            text: Some(text.to_owned()),
            control: false,
            alt: false,
            shift: false,
            platform: false,
            function: false,
        }
    }

    fn frame_text(frame: &TerminalFrame) -> String {
        frame
            .rows
            .iter()
            .map(|row| row.iter().map(|run| run.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn frame_has_output_line(frame: &TerminalFrame, marker: &str) -> bool {
        frame.rows.iter().any(|row| {
            row.iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
                .trim()
                == marker
        })
    }

    fn find_text(frame: &TerminalFrame, needle: &str) -> Option<TerminalPosition> {
        frame.rows.iter().enumerate().find_map(|(row, runs)| {
            let line = runs.iter().map(|run| run.text.as_str()).collect::<String>();
            line.find(needle).map(|column| TerminalPosition {
                row: row as u16,
                column: column as u16,
            })
        })
    }
}
