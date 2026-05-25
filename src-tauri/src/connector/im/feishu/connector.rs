//! `FeishuConnector` — implements `IMConnector` for Lark/Feishu.
//!
//! PR3: `start()` spins up the WebSocket runtime (see `super::stream`) and
//! returns a `BoxStream<ChannelMessage>` to the manager.
//! PR4: `send()` posts `Text`/`Markdown` to `im.v1.messages` using a lazily
//! constructed `TokenCache<FeishuTokenSource>`.
//! PR5: `send()` handles `AiCardChunk` / `AiCardFail` via `super::card::CardKitSender`
//! — per-card serial mpsc + 100 ms throttle. See `super::card` for the details.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use tokio::sync::{OnceCell, RwLock};

use crate::connector::im::shared::token::TokenCache;
use crate::connector::im::trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector, InboundModel,
    ReplyContent, ReplyTarget,
};
use crate::connector::im::types::{ChannelConnectionState, ChannelMessage, Platform};

use super::card::CardKitSender;
use super::token::FeishuTokenSource;
use super::types::FeishuSessionTarget;

pub struct FeishuConnector {
    /// Tenant app credentials. `app_id` is also passed through `ReplyTarget`
    /// indirectly as the router namespacing key by the manager (see manager.rs
    /// worker loop).
    app_id: String,
    app_secret: String,
    /// Per-session reply target (`receive_id_type` + `receive_id`), populated
    /// by the manager worker when a message arrives and consumed by `send()`
    /// in PR4+. Keyed by internal `session_id`.
    session_targets: Arc<RwLock<HashMap<String, FeishuSessionTarget>>>,
    /// Lazy-init tenant_access_token cache. Built on first `send()` from the
    /// stored `app_id` / `app_secret`. `OnceCell` keeps repeated `send()`
    /// calls cheap (no per-call cache construction) and avoids a
    /// "send-before-start" race that an `Option<...>` set in `start()` would
    /// introduce.
    token_cache: OnceCell<Arc<TokenCache<FeishuTokenSource>>>,
    /// Lazy-init CardKit streaming sender. Same `OnceCell` pattern as
    /// `token_cache` — first `send(AiCardChunk)` builds it, subsequent calls
    /// reuse the same `Arc`. Sender holds the per-`card_id` mpsc registry
    /// internally; we don't poke at it from this side except via dispatch.
    card_sender: OnceCell<Arc<CardKitSender>>,
    /// Streams real WS lifecycle states back to ChannelManager. `Connected`
    /// must mean the Feishu WS endpoint opened successfully, not merely that
    /// the background task was spawned.
    on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
}

impl FeishuConnector {
    pub fn new(app_id: String, app_secret: String) -> Self {
        Self::with_status_callback(app_id, app_secret, Arc::new(|_state, _err| {}))
    }

    pub fn with_status_callback(
        app_id: String,
        app_secret: String,
        on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
    ) -> Self {
        Self {
            app_id,
            app_secret,
            session_targets: Arc::new(RwLock::new(HashMap::new())),
            token_cache: OnceCell::new(),
            card_sender: OnceCell::new(),
            on_status,
        }
    }

    /// Exposed to the manager (non-trait) so the worker loop can cache reply
    /// credentials at receive time, before kicking off the LLM turn that will
    /// eventually call `send()` with just a `ReplyTarget`.
    pub async fn remember_session(&self, session_id: String, target: FeishuSessionTarget) {
        self.session_targets
            .write()
            .await
            .insert(session_id, target);
    }

    /// Reply forwarder uses this to filter only feishu-owned sessions.
    pub async fn has_session(&self, session_id: &str) -> bool {
        self.session_targets.read().await.contains_key(session_id)
    }

    /// Caller-visible app_id; manager passes this to the router as the
    /// namespacing key (vs dingtalk's robot_code) — see ChannelSessionRouter.
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// Caller-visible app_secret accessor for PR4+ token cache construction.
    /// Stays inside the manager / connector boundary (never serialized).
    #[allow(dead_code)]
    pub(crate) fn app_secret(&self) -> &str {
        &self.app_secret
    }

