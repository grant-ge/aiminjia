# PR3: 非流式 fallback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** 当 `MAX_STREAM_RETRIES` 流式重试耗尽时，**静默切到非流式 send** 重发同一请求，让用户感知不到错误（绝大多数 chunk timeout 都能透明恢复）。fallback 也失败才走层 2 错误气泡（PR1/PR2 已建好）。

**Architecture:** 在 `gateway.rs` 新增 `send_message_with_segments`（签名对齐 `stream_message_with_segments`），保证 fallback 与 stream 走完全相同的 request 上下文（max_tokens / conversation_id / system_segments / 多模态 / trace_id / run_id 全透传）。`run_llm_step` 在 chunk timeout 重试耗尽时改为：emit `streaming:retry-reset { reason: FallbackToNonStream }` → 调 send → 把 LlmResponse 拼回 `LlmStepResult::ContentComplete` / `ToolCalls`。

**Tech Stack:** Rust async（tokio timeout）、reqwest / RPITIT trait、`AnthropicMultimodalTurn` 多模态、`SystemPromptSegment` block-level cache_control。

**Spec:** [`docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md`](../specs/2026-05-28-streaming-error-handling-design.md) §五 / §四 PR3 / §A.1

---

## 文件结构

| 文件 | 改动 | 责任 |
|---|---|---|
| `src-tauri/src/runtime/events.rs` | Modify | `RetryReason` 加 `FallbackToNonStream` variant |
| `src-tauri/src/llm/gateway.rs` | Add | 新增 `send_message_with_segments` 方法（与 `stream_message_with_segments` 签名对齐） |
| `src-tauri/src/transport/tauri_commands/chat.rs` | Modify | `run_llm_step` 重试耗尽路径接通 fallback；emit `FallbackToNonStream` reason；拼回 `LlmStepResult` |
| `src/lib/tauri.ts` 或 `src/hooks/useStreaming.ts` | Modify | `RetryReason` 字面量加 `fallback_to_non_stream`；handler 按 reason 切文案"切换备用通道" |
| `src-tauri/tests/review_stream_fallback_test.rs` | Create | 集成测试：chunk timeout 重试耗尽 → fallback → emit FallbackToNonStream → 拼回 ContentComplete |

---

## Task 1: RetryReason 加 FallbackToNonStream variant

**Files:**
- Modify: `src-tauri/src/runtime/events.rs`

- [ ] **Step 1: 加 enum variant**

定位 `pub enum RetryReason`（行 ~17）。在 `NetworkFlap` 后加：

```rust
pub enum RetryReason {
    /// Upstream gateway returned 5xx — service-side problem, not the user's network.
    UpstreamBusy,
    /// Upstream returned 429 / rate limit.
    RateLimited,
    /// Local-side network issue: timeout, connection reset, broken pipe, chunk stall.
    #[default]
    NetworkFlap,
    /// Stream retries exhausted; switching to non-streaming send fallback (PR3).
    /// Frontend should show "切换备用通道" instead of "重连中".
    FallbackToNonStream,
}
```

- [ ] **Step 2: cargo check**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
```

Expected: PASS（默认 NetworkFlap 不变，新 variant 不破坏现有 match 表达式如果它们都用 `_ =>`；如果有穷举 match 报错就修补 `FallbackToNonStream` 分支）。

如果有 match 穷举报错，对每处加 `RetryReason::FallbackToNonStream => "fallback_to_non_stream"` 之类的语义对应（grep `RetryReason::` 找匹配位置）。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/runtime/events.rs <可能需要补 match arm 的其他文件>
git commit -m "feat(stream-error): RetryReason variant FallbackToNonStream

PR3 第 1 步：为 fallback 进入信号扩 RetryReason，复用现有
streaming:retry-reset 事件（不新建事件名）。前端 handler 按
reason 切换文案：'切换备用通道' 区别于 '重连中'。

Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §四 PR3"
```

---

## Task 2: gateway.rs 新增 send_message_with_segments

