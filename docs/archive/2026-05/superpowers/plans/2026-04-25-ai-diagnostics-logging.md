# AI Diagnostics Logging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build high-volume, frontend-queryable AI diagnostics logs across frontend and backend so Codex can reconstruct chat/LLM/tool/permission/subagent timelines with `rg`, `jq`, and the app UI.

**Architecture:** Reuse the existing `logs/metrics.jsonl` file, export/clear/info commands, and shard naming, but diagnostics and future telemetry writes must be plain compact JSONL without the legacy `\t✓` completion marker so `rg ... | jq -c ...` works directly. Backend readers must remain backward-compatible with older marker-suffixed metrics lines. Frontend and backend both emit the same top-level searchable schema (`ts`, `event`, `runId`, `conversationId`, `toolCallId`, etc.); backend writes to disk and emits `diagnostics:event`, while frontend records to an in-memory diagnostics store and forwards frontend diagnostics to backend via IPC.

**Tech Stack:** React 19, Zustand, Vitest, Tauri v2, Rust 2021, serde/serde_json, existing telemetry JSONL helpers, existing Tauri event/listen wrappers.

---

## Scope And Decisions

- Diagnostics are for machine/Codex querying first, not human-readable prose.
- Use compact JSONL: one event per line, no pretty printing.
- Keep critical query fields at the top level for easy `rg` and `jq`.
- Reuse `logs/metrics.jsonl`, export/clear/info mechanics, and shard naming; write diagnostics as plain JSONL, and keep readers compatible with older `\t✓` marker-suffixed telemetry lines.
- Use `ts` only for wall-clock time; do not store duplicate `localTime`.
- Add `seq`, `elapsedMs`, and `durationMs` for ordering and latency analysis.
- Do not hide volume by default. High-frequency events are allowed, but `streaming.delta` stores metadata by default and can include text only when explicitly enabled by code-level option.
- Redact secrets: API keys, tokens, cookies, authorization headers, bearer tokens, and obvious password fields.
- This is a lotus-app custom diagnostics extension. It is inspired by `claude-code-best` debug/transcript ideas but not a one-to-one clone.

## Query Contract

Diagnostics lines must support these commands:

```bash
# Full chain for one run
rg '"runId":"run_123"' logs/metrics.jsonl | jq -c 'select(.category=="diagnostics")'

# All errors for one conversation
jq -c 'select(.category=="diagnostics" and .conversationId=="conv_123" and .level=="error")' logs/metrics.jsonl

# Tool failures
jq -c 'select(.category=="diagnostics" and .event=="tool.execute.failed")' logs/metrics.jsonl

# Timeline summary
jq -c 'select(.category=="diagnostics" and .runId=="run_123") | {ts,seq,source,event,durationMs,ok,error}' logs/metrics.jsonl

# Backend emitted events vs frontend received events
jq -c 'select(.category=="diagnostics" and .runId=="run_123" and (.event=="event.emit.completed" or .event=="event.received")) | {ts,source,event,payload}' logs/metrics.jsonl
```

## Target Diagnostic Schema

Use this logical TypeScript shape across frontend and backend:

```ts
export type DiagnosticLevel = 'debug' | 'info' | 'warn' | 'error'
export type DiagnosticSource = 'frontend' | 'backend'

export interface DiagnosticEvent {
  ts: string
  seq: number
  category: 'diagnostics'
  level: DiagnosticLevel
  source: DiagnosticSource
  event: string
  ok?: boolean
  conversationId?: string
  runId?: string
  messageId?: string
  clientMessageId?: string
  toolCallId?: string
  agentId?: string
  interactionId?: string
  taskId?: string
  command?: string
  durationMs?: number
  elapsedMs?: number
  error?: string
  payload?: unknown
}
```

Rust serialization must use `camelCase` for fields except `ts`, `seq`, `ok`, `category`, `level`, `source`, and `event`, which are already stable lower/camel keys.

## Event Naming Contract

Use fixed dot-separated event names:

```text
chat.submit.started
chat.submit.completed
chat.submit.failed
conversation.create.started
conversation.create.completed
conversation.create.failed
conversation.switch.started
conversation.switch.completed
conversation.switch.failed
ipc.invoke.started
ipc.invoke.completed
ipc.invoke.failed
event.emit.started
event.emit.completed
event.emit.failed
event.received
event.handler.started
event.handler.completed
event.handler.failed
streaming.delta.received
streaming.delta.flushed
streaming.done.received
streaming.error.received
streaming.watchdog.stale_detected
store.messages.set
store.messages.upsert
store.streaming.append
store.streaming.clear
store.busy.add
store.busy.remove
permission.ask.received
permission.resolve.started
permission.resolve.completed
permission.resolve.failed
interaction.required.received
interaction.resolve.started
interaction.resolve.completed
interaction.resolve.failed
backend.command.started
backend.command.completed
backend.command.failed
turn.started
turn.config.loaded
turn.history.loaded
turn.completed
turn.failed
turn.cancelled
llm.settings.loaded
llm.step.started
llm.step.completed
llm.step.failed
tool.round.started
tool.round.completed
tool.execute.started
tool.execute.completed
tool.execute.failed
storage.message.persist.started
storage.message.persist.completed
storage.message.persist.failed
subagent.spawn.started
subagent.completed
subagent.failed
cancel.requested
agent.busy.rejected
```

---

## File Structure

### Backend

- Modify `src-tauri/src/telemetry.rs`
  - Add `DiagnosticEvent`, `DiagnosticLevel`, `DiagnosticSource`.
  - Add `record_diagnostic(workspace, event)` that appends the flat diagnostics record into `logs/metrics.jsonl` as plain JSONL for direct `jq` consumption.
  - Keep existing `MetricsEntry` and `record(...)` behavior intact.
  - Add tests for flat JSONL fields, direct `serde_json`/`jq`-compatible raw lines, mixed metrics+diagnostics export/info, redaction, and existing metrics compatibility.

- Modify `src-tauri/src/commands/workspace.rs`
  - Add `record_frontend_diagnostic` command.
  - Add optional `export_diagnostics` command only if existing metrics export cannot filter diagnostics cleanly; first implementation can rely on existing `export_metrics`.

- Modify `src-tauri/src/lib.rs`
  - Register `record_frontend_diagnostic` in Tauri commands.

- Modify `src-tauri/src/runtime/event_bus.rs`
  - Emit backend diagnostics around runtime event dispatch.
  - Include `eventName`, `payloadBytes`, `conversationId`, `runId` where available.

- Modify `src-tauri/src/transport/tauri_runtime_host.rs`
  - Emit diagnostics around Tauri legacy event emit success/failure.

- Modify `src-tauri/src/commands/chat.rs`
  - Emit diagnostics around chat command entry, success, failure, stop/cancel paths, and trace capture save paths.

- Modify `src-tauri/src/runtime/chat/chat_turn_driver.rs`
  - Emit diagnostics for turn lifecycle, config/history/system prompt/user persist/LLM step outcome.

- Modify `src-tauri/src/runtime/chat/tool_round_driver.rs`
  - Emit diagnostics for tool round start/completion and per-tool execution boundaries where this file owns the loop.

- Modify `src-tauri/src/runtime/tools/dispatcher.rs` and `src-tauri/src/runtime/tools/executor.rs`
  - Emit diagnostics for permission checks, tool execution start/completion/failure, and result summaries where these files own the boundaries.

- Modify `src-tauri/src/runtime/interaction/control_plane.rs`
  - Emit diagnostics for pending interaction insert/resolve/cancel.

