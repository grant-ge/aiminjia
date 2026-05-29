# IM Channel Hard Interaction Downgrade Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让钉钉 IM 会话中的 `PermissionAskRequired` 和 `UserInteractionRequired(AskUserQuestion)` 不再无限等待桌面 UI，而是通过钉钉文本化询问、LLM 判定用户回复、resolve 原 pending ask，并在用户跑题时把新消息重新路由为新 turn。

**Architecture:** 新增 `IMAskCoordinator` 作为 IM 专属状态机，订阅 runtime event bus 但第一步通过 `ChannelSessionRegistry` 过滤非 IM session；`DingtalkReplyManager` 只实现 `AskOutputSink` 负责发钉钉 AI Card，不保存 ask 状态；`ChannelManager` 收到新 IM 消息后先调用 `try_handle_reply()`，未消费或 reroute 时才走原 `send_chat_request()` 路径。

**Tech Stack:** Rust 2021, tokio, async-trait, serde_json, RuntimeEventBus, PendingPermissionControlPlane, PendingInteractionControlPlane, LlmGateway, DingTalk AI Card。

---

## 背景与架构对标

- 规格来源：`docs/superpowers/specs/2026-05-08-im-channel-hard-interaction-downgrade-design.md`。
- 指定基线 `/Users/a20250311/github/claude-code-best` 未在本机找到；替代只读参考为 `/Users/oayzz/Documents/claude-code-main`。
- 对标原则：权限真相源留在 runtime control plane；channel 只是 resolver 和 output sink；非 UI / headless / IM 上下文必须有 no-hang 策略；pending request 必须有 request id、pending map、cancel/timeout 清理。
- lotus 自定义扩展：在 IM 文本里支持 `AskUserQuestion` 多选 / Other 自由文本、用 LLM 判断 answered/abandoned/ambiguous、钉钉 AI Card 策略，均不是对标仓库的直接能力，需要在实现说明中标注为 IM 通道扩展。

## File Map

**Create**
- `src-tauri/src/connector/channel/ask_coordinator.rs` — IM ask 状态机、判断器、trait、deadline、resolve。
- `src-tauri/tests/review_im_ask_coordinator.rs` — 架构约束测试。
- `src-tauri/tests/im_ask_coordinator_integration_test.rs` — 端到端 pending ask 分流集成测试。

**Modify**
- `src-tauri/src/connector/channel/router.rs` — 增加 session_id 反向索引，实现 `ChannelSessionRegistry`。
- `src-tauri/src/connector/channel/reply_manager.rs` — 增加卡片 lifecycle，按需开卡，实现 `AskOutputSink`。
- `src-tauri/src/connector/channel/manager.rs` — 注入 coordinator，订阅 event bus，接收侧先分流 pending ask reply。
- `src-tauri/src/connector/channel/mod.rs` — 导出 `ask_coordinator`。
- `src-tauri/src/lib.rs` — 构造 `IMAskCoordinator` 所需依赖并传入 `ChannelManager`。
- `src-tauri/src/runtime/events.rs` — 给 ask event 携带 `primary_model`，避免 coordinator 耦合 settings store。
- `src-tauri/src/runtime/chat/chat_turn_driver.rs` — emit ask event 时填 `primary_model`。

**Do Not Modify**
- 前端 React 组件 — IM 降级不改变 app 内 dialog。
- `transport/tauri_event_adapter.rs` — app 内 `permission:ask` / `interaction:required` 继续照常发送。
- 工具实现本身 — 不在 tool 内判断 IM channel。

## Task 1: RuntimeEvent 携带 primary_model

**Files:**
- Modify: `src-tauri/src/runtime/events.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Test: `src-tauri/tests/review_backend_event_payload_test.rs`

- [ ] **Step 1: 修改 RuntimeEventKind 字段**

在 `src-tauri/src/runtime/events.rs` 中把两个 variant 改成：

```rust
PermissionAskRequired {
    tool_call_id: ToolCallId,
    tool_name: String,
    message: String,
    suggestions: Vec<String>,
    mode: PermissionMode,
    remember_options: Vec<PermissionDestination>,
    default_destination: Option<PermissionDestination>,
    primary_model: String,
},
UserInteractionRequired {
    interaction_id: crate::runtime::interaction::InteractionId,
    tool_call_id: ToolCallId,
    tool_name: String,
    kind: crate::runtime::interaction::InteractionKind,
    payload: serde_json::Value,
    primary_model: String,
},
```

- [ ] **Step 2: 更新 `RuntimeEvent::new` 匹配**

确认 `RuntimeEvent::new` 中 pattern 仍忽略新增字段：

```rust
RuntimeEventKind::PermissionAskRequired { tool_call_id, .. } => {
    Some(tool_call_id.clone())
}
RuntimeEventKind::UserInteractionRequired { tool_call_id, .. } => {
    Some(tool_call_id.clone())
}
```

- [ ] **Step 3: chat_turn_driver emit permission event 填模型**

在 `resolve_permission_asks()` 的 `RuntimeEventKind::PermissionAskRequired` 构造中加入：

```rust
primary_model: turn.config().llm_settings.primary_model.clone(),
```

如果当前函数拿不到 `turn.config()`，使用同文件中已存在的 config 变量或在进入 resolve 函数前从 `turn` 提供只读 accessor；新增 accessor 时写成：

```rust
impl TurnState {
    pub fn primary_model(&self) -> &str {
        &self.config.llm_settings.primary_model
    }
}
```

然后填：

```rust
primary_model: turn.primary_model().to_string(),
```

- [ ] **Step 4: chat_turn_driver emit interaction event 填模型**

在 `resolve_interaction_requests()` 的 `RuntimeEventKind::UserInteractionRequired` 构造中加入：

```rust
primary_model: turn.primary_model().to_string(),
```

- [ ] **Step 5: 更新测试构造器**

对所有手写 `PermissionAskRequired` / `UserInteractionRequired` 的测试，补：

```rust
primary_model: "deepseek-v3".into(),
```

优先用 `rg -n "PermissionAskRequired|UserInteractionRequired" src-tauri/tests src-tauri/src -S` 找全。

- [ ] **Step 6: 编译验证**

Run:

```bash
cargo check --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml
```

Expected: PASS。

- [ ] **Step 7: Commit**

```bash
git add /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/runtime/events.rs /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/runtime/chat/chat_turn_driver.rs /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/tests
git commit -m "feat(runtime): include primary model in ask events"
```

## Task 2: Coordinator trait 与非 IM session 过滤

**Files:**
- Create: `src-tauri/src/connector/channel/ask_coordinator.rs`
- Modify: `src-tauri/src/connector/channel/mod.rs`
- Modify: `src-tauri/src/connector/channel/router.rs`

- [ ] **Step 1: 创建 ask_coordinator 骨架**

新建 `src-tauri/src/connector/channel/ask_coordinator.rs`：

```rust
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::runtime::event_bus::RuntimeEventSubscriber;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
use crate::runtime::ids::{RunId, SessionId, ToolCallId};
use crate::runtime::interaction::{InteractionId, InteractionResolution, PendingInteractionControlPlane};
use crate::runtime::store::{PendingPermissionControlPlane, PendingPermissionResolution};