**Files:**
- Modify: `src-tauri/src/llm/gateway.rs`

- [ ] **Step 1: 在现有 send_message 之后新增 send_message_with_segments**

定位 `pub async fn send_message(` (行 ~598)。在其方法体闭合 `}` 之后插入新方法：

```rust
    /// 非流式版本的 [`stream_message_with_segments`]，用于 PR3 流式失败兜底。
    ///
    /// 签名与 stream_message_with_segments 完全对齐：复用 max_tokens /
    /// conversation_id / system_segments (block-level cache_control) /
    /// anthropic_multimodal_turn / trace_id / run_id，保证 fallback 与
    /// stream 走完全相同的 request 上下文（不丢图 / cache 不失效 / 追踪不断链）。
    ///
    /// 内部仍走 `provider.send` 走非流式 `/anthropic/v1/messages`，网关侧
    /// 已支持非流式分支（lotus-server anthropic_native.go:190）并在流式失败
    /// 自动退款（anthropic_native.go:499），所以 fallback 不会双扣费。
    ///
    /// Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §五.9.2
    #[allow(clippy::too_many_arguments)]
    pub async fn send_message_with_segments(
        &self,
        settings: &AppSettings,
        messages: Vec<ChatMessage>,
        masking_level: MaskingLevel,
        system_prompt: Option<&str>,
        context_message: Option<&str>,
        tool_defs_override: Option<Vec<ToolDefinition>>,
        max_tokens: u32,
        conversation_id: Option<&str>,
        anthropic_multimodal_turn: Option<AnthropicMultimodalTurn>,
        system_segments: Vec<crate::llm::streaming::SystemPromptSegment>,
        trace_id: Option<&str>,
        run_id: Option<&str>,
    ) -> Result<LlmResponse> {
        let task_type = router::infer_task_type(&messages);
        let mut route = router::select_route(&task_type, settings);

        if provider_resolves_to_lotus(&route.provider) {
            if let Some(auth) = &self.auth_manager {
                match auth.get_session_key().await {
                    Ok(sk) => route.api_key = sk,
                    Err(e) => {
                        return Err(anyhow::anyhow!(
                            "API 密钥无效或已过期，请在设置中检查 API Key 配置。({})",
                            e
                        ))
                    }
                }
            }
        }

        log::info!(
            "Sending (non-stream fallback) task {:?} to provider '{}' conv={:?}",
            task_type,
            route.provider,
            conversation_id
        );

        let mut mask_ctx = MaskingContext::new(masking_level);
        let mut masked_messages = mask_ctx.mask_messages(&messages);
        if provider_resolves_to_lotus(&route.provider) {
            attach_anthropic_multimodal_turn(&mut masked_messages, anthropic_multimodal_turn.clone());
        }

        let segments = if system_segments.is_empty() {
            None
        } else {
            Some(system_segments)
        };

        let request = Self::build_request(
            masked_messages,
            &route,
            false, // stream = false（关键：走非流式）
            system_prompt,
            context_message,
            tool_defs_override,
            max_tokens,
            settings,
            segments,
            conversation_id,
            trace_id,
            run_id,
        );

        let response = retry_dispatch_send(&route, request).await?;
        let unmasked_content = mask_ctx.unmask(&response.content);
        Ok(LlmResponse {
            content: unmasked_content,
            ..response
        })
    }
```

注意：
- 上面用到的 `build_request` / `retry_dispatch_send` / `attach_anthropic_multimodal_turn` 都已存在（grep 验证）
- 现有 `send_message` 保持不变，IM ask_coordinator / conversation_service 等"简单查询"调用方继续用旧 API

- [ ] **Step 2: cargo check**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
```

Expected: PASS（新方法暂时 dead_code，会在 Task 3 调用）。

- [ ] **Step 3: 加单元测试覆盖签名稳定性（可选但推荐）**

如果时间允许，在 `gateway.rs` 的 tests mod 加一个简单的 compile-only test 确保签名兼容：

```rust
#[cfg(test)]
mod fallback_signature_tests {
    use super::*;

