# Human Interaction Priority And Permission Group Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `permissionAsk` and `AskUserQuestion` share one human-interaction state machine so pending messages, natural-language replies, batch permissions, App cards, and every IM channel behave consistently.

**Architecture:** Keep the existing runtime control planes, but put a `HumanInteractionRegistry` and router in front of IM/App user input. A live human interaction owns the next user message before busy queue or new-turn logic; permission asks are grouped by run and risk key so one user decision can resolve all covered approvals. LLM judge becomes a schema-producing fallback only after deterministic routing cannot decide.

**Tech Stack:** Rust/Tauri backend, existing `RuntimeRunRegistry`, `PendingInteractionControlPlane`, `PendingPermissionControlPlane`, `PendingQueueManager`, shared IM coordinator, React permission/ask dialogs, Cargo unit tests, Vitest component tests.

---

## Scope

- Worktree: `/Users/oayzz/.codex/worktrees/9a36/lotus-app`.
- Source spec: `docs/superpowers/specs/2026-06-09-human-interaction-priority-and-permission-group-design.md`.
- Included: permissionAsk, AskUserQuestion, late-registration drain, busy queue boundary, permission grouping, batch approval, App/IM output parity.
- Included: DingTalk, Feishu, WeCom, WeChat, Telegram, WhatsApp through shared IM code.
- Excluded for now: LLM judge loading card UI. Keep the hook possible, but do not implement the loading card in this plan.
- Excluded: rewriting permission profile storage format.
- Excluded: deleting `/approve` and `/answer` compatibility commands.

## File Structure

Create:

- `src-tauri/src/runtime/human_interaction/registry.rs` — session/run scoped live interaction registry plus early-message buffer.
- `src-tauri/src/runtime/human_interaction/permission_group.rs` — permission grouping, coverage checks, and fan-out resolution helpers.
- `src-tauri/src/runtime/human_interaction/judge_schema.rs` — validated LLM judge schema types.
- `src-tauri/src/runtime/human_interaction/registry_tests.rs` — registry and late-registration unit tests.
- `src-tauri/src/runtime/human_interaction/permission_group_tests.rs` — permission grouping and fan-out unit tests.

Modify:

- `src-tauri/src/runtime/human_interaction/mod.rs` — export new registry, permission group, judge schema modules.
- `src-tauri/src/runtime/human_interaction/types.rs` — add interaction status/group ids where missing.
- `src-tauri/src/runtime/human_interaction/router.rs` — make local deterministic router authoritative before LLM judge.
- `src-tauri/src/runtime/human_interaction/control_plane.rs` — expose live interaction lookup and batch resolve adapters.
- `src-tauri/src/runtime/pending/types.rs` — keep `HeldForHumanInteraction`, add enough metadata for early drain if missing.
- `src-tauri/src/runtime/pending/queue_manager.rs` — route suspended input to interaction registry, not ordinary pending queue.
- `src-tauri/src/runtime/pending/queue_manager_test.rs` — prove suspended input is held for live interaction and running input still queues.
- `src-tauri/src/runtime/run_registry.rs` — use existing `SuspendedForHuman` APIs; only adjust if tests expose missing state.
- `src-tauri/src/runtime/store/pending_permission_request_store.rs` — retain single request APIs and add group list/resolve helper data if needed.
- `src-tauri/src/connector/im/shared/ask_coordinator.rs` — replace session single-slot pending handling with registry-backed routing.
- `src-tauri/src/connector/im/shared/reply_manager.rs` — ensure only run-origin IM outputs are mirrored back to IM.
- `src-tauri/src/connector/im/manager.rs` — make every IM platform call the same coordinator entry point.
- `src-tauri/src/transport/tauri_commands/chat.rs` — add group approval command while keeping single approval command.
- `src/components/common/PermissionAskDialog.tsx` — render permission group as one card/dialog.
- `src/components/common/PermissionAskDialog.test.tsx` — group-card and no-duplicate regression tests.
- `src/components/interactions/AskUserQuestionDialog.tsx` — only touch if shared route requires answer metadata.

## Invariants

- If `HumanInteractionRegistry` has a live interaction for a session, the next user message is first interpreted against that interaction.
- `SuspendedForHuman` is not `busy`; it means “waiting for a user message”.
- A message can be consumed exactly once: resolve interaction, abandon and start new turn, clarify, or queue while running.
- LLM judge cannot directly write permission files, resolve tools, or send free-form confirmations. It returns a schema; program code executes.
- Multiple permission asks in the same run and same risk group are displayed as one interaction group and can be resolved together.
- App and IM use the same resolution path. UI clicks and natural-language replies only differ in input surface, not business logic.

---

### Task 0: Baseline Audit And Commit Boundary

**Files:**
- Read: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Read: `src-tauri/src/runtime/human_interaction/router.rs`
- Read: `src-tauri/src/runtime/pending/queue_manager.rs`
- Read: `src-tauri/src/transport/tauri_commands/chat.rs`
- Create: `docs/superpowers/plans/2026-06-09-human-interaction-priority-baseline.md`

- [ ] **Step 1: Capture current dirty state**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app
git status --short
git diff -- src-tauri/src/runtime/human_interaction \
  src-tauri/src/connector/im/shared/ask_coordinator.rs \
  src-tauri/src/runtime/pending/queue_manager.rs \
  src-tauri/src/transport/tauri_commands/chat.rs \
  src/components/common/PermissionAskDialog.tsx
```

Expected: existing experimental changes are visible. Do not revert them.

- [ ] **Step 2: Record what must be preserved**

Create `docs/superpowers/plans/2026-06-09-human-interaction-priority-baseline.md`:

```markdown
# Human Interaction Priority Baseline

## Preserve

- Existing `SuspendedForHuman` run-registry semantics.
- Existing single permission request API compatibility.
- Existing `/approve` and `/answer` command parsing tests.
- Existing run-scoped IM output binding work.

## Replace

- `IMAskCoordinator` single `session_id -> PendingAsk` slot.
- Any permission reply path where LLM judge can answer in prose without resolving code state.
- Any late AskUserQuestion path where a queued IM message waits for the next user message before draining.
- Any App/IM divergence where one side shows a permission card and the other side receives unrelated final output.