const ASK_DEADLINE: Duration = Duration::from_secs(10 * 60);

#[async_trait]
pub trait AskOutputSink: Send + Sync {
    async fn deliver_ask_card(&self, session_id: &SessionId, markdown: String) -> Result<()>;
    async fn force_finish_current_card(&self, session_id: &SessionId, reason_for_log: &str) -> Result<()>;
}

pub trait ChannelSessionRegistry: Send + Sync {
    fn is_channel_session(&self, session_id: &SessionId) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandleOutcome {
    NotPending,
    Consumed,
    Reroute { content: String },
}

#[derive(Debug, Clone)]
pub enum PendingAskKind {
    Permission {
        tool_call_id: ToolCallId,
        tool_name: String,
        message: String,
        suggestions: Vec<String>,
    },
    UserQuestion {
        interaction_id: InteractionId,
        tool_call_id: ToolCallId,
        questions: serde_json::Value,
    },
}

#[derive(Debug)]
struct PendingAsk {
    session_id: SessionId,
    run_id: RunId,
    kind: PendingAskKind,
    deadline_at: Instant,
    cancel: CancellationToken,
    primary_model: String,
}

pub struct IMAskCoordinator {
    pending: Mutex<HashMap<String, PendingAsk>>,
    registry: Arc<dyn ChannelSessionRegistry>,
    sink: Arc<dyn AskOutputSink>,
    permission_cp: Arc<dyn PendingPermissionControlPlane>,
    interaction_cp: Arc<dyn PendingInteractionControlPlane>,
}

impl IMAskCoordinator {
    pub fn new(
        registry: Arc<dyn ChannelSessionRegistry>,
        sink: Arc<dyn AskOutputSink>,
        permission_cp: Arc<dyn PendingPermissionControlPlane>,
        interaction_cp: Arc<dyn PendingInteractionControlPlane>,
    ) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            registry,
            sink,
            permission_cp,
            interaction_cp,
        }
    }

    pub async fn try_handle_reply(
        &self,
        session_id: &SessionId,
        content: String,
    ) -> Result<HandleOutcome> {
        if !self.pending.lock().await.contains_key(session_id.as_str()) {
            return Ok(HandleOutcome::NotPending);
        }
        Ok(HandleOutcome::Consumed)
    }

    async fn register_pending(&self, event: &RuntimeEvent, kind: PendingAskKind, primary_model: String) -> Result<()> {
        if !self.registry.is_channel_session(&event.session_id) {
            log::trace!("[im-ask] ignore non-channel session {}", event.session_id.as_str());
            return Ok(());
        }
        let cancel = CancellationToken::new();
        let pending = PendingAsk {
            session_id: event.session_id.clone(),
            run_id: event.run_id.clone(),
            kind: kind.clone(),
            deadline_at: Instant::now() + ASK_DEADLINE,
            cancel: cancel.clone(),
            primary_model,
        };
        self.pending
            .lock()
            .await
            .insert(event.session_id.as_str().to_string(), pending);
        self.sink
            .deliver_ask_card(&event.session_id, format_pending_ask_markdown(&kind))
            .await?;
        Ok(())
    }
}

#[async_trait]
impl RuntimeEventSubscriber for IMAskCoordinator {
    async fn on_event(&self, event: &RuntimeEvent) -> Result<()> {
        match &event.kind {
            RuntimeEventKind::PermissionAskRequired {
                tool_call_id,
                tool_name,
                message,
                suggestions,
                primary_model,
                ..
            } => {
                self.register_pending(
                    event,
                    PendingAskKind::Permission {
                        tool_call_id: tool_call_id.clone(),
                        tool_name: tool_name.clone(),
                        message: message.clone(),
                        suggestions: suggestions.clone(),
                    },
                    primary_model.clone(),
                )
                .await
            }
            RuntimeEventKind::UserInteractionRequired {
                interaction_id,
                tool_call_id,
                payload,
                primary_model,
                ..
            } => {
                self.register_pending(
                    event,
                    PendingAskKind::UserQuestion {
                        interaction_id: interaction_id.clone(),
                        tool_call_id: tool_call_id.clone(),
                        questions: payload.clone(),
                    },
                    primary_model.clone(),
                )
                .await
            }
            _ => Ok(()),
        }
    }
}

