# 流式错误处理完整方案

**作者**: pzc
**日期**: 2026-05-28
**分支**: `impl/llm-output-fidelity`
**状态**: 草案待审

---

## 一、问题（30 秒）

**现象**：客户给 AI 大任务（处理长文档 / 大段听记），模型调用工具一段时间后，对话区**完全空白** — 没气泡、没提示、没入口。

**事实**（参考客户日志 `~/Downloads/renlijia_laoxia.log`）：
- 模型 `claude-sonnet-4-5`，token `in=` 26%（不是 PromptTooLong）
- 真错误：反复 `Chunk timeout (90s)`，`run_chat_turn ok=false`

**根因**：
1. **表层**：`chat_turn_driver.rs:2071` 的 `Err(err)` 分支直接 `return`，跳过 Step 6-8（`finalize_content` / `MessagePersisted` / `StreamDone` / `AgentIdle`），前端 chatStore 没 assistant message → 白屏。
2. **深层**：即使补 Step 6-8 让错误进对话气泡，每次 chunk timeout 都让用户看到错误也是糟体验。对标 claude-code-best 后发现，他们在同样位置走了**完全不同的路径**：流式失败时**静默切非流式重发同请求**（`claude.ts:2546-2636`），用户感知只是慢了一点。lotus 缺的不只是 Step 6-8，更是这层**透明兜底**。

---

## 二、方案

### 2.1 设计哲学

> **"全自动透明兜底 + 终态消息化进对话流 + 无重试按钮"**

| # | 原则 | 含义 |
|---|---|---|
| 1 | 能消除的错误就消除 | 后端自动 fallback，对用户透明 |
| 2 | 消除不掉的错误必须可见 | 作为 assistant 消息进对话流（红色 callout），不进 toast |
| 3 | 前端不做"重试"按钮 | 用户重试 = 在输入框再发一次（与微信失败消息、claude-code-best 范式一致） |

### 2.2 三层兜底架构

```
用户发消息
  │
  ├─ 层 1A: stream + 内部重试（10 次退避）          ── 已有，保留
  │   └ 失败 ↓
  │
  ├─ 层 1B: 静默切非流式 send fallback              ── 新增
  │   └ 失败 ↓
  │
  └─ 层 2:  错误消息化进对话流（红色 callout，无按钮）── 新增
            └ 用户重试 = 输入框再发
```

**时间预算**：

| 层 | 退避策略 | 累计耗时 |
|---|---|---|
| 1A | 重试间隔 `2, 4, 8, 16, 32, 60, 60, 60, 60, 60` 秒（指数 + clamp 60s）；每次失败识别本身耗时不定（网络断开 ms 级；chunk timeout 最多 90s） | 仅退避 ~6.4min；典型 ~10-15min；最坏 ~21min |
| 1B | 一次非流式调用（封顶 60s） | ~60s |
| 2 | 立即落库 | <100ms |
| **整 turn 最坏** | | **~7.5min（典型）/ ~21min（最坏极端）** |

> 真实数字来自 `chat.rs:51-67` 的 `STREAM_RETRY_DELAY_SECS=2` + `STREAM_RETRY_MAX_BACKOFF_SECS=60` + 注释 "worst-case total ≈ 6.4 min"。

### 2.3 关键决策（5 条带理由）

| 决策 | 选定 | 理由 |
|---|---|---|
| 错误显示载体 | **对话气泡**（不 toast） | 与 claude-code-best 一致；与 chat 产品形态匹配 |
| 错误是否可删 | **保留**（"伤疤"） | claude-code-best 永远保留；删除连锁风险大（id / tool_call_id 配对、_rev LWW） |
| 重试入口 | **无按钮，用户主动重发** | claude-code-best + 微信范式；UX 差异极小但简化前端 |
| stream 失败兜底 | **fallback 非流式重发** | 网关支持 + 流式失败自动退款（不会双扣费）+ stream-end-then-execute 架构（不会双执行工具） |
| partial 处理 | **重新生成**（不续接） | 与 claude-code-best 一致；避免 LLM 续接行为不可预期 |

**5 条都不做的事**（参见附录 B）：重试按钮 / 删错误气泡 / `retryable` 字段 / chunk timeout 续写 / `max_output_tokens` 续写 / 修 ToastContainer actions 渲染 / 改 55 个非 stream toast。

---

## 三、数据模型

### 3.1 `MessageError` schema

