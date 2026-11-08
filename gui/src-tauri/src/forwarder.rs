//! Local proxy forwarder.
//!
//! Firefox cannot fill in proxy credentials itself (the Juggler-only
//! `CAMOU_PROXY_AUTH` channel is unavailable on a bare launch), so for
//! authenticated proxies the browser talks to a local SOCKS5 listener and this
//! forwarder relays to the upstream HTTP (CONNECT) or SOCKS5 proxy, injecting
//! the credentials. Remote DNS is preserved: the browser sends hostnames, and
//! the upstream CONNECT / SOCKS5 request carries the name.

use std::fmt;
use std::sync::Arc;

use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use crate::types::ProxyConfig;

const SOCKS5_VERSION: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpstreamKind {
    Http,
    Socks5,
}

/// A handle to a running forwarder; `stop` ends the listener loop.
pub struct ForwarderHandle {
    pub port: u16,
    stop_tx: watch::Sender<bool>,
}

impl fmt::Debug for ForwarderHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ForwarderHandle").field("port", &self.port).finish()
    }
}

impl ForwarderHandle {
    pub fn stop(self) {
        let _ = self.stop_tx.send(true);
    }
}

#[derive(Debug)]
enum ForwardError {
    ClientClosed,
    Malformed(&'static str),
    UnsupportedCommand(u8),
    Upstream(String),
}

impl fmt::Display for ForwardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ForwardError::ClientClosed => write!(f, "client closed early"),
            ForwardError::Malformed(m) => write!(f, "malformed SOCKS5: {m}"),
            ForwardError::UnsupportedCommand(c) => write!(f, "unsupported SOCKS5 command {c}"),
            ForwardError::Upstream(m) => write!(f, "upstream: {m}"),
        }
    }
}

impl From<std::io::Error> for ForwardError {
    fn from(e: std::io::Error) -> Self {
        ForwardError::Upstream(e.to_string())
    }
}

/// Start a forwarder that listens on 127.0.0.1:<ephemeral> and relays to the
/// upstream proxy described by `config`.
pub async fn start(config: ProxyConfig) -> Result<ForwarderHandle, String> {
    let kind = match config.kind.as_str() {
        "http" => UpstreamKind::Http,
        "socks5" => UpstreamKind::Socks5,
        other => return Err(format!("unsupported proxy kind: {other}")),
    };
    if config.host.is_empty() || config.port == 0 {
        return Err("upstream proxy host/port missing".into());
    }
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("cannot bind local forwarder: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let (stop_tx, mut stop_rx) = watch::channel(false);
    let upstream = Arc::new(config);

    // tokio::spawn (not tauri::async_runtime::spawn): on the tauri runtime
    // both are the same executor, and this keeps the forwarder testable under
    // a plain #[tokio::test].
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = stop_rx.changed() => break,
                accepted = listener.accept() => {
                    let Ok((client, _addr)) = accepted else { continue };
                    let upstream = upstream.clone();
                    tokio::spawn(async move {
                        let mut client = client;
                        let _ = serve_conn(&mut client, &upstream, kind).await;
                    });
                }
            }
        }
    });

    Ok(ForwarderHandle { port, stop_tx })
}

async fn serve_conn(
    client: &mut TcpStream,
    config: &ProxyConfig,
    kind: UpstreamKind,
) -> Result<(), ForwardError> {
    let (host, port) = socks5_handshake(client).await?;
    let mut upstream = match kind {
        UpstreamKind::Http => connect_http(config, &host, port).await?,
        UpstreamKind::Socks5 => connect_socks5(config, &host, port).await?,
    };
    let _ = tokio::io::copy_bidirectional(client, &mut upstream).await;
    Ok(())
}