pub fn format_pending_ask_markdown(kind: &PendingAskKind) -> String {
    match kind {
        PendingAskKind::Permission { tool_name, message, suggestions, .. } => {
            let mut text = format!(
                "🔒 我需要你的确认才能继续\n\n打算执行：**{}**\n\n> {}\n\n是否允许？请直接回复，例如“可以”或“不要”。",
                tool_name,
                message
            );
            if !suggestions.is_empty() {
                text.push_str("\n\n建议参数：\n");
                for suggestion in suggestions {
                    text.push_str("- ");
                    text.push_str(suggestion);
                    text.push('\n');
                }
            }
            text
        }
        PendingAskKind::UserQuestion { questions, .. } => {
            let questions_array = questions
                .get("questions")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let mut text = "❓ 我有几个问题想问你\n".to_string();
            for (idx, question) in questions_array.iter().enumerate() {
                let title = question
                    .get("question")
                    .and_then(|v| v.as_str())
                    .unwrap_or("请选择一个选项");
                text.push_str(&format!("\n**{}. {}**\n", idx + 1, title));
                if question.get("multiSelect").and_then(|v| v.as_bool()).unwrap_or(false) {
                    text.push_str("（可多选）\n");
                }
                if let Some(options) = question.get("options").and_then(|v| v.as_array()) {
                    for option in options {
                        if let Some(label) = option.get("label").and_then(|v| v.as_str()) {
                            text.push_str("- ");
                            text.push_str(label);
                            text.push('\n');
                        }
                    }
                }
            }
            text.push_str("\n请直接回复你的选择，自然语言即可。");
            text
        }
    }
}
```

- [ ] **Step 2: 导出模块**

在 `src-tauri/src/connector/channel/mod.rs` 加入：

```rust
pub mod ask_coordinator;
```

- [ ] **Step 3: router 增加反向索引**

在 `src-tauri/src/connector/channel/router.rs` imports 改成：

```rust
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
```

给 `ChannelSessionRouter` 增加字段：

```rust
session_ids: HashSet<String>,
```

新增 helper：

```rust
fn build_session_ids(state: &SessionsState) -> HashSet<String> {
    state.sessions.values().cloned().collect()
}
```

所有构造 `Self { sessions_path, state }` 的地方改为：

```rust
let session_ids = Self::build_session_ids(&state);
Ok(Self {
    sessions_path: sessions_path.to_path_buf(),
    state,
    session_ids,
})
```

新建空 router 的地方改成：

```rust
let state = SessionsState {
    schema_version: CURRENT_SCHEMA_VERSION,
    sessions: HashMap::new(),
};
let router = Self {
    sessions_path: sessions_path.to_path_buf(),
    session_ids: Self::build_session_ids(&state),
    state,
};
```

在 `get_or_create_session` 插入新 session 后补：

```rust
self.session_ids.insert(session_id.clone());
```

- [ ] **Step 4: 实现 ChannelSessionRegistry**

在 `router.rs` 文件底部 tests 前加入：

```rust
impl super::ask_coordinator::ChannelSessionRegistry for ChannelSessionRouter {
    fn is_channel_session(&self, session_id: &crate::runtime::ids::SessionId) -> bool {
        self.session_ids.contains(session_id.as_str())
    }
}
```

- [ ] **Step 5: 写非 IM 过滤单测**

在 `ask_coordinator.rs` tests mod 追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    struct Registry(bool);
    impl ChannelSessionRegistry for Registry {
        fn is_channel_session(&self, _session_id: &SessionId) -> bool { self.0 }
    }

    struct RecordingSink { calls: StdMutex<Vec<String>> }
    #[async_trait]
    impl AskOutputSink for RecordingSink {
        async fn deliver_ask_card(&self, _session_id: &SessionId, markdown: String) -> Result<()> {
            self.calls.lock().unwrap().push(markdown);
            Ok(())
        }
        async fn force_finish_current_card(&self, _session_id: &SessionId, _reason_for_log: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn permission_markdown_contains_operation() {
        let text = format_pending_ask_markdown(&PendingAskKind::Permission {
            tool_call_id: ToolCallId::new("tool-1"),
            tool_name: "bash".into(),
            message: "命令：`ls /tmp`".into(),
            suggestions: vec!["cwd=/tmp".into()],
        });
        assert!(text.contains("bash"));
        assert!(text.contains("ls /tmp"));
        assert!(text.contains("cwd=/tmp"));
    }

    #[test]
    fn question_markdown_renders_options() {
        let text = format_pending_ask_markdown(&PendingAskKind::UserQuestion {
            interaction_id: InteractionId::new("ask-1"),
            tool_call_id: ToolCallId::new("tool-1"),
            questions: serde_json::json!({
                "questions": [{
                    "question": "用哪个数据源？",
                    "multiSelect": true,
                    "options": [{"label": "A"}, {"label": "B"}]
                }]
            }),
        });
        assert!(text.contains("用哪个数据源"));
        assert!(text.contains("可多选"));
        assert!(text.contains("- A"));
    }
}
```

- [ ] **Step 6: 编译和单测**

Run:

```bash
cargo check --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --lib connector::channel::ask_coordinator::tests -- --nocapture
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --lib connector::channel::router::tests -- --nocapture
```

Expected: PASS。

- [ ] **Step 7: Commit**

```bash
git add /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/ask_coordinator.rs /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/mod.rs /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/router.rs
git commit -m "feat(channel): add IM ask coordinator skeleton"
```

## Task 3: PendingAsk deadline 与 resolve-once 清理

**Files:**
- Modify: `src-tauri/src/connector/channel/ask_coordinator.rs`
- Modify: `src-tauri/src/runtime/store/pending_permission_request_store.rs`
- Modify: `src-tauri/src/runtime/interaction/control_plane.rs`

- [ ] **Step 1: 给 control plane 增加 pending 查询**

在 `PendingPermissionControlPlane` trait 加：

```rust
fn is_pending(&self, tool_call_id: &ToolCallId) -> bool;
```

在 `impl PendingPermissionControlPlane for PendingPermissionRequestStore` 加：

```rust
fn is_pending(&self, tool_call_id: &ToolCallId) -> bool {
    self.get(tool_call_id).is_some()
}
```

在 `PendingInteractionControlPlane` trait 加：

```rust
fn is_pending(&self, interaction_id: &InteractionId) -> bool;
```

在 `impl PendingInteractionControlPlane for InMemoryInteractionControlPlane` 加：

```rust
fn is_pending(&self, interaction_id: &InteractionId) -> bool {
    self.inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .contains_key(interaction_id.as_str())
}
```

- [ ] **Step 2: coordinator 增加 deadline spawn**

在 `register_pending` 插入 pending 后加入：

```rust
let session_id = event.session_id.clone();
let coordinator = self.clone_handle();
tokio::spawn(async move {
    tokio::select! {
        _ = tokio::time::sleep(ASK_DEADLINE) => {
            coordinator.resolve_deadline(session_id).await;
        }
        _ = cancel.cancelled() => {}
    }
});
```

在 `IMAskCoordinator` impl 中新增：

```rust
fn clone_handle(&self) -> IMAskCoordinatorHandle {
    IMAskCoordinatorHandle {
        pending: Arc::new(self.pending.clone()),
        sink: Arc::clone(&self.sink),
        permission_cp: Arc::clone(&self.permission_cp),
        interaction_cp: Arc::clone(&self.interaction_cp),
    }
}
```

如果 `tokio::sync::Mutex` 不能直接 clone，则把 `pending` 字段类型改为：

```rust
pending: Arc<Mutex<HashMap<String, PendingAsk>>>,
```

并在 `new()` 初始化：

```rust
pending: Arc::new(Mutex::new(HashMap::new())),
```

新增 handle：

```rust
#[derive(Clone)]
struct IMAskCoordinatorHandle {
    pending: Arc<Mutex<HashMap<String, PendingAsk>>>,
    sink: Arc<dyn AskOutputSink>,
    permission_cp: Arc<dyn PendingPermissionControlPlane>,
    interaction_cp: Arc<dyn PendingInteractionControlPlane>,
}

impl IMAskCoordinatorHandle {
    async fn resolve_deadline(&self, session_id: SessionId) {
        let pending = self.pending.lock().await.remove(session_id.as_str());
        if let Some(pending) = pending {
            resolve_pending_as_timeout(
                self.permission_cp.as_ref(),
                self.interaction_cp.as_ref(),
                &pending.kind,
            );
            let _ = self
                .sink
                .force_finish_current_card(&session_id, "deadline")
                .await;
        }
    }
}
```

新增 resolve helper：

