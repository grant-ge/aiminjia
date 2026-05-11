# Anthropic 协议迁移后的清理与加固

**日期**: 2026-05-10
**范围**: `src-tauri/src/llm/` + `runtime/chat/prompt/` + `runtime/query_engine.rs` + `transport/tauri_*` + 前端 TS 类型
**背景**: 几天前桌面端与 LLM 的通信协议从 OpenAI 协议切换为 Anthropic 协议（生产路径 `lotus.rs` → `claude.rs`，直接透传 `/anthropic/v1/messages`）。本次工作针对切换后的遗留与命名清理。

---

## 一、问题评估结论

切换后主路径功能正确，无 P0 阻断。存在以下偏差需要修复：

| 维度 | 现状 | 影响 |
|---|---|---|
| 对话/SSE | Anthropic 事件解析完整 | ✅ |
| Token 统计 | `TokenUsage` 缺 `cache_creation_input_tokens` / `cache_read_input_tokens` | ⚠️ 命中率/成本不可观测 |
| 文件处理 | 走 workspace + Read/Glob/Grep 工具，协议无关 | ✅ |
| 工具调用 | schema / 流式累积 / tool_result 全部正确；仅 `redacted_thinking` 块仅透传 `type`+`data` | ⚠️ thinking 签名链脆弱 |
| 命名 | `OpenAiChatPromptRenderer` / `openai_system_message` 等命名误导 | 💡 |
| 死代码 | deepseek_v3/r1 / qwen / volcano 四个直连 provider 已无路由命中 | 💡 |
| Cost 估算 | 仅 `(input+output) × rate`，未区分 Anthropic cache 定价 | ⚠️ 估算偏低 |

---

## 二、修复内容（按 commit 时序）

### 1. `91591cd` fix(llm/claude): redacted_thinking 整块原样回传 + cache token 边界单测

- `claude.rs::process_sse_data` 的 `content_block_start` 分支识别 `redacted_thinking`，**整块 `block.clone()`** 经 `StreamEvent::ThinkingBlock` 透传，避免未来 API 加字段时丢失。
- 新增 3 个边界单测：
  - `test_message_delta_does_not_clobber_cache_tokens_from_message_start` — `message_delta` 缺 `cache_*` 字段时必须保留 `message_start` 已读值
  - `test_cache_tokens_robust_to_missing_or_invalid_usage` — `usage` 缺失 / null / 字符串 / 浮点 / 负数 均安全降级为 `None`
  - `test_redacted_thinking_block_preserves_all_fields` — 整块透传所有字段

### 2. `a798b4d` feat(query_engine): cost 估算按 Anthropic 定价区分 cache 读写

`runtime/query_engine.rs::estimated_cost_usd` 公式更新为：

```
cost = (input + output + cache_creation × 1.25 + cache_read × 0.1) / 1000 × rate
```

对应 Anthropic 官方定价（cache write 1.25× input、cache read 0.1× input）。新增单测 `o4_estimated_cost_usd_anthropic_cache_weighting`。

> **注**：output 仍按 `rate` 计费，与切换前一致。Anthropic 实际 output 单价通常为 input 的 ~5×，但当前结构只携带单一 rate；这是历史遗留，需要分拆 `input_rate` / `output_rate` 时再做（不在本次范围）。

### 3. `96dc609` feat(llm): per-block cache_control 结构化透传到 Anthropic API

修复 system prompt 多段缓存策略（StaticPrefix / SessionDynamic / Volatile）被 flatten 成单一字符串、只能整体打缓存的问题。

- 新增类型 `llm::streaming::SystemPromptSegment { text, cache }`
- `LlmRequest` 增加 `system_segments: Option<Vec<SystemPromptSegment>>`
- `LlmGateway` 新增并存式入口 `stream_message_with_segments(...)`，老 `stream_message(...)` 签名不变（内部走 `None`）
- `claude.rs::build_request_body`：
  - 当 `system_segments` 非空时，按段渲染为 `[{type:"text", text, cache_control?:{type:"ephemeral"}}, ...]`
  - **system 侧最多 3 个 ephemeral 块**（Anthropic 总额度 4 个，预留 1 给 tools）；超出则丢弃多余 `cache_control` 标记并 log warn
  - 老路径（仅字符串）保持单块 ephemeral 行为
- `openai.rs`（OpenAI 兼容协议）：segments 简单 join，行为等价
- `transport/tauri_commands/chat.rs::system_prompt_segments()` 从渲染后的 system message JSON 反提取 segments，`run_llm_step` 改调 `stream_message_with_segments`
- 新增 3 个单测覆盖：多段 cache_control 正确渲染 / cap 裁剪 / 无 segments 时走单块 fallback

### 4. `38dd9e7` refactor(llm): 删除死代码 provider + prompt renderer 协议中性重命名

**删除文件**：
- `src-tauri/src/llm/providers/deepseek_v3.rs`
- `src-tauri/src/llm/providers/deepseek_r1.rs`
- `src-tauri/src/llm/providers/qwen.rs`
- `src-tauri/src/llm/providers/volcano.rs`

