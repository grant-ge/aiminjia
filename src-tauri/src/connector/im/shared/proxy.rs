//! 统一的 IM 渠道代理来源。
//!
//! 策略很简单:**直接读系统级网络代理**。本机系统代理开了 → 用；没开 → 直连。
//! 不再读 env、不再解析 shell 里 export 的 `HTTPS_PROXY`、不需要 in-app 代理面板。
//!
//! 这样:
//! - 用户在 ClashX / Surge / Clash for Windows 等翻墙工具点"系统代理"开关 →
//!   sysproxy crate 通过 macOS `scutil --proxy` / Windows 注册表读到 → IM 连接用上
//! - 用户关掉系统代理 → 直连
//! - 双击 .app / Dock / Spotlight 启动也能读到(不依赖 shell env)
//!
//! `parse_proxy_url` / `ProxyEndpoint` / `ProxyScheme` 保留对外签名,WhatsApp
//! 的自实现 transport 还在用它把代理 URL 拆成 host+port+scheme。

use anyhow::Result;
use http::Uri;
use std::time::Duration;

/// 代理协议。`socks5h://` 跟 `socks5://` 等价(DNS 由代理解析,wa-rs 现行行为)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProxyScheme {
    Socks5,
    Http,
}

/// 解析后的代理端点。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyEndpoint {
    pub scheme: ProxyScheme,
    pub host: String,
    pub port: u16,
    /// 原始 URL,reqwest 需要直接拿这个传给 `reqwest::Proxy::all`。
    pub raw_url: String,
}

/// 读系统代理。开了 → 返回 `http://host:port`;没开 / 读不到 → `None`(直连)。
///
/// `_target_host` 保留参数位置以维持 API 兼容(老实现里用于 `no_proxy` 匹配,
/// 现在已废弃,sysproxy 自己有 bypass 列表但我们不暴露它)。
pub fn resolve_proxy_url(_target_host: &str) -> Option<String> {
    let proxy = sysproxy::Sysproxy::get_system_proxy().ok()?;
    if !proxy.enable {
        return None;
    }
    Some(format!("http://{}:{}", proxy.host, proxy.port))
}

/// Parse `socks5://host:port` / `http(s)://host:port` 形式的 proxy URL。
/// Unknown scheme → 当 http 处理(打 warn)。
pub fn parse_proxy_url(url: &str) -> Result<ProxyEndpoint> {
    let uri: Uri = url
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid proxy url '{url}': {e}"))?;
    let scheme_str = uri.scheme_str().unwrap_or("http").to_ascii_lowercase();
    let scheme = match scheme_str.as_str() {
        "socks5" | "socks5h" => ProxyScheme::Socks5,
        "http" | "https" => ProxyScheme::Http,
        other => {
            log::warn!("[im/proxy] unknown scheme '{other}', treating as http");
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
    Ok(ProxyEndpoint {
        scheme,
        host,
        port,
        raw_url: url.to_string(),
    })
}

/// 构造一个 reqwest::Client。系统代理开了就装上,没开就显式 `.no_proxy()`
/// 关掉 reqwest 自己的 env fallback。
pub fn build_reqwest_client_with_proxy(
    timeout: Duration,
    target_host: &str,
) -> Result<reqwest::Client> {
    let builder = reqwest::Client::builder().timeout(timeout);
    let builder = match resolve_proxy_url(target_host) {
        Some(proxy_url) => {
            log::info!("[im/proxy] reqwest target={target_host} proxy={proxy_url}");
            let proxy = reqwest::Proxy::all(&proxy_url).map_err(|e| {
                anyhow::anyhow!("reqwest::Proxy::all('{proxy_url}') failed: {e}")
            })?;
            builder.proxy(proxy)
        }
        None => {
            log::info!("[im/proxy] reqwest target={target_host} direct (no system proxy)");
            builder.no_proxy()
        }
    };
    builder
        .build()
        .map_err(|e| anyhow::anyhow!("reqwest client build failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_socks5_proxy_url() {
        let ep = parse_proxy_url("socks5://127.0.0.1:7890").unwrap();
        assert_eq!(ep.scheme, ProxyScheme::Socks5);
        assert_eq!(ep.host, "127.0.0.1");
        assert_eq!(ep.port, 7890);
        assert_eq!(ep.raw_url, "socks5://127.0.0.1:7890");
    }

    #[test]
    fn parse_http_proxy_url() {
        let ep = parse_proxy_url("http://127.0.0.1:7890").unwrap();
        assert_eq!(ep.scheme, ProxyScheme::Http);
        assert_eq!(ep.host, "127.0.0.1");
        assert_eq!(ep.port, 7890);
    }

    #[test]
    fn parse_proxy_default_socks5_port() {
        let ep = parse_proxy_url("socks5://127.0.0.1").unwrap();
        assert_eq!(ep.port, 1080);
    }

    #[test]
    fn build_reqwest_client_does_not_panic() {
        // 系统代理读不读得到都不能 panic,client 必须建得起来。
        let _ = build_reqwest_client_with_proxy(Duration::from_secs(5), "api.telegram.org");
    }
}