```rust
pub struct StoredMessage {
    // ... 现有字段（id / role / content / tool_calls / ... 全是 Option）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<MessageError>,    // 顶层字段，与 content 同级（**不**塞进 content）
}

pub struct MessageError {
    pub kind: ErrorKind,
    pub message: String,            // UI 兜底文案；i18n 标题由前端按 kind 查表
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,        // 原始错误（脱敏后）；UI 默认不显示
}

#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    ChunkTimeout,    Network,         PromptTooLong,
    AuthFailed,      RateLimited,
    MaxIterations,   BudgetExceeded,
    ExecutionError,  Unknown,
}
```

**字段去留理由**：每个字段都绑定一条具体演化路径

| 字段 | 演化路径 |
|---|---|
| `kind` | UI 差异化引导 / fallback 路由 / 埋点漏斗 / i18n 标题查表 |
| `message` | UI 兜底渲染（i18n 缺失时也能展示） |
| `raw` | 客户截图气泡时自带可排查信息 |
| ~~`title`~~ | 不留：i18n 由前端 `kind → key` 查表 |
| ~~`retryable` / `actions`~~ | 不留：行为由 UI 按 kind 推断 |

**`raw` 脱敏规则**（参考 `docs/harness/diagnostics-log-debugging-guide.md`）：
- 截断 ≤ 500 字符
- 移除 query string 中已知敏感参数（`token` / `api_key` / `session`）
- 不写 stack trace

### 3.2 守卫规则

`error.is_some()` 是核心守卫位（对标 claude-code-best `isApiErrorMessage:true`）：

| 场景 | 行为 | 实现位置 |
|---|---|---|
| UI 渲染 | 永远显示（红色 callout） | `AiBubble` 加 `ErrorCallout` 子组件 |
| 持久化 | 写盘保留 | `messages.jsonl` 自动序列化（`StoredMessage` 已是全 Option 结构，向后兼容） |
| **发给 LLM 下一轮** | **过滤** | `history.rs::build_chat_history` 在 `apply_boundary` 之后过滤 `error.is_some()` |
| session 恢复找上轮终点 | 跳过 | 同上 |
| 用户主动 /rewind | 才删除 | 不在本期 scope |

### 3.3 前端表现

```
┌─────────────────────────────────────┐
│ ⚠ 响应失败                          │
│ AI 服务暂时无法响应（已自动尝试多次）。│
│ 请稍后再试，或换个方式提问。         │
└─────────────────────────────────────┘
```

- 红色 callout 子组件挂在 `AiBubble` 内（保持 React.memo 命中）
- **无按钮**。用户想再试 = 输入框再发

---

## 四、实现：4 个 PR 串行（每个独立有价值）

### PR1 — 修白屏（最小必要，可单独上线）

**范围**：
- `chat_turn_driver.rs` 的 `Err(err)` + `PromptTooLong` 分支补 `MessagePersisted+StreamDone+AgentIdle`
- 内容用文本占位（**先不引入 `error` 字段**，PR2 再引入；本期直接往 `state.full_content` 写错误文案字符串）
- **不动 `finalize_content` 签名**（PR2 才扩展）

**验收**：
- 复现脚本（`/tmp/aijia_hang_stream`）→ 对话区有错误气泡，不再白屏
- `cargo test review_*` 全绿

### PR2 — 错误数据模型 + 红色 callout

**范围**：
- `StoredMessage` 加 `error: Option<MessageError>` + `MessageError` 类型 + `ErrorKind`（9 个）。**向后兼容**：现有字段全是 `#[serde(skip_serializing_if = "Option::is_none")]`；`messages.jsonl` 走 `serde_json::to_string` 自动序列化，新字段自动透传（PR2 启动时 grep 确认无手写字段白名单拼装）
- **`runtime/events.rs::MessagePersisted` 扩字段**：行 172-186 当前 5 个字段（`message_id` / `role` / `content` / `client_message_id` / `tool_calls`），**没有 error**。PR2 必须新增 `error: Option<MessageError>` 字段；`chat_turn_driver` emit 事件时传入；否则前端 `message:updated` 拿不到错误信息
- **`tauri_event_adapter.rs::MessagePersisted` 透传**：行 175 的 `payload` 拼装新增 `if let Some(error) = error { payload["error"] = json!(error); }`，与 `clientMessageId` / `toolCalls` 同模式
- **前端 TS 类型同步**：`src/types/message.ts:8` 的 `Message` **顶层**（不是 `MessageContent`）加 `error?: MessageError`；新增 `MessageError` + `ErrorKind` TS 类型，与 Rust `snake_case` 序列化对齐（`chunk_timeout` 等字面量）
- **`finalize_content` 扩签名**：增 `error: Option<&MessageError>` 参数；PR1 占位字符串路径收编到这里
- 现有错误分支构造 `MessageError`，复用 `ChatTurnOutcome::is_error()`（`turn_outcome.rs:18`）做 outcome → kind 映射
- **`history.rs::build_chat_history` 过滤 `error.is_some()`** 的 `StoredMessage`（在 `apply_boundary` 之后、`stored_to_chat` 之前）
- 前端 `AiBubble` 加 `ErrorCallout` 子组件
- 删 5 个对应 toast：`streamingError` / `streamTimeout` / `MaxIter` / `Budget` / `ExecutionError`

**验收**：
- 错误气泡走红色 callout，不同 kind 显示不同文案
- 连续多次 turn 后历史里的错误消息**不被发回 LLM**（调试日志确认 `messagesForApi`）
- PR1 测试不退化

### PR3 — 非流式 fallback

**前置事实**（已核源码，详见 §五）：
- 服务端零改动（网关已支持非流式 + 自动退款）
- 客户端 `LotusProvider.inner = ClaudeProvider`，fallback 自动走 Anthropic 协议非流式

**范围**：
- **fallback 路径复用 `attach_anthropic_multimodal_turn`**：stream 路径在调 `build_request` 之前先调 `attach_anthropic_multimodal_turn`（`gateway.rs:460`）把多模态信息塞进 messages 数组里，所以 `build_request` 行 286 的 `anthropic_multimodal_turn: None` **不是 bug**。fallback 路径必须以同样的顺序调用，否则丢图
- **`gateway.rs` 新增 `send_message_with_segments`**：签名对齐 `stream_message_with_segments`（含 `max_tokens` / `conversation_id` / `system_segments` / `anthropic_multimodal_turn` / `trace_id` / `run_id`）。现有 `send_message`（行 598，硬编码 4096）保留，仅供 IM ask_coordinator + conversation_service 标题生成等"简单查询"路径继续使用（共 3 处调用点）
- `chat.rs::run_llm_step` 在 `MAX_STREAM_RETRIES`（=10）耗尽后调 `gateway.send_message_with_segments(同 request)`
- 调 fallback 前 emit `RuntimeEvent::StreamRetryReset { reason: RetryReason::FallbackToNonStream }`（**新增 reason 值**，**不**新建事件名）。前端 `useStreaming.ts:524` 现有 `streaming:retry-reset` handler **已经**调 `resetConversationStreamContent(convId)` 清空 partial bubble + 弹 toast；PR3 复用该路径，新 reason 文案改为"切换备用通道"。**注意**：现有 handler 每次 reset 都弹 toast，fallback 进入会再弹一次"切换备用通道"toast — 这与 D' 哲学"不打扰用户"有张力，但比"静默切换"可解释性强，**保留**该 toast
- **fallback 结果拼回**：从 `LlmResponse` 提取 content / tool_calls / usage（含 `cache_creation_input_tokens` / `cache_read_input_tokens`）/ thinking_blocks / stop_reason，按 stop_reason 拼成 `LlmStepResult::ContentComplete` 或 `LlmStepResult::ToolCalls`（`turn_config.rs:176-199` 字段缺一不可，否则丢 token 计费 + thinking 持久化）
- 拼回后走正常 Step 6-8
- 总体超时封顶 60s
- **错误分类复用** `is_retryable_stream_error_str` + `RetryReason`（`chat.rs:2094` / `events.rs:17`）：`NetworkFlap` / `UpstreamBusy` 走 fallback；`RateLimited` 不走（已退避完）；其他错误（401 / 413 / prompt_too_long）直接进层 2
- ⚠️ **`is_retryable_stream_error_str` 当前是字符串子串匹配**（`contains("500") / contains("timeout") / ...`）。CLAUDE.md 第 11 条只点名 401 auth 判定禁止子串匹配，本场景（5xx / 网络错）严格合规；但未来若加 401 fallback 判定**不要复用这函数**，必须用 HTTP status + 错误码 typed 判定

**验收**：
- chunk timeout 用户感知不到错误
- 人为让 fallback 也失败（gateway down），层 2 红色 callout 正确出现

### PR4 — 诊断日志（可选）

按 `docs/harness/diagnostics-log-debugging-guide.md` 加点：
- 层 1A：`stream.retry.attempt` / `stream.retry.exhausted`
- 层 1B：`stream.fallback.{started,success,failed}`
- 层 2：`stream.error_message.persisted { kind }`