    /// Lazily build (and remember) the `TokenCache<FeishuTokenSource>` used
    /// by `send()`. Repeated calls reuse the same `Arc`, which is what makes
    /// downstream HTTP calls hit the cached `tenant_access_token` instead of
    /// re-issuing `/open-apis/auth/v3/tenant_access_token/internal` per send.
    async fn get_or_init_token_cache(&self) -> Arc<TokenCache<FeishuTokenSource>> {
        self.token_cache
            .get_or_init(|| async {
                let source = Arc::new(FeishuTokenSource::new(
                    self.app_id.clone(),
                    self.app_secret.clone(),
                ));
                Arc::new(TokenCache::new(source))
            })
            .await
            .clone()
    }

    /// Lazily build (and remember) the `CardKitSender` used by the
    /// `AiCardChunk` / `AiCardFail` branches of `send()`. Shares the same
    /// `TokenCache` instance as the text path so token refreshes from
    /// either side benefit both.
    async fn get_or_init_card_sender(&self) -> Arc<CardKitSender> {
        let token_cache = self.get_or_init_token_cache().await;
        self.card_sender
            .get_or_init(|| async move { Arc::new(CardKitSender::new(token_cache)) })
            .await
            .clone()
    }

    /// Build a `FeishuFileDownloader` bound to this connector's shared
    /// `TokenCache` and write to `dest_dir`. PR6 entry point — called by
    /// the manager once per `connect_feishu` so the downloader's HTTP client
    /// reuses the same tenant_access_token cache as `send()` and the
    /// CardKit streaming path. Cheap to call; the downloader itself holds
    /// only a `reqwest::Client` and a `PathBuf`.
    pub(crate) async fn make_downloader(
        &self,
        dest_dir: std::path::PathBuf,
    ) -> Arc<super::download::FeishuFileDownloader> {
        let token_cache = self.get_or_init_token_cache().await;
        Arc::new(super::download::FeishuFileDownloader::new(
            token_cache,
            dest_dir,
        ))
    }
}

