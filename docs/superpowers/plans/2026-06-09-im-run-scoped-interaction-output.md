# IM Run-Scoped Interaction Output Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make IM/RM pending interaction output run-scoped, hide internal fallback commands from user cards, prevent NewTurn duplicate queue execution, and stop APP-only replies from leaking back to IM/RM.

**Architecture:** Keep `IMAskCoordinator` as the pre-dispatch router and `DingtalkReplyManager` as the DingTalk AI-card sink, but make both run-aware. Card output is keyed by `session_id + run_id`; APP-side pending resolutions send only a short feedback through a new IM feedback coordinator, while normal APP runs have no IM reply route. Internal `/approve` and `/answer` parsing remains available, but card markdown no longer exposes those commands.

**Tech Stack:** Rust, Tokio, Tauri commands, RuntimeEventBus, existing IM connector abstractions, focused Rust unit tests with `cargo test --lib`.

---

## File Structure

- Modify `src-tauri/src/connector/im/shared/ask_coordinator.rs`
  - Remove user-visible fallback command text from `format_pending_ask_markdown`.
  - Add `HandleOutcome::NewTurnAfterAbandon`.
  - Change `AskOutputSink::deliver_ask_card` to receive `run_id`.
  - Register app-side pending feedback routes when IM/RM-origin permission or AskUserQuestion asks arrive.
  - Clear app-side feedback routes when pending asks are resolved, cancelled, or run-cleaned.

- Modify `src-tauri/src/connector/im/shared/reply_manager.rs`
  - Store card contexts in `HashMap<String, ReplyContext>` where the key is `card_context_key(session_id, run_id)`.
  - Merge same-run preface text and pending ask markdown into one card.
  - Remove session-credential-only lazy creation for ordinary runtime events.
  - Add a method for short app-side pending-resolution feedback.

- Create `src-tauri/src/connector/im/shared/app_feedback.rs`
  - Store pending interaction feedback routes keyed by `tool_call_id` or `interaction_id`.
  - Provide short feedback message construction.
  - Let app-side resolve commands notify IM/RM only when the pending request originated from IM/RM.

- Modify `src-tauri/src/connector/im/shared/mod.rs`
  - Export `app_feedback`.

- Modify `src-tauri/src/transport/tauri_commands/chat.rs`
  - Inject optional `IMAppFeedbackCoordinator`.
  - Before resolving a permission or interaction, snapshot the pending request by id.
  - After successful resolve, send the short feedback through the coordinator.
  - Keep normal APP `send_message` unchanged and APP-only.

- Modify `src-tauri/src/runtime/session_runtime.rs`
  - Add lookup helpers for pending permission and pending interaction by id so app commands can build feedback after resolving.

- Modify `src-tauri/src/runtime/interaction/control_plane.rs`
  - Add `get_pending(&InteractionId) -> Option<InteractionRequest>`.

- Modify `src-tauri/src/connector/im/manager.rs`
  - Treat `NewTurnAfterAbandon` as normal fallthrough dispatch for all wired IM channels.
  - Ensure the fallthrough message is not queued behind the abandoned interaction.
  - For DingTalk direct IM runs, continue to register reply context with the new run-aware API.

- Modify `src-tauri/src/lib.rs`
  - Construct and manage the shared `IMAppFeedbackCoordinator`.
  - Pass it into `TauriChatCommandAdapter` and `IMAskCoordinator`.

- Tests live in existing Rust test modules:
  - `src-tauri/src/connector/im/shared/ask_coordinator.rs`
  - `src-tauri/src/connector/im/shared/reply_manager.rs`
  - `src-tauri/src/connector/im/shared/app_feedback.rs`
  - `src-tauri/src/runtime/session_runtime.rs`
  - `src-tauri/src/runtime/interaction/control_plane.rs`

## Task 1: Hide Internal Fallback Commands From IM Cards

**Files:**
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Test: `src-tauri/src/connector/im/shared/ask_coordinator.rs`

- [ ] **Step 1: Write failing markdown tests**

Add these tests inside the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn permission_markdown_hides_internal_approve_commands() {
    let text = format_pending_ask_markdown(&PendingAskKind::Permission {
        tool_call_id: ToolCallId::new("call_00_secret"),
        tool_name: "Read".into(),
        message: "该路径未授权，需要用户确认：路径=/private/tmp/a.txt".into(),
        suggestions: vec!["仅本次允许".into(), "永久允许".into(), "拒绝".into()],
        path_auth_scope: Some("path:/private/tmp".into()),
    });

    assert!(text.contains("我需要你的确认才能继续"));
    assert!(text.contains("Read"));
    assert!(text.contains("仅本次允许"));
    assert!(!text.contains("/approve"));
    assert!(!text.contains("call_00_secret"));
    assert!(!text.contains("备用指令"));
}