**全局回归**：
- `cargo test review_*` / `pnpm test` 全绿
- 剩余 55 个非 stream toast 不退化（设置 / 文件 / IM / 技能 / 认证 / 更新 / 拖拽）

---

## 五、已确认的关键事实（核源码结论）

PR 启动时无需再核对的 5 条事实：

| # | 事实 | 来源 |
|---|---|---|
| 1 | 网关 `/anthropic/v1/messages` 同时支持流式和非流式分支；流式失败自动 `refundBalance(preDeductAmount)` 全额退款；非流式按实际 token 一次性扣费 | lotus-server `anthropic_native.go:190` / `anthropic.go:441` |
| 2 | 客户端 `LotusProvider.inner: ClaudeProvider`，生产路径 100% 走 Anthropic 协议（OpenAI 协议是死代码） | `providers/lotus.rs:43`、CLAUDE.md 第 5 条 |
| 3 | lotus 是 **stream-end-then-execute**（stream 完整结束后才 `execute_round`），fallback 不会双执行工具（与 claude-code-best `claude.ts:2507` 担心的双执行问题无关） | `chat_turn_driver.rs:2247` |
| 4 | `MAX_STREAM_RETRIES = 10`；现有 `RetryReason` 三种（`NetworkFlap` / `RateLimited` / `UpstreamBusy`）+ `is_retryable_stream_error_str` 已就绪可复用 | `chat.rs:51` / `events.rs:17` / `chat.rs:2094` |
| 5 | 前端 `chatStore.resetConversationStreamContent(convId)` 已被现有 `streaming:retry-reset` handler 调用清空 partial（`useStreaming.ts:524-549`）；PR3 复用同事件，新增 `RetryReason::FallbackToNonStream` reason 字面量即可，**前端 handler 主体不动**（只加文案分支） | `useStreaming.ts:524` / `tauri_event_adapter.rs:38` |
| 6 | lotus 已实现 `max_output_tokens` 续写（`MAX_OUTPUT_TOKENS_RECOVERY_LIMIT`），与 claude-code-best 同模式 — 本期不重复 | `chat_turn_driver.rs:2107` |
| 7 | `messages.jsonl` 走 `serde_json::to_string` 自动序列化（注释明确），PR2 加 Option 字段无需改写入路径（PR2 启动时再 grep 一次确认） | `storage/file_store/types.rs:4` |
| 8 | 前端 `Message` / `MessageContent` TS 类型存在，PR2 必须加 `error?: MessageError` 同步对应 | `src/types/message.ts:8 / 67` |
| 9 | fallback 是 **turn-scoped**：`MAX_STREAM_RETRIES` / fallback 触发判定都是 `run_llm_step` 函数内的**局部变量**，turn 完成即丢弃。下一轮用户发新消息 → 全新 `run_llm_step` 调用 → 默认走流式。fallback **不会锁死**会话进入非流式模式（与 claude-code-best 一致；网关每次请求独立计费） | `chat.rs:485-820 stream_retry_count` 局部变量作用域 |

---

## 附录 A：claude-code-best 对标依据（已源码核对）

### A.1 三层兜底链路

```
LLM stream 失败
  → [watchdog: 90s 无 chunk 主动 abort]            claude.ts:1916-1969
  → [非流式 fallback: 同请求重发]                  claude.ts:2546-2636
  → [withRetry: 10 次指数退避]                     withRetry.ts:170-517
  → 最终错误以 assistant `isApiErrorMessage:true`  messages.ts:436
     消息插入 messages[]，UI 红字渲染
```

### A.2 错误消息守卫规则

`isApiErrorMessage: true` 的全部使用点（与本方案 §3.2 守卫表对齐）：

| 场景 | 位置 |
|---|---|
| UI 永远渲染 | `Messages.tsx:160` |
| 持久化 | `messages.ts:772` |
| 发给 LLM 下一轮过滤 | `services/api/errors.ts isSyntheticApiErrorMessage` |
| session 恢复跳过 | `conversationRecovery.ts:288` |
| stop hooks / proactive 跳过防死循环 | `query.ts:1265`、`REPL.tsx:3083` |

### A.3 claude-code-best **不**做的事（本方案对齐）

- 不删除错误气泡（永远保留为"伤疤"）
- 不做"重试"按钮（用户重试 = 自己再发）
- chunk timeout 不做 continue 续写（partial 不可靠）
- 只有 `max_output_tokens` 才做续写（`MAX_OUTPUT_TOKENS_RECOVERY_LIMIT = 3`） — **lotus 也已经实现了相同模式**（`chat_turn_driver.rs:2107`），本期不重复