- Modify `src-tauri/src/runtime/agent/worker_runtime.rs`
  - Emit diagnostics for subagent spawn/start/completion/failure/cancellation.

- Add tests under `src-tauri/tests/diagnostics_logging_test.rs`
  - Validate diagnostics JSONL is flat and query-friendly.
  - Validate frontend command records diagnostics.
  - Validate redaction removes secrets.

### Frontend

- Create `src/lib/diagnostics.ts`
  - Define `DiagnosticEvent`, `DiagnosticInput`, `recordDiagnostic`, `recordDiagnosticError`, `withDiagnosticSpan`, `summarizePayload`, `redactDiagnosticPayload`.
  - Maintain frontend `seq` counter.
  - Use `performance.now()` for elapsed/duration calculations.
  - Forward events to backend with `record_frontend_diagnostic`.
  - Also write events into diagnostics store.

- Create `src/stores/diagnosticsStore.ts`
  - Ring buffer of recent diagnostics events.
  - Actions: `appendDiagnostic`, `clearDiagnostics`, `getByRunId`, `getByConversationId`.

- Modify `src/lib/tauri.ts`
  - Add `recordFrontendDiagnostic` wrapper.
  - Add diagnostic instrumentation to selected IPC wrappers or add a generic `invokeWithDiagnostics` helper and use it for chat-critical commands.
  - Add `TAURI_EVENTS.DIAGNOSTICS_EVENT = 'diagnostics:event'` and listener type.

- Modify `src/hooks/useTauriEvent.ts`
  - Wrap event handlers with `event.received`, `event.handler.started`, `event.handler.completed`, `event.handler.failed` diagnostics.

- Modify `src/hooks/useStreaming.ts`
  - Add diagnostics for streaming events, delta buffer flush, done/error, watchdog, tool event handling, permission/interaction handling, turn completion.

- Modify `src/hooks/useChat.ts`
  - Add diagnostics for submit, create/switch/delete/archive/rename conversation, stop streaming, optimistic updates, rollback, and busy rejection.

- Modify `src/stores/chatStore.ts` and `src/stores/streamingStore.ts`
  - Add focused diagnostics around key state mutations only: messages set/upsert, streaming append/clear, busy add/remove, tool execution add/update, task state changes.

- Add frontend tests:
  - `src/lib/diagnostics.test.ts`
  - `src/stores/diagnosticsStore.test.ts`
  - Extend `src/lib/tauri.events.test.ts` or add `src/lib/tauri.diagnostics.test.ts`.

---

## Task 1: Backend Diagnostics Schema And Storage

**Files:**
- Modify: `src-tauri/src/telemetry.rs`

- [ ] **Step 1: Add failing backend tests for flat diagnostics JSONL**

Append these tests to the existing `#[cfg(test)] mod tests` in `src-tauri/src/telemetry.rs`:

```rust
    #[test]
    fn test_record_diagnostic_writes_flat_queryable_jsonl() {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path();

        let event = DiagnosticEvent::new("turn.started", DiagnosticSource::Backend)
            .level(DiagnosticLevel::Info)
            .conversation_id("conv_1")
            .run_id("run_1")
            .message_id("msg_1")
            .duration_ms(12)
            .ok(true)
            .payload(serde_json::json!({"phase":"start"}));

        record_diagnostic(workspace, event);

        let path = metrics_path(workspace);
        let raw = std::fs::read_to_string(path).unwrap();
        let line = raw.lines().next().unwrap();
        let value: serde_json::Value = serde_json::from_str(line).unwrap();

        assert_eq!(value["category"], "diagnostics");
        assert_eq!(value["source"], "backend");
        assert_eq!(value["event"], "turn.started");
        assert_eq!(value["conversationId"], "conv_1");
        assert_eq!(value["runId"], "run_1");
        assert_eq!(value["messageId"], "msg_1");
        assert_eq!(value["durationMs"], 12);
        assert_eq!(value["ok"], true);
        assert_eq!(value["payload"]["phase"], "start");
        assert!(value.get("fields").is_none());
        assert!(value["ts"].as_str().unwrap().ends_with('Z'));
        assert!(value["seq"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn test_record_diagnostic_writes_plain_jq_parseable_jsonl() {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path();

        record_diagnostic(
            workspace,
            DiagnosticEvent::new("tool.execute.failed", DiagnosticSource::Backend)
                .conversation_id("conv_1")
                .run_id("run_1"),
        );

        let raw = std::fs::read_to_string(metrics_path(workspace)).unwrap();
        assert!(!raw.contains('\t'));
        for line in raw.lines() {
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
    }

    #[test]
    fn test_record_diagnostic_redacts_secret_values() {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path();

        let event = DiagnosticEvent::new("ipc.invoke.started", DiagnosticSource::Frontend)
            .payload(serde_json::json!({
                "authorization": "Bearer secret-token",
                "apiKey": "sk-secret",
                "nested": {"password": "123456", "safe": "visible"}
            }));

        record_diagnostic(workspace, event);

        let raw = std::fs::read_to_string(metrics_path(workspace)).unwrap();
        assert!(!raw.contains("secret-token"));
        assert!(!raw.contains("sk-secret"));
        assert!(!raw.contains("123456"));
        assert!(raw.contains("[REDACTED]"));
        assert!(raw.contains("visible"));
    }

    #[test]
    fn test_existing_metrics_record_still_uses_fields_shape() {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path();

        record("tool", workspace, &[("conv", "c1")]);

        let raw = std::fs::read_to_string(metrics_path(workspace)).unwrap();
        let value: serde_json::Value = serde_json::from_str(raw.lines().next().unwrap()).unwrap();
        assert_eq!(value["category"], "tool");
        assert_eq!(value["fields"]["conv"], "c1");
        assert!(value.get("event").is_none());
    }
```

- [ ] **Step 2: Run the focused backend test and verify it fails**

Run:

```bash
cd src-tauri && cargo test telemetry::tests::test_record_diagnostic_writes_flat_queryable_jsonl --lib
```

Expected: FAIL because `DiagnosticEvent`, `DiagnosticSource`, `DiagnosticLevel`, and `record_diagnostic` do not exist.

- [ ] **Step 3: Implement diagnostics schema in `src-tauri/src/telemetry.rs`**

Add these imports near the top:

```rust
use std::sync::atomic::{AtomicU64, Ordering};
```

Add this static near constants:

```rust
static DIAGNOSTIC_SEQ: AtomicU64 = AtomicU64::new(1);
```