#[async_trait]
impl IMConnector for FeishuConnector {
    fn platform(&self) -> Platform {
        Platform::Feishu
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            inbound: InboundModel::Stream,
            outbound_aicard: true,
            outbound_text_streaming: false, // AI Card path; this field is only meaningful when outbound_aicard=false
            outbound_markdown: true,
            supports_attachments: true,
            supports_group_chat: true,
            supports_private_chat: true,
            auth_flow: AuthFlow::DeviceCode,
        }
    }

    async fn start(
        &self,
        ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
        // The /callback/ws/endpoint handshake takes AppID/AppSecret directly —
        // no tenant_access_token is needed at the ws layer (token is still
        // required for IM REST calls in PR4+, owned by FeishuTokenSource at
        // the connector level not the stream level).
        let (msg_tx, msg_rx) = tokio::sync::mpsc::channel::<ChannelMessage>(256);
        let client = super::stream::FeishuStreamClient::new(
            self.app_id.clone(),
            self.app_secret.clone(),
            msg_tx,
        );
        let on_status = Arc::clone(&self.on_status);
        client.start(
            move |state, err| on_status(state, err),
            ctx.cancel_token.clone(),
        );
        let stream = tokio_stream::wrappers::ReceiverStream::new(msg_rx).boxed();
        Ok(stream)
    }

    async fn send(&self, target: ReplyTarget, content: ReplyContent) -> Result<(), ConnectorError> {
        match content {
            // Both Text and Markdown go out as `msg_type=text`. Feishu has no
            // native `markdown` msg_type — proper rendering requires wrapping
            // markdown in an `interactive` card, which is PR5's job.
            //
            // TODO(pr5+): render Markdown via interactive card (see
            // openclaw-lark-main/src/card/markdown-style.ts) for proper
            // formatting; for now feishu clients show the raw markdown source.
            ReplyContent::Text(text) | ReplyContent::Markdown(text) => {
                // 1. Look up session target captured at receive time.
                let session = {
                    let map = self.session_targets.read().await;
                    map.get(&target.session_id).cloned()
                };
                let Some(session) = session else {
                    return Err(ConnectorError::Fatal(format!(
                        "FeishuConnector::send no session target for {}",
                        target.session_id
                    )));
                };

                // 2. Acquire (lazily) the tenant_access_token cache and pull
                //    a fresh-enough token.
                let cache = self.get_or_init_token_cache().await;
                let token = cache
                    .get()
                    .await
                    .map_err(|e| ConnectorError::Transient(format!("feishu token: {e:#}")))?;

                // 3. POST /open-apis/im/v1/messages with the right
                //    receive_id_type. `content` is a JSON-stringified inner
                //    payload — feishu requires this even for `msg_type=text`
                //    (see openclaw-lark-main/src/tools/oapi/im/message.ts line
                //    312-322 — the SDK serializes `data.content` as a string
                //    too).
                let url = format!(
                    "https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type={}",
                    session.receive_id_type
                );
                let body_content = serde_json::json!({ "text": text }).to_string();
                let body = serde_json::json!({
                    "receive_id": session.receive_id,
                    "msg_type": "text",
                    "content": body_content,
                });

                let client = reqwest::Client::new();
                let resp = client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| ConnectorError::Transient(format!("feishu send http: {e:#}")))?;

                let status = resp.status();
                let resp_body = resp.text().await.unwrap_or_default();
                classify_feishu_envelope_response(status, &resp_body)
            }
            ReplyContent::AiCardChunk { delta, final_chunk } => {
                // Same Fatal-on-missing-session guard as the Text branch.
                // CardKit needs a `receive_id_type` + `receive_id` to deliver
                // the card to chat (the card.create call alone doesn't
                // address a conversation).
                let session = {
                    let map = self.session_targets.read().await;
                    map.get(&target.session_id).cloned()
                };
                let Some(session) = session else {
                    return Err(ConnectorError::Fatal(format!(
                        "FeishuConnector::send no session target for AiCardChunk {}",
                        target.session_id
                    )));
                };
                let sender = self.get_or_init_card_sender().await;
                sender
                    .dispatch_chunk(&target.session_id, &session, &delta, final_chunk)
                    .await
                    .map_err(|e| ConnectorError::Transient(format!("cardkit chunk: {e:#}")))
            }
            ReplyContent::AiCardFail => {
                // No session lookup: dispatch_fail is a no-op when no card
                // was created yet (e.g. the run errored before producing any
                // delta), which matches the "show fail card only if there
                // was something to fail" UX intent. Errors during the fail
                // path itself are surfaced as Transient because the user
                // likely got the error from another channel already.
                let sender = self.get_or_init_card_sender().await;
                sender
                    .dispatch_fail(&target.session_id)
                    .await
                    .map_err(|e| ConnectorError::Transient(format!("cardkit fail: {e:#}")))
            }
        }
    }
}

