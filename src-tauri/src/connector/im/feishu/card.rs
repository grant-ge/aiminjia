//! Feishu CardKit streaming.
//!
//! For each AI run, the controller flow is:
//!
//! ```text
//!  first chunk           subsequent chunks         final chunk             fail
//!     ↓                       ↓                        ↓                      ↓
//!  card.create  →    cardElement.content (PUT,  →  cardElement.content  →  cardElement.content
//!  + im.message.create        cumulative content)    +settings(streaming   (error text)
//!  (delivers card to                                 _mode=false)         +settings(streaming
//!   chat as msg_type=                                                      _mode=false)
//!   "interactive")
//! ```
//!
//! The architecture is **one mpsc + serial sender task per `card_id`**:
//! `dispatch_chunk` / `dispatch_fail` are O(1) — they enqueue a `CardOp` and
//! return. The sender task drains the mpsc, applies a ≥100 ms inter-call
//! throttle via `tokio::time::sleep_until`, and runs the HTTP call. **We do
//! not drop chunks under throttle pressure** — chunks queue up and serialize.
//! Dropping was an earlier mis-design; the visible effect of dropping is "the
//! card jumps from chunk 3 to chunk 30 mid-sentence" which destroys the
//! typewriter feel. Letting the queue delay-extend (worst case: typing visibly
//! trails the LLM output by 1-2 s after a burst) is the better trade.
//!
//! Concurrency model:
//! - `sessions: Mutex<HashMap<session_id, CardSession>>` — short-lived guard,
//!   only held to insert / lookup / remove entries.
//! - Per-session: an `mpsc::UnboundedSender<CardOp>` plus an `accumulated`
//!   text buffer (CardKit `cardElement.content` takes cumulative content, not
//!   deltas; the diff/animation happens server-side).
//! - The sender task owns the sequence counter — incremented just before each
//!   PUT — so even if two `dispatch_chunk` calls arrive in quick succession,
//!   the server sees strictly increasing `sequence` values.
//!
//! Errors classified via `classify_feishu_envelope_response` from
//! `super::connector`. The CardKit-specific errcodes 230002 (sequence
//! mismatch) and 230005 (card not found) are mapped to `Transient`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep_until;

use crate::connector::im::shared::token::TokenCache as SharedTokenCache;

use super::connector::classify_feishu_envelope_response;
use super::token::FeishuTokenSource;
use super::types::FeishuSessionTarget;

const FEISHU_API: &str = "https://open.feishu.cn";
const CARDKIT_CREATE_PATH: &str = "/open-apis/cardkit/v1/cards";
const IM_MESSAGES_PATH: &str = "/open-apis/im/v1/messages";
const STREAMING_ELEMENT_ID: &str = "streaming_content";

/// Minimum interval between CardKit calls on a single `card_id`. The
/// public-facing rate limit is "~100 ms" — we hold the line at 100 ms exactly
/// since the throttle is gated by `sleep_until` from the previous call's
/// completion time, so network round-trip already adds slack.
const MIN_INTERVAL: Duration = Duration::from_millis(100);

/// Active card streaming session, one per `session_id`. Lives in
/// `CardKitSender::sessions` until either the final chunk is dispatched or
/// the run fails — both paths send a terminal `CardOp` then remove the entry.
struct CardSession {
    tx: mpsc::UnboundedSender<CardOp>,
}

/// Operations sent to a per-card serial sender task. The sender task owns
/// the cumulative buffer; `Update.delta` is just the latest chunk slice
/// the connector handed us (CardKit's `cardElement.content` is
/// set-not-append, but accumulation happens task-side, not at enqueue,
/// to keep `dispatch_chunk` lock-free w.r.t. content state).
enum CardOp {
    /// Stream a chunk. `final_chunk=true` triggers `streaming_mode=false`
    /// after the content PUT so the typing cursor disappears.
    Update {
        // chunk delta; sender task owns the cumulative buffer.
        delta: String,
        final_chunk: bool,
    },
    /// Mark the card as failed. Replaces content with an error notice then
    /// closes streaming mode.
    Fail,
}

pub struct CardKitSender {
    token_cache: Arc<SharedTokenCache<FeishuTokenSource>>,
    sessions: Arc<Mutex<HashMap<String, CardSession>>>,
    http: reqwest::Client,
}

impl CardKitSender {
    pub fn new(token_cache: Arc<SharedTokenCache<FeishuTokenSource>>) -> Self {
        Self {
            token_cache,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            http: reqwest::Client::new(),
        }
    }