### A.4 调研子 agent 报告

- subagent **a5f25**：错误消息数据结构 → message 类型 / 错误字段 / synthetic tool_result / 持久化
- subagent **ac86c**：stream 错误处理路径 → watchdog / fallback / withRetry / 错误分类
- subagent **abc1c**：UI 错误渲染 → 组件路径 / 视觉规格 / 数据流 / 用户交互

主路径源码已抽样核对：`claude.ts` / `messages.ts` / `SystemAPIErrorMessage.tsx` / `query.ts` / `REPL.tsx`，结论一致。

---

## 附录 B：明确不做的事

| 不做 | 理由 |
|---|---|
| 重试按钮 / `actions` 字段 | 用户重试 = 自己再发 |
| 删除错误气泡 | 永远保留为"伤疤" |
| `retryable` / `recoverable` 标志 | 用 `kind` 推断 |
| chunk timeout 续写（continue） | partial 不可靠（可能在 tool_use input_json_delta 中断） |
| `max_output_tokens` 自动续写 | **lotus 已实现**（`chat_turn_driver.rs:2107` `MAX_OUTPUT_TOKENS_RECOVERY_LIMIT`，与 claude-code-best 同模式）。本期不动 |
| 修 `ToastContainer` actions 渲染 | 全仓 grep 后确认现有所有 toast 调用点都传 `actions: []`，删完 5 个 stream toast 后无人依赖 |
| 改 55 个非 stream toast | 不在本期 scope |

---

## 附录 C：关键代码位置

| 文件 | 行 | 用途 |
|---|---|---|
| `runtime/chat/chat_turn_driver.rs` | 2071-2078 | **PR1 修复点**（Err 分支） |
| 同上 | 2001-2070 | PR1 同步处理（PromptTooLong 分支） |
| 同上 | 2247 | stream-end-then-execute 验证点 |
| 同上 | 2470-2488 | Step 8 三件套（PR1 抄写参考） |
| `runtime/chat/history.rs` | 28-58 | `build_chat_history`（**PR2 过滤点**） |
| `runtime/chat/post_process.rs` | 51-80 | `finalize_content`（PR2 扩签名） |
| `runtime/chat/turn_outcome.rs` | 9-30 | `ChatTurnOutcome` 枚举 + `is_error()` |
| `runtime/chat/turn_config.rs` | 176-199 | `LlmStepResult` 完整字段 |
| `runtime/events.rs` | 17-25 | `RetryReason` 枚举 |
| `transport/tauri_commands/chat.rs` | 51 | `MAX_STREAM_RETRIES = 10` |
| 同上 | 485-820 | `run_llm_step`（**PR3 接入点**） |
| 同上 | 680-720 | chunk_timeout 重试耗尽（**PR3 替换为 fallback 触发**） |
| 同上 | 2094 | `is_retryable_stream_error_str` |
| `transport/tauri_event_adapter.rs` | 37-40 | `streaming:retry-reset` 事件适配 |
| `llm/gateway.rs` | 244-298 | `build_request`（**PR3 修 bug 点**） |
| 同上 | 322-400 | `stream_message` / `stream_message_with_segments` |
| 同上 | 595-663 | `send_message`（PR3 **新增** `send_message_with_segments` 兄弟方法） |
| `llm/providers/lotus.rs` | 43 / 150-186 | `LotusProvider` 协议确认 + `send` 复用 |
| `storage/file_store/types.rs` | 195-220 | `StoredMessage` 类型（PR2 加 `error` 字段） |
| `src/stores/chatStore.ts` | 75-95 | `resetConversationStreamContent`（PR3 复用，由现有 retry-reset handler 调用） |
| `src/types/message.ts` | 8 / 67 | `Message` / `MessageContent`（**PR2 加 `error?: MessageError`**） |
| `src/components/chat/AiBubble.tsx` | — | PR2 加 `ErrorCallout` |
| `src/hooks/useStreaming.ts` | 353-435 / 780-820 / 985-1000 | PR2 删 5 个 toast |
| 同上 | 524 | `streaming:retry-reset` handler |
| **lotus-server** `proxy/anthropic_native.go` | 156-413 | 网关流式 / 非流式分支 |
| **lotus-server** `proxy/anthropic.go` | 425-451 | 流式预扣 + 失败退款 |
