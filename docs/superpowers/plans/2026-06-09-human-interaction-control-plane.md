# Human Interaction Control Plane Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a shared human-interaction control plane so permissionAsk and AskUserQuestion suspend/resume runs consistently across APP and every IM channel, with run-scoped output binding and correct pending behavior.

**Architecture:** Introduce shared runtime types for `TurnOrigin`, `OutputBinding`, `InboundUserMessage`, and unified `HumanInteractionRequest`. Route every APP/IM user input through a central interaction router before pending queue; distinguish `Running` from `SuspendedForHuman`; bind outbound replies by `(session_id, run_id)` instead of session credentials.

**Tech Stack:** Rust/Tauri backend, existing `RuntimeRunRegistry`, `PendingQueueManager`, `PendingInteractionControlPlane`, `PendingPermissionControlPlane`, IM connector shared layer, Cargo unit tests.

---

## Scope And Ground Rules

- Worktree: `/Users/oayzz/.codex/worktrees/9a36/lotus-app`.
- Current dirty files from earlier experimental fixes must be inspected before implementation and either folded into the new architecture or deliberately replaced. Do not revert blindly.
- The plan targets all shared IM platforms: DingTalk, Feishu, Wecom, WeChat, Telegram, WhatsApp.
- TDD is required: every behavior change starts with a focused failing test.
- Commit after each task. Keep commits scoped.

## File Structure

Create:

- `src-tauri/src/runtime/human_interaction/mod.rs` — module exports.
- `src-tauri/src/runtime/human_interaction/types.rs` — shared origin, binding, inbound envelope, interaction request/result types.
- `src-tauri/src/runtime/human_interaction/router.rs` — deterministic routing for AskUserQuestion plus permission intent dispatch wrapper.
- `src-tauri/src/runtime/human_interaction/control_plane.rs` — adapter over existing permission and interaction control planes.
- `src-tauri/src/runtime/human_interaction/output_binding.rs` — run-scoped output binding registry.
- `src-tauri/src/runtime/human_interaction/tests.rs` — unit tests for router/control-plane behavior.
- `src-tauri/src/connector/im/shared/envelope.rs` — IM message to `InboundUserMessage` helpers.
- `src-tauri/src/connector/im/shared/output_binding_test.rs` — shared IM output binding regression tests if module test layout prefers separate file.

Modify:

- `src-tauri/src/runtime/mod.rs` — export `human_interaction`.
- `src-tauri/src/runtime/chat/chat_turn_driver.rs` — add origin/binding fields to `ChatTurnRequest`.
- `src-tauri/src/runtime/run_registry.rs` — add suspended state or companion state APIs.
- `src-tauri/src/runtime/interaction/types.rs` — attach optional origin/binding metadata to `InteractionRequest`.
- `src-tauri/src/runtime/store/pending_permission_request_store.rs` — attach optional origin/binding metadata to `PendingPermissionRequest`.
- `src-tauri/src/runtime/pending/types.rs` — make `PendingItem` carry inbound envelope metadata.
- `src-tauri/src/runtime/pending/queue_manager.rs` — route early buffered messages and drain with output binding.
- `src-tauri/src/runtime/pending/queue_manager_test.rs` — pending and drain regressions.
- `src-tauri/src/connector/im/shared/ask_coordinator.rs` — shrink to compatibility adapter, use unified router.
- `src-tauri/src/connector/im/shared/pending_adapter.rs` — produce envelope metadata for all platforms.
- `src-tauri/src/connector/im/shared/reply_manager.rs` — use run-scoped output binding.
- `src-tauri/src/connector/im/manager.rs` — dispatch IM messages through shared envelope and binding.
- `src-tauri/src/connector/im/{feishu,telegram,wechat,wecom,whatsapp}/*reply_forwarder.rs` — verify or adapt outbound sink to shared output binding.
- `src-tauri/src/lib.rs` — wire new registries into managed state.

---

### Task 0: Baseline Audit And Safety Net

**Files:**
- Read: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Read: `src-tauri/src/connector/im/shared/reply_manager.rs`
- Read: `src-tauri/src/runtime/pending/queue_manager.rs`
- Read: `src-tauri/src/runtime/pending/queue_manager_test.rs`
- Read: `src-tauri/src/lib.rs`
- Create: `docs/superpowers/plans/2026-06-09-human-interaction-baseline-notes.md`

- [ ] **Step 1: Capture current dirty diff**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app
git status --short
git diff -- src-tauri/src/connector/im/shared/ask_coordinator.rs \
  src-tauri/src/connector/im/shared/reply_manager.rs \
  src-tauri/src/lib.rs \
  src-tauri/src/runtime/pending/queue_manager.rs \
  src-tauri/src/runtime/pending/queue_manager_test.rs
```

Expected: status lists only the five known modified code files before implementation starts.

- [ ] **Step 2: Write baseline note**

Create `docs/superpowers/plans/2026-06-09-human-interaction-baseline-notes.md` with this structure:

```markdown
# Human Interaction Baseline Notes

## Dirty Files

- `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- `src-tauri/src/connector/im/shared/reply_manager.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/runtime/pending/queue_manager.rs`
- `src-tauri/src/runtime/pending/queue_manager_test.rs`

## Keep

- Existing tests that prove queued IM messages can be taken from pending queue are useful evidence.
- Existing reply-manager lifecycle tests are useful evidence.

## Replace With Shared Architecture

- Any direct AskUserQuestion pending-queue harvesting inside `ask_coordinator.rs`.
- Any reply-manager behavior that lazy-creates IM output from session credentials without run output binding.

## Implementation Rule

Do not revert these files blindly. Fold useful assertions into the new tests first, then remove duplicated logic only after replacement tests pass.
```

- [ ] **Step 3: Commit baseline note**

Run:

```bash
git add docs/superpowers/plans/2026-06-09-human-interaction-baseline-notes.md
git commit -m "docs: capture human interaction baseline"
```

Expected: commit succeeds with only the baseline note staged.

---

### Task 1: Define Shared Human Interaction Types

**Files:**
- Create: `src-tauri/src/runtime/human_interaction/mod.rs`
- Create: `src-tauri/src/runtime/human_interaction/types.rs`
- Modify: `src-tauri/src/runtime/mod.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Test: `src-tauri/src/runtime/human_interaction/tests.rs`

- [ ] **Step 1: Write failing type/default tests**

Create `src-tauri/src/runtime/human_interaction/tests.rs`:

```rust
use crate::runtime::chat::ChatTurnRequest;
use crate::runtime::human_interaction::{
    ImPlatform, OutputBinding, TurnOrigin,
};

#[test]
fn chat_turn_request_defaults_to_app_origin_and_app_only_output() {
    let request = ChatTurnRequest::new("session-1", "hello", Vec::new());

    assert_eq!(request.turn_origin, TurnOrigin::App);
    assert_eq!(request.output_binding, OutputBinding::AppOnly);
}

#[test]
fn im_output_binding_preserves_platform_and_target() {
    let binding = OutputBinding::im(
        ImPlatform::Dingtalk,
        "session-1",
        "conversation-1",
        true,
    );

    match binding {
        OutputBinding::Im { platform, target, allow_streaming_reply } => {
            assert_eq!(platform, ImPlatform::Dingtalk);
            assert_eq!(target.session_id, "session-1");
            assert_eq!(target.external_conversation_key, "conversation-1");
            assert!(allow_streaming_reply);
        }
        OutputBinding::AppOnly => panic!("expected IM binding"),
    }
}
```

Update `src-tauri/src/runtime/human_interaction/mod.rs` later to include:

```rust
pub mod types;