Add these types after `MetricsEntry`:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSource {
    Frontend,
    Backend,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEvent {
    pub ts: String,
    pub seq: u64,
    pub category: String,
    pub level: DiagnosticLevel,
    pub source: DiagnosticSource,
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

impl DiagnosticEvent {
    pub fn new(event: impl Into<String>, source: DiagnosticSource) -> Self {
        Self {
            ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            seq: DIAGNOSTIC_SEQ.fetch_add(1, Ordering::Relaxed),
            category: "diagnostics".to_string(),
            level: DiagnosticLevel::Info,
            source,
            event: event.into(),
            ok: None,
            conversation_id: None,
            run_id: None,
            message_id: None,
            client_message_id: None,
            tool_call_id: None,
            agent_id: None,
            interaction_id: None,
            task_id: None,
            command: None,
            duration_ms: None,
            elapsed_ms: None,
            error: None,
            payload: None,
        }
    }

    pub fn level(mut self, level: DiagnosticLevel) -> Self {
        self.level = level;
        self
    }

    pub fn ok(mut self, ok: bool) -> Self {
        self.ok = Some(ok);
        self
    }

    pub fn conversation_id(mut self, value: impl Into<String>) -> Self {
        self.conversation_id = Some(value.into());
        self
    }

    pub fn run_id(mut self, value: impl Into<String>) -> Self {
        self.run_id = Some(value.into());
        self
    }

    pub fn message_id(mut self, value: impl Into<String>) -> Self {
        self.message_id = Some(value.into());
        self
    }

    pub fn client_message_id(mut self, value: impl Into<String>) -> Self {
        self.client_message_id = Some(value.into());
        self
    }

    pub fn tool_call_id(mut self, value: impl Into<String>) -> Self {
        self.tool_call_id = Some(value.into());
        self
    }

    pub fn agent_id(mut self, value: impl Into<String>) -> Self {
        self.agent_id = Some(value.into());
        self
    }

    pub fn interaction_id(mut self, value: impl Into<String>) -> Self {
        self.interaction_id = Some(value.into());
        self
    }

    pub fn task_id(mut self, value: impl Into<String>) -> Self {
        self.task_id = Some(value.into());
        self
    }

    pub fn command(mut self, value: impl Into<String>) -> Self {
        self.command = Some(redact_sensitive_text(&value.into()));
        self
    }

    pub fn duration_ms(mut self, value: u64) -> Self {
        self.duration_ms = Some(value);
        self
    }

    pub fn elapsed_ms(mut self, value: u64) -> Self {
        self.elapsed_ms = Some(value);
        self
    }

    pub fn error(mut self, value: impl Into<String>) -> Self {
        self.error = Some(redact_sensitive_text(&value.into()));
        self.level = DiagnosticLevel::Error;
        self.ok = Some(false);
        self
    }

    pub fn payload(mut self, value: serde_json::Value) -> Self {
        self.payload = Some(redact_value(value));
        self
    }
}
```

Add these helper functions before `record(...)`:

```rust
fn is_secret_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("token")
        || lower.contains("apikey")
        || lower.contains("api_key")
        || lower.contains("authorization")
        || lower.contains("cookie")
        || lower.contains("password")
        || lower.contains("secret")
}

fn redact_value(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    if is_secret_key(&key) {
                        (key, serde_json::Value::String("[REDACTED]".to_string()))
                    } else {
                        (key, redact_value(value))
                    }
                })
                .collect(),
        ),
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(redact_value).collect())
        }
        serde_json::Value::String(value) => serde_json::Value::String(redact_sensitive_text(&value)),
        other => other,
    }
}

fn redact_sensitive_text(value: &str) -> String {
    let mut redacted = redact_after_case_insensitive(value, "bearer ");
    for marker in ["access_token=", "token=", "api_key=", "apikey=", "password=", "secret="] {
        redacted = redact_after_case_insensitive(&redacted, marker);
    }
    redact_prefixed_secret(&redacted, "sk-")
}
```

Add this function after `record(...)` or before it:

```rust
pub fn record_diagnostic(workspace: &Path, event: DiagnosticEvent) {
    let path = metrics_path(workspace);
    if let Err(e) = append_plain_jsonl_with_split(&path, &event, SPLIT_THRESHOLD) {
        log::warn!("[telemetry] Failed to write diagnostic entry: {}", e);
    }
}
```

- [ ] **Step 4: Run backend telemetry tests**

Run:

```bash
cd src-tauri && cargo test telemetry::tests --lib
```

Expected: PASS. Existing metrics tests still pass, and new diagnostics tests pass.

- [ ] **Step 5: Commit Task 1**

```bash
git add src-tauri/src/telemetry.rs docs/superpowers/plans/2026-04-25-ai-diagnostics-logging.md
git commit -m "feat(diagnostics): add flat telemetry log schema"
```

---

## Task 2: Backend IPC Command For Frontend Diagnostics

**Files:**
- Modify: `src-tauri/src/commands/workspace.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/tests/diagnostics_logging_test.rs`

- [ ] **Step 1: Add failing integration test**

Create `src-tauri/tests/diagnostics_logging_test.rs`:

```rust
use app_lib::telemetry::{DiagnosticEvent, DiagnosticLevel, DiagnosticSource};
use tempfile::TempDir;

#[test]
fn frontend_diagnostic_event_serializes_queryable_keys() {
    let event = DiagnosticEvent::new("ipc.invoke.started", DiagnosticSource::Frontend)
        .level(DiagnosticLevel::Debug)
        .conversation_id("conv_test")
        .run_id("run_test")
        .command("send_message")
        .payload(serde_json::json!({"argBytes": 25}));

    let value = serde_json::to_value(event).unwrap();
    assert_eq!(value["category"], "diagnostics");
    assert_eq!(value["source"], "frontend");
    assert_eq!(value["event"], "ipc.invoke.started");
    assert_eq!(value["conversationId"], "conv_test");
    assert_eq!(value["runId"], "run_test");
    assert_eq!(value["command"], "send_message");
}

#[test]
fn diagnostics_can_share_metrics_jsonl_file() {
    let dir = TempDir::new().unwrap();
    let workspace = dir.path();
    app_lib::telemetry::record_diagnostic(
        workspace,
        DiagnosticEvent::new("chat.submit.started", DiagnosticSource::Frontend)
            .conversation_id("conv_test"),
    );

    let (json, count) = app_lib::telemetry::export_all(workspace).unwrap();
    assert_eq!(count, 1);
    assert!(json.contains("chat.submit.started"));
    assert!(json.contains("conversationId"));
}
```

- [ ] **Step 2: Run the focused integration test**

Run:

```bash
cd src-tauri && cargo test --test diagnostics_logging_test
```

Expected: PASS after Task 1. If it fails because `telemetry` is not publicly exported in the test crate, verify `src-tauri/src/lib.rs` already has `pub mod telemetry;` and fix export visibility only.

- [ ] **Step 3: Add command payload type and command**

In `src-tauri/src/commands/workspace.rs`, add imports:

```rust
use crate::telemetry::{record_diagnostic, DiagnosticEvent, DiagnosticLevel, DiagnosticSource};
```

Add this payload struct near other command DTOs:

```rust
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendDiagnosticPayload {
    pub event: String,
    pub level: Option<DiagnosticLevel>,
    pub ok: Option<bool>,
    pub conversation_id: Option<String>,
    pub run_id: Option<String>,
    pub message_id: Option<String>,
    pub client_message_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub agent_id: Option<String>,
    pub interaction_id: Option<String>,
    pub task_id: Option<String>,
    pub command: Option<String>,
    pub duration_ms: Option<u64>,
    pub elapsed_ms: Option<u64>,
    pub error: Option<String>,
    pub payload: Option<serde_json::Value>,
}
```

Add this command near metrics commands:

```rust
#[tauri::command]
pub async fn record_frontend_diagnostic(
    state: tauri::State<'_, crate::runtime::state::AppState>,
    diagnostic: FrontendDiagnosticPayload,
) -> Result<(), String> {
    let workspace = state
        .storage
        .base_dir()
        .to_path_buf();

    let mut event = DiagnosticEvent::new(diagnostic.event, DiagnosticSource::Frontend);
    if let Some(level) = diagnostic.level {
        event = event.level(level);
    }
    if let Some(ok) = diagnostic.ok {
        event = event.ok(ok);
    }
    if let Some(value) = diagnostic.conversation_id {
        event = event.conversation_id(value);
    }
    if let Some(value) = diagnostic.run_id {
        event = event.run_id(value);
    }
    if let Some(value) = diagnostic.message_id {
        event = event.message_id(value);
    }
    if let Some(value) = diagnostic.client_message_id {
        event = event.client_message_id(value);
    }
    if let Some(value) = diagnostic.tool_call_id {
        event = event.tool_call_id(value);
    }
    if let Some(value) = diagnostic.agent_id {
        event = event.agent_id(value);
    }
    if let Some(value) = diagnostic.interaction_id {
        event = event.interaction_id(value);
    }
    if let Some(value) = diagnostic.task_id {
        event = event.task_id(value);
    }
    if let Some(value) = diagnostic.command {
        event = event.command(value);
    }
    if let Some(value) = diagnostic.duration_ms {
        event = event.duration_ms(value);
    }
    if let Some(value) = diagnostic.elapsed_ms {
        event = event.elapsed_ms(value);
    }
    if let Some(value) = diagnostic.error {
        event = event.error(value);
    }
    if let Some(value) = diagnostic.payload {
        event = event.payload(value);
    }

    record_diagnostic(&workspace, event);
    Ok(())
}
```

If `state.storage.base_dir()` does not exist, inspect `src-tauri/src/storage/file_store/mod.rs` for the actual accessor. Use the existing workspace metrics commands in `workspace.rs` as the exact pattern and use the same base directory expression.

- [ ] **Step 4: Register the command**

In `src-tauri/src/lib.rs`, add `workspace::record_frontend_diagnostic` to the `tauri::generate_handler!` list near `export_metrics`, `clear_metrics`, and `get_metrics_info`.

- [ ] **Step 5: Run Rust checks**

Run:

```bash
cd src-tauri && cargo check
cd src-tauri && cargo test --test diagnostics_logging_test
```

Expected: `cargo check` PASS and diagnostics integration test PASS.

- [ ] **Step 6: Commit Task 2**

```bash
git add src-tauri/src/commands/workspace.rs src-tauri/src/lib.rs src-tauri/tests/diagnostics_logging_test.rs
git commit -m "feat(diagnostics): accept frontend diagnostic events"
```

---

## Task 3: Frontend Diagnostics Library And Store

**Files:**
- Create: `src/lib/diagnostics.ts`
- Create: `src/stores/diagnosticsStore.ts`
- Create: `src/lib/diagnostics.test.ts`
- Create: `src/stores/diagnosticsStore.test.ts`
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: Add failing frontend diagnostics tests**

Create `src/lib/diagnostics.test.ts`:

```ts
import { describe, expect, it, vi, beforeEach } from 'vitest'
import {
  buildDiagnosticEvent,
  redactDiagnosticPayload,
  summarizePayload,
} from './diagnostics'

