use reqwest::StatusCode;

use crate::runtime::network::state::{NetworkErrorKind, NetworkStatus};

/// 把一次 HEAD 请求的结果（reqwest::Result<reqwest::Response>）映射为三态。
pub(crate) fn classify_response(
    result: &Result<reqwest::Response, reqwest::Error>,
) -> (NetworkStatus, Option<NetworkErrorKind>) {
    match result {
        Ok(resp) => {
            let status = resp.status();
            if status.is_server_error() {
                (NetworkStatus::ServerDegraded, None)
            } else {
                // 2xx / 3xx / 4xx including 401/403 — TCP+TLS+HTTP shook hands.
                (NetworkStatus::Online, None)
            }
        }
        Err(err) => {
            let kind = classify_error(err);
            (NetworkStatus::Offline, Some(kind))
        }
    }
}

pub(crate) fn classify_error(err: &reqwest::Error) -> NetworkErrorKind {
    if err.is_timeout() {
        return NetworkErrorKind::Timeout;
    }
    if err.is_connect() {
        let msg = err.to_string().to_lowercase();
        if msg.contains("dns") || msg.contains("name resolution") || msg.contains("lookup") {
            return NetworkErrorKind::Dns;
        }
        if msg.contains("refused") {
            return NetworkErrorKind::ConnectRefused;
        }
        if msg.contains("certificate") || msg.contains("tls") || msg.contains("ssl") {
            return NetworkErrorKind::Tls;
        }
        return NetworkErrorKind::Other;
    }
    NetworkErrorKind::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_response(status: StatusCode) -> Result<reqwest::Response, reqwest::Error> {
        Ok(reqwest::Response::from(
            http::Response::builder()
                .status(status)
                .body("")
                .unwrap(),
        ))
    }

    #[test]
    fn test_200_is_online() {
        let (status, kind) = classify_response(&ok_response(StatusCode::OK));
        assert_eq!(status, NetworkStatus::Online);
        assert_eq!(kind, None);
    }

    #[test]
    fn test_401_is_online() {
        let (status, kind) = classify_response(&ok_response(StatusCode::UNAUTHORIZED));
        assert_eq!(status, NetworkStatus::Online);
        assert_eq!(kind, None);
    }

    #[test]
    fn test_500_is_server_degraded() {
        let (status, _) = classify_response(&ok_response(StatusCode::INTERNAL_SERVER_ERROR));
        assert_eq!(status, NetworkStatus::ServerDegraded);
    }

    #[test]
    fn test_502_is_server_degraded() {
        let (status, _) = classify_response(&ok_response(StatusCode::BAD_GATEWAY));
        assert_eq!(status, NetworkStatus::ServerDegraded);
    }
}
