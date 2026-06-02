# V2 Gateway Observability and Error Semantics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve tool error status into v2 canonical requests and make desktop v2 gateway logs self-correlating with explicit stream lifecycle events.

**Architecture:** Keep the request schema backward compatible and extend only optional fields. Runtime truth flows from `RuntimeToolCallOutcome`/stored tool messages into `ChatMessage`, then into v2 `CanonicalMessage`; gateway observability is centralized in `llm/gate_log.rs` and consumed by `aijia_gateway_v2.rs`.

**Tech Stack:** Rust, Tauri backend, serde JSON, Tokio streams, cargo tests.

---

## File Structure

- Modify `src-tauri/src/llm/streaming.rs`: add `ChatMessage.is_error`, default serialization behavior, and status-aware tool-result constructor.
- Modify `src-tauri/src/runtime/chat/history.rs`: preserve stored tool `content.isError` when rebuilding `ChatMessage` history.
- Modify `src-tauri/src/runtime/chat/tool_result_collector.rs`: add a regression test proving `isError` survives the collector JSON shape.
- Modify `src-tauri/src/runtime/agent/worker_runtime.rs`: use status-aware tool-result construction for subagent and teammate tool loops.
- Modify `src-tauri/src/llm/providers/aijia_gateway_v2.rs`: propagate `ChatMessage.is_error`, populate client metadata, and emit lifecycle events during SSE parsing.
- Modify `src-tauri/src/llm/canonical.rs`: extend `ClientInfo` with optional metadata fields and update tests.
- Modify `src-tauri/src/llm/gate_log.rs`: add request context enrichment, lifecycle event helpers, and stream-close logging.

## Task 1: Add `ChatMessage.is_error`

**Files:**
- Modify: `src-tauri/src/llm/streaming.rs`

- [ ] **Step 1: Write failing tests for default and explicit error status**

Add this test module at the bottom of `src-tauri/src/llm/streaming.rs`:

```rust
#[cfg(test)]
mod chat_message_error_status_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_is_error_defaults_false() {
        let message: ChatMessage = serde_json::from_value(json!({
            "role": "tool",
            "content": "ok",
            "toolCallId": "call_1",
            "name": "Bash"
        }))
        .expect("deserialize chat message");

        assert!(!message.is_error);
    }

    #[test]
    fn tool_result_with_status_serializes_camel_case_is_error() {
        let message =
            ChatMessage::tool_result_with_status("call_1", "Bash", "failed".to_string(), true);
        let value = serde_json::to_value(message).expect("serialize chat message");

        assert_eq!(value["role"], "tool");
        assert_eq!(value["toolCallId"], "call_1");
        assert_eq!(value["name"], "Bash");
        assert_eq!(value["isError"], true);
    }

    #[test]
    fn success_tool_result_omits_is_error() {
        let message = ChatMessage::tool_result("call_1", "Bash", "ok".to_string());
        let value = serde_json::to_value(message).expect("serialize chat message");

        assert!(value.get("isError").is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::streaming::chat_message_error_status_tests --lib
```

Expected: compile failure because `ChatMessage::is_error` and `tool_result_with_status` do not exist.

- [ ] **Step 3: Implement `is_error` and constructor**

In `src-tauri/src/llm/streaming.rs`, add this helper near the top of the file after imports:

```rust
fn is_false(value: &bool) -> bool {
    !*value
}
```

Add this field to `ChatMessage` after `name`:

```rust
    /// Whether a tool result represents a tool-level error.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_error: bool,
```

Update every `ChatMessage` constructor in `impl ChatMessage` to initialize `is_error: false` for `text` and `assistant_with_tool_calls`.

Replace the existing `tool_result` body with a call to the new helper and add the helper:

```rust
    pub fn tool_result(tool_call_id: &str, tool_name: &str, content: String) -> Self {
        Self::tool_result_with_status(tool_call_id, tool_name, content, false)
    }

    pub fn tool_result_with_status(
        tool_call_id: &str,
        tool_name: &str,
        content: String,
        is_error: bool,
    ) -> Self {
        Self {
            role: "tool".to_string(),
            content,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.to_string()),
            name: Some(tool_name.to_string()),
            is_error,
            thinking: None,
            thinking_blocks: None,
            anthropic_multimodal_turn: None,
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::streaming::chat_message_error_status_tests --lib
```