describe('diagnostics', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-04-25T12:34:56.789Z'))
  })

  it('builds flat queryable diagnostic events', () => {
    const event = buildDiagnosticEvent({
      event: 'chat.submit.started',
      level: 'info',
      conversationId: 'conv_1',
      runId: 'run_1',
      payload: { messageLength: 12 },
    })

    expect(event.ts).toBe('2026-04-25T12:34:56.789Z')
    expect(event.category).toBe('diagnostics')
    expect(event.source).toBe('frontend')
    expect(event.event).toBe('chat.submit.started')
    expect(event.conversationId).toBe('conv_1')
    expect(event.runId).toBe('run_1')
    expect(event.payload).toEqual({ messageLength: 12 })
    expect(event.seq).toBeGreaterThan(0)
  })

  it('redacts secret payload keys recursively', () => {
    const redacted = redactDiagnosticPayload({
      authorization: 'Bearer abc',
      apiKey: 'sk-test',
      nested: { password: 'pw', safe: 'ok' },
    })

    expect(JSON.stringify(redacted)).not.toContain('Bearer abc')
    expect(JSON.stringify(redacted)).not.toContain('sk-test')
    expect(JSON.stringify(redacted)).not.toContain('pw')
    expect(redacted).toMatchObject({
      authorization: '[REDACTED]',
      apiKey: '[REDACTED]',
      nested: { password: '[REDACTED]', safe: 'ok' },
    })
  })

  it('summarizes large payloads without dropping query fields', () => {
    const summary = summarizePayload({
      text: 'x'.repeat(300),
      list: Array.from({ length: 20 }, (_, i) => i),
    })

    expect(summary).toMatchObject({
      text: expect.stringContaining('[truncated'),
      list: expect.arrayContaining([0, 1, 2]),
    })
  })
})
```

Create `src/stores/diagnosticsStore.test.ts`:

```ts
import { beforeEach, describe, expect, it } from 'vitest'
import { useDiagnosticsStore } from './diagnosticsStore'

describe('diagnosticsStore', () => {
  beforeEach(() => {
    useDiagnosticsStore.getState().clearDiagnostics()
  })

  it('keeps recent diagnostics in insertion order', () => {
    useDiagnosticsStore.getState().appendDiagnostic({
      ts: '2026-04-25T00:00:00.000Z',
      seq: 1,
      category: 'diagnostics',
      source: 'frontend',
      level: 'info',
      event: 'chat.submit.started',
      conversationId: 'conv_1',
      runId: 'run_1',
    })

    expect(useDiagnosticsStore.getState().events).toHaveLength(1)
    expect(useDiagnosticsStore.getState().getByRunId('run_1')).toHaveLength(1)
    expect(useDiagnosticsStore.getState().getByConversationId('conv_1')).toHaveLength(1)
  })
})
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
pnpm vitest run src/lib/diagnostics.test.ts src/stores/diagnosticsStore.test.ts
```

Expected: FAIL because files do not exist.

- [ ] **Step 3: Implement diagnostics store**

Create `src/stores/diagnosticsStore.ts`:

```ts
import { create } from 'zustand'
import type { DiagnosticEvent } from '@/lib/diagnostics'

const MAX_DIAGNOSTIC_EVENTS = 5000

interface DiagnosticsState {
  events: DiagnosticEvent[]
  appendDiagnostic: (event: DiagnosticEvent) => void
  clearDiagnostics: () => void
  getByRunId: (runId: string) => DiagnosticEvent[]
  getByConversationId: (conversationId: string) => DiagnosticEvent[]
}

export const useDiagnosticsStore = create<DiagnosticsState>((set, get) => ({
  events: [],
  appendDiagnostic: (event) =>
    set((state) => {
      const next = [...state.events, event]
      return { events: next.length > MAX_DIAGNOSTIC_EVENTS ? next.slice(-MAX_DIAGNOSTIC_EVENTS) : next }
    }),
  clearDiagnostics: () => set({ events: [] }),
  getByRunId: (runId) => get().events.filter((event) => event.runId === runId),
  getByConversationId: (conversationId) =>
    get().events.filter((event) => event.conversationId === conversationId),
}))
```

- [ ] **Step 4: Add frontend Tauri wrapper type**

In `src/lib/tauri.ts`, add this event constant:

```ts
DIAGNOSTICS_EVENT: 'diagnostics:event',
```

Add these types and wrapper near metrics wrappers:

```ts
export type DiagnosticLevel = 'debug' | 'info' | 'warn' | 'error'

export interface FrontendDiagnosticPayload {
  event: string
  level?: DiagnosticLevel
  ok?: boolean
  conversationId?: string
  runId?: string
  messageId?: string
  clientMessageId?: string
  toolCallId?: string
  agentId?: string
  interactionId?: string
  taskId?: string
  command?: string
  durationMs?: number
  elapsedMs?: number
  error?: string
  payload?: unknown
}

export async function recordFrontendDiagnostic(diagnostic: FrontendDiagnosticPayload): Promise<void> {
  return invoke<void>('record_frontend_diagnostic', { diagnostic })
}
```

- [ ] **Step 5: Implement diagnostics library**

Create `src/lib/diagnostics.ts`:

```ts
import { recordFrontendDiagnostic } from './tauri'
import { useDiagnosticsStore } from '@/stores/diagnosticsStore'
import type { DiagnosticLevel, FrontendDiagnosticPayload } from './tauri'