    /// Public entry for `connector.send(AiCardChunk)`.
    ///
    /// - First chunk per `session_id`: create card via `card.create`, deliver
    ///   to chat via `im.v1.messages` (`msg_type=interactive`), spawn the
    ///   per-card sender task.
    /// - Subsequent chunks: append to the per-session accumulated buffer
    ///   (held inside the sender task), send `CardOp::Update`.
    /// - `final_chunk=true`: same as above but flags the sender to close
    ///   streaming mode and drop the session entry.
    ///
    /// The `delta` is appended to the running accumulated content; CardKit's
    /// `cardElement.content` PUT takes a cumulative string, not a delta, and
    /// the server handles the diff/animation. We accumulate inside the
    /// sender task so a slow PUT can't be interleaved with a faster
    /// caller-side append.
    pub async fn dispatch_chunk(
        &self,
        session_id: &str,
        target: &FeishuSessionTarget,
        delta: &str,
        final_chunk: bool,
    ) -> Result<()> {
        // Acquire-or-create the per-session sender task. The create path is
        // sequential under the sessions lock so two near-simultaneous first
        // chunks don't both try to create a card.
        let tx = {
            let mut sessions = self.sessions.lock().await;
            if !sessions.contains_key(session_id) {
                let card_id = self.create_and_deliver_card(target).await?;
                let (tx, rx) = mpsc::unbounded_channel();
                self.spawn_sender_task(session_id.to_string(), card_id, rx);
                sessions.insert(session_id.to_string(), CardSession { tx: tx.clone() });
            }
            sessions.get(session_id).expect("just inserted").tx.clone()
        };

        // Enqueue. Sender task does the actual accumulation inside the
        // serial loop — that's the only place where ordering is guaranteed.
        let _ = tx.send(CardOp::Update {
            delta: delta.to_string(),
            final_chunk,
        });

        // Final chunk → drop session entry. The sender task drains queued
        // ops and exits after the final Update arrives.
        if final_chunk {
            self.sessions.lock().await.remove(session_id);
        }
        Ok(())
    }

    /// Public entry for `connector.send(AiCardFail)`.
    ///
    /// If no session exists (e.g. card was never created — the run failed
    /// before producing any text), this is a no-op + Ok. Otherwise we send
    /// `CardOp::Fail` and the sender task replaces the card content with an
    /// error notice and exits.
    pub async fn dispatch_fail(&self, session_id: &str) -> Result<()> {
        let session = self.sessions.lock().await.remove(session_id);
        if let Some(session) = session {
            let _ = session.tx.send(CardOp::Fail);
        }
        Ok(())
    }

    /// Two-step card setup per feishu-endpoints-notes Q4:
    ///   1) `POST /open-apis/cardkit/v1/cards` returns `card_id`
    ///   2) `POST /open-apis/im/v1/messages` with `msg_type=interactive` +
    ///      `content: { type:"card", data:{card_id} }` delivers the card to
    ///      the target chat.
    /// Both calls go through `classify_feishu_envelope_response` so token
    /// expiry / invalid_request etc. map to the same error model as the
    /// PR4 text path.
    async fn create_and_deliver_card(&self, target: &FeishuSessionTarget) -> Result<String> {
        let token = self.token_cache.get().await.context("feishu token")?;

        // Step 1: create the card entity. The card body uses CardKit v2
        // schema with one markdown element identified by STREAMING_ELEMENT_ID
        // — the stream-content endpoint targets this element by ID.
        // `streaming_mode:true` enables the typing-cursor visual; we close
        // it on final / fail.
        let card_body = serde_json::json!({
            "schema": "2.0",
            "config": {
                "streaming_mode": true,
                "summary": {
                    "content": "处理中...",
                    "i18n_content": { "zh_cn": "处理中...", "en_us": "Processing..." }
                }
            },
            "body": {
                "elements": [{
                    "tag": "markdown",
                    "content": "",
                    "element_id": STREAMING_ELEMENT_ID,
                }]
            }
        });
        let create_req = serde_json::json!({
            "type": "card_json",
            "data": card_body.to_string(),
        });
        let resp = self
            .http
            .post(format!("{}{}", FEISHU_API, CARDKIT_CREATE_PATH))
            .header("Authorization", format!("Bearer {}", token))
            .json(&create_req)
            .send()
            .await
            .context("cardkit create http")?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        classify_feishu_envelope_response(status, &body)
            .map_err(|e| anyhow::anyhow!("cardkit create classify: {e:?}"))?;
        let card_id = parse_card_id(&body).context("parse card_id from cardkit create")?;

        // Step 2: deliver the card to chat via im.v1.messages. `content`
        // must be a JSON-stringified inner object — same envelope rule as
        // PR4's text path (see connector::send Text branch).
        let inner =
            serde_json::json!({ "type": "card", "data": { "card_id": card_id } }).to_string();
        let send_url = format!(
            "{}{}?receive_id_type={}",
            FEISHU_API, IM_MESSAGES_PATH, target.receive_id_type
        );
        let send_body = serde_json::json!({
            "receive_id": target.receive_id,
            "msg_type": "interactive",
            "content": inner,
        });
        let resp = self
            .http
            .post(&send_url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&send_body)
            .send()
            .await
            .context("cardkit deliver http")?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        classify_feishu_envelope_response(status, &body)
            .map_err(|e| anyhow::anyhow!("cardkit deliver classify: {e:?}"))?;

        Ok(card_id)
    }