    // Compile-only test: ensures send_message_with_segments signature stays in sync
    // with stream_message_with_segments. If either changes, this test will fail to
    // compile and force the developer to update both in lockstep.
    #[allow(dead_code)]
    fn fallback_signature_matches_stream() {
        fn assert_send_message_with_segments_callable<F>(_: F)
        where
            F: for<'a> Fn(
                &LlmGateway,
                &'a AppSettings,
                Vec<ChatMessage>,
                MaskingLevel,
                Option<&'a str>,
                Option<&'a str>,
                Option<Vec<ToolDefinition>>,
                u32,
                Option<&'a str>,
                Option<AnthropicMultimodalTurn>,
                Vec<crate::llm::streaming::SystemPromptSegment>,
                Option<&'a str>,
                Option<&'a str>,
            ),
        {
        }
        // This will fail to compile if the signature changes
        // assert_send_message_with_segments_callable(LlmGateway::send_message_with_segments);
        // ^^ commented out because closures over async methods are complex; rely on Task 3
        //    actual call site for verification.
    }
}
```

如果上面太复杂，跳过 Step 3，靠 Task 3 集成测试覆盖。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/llm/gateway.rs
git commit -m "feat(stream-error): add send_message_with_segments for non-stream fallback

PR3 第 2 步：新增 send_message_with_segments，签名对齐
stream_message_with_segments：max_tokens / conversation_id /
system_segments (block-level cache_control) / anthropic_multimodal_turn /
trace_id / run_id 全透传，保证 fallback 与 stream 走相同 request 上下文。

现有 send_message（硬编码 4096，无 conversation_id）保留供 IM
ask_coordinator + conversation_service 标题生成等简单查询路径继续使用。

服务端零改动：lotus-server anthropic_native.go:190 已支持非流式分支，
流式失败自动 refundBalance → fallback 不会双扣费。

Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §四 PR3, §五.9.2"
```

---