export type { DiagnosticLevel }
export type DiagnosticSource = 'frontend' | 'backend'

export interface DiagnosticEvent extends FrontendDiagnosticPayload {
  ts: string
  seq: number
  category: 'diagnostics'
  level: DiagnosticLevel
  source: DiagnosticSource
}

export type DiagnosticInput = Omit<FrontendDiagnosticPayload, 'payload'> & {
  payload?: unknown
}

let seq = 1
const appStartMs = typeof performance !== 'undefined' ? performance.now() : Date.now()

function isSecretKey(key: string): boolean {
  const lower = key.toLowerCase()
  return (
    lower.includes('token') ||
    lower.includes('apikey') ||
    lower.includes('api_key') ||
    lower.includes('authorization') ||
    lower.includes('cookie') ||
    lower.includes('password') ||
    lower.includes('secret')
  )
}

export function redactDiagnosticPayload<T>(value: T): T {
  if (Array.isArray(value)) {
    return value.map((item) => redactDiagnosticPayload(item)) as T
  }
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([key, nested]) => [
        key,
        isSecretKey(key) ? '[REDACTED]' : redactDiagnosticPayload(nested),
      ]),
    ) as T
  }
  return value
}

export function summarizePayload(value: unknown): unknown {
  if (typeof value === 'string') {
    if (value.length <= 240) return value
    return `${value.slice(0, 240)}...[truncated ${value.length - 240} chars]`
  }
  if (Array.isArray(value)) {
    const head = value.slice(0, 10).map(summarizePayload)
    if (value.length > 10) head.push(`[truncated ${value.length - 10} items]`)
    return head
  }
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([key, nested]) => [
        key,
        summarizePayload(nested),
      ]),
    )
  }
  return value
}

export function buildDiagnosticEvent(input: DiagnosticInput): DiagnosticEvent {
  const nowMs = typeof performance !== 'undefined' ? performance.now() : Date.now()
  const payload = input.payload === undefined
    ? undefined
    : summarizePayload(redactDiagnosticPayload(input.payload))

  return {
    ...input,
    ts: new Date().toISOString(),
    seq: seq++,
    category: 'diagnostics',
    source: 'frontend',
    level: input.level ?? 'info',
    elapsedMs: input.elapsedMs ?? Math.max(0, Math.round(nowMs - appStartMs)),
    payload,
  }
}

export function recordDiagnostic(input: DiagnosticInput): DiagnosticEvent {
  const event = buildDiagnosticEvent(input)
  useDiagnosticsStore.getState().appendDiagnostic(event)
  void recordFrontendDiagnostic(event).catch((error) => {
    useDiagnosticsStore.getState().appendDiagnostic(
      buildDiagnosticEvent({
        event: 'diagnostics.forward.failed',
        level: 'warn',
        ok: false,
        error: error instanceof Error ? error.message : String(error),
        payload: { originalEvent: event.event, originalSeq: event.seq },
      }),
    )
  })
  return event
}

export function recordDiagnosticError(
  event: string,
  error: unknown,
  input: Omit<DiagnosticInput, 'event' | 'level' | 'ok' | 'error'> = {},
): DiagnosticEvent {
  return recordDiagnostic({
    ...input,
    event,
    level: 'error',
    ok: false,
    error: error instanceof Error ? error.message : String(error),
    payload: {
      ...(typeof input.payload === 'object' && input.payload ? input.payload as object : {}),
      stack: error instanceof Error ? error.stack : undefined,
    },
  })
}

export async function withDiagnosticSpan<T>(
  input: DiagnosticInput,
  fn: () => Promise<T>,
): Promise<T> {
  const startedAt = typeof performance !== 'undefined' ? performance.now() : Date.now()
  recordDiagnostic({ ...input, event: `${input.event}.started` })
  try {
    const result = await fn()
    const endedAt = typeof performance !== 'undefined' ? performance.now() : Date.now()
    recordDiagnostic({
      ...input,
      event: `${input.event}.completed`,
      ok: true,
      durationMs: Math.round(endedAt - startedAt),
    })
    return result
  } catch (error) {
    const endedAt = typeof performance !== 'undefined' ? performance.now() : Date.now()
    recordDiagnosticError(`${input.event}.failed`, error, {
      ...input,
      durationMs: Math.round(endedAt - startedAt),
    })
    throw error
  }
}
```

- [ ] **Step 6: Run frontend diagnostics tests**

Run:

```bash
pnpm vitest run src/lib/diagnostics.test.ts src/stores/diagnosticsStore.test.ts
```

Expected: PASS.

- [ ] **Step 7: Commit Task 3**

```bash
git add src/lib/diagnostics.ts src/stores/diagnosticsStore.ts src/lib/diagnostics.test.ts src/stores/diagnosticsStore.test.ts src/lib/tauri.ts
git commit -m "feat(diagnostics): add frontend diagnostic recorder"
```

---

## Task 4: Instrument Frontend IPC And Tauri Event Handling

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src/hooks/useTauriEvent.ts`
- Test: `src/lib/tauri.diagnostics.test.ts` or extend `src/lib/tauri.events.test.ts`

- [ ] **Step 1: Add failing test for event instrumentation helper**

Create `src/lib/tauri.diagnostics.test.ts`:

```ts
import { describe, expect, it, vi } from 'vitest'
import { createInstrumentedEventHandler } from './tauri'
import { useDiagnosticsStore } from '@/stores/diagnosticsStore'

describe('tauri diagnostics helpers', () => {
  it('records handler success and failure around event callbacks', async () => {
    useDiagnosticsStore.getState().clearDiagnostics()
    const handler = createInstrumentedEventHandler('streaming:done', () => undefined)

    await handler({ payload: { conversationId: 'conv_1' } } as never)

    const events = useDiagnosticsStore.getState().events.map((event) => event.event)
    expect(events).toContain('event.received')
    expect(events).toContain('event.handler.completed')
  })

  it('records handler failure before rethrowing', async () => {
    useDiagnosticsStore.getState().clearDiagnostics()
    const handler = createInstrumentedEventHandler('streaming:error', () => {
      throw new Error('boom')
    })

    await expect(handler({ payload: { conversationId: 'conv_1' } } as never)).rejects.toThrow('boom')

    const failed = useDiagnosticsStore.getState().events.find((event) => event.event === 'event.handler.failed')
    expect(failed?.level).toBe('error')
    expect(failed?.payload).toMatchObject({ eventName: 'streaming:error' })
  })
})
```

- [ ] **Step 2: Run the test and verify failure**

Run:

```bash
pnpm vitest run src/lib/tauri.diagnostics.test.ts
```

Expected: FAIL because `createInstrumentedEventHandler` does not exist.

- [ ] **Step 3: Add event handler instrumentation helper**

In `src/lib/tauri.ts`, add:

```ts
import { recordDiagnostic, recordDiagnosticError } from './diagnostics'
```

If this creates a circular import because `diagnostics.ts` imports `recordFrontendDiagnostic` from `tauri.ts`, move `recordFrontendDiagnostic` into a new `src/lib/tauriDiagnostics.ts` file and import it from both places. Use the new file only for the single IPC wrapper:

```ts
import { invoke } from '@tauri-apps/api/core'
import type { FrontendDiagnosticPayload } from './tauri'

export async function recordFrontendDiagnostic(diagnostic: FrontendDiagnosticPayload): Promise<void> {
  return invoke<void>('record_frontend_diagnostic', { diagnostic })
}
```