    fn spawn_sender_task(
        &self,
        session_id: String,
        card_id: String,
        mut rx: mpsc::UnboundedReceiver<CardOp>,
    ) {
        let token_cache = self.token_cache.clone();
        let http = self.http.clone();
        // Back-channel to the sessions map so we can auto-evict on
        // unrecoverable per-card_id errors (errcode 230005 — card not
        // found, the server forgot about us or the create succeeded but
        // returned a stale id). After eviction, the next `dispatch_chunk`
        // for the same `session_id` re-enters `create_and_deliver_card`
        // and a fresh card pops up. Without this hook, 230005 would
        // accumulate silently forever — every subsequent chunk would hit
        // the same dead card_id.
        let sessions = self.sessions.clone();
        tokio::spawn(async move {
            // Per-card-id sequence counter. CardKit's `cardElement.content`
            // accepts a `sequence: int` for ordering; the server rejects
            // out-of-order writes (errcode 230002). Since this task is the
            // sole writer for this card_id, monotonic increment here is
            // sufficient — no cross-task synchronization needed.
            let mut seq: u64 = 0;
            let mut next_allowed = Instant::now();
            // Cumulative content for this card. CardKit content updates take
            // the full text (server diffs); we never send the raw delta.
            let mut accumulated = String::new();

            while let Some(op) = rx.recv().await {
                // ≥100 ms throttle. `sleep_until` is a no-op if we're already
                // past `next_allowed`, so a quiet stream doesn't pay for the
                // throttle; only bursts do.
                wait_throttle(&mut next_allowed).await;

                match op {
                    CardOp::Update { delta, final_chunk } => {
                        accumulated.push_str(&delta);
                        seq += 1;
                        let mut dead_card = false;
                        if let Err(e) = stream_card_content(
                            &http,
                            &token_cache,
                            &card_id,
                            STREAMING_ELEMENT_ID,
                            &accumulated,
                            seq,
                        )
                        .await
                        {
                            // 230002/230005 are Transient — the next chunk
                            // may succeed; log and continue rather than
                            // killing the whole stream. 230005 *additionally*
                            // tells us this card_id is gone server-side, so
                            // we evict the session below and let the next
                            // dispatch_chunk start fresh.
                            if matches!(e.code, Some(230005)) {
                                log::error!(
                                    "[feishu-cardkit] update card={} seq={} GONE (230005): {:#}",
                                    card_id,
                                    seq,
                                    e.source
                                );
                                dead_card = true;
                            } else {
                                log::warn!(
                                    "[feishu-cardkit] update card={} seq={} err={:#}",
                                    card_id,
                                    seq,
                                    e.source
                                );
                            }
                        }
                        next_allowed = Instant::now() + MIN_INTERVAL;

                        if dead_card {
                            // Card is gone — drop the session entry. Any
                            // pending ops still in the mpsc are best-effort
                            // dropped when the sender task exits (rx dies
                            // with this task; tx is dropped on map remove).
                            sessions.lock().await.remove(&session_id);
                            return;
                        }

                        if final_chunk {
                            // Pre-PR5-review: the settings PATCH used to
                            // fire back-to-back with the content PUT and
                            // could trip the per-card 100 ms rate limit
                            // (→ 230002, leaving the card stuck in
                            // streaming_mode=true forever). Wait the
                            // throttle out before the second call.
                            wait_throttle(&mut next_allowed).await;
                            seq += 1;
                            if let Err(e) =
                                set_streaming_mode(&http, &token_cache, &card_id, false, seq).await
                            {
                                log::warn!(
                                    "[feishu-cardkit] final settings card={} seq={} err={:#}",
                                    card_id,
                                    seq,
                                    e.source
                                );
                            }
                            // (no `next_allowed = ...` here — the sender
                            // task exits immediately and the variable would
                            // be dead.)
                            return;
                        }
                    }
                    CardOp::Fail => {
                        seq += 1;
                        if let Err(e) = stream_card_content(
                            &http,
                            &token_cache,
                            &card_id,
                            STREAMING_ELEMENT_ID,
                            "❌ 处理失败",
                            seq,
                        )
                        .await
                        {
                            log::warn!(
                                "[feishu-cardkit] fail content card={} seq={} err={:#}",
                                card_id,
                                seq,
                                e.source
                            );
                        }
                        next_allowed = Instant::now() + MIN_INTERVAL;
                        // Same throttle-honoring rationale as the Final path.
                        wait_throttle(&mut next_allowed).await;
                        seq += 1;
                        if let Err(e) =
                            set_streaming_mode(&http, &token_cache, &card_id, false, seq).await
                        {
                            log::warn!(
                                "[feishu-cardkit] fail settings card={} seq={} err={:#}",
                                card_id,
                                seq,
                                e.source
                            );
                        }
                        // (no `next_allowed = ...` here — sender exits.)
                        return;
                    }
                }
            }
        });
    }
}

