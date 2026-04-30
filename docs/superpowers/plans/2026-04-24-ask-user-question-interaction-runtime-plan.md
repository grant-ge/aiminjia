# AskUserQuestion Tool & Interaction Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 lotus-app 后端 agent 实现独立的 Interaction Runtime，并在其上实现 `AskUserQuestion` 工具，让模型可以在执行过程中向用户提出结构化多选问题并等待回答后继续推理。

**Architecture:** 新建 `runtime/interaction/` 模块（独立的 pending interaction control plane，与权限 pipeline 完全分离），新增 `RuntimeToolCallOutcome::InteractionRequired` variant，`ChatTurnDriver` 路由到 interaction control plane 后发送 `UserInteractionRequired` 事件，前端渲染 `AskUserQuestionDialog` 收集答案，用户提交后通过新 Tauri command 回传，TurnDriver replay tool call 并携带答案继续推理。

**Tech Stack:** Rust / tokio::sync::oneshot / serde_json (后端); React / Zustand / TypeScript (前端)


---

## 背景与参考信息

### 项目现状

- 现有 permission ask 流水线：
  - `src-tauri/src/runtime/store/pending_permission_request_store.rs` — `PendingPermissionControlPlane` trait + `PendingPermissionRequestStore`，用 `oneshot` channel 异步等待用户 resolution
  - `src-tauri/src/runtime/chat/chat_turn_driver.rs` — `resolve_permission_asks()` 等待 `oneshot::Receiver<PendingPermissionResolution>`，完成后 replay tool call
  - `src-tauri/src/transport/tauri_commands/chat.rs` — `approve_permission_request / deny_permission_request / cancel_permission_request` Tauri commands，写入 control plane
  - `src-tauri/src/runtime/events.rs` — `PermissionAskRequired` event
  - `src-tauri/src/transport/tauri_event_adapter.rs` — 映射为 `permission:ask`
  - `src/lib/tauri.ts` — `PermissionAskPayload` 和 `approvePermissionRequest/denyPermissionRequest/cancelPermissionRequest`
  - `src/components/common/PermissionAskDialog.tsx` — 工具权限确认弹窗（**不是**用户问答，二者必须严格分离）
- 工具调用结果类型：`src-tauri/src/runtime/chat/tool_round_types.rs` — `RuntimeToolCallOutcome { Completed, AskRequired }`

### 为何必须独立的 Interaction Runtime，而不复用 permission:ask

| 维度 | permission:ask | AskUserQuestion |
|---|---|---|
| 语义 | "是否允许工具执行（安全决策）" | "模型请求用户提供信息/做选择" |
| payload | message + suggestions（文字） | questions 数组（结构化多选） |
| UI | 简单确认/拒绝 | 多问题 × 多选项 + multiSelect + preview |
| Resolution | Allow/Deny/Cancel | Submit（含 answers）/Cancel |
| 持久化意图 | "记住这次权限" | 无 |
| 前端组件 | PermissionAskDialog | AskUserQuestionDialog（新建） |

混用会导致 `PermissionDecision::Ask` 同时承载安全语义和业务语义，长期维护成本极高。

### 对标 claude-code-best

参考文件：`/Users/a20250311/github/claude-code-best/src/tools/AskUserQuestionTool/AskUserQuestionTool.tsx`

关键设计：
- `requiresUserInteraction() = true`
- `checkPermissions()` 返回 `{ behavior: 'ask', message: 'Answer questions?' }`
- `call()` 接收包含 answers 的 input，直接返回 `{ questions, answers, annotations? }`
- 工具名：`AskUserQuestion`
- 输入 schema：`{ questions: Question[], answers?, annotations?, metadata? }`
- `Question` 字段：`question, header, options: { label, description, preview? }[], multiSelect?`

### 架构约束

1. Interaction Runtime 模块路径：`src-tauri/src/runtime/interaction/`
2. 不得 `use tauri::*`，只通过 `RuntimeEvent` → `TauriEventAdapter` + command handler 通信
3. `RuntimeToolCallOutcome` 新增 `InteractionRequired` variant，与 `AskRequired` 平级
4. `AskUserQuestion` 实现为 `RuntimeTool`，不用 `ToolPlugin`
5. 前端新增 `interactionStore.ts` 和 `AskUserQuestionDialog.tsx`，与 `PermissionAskDialog.tsx` 相互独立

---

## File Map

**新建：**
- `src-tauri/src/runtime/interaction/mod.rs` — 模块入口
- `src-tauri/src/runtime/interaction/types.rs` — InteractionId、InteractionRequest、InteractionResolution
- `src-tauri/src/runtime/interaction/control_plane.rs` — PendingInteractionControlPlane trait + impl
- `src-tauri/src/runtime/tools/builtin/ask_user_question.rs` — AskUserQuestionRuntimeTool
- `src/stores/interactionStore.ts` — 前端 pending interaction 状态
- `src/components/interactions/AskUserQuestionDialog.tsx` — 多选问题 UI

**修改：**
- `src-tauri/src/runtime/chat/tool_round_types.rs` — 新增 `InteractionRequired` variant
- `src-tauri/src/runtime/chat/chat_turn_driver.rs` — 路由 InteractionRequired，wait + replay
- `src-tauri/src/runtime/events.rs` — 新增 `UserInteractionRequired` / `UserInteractionResolved`
- `src-tauri/src/transport/tauri_event_adapter.rs` — 映射为 `interaction:required` / `interaction:resolved`
- `src-tauri/src/transport/tauri_commands/chat.rs` — 新增 `submit_user_interaction / cancel_user_interaction`
- `src-tauri/src/runtime/tools/builtin/mod.rs` — 添加 `pub mod ask_user_question;`
- `src-tauri/src/runtime/tools/catalog.rs` — 新增 AskUserQuestion entry + DAILY_ALLOWED_TOOLS
- `src-tauri/src/plugin/builtin/tools/mod.rs` — 注册 AskUserQuestionRuntimeTool
- `src/lib/tauri.ts` — 新增事件常量、payload 类型、submit/cancel commands、onInteractionRequired listener
- `src-tauri/src/runtime/mod.rs` 或 `lib.rs` — expose interaction module

---

## Task 1: Interaction Runtime 类型层

**Files:**
- Create: `src-tauri/src/runtime/interaction/types.rs`
- Create: `src-tauri/src/runtime/interaction/mod.rs`

- [ ] **Step 1: 创建 types.rs**

新建 `src-tauri/src/runtime/interaction/types.rs`：