#[test]
fn user_question_markdown_hides_internal_answer_commands() {
    let text = format_pending_ask_markdown(&PendingAskKind::UserQuestion {
        interaction_id: InteractionId::new("ask-secret"),
        tool_call_id: ToolCallId::new("tool-1"),
        questions: serde_json::json!({
            "questions": [
                {
                    "question": "专业领域",
                    "options": [
                        { "label": "HR/人事" },
                        { "label": "财务" }
                    ]
                }
            ]
        }),
    });

    assert!(text.contains("我有几个问题想问你"));
    assert!(text.contains("专业领域"));
    assert!(text.contains("HR/人事"));
    assert!(!text.contains("/answer"));
    assert!(!text.contains("ask-secret"));
    assert!(!text.contains("备用指令"));
}
```

- [ ] **Step 2: Run tests and confirm failure**

Run:

```bash
cd src-tauri
cargo test --lib connector::im::shared::ask_coordinator::tests::permission_markdown_hides_internal_approve_commands connector::im::shared::ask_coordinator::tests::user_question_markdown_hides_internal_answer_commands -- --nocapture
```

Expected: both tests fail because `format_pending_ask_markdown` still prints `/approve`, `/answer`, and ids.

- [ ] **Step 3: Update permission markdown**

Replace the permission branch of `format_pending_ask_markdown` with this shape:

```rust
let mut text = format!(
    "🔒 我需要你的确认才能继续\n\n工具：**{}**\n\n> {}\n\n请选择以下操作之一：",
    tool_name,
    message
);
if !suggestions.is_empty() {
    text.push_str("\n\n");
    for (idx, suggestion) in suggestions.iter().enumerate() {
        text.push_str(&format!("{}. {}\n", idx + 1, suggestion));
    }
} else {
    text.push_str("\n\n1. 仅本次允许\n2. 永久允许\n3. 拒绝\n4. 取消当前任务\n");
}
text.push_str("\n你也可以直接回复自然语言说明授权范围或调整要求。");
text
```

- [ ] **Step 4: Update AskUserQuestion markdown**

Remove the final user-question branch block that currently appends:

```rust
text.push_str(&format!(
    "\n备用指令：\n- `/answer {} <你的答案>` 提交答案\n- `/answer {} cancel` 取消当前任务",
    interaction_id.as_str(),
    interaction_id.as_str()
));
```

End the branch with:

```rust
text.push_str("\n你可以按选项回复，也可以直接用自然语言回答。");
text
```

- [ ] **Step 5: Remove user-facing command advice from judge fallback messages**

Change the two `PendingPermissionReplyIntent::Unclear` messages in `UnavailablePendingReplyJudge` and `GatewayPendingReplyJudge` from command advice to natural-language advice:

```rust
message: "当前没有可用的语义解析器，请直接说“允许一次”“以后都允许”“拒绝”或“取消当前任务”。".into()
```

and:

```rust
message: "语义解析暂时失败了，请直接说“允许一次”“以后都允许”“拒绝”或“取消当前任务”。".into()
```

- [ ] **Step 6: Run focused tests**

Run:

```bash
cd src-tauri
cargo test --lib connector::im::shared::ask_coordinator::tests:: -- --nocapture
```

Expected: all ask coordinator tests pass.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/connector/im/shared/ask_coordinator.rs
git commit -m "fix: hide IM internal approval commands"
```

## Task 2: Mark NewTurn-After-Abandon Separately

**Files:**
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Modify: `src-tauri/src/connector/im/manager.rs`
- Test: `src-tauri/src/connector/im/shared/ask_coordinator.rs`

- [ ] **Step 1: Write failing outcome test**

Update the existing `permission_judge_new_turn_intent_abandons_pending_and_falls_through` test to expect the new outcome:

```rust
assert_eq!(outcome, HandleOutcome::NewTurnAfterAbandon);
```

Add a second assertion in `permission_judge_new_turn_intent_clears_pending_without_consuming_message`:

```rust
assert_eq!(outcome, HandleOutcome::NewTurnAfterAbandon);
```

- [ ] **Step 2: Run tests and confirm failure**

Run:

```bash
cd src-tauri
cargo test --lib connector::im::shared::ask_coordinator::tests::permission_judge_new_turn_intent_ -- --nocapture
```

Expected: tests fail because the code currently returns `HandleOutcome::NotPending`.

- [ ] **Step 3: Add the enum variant**

Change `HandleOutcome`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandleOutcome {
    NotPending,
    NewTurnAfterAbandon,
    ApprovalResolved,
    AnswerResolved,
    InvalidApprovalAction { message: String },
}
```

- [ ] **Step 4: Return the new outcome for permission NewTurn**

Change the `PendingPermissionReplyIntent::NewTurn` branch:

```rust
PendingPermissionReplyIntent::NewTurn { reason } => {
    if !self.resolve_abandoned(&pending, reason)? {
        return Ok(HandleOutcome::InvalidApprovalAction {
            message: "当前审批已失效，请重新发送你的请求。".to_string(),
        });
    }
    self.remove_pending_if_current(session_id, &pending).await;
    Ok(HandleOutcome::NewTurnAfterAbandon)
}
```

- [ ] **Step 5: Update all IM manager matches**

For every `match handle_pending_action_pre_dispatch(...)` in `src-tauri/src/connector/im/manager.rs`, change:

```rust
Ok(super::shared::ask_coordinator::HandleOutcome::NotPending) => {}
```

to:

```rust
Ok(super::shared::ask_coordinator::HandleOutcome::NotPending)
| Ok(super::shared::ask_coordinator::HandleOutcome::NewTurnAfterAbandon) => {}
```

Do not add an ACK or `continue` for `NewTurnAfterAbandon`; it must fall through to normal dispatch exactly once.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cd src-tauri
cargo test --lib connector::im::shared::ask_coordinator::tests::permission_judge_new_turn_intent_ -- --nocapture
```