Then remove `recordFrontendDiagnostic` from `tauri.ts` and import it in `diagnostics.ts` from `./tauriDiagnostics`.

Add this helper in `src/lib/tauri.ts`:

```ts
function getConversationIdFromPayload(payload: unknown): string | undefined {
  return payload && typeof payload === 'object' && 'conversationId' in payload
    ? String((payload as { conversationId?: unknown }).conversationId ?? '') || undefined
    : undefined
}

function getRunIdFromPayload(payload: unknown): string | undefined {
  return payload && typeof payload === 'object' && 'runId' in payload
    ? String((payload as { runId?: unknown }).runId ?? '') || undefined
    : undefined
}

export function createInstrumentedEventHandler<T>(
  eventName: string,
  handler: (event: { payload: T }) => void | Promise<void>,
): (event: { payload: T }) => Promise<void> {
  return async (event) => {
    const startedAt = performance.now()
    const conversationId = getConversationIdFromPayload(event.payload)
    const runId = getRunIdFromPayload(event.payload)
    recordDiagnostic({
      event: 'event.received',
      conversationId,
      runId,
      payload: { eventName, payload: event.payload },
    })
    recordDiagnostic({
      event: 'event.handler.started',
      conversationId,
      runId,
      payload: { eventName },
    })
    try {
      await handler(event)
      recordDiagnostic({
        event: 'event.handler.completed',
        ok: true,
        conversationId,
        runId,
        durationMs: Math.round(performance.now() - startedAt),
        payload: { eventName },
      })
    } catch (error) {
      recordDiagnosticError('event.handler.failed', error, {
        conversationId,
        runId,
        durationMs: Math.round(performance.now() - startedAt),
        payload: { eventName },
      })
      throw error
    }
  }
}
```

- [ ] **Step 4: Use helper in `useTauriEvent`**

Open `src/hooks/useTauriEvent.ts`. Wrap the callback passed to `listen(...)` with `createInstrumentedEventHandler(eventName, callback)`. The final shape should be equivalent to:

```ts
import { listen } from '@tauri-apps/api/event'
import { createInstrumentedEventHandler } from '@/lib/tauri'

export function useTauriEvent<T>(eventName: string, handler: (payload: T) => void | Promise<void>) {
  // Preserve existing hook signature and cleanup behavior.
  // Inside the existing effect:
  const unlistenPromise = listen<T>(
    eventName,
    createInstrumentedEventHandler(eventName, async (event) => handler(event.payload)),
  )
}
```

Do not rewrite unrelated hook behavior; preserve dependency arrays and cleanup semantics.

- [ ] **Step 5: Run event diagnostics tests**

Run:

```bash
pnpm vitest run src/lib/tauri.diagnostics.test.ts src/hooks/useTauriEvent.test.ts src/lib/tauri.events.test.ts
```

Expected: PASS.

- [ ] **Step 6: Commit Task 4**

```bash
git add src/lib/tauri.ts src/lib/tauri.diagnostics.test.ts src/hooks/useTauriEvent.ts src/lib/tauriDiagnostics.ts
git commit -m "feat(diagnostics): trace frontend ipc events"
```

If `src/lib/tauriDiagnostics.ts` was not needed, omit it from `git add`.

---

## Task 5: Instrument Frontend Chat And Streaming Paths

**Files:**
- Modify: `src/hooks/useChat.ts`
- Modify: `src/hooks/useStreaming.ts`
- Modify: `src/stores/chatStore.ts`
- Modify: `src/stores/streamingStore.ts`
- Test: extend existing tests in `src/hooks/useStreaming.integration.test.tsx`, `src/hooks/__tests__/useChat.archive.test.ts`, `src/stores/chatStore.test.ts`, `src/stores/streamingStore.test.ts`

- [ ] **Step 1: Add diagnostics assertions to existing store tests**

In `src/stores/chatStore.test.ts`, add a test that clears diagnostics, calls a key message mutation, and asserts an event was appended:

```ts
import { useDiagnosticsStore } from './diagnosticsStore'

it('records diagnostics when messages are set', () => {
  useDiagnosticsStore.getState().clearDiagnostics()
  useChatStore.getState().setMessages([])
  expect(useDiagnosticsStore.getState().events.some((event) => event.event === 'store.messages.set')).toBe(true)
})
```

In `src/stores/streamingStore.test.ts`, add:

```ts
import { useDiagnosticsStore } from './diagnosticsStore'

it('records diagnostics when streaming content changes', () => {
  useDiagnosticsStore.getState().clearDiagnostics()
  useStreamingStore.getState().appendConversationStreamingContent('conv_1', 'hello')
  expect(useDiagnosticsStore.getState().events.some((event) => event.event === 'store.streaming.append')).toBe(true)
})
```

Adjust import paths if these stores expose actions through `useChatStore` only. Use the actual store action names from the files.

- [ ] **Step 2: Run store tests and verify failure**

Run:

```bash
pnpm vitest run src/stores/chatStore.test.ts src/stores/streamingStore.test.ts
```

Expected: FAIL because store actions do not record diagnostics.

- [ ] **Step 3: Instrument `chatStore` key mutations**

In `src/stores/chatStore.ts`, import:

```ts
import { recordDiagnostic } from '@/lib/diagnostics'
```

Add diagnostics inside these existing actions without changing state semantics:

```ts
recordDiagnostic({
  event: 'store.messages.set',
  conversationId: get().activeConversationId ?? undefined,
  payload: { messageCount: messages.length },
})
```

For message upsert/update actions, use:

```ts
recordDiagnostic({
  event: 'store.messages.upsert',
  conversationId: message.conversationId,
  messageId: message.id,
  payload: { role: message.role, hasToolCalls: Boolean(message.toolCalls?.length) },
})
```

For busy state:

```ts
recordDiagnostic({ event: 'store.busy.add', conversationId, payload: { busyCount: nextBusyIds.length } })
recordDiagnostic({ event: 'store.busy.remove', conversationId, payload: { busyCount: nextBusyIds.length } })
```

Use actual local variable names from `chatStore.ts`.

- [ ] **Step 4: Instrument `streamingStore` key mutations**

In `src/stores/streamingStore.ts`, import `recordDiagnostic` and add:

```ts
recordDiagnostic({
  event: 'store.streaming.append',
  conversationId,
  payload: { deltaLength: content.length },
})
```

For clear/reset actions:

```ts
recordDiagnostic({
  event: 'store.streaming.clear',
  conversationId,
})
```

For tool execution add/update:

```ts
recordDiagnostic({
  event: 'store.tool_execution.update',
  conversationId,
  toolCallId: toolId,
  payload: { toolName, status },
})
```

Use actual action parameter names from the file.

- [ ] **Step 5: Instrument `useChat` high-level actions**

In `src/hooks/useChat.ts`, import:

```ts
import { recordDiagnostic, recordDiagnosticError } from '@/lib/diagnostics'
```

Add events:

```ts
recordDiagnostic({ event: 'conversation.create.started', payload: { optimisticId } })
recordDiagnostic({ event: 'conversation.create.completed', ok: true, conversationId: backendId ?? optimisticId })
recordDiagnosticError('conversation.create.failed', err, { conversationId: optimisticId })

recordDiagnostic({ event: 'conversation.switch.started', conversationId: id })
recordDiagnostic({ event: 'conversation.switch.completed', ok: true, conversationId: id, payload: { messageCount: loadedMessages.length } })
recordDiagnosticError('conversation.switch.failed', err, { conversationId: id })

recordDiagnostic({ event: 'chat.submit.started', conversationId, clientMessageId, payload: { messageLength: content.length, fileCount: files.length } })
recordDiagnostic({ event: 'chat.submit.completed', ok: true, conversationId, clientMessageId })
recordDiagnosticError('chat.submit.failed', err, { conversationId, clientMessageId })
```