```rust
//! Interaction Runtime types.
//!
//! Separates user-facing interactive tools (AskUserQuestion, etc.)
//! from the permission/security pipeline (PermissionDecision::Ask).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use crate::runtime::ids::{RunId, SessionId, ToolCallId};

/// Unique identifier for a pending user interaction.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InteractionId(String);

impl InteractionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for InteractionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for InteractionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// The kind of user interaction requested.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InteractionKind {
    AskUserQuestion,
}

/// A pending user interaction request emitted by a tool during execution.
#[derive(Clone, Debug)]
pub struct InteractionRequest {
    pub interaction_id: InteractionId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_call_id: ToolCallId,
    pub tool_name: String,
    pub kind: InteractionKind,
    /// Structured payload (e.g., questions for AskUserQuestion).
    pub payload: Value,
    /// The original tool call request for replay after resolution.
    pub original_request: RuntimeToolCallRequest,
}

/// Resolution of a pending user interaction.
#[derive(Clone, Debug)]
pub enum InteractionResolution {
    /// User submitted answers/data.
    Submit {
        /// Serialized answer data merged back into the tool call input.
        value: Value,
    },
    /// User cancelled the interaction.
    Cancel {
        message: String,
    },
}
```

- [ ] **Step 2: 创建 mod.rs**

新建 `src-tauri/src/runtime/interaction/mod.rs`：

```rust
//! Interaction Runtime — first-class abstraction for tools that require user input.
//!
//! Distinct from the permission pipeline (PermissionDecision::Ask):
//! - Permission ask: "should this tool be allowed to run?" (security gate)
//! - Interaction: "the tool needs structured input from the user to continue" (UX)

pub mod control_plane;
pub mod types;

pub use control_plane::{InMemoryInteractionControlPlane, PendingInteractionControlPlane};
pub use types::{InteractionId, InteractionKind, InteractionRequest, InteractionResolution};
```

- [ ] **Step 3: 将 interaction 模块暴露到 runtime**

找到 `src-tauri/src/runtime/mod.rs`，添加：

```rust
pub mod interaction;
```

- [ ] **Step 4: 编译验证**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app/src-tauri
cargo check 2>&1 | head -30
```

预期：只有 control_plane 模块未找到的错误（下一步创建）。

---

## Task 2: Interaction Control Plane

**Files:**
- Create: `src-tauri/src/runtime/interaction/control_plane.rs`

- [ ] **Step 1: 创建 control_plane.rs**

新建 `src-tauri/src/runtime/interaction/control_plane.rs`：

```rust
//! PendingInteractionControlPlane — async wait/resolve for user interactions.
//!
//! Pattern mirrors PendingPermissionControlPlane (oneshot channel per request).

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use tokio::sync::oneshot;

use super::types::{InteractionId, InteractionRequest, InteractionResolution};

struct PendingEntry {
    request: InteractionRequest,
    resolution_tx: oneshot::Sender<InteractionResolution>,
}

pub trait PendingInteractionControlPlane: Send + Sync {
    fn insert_pending(
        &self,
        request: InteractionRequest,
    ) -> Result<oneshot::Receiver<InteractionResolution>>;

    fn resolve(
        &self,
        interaction_id: &InteractionId,
        resolution: InteractionResolution,
    ) -> Result<()>;

    fn cancel_for_session(&self, session_id: &str, message: &str) -> usize;

    fn pending_count_for_session(&self, session_id: &str) -> usize;
}

#[derive(Default)]
pub struct InMemoryInteractionControlPlane {
    inner: Mutex<HashMap<String, PendingEntry>>,
}

impl InMemoryInteractionControlPlane {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PendingInteractionControlPlane for InMemoryInteractionControlPlane {
    fn insert_pending(
        &self,
        request: InteractionRequest,
    ) -> Result<oneshot::Receiver<InteractionResolution>> {
        let mut inner = self.inner.lock().unwrap();
        let key = request.interaction_id.as_str().to_string();
        if inner.contains_key(&key) {
            return Err(anyhow!(
                "pending interaction already exists for id: {}",
                key
            ));
        }
        let (tx, rx) = oneshot::channel();
        inner.insert(key, PendingEntry { request, resolution_tx: tx });
        Ok(rx)
    }

    fn resolve(
        &self,
        interaction_id: &InteractionId,
        resolution: InteractionResolution,
    ) -> Result<()> {
        let entry = self
            .inner
            .lock()
            .unwrap()
            .remove(interaction_id.as_str())
            .ok_or_else(|| anyhow!("pending interaction not found: {}", interaction_id))?;
        entry
            .resolution_tx
            .send(resolution)
            .map_err(|_| anyhow!("receiver dropped for interaction: {}", interaction_id))
    }

    fn cancel_for_session(&self, session_id: &str, message: &str) -> usize {
        let mut inner = self.inner.lock().unwrap();
        let to_cancel: Vec<String> = inner
            .iter()
            .filter(|(_, e)| e.request.session_id.as_str() == session_id)
            .map(|(k, _)| k.clone())
            .collect();
        let count = to_cancel.len();
        for key in to_cancel {
            if let Some(entry) = inner.remove(&key) {
                let _ = entry.resolution_tx.send(InteractionResolution::Cancel {
                    message: message.to_string(),
                });
            }
        }
        count
    }

    fn pending_count_for_session(&self, session_id: &str) -> usize {
        self.inner
            .lock()
            .unwrap()
            .values()
            .filter(|e| e.request.session_id.as_str() == session_id)
            .count()
    }
}
```

- [ ] **Step 2: 编译验证**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app/src-tauri
cargo check 2>&1 | head -30
```

预期：无编译错误。

- [ ] **Step 3: 为 control_plane 写测试**

在 `control_plane.rs` 末尾添加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::chat::tool_round_types::RuntimeToolCallRequest;
    use crate::runtime::ids::{RunId, SessionId, ToolCallId};
    use serde_json::json;

    fn make_request(id: &str, session_id: &str) -> InteractionRequest {
        InteractionRequest {
            interaction_id: InteractionId::new(id),
            session_id: SessionId::from(session_id.to_string()),
            run_id: RunId::new("run-test"),
            tool_call_id: ToolCallId::new("tc-test"),
            tool_name: "AskUserQuestion".into(),
            kind: crate::runtime::interaction::types::InteractionKind::AskUserQuestion,
            payload: json!({}),
            original_request: RuntimeToolCallRequest {
                tool_call_id: "tc-test".into(),
                tool_name: "AskUserQuestion".into(),
                args: json!({}),
                purpose: None,
            },
        }
    }

