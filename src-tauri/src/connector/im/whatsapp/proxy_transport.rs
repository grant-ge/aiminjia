//! Proxy-aware WebSocket transport + HTTP client for wa-rs.
//!
//! wa-rs 0.2 上游的 `wa-rs-tokio-transport` / `wa-rs-ureq-http` 不读
//! `HTTPS_PROXY` / `ALL_PROXY` 环境变量；用户在国内环境运行（WhatsApp 服务器
//! 直连被墙）就连不上。OpenClaw 用 Node `https-proxy-agent` 读 env 注入
//! Baileys WebSocket agent —— 这里照同一个思路：自己实现 wa-rs 的两个 trait
//! (`TransportFactory` + `HttpClient`)，读 env 走 SOCKS5/HTTP proxy。
//!
//! 用户系统配代理后 (`ALL_PROXY=socks5://127.0.0.1:7890` 等)，**零应用层
//! 配置**：transport 直接拨代理拿 TCP，上 TLS，上 WebSocket。
//!
//! HTTP 走 reqwest（已 socks feature），无需手动拼。

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use http::Uri;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_socks::tcp::Socks5Stream;
use tokio_websockets::{ClientBuilder, Message, WebSocketStream};
use wa_rs::http::{HttpClient, HttpRequest, HttpResponse};
use wa_rs::transport::{Transport, TransportEvent, TransportFactory};

/// WhatsApp Web WebSocket URL —— 同 wa-rs-core::net::WHATSAPP_WEB_WS_URL，
/// 但 wa-rs 0.2 没在顶层 re-export，直接硬编码避免引 wa-rs-core 作为额外 dep。
const WHATSAPP_WEB_WS_URL: &str = "wss://web.whatsapp.com/ws/chat";

// =============================================================================
// Proxy resolution
// =============================================================================