#[cfg(test)]
mod tests;

pub use types::*;
```

- [ ] **Step 2: Run failing tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction -- --nocapture
```

Expected: FAIL because `runtime::human_interaction`, `ChatTurnRequest::turn_origin`, and `ChatTurnRequest::output_binding` do not exist.

- [ ] **Step 3: Implement shared types**

Create `src-tauri/src/runtime/human_interaction/types.rs`:

```rust
use serde::{Deserialize, Serialize};

use crate::runtime::chat::chat_turn_driver::ChatAttachmentRef;
use crate::runtime::ids::{RunId, SessionId, ToolCallId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImPlatform {
    Dingtalk,
    Feishu,
    Wecom,
    Wechat,
    Telegram,
    Whatsapp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnOrigin {
    App,
    Im {
        platform: ImPlatform,
        external_conversation_key: String,
        sender_id: Option<String>,
        sender_label: Option<String>,
        account_id: Option<String>,
        thread_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImReplyTarget {
    pub session_id: String,
    pub external_conversation_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputBinding {
    AppOnly,
    Im {
        platform: ImPlatform,
        target: ImReplyTarget,
        allow_streaming_reply: bool,
    },
}

impl OutputBinding {
    pub fn im(
        platform: ImPlatform,
        session_id: impl Into<String>,
        external_conversation_key: impl Into<String>,
        allow_streaming_reply: bool,
    ) -> Self {
        Self::Im {
            platform,
            target: ImReplyTarget {
                session_id: session_id.into(),
                external_conversation_key: external_conversation_key.into(),
            },
            allow_streaming_reply,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundUserMessage {
    pub session_id: SessionId,
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<ChatAttachmentRef>,
    pub origin: TurnOrigin,
    pub output_binding: OutputBinding,
    pub received_at_ms: i64,
    pub source_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HumanInteractionId(String);

impl HumanInteractionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HumanInteractionKind {
    PermissionAsk,
    AskUserQuestion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HumanInteractionStatus {
    Pending,
    Resolved,
    Cancelled,
    Abandoned,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanInteractionRef {
    pub id: HumanInteractionId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_call_id: ToolCallId,
    pub kind: HumanInteractionKind,
    pub turn_origin: TurnOrigin,
    pub output_binding: OutputBinding,
    pub status: HumanInteractionStatus,
}
```

Modify `src-tauri/src/runtime/mod.rs`:

```rust
pub mod human_interaction;
```

Modify `ChatTurnRequest` in `src-tauri/src/runtime/chat/chat_turn_driver.rs`:

```rust
use crate::runtime::human_interaction::{OutputBinding, TurnOrigin};

pub struct ChatTurnRequest {
    pub conversation_id: SessionId,
    pub content: String,
    pub attachments: Vec<ChatAttachmentRef>,
    pub skill_command: Option<SkillCommandRef>,
    pub channel_context: Option<String>,
    pub turn_origin: TurnOrigin,
    pub output_binding: OutputBinding,
    // existing fields continue here
}
```

In `ChatTurnRequest::new`, set:

```rust
turn_origin: TurnOrigin::App,
output_binding: OutputBinding::AppOnly,
```

- [ ] **Step 4: Run type tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Run cargo check**

Run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS or only pre-existing unrelated warnings. Any compile error about missing `turn_origin` or `output_binding` must be fixed by adding default values at `ChatTurnRequest` struct literals.

- [ ] **Step 6: Commit**

Run:

```bash
git add src-tauri/src/runtime/mod.rs \
  src-tauri/src/runtime/human_interaction \
  src-tauri/src/runtime/chat/chat_turn_driver.rs
git commit -m "feat(runtime): add human interaction envelope types"
```

Expected: commit succeeds.

---

### Task 2: Add Run Activity State And Suspended-For-Human APIs

**Files:**
- Modify: `src-tauri/src/runtime/run_registry.rs`
- Test: `src-tauri/src/runtime/run_registry.rs`

- [ ] **Step 1: Write failing run-state tests**

Append tests to `src-tauri/src/runtime/run_registry.rs` test module:

```rust
#[test]
fn suspended_for_human_is_not_busy_but_keeps_run_identity() {
    let registry = RuntimeRunRegistry::new();
    let run_id = RunId::new("run-human");

    registry.reserve("sess", run_id.clone()).unwrap();
    registry.suspend_for_human("sess", "interaction-1").unwrap();

    assert!(!registry.is_session_busy("sess"));
    assert!(registry.is_session_suspended_for_human("sess"));
    assert_eq!(registry.run_id_for_session("sess").unwrap(), run_id);
    assert_eq!(
        registry.suspended_interaction_id("sess").as_deref(),
        Some("interaction-1")
    );
}

#[test]
fn resume_from_human_reacquires_busy_for_same_run() {
    let registry = RuntimeRunRegistry::new();
    let run_id = RunId::new("run-human");

    registry.reserve("sess", run_id.clone()).unwrap();
    registry.suspend_for_human("sess", "interaction-1").unwrap();
    registry.resume_from_human("sess").unwrap();

    assert!(registry.is_session_busy("sess"));
    assert!(!registry.is_session_suspended_for_human("sess"));
    assert_eq!(registry.run_id_for_session("sess").unwrap(), run_id);
}
```

- [ ] **Step 2: Run failing tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib runtime::run_registry -- --nocapture
```

Expected: FAIL because suspended APIs do not exist.

- [ ] **Step 3: Implement run state**

In `src-tauri/src/runtime/run_registry.rs`, replace `active_runs: Mutex<HashMap<String, ActiveRun>>` with:

```rust
enum RunEntry {
    Running(ActiveRun),
    SuspendedForHuman {
        run: ActiveRun,
        interaction_id: String,
        suspended_at: Instant,
    },
}

impl RunEntry {
    fn run(&self) -> &ActiveRun {
        match self {
            RunEntry::Running(run) => run,
            RunEntry::SuspendedForHuman { run, .. } => run,
        }
    }

    fn run_mut(&mut self) -> &mut ActiveRun {
        match self {
            RunEntry::Running(run) => run,
            RunEntry::SuspendedForHuman { run, .. } => run,
        }
    }

    fn is_running(&self) -> bool {
        matches!(self, RunEntry::Running(_))
    }
}
```

Update the map type:

```rust
entries: Mutex<HashMap<String, RunEntry>>,
```

Add APIs:

```rust
pub fn suspend_for_human(
    &self,
    session_id: &str,
    interaction_id: impl Into<String>,
) -> Result<(), String> {
    let mut entries = self.active_runs();
    let Some(entry) = entries.remove(session_id) else {
        return Err("No active run to suspend.".to_string());
    };
    let run = match entry {
        RunEntry::Running(run) => run,
        RunEntry::SuspendedForHuman { run, .. } => run,
    };
    entries.insert(
        session_id.to_string(),
        RunEntry::SuspendedForHuman {
            run,
            interaction_id: interaction_id.into(),
            suspended_at: Instant::now(),
        },
    );
    Ok(())
}

pub fn resume_from_human(&self, session_id: &str) -> Result<(), String> {
    let mut entries = self.active_runs();
    let Some(entry) = entries.remove(session_id) else {
        return Err("No suspended run to resume.".to_string());
    };
    let run = match entry {
        RunEntry::Running(run) => run,
        RunEntry::SuspendedForHuman { run, .. } => run,
    };
    entries.insert(session_id.to_string(), RunEntry::Running(run));
    Ok(())
}