    #[tokio::test]
    async fn insert_and_resolve_submit() {
        let cp = InMemoryInteractionControlPlane::new();
        let rx = cp.insert_pending(make_request("i-1", "sess-1")).unwrap();

        cp.resolve(
            &InteractionId::new("i-1"),
            InteractionResolution::Submit {
                value: json!({ "answers": { "Q1": "A1" } }),
            },
        )
        .unwrap();

        let resolution = rx.await.unwrap();
        assert!(matches!(resolution, InteractionResolution::Submit { .. }));
    }

    #[tokio::test]
    async fn cancel_for_session_drops_pending() {
        let cp = InMemoryInteractionControlPlane::new();
        let _rx = cp.insert_pending(make_request("i-2", "sess-cancel")).unwrap();

        let cancelled = cp.cancel_for_session("sess-cancel", "session ended");
        assert_eq!(cancelled, 1);
        assert_eq!(cp.pending_count_for_session("sess-cancel"), 0);
    }
}
```

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app/src-tauri
cargo test runtime::interaction -- --nocapture 2>&1
```

预期：2 个测试通过。

- [ ] **Step 4: 提交**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app
git add src-tauri/src/runtime/interaction/ src-tauri/src/runtime/mod.rs
git commit -m "feat(runtime): add Interaction Runtime control plane"
```

---

## Task 3: RuntimeToolCallOutcome::InteractionRequired

**Files:**
- Modify: `src-tauri/src/runtime/chat/tool_round_types.rs`

- [ ] **Step 1: 新增 InteractionRequired variant**

打开 `src-tauri/src/runtime/chat/tool_round_types.rs`，找到 `pub enum RuntimeToolCallOutcome`，在 `AskRequired` variant 之后添加：

```rust
    /// The tool requires structured user input to continue.
    ///
    /// Distinct from `AskRequired` (which is a permission/security gate).
    /// The `ChatTurnDriver` routes this to the `PendingInteractionControlPlane`,
    /// waits for user resolution, then replays the tool call with the answers
    /// merged into the original input.
    InteractionRequired {
        tool_call_id: String,
        tool_name: String,
        original_request: RuntimeToolCallRequest,
        interaction_request: crate::runtime::interaction::InteractionRequest,
    },
```

- [ ] **Step 2: 扩展 `tool_call_id()`, `tool_name()`, `is_error()` 等方法**

在 `impl RuntimeToolCallOutcome` 里，为每个 match arm 添加 `InteractionRequired` 分支（编译器会提示所有缺失的地方）：

```rust
// tool_call_id()
Self::InteractionRequired { tool_call_id, .. } => tool_call_id,

// tool_name()
Self::InteractionRequired { tool_name, .. } => tool_name,

// is_error() — InteractionRequired 视为暂停，不是真正错误，返回 false
Self::InteractionRequired { .. } => false,

// max_result_size_chars()
Self::InteractionRequired { .. } => 0,

// content()
Self::InteractionRequired { .. } => "",

// context_modifier_message()
Self::InteractionRequired { .. } => None,

// file_meta()
Self::InteractionRequired { .. } => None,

// skill_runtime_patch()
Self::InteractionRequired { .. } => None,
```

- [ ] **Step 3: 编译验证**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app/src-tauri
cargo check 2>&1 | head -40
```

修复所有 non-exhaustive 匹配错误后继续。

- [ ] **Step 4: 提交**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app
git add src-tauri/src/runtime/chat/tool_round_types.rs
git commit -m "feat(runtime): add InteractionRequired outcome variant"
```

---

## Task 4: AskUserQuestionRuntimeTool 后端实现

**Files:**
- Create: `src-tauri/src/runtime/tools/builtin/ask_user_question.rs`
- Modify: `src-tauri/src/runtime/tools/builtin/mod.rs`
- Modify: `src-tauri/src/runtime/tools/catalog.rs`
- Modify: `src-tauri/src/plugin/builtin/tools/mod.rs`

- [ ] **Step 1: 在 catalog.rs 新增 AskUserQuestion entry**

在 `build_default_catalog()` 的 Support tools 块添加：

```rust
c.insert(CatalogEntry::new(
    ToolDefinition::new(
        "AskUserQuestion",
        "向用户提出结构化多选问题，等待用户回答后继续。\
        \n\n用途：收集用户偏好、澄清歧义、让用户在多个方案中选择。\
        \n\n每次调用支持 1-4 个问题，每个问题 2-4 个选项，\
        用户始终可以选择 Other 输入自定义回答。",
    )
    .with_kind(ToolKind::Support)
    .with_read_only(true),
    json!({
        "type": "object",
        "required": ["questions"],
        "properties": {
            "questions": {
                "type": "array",
                "description": "要向用户提出的问题列表（1-4 个）",
                "minItems": 1,
                "maxItems": 4,
                "items": {
                    "type": "object",
                    "required": ["question", "header", "options"],
                    "properties": {
                        "question": {
                            "type": "string",
                            "description": "完整问题文本，以问号结尾"
                        },
                        "header": {
                            "type": "string",
                            "description": "极短标签（最多 12 字符），如 '认证方式'"
                        },
                        "options": {
                            "type": "array",
                            "description": "2-4 个选项",
                            "minItems": 2,
                            "maxItems": 4,
                            "items": {
                                "type": "object",
                                "required": ["label", "description"],
                                "properties": {
                                    "label": {
                                        "type": "string",
                                        "description": "选项标签，1-5 个词"
                                    },
                                    "description": {
                                        "type": "string",
                                        "description": "选项说明，描述含义或权衡"
                                    },
                                    "preview": {
                                        "type": "string",
                                        "description": "可选预览内容（代码片段、布局示意等）"
                                    }
                                }
                            }
                        },
                        "multiSelect": {
                            "type": "boolean",
                            "description": "是否允许多选，默认 false",
                            "default": false
                        }
                    }
                }
            },
            "answers": {
                "type": "object",
                "description": "用户回答（由系统填入，模型勿填）"
            },
            "metadata": {
                "type": "object",
                "description": "可选元数据，如来源标识"
            }
        }
    }),
));
```

将 `"AskUserQuestion"` 加入 `DAILY_ALLOWED_TOOLS`：

```rust
pub const DAILY_ALLOWED_TOOLS: &[&str] = &[
    "bash",
    "read_workspace_file",
    "write_file",
    "edit_file",
    "list_directory",
    "search_files",
    "get_file_info",
    "grep_content",
    "write_memory",
    "search_memory",
    "TodoWrite",
    "AskUserQuestion",
];
```

- [ ] **Step 2: 创建 ask_user_question.rs**

新建 `src-tauri/src/runtime/tools/builtin/ask_user_question.rs`：

```rust
//! AskUserQuestionRuntimeTool — structured user question tool.
//!
//! 对标 claude-code-best AskUserQuestionTool:
//! - 工具名：AskUserQuestion
//! - 首次 execute：返回 ToolError::InteractionRequired，挂起 turn
//! - Replay（ctx.interaction_resolution 已填）：直接返回带 answers 的 ToolResult
//!
//! 注意：不复用 PermissionDecision::Ask，走独立 Interaction Runtime。

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::runtime::interaction::types::{
    InteractionId, InteractionKind, InteractionRequest,
};
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct AskUserQuestionRuntimeTool;