Expected: tests pass.

- [ ] **Step 7: Compile IM manager**

Run:

```bash
cd src-tauri
cargo check
```

Expected: check succeeds with no non-exhaustive `HandleOutcome` match errors.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/connector/im/shared/ask_coordinator.rs src-tauri/src/connector/im/manager.rs
git commit -m "fix: mark abandoned IM approval new turns"
```

## Task 3: Make Ask Card Delivery Run-Aware

**Files:**
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Modify: `src-tauri/src/connector/im/shared/reply_manager.rs`
- Test: `src-tauri/src/connector/im/shared/reply_manager.rs`

- [ ] **Step 1: Add the planned key helper to tests**

The implementation will add this helper near `scheduled_card_update_key`:

```rust
fn card_context_key(session_id: &str, run_id: &str) -> String {
    format!("{session_id}\n{run_id}")
}
```

Use this helper in the new tests below so the tests describe the target run-aware storage shape.

- [ ] **Step 2: Write failing same-run merge test**

Add this test in `reply_manager.rs` tests:

```rust
#[tokio::test]
async fn deliver_ask_card_merges_with_same_run_preface() {
    use super::super::ask_coordinator::AskOutputSink;

    let mgr = DingtalkReplyManager::new();
    {
        let mut ctx = mgr.contexts.lock().await;
        let key = card_context_key("sess-merge", "run-question");
        ctx.insert(
            key,
            ReplyContext {
                card_lifecycle: CardLifecycle::Streaming(CardInstance {
                    card_instance_id: "card-merge".into(),
                    inputing_started: true,
                }),
                accumulated_text: "好的，我来问你三个问题。".into(),
                app_key: "key".into(),
                app_secret: "secret".into(),
                robot_code: "robot".into(),
                target: CardTarget::Private {
                    user_id: "user".into(),
                },
                run_id: "run-question".into(),
            },
        );
    }

    let _ = mgr
        .deliver_ask_card(
            &SessionId::new("sess-merge"),
            &RunId::new("run-question"),
            "❓ 我有几个问题想问你".into(),
        )
        .await;

    let ctx = mgr.contexts.lock().await;
    let key = card_context_key("sess-merge", "run-question");
    let merged = &ctx[&key].accumulated_text;
    assert!(merged.contains("好的，我来问你三个问题。"));
    assert!(merged.contains("❓ 我有几个问题想问你"));
    assert!(matches!(ctx[&key].card_lifecycle, CardLifecycle::Finished));
}
```

- [ ] **Step 3: Write failing cross-run isolation test**

Add:

```rust
#[tokio::test]
async fn deliver_ask_card_does_not_modify_other_run_context() {
    use super::super::ask_coordinator::AskOutputSink;

    let mgr = DingtalkReplyManager::new();
    {
        let mut ctx = mgr.contexts.lock().await;
        let old_key = card_context_key("sess-cross", "run-read");
        ctx.insert(
            old_key,
            ReplyContext {
                card_lifecycle: CardLifecycle::Streaming(CardInstance {
                    card_instance_id: "card-old".into(),
                    inputing_started: true,
                }),
                accumulated_text: "好的，我来查看文件。".into(),
                app_key: "key".into(),
                app_secret: "secret".into(),
                robot_code: "robot".into(),
                target: CardTarget::Private {
                    user_id: "user".into(),
                },
                run_id: "run-read".into(),
            },
        );
    }

    let _ = mgr
        .deliver_ask_card(
            &SessionId::new("sess-cross"),
            &RunId::new("run-question"),
            "❓ 我有几个问题想问你".into(),
        )
        .await;

    let ctx = mgr.contexts.lock().await;
    let old_key = card_context_key("sess-cross", "run-read");
    let new_key = card_context_key("sess-cross", "run-question");
    assert_eq!(ctx[&old_key].accumulated_text, "好的，我来查看文件。");
    assert_eq!(ctx[&old_key].run_id, "run-read");
    assert!(!ctx.contains_key(&new_key));
}
```

- [ ] **Step 4: Run tests and confirm failure**

Run:

```bash
cd src-tauri
cargo test --lib connector::im::shared::reply_manager::tests::deliver_ask_card_ -- --nocapture
```

Expected: compile fails because `AskOutputSink::deliver_ask_card` does not accept `RunId`.

- [ ] **Step 5: Update the trait signature**

In `ask_coordinator.rs`:

```rust
#[async_trait]
pub trait AskOutputSink: Send + Sync {
    async fn deliver_ask_card(
        &self,
        session_id: &SessionId,
        run_id: &RunId,
        markdown: String,
    ) -> Result<()>;
    async fn force_finish_current_card(
        &self,
        session_id: &SessionId,
        reason_for_log: &str,
    ) -> Result<()>;
}
```

- [ ] **Step 6: Pass run_id from register_pending**

Change:

```rust
self.sink
    .deliver_ask_card(&event.session_id, markdown)
    .await?;
```

to:

```rust
self.sink
    .deliver_ask_card(&event.session_id, &event.run_id, markdown)
    .await?;