## Manual Scenarios To Re-test

- User says `问我三个问题` while permission is pending.
- User says `好的，先拒绝吧` while permission is pending.
- User sends `好了没啊` before AskUserQuestion card arrives.
- Two permission asks arrive for the same run and same directory.
```

- [ ] **Step 3: Commit baseline note only**

Run:

```bash
git add docs/superpowers/plans/2026-06-09-human-interaction-priority-baseline.md
git commit -m "docs: capture human interaction priority baseline"
```

Expected: commit contains only the baseline note.

---

### Task 1: Build HumanInteractionRegistry

**Files:**
- Create: `src-tauri/src/runtime/human_interaction/registry.rs`
- Create: `src-tauri/src/runtime/human_interaction/registry_tests.rs`
- Modify: `src-tauri/src/runtime/human_interaction/mod.rs`
- Modify: `src-tauri/src/runtime/human_interaction/types.rs`

- [ ] **Step 1: Write failing registry tests**

Create `src-tauri/src/runtime/human_interaction/registry_tests.rs`:

```rust
use crate::runtime::human_interaction::{
    HumanInteractionId, HumanInteractionKind, HumanInteractionRef, HumanInteractionRegistry,
    HumanInteractionStatus, ImPlatform, InboundUserMessage, OutputBinding, TurnOrigin,
};
use crate::runtime::ids::{RunId, SessionId, ToolCallId};

fn interaction(id: &str, run: &str, kind: HumanInteractionKind) -> HumanInteractionRef {
    HumanInteractionRef {
        id: HumanInteractionId::new(id),
        session_id: SessionId::new("sess-1"),
        run_id: RunId::new(run),
        tool_call_id: ToolCallId::new(format!("tool-{id}")),
        kind,
        turn_origin: TurnOrigin::App,
        output_binding: OutputBinding::AppOnly,
        status: HumanInteractionStatus::Pending,
    }
}

#[test]
fn latest_live_interaction_owns_session_input() {
    let registry = HumanInteractionRegistry::default();
    registry.register(interaction("permission-1", "run-1", HumanInteractionKind::PermissionAsk));
    registry.register(interaction("ask-1", "run-1", HumanInteractionKind::AskUserQuestion));

    let live = registry.latest_live_for_session("sess-1").expect("live interaction");

    assert_eq!(live.id.as_str(), "ask-1");
    assert_eq!(live.kind, HumanInteractionKind::AskUserQuestion);
}

#[test]
fn resolved_interaction_cannot_consume_later_input() {
    let registry = HumanInteractionRegistry::default();
    registry.register(interaction("permission-1", "run-1", HumanInteractionKind::PermissionAsk));
    registry.mark_resolved(&HumanInteractionId::new("permission-1"));

    assert!(registry.latest_live_for_session("sess-1").is_none());
}

#[test]
fn early_messages_are_drained_when_interaction_registers() {
    let registry = HumanInteractionRegistry::default();
    registry.buffer_early_message(InboundUserMessage::im_text(
        "sess-1",
        ImPlatform::Dingtalk,
        "conv-1",
        "好了没啊",
    ));

    let drained = registry.register_and_drain(interaction(
        "ask-1",
        "run-1",
        HumanInteractionKind::AskUserQuestion,
    ));

    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].content, "好了没啊");
    assert!(registry.take_early_messages("sess-1").is_empty());
}
```

- [ ] **Step 2: Run failing registry tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction::registry_tests -- --nocapture
```

Expected: FAIL because `HumanInteractionRegistry` and `InboundUserMessage` do not exist or are incomplete.

- [ ] **Step 3: Implement registry and inbound message types**

Add to `src-tauri/src/runtime/human_interaction/types.rs`:

```rust
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundUserMessage {
    pub session_id: SessionId,
    pub turn_origin: TurnOrigin,
    pub output_binding: OutputBinding,
    pub content: String,
    pub received_at_ms: i64,
}

impl InboundUserMessage {
    pub fn app_text(session_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            session_id: SessionId::new(session_id.into()),
            turn_origin: TurnOrigin::App,
            output_binding: OutputBinding::AppOnly,
            content: content.into(),
            received_at_ms: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn im_text(
        session_id: impl Into<String>,
        platform: ImPlatform,
        external_conversation_key: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let session_id = session_id.into();
        let external_conversation_key = external_conversation_key.into();
        Self {
            session_id: SessionId::new(session_id.clone()),
            turn_origin: TurnOrigin::Im {
                platform,
                external_conversation_key: external_conversation_key.clone(),
                sender_id: None,
                sender_label: None,
                account_id: None,
                thread_id: None,
            },
            output_binding: OutputBinding::im(platform, session_id, external_conversation_key, true),
            content: content.into(),
            received_at_ms: chrono::Utc::now().timestamp_millis(),
        }
    }
}
```

Create `src-tauri/src/runtime/human_interaction/registry.rs`:

```rust
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::{HumanInteractionId, HumanInteractionRef, HumanInteractionStatus, InboundUserMessage};

#[derive(Clone, Default)]
pub struct HumanInteractionRegistry {
    inner: Arc<Mutex<RegistryState>>,
}

#[derive(Default)]
struct RegistryState {
    live: HashMap<String, Vec<HumanInteractionRef>>,
    early: HashMap<String, Vec<InboundUserMessage>>,
}

impl HumanInteractionRegistry {
    pub fn register(&self, interaction: HumanInteractionRef) {
        let mut guard = self.inner.lock().expect("registry lock");
        guard
            .live
            .entry(interaction.session_id.as_str().to_string())
            .or_default()
            .push(interaction);
    }

    pub fn register_and_drain(&self, interaction: HumanInteractionRef) -> Vec<InboundUserMessage> {
        let session = interaction.session_id.as_str().to_string();
        let mut guard = self.inner.lock().expect("registry lock");
        guard.live.entry(session.clone()).or_default().push(interaction);
        guard.early.remove(&session).unwrap_or_default()
    }

    pub fn latest_live_for_session(&self, session_id: &str) -> Option<HumanInteractionRef> {
        let guard = self.inner.lock().expect("registry lock");
        guard.live.get(session_id).and_then(|items| {
            items
                .iter()
                .rev()
                .find(|item| item.status == HumanInteractionStatus::Pending)
                .cloned()
        })
    }

    pub fn mark_resolved(&self, interaction_id: &HumanInteractionId) {
        self.mark_status(interaction_id, HumanInteractionStatus::Resolved);
    }

    pub fn mark_cancelled(&self, interaction_id: &HumanInteractionId) {
        self.mark_status(interaction_id, HumanInteractionStatus::Cancelled);
    }

    pub fn mark_abandoned(&self, interaction_id: &HumanInteractionId) {
        self.mark_status(interaction_id, HumanInteractionStatus::Abandoned);
    }

    pub fn buffer_early_message(&self, message: InboundUserMessage) {
        let mut guard = self.inner.lock().expect("registry lock");
        guard
            .early
            .entry(message.session_id.as_str().to_string())
            .or_default()
            .push(message);
    }

    pub fn take_early_messages(&self, session_id: &str) -> Vec<InboundUserMessage> {
        let mut guard = self.inner.lock().expect("registry lock");
        guard.early.remove(session_id).unwrap_or_default()
    }

    fn mark_status(&self, interaction_id: &HumanInteractionId, status: HumanInteractionStatus) {
        let mut guard = self.inner.lock().expect("registry lock");
        for items in guard.live.values_mut() {
            for item in items.iter_mut() {
                if item.id.as_str() == interaction_id.as_str() {
                    item.status = status;
                }
            }
        }
    }
}
```

Update `src-tauri/src/runtime/human_interaction/mod.rs`:

```rust
pub mod control_plane;
pub mod output_binding;
pub mod registry;
pub mod router;
pub mod types;

#[cfg(test)]
mod registry_tests;
#[cfg(test)]
mod tests;

pub use control_plane::*;
pub use output_binding::*;
pub use registry::*;
pub use router::*;
pub use types::*;
```

- [ ] **Step 4: Run registry tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction::registry_tests -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit registry**

Run:

```bash
git add src-tauri/src/runtime/human_interaction
git commit -m "feat: add human interaction registry"
```

---

### Task 2: Make Router Own Interpretation Before LLM Judge

**Files:**
- Modify: `src-tauri/src/runtime/human_interaction/router.rs`
- Modify: `src-tauri/src/runtime/human_interaction/judge_schema.rs`
- Modify: `src-tauri/src/runtime/human_interaction/mod.rs`
- Test: `src-tauri/src/runtime/human_interaction/tests.rs`

- [ ] **Step 1: Add router regression tests**

Append to `src-tauri/src/runtime/human_interaction/tests.rs`:

```rust
use crate::runtime::human_interaction::{
    AskQuestionSpec, HumanInteractionRouter, HumanReplyRoute, PermissionAskSpec,
    PermissionDecisionIntent,
};

#[test]
fn permission_reply_explicit_deny_is_not_llm_judge_work() {
    let route = HumanInteractionRouter::route_permission_reply(
        &ask_ref(HumanInteractionKind::PermissionAsk),
        &PermissionAskSpec {
            tool_name: "Read".into(),
            requested_path: Some("/private/tmp/aijia-permission-test/secret3.txt".into()),
            current_scope: None,
        },
        "好的，先拒绝吧",
    );

    assert_eq!(
        route,
        HumanReplyRoute::ResolvePermission {
            intent: PermissionDecisionIntent::Deny { reason: None }
        }
    );
}

#[test]
fn permission_reply_new_topic_abandons_permission_and_starts_new_turn() {
    let route = HumanInteractionRouter::route_permission_reply(
        &ask_ref(HumanInteractionKind::PermissionAsk),
        &PermissionAskSpec {
            tool_name: "Read".into(),
            requested_path: Some("/private/tmp/aijia-permission-test/secret3.txt".into()),
            current_scope: None,
        },
        "问我三个问题",
    );

    match route {
        HumanReplyRoute::AbandonAndStartNewTurn { text, .. } => assert_eq!(text, "问我三个问题"),
        other => panic!("expected abandon route, got {other:?}"),
    }
}

#[test]
fn ask_user_question_plain_answer_is_submitted_directly() {
    let route = HumanInteractionRouter::route_ask_user_question(
        &ask_ref(HumanInteractionKind::AskUserQuestion),
        &AskQuestionSpec {
            questions: vec!["专业领域".into(), "输出风格".into()],
        },
        "HR/人事\n结论优先",
    );

    match route {
        HumanReplyRoute::ResolveAskUserQuestion { answers, raw_text } => {
            assert_eq!(raw_text, "HR/人事\n结论优先");
            assert_eq!(answers["专业领域"], "HR/人事");
            assert_eq!(answers["输出风格"], "结论优先");
        }
        other => panic!("expected ask-user-question resolution, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run failing router tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction -- --nocapture
```

Expected: FAIL if current router still treats explicit deny/new topic ambiguously or test helpers need adjustment.

- [ ] **Step 3: Add judge schema but keep it fallback-only**

Create `src-tauri/src/runtime/human_interaction/judge_schema.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeAction {
    Resolve,
    AbandonNewTurn,
    Clarify,
    NotForInteraction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeKind {
    Permission,
    AskUserQuestion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanInteractionJudgeDecision {
    pub action: JudgeAction,
    pub kind: JudgeKind,
    #[serde(default)]
    pub payload: serde_json::Value,
    pub reason: String,
}

impl HumanInteractionJudgeDecision {
    pub fn parse_json(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }
}
```

Update `src-tauri/src/runtime/human_interaction/mod.rs`:

```rust
pub mod judge_schema;
pub use judge_schema::*;
```

- [ ] **Step 4: Tighten deterministic router**

In `src-tauri/src/runtime/human_interaction/router.rs`, ensure the permission order is:

```rust
if is_topic_change(trimmed) {
    return HumanReplyRoute::AbandonAndStartNewTurn {
        reason: "user changed topic while permission was pending".into(),
        text: trimmed.into(),
    };
}
if contains_any(trimmed, &["拒绝", "不允许", "先拒绝", "不行", "deny"]) {
    return HumanReplyRoute::ResolvePermission {
        intent: PermissionDecisionIntent::Deny { reason: None },
    };
}
if contains_any(trimmed, &["取消", "算了", "不用了", "cancel"]) {
    return HumanReplyRoute::ResolvePermission {
        intent: PermissionDecisionIntent::Cancel { reason: None },
    };
}
if contains_any(trimmed, &["以后", "永久", "都可以", "都允许", "always"]) {
    return HumanReplyRoute::ResolvePermission {
        intent: PermissionDecisionIntent::AllowAlways {
            scope: extract_path_like_scope(trimmed),
        },
    };
}
if contains_any(trimmed, &["允许", "可以", "同意", "好的", "行", "allow"]) {
    return HumanReplyRoute::ResolvePermission {
        intent: PermissionDecisionIntent::AllowOnce,
    };
}
```

- [ ] **Step 5: Run router tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit router**

Run:

```bash
git add src-tauri/src/runtime/human_interaction
git commit -m "feat: prioritize deterministic human interaction routing"
```

---

### Task 3: Replace IMAskCoordinator Single Slot With Registry Routing

**Files:**
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Modify: `src-tauri/src/runtime/human_interaction/control_plane.rs`
- Test: `src-tauri/src/connector/im/shared/ask_coordinator.rs`

- [ ] **Step 1: Add coordinator tests for no-overwrite and exact consumption**

Append tests in `src-tauri/src/connector/im/shared/ask_coordinator.rs` test module:

```rust
#[tokio::test]
async fn two_permission_asks_in_same_session_do_not_overwrite_each_other() {
    let fixture = TestFixture::new();
    fixture.emit_permission("tool-read-1", "Read", "/private/tmp/a.txt").await;
    fixture.emit_permission("tool-read-2", "Read", "/private/tmp/b.txt").await;

    let pending = fixture.coordinator.pending_for_session(&SessionId::new("sess-im"));

    assert_eq!(pending.len(), 2);
    assert!(pending.iter().any(|item| item.tool_call_id.as_str() == "tool-read-1"));
    assert!(pending.iter().any(|item| item.tool_call_id.as_str() == "tool-read-2"));
}

#[tokio::test]
async fn live_permission_reply_is_consumed_once_and_not_queued_again() {
    let fixture = TestFixture::new();
    fixture.emit_permission("tool-read-1", "Read", "/private/tmp/a.txt").await;

    let first = fixture
        .coordinator
        .try_handle_reply(&SessionId::new("sess-im"), "先拒绝".into())
        .await
        .expect("reply handled");

    let second = fixture
        .coordinator
        .try_handle_reply(&SessionId::new("sess-im"), "先拒绝".into())
        .await
        .expect("reply handled");

    assert_eq!(first, HandleOutcome::Consumed);
    assert_eq!(second, HandleOutcome::NotPending);
}
```

If existing fixtures use different helper names, implement equivalent helpers in the test module:

```rust
impl TestFixture {
    async fn emit_permission(&self, tool_call_id: &str, tool_name: &str, path: &str) {
        self.coordinator
            .handle_runtime_event(RuntimeEventKind::PermissionAskRequired {
                tool_call_id: ToolCallId::new(tool_call_id),
                tool_name: tool_name.into(),
                message: format!("path={path}"),
                suggestions: vec!["仅本次允许".into(), "永久允许".into(), "拒绝".into()],
                mode: PermissionAskMode::Read,
                remember_options: Vec::new(),
                capability_scopes: Vec::new(),
                default_destination: None,
                original_request: json!({ "path": path }),
                turn_origin: TurnOrigin::App,
                output_binding: OutputBinding::AppOnly,
                path_auth_scope: Some(PathAuthScope::Path(path.into())),
            })
            .await
            .expect("emit permission");
    }
}
```

- [ ] **Step 2: Run failing coordinator tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::ask_coordinator -- --nocapture
```

Expected: FAIL if current coordinator still stores one pending ask per session or consumes reply twice.

- [ ] **Step 3: Add registry to `IMAskCoordinator`**

In `src-tauri/src/connector/im/shared/ask_coordinator.rs`, change coordinator state from single session slot to registry-backed lookup:

```rust
pub struct IMAskCoordinator {
    pending: Arc<Mutex<HashMap<String, Vec<PendingAsk>>>>,
    human_registry: HumanInteractionRegistry,
    registry: Arc<dyn RunActivityController>,
    sink: Arc<dyn AskOutputSink>,
    permission_cp: Arc<dyn PendingPermissionControlPlane>,
    interaction_cp: Arc<dyn PendingInteractionControlPlane>,
    judge: Arc<dyn AskReplyJudge>,
}
```

Add helper:

```rust
fn push_pending(&self, session_id: &SessionId, ask: PendingAsk) {
    self.pending
        .lock()
        .expect("pending ask lock")
        .entry(session_id.as_str().to_string())
        .or_default()
        .push(ask);
}

fn remove_pending_by_tool_call(&self, session_id: &SessionId, tool_call_id: &ToolCallId) {
    if let Some(items) = self
        .pending
        .lock()
        .expect("pending ask lock")
        .get_mut(session_id.as_str())
    {
        items.retain(|item| item.tool_call_id() != tool_call_id);
    }
}
```

Register every ask into `HumanInteractionRegistry` by building a `HumanInteractionRef` with `run_id`, `session_id`, `tool_call_id`, `kind`, `turn_origin`, and `output_binding`.

- [ ] **Step 4: Rewrite `try_handle_reply` order**

In `try_handle_reply`, the first branch must be:

```rust
let Some(live) = self.human_registry.latest_live_for_session(session_id.as_str()) else {
    tracing::debug!("[im-ask] no live interaction session={}", session_id.as_str());
    return Ok(HandleOutcome::NotPending);
};

let route = match live.kind {
    HumanInteractionKind::PermissionAsk => {
        let spec = self.permission_spec_for(&live)?;
        HumanInteractionRouter::route_permission_reply(&live, &spec, &content)
    }
    HumanInteractionKind::AskUserQuestion => {
        let spec = self.ask_question_spec_for(&live)?;
        HumanInteractionRouter::route_ask_user_question(&live, &spec, &content)
    }
};