Expected: all three tests pass.

- [ ] **Step 5: Commit**

```bash
git -C /Users/gezhigang/work-codeup/aijia/code add src-tauri/src/llm/streaming.rs
git -C /Users/gezhigang/work-codeup/aijia/code commit -m "fix: preserve chat tool error status"
```

## Task 2: Propagate Error Status Into V2 Canonical Requests

**Files:**
- Modify: `src-tauri/src/llm/providers/aijia_gateway_v2.rs`

- [ ] **Step 1: Write failing canonical propagation test**

Add this test to the existing `tests` module in `src-tauri/src/llm/providers/aijia_gateway_v2.rs`:

```rust
    #[test]
    fn build_request_preserves_tool_result_error_status() {
        let req = LlmRequest {
            messages: vec![ChatMessage::tool_result_with_status(
                "call_1",
                "Bash",
                "permission denied".to_string(),
                true,
            )],
            tools: vec![],
            max_tokens: 1000,
            temperature: 0.7,
            stream: true,
            thinking_config: None,
            anthropic_multimodal_turn: None,
            system_segments: None,
            conversation_id: Some("conv".to_string()),
            trace_id: Some("trace".to_string()),
            run_id: Some("run".to_string()),
        };

        let canonical = build_aijia_request(req);
        let tool_result = &canonical.context.messages[0];

        assert_eq!(tool_result.role, "tool_result");
        assert_eq!(tool_result.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(tool_result.tool_name.as_deref(), Some("Bash"));
        assert!(tool_result.is_error);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::providers::aijia_gateway_v2::tests::build_request_preserves_tool_result_error_status --lib
```

Expected: assertion fails because `to_canonical_message` still hardcodes `is_error=false`.

- [ ] **Step 3: Implement canonical propagation**

In `to_canonical_message` in `src-tauri/src/llm/providers/aijia_gateway_v2.rs`, replace:

```rust
        is_error: false,
```

with:

```rust
        is_error: message.is_error,
```

- [ ] **Step 4: Run the v2 provider tests**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::providers::aijia_gateway_v2::tests --lib
```

Expected: all v2 provider tests pass.

- [ ] **Step 5: Commit**

```bash
git -C /Users/gezhigang/work-codeup/aijia/code add src-tauri/src/llm/providers/aijia_gateway_v2.rs
git -C /Users/gezhigang/work-codeup/aijia/code commit -m "fix: propagate v2 tool result errors"
```

## Task 3: Preserve `isError` Through Main Chat History

**Files:**
- Modify: `src-tauri/src/runtime/chat/history.rs`
- Modify: `src-tauri/src/runtime/chat/tool_result_collector.rs`

- [ ] **Step 1: Write failing history rebuild test**

In `src-tauri/src/runtime/chat/history.rs`, add this test to an existing `#[cfg(test)]` module or create a new one:

```rust
#[cfg(test)]
mod tool_error_status_history_tests {
    use super::*;
    use crate::storage::file_store::types::StoredMessage;
    use serde_json::json;

    #[test]
    fn stored_tool_message_preserves_is_error() {
        let stored = StoredMessage {
            seq: None,
            rev: None,
            id: "tool-1".to_string(),
            conversation_id: "conv".to_string(),
            role: "tool".to_string(),
            content: json!({
                "content": "permission denied",
                "toolCallId": "call_1",
                "name": "Bash",
                "isError": true
            }),
            created_at: "2026-06-02T00:00:00Z".to_string(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            run_id: Some("run".to_string()),
            schema_version: None,
            sequence: None,
            error: None,
        };

        let message = stored_to_chat(&stored, &HistoryConfig::default());

        assert_eq!(message.role, "tool");
        assert_eq!(message.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(message.name.as_deref(), Some("Bash"));
        assert!(message.is_error);
    }
}
```

- [ ] **Step 2: Write failing collector shape test**

In `src-tauri/src/runtime/chat/tool_result_collector.rs`, add this test to the existing tests module:

```rust
    #[test]
    fn collected_error_tool_result_deserializes_to_chat_message_with_error_status() {
        let out = collect_results(vec![completed("call_1", "Bash", "permission denied", true)]);
        let message: crate::llm::streaming::ChatMessage =
            serde_json::from_value(out.tool_result_messages[0].clone())
                .expect("tool result json should deserialize as ChatMessage");

        assert_eq!(message.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(message.name.as_deref(), Some("Bash"));
        assert!(message.is_error);
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test runtime::chat::history::tool_error_status_history_tests --lib
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test runtime::chat::tool_result_collector::tests::collected_error_tool_result_deserializes_to_chat_message_with_error_status --lib
```

Expected: the history test fails until `stored_to_chat` reads `isError`.

- [ ] **Step 4: Implement history extraction**

In `stored_to_chat` in `src-tauri/src/runtime/chat/history.rs`, add this field to the `ChatMessage` literal:

```rust
        is_error: message
            .content
            .get("isError")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
```

Place it after `name` and before `thinking` to match the struct field order.

- [ ] **Step 5: Run targeted tests**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test runtime::chat::history::tool_error_status_history_tests --lib
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test runtime::chat::tool_result_collector::tests::collected_error_tool_result_deserializes_to_chat_message_with_error_status --lib
```

Expected: both tests pass.

- [ ] **Step 6: Commit**

```bash
git -C /Users/gezhigang/work-codeup/aijia/code add src-tauri/src/runtime/chat/history.rs src-tauri/src/runtime/chat/tool_result_collector.rs
git -C /Users/gezhigang/work-codeup/aijia/code commit -m "fix: preserve stored tool error status"
```

## Task 4: Update Subagent and Teammate Tool Result Producers

**Files:**
- Modify: `src-tauri/src/runtime/agent/worker_runtime.rs`

- [ ] **Step 1: Write failing regression tests**

Add tests near existing worker runtime tests in `src-tauri/src/runtime/agent/worker_runtime.rs`:

```rust
#[cfg(test)]
mod tool_result_status_tests {
    use super::*;
    use crate::llm::streaming::ChatMessage;

    #[test]
    fn status_aware_tool_result_marks_error() {
        let message =
            ChatMessage::tool_result_with_status("call_1", "Bash", "failed".to_string(), true);

        assert!(message.is_error);
        assert_eq!(message.role, "tool");
    }

    #[test]
    fn default_tool_result_remains_success() {
        let message = ChatMessage::tool_result("call_1", "Bash", "ok".to_string());

        assert!(!message.is_error);
    }
}
```

This pair guards the helper used by subagent paths. The code changes below are verified by the full worker-runtime compile and existing tests.

- [ ] **Step 2: Run tests to verify helper availability**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test runtime::agent::worker_runtime::tool_result_status_tests --lib
```

Expected: tests pass after Task 1. If they fail to compile, Task 1 was not applied correctly.

- [ ] **Step 3: Replace status-known subagent tool result constructors**

In `src-tauri/src/runtime/agent/worker_runtime.rs`, update the synchronous subagent loop:

For blocked results around the existing `ToolRoundResult::Blocked` branch, replace:

```rust
                            request.messages.push(ChatMessage::tool_result(
                                &blocked.tool_call_id,
                                &blocked.tool_name,
                                blocked.reason,
                            ));
```

with:

```rust
                            request.messages.push(ChatMessage::tool_result_with_status(
                                &blocked.tool_call_id,
                                &blocked.tool_name,
                                blocked.reason,
                                true,
                            ));
```

For completed results around the existing `RuntimeToolCallOutcome::Completed` branch, replace:

```rust
                            request.messages.push(ChatMessage::tool_result(
                                &tool_call_id,
                                &tool_name,
                                content_for_message,
                            ));
```

with:

```rust
                            request.messages.push(ChatMessage::tool_result_with_status(
                                &tool_call_id,
                                &tool_name,
                                content_for_message,
                                is_error,
                            ));
```

For AskRequired and InteractionRequired branches in the same loop, use `tool_result_with_status(..., true)` when the existing code treats the branch as failed, and `tool_result_with_status(..., false)` when the branch is intentionally non-error.

- [ ] **Step 4: Replace teammate idle constructor**