/// Classify a feishu envelope response (used by both `/open-apis/im/v1/messages`
/// and `/open-apis/cardkit/v1/*` — both wrap their results in the same
/// `{code, msg, data}` envelope) into a `Result<(), ConnectorError>` per the
/// table in `docs/superpowers/specs/2026-05-18-im-feishu-phase1-design.md` §3:
///
/// | code     | maps to       | reason |
/// |----------|---------------|--------|
/// | 0        | `Ok(())`      | success |
/// | 99991661 | `Fatal`       | invalid_request — parameter bug in our code, retry won't help |
/// | 99991663 | `Transient`   | tenant_access_token expired — TokenCache auto-refreshes next call |
/// | 99991664 | `Fatal`       | user_access_token invalid (Non-Goals path) |
/// | 99991668 | `AuthExpired` | token invalid — force device-code re-registration |
/// | 230002   | `Transient`   | CardKit sequence error — next chunk's higher seq usually wins |
/// | 230005   | `Transient`   | CardKit card not found — sender task gives up gracefully |
/// | HTTP 401 | `AuthExpired` | unauthorized at HTTP layer |
/// | other    | `Transient`   | default retryable; includes non-JSON / 5xx |
///
/// Extracted as a pure function so the mapping is unit-testable without
/// standing up an HTTP mock. Renamed from `classify_feishu_envelope_response`
/// in PR5 because `card.rs` re-uses it for the cardkit envelope shape.
pub(super) fn classify_feishu_envelope_response(
    status: reqwest::StatusCode,
    body: &str,
) -> Result<(), ConnectorError> {
    // Feishu wraps even successful responses in `{ code, msg, data }`.
    // A 200 with `code != 0` is still an application-level failure.
    // Parse leniently — body might not be JSON on infra failures.
    let code: Option<i64> = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("code").and_then(|c| c.as_i64()));

    if status.is_success() && code == Some(0) {
        return Ok(());
    }

    // Errcode-driven branches take precedence over HTTP status so that a
    // 200 + `code=99991668` still maps to AuthExpired (feishu loves to put
    // app-level failures in a 200 envelope).
    match code {
        Some(99991663) => {
            return Err(ConnectorError::Transient(format!(
                "feishu tenant_access_token expired: status={} body={}",
                status, body
            )));
        }
        Some(99991668) => {
            return Err(ConnectorError::AuthExpired(format!(
                "feishu token invalid: status={} body={}",
                status, body
            )));
        }
        Some(99991661) | Some(99991664) => {
            return Err(ConnectorError::Fatal(format!(
                "feishu request error code={:?} status={} body={}",
                code, status, body
            )));
        }
        // CardKit-specific transient errors (see spec §3 + plan §3). 230002
        // is "sequence error" — when a sender task somehow desyncs from the
        // server, the next chunk's higher seq usually breaks the deadlock.
        // 230005 is "card not found" — the card may have been GC'd or the
        // sender task is operating on a stale id. Either way: Transient so
        // the next AI run can re-create.
        Some(230002) | Some(230005) => {
            return Err(ConnectorError::Transient(format!(
                "feishu cardkit error code={:?} status={} body={}",
                code, status, body
            )));
        }
        _ => {}
    }

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(ConnectorError::AuthExpired(format!(
            "feishu HTTP 401: body={}",
            body
        )));
    }

    Err(ConnectorError::Transient(format!(
        "feishu send failed: status={} body={}",
        status, body
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_is_feishu() {
        let c = FeishuConnector::new("app_id".into(), "app_secret".into());
        assert_eq!(c.platform(), Platform::Feishu);
    }

    #[test]
    fn capabilities_reports_stream_and_aicard_and_device_code() {
        let c = FeishuConnector::new("ak".into(), "as".into());
        let caps = c.capabilities();
        assert!(matches!(caps.inbound, InboundModel::Stream));
        assert!(caps.outbound_aicard);
        assert!(caps.outbound_markdown);
        assert!(caps.supports_attachments);
        assert!(matches!(caps.auth_flow, AuthFlow::DeviceCode));
    }

    #[test]
    fn app_id_accessor_returns_stored_value() {
        let c = FeishuConnector::new("cli_abc".into(), "as".into());
        assert_eq!(c.app_id(), "cli_abc");
    }

    #[tokio::test]
    async fn remember_session_inserts() {
        let c = FeishuConnector::new("ak".into(), "as".into());
        c.remember_session(
            "sess-1".into(),
            FeishuSessionTarget {
                receive_id_type: "open_id".into(),
                receive_id: "ou_xxx".into(),
            },
        )
        .await;
        let map = c.session_targets.read().await;
        assert!(map.contains_key("sess-1"));
    }

    #[tokio::test]
    async fn send_text_without_known_session_returns_fatal() {
        // Pre-PR4 send() unconditionally returned NotSupported. Post-PR4 the
        // first guard is the session_targets lookup — a Fatal here proves
        // we entered the Text branch and the lookup is the failure point.
        let c = FeishuConnector::new("ak".into(), "as".into());
        let err = c
            .send(
                ReplyTarget {
                    session_id: "missing".into(),
                    external_conversation_key: "oc_x".into(),
                },
                ReplyContent::Text("hi".into()),
            )
            .await
            .unwrap_err();
        match err {
            ConnectorError::Fatal(msg) => {
                assert!(
                    msg.contains("no session target") && msg.contains("missing"),
                    "expected Fatal('no session target ... missing'), got: {msg}"
                );
            }
            other => panic!("expected Fatal, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn send_markdown_without_known_session_also_returns_fatal() {
        // Markdown shares the Text branch — same Fatal guard fires first.
        let c = FeishuConnector::new("ak".into(), "as".into());
        let err = c
            .send(
                ReplyTarget {
                    session_id: "missing".into(),
                    external_conversation_key: "oc_x".into(),
                },
                ReplyContent::Markdown("**hi**".into()),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ConnectorError::Fatal(_)));
    }

    #[tokio::test]
    async fn send_aicard_chunk_without_known_session_returns_fatal() {
        // Pre-PR5: this returned NotSupported (CardKit stub). Post-PR5 the
        // first guard is the same session_targets lookup as the Text path —
        // Fatal here proves we entered the AiCardChunk branch and the
        // session-missing guard fired, not a stale stub.
        let c = FeishuConnector::new("ak".into(), "as".into());
        let err = c
            .send(
                ReplyTarget {
                    session_id: "missing".into(),
                    external_conversation_key: "oc_x".into(),
                },
                ReplyContent::AiCardChunk {
                    delta: "x".into(),
                    final_chunk: true,
                },
            )
            .await
            .unwrap_err();
        match err {
            ConnectorError::Fatal(msg) => {
                assert!(
                    msg.contains("no session target")
                        && msg.contains("AiCardChunk")
                        && msg.contains("missing"),
                    "expected Fatal('no session target for AiCardChunk ... missing'), got: {msg}"
                );
            }
            other => panic!("expected Fatal, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn send_aicard_fail_for_unknown_session_is_ok_noop() {
        // AiCardFail is intentionally tolerant: if no card was ever created
        // for this session (e.g. the run errored before producing any
        // delta), there's nothing to fail. Returning Ok here matches the
        // "show fail card only if there was something to fail" UX, and
        // saves the manager from a noisy error log on every aborted run.
        let c = FeishuConnector::new("ak".into(), "as".into());
        let result = c
            .send(
                ReplyTarget {
                    session_id: "never-created".into(),
                    external_conversation_key: "oc_x".into(),
                },
                ReplyContent::AiCardFail,
            )
            .await;
        assert!(result.is_ok(), "expected Ok no-op, got {:?}", result);
    }

    #[tokio::test]
    async fn get_or_init_token_cache_returns_same_arc_across_calls() {
        // Establishes the OnceCell semantics: the cache is constructed once
        // and subsequent send() calls reuse the same TokenCache, so an
        // already-acquired tenant_access_token actually gets reused.
        let c = FeishuConnector::new("ak".into(), "as".into());
        let a = c.get_or_init_token_cache().await;
        let b = c.get_or_init_token_cache().await;
        assert!(
            Arc::ptr_eq(&a, &b),
            "OnceCell must hand out the same Arc on every call"
        );
    }

    // ------------------------------------------------------------------
    // classify_feishu_envelope_response: one test per row of the §3 table.
    // Pure function — no HTTP mock required.
    // ------------------------------------------------------------------

    #[test]
    fn classify_response_success_returns_ok() {
        let body = r#"{"code":0,"msg":"ok","data":{}}"#;
        assert!(classify_feishu_envelope_response(reqwest::StatusCode::OK, body).is_ok());
    }

    #[test]
    fn classify_response_99991661_maps_to_fatal() {
        let body = r#"{"code":99991661,"msg":"invalid_request"}"#;
        let err = classify_feishu_envelope_response(reqwest::StatusCode::OK, body).unwrap_err();
        assert!(
            matches!(err, ConnectorError::Fatal(_)),
            "99991661 must be Fatal (param bug — retry doesn't help), got: {err:?}"
        );
    }

    #[test]
    fn classify_response_99991663_maps_to_transient() {
        // Critical: tenant_access_token expiry happens every 2h in production.
        // TokenCache auto-refreshes, so this MUST be Transient — mapping to
        // AuthExpired would spam users with re-registration prompts hourly.
        let body = r#"{"code":99991663,"msg":"token expired"}"#;
        let err = classify_feishu_envelope_response(reqwest::StatusCode::OK, body).unwrap_err();
        assert!(
            matches!(err, ConnectorError::Transient(_)),
            "99991663 must be Transient (TokenCache auto-refreshes), got: {err:?}"
        );
    }

    #[test]
    fn classify_response_99991664_maps_to_fatal() {
        let body = r#"{"code":99991664,"msg":"user_access_token invalid"}"#;
        let err = classify_feishu_envelope_response(reqwest::StatusCode::OK, body).unwrap_err();
        assert!(
            matches!(err, ConnectorError::Fatal(_)),
            "99991664 (user_access_token, Non-Goals) must be Fatal, got: {err:?}"
        );
    }

    #[test]
    fn classify_response_99991668_maps_to_auth_expired() {
        let body = r#"{"code":99991668,"msg":"token invalid"}"#;
        let err = classify_feishu_envelope_response(reqwest::StatusCode::OK, body).unwrap_err();
        assert!(
            matches!(err, ConnectorError::AuthExpired(_)),
            "99991668 must be AuthExpired (forces device-code re-registration), got: {err:?}"
        );
    }

    #[test]
    fn classify_response_http_401_maps_to_auth_expired() {
        let err =
            classify_feishu_envelope_response(reqwest::StatusCode::UNAUTHORIZED, "").unwrap_err();
        assert!(
            matches!(err, ConnectorError::AuthExpired(_)),
            "HTTP 401 must be AuthExpired, got: {err:?}"
        );
    }

    #[test]
    fn classify_response_unknown_500_maps_to_transient() {
        let err = classify_feishu_envelope_response(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"code":99999,"msg":"unknown"}"#,
        )
        .unwrap_err();
        assert!(
            matches!(err, ConnectorError::Transient(_)),
            "unknown errcode + 5xx must be Transient (default retryable), got: {err:?}"
        );
    }

    #[test]
    fn classify_response_non_json_body_falls_back_to_transient() {
        // Infra-layer failures (Cloudflare 5xx, gateway HTML, etc.) — body
        // isn't JSON, code parse returns None, must still be retryable.
        let err = classify_feishu_envelope_response(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "<html>internal error</html>",
        )
        .unwrap_err();
        assert!(
            matches!(err, ConnectorError::Transient(_)),
            "non-JSON body must fall back to Transient, got: {err:?}"
        );
    }

    // CardKit-specific errcodes (PR5): pin the explicit branch so a future
    // edit can't accidentally collapse them into the default-Transient
    // fallback and lose the dedicated log line / error message shape.
    #[test]
    fn classify_response_230002_cardkit_sequence_maps_to_transient() {
        let body = r#"{"code":230002,"msg":"sequence error"}"#;
        let err = classify_feishu_envelope_response(reqwest::StatusCode::OK, body).unwrap_err();
        assert!(
            matches!(err, ConnectorError::Transient(_)),
            "230002 (CardKit sequence error) must be Transient, got: {err:?}"
        );
    }

    #[test]
    fn classify_response_230005_cardkit_card_not_found_maps_to_transient() {
        let body = r#"{"code":230005,"msg":"card not found"}"#;
        let err = classify_feishu_envelope_response(reqwest::StatusCode::OK, body).unwrap_err();
        assert!(
            matches!(err, ConnectorError::Transient(_)),
            "230005 (CardKit card not found) must be Transient, got: {err:?}"
        );
    }
}