pub fn is_session_suspended_for_human(&self, session_id: &str) -> bool {
    self.active_runs()
        .get(session_id)
        .map(|entry| matches!(entry, RunEntry::SuspendedForHuman { .. }))
        .unwrap_or(false)
}

pub fn suspended_interaction_id(&self, session_id: &str) -> Option<String> {
    self.active_runs().get(session_id).and_then(|entry| match entry {
        RunEntry::SuspendedForHuman { interaction_id, .. } => Some(interaction_id.clone()),
        RunEntry::Running(_) => None,
    })
}
```

Keep current external semantics:

```rust
pub fn is_session_busy(&self, session_id: &str) -> bool {
    self.active_runs()
        .get(session_id)
        .map(RunEntry::is_running)
        .unwrap_or(false)
}

pub fn run_id_for_session(&self, session_id: &str) -> Option<RunId> {
    self.active_runs()
        .get(session_id)
        .map(|entry| entry.run().run_id.clone())
}
```

- [ ] **Step 4: Run run-registry tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib runtime::run_registry -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Run pending queue tests for regression**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib pending::queue_manager -- --nocapture
```

Expected: PASS. If tests assumed suspended means busy, update them to match the new spec: suspended is not busy.

- [ ] **Step 6: Commit**

Run:

```bash
git add src-tauri/src/runtime/run_registry.rs src-tauri/src/runtime/pending/queue_manager_test.rs
git commit -m "feat(runtime): track suspended human interactions"
```

Expected: commit succeeds.

---

### Task 3: Build Unified Human Interaction Router

**Files:**
- Create: `src-tauri/src/runtime/human_interaction/router.rs`
- Modify: `src-tauri/src/runtime/human_interaction/mod.rs`
- Modify: `src-tauri/src/runtime/interaction/types.rs`
- Modify: `src-tauri/src/runtime/store/pending_permission_request_store.rs`
- Test: `src-tauri/src/runtime/human_interaction/tests.rs`

- [ ] **Step 1: Write failing router tests**

Append to `src-tauri/src/runtime/human_interaction/tests.rs`:

```rust
use crate::runtime::human_interaction::{
    AskQuestionSpec, HumanInteractionRef, HumanInteractionKind, HumanInteractionRouter,
    HumanReplyRoute,
    PermissionAskSpec, PermissionDecisionIntent,
};
use crate::runtime::ids::{RunId, SessionId, ToolCallId};

fn ask_ref(kind: HumanInteractionKind) -> HumanInteractionRef {
    HumanInteractionRef {
        id: crate::runtime::human_interaction::HumanInteractionId::new("hi-1"),
        session_id: SessionId::new("sess"),
        run_id: RunId::new("run"),
        tool_call_id: ToolCallId::new("tool"),
        kind,
        turn_origin: TurnOrigin::App,
        output_binding: OutputBinding::AppOnly,
        status: crate::runtime::human_interaction::HumanInteractionStatus::Pending,
    }
}

#[test]
fn ask_user_question_free_text_is_consumed_as_answer() {
    let route = HumanInteractionRouter::route_ask_user_question(
        &ask_ref(HumanInteractionKind::AskUserQuestion),
        &AskQuestionSpec {
            questions: vec!["专业领域".into()],
        },
        "HR/人事",
    );

    match route {
        HumanReplyRoute::ResolveAskUserQuestion { answers, raw_text } => {
            assert_eq!(raw_text, "HR/人事");
            assert_eq!(answers.get("专业领域").unwrap(), "HR/人事");
        }
        other => panic!("unexpected route: {other:?}"),
    }
}

#[test]
fn ask_user_question_topic_change_abandons_and_starts_new_turn() {
    let route = HumanInteractionRouter::route_ask_user_question(
        &ask_ref(HumanInteractionKind::AskUserQuestion),
        &AskQuestionSpec {
            questions: vec!["专业领域".into()],
        },
        "算了，看看别的文件",
    );

    assert!(matches!(route, HumanReplyRoute::AbandonAndStartNewTurn { .. }));
}

#[test]
fn permission_allow_once_is_structured_intent() {
    let route = HumanInteractionRouter::route_permission_reply(
        &ask_ref(HumanInteractionKind::PermissionAsk),
        &PermissionAskSpec {
            tool_name: "Read".into(),
            requested_path: Some("/private/tmp/aijia-permission-test/secret.txt".into()),
            current_scope: Some("path:/private/tmp/aijia-permission-test".into()),
        },
        "好的，那就允许你访问一次吧",
    );

    assert!(matches!(
        route,
        HumanReplyRoute::ResolvePermission {
            intent: PermissionDecisionIntent::AllowOnce
        }
    ));
}
```

- [ ] **Step 2: Run failing router tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction -- --nocapture
```

Expected: FAIL because router types do not exist.

- [ ] **Step 3: Implement deterministic router**

Create `src-tauri/src/runtime/human_interaction/router.rs`:

```rust
use std::collections::BTreeMap;