Use actual variable names in `sendUserMessage`; do not invent `content`, `files`, or `loadedMessages` if the function uses different names.

- [ ] **Step 6: Instrument `useStreaming` event handlers**

In `src/hooks/useStreaming.ts`, import diagnostics and add events inside existing handlers:

```ts
recordDiagnostic({
  event: 'streaming.delta.received',
  conversationId: payload.conversationId,
  payload: { deltaLength: payload.delta.length },
})

recordDiagnostic({
  event: 'streaming.delta.flushed',
  conversationId: convId,
  payload: { accumulatedLength: accumulated.length },
})

recordDiagnostic({ event: 'streaming.done.received', conversationId: payload.conversationId })
recordDiagnostic({ event: 'streaming.error.received', level: 'error', ok: false, conversationId: payload.conversationId, error: payload.error })
recordDiagnostic({ event: 'permission.ask.received', conversationId: payload.conversationId, runId: payload.runId, toolCallId: payload.toolCallId, payload: { toolName: payload.toolName, mode: payload.mode } })
recordDiagnostic({ event: 'interaction.required.received', conversationId: payload.conversationId, runId: payload.runId, interactionId: payload.interactionId, toolCallId: payload.toolCallId })
recordDiagnostic({ event: 'streaming.watchdog.stale_detected', conversationId: convId, payload: { lastActivityAt, now } })
```

- [ ] **Step 7: Run frontend affected tests**

Run:

```bash
pnpm vitest run src/stores/chatStore.test.ts src/stores/streamingStore.test.ts src/hooks/useStreaming.integration.test.tsx src/hooks/__tests__/useChat.archive.test.ts
```

Expected: PASS.

- [ ] **Step 8: Commit Task 5**

```bash
git add src/hooks/useChat.ts src/hooks/useStreaming.ts src/stores/chatStore.ts src/stores/streamingStore.ts src/stores/chatStore.test.ts src/stores/streamingStore.test.ts
git commit -m "feat(diagnostics): trace chat streaming state"
```

---

## Task 6: Backend Runtime Instrumentation

**Files:**
- Modify: `src-tauri/src/runtime/event_bus.rs`
- Modify: `src-tauri/src/transport/tauri_runtime_host.rs`
- Modify: `src-tauri/src/commands/chat.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/runtime/chat/tool_round_driver.rs`
- Modify: `src-tauri/src/runtime/tools/dispatcher.rs`
- Modify: `src-tauri/src/runtime/tools/executor.rs`
- Modify: `src-tauri/src/runtime/interaction/control_plane.rs`
- Modify: `src-tauri/src/runtime/agent/worker_runtime.rs`
- Test: add or extend focused tests under `src-tauri/tests/diagnostics_logging_test.rs`

- [ ] **Step 1: Add small backend helper to reduce call-site noise**

In `src-tauri/src/telemetry.rs`, add:

```rust
pub fn backend_diagnostic(event: impl Into<String>) -> DiagnosticEvent {
    DiagnosticEvent::new(event, DiagnosticSource::Backend)
}
```

Run:

```bash
cd src-tauri && cargo test telemetry::tests --lib
```

Expected: PASS.

- [ ] **Step 2: Instrument backend command boundaries**

In `src-tauri/src/commands/chat.rs`, add diagnostics at command entry and exit for the main send/stop commands. Pattern:

```rust
let started_at = std::time::Instant::now();
let workspace = state.storage.base_dir().to_path_buf();
crate::telemetry::record_diagnostic(
    &workspace,
    crate::telemetry::backend_diagnostic("backend.command.started")
        .command("send_message")
        .conversation_id(conversation_id.clone())
        .payload(serde_json::json!({"contentLength": content.len(), "fileCount": file_ids.len()})),
);
```

On success:

```rust
crate::telemetry::record_diagnostic(
    &workspace,
    crate::telemetry::backend_diagnostic("backend.command.completed")
        .command("send_message")
        .conversation_id(conversation_id.clone())
        .duration_ms(started_at.elapsed().as_millis() as u64)
        .ok(true),
);
```

On error before returning:

```rust
crate::telemetry::record_diagnostic(
    &workspace,
    crate::telemetry::backend_diagnostic("backend.command.failed")
        .command("send_message")
        .conversation_id(conversation_id.clone())
        .duration_ms(started_at.elapsed().as_millis() as u64)
        .error(err.to_string()),
);
```

Use exact function parameter names from `chat.rs`.

- [ ] **Step 3: Instrument event emit boundaries**

In `src-tauri/src/transport/tauri_runtime_host.rs`, wrap `emit_legacy_event` implementation:

```rust
let started_at = std::time::Instant::now();
let payload_bytes = serde_json::to_vec(&payload).map(|bytes| bytes.len()).unwrap_or(0);
```

Before emit, record:

```rust
backend_diagnostic("event.emit.started")
    .payload(serde_json::json!({"eventName": name, "payloadBytes": payload_bytes}))
```

After emit success/failure, record `event.emit.completed` or `event.emit.failed` with duration. If this layer does not have workspace path, add diagnostics in the nearest caller with workspace access instead of changing trait signatures.

- [ ] **Step 4: Instrument turn lifecycle**

In `src-tauri/src/runtime/chat/chat_turn_driver.rs`, record:

```rust
turn.started
turn.config.loaded
turn.history.loaded
llm.step.started
llm.step.completed
llm.step.failed
turn.completed
turn.failed
turn.cancelled
```

Each event should include `conversationId` and `runId` from `ChatTurnRequest` / `TurnConfig`. If workspace path is only available through `executor.load_workspace_path()`, load once and reuse the `PathBuf` for diagnostics.

- [ ] **Step 5: Instrument tool lifecycle**

In `src-tauri/src/runtime/chat/tool_round_driver.rs`, `src-tauri/src/runtime/tools/dispatcher.rs`, and `src-tauri/src/runtime/tools/executor.rs`, record:

```rust
tool.round.started
tool.execute.started
tool.execute.completed
tool.execute.failed
tool.round.completed
```

Each tool event should include `conversationId`, `runId`, `toolCallId`, and payload `{ "toolName": "..." }`. For results, include `durationMs`, `ok`, and a result-size summary rather than full binary/file content.

- [ ] **Step 6: Instrument permission and interaction lifecycle**

In permission-owning code under `runtime/tools/*` and `src-tauri/src/runtime/interaction/control_plane.rs`, record:

```rust
permission.requested
permission.resolved
permission.denied
interaction.required
interaction.resolved
interaction.cancelled
```

Include `conversationId`, `runId`, `toolCallId`, `interactionId`, and payload containing `toolName` and resolution kind.

- [ ] **Step 7: Instrument subagent lifecycle**

In `src-tauri/src/runtime/agent/worker_runtime.rs`, record:

```rust
subagent.spawn.started
subagent.completed
subagent.failed
```

Include parent `runId`, child run id, `agentId`, `agentType`, background flag, and allowed tool count.

- [ ] **Step 8: Run Rust verification**

Run:

```bash
cd src-tauri && cargo check
cd src-tauri && cargo test --test diagnostics_logging_test
```

Expected: PASS. Do not run broad filtered `cargo test <filter>` that compiles every integration test binary unless needed.

- [ ] **Step 9: Commit Task 6**

