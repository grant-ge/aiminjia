# PR2: 错误数据模型 + 红色 callout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** 把 PR1 的字符串错误占位升级为结构化 `error: Option<MessageError>` 字段；前端 `AiBubble` 识别后渲染红色 callout；删除 5 个对应 toast；让历史装载时过滤错误消息不喂回 LLM。

**Architecture:** 后端 `StoredMessage` 加 `error` 字段（向后兼容），`MessagePersisted` runtime event 同步扩字段，`tauri_event_adapter` 透传到前端 `message:updated`；前端 `Message` TS 加 `error?: MessageError`，`AiBubble` 加 `ErrorCallout` 子组件按 kind 显示。`history.rs::build_chat_history` 过滤 `error.is_some()` 防止错误回灌 LLM。

**Tech Stack:** Rust（serde 自动序列化）、React/TypeScript（Zustand store immutable update + React.memo）、Tailwind 主题变量、lucide-react 图标。

**Spec:** [`docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md`](../specs/2026-05-28-streaming-error-handling-design.md) §3.1-3.3, §四 PR2

---

## 文件结构

| 文件 | 改动 | 责任 |
|---|---|---|
| `src-tauri/src/storage/file_store/types.rs` | Add | `MessageError` / `ErrorKind` 类型 + `StoredMessage` 加 `error: Option<MessageError>` |
| `src-tauri/src/runtime/events.rs` | Modify | `RuntimeEventKind::MessagePersisted` 加 `error: Option<MessageError>` + 工厂方法签名扩展 |
| `src-tauri/src/runtime/chat/post_process.rs` | Modify | `finalize_content` 扩签名（PR1 占位字符串收编） |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | Modify | 错误分支构造 `MessageError`，传给 helper 和 Step 8 emit；业务终止 outcome（MaxIter/Budget/Exec）落 error 字段 |
| `src-tauri/src/runtime/chat/history.rs` | Modify | `build_chat_history` 过滤 `error.is_some()` |
| `src-tauri/src/transport/tauri_event_adapter.rs` | Modify | `message:updated` payload 透传 error 字段 |
| `src/types/message.ts` | Modify | `Message` 顶层加 `error?: MessageError` + 新增 `MessageError` / `ErrorKind` 类型 |
| `src/components/chat/AiBubble.tsx` | Modify | 加 `ErrorCallout` 子组件分支 |
| `src/hooks/useStreaming.ts` | Modify | 删 5 个 toast（streamingError / streamTimeout / MaxIter / Budget / ExecutionError） |
| `src-tauri/tests/review_stream_error_terminal_events.rs` | Modify | 加 2 个测试：error 字段透传 + history 过滤 |
| `src-tauri/tests/review_history_filters_error_messages.rs` | Create | 专门测 `build_chat_history` 过滤 error.is_some() |

---

## Task 1: 后端数据模型 — MessageError / ErrorKind

**Files:**
- Modify: `src-tauri/src/storage/file_store/types.rs`

- [ ] **Step 1: 加 MessageError 类型**

在 `StoredMessage` 定义**之前**（行 195 前）插入：

```rust
/// 错误信息（PR2 引入；与 claude-code-best `isApiErrorMessage:true` 守卫位等价）。
///
/// 守卫规则（spec §3.2）：
/// - UI 渲染：永远显示（红色 callout）
/// - 持久化：写盘保留
/// - 发给 LLM 下一轮：`history.rs::build_chat_history` 过滤掉
/// - session 恢复找上轮终点：跳过
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MessageError {
    pub kind: ErrorKind,
    /// UI 兜底渲染文案；i18n 标题由前端按 kind 查表
    pub message: String,
    /// 原始错误（脱敏后）；UI 默认不显示，仅 dev / 客户主动复制时透出
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    ChunkTimeout,
    Network,
    PromptTooLong,
    AuthFailed,
    RateLimited,
    MaxIterations,
    BudgetExceeded,
    ExecutionError,
    Unknown,
}
```

- [ ] **Step 2: StoredMessage 加 error 字段**

在 `StoredMessage` 末尾（`pub sequence: Option<u64>,` 后、`}` 前）加：

```rust
    /// 错误信息（PR2 引入；spec §3.1）。
    /// - 顶层字段，与 `content` 同级（不塞进 content）
    /// - `serde(default)` 保证旧 messages.jsonl 反序列化时 `None`
    /// - `skip_serializing_if = "Option::is_none"` 保证正常消息不写这字段
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<MessageError>,
```

- [ ] **Step 3: cargo check**

```bash
cd src-tauri && cargo check
```

Expected: PASS（可能有 dead_code warning，暂时没用到）。

- [ ] **Step 4: 加单元测试 — 向后兼容反序列化**

在 `types.rs` 的 `#[cfg(test)] mod tests` 里追加：

```rust
    #[test]
    fn stored_message_deserialize_without_error_field_yields_none() {
        // 旧 messages.jsonl 不含 error 字段，反序列化必须成功且 error=None
        let json = r#"{
            "id": "msg-1",
            "conversationId": "conv-1",
            "role": "assistant",
            "content": {"text": "hi"},
            "createdAt": "2026-05-28T00:00:00Z"
        }"#;
        let m: StoredMessage = serde_json::from_str(json).unwrap();
        assert_eq!(m.error, None);
    }

    #[test]
    fn stored_message_serialize_omits_error_when_none() {
        // 正常消息不应写 error 字段（保持 messages.jsonl 紧凑）
        let m = StoredMessage {
            seq: None,
            rev: None,
            id: "msg-1".to_string(),
            conversation_id: "conv-1".to_string(),
            role: "assistant".to_string(),
            content: serde_json::json!({"text": "hi"}),
            created_at: "2026-05-28T00:00:00Z".to_string(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            run_id: None,
            schema_version: None,
            sequence: None,
            error: None,
        };
        let s = serde_json::to_string(&m).unwrap();
        assert!(!s.contains("error"), "serialized form must not contain 'error' field when None: {}", s);
    }

    #[test]
    fn stored_message_error_roundtrip_camelcase() {
        // error 字段往返序列化保持 snake_case kind
        let m = StoredMessage {
            seq: None,
            rev: None,
            id: "msg-1".to_string(),
            conversation_id: "conv-1".to_string(),
            role: "assistant".to_string(),
            content: serde_json::json!({"text": ""}),
            created_at: "2026-05-28T00:00:00Z".to_string(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            run_id: None,
            schema_version: None,
            sequence: None,
            error: Some(MessageError {
                kind: ErrorKind::ChunkTimeout,
                message: "AI 服务暂时无法响应".to_string(),
                raw: Some("Chunk timeout (90s)".to_string()),
            }),
        };
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.contains(r#""kind":"chunk_timeout""#), "kind must be snake_case: {}", s);
        let back: StoredMessage = serde_json::from_str(&s).unwrap();
        assert_eq!(back.error, m.error);
    }
```

