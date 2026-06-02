use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use serde::Serialize;
use serde_json::{json, Value};

static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Default)]
struct GatewayRouteSummary {
    logical_model: Option<Value>,
    route_provider: Option<Value>,
    route_api: Option<Value>,
    route_model: Option<Value>,
    endpoint_id: Option<Value>,
}

#[derive(Clone, Debug)]
struct GatewayLogContext {
    provider: String,
    url: String,
    conversation_id: Option<Value>,
    run_id: Option<Value>,
    trace_id: Option<Value>,
    gateway_request_id: Option<Value>,
    response_id: Option<Value>,
    route: GatewayRouteSummary,
    created_at_ms: i64,
}

static GATEWAY_LOG_CONTEXTS: LazyLock<Mutex<HashMap<String, GatewayLogContext>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn next_request_id() -> String {
    let seq = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("gate-{}-{}", chrono::Utc::now().timestamp_millis(), seq)
}

pub fn record_request<T: Serialize>(request_id: &str, provider: &str, url: &str, body: &T) {
    let body = serde_json::to_value(body).unwrap_or_else(|err| {
        json!({
            "_serialization_error": err.to_string()
        })
    });
    remember_request_context(request_id, provider, url, &body);
    record_event(
        "gateway.request",
        request_id,
        json!({
            "provider": provider,
            "url": url,
            "body": body,
        }),
    );
}

pub fn record_response_status(request_id: &str, status: u16, gateway_request_id: Option<&str>) {
    remember_gateway_request_id(request_id, gateway_request_id);
    record_event(
        "gateway.response_status",
        request_id,
        json!({
            "status": status,
            "gateway_request_id": gateway_request_id,
        }),
    );
}

pub fn record_response_body(request_id: &str, status: u16, body: &str) {
    record_event(
        "gateway.response_body",
        request_id,
        json!({
            "status": status,
            "body": body,
        }),
    );
}

pub fn record_route(request_id: &str, response_id: Option<&str>, route: &Value) {
    remember_route_context(request_id, response_id, route);
    record_event(
        "gateway.route",
        request_id,
        json!({
            "response_id": response_id,
            "logical_model": route.get("logical_model").cloned().unwrap_or(Value::Null),
            "provider": route.get("provider").cloned().unwrap_or(Value::Null),
            "api": route.get("api").cloned().unwrap_or(Value::Null),
            "model": route.get("model").cloned().unwrap_or(Value::Null),
            "endpoint_id": route.get("endpoint_id").cloned().unwrap_or(Value::Null),
            "route": route,
        }),
    );
}

pub fn record_response_chunk(request_id: &str, bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    record_event(
        "gateway.response_chunk",
        request_id,
        json!({
            "bytes": bytes.len(),
            "events": sse_event_names_from_chunk(&text),
            "content_delta_count": text.matches("event: content.delta").count(),
            "tool_call_count": text.matches("event: tool_call").count(),
            "keepalive_count": text.matches("event: keepalive").count(),
        }),
    );
}

pub fn record_stream_error(request_id: &str, error: &str) {
    record_event(
        "gateway.stream_error",
        request_id,
        json!({
            "error": error,
        }),
    );
}

pub fn record_stream_started(request_id: &str) {
    record_event("gateway.stream_started", request_id, json!({}));
}

pub fn record_first_event(request_id: &str, event_name: Option<&str>) {
    record_event(
        "gateway.first_event",
        request_id,
        json!({
            "event_name": event_name,
        }),
    );
}

pub fn record_response_completed(request_id: &str, stop_reason: Option<&str>) {
    record_event(
        "gateway.response_completed",
        request_id,
        json!({
            "lifecycle_event_name": "response.completed",
            "stop_reason": stop_reason,
        }),
    );
}

pub fn record_stream_closed(request_id: &str, reason: &str, error: Option<&str>) {
    let path = gate_log_path();
    let row = stream_closed_event_row(request_id, reason, error);
    if let Err(err) = append_event_to_path(&path, row) {
        log::warn!("[gate-log] failed to append {}: {}", path.display(), err);
    }
}

pub fn record_stream_end(request_id: &str) {
    record_event("gateway.stream_end", request_id, json!({}));
}

