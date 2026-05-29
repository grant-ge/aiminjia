use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde_json::{json, Value};

static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

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
    if let Some(payload_obj) = payload.as_object_mut() {
        for (key, value) in std::mem::take(payload_obj) {
            base.insert(key, value);
        }
    } else {
        base.insert("payload".to_string(), payload);
    }
    Value::Object(base)
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
}