```

- [ ] **Step 7: Update test sink implementation**

In the `RecordingSink` test implementation:

```rust
async fn deliver_ask_card(
    &self,
    _session_id: &SessionId,
    _run_id: &RunId,
    markdown: String,
) -> Result<()> {
    self.calls.lock().unwrap().push(markdown);
    Ok(())
}
```

- [ ] **Step 8: Add run-aware context lookup**

Add the helper near `scheduled_card_update_key`:

```rust
fn card_context_key(session_id: &str, run_id: &str) -> String {
    format!("{session_id}\n{run_id}")
}
```

Update all context reads and writes:

```rust
let key = card_context_key(session_id, run_id);
contexts.get(&key)
contexts.get_mut(&key)
contexts.insert(key, ReplyContext { ... })
contexts.remove(&key)
```

When code only has `session_id` and no `run_id`, it must not touch streaming contexts. Use the session-only `session_credentials` map only for short feedback cards.

- [ ] **Step 9: Update `DingtalkReplyManager` merge behavior**

Change the implementation to reject cross-run mutation and merge same-run markdown:

```rust
async fn deliver_ask_card(
    &self,
    session_id: &SessionId,
    run_id: &RunId,
    markdown: String,
) -> Result<()> {
    let mut contexts = self.contexts.lock().await;
    let key = card_context_key(session_id.as_str(), run_id.as_str());
    let Some(ctx) = contexts.get_mut(&key) else {
        return Ok(());
    };

    let ask_content = non_empty_ask_content(markdown);
    if ctx.accumulated_text.trim().is_empty() {
        ctx.accumulated_text = ask_content;
    } else {
        ctx.accumulated_text.push_str("\n\n");
        ctx.accumulated_text.push_str(&ask_content);
    }

    if let CardLifecycle::Streaming(card) = &mut ctx.card_lifecycle {
        let text = ctx.accumulated_text.clone();
        let _ = dingtalk_card::finish_card(
            &self.token_cache,
            &ctx.app_key,
            &ctx.app_secret,
            card,
            &text,
        )
        .await;
    }
    ctx.card_lifecycle = CardLifecycle::Finished;
    Ok(())
}
```

- [ ] **Step 10: Update `deliver_pending_approval_ack`**

`deliver_pending_approval_ack` is not tied to a runtime run. Keep it out of the trait and send via a new private helper:

```rust
async fn deliver_session_feedback_card(
    &self,
    session_id: &SessionId,
    message: String,
) -> anyhow::Result<()> {
    let creds = self.session_credentials.lock().await.get(session_id.as_str()).cloned();
    let Some(creds) = creds else {
        return Ok(());
    };
    if let Some(mut card) = dingtalk_card::create_and_deliver_card(
        &self.token_cache,
        &creds.app_key,
        &creds.app_secret,
        &creds.robot_code,
        &creds.target,
    )
    .await
    {
        let _ = dingtalk_card::finish_card(
            &self.token_cache,
            &creds.app_key,
            &creds.app_secret,
            &mut card,
            &message,
        )
        .await;
    }
    Ok(())
}
```

Then call it from `deliver_pending_approval_ack`.

- [ ] **Step 11: Run focused tests**

Run:

```bash
cd src-tauri
cargo test --lib connector::im::shared::reply_manager::tests::deliver_ask_card_ connector::im::shared::ask_coordinator::tests:: -- --nocapture
```

Expected: reply manager and ask coordinator tests pass.

- [ ] **Step 12: Commit**

```bash
git add src-tauri/src/connector/im/shared/ask_coordinator.rs src-tauri/src/connector/im/shared/reply_manager.rs
git commit -m "fix: merge IM ask cards by run"
```

## Task 4: Prevent APP-Only Runs From Lazy-Creating IM Cards

**Files:**
- Modify: `src-tauri/src/connector/im/shared/reply_manager.rs`
- Test: `src-tauri/src/connector/im/shared/reply_manager.rs`

- [ ] **Step 1: Write failing APP-only leak test**

Add:

```rust
#[tokio::test]
async fn stream_delta_without_registered_run_does_not_lazy_create_context() {
    let mgr = DingtalkReplyManager::new();
    mgr.remember_credentials(
        "sess-app-only".into(),
        "key".into(),
        "secret".into(),
        "robot".into(),
        CardTarget::Private {
            user_id: "user".into(),
        },
    )
    .await;

    let _ = mgr
        .on_event(&make_event(
            "sess-app-only",
            "app-run",
            RuntimeEventKind::StreamDelta {
                content: "APP 里普通回复".into(),
            },
        ))
        .await;

    let ctx = mgr.contexts.lock().await;
    assert!(
        !ctx.keys().any(|key| key.starts_with("sess-app-only\n")),
        "APP-only stream must not create IM card context from cached credentials"
    );
}
```

- [ ] **Step 2: Run test and confirm failure**

Run:

```bash
cd src-tauri
cargo test --lib connector::im::shared::reply_manager::tests::stream_delta_without_registered_run_does_not_lazy_create_context -- --nocapture
```

Expected: fails because `ensure_context_for_event` creates a context from cached credentials.

- [ ] **Step 3: Remove lazy context creation from runtime event flow**

Replace `ensure_context_for_event` with a pure check:

```rust
async fn has_matching_context_for_event(&self, session_id: &str, run_id: &str) -> bool {
    let contexts = self.contexts.lock().await;
    let key = card_context_key(session_id, run_id);
    contexts
        .get(&key)
        .map(|ctx| ctx.run_id == run_id)
        .unwrap_or(false)
}
```

Change `StreamDelta` handling:

```rust
if !self.has_matching_context_for_event(&session_id, &run_id).await {
    return Ok(());
}
```

Do not create a context from `session_credentials` in `on_event`.

- [ ] **Step 4: Keep direct IM run registration working**

Add this assertion test:

```rust
#[tokio::test]
async fn registered_im_run_still_accumulates_stream_delta() {
    let mgr = DingtalkReplyManager::new();
    {
        let mut ctx = mgr.contexts.lock().await;
        ctx.insert(card_context_key("sess-im", "run-im"), make_context("card-im", "run-im"));
    }

    let _ = mgr
        .on_event(&make_event(
            "sess-im",
            "run-im",
            RuntimeEventKind::StreamDelta {
                content: "IM 回复".into(),
            },
        ))
        .await;

    let ctx = mgr.contexts.lock().await;
    let key = card_context_key("sess-im", "run-im");
    assert_eq!(ctx[&key].accumulated_text, "IM 回复");
}
```

- [ ] **Step 5: Run reply manager tests**

Run:

```bash
cd src-tauri
cargo test --lib connector::im::shared::reply_manager::tests:: -- --nocapture
```

Expected: all reply manager tests pass.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/connector/im/shared/reply_manager.rs
git commit -m "fix: stop app-only replies from IM cards"
```