fn record_event(event: &str, request_id: &str, payload: Value) {
    let path = gate_log_path();
    let row = event_row(event, request_id, payload);
    if let Err(err) = append_event_to_path(&path, row) {
        log::warn!("[gate-log] failed to append {}: {}", path.display(), err);
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn remember_request_context(request_id: &str, provider: &str, url: &str, body: &Value) {
    let mut contexts = GATEWAY_LOG_CONTEXTS
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    contexts.retain(|_, ctx| now_ms() - ctx.created_at_ms < 30 * 60 * 1000);
    contexts.insert(
        request_id.to_string(),
        GatewayLogContext {
            provider: provider.to_string(),
            url: url.to_string(),
            conversation_id: body.get("conversation_id").cloned(),
            run_id: body.get("run_id").cloned(),
            trace_id: body.get("trace_id").cloned(),
            gateway_request_id: None,
            response_id: None,
            route: GatewayRouteSummary::default(),
            created_at_ms: now_ms(),
        },
    );
}

fn remember_gateway_request_id(request_id: &str, gateway_request_id: Option<&str>) {
    if let Some(id) = gateway_request_id {
        let mut contexts = GATEWAY_LOG_CONTEXTS
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(ctx) = contexts.get_mut(request_id) {
            ctx.gateway_request_id = Some(Value::String(id.to_string()));
        }
    }
}

fn remember_route_context(request_id: &str, response_id: Option<&str>, route: &Value) {
    let mut contexts = GATEWAY_LOG_CONTEXTS
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if let Some(ctx) = contexts.get_mut(request_id) {
        ctx.response_id = response_id.map(|id| Value::String(id.to_string()));
        ctx.route.logical_model = route.get("logical_model").cloned();
        ctx.route.route_provider = route.get("provider").cloned();
        ctx.route.route_api = route.get("api").cloned();
        ctx.route.route_model = route.get("model").cloned();
        ctx.route.endpoint_id = route.get("endpoint_id").cloned();
    }
}

#[allow(dead_code)]
fn forget_request_context(request_id: &str) {
    GATEWAY_LOG_CONTEXTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(request_id);
}

fn gate_log_path() -> PathBuf {
    crate::storage::AiJiaHome::from_home()
        .root()
        .join("logs")
        .join("gate.log")
}

fn event_row(event: &str, request_id: &str, mut payload: Value) -> Value {
    let mut base = serde_json::Map::new();
    base.insert(
        "ts".to_string(),
        Value::String(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    );
    base.insert("event".to_string(), Value::String(event.to_string()));
    base.insert(
        "request_id".to_string(),
        Value::String(request_id.to_string()),
    );
    if let Some(ctx) = GATEWAY_LOG_CONTEXTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(request_id)
        .cloned()
    {
        base.insert(
            "desktop_provider".to_string(),
            Value::String(ctx.provider.to_string()),
        );
        base.insert("url".to_string(), Value::String(ctx.url.to_string()));
        if let Some(value) = ctx.conversation_id {
            base.insert("conversation_id".to_string(), value);
        }
        if let Some(value) = ctx.run_id {
            base.insert("run_id".to_string(), value);
        }
        if let Some(value) = ctx.trace_id {
            base.insert("trace_id".to_string(), value);
        }
        if let Some(value) = ctx.gateway_request_id {
            base.insert("gateway_request_id".to_string(), value);
        }
        if let Some(value) = ctx.response_id {
            base.insert("response_id".to_string(), value);
        }
        if let Some(value) = ctx.route.logical_model {
            base.insert("logical_model".to_string(), value);
        }
        if let Some(value) = ctx.route.route_provider {
            base.insert("route_provider".to_string(), value);
        }
        if let Some(value) = ctx.route.route_api {
            base.insert("route_api".to_string(), value);
        }
        if let Some(value) = ctx.route.route_model {
            base.insert("route_model".to_string(), value);
        }
        if let Some(value) = ctx.route.endpoint_id {
            base.insert("endpoint_id".to_string(), value);
        }
    }
    if let Some(payload_obj) = payload.as_object_mut() {
        for (key, value) in std::mem::take(payload_obj) {
            base.insert(key, value);
        }
    } else {
        base.insert("payload".to_string(), payload);
    }
    Value::Object(base)
}

fn stream_closed_event_row(request_id: &str, reason: &str, error: Option<&str>) -> Value {
    let row = event_row(
        "gateway.stream_closed",
        request_id,
        json!({
            "reason": reason,
            "error": error,
        }),
    );
    forget_request_context(request_id);
    row
}

fn append_event_to_path(path: &Path, row: Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(
        file,
        "{}",
        serde_json::to_string(&row).unwrap_or_else(|_| "{}".to_string())
    )
}

fn sse_event_names_from_chunk(chunk: &str) -> Vec<String> {
    let mut events = Vec::new();
    for line in chunk.lines() {
        if let Some(event) = line.strip_prefix("event: ") {
            if !events.iter().any(|seen| seen == event) {
                events.push(event.to_string());
            }
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    #[test]
    fn appends_full_gateway_request_and_response_chunks_as_jsonl() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("gate.log");
        let request_id = "gate-test-1";

        super::append_event_to_path(
            &path,
            super::event_row(
                "gateway.request",
                request_id,
                json!({
                    "provider": "aijia-v2",
                    "url": "https://ai-tenant.renlijia.com/aijia/v2/ai/responses",
                    "body": {
                        "messages": [{"role": "user", "content": "完整请求"}],
                        "tools": [{"name": "lookup", "parameters": {"type": "object"}}]
                    }
                }),
            ),
        )
        .expect("append request");
        super::append_event_to_path(
            &path,
            super::event_row(
                "gateway.route",
                request_id,
                json!({
                    "response_id": "lreq_1",
                    "logical_model": "default-chat",
                    "provider": "anthropic",
                    "api": "anthropic-messages",
                    "model": "claude-sonnet-4-5",
                    "endpoint_id": 1,
                    "route": {
                        "logical_model": "default-chat",
                        "provider": "anthropic",
                        "api": "anthropic-messages",
                        "model": "claude-sonnet-4-5",
                        "endpoint_id": 1
                    }
                }),
            ),
        )
        .expect("append route");
        super::append_event_to_path(
            &path,
            super::event_row(
                "gateway.response_chunk",
                request_id,
                json!({
                    "bytes": 37,
                    "events": ["content.delta"],
                    "content_delta_count": 1,
                    "tool_call_count": 0,
                    "keepalive_count": 0
                }),
            ),
        )
        .expect("append chunk");

        let raw = std::fs::read_to_string(path).expect("read gate.log");
        let rows: Vec<Value> = raw
            .lines()
            .map(|line| serde_json::from_str(line).expect("jsonl row"))
            .collect();

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["event"], "gateway.request");
        assert_eq!(rows[0]["request_id"], request_id);
        assert_eq!(rows[0]["provider"], "aijia-v2");
        assert_eq!(rows[0]["body"]["messages"][0]["content"], "完整请求");
        assert_eq!(rows[1]["event"], "gateway.route");
        assert_eq!(rows[1]["provider"], "anthropic");
        assert_eq!(rows[1]["model"], "claude-sonnet-4-5");
        assert_eq!(rows[1]["endpoint_id"], 1);
        assert_eq!(rows[2]["event"], "gateway.response_chunk");
        assert!(rows[2].get("chunk_utf8_lossy").is_none());
        assert_eq!(rows[2]["events"], json!(["content.delta"]));
        assert_eq!(rows[2]["content_delta_count"], 1);
        assert!(rows[0]["ts"].as_str().unwrap().ends_with('Z'));
    }

    #[test]
    fn response_status_row_is_enriched_from_request_context() {
        let request_id = "gate-test-enrich-1";
        super::remember_request_context(
            request_id,
            "aijia-v2",
            "https://ai-tenant.renlijia.com/aijia/v2/ai/responses",
            &json!({
                "conversation_id": "conv",
                "run_id": "run",
                "trace_id": "trace"
            }),
        );

        let row = super::event_row(
            "gateway.response_status",
            request_id,
            json!({
                "status": 200,
                "gateway_request_id": "lreq_1"
            }),
        );

        assert_eq!(row["conversation_id"], "conv");
        assert_eq!(row["run_id"], "run");
        assert_eq!(row["trace_id"], "trace");
        assert_eq!(row["gateway_request_id"], "lreq_1");
    }

    #[test]
    fn route_context_enriches_later_rows() {
        let request_id = "gate-test-enrich-2";
        super::remember_request_context(
            request_id,
            "aijia-v2",
            "https://ai-tenant.renlijia.com/aijia/v2/ai/responses",
            &json!({
                "conversation_id": "conv",
                "run_id": "run",
                "trace_id": "trace"
            }),
        );
        super::remember_gateway_request_id(request_id, Some("lreq_2"));
        super::remember_route_context(
            request_id,
            Some("lreq_2"),
            &json!({
                "logical_model": "default-chat",
                "provider": "deepseek",
                "api": "anthropic-messages",
                "model": "deepseek-v4-pro",
                "endpoint_id": 2
            }),
        );

        let row = super::event_row("gateway.response_chunk", request_id, json!({"bytes": 10}));

        assert_eq!(row["gateway_request_id"], "lreq_2");
        assert_eq!(row["response_id"], "lreq_2");
        assert_eq!(row["logical_model"], "default-chat");
        assert_eq!(row["route_provider"], "deepseek");
        assert_eq!(row["route_api"], "anthropic-messages");
        assert_eq!(row["route_model"], "deepseek-v4-pro");
        assert_eq!(row["endpoint_id"], 2);
    }

    #[test]
    fn stream_closed_forgets_request_context() {
        let request_id = "gate-test-stream-closed";
        super::remember_request_context(
            request_id,
            "aijia-v2",
            "https://ai-tenant.renlijia.com/aijia/v2/ai/responses",
            &json!({
                "conversation_id": "conv",
                "run_id": "run",
                "trace_id": "trace"
            }),
        );

        let close_row = super::stream_closed_event_row(request_id, "eof", None);
        let later_row = super::event_row("gateway.stream_end", request_id, json!({}));

        assert_eq!(close_row["conversation_id"], "conv");
        assert_eq!(close_row["reason"], "eof");
        assert!(later_row.get("conversation_id").is_none());
        assert!(later_row.get("run_id").is_none());
        assert!(later_row.get("trace_id").is_none());
    }

    #[test]
    fn response_completed_row_contains_lifecycle_event_name() {
        let row = super::event_row(
            "gateway.response_completed",
            "gate-test-response-completed",
            json!({
                "lifecycle_event_name": "response.completed",
                "stop_reason": "end_turn"
            }),
        );

        assert_eq!(row["event"], "gateway.response_completed");
        assert_eq!(row["lifecycle_event_name"], "response.completed");
        assert_eq!(row["stop_reason"], "end_turn");
    }
}