```rust
fn resolve_pending_as_timeout(
    permission_cp: &dyn PendingPermissionControlPlane,
    interaction_cp: &dyn PendingInteractionControlPlane,
    kind: &PendingAskKind,
) {
    match kind {
        PendingAskKind::Permission { tool_call_id, .. } => {
            if permission_cp.is_pending(tool_call_id) {
                let _ = permission_cp.resolve_pending_request(
                    tool_call_id,
                    PendingPermissionResolution::Deny {
                        message: "IM permission request timed out without user response.".to_string(),
                        remember: false,
                        destination: None,
                    },
                );
            }
        }
        PendingAskKind::UserQuestion { interaction_id, .. } => {
            if interaction_cp.is_pending(interaction_id) {
                let _ = interaction_cp.resolve(
                    interaction_id,
                    InteractionResolution::Cancel {
                        message: "IM user question timed out without user response.".to_string(),
                    },
                );
            }
        }
    }
}
```

- [ ] **Step 3: reply 先到时取消 deadline**

在 `try_handle_reply` 取 pending 时改为 remove，并 cancel：

```rust
let pending = self.pending.lock().await.remove(session_id.as_str());
let Some(pending) = pending else {
    return Ok(HandleOutcome::NotPending);
};
pending.cancel.cancel();
```

本 task 暂时返回：

```rust
Ok(HandleOutcome::Consumed)
```

- [ ] **Step 4: 写 deadline 单测**

在 `ask_coordinator.rs` tests 中增加 fake control plane 时，直接使用真实 `PendingPermissionRequestStore` 和 `InMemoryInteractionControlPlane`；新增 tokio 测试：

```rust
#[tokio::test(start_paused = true)]
async fn deadline_denies_permission_and_clears_slot() {
    let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
    let interaction = Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
    let sink = Arc::new(RecordingSink { calls: StdMutex::new(Vec::new()) });
    let coordinator = IMAskCoordinator::new(
        Arc::new(Registry(true)),
        sink,
        permission.clone(),
        interaction,
    );

    let event = RuntimeEvent::new(
        SessionId::new("sess-im"),
        RunId::new("run-1"),
        RuntimeEventKind::PermissionAskRequired {
            tool_call_id: ToolCallId::new("tool-1"),
            tool_name: "bash".into(),
            message: "run ls".into(),
            suggestions: vec![],
            mode: crate::runtime::tools::permission::PermissionMode::Default,
            remember_options: vec![],
            default_destination: None,
            primary_model: "deepseek-v3".into(),
        },
    );

    coordinator.on_event(&event).await.unwrap();
    tokio::time::advance(Duration::from_secs(10 * 60 + 1)).await;

    assert!(coordinator.pending.lock().await.is_empty());
}
```

- [ ] **Step 5: 跑 deadline 测试**

Run:

```bash
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --lib connector::channel::ask_coordinator::tests -- --nocapture
```

Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/ask_coordinator.rs /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/runtime/store/pending_permission_request_store.rs /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/runtime/interaction/control_plane.rs
git commit -m "feat(channel): add IM ask timeout handling"
```

## Task 4: LLM 判断器与三档 resolve

**Files:**
- Modify: `src-tauri/src/connector/channel/ask_coordinator.rs`

- [ ] **Step 1: 定义判断器 trait 与结果类型**

在 `ask_coordinator.rs` traits 后加入：

```rust
#[async_trait]
pub trait AskReplyJudge: Send + Sync {
    async fn judge_permission(
        &self,
        model: &str,
        tool_name: &str,
        ask_message: &str,
        suggestions: &[String],
        user_reply: &str,
    ) -> JudgeResult;