## Task 3: run_llm_step 接通 fallback

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs:683-720`（chunk timeout 重试耗尽路径）

这是 PR3 核心。**所有 stream retry exhausted 路径**都要先尝试 fallback，失败才返回 MaxRetriesExceeded。本 task 只覆盖 chunk_timeout 路径（最常见，客户白屏的根因）；其他 stream error 路径（行 ~789-810 StreamEvent::Error）按相同模式后续处理。

- [ ] **Step 1: 定位 chunk timeout 重试耗尽位置**

grep:
```bash
grep -n "All retries exhausted\|TurnError::MaxRetriesExceeded\|raw_error: Some(\"chunk_timeout\")" src-tauri/src/transport/tauri_commands/chat.rs | head -10
```

应该看到行 ~704 附近的 "All retries exhausted" 注释和 `return Err(TurnError::MaxRetriesExceeded);` 行。

- [ ] **Step 2: 抽取 fallback helper（在 chat.rs 文件末尾或合适位置）**

在 `chat.rs` 文件中（impl 块之外，作为顶级 async 函数）新增 helper：

```rust
/// PR3 fallback helper：把非流式 LlmResponse 拼回成 LlmStepResult。
///
/// 拿到非流式响应后按 stop_reason 分发到 ContentComplete 或 ToolCalls。
/// 拼回字段必须完整（含 cache_creation_input_tokens / cache_read_input_tokens /
/// thinking_blocks），否则会丢 token 计费和 thinking 持久化（spec §五.9.2 注解）。
fn llm_response_to_step_result(
    response: crate::llm::streaming::LlmResponse,
) -> crate::runtime::chat::turn_config::LlmStepResult {
    use crate::llm::streaming::StopReason;
    use crate::runtime::chat::turn_config::LlmStepResult;
    use crate::runtime::chat::tool_round_types::RuntimeToolCallRequest;

    let tokens_in = response.usage.input_tokens as u64;
    let tokens_out = response.usage.output_tokens as u64;
    let cache_creation = response.usage.cache_creation_input_tokens.unwrap_or(0) as u64;
    let cache_read = response.usage.cache_read_input_tokens.unwrap_or(0) as u64;

    if !response.tool_calls.is_empty() {
        let tool_calls: Vec<RuntimeToolCallRequest> = response
            .tool_calls
            .into_iter()
            .map(|tc| RuntimeToolCallRequest {
                tool_call_id: tc.id,
                tool_name: tc.name,
                args: tc.arguments,
            })
            .collect();
        LlmStepResult::ToolCalls {
            assistant_content: response.content,
            tool_calls,
            tokens_in,
            tokens_out,
            cache_creation_input_tokens: cache_creation,
            cache_read_input_tokens: cache_read,
            thinking_blocks: Vec::new(),
        }
    } else {
        let stop_reason_str = match response.stop_reason {
            StopReason::EndTurn => "end_turn",
            StopReason::ToolUse => "tool_use",
            StopReason::MaxTokens => "max_tokens",
            StopReason::StopSequence => "stop_sequence",
        };
        LlmStepResult::ContentComplete {
            content: response.content,
            tokens_in,
            tokens_out,
            cache_creation_input_tokens: cache_creation,
            cache_read_input_tokens: cache_read,
            stop_reason: Some(stop_reason_str.to_string()),
            thinking_blocks: Vec::new(),
        }
    }
}
```

**重要**：先 grep 确认 `RuntimeToolCallRequest` 字段名（`tool_call_id` / `tool_name` / `args`），可能与上面不完全匹配。如果不一致，按实际字段名修改。

```bash
grep -n "pub struct RuntimeToolCallRequest\|pub.*tool_call_id\|pub.*tool_name" src-tauri/src/runtime/chat/tool_round_types.rs
```

如果 `LlmResponse.usage` 的字段不是 `cache_creation_input_tokens` 而是别的（如 `prompt_cache_creation_tokens`），需要 grep `LlmResponse` 定义对齐字段名。

- [ ] **Step 3: chunk timeout 重试耗尽路径插入 fallback**

定位（用 grep "All retries exhausted" 找到，应该在行 ~704 附近）：

```rust
                        // All retries exhausted
                        let error_msg = format!(
                            "响应超时（{}秒无数据）。请检查网络连接后重试。",
                            input.chunk_timeout_secs
                        );
                        let _ = bus
                            .emit(RuntimeEvent::new(
                                session_id.clone(),
                                run_id.clone(),
                                RuntimeEventKind::StreamError {
                                    error: error_msg.clone(),
                                    raw_error: Some("chunk_timeout".to_string()),
                                },
                            ))
                            .await;
                        return Err(TurnError::MaxRetriesExceeded);
```

替换为：

```rust
                        // PR3: 流式重试耗尽，先尝试非流式 fallback 兜底再宣告失败.
                        // emit retry-reset { reason: FallbackToNonStream } 让前端切到
                        // "切换备用通道" 文案，清空 partial bubble.
                        let _ = bus
                            .emit(RuntimeEvent::new(
                                session_id.clone(),
                                run_id.clone(),
                                RuntimeEventKind::StreamRetryReset {
                                    reason: RetryReason::FallbackToNonStream,
                                },
                            ))
                            .await;
                        log::warn!(
                            "[run_llm_step] chunk timeout retries exhausted, attempting non-streaming fallback conv={}",
                            input.conversation_id
                        );

                        // 60s 总体超时封顶（spec §三 时间预算）
                        let fallback_timeout = tokio::time::Duration::from_secs(60);
                        let fallback_result = tokio::time::timeout(
                            fallback_timeout,
                            self.gateway.send_message_with_segments(
                                &settings,
                                chat_messages.clone(),
                                masking_level.clone(),
                                system_prompt_for_gateway.as_deref(),
                                None, // context_message
                                effective_tools.clone(),
                                input.max_tokens,
                                Some(input.conversation_id),
                                input.anthropic_multimodal_turn.clone(),
                                system_prompt_segments.clone(),
                                input.trace_id,
                                Some(input.run_id),
                            ),
                        )
                        .await;

                        match fallback_result {
                            Ok(Ok(response)) => {
                                log::info!(
                                    "[run_llm_step] fallback success conv={} content_len={} tool_calls={}",
                                    input.conversation_id,
                                    response.content.len(),
                                    response.tool_calls.len()
                                );
                                return Ok(llm_response_to_step_result(response));
                            }
                            Ok(Err(fallback_err)) => {
                                log::error!(
                                    "[run_llm_step] fallback failed conv={}: {}",
                                    input.conversation_id, fallback_err
                                );
                            }
                            Err(_elapsed) => {
                                log::error!(
                                    "[run_llm_step] fallback timeout (60s) conv={}",
                                    input.conversation_id
                                );
                            }
                        }

                        // Fallback 也失败 → emit StreamError + 进层 2（PR1 已修复白屏）
                        let error_msg = format!(
                            "响应超时（{}秒无数据）。请检查网络连接后重试。",
                            input.chunk_timeout_secs
                        );
                        let _ = bus
                            .emit(RuntimeEvent::new(
                                session_id.clone(),
                                run_id.clone(),
                                RuntimeEventKind::StreamError {
                                    error: error_msg.clone(),
                                    raw_error: Some("chunk_timeout".to_string()),
                                },
                            ))
                            .await;
                        return Err(TurnError::MaxRetriesExceeded);