## Task 5: Add APP-Side Pending Interaction Feedback Coordinator

**Files:**
- Create: `src-tauri/src/connector/im/shared/app_feedback.rs`
- Modify: `src-tauri/src/connector/im/shared/mod.rs`
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Modify: `src-tauri/src/connector/im/shared/reply_manager.rs`
- Test: `src-tauri/src/connector/im/shared/app_feedback.rs`

- [ ] **Step 1: Create feedback coordinator tests**

Create `src-tauri/src/connector/im/shared/app_feedback.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::ids::{RunId, SessionId, ToolCallId};
    use crate::runtime::interaction::InteractionId;

    #[test]
    fn permission_feedback_messages_are_short_and_user_facing() {
        assert_eq!(
            feedback_message(AppFeedbackDecision::PermissionAllow { remember: false }),
            "已允许本次操作，任务继续执行。"
        );
        assert_eq!(
            feedback_message(AppFeedbackDecision::PermissionAllow { remember: true }),
            "已记录授权范围，任务继续执行。"
        );
        assert_eq!(
            feedback_message(AppFeedbackDecision::PermissionDeny),
            "已拒绝本次权限请求。"
        );
        assert_eq!(
            feedback_message(AppFeedbackDecision::PermissionCancel),
            "已取消当前任务。"
        );
    }

    #[test]
    fn interaction_feedback_messages_are_short_and_user_facing() {
        assert_eq!(
            feedback_message(AppFeedbackDecision::InteractionSubmit),
            "已提交你的回答，任务继续执行。"
        );
        assert_eq!(
            feedback_message(AppFeedbackDecision::InteractionCancel),
            "已取消这次提问。"
        );
    }

    #[test]
    fn routes_can_be_registered_and_taken_by_id() {
        let coordinator = IMAppFeedbackCoordinator::new();
        coordinator.register_permission(
            ToolCallId::new("tool-1"),
            SessionId::new("sess-im"),
            RunId::new("run-im"),
        );
        let route = coordinator.take_permission(&ToolCallId::new("tool-1"));
        assert_eq!(route.unwrap().session_id.as_str(), "sess-im");
        assert!(coordinator.take_permission(&ToolCallId::new("tool-1")).is_none());

        coordinator.register_interaction(
            InteractionId::new("ask-1"),
            SessionId::new("sess-im"),
            RunId::new("run-im"),
        );
        let route = coordinator.take_interaction(&InteractionId::new("ask-1"));
        assert_eq!(route.unwrap().run_id.as_str(), "run-im");
    }
}
```

- [ ] **Step 2: Implement coordinator types**

Implement above the tests:

```rust
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::runtime::ids::{RunId, SessionId, ToolCallId};
use crate::runtime::interaction::InteractionId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppFeedbackRoute {
    pub session_id: SessionId,
    pub run_id: RunId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppFeedbackDecision {
    PermissionAllow { remember: bool },
    PermissionDeny,
    PermissionCancel,
    InteractionSubmit,
    InteractionCancel,
}

pub fn feedback_message(decision: AppFeedbackDecision) -> &'static str {
    match decision {
        AppFeedbackDecision::PermissionAllow { remember: false } => {
            "已允许本次操作，任务继续执行。"
        }
        AppFeedbackDecision::PermissionAllow { remember: true } => {
            "已记录授权范围，任务继续执行。"
        }
        AppFeedbackDecision::PermissionDeny => "已拒绝本次权限请求。",
        AppFeedbackDecision::PermissionCancel => "已取消当前任务。",
        AppFeedbackDecision::InteractionSubmit => "已提交你的回答，任务继续执行。",
        AppFeedbackDecision::InteractionCancel => "已取消这次提问。",
    }
}

#[derive(Default)]
pub struct IMAppFeedbackCoordinator {
    permissions: Mutex<HashMap<String, AppFeedbackRoute>>,
    interactions: Mutex<HashMap<String, AppFeedbackRoute>>,
}

impl IMAppFeedbackCoordinator {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn register_permission(&self, id: ToolCallId, session_id: SessionId, run_id: RunId) {
        self.permissions.lock().unwrap().insert(
            id.as_str().to_string(),
            AppFeedbackRoute { session_id, run_id },
        );
    }

    pub fn register_interaction(&self, id: InteractionId, session_id: SessionId, run_id: RunId) {
        self.interactions.lock().unwrap().insert(
            id.as_str().to_string(),
            AppFeedbackRoute { session_id, run_id },
        );
    }

    pub fn take_permission(&self, id: &ToolCallId) -> Option<AppFeedbackRoute> {
        self.permissions.lock().unwrap().remove(id.as_str())
    }

    pub fn take_interaction(&self, id: &InteractionId) -> Option<AppFeedbackRoute> {
        self.interactions.lock().unwrap().remove(id.as_str())
    }

    pub fn clear_permission(&self, id: &ToolCallId) {
        self.permissions.lock().unwrap().remove(id.as_str());
    }

    pub fn clear_interaction(&self, id: &InteractionId) {
        self.interactions.lock().unwrap().remove(id.as_str());
    }
}
```

- [ ] **Step 3: Export the module**

In `src-tauri/src/connector/im/shared/mod.rs` add:

```rust
pub mod app_feedback;
```

- [ ] **Step 4: Wire registration from IMAskCoordinator**

Add an optional field:

```rust
app_feedback: Option<Arc<super::app_feedback::IMAppFeedbackCoordinator>>,
```

Add a constructor helper:

```rust
pub fn with_app_feedback(
    mut self,
    app_feedback: Arc<super::app_feedback::IMAppFeedbackCoordinator>,
) -> Self {
    self.app_feedback = Some(app_feedback);
    self
}
```

Inside `register_pending`, before storing `pending`, register:

```rust
if let Some(app_feedback) = self.app_feedback.as_ref() {
    match &kind {
        PendingAskKind::Permission { tool_call_id, .. } => {
            app_feedback.register_permission(
                tool_call_id.clone(),
                event.session_id.clone(),
                event.run_id.clone(),
            );
        }
        PendingAskKind::UserQuestion { interaction_id, .. } => {
            app_feedback.register_interaction(
                interaction_id.clone(),
                event.session_id.clone(),
                event.run_id.clone(),
            );
        }
    }
}
```

- [ ] **Step 5: Clear feedback route on IM-side resolution**

In `remove_pending_if_current`, after removing matching pending, clear the feedback route:

```rust
if let Some(removed) = removed {
    if let Some(app_feedback) = self.app_feedback.as_ref() {
        match removed.kind {
            PendingAskKind::Permission { tool_call_id, .. } => {
                app_feedback.clear_permission(&tool_call_id);
            }
            PendingAskKind::UserQuestion { interaction_id, .. } => {
                app_feedback.clear_interaction(&interaction_id);
            }
        }
    }
}
```

Implement this by changing `guard.remove(key);` into `let removed = guard.remove(key);` and using the snippet above after the lock guard is no longer needed.

- [ ] **Step 6: Add short feedback delivery to DingtalkReplyManager**

Add:

```rust
pub async fn deliver_app_feedback(
    &self,
    session_id: &SessionId,
    message: &str,
) -> anyhow::Result<()> {
    self.deliver_session_feedback_card(session_id, message.to_string()).await
}
```

- [ ] **Step 7: Run tests**

Run:

```bash
cd src-tauri
cargo test --lib connector::im::shared::app_feedback::tests:: connector::im::shared::ask_coordinator::tests:: -- --nocapture
```

Expected: tests pass.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/connector/im/shared/app_feedback.rs src-tauri/src/connector/im/shared/mod.rs src-tauri/src/connector/im/shared/ask_coordinator.rs src-tauri/src/connector/im/shared/reply_manager.rs
git commit -m "feat: track IM pending app feedback"
```

## Task 6: Notify IM/RM After APP Resolves Pending Interaction

**Files:**
- Modify: `src-tauri/src/runtime/interaction/control_plane.rs`
- Modify: `src-tauri/src/runtime/session_runtime.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/runtime/interaction/control_plane.rs`
- Test: `src-tauri/src/runtime/session_runtime.rs`

- [ ] **Step 1: Add pending interaction lookup test**

In `control_plane.rs` tests, add:

```rust
#[test]
fn get_pending_returns_cloned_interaction_request() {
    let cp = InMemoryInteractionControlPlane::new();
    let request = InteractionRequest {
        interaction_id: InteractionId::new("ask-1"),
        session_id: SessionId::new("sess-im"),
        run_id: RunId::new("run-im"),
        tool_call_id: ToolCallId::new("tool-1"),
        tool_name: "AskUserQuestion".into(),
        kind: InteractionKind::AskUserQuestion,
        payload: serde_json::json!({"questions": []}),
        original_request: RuntimeToolCallRequest {
            id: ToolCallId::new("tool-1"),
            name: "AskUserQuestion".into(),
            args: serde_json::json!({}),
        },
    };
    let _rx = cp.insert_pending(request).unwrap();

    let found = cp.get_pending(&InteractionId::new("ask-1")).unwrap();
    assert_eq!(found.session_id.as_str(), "sess-im");
    assert_eq!(found.run_id.as_str(), "run-im");
}
```

- [ ] **Step 2: Extend the trait and implementation**

Add to `PendingInteractionControlPlane`:

```rust
fn get_pending(&self, interaction_id: &InteractionId) -> Option<InteractionRequest>;
```

Implement:

```rust
fn get_pending(&self, interaction_id: &InteractionId) -> Option<InteractionRequest> {
    self.inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(interaction_id.as_str())
        .map(|entry| entry.request.clone())
}
```

- [ ] **Step 3: Add SessionRuntime lookup helpers**

In `SessionRuntime` add:

```rust
pub fn pending_permission_request_by_id(
    &self,
    tool_call_id: &ToolCallId,
) -> Option<crate::runtime::store::PendingPermissionRequest> {
    self.pending_permission_store.get(tool_call_id)
}

