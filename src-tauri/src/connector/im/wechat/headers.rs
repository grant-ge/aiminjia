//! Common request-header builder for all iLink API calls (spec §1.3).
//!
//! Every business endpoint AND the QR-login GET endpoints must carry the same
//! mandatory headers (`iLink-App-Id`, `iLink-App-ClientVersion`, `X-WECHAT-UIN`
//! etc.); using this helper centrally prevents accidental omissions that lead
//! to opaque 403s in production.

use base64::Engine;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};

pub const AUTHORIZATION_TYPE_VALUE: &str = "ilink_bot_token";

/// `iLink-App-ClientVersion` is sent as a decimal uint32 encoded
/// `major<<16 | minor<<8 | patch` (per openclaw `buildClientVersion`).
/// Non-numeric / missing components default to 0, with each clamped to u8.
pub fn encode_client_version(semver: &str) -> u32 {
    let mut parts = semver.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let major = parts.next().unwrap_or(0) & 0xff;
    let minor = parts.next().unwrap_or(0) & 0xff;
    let patch = parts.next().unwrap_or(0) & 0xff;
    (major << 16) | (minor << 8) | patch
}

/// `X-WECHAT-UIN` value: base64(decimal_string(random_u32)).
fn generate_wechat_uin() -> String {
    let n: u32 = fastrand::u32(..);
    base64::engine::general_purpose::STANDARD.encode(n.to_string().as_bytes())
}

#[derive(Clone, Copy)]
pub struct HeaderInputs<'a> {
    pub app_id: &'a str,
    pub client_version: &'a str,
    pub bot_token: Option<&'a str>,
    pub route_tag: Option<&'a str>,
}

/// Build the full header map for an iLink HTTP request (GET or POST).
/// For GET endpoints (QR login), pass `bot_token: None`; iLink only requires
/// the bearer for business endpoints.
pub fn build_headers(inputs: HeaderInputs) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    // Custom iLink headers — case-sensitive per server expectation. reqwest's
    // HeaderName requires lower-case, but the wire transmits exactly what we
    // pass; the server is documented to be case-insensitive (HTTP/1.1 spec),
    // and openclaw plugin in fact ships them in PascalCase. Be defensive and
    // use HeaderName::from_bytes with the original casing.
    if let (Ok(name), Ok(value)) = (
        HeaderName::from_bytes(b"iLink-App-Id"),
        HeaderValue::from_str(inputs.app_id),
    ) {
        h.insert(name, value);
    }
    if let (Ok(name), Ok(value)) = (
        HeaderName::from_bytes(b"iLink-App-ClientVersion"),
        HeaderValue::from_str(&encode_client_version(inputs.client_version).to_string()),
    ) {
        h.insert(name, value);
    }
    if let Ok(name) = HeaderName::from_bytes(b"AuthorizationType") {
        h.insert(name, HeaderValue::from_static(AUTHORIZATION_TYPE_VALUE));
    }
    if let Some(t) = inputs.bot_token.map(str::trim).filter(|t| !t.is_empty()) {
        if let Ok(v) = HeaderValue::from_str(&format!("Bearer {t}")) {
            h.insert(AUTHORIZATION, v);
        }
    }
    if let (Ok(name), Ok(value)) = (
        HeaderName::from_bytes(b"X-WECHAT-UIN"),
        HeaderValue::from_str(&generate_wechat_uin()),
    ) {
        h.insert(name, value);
    }
    if let Some(rt) = inputs.route_tag.filter(|s| !s.is_empty()) {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(b"SKRouteTag"),
            HeaderValue::from_str(rt),
        ) {
            h.insert(name, value);
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_client_version_known_values() {
        assert_eq!(encode_client_version("1.0.11"), 0x0001_000b);
        assert_eq!(encode_client_version("0.0.0"), 0);
        assert_eq!(encode_client_version("2.1.7"), 0x0002_0107);
    }

    #[test]
    fn encode_client_version_saturates_per_byte() {
        assert_eq!(encode_client_version("256.0.0"), 0);
        assert_eq!(encode_client_version("1.300.7"), 0x0001_2c07);
    }

    #[test]
    fn build_headers_includes_required_keys_when_token_present() {
        let h = build_headers(HeaderInputs {
            app_id: "test-app-id",
            client_version: "1.2.3",
            bot_token: Some("the-token"),
            route_tag: Some("zone-a"),
        });
        assert_eq!(h.get(CONTENT_TYPE).unwrap(), "application/json");
        // reqwest stores header names lowercased internally — query via lower-case.
        assert_eq!(h.get("ilink-app-id").unwrap(), "test-app-id");
        assert_eq!(
            h.get("ilink-app-clientversion").unwrap(),
            &encode_client_version("1.2.3").to_string()[..]
        );
        assert_eq!(h.get("authorizationtype").unwrap(), "ilink_bot_token");
        assert_eq!(h.get(AUTHORIZATION).unwrap(), "Bearer the-token");
        assert_eq!(h.get("skroutetag").unwrap(), "zone-a");
        let uin = h.get("x-wechat-uin").unwrap().to_str().unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(uin)
            .unwrap();
        let s = std::str::from_utf8(&decoded).unwrap();
        assert!(s.chars().all(|c| c.is_ascii_digit()), "uin payload: {s}");
    }

    #[test]
    fn build_headers_omits_authorization_when_token_absent_or_blank() {
        for tok in [None, Some(""), Some("  "), Some("\n")] {
            let h = build_headers(HeaderInputs {
                app_id: "x",
                client_version: "0.1.0",
                bot_token: tok,
                route_tag: None,
            });
            assert!(h.get(AUTHORIZATION).is_none(), "token={tok:?} should skip");
        }
    }
}