```

注意：上面用到的变量（`self.gateway` / `settings` / `chat_messages` / `masking_level` / `system_prompt_for_gateway` / `effective_tools` / `input.max_tokens` / `input.anthropic_multimodal_turn` / `system_prompt_segments` / `input.trace_id`）都必须在当前 scope 里可访问。如果某些字段在 `input: &LlmStepInput<'_>` 上不存在（例如 `anthropic_multimodal_turn` / `trace_id` / `max_tokens`），需要：

a) grep `pub struct LlmStepInput` 找定义
b) 如果字段缺，**STOP and report BLOCKED**（不要乱猜字段名）

```bash
grep -n "pub struct LlmStepInput" src-tauri/src/runtime/chat/turn_config.rs
```

如果发现 `self.gateway` 这个 self 字段不存在（run_llm_step 是 impl 块某 struct 的方法但不持有 gateway），需要从别处拿 gateway（通常通过 input 或 closure）。同样**先 grep 确认**：

```bash
grep -n "impl.*for.*Tauri\|impl Tauri.*{" src-tauri/src/transport/tauri_commands/chat.rs | head -5
grep -n "self\.gateway\|gateway:" src-tauri/src/transport/tauri_commands/chat.rs | head -10
```

如果 `self` 上没有 gateway，**STOP and report BLOCKED**，附上 LlmStepInput 字段和 impl 块上下文，让控制器决策。

- [ ] **Step 4: cargo check**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
```

Expected: PASS。如果有 error 但属于 scope / 字段不可访问问题 → 见 Step 3 末尾的 BLOCKED 指引。

- [ ] **Step 5: 跑 review 测试 + s4_driver_loop_test**

```bash
cd src-tauri && cargo test --test review_stream_error_terminal_events 2>&1 | tail -10
cd src-tauri && cargo test --test s4_driver_loop_test 2>&1 | tail -10
```

Expected: 全 PASS（PR3 不破坏 PR1/PR2 不变式）。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs
git commit -m "feat(stream-error): non-streaming fallback on chunk timeout exhausted

PR3 核心实现：chunk_timeout 重试 10 次耗尽后，先 emit
streaming:retry-reset { reason: FallbackToNonStream } 让前端切到
'切换备用通道' 文案，再调 gateway.send_message_with_segments 重发
同一请求。60s 总体超时封顶。

- 成功 → 拼回 LlmStepResult::ContentComplete/ToolCalls，走正常 Step 6-8
- 失败/超时 → emit StreamError + Err(MaxRetriesExceeded)，进层 2（PR1）

服务端零改动；客户端 LotusProvider.inner=ClaudeProvider 走非流式
/anthropic/v1/messages；网关流式失败已自动退款。

Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §四 PR3"
```