    async fn judge_user_question(
        &self,
        model: &str,
        questions: &serde_json::Value,
        user_reply: &str,
    ) -> JudgeResult;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JudgeResult {
    PermissionAnswered { allow: bool, reason: String },
    UserQuestionAnswered { value: serde_json::Value, reason: String },
    Abandoned { reason: String },
    Ambiguous { reason: String },
}
```

给 `IMAskCoordinator` 增加字段：

```rust
judge: Arc<dyn AskReplyJudge>,
```

`new()` 参数也加入 `judge: Arc<dyn AskReplyJudge>` 并赋值。

- [ ] **Step 2: 实现 try_handle_reply 三档分流**

把 `try_handle_reply` 的 consumed stub 替换为：

```rust
if content.trim().is_empty() {
    self.resolve_ambiguous(&pending, &content, "empty reply".to_string()).await?;
    return Ok(HandleOutcome::Consumed);
}
let judgement = match &pending.kind {
    PendingAskKind::Permission { tool_name, message, suggestions, .. } => {
        self.judge
            .judge_permission(&pending.primary_model, tool_name, message, suggestions, &content)
            .await
    }
    PendingAskKind::UserQuestion { questions, .. } => {
        self.judge
            .judge_user_question(&pending.primary_model, questions, &content)
            .await
    }
};
match judgement {
    JudgeResult::PermissionAnswered { allow, reason } => {
        self.resolve_permission_answer(&pending, allow, reason)?;
        Ok(HandleOutcome::Consumed)
    }
    JudgeResult::UserQuestionAnswered { value, .. } => {
        self.resolve_user_question_answer(&pending, value)?;
        Ok(HandleOutcome::Consumed)
    }
    JudgeResult::Abandoned { reason } => {
        self.resolve_abandoned(&pending, reason)?;
        self.sink.force_finish_current_card(session_id, "abandoned").await?;
        Ok(HandleOutcome::Reroute { content })
    }
    JudgeResult::Ambiguous { reason } => {
        self.resolve_ambiguous(&pending, &content, reason).await?;
        Ok(HandleOutcome::Consumed)
    }
}
```

新增 resolve methods：

```rust
fn resolve_permission_answer(&self, pending: &PendingAsk, allow: bool, reason: String) -> Result<()> {
    if let PendingAskKind::Permission { tool_call_id, .. } = &pending.kind {
        if allow {
            self.permission_cp.resolve_pending_request(
                tool_call_id,
                PendingPermissionResolution::Allow {
                    updated_input: None,
                    remember: false,
                    destination: None,
                },
            )?;
        } else {
            self.permission_cp.resolve_pending_request(
                tool_call_id,
                PendingPermissionResolution::Deny {
                    message: reason,
                    remember: false,
                    destination: None,
                },
            )?;
        }
    }
    Ok(())
}

fn resolve_user_question_answer(&self, pending: &PendingAsk, value: serde_json::Value) -> Result<()> {
    if let PendingAskKind::UserQuestion { interaction_id, .. } = &pending.kind {
        self.interaction_cp.resolve(interaction_id, InteractionResolution::Submit { value })?;
    }
    Ok(())
}

fn resolve_abandoned(&self, pending: &PendingAsk, reason: String) -> Result<()> {
    match &pending.kind {
        PendingAskKind::Permission { tool_call_id, .. } => {
            if self.permission_cp.is_pending(tool_call_id) {
                self.permission_cp.resolve_pending_request(
                    tool_call_id,
                    PendingPermissionResolution::Deny {
                        message: format!("User changed topic in IM channel: {}", reason),
                        remember: false,
                        destination: None,
                    },
                )?;
            }
        }
        PendingAskKind::UserQuestion { interaction_id, .. } => {
            if self.interaction_cp.is_pending(interaction_id) {
                self.interaction_cp.resolve(
                    interaction_id,
                    InteractionResolution::Cancel {
                        message: format!("User changed topic in IM channel: {}", reason),
                    },
                )?;
            }
        }
    }
    Ok(())
}

async fn resolve_ambiguous(&self, pending: &PendingAsk, user_reply: &str, reason: String) -> Result<()> {
    match &pending.kind {
        PendingAskKind::Permission { tool_call_id, .. } => {
            if self.permission_cp.is_pending(tool_call_id) {
                self.permission_cp.resolve_pending_request(
                    tool_call_id,
                    PendingPermissionResolution::Deny {
                        message: format!("IM reply did not clearly grant permission. User said: {}. Judge reason: {}", user_reply, reason),
                        remember: false,
                        destination: None,
                    },
                )?;
            }
        }
        PendingAskKind::UserQuestion { interaction_id, .. } => {
            if self.interaction_cp.is_pending(interaction_id) {
                self.interaction_cp.resolve(
                    interaction_id,
                    InteractionResolution::Submit {
                        value: serde_json::json!({
                            "kind": "user_did_not_answer",
                            "user_said": user_reply,
                            "guidance": reason,
                        }),
                    },
                )?;
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 3: 写 scripted judge 测试**

在 tests mod 加：

```rust
struct ScriptedJudge { result: StdMutex<JudgeResult> }
#[async_trait]
impl AskReplyJudge for ScriptedJudge {
    async fn judge_permission(&self, _model: &str, _tool_name: &str, _ask_message: &str, _suggestions: &[String], _user_reply: &str) -> JudgeResult {
        self.result.lock().unwrap().clone()
    }
    async fn judge_user_question(&self, _model: &str, _questions: &serde_json::Value, _user_reply: &str) -> JudgeResult {
        self.result.lock().unwrap().clone()
    }
}
```

新增 answered/abandoned/ambiguous 测试：

```rust
#[tokio::test]
async fn answered_permission_is_consumed() {
    let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
    let interaction = Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
    let coordinator = IMAskCoordinator::new(
        Arc::new(Registry(true)),
        Arc::new(RecordingSink { calls: StdMutex::new(Vec::new()) }),
        permission,
        interaction,
        Arc::new(ScriptedJudge { result: StdMutex::new(JudgeResult::PermissionAnswered { allow: true, reason: "user allowed".into() }) }),
    );
    coordinator.pending.lock().await.insert("sess-im".into(), PendingAsk {
        session_id: SessionId::new("sess-im"),
        run_id: RunId::new("run-1"),
        kind: PendingAskKind::Permission {
            tool_call_id: ToolCallId::new("tool-1"),
            tool_name: "bash".into(),
            message: "run ls".into(),
            suggestions: vec![],
        },
        deadline_at: Instant::now() + ASK_DEADLINE,
        cancel: CancellationToken::new(),
        primary_model: "deepseek-v3".into(),
    });
    let outcome = coordinator.try_handle_reply(&SessionId::new("sess-im"), "可以".into()).await.unwrap();
    assert_eq!(outcome, HandleOutcome::Consumed);
}

#[tokio::test]
async fn abandoned_reply_is_rerouted() {
    let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
    let interaction = Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
    let coordinator = IMAskCoordinator::new(
        Arc::new(Registry(true)),
        Arc::new(RecordingSink { calls: StdMutex::new(Vec::new()) }),
        permission,
        interaction,
        Arc::new(ScriptedJudge { result: StdMutex::new(JudgeResult::Abandoned { reason: "new topic".into() }) }),
    );
    coordinator.pending.lock().await.insert("sess-im".into(), PendingAsk {
        session_id: SessionId::new("sess-im"),
        run_id: RunId::new("run-1"),
        kind: PendingAskKind::UserQuestion {
            interaction_id: InteractionId::new("ask-1"),
            tool_call_id: ToolCallId::new("tool-1"),
            questions: serde_json::json!({"questions": []}),
        },
        deadline_at: Instant::now() + ASK_DEADLINE,
        cancel: CancellationToken::new(),
        primary_model: "deepseek-v3".into(),
    });
    let outcome = coordinator.try_handle_reply(&SessionId::new("sess-im"), "帮我查天气".into()).await.unwrap();
    assert_eq!(outcome, HandleOutcome::Reroute { content: "帮我查天气".into() });
}
```

- [ ] **Step 4: 跑 coordinator 测试**

Run:

```bash
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --lib connector::channel::ask_coordinator::tests -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/ask_coordinator.rs
git commit -m "feat(channel): resolve IM ask replies"
```

## Task 5: LlmGateway 判断器实现

**Files:**
- Modify: `src-tauri/src/connector/channel/ask_coordinator.rs`

- [ ] **Step 1: 新增 gateway judge 类型**

在 `ask_coordinator.rs` 中加入：

```rust
pub struct GatewayAskReplyJudge {
    gateway: Arc<crate::llm::gateway::LlmGateway>,
    settings: crate::models::settings::AppSettings,
}

impl GatewayAskReplyJudge {
    pub fn new(gateway: Arc<crate::llm::gateway::LlmGateway>, settings: crate::models::settings::AppSettings) -> Self {
        Self { gateway, settings }
    }
}
```

- [ ] **Step 2: 实现 prompt 和 JSON 解析**

加入 structs：

```rust
#[derive(serde::Deserialize)]
struct PermissionJudgeJson {
    verdict: String,
    decision: Option<String>,
    reason: Option<String>,
}

#[derive(serde::Deserialize)]
struct UserQuestionJudgeJson {
    verdict: String,
    selections: Option<serde_json::Value>,
    reason: Option<String>,
}

fn strip_json_fence(input: &str) -> &str {
    input
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
}
```

实现 trait：

```rust
#[async_trait]
impl AskReplyJudge for GatewayAskReplyJudge {
    async fn judge_permission(
        &self,
        model: &str,
        tool_name: &str,
        ask_message: &str,
        suggestions: &[String],
        user_reply: &str,
    ) -> JudgeResult {
        let mut settings = self.settings.clone();
        settings.primary_model = model.to_string();
        let prompt = format!(
            "你是一个分诊器。AI 助手刚向用户请求高风险操作授权。只输出 JSON。\n\nAI 想做的操作：\n{}: {}\n建议参数：{}\n\n用户回复：\n\"\"\"{}\"\"\"\n\n输出 JSON：{{\"verdict\":\"answered|abandoned|ambiguous\",\"decision\":\"allow|deny\",\"reason\":\"一句话\"}}",
            tool_name,
            ask_message,
            suggestions.join("\n"),
            user_reply
        );
        let response = tokio::time::timeout(
            Duration::from_secs(30),
            self.gateway.send_message(
                &settings,
                vec![crate::llm::streaming::ChatMessage::text("user", prompt)],
                crate::llm::masking::MaskingLevel::None,
                None,
                None,
                Some(Vec::new()),
            ),
        )
        .await;
        let Ok(Ok(response)) = response else {
            return JudgeResult::Ambiguous { reason: "judge call failed".into() };
        };
        let parsed: Result<PermissionJudgeJson, _> = serde_json::from_str(strip_json_fence(&response.content));
        match parsed {
            Ok(v) if v.verdict == "answered" => JudgeResult::PermissionAnswered {
                allow: v.decision.as_deref() == Some("allow"),
                reason: v.reason.unwrap_or_else(|| "permission answered by IM user".into()),
            },
            Ok(v) if v.verdict == "abandoned" => JudgeResult::Abandoned {
                reason: v.reason.unwrap_or_else(|| "user changed topic".into()),
            },
            Ok(v) => JudgeResult::Ambiguous {
                reason: v.reason.unwrap_or_else(|| "unclear permission reply".into()),
            },
            Err(_) => JudgeResult::Ambiguous { reason: "judge JSON parse failed".into() },
        }
    }

    async fn judge_user_question(
        &self,
        model: &str,
        questions: &serde_json::Value,
        user_reply: &str,
    ) -> JudgeResult {
        let mut settings = self.settings.clone();
        settings.primary_model = model.to_string();
        let prompt = format!(
            "你是一个分诊器。AI 助手刚通过 AskUserQuestion 工具向用户问了一组问题。只输出 JSON。\n\nAI 提的问题：\n{}\n\n用户回复：\n\"\"\"{}\"\"\"\n\n输出 JSON：{{\"verdict\":\"answered|abandoned|ambiguous\",\"selections\":[{{\"questionIndex\":0,\"labels\":[\"...\"],\"freeText\":null}}],\"reason\":\"一句话\"}}",
            questions,
            user_reply
        );
        let response = tokio::time::timeout(
            Duration::from_secs(30),
            self.gateway.send_message(
                &settings,
                vec![crate::llm::streaming::ChatMessage::text("user", prompt)],
                crate::llm::masking::MaskingLevel::None,
                None,
                None,
                Some(Vec::new()),
            ),
        )
        .await;
        let Ok(Ok(response)) = response else {
            return JudgeResult::Ambiguous { reason: "judge call failed".into() };
        };
        let parsed: Result<UserQuestionJudgeJson, _> = serde_json::from_str(strip_json_fence(&response.content));
        match parsed {
            Ok(v) if v.verdict == "answered" => match v.selections {
                Some(selections) => JudgeResult::UserQuestionAnswered {
                    value: serde_json::json!({ "selections": selections }),
                    reason: v.reason.unwrap_or_else(|| "question answered by IM user".into()),
                },
                None => JudgeResult::Ambiguous { reason: "answered without selections".into() },
            },
            Ok(v) if v.verdict == "abandoned" => JudgeResult::Abandoned {
                reason: v.reason.unwrap_or_else(|| "user changed topic".into()),
            },
            Ok(v) => JudgeResult::Ambiguous {
                reason: v.reason.unwrap_or_else(|| "unclear question reply".into()),
            },
            Err(_) => JudgeResult::Ambiguous { reason: "judge JSON parse failed".into() },
        }
    }
}
```

- [ ] **Step 3: 编译验证**

Run:

```bash
cargo check --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml
```

Expected: PASS。

- [ ] **Step 4: Commit**

```bash
git add /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/ask_coordinator.rs
git commit -m "feat(channel): add LLM ask reply judge"
```

## Task 6: ReplyManager 实现 AskOutputSink 与按需开卡

**Files:**
- Modify: `src-tauri/src/connector/channel/reply_manager.rs`

- [ ] **Step 1: 增加 CardLifecycle**

把 `ReplyContext` 改为：

```rust
#[derive(Debug)]
enum CardLifecycle {
    Streaming(CardInstance),
    Finished,
}

#[derive(Debug)]
struct ReplyContext {
    card_lifecycle: CardLifecycle,
    accumulated_text: String,
    app_key: String,
    app_secret: String,
    robot_code: String,
    target: CardTarget,
    run_id: String,
}
```

`register()` 插入 context 时改为：

```rust
ReplyContext {
    card_lifecycle: CardLifecycle::Streaming(card),
    accumulated_text: String::new(),
    app_key,
    app_secret,
    robot_code,
    target,
    run_id,
}
```

- [ ] **Step 2: StreamDelta 按需开卡**

在 `StreamDelta` 分支中，拿到 ctx 后先确保 card：

```rust
if matches!(ctx.card_lifecycle, CardLifecycle::Finished) {
    if let Some(card) = dingtalk_card::create_and_deliver_card(
        &self.token_cache,
        &ctx.app_key,
        &ctx.app_secret,
        &ctx.robot_code,
        &ctx.target,
    )
    .await
    {
        ctx.card_lifecycle = CardLifecycle::Streaming(card);
    }
}
```

然后 stream 时改成：

```rust
if let CardLifecycle::Streaming(card) = &mut ctx.card_lifecycle {
    if let Err(e) = dingtalk_card::stream_card(
        &cache,
        &app_key,
        &app_secret,
        card,
        &text,
        false,
    )
    .await
    {
        log::warn!("[reply-manager] stream_card failed: {:#}", e);
    }
}
```

- [ ] **Step 3: StreamDone / StreamError 适配 lifecycle**

`StreamDone` 中：

```rust
if let CardLifecycle::Streaming(card) = &mut ctx.card_lifecycle {
    if let Err(e) = dingtalk_card::finish_card(
        &cache,
        &app_key,
        &app_secret,
        card,
        &text,
    )
    .await
    {
        log::warn!("[reply-manager] finish_card failed: {:#}", e);
    }
}
contexts.remove(&session_id);
```

`StreamError` 中：

```rust
if let CardLifecycle::Streaming(card) = &ctx.card_lifecycle {
    if let Err(e) = dingtalk_card::fail_card(&cache, &ctx.app_key, &ctx.app_secret, card).await {
        log::warn!("[reply-manager] fail_card error: {:#}", e);
    }
}
```

- [ ] **Step 4: 实现 AskOutputSink**

在 `reply_manager.rs` 中加入：

```rust
#[async_trait]
impl super::ask_coordinator::AskOutputSink for DingtalkReplyManager {
    async fn deliver_ask_card(
        &self,
        session_id: &crate::runtime::ids::SessionId,
        markdown: String,
    ) -> Result<()> {
        let mut contexts = self.contexts.lock().await;
        let Some(ctx) = contexts.get_mut(session_id.as_str()) else {
            return Ok(());
        };
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
        if let Some(mut ask_card) = dingtalk_card::create_and_deliver_card(
            &self.token_cache,
            &ctx.app_key,
            &ctx.app_secret,
            &ctx.robot_code,
            &ctx.target,
        )
        .await
        {
            let _ = dingtalk_card::finish_card(
                &self.token_cache,
                &ctx.app_key,
                &ctx.app_secret,
                &mut ask_card,
                &markdown,
            )
            .await;
        }
        ctx.card_lifecycle = CardLifecycle::Finished;
        Ok(())
    }

    async fn force_finish_current_card(
        &self,
        session_id: &crate::runtime::ids::SessionId,
        reason_for_log: &str,
    ) -> Result<()> {
        let mut contexts = self.contexts.lock().await;
        let Some(ctx) = contexts.get_mut(session_id.as_str()) else {
            return Ok(());
        };
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
        log::info!("[reply-manager] force finished card session={} reason={}", session_id.as_str(), reason_for_log);
        Ok(())
    }
}
```

- [ ] **Step 5: 更新 reply_manager 单测构造器**

把测试中的 `ReplyContext { card: CardInstance { ... }, ... }` 全部改为：

```rust
ReplyContext {
    card_lifecycle: CardLifecycle::Streaming(CardInstance {
        card_instance_id: "card1".into(),
        inputing_started: false,
    }),
    accumulated_text: String::new(),
    app_key: "key".into(),
    app_secret: "secret".into(),
    robot_code: "robot".into(),
    target: CardTarget::Private { user_id: "user".into() },
    run_id: "run1".into(),
}
```

- [ ] **Step 6: 跑 reply_manager 测试**

Run:

```bash
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --lib connector::channel::reply_manager::tests -- --nocapture
```

Expected: PASS。

- [ ] **Step 7: Commit**

```bash
git add /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/reply_manager.rs
git commit -m "feat(dingtalk): render IM ask cards"
```

## Task 7: ChannelManager 接线与接收侧分流

**Files:**
- Modify: `src-tauri/src/connector/channel/manager.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: ChannelManager 增加 coordinator 字段**

在 `ChannelManager` struct 加：

```rust
ask_coordinator: Option<Arc<super::ask_coordinator::IMAskCoordinator>>,
ask_subscribed: Arc<AtomicBool>,
```

`new()` 参数加入：

```rust
ask_coordinator: Option<Arc<super::ask_coordinator::IMAskCoordinator>>,
```

初始化加入：

```rust
ask_coordinator,
ask_subscribed: Arc::new(AtomicBool::new(false)),
```

- [ ] **Step 2: 订阅 coordinator**

在 `connect_dingtalk` 中 reply_manager 订阅后加入：

```rust
if let Some(coordinator) = self.ask_coordinator.as_ref() {
    if claim_first_subscription(&self.ask_subscribed) {
        let sub = Arc::clone(coordinator) as Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber>;
        self.chat_adapter.subscribe_event_listener(sub);
    }
}
```

- [ ] **Step 3: worker 捕获 coordinator**

在 worker 捕获区加入：

```rust
let ask_coordinator_ref = self.ask_coordinator.as_ref().map(Arc::clone);
```

- [ ] **Step 4: 收到消息后先分流 pending ask**

在构造 `request` 前加入：

```rust
if let Some(coordinator) = ask_coordinator_ref.as_ref() {
    match coordinator
        .try_handle_reply(&crate::runtime::ids::SessionId::new(session_id.clone()), text.clone())
        .await
    {
        Ok(super::ask_coordinator::HandleOutcome::NotPending) => {}
        Ok(super::ask_coordinator::HandleOutcome::Consumed) => continue,
        Ok(super::ask_coordinator::HandleOutcome::Reroute { content }) => {
            log::info!("[channel] IM ask abandoned, rerouting message session={}", session_id);
            let text = content;
            let content = match &conv_type {
                ConversationType::Group => format!("[{}]: {}", sender_nick, text),
                ConversationType::Private => text,
            };
            let request = ChatTurnRequest::new(session_id.clone(), content, vec![]);
            let run_id = request.run_id.as_str().to_string();
            let card_target = match &conv_type {
                ConversationType::Group => CardTarget::Group { open_conversation_id: conv_key.clone() },
                ConversationType::Private => CardTarget::Private { user_id: msg.sender_id.clone() },
            };
            reply_manager_ref.register(
                session_id.clone(),
                run_id,
                reply_app_key.clone(),
                reply_app_secret.clone(),
                reply_robot_code.clone(),
                card_target,
            ).await;
            if let Err(e) = adapter.send_chat_request(request).await {
                log::error!("[channel] rerouted send_chat_request failed: {}", e);
            }
            continue;
        }
        Err(error) => {
            log::warn!("[channel] IM ask coordinator failed, falling back to normal turn: {:#}", error);
        }
    }
}
```

- [ ] **Step 5: lib.rs 构造时先传 None**

把 `ChannelManager::new(...)` 调用最后补：

```rust
None,
```

这样本 task 先保证编译，下一 task 注入真实 coordinator。

- [ ] **Step 6: 编译验证**

Run:

```bash
cargo check --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml
```

Expected: PASS。

- [ ] **Step 7: Commit**

```bash
git add /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/manager.rs /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/lib.rs
git commit -m "feat(channel): route IM replies through ask coordinator"
```

## Task 8: lib.rs 注入真实 coordinator

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 1: 暴露 runtime control plane getter**

在 `TauriChatCommandAdapter` impl 中增加：

```rust
pub fn permission_control_plane(&self) -> Arc<dyn crate::runtime::store::PendingPermissionControlPlane> {
    self.runtime.permission_control_plane()
}

pub fn interaction_control_plane(&self) -> Arc<dyn crate::runtime::interaction::PendingInteractionControlPlane> {
    self.runtime.interaction_control_plane()
}
```

如果 `SessionRuntime` 尚无 getter，在 `src-tauri/src/runtime/session_runtime.rs` 增加：

```rust
pub fn permission_control_plane(&self) -> Arc<dyn crate::runtime::store::PendingPermissionControlPlane> {
    self.pending_permission_control_plane.clone()
}

pub fn interaction_control_plane(&self) -> Arc<dyn crate::runtime::interaction::PendingInteractionControlPlane> {
    self.pending_interaction_control_plane.clone()
}
```

字段名以当前 `SessionRuntime` 实际字段为准；若字段是 concrete store，则返回 `Arc::clone(&field) as Arc<dyn ...>`。

- [ ] **Step 2: lib.rs 构造 coordinator**

在创建 `channel_manager` 前加入：

```rust
let reply_manager = Arc::new(connector::channel::DingtalkReplyManager::new());
let registry = Arc::new(connector::channel::router::ChannelSessionRouter::migrate_or_load(
    &channels_dir.join("dingtalk").join("sessions.json"),
    conversation_store.as_ref(),
).unwrap_or_else(|_| connector::channel::router::ChannelSessionRouter::empty_for_runtime(channels_dir.join("dingtalk").join("sessions.json"))))
    as Arc<dyn connector::channel::ask_coordinator::ChannelSessionRegistry>;
let judge = Arc::new(connector::channel::ask_coordinator::GatewayAskReplyJudge::new(
    gateway.clone(),
    models::settings::AppSettings::default(),
));
let ask_coordinator = Arc::new(connector::channel::ask_coordinator::IMAskCoordinator::new(
    registry,
    reply_manager.clone() as Arc<dyn connector::channel::ask_coordinator::AskOutputSink>,
    chat_adapter.permission_control_plane(),
    chat_adapter.interaction_control_plane(),
    judge,
));
```

如果 `ChannelManager` 自己创建 reply manager，改造 `ChannelManager::new` 接受 `reply_manager: Arc<DingtalkReplyManager>`，确保 coordinator 和 manager 使用同一个 sink 实例。

- [ ] **Step 3: ChannelManager::new 传真实 coordinator**

把 lib.rs 中 `None` 替换为：

```rust
Some(ask_coordinator.clone()),
```

同时如上一步改造了 reply manager 参数，传：

```rust
reply_manager.clone(),
```

- [ ] **Step 4: 修正 registry 生命周期**

如果 `ChannelSessionRouter` 在 worker 内独立加载导致 registry 不是同一实例，改成 manager 持有：

```rust
session_registry: Arc<RwLock<ChannelSessionRouter>>,
```

并让 worker 使用同一 `Arc<RwLock<ChannelSessionRouter>>`；`ChannelSessionRegistry` 为 `RwLock<ChannelSessionRouter>` 实现：

```rust
impl super::ask_coordinator::ChannelSessionRegistry for tokio::sync::RwLock<ChannelSessionRouter> {
    fn is_channel_session(&self, session_id: &crate::runtime::ids::SessionId) -> bool {
        self.blocking_read().is_channel_session_id(session_id)
    }
}
```

更安全的实现是在 `ChannelManager` 内维护 `Arc<std::sync::RwLock<HashSet<String>>>` 作为 registry，并在 worker create session 后同步插入 session_id，避免 async lock trait 中 blocking read。

- [ ] **Step 5: 编译验证**

Run:

```bash
cargo check --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml
```

Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/lib.rs /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/transport/tauri_commands/chat.rs /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/runtime/session_runtime.rs /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/manager.rs
git commit -m "feat(channel): wire IM ask coordinator"
```

## Task 9: 架构约束与集成测试

**Files:**
- Create: `src-tauri/tests/review_im_ask_coordinator.rs`
- Create: `src-tauri/tests/im_ask_coordinator_integration_test.rs`

- [ ] **Step 1: 架构约束测试**

新建 `src-tauri/tests/review_im_ask_coordinator.rs`：

```rust
#[test]
fn ask_coordinator_does_not_depend_on_tauri() {
    let source = std::fs::read_to_string("src/connector/channel/ask_coordinator.rs")
        .expect("read ask_coordinator.rs");
    assert!(!source.contains("use tauri"));
    assert!(!source.contains("tauri::"));
}

#[test]
fn ask_coordinator_uses_sink_trait_for_dingtalk_output() {
    let source = std::fs::read_to_string("src/connector/channel/ask_coordinator.rs")
        .expect("read ask_coordinator.rs");
    assert!(source.contains("trait AskOutputSink"));
    assert!(!source.contains("dingtalk_card::"));
    assert!(!source.contains("create_and_deliver_card"));
}
```

- [ ] **Step 2: 集成测试 skeleton**

新建 `src-tauri/tests/im_ask_coordinator_integration_test.rs`：

```rust
use app_lib::connector::channel::ask_coordinator::{format_pending_ask_markdown, PendingAskKind};
use app_lib::runtime::ids::ToolCallId;

#[test]
fn permission_ask_markdown_is_plain_im_text() {
    let markdown = format_pending_ask_markdown(&PendingAskKind::Permission {
        tool_call_id: ToolCallId::new("tool-1"),
        tool_name: "bash".into(),
        message: "命令：`ls /tmp`".into(),
        suggestions: vec!["只读命令".into()],
    });
    assert!(markdown.contains("我需要你的确认"));
    assert!(markdown.contains("bash"));
    assert!(markdown.contains("ls /tmp"));
}
```

- [ ] **Step 3: 跑架构测试**

Run:

```bash
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --test review_im_ask_coordinator
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --test im_ask_coordinator_integration_test
```

Expected: PASS。

- [ ] **Step 4: 跑相关回归**

Run:

```bash
cargo check --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --test p0_a2_permission_ask_routing_test
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --test p0_permission_control_plane_test
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --test ask_user_question_test
```

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/tests/review_im_ask_coordinator.rs /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/tests/im_ask_coordinator_integration_test.rs
git commit -m "test(channel): guard IM ask coordinator architecture"
```

## Final Verification

Run:

```bash
cargo check --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --test review_im_ask_coordinator
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --test im_ask_coordinator_integration_test
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --test p0_a2_permission_ask_routing_test
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --test p0_permission_control_plane_test
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --test ask_user_question_test
```

Expected: all commands PASS。

## Manual Verification Matrix

- 钉钉群触发 `AskUserQuestion`：群里出现 ask 卡；回复选项后旧 run 继续。
- 钉钉群触发 `AskUserQuestion`：回复“算了，帮我查天气”；旧 ask cancel，新消息作为新 turn。
- 钉钉群触发 `AskUserQuestion`：回复模糊内容；LLM 收到 `user_did_not_answer` 结构化结果并自行重问或换路径。
- 钉钉群触发 `write_file` permission：回复“可以”；原 tool call 继续。
- 钉钉群触发 `write_file` permission：回复“不行”；原 tool call deny，LLM 自行换方案。
- 钉钉群触发 ask 后 10 分钟无回复：后端静默 deny/cancel，不发超时通知。
- app 内对话触发 `write_file` permission：协调器过滤非 IM session，前端 dialog 在 11 分钟后仍可点击并正常 resolve。
- app 内对话触发 `AskUserQuestion`：前端 dialog 不被 IM coordinator 抢占。

## Self-Review

- Spec coverage: 覆盖 AskUserQuestion、permission ask、answered/abandoned/ambiguous、10 分钟 deadline、非 IM session 过滤、AI Card 策略、manager reroute。
- Placeholder scan: 本计划不包含待填占位步骤；所有关键代码步骤给出可直接落地的代码块。
- Type consistency: `HandleOutcome`、`PendingAskKind`、`AskOutputSink`、`ChannelSessionRegistry`、`AskReplyJudge` 名称在各任务中保持一致。