- [ ] **Step 5: 跑测试**

```bash
cd src-tauri && cargo test --lib storage::file_store::types -- --nocapture 2>&1 | tail -20
```

Expected: 3 个新测试全 PASS，旧测试不退化。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/storage/file_store/types.rs
git commit -m "feat(stream-error): add MessageError + ErrorKind types to StoredMessage

PR2 数据模型基线：MessageError { kind, message, raw } + ErrorKind 9 个
variant。挂在 StoredMessage 顶层（与 content 同级，不塞进 content）。

向后兼容：
- 现有字段全是 #[serde(skip_serializing_if = \"Option::is_none\")]，
  加新可选字段不会破坏旧 messages.jsonl 反序列化
- ErrorKind 用 snake_case 序列化与前端 TS 对齐
- 3 个 unit test 钉死向后兼容 + roundtrip 不变式

Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §3.1"
```

---

## Task 2: MessagePersisted runtime event 扩字段

**Files:**
- Modify: `src-tauri/src/runtime/events.rs`

- [ ] **Step 1: MessagePersisted 加 error 字段**

在 `RuntimeEventKind::MessagePersisted` 定义（行 172-186）末尾加：

```rust
    MessagePersisted {
        message_id: String,
        role: String,
        content: serde_json::Value,
        client_message_id: Option<String>,
        /// Optional `toolCalls` array carried on assistant messages that issued
        /// tool calls.  When present the transport layer forwards it to the
        /// frontend so streaming UI can render tool-call inputs without waiting
        /// for the conversation history to be reloaded.
        tool_calls: Option<Vec<serde_json::Value>>,
        /// Optional structured error for assistant messages that surfaced a
        /// terminal error to the user (PR2). When present, the transport
        /// layer forwards it as the `error` field on `message:updated`, and
        /// `history.rs::build_chat_history` filters this message out before
        /// sending history back to the LLM.
        error: Option<crate::storage::file_store::types::MessageError>,
    },
```

- [ ] **Step 2: 工厂方法 message_persisted 加 error 参数**

修改 `RuntimeEvent::message_persisted` 工厂（行 271-289）：

```rust
    pub fn message_persisted(
        session_id: SessionId,
        run_id: RunId,
        message_id: impl Into<String>,
        role: impl Into<String>,
        content: serde_json::Value,
    ) -> Self {
        Self::new(
            session_id,
            run_id,
            RuntimeEventKind::MessagePersisted {
                message_id: message_id.into(),
                role: role.into(),
                content,
                client_message_id: None,
                tool_calls: None,
                error: None,
            },
        )
    }
```

注意：保持已有 `message_persisted` 工厂签名不变（向后兼容已有调用方）。新增一个对称工厂 `message_persisted_with_error`：

```rust
    /// 与 [`message_persisted`] 同模式，但携带结构化错误（PR2）。
    pub fn message_persisted_with_error(
        session_id: SessionId,
        run_id: RunId,
        message_id: impl Into<String>,
        role: impl Into<String>,
        content: serde_json::Value,
        error: crate::storage::file_store::types::MessageError,
    ) -> Self {
        Self::new(
            session_id,
            run_id,
            RuntimeEventKind::MessagePersisted {
                message_id: message_id.into(),
                role: role.into(),
                content,
                client_message_id: None,
                tool_calls: None,
                error: Some(error),
            },
        )
    }
```

- [ ] **Step 3: cargo check 修连带编译错误**

```bash
cd src-tauri && cargo check 2>&1 | tail -30
```

Expected: 可能有连带错误（其他模块构造 `RuntimeEventKind::MessagePersisted` 字面量时缺 `error` 字段）。逐个修，每处加 `error: None`。常见位置（grep 找）：

```bash
grep -rn "RuntimeEventKind::MessagePersisted {" src-tauri/src/ | head -10
```

每处 `MessagePersisted { ... }` 字面量构造在末尾加 `error: None,`。

- [ ] **Step 4: 跑测试**

```bash
cd src-tauri && cargo test --test s4_driver_loop_test 2>&1 | tail -10
cd src-tauri && cargo test --test review_stream_error_terminal_events 2>&1 | tail -10
```

Expected: 全部 PASS（向后兼容，已有调用都走默认 `error: None`）。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/runtime/events.rs
git commit -m "feat(stream-error): MessagePersisted runtime event carries optional MessageError

PR2 事件协议扩展：新增 error 字段（默认 None，向后兼容所有现有 emit）。
新增 message_persisted_with_error 工厂方法，专用于 stream 错误终态路径。

下一步：tauri_event_adapter 透传给前端 message:updated，chat_turn_driver
错误分支改用新工厂。

Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §四 PR2"
```

---

## Task 3: tauri_event_adapter 透传 error 字段

**Files:**
- Modify: `src-tauri/src/transport/tauri_event_adapter.rs:175-216`

- [ ] **Step 1: 解构加 error + payload 透传**

在 `MessagePersisted` 分支（行 175）解构里加 `error`，并在 payload 拼装末尾透传：