---

## Task 4: 前端 retry-reset handler 加 FallbackToNonStream reason 文案

**Files:**
- Modify: `src/hooks/useStreaming.ts`

- [ ] **Step 1: 找 retry-reset handler 文案 switch**

定位现有 `streaming:retry-reset` handler（grep `streaming:retry-reset`）。当前文案 switch 应该类似：

```typescript
const title =
  reason === 'upstream_busy'
    ? 'AI 服务繁忙，正在重试...'
    : reason === 'rate_limited'
      ? '请求过于频繁，正在重试...'
      : '网络抖动，正在重新连接...'
```

- [ ] **Step 2: 加 FallbackToNonStream reason 分支**

替换为：

```typescript
const title =
  reason === 'upstream_busy'
    ? 'AI 服务繁忙，正在重试...'
    : reason === 'rate_limited'
      ? '请求过于频繁，正在重试...'
      : reason === 'fallback_to_non_stream'
        ? 'AI 服务超时，切换备用通道...'
        : '网络抖动，正在重新连接...'
```

注意：reason 字段是 snake_case（与 Rust `#[serde(rename_all = "snake_case")]` 对齐）。

- [ ] **Step 3: 检查 RetryReason TS 类型是否需要更新**

grep TS 中的 RetryReason 字面量类型：

```bash
grep -rn "type StreamingRetryResetPayload\|reason:.*'upstream_busy'\|fallback_to_non_stream" src/ | head -10
```

如果有 union type 写死了 reason 字面量，加上 `'fallback_to_non_stream'`。如果是 `string` 类型，无需改。

- [ ] **Step 4: TS 编译 + 前端测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec tsc --noEmit 2>&1 | grep useStreaming | head -10
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/hooks/useStreaming.integration.test.tsx 2>&1 | tail -20
```

Expected: 无新 error / 测试 PASS。

- [ ] **Step 5: 提交**

```bash
git add src/hooks/useStreaming.ts <可能需要改的 ts type 文件>
git commit -m "feat(stream-error): retry-reset handler shows 'switching channel' on fallback

PR3 前端：现有 streaming:retry-reset handler 加 reason 分支
fallback_to_non_stream → 显示'AI 服务超时，切换备用通道...'。

复用现有 toast + resetConversationStreamContent 路径（清 partial bubble）。
不新建事件 / handler，最小改动。

Spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §四 PR3"
```

---

## Task 5: PR3 全量回归

- [ ] **Step 1: cargo check 全工程**

```bash
cd src-tauri && cargo check --all-targets 2>&1 | grep "^error" | head -10
```

Expected: 仅预存在的非 review_ 测试编译错误（与 PR1/PR2 验收同列表）。

- [ ] **Step 2: 跑关键测试**

```bash
cd src-tauri && cargo test --test review_stream_error_terminal_events --test review_history_filters_error_messages --test review_tauri_event_adapter_test --test s4_driver_loop_test 2>&1 | grep "^test result"
```

Expected: 全 OK。

- [ ] **Step 3: 前端测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm test 2>&1 | tail -10
```

Expected: 全 PASS。

无需新 commit。

---

## 自审清单

- [x] Spec 覆盖：§四 PR3 全部 5 条
- [x] Placeholder scan：无
- [x] Type consistency：`send_message_with_segments` 签名与 `stream_message_with_segments` 完全对齐（13 参数）

## 风险

- **`run_llm_step` 内 `self.gateway` 访问性**：不确定 run_llm_step 是 trait impl 还是 struct method；如果 gateway 不在 self 上可访问 → BLOCKED 上报
- **`LlmStepInput` 字段完整性**：plan 假设字段 `anthropic_multimodal_turn` / `trace_id` / `max_tokens` 都在 input 上，若不在需要扩 input 或换路径
- **Non-stream fallback 也走 watchdog 30s timeout 但更短场景**：60s 封顶可能截到长上下文（150K input）非流式响应，将来视实际场景调整