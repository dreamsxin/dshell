use std::{
    env,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::Duration,
};

use anyhow::{Context as _, Result, anyhow, bail};
use russh::{
    ChannelMsg, Disconnect,
    client::{self, AuthResult, Handler},
    keys::{PublicKey, PublicKeyOrCertificate, check_known_hosts_path},
};
use tokio::{
    sync::mpsc,
    time::{MissedTickBehavior, interval},
};
use zeroize::Zeroizing;

use crate::terminal::{TerminalEmulator, TerminalFrame, TerminalRequest, TerminalSize};

pub struct ConnectionSpec {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Zeroizing<String>,
    pub terminal_size: TerminalSize,
}

pub enum UiEvent {
    Status(String),
    Screen(TerminalFrame),
    Failed(String),
    Disconnected,
}

struct KnownHostsHandler {
    host: String,
    port: u16,
    path: PathBuf,
}

impl Handler for KnownHostsHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key = server_public_key.public_key();
        match is_known_host_key(&self.host, self.port, &key, &self.path)? {
            true => Ok(true),
            false => Err(anyhow!(
                "Server key for {}:{} is not present in {}",
                self.host,
                self.port,
                self.path.display()
            )),
        }
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

pub fn spawn_connection(
    spec: ConnectionSpec,
    events: mpsc::Sender<UiEvent>,
) -> Result<mpsc::Sender<TerminalRequest>> {
    let (command_tx, command_rx) = mpsc::channel(128);
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

            if let Err(error) = runtime.block_on(run_connection(spec, command_rx, events.clone())) {
                let _ =
                    events.blocking_send(UiEvent::Failed(format!("Connection failed: {error}")));
            }
        })
        .context("could not start the SSH runtime thread")?;

    Ok(command_tx)
}

async fn run_connection(
    spec: ConnectionSpec,
    commands: mpsc::Receiver<TerminalRequest>,
    events: mpsc::Sender<UiEvent>,
) -> Result<()> {
    let known_hosts_path = default_known_hosts_path()?;
    run_connection_with_known_hosts(spec, commands, events, known_hosts_path).await
}