match route {
    HumanReplyRoute::ResolvePermission { intent } => {
        self.resolve_permission_route(session_id, &live, intent).await?;
        self.human_registry.mark_resolved(&live.id);
        self.remove_pending_by_tool_call(session_id, &live.tool_call_id);
        Ok(HandleOutcome::Consumed)
    }
    HumanReplyRoute::ResolveAskUserQuestion { answers, raw_text } => {
        self.resolve_ask_question_route(&live, answers, raw_text).await?;
        self.human_registry.mark_resolved(&live.id);
        self.remove_pending_by_tool_call(session_id, &live.tool_call_id);
        Ok(HandleOutcome::Consumed)
    }
    HumanReplyRoute::AbandonAndStartNewTurn { text, .. } => {
        self.abandon_interaction(&live).await?;
        self.human_registry.mark_abandoned(&live.id);
        self.remove_pending_by_tool_call(session_id, &live.tool_call_id);
        Ok(HandleOutcome::Reroute { content: text })
    }
    HumanReplyRoute::Clarify { message } => {
        self.sink.send_text(session_id, message).await?;
        Ok(HandleOutcome::Consumed)
    }
}
```

Only call `judge_permission` or `judge_user_question` inside the `Clarify` branch if local rules produce an explicit `NeedsJudge` state. Do not call judge before local routing.

- [ ] **Step 5: Run coordinator tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::ask_coordinator -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit coordinator registry routing**

Run:

```bash
git add src-tauri/src/connector/im/shared/ask_coordinator.rs src-tauri/src/runtime/human_interaction
git commit -m "feat: route IM replies through human interaction registry"
```

---

### Task 4: Add PermissionInteractionGroup And Batch Resolve

**Files:**
- Create: `src-tauri/src/runtime/human_interaction/permission_group.rs`
- Create: `src-tauri/src/runtime/human_interaction/permission_group_tests.rs`
- Modify: `src-tauri/src/runtime/human_interaction/mod.rs`
- Modify: `src-tauri/src/runtime/store/pending_permission_request_store.rs`
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`

- [ ] **Step 1: Write permission group tests**

Create `src-tauri/src/runtime/human_interaction/permission_group_tests.rs`:

```rust
use crate::runtime::human_interaction::{
    PermissionDecisionIntent, PermissionGroup, PermissionGroupKey, PermissionGroupResolution,
};
use crate::runtime::ids::{RunId, SessionId, ToolCallId};

#[test]
fn same_run_same_tool_same_directory_groups_together() {
    let key = PermissionGroupKey::read_path(
        SessionId::new("sess-1"),
        RunId::new("run-1"),
        "Read",
        "/private/tmp/aijia-permission-test/secret1.txt",
    );
    let mut group = PermissionGroup::new(key);

    group.push_request(ToolCallId::new("tool-1"), "/private/tmp/aijia-permission-test/secret1.txt");
    group.push_request(ToolCallId::new("tool-2"), "/private/tmp/aijia-permission-test/secret2.txt");

    assert_eq!(group.items().len(), 2);
    assert_eq!(group.coverage_scope(), Some("/private/tmp/aijia-permission-test".to_string()));
}

#[test]
fn allow_always_scope_must_cover_every_item_before_batch_resolve() {
    let key = PermissionGroupKey::read_path(
        SessionId::new("sess-1"),
        RunId::new("run-1"),
        "Read",
        "/private/tmp/aijia-permission-test/secret1.txt",
    );
    let mut group = PermissionGroup::new(key);
    group.push_request(ToolCallId::new("tool-1"), "/private/tmp/aijia-permission-test/secret1.txt");
    group.push_request(ToolCallId::new("tool-2"), "/private/tmp/aijia-permission-test/secret2.txt");

    let result = group.resolve(PermissionDecisionIntent::AllowAlways {
        scope: Some("/private/tmp/aijia-permission-test".into()),
    });

    assert_eq!(result, PermissionGroupResolution::ResolveAll);
}

#[test]
fn too_narrow_scope_does_not_batch_resolve() {
    let key = PermissionGroupKey::read_path(
        SessionId::new("sess-1"),
        RunId::new("run-1"),
        "Read",
        "/private/tmp/aijia-permission-test/secret1.txt",
    );
    let mut group = PermissionGroup::new(key);
    group.push_request(ToolCallId::new("tool-1"), "/private/tmp/aijia-permission-test/secret1.txt");
    group.push_request(ToolCallId::new("tool-2"), "/private/tmp/aijia-permission-test/secret2.txt");

    let result = group.resolve(PermissionDecisionIntent::AllowAlways {
        scope: Some("/private/tmp/aijia-permission-test/secret1.txt".into()),
    });

    assert_eq!(
        result,
        PermissionGroupResolution::NeedClarification {
            message: "授权范围没有覆盖全部待审批请求，请选择仅本次、拒绝，或说明包含全部文件的目录范围。".into()
        }
    );
}
```

- [ ] **Step 2: Run failing group tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction::permission_group_tests -- --nocapture
```

Expected: FAIL because permission group types do not exist.

- [ ] **Step 3: Implement permission group module**

Create `src-tauri/src/runtime/human_interaction/permission_group.rs`:

```rust
use std::path::{Path, PathBuf};

use crate::runtime::ids::{RunId, SessionId, ToolCallId};

use super::PermissionDecisionIntent;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PermissionGroupKey {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_name: String,
    pub scope_key: String,
}