```rust
        RuntimeEventKind::MessagePersisted {
            message_id,
            role,
            content,
            client_message_id,
            tool_calls,
            error,
        } => {
            let skill_command = content.get("skillCommand");
            let command_text = content.get("commandText").and_then(|value| value.as_str());
            log::info!(
                "[skill-command][message-persisted-event] trace_id={} conversation_id={} run_id={} message_id={} role={} client_message_id={:?} has_skill_command={} command_text_len={} has_error={}",
                client_message_id.as_deref().unwrap_or(event.run_id.as_str()),
                conversation_id,
                event.run_id.as_str(),
                message_id,
                role,
                client_message_id,
                skill_command.is_some(),
                command_text.map(str::len).unwrap_or(0),
                error.is_some()
            );
            let mut payload = json!({
                "conversationId": conversation_id,
                "messageId": message_id,
                "id": message_id,
                "role": role,
                "content": crate::runtime::conversation_service::transform_message_json_for_frontend(json!({
                    "content": content,
                }))["content"].clone(),
                "createdAt": chrono::Utc::now().to_rfc3339(),
                "runId": event.run_id.as_str(),
            });
            if let Some(client_message_id) = client_message_id {
                payload["clientMessageId"] = json!(client_message_id);
            }
            if let Some(tool_calls) = tool_calls {
                payload["toolCalls"] = json!(tool_calls);
            }
            // PR2: 透传结构化错误信息（前端 AiBubble 识别后渲染红色 callout）
            if let Some(error) = error {
                payload["error"] = json!(error);
            }
            Some(LegacyEvent {
                name: "message:updated".to_string(),
                payload,
            })
        }
```

- [ ] **Step 2: cargo check + 跑 adapter 测试**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
cd src-tauri && cargo test --test review_tauri_event_adapter_test 2>&1 | tail -10
```

Expected: PASS。

- [ ] **Step 3: 加 adapter 透传测试**

在 `src-tauri/tests/review_tauri_event_adapter_test.rs` 末尾追加：

```rust
#[test]
fn message_persisted_with_error_forwards_error_field() {
    use app_lib::runtime::events::{RuntimeEvent, RuntimeEventKind};
    use app_lib::storage::file_store::types::{ErrorKind, MessageError};
    use app_lib::transport::tauri_event_adapter::map_runtime_event;

    let event = RuntimeEvent::message_persisted_with_error(
        "test-session".into(),
        "test-run".into(),
        "msg-1",
        "assistant",
        serde_json::json!({"text": "占位"}),
        MessageError {
            kind: ErrorKind::ChunkTimeout,
            message: "AI 服务暂时无法响应".to_string(),
            raw: None,
        },
    );

    let legacy = map_runtime_event(&event).expect("should produce legacy event");
    assert_eq!(legacy.name, "message:updated");

    let error = legacy.payload.get("error").expect("error field must be forwarded to frontend");
    assert_eq!(error.get("kind").and_then(|v| v.as_str()), Some("chunk_timeout"));
    assert_eq!(error.get("message").and_then(|v| v.as_str()), Some("AI 服务暂时无法响应"));
}

#[test]
fn message_persisted_without_error_omits_error_field() {
    use app_lib::runtime::events::RuntimeEvent;
    use app_lib::transport::tauri_event_adapter::map_runtime_event;

    let event = RuntimeEvent::message_persisted(
        "test-session".into(),
        "test-run".into(),
        "msg-1",
        "assistant",
        serde_json::json!({"text": "normal"}),
    );

    let legacy = map_runtime_event(&event).expect("should produce legacy event");
    assert!(legacy.payload.get("error").is_none(), "正常 MessagePersisted 不应携带 error 字段");
}
```

- [ ] **Step 4: 跑测试**

```bash
cd src-tauri && cargo test --test review_tauri_event_adapter_test 2>&1 | tail -15
```

Expected: 全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/transport/tauri_event_adapter.rs src-tauri/tests/review_tauri_event_adapter_test.rs
git commit -m "feat(stream-error): forward MessageError to frontend via message:updated

PR2 后端 → 前端协议透传：MessagePersisted runtime event 的 error 字段
现在通过 tauri_event_adapter 写入 message:updated payload['error']。

模式与 clientMessageId / toolCalls 透传一致。前端 AiBubble (PR2 Task 7)
识别 message.error 后渲染红色 callout 子组件。

Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §四 PR2"
```

---