/// Browser side: answer a SOCKS5 NO-AUTH greeting and a CONNECT request,
/// returning the requested host and port. The whole request (address and
/// port) is consumed before any verdict is written — replying and dropping
/// with bytes still unread makes Windows reset the connection and destroy
/// the in-flight reply.
async fn socks5_handshake(client: &mut TcpStream) -> Result<(String, u16), ForwardError> {
    let mut head = [0u8; 2];
    read_exact(client, &mut head).await?;
    if head[0] != SOCKS5_VERSION {
        return Err(ForwardError::Malformed("bad version"));
    }
    let nmethods = head[1] as usize;
    if nmethods == 0 || nmethods > 32 {
        return Err(ForwardError::Malformed("bad method count"));
    }
    let mut methods = vec![0u8; nmethods];
    read_exact(client, &mut methods).await?;
    client
        .write_all(&[SOCKS5_VERSION, 0x00])
        .await
        .map_err(|_| ForwardError::ClientClosed)?;

    // Request: VER CMD RSV ATYP, then ADDR per ATYP, then PORT.
    let mut head = [0u8; 4];
    read_exact(client, &mut head).await?;
    if head[0] != SOCKS5_VERSION {
        return Err(ForwardError::Malformed("bad request version"));
    }
    if head[2] != 0x00 {
        return Err(ForwardError::Malformed("nonzero reserved"));
    }
    let host = match head[3] {
        0x01 => {
            let mut octets = [0u8; 4];
            read_exact(client, &mut octets).await?;
            std::net::Ipv4Addr::from(octets).to_string()
        }
        0x03 => {
            let mut len = [0u8; 1];
            read_exact(client, &mut len).await?;
            let mut name = vec![0u8; len[0] as usize];
            read_exact(client, &mut name).await?;
            String::from_utf8_lossy(&name).into_owned()
        }
        0x04 => {
            let mut octets = [0u8; 16];
            read_exact(client, &mut octets).await?;
            std::net::Ipv6Addr::from(octets).to_string()
        }
        _ => return Err(ForwardError::Malformed("bad address type")),
    };
    let mut port_buf = [0u8; 2];
    read_exact(client, &mut port_buf).await?;
    let port = u16::from_be_bytes(port_buf);

    // CONNECT only; BIND/UDP ASSOCIATE make no sense for a browser relay.
    // The request is fully consumed at this point, so replying 0x07 and
    // closing is a clean FIN, not a reset.
    if head[1] != 0x01 {
        let _ = client
            .write_all(&[SOCKS5_VERSION, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await;
        return Err(ForwardError::UnsupportedCommand(head[1]));
    }

    // Success; the bound address is meaningless for a relay.
    client
        .write_all(&[SOCKS5_VERSION, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await
        .map_err(|_| ForwardError::ClientClosed)?;

    Ok((host, port))
}

/// Upstream side: HTTP CONNECT, injecting Proxy-Authorization when set.
async fn connect_http(
    config: &ProxyConfig,
    host: &str,
    port: u16,
) -> Result<TcpStream, ForwardError> {
    let mut stream = TcpStream::connect((config.host.as_str(), config.port))
        .await
        .map_err(|e| ForwardError::Upstream(format!("{}:{}: {e}", config.host, config.port)))?;
    let mut request = format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n");
    if !config.username.is_empty() || !config.password.is_empty() {
        let credentials = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", config.username, config.password));
        request.push_str(&format!("Proxy-Authorization: Basic {credentials}\r\n"));
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| ForwardError::Upstream(e.to_string()))?;

    let mut response = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = stream
            .read(&mut byte)
            .await
            .map_err(|e| ForwardError::Upstream(e.to_string()))?;
        if n == 0 {
            return Err(ForwardError::Upstream(
                "connection closed during CONNECT".into(),
            ));
        }
        response.push(byte[0]);
        if response.ends_with(b"\r\n\r\n") || response.len() > 64 * 1024 {
            break;
        }
    }
    let head = String::from_utf8_lossy(&response);
    let status = head.split_whitespace().nth(1).unwrap_or("0");
    if !status.starts_with('2') {
        return Err(ForwardError::Upstream(format!(
            "CONNECT failed: {}",
            head.lines().next().unwrap_or("").trim()
        )));
    }
    Ok(stream)
}

/// Upstream side: SOCKS5 with optional RFC 1929 credentials, forwarding the
/// target as a DOMAIN address so DNS stays on the proxy exit.
async fn connect_socks5(
    config: &ProxyConfig,
    host: &str,
    port: u16,
) -> Result<TcpStream, ForwardError> {
    let mut stream = TcpStream::connect((config.host.as_str(), config.port))
        .await
        .map_err(|e| ForwardError::Upstream(format!("{}:{}: {e}", config.host, config.port)))?;

    stream
        .write_all(&[SOCKS5_VERSION, 2, 0x00, 0x02])
        .await
        .map_err(|e| ForwardError::Upstream(e.to_string()))?;
    let mut chosen = [0u8; 2];
    read_exact(&mut stream, &mut chosen).await?;
    if chosen[0] != SOCKS5_VERSION {
        return Err(ForwardError::Upstream("bad upstream SOCKS5 version".into()));
    }
    match chosen[1] {
        0x00 => {}
        0x02 => {
            let code = rfc1929_auth(&mut stream, &config.username, &config.password).await?;
            if code != 0 {
                return Err(ForwardError::Upstream(
                    "upstream rejected credentials".into(),
                ));
            }
        }
        method => {
            return Err(ForwardError::Upstream(format!(
                "upstream chose unsupported method 0x{method:02x}"
            )))
        }
    }

    let mut request = vec![SOCKS5_VERSION, 0x01, 0x00, 0x03, host.len() as u8];
    request.extend_from_slice(host.as_bytes());
    request.extend_from_slice(&port.to_be_bytes());
    stream
        .write_all(&request)
        .await
        .map_err(|e| ForwardError::Upstream(e.to_string()))?;

    let mut head = [0u8; 4];
    read_exact(&mut stream, &mut head).await?;
    if head[1] != 0x00 {
        return Err(ForwardError::Upstream(format!(
            "CONNECT failed with code 0x{:02x}",
            head[1]
        )));
    }
    let skip = match head[3] {
        0x01 => 4 + 2,
        0x03 => {
            let mut len = [0u8; 1];
            read_exact(&mut stream, &mut len).await?;
            len[0] as usize + 2
        }
        0x04 => 16 + 2,
        _ => {
            return Err(ForwardError::Upstream(
                "bad upstream reply address type".into(),
            ))
        }
    };
    let mut rest = vec![0u8; skip];
    read_exact(&mut stream, &mut rest).await?;
    Ok(stream)
}

async fn rfc1929_auth(
    stream: &mut TcpStream,
    username: &str,
    password: &str,
) -> Result<u8, ForwardError> {
    let mut packet = vec![0x01, username.len() as u8];
    packet.extend_from_slice(username.as_bytes());
    packet.push(password.len() as u8);
    packet.extend_from_slice(password.as_bytes());
    stream
        .write_all(&packet)
        .await
        .map_err(|e| ForwardError::Upstream(e.to_string()))?;
    let mut reply = [0u8; 2];
    read_exact(stream, &mut reply).await?;
    Ok(reply[1])
}

async fn read_exact(stream: &mut TcpStream, buf: &mut [u8]) -> Result<(), ForwardError> {
    stream
        .read_exact(buf)
        .await
        .map(|_| ())
        .map_err(|_| ForwardError::ClientClosed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(kind: &str, host: &str, port: u16, user: &str, pass: &str) -> ProxyConfig {
        ProxyConfig {
            kind: kind.into(),
            host: host.into(),
            port,
            username: user.into(),
            password: pass.into(),
        }
    }

    /// A mock SOCKS5 upstream that requires username/password auth, asserts it
    /// received "u"/"p", and echoes the connection.
    async fn spawn_socks5_upstream() -> (u16, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut head = [0u8; 2];
                    sock.read_exact(&mut head).await.unwrap();
                    // Consume the method list the greeting advertises before
                    // replying, or the auth read below swallows those bytes.
                    let mut methods = vec![0u8; head[1] as usize];
                    sock.read_exact(&mut methods).await.unwrap();
                    sock.write_all(&[5, 2]).await.unwrap();
                    // RFC 1929: VER ULEN UNAME PLEN PASSWD.
                    let mut ver = [0u8; 1];
                    sock.read_exact(&mut ver).await.unwrap();
                    let mut ulen = [0u8; 1];
                    sock.read_exact(&mut ulen).await.unwrap();
                    let mut user = vec![0u8; ulen[0] as usize];
                    sock.read_exact(&mut user).await.unwrap();
                    let mut plen = [0u8; 1];
                    sock.read_exact(&mut plen).await.unwrap();
                    let mut pass = vec![0u8; plen[0] as usize];
                    sock.read_exact(&mut pass).await.unwrap();
                    assert_eq!(user, b"u");
                    assert_eq!(pass, b"p");
                    sock.write_all(&[1, 0]).await.unwrap();
                    let mut req_head = [0u8; 4];
                    sock.read_exact(&mut req_head).await.unwrap();
                    assert_eq!(req_head[3], 0x03, "hostnames must reach the upstream");
                    let mut dlen = [0u8; 1];
                    sock.read_exact(&mut dlen).await.unwrap();
                    let mut rest = vec![0u8; dlen[0] as usize + 2];
                    sock.read_exact(&mut rest).await.unwrap();
                    sock.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await.unwrap();
                    // Echo: split so the read and write halves can be used at once.
                    let (mut rd, mut wr) = sock.into_split();
                    tokio::spawn(async move {
                        let mut buf = [0u8; 1024];
                        loop {
                            let n = match rd.read(&mut buf).await {
                                Ok(0) | Err(_) => break,
                                Ok(n) => n,
                            };
                            if wr.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    });
                });
            }
        });
        (port, handle)
    }

    async fn socks5_client_roundtrip(proxy_port: u16) -> Vec<u8> {
        let mut sock = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        sock.write_all(&[5, 1, 0]).await.unwrap(); // offer NO-AUTH only
        let mut reply = [0u8; 2];
        sock.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply, [5, 0]);
        let mut req = vec![5, 1, 0, 3, 9];
        req.extend_from_slice(b"localhost");
        req.extend_from_slice(&1234u16.to_be_bytes());
        sock.write_all(&req).await.unwrap();
        let mut ok = [0u8; 10];
        sock.read_exact(&mut ok).await.unwrap();
        assert_eq!(ok[1], 0);
        sock.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        sock.read_exact(&mut buf).await.unwrap();
        buf.to_vec()
    }

    // multi_thread matches the production runtime (tauri's tokio): the
    // current-thread flavor stalls these cross-task socket handshakes.
    #[tokio::test(flavor = "multi_thread")]
    async fn relays_through_authenticated_socks5_upstream() {
        let (upstream_port, upstream) = spawn_socks5_upstream().await;
        let handle = start(config("socks5", "127.0.0.1", upstream_port, "u", "p"))
            .await
            .unwrap();
        assert_eq!(socks5_client_roundtrip(handle.port).await, b"ping");
        handle.stop();
        upstream.abort();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rejects_unsupported_command() {
        let (upstream_port, upstream) = spawn_socks5_upstream().await;
        let handle = start(config("socks5", "127.0.0.1", upstream_port, "u", "p"))
            .await
            .unwrap();
        let mut sock = TcpStream::connect(("127.0.0.1", handle.port)).await.unwrap();
        sock.write_all(&[5, 1, 0]).await.unwrap(); // offer NO-AUTH only
        let mut reply = [0u8; 2];
        sock.read_exact(&mut reply).await.unwrap();
        // UDP ASSOCIATE (cmd 3) -> 0x07 command not supported.
        sock.write_all(&[5, 3, 0, 1, 0, 0, 0, 0, 0, 0]).await.unwrap();
        let mut resp = [0u8; 10];
        sock.read_exact(&mut resp).await.unwrap();
        assert_eq!(resp[1], 0x07);
        handle.stop();
        upstream.abort();
    }

    #[tokio::test]
    async fn rejects_bad_kind() {
        assert!(start(config("gopher", "127.0.0.1", 1, "", "")).await.is_err());
    }
}