#[async_trait]
impl RuntimeTool for AskUserQuestionRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("AskUserQuestion")
            .unwrap_or_else(|| ToolDefinition::new("AskUserQuestion", "向用户提问"))
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        // 如果已有 interaction_resolution，说明这是 replay，直接返回结果
        if let Some(resolution) = ctx.interaction_resolution.as_ref() {
            let questions = input.get("questions").cloned().unwrap_or(json!([]));
            let answers = resolution.get("answers").cloned().unwrap_or(json!({}));
            let annotations = resolution.get("annotations").cloned();

            let mut result_data = json!({
                "questions": questions,
                "answers": answers,
            });
            if let Some(ann) = annotations {
                result_data["annotations"] = ann;
            }

            // 生成 LLM 可读的文本 result
            let answers_text = if let Some(obj) = answers.as_object() {
                obj.iter()
                    .map(|(q, a)| format!("\"{}\"=\"{}\"", q, a))
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                String::new()
            };

            return Ok(ToolResult::new(
                "AskUserQuestion",
                format!(
                    "User has answered your questions: {}. You can now continue with the user's answers in mind.",
                    answers_text
                ),
                Some(result_data),
            ));
        }

        // 首次执行：校验并发出 InteractionRequired
        let questions = input
            .get("questions")
            .ok_or_else(|| ToolError::InputValidationError {
                tool_name: "AskUserQuestion".into(),
                message: "missing 'questions' field".into(),
            })?;

        // 基础校验：questions 必须是 1-4 个
        let q_len = questions.as_array().map(|a| a.len()).unwrap_or(0);
        if q_len == 0 || q_len > 4 {
            return Err(ToolError::InputValidationError {
                tool_name: "AskUserQuestion".into(),
                message: format!("questions must have 1-4 items, got {}", q_len),
            });
        }

        let interaction_id = InteractionId::new(Uuid::new_v4().to_string());

        // 从 chat_turn_driver 来的 original_request 路径：这里构造 InteractionRequest
        // 需要注入到 ToolExecutionContext 中（见 Task 5）
        let original_request = ctx
            .current_tool_call_request
            .clone()
            .ok_or_else(|| ToolError::ExecutionFailed(
                "AskUserQuestion: missing current_tool_call_request in ctx".into(),
            ))?;

        let interaction_request = InteractionRequest {
            interaction_id: interaction_id.clone(),
            session_id: ctx.session_id.clone(),
            run_id: ctx.run_id.clone(),
            tool_call_id: ctx.tool_call_id.clone(),
            tool_name: "AskUserQuestion".into(),
            kind: InteractionKind::AskUserQuestion,
            payload: json!({
                "questions": questions,
                "metadata": input.get("metadata"),
            }),
            original_request,
        };

        Err(ToolError::InteractionRequired(Box::new(interaction_request)))
    }
}
```

- [ ] **Step 3: 在 ToolError 新增 InteractionRequired variant**

打开 `src-tauri/src/runtime/tools/executor.rs`，在 `ToolError` 枚举中添加：

```rust
    #[error("user interaction required")]
    InteractionRequired(Box<crate::runtime::interaction::InteractionRequest>),
```

同时在文件顶部或使用处确保 `crate::runtime::interaction` 已可访问。

- [ ] **Step 4: 在 ToolExecutionContext 添加两个新字段**

打开 `src-tauri/src/runtime/tools/context.rs`，在 `ToolExecutionContext` 结构体末尾添加：

```rust
    /// 当 tool 是 InteractionRequired 后被 replay 时，此字段填入用户提交的答案 Value。
    pub interaction_resolution: Option<Value>,
    /// 原始工具调用请求，供工具在需要时构造 InteractionRequest。
    pub current_tool_call_request: Option<crate::runtime::chat::tool_round_types::RuntimeToolCallRequest>,
```

在 `ToolExecutionContext::new()` 中添加初始化：

```rust
interaction_resolution: None,
current_tool_call_request: None,
```

- [ ] **Step 5: 在 builtin/mod.rs 导出 ask_user_question 模块**

```rust
pub mod ask_user_question;
```

- [ ] **Step 6: 编译验证**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app/src-tauri
cargo check 2>&1 | head -50
```

- [ ] **Step 7: 注册到 ToolDispatcher**

打开 `src-tauri/src/plugin/builtin/tools/mod.rs`，在 RuntimeTool 注册块末尾（`registry.validate_catalog_consistency().await;` 之前）添加：

```rust
    use crate::runtime::tools::builtin::ask_user_question::AskUserQuestionRuntimeTool;
    registry
        .register_runtime(Arc::new(AskUserQuestionRuntimeTool))
        .await;
```

- [ ] **Step 8: 编译验证**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app/src-tauri
cargo check 2>&1 | head -30
```

- [ ] **Step 9: 提交**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app
git add src-tauri/src/runtime/tools/builtin/ask_user_question.rs \
        src-tauri/src/runtime/tools/builtin/mod.rs \
        src-tauri/src/runtime/tools/catalog.rs \
        src-tauri/src/runtime/tools/executor.rs \
        src-tauri/src/runtime/tools/context.rs \
        src-tauri/src/plugin/builtin/tools/mod.rs
git commit -m "feat(tools): add AskUserQuestionRuntimeTool skeleton"
```

---

