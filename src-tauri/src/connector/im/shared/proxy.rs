//! 统一的 IM 渠道代理来源。
//!
//! 背景:Telegram(reqwest 默认 env 解析,`HTTPS_PROXY` 大写优先) 与 WhatsApp
//! (`proxy_transport` 自实现,小写优先) 此前各跑各的,导致同样的 env 在两个
//! 渠道上行为不一致(大小写优先级不同 + reqwest 不感知 `socks5://`)。这里把
//! 解析逻辑统一到一个出口,WhatsApp 的 `proxy_transport` 和 Telegram 的 reqwest
//! client 都从这里取代理 URL,行为对齐。
//!
//! 后续接入 app 内置代理设置 UI 时,只需把 [`resolve_proxy_url`] 改成
//! "先读 settings,fallback env",其它调用方零改动。

use anyhow::Result;
use http::Uri;
use std::sync::OnceLock;
use std::time::Duration;

// =============================================================================
// 启动期一次性捕获代理 env + 隔离
// =============================================================================

/// 启动时从 env 捕获的代理配置 snapshot。
///
/// 一旦初始化后,即使后续 env 被修改也不影响 [`resolve_proxy_url`] 的返回。
/// 这是为了避免**默认** `reqwest::Client::new()` / `Client::builder().build()`
/// 隐式读 env 里被污染的 `HTTPS_PROXY`(例如 antproxy 这类工具临时 export 大写
/// 后未清理)导致**国内 IM 渠道**(钉钉 / 飞书 / 企微)直连 OSS / api.dingtalk.com
/// 等域名时被错路由到无关代理 → 502 → 附件下载失败。
///
/// 隔离策略:[`capture_and_isolate_proxy_env`] 启动时调用,
/// 1) 读 env 6 个 key,选小写优先的值存进 snapshot
/// 2) `std::env::remove_var` 把 6 个 key 全清掉
///
/// 之后:
/// - 默认 reqwest client(没显式 `.proxy()` 也没 `.no_proxy()`)看不到 env → 直连
/// - Telegram / WhatsApp 显式走 [`build_reqwest_client_with_proxy`] → 从 snapshot 读
static PROXY_SNAPSHOT: OnceLock<ProxySnapshot> = OnceLock::new();

#[derive(Debug, Clone, Default)]
struct ProxySnapshot {
    /// 最终选出来的 proxy URL(小写 > 大写,`*_proxy > *_PROXY`)。`None` = 不用代理。
    chosen: Option<String>,
    /// `no_proxy` / `NO_PROXY` 原值。命中时直连。
    no_proxy: Option<String>,
}

/// 启动期一次性捕获代理 env 并从进程环境里清除。**必须**在所有 reqwest client
/// 创建之前(包括 IM connectors / LLM gateway / OSS uploader / update checker
/// 等)调用,才有意义。
///
/// 幂等:重复调用是 no-op(`OnceLock` 已 set)。
pub fn capture_and_isolate_proxy_env() {
    let chosen = ["https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"]
        .iter()
        .find_map(|k| {
            std::env::var(k)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        });
    let no_proxy = std::env::var("no_proxy")
        .ok()
        .or_else(|| std::env::var("NO_PROXY").ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    let snapshot = ProxySnapshot {
        chosen: chosen.clone(),
        no_proxy: no_proxy.clone(),
    };
    let was_first = PROXY_SNAPSHOT.set(snapshot).is_ok();

    // 即便已经初始化过(理论上不应该)也要确保 env 被清掉,免得后来才创建的
    // reqwest client 又读到被污染的值。
    // SAFETY: `set_var` / `remove_var` 在 Rust 1.x 已被标记为 unsafe(非线程安全
    // 的 libc setenv),但启动期所有 reqwest client 尚未起,这里是唯一线程,
    // 没有竞争。
    for k in [
        "https_proxy",
        "HTTPS_PROXY",
        "http_proxy",
        "HTTP_PROXY",
        "all_proxy",
        "ALL_PROXY",
    ] {
        unsafe {
            std::env::remove_var(k);
        }
    }

    if was_first {
        log::info!(
            "[im/proxy] env captured + isolated chosen={:?} no_proxy={:?}",
            chosen,
            no_proxy
        );
    }
}

/// **仅供单测**:重置 snapshot(`OnceLock` 不能真清,这里只 set 一个空值。
/// 由于 `OnceLock::set` 第二次返回 Err,实际上是 no-op,所以测试必须在第一次
/// 用之前自己捕获)。生产代码绝对不要调。
#[cfg(test)]
#[allow(dead_code)]
fn _set_snapshot_for_tests(chosen: Option<String>, no_proxy: Option<String>) {
    let _ = PROXY_SNAPSHOT.set(ProxySnapshot { chosen, no_proxy });
}

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

/// 从启动期捕获的 snapshot 决定走代理。优先级在 [`capture_and_isolate_proxy_env`]
/// 已经折算好:`https_proxy` > `HTTPS_PROXY` > `all_proxy` > `ALL_PROXY`(小写优先
/// 避开 antproxy 等只 export 大写的工具的污染)。`no_proxy` / `NO_PROXY` 命中
/// `target_host` → 返回 None(直连)。
///
/// **不再读 env** —— 启动后 env 已被清,如果 capture 没调用过,snapshot 是默认
/// 空值 → 全部直连(这是安全 fallback,不是 bug)。
pub fn resolve_proxy_url(target_host: &str) -> Option<String> {
    let snapshot = PROXY_SNAPSHOT.get_or_init(ProxySnapshot::default);
    if let Some(np) = &snapshot.no_proxy {
        if no_proxy_matches(np, target_host) {
            return None;
        }
    }
    snapshot.chosen.clone()
}

/// `no_proxy` 规则匹配:逗号分隔,前导 `.` 可选,suffix 匹配。
pub fn no_proxy_matches(no_proxy: &str, host: &str) -> bool {
    no_proxy
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .any(|rule| {
            let rule = rule.trim_start_matches('.');
            host == rule || host.ends_with(&format!(".{rule}"))
        })
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

/// 构造一个 reqwest::Client,按 [`resolve_proxy_url`] 的决定显式装代理或显式禁代理。
///
/// **关键**:reqwest 默认会自己读 `HTTP_PROXY` / `HTTPS_PROXY`(大写优先,跟我们
/// 的优先级冲突)。这里 resolver 返回 None 时**显式 `.no_proxy()`** 关掉这条
/// fallback,避免被污染的大写 env 偷偷接管。
///
/// `target_host` 用于 `no_proxy` 规则判定。
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
            log::info!("[im/proxy] reqwest target={target_host} direct");
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
    fn build_reqwest_client_no_proxy_does_not_panic() {
        // 无 env 时 resolver 返回 None,client 必须显式禁代理建得起来。
        let _ = build_reqwest_client_with_proxy(Duration::from_secs(5), "api.telegram.org");
    }
}