use super::types::HumanInteractionRef;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskQuestionSpec {
    pub questions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionAskSpec {
    pub tool_name: String,
    pub requested_path: Option<String>,
    pub current_scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecisionIntent {
    AllowOnce,
    AllowAlways { scope: Option<String> },
    Deny { reason: Option<String> },
    Cancel { reason: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HumanReplyRoute {
    ResolveAskUserQuestion {
        answers: BTreeMap<String, String>,
        raw_text: String,
    },
    ResolvePermission {
        intent: PermissionDecisionIntent,
    },
    AbandonAndStartNewTurn {
        reason: String,
        text: String,
    },
    Clarify {
        message: String,
    },
}

pub struct HumanInteractionRouter;

impl HumanInteractionRouter {
    pub fn route_ask_user_question(
        _interaction: &HumanInteractionRef,
        spec: &AskQuestionSpec,
        text: &str,
    ) -> HumanReplyRoute {
        let trimmed = text.trim();
        if is_topic_change(trimmed) {
            return HumanReplyRoute::AbandonAndStartNewTurn {
                reason: "user changed topic while ask user question was pending".into(),
                text: trimmed.into(),
            };
        }
        let lines: Vec<&str> = trimmed
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let mut answers = BTreeMap::new();
        if spec.questions.len() <= 1 {
            let key = spec.questions.first().cloned().unwrap_or_else(|| "answer".into());
            answers.insert(key, trimmed.into());
        } else {
            for (idx, question) in spec.questions.iter().enumerate() {
                if let Some(line) = lines.get(idx) {
                    answers.insert(question.clone(), (*line).to_string());
                }
            }
            if answers.is_empty() {
                answers.insert("rawText".into(), trimmed.into());
            }
        }
        HumanReplyRoute::ResolveAskUserQuestion {
            answers,
            raw_text: trimmed.into(),
        }
    }

    pub fn route_permission_reply(
        _interaction: &HumanInteractionRef,
        _spec: &PermissionAskSpec,
        text: &str,
    ) -> HumanReplyRoute {
        let trimmed = text.trim();
        if is_topic_change(trimmed) {
            return HumanReplyRoute::AbandonAndStartNewTurn {
                reason: "user changed topic while permission was pending".into(),
                text: trimmed.into(),
            };
        }
        if contains_any(trimmed, &["拒绝", "不允许", "不行"]) {
            return HumanReplyRoute::ResolvePermission {
                intent: PermissionDecisionIntent::Deny { reason: None },
            };
        }
        if contains_any(trimmed, &["取消", "算了", "不用了"]) {
            return HumanReplyRoute::ResolvePermission {
                intent: PermissionDecisionIntent::Cancel { reason: None },
            };
        }
        if contains_any(trimmed, &["以后", "永久", "都可以", "都允许"]) {
            return HumanReplyRoute::ResolvePermission {
                intent: PermissionDecisionIntent::AllowAlways {
                    scope: extract_path_like_scope(trimmed),
                },
            };
        }
        if contains_any(trimmed, &["允许", "可以", "同意", "好的", "行"]) {
            return HumanReplyRoute::ResolvePermission {
                intent: PermissionDecisionIntent::AllowOnce,
            };
        }
        HumanReplyRoute::Clarify {
            message: "我需要确认这是允许、拒绝、取消，还是一个新任务。".into(),
        }
    }
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn is_topic_change(text: &str) -> bool {
    contains_any(text, &["看看别的", "问我", "聊别的", "换个事", "新的任务"])
}

fn extract_path_like_scope(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|part| part.starts_with('/'))
        .map(|part| {
            part.trim_matches(|ch: char| {
                ch.is_whitespace() || matches!(ch, '，' | '。' | ',' | ';' | '；' | '`')
            })
            .to_string()
        })
}
```

Update `src-tauri/src/runtime/human_interaction/mod.rs`:

```rust
pub mod router;
pub mod types;

#[cfg(test)]
mod tests;

pub use router::*;
pub use types::*;
```

- [ ] **Step 4: Attach optional origin/binding to existing pending request types**

Modify `src-tauri/src/runtime/interaction/types.rs`:

```rust
use crate::runtime::human_interaction::{OutputBinding, TurnOrigin};

pub struct InteractionRequest {
    pub interaction_id: InteractionId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_call_id: ToolCallId,
    pub tool_name: String,
    pub kind: InteractionKind,
    pub payload: Value,
    pub original_request: RuntimeToolCallRequest,
    pub turn_origin: TurnOrigin,
    pub output_binding: OutputBinding,
}
```

Update constructors to pass `ctx.turn_origin.clone()` and `ctx.output_binding.clone()` after Task 4 adds those fields to tool context. Until Task 4, use `TurnOrigin::App` and `OutputBinding::AppOnly` in tests and existing constructor sites.

Modify `src-tauri/src/runtime/store/pending_permission_request_store.rs`:

```rust
use crate::runtime::human_interaction::{OutputBinding, TurnOrigin};

pub struct PendingPermissionRequest {
    pub tool_call_id: ToolCallId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_name: String,
    pub capability_scopes: Vec<String>,
    pub message: String,
    pub suggestions: Vec<String>,
    pub mode: PermissionMode,
    pub remember_options: Vec<PermissionDestination>,
    pub default_destination: Option<PermissionDestination>,
    pub original_request: RuntimeToolCallRequest,
    pub path_auth_scope: Option<String>,
    pub turn_origin: TurnOrigin,
    pub output_binding: OutputBinding,
}
```

For existing construction sites, initially fill app defaults:

```rust
turn_origin: TurnOrigin::App,
output_binding: OutputBinding::AppOnly,
```

- [ ] **Step 5: Run router tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Run cargo check**

Run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS after updating all struct literals.

- [ ] **Step 7: Commit**

Run:

```bash
git add src-tauri/src/runtime/human_interaction \
  src-tauri/src/runtime/interaction/types.rs \
  src-tauri/src/runtime/store/pending_permission_request_store.rs
git commit -m "feat(runtime): route human interaction replies"
```

Expected: commit succeeds.

---

### Task 4: Connect Tool Interactions To Suspended Run State

**Files:**
- Modify: `src-tauri/src/runtime/tools/context.rs`
- Modify: `src-tauri/src/runtime/tools/builtin/ask_user_question.rs`
- Modify: `src-tauri/src/runtime/chat/tool_round_driver.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Test: `src-tauri/src/runtime/human_interaction/tests.rs`

- [ ] **Step 1: Write failing metadata propagation test**

Append to `src-tauri/src/runtime/human_interaction/tests.rs`:

```rust
#[test]
fn interaction_ref_preserves_origin_and_output_binding() {
    let origin = TurnOrigin::Im {
        platform: ImPlatform::Feishu,
        external_conversation_key: "chat-1".into(),
        sender_id: Some("sender-1".into()),
        sender_label: Some("飞书用户".into()),
        account_id: Some("bot-1".into()),
        thread_id: None,
    };
    let binding = OutputBinding::im(ImPlatform::Feishu, "sess", "chat-1", true);
    let interaction = HumanInteractionRef {
        id: crate::runtime::human_interaction::HumanInteractionId::new("hi-1"),
        session_id: SessionId::new("sess"),
        run_id: RunId::new("run"),
        tool_call_id: ToolCallId::new("tool"),
        kind: HumanInteractionKind::AskUserQuestion,
        turn_origin: origin.clone(),
        output_binding: binding.clone(),
        status: crate::runtime::human_interaction::HumanInteractionStatus::Pending,
    };

    assert_eq!(interaction.turn_origin, origin);
    assert_eq!(interaction.output_binding, binding);
}
```

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction -- --nocapture
```

Expected: PASS if Task 3 types compile.

- [ ] **Step 2: Add origin/binding to tool execution context**

Modify `src-tauri/src/runtime/tools/context.rs`:

```rust
use crate::runtime::human_interaction::{OutputBinding, TurnOrigin};

pub struct ToolExecutionContext {
    // existing fields
    pub turn_origin: TurnOrigin,
    pub output_binding: OutputBinding,
}
```

At each `ToolExecutionContext` construction site, use the current `ChatTurnRequest` metadata:

```rust
turn_origin: request.turn_origin.clone(),
output_binding: request.output_binding.clone(),
```

For tests or helper contexts that do not have a request, use:

```rust
turn_origin: TurnOrigin::App,
output_binding: OutputBinding::AppOnly,
```

- [ ] **Step 3: Propagate metadata into AskUserQuestion**

Modify `src-tauri/src/runtime/tools/builtin/ask_user_question.rs` request construction:

```rust
let interaction_request = InteractionRequest {
    interaction_id: InteractionId::new(Uuid::new_v4().to_string()),
    session_id: ctx.session_id.clone(),
    run_id: ctx.run_id.clone(),
    tool_call_id: ctx.tool_call_id.clone(),
    tool_name: "AskUserQuestion".into(),
    kind: InteractionKind::AskUserQuestion,
    payload: json!({
        "questions": questions,
        "metadata": input.get("metadata").cloned().unwrap_or(Value::Null),
    }),
    original_request,
    turn_origin: ctx.turn_origin.clone(),
    output_binding: ctx.output_binding.clone(),
};
```

- [ ] **Step 4: Mark run suspended when interaction is inserted**

In the code path that handles `ToolError::InteractionRequired` and `ToolDispatchOutcome::AskRequired`, call:

```rust
run_registry.suspend_for_human(
    ctx.session_id.as_str(),
    interaction_id_or_tool_call_id,
)?;
```

Use the real `run_registry` reference already used by `PendingQueueManager` and session runtime. If the current layer lacks registry access, pass `Arc<RuntimeRunRegistry>` into the tool round driver constructor rather than reaching through global state.

- [ ] **Step 5: Reacquire running when resolving**

When `PendingInteractionControlPlane::resolve` or `PendingPermissionControlPlane::resolve_pending_request` successfully sends a resolution, call:

```rust
run_registry.resume_from_human(session_id.as_str())?;
```

If resolution comes from APP or IM and the run has already been cancelled/abandoned, return a structured stale result instead of starting a fake resume.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib runtime::run_registry -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Run cargo check**

Run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS.

- [ ] **Step 8: Commit**

Run:

```bash
git add src-tauri/src/runtime/tools/context.rs \
  src-tauri/src/runtime/tools/builtin/ask_user_question.rs \
  src-tauri/src/runtime/chat/tool_round_driver.rs \
  src-tauri/src/runtime/chat/chat_turn_driver.rs \
  src-tauri/src/runtime/human_interaction/tests.rs
git commit -m "feat(runtime): suspend runs for human interaction"
```

Expected: commit succeeds.

---

### Task 5: Make Pending Queue Envelope-Aware With Early Buffer

**Files:**
- Modify: `src-tauri/src/runtime/pending/types.rs`
- Modify: `src-tauri/src/runtime/pending/queue_manager.rs`
- Modify: `src-tauri/src/runtime/pending/queue_manager_test.rs`

- [ ] **Step 1: Write failing pending tests**

Append to `src-tauri/src/runtime/pending/queue_manager_test.rs`:

```rust
#[tokio::test]
async fn suspended_session_consumes_im_message_without_queueing() {
    let temp = tempfile::tempdir().unwrap();
    let (manager, registry) = build_manager(&temp);
    let session = SessionId::new("sess-human");
    let run_id = RunId::new("run-human");

    registry.reserve(session.as_str(), run_id).unwrap();
    registry.suspend_for_human(session.as_str(), "hi-1").unwrap();

    let item = PendingItem::im_text_for_test(
        PendingSource::ImDingtalk,
        "hello",
        "conv-1",
    );
    let outcome = manager.enqueue_or_send(session.clone(), item).await.unwrap();

    assert!(matches!(outcome, EnqueueOutcome::HeldForHumanInteraction { .. }));
    assert!(manager.snapshot(&session).await.unwrap().is_empty());
}

#[tokio::test]
async fn drained_im_batch_preserves_output_binding() {
    let item = PendingItem::im_text_for_test(
        PendingSource::ImFeishu,
        "你好",
        "feishu-chat",
    );
    let request = crate::runtime::pending::build_request_from_batch_for_test(
        &SessionId::new("sess-feishu"),
        vec![item],
    );

    assert!(matches!(
        request.output_binding,
        crate::runtime::human_interaction::OutputBinding::Im { .. }
    ));
}
```

Add a helper only under `#[cfg(test)]` in `PendingItem` implementation:

```rust
impl PendingItem {
    pub fn im_text_for_test(
        source: PendingSource,
        text: impl Into<String>,
        external_conversation_key: impl Into<String>,
    ) -> Self {
        let text = text.into();
        let external_conversation_key = external_conversation_key.into();
        Self {
            id: "pending-test".into(),
            source,
            text,
            sender_nick: None,
            attachments: Vec::new(),
            skill_command: None,
            received_at: "2026-06-09T00:00:00Z".into(),
            origin: crate::runtime::human_interaction::TurnOrigin::Im {
                platform: source.into(),
                external_conversation_key: external_conversation_key.clone(),
                sender_id: None,
                sender_label: None,
                account_id: None,
                thread_id: None,
            },
            output_binding: crate::runtime::human_interaction::OutputBinding::im(
                source.into(),
                "sess-test",
                external_conversation_key,
                true,
            ),
        }
    }
}
```

- [ ] **Step 2: Run failing pending tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib pending::queue_manager -- --nocapture
```

Expected: FAIL because `HeldForHumanInteraction`, envelope fields, and test helper do not exist.

- [ ] **Step 3: Extend PendingItem and PendingSource conversion**

Modify `src-tauri/src/runtime/pending/types.rs`:

```rust
use crate::runtime::human_interaction::{ImPlatform, OutputBinding, TurnOrigin};

pub struct PendingItem {
    pub id: String,
    pub source: PendingSource,
    pub text: String,
    pub sender_nick: Option<String>,
    pub attachments: Vec<PendingAttachment>,
    pub skill_command: Option<SkillCommandRef>,
    pub received_at: String,
    pub origin: TurnOrigin,
    pub output_binding: OutputBinding,
}

impl PendingSource {
    pub fn im_platform(self) -> Option<ImPlatform> {
        match self {
            PendingSource::ImDingtalk => Some(ImPlatform::Dingtalk),
            PendingSource::ImFeishu => Some(ImPlatform::Feishu),
            PendingSource::ImWecom => Some(ImPlatform::Wecom),
            PendingSource::ImWechat => Some(ImPlatform::Wechat),
            PendingSource::ImTelegram => Some(ImPlatform::Telegram),
            PendingSource::ImWhatsapp => Some(ImPlatform::Whatsapp),
            PendingSource::App => None,
        }
    }
}
```

For `PendingSource::App`, `im_platform()` returns `None`; set `TurnOrigin::App` and `OutputBinding::AppOnly` explicitly in constructors.

Extend `EnqueueOutcome`:

```rust
pub enum EnqueueOutcome {
    SentDirectly { request: ChatTurnRequest },
    Queued { snapshot: Vec<PendingItem> },
    HeldForHumanInteraction { interaction_id: Option<String> },
    Rejected { reason: EnqueueRejection },
}
```

- [ ] **Step 4: Update queue behavior**

In `PendingQueueManager::enqueue_or_send`, before checking busy:

```rust
if self.run_registry.is_session_suspended_for_human(session_id.as_str()) {
    return Ok(EnqueueOutcome::HeldForHumanInteraction {
        interaction_id: self.run_registry.suspended_interaction_id(session_id.as_str()),
    });
}
```

In `build_request_from_batch`, copy origin/binding from the batch:

```rust
let mut req = ChatTurnRequest::new(session_id.clone(), merged_text, attachments);
if let Some(last) = items.last() {
    req.turn_origin = last.origin.clone();
    req.output_binding = last.output_binding.clone();
}
req.pending_batch = Some(items);
```

If batch items have different `output_binding`, split batches before dispatch. Add this helper:

```rust
fn split_items_by_output_binding(items: Vec<PendingItem>) -> Vec<Vec<PendingItem>> {
    let mut batches: Vec<Vec<PendingItem>> = Vec::new();
    for item in items {
        if let Some(last_batch) = batches.last_mut() {
            if last_batch
                .last()
                .map(|last| last.output_binding == item.output_binding)
                .unwrap_or(false)
            {
                last_batch.push(item);
                continue;
            }
        }
        batches.push(vec![item]);
    }
    batches
}
```

Use it in drain:

```rust
for batch in split_items_by_output_binding(items) {
    let request = build_request_from_batch(&session_id, batch);
    dispatcher.dispatch(request).await?;
}
```

- [ ] **Step 5: Run pending tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib pending::queue_manager -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```bash
git add src-tauri/src/runtime/pending/types.rs \
  src-tauri/src/runtime/pending/queue_manager.rs \
  src-tauri/src/runtime/pending/queue_manager_test.rs
git commit -m "feat(pending): preserve envelopes across queue drain"
```

Expected: commit succeeds.

---

### Task 6: Add Run-Scoped Output Binding Registry

**Files:**
- Create: `src-tauri/src/runtime/human_interaction/output_binding.rs`
- Modify: `src-tauri/src/runtime/human_interaction/mod.rs`
- Modify: `src-tauri/src/connector/im/shared/reply_manager.rs`
- Modify: `src-tauri/src/connector/im/manager.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/connector/im/shared/reply_manager.rs`

- [ ] **Step 1: Write failing output binding tests**

Append to `src-tauri/src/connector/im/shared/reply_manager.rs` tests:

```rust
#[tokio::test]
async fn app_only_run_does_not_lazy_create_im_card_from_session_credentials() {
    let manager = DingtalkReplyManager::new();
    let session = SessionId::new("sess");
    let run = RunId::new("run-app");

    manager
        .remember_credentials(
            session.as_str().to_string(),
            "app-key".into(),
            "app-secret".into(),
            "robot".into(),
            CardTarget {
                conversation_id: "conv".into(),
                open_conversation_id: None,
                session_webhook: Some("https://example.invalid".into()),
            },
        )
        .await;

    let delivered = manager
        .dispatch_chunk_for_test(session.clone(), run.clone(), "hello", true)
        .await;

    assert!(!delivered, "app-only run must not deliver to IM from cached credentials");
}
```

If `dispatch_chunk_for_test` does not exist, add it as a test-only wrapper around the same internal path used by runtime events.

- [ ] **Step 2: Run failing reply-manager tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::reply_manager -- --nocapture
```

Expected: FAIL if current code lazy-creates from session credentials.

- [ ] **Step 3: Implement output binding registry**

Create `src-tauri/src/runtime/human_interaction/output_binding.rs`:

```rust
use std::collections::HashMap;
use std::sync::Mutex;

use crate::runtime::ids::{RunId, SessionId};

use super::types::OutputBinding;

#[derive(Default)]
pub struct RunOutputBindingRegistry {
    inner: Mutex<HashMap<(String, String), OutputBinding>>,
}

impl RunOutputBindingRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, session_id: &SessionId, run_id: &RunId, binding: OutputBinding) {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert((session_id.as_str().into(), run_id.as_str().into()), binding);
    }

    pub fn get(&self, session_id: &SessionId, run_id: &RunId) -> Option<OutputBinding> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&(session_id.as_str().into(), run_id.as_str().into()))
            .cloned()
    }

    pub fn clear(&self, session_id: &SessionId, run_id: &RunId) {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&(session_id.as_str().into(), run_id.as_str().into()));
    }
}
```

Update module:

```rust
pub mod output_binding;
pub use output_binding::*;
```

- [ ] **Step 4: Register binding before dispatch**

Where `ChatTurnRequest` is dispatched, register:

```rust
output_binding_registry.register(
    &request.conversation_id,
    &request.run_id,
    request.output_binding.clone(),
);
```

Do this for:

- direct APP dispatch
- direct IM dispatch
- pending drain dispatch
- resume dispatch if it creates a new request wrapper

- [ ] **Step 5: Make reply manager require run binding**

In `reply_manager.rs`, before IM delivery:

```rust
let Some(binding) = self.output_binding_registry.get(&session_id, &run_id) else {
    log::debug!(
        "[reply-manager] no output binding for session={} run={}; skip IM delivery",
        session_id.as_str(),
        run_id.as_str()
    );
    return Ok(());
};

let OutputBinding::Im { platform: ImPlatform::Dingtalk, .. } = binding else {
    return Ok(());
};
```

Keep session credentials as delivery material only after binding authorizes the run.

- [ ] **Step 6: Run reply-manager tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::reply_manager -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Run cargo check**

Run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS.

- [ ] **Step 8: Commit**

Run:

```bash
git add src-tauri/src/runtime/human_interaction/output_binding.rs \
  src-tauri/src/runtime/human_interaction/mod.rs \
  src-tauri/src/connector/im/shared/reply_manager.rs \
  src-tauri/src/connector/im/manager.rs \
  src-tauri/src/lib.rs
git commit -m "feat(im): bind replies to originating run"
```

Expected: commit succeeds.

---

### Task 7: Connect All IM Channels Through Shared Envelope

**Files:**
- Create: `src-tauri/src/connector/im/shared/envelope.rs`
- Modify: `src-tauri/src/connector/im/shared/mod.rs`
- Modify: `src-tauri/src/connector/im/shared/pending_adapter.rs`
- Modify: `src-tauri/src/connector/im/manager.rs`
- Inspect/Modify: `src-tauri/src/connector/im/feishu/reply_forwarder.rs`
- Inspect/Modify: `src-tauri/src/connector/im/telegram/reply_forwarder.rs`
- Inspect/Modify: `src-tauri/src/connector/im/wechat/reply_forwarder.rs`
- Inspect/Modify: `src-tauri/src/connector/im/wecom/reply_forwarder.rs`
- Inspect/Modify: `src-tauri/src/connector/im/whatsapp/reply_forwarder.rs`
- Test: `src-tauri/src/connector/im/shared/pending_adapter.rs`

- [ ] **Step 1: Write failing shared envelope tests**

Append to `src-tauri/src/connector/im/shared/pending_adapter.rs` tests or create a test module:

```rust
#[test]
fn every_im_pending_source_builds_im_origin_and_binding() {
    let sources = [
        PendingSource::ImDingtalk,
        PendingSource::ImFeishu,
        PendingSource::ImWecom,
        PendingSource::ImTelegram,
        PendingSource::ImWechat,
        PendingSource::ImWhatsapp,
    ];

    for source in sources {
        let item = build_pending_item_for_source_for_test(
            source,
            "conv-key",
            "sender",
            "hello",
        );

        assert!(matches!(item.origin, TurnOrigin::Im { .. }));
        assert!(matches!(item.output_binding, OutputBinding::Im { .. }));
    }
}
```

- [ ] **Step 2: Run failing adapter tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::pending_adapter -- --nocapture
```

Expected: FAIL because helper and envelope fields are not wired for all sources.

- [ ] **Step 3: Create shared envelope helper**

Create `src-tauri/src/connector/im/shared/envelope.rs`:

```rust
use crate::runtime::human_interaction::{ImPlatform, OutputBinding, TurnOrigin};
use crate::runtime::pending::PendingSource;

pub fn im_platform_for_source(source: PendingSource) -> Option<ImPlatform> {
    match source {
        PendingSource::App => None,
        PendingSource::ImDingtalk => Some(ImPlatform::Dingtalk),
        PendingSource::ImFeishu => Some(ImPlatform::Feishu),
        PendingSource::ImWecom => Some(ImPlatform::Wecom),
        PendingSource::ImWechat => Some(ImPlatform::Wechat),
        PendingSource::ImTelegram => Some(ImPlatform::Telegram),
        PendingSource::ImWhatsapp => Some(ImPlatform::Whatsapp),
    }
}

pub fn im_origin_and_binding(
    source: PendingSource,
    session_id: &str,
    external_conversation_key: &str,
    sender_label: Option<String>,
    allow_streaming_reply: bool,
) -> (TurnOrigin, OutputBinding) {
    let Some(platform) = im_platform_for_source(source) else {
        return (TurnOrigin::App, OutputBinding::AppOnly);
    };
    (
        TurnOrigin::Im {
            platform,
            external_conversation_key: external_conversation_key.to_string(),
            sender_id: None,
            sender_label,
            account_id: None,
            thread_id: None,
        },
        OutputBinding::im(
            platform,
            session_id.to_string(),
            external_conversation_key.to_string(),
            allow_streaming_reply,
        ),
    )
}
```

Update `src-tauri/src/connector/im/shared/mod.rs`:

```rust
pub mod envelope;
```

- [ ] **Step 4: Use helper in pending adapter for every platform**

In `build_pending_item_inner`, add parameters `session_id` and `external_conversation_key` if current call sites have them. If not available at that layer, set these fields in `manager.rs` immediately after item creation:

```rust
let (origin, output_binding) = im_origin_and_binding(
    pending_item.source,
    &session_id,
    &conv_key,
    pending_item.sender_nick.clone(),
    true,
);
pending_item.origin = origin;
pending_item.output_binding = output_binding;
```

Do not add DingTalk-only code. The platform must come from `PendingSource`.

- [ ] **Step 5: Verify non-DingTalk reply forwarders use shared send path**

For each file:

```bash
rg -n "ReplyContent|send\\(|AiCardChunk|Markdown|Text" \
  src-tauri/src/connector/im/feishu/reply_forwarder.rs \
  src-tauri/src/connector/im/telegram/reply_forwarder.rs \
  src-tauri/src/connector/im/wechat/reply_forwarder.rs \
  src-tauri/src/connector/im/wecom/reply_forwarder.rs \
  src-tauri/src/connector/im/whatsapp/reply_forwarder.rs
```

Expected: each channel has a path that can receive normalized `ReplyContent` or final text. If any channel does not, add a minimal adapter that accepts `OutputBinding::Im` for that platform and sends final markdown/text.

- [ ] **Step 6: Run shared adapter tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::pending_adapter -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Run cargo check**

Run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS.

- [ ] **Step 8: Commit**

Run:

```bash
git add src-tauri/src/connector/im/shared/envelope.rs \
  src-tauri/src/connector/im/shared/mod.rs \
  src-tauri/src/connector/im/shared/pending_adapter.rs \
  src-tauri/src/connector/im/manager.rs \
  src-tauri/src/connector/im/feishu/reply_forwarder.rs \
  src-tauri/src/connector/im/telegram/reply_forwarder.rs \
  src-tauri/src/connector/im/wechat/reply_forwarder.rs \
  src-tauri/src/connector/im/wecom/reply_forwarder.rs \
  src-tauri/src/connector/im/whatsapp/reply_forwarder.rs
git commit -m "feat(im): normalize inbound envelopes across channels"
```

Expected: commit succeeds. If a reply forwarder did not need changes, omit it from `git add`.

---

### Task 8: Replace ask_coordinator Patches With Unified Control Flow

**Files:**
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Modify: `src-tauri/src/runtime/human_interaction/control_plane.rs`
- Test: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Test: `src-tauri/src/runtime/human_interaction/tests.rs`

- [ ] **Step 1: Write failing coordinator tests**

Append to `src-tauri/src/connector/im/shared/ask_coordinator.rs` tests:

```rust
#[tokio::test]
async fn user_question_reply_uses_unified_router_not_pending_drain() {
    let harness = AskCoordinatorHarness::new_user_question_pending().await;

    let outcome = harness
        .coordinator
        .try_handle_reply(&harness.session_id, "我都可以，随便".into())
        .await
        .unwrap();

    assert!(matches!(outcome, HandleOutcome::AnswerResolved));
    assert!(harness.pending_queue_snapshot().await.is_empty());
}

#[tokio::test]
async fn permission_topic_change_abandons_and_falls_through_once() {
    let harness = AskCoordinatorHarness::new_permission_pending().await;

    let outcome = harness
        .coordinator
        .try_handle_reply(&harness.session_id, "问我三个问题".into())
        .await
        .unwrap();

    assert!(matches!(outcome, HandleOutcome::NewTurnAfterAbandon));
    assert_eq!(harness.fallthrough_dispatch_count().await, 1);
}
```

If `AskCoordinatorHarness` does not exist, create a local test helper that wires fake permission and interaction control planes. Keep the helper in the test module.

- [ ] **Step 2: Run failing coordinator tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::ask_coordinator -- --nocapture
```

Expected: FAIL until coordinator delegates to unified router and stops ad hoc pending harvesting.

- [ ] **Step 3: Create control-plane adapter**

Create `src-tauri/src/runtime/human_interaction/control_plane.rs`:

```rust
use std::sync::Arc;

use crate::runtime::interaction::PendingInteractionControlPlane;
use crate::runtime::store::PendingPermissionControlPlane;

use super::{HumanInteractionKind, HumanInteractionRef, HumanInteractionStatus};

pub struct HumanInteractionControlPlane {
    interactions: Arc<dyn PendingInteractionControlPlane>,
    permissions: Arc<dyn PendingPermissionControlPlane>,
}

impl HumanInteractionControlPlane {
    pub fn new(
        interactions: Arc<dyn PendingInteractionControlPlane>,
        permissions: Arc<dyn PendingPermissionControlPlane>,
    ) -> Self {
        Self {
            interactions,
            permissions,
        }
    }

    pub fn pending_for_session(&self, session_id: &str) -> Vec<HumanInteractionRef> {
        let mut refs = Vec::new();
        refs.extend(self.interactions.pending_for_session(session_id).into_iter().map(|req| {
            HumanInteractionRef {
                id: super::HumanInteractionId::new(req.interaction_id.as_str().to_string()),
                session_id: req.session_id,
                run_id: req.run_id,
                tool_call_id: req.tool_call_id,
                kind: HumanInteractionKind::AskUserQuestion,
                turn_origin: req.turn_origin,
                output_binding: req.output_binding,
                status: HumanInteractionStatus::Pending,
            }
        }));
        refs.extend(self.permissions.pending_for_session(&SessionId::new(session_id.to_string())).into_iter().map(|req| {
            HumanInteractionRef {
                id: super::HumanInteractionId::new(req.tool_call_id.as_str().to_string()),
                session_id: req.session_id,
                run_id: req.run_id,
                tool_call_id: req.tool_call_id,
                kind: HumanInteractionKind::PermissionAsk,
                turn_origin: req.turn_origin,
                output_binding: req.output_binding,
                status: HumanInteractionStatus::Pending,
            }
        }));
        refs
    }
}
```

If `PendingPermissionControlPlane` lacks `pending_for_session`, add:

```rust
fn pending_for_session(&self, session_id: &SessionId) -> Vec<PendingPermissionRequest>;
```

Implement it using the existing store map.

- [ ] **Step 4: Delegate coordinator routing**

In `ask_coordinator.rs`, replace direct ad hoc routing for ordinary text with:

```rust
match HumanInteractionRouter::route_ask_user_question(&interaction_ref, &spec, &content) {
    HumanReplyRoute::ResolveAskUserQuestion { answers, raw_text } => {
        self.resolve_user_question_answer(
            &pending,
            serde_json::json!({
                "answers": answers,
                "annotations": {
                    "rawText": raw_text,
                    "source": "im",
                    "answerMode": "freeText"
                }
            }),
        )?;
        self.remove_pending_if_current(session_id, &pending).await;
        Ok(HandleOutcome::AnswerResolved)
    }
    HumanReplyRoute::AbandonAndStartNewTurn { reason, .. } => {
        self.resolve_abandoned(&pending, reason)?;
        self.remove_pending_if_current(session_id, &pending).await;
        Ok(HandleOutcome::NewTurnAfterAbandon)
    }
    HumanReplyRoute::Clarify { message } => Ok(HandleOutcome::InvalidApprovalAction { message }),
    HumanReplyRoute::ResolvePermission { .. } => unreachable!("ask router returned permission"),
}
```

For permission, keep LLM judge for complex scope but make it return `HumanReplyRoute::ResolvePermission` and pass through program validation.

- [ ] **Step 5: Run coordinator tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::ask_coordinator -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Run human interaction tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit**

Run:

```bash
git add src-tauri/src/connector/im/shared/ask_coordinator.rs \
  src-tauri/src/runtime/human_interaction/control_plane.rs \
  src-tauri/src/runtime/human_interaction/mod.rs \
  src-tauri/src/runtime/store/pending_permission_request_store.rs
git commit -m "feat(runtime): unify IM human reply routing"
```

Expected: commit succeeds.

---

### Task 9: Hide Internal Fallback Commands From User Cards

**Files:**
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Modify: IM card formatting helpers in `src-tauri/src/connector/im/dingtalk/card.rs` if prompt text is assembled there
- Test: `src-tauri/src/connector/im/shared/ask_coordinator.rs`

- [ ] **Step 1: Write failing card text tests**

Append tests:

```rust
#[test]
fn permission_card_text_does_not_show_internal_approve_command() {
    let text = render_permission_prompt_for_im_for_test(
        "Read",
        "/private/tmp/aijia-permission-test/secret.txt",
    );

    assert!(!text.contains("/approve"));
    assert!(!text.contains("call_00_"));
    assert!(text.contains("仅本次允许"));
    assert!(text.contains("永久允许"));
    assert!(text.contains("你也可以直接回复自然语言"));
}

#[test]
fn ask_user_question_card_text_does_not_show_internal_answer_command() {
    let text = render_ask_user_question_for_im_for_test(vec!["专业领域"]);

    assert!(!text.contains("/answer"));
    assert!(!text.contains("interaction"));
    assert!(text.contains("你可以按选项回复"));
}
```

- [ ] **Step 2: Run failing card tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::ask_coordinator -- --nocapture
```

Expected: FAIL if fallback commands still render.

- [ ] **Step 3: Remove command rendering from user-visible copy**

Update permission prompt renderer to produce:

```text
🔒 我需要你的确认才能继续

工具：Read
路径：/private/tmp/aijia-permission-test/secret.txt

请选择以下操作之一：

1. 仅本次允许
2. 永久允许
3. 拒绝
4. 取消当前任务

你也可以直接回复自然语言说明授权范围或调整要求。
```

Update AskUserQuestion prompt renderer to produce:

```text
❓ 我有几个问题想问你

1. 专业领域

你可以按选项回复，也可以直接用自然语言回答。
```

Keep `/approve` and `/answer` parser code unchanged for compatibility.

- [ ] **Step 4: Run card tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::ask_coordinator -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

Run:

```bash
git add src-tauri/src/connector/im/shared/ask_coordinator.rs src-tauri/src/connector/im/dingtalk/card.rs
git commit -m "fix(im): hide internal approval commands from cards"
```

Expected: commit succeeds. If DingTalk card file did not change, omit it from `git add`.

---

### Task 10: End-To-End Regression Matrix

**Files:**
- Modify: `src-tauri/src/runtime/human_interaction/tests.rs`
- Modify: `src-tauri/src/runtime/pending/queue_manager_test.rs`
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Modify: `src-tauri/src/connector/im/shared/reply_manager.rs`
- Create: `docs/superpowers/plans/2026-06-09-human-interaction-verification.md`

- [ ] **Step 1: Run focused Rust tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib runtime::run_registry -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib pending::queue_manager -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::ask_coordinator -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::reply_manager -- --nocapture
```

Expected: all PASS.

- [ ] **Step 2: Run cargo check**

Run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS.

- [ ] **Step 3: Verify main scenarios manually with dev server**

Run:

```bash
pnpm run tauri:dev
```

Manual scenarios:

1. DingTalk: ask to read `/tmp/aijia-permission-test/secret3.txt`; reply `好的，那就允许你访问一次吧`; expected original run resumes and DingTalk receives final content.
2. DingTalk: ask to read same file; reply `以后 /tmp/aijia-permission-test 都可以读`; expected `permissions.json` contains the path scope and a second read does not ask again.
3. DingTalk: while permission card is visible, send `问我三个问题`; expected permission abandoned, new turn asks questions once, message is not replayed by pending drain.
4. DingTalk: AskUserQuestion visible; reply `我都可以，随便`; expected reply is consumed as answer and not queued as new turn.
5. APP: send ordinary APP message in the same conversation after DingTalk credentials exist; expected no DingTalk output.
6. One non-DingTalk configured channel if available: send a busy queued message; expected drain replies to that same channel.

- [ ] **Step 4: Capture verification notes**

Create `docs/superpowers/plans/2026-06-09-human-interaction-verification.md`:

```markdown
# Human Interaction Verification

## Automated

- `cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction -- --nocapture`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml --lib runtime::run_registry -- --nocapture`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml --lib pending::queue_manager -- --nocapture`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::ask_coordinator -- --nocapture`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::reply_manager -- --nocapture`: PASS
- `cargo check --manifest-path src-tauri/Cargo.toml`: PASS

## Manual

- DingTalk allow once: PASS
- DingTalk allow always and permissions.json persistence: PASS
- DingTalk abandon permission into new AskUserQuestion turn: PASS
- DingTalk AskUserQuestion free text answer: PASS
- APP-only output does not go to IM: PASS
- Non-DingTalk shared channel path: PASS or not configured locally

## Notes

- If a non-DingTalk channel is not configured locally, automated shared-envelope tests cover the platform path and the missing live manual check is recorded here.
```

- [ ] **Step 5: Commit verification**

Run:

```bash
git add docs/superpowers/plans/2026-06-09-human-interaction-verification.md
git commit -m "docs: record human interaction verification"
```

Expected: commit succeeds.

---

## Self-Review

Spec coverage:

- Unified permissionAsk and AskUserQuestion lifecycle: Tasks 1, 3, 4, 8.
- `Running` vs `SuspendedForHuman`: Task 2.
- Next message consumed by interaction before pending queue: Tasks 3, 5, 8.
- Early message before interaction registration: Task 5.
- Run-scoped output binding: Tasks 1, 6.
- All IM channels: Task 7 and Task 10.
- Hidden fallback commands: Task 9.
- Permission safety and program validation: Tasks 3 and 8 keep LLM/parsing as intent only; validation remains in permission control plane.
- APP/IM sync rule: Tasks 6 and 10.

No placeholders:

- Every task has concrete files, commands, expected results, and code skeletons for the new APIs.
- Any conditional file in `git add` is explicitly tied to whether that file changed.

Type consistency:

- `TurnOrigin`, `OutputBinding`, `InboundUserMessage`, `HumanInteractionRef`, `HumanReplyRoute`, and `RunOutputBindingRegistry` are introduced before downstream tasks use them.
- Pending queue references `OutputBinding` only after Task 1 creates it.
- Coordinator references `HumanInteractionRouter` only after Task 3 creates it.

Implementation order:

- The plan starts with a baseline note because this worktree contains prior experimental modifications.
- Each task can be tested and committed independently.