## Task 5: ChatTurnDriver 路由 InteractionRequired

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`

**背景：** 与 `resolve_permission_asks` 类似，需要新增 `resolve_interaction_asks`，在 tool round 结束后检查 `InteractionRequired` outcome，写入 interaction control plane，等待用户提交，然后 replay。

ChatTurnDriver 需要持有 `Option<Arc<dyn PendingInteractionControlPlane>>`，从构造时注入，与 `pending_permission_control_plane` 类似。

- [ ] **Step 1: 在 ChatTurnDriver struct 增加 interaction_control_plane 字段**

找到 `ChatTurnDriver` 的结构体定义和构造函数（在 `src-tauri/src/runtime/chat/chat_turn_driver.rs`），添加：

```rust
pub pending_interaction_control_plane: Option<Arc<dyn PendingInteractionControlPlane>>,
```

在 `new()` 中初始化为 `None`，添加 builder 方法：

```rust
pub fn with_interaction_control_plane(
    mut self,
    cp: Arc<dyn PendingInteractionControlPlane>,
) -> Self {
    self.pending_interaction_control_plane = Some(cp);
    self
}
```

- [ ] **Step 2: 实现 resolve_interaction_asks 方法**

在 `impl ChatTurnDriver` 中添加（参照 `resolve_permission_asks` 的结构）：

```rust
async fn resolve_interaction_asks(
    &self,
    turn: &TurnState,
    cancel: &CancellationToken,
    round_results: Vec<ToolRoundResult>,
) -> Result<Vec<ToolRoundResult>> {
    use crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome;
    use crate::runtime::events::RuntimeEventKind;
    use crate::runtime::interaction::InteractionResolution;

    let mut resolved = Vec::with_capacity(round_results.len());

    for rr in round_results {
        let ToolRoundResult::Ok(RuntimeToolCallOutcome::InteractionRequired {
            tool_call_id,
            tool_name,
            original_request,
            interaction_request,
        }) = rr
        else {
            resolved.push(rr);
            continue;
        };

        let Some(cp) = self.pending_interaction_control_plane.as_ref() else {
            return Err(anyhow::anyhow!(
                "interaction control plane is required to handle InteractionRequired outcome"
            ));
        };

        let interaction_id = interaction_request.interaction_id.clone();
        let resolution_rx = cp.insert_pending(interaction_request)?;

        // Emit event to frontend
        self.event_bus.emit(RuntimeEvent::new(
            turn.session_id().clone(),
            turn.run_id().clone(),
            RuntimeEventKind::UserInteractionRequired {
                interaction_id: interaction_id.clone(),
                tool_call_id: ToolCallId::new(tool_call_id.clone()),
                tool_name: tool_name.clone(),
                kind: crate::runtime::interaction::InteractionKind::AskUserQuestion,
                payload: serde_json::json!({}), // payload already in cp
            },
        ));

        // Wait for user (or cancellation)
        let resolution = tokio::select! {
            r = resolution_rx => {
                r.map_err(|_| anyhow::anyhow!("interaction control plane dropped sender"))?
            }
            _ = cancel.cancelled() => {
                InteractionResolution::Cancel {
                    message: "Turn cancelled while waiting for user interaction.".into(),
                }
            }
        };

        // Build new ToolRoundResult based on resolution
        let new_result = match resolution {
            InteractionResolution::Submit { value } => {
                // Replay the original tool call with answers merged in
                let mut new_args = original_request.args.clone();
                if let Some(obj) = new_args.as_object_mut() {
                    if let Some(answers) = value.get("answers") {
                        obj.insert("answers".into(), answers.clone());
                    }
                    if let Some(annotations) = value.get("annotations") {
                        obj.insert("annotations".into(), annotations.clone());
                    }
                }
                // Re-dispatch with interaction_resolution set
                let mut ctx = self.build_tool_ctx(turn, &original_request, cancel);
                ctx.interaction_resolution = Some(value);
                ctx.current_tool_call_request = Some(original_request.clone());
                let outcome = self
                    .dispatch_single_tool(original_request.clone(), ctx)
                    .await;
                ToolRoundResult::Ok(outcome)
            }
            InteractionResolution::Cancel { message } => {
                // Synthesize a cancelled tool result
                ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
                    tool_call_id,
                    tool_name,
                    content: format!("User cancelled the interaction: {}", message),
                    is_error: false,
                    max_result_size_chars: 1000,
                    context_modifier_message: None,
                    file_meta: None,
                    skill_runtime_patch: None,
                    msg_id: uuid::Uuid::new_v4().to_string(),
                    duration_ms: None,
                })
            }
        };
        resolved.push(new_result);
    }

    Ok(resolved)
}
```

**注意：** Replay 的实际路径是通过 `QueryEngine::run_tool_call_with_bus`（`src-tauri/src/runtime/query_engine.rs:350`），该方法已接受 `RuntimeToolCallRequest` 并返回 `RuntimeToolCallOutcome`。`ToolExecutionContext` 的构建在 `src-tauri/src/runtime/state.rs:60`（`TurnState::make_tool_execution_context`）。replay 时：
1. 修改 `original_request.args`（merge in answers）
2. 通过 `self.query_engine.run_tool_call_with_bus_with_override(turn, bus, original_request, ctx_override)` 或等价方式注入 `interaction_resolution`
3. 参照 `resolve_permission_asks` 中调用 `run_tool_call_with_bus` 的现有代码路径

- [ ] **Step 3: 在 turn loop 中调用 resolve_interaction_asks**

在 `resolve_permission_asks(...)` 的调用之后，链式调用：

```rust
let round_results = self
    .resolve_permission_asks(turn, &cancel, round_results)
    .await?;
let round_results = self
    .resolve_interaction_asks(turn, &cancel, round_results)
    .await?;
```

- [ ] **Step 4: 在 ctx 构建时填入 current_tool_call_request**

找到构建 `ToolExecutionContext` 的地方，在 tool dispatch 前填入：

```rust
ctx.current_tool_call_request = Some(original_request.clone());
```

- [ ] **Step 5: 编译验证**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app/src-tauri
cargo check 2>&1 | head -60
```

修复所有错误后继续。

- [ ] **Step 6: 提交**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app
git add src-tauri/src/runtime/chat/chat_turn_driver.rs
git commit -m "feat(runtime): route InteractionRequired through interaction control plane"
```

---

## Task 6: 事件与 Tauri Commands

**Files:**
- Modify: `src-tauri/src/runtime/events.rs`
- Modify: `src-tauri/src/transport/tauri_event_adapter.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: 找 `ChatTurnDriver` 的构建入口，注入 `InMemoryInteractionControlPlane`

- [ ] **Step 1: 新增 runtime events**

打开 `src-tauri/src/runtime/events.rs`，在 `RuntimeEventKind` 枚举里添加：