In the teammate idle loop around the existing `let tool_result_msg = ChatMessage::tool_result(...)`, use the `is_err` boolean already computed in the tuple:

```rust
            let tool_result_msg =
                ChatMessage::tool_result_with_status(&tcid, &tname, content_str.clone(), is_err);
```

If the local tuple variable is named differently, keep the existing name and pass that boolean.

- [ ] **Step 5: Run worker runtime tests and compile check**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test runtime::agent::worker_runtime --lib
```

Expected: worker runtime tests pass.

- [ ] **Step 6: Commit**

```bash
git -C /Users/gezhigang/work-codeup/aijia/code add src-tauri/src/runtime/agent/worker_runtime.rs
git -C /Users/gezhigang/work-codeup/aijia/code commit -m "fix: preserve agent tool error status"
```

## Task 5: Extend `ClientInfo` With Optional Metadata

**Files:**
- Modify: `src-tauri/src/llm/canonical.rs`
- Modify: `src-tauri/src/llm/providers/aijia_gateway_v2.rs`

- [ ] **Step 1: Write failing `ClientInfo` serialization tests**

In the tests module in `src-tauri/src/llm/canonical.rs`, add:

```rust
    #[test]
    fn client_info_omits_unavailable_optional_metadata() {
        let client = ClientInfo {
            name: "aijia-desktop".to_string(),
            version: "0.5.32".to_string(),
            platform: "aarch64".to_string(),
            os: None,
            arch: None,
            locale: None,
            timezone: None,
            device_id_hash: None,
            scope_key_hash: None,
        };

        let value = serde_json::to_value(client).expect("serialize client");
        assert_eq!(value["name"], "aijia-desktop");
        assert_eq!(value["platform"], "aarch64");
        assert!(value.get("os").is_none());
        assert!(value.get("scope_key_hash").is_none());
    }

    #[test]
    fn client_info_serializes_available_optional_metadata() {
        let client = ClientInfo {
            name: "aijia-desktop".to_string(),
            version: "0.5.32".to_string(),
            platform: "aarch64".to_string(),
            os: Some("macos".to_string()),
            arch: Some("aarch64".to_string()),
            locale: Some("zh-CN".to_string()),
            timezone: Some("America/New_York".to_string()),
            device_id_hash: Some("devhash".to_string()),
            scope_key_hash: Some("scopehash".to_string()),
        };

        let value = serde_json::to_value(client).expect("serialize client");
        assert_eq!(value["os"], "macos");
        assert_eq!(value["arch"], "aarch64");
        assert_eq!(value["locale"], "zh-CN");
        assert_eq!(value["timezone"], "America/New_York");
        assert_eq!(value["device_id_hash"], "devhash");
        assert_eq!(value["scope_key_hash"], "scopehash");
    }
```

- [ ] **Step 2: Write failing v2 provider metadata test**

In `src-tauri/src/llm/providers/aijia_gateway_v2.rs`, add this test:

```rust
    #[test]
    fn build_request_populates_basic_client_metadata() {
        let req = LlmRequest {
            messages: vec![ChatMessage::text("user", "hello")],
            tools: vec![],
            max_tokens: 1000,
            temperature: 0.7,
            stream: true,
            thinking_config: None,
            anthropic_multimodal_turn: None,
            system_segments: None,
            conversation_id: Some("conv".to_string()),
            trace_id: Some("trace".to_string()),
            run_id: Some("run".to_string()),
        };

        let canonical = build_aijia_request(req);

        assert_eq!(canonical.client.os.as_deref(), Some(std::env::consts::OS));
        assert_eq!(canonical.client.arch.as_deref(), Some(std::env::consts::ARCH));
        assert_eq!(canonical.client.platform, std::env::consts::ARCH);
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::canonical::tests::client_info --lib
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::providers::aijia_gateway_v2::tests::build_request_populates_basic_client_metadata --lib
```

Expected: compile failures because `ClientInfo` does not have optional metadata fields.

- [ ] **Step 4: Extend `ClientInfo` and request builder**

In `src-tauri/src/llm/canonical.rs`, extend `ClientInfo`:

```rust
pub struct ClientInfo {
    pub name: String,
    pub version: String,
    pub platform: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_key_hash: Option<String>,
}
```

Update existing `ClientInfo` literals in tests to include the new fields with `None`.

In `build_aijia_request_for_route` in `src-tauri/src/llm/providers/aijia_gateway_v2.rs`, set:

```rust
        client: ClientInfo {
            name: "aijia-desktop".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            platform: std::env::consts::ARCH.to_string(),
            os: Some(std::env::consts::OS.to_string()),
            arch: Some(std::env::consts::ARCH.to_string()),
            locale: None,
            timezone: None,
            device_id_hash: None,
            scope_key_hash: None,
        },
```

- [ ] **Step 5: Run tests**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::canonical::tests::client_info --lib
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::providers::aijia_gateway_v2::tests::build_request_populates_basic_client_metadata --lib
```

Expected: tests pass.

- [ ] **Step 6: Commit**

```bash
git -C /Users/gezhigang/work-codeup/aijia/code add src-tauri/src/llm/canonical.rs src-tauri/src/llm/providers/aijia_gateway_v2.rs
git -C /Users/gezhigang/work-codeup/aijia/code commit -m "feat: add v2 desktop client metadata"
```

## Task 6: Add Gateway Log Context and Route/Trace Enrichment

**Files:**
- Modify: `src-tauri/src/llm/gate_log.rs`

- [ ] **Step 1: Write failing enrichment tests**

Add these tests to the existing tests module in `src-tauri/src/llm/gate_log.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::gate_log::tests::response_status_row_is_enriched_from_request_context --lib
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::gate_log::tests::route_context_enriches_later_rows --lib
```

Expected: compile failures because context helpers do not exist.

- [ ] **Step 3: Implement process-local context**

In `src-tauri/src/llm/gate_log.rs`, add imports:

```rust
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
```

Add structs near `REQUEST_SEQUENCE`:

```rust
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
```

Add helper functions:

```rust
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn remember_request_context(request_id: &str, provider: &str, url: &str, body: &Value) {
    let mut contexts = GATEWAY_LOG_CONTEXTS.lock().unwrap_or_else(|p| p.into_inner());
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
        let mut contexts = GATEWAY_LOG_CONTEXTS.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(ctx) = contexts.get_mut(request_id) {
            ctx.gateway_request_id = Some(Value::String(id.to_string()));
        }
    }
}

fn remember_route_context(request_id: &str, response_id: Option<&str>, route: &Value) {
    let mut contexts = GATEWAY_LOG_CONTEXTS.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(ctx) = contexts.get_mut(request_id) {
        ctx.response_id = response_id.map(|id| Value::String(id.to_string()));
        ctx.route.logical_model = route.get("logical_model").cloned();
        ctx.route.route_provider = route.get("provider").cloned();
        ctx.route.route_api = route.get("api").cloned();
        ctx.route.route_model = route.get("model").cloned();
        ctx.route.endpoint_id = route.get("endpoint_id").cloned();
    }
}

fn forget_request_context(request_id: &str) {
    GATEWAY_LOG_CONTEXTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(request_id);
}
```

In `record_request`, after serializing `body`, call:

```rust
    remember_request_context(request_id, provider, url, &body);
```

In `record_response_status`, before `record_event`, call:

```rust
    remember_gateway_request_id(request_id, gateway_request_id);
```

In `record_route`, before `record_event`, call:

```rust
    remember_route_context(request_id, response_id, route);
```

Modify `event_row` to enrich the base object before payload keys are merged:

```rust
    if let Some(ctx) = GATEWAY_LOG_CONTEXTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(request_id)
        .cloned()
    {
        base.insert("desktop_provider".to_string(), Value::String(ctx.provider));
        base.insert("url".to_string(), Value::String(ctx.url));
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
```

Keep payload merge after this block so explicit event payload values can override enriched defaults.

- [ ] **Step 4: Run gate log tests**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::gate_log::tests --lib
```

Expected: all gate log tests pass.

- [ ] **Step 5: Commit**

```bash
git -C /Users/gezhigang/work-codeup/aijia/code add src-tauri/src/llm/gate_log.rs
git -C /Users/gezhigang/work-codeup/aijia/code commit -m "feat: enrich v2 gateway logs"
```

## Task 7: Add Gateway Lifecycle Events

**Files:**
- Modify: `src-tauri/src/llm/gate_log.rs`
- Modify: `src-tauri/src/llm/providers/aijia_gateway_v2.rs`

- [ ] **Step 1: Write failing gate log lifecycle tests**

Add these tests to `src-tauri/src/llm/gate_log.rs`:

```rust
    #[test]
    fn stream_closed_forgets_request_context() {
        let request_id = "gate-test-close";
        super::remember_request_context(
            request_id,
            "aijia-v2",
            "https://ai-tenant.renlijia.com/aijia/v2/ai/responses",
            &json!({"conversation_id": "conv"}),
        );

        super::record_stream_closed(request_id, "response_completed", None);

        assert!(
            !super::GATEWAY_LOG_CONTEXTS
                .lock()
                .unwrap()
                .contains_key(request_id)
        );
    }

    #[test]
    fn response_completed_row_contains_lifecycle_event_name() {
        let row = super::event_row(
            "gateway.response_completed",
            "gate-test-completed",
            json!({"stop_reason": "end_turn"}),
        );

        assert_eq!(row["event"], "gateway.response_completed");
        assert_eq!(row["stop_reason"], "end_turn");
    }
```

- [ ] **Step 2: Write failing provider parser lifecycle test**

In `src-tauri/src/llm/providers/aijia_gateway_v2.rs`, add a parser-unit helper test that targets a pure helper introduced in implementation:

```rust
    #[test]
    fn frame_lifecycle_detects_response_completed() {
        assert!(frame_has_event(
            "event: response.completed\ndata: {\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}\n\n",
            "response.completed"
        ));
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::gate_log::tests::stream_closed_forgets_request_context --lib
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::providers::aijia_gateway_v2::tests::frame_lifecycle_detects_response_completed --lib
```

Expected: compile failures because lifecycle helpers do not exist.

- [ ] **Step 4: Add lifecycle helpers in `gate_log.rs`**

Add these public helper functions:

```rust
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
            "stop_reason": stop_reason,
        }),
    );
}