pub fn pending_interaction_request_by_id(
    &self,
    interaction_id: &InteractionId,
) -> Option<crate::runtime::interaction::InteractionRequest> {
    use crate::runtime::interaction::PendingInteractionControlPlane;
    self.pending_interaction_store.get_pending(interaction_id)
}
```

- [ ] **Step 4: Add adapter field**

Add to `TauriChatCommandAdapter`:

```rust
im_app_feedback: Option<Arc<crate::connector::im::shared::app_feedback::IMAppFeedbackCoordinator>>,
```

In constructors, default to `None`. Add builder:

```rust
pub fn with_im_app_feedback(
    mut self,
    feedback: Arc<crate::connector::im::shared::app_feedback::IMAppFeedbackCoordinator>,
) -> Self {
    self.im_app_feedback = Some(feedback);
    self
}
```

- [ ] **Step 5: Notify after permission approve succeeds**

In `approve_permission_request`, before resolving:

```rust
let tool_call = ToolCallId::new(tool_call_id.clone());
let pending_before = self.runtime.pending_permission_request_by_id(&tool_call);
```

After successful resolve:

```rust
if result.is_ok() {
    if let (Some(feedback), Some(_pending)) = (&self.im_app_feedback, pending_before) {
        if let Some(route) = feedback.take_permission(&tool_call) {
            let decision = crate::connector::im::shared::app_feedback::AppFeedbackDecision::PermissionAllow {
                remember: remember.unwrap_or(false),
            };
            let message = crate::connector::im::shared::app_feedback::feedback_message(decision);
            let _ = feedback.deliver(route, message).await;
        }
    }
}
```

Use the same pattern for deny and cancel with `PermissionDeny` and `PermissionCancel`.

- [ ] **Step 6: Give coordinator a delivery hook**

In `app_feedback.rs`, add a small sink trait:

```rust
#[async_trait::async_trait]
pub trait AppFeedbackSink: Send + Sync {
    async fn deliver_app_feedback(&self, session_id: &SessionId, message: &str) -> anyhow::Result<()>;
}
```

Change coordinator to store:

```rust
sink: Mutex<Option<Arc<dyn AppFeedbackSink>>>,
```

Add:

```rust
pub fn set_sink(&self, sink: Arc<dyn AppFeedbackSink>) {
    *self.sink.lock().unwrap() = Some(sink);
}