```rust
    UserInteractionRequired {
        interaction_id: crate::runtime::interaction::InteractionId,
        tool_call_id: ToolCallId,
        tool_name: String,
        kind: crate::runtime::interaction::InteractionKind,
        /// Full payload to render the interaction UI (questions for AskUserQuestion).
        payload: serde_json::Value,
    },
    UserInteractionResolved {
        interaction_id: crate::runtime::interaction::InteractionId,
    },
```

- [ ] **Step 2: tauri_event_adapter 映射新事件**

打开 `src-tauri/src/transport/tauri_event_adapter.rs`，在 `match event.kind` 中添加：

```rust
RuntimeEventKind::UserInteractionRequired {
    interaction_id,
    tool_call_id,
    tool_name,
    kind,
    payload,
} => Some(LegacyEvent {
    name: "interaction:required".to_string(),
    payload: serde_json::json!({
        "conversationId": conversation_id,
        "runId": event.run_id.as_str(),
        "interactionId": interaction_id.as_str(),
        "toolCallId": tool_call_id.as_str(),
        "toolName": tool_name,
        "kind": kind,
        "payload": payload,
    }),
}),
RuntimeEventKind::UserInteractionResolved {
    interaction_id,
} => Some(LegacyEvent {
    name: "interaction:resolved".to_string(),
    payload: serde_json::json!({
        "conversationId": conversation_id,
        "runId": event.run_id.as_str(),
        "interactionId": interaction_id.as_str(),
    }),
}),
```

- [ ] **Step 3: 新增 submit/cancel Tauri commands**

打开 `src-tauri/src/transport/tauri_commands/chat.rs`，仿照 `approve_permission_request` 模式添加（在同一 impl 块中）：

```rust
pub async fn submit_user_interaction(
    &self,
    interaction_id: String,
    value: serde_json::Value,
) -> Result<(), String> {
    use crate::runtime::interaction::{InteractionId, InteractionResolution};
    self.pending_interaction_control_plane
        .as_ref()
        .ok_or_else(|| "interaction control plane not available".to_string())?
        .resolve(
            &InteractionId::new(interaction_id),
            InteractionResolution::Submit { value },
        )
        .map_err(|e| e.to_string())
}

pub async fn cancel_user_interaction(
    &self,
    interaction_id: String,
    message: Option<String>,
) -> Result<(), String> {
    use crate::runtime::interaction::{InteractionId, InteractionResolution};
    self.pending_interaction_control_plane
        .as_ref()
        .ok_or_else(|| "interaction control plane not available".to_string())?
        .resolve(
            &InteractionId::new(interaction_id),
            InteractionResolution::Cancel {
                message: message.unwrap_or_else(|| "User cancelled.".into()),
            },
        )
        .map_err(|e| e.to_string())
}
```

`TauriChatCommandAdapter` 需要持有 `Option<Arc<dyn PendingInteractionControlPlane>>`，与 `pending_permission_control_plane` 类似，在构造时注入。

- [ ] **Step 4: 将 InMemoryInteractionControlPlane 注入到 app 启动链**

找到 `src-tauri/src/lib.rs` 中构建 `TauriChatCommandAdapter` 和 `ChatTurnDriver` 的地方，创建共享的：

```rust
let interaction_cp = Arc::new(InMemoryInteractionControlPlane::new());
// 注入到 TauriChatCommandAdapter
// 注入到 ChatTurnDriver
```

- [ ] **Step 5: 在 Tauri app.invoke_handler 中注册新 commands**

找到注册 `approve_permission_request / deny_permission_request` 的地方，按相同模式注册：

```rust
tauri::generate_handler![
    // ... 已有 handlers ...
    submit_user_interaction,
    cancel_user_interaction,
]
```

- [ ] **Step 6: 编译验证**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app/src-tauri
cargo check 2>&1 | head -50
```

- [ ] **Step 7: 运行 review 回归测试**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app/src-tauri
cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

- [ ] **Step 8: 提交**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app
git add src-tauri/src/runtime/events.rs \
        src-tauri/src/transport/tauri_event_adapter.rs \
        src-tauri/src/transport/tauri_commands/chat.rs \
        src-tauri/src/lib.rs
git commit -m "feat(runtime): add interaction events and submit/cancel commands"
```

---

## Task 7: 前端 — interaction store 与事件类型

**Files:**
- Modify: `src/lib/tauri.ts`
- Create: `src/stores/interactionStore.ts`

- [ ] **Step 1: 在 tauri.ts 添加事件常量和类型**

在 `TAURI_EVENTS` 对象添加：

```typescript
  INTERACTION_REQUIRED: 'interaction:required',
  INTERACTION_RESOLVED: 'interaction:resolved',
```

在 payload 类型区块添加：

```typescript
export interface QuestionOption {
  label: string
  description: string
  preview?: string
}

export interface Question {
  question: string
  header: string
  options: QuestionOption[]
  multiSelect?: boolean
}

export interface InteractionRequiredPayload {
  conversationId: string
  runId: string
  interactionId: string
  toolCallId: string
  toolName: string
  kind: 'askUserQuestion'
  payload: {
    questions: Question[]
    metadata?: unknown
  }
}

export interface InteractionResolvedPayload {
  conversationId: string
  runId: string
  interactionId: string
}
```

添加 invoke 函数：

```typescript
export function submitUserInteraction(
  interactionId: string,
  value: { answers: Record<string, string>; annotations?: Record<string, unknown> },
): Promise<void> {
  return invoke<void>('submit_user_interaction', { interactionId, value })
}

export function cancelUserInteraction(
  interactionId: string,
  message?: string,
): Promise<void> {
  return invoke<void>('cancel_user_interaction', { interactionId, message })
}
```

添加 listener：

```typescript
export function onInteractionRequired(
  handler: (payload: InteractionRequiredPayload) => void,
): Promise<() => void> {
  return listen<InteractionRequiredPayload>(
    TAURI_EVENTS.INTERACTION_REQUIRED,
    (event) => handler(event.payload),
  )
}

export function onInteractionResolved(
  handler: (payload: InteractionResolvedPayload) => void,
): Promise<() => void> {
  return listen<InteractionResolvedPayload>(
    TAURI_EVENTS.INTERACTION_RESOLVED,
    (event) => handler(event.payload),
  )
}
```

- [ ] **Step 2: 创建 interactionStore.ts**

新建 `src/stores/interactionStore.ts`：