pub fn record_stream_closed(request_id: &str, reason: &str, error: Option<&str>) {
    record_event(
        "gateway.stream_closed",
        request_id,
        json!({
            "reason": reason,
            "error": error,
        }),
    );
    forget_request_context(request_id);
}
```

Keep `record_stream_end` unchanged for compatibility.

- [ ] **Step 5: Add lifecycle state in `aijia_gateway_v2.rs`**

Add a local state type near `sse_bytes_to_events`:

```rust
#[derive(Debug, Default)]
struct GatewayStreamLifecycle {
    first_event_seen: bool,
    response_completed: bool,
    closed: bool,
    last_error: Option<String>,
}

impl GatewayStreamLifecycle {
    fn record_frame(&mut self, request_id: Option<&str>, frame: &str) {
        let first_event = sse_event_name(frame);
        if !self.first_event_seen {
            self.first_event_seen = true;
            if let Some(id) = request_id {
                crate::llm::gate_log::record_first_event(id, first_event.as_deref());
            }
        }
        if frame_has_event(frame, "response.completed") {
            self.response_completed = true;
            if let Some(id) = request_id {
                crate::llm::gate_log::record_response_completed(id, Some("end_turn"));
            }
        }
    }

    fn record_error(&mut self, request_id: Option<&str>, error: &str) {
        self.last_error = Some(error.to_string());
        if let Some(id) = request_id {
            crate::llm::gate_log::record_stream_error(id, error);
            self.close(id, "error");
        }
    }

    fn close(&mut self, request_id: &str, fallback_reason: &str) {
        if self.closed {
            return;
        }
        self.closed = true;
        let reason = if fallback_reason == "eof" && self.response_completed {
            "response_completed"
        } else {
            fallback_reason
        };
        crate::llm::gate_log::record_stream_closed(
            request_id,
            reason,
            self.last_error.as_deref(),
        );
    }
}

fn frame_has_event(frame: &str, event_name: &str) -> bool {
    frame
        .lines()
        .any(|line| line.strip_prefix("event: ") == Some(event_name))
}

