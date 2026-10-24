use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    sync::{mpsc, watch},
    task::JoinHandle,
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForwardKind {
    #[default]
    Local,
    Remote,
    Dynamic,
}

impl ForwardKind {
    pub const ALL: [Self; 3] = [Self::Local, Self::Remote, Self::Dynamic];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::Remote => "Remote",
            Self::Dynamic => "Dynamic",
        }
    }

    pub const fn needs_target(self) -> bool {
        !matches!(self, Self::Dynamic)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ForwardRuleSpec {
    pub kind: ForwardKind,
    pub listen_host: String,
    pub listen_port: u16,
    pub target_host: String,
    pub target_port: u16,
}

#[derive(Clone, Debug)]
pub struct ForwardRuleUiState {
    pub spec: ForwardRuleSpec,
    pub bound_port: u16,
    pub connections: u32,
}

#[derive(Debug)]
pub enum ForwardCommand {
    Add(ForwardRuleSpec),
    Remove(u64),
}

#[derive(Clone, Debug)]
pub enum ForwardEvent {
    RuleAdded {
        id: u64,
        spec: ForwardRuleSpec,
        bound_port: u16,
    },
    RuleRemoved {
        id: u64,
    },
    Error {
        id: Option<u64>,
        error: String,
    },
    ConnectionOpened {
        id: u64,
        peer: String,
        target: String,
    },
    ConnectionClosed {
        id: u64,
        peer: String,
    },
}

#[derive(Clone, Debug)]
pub struct RemoteTarget {
    pub rule_id: u64,
    pub host: String,
    pub port: u16,
}

pub type RemoteTargetMap = Arc<Mutex<HashMap<u32, RemoteTarget>>>;

pub struct AcceptedConnection {
    pub rule_id: u64,
    pub stream: TcpStream,
    pub peer: SocketAddr,
    pub target_host: String,
    pub target_port: u16,
    pub socks5: bool,
}

pub async fn bind_local_rule(
    id: u64,
    spec: ForwardRuleSpec,
    accepted: mpsc::UnboundedSender<AcceptedConnection>,
    events: mpsc::UnboundedSender<ForwardEvent>,
) -> Result<(u16, watch::Sender<bool>, JoinHandle<()>)> {
    let listener = TcpListener::bind((spec.listen_host.as_str(), spec.listen_port))
        .await
        .with_context(|| {
            format!(
                "Could not listen on {}:{}",
                spec.listen_host, spec.listen_port
            )
        })?;
    let bound_port = listener
        .local_addr()
        .context("The local forwarding listener has no address")?
        .port();
    let (stop_tx, mut stop_rx) = watch::channel(false);
    let task = tokio::spawn(async move {
        loop {
            let accepted_socket = tokio::select! {
                _ = stop_rx.changed() => break,
                accepted_socket = listener.accept() => accepted_socket,
            };
            let (mut stream, peer) = match accepted_socket {
                Ok(accepted_socket) => accepted_socket,
                Err(error) => {
                    let _ = events.send(ForwardEvent::Error {
                        id: Some(id),
                        error: format!("Local forwarding listener failed: {error}"),
                    });
                    break;
                }
            };
            let (target_host, target_port, socks5) = if spec.kind == ForwardKind::Dynamic {
                match tokio::time::timeout(Duration::from_secs(10), socks5_handshake(&mut stream))
                    .await
                {
                    Ok(Ok(target)) => (target.0, target.1, true),
                    Ok(Err(error)) => {
                        let _ = socks5_reject(&mut stream, 1).await;
                        let _ = events.send(ForwardEvent::Error {
                            id: Some(id),
                            error: format!("SOCKS5 handshake from {peer} failed: {error}"),
                        });
                        continue;
                    }
                    Err(_) => {
                        let _ = socks5_reject(&mut stream, 1).await;
                        let _ = events.send(ForwardEvent::Error {
                            id: Some(id),
                            error: format!(
                                "SOCKS5 handshake from {peer} timed out after 10 seconds"
                            ),
                        });
                        continue;
                    }
                }
            } else {
                (spec.target_host.clone(), spec.target_port, false)
            };
            if accepted
                .send(AcceptedConnection {
                    rule_id: id,
                    stream,
                    peer,
                    target_host,
                    target_port,
                    socks5,
                })
                .is_err()
            {
                break;
            }
        }
    });
    Ok((bound_port, stop_tx, task))
}

pub async fn socks5_handshake<S>(stream: &mut S) -> Result<(String, u16)>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut greeting = [0u8; 2];
    stream
        .read_exact(&mut greeting)
        .await
        .context("Could not read the SOCKS5 greeting")?;
    if greeting[0] != 5 {
        bail!("unsupported SOCKS version {}", greeting[0]);
    }
    let method_count = usize::from(greeting[1]);
    if method_count == 0 {
        bail!("the SOCKS5 client offered no authentication methods");
    }
    let mut methods = vec![0u8; method_count];
    stream
        .read_exact(&mut methods)
        .await
        .context("Could not read the SOCKS5 methods")?;
    if !methods.contains(&0) {
        stream
            .write_all(&[5, 0xff])
            .await
            .context("Could not reject the SOCKS5 greeting")?;
        bail!("the SOCKS5 client did not offer no-authentication");
    }
    stream
        .write_all(&[5, 0])
        .await
        .context("Could not accept the SOCKS5 greeting")?;

    let mut request = [0u8; 4];
    stream
        .read_exact(&mut request)
        .await
        .context("Could not read the SOCKS5 request")?;
    if request[0] != 5 {
        bail!("unsupported SOCKS version {}", request[0]);
    }
    if request[1] != 1 {
        bail!("unsupported SOCKS command {}", request[1]);
    }
    let host = match request[3] {
        1 => {
            let mut octets = [0u8; 4];
            stream.read_exact(&mut octets).await?;
            std::net::Ipv4Addr::from(octets).to_string()
        }
        3 => {
            let mut length = [0u8; 1];
            stream.read_exact(&mut length).await?;
            if length[0] == 0 {
                bail!("the SOCKS5 request has an empty domain name");
            }
            let mut domain = vec![0u8; usize::from(length[0])];
            stream.read_exact(&mut domain).await?;
            String::from_utf8(domain).context("the SOCKS5 domain is not UTF-8")?
        }
        4 => {
            let mut octets = [0u8; 16];
            stream.read_exact(&mut octets).await?;
            std::net::Ipv6Addr::from(octets).to_string()
        }
        address_type => bail!("unsupported SOCKS5 address type {address_type}"),
    };
    let mut port = [0u8; 2];
    stream
        .read_exact(&mut port)
        .await
        .context("Could not read the SOCKS5 port")?;
    Ok((host, u16::from_be_bytes(port)))
}