/// 读 env 决定走代理。优先级：`https_proxy` > `HTTPS_PROXY` > `all_proxy` > `ALL_PROXY`。
/// `no_proxy` / `NO_PROXY` 命中目标 host 时返回 None（直连）。
fn resolve_proxy_url(target_host: &str) -> Option<String> {
    if let Some(no_proxy) = std::env::var("no_proxy").ok().or_else(|| std::env::var("NO_PROXY").ok()) {
        if no_proxy_matches(&no_proxy, target_host) {
            return None;
        }
    }
    for key in ["https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"] {
        if let Ok(v) = std::env::var(key) {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn no_proxy_matches(no_proxy: &str, host: &str) -> bool {
    no_proxy
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .any(|rule| {
            let rule = rule.trim_start_matches('.');
            host == rule || host.ends_with(&format!(".{rule}"))
        })
}

/// Parse `socks5://host:port` 或 `http(s)://host:port` 形式的 proxy URL。
/// 返回 (scheme, host, port)。Unknown scheme → 当 http 处理。
fn parse_proxy_url(url: &str) -> anyhow::Result<(ProxyScheme, String, u16)> {
    let uri: Uri = url
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid proxy url '{url}': {e}"))?;
    let scheme = uri.scheme_str().unwrap_or("http").to_ascii_lowercase();
    let scheme = match scheme.as_str() {
        "socks5" | "socks5h" => ProxyScheme::Socks5,
        "http" | "https" => ProxyScheme::Http,
        other => {
            log::warn!("[whatsapp/proxy] unknown scheme '{other}', treating as http");
            ProxyScheme::Http
        }
    };
    let host = uri
        .host()
        .ok_or_else(|| anyhow::anyhow!("proxy url missing host: {url}"))?
        .to_string();
    let port = uri.port_u16().unwrap_or(match scheme {
        ProxyScheme::Socks5 => 1080,
        ProxyScheme::Http => 8080,
    });
    Ok((scheme, host, port))
}

#[derive(Clone, Copy, Debug)]
enum ProxyScheme {
    Socks5,
    Http,
}

/// 拨 TCP（走代理或直连）。
async fn dial_tcp(target_host: &str, target_port: u16) -> anyhow::Result<TcpStream> {
    match resolve_proxy_url(target_host) {
        Some(proxy_url) => {
            let (scheme, proxy_host, proxy_port) = parse_proxy_url(&proxy_url)?;
            let proxy_addr = format!("{proxy_host}:{proxy_port}");
            log::info!(
                "[whatsapp/proxy] dial {target_host}:{target_port} via {scheme:?} {proxy_addr}"
            );
            match scheme {
                ProxyScheme::Socks5 => {
                    let stream =
                        Socks5Stream::connect(proxy_addr.as_str(), (target_host, target_port))
                            .await
                            .map_err(|e| {
                                anyhow::anyhow!("SOCKS5 connect to {proxy_addr} failed: {e}")
                            })?;
                    Ok(stream.into_inner())
                }
                ProxyScheme::Http => {
                    http_connect_tunnel(&proxy_addr, target_host, target_port).await
                }
            }
        }
        None => {
            log::info!("[whatsapp/proxy] direct dial {target_host}:{target_port}");
            tokio::time::timeout(
                Duration::from_secs(15),
                TcpStream::connect((target_host, target_port)),
            )
            .await
            .map_err(|_| anyhow::anyhow!("connect to {target_host}:{target_port} timed out"))?
            .map_err(|e| anyhow::anyhow!("connect to {target_host}:{target_port} failed: {e}"))
        }
    }
}

/// HTTP CONNECT tunnel for `http://` proxies.
async fn http_connect_tunnel(
    proxy_addr: &str,
    target_host: &str,
    target_port: u16,
) -> anyhow::Result<TcpStream> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::time::timeout(
        Duration::from_secs(15),
        TcpStream::connect(proxy_addr),
    )
    .await
    .map_err(|_| anyhow::anyhow!("connect to proxy {proxy_addr} timed out"))?
    .map_err(|e| anyhow::anyhow!("connect to proxy {proxy_addr} failed: {e}"))?;
    let req = format!(
        "CONNECT {target_host}:{target_port} HTTP/1.1\r\nHost: {target_host}:{target_port}\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).await?;
    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).await?;
    let resp = String::from_utf8_lossy(&buf[..n]);
    let first_line = resp.lines().next().unwrap_or_default();
    if !first_line.contains(" 200 ") {
        return Err(anyhow::anyhow!(
            "HTTP CONNECT tunnel rejected: {first_line}"
        ));
    }
    Ok(stream)
}

// =============================================================================
// TLS connector — 复用 wa-rs 同款 webpki-roots
// =============================================================================

fn build_tls_connector() -> anyhow::Result<TlsConnector> {
    // Install rustls crypto provider once (defensive — wa-rs 内部也会 install）
    let _ = rustls::crypto::ring::default_provider().install_default();

    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

// =============================================================================
// WebSocket transport — wa_rs::Transport impl
// =============================================================================

type WsSink = futures::stream::SplitSink<
    WebSocketStream<TlsStream<TcpStream>>,
    Message,
>;
type WsStream =
    futures::stream::SplitStream<WebSocketStream<TlsStream<TcpStream>>>;

pub struct ProxyWebSocketTransport {
    ws_sink: Arc<tokio::sync::Mutex<Option<WsSink>>>,
}

#[async_trait]
impl Transport for ProxyWebSocketTransport {
    async fn send(&self, data: Vec<u8>) -> Result<(), anyhow::Error> {
        let mut guard = self.ws_sink.lock().await;
        let sink = guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("WebSocket already closed"))?;
        sink.send(Message::binary(data))
            .await
            .map_err(|e| anyhow::anyhow!("WebSocket send failed: {e}"))
    }

    async fn disconnect(&self) {
        let mut guard = self.ws_sink.lock().await;
        if let Some(mut sink) = guard.take() {
            let _ = sink.close().await;
        }
    }
}

async fn read_pump(
    mut stream: WsStream,
    event_tx: async_channel::Sender<TransportEvent>,
) {
    loop {
        match stream.next().await {
            Some(Ok(msg)) => {
                if msg.is_binary() {
                    let payload = msg.into_payload();
                    if event_tx
                        .send(TransportEvent::DataReceived(Bytes::from(payload)))
                        .await
                        .is_err()
                    {
                        break;
                    }
                } else if msg.is_close() {
                    break;
                }
            }
            Some(Err(e)) => {
                log::warn!("[whatsapp/proxy] ws read error: {e}");
                break;
            }
            None => break,
        }
    }
    let _ = event_tx.send(TransportEvent::Disconnected).await;
}

// =============================================================================
// TransportFactory impl
// =============================================================================

pub struct ProxyWebSocketTransportFactory {
    url: String,
}

impl ProxyWebSocketTransportFactory {
    pub fn new() -> Self {
        Self {
            url: WHATSAPP_WEB_WS_URL.to_string(),
        }
    }
}

impl Default for ProxyWebSocketTransportFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TransportFactory for ProxyWebSocketTransportFactory {
    async fn create_transport(
        &self,
    ) -> Result<
        (
            Arc<dyn Transport>,
            async_channel::Receiver<TransportEvent>,
        ),
        anyhow::Error,
    > {
        let uri: Uri = self
            .url
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid WS url '{}': {e}", self.url))?;
        let host = uri
            .host()
            .ok_or_else(|| anyhow::anyhow!("WS url missing host: {}", self.url))?
            .to_string();
        let port = uri.port_u16().unwrap_or(443);

        // 1. TCP via proxy (or direct)
        let tcp = dial_tcp(&host, port).await?;

        // 2. TLS handshake
        let tls_connector = build_tls_connector()?;
        let server_name = rustls::pki_types::ServerName::try_from(host.clone())
            .map_err(|e| anyhow::anyhow!("invalid server name '{host}': {e}"))?;
        let tls = tls_connector
            .connect(server_name, tcp)
            .await
            .map_err(|e| anyhow::anyhow!("TLS handshake to {host} failed: {e}"))?;

        // 3. WebSocket handshake on the TLS stream
        let (ws, _resp) = ClientBuilder::from_uri(uri)
            .connect_on(tls)
            .await
            .map_err(|e| anyhow::anyhow!("WebSocket handshake failed: {e}"))?;

        let (sink, stream) = ws.split();
        let (event_tx, event_rx) = async_channel::bounded(10000);

        let transport = Arc::new(ProxyWebSocketTransport {
            ws_sink: Arc::new(tokio::sync::Mutex::new(Some(sink))),
        });

        let event_tx_clone = event_tx.clone();
        tokio::spawn(read_pump(stream, event_tx_clone));
        let _ = event_tx.send(TransportEvent::Connected).await;

        log::info!("[whatsapp/proxy] WebSocket connected to {host}:{port}");
        Ok((transport, event_rx))
    }
}

// =============================================================================
// HttpClient — reqwest (with socks feature, reads ALL_PROXY env automatically)
// =============================================================================

pub struct ProxyHttpClient {
    client: reqwest::Client,
}

impl ProxyHttpClient {
    pub fn new() -> anyhow::Result<Self> {
        // reqwest 0.12 + socks feature: 默认从 env 拿 ALL_PROXY / HTTPS_PROXY。
        // 显式 builder 防 user 把 dev shell env 改了又 reload。
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| anyhow::anyhow!("reqwest client build failed: {e}"))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl HttpClient for ProxyHttpClient {
    async fn execute(&self, request: HttpRequest) -> anyhow::Result<HttpResponse> {
        let method = match request.method.to_ascii_uppercase().as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            other => return Err(anyhow::anyhow!("unsupported method: {other}")),
        };
        let mut builder = self.client.request(method, &request.url);
        for (k, v) in request.headers.iter() {
            builder = builder.header(k, v);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        let resp = builder.send().await.map_err(|e| {
            anyhow::anyhow!("HTTP request to {} failed: {e}", request.url)
        })?;
        let status_code = resp.status().as_u16();
        let body = resp
            .bytes()
            .await
            .map_err(|e| anyhow::anyhow!("read response body failed: {e}"))?
            .to_vec();
        Ok(HttpResponse {
            status_code,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_proxy_matches_exact_host() {
        assert!(no_proxy_matches("example.com", "example.com"));
    }

    #[test]
    fn no_proxy_matches_suffix() {
        assert!(no_proxy_matches(".example.com", "api.example.com"));
        assert!(no_proxy_matches("example.com", "api.example.com"));
    }

    #[test]
    fn no_proxy_does_not_match_unrelated() {
        assert!(!no_proxy_matches("example.com", "other.com"));
    }

    #[test]
    fn parse_socks5_proxy_url() {
        let (scheme, host, port) = parse_proxy_url("socks5://127.0.0.1:7890").unwrap();
        assert!(matches!(scheme, ProxyScheme::Socks5));
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 7890);
    }

    #[test]
    fn parse_http_proxy_url() {
        let (scheme, host, port) = parse_proxy_url("http://127.0.0.1:7890").unwrap();
        assert!(matches!(scheme, ProxyScheme::Http));
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 7890);
    }

    #[test]
    fn parse_proxy_default_socks5_port() {
        let (_, _, port) = parse_proxy_url("socks5://127.0.0.1").unwrap();
        assert_eq!(port, 1080);
    }
}