fn sse_event_name(frame: &str) -> Option<String> {
    frame
        .lines()
        .find_map(|line| line.strip_prefix("event: ").map(str::to_string))
}
```

Update `sse_bytes_to_events` state tuple to include `GatewayStreamLifecycle::default()`.

When HTTP streaming is successfully created in `stream`, before returning `sse_bytes_to_events`, call:

```rust
        crate::llm::gate_log::record_stream_started(&gate_log_id);
```

When a frame is drained in `drain_sse_frames`, call `lifecycle.record_frame(gate_log_id, &frame)` before `chunk_to_stream_event`.

When `byte_stream` returns an error, call `lifecycle.record_error(gate_log_id.as_deref(), &err.to_string())`.

When EOF is reached, call both the old EOF event and the new close event:

```rust
if let Some(request_id) = gate_log_id.as_deref() {
    crate::llm::gate_log::record_stream_end(request_id);
    lifecycle.close(request_id, "eof");
}
```

If using a drop guard instead of tuple state, ensure `gateway.stream_closed` is emitted exactly once.

- [ ] **Step 6: Run lifecycle tests**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::gate_log::tests::stream_closed_forgets_request_context --lib
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::providers::aijia_gateway_v2::tests::frame_lifecycle_detects_response_completed --lib
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::providers::aijia_gateway_v2::tests --lib
```

Expected: tests pass.

- [ ] **Step 7: Commit**

```bash
git -C /Users/gezhigang/work-codeup/aijia/code add src-tauri/src/llm/gate_log.rs src-tauri/src/llm/providers/aijia_gateway_v2.rs
git -C /Users/gezhigang/work-codeup/aijia/code commit -m "feat: log v2 gateway stream lifecycle"
```

## Task 8: Final Verification

**Files:**
- No new files unless tests reveal a scoped fix.

- [ ] **Step 1: Run focused Rust tests**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::streaming::chat_message_error_status_tests --lib
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::providers::aijia_gateway_v2::tests --lib
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test llm::gate_log::tests --lib
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test runtime::chat::tool_result_collector::tests --lib
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test runtime::chat::history --lib
```

Expected: all focused tests pass.

- [ ] **Step 2: Run broader backend test pass**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test --lib
```

Expected: library tests pass.

- [ ] **Step 3: Run full workspace test if time permits**

Run:

```bash
cd /Users/gezhigang/work-codeup/aijia/code/src-tauri
GOCACHE=/private/tmp/aijia-gateway-v2-go-cache cargo test
```

Expected: all tests pass. If known unrelated integration tests fail, record the exact failing tests and evidence before stopping.

- [ ] **Step 4: Inspect git status**

Run:

```bash
git -C /Users/gezhigang/work-codeup/aijia/code status --short
```

Expected: only intentional implementation files are modified. Existing unrelated `scripts/sign-and-upload-macos.sh` may still be present and must not be reverted or included.

- [ ] **Step 5: Commit final test/documentation adjustments if needed**

Only if Task 8 required small follow-up fixes:

```bash
git -C /Users/gezhigang/work-codeup/aijia/code add src-tauri/src/llm/streaming.rs src-tauri/src/runtime/chat/history.rs src-tauri/src/runtime/chat/tool_result_collector.rs src-tauri/src/runtime/agent/worker_runtime.rs src-tauri/src/llm/providers/aijia_gateway_v2.rs src-tauri/src/llm/canonical.rs src-tauri/src/llm/gate_log.rs
git -C /Users/gezhigang/work-codeup/aijia/code commit -m "test: cover v2 gateway observability fixes"
```

If no follow-up fixes are needed, do not create an empty commit.

## Self-Review Notes

- Spec coverage: tasks cover `is_error` propagation, optional `ClientInfo` metadata, lifecycle logging, and route/trace enrichment.
- Deferred scope preserved: no task changes tool governance metadata or binary/file artifact metadata.
- Compatibility preserved: no `schema_version` bump and all new request fields are optional or already present in canonical message schema.
- Residual risk: stream cancellation classification may need existing cancellation signal plumbing. If no cancellation signal is available, implement `dropped`/`error`/`response_completed` first and leave `client_cancelled` for a later explicit cancellation hook.