pub async fn socks5_accept<S>(stream: &mut S) -> Result<()>
where
    S: AsyncWrite + Unpin,
{
    stream
        .write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0])
        .await
        .context("Could not confirm the SOCKS5 connection")
}

pub async fn socks5_reject<S>(stream: &mut S, code: u8) -> Result<()>
where
    S: AsyncWrite + Unpin,
{
    stream
        .write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0])
        .await
        .context("Could not reject the SOCKS5 connection")
}

#[cfg(test)]
mod tests {
    use super::{ForwardKind, socks5_handshake};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    #[tokio::test]
    async fn socks5_parses_ipv4_and_domain_requests() {
        let (mut client, mut server) = tokio::io::duplex(128);
        let (_, target) = tokio::join!(
            async {
                client.write_all(&[5, 1, 0]).await.unwrap();
                client
                    .write_all(&[5, 1, 0, 1, 127, 0, 0, 1, 0, 22])
                    .await
                    .unwrap();
                let mut reply = [0u8; 2];
                client.read_exact(&mut reply).await.unwrap();
                client
            },
            socks5_handshake(&mut server)
        );
        assert_eq!(target.unwrap(), ("127.0.0.1".to_owned(), 22));

        let (mut client, mut server) = tokio::io::duplex(128);
        let (_, target) = tokio::join!(
            async {
                client.write_all(&[5, 1, 0]).await.unwrap();
                client.write_all(&[5, 1, 0, 3, 11]).await.unwrap();
                client.write_all(b"example.com").await.unwrap();
                client.write_all(&[0x01, 0xbb]).await.unwrap();
                let mut reply = [0u8; 2];
                client.read_exact(&mut reply).await.unwrap();
                client
            },
            socks5_handshake(&mut server)
        );
        assert_eq!(target.unwrap(), ("example.com".to_owned(), 443));
    }

    #[test]
    fn forwarding_kinds_describe_their_target_needs() {
        assert!(ForwardKind::Local.needs_target());
        assert!(ForwardKind::Remote.needs_target());
        assert!(!ForwardKind::Dynamic.needs_target());
        assert_eq!(ForwardKind::default(), ForwardKind::Local);
    }
}
