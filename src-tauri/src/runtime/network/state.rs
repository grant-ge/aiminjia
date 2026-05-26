use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkStatus {
    Online,
    Offline,
    ServerDegraded,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
// snake_case: spec §5.2 requires "connect_refused" (kebab-case would give "connect-refused")
#[serde(rename_all = "snake_case")]
pub enum NetworkErrorKind {
    Timeout,
    Dns,
    ConnectRefused,
    Tls,
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSnapshot {
    pub status: NetworkStatus,
    pub last_check_at_ms: i64,
    pub latency_ms: Option<u32>,
    pub error_kind: Option<NetworkErrorKind>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serializes_kebab_case() {
        let json = serde_json::to_string(&NetworkStatus::ServerDegraded).unwrap();
        assert_eq!(json, "\"server-degraded\"");
    }

    #[test]
    fn test_error_kind_serializes_snake_case() {
        let json = serde_json::to_string(&NetworkErrorKind::ConnectRefused).unwrap();
        assert_eq!(json, "\"connect_refused\"");
    }

    #[test]
    fn test_snapshot_camel_case_keys() {
        let snap = NetworkSnapshot {
            status: NetworkStatus::Offline,
            last_check_at_ms: 1234,
            latency_ms: None,
            error_kind: Some(NetworkErrorKind::Dns),
        };
        let json = serde_json::to_value(&snap).unwrap();
        assert!(json.get("lastCheckAtMs").is_some());
        assert!(json.get("errorKind").is_some());
        assert!(json.get("latencyMs").is_some());
        assert_eq!(json["latencyMs"], serde_json::Value::Null);
        // Also verify no snake_case keys leaked through
        assert!(json.get("last_check_at_ms").is_none());
        assert!(json.get("error_kind").is_none());
        // And verify actual values
        assert_eq!(json["status"], "offline");
        assert_eq!(json["lastCheckAtMs"], 1234);
        assert_eq!(json["errorKind"], "dns");
    }
}