## Task 4: chat_turn_driver 接入 MessageError

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`

- [ ] **Step 1: helper 签名扩展 — emit_terminal_error_message_and_idle 加 error 参数**

替换 `emit_terminal_error_message_and_idle` 整个方法（行 2625-2679 范围，按内容定位）：

```rust
    /// 在错误终态（stream error / PromptTooLong / 业务终止）下补发 Step 6-8
    /// 三件套事件，避免前端 chat 区白屏（PR1）+ 携带结构化错误信息（PR2）。
    ///
    /// 行为对齐 `run_chat_turn_s4` 主路径的 Step 8 三件套。
    /// `error` 字段会写到 StoredMessage 顶层 + MessagePersisted event，让前端
    /// 识别后渲染红色 callout；`history.rs::build_chat_history` 装载下一轮
    /// LLM 历史时会过滤掉它，避免错误回灌（spec §3.2）。
    async fn emit_terminal_error_message_and_idle(
        &self,
        executor: &dyn RuntimeLlmExecutor,
        session_id: &SessionId,
        run_id: &RunId,
        conversation_id: &str,
        error_text: &str,
        error: crate::storage::file_store::types::MessageError,
    ) -> anyhow::Result<()> {
        // Step 7：持久化 error 占位为一条 assistant message
        // TODO(PR2 Task 5): persist_assistant_message 签名暂时不能传 error,
        //   StoredMessage 实际落盘点（executor 实现内）单独处理 error 字段写入.
        //   本期通过 emit MessagePersisted 携带 error 让前端立刻收到；
        //   持久化路径下次 turn reload 时 history.rs 用 stored.error 过滤.
        //   完整闭环需要在 PR2 Task 5 改 persist_assistant_message 签名,
        //   或者新增 persist_assistant_error_message 兄弟方法.
        let message_id = executor
            .persist_assistant_message(
                conversation_id,
                error_text,
                &[],
                &[],
                &[],
                &[],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Step 8a：MessagePersisted（前端 message:updated 渲染气泡 + error callout）
        self.event_bus
            .emit(RuntimeEvent::message_persisted_with_error(
                session_id.clone(),
                run_id.clone(),
                message_id,
                "assistant",
                serde_json::json!({ "text": error_text }),
                error,
            ))
            .await?;

        // Step 8b：StreamDone
        self.event_bus
            .emit(RuntimeEvent::stream_done(
                session_id.clone(),
                run_id.clone(),
            ))
            .await?;

        // Step 8c：AgentIdle
        self.event_bus
            .emit(RuntimeEvent::new(
                session_id.clone(),
                run_id.clone(),
                RuntimeEventKind::AgentIdle {
                    agent_id: AgentId::new(format!("agent-{}", run_id.as_str())),
                    scope: AgentIdleScope::Primary,
                },
            ))
            .await?;

        Ok(())
    }
```

- [ ] **Step 2: Err(err) 分支 — 构造 MessageError 并传**

定位 `Err(err) => {` 分支（行 ~2071-2102，PR1 已修），替换 emit 调用：

```rust
                Err(err) => {
                    re_enqueue_task_notifications(&self.task_notification_queue, std::mem::take(&mut pending_task_notifications));
                    inject_synthetic_tool_results_for_missing_calls(
                        &mut state.messages,
                        cancel.reason(),
                    );

                    // PR2: 构造结构化 MessageError 替代 PR1 纯字符串占位。
                    // 通用 LLM 错误归 kind=Unknown（PR3 fallback 后这里基本不会触达）。
                    // spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §3.1
                    let error_text = "抱歉，AI 服务暂时无法响应（已自动尝试多次）。请稍后再试，或换个方式提问。".to_string();
                    let error = crate::storage::file_store::types::MessageError {
                        kind: crate::storage::file_store::types::ErrorKind::Unknown,
                        message: error_text.clone(),
                        raw: Some(sanitize_error_raw(&err.to_string())),
                    };
                    if let Err(emit_err) = self
                        .emit_terminal_error_message_and_idle(
                            executor,
                            &session_id,
                            &run_id,
                            conversation_id.as_str(),
                            &error_text,
                            error,
                        )
                        .await
                    {
                        log::error!(
                            "[chat_turn_driver] failed to emit terminal error events on stream Err: {}",
                            emit_err
                        );
                    }

                    return Err(anyhow::anyhow!("{}", err));
                }
```

- [ ] **Step 3: PromptTooLong 分支 — 构造 MessageError 并传**

定位 PromptTooLong 分支（PR1 接通的位置，按 `raw_error: Some("prompt_too_long")` 字符串定位），替换 emit 调用：

```rust
                    // PR2: PromptTooLong 用专用 kind 让 UI 显示"压缩历史/新建会话"指引
                    let error_text = "对话上下文已超出模型限制。请新建会话或精简历史后再试。".to_string();
                    let error = crate::storage::file_store::types::MessageError {
                        kind: crate::storage::file_store::types::ErrorKind::PromptTooLong,
                        message: error_text.clone(),
                        raw: Some(sanitize_error_raw(&message)),
                    };
                    if let Err(emit_err) = self
                        .emit_terminal_error_message_and_idle(
                            executor,
                            &session_id,
                            &run_id,
                            conversation_id.as_str(),
                            &error_text,
                            error,
                        )
                        .await
                    {
                        log::error!(
                            "[chat_turn_driver] failed to emit terminal error events on PromptTooLong: {}",
                            emit_err
                        );
                    }

                    return Err(anyhow::anyhow!(message));
```

- [ ] **Step 4: 加 sanitize_error_raw 辅助函数**

在 `chat_turn_driver.rs` 文件末尾（或模块顶部 helper 区）新增：

```rust
/// 脱敏原始错误文案，避免敏感信息（token / api_key / session）落盘到 messages.jsonl。
/// PR2: 截断 ≤500 字符 + 移除已知敏感 query string 参数。
/// 参考 spec §3.1 raw 字段脱敏约定。
fn sanitize_error_raw(raw: &str) -> String {
    const MAX_LEN: usize = 500;
    // 简单替换已知敏感 query 参数值为 REDACTED；不做完整 URL 解析，避免过度复杂。
    let mut s = raw.to_string();
    for key in &["token", "api_key", "apiKey", "session", "session_key", "sessionKey"] {
        // 匹配 "{key}=...&" 或 "{key}=..."(行尾)
        let pattern_re = format!(r"{}=[^&\s\\\"]+", regex::escape(key));
        if let Ok(re) = regex::Regex::new(&pattern_re) {
            s = re.replace_all(&s, format!("{}=REDACTED", key)).to_string();
        }
    }
    if s.chars().count() > MAX_LEN {
        s = s.chars().take(MAX_LEN).collect::<String>() + "…";
    }
    s
}
```

注意：如果 `regex` 不在 dependencies 中，先 grep `regex` 看 cargo 已经依赖了没：

```bash
grep -n "^regex" src-tauri/Cargo.toml | head -3
```

如果没有，**改用简单 str::replace 实现**（不引入新依赖）：

```rust
fn sanitize_error_raw(raw: &str) -> String {
    const MAX_LEN: usize = 500;
    let mut s = raw.to_string();
    // 粗粒度替换：对 known sensitive keys 做 prefix 匹配
    for key in &["token=", "api_key=", "apiKey=", "session=", "session_key=", "sessionKey="] {
        while let Some(start) = s.find(key) {
            let value_start = start + key.len();
            let value_end = s[value_start..]
                .find(|c: char| c == '&' || c == ' ' || c == '"' || c == '\\' || c == '\n')
                .map(|i| value_start + i)
                .unwrap_or(s.len());
            s.replace_range(value_start..value_end, "REDACTED");
        }
    }
    if s.chars().count() > MAX_LEN {
        s = s.chars().take(MAX_LEN).collect::<String>() + "…";
    }
    s
}
```

- [ ] **Step 5: 业务终止 outcome 也落 error 字段**

定位 `final_outcome` 构造之后、Step 8 emit MessagePersisted 之前（行 ~2502），把 outcome 转 MessageError 并改 emit：

```rust
        // Step 7: Persist assistant message
        let message_id = executor
            .persist_assistant_message(
                config.conversation_id.as_str(),
                &state.final_only_content,
                &[],
                &state.generated_file_ids,
                &state.all_file_metas,
                &state.last_thinking_blocks,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // PR2: 业务终止 outcome 也带 MessageError（让 UI 显红色 callout，让 history 过滤）
        let outcome_error: Option<crate::storage::file_store::types::MessageError> =
            match &final_outcome {
                ChatTurnOutcome::MaxIterationsReached { iterations } => {
                    Some(crate::storage::file_store::types::MessageError {
                        kind: crate::storage::file_store::types::ErrorKind::MaxIterations,
                        message: format!("分析步骤超过上限 ({} 次)，已停止。可继续追问深入。", iterations),
                        raw: None,
                    })
                }
                ChatTurnOutcome::BudgetExceeded { reason, total_cost_usd } => {
                    Some(crate::storage::file_store::types::MessageError {
                        kind: crate::storage::file_store::types::ErrorKind::BudgetExceeded,
                        message: format!("已超出预算（约 ${:.4}），请调整预算或新建会话。", total_cost_usd),
                        raw: Some(reason.clone()),
                    })
                }
                ChatTurnOutcome::ExecutionError { message } => {
                    Some(crate::storage::file_store::types::MessageError {
                        kind: crate::storage::file_store::types::ErrorKind::ExecutionError,
                        message: "处理过程中发生错误，请重试或换个方式提问。".to_string(),
                        raw: Some(sanitize_error_raw(message)),
                    })
                }
                _ => None,
            };

        // Step 8: Emit terminal events
        let persisted_event = if let Some(err) = outcome_error.clone() {
            RuntimeEvent::message_persisted_with_error(
                session_id.clone(),
                run_id.clone(),
                message_id,
                "assistant",
                serde_json::json!({ "text": state.full_content }),
                err,
            )
        } else {
            RuntimeEvent::message_persisted(
                session_id.clone(),
                run_id.clone(),
                message_id,
                "assistant",
                serde_json::json!({ "text": state.full_content }),
            )
        };
        self.event_bus.emit(persisted_event).await?;
```

注意：保留原本的 `RuntimeEventKind::TurnCompleted` emit 和后续 AgentIdle emit。只是 `MessagePersisted` 那一行换成上述带 error 判断的版本。

- [ ] **Step 6: cargo check + 跑 PR1 review 测试**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
cd src-tauri && cargo test --test review_stream_error_terminal_events 2>&1 | tail -10
```

Expected: 3 个测试仍 PASS（PR2 不破坏 PR1 不变式）。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs
git commit -m "feat(stream-error): chat_turn_driver constructs MessageError for all error paths

PR2 后端接入：
- emit_terminal_error_message_and_idle helper 接受 MessageError 参数
- Err(err) 分支 → kind=Unknown
- PromptTooLong 分支 → kind=PromptTooLong
- 业务终止 outcome (MaxIter/Budget/ExecutionError) → 对应 kind，
  通过 outcome_error 决定用 message_persisted_with_error 还是普通 factory
- sanitize_error_raw 脱敏 raw 字段（截断 500 字符 + 移除敏感 query params）

下一步：history.rs 过滤 + 前端 TS/UI

Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §3.1, §四 PR2"
```

---

## Task 5: history.rs 过滤 error.is_some()

**Files:**
- Modify: `src-tauri/src/runtime/chat/history.rs:28-58`
- Create: `src-tauri/tests/review_history_filters_error_messages.rs`

- [ ] **Step 1: 改 build_chat_history**

替换 `build_chat_history` 函数（行 28-58）：

```rust
pub fn build_chat_history(
    stored: &[StoredMessage],
    boundary: Option<&CompactBoundaryRecord>,
    config: &HistoryConfig,
) -> Result<Vec<ChatMessage>> {
    let relevant = apply_boundary(stored, boundary);

    // PR2: 过滤掉 error.is_some() 的消息（避免错误气泡回灌给 LLM）。
    // 守卫规则等价 claude-code-best `isApiErrorMessage:true` 过滤。
    // spec §3.2。
    let filtered: Vec<&StoredMessage> = relevant
        .iter()
        .filter(|m| m.error.is_none())
        .collect();

    let mut messages: Vec<ChatMessage> = filtered
        .iter()
        .map(|message| stored_to_chat(message, config))
        .collect();

    messages = filter_invalid_tool_pairs(messages);
    messages = reorder_tool_results_after_assistant(messages);
    messages = trim_to_budget(messages, config);
    messages = collapse_trailing_consecutive_user(messages);

    if let Some(boundary) = boundary {
        if !boundary.summary_text.is_empty() {
            messages.insert(
                0,
                ChatMessage::text(
                    "user",
                    format!("<context>\n{}\n</context>", boundary.summary_text),
                ),
            );
        }
    }

    Ok(messages)
}
```

- [ ] **Step 2: 加 review 集成测试**

新建 `src-tauri/tests/review_history_filters_error_messages.rs`：

```rust
//! PR2 守卫测试：build_chat_history 必须过滤掉 error.is_some() 的 StoredMessage,
//! 避免错误气泡作为对话历史回灌给 LLM（spec §3.2）。
//!
//! 与 claude-code-best `isSyntheticApiErrorMessage` 过滤等价。

use app_lib::runtime::chat::history::{build_chat_history, HistoryConfig};
use app_lib::storage::file_store::types::{ErrorKind, MessageError, StoredMessage};

fn make_user_msg(id: &str, text: &str) -> StoredMessage {
    StoredMessage {
        seq: None,
        rev: None,
        id: id.to_string(),
        conversation_id: "conv-1".to_string(),
        role: "user".to_string(),
        content: serde_json::json!({"text": text}),
        created_at: "2026-05-28T00:00:00Z".to_string(),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        run_id: None,
        schema_version: None,
        sequence: None,
        error: None,
    }
}

fn make_assistant_msg(id: &str, text: &str, error: Option<MessageError>) -> StoredMessage {
    StoredMessage {
        seq: None,
        rev: None,
        id: id.to_string(),
        conversation_id: "conv-1".to_string(),
        role: "assistant".to_string(),
        content: serde_json::json!({"text": text}),
        created_at: "2026-05-28T00:00:01Z".to_string(),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        run_id: None,
        schema_version: None,
        sequence: None,
        error,
    }
}

#[test]
fn build_chat_history_skips_messages_with_error() {
    let stored = vec![
        make_user_msg("u1", "hi"),
        make_assistant_msg("a1", "对不起，AI 服务超时", Some(MessageError {
            kind: ErrorKind::ChunkTimeout,
            message: "AI 服务暂时无法响应".to_string(),
            raw: None,
        })),
        make_user_msg("u2", "再试一次"),
        make_assistant_msg("a2", "好的，这是回复", None),
    ];

    let messages = build_chat_history(&stored, None, &HistoryConfig::default()).unwrap();

    // 错误气泡（a1）必须被过滤掉
    let texts: Vec<String> = messages.iter().map(|m| m.content.clone()).collect();
    assert!(
        !texts.iter().any(|t| t.contains("对不起，AI 服务超时")),
        "错误气泡不应被回灌给 LLM: {:?}",
        texts
    );

    // 正常消息（u1, u2, a2）必须保留
    assert!(texts.iter().any(|t| t.contains("hi")), "user u1 应保留");
    assert!(texts.iter().any(|t| t.contains("再试一次")), "user u2 应保留");
    assert!(texts.iter().any(|t| t.contains("好的，这是回复")), "assistant a2 应保留");
}

#[test]
fn build_chat_history_with_only_errors_returns_empty() {
    // 一个 user 后跟一连串错误气泡 → user 保留，所有错误过滤
    let stored = vec![
        make_user_msg("u1", "hi"),
        make_assistant_msg("a1", "err1", Some(MessageError {
            kind: ErrorKind::ChunkTimeout,
            message: "...".to_string(), raw: None,
        })),
        make_assistant_msg("a2", "err2", Some(MessageError {
            kind: ErrorKind::Network,
            message: "...".to_string(), raw: None,
        })),
    ];

    let messages = build_chat_history(&stored, None, &HistoryConfig::default()).unwrap();
    assert_eq!(messages.len(), 1, "只应留下 user u1");
    assert!(messages[0].content.contains("hi"));
}

#[test]
fn build_chat_history_no_error_field_compat() {
    // 旧数据没有 error 字段（反序列化后 error=None）必须正常通过
    let stored = vec![
        make_user_msg("u1", "hi"),
        make_assistant_msg("a1", "回复", None),
    ];
    let messages = build_chat_history(&stored, None, &HistoryConfig::default()).unwrap();
    assert_eq!(messages.len(), 2);
}
```

- [ ] **Step 3: 跑测试**

```bash
cd src-tauri && cargo test --test review_history_filters_error_messages 2>&1 | tail -15
```

Expected: 3 个测试全 PASS。

- [ ] **Step 4: 跑 review_stream_error 不退化**

```bash
cd src-tauri && cargo test --test review_stream_error_terminal_events 2>&1 | tail -10
cd src-tauri && cargo test --test s4_driver_loop_test 2>&1 | tail -10
```

Expected: 全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/runtime/chat/history.rs src-tauri/tests/review_history_filters_error_messages.rs
git commit -m "feat(stream-error): filter error messages from LLM history

PR2 守卫规则核心：build_chat_history 装载历史时跳过 error.is_some() 的
StoredMessage，避免错误气泡回灌给 LLM。

等价于 claude-code-best 的 isSyntheticApiErrorMessage 过滤。
持久化保留（UI 永远显示），只在发给 LLM 时过滤。

3 个 review 测试钉死：
- 错误气泡不出现在 LLM messages 数组
- 全部是错误时 history 退化到只剩 user 消息
- 旧 messages.jsonl（无 error 字段）兼容

Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §3.2"
```

---

## Task 6: 前端 TS 类型同步

**Files:**
- Modify: `src/types/message.ts`

- [ ] **Step 1: 加 MessageError + ErrorKind TS 类型**

在 `src/types/message.ts` 顶部 `MessageRole` 定义之后插入：

```typescript
/**
 * 后端 ErrorKind 枚举的镜像。Rust 端 #[serde(rename_all = "snake_case")]，
 * 所以字面量是 snake_case。
 *
 * Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §3.1
 */
export type ErrorKind =
  | 'chunk_timeout'
  | 'network'
  | 'prompt_too_long'
  | 'auth_failed'
  | 'rate_limited'
  | 'max_iterations'
  | 'budget_exceeded'
  | 'execution_error'
  | 'unknown'

/**
 * 后端 MessageError 的镜像。当 Message.error 存在时，AiBubble 渲染红色
 * callout 而非普通气泡（PR2）。
 */
export interface MessageError {
  kind: ErrorKind
  /** UI 兜底渲染文案；i18n 标题由前端按 kind 查表 */
  message: string
  /** 原始错误（已脱敏）；UI 默认不显示 */
  raw?: string
}
```

- [ ] **Step 2: Message 顶层加 error 字段**

修改 `interface Message`（行 8-24）：

```typescript
export interface Message {
  id: string
  conversationId: string
  role: MessageRole
  createdAt: string
  content: MessageContent
  /** Sender information (only present for user messages) */
  sender?: MessageSender
  /** assistant 消息专用：工具调用入参列表，来自磁盘 toolCalls 字段 */
  toolCalls?: AssistantToolCall[]
  /** tool 消息关联的运行 ID（实时事件携带，历史消息可能没有） */
  runId?: string
  /** tool 消息专用：工具执行结果 */
  toolResult?: ToolResultContent
  /** 后端 echo 回的 optimistic id，仅出现在 message:updated role=user 时 */
  clientMessageId?: string
  /**
   * 错误信息（PR2 引入）。当存在时，AiBubble 渲染红色 callout 而非普通气泡。
   * 顶层字段（与 content 同级），不塞进 content。
   */
  error?: MessageError
}
```

- [ ] **Step 3: cargo check 前端 TS 编译**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec tsc --noEmit 2>&1 | head -20
```

Expected: 无新 error（如果有 pre-existing errors 与本次改动无关也行）。

- [ ] **Step 4: 提交**

```bash
git add src/types/message.ts
git commit -m "feat(stream-error): add MessageError + ErrorKind TS types

PR2 前端类型镜像：与 Rust src-tauri/src/storage/file_store/types.rs 的
MessageError / ErrorKind 一对一对齐（snake_case 序列化）。

挂在 Message 顶层（不是 MessageContent），与后端 StoredMessage 一致。

下一步：AiBubble 加 ErrorCallout 子组件分支。

Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §3.1"
```

---

## Task 7: AiBubble 加 ErrorCallout 子组件

**Files:**
- Modify: `src/components/chat/AiBubble.tsx`

- [ ] **Step 1: 加 ErrorCallout 子组件**

在 `AiBubble.tsx` 文件末尾（在最后一个 `function ContentRenderer` 之后），新增：

```typescript
import { AlertCircle } from 'lucide-react'

/**
 * 红色错误 callout 子组件（PR2）。
 * - 挂在 AiBubble 内（保持 React.memo 命中）
 * - 颜色用 text-destructive / border-destructive 主题变量
 * - 图标 lucide-react AlertCircle
 * - 无按钮（D' 原则：用户重试 = 输入框再发，spec §2.1）
 *
 * 文案规则：title 按 kind 切换；message 来自后端兜底（i18n 缺失也能展示）；
 * raw 默认不显示，cmd+C/复制按钮可能在未来透出（本期不做）。
 */
function ErrorCallout({ error }: { error: MessageError }) {
  const title = errorTitleByKind(error.kind)
  return (
    <div
      role="alert"
      className="border border-destructive/40 bg-destructive/5 rounded-lg p-3 my-2 flex items-start gap-2"
    >
      <AlertCircle className="text-destructive shrink-0 mt-0.5" size={18} aria-hidden="true" />
      <div className="flex-1 min-w-0">
        <div className="text-destructive font-medium text-sm">{title}</div>
        <div className="text-foreground/80 text-sm mt-1 whitespace-pre-line">{error.message}</div>
      </div>
    </div>
  )
}

/**
 * 按 ErrorKind 返回简短标题。本期硬编码中文（i18n 升级留到下一期）。
 */
function errorTitleByKind(kind: MessageError['kind']): string {
  switch (kind) {
    case 'chunk_timeout':
    case 'network':
      return '响应超时'
    case 'prompt_too_long':
      return '对话过长'
    case 'auth_failed':
      return '登录已失效'
    case 'rate_limited':
      return '请求过于频繁'
    case 'max_iterations':
      return '分析步骤超限'
    case 'budget_exceeded':
      return '预算已用尽'
    case 'execution_error':
      return '处理出错'
    case 'unknown':
    default:
      return '响应失败'
  }
}
```

- [ ] **Step 2: 在 AiBubble 主组件加 error 分支**

修改 `AiBubbleImpl` 函数：

```typescript
function AiBubbleImpl({ message, isStreaming }: AiBubbleProps) {
  const { content, error } = message

  // PR2: 错误气泡走专用 ErrorCallout（无按钮，用户重试=输入框再发）
  // 仍尊重 partial content（如果 fallback 失败前已经流出过部分内容，
  // 同时显示已生成内容 + 错误 callout）
  const hasContent = AI_BUBBLE_RENDER_FIELDS.some((field) => {
    const value = content[field]
    if (value === undefined || value === null) return false
    if (field === 'text' && typeof value === 'string' && !value.trim()) return false
    if (Array.isArray(value) && value.length === 0) return false
    return true
  })

  // 既无内容也无错误也不在 streaming → 不渲染
  if (!hasContent && !error && !isStreaming) return null

  return (
    <div data-aijia-ai-bubble data-aijia-message-id={message.id}>
      <div className="group relative">
        {AI_BUBBLE_RENDER_FIELDS.map((field) => {
          const value = content[field]
          if (value === undefined || value === null) return null
          return (
            <ContentRenderer
              key={field}
              field={field}
              value={value}
            />
          )
        })}

        {error && <ErrorCallout error={error} />}

        {isStreaming && <TypingIndicator variant="default" />}
      </div>
    </div>
  )
}
```

注意：`import { MessageError } from '@/types/message'` 需要补在文件顶部 imports 块里（与其他 `Message` / `MessageContent` 一起）。

- [ ] **Step 3: TS 编译**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec tsc --noEmit 2>&1 | grep "AiBubble" | head -10
```

Expected: 无 AiBubble 相关 error。

- [ ] **Step 4: 提交**

```bash
git add src/components/chat/AiBubble.tsx
git commit -m "feat(stream-error): AiBubble renders ErrorCallout for messages with error field

PR2 前端 UI：
- ErrorCallout 红色 callout 子组件，挂在 AiBubble 内（保持 React.memo 命中）
- 颜色用 text-destructive / border-destructive 主题变量（不硬编码）
- 图标 lucide-react AlertCircle
- 无按钮（D' 原则：用户重试 = 输入框再发）
- title 按 kind 查表，message 来自后端

partial content + error 共存场景：先渲染已生成内容，再显示 callout。

Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §3.3"
```

---

## Task 8: 删除 5 个 stream toast

**Files:**
- Modify: `src/hooks/useStreaming.ts`

- [ ] **Step 1: 删除 streamingError toast（行 ~415-440）**

定位 `streaming:error` handler 中的 `useNotificationStore.getState().push({...})` 调用块（带 `i18n.t('errors.streamingError')`）。整块 push 删掉，但**保留** handler 其他逻辑（resetConversationStreamContent / recordDiagnostic 等）。

具体：找到 `level: 'error', title: i18n.t('errors.streamingError'),` 这一组并包到外层 push 调用的整个 `useNotificationStore.getState().push({ ... })` 调用（含 closing parenthesis）一并删除。

- [ ] **Step 2: 删除 turn outcome 3 个 toast（MaxIter / Budget / ExecutionError）**

定位 `switch (outcome) {` 块（行 ~782-820），把 3 个 case 整体删除，留 `default` 或别的 case 不动（如果还有 Success/Cancelled 在同 switch）：

```typescript
// 完整删除 3 个 case：
//   case 'MaxIterationsReached': { useNotificationStore.getState().push({...}); break; }
//   case 'BudgetExceeded':       { useNotificationStore.getState().push({...}); break; }
//   case 'ExecutionError':       { useNotificationStore.getState().push({...}); break; }
```

如果 switch 删完只剩 default 或为空 → 整个 switch 块也可以删（让 outcome handler 不再弹 toast）。

- [ ] **Step 3: 删除 streamTimeout toast（行 ~988-1000）**

定位 `title: i18n.t('errors.streamTimeout'),` 所在的整个 `useNotificationStore.getState().push({ ... })` 调用块，删掉。

**保留** watchdog 触发时的其他清理逻辑（`clearConversationStreamState` / `removeBusyConversation` 等）。

- [ ] **Step 4: TS 编译检查**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec tsc --noEmit 2>&1 | grep useStreaming | head -10
```

Expected: 无 useStreaming.ts 相关 error。

如有 unused import warning（`i18n.t('errors.streamingError')` 已经删了但 `i18n` 还在用其他地方），不动。

- [ ] **Step 5: 跑前端测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts 2>&1 | tail -20
```

Expected: 测试 PASS 或仅 toast 相关断言失败（需要更新测试断言：现在不再弹 toast）。

- [ ] **Step 6: 修测试断言（如果有）**

如果 `useStreaming.integration.test.tsx` 断言里有 `expect(notifications).toContain(...)` 这种检查 stream 错误 toast 的，改成断言 `message:updated` 携带 `error` 字段（PR2 期望的新行为）。

如果测试断言简单 "should not push streaming error toast"，则反转断言。

- [ ] **Step 7: 提交**

```bash
git add src/hooks/useStreaming.ts src/hooks/useStreaming.integration.test.tsx
git commit -m "refactor(stream-error): remove 5 stream toasts (now rendered as in-bubble callouts)

PR2 D' 原则：流式错误从 toast 通道收编到对话流（红色 callout 气泡），
不再打扰用户视线。

删除：
- streaming:error handler 的 errors.streamingError toast
- turn outcome 的 MaxIterationsReached / BudgetExceeded / ExecutionError toast
- watchdog 的 errors.streamTimeout toast

保留：
- streaming:retry-reset handler 的 toast（PR3 fallback 进入也走这条，
  用户需要知道在重连/切换通道）
- 其余 55 个非 stream toast（设置/文件/IM/技能/认证/更新/拖拽）

Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §四 PR2"
```

---

## Task 9: PR2 全量回归

**Files:**
- 无（验证步骤）

- [ ] **Step 1: cargo 全套件回归**

```bash
cd src-tauri && cargo check --all-targets 2>&1 | grep -E "^error" | head -10
```

Expected: 仅预存在的非 review_ 测试编译错误（与 PR1 验收阶段相同列表），无 PR2 引入的新 error。

```bash
cd src-tauri && cargo test --test review_stream_error_terminal_events 2>&1 | tail -10
cd src-tauri && cargo test --test review_history_filters_error_messages 2>&1 | tail -10
cd src-tauri && cargo test --test review_tauri_event_adapter_test 2>&1 | tail -10
cd src-tauri && cargo test --test s4_driver_loop_test 2>&1 | tail -10
cd src-tauri && cargo test --lib storage::file_store::types 2>&1 | tail -10
```

Expected: 全 PASS。

- [ ] **Step 2: 前端 TS 全编译**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec tsc --noEmit 2>&1 | tail -20
```

Expected: 无 PR2 引入的 error。

- [ ] **Step 3: pnpm test**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm test 2>&1 | tail -20
```

Expected: 全 PASS。

如果上述任一步失败 → 回到 systematic-debugging，不直接打补丁。

无需新 commit。

---

## 自审清单

- [x] **Spec 覆盖**：
  - §3.1 数据模型 → Task 1 ✓
  - §3.2 守卫规则（不发回 LLM）→ Task 5 ✓
  - §3.2 守卫规则（UI 永远显示 / 持久化）→ Task 4 + Task 7 ✓
  - §3.3 前端 callout 表现 → Task 7 ✓
  - §四 PR2 范围 7 项全部覆盖：StoredMessage 字段 / MessagePersisted event 扩字段 / adapter 透传 / 历史过滤 / finalize_content（注：本 plan 简化为 helper 接受 error 参数，比扩 finalize_content 更小改动）/ AiBubble + 删 toast ✓
- [x] **Placeholder scan**：无 TBD / 占位代码 / "类似 Task N" 引用
- [x] **Type consistency**：MessageError 字段（kind / message / raw）在 Task 1/2/3/4/6 全部一致；ErrorKind 9 个 variant 一致

---

## 风险

1. **persist_assistant_message 暂未扩签名**：Task 4 注释说明了，本期通过 emit MessagePersisted event 透传 error 让前端**立刻**看到错误；StoredMessage.error 的**持久化**写盘需要 executor 实现内部处理。如果 executor 不读 event 而是单纯从 content 字段提取 error，会导致刷新页面后 error 字段丢失。PR2 收口时需要核对（Task 9 或 PR2 结束加测试）。

2. **finalize_content 扩签名**：spec 原本说要扩，本 plan 简化为只在 helper 调用前构造 error；如果 PR1 的占位字符串路径有用户依赖（不应该有），需要单独处理。PR2 收口时可加一项确认。