async fn run_connection_with_known_hosts(
    spec: ConnectionSpec,
    mut commands: mpsc::Receiver<TerminalRequest>,
    events: mpsc::Sender<UiEvent>,
    known_hosts_path: PathBuf,
) -> Result<()> {
    let ConnectionSpec {
        host,
        port,
        username,
        mut password,
        terminal_size,
    } = spec;
    let handler = KnownHostsHandler {
        host: host.clone(),
        port,
        path: known_hosts_path,
    };
    let config = Arc::new(client::Config::default());
    let mut session = client::connect(config, (host.as_str(), port), handler)
        .await
        .context("SSH handshake failed")?;

    let auth = session
        .authenticate_password(username.clone(), std::mem::take(&mut *password))
        .await
        .context("SSH password authentication failed")?;
    drop(password);
    if !matches!(auth, AuthResult::Success) {
        bail!("SSH password authentication was rejected");
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
    events
        .send(UiEvent::Status(format!(
            "Connected to {}:{} as {}",
            host, port, username
        )))
        .await
        .context("UI closed while connecting")?;

    let (mut reader, writer) = channel.split();
    let mut terminal = TerminalEmulator::new(terminal_size);
    let mut screen_dirty = false;
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
                    Some(TerminalRequest::Scroll(rows)) => {
                        terminal.scroll_display(rows);
                        events.send(UiEvent::Screen(terminal.frame())).await
                            .context("UI closed during terminal scroll")?;
                        screen_dirty = false;
                    }
                    Some(TerminalRequest::StartSelection(position)) => {
                        terminal.start_selection(position);
                        screen_dirty = true;
                    }
                    Some(TerminalRequest::UpdateSelection(position)) => {
                        terminal.update_selection(position);
                        screen_dirty = true;
                    }
                    Some(TerminalRequest::Paste(text)) => {
                        writer.data_bytes(terminal.paste_bytes(&text)).await?;
                    }
                    None => break,
                }
            }
            _ = render_tick.tick(), if screen_dirty => {
                events.send(UiEvent::Screen(terminal.frame())).await
                    .context("UI closed during terminal output")?;
                screen_dirty = false;
            }
            message = reader.wait() => {
                match message {
                    Some(ChannelMsg::Data { data }) => {
                        terminal.feed(&data);
                        screen_dirty = true;
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        terminal.feed(&data);
                        screen_dirty = true;
                    }
                    Some(ChannelMsg::ExitStatus { .. }) | None => break,
                    Some(_) => {}
                }
            }
        }
    }

    let _ = session
        .disconnect(Disconnect::ByApplication, "client closed", "en")
        .await;
    let _ = events.send(UiEvent::Disconnected).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ConnectionSpec, UiEvent, is_known_host_key, run_connection_with_known_hosts};
    use crate::terminal::{
        TerminalFrame, TerminalKeyInput, TerminalPosition, TerminalRequest, TerminalSize,
    };
    use anyhow::{Context, Result, bail};
    use russh::keys::PublicKey;
    use std::{env, path::PathBuf, time::Duration};
    use tokio::{sync::mpsc, time::timeout};
    use zeroize::Zeroizing;

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
                let spec = ConnectionSpec {
                    host,
                    port,
                    username,
                    password: Zeroizing::new(password),
                    terminal_size: TerminalSize {
                        columns: 80,
                        rows: 24,
                    },
                };
                let connection =
                    run_connection_with_known_hosts(spec, command_rx, event_tx, known_hosts_path);
                tokio::pin!(connection);

                let exercise = async {
                    let mut events = event_rx;
                    loop {
                        match timeout(Duration::from_secs(20), events.recv())
                            .await
                            .context("timed out waiting for SSH connection")?
                            .context("SSH event stream closed during connection")?
                        {
                            UiEvent::Status(status) if status.starts_with("Connected to ") => break,
                            UiEvent::Failed(error) => bail!("SSH connection failed: {error}"),
                            UiEvent::Disconnected => bail!("SSH disconnected during connection"),
                            UiEvent::Status(_) | UiEvent::Screen(_) => {}
                        }
                    }

                    command_tx
                        .send(TerminalRequest::Resize(TerminalSize {
                            columns: 100,
                            rows: 30,
                        }))
                        .await?;
                    next_screen(&mut events, |_| true).await?;

                    command_tx
                        .send(TerminalRequest::Paste("stty -echo".to_owned()))
                        .await?;
                    command_tx.send(TerminalRequest::Key(enter_key())).await?;
                    tokio::time::sleep(Duration::from_millis(200)).await;

                    command_tx
                        .send(TerminalRequest::Paste(
                            r"printf 'DSHELL_SSH_READY\n'".to_owned(),
                        ))
                        .await?;
                    command_tx.send(TerminalRequest::Key(enter_key())).await?;
                    let ready = next_screen(&mut events, |frame| {
                        frame_text(frame).contains("DSHELL_SSH_READY")
                    })
                    .await?;
                    let position = find_text(&ready, "DSHELL_SSH_READY")
                        .context("SSH output marker was not found in the terminal grid")?;
                    command_tx
                        .send(TerminalRequest::StartSelection(position))
                        .await?;
                    command_tx
                        .send(TerminalRequest::UpdateSelection(TerminalPosition {
                            row: position.row,
                            column: position.column + "DSHELL_SSH_READY".len() as u16 - 1,
                        }))
                        .await?;
                    next_screen(&mut events, |frame| {
                        frame
                            .selection_text
                            .as_deref()
                            .is_some_and(|text| text.contains("DSHELL_SSH_READY"))
                    })
                    .await?;

                    command_tx
                        .send(TerminalRequest::Paste(
                            r"printf 'DSHELL_PASTE_OK\n'".to_owned(),
                        ))
                        .await?;
                    command_tx.send(TerminalRequest::Key(enter_key())).await?;
                    next_screen(&mut events, |frame| {
                        frame_text(frame).contains("DSHELL_PASTE_OK")
                    })
                    .await?;

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
                            UiEvent::Status(_) | UiEvent::Screen(_) => {}
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

    async fn next_screen(
        events: &mut mpsc::Receiver<UiEvent>,
        matches: impl Fn(&TerminalFrame) -> bool,
    ) -> Result<TerminalFrame> {
        loop {
            match timeout(Duration::from_secs(20), events.recv())
                .await
                .context("timed out waiting for terminal output")?
                .context("SSH event stream closed while waiting for terminal output")?
            {
                UiEvent::Screen(frame) if matches(&frame) => return Ok(frame),
                UiEvent::Failed(error) => bail!("SSH session failed: {error}"),
                UiEvent::Disconnected => {
                    bail!("SSH disconnected while waiting for terminal output")
                }
                UiEvent::Status(_) | UiEvent::Screen(_) => {}
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

    fn frame_text(frame: &TerminalFrame) -> String {
        frame
            .rows
            .iter()
            .map(|row| row.iter().map(|run| run.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
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
