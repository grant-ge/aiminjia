# IM Pending Interaction Routing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make IM/app replies during AskUserQuestion or permission approval resume the suspended interaction by default, while using pending queue only when a run is actively busy.

**Architecture:** Keep `IMAskCoordinator` as the deterministic pre-dispatch router, but change its default behavior for live pending interactions. AskUserQuestion free text resolves `InteractionResolution::Submit`; permission free text is first captured so it never becomes an accidental new turn, then simple absolute-path grant text resolves through a structured `path_auth_scope_override` that `SessionRuntime` persists through the existing path-auth store. Pending ask state becomes run-aware so a different run's completion cannot clear an older suspended interaction.

**Tech Stack:** Rust, Tokio, Tauri runtime events, existing `PendingInteractionControlPlane`, existing `PendingPermissionControlPlane`, focused Rust unit tests with `cargo test`.

---

## File Structure

- Modify `src-tauri/src/connector/im/shared/ask_coordinator.rs`
  - Owns IM pre-dispatch routing for pending AskUserQuestion and permission approval.
  - Add free-text answer shaping for AskUserQuestion.
  - Add explicit cancel/new-turn phrase detection.
  - Change pending storage from `HashMap<String, PendingAsk>` to a session-indexed run-aware entry.
  - Parse simple permission text such as `以后 /tmp 这个目录下的文件都可以读` into a validated path-auth override.
  - Update tests in the existing `#[cfg(test)]` module.

- Modify `src-tauri/src/runtime/store/pending_permission_request_store.rs`
  - Add `path_auth_scope_override` to `PendingPermissionResolution::Allow`.
  - Keep the pending store itself as a resolver; it still does not write permissions directly.

- Modify `src-tauri/src/runtime/session_runtime.rs`
  - Prefer `path_auth_scope_override` over the original pending request scope when persisting remembered allow decisions.
  - Continue to use `persist_path_auth_grant` for actual path-auth store writes.

- Modify `src-tauri/src/runtime/chat/chat_turn_driver.rs`
  - Include `path_auth_scope` in permission ask events so IM routing can know whether a natural-language scope override is valid for this request.

- Modify `src-tauri/src/runtime/events.rs`
  - Add `path_auth_scope: Option<String>` to `RuntimeEventKind::PermissionAskRequired`.

- Test via existing Rust tests in:
  - `src-tauri/src/connector/im/shared/ask_coordinator.rs`
  - `src-tauri/src/runtime/session_runtime.rs`
  - `src-tauri/src/runtime/chat/chat_turn_driver.rs`

Do not modify `src-tauri/src/runtime/interaction/control_plane.rs` for this batch; AskUserQuestion already resolves by `interaction_id`, and run-aware cleanup can live inside `IMAskCoordinator`.

## Task 1: AskUserQuestion Free-Text Resume

**Files:**
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Test: `src-tauri/src/connector/im/shared/ask_coordinator.rs`

- [ ] **Step 1: Write failing tests for ordinary AskUserQuestion replies**

Add these tests inside the existing `#[cfg(test)] mod tests` in `src-tauri/src/connector/im/shared/ask_coordinator.rs`:

```rust
#[tokio::test]
async fn ordinary_user_question_reply_resolves_as_answer() {
    let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
    let interaction =
        Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
    let mut resolution_rx = interaction
        .insert_pending(interaction_request("ask-1"))
        .expect("interaction insert");
    let coordinator = IMAskCoordinator::new(
        Arc::new(Registry(true)),
        Arc::new(RecordingSink {
            calls: StdMutex::new(Vec::new()),
        }),
        permission,
        interaction,
    );
    coordinator.pending.lock().await.insert(
        "sess-im".into(),
        PendingAsk {
            kind: PendingAskKind::UserQuestion {
                interaction_id: InteractionId::new("ask-1"),
                tool_call_id: ToolCallId::new("tool-1"),
                questions: serde_json::json!({
                    "questions": [
                        { "id": "domain", "question": "专业领域" },
                        { "id": "help", "question": "最需要协助" },
                        { "id": "style", "question": "输出风格" }
                    ]
                }),
            },
            primary_model: "qwen-plus".into(),
        },
    );

    let outcome = coordinator
        .try_handle_reply(
            &SessionId::new("sess-im"),
            "HR/人事\n数据处理与分析\n结论优先".into(),
        )
        .await
        .unwrap();

    assert_eq!(outcome, HandleOutcome::AnswerResolved);
    match resolution_rx.try_recv().expect("interaction should resolve") {
        InteractionResolution::Submit { value } => {
            assert_eq!(
                value,
                serde_json::json!({
                    "answers": {
                        "domain": "HR/人事",
                        "help": "数据处理与分析",
                        "style": "结论优先"
                    },
                    "rawText": "HR/人事\n数据处理与分析\n结论优先"
                })
            );
        }
        other => panic!("expected submit resolution, got {:?}", other),
    }
    assert!(
        !coordinator.pending.lock().await.contains_key("sess-im"),
        "resolved AskUserQuestion should be removed"
    );
}

#[tokio::test]
async fn ordinary_user_question_reply_without_question_ids_keeps_raw_text() {
    let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
    let interaction =
        Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
    let mut resolution_rx = interaction
        .insert_pending(interaction_request("ask-1"))
        .expect("interaction insert");
    let coordinator = IMAskCoordinator::new(
        Arc::new(Registry(true)),
        Arc::new(RecordingSink {
            calls: StdMutex::new(Vec::new()),
        }),
        permission,
        interaction,
    );
    coordinator.pending.lock().await.insert(
        "sess-im".into(),
        PendingAsk {
            kind: PendingAskKind::UserQuestion {
                interaction_id: InteractionId::new("ask-1"),
                tool_call_id: ToolCallId::new("tool-1"),
                questions: serde_json::json!({
                    "questions": [
                        { "question": "专业领域" },
                        { "question": "最需要协助" }
                    ]
                }),
            },
            primary_model: "qwen-plus".into(),
        },
    );

    let outcome = coordinator
        .try_handle_reply(&SessionId::new("sess-im"), "HR/人事\n数据处理与分析".into())
        .await
        .unwrap();

    assert_eq!(outcome, HandleOutcome::AnswerResolved);
    match resolution_rx.try_recv().expect("interaction should resolve") {
        InteractionResolution::Submit { value } => {
            assert_eq!(
                value,
                serde_json::json!({
                    "answers": {
                        "专业领域": "HR/人事",
                        "最需要协助": "数据处理与分析"
                    },
                    "rawText": "HR/人事\n数据处理与分析"
                })
            );
        }
        other => panic!("expected submit resolution, got {:?}", other),
    }
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app/src-tauri
cargo test connector::im::shared::ask_coordinator::tests::ordinary_user_question_reply_resolves_as_answer --lib
cargo test connector::im::shared::ask_coordinator::tests::ordinary_user_question_reply_without_question_ids_keeps_raw_text --lib
```

Expected: both tests fail with `HandleOutcome::NotPending` or missing expected submit value.

- [ ] **Step 3: Add answer shaping helpers**

In `src-tauri/src/connector/im/shared/ask_coordinator.rs`, add these helpers above `parse_pending_action_command`:

```rust
fn build_user_question_free_text_answer(
    questions_payload: &serde_json::Value,
    content: &str,
) -> serde_json::Value {
    let trimmed = content.trim();
    let lines: Vec<&str> = trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let questions = questions_payload
        .get("questions")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let mut answers = serde_json::Map::new();
    for (index, question) in questions.iter().enumerate() {
        let Some(answer) = lines.get(index).copied() else {
            break;
        };
        let key = question
            .get("id")
            .and_then(|value| value.as_str())
            .or_else(|| question.get("question").and_then(|value| value.as_str()))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("question_{}", index + 1));
        answers.insert(key, serde_json::Value::String(answer.to_string()));
    }

    if answers.is_empty() {
        serde_json::json!({
            "answers": { "answer": trimmed },
            "rawText": trimmed
        })
    } else {
        serde_json::json!({
            "answers": serde_json::Value::Object(answers),
            "rawText": trimmed
        })
    }
}

fn is_cancel_or_topic_change(content: &str) -> bool {
    let normalized: String = content
        .trim()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect();
    matches!(
        normalized.as_str(),
        "算了" | "别问了" | "取消" | "不用了" | "换个事" | "先不回答这个"
    ) || normalized.starts_with("算了，")
        || normalized.starts_with("算了,")
        || normalized.contains("看看别的文件")
}
```

- [ ] **Step 4: Route ordinary AskUserQuestion text to submit**

In `try_handle_reply`, replace the final `(_, None) => Ok(HandleOutcome::NotPending),` arm with these two arms before the final fallthrough:

```rust
            (
                PendingAskKind::UserQuestion {
                    questions,
                    ..
                },
                None,
            ) if is_cancel_or_topic_change(&content) => {
                if !self.resolve_abandoned(&pending, content.clone())? {
                    return Ok(HandleOutcome::InvalidApprovalAction {
                        message: "当前提问已失效，请重新发送你的请求。".to_string(),
                    });
                }
                self.remove_pending_if_current(session_id, &pending).await;
                Ok(HandleOutcome::AnswerResolved)
            }
            (
                PendingAskKind::UserQuestion {
                    questions,
                    ..
                },
                None,
            ) => {
                let value = build_user_question_free_text_answer(questions, &content);
                if !self.resolve_user_question_answer(&pending, value)? {
                    return Ok(HandleOutcome::InvalidApprovalAction {
                        message: "当前提问已失效，请重新发送你的请求。".to_string(),
                    });
                }
                self.remove_pending_if_current(session_id, &pending).await;
                Ok(HandleOutcome::AnswerResolved)
            }
            (_, None) => Ok(HandleOutcome::NotPending),
```

If the compiler warns that `questions` is unused in the cancel arm, replace the pattern with `PendingAskKind::UserQuestion { .. }`.

- [ ] **Step 5: Run focused tests**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app/src-tauri
cargo test connector::im::shared::ask_coordinator::tests::ordinary_user_question_reply_resolves_as_answer --lib
cargo test connector::im::shared::ask_coordinator::tests::ordinary_user_question_reply_without_question_ids_keeps_raw_text --lib
cargo test connector::im::shared::ask_coordinator::tests::explicit_user_question_answer_resolves_control_plane --lib
cargo test connector::im::shared::ask_coordinator::tests::explicit_user_question_cancel_resolves_control_plane --lib
```

Expected: all four tests pass.

- [ ] **Step 6: Commit Task 1**

```bash
git add src-tauri/src/connector/im/shared/ask_coordinator.rs
git commit -m "fix: resume IM user questions from free text"
```

## Task 2: Run-Aware Pending Ask Cleanup

**Files:**
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Test: `src-tauri/src/connector/im/shared/ask_coordinator.rs`

- [ ] **Step 1: Write failing test for cross-run completion**

Replace the current test `turn_completed_clears_pending_ask_for_session` with:

```rust
#[tokio::test]
async fn turn_completed_for_other_run_does_not_clear_pending_ask() {
    let coordinator = make_coordinator();
    coordinator.pending.lock().await.insert(
        "sess-im".into(),
        PendingAsk {
            run_id: RunId::new("run-waiting"),
            kind: PendingAskKind::UserQuestion {
                interaction_id: InteractionId::new("ask-1"),
                tool_call_id: ToolCallId::new("tool-1"),
                questions: serde_json::json!({"questions": []}),
            },
            primary_model: "qwen-plus".into(),
        },
    );

    coordinator
        .on_event(&RuntimeEvent::new(
            SessionId::new("sess-im"),
            RunId::new("run-new-turn"),
            RuntimeEventKind::TurnCompleted {
                outcome: ChatTurnOutcome::Success,
                total_input_tokens: 0,
                total_output_tokens: 0,
                total_cache_creation_input_tokens: 0,
                total_cache_read_input_tokens: 0,
                total_cost_usd: None,
                permission_denial_count: 0,
            },
        ))
        .await
        .unwrap();

    assert!(
        coordinator.pending.lock().await.contains_key("sess-im"),
        "a different run completing must not clear suspended AskUserQuestion"
    );
}

#[tokio::test]
async fn run_cancelled_for_same_run_clears_pending_ask() {
    let coordinator = make_coordinator();
    coordinator.pending.lock().await.insert(
        "sess-im".into(),
        PendingAsk {
            run_id: RunId::new("run-waiting"),
            kind: PendingAskKind::UserQuestion {
                interaction_id: InteractionId::new("ask-1"),
                tool_call_id: ToolCallId::new("tool-1"),
                questions: serde_json::json!({"questions": []}),
            },
            primary_model: "qwen-plus".into(),
        },
    );

    coordinator
        .on_event(&RuntimeEvent::new(
            SessionId::new("sess-im"),
            RunId::new("run-waiting"),
            RuntimeEventKind::RunCancelled,
        ))
        .await
        .unwrap();

    assert!(
        !coordinator.pending.lock().await.contains_key("sess-im"),
        "same run cancellation should clear its pending AskUserQuestion"
    );
}
```

- [ ] **Step 2: Run tests and verify they fail to compile**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app/src-tauri
cargo test connector::im::shared::ask_coordinator::tests::turn_completed_for_other_run_does_not_clear_pending_ask --lib
```

Expected: compile fails because `PendingAsk` has no `run_id`.

- [ ] **Step 3: Add run id to pending ask state**

Change `PendingAsk` to:

```rust
#[derive(Debug, Clone)]
struct PendingAsk {
    run_id: crate::runtime::ids::RunId,
    kind: PendingAskKind,
    primary_model: String,
}
```

Update `register_pending`:

```rust
        let pending = PendingAsk {
            run_id: event.run_id.clone(),
            kind,
            primary_model,
        };
```

Update every test-created `PendingAsk` with `run_id: RunId::new("run-1"),` unless the test needs a different run id.

- [ ] **Step 4: Replace session-only cleanup with run-aware cleanup**

Replace `remove_pending_for_session` with:

```rust
    async fn remove_pending_for_run(
        &self,
        session_id: &SessionId,
        run_id: &crate::runtime::ids::RunId,
        reason: &str,
    ) {
        let mut guard = self.pending.lock().await;
        let key = session_id.as_str();
        let should_remove = guard
            .get(key)
            .is_some_and(|pending| pending.run_id == *run_id);
        if should_remove {
            let removed = guard.remove(key);
            if let Some(pending) = removed {
                log::info!(
                    "[im-ask] removed pending ask session={} run={} kind={} reason={}",
                    session_id.as_str(),
                    run_id.as_str(),
                    match &pending.kind {
                        PendingAskKind::Permission { .. } => "permission",
                        PendingAskKind::UserQuestion { .. } => "user_question",
                    },
                    reason
                );
            }
        }
    }
```

Change event handling:

```rust
            RuntimeEventKind::TurnCompleted { .. } => {
                self.remove_pending_for_run(&event.session_id, &event.run_id, "turn_completed")
                    .await;
                Ok(())
            }
            RuntimeEventKind::RunCancelled | RuntimeEventKind::RunCompleted => {
                self.remove_pending_for_run(&event.session_id, &event.run_id, "run_finished")
                    .await;
                Ok(())
            }
```

- [ ] **Step 5: Run focused cleanup tests**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app/src-tauri
cargo test connector::im::shared::ask_coordinator::tests::turn_completed_for_other_run_does_not_clear_pending_ask --lib
cargo test connector::im::shared::ask_coordinator::tests::run_cancelled_for_same_run_clears_pending_ask --lib
cargo test connector::im::shared::ask_coordinator::tests::deadline_does_not_auto_resolve_permission_or_user_question --lib
```

Expected: all three tests pass.

- [ ] **Step 6: Commit Task 2**

```bash
git add src-tauri/src/connector/im/shared/ask_coordinator.rs
git commit -m "fix: keep IM pending asks scoped to run"
```

## Task 3: Permission Free Text Does Not Fall Through

**Files:**
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Test: `src-tauri/src/connector/im/shared/ask_coordinator.rs`

- [ ] **Step 1: Replace old permission fallthrough tests**

Remove or rewrite these old-meaning tests:

- `ordinary_message_falls_through_while_permission_is_pending`
- `concurrent_ordinary_replies_fall_through_without_clearing_pending`
- `embedded_permission_shortcut_phrase_falls_through`

Add these tests:

```rust
#[tokio::test]
async fn permission_natural_language_scope_reply_is_captured() {
    let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
    let _resolution_rx = permission.insert(permission_request("tool-1")).unwrap();
    let interaction =
        Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
    let coordinator = IMAskCoordinator::new(
        Arc::new(Registry(true)),
        Arc::new(RecordingSink {
            calls: StdMutex::new(Vec::new()),
        }),
        permission,
        interaction,
    );
    coordinator.pending.lock().await.insert(
        "sess-im".into(),
        PendingAsk {
            run_id: RunId::new("run-1"),
            kind: PendingAskKind::Permission {
                tool_call_id: ToolCallId::new("tool-1"),
                tool_name: "Read".into(),
                message: "read file".into(),
                suggestions: vec![],
            },
            primary_model: "qwen-plus".into(),
        },
    );

    let outcome = coordinator
        .try_handle_reply(
            &SessionId::new("sess-im"),
            "以后 /tmp 这个目录下的文件都可以读".into(),
        )
        .await
        .unwrap();

    assert_eq!(
        outcome,
        HandleOutcome::InvalidApprovalAction {
            message: "我收到了你的授权说明，但当前版本还不能安全解析范围，请使用卡片按钮或 /approve 指令处理这次审批。".into(),
        }
    );
    assert!(
        coordinator.pending.lock().await.contains_key("sess-im"),
        "natural-language permission text must not fall through as a new turn"
    );
}

#[tokio::test]
async fn permission_cancel_and_new_turn_phrase_cancels_pending() {
    let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
    let mut resolution_rx = permission.insert(permission_request("tool-1")).unwrap();
    let interaction =
        Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
    let coordinator = IMAskCoordinator::new(
        Arc::new(Registry(true)),
        Arc::new(RecordingSink {
            calls: StdMutex::new(Vec::new()),
        }),
        permission,
        interaction,
    );
    coordinator.pending.lock().await.insert(
        "sess-im".into(),
        PendingAsk {
            run_id: RunId::new("run-1"),
            kind: PendingAskKind::Permission {
                tool_call_id: ToolCallId::new("tool-1"),
                tool_name: "Read".into(),
                message: "read file".into(),
                suggestions: vec![],
            },
            primary_model: "qwen-plus".into(),
        },
    );

    let outcome = coordinator
        .try_handle_reply(&SessionId::new("sess-im"), "算了，看看别的文件".into())
        .await
        .unwrap();

    assert_eq!(outcome, HandleOutcome::ApprovalResolved);
    match resolution_rx.try_recv().expect("permission should resolve") {
        PendingPermissionResolution::Deny { message, .. } => {
            assert!(message.contains("算了，看看别的文件"));
        }
        other => panic!("expected deny/cancel resolution, got {:?}", other),
    }
}
```

- [ ] **Step 2: Run tests and verify the first one fails**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app/src-tauri
cargo test connector::im::shared::ask_coordinator::tests::permission_natural_language_scope_reply_is_captured --lib
```

Expected: fails because current code returns `HandleOutcome::NotPending`.

- [ ] **Step 3: Add permission natural-language capture**

In `try_handle_reply`, after the permission shortcut arm and before AskUserQuestion free-text arms, add:

```rust
            (PendingAskKind::Permission { .. }, None) if is_cancel_or_topic_change(&content) => {
                if !self.resolve_abandoned(&pending, content.clone())? {
                    return Ok(HandleOutcome::InvalidApprovalAction {
                        message: "当前审批已失效，请重新发送你的请求。".to_string(),
                    });
                }
                self.remove_pending_if_current(session_id, &pending).await;
                Ok(HandleOutcome::ApprovalResolved)
            }
            (PendingAskKind::Permission { .. }, None) => Ok(HandleOutcome::InvalidApprovalAction {
                message: "我收到了你的授权说明，但当前版本还不能安全解析范围，请使用卡片按钮或 /approve 指令处理这次审批。".to_string(),
            }),
```

This deliberately blocks fallthrough first. The safe parser and permission-store persistence can be added in the next task without letting ambiguous approval text become a new chat turn.

- [ ] **Step 4: Run focused permission tests**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app/src-tauri
cargo test connector::im::shared::ask_coordinator::tests::permission_shortcut_allow_once_resolves_control_plane --lib
cargo test connector::im::shared::ask_coordinator::tests::permission_shortcut_allow_always_remembers_user --lib
cargo test connector::im::shared::ask_coordinator::tests::permission_natural_language_scope_reply_is_captured --lib
cargo test connector::im::shared::ask_coordinator::tests::permission_cancel_and_new_turn_phrase_cancels_pending --lib
cargo test connector::im::shared::ask_coordinator::tests::invalid_approval_command_is_rejected_without_clearing_pending --lib
```

Expected: all five tests pass.

- [ ] **Step 5: Commit Task 3**

```bash
git add src-tauri/src/connector/im/shared/ask_coordinator.rs
git commit -m "fix: keep permission replies in pending interaction"
```

## Task 4: Permission Path Scope Override

**Files:**
- Modify: `src-tauri/src/runtime/store/pending_permission_request_store.rs`
- Modify: `src-tauri/src/runtime/session_runtime.rs`
- Modify: `src-tauri/src/runtime/events.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Test: `src-tauri/src/connector/im/shared/ask_coordinator.rs`
- Test: `src-tauri/src/runtime/session_runtime.rs`

- [ ] **Step 1: Write failing test for natural-language path grant resolution**

Add this test inside `src-tauri/src/connector/im/shared/ask_coordinator.rs`:

```rust
#[tokio::test]
async fn permission_natural_language_path_scope_resolves_remembered_allow() {
    let temp = tempfile::tempdir().expect("tempdir");
    let canonical = std::fs::canonicalize(temp.path()).expect("canonical tempdir");
    let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
    let mut request = permission_request("tool-1");
    request.path_auth_scope = Some(format!("path:{}/secret.txt", canonical.display()));
    let mut resolution_rx = permission.insert(request).unwrap();
    let interaction =
        Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
    let coordinator = IMAskCoordinator::new(
        Arc::new(Registry(true)),
        Arc::new(RecordingSink {
            calls: StdMutex::new(Vec::new()),
        }),
        permission,
        interaction,
    );
    coordinator.pending.lock().await.insert(
        "sess-im".into(),
        PendingAsk {
            run_id: RunId::new("run-1"),
            kind: PendingAskKind::Permission {
                tool_call_id: ToolCallId::new("tool-1"),
                tool_name: "Read".into(),
                message: "read file".into(),
                suggestions: vec![],
                path_auth_scope: Some(format!("path:{}/secret.txt", canonical.display())),
            },
            primary_model: "qwen-plus".into(),
        },
    );

    let outcome = coordinator
        .try_handle_reply(
            &SessionId::new("sess-im"),
            format!("可以的，以后 {} 这个目录下的文件都可以读", canonical.display()),
        )
        .await
        .unwrap();

    assert_eq!(outcome, HandleOutcome::ApprovalResolved);
    match resolution_rx.try_recv().expect("permission should resolve") {
        PendingPermissionResolution::Allow {
            remember,
            destination,
            path_auth_scope_override,
            ..
        } => {
            assert!(remember);
            assert_eq!(destination, Some(PermissionDestination::User));
            assert_eq!(
                path_auth_scope_override,
                Some(format!("path:{}", canonical.display()))
            );
        }
        other => panic!("expected allow resolution, got {:?}", other),
    }
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app/src-tauri
cargo test connector::im::shared::ask_coordinator::tests::permission_natural_language_path_scope_resolves_remembered_allow --lib
```

Expected: compile fails because `PendingAskKind::Permission` and `PendingPermissionResolution::Allow` do not have `path_auth_scope` / `path_auth_scope_override`.

- [ ] **Step 3: Extend permission resolution type**

In `src-tauri/src/runtime/store/pending_permission_request_store.rs`, change `PendingPermissionResolution::Allow`:

```rust
Allow {
    updated_input: Option<Value>,
    remember: bool,
    destination: Option<PermissionDestination>,
    message: Option<String>,
    path_auth_scope_override: Option<String>,
},
```

Then update every existing `PendingPermissionResolution::Allow { ... }` construction found by:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app
rg -n "PendingPermissionResolution::Allow|ApprovalCommandDecision::Allow" src-tauri/src src-tauri/tests
```

For existing explicit allow paths, add:

```rust
path_auth_scope_override: None,
```

- [ ] **Step 4: Persist override scope through SessionRuntime**

In `src-tauri/src/runtime/session_runtime.rs`, change the `Allow` match in `persist_resolved_permission`:

```rust
        let (remember, destination, decision, path_auth_scope_override) = match resolution {
            PendingPermissionResolution::Allow {
                remember,
                destination,
                path_auth_scope_override,
                ..
            } => (
                *remember,
                *destination,
                PolicyDecision::Allow,
                path_auth_scope_override.as_ref(),
            ),
            PendingPermissionResolution::Deny {
                remember,
                destination,
                ..
            } => (*remember, *destination, PolicyDecision::Deny, None),
            PendingPermissionResolution::Cancel { .. } => return,
        };
```

Replace:

```rust
        if let Some(scope) = pending_request.path_auth_scope.as_ref() {
```

with:

```rust
        let path_auth_scope = path_auth_scope_override.or(pending_request.path_auth_scope.as_ref());
        if let Some(scope) = path_auth_scope {
```

- [ ] **Step 5: Carry path auth scope in runtime events**

In `src-tauri/src/runtime/events.rs`, add to `RuntimeEventKind::PermissionAskRequired`:

```rust
path_auth_scope: Option<String>,
```

In `src-tauri/src/runtime/chat/chat_turn_driver.rs`, add the field when emitting `PermissionAskRequired`:

```rust
path_auth_scope: path_auth_scope.clone(),
```

Update any test-created `RuntimeEventKind::PermissionAskRequired` with `path_auth_scope: None`.

- [ ] **Step 6: Carry path auth scope in IM pending ask**

In `src-tauri/src/connector/im/shared/ask_coordinator.rs`, extend `PendingAskKind::Permission`:

```rust
Permission {
    tool_call_id: ToolCallId,
    tool_name: String,
    message: String,
    suggestions: Vec<String>,
    path_auth_scope: Option<String>,
},
```

Update `on_event` registration:

```rust
PendingAskKind::Permission {
    tool_call_id: tool_call_id.clone(),
    tool_name: tool_name.clone(),
    message: message.clone(),
    suggestions: suggestions.clone(),
    path_auth_scope: path_auth_scope.clone(),
}
```

Update all test `PendingAskKind::Permission` literals with `path_auth_scope: None` unless the test needs a scope.

- [ ] **Step 7: Add path extraction and scope override parser**

Add this helper in `src-tauri/src/connector/im/shared/ask_coordinator.rs`:

```rust
fn parse_permission_path_scope_override(
    content: &str,
    current_path_auth_scope: Option<&str>,
) -> Option<String> {
    let current = current_path_auth_scope?;
    if !content.contains("以后") && !content.contains("都可以") && !content.contains("永久") {
        return None;
    }
    let raw_path = content
        .split_whitespace()
        .find(|part| part.starts_with('/'))?
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(ch, '`' | '"' | '\'' | '“' | '”' | '。' | '，' | ',' | '；' | ';')
        });
    let canonical = crate::runtime::path_auth::decide::canonicalize_or_ancestor(
        std::path::Path::new(raw_path),
    )
    .ok()?;
    let kind = if current.starts_with("pathwrite:") {
        "pathwrite"
    } else {
        "path"
    };
    Some(format!("{}:{}", kind, canonical.display()))
}
```

- [ ] **Step 8: Resolve natural-language path grant**

In `try_handle_reply`, place this arm before the generic permission natural-language capture arm:

```rust
            (
                PendingAskKind::Permission {
                    path_auth_scope,
                    ..
                },
                None,
            ) if parse_permission_path_scope_override(&content, path_auth_scope.as_deref()).is_some() =>
            {
                let override_scope = parse_permission_path_scope_override(
                    &content,
                    path_auth_scope.as_deref(),
                )
                .expect("guard checked path override");
                if let PendingAskKind::Permission { tool_call_id, .. } = &pending.kind {
                    self.permission_cp.resolve_pending_request(
                        tool_call_id,
                        PendingPermissionResolution::Allow {
                            updated_input: None,
                            remember: true,
                            destination: Some(PermissionDestination::User),
                            message: Some(content.clone()),
                            path_auth_scope_override: Some(override_scope),
                        },
                    )?;
                    self.remove_pending_if_current(session_id, &pending).await;
                    Ok(HandleOutcome::ApprovalResolved)
                } else {
                    Ok(HandleOutcome::NotPending)
                }
            }
```

- [ ] **Step 9: Add persistence test for override**

Add this test in `src-tauri/src/runtime/session_runtime.rs` near the existing path-auth persistence tests:

```rust
#[test]
fn permanent_allow_with_path_auth_scope_override_persists_override() {
    let pending_permission_store = Arc::new(PendingPermissionRequestStore::new());
    let permission_store = Arc::new(crate::runtime::store::PermissionStore::in_memory());
    let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new())
        .with_pending_permission_store(pending_permission_store.clone())
        .with_permission_store(permission_store.clone());
    let tool_call_id = ToolCallId::new("tool-override");
    let pending = PendingPermissionRequest {
        tool_call_id: tool_call_id.clone(),
        session_id: SessionId::new("sess-override"),
        run_id: RunId::new("run-override"),
        tool_name: "Read".into(),
        capability_scopes: vec!["fs:read".into()],
        message: "read file".into(),
        suggestions: vec![],
        mode: PermissionMode::Default,
        remember_options: vec![PermissionDestination::User],
        default_destination: Some(PermissionDestination::User),
        original_request: RuntimeToolCallRequest {
            tool_call_id: "tool-override".into(),
            tool_name: "Read".into(),
            args: serde_json::json!({}),
            purpose: None,
        },
        path_auth_scope: Some("path:/Users/example/Old".to_string()),
    };
    let _rx = pending_permission_store.insert(pending).unwrap();

    runtime
        .resolve_permission_request(
            &tool_call_id,
            PendingPermissionResolution::Allow {
                updated_input: None,
                remember: true,
                destination: Some(PermissionDestination::User),
                message: None,
                path_auth_scope_override: Some("path:/Users/example/New".to_string()),
            },
        )
        .unwrap();

    let entries = crate::runtime::path_auth::load_path_auth_entries(&permission_store);
    assert!(
        entries
            .working_dirs
            .contains_key(&std::path::PathBuf::from("/Users/example/New")),
        "override path should be persisted instead of original scope: {:?}",
        entries.working_dirs
    );
}
```

- [ ] **Step 10: Run focused permission path tests**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app/src-tauri
cargo test connector::im::shared::ask_coordinator::tests::permission_natural_language_path_scope_resolves_remembered_allow --lib
cargo test runtime::session_runtime::tests::permanent_allow_with_path_auth_scope_override_persists_override --lib
```

Expected: both tests pass.

- [ ] **Step 11: Commit Task 4**

```bash
git add src-tauri/src/runtime/store/pending_permission_request_store.rs src-tauri/src/runtime/session_runtime.rs src-tauri/src/runtime/events.rs src-tauri/src/runtime/chat/chat_turn_driver.rs src-tauri/src/connector/im/shared/ask_coordinator.rs
git commit -m "fix: persist natural language path permission grants"
```

## Task 5: Runtime Suspension Does Not Count As Active Busy

**Files:**
- Test: `src-tauri/src/runtime/chat/chat_turn_driver.rs`

- [ ] **Step 1: Verify existing suspension tests are present**

Confirm these two tests exist in `src-tauri/src/runtime/chat/chat_turn_driver.rs`:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app
rg -n "resolve_permission_asks_suspends_active_run_while_waiting_for_user|resolve_interaction_requests_suspends_active_run_while_waiting_for_user" src-tauri/src/runtime/chat/chat_turn_driver.rs
```

Expected: both test names are printed. These tests are the current contract that waiting for permission or AskUserQuestion releases active busy and resumes afterward.

- [ ] **Step 2: Run the permission suspension test**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app/src-tauri
cargo test runtime::chat::chat_turn_driver::tests::resolve_permission_asks_suspends_active_run_while_waiting_for_user --lib
```

Expected: pass, with `activity.calls()` equal to `["suspend:conv-wait:run-wait", "resume:conv-wait:run-wait"]`.

- [ ] **Step 3: Run the AskUserQuestion suspension test**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app/src-tauri
cargo test runtime::chat::chat_turn_driver::tests::resolve_interaction_requests_suspends_active_run_while_waiting_for_user --lib
```

Expected: pass, with `activity.calls()` equal to `["suspend:conv-question:run-question", "resume:conv-question:run-question"]`.

- [ ] **Step 4: Do not change runtime suspension code in this implementation batch**

Record in the task notes:

```text
Runtime suspension contract already exists and passed. This batch keeps runtime busy-state code unchanged and focuses implementation on IM pre-dispatch routing plus run-aware pending cleanup.
```

- [ ] **Step 5: Commit only if test names or assertions were intentionally updated**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs
git commit -m "test: guard user interaction suspension contract"
```

## Task 6: Full Verification

**Files:**
- No source changes expected.

- [ ] **Step 1: Run coordinator test suite**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app/src-tauri
cargo test connector::im::shared::ask_coordinator::tests --lib
```

Expected: all `ask_coordinator` tests pass.

- [ ] **Step 2: Run interaction and stage tests**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app/src-tauri
cargo test --test ask_user_question_test
cargo test --test review_turn_stages_test
```

Expected: both test binaries pass.

- [ ] **Step 3: Run compile check**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/9a36/lotus-app/src-tauri
cargo check
```

Expected: command exits with status 0.

- [ ] **Step 4: Manual regression using the known conversation shape**

In the dev app, start or reuse an IM session and send:

```text
问我三个问题
```

When the model asks questions, reply:

```text
HR/人事
数据处理与分析
结论优先
```

Expected:

- The reply does not create a new run.
- The pending AskUserQuestion resolves.
- The original run continues and uses the three answers.
- No `WriteMemory` calls happen merely because the three answers were mistaken for a preference-setting turn.
- Logs contain `try_handle_reply ... found pending kind=user_question` followed by an interaction resolve event for the same run.

- [ ] **Step 5: Final commit if verification uncovered doc/test adjustments**

If verification required editing `src-tauri/src/connector/im/shared/ask_coordinator.rs`, run:

```bash
git status --short
git add src-tauri/src/connector/im/shared/ask_coordinator.rs
git commit -m "test: verify IM pending interaction routing"
```

If verification required editing `src-tauri/src/runtime/chat/chat_turn_driver.rs`, run:

```bash
git status --short
git add src-tauri/src/runtime/chat/chat_turn_driver.rs
git commit -m "test: verify user interaction suspension routing"
```

If verification did not require edits, do not create a commit for Task 5.

## Self-Review

- Spec coverage:
  - AskUserQuestion ordinary text resume is covered by Task 1.
  - Explicit `/answer` behavior is preserved by Task 1 focused tests.
  - Run-aware pending lifecycle is covered by Task 2.
  - Permission reply fallthrough is blocked by Task 3.
  - Permission path scope override persistence is covered by Task 4.
  - Suspended waiting user vs active busy is checked in Task 5 and full verification.
  - Known conversation `f25ed287-2d64-4708-9798-ab57f1038abc` is covered by Task 6 manual regression.

- Placeholder scan:
  - No placeholder steps remain.
  - Task 4 is a verification gate because current code already contains the required suspension contract and tests.

- Type consistency:
  - `PendingAsk.run_id` uses existing `RunId`.
  - AskUserQuestion resolution uses existing `InteractionResolution::Submit { value }`.
  - Permission resolution uses existing `PendingPermissionResolution`.
  - `HandleOutcome` variants stay unchanged.