```typescript
import { create } from 'zustand'
import type { InteractionRequiredPayload } from '@/lib/tauri'

interface InteractionState {
  pendingInteractions: InteractionRequiredPayload[]
  addInteraction: (payload: InteractionRequiredPayload) => void
  removeInteraction: (interactionId: string) => void
  clearForConversation: (conversationId: string) => void
}

export const useInteractionStore = create<InteractionState>((set) => ({
  pendingInteractions: [],

  addInteraction(payload) {
    set((state) => ({
      pendingInteractions: [...state.pendingInteractions, payload],
    }))
  },

  removeInteraction(interactionId) {
    set((state) => ({
      pendingInteractions: state.pendingInteractions.filter(
        (i) => i.interactionId !== interactionId,
      ),
    }))
  },

  clearForConversation(conversationId) {
    set((state) => ({
      pendingInteractions: state.pendingInteractions.filter(
        (i) => i.conversationId !== conversationId,
      ),
    }))
  },
}))
```

- [ ] **Step 3: 在事件订阅入口订阅 interaction 事件**

在事件订阅入口文件中，参照 `onPermissionAsk` 的注册模式（在 `src/App.tsx` 中通过 `useEffect` 注册）添加：

```typescript
import { onInteractionRequired, onInteractionResolved } from '@/lib/tauri'
import { useInteractionStore } from '@/stores/interactionStore'

const unsubInteractionRequired = await onInteractionRequired((payload) => {
  useInteractionStore.getState().addInteraction(payload)
})
const unsubInteractionResolved = await onInteractionResolved((payload) => {
  useInteractionStore.getState().removeInteraction(payload.interactionId)
})
// cleanup 时调用 unsubInteractionRequired() 和 unsubInteractionResolved()
```

- [ ] **Step 4: TypeScript 类型检查**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app
pnpm exec tsc --noEmit 2>&1 | head -30
```

- [ ] **Step 5: 提交**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app
git add src/lib/tauri.ts src/stores/interactionStore.ts
# 加上修改的订阅入口文件
git commit -m "feat(frontend): add interactionStore and interaction event types"
```

---

## Task 8: 前端 — AskUserQuestionDialog 组件

**Files:**
- Create: `src/components/interactions/AskUserQuestionDialog.tsx`
- Modify: 找到对话界面挂载 PermissionAskDialog 的地方，同级挂载新 dialog

- [ ] **Step 1: 创建 AskUserQuestionDialog.tsx**

新建 `src/components/interactions/AskUserQuestionDialog.tsx`：

```tsx
import React, { useState } from 'react'
import type { Question } from '@/lib/tauri'
import { submitUserInteraction, cancelUserInteraction } from '@/lib/tauri'

interface Props {
  interactionId: string
  questions: Question[]
  onClose: () => void
}

export function AskUserQuestionDialog({ interactionId, questions, onClose }: Props) {
  const [answers, setAnswers] = useState<Record<string, string[]>>({})
  const [customInputs, setCustomInputs] = useState<Record<string, string>>({})

  function toggleOption(questionText: string, label: string, multiSelect: boolean) {
    setAnswers((prev) => {
      const current = prev[questionText] ?? []
      if (multiSelect) {
        return {
          ...prev,
          [questionText]: current.includes(label)
            ? current.filter((l) => l !== label)
            : [...current, label],
        }
      }
      return { ...prev, [questionText]: [label] }
    })
  }

  async function handleSubmit() {
    // Convert answers array to comma-separated string (multi-select)
    const flatAnswers: Record<string, string> = {}
    for (const q of questions) {
      const selected = answers[q.question] ?? []
      if (selected.includes('__other__')) {
        const custom = customInputs[q.question] ?? ''
        flatAnswers[q.question] = custom || 'Other'
      } else {
        flatAnswers[q.question] = selected.join(', ')
      }
    }
    await submitUserInteraction(interactionId, { answers: flatAnswers })
    onClose()
  }

  async function handleCancel() {
    await cancelUserInteraction(interactionId, 'User dismissed the question dialog.')
    onClose()
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="bg-background border border-border rounded-xl shadow-xl w-full max-w-lg p-6 space-y-5">
        <div className="text-sm font-semibold text-foreground">AI 向你提问</div>

        {questions.map((q) => (
          <div key={q.question} className="space-y-2">
            <div className="text-sm font-medium">{q.question}</div>
            <div className="flex flex-wrap gap-2">
              {q.options.map((opt) => {
                const selected = (answers[q.question] ?? []).includes(opt.label)
                return (
                  <button
                    key={opt.label}
                    onClick={() => toggleOption(q.question, opt.label, !!q.multiSelect)}
                    className={`rounded-lg border px-3 py-2 text-xs text-left transition-colors ${
                      selected
                        ? 'border-primary bg-primary/10 text-primary'
                        : 'border-border bg-muted/30 text-foreground hover:bg-muted'
                    }`}
                  >
                    <div className="font-medium">{opt.label}</div>
                    <div className="text-muted-foreground mt-0.5">{opt.description}</div>
                  </button>
                )
              })}
              {/* Other option */}
              <button
                onClick={() => toggleOption(q.question, '__other__', !!q.multiSelect)}
                className={`rounded-lg border px-3 py-2 text-xs text-left transition-colors ${
                  (answers[q.question] ?? []).includes('__other__')
                    ? 'border-primary bg-primary/10 text-primary'
                    : 'border-border bg-muted/30 text-foreground hover:bg-muted'
                }`}
              >
                <div className="font-medium">其他</div>
                <div className="text-muted-foreground mt-0.5">输入自定义回答</div>
              </button>
            </div>
            {(answers[q.question] ?? []).includes('__other__') && (
              <input
                type="text"
                placeholder="请输入..."
                value={customInputs[q.question] ?? ''}
                onChange={(e) =>
                  setCustomInputs((prev) => ({ ...prev, [q.question]: e.target.value }))
                }
                className="w-full rounded-lg border border-border bg-background px-3 py-2 text-sm"
              />
            )}
          </div>
        ))}

        <div className="flex justify-end gap-2 pt-2">
          <button
            onClick={handleCancel}
            className="rounded-lg border border-border px-4 py-2 text-sm text-muted-foreground hover:bg-muted"
          >
            取消
          </button>
          <button
            onClick={handleSubmit}
            disabled={questions.some((q) => !(answers[q.question]?.length))}
            className="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground disabled:opacity-50 hover:bg-primary/90"
          >
            提交
          </button>
        </div>
      </div>
    </div>
  )
}
```

- [ ] **Step 2: 在对话界面挂载 AskUserQuestionDialog**