pub async fn deliver(&self, route: AppFeedbackRoute, message: &str) -> anyhow::Result<()> {
    let sink = self.sink.lock().unwrap().clone();
    let Some(sink) = sink else {
        return Ok(());
    };
    sink.deliver_app_feedback(&route.session_id, message).await
}
```

Implement `AppFeedbackSink` for `DingtalkReplyManager`.

- [ ] **Step 7: Notify after interaction submit/cancel succeeds**

In `submit_user_interaction`:

```rust
let interaction = InteractionId::new(interaction_id.clone());
let pending_before = self.runtime.pending_interaction_request_by_id(&interaction);
let result = self.runtime.resolve_interaction_request(
    &interaction,
    crate::runtime::interaction::InteractionResolution::Submit { value },
);
if result.is_ok() {
    if let (Some(feedback), Some(_pending)) = (&self.im_app_feedback, pending_before) {
        if let Some(route) = feedback.take_interaction(&interaction) {
            let message = crate::connector::im::shared::app_feedback::feedback_message(
                crate::connector::im::shared::app_feedback::AppFeedbackDecision::InteractionSubmit,
            );
            let _ = feedback.deliver(route, message).await;
        }
    }
}
result.map_err(|e| e.to_string())
```

Use `InteractionCancel` in `cancel_user_interaction`.

- [ ] **Step 8: Wire in lib.rs**

Near app setup, create:

```rust
let im_app_feedback = connector::im::shared::app_feedback::IMAppFeedbackCoordinator::new();
app.manage(im_app_feedback.clone());
```

When constructing `TauriChatCommandAdapter`, call:

```rust
.with_im_app_feedback(im_app_feedback.clone())
```

When constructing `DingtalkReplyManager`, set the sink:

```rust
im_app_feedback.set_sink(reply_manager.clone());
```

When constructing `IMAskCoordinator`, call:

```rust
.with_app_feedback(im_app_feedback.clone())
```

- [ ] **Step 9: Run tests**

Run:

```bash
cd src-tauri
cargo test --lib runtime::interaction::control_plane::tests:: runtime::session_runtime::tests:: connector::im::shared::app_feedback::tests:: -- --nocapture
cargo check
```

Expected: tests and check pass.

- [ ] **Step 10: Commit**

```bash
git add src-tauri/src/runtime/interaction/control_plane.rs src-tauri/src/runtime/session_runtime.rs src-tauri/src/transport/tauri_commands/chat.rs src-tauri/src/lib.rs src-tauri/src/connector/im/shared/app_feedback.rs src-tauri/src/connector/im/shared/reply_manager.rs
git commit -m "fix: notify IM after app pending resolution"
```

## Task 7: Prevent Abandoned Permission From Looking Resolved

**Files:**
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Test: `src-tauri/src/connector/im/shared/ask_coordinator.rs`

- [ ] **Step 1: Write stale permission test**

Add:

```rust
#[tokio::test]
async fn stale_permission_reply_does_not_claim_approval_success() {
    let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
    let interaction =
        Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
    let coordinator = make_coordinator_with_judge(
        permission,
        interaction,
        ScriptedJudge::one(PendingPermissionReplyIntent::Resolve {
            decision: ApprovalCommandDecision::AllowOnce,
            scope: None,
            reason: "user said yes later".into(),
        }),
    );
    coordinator.pending.lock().await.insert(
        "sess-im".into(),
        PendingAsk {
            run_id: RunId::new("run-old"),
            kind: PendingAskKind::Permission {
                tool_call_id: ToolCallId::new("tool-missing"),
                tool_name: "Read".into(),
                message: "read old file".into(),
                suggestions: vec![],
                path_auth_scope: None,
            },
            primary_model: "qwen-plus".into(),
        },
    );

    let outcome = coordinator
        .try_handle_reply(&SessionId::new("sess-im"), "刚刚那个权限我同意".into())
        .await
        .unwrap();

    assert_eq!(
        outcome,
        HandleOutcome::InvalidApprovalAction {
            message: "刚才那次权限请求已经失效，请重新发起需要权限的操作。".into()
        }
    );
}
```

- [ ] **Step 2: Update stale pending branch**

Change the stale live check:

```rust
if !self.is_pending_ask_live(&pending) {
    self.remove_pending_if_current(session_id, &pending).await;
    return Ok(match pending.kind {
        PendingAskKind::Permission { .. } => HandleOutcome::InvalidApprovalAction {
            message: "刚才那次权限请求已经失效，请重新发起需要权限的操作。".to_string(),
        },
        PendingAskKind::UserQuestion { .. } => HandleOutcome::NotPending,
    });
}
```

This prevents an old permission agreement from falling through into a normal model turn where the model can claim approval succeeded.

- [ ] **Step 3: Run tests**

Run:

```bash
cd src-tauri
cargo test --lib connector::im::shared::ask_coordinator::tests::stale_permission_reply_does_not_claim_approval_success connector::im::shared::ask_coordinator::tests:: -- --nocapture
```

Expected: tests pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/connector/im/shared/ask_coordinator.rs
git commit -m "fix: reject stale IM permission replies"
```

## Task 8: Full Verification

**Files:**
- No new files.
- Verify all touched Rust modules.

- [ ] **Step 1: Run IM ask coordinator tests**

```bash
cd src-tauri
cargo test --lib connector::im::shared::ask_coordinator::tests:: -- --nocapture
```

Expected: all ask coordinator tests pass.

- [ ] **Step 2: Run reply manager tests**

```bash
cd src-tauri
cargo test --lib connector::im::shared::reply_manager::tests:: -- --nocapture
```

Expected: all reply manager tests pass.

- [ ] **Step 3: Run app feedback and runtime lookup tests**

```bash
cd src-tauri
cargo test --lib connector::im::shared::app_feedback::tests:: runtime::interaction::control_plane::tests:: runtime::session_runtime::tests:: -- --nocapture
```

Expected: all tests pass.

- [ ] **Step 4: Run cargo check**

```bash
cd src-tauri
cargo check
```

Expected: build check succeeds.

- [ ] **Step 5: Optional manual DingTalk smoke test**

Run the app:

```bash
pnpm run tauri:dev
```

Manual scenario:

1. DingTalk asks to read `/tmp/aijia-permission-test/secret3.txt`.
2. When permission card appears, send “问我三个问题”.
3. Verify the old read card text remains unchanged.
4. Verify the new question run appears once.
5. Verify permission and question cards do not show `/approve`, `/answer`, `call_00`, or interaction ids.
6. In APP, send an ordinary message in the same conversation and verify DingTalk does not receive the normal AI reply.
7. Trigger a pending permission from DingTalk, approve it in APP, and verify DingTalk receives only the short status feedback.

- [ ] **Step 6: Final commit if manual-only adjustments were needed**

If no manual-only code changes were needed, skip this step. If small manual-smoke fixes were needed:

```bash
git add <changed-files>
git commit -m "fix: polish IM run-scoped interaction output"
```