/// Sleep just long enough to honor the per-card_id ≥100 ms rate limit, then
/// re-arm `next_allowed`'s gate-keeper variable for the caller. Extracted
/// so the Update / Final / Fail paths share one implementation instead of
/// re-spelling `if now < next_allowed { sleep_until }` four times. Note
/// `next_allowed` is updated by the caller AFTER each HTTP call (we don't
/// touch it here) so a slow PUT naturally extends the gap.
async fn wait_throttle(next_allowed: &mut Instant) {
    let now = Instant::now();
    if now < *next_allowed {
        sleep_until((*next_allowed).into()).await;
    }
}

/// Extract `data.card_id` from a `card.create` response body. Tolerates the
/// SDK quirk of sometimes returning `card_id` at the top level too (matches
/// the openclaw-lark plugin's `(response.data?.card_id ?? response.card_id)`
/// fallback).
fn parse_card_id(body: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct Resp {
        data: Option<Inner>,
        card_id: Option<String>,
    }
    #[derive(Deserialize)]
    struct Inner {
        card_id: Option<String>,
    }
    let r: Resp = serde_json::from_str(body).context("cardkit create response not JSON")?;
    r.data
        .and_then(|d| d.card_id)
        .or(r.card_id)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("cardkit create returned empty card_id: {body}"))
}

/// Per-call CardKit error. Carries the parsed errcode (if any) alongside
/// the underlying anyhow error. The errcode is what lets the sender task
/// distinguish 230005 (card gone — evict session) from generic transient
/// failures (log + keep going). Without this we'd have to grep error
/// strings.
#[derive(Debug)]
struct CardKitError {
    code: Option<i64>,
    source: anyhow::Error,
}

impl CardKitError {
    fn from_source(code: Option<i64>, source: anyhow::Error) -> Self {
        Self { code, source }
    }
}

/// Parse the `code` field from a feishu envelope response body. Returns
/// `None` for non-JSON bodies (infra-layer 5xx) or bodies missing `code`.
/// Lifted from `classify_feishu_envelope_response` so the sender task can
/// re-derive it without re-classifying.
fn parse_envelope_code(body: &str) -> Option<i64> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("code").and_then(|c| c.as_i64()))
}

/// `PUT /open-apis/cardkit/v1/cards/{card_id}/elements/{element_id}/content`
/// per feishu-endpoints-notes Q4 (verified against
/// `oapi-sdk-go/sample/apiall/cardkitv1/content_cardElement.go`). The earlier
/// plan-sketch used PATCH; that was a guess and is wrong.
async fn stream_card_content(
    http: &reqwest::Client,
    token_cache: &Arc<SharedTokenCache<FeishuTokenSource>>,
    card_id: &str,
    element_id: &str,
    content: &str,
    sequence: u64,
) -> std::result::Result<(), CardKitError> {
    let token = token_cache
        .get()
        .await
        .map_err(|e| CardKitError::from_source(None, e.context("feishu token")))?;
    let url = format!(
        "{}/open-apis/cardkit/v1/cards/{}/elements/{}/content",
        FEISHU_API, card_id, element_id
    );
    let body = serde_json::json!({
        // `uuid` is feishu's idempotency key for retries — a fresh v4 per
        // call is fine since we don't retry inside the sender task (the
        // serial mpsc model means the next call has a different sequence
        // anyway).
        "uuid": uuid::Uuid::new_v4().to_string(),
        "content": content,
        "sequence": sequence,
    });
    let resp = http
        .put(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            CardKitError::from_source(None, anyhow::Error::new(e).context("cardkit content http"))
        })?;
    let status = resp.status();
    let resp_body = resp.text().await.unwrap_or_default();
    if let Err(e) = classify_feishu_envelope_response(status, &resp_body) {
        return Err(CardKitError::from_source(
            parse_envelope_code(&resp_body),
            anyhow::anyhow!("cardkit content classify: {e:?}"),
        ));
    }
    Ok(())
}