```bash
git add src-tauri/src/telemetry.rs src-tauri/src/runtime/event_bus.rs src-tauri/src/transport/tauri_runtime_host.rs src-tauri/src/commands/chat.rs src-tauri/src/runtime/chat/chat_turn_driver.rs src-tauri/src/runtime/chat/tool_round_driver.rs src-tauri/src/runtime/tools/dispatcher.rs src-tauri/src/runtime/tools/executor.rs src-tauri/src/runtime/interaction/control_plane.rs src-tauri/src/runtime/agent/worker_runtime.rs src-tauri/tests/diagnostics_logging_test.rs
git commit -m "feat(diagnostics): trace backend runtime timeline"
```

---

## Task 7: Diagnostics Event Stream To Frontend

**Files:**
- Modify: `src-tauri/src/telemetry.rs`
- Modify: backend call sites with app handle if needed
- Modify: `src/lib/tauri.ts`
- Modify: `src/hooks/useStreaming.ts` or the top-level hook mounting global listeners
- Test: `src/lib/tauri.events.test.ts`, backend compile checks

- [ ] **Step 1: Decide event emission boundary**

Use this implementation rule:

```text
record_diagnostic(workspace, event) writes JSONL only.
record_diagnostic_and_emit(app_handle, workspace, event) writes JSONL and emits diagnostics:event.
```

This avoids forcing every storage-only test to construct a Tauri app handle.

- [ ] **Step 2: Add backend helper**

In `src-tauri/src/telemetry.rs`, add:

```rust
pub fn record_diagnostic_and_emit<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    workspace: &Path,
    event: DiagnosticEvent,
) {
    let payload = match serde_json::to_value(&event) {
        Ok(value) => value,
        Err(err) => {
            log::warn!("[telemetry] Failed to serialize diagnostic event: {}", err);
            serde_json::Value::Null
        }
    };
    record_diagnostic(workspace, event);
    if !payload.is_null() {
        if let Err(err) = app.emit("diagnostics:event", payload) {
            log::warn!("[telemetry] Failed to emit diagnostic event: {}", err);
        }
    }
}
```

Add `use tauri::Emitter;` at the top of the file.

- [ ] **Step 3: Use emit helper where app handle exists**

Replace `record_diagnostic` with `record_diagnostic_and_emit` in command/runtime locations that already have `app_handle` or `AppHandle`. Keep pure lower-level modules on write-only `record_diagnostic` if adding app handle would pollute interfaces.

- [ ] **Step 4: Add frontend event type and listener**

In `src/lib/tauri.ts`, add:

```ts
export type DiagnosticsEventPayload = import('@/lib/diagnostics').DiagnosticEvent

export function onDiagnosticsEvent(callback: (payload: DiagnosticsEventPayload) => void) {
  return listen<DiagnosticsEventPayload>(TAURI_EVENTS.DIAGNOSTICS_EVENT, (event) => callback(event.payload))
}
```

In the top-level hook that is mounted once, register `onDiagnosticsEvent` and append backend events to `useDiagnosticsStore`:

```ts
onDiagnosticsEvent((payload) => {
  useDiagnosticsStore.getState().appendDiagnostic(payload)
})
```

Use `src/hooks/useStreaming.ts` if it is already mounted once for app lifetime; otherwise use the actual top-level event listener hook.

- [ ] **Step 5: Run checks**

Run:

```bash
pnpm vitest run src/lib/tauri.events.test.ts src/lib/diagnostics.test.ts src/stores/diagnosticsStore.test.ts
cd src-tauri && cargo check
```

Expected: PASS.

- [ ] **Step 6: Commit Task 7**

```bash
git add src-tauri/src/telemetry.rs src/lib/tauri.ts src/hooks/useStreaming.ts
git commit -m "feat(diagnostics): stream backend diagnostics to frontend"
```

---

## Task 8: End-To-End Verification And Query Examples

**Files:**
- Modify: `docs/superpowers/plans/2026-04-25-ai-diagnostics-logging.md` only if verification discoveries require plan notes
- No production files unless tests reveal defects

- [ ] **Step 1: Run frontend tests**

Run:

```bash
pnpm vitest run src/lib/diagnostics.test.ts src/stores/diagnosticsStore.test.ts src/lib/tauri.diagnostics.test.ts src/hooks/useTauriEvent.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts src/stores/streamingStore.test.ts
```

Expected: PASS.

- [ ] **Step 2: Run frontend build or typecheck**

Run:

```bash
pnpm build
```

Expected: PASS.

- [ ] **Step 3: Run backend checks**

Run:

```bash
cd src-tauri && cargo check
cd src-tauri && cargo test telemetry::tests --lib
cd src-tauri && cargo test --test diagnostics_logging_test
```

Expected: PASS.

- [ ] **Step 4: Manually inspect generated JSONL shape**

Run a local dev flow that records at least one frontend diagnostic event, then inspect metrics JSONL. If the app workspace path is not obvious, use existing metrics export UI/command first. Once a metrics file is available, run:

```bash
rg '"category":"diagnostics"' logs/metrics.jsonl | head -20
jq -c 'select(.category=="diagnostics") | {ts,seq,source,event,conversationId,runId,durationMs,ok,error}' logs/metrics.jsonl | head -20
```

Expected: Lines contain top-level `ts`, `seq`, `category`, `source`, `event`; no nested-only `fields` for diagnostics lines.

- [ ] **Step 5: Verify secret redaction**

Run:

```bash
rg 'Bearer |sk-|password|authorization' logs/metrics.jsonl
```

Expected: no unredacted obvious secret values from diagnostics payloads. Existing non-diagnostics metrics are outside this task.

- [ ] **Step 6: Commit final verification notes if any docs changed**

If no docs changed, do not commit. If docs changed:

```bash
git add docs/superpowers/plans/2026-04-25-ai-diagnostics-logging.md
git commit -m "docs(diagnostics): document verification queries"
```

---

## Self-Review

### Spec Coverage

- Flat JSONL query contract: covered by Task 1 tests and schema.
- Reuse `metrics.jsonl`: covered by Task 1 and Task 2.
- No duplicate `localTime`: schema uses `ts` only.
- Frontend can query/store diagnostics: covered by Task 3 and Task 7.
- Frontend action/IPC/event/store/streaming instrumentation: covered by Task 4 and Task 5.
- Backend command/turn/LLM/tool/permission/interaction/subagent/event/storage-adjacent instrumentation: covered by Task 6.
- High-volume logs: design does not disable by default; delta text remains metadata-first to avoid accidental full output capture unless a later requirement explicitly changes it.
- Pipe-friendly querying: query examples included and verified in Task 8.
- Secret redaction: covered by Task 1 and Task 3 tests.

### Placeholder Scan

No `TBD`, `TODO`, `implement later`, or unspecified test instructions remain. Some steps instruct the implementer to use actual local variable names after opening files; this is intentional because the plan must preserve existing code semantics and avoid inventing identifiers in large existing functions.

### Type Consistency

- `DiagnosticEvent` top-level keys are consistent across TypeScript and Rust.
- `category` is always `diagnostics` for diagnostics records.
- Raw diagnostics lines are plain compact JSON without the legacy completion marker; telemetry readers accept both plain and marker-suffixed historical lines.
- `source` uses `frontend` or `backend` only.
- Event names use fixed dot-separated strings.
- `runId`, `conversationId`, `toolCallId`, `interactionId`, and `clientMessageId` use the same camelCase spelling across logs, tests, and query examples.
- Existing `MetricsEntry { timestamp, category, fields }` remains compatible for non-diagnostics metrics.

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-04-25-ai-diagnostics-logging.md`.

Two execution options:

1. **Subagent-Driven (recommended)** - Dispatch a fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints.