**同步清理**：
- `providers/mod.rs` 的 `pub mod` + P-router-model-passthrough 计划注释
- `gateway.rs::dispatch_stream` / `dispatch_send` 4 个 match arm + unknown fallback 改 `lotus`
- `router.rs::get_provider_capabilities` 3 个 arm + default fallback
- `transport/tauri_commands/settings.rs` + `commands/settings.rs` 的 `validate_api_key` arm
- `router.rs` 测试默认 `primary_model="claude"`，相关 assertion 同步

**重命名（协议中性）**：

| 旧 | 新 |
|---|---|
| `runtime/chat/prompt/renderer_openai.rs` | `runtime/chat/prompt/renderer.rs`（`git mv` 保留历史） |
| `OpenAiChatPromptRenderer` | `ChatPromptRenderer` |
| `openai_system_message` / `openai_system_message_flat` | `system_message` / `system_message_flat` |
| `openai_system_prompt_content` | `system_prompt_content` |
| `openai_system_prompt_segments` | `system_prompt_segments` |

涉及字段：`TurnPromptSnapshot` / `LlmStepInput` / `WorkerTurnRequest` 三处同步改名。

**保留**：
- `openai.rs` —— `custom.rs::send_openai_compat` 仍依赖它处理 OpenAI 兼容第三方供应商
- `AppSettings.cloud_model_type` —— 持久化兼容字段，no-op
- 测试守卫 `lotus_anthropic_ingress_test.rs` 中"响应不应含 `function_call`"等断言

---

## 三、已落地能力清单（Anthropic 协议侧）

- ✅ 流式事件全集：`message_start` / `content_block_start|delta|stop` / `message_delta` / `error` / `ping` / `message_stop`
- ✅ Content block 类型：`text` / `thinking` / `redacted_thinking` / `tool_use`
- ✅ Thinking 签名往返：`thinking_blocks` 整块（含未来字段）原样回传
- ✅ Tool use 流式累积（`input_json_delta.partial_json` 拼接 + `content_block_stop` finalize + 流末尾兜底）
- ✅ Tool schema 翻译：`parameters` → `input_schema`
- ✅ Tool result 回传：`role:"user"` + `content:[{type:"tool_result", tool_use_id, content}]`
- ✅ System prompt：从 messages 提取到顶层 `system` 字段
- ✅ Prompt caching：支持 per-block `cache_control: ephemeral`（system 侧 cap 3 块）
- ✅ Token 统计：`input_tokens` / `output_tokens` / `cache_creation_input_tokens` / `cache_read_input_tokens`
- ✅ Cost 估算：cache write 1.25× + cache read 0.1×

---

## 四、刻意未做（已评估，未来需要时再做）

| 项 | 原因 | 触发条件 |
|---|---|---|
| `ChatMessage.content` 升级为结构化 enum（支持 `image` 块） | 当前附件走 workspace+Read 工具路径，协议无关；改造影响面大 | 产品要做原生 vision（截图直发模型不走文件中转） |
| `openai.rs` 文件级改名为 `openai_compat.rs` + `OpenAiProvider` → `OpenAiCompatProvider` | 当前命名已不再误导（外部使用者明确"OpenAI 兼容协议"） | 出现新人误解或大幅重构时一并做 |
| `cost_per_1k_tokens` 分拆 `input_rate` / `output_rate` | 历史遗留，Anthropic output 实际 ~5× input | 计费精度需求出现 |
| `router.rs::test_route_reasoning_uses_reasoning_model` 语义漂移 | C 清理后 primary=`claude`，落到默认分支 `use_tools=true`（原 deepseek-R1 路径是 `use_tools=false`）。测试已同步更新；若其他地方有"reasoning 必关 tools"的隐式假设需排查 | 出现工具调用在 reasoning 模式下行为异常 |

---

## 五、测试基线

- `cargo check --lib` clean（仅 4 个 pre-existing 警告）
- `cargo test --lib llm::` — 213 passed（1 个 pre-existing 失败 `test_llm_request_default`，max_tokens 默认值漂移）
- `cargo test --lib llm::providers::claude` — 16 passed
- `cargo test --test lotus_anthropic_ingress_test` — 5 passed
- `cargo test --test review_tauri_event_adapter_test` — 14 passed
- `cargo test --test plan_o_queryengine_session_state_test` — 10 passed
- `cargo test --test prompt_openai_renderer_test prompt_renderer_openai_test` — 5 passed

---

## 六、commit 列表

| Hash | 说明 |
|---|---|
| `91591cd` | redacted_thinking 整块回传 + cache token 边界单测 |
| `a798b4d` | cost 估算按 Anthropic cache 定价加权 |
| `96dc609` | per-block cache_control 结构化透传 |
| `38dd9e7` | 死代码 provider 清理 + prompt renderer 协议中性重命名 |

注：P1 主体（`TokenUsage` / `TotalTokenUsage` cache 字段、`claude.rs` 三处响应解析补读、`TurnCompleted` 事件透传、前端 TS 类型同步）已被前置 commit `ffa8068`（"流式假超时/工具结果重复/迭代上限"）一并吃掉，所以本次 commit 链从 review 阶段（`91591cd`）算起。