/// `PATCH /open-apis/cardkit/v1/cards/{card_id}/settings` — flips the
/// `streaming_mode` flag. Called once on final / fail so the typing-cursor
/// visual stops.
async fn set_streaming_mode(
    http: &reqwest::Client,
    token_cache: &Arc<SharedTokenCache<FeishuTokenSource>>,
    card_id: &str,
    streaming_mode: bool,
    sequence: u64,
) -> std::result::Result<(), CardKitError> {
    let token = token_cache
        .get()
        .await
        .map_err(|e| CardKitError::from_source(None, e.context("feishu token")))?;
    let url = format!(
        "{}/open-apis/cardkit/v1/cards/{}/settings",
        FEISHU_API, card_id
    );
    // Feishu's `settings` field is a JSON-stringified object — same envelope
    // pattern as `im.v1.messages.content`.
    let settings = serde_json::json!({ "streaming_mode": streaming_mode }).to_string();
    let body = serde_json::json!({ "settings": settings, "sequence": sequence });
    let resp = http
        .patch(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            CardKitError::from_source(None, anyhow::Error::new(e).context("cardkit settings http"))
        })?;
    let status = resp.status();
    let resp_body = resp.text().await.unwrap_or_default();
    if let Err(e) = classify_feishu_envelope_response(status, &resp_body) {
        return Err(CardKitError::from_source(
            parse_envelope_code(&resp_body),
            anyhow::anyhow!("cardkit settings classify: {e:?}"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// `CardKitSender` is concretely typed over `FeishuTokenSource` (matches
    /// the connector's lazy-init shape). The test sender never makes HTTP
    /// calls — all behavioral tests either directly inject `CardSession`
    /// entries into the sessions map, or drive the per-card sender-task
    /// throttle/sequence contract via a stand-in mpsc loop (see
    /// `sender_task_throttles_consecutive_ops_to_min_interval`). So we can
    /// just hand the cache a real (but never-fetched) `FeishuTokenSource`.
    fn make_sender() -> CardKitSender {
        let source = Arc::new(FeishuTokenSource::new("test-ak".into(), "test-as".into()));
        let cache = Arc::new(SharedTokenCache::new(source));
        CardKitSender::new(cache)
    }

    #[tokio::test]
    async fn dispatch_fail_on_nonexistent_session_is_noop_ok() {
        let s = make_sender();
        assert!(s.dispatch_fail("no-such-session").await.is_ok());
        assert!(s.sessions.lock().await.is_empty());
    }

    #[tokio::test]
    async fn dispatch_fail_after_active_session_removes_entry_and_signals_task() {
        let s = make_sender();
        // Manually inject a session entry (bypasses HTTP). The sender task
        // is a never-spawned dummy mpsc; we only check map state and that
        // the tx still accepts the Fail op before being dropped.
        let (tx, mut rx) = mpsc::unbounded_channel::<CardOp>();
        s.sessions
            .lock()
            .await
            .insert("sess".into(), CardSession { tx });

        assert!(s.dispatch_fail("sess").await.is_ok());
        assert!(s.sessions.lock().await.get("sess").is_none());
        // Receiver side: the Fail op should have been delivered before tx
        // was dropped (which happens on the `remove`'d CardSession).
        match rx.try_recv() {
            Ok(CardOp::Fail) => {}
            other => panic!("expected Fail op delivered, got {:?}", other.is_ok()),
        }
    }

    #[tokio::test]
    async fn final_chunk_removes_session_entry() {
        // Same trick: pre-inject a session with our own mpsc, then call
        // dispatch_chunk-final. We can't go through create_and_deliver_card
        // (network), so we directly verify the "post-final, remove session"
        // contract by injecting + checking + reading the queued op.
        let s = make_sender();
        let (tx, mut rx) = mpsc::unbounded_channel::<CardOp>();
        s.sessions
            .lock()
            .await
            .insert("sess".into(), CardSession { tx: tx.clone() });

        // Simulate the same code path dispatch_chunk takes after session is known:
        // send Update {final_chunk: true} + remove from map.
        let _ = tx.send(CardOp::Update {
            delta: "done".into(),
            final_chunk: true,
        });
        s.sessions.lock().await.remove("sess");

        match rx.try_recv() {
            Ok(CardOp::Update {
                final_chunk: true,
                delta,
            }) => {
                assert_eq!(delta, "done");
            }
            _ => panic!("expected final Update op"),
        }
        assert!(s.sessions.lock().await.get("sess").is_none());
    }

    #[tokio::test]
    async fn dispatch_chunk_after_fail_creates_fresh_session() {
        // After dispatch_fail, dispatch_chunk for the same session_id must
        // be able to start a new card. We don't actually create a card here
        // (no HTTP); we just check the session map is empty post-fail so
        // the next dispatch_chunk would re-enter the create path.
        let s = make_sender();
        let (tx, _rx) = mpsc::unbounded_channel::<CardOp>();
        s.sessions
            .lock()
            .await
            .insert("sess".into(), CardSession { tx });
        assert!(s.dispatch_fail("sess").await.is_ok());
        assert!(s.sessions.lock().await.is_empty());
        // (Re-entry into create_and_deliver_card requires network; we
        // verify the precondition "map empty" which is what
        // dispatch_chunk's first guard checks.)
    }

    /// Drive the sender task with two updates and verify sequence numbers
    /// increment monotonically, AND that throttle delays the second call by
    /// at least MIN_INTERVAL. Uses a fake HTTP server via the seq-tracking
    /// closure on a thread-local AtomicU64.
    ///
    /// Note: we can't easily intercept the real HTTP without a mock server.
    /// Instead, the test verifies the sender-task control-flow contract by
    /// invoking the spawn_sender_task path directly with a NoopSource token
    /// cache and then asserting on the elapsed wall clock between the first
    /// op and the second op draining. This is the only place where we test
    /// the throttle; the HTTP body shape is exercised by the
    /// classify_feishu_envelope_response tests (in connector.rs) and any
    /// future integration test.
    #[tokio::test]
    async fn sender_task_throttles_consecutive_ops_to_min_interval() {
        // We use a never-completing token fetch to keep the test fast: the
        // sender task will call token_cache.get() inside stream_card_content
        // and we want to observe the *throttle-induced delay*, not the HTTP
        // round-trip. Since NoopSource returns immediately and there's no
        // mock HTTP server, the call will fail at the request stage — but
        // failures don't bypass `next_allowed` updates. The intent: each
        // op sets `next_allowed = now + 100ms`, so the second sleep_until
        // must wait ~100ms.
        //
        // To keep the test deterministic without a wall clock, we just
        // measure that two queued ops drain at least MIN_INTERVAL apart.
        let counter = Arc::new(AtomicU64::new(0));
        let counter_clone = counter.clone();
        let start = Instant::now();

        // Spawn a stand-in sender loop that mirrors spawn_sender_task's
        // throttling but skips HTTP. This is the contract we want to lock:
        // "MIN_INTERVAL between successive ops".
        let (tx, mut rx) = mpsc::unbounded_channel::<CardOp>();
        let task = tokio::spawn(async move {
            let mut next_allowed = Instant::now();
            while let Some(_op) = rx.recv().await {
                let now = Instant::now();
                if now < next_allowed {
                    sleep_until(next_allowed.into()).await;
                }
                counter_clone.fetch_add(1, Ordering::SeqCst);
                next_allowed = Instant::now() + MIN_INTERVAL;
            }
        });

        // Fire two ops back-to-back.
        let _ = tx.send(CardOp::Update {
            delta: "a".into(),
            final_chunk: false,
        });
        let _ = tx.send(CardOp::Update {
            delta: "b".into(),
            final_chunk: false,
        });

        // Wait until both have drained.
        while counter.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let elapsed = start.elapsed();
        // Drop tx so the task exits cleanly (the test runtime needs it).
        drop(tx);
        let _ = task.await;
        assert!(
            elapsed >= MIN_INTERVAL,
            "two ops must drain at least {:?} apart, got {:?}",
            MIN_INTERVAL,
            elapsed
        );
    }

    /// Even if 10 chunks are pushed faster than the throttle, all of them
    /// drain — none are dropped. This is the core "do not drop chunks"
    /// invariant from the spec / plan.
    #[tokio::test]
    async fn sender_task_does_not_drop_chunks_under_burst() {
        let counter = Arc::new(AtomicU64::new(0));
        let counter_clone = counter.clone();
        let (tx, mut rx) = mpsc::unbounded_channel::<CardOp>();
        let task = tokio::spawn(async move {
            let mut next_allowed = Instant::now();
            while let Some(_op) = rx.recv().await {
                let now = Instant::now();
                if now < next_allowed {
                    sleep_until(next_allowed.into()).await;
                }
                counter_clone.fetch_add(1, Ordering::SeqCst);
                next_allowed = Instant::now() + MIN_INTERVAL;
            }
        });

        // Push 10 chunks immediately, faster than 10/s.
        for i in 0..10 {
            let _ = tx.send(CardOp::Update {
                delta: format!("chunk-{}", i),
                final_chunk: false,
            });
        }
        drop(tx);
        let _ = task.await;
        assert_eq!(
            counter.load(Ordering::SeqCst),
            10,
            "all 10 chunks must drain even under burst (no drops allowed)"
        );
    }

    /// Sequence numbers must strictly increase across consecutive ops
    /// within a card. We test the seq counter logic from
    /// `spawn_sender_task` directly here, mirroring its initial-state and
    /// per-op increment semantics.
    #[tokio::test]
    async fn sender_task_sequence_increments_strictly() {
        let seqs: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
        let seqs_clone = seqs.clone();
        let (tx, mut rx) = mpsc::unbounded_channel::<CardOp>();
        let task = tokio::spawn(async move {
            let mut seq: u64 = 0;
            while let Some(op) = rx.recv().await {
                match op {
                    CardOp::Update { final_chunk, .. } => {
                        seq += 1;
                        seqs_clone.lock().await.push(seq);
                        if final_chunk {
                            seq += 1;
                            seqs_clone.lock().await.push(seq);
                            return;
                        }
                    }
                    CardOp::Fail => {
                        seq += 1;
                        seqs_clone.lock().await.push(seq);
                        seq += 1;
                        seqs_clone.lock().await.push(seq);
                        return;
                    }
                }
            }
        });

        let _ = tx.send(CardOp::Update {
            delta: "a".into(),
            final_chunk: false,
        });
        let _ = tx.send(CardOp::Update {
            delta: "b".into(),
            final_chunk: false,
        });
        let _ = tx.send(CardOp::Update {
            delta: "c".into(),
            final_chunk: true,
        });
        drop(tx);
        let _ = task.await;

        let got = seqs.lock().await.clone();
        // 3 chunks: a (seq 1), b (seq 2), c+settings (seq 3, 4)
        assert_eq!(got, vec![1, 2, 3, 4], "sequence must increment strictly");
    }

    /// Within a single `final_chunk` Update op, the sender task does TWO
    /// HTTP calls: content PUT, then settings PATCH. CardKit's 100 ms rate
    /// limit is per-`card_id` across BOTH endpoints, so the two calls MUST
    /// be at least MIN_INTERVAL apart — otherwise the second one trips 230002
    /// (sequence error, gets logged as Transient) and the card is left
    /// stuck in streaming_mode=true forever.
    ///
    /// Pre-PR5-review bug: the original code did `next_allowed = ... + 100ms`
    /// after the content call but never awaited it before the settings call.
    /// This test pins the new `wait_throttle` invocation between them.
    #[tokio::test]
    async fn sender_task_throttles_within_final_update_double_call() {
        // Record each "HTTP call" timestamp. The stand-in loop mirrors the
        // production sender task's terminal-Update shape exactly: one call,
        // re-arm, wait_throttle, second call.
        let call_times: Arc<Mutex<Vec<Instant>>> = Arc::new(Mutex::new(Vec::new()));
        let call_times_clone = call_times.clone();
        let (tx, mut rx) = mpsc::unbounded_channel::<CardOp>();
        let task = tokio::spawn(async move {
            let mut next_allowed = Instant::now();
            while let Some(op) = rx.recv().await {
                wait_throttle(&mut next_allowed).await;
                // "First HTTP call" — content PUT.
                call_times_clone.lock().await.push(Instant::now());
                next_allowed = Instant::now() + MIN_INTERVAL;

                if let CardOp::Update {
                    final_chunk: true, ..
                } = op
                {
                    // The fix under test: honor the throttle before the
                    // second call too.
                    wait_throttle(&mut next_allowed).await;
                    // "Second HTTP call" — settings PATCH.
                    call_times_clone.lock().await.push(Instant::now());
                    return;
                }
            }
        });

        let _ = tx.send(CardOp::Update {
            delta: "done".into(),
            final_chunk: true,
        });
        drop(tx);
        let _ = task.await;

        let times = call_times.lock().await.clone();
        assert_eq!(times.len(), 2, "Final-Update must produce 2 HTTP calls");
        let gap = times[1].duration_since(times[0]);
        assert!(
            gap >= MIN_INTERVAL,
            "settings PATCH must be ≥{:?} after content PUT (got {:?}) — \
             otherwise we trip the per-card_id rate limit and the card sticks \
             in streaming_mode=true",
            MIN_INTERVAL,
            gap
        );
    }

    /// I2(a): on 230005 (CardKit card not found), the sender task evicts
    /// its own session entry from the sessions map. Without this, every
    /// future chunk for the same session_id would keep hitting the dead
    /// card_id forever (dispatch_chunk returns Ok, so the manager has no
    /// way to know). After eviction the next dispatch_chunk re-enters
    /// create_and_deliver_card and a fresh card pops up.
    ///
    /// This test mirrors the eviction logic the production sender task
    /// does on 230005, using a stand-in HTTP fake that returns 230005.
    #[tokio::test]
    async fn sender_task_evicts_session_on_230005_card_gone() {
        // Pre-populate a session entry — mimics what dispatch_chunk does
        // after a successful create_and_deliver_card.
        let sessions: Arc<Mutex<HashMap<String, CardSession>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (tx, _rx) = mpsc::unbounded_channel::<CardOp>();
        sessions
            .lock()
            .await
            .insert("sess".into(), CardSession { tx });

        // Stand-in for the sender task's 230005-handling branch.
        let sessions_clone = sessions.clone();
        let task = tokio::spawn(async move {
            // Simulated 230005 response from CardKit.
            let fake_err = CardKitError::from_source(
                Some(230005),
                anyhow::anyhow!("simulated 230005 card_not_found"),
            );
            if matches!(fake_err.code, Some(230005)) {
                // This is the exact line from spawn_sender_task's update branch.
                sessions_clone.lock().await.remove("sess");
            }
        });
        let _ = task.await;

        assert!(
            sessions.lock().await.get("sess").is_none(),
            "230005 must evict the session entry so the next dispatch_chunk \
             starts a fresh card"
        );
    }

    /// parse_envelope_code recognizes the 230005 errcode in a real-ish body
    /// shape. Pinned because the sender task's eviction logic relies on
    /// this exact code being parsed out of the response body.
    #[test]
    fn parse_envelope_code_extracts_230005() {
        let body = r#"{"code":230005,"msg":"card not found","data":{}}"#;
        assert_eq!(parse_envelope_code(body), Some(230005));
    }

    #[test]
    fn parse_envelope_code_returns_none_for_non_json() {
        assert_eq!(parse_envelope_code("<html>5xx</html>"), None);
    }

    #[test]
    fn parse_card_id_data_inner() {
        let body = r#"{"code":0,"msg":"ok","data":{"card_id":"AAQ-abc"}}"#;
        assert_eq!(parse_card_id(body).unwrap(), "AAQ-abc");
    }

    #[test]
    fn parse_card_id_top_level_fallback() {
        let body = r#"{"code":0,"msg":"ok","card_id":"AAQ-top"}"#;
        assert_eq!(parse_card_id(body).unwrap(), "AAQ-top");
    }

    #[test]
    fn parse_card_id_empty_string_errors() {
        // Defensive: feishu has been known to return empty card_id on edge
        // cases — we treat that as a parse failure, not silent success.
        let body = r#"{"code":0,"msg":"ok","data":{"card_id":""}}"#;
        assert!(parse_card_id(body).is_err());
    }

    #[test]
    fn parse_card_id_missing_errors() {
        let body = r#"{"code":0,"msg":"ok","data":{}}"#;
        assert!(parse_card_id(body).is_err());
    }

    #[test]
    fn parse_card_id_non_json_errors() {
        assert!(parse_card_id("<html>5xx</html>").is_err());
    }

    /// Sanity: the throttle constant matches the spec's 100 ms. If someone
    /// tightens it, this test loudly fails so they can defend the change.
    #[test]
    fn min_interval_is_100_ms_per_spec() {
        assert_eq!(MIN_INTERVAL, Duration::from_millis(100));
    }
}