impl PermissionGroupKey {
    pub fn read_path(
        session_id: SessionId,
        run_id: RunId,
        tool_name: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        let path = normalize_path(path.as_ref());
        let scope_key = parent_dir(&path).unwrap_or(path);
        Self {
            session_id,
            run_id,
            tool_name: tool_name.into(),
            scope_key,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionGroupItem {
    pub tool_call_id: ToolCallId,
    pub requested_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionGroup {
    key: PermissionGroupKey,
    items: Vec<PermissionGroupItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionGroupResolution {
    ResolveAll,
    ResolveOne(ToolCallId),
    NeedClarification { message: String },
}

impl PermissionGroup {
    pub fn new(key: PermissionGroupKey) -> Self {
        Self {
            key,
            items: Vec::new(),
        }
    }

    pub fn key(&self) -> &PermissionGroupKey {
        &self.key
    }

    pub fn items(&self) -> &[PermissionGroupItem] {
        &self.items
    }

    pub fn push_request(&mut self, tool_call_id: ToolCallId, requested_path: impl AsRef<str>) {
        if self.items.iter().any(|item| item.tool_call_id == tool_call_id) {
            return;
        }
        self.items.push(PermissionGroupItem {
            tool_call_id,
            requested_path: normalize_path(requested_path.as_ref()),
        });
    }

    pub fn coverage_scope(&self) -> Option<String> {
        let mut dirs = self
            .items
            .iter()
            .filter_map(|item| parent_dir(&item.requested_path));
        let first = dirs.next()?;
        if dirs.all(|dir| dir == first) {
            Some(first)
        } else {
            None
        }
    }

    pub fn resolve(&self, intent: PermissionDecisionIntent) -> PermissionGroupResolution {
        match intent {
            PermissionDecisionIntent::AllowOnce
            | PermissionDecisionIntent::Deny { .. }
            | PermissionDecisionIntent::Cancel { .. } => PermissionGroupResolution::ResolveAll,
            PermissionDecisionIntent::AllowAlways { scope } => {
                let Some(scope) = scope.or_else(|| self.coverage_scope()) else {
                    return PermissionGroupResolution::NeedClarification {
                        message: "授权范围没有覆盖全部待审批请求，请选择仅本次、拒绝，或说明包含全部文件的目录范围。".into(),
                    };
                };
                let scope = normalize_path(&scope);
                if self.items.iter().all(|item| path_contains(&scope, &item.requested_path)) {
                    PermissionGroupResolution::ResolveAll
                } else {
                    PermissionGroupResolution::NeedClarification {
                        message: "授权范围没有覆盖全部待审批请求，请选择仅本次、拒绝，或说明包含全部文件的目录范围。".into(),
                    }
                }
            }
        }
    }
}

fn normalize_path(path: &str) -> String {
    if path == "/tmp" {
        return "/private/tmp".into();
    }
    path.strip_prefix("/tmp/")
        .map(|rest| format!("/private/tmp/{rest}"))
        .unwrap_or_else(|| path.trim_end_matches('/').to_string())
}

fn parent_dir(path: &str) -> Option<String> {
    Path::new(path)
        .parent()
        .map(PathBuf::from)
        .map(|path| path.to_string_lossy().trim_end_matches('/').to_string())
}

fn path_contains(scope: &str, path: &str) -> bool {
    path == scope || path.starts_with(&format!("{}/", scope.trim_end_matches('/')))
}
```

Update `src-tauri/src/runtime/human_interaction/mod.rs`:

```rust
pub mod permission_group;

#[cfg(test)]
mod permission_group_tests;

pub use permission_group::*;
```

- [ ] **Step 4: Wire coordinator fan-out**

In `src-tauri/src/connector/im/shared/ask_coordinator.rs`, add group map:

```rust
permission_groups: Arc<Mutex<HashMap<PermissionGroupKey, PermissionGroup>>>,
```

When a `PermissionAskRequired` event arrives:

```rust
let key = PermissionGroupKey::read_path(
    session_id.clone(),
    run_id.clone(),
    tool_name.clone(),
    requested_path.clone(),
);
let group = {
    let mut guard = self.permission_groups.lock().expect("permission groups lock");
    let group = guard.entry(key.clone()).or_insert_with(|| PermissionGroup::new(key));
    group.push_request(tool_call_id.clone(), requested_path.clone());
    group.clone()
};
self.render_permission_group(session_id, &group).await?;
```

When a permission route resolves:

```rust
let group = self.group_for_live_permission(&live)?;
match group.resolve(intent.clone()) {
    PermissionGroupResolution::ResolveAll => {
        for item in group.items() {
            self.resolve_single_permission(&item.tool_call_id, intent.clone()).await?;
            self.remove_pending_by_tool_call(session_id, &item.tool_call_id);
        }
        self.remove_group(group.key());
    }
    PermissionGroupResolution::ResolveOne(tool_call_id) => {
        self.resolve_single_permission(&tool_call_id, intent).await?;
        self.remove_pending_by_tool_call(session_id, &tool_call_id);
    }
    PermissionGroupResolution::NeedClarification { message } => {
        self.sink.send_text(session_id, message).await?;
    }
}
```

- [ ] **Step 5: Run group and coordinator tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction::permission_group_tests -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::ask_coordinator -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit permission grouping**

Run:

```bash
git add src-tauri/src/runtime/human_interaction src-tauri/src/connector/im/shared/ask_coordinator.rs
git commit -m "feat: group related permission approvals"
```

---

### Task 5: Fix Late-Registration Drain And Queue Boundary

**Files:**
- Modify: `src-tauri/src/runtime/pending/queue_manager.rs`
- Modify: `src-tauri/src/runtime/pending/queue_manager_test.rs`
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Modify: `src-tauri/src/connector/im/manager.rs`

- [ ] **Step 1: Add failing queue boundary tests**

Append to `src-tauri/src/runtime/pending/queue_manager_test.rs`:

```rust
#[tokio::test]
async fn suspended_for_human_input_is_not_busy_pending_queue() {
    let fixture = QueueFixture::new();
    fixture
        .run_registry
        .start_run("sess-1", RunId::new("run-1"))
        .expect("start");
    fixture
        .run_registry
        .suspend_for_human("sess-1", "ask-1")
        .expect("suspend");

    let outcome = fixture
        .manager
        .enqueue_or_send(test_pending_item("sess-1", "好了没啊"))
        .await
        .expect("enqueue");

    assert_eq!(
        outcome,
        EnqueueOutcome::HeldForHumanInteraction {
            interaction_id: Some("ask-1".into())
        }
    );
    assert_eq!(fixture.manager.pending_count("sess-1"), 0);
}

#[tokio::test]
async fn running_without_registered_interaction_buffers_early_message() {
    let fixture = QueueFixture::new();
    fixture
        .run_registry
        .start_run("sess-1", RunId::new("run-1"))
        .expect("start");

    let outcome = fixture
        .manager
        .enqueue_or_send(test_pending_item("sess-1", "好了没啊"))
        .await
        .expect("enqueue");

    assert!(matches!(outcome, EnqueueOutcome::Queued { .. }));
}
```

- [ ] **Step 2: Run failing queue tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib runtime::pending::queue_manager_test -- --nocapture
```

Expected: FAIL if suspended input still enters ordinary pending queue or test helpers need adaptation.

- [ ] **Step 3: Drain early messages on interaction registration**

In `src-tauri/src/connector/im/shared/ask_coordinator.rs`, after registering an interaction:

```rust
let drained = self.human_registry.register_and_drain(interaction_ref);
for message in drained {
    let outcome = self.try_handle_reply(&message.session_id, message.content).await?;
    if let HandleOutcome::Reroute { content } = outcome {
        self.dispatch_new_turn(message.with_content(content)).await?;
    }
}
```

If existing `PendingQueueManager` already stores early messages, add:

```rust
let queued = self.pending_queue.take_recent_for_session(session_id.as_str(), Duration::from_millis(1500));
for item in queued {
    self.human_registry.buffer_early_message(InboundUserMessage::from_pending_item(item));
}
```

Do not require a new user message to trigger this drain.

- [ ] **Step 4: Keep ordinary busy queue behavior unchanged**

In `src-tauri/src/runtime/pending/queue_manager.rs`, preserve:

```rust
if self.run_registry.is_session_busy(session_id.as_str()) {
    return self.enqueue_busy_item(item).await;
}
```

but ensure this check is not true for `SuspendedForHuman`, using current `RuntimeRunRegistry::is_session_busy`.

- [ ] **Step 5: Run queue/coordinator tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib runtime::pending::queue_manager_test -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::ask_coordinator -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit queue boundary**

Run:

```bash
git add src-tauri/src/runtime/pending src-tauri/src/connector/im/shared/ask_coordinator.rs src-tauri/src/connector/im/manager.rs
git commit -m "fix: drain early messages into live human interactions"
```

---

### Task 6: Add App Permission Group UI And Commands

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src/components/common/PermissionAskDialog.tsx`
- Modify: `src/components/common/PermissionAskDialog.test.tsx`

- [ ] **Step 1: Add frontend regression tests**

Append to `src/components/common/PermissionAskDialog.test.tsx`:

```tsx
it('renders related permission requests as one grouped dialog', () => {
  render(
    <PermissionAskDialog
      requests={[
        permissionRequest({ toolCallId: 'tool-1', requestedPath: '/private/tmp/a/1.txt' }),
        permissionRequest({ toolCallId: 'tool-2', requestedPath: '/private/tmp/a/2.txt' }),
      ]}
      onApprove={vi.fn()}
      onDeny={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  expect(screen.getByText('需要你确认 2 个权限请求')).toBeInTheDocument();
  expect(screen.getByText('/private/tmp/a')).toBeInTheDocument();
  expect(screen.queryAllByText('仅本次允许')).toHaveLength(1);
});

it('submits one grouped approval action instead of two duplicate cards', async () => {
  const onApprove = vi.fn();
  render(
    <PermissionAskDialog
      requests={[
        permissionRequest({ toolCallId: 'tool-1', requestedPath: '/private/tmp/a/1.txt' }),
        permissionRequest({ toolCallId: 'tool-2', requestedPath: '/private/tmp/a/2.txt' }),
      ]}
      onApprove={onApprove}
      onDeny={vi.fn()}
      onCancel={vi.fn()}
    />,
  );

  await userEvent.click(screen.getByRole('button', { name: '提交' }));

  expect(onApprove).toHaveBeenCalledTimes(1);
  expect(onApprove).toHaveBeenCalledWith({
    toolCallIds: ['tool-1', 'tool-2'],
    remember: false,
    scopeOverride: undefined,
  });
});
```

- [ ] **Step 2: Run failing frontend tests**

Run:

```bash
pnpm exec vitest run src/components/common/PermissionAskDialog.test.tsx
```

Expected: FAIL because dialog still expects a single request or emits separate approvals.

- [ ] **Step 3: Add backend group approval command**

In `src-tauri/src/transport/tauri_commands/chat.rs`, add:

```rust
#[tauri::command]
pub async fn approve_permission_group_request(
    state: tauri::State<'_, ChatState>,
    tool_call_ids: Vec<String>,
    remember: bool,
    path_auth_scope_override: Option<PathAuthScope>,
) -> Result<(), String> {
    for tool_call_id in tool_call_ids {
        state
            .permission_control_plane
            .resolve_pending_request(
                &ToolCallId::new(tool_call_id),
                PendingPermissionResolution::Allow {
                    updated_input: None,
                    remember,
                    destination: None,
                    message: None,
                    path_auth_scope_override: path_auth_scope_override.clone(),
                },
            )
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}
```

Keep existing `approve_permission_request(tool_call_id)` unchanged.

- [ ] **Step 4: Render one grouped App dialog**

Update `src/components/common/PermissionAskDialog.tsx` to accept grouped requests:

```tsx
type PermissionAskDialogProps = {
  requests: PermissionAskSnapshot[];
  onApprove: (input: {
    toolCallIds: string[];
    remember: boolean;
    scopeOverride?: PathAuthScope;
  }) => void;
  onDeny: (input: { toolCallIds: string[] }) => void;
  onCancel: (input: { toolCallIds: string[] }) => void;
};

const requestedPaths = requests.map((request) => request.pathAuthScope?.path).filter(Boolean);
const commonScope = findCommonDirectory(requestedPaths);
const title =
  requests.length > 1
    ? `需要你确认 ${requests.length} 个权限请求`
    : '需要你确认权限请求';
```

Ensure the card has one button row and one `提交`.

- [ ] **Step 5: Run frontend tests**

Run:

```bash
pnpm exec vitest run src/components/common/PermissionAskDialog.test.tsx
```

Expected: PASS.

- [ ] **Step 6: Commit App group UI**

Run:

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs src/components/common/PermissionAskDialog.tsx src/components/common/PermissionAskDialog.test.tsx
git commit -m "feat: approve grouped permissions from app UI"
```

---

### Task 7: Keep IM Output Bound To Origin Run

**Files:**
- Modify: `src-tauri/src/connector/im/shared/reply_manager.rs`
- Modify: `src-tauri/src/connector/im/manager.rs`
- Test: `src-tauri/src/connector/im/shared/reply_manager.rs`

- [ ] **Step 1: Add output parity tests**

Append to `src-tauri/src/connector/im/shared/reply_manager.rs` tests:

```rust
#[tokio::test]
async fn app_origin_reply_is_not_forwarded_to_im() {
    let fixture = ReplyFixture::new();
    fixture.bind_run_app_only("sess-1", "run-app");

    fixture
        .manager
        .handle_assistant_delta("sess-1", "run-app", "App only output")
        .await
        .expect("delta");

    assert_eq!(fixture.im_sink.messages(), Vec::<String>::new());
}

#[tokio::test]
async fn im_origin_reply_uses_original_platform_target() {
    let fixture = ReplyFixture::new();
    fixture.bind_run_im("sess-1", "run-im", ImPlatform::Dingtalk, "conv-1");

    fixture
        .manager
        .handle_assistant_delta("sess-1", "run-im", "IM output")
        .await
        .expect("delta");

    assert_eq!(fixture.im_sink.messages(), vec![("conv-1".to_string(), "IM output".to_string())]);
}
```

- [ ] **Step 2: Run failing reply tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::reply_manager -- --nocapture
```

Expected: FAIL if app-origin output can still leak to IM, or if fixture names need adaptation.

- [ ] **Step 3: Route replies by `OutputBinding` only**

In `src-tauri/src/connector/im/shared/reply_manager.rs`, ensure outbound IM send is guarded:

```rust
let Some(binding) = self.output_bindings.get(session_id, run_id) else {
    tracing::debug!("[im-reply] no output binding session={} run={}", session_id, run_id);
    return Ok(());
};

match binding {
    OutputBinding::AppOnly => Ok(()),
    OutputBinding::Im { target, allow_streaming_reply, .. } if allow_streaming_reply => {
        self.im_sink.send_to_target(target, content).await
    }
    OutputBinding::Im { .. } => Ok(()),
}
```

Do not infer an IM target from session credentials when there is no run binding.

- [ ] **Step 4: Run reply tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::reply_manager -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit reply binding**

Run:

```bash
git add src-tauri/src/connector/im/shared/reply_manager.rs src-tauri/src/connector/im/manager.rs
git commit -m "fix: bind IM replies to originating run"
```

---

### Task 8: Full Regression Verification

**Files:**
- No new source files.
- Read: `~/.renlijia/logs/` only if any verification fails.

- [ ] **Step 1: Rust focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::ask_coordinator -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib runtime::pending::queue_manager_test -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::reply_manager -- --nocapture
```

Expected: all PASS.

- [ ] **Step 2: Cargo check**

Run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS with no compile errors.

- [ ] **Step 3: Frontend focused tests**

Run:

```bash
pnpm exec vitest run src/components/common/PermissionAskDialog.test.tsx
```

Expected: PASS.

- [ ] **Step 4: Manual IM/App smoke test**

Run:

```bash
pnpm run tauri:dev
```

Manual scenarios:

1. DingTalk sends `/tmp/aijia-permission-test/secret3.txt 读取总结一下这个文件`.
2. When permission is pending, DingTalk sends `问我三个问题`.
3. Expected: permission interaction is abandoned, one new turn starts, no stale permission reply loops.
4. Send another read request, then reply `好的，先拒绝吧`.
5. Expected: request is denied, App and DingTalk both show denial, no file read happens.
6. Trigger two same-directory reads in one run.
7. Expected: one grouped permission card, one decision resolves all covered approvals.
8. Send a message before AskUserQuestion card finishes rendering.
9. Expected: message drains immediately after registration and does not wait for another message.

- [ ] **Step 5: Inspect logs only if smoke test fails**

Run:

```bash
rg -n "\[im-ask\]|\[human-interaction\]|\[permission-group\]|HeldForHumanInteraction|PermissionAskRequired|AskUserQuestion" ~/.renlijia/logs
```

Expected when healthy: one route decision per inbound message and no duplicated `PermissionAskRequired` card for the same `tool_call_id`.

- [ ] **Step 6: Final commit**

Run:

```bash
git status --short
git add src-tauri/src/runtime/human_interaction \
  src-tauri/src/runtime/pending \
  src-tauri/src/runtime/store/pending_permission_request_store.rs \
  src-tauri/src/connector/im/shared/ask_coordinator.rs \
  src-tauri/src/connector/im/shared/reply_manager.rs \
  src-tauri/src/connector/im/manager.rs \
  src-tauri/src/transport/tauri_commands/chat.rs \
  src/components/common/PermissionAskDialog.tsx \
  src/components/common/PermissionAskDialog.test.tsx \
  docs/superpowers/plans/2026-06-09-human-interaction-priority-and-permission-group.md
git commit -m "feat: unify human interaction routing and permission groups"
```

Expected: final commit contains only the planned backend/frontend interaction changes plus this plan.

## Self-Review

- Spec coverage:
  - Priority interpretation: Tasks 1-3.
  - LLM judge fallback schema: Task 2.
  - Late-registration drain: Task 5.
  - Busy queue boundary: Task 5.
  - Permission group and batch approval: Tasks 4 and 6.
  - App/IM parity: Tasks 6 and 7.
  - All IM channels: Task 7 uses shared IM manager/reply code, so platform-specific edits should not be needed unless a channel bypasses shared coordinator.
- Placeholder scan:
  - No `TBD`, `TODO`, or deferred implementation steps.
  - LLM judge loading is explicitly out of scope.
- Risk:
  - Existing dirty experimental changes may already contain partial versions of these modules. Task 0 prevents blind revert and forces folding useful tests before replacement.
  - Frontend props may differ from the snippets. Preserve existing prop names where possible, but the behavioral tests define the contract.
