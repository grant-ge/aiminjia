# V2 Gateway Observability and Error Semantics Design

## Scope

This design covers the first stabilization phase for the desktop v2 gateway path.

Included:

- Preserve tool error status from runtime execution into v2 canonical history.
- Extend desktop client metadata with optional structured fields.
- Add gateway stream lifecycle events that distinguish protocol completion from stream closure.
- Attach request, route, and trace identifiers consistently across gateway log events.

Deferred:

- Tool governance metadata in model-visible tool schemas (`kind`, `capability_scope`, read-only, destructive).
- File and binary artifact metadata in tool results.
- A breaking v2 request schema version change.

The phase is intentionally narrow. It improves correctness and debuggability without requiring server-side behavioral changes.

## Current State

The observed session `9599c06e-e9d7-4f15-bd88-7692976fd4e9` sent valid v2 requests to `/aijia/v2/ai/responses`. All requests used `schema_version=aijia.ai.response.v1`, received HTTP 200, emitted `response.completed`, and routed to `deepseek / anthropic-messages / deepseek-v4-pro`.

The gaps are structural:

- `RuntimeToolCallOutcome` already carries `is_error`, but `ChatMessage` does not. The v2 canonical adapter therefore serializes every tool result as `is_error=false`.
- `ClientInfo` only carries `name`, `version`, and `platform`.
- Gateway logs have request/status/route/chunk rows, but lifecycle closure is incomplete and identifiers are not repeated consistently enough for one-line correlation.
- `response.completed` is a provider protocol event, while `gateway.stream_end` only records byte-stream EOF. Logs need both concepts.

## Design Goals

1. Keep the v2 request backward compatible for the server.
2. Preserve runtime truth instead of inferring it at the gateway edge.
3. Make local gateway logs self-correlating by `request_id`, `run_id`, `trace_id`, and `gateway_request_id`.
4. Make stream lifecycle explicit enough to debug completed, dropped, cancelled, and errored streams.
5. Avoid broad tool-catalog or artifact-result refactors in this phase.

## Non-Goals

- Do not change the model-visible tool call names or argument schema.
- Do not introduce a new `schema_version`.
- Do not move authentication or authorization decisions into client-reported metadata.
- Do not encode tenant/user IDs in request bodies as trusted identity.
- Do not fix binary tool result representation in this phase.

## 1. Tool Error Status Preservation

### Problem

`RuntimeToolCallOutcome::Completed` has `is_error`, and `tool_result_collector` emits JSON with `isError`. When those messages become `ChatMessage`, the field is not represented. `aijia_gateway_v2::to_canonical_message` then hardcodes `is_error=false`.

### Design

Extend `llm::streaming::ChatMessage` with:

```rust
#[serde(default, skip_serializing_if = "is_false")]
pub is_error: bool,
```

Add helpers:

```rust
pub fn tool_result(tool_call_id: &str, tool_name: &str, content: String) -> Self;
pub fn tool_result_with_status(
    tool_call_id: &str,
    tool_name: &str,
    content: String,
    is_error: bool,
) -> Self;
```

The existing `tool_result` remains success-default for compatibility. Callers with known status use `tool_result_with_status`.

`aijia_gateway_v2::to_canonical_message` maps:

```rust
is_error: message.is_error
```

instead of hardcoding false.

### Affected Producers

Update producers that synthesize tool result `ChatMessage`s:

- Main chat tool result ingestion from JSON messages.
- Subagent worker runtime.
- Teammate idle runtime.
- Synthetic blocked, permission-denied, cancelled, or auto-denied tool results.

The conversion rule is simple:

- runtime success: `is_error=false`
- runtime execution error: `is_error=true`
- blocked tool: `is_error=true`
- permission denied / Ask auto-denied: `is_error=true`
- user interaction required without execution failure: preserve existing behavior unless the caller currently treats it as error

## 2. Client Metadata Extension

### Problem

The current v2 request only reports:

```json
{"name":"aijia-desktop","version":"...","platform":"aarch64"}
```

This is insufficient for diagnostics, route analysis, and desktop rollout visibility.

### Design

Extend `ClientInfo` with optional fields:

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

Field semantics:

- `os`: `std::env::consts::OS`.
- `arch`: `std::env::consts::ARCH`.
- `platform`: keep existing value for backward compatibility.
- `locale`: best-effort process or app locale. If not available, omit.
- `timezone`: best-effort local timezone. If not available, omit.
- `device_id_hash`: stable non-reversible hash if an existing device identifier is available. If not, omit.
- `scope_key_hash`: stable non-reversible hash of the local user scope key if available. If not, omit.

Identity rule:

Client metadata is observability-only. The server must continue to derive tenant/user identity from the bearer session token. If server-side validation is later added, it should compare metadata-derived hints against token claims rather than trusting the body.

### Rollout

All fields are optional and skip serialization when unavailable. This keeps old requests valid and avoids requiring every call site to resolve scope/device metadata immediately.

## 3. Gateway Lifecycle Logging

### Problem

The existing log rows show request, HTTP status, route, and response chunks. `response.completed` is present when the provider protocol finishes. `gateway.stream_end` is recorded only when the byte stream reaches EOF with an empty buffer. A completed response can therefore lack a stream closure row.

### Design

Add explicit lifecycle events:

- `gateway.stream_started`: emitted after successful HTTP status and before consuming SSE bytes.
- `gateway.first_event`: emitted once when the first SSE event frame is parsed.
- `gateway.response_completed`: emitted when an SSE `response.completed` frame is parsed.
- `gateway.stream_closed`: emitted exactly once when the stream wrapper is dropped or reaches terminal state.

`gateway.stream_closed.reason` values:

- `eof`: byte stream ended normally.
- `response_completed`: protocol completed and stream was subsequently closed.
- `client_cancelled`: local cancellation closed the stream before protocol completion.
- `dropped`: stream dropped without protocol completion and without a known cancellation signal.
- `error`: network or parser error.

The old `gateway.stream_end` can remain as a compatibility event for EOF, but new diagnostics should read `gateway.stream_closed`.

### State Model

Introduce an internal stream log state:

```rust
struct GatewayStreamLogState {
    request_id: String,
    first_event_seen: bool,
    response_completed: bool,
    stream_closed: bool,
    last_error: Option<String>,
}
```

The parser records transitions through helper methods so closure cannot be missed. A drop guard is acceptable if it can avoid duplicate close rows.

## 4. Request, Route, and Trace Correlation

### Problem

Gateway rows can be correlated by `request_id`, but important fields are distributed across rows. A support/debugging query should not need to manually join request, status, and route rows to answer which run and upstream route a row belongs to.

### Design

Introduce a `GatewayLogContext` stored in memory by `request_id`:

```rust
struct GatewayLogContext {
    request_id: String,
    provider: String,
    url: String,
    conversation_id: Option<String>,
    run_id: Option<String>,
    trace_id: Option<String>,
    gateway_request_id: Option<String>,
    response_id: Option<String>,
    route: Option<GatewayRouteSummary>,
}

struct GatewayRouteSummary {
    logical_model: Option<String>,
    provider: Option<String>,
    api: Option<String>,
    model: Option<String>,
    endpoint_id: Option<i64>,
}
```

Every gateway event should include available correlation fields:

- `request_id`
- `conversation_id`
- `run_id`
- `trace_id`
- `gateway_request_id`
- `response_id`
- `logical_model`
- `route_provider`
- `route_api`
- `route_model`
- `endpoint_id`

Use `route_provider` rather than `provider` on enriched rows to avoid collision with the desktop provider name (`aijia-v2`).

### Storage and Cleanup

The log context can be a process-local `DashMap` or `Mutex<HashMap<...>>`. Insert on `record_request`, update on `record_response_status` and `record_route`, remove after `gateway.stream_closed`.

If stream closure is missed, stale contexts should not grow unbounded. Add a lightweight cleanup strategy:

- remove on `stream_closed`
- remove on non-2xx `response_body`
- optionally evict contexts older than 30 minutes on insert

## Compatibility

Server compatibility:

- The request body keeps `schema_version=aijia.ai.response.v1`.
- New `client` fields are optional.
- `CanonicalMessage.is_error` already exists in canonical schema, but now gets correct values.
- No change to tool definitions in this phase.

Local compatibility:

- Existing persisted messages without `isError` deserialize as `false`.
- Existing tests using `ChatMessage::tool_result` continue to pass after helper initialization updates.
- Existing log readers continue to see old event rows; new rows are additive.

## Testing

Required tests:

1. `ChatMessage` deserializes missing `isError` as false.
2. `ChatMessage::tool_result_with_status(..., true)` serializes and converts to canonical `is_error=true`.
3. `tool_result_collector` output with `isError=true` survives the JSON-to-`ChatMessage` path used by the LLM executor.
4. Subagent and teammate blocked/error tool results append `ChatMessage` with `is_error=true`.
5. `ClientInfo` serializes old required fields and omits unavailable optional fields.
6. `ClientInfo` includes `os` and `arch` when built by the v2 gateway provider.
7. Parsing an SSE frame with `response.completed` records `gateway.response_completed`.
8. A normal stream records one `gateway.stream_closed` row.
9. A stream that errors records `gateway.stream_closed.reason=error`.
10. `gateway.route` and later chunk/closed rows include `run_id`, `trace_id`, and route summary fields when known.

## Rollout Plan

Phase 1:

- Implement `ChatMessage.is_error` and v2 canonical propagation.
- Update main chat, subagent, and teammate tool-result producers.
- Add tests for success, error, blocked, and permission-denied paths.

Phase 2:

- Extend `ClientInfo` with optional metadata.
- Populate `os` and `arch` immediately.
- Populate `locale`, `timezone`, `device_id_hash`, and `scope_key_hash` only when existing services already expose the values without broad dependency plumbing.

Phase 3:

- Add `GatewayLogContext` and enriched gateway log rows.
- Add lifecycle events and stream-close state.
- Keep old log events for compatibility.

Phase 4:

- Run a local v2 gateway smoke and inspect one session log.
- Confirm every request has status, route, response completion, and stream closure.
- Confirm tool error rows preserve `is_error=true` in the outbound v2 request on the following iteration.

## Open Decisions

1. Whether `device_id_hash` and `scope_key_hash` should be populated in this phase or left as explicit `None` until existing identity helpers are identified.
2. Whether `gateway.stream_end` should remain permanently as an alias for EOF or be documented as deprecated after `gateway.stream_closed` lands.
3. Whether `client.timezone` should use IANA names only or allow platform-local fallback strings.

The implementation can proceed without resolving these decisions by treating the fields as optional and additive.