找到 `PermissionAskDialog` 被挂载的位置（`src/App.tsx`，找到 `<PermissionAskDialog` 标签），在同级位置添加：

```tsx
import { AskUserQuestionDialog } from '@/components/interactions/AskUserQuestionDialog'
import { useInteractionStore } from '@/stores/interactionStore'

// 在组件内：
const pendingInteractions = useInteractionStore((s) =>
  s.pendingInteractions.filter((i) => i.conversationId === conversationId)
)

// 在 JSX 中：
{pendingInteractions.map((interaction) => (
  <AskUserQuestionDialog
    key={interaction.interactionId}
    interactionId={interaction.interactionId}
    questions={interaction.payload.questions}
    onClose={() =>
      useInteractionStore.getState().removeInteraction(interaction.interactionId)
    }
  />
))}
```

- [ ] **Step 3: TypeScript 类型检查**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app
pnpm exec tsc --noEmit 2>&1 | head -30
```

- [ ] **Step 4: 前端测试**

```bash
pnpm test 2>&1 | tail -20
```

- [ ] **Step 5: 提交**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app
git add src/components/interactions/AskUserQuestionDialog.tsx
# 加上修改了的对话界面文件
git commit -m "feat(frontend): add AskUserQuestionDialog"
```

---

## Task 9: 后端集成测试

**Files:**
- Create: `src-tauri/tests/ask_user_question_test.rs`

- [ ] **Step 1: 编写测试**

新建 `src-tauri/tests/ask_user_question_test.rs`：

```rust
//! Integration tests for AskUserQuestionRuntimeTool + Interaction Runtime.

use lotus_app::runtime::interaction::{
    InMemoryInteractionControlPlane, InteractionResolution, PendingInteractionControlPlane,
};
use lotus_app::runtime::tools::catalog::TOOL_CATALOG;
use lotus_app::runtime::tools::catalog::DAILY_ALLOWED_TOOLS;
use serde_json::json;

#[test]
fn ask_user_question_catalog_entry_exists() {
    let entry = TOOL_CATALOG.get_entry("AskUserQuestion");
    assert!(entry.is_some(), "AskUserQuestion must be in TOOL_CATALOG");
}

#[test]
fn ask_user_question_in_daily_allowed_tools() {
    assert!(
        DAILY_ALLOWED_TOOLS.contains(&"AskUserQuestion"),
        "AskUserQuestion should be in DAILY_ALLOWED_TOOLS"
    );
}

#[tokio::test]
async fn interaction_control_plane_submit_resolves() {
    use lotus_app::runtime::interaction::types::{
        InteractionId, InteractionKind, InteractionRequest,
    };
    use lotus_app::runtime::chat::tool_round_types::RuntimeToolCallRequest;
    use lotus_app::runtime::ids::{RunId, SessionId, ToolCallId};

    let cp = InMemoryInteractionControlPlane::new();
    let req = InteractionRequest {
        interaction_id: InteractionId::new("i-test-1"),
        session_id: SessionId::from("sess-test".to_string()),
        run_id: RunId::new("run-test"),
        tool_call_id: ToolCallId::new("tc-test"),
        tool_name: "AskUserQuestion".into(),
        kind: InteractionKind::AskUserQuestion,
        payload: json!({ "questions": [] }),
        original_request: RuntimeToolCallRequest {
            tool_call_id: "tc-test".into(),
            tool_name: "AskUserQuestion".into(),
            args: json!({}),
            purpose: None,
        },
    };

    let rx = cp.insert_pending(req).unwrap();
    cp.resolve(
        &InteractionId::new("i-test-1"),
        InteractionResolution::Submit {
            value: json!({ "answers": { "Which approach?": "Option A" } }),
        },
    )
    .unwrap();

    let resolution = rx.await.unwrap();
    assert!(matches!(resolution, InteractionResolution::Submit { .. }));
}

#[tokio::test]
async fn interaction_control_plane_cancel_for_session() {
    use lotus_app::runtime::interaction::types::{
        InteractionId, InteractionKind, InteractionRequest,
    };
    use lotus_app::runtime::chat::tool_round_types::RuntimeToolCallRequest;
    use lotus_app::runtime::ids::{RunId, SessionId, ToolCallId};

    let cp = InMemoryInteractionControlPlane::new();

    for i in 0..3 {
        let req = InteractionRequest {
            interaction_id: InteractionId::new(format!("i-cancel-{}", i)),
            session_id: SessionId::from("sess-cancel".to_string()),
            run_id: RunId::new("run-cancel"),
            tool_call_id: ToolCallId::new(format!("tc-{}", i)),
            tool_name: "AskUserQuestion".into(),
            kind: InteractionKind::AskUserQuestion,
            payload: json!({}),
            original_request: RuntimeToolCallRequest {
                tool_call_id: format!("tc-{}", i),
                tool_name: "AskUserQuestion".into(),
                args: json!({}),
                purpose: None,
            },
        };
        cp.insert_pending(req).unwrap();
    }

    let cancelled = cp.cancel_for_session("sess-cancel", "session ended");
    assert_eq!(cancelled, 3);
    assert_eq!(cp.pending_count_for_session("sess-cancel"), 0);
}
```

- [ ] **Step 2: 运行测试**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app/src-tauri
cargo test --test ask_user_question_test -- --nocapture 2>&1
```

预期：全部通过。

- [ ] **Step 3: 提交**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app
git add src-tauri/tests/ask_user_question_test.rs
git commit -m "test(tools): add AskUserQuestion and Interaction Runtime integration tests"
```

---

## Task 10: 端到端验证

- [ ] **Step 1: 运行全部 review 回归测试**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app/src-tauri
cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
```

- [ ] **Step 2: 运行关键前端集成测试**

```bash
cd /Users/a20250311/.codex/worktrees/dfd2/lotus-app
pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts 2>&1 | tail -30
```

- [ ] **Step 3: 运行全部前端测试**

```bash
pnpm test 2>&1 | tail -20
```

全部通过后可合并。

---

## 自查结论

- Interaction Runtime 独立于 permission pipeline ✓
- `RuntimeToolCallOutcome::InteractionRequired` 与 `AskRequired` 平级 ✓
- AskUserQuestionDialog 与 PermissionAskDialog 严格分离 ✓
- 所有工具名、字段名在后端/前端定义一致（AskUserQuestion、interactionId、payload.questions）✓
- catalog entry ✓、DAILY_ALLOWED_TOOLS ✓、RuntimeTool 注册 ✓
- 无占位符、无 ToolPlugin、无 tauri::* 依赖 ✓
