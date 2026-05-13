# 子 agent 空响应韧性 · max_tokens 续写 + transcript audit

**日期**:2026-05-13
**范围**:`src-tauri/src/runtime/agent/` 三个文件
**类型**:bugfix(韧性 + 可观测性)+ helper 抽取
**估算**:~120 行生产代码 + ~200 行测试

---

## 1. 背景

### 1.1 现象

父 agent 调用 Agent tool 派活,子 agent 跑了 92 秒后父 agent 拿到 `"Tool ran without output or errors"`。事后查 transcript 只有原始 user prompt 一条记录,看不出 LLM 是否被调用过。

### 1.2 Root cause

应用日志的决定性证据:

```
[2026-05-13][10:01:32][INFO] [SubAgent] iter=0 content_len=0 tool_calls=0 stop=MaxTokens
```

Anthropic 上游对 `claude-sonnet-4-5` + `max_tokens=64000` 返回 `stop_reason="max_tokens"` 且 `content=[]`(大概率推理 token 把 64k 输出预算烧光,可见 token 一个没出)。

排查交叉验证:
- lotus gateway 透传不改请求/响应(`anthropic_native.go:96-98` 只改 model 字段)
- 64000 已是 `claude-sonnet-4-5` 协议天花板,传更大会被 Bedrock reject
- 我们这边 `thinking` 字段没设(`thinkingType="" → ThinkingConfig::Disabled`),仍命中 thinking 行为属于上游/网关侧 reasoning 注入

**所以 root cause 在上游模型行为,桌面端代码改不到。** 但桌面端有 3 个放大伤害的 bug 让这次"哑跑"完全不可观测,而且没有任何韧性恢复机制。

### 1.3 桌面端的 3 个 bug

**Bug 1 · 没有 max_tokens 续写恢复**
LLM 返回 `stop=MaxTokens + content="" + tool_calls=[]` 时,子 agent 直接放弃,父 agent 拿到空字符串。这种"哑跑"在很多场景下其实只要给模型一个 hint 让它"绕过推理直接写内容"就能成功——但当前完全没这个机制。

**Bug 2 · output 兜底缺一支**
`worker_runtime.rs:619-628` 只覆盖 iter-limit 和 cancelled 两种 break,没覆盖"LLM 正常 break 但 content/tool_calls 都空"的第三种 → `envelope.output=""` → 父 agent 完全没线索。

**Bug 3 · transcript audit 丢失**
`worker_runtime.rs:386-394` 的 `if !iter_content.is_empty()` 守卫让空 content 的 assistant turn 不进 `request.messages`;而 transcript 落盘(line 635-644)直接遍历 `request.messages` → 事后看 transcript 只有最初 user 一条,误以为 LLM 根本没被调过。

**Bug 4(pre-existing,顺手修)**
`StreamEvent::Error` 分支(line 363-369)设了 `output = "Sub-agent stream error: ..."` 后只 `break` 内层 while,外层 line 387 立即用空的 `iter_content` 覆盖回去 → stream 错误信息永远丢失。

---

## 2. 设计哲学

参考 claude-code-best `src/query.ts:1188-1259` 的三段式 max_output_tokens 处理(escalate / recover / surface),**适配子 agent RPC 场景**:

- 把"哑跑"当一等公民:不是异常通道,而是数据流上的有意义状态
- 错误信息结构化,可被上层(父 agent LLM)程序性消费
- transcript 始终反映"LLM 真的被调过 + 用了几次 recovery"
- **韧性问题归 worker_runtime,不向上泄露给任务编排层**——recovery 是 RPC client 的标配能力,跟 HTTP/gRPC client 内置 retry 同理

### 2.1 与 claude-code-best 的对照

| 阶段 | claude-code-best | 我们的适配 |
|---|---|---|
| Escalate(8k → 64k) | 有(主对话 capped default 8k) | **不做**——我们子 agent 起步就是 64k |
| Recover(注入 hint 续写) | `isMeta:true` user message,最多 3 次 | 注入普通 user message(没有 isMeta 概念),最多 **2 次** |
| Surface(兜底文案给上层) | yield 错误 message | envelope.output 兜底文案 + 结构化字段 |

**为什么 N=2 不是 3**:
- claude-code-best 是长对话流,用户在等模型继续说,3 次合理
- 我们是子 agent 一次性 RPC,92s × 3 = 4.6 分钟用户已经懵了
- 2 次平衡:第 1 次 hint 让模型避开推理陷阱,第 2 次允许它再调整一次

### 2.2 helper 抽取的必要性

父 agent 主对话循环(`chat_turn_driver.rs`)和子 agent 循环(`worker_runtime.rs`)是**两份独立代码但消费同一个 LLM stream API**。max_tokens 哑跑这种上游级别的问题**理论上两边都会遇到**,父 agent 那边只是当前没人报告。

所以 recovery 逻辑必须抽成可复用 helper。**本次 PR 只在子 agent 接入**,父 agent 接入留下个 PR——等子 agent 上线 1-2 周观察 helper 设计无问题后再无风险复用。

---

## 3. 文件改动清单

| 文件 | 改动类型 | 说明 |
|---|---|---|
| `runtime/agent/empty_response_recovery.rs` | 新建 | recovery 决策 helper(独立 module,完整单测) |
| `runtime/agent/mod.rs` | 加 `pub mod` | 导出 helper |
| `runtime/agent/subagent_result_envelope.rs` | 加 2 字段 | `terminal_stop_reason` + `max_tokens_recovery_attempts` |
| `runtime/agent/worker_runtime.rs` | 5 处改动 | 接入 helper、修 stream error bug、空 turn audit |
| `tests/empty_response_recovery_test.rs` | 新建 | helper 行为单测(7 条 state machine 测试) |
| `tests/review_subagent_empty_response_handling.rs` | 新建 | 子 agent 接入的 review-style 断言(4 条) |

---

## 4. Helper · `runtime/agent/empty_response_recovery.rs`

### 4.1 文案语言策略

- **RECOVERY_HINT**(发给 LLM 的续写提示):**中文**——跟 `base_prompt.rs::DAILY_BASE_PROMPT` 一致(已确认基线 prompt 中文化),模型不需要做语种切换
- **Surface fallback_output**(给父 agent LLM 看 + 写日志):**中文**——同上,父 agent LLM 也是中文环境
- **transcript 占位 `[empty turn: stop_reason=MaxTokens]`**:**英文**——debug 元信息,字段名对应 Anthropic API,grep 友好
- **`info!` 日志 `[SubAgent] empty-response recovery attempt N`**:**英文**——日志统一英文

### 4.2 完整代码

```rust
use crate::llm::streaming::StopReason;

const DEFAULT_MAX_ATTEMPTS: u32 = 2;

const RECOVERY_HINT: &str =
    "上一轮被 max_tokens 截断且没有产生任何可见输出——很可能被推理过程消耗光了。\
     请直接开始写实际内容,跳过计划性的叙述。\
     如果任务输出较长,请拆成小块,优先调用工具(如 write_file)分次写入,\
     不要试图在一次回复里返回一段超长文本。";

pub struct EmptyResponseRecoveryConfig {
    pub max_attempts: u32,
}

impl Default for EmptyResponseRecoveryConfig {
    fn default() -> Self {
        Self { max_attempts: DEFAULT_MAX_ATTEMPTS }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryDecision {
    /// quota 充足,注入 hint 再调一次 LLM
    Retry { hint_message: &'static str },
    /// quota 用完或非可恢复 stop_reason,生成兜底文案给上层
    Surface { fallback_output: String },
    /// 不该 recovery(其他正常路径,例如有 content 或 tool_calls)
    NoRecovery,
}

pub struct EmptyResponseRecoveryState {
    attempts_used: u32,
    config: EmptyResponseRecoveryConfig,
}

impl EmptyResponseRecoveryState {
    pub fn new(config: EmptyResponseRecoveryConfig) -> Self {
        Self { attempts_used: 0, config }
    }

    pub fn attempts_used(&self) -> u32 { self.attempts_used }

    pub fn decide(
        &mut self,
        stop_reason: StopReason,
        had_content: bool,
        had_tool_calls: bool,
        max_tokens: u32,
        iterations_used: u32,
    ) -> RecoveryDecision {
        // 正常产出 → 不需要 recovery
        if had_content || had_tool_calls {
            return RecoveryDecision::NoRecovery;
        }

        match stop_reason {
            StopReason::MaxTokens => {
                if self.attempts_used < self.config.max_attempts {
                    self.attempts_used += 1;
                    RecoveryDecision::Retry { hint_message: RECOVERY_HINT }
                } else {
                    RecoveryDecision::Surface {
                        fallback_output: format!(
                            "子代理在 {} 次内部重试后,仍以 stop_reason=max_tokens 结束且没有任何文本/工具调用 \
                             (iterations={}, max_tokens={})。上游持续把输出预算消耗在不可见内容上 \
                             (通常是推理 token),建议把任务拆成更小的子任务,或用更紧凑的 prompt 重新派发。",
                            self.attempts_used, iterations_used, max_tokens,
                        ),
                    }
                }
            }
            StopReason::ContentFilter => RecoveryDecision::Surface {
                fallback_output: format!(
                    "子代理被上游内容过滤拦截 (iterations={}),没有产生输出。\
                     请检查 prompt 是否包含可能触发过滤的内容。",
                    iterations_used,
                ),
            },
            other => RecoveryDecision::Surface {
                fallback_output: format!(
                    "子代理结束但没有产生任何输出 (iterations={}, stop_reason={:?})。\
                     LLM 在没有写文本也没有调用工具的情况下结束了本轮。",
                    iterations_used, other,
                ),
            },
        }
    }
}
```

**关键设计**:
- 纯 state machine,**不依赖** worker_runtime / chat_turn_driver / messages / gateway → 任何调用方都能复用
- `decide()` 是唯一入口,返回明确的 3 态枚举,调用方按 match 写 push/continue/break 即可
- `attempts_used()` 给 envelope 字段填充用
- 只有 MaxTokens 触发 Retry,其他 stop_reason(ContentFilter / EndTurn 空响应)直接 Surface(因为续写救不了它们)

---

## 5. Envelope · `runtime/agent/subagent_result_envelope.rs`

**纯 additive 演进**:加两个可选字段,**不 bump** `schema_version`(保持 1),前后兼容。

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentResultEnvelope {
    pub schema_version: u32,
    pub output: String,
    pub iterations_used: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminal_tool_results: Vec<SubAgentTerminalToolResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transcript_snapshot: Vec<SubAgentTranscriptEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_ref: Option<String>,
    /// Anthropic stop_reason snake_case ("max_tokens" / "end_turn" / "tool_use" /
    /// "stop_sequence" / "content_filter"). 仅当 LLM 自然结束时设置。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_stop_reason: Option<String>,
    /// max_tokens 哑跑触发的内部续写次数,0 表示从未 recovery。
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub max_tokens_recovery_attempts: u32,
}

fn is_zero_u32(v: &u32) -> bool { *v == 0 }
```

`to_storage_summary` 的 fallback 字符串里 `"schemaVersion":1` 保持不变。

---

## 6. Worker runtime · `worker_runtime.rs`

### 6.1 改动 A · loop 前提取常量(line 280-285 区段)

```rust
let mut output = String::new();
let mut files: Vec<String> = Vec::new();
let mut iterations_used = 0;
let mut pending_ask: Option<PermissionDecision> = None;
let mut terminal_tool_results: Vec<SubAgentTerminalToolResult> = Vec::new();
let mut cancelled = false;
// 新增:
let mut last_stop_reason: Option<StopReason> = None;
let mut recovery = EmptyResponseRecoveryState::new(
    EmptyResponseRecoveryConfig::default(),
);
let max_tokens = crate::llm::max_tokens::default_max_tokens_for_model(
    &effective_settings.primary_model,
);
```

原 line 301-302 loop 内的 `let max_tokens` 删除(loop 内引用不变)。

### 6.2 改动 B · StreamEvent::Error 跳出外层(line 363-369)

```rust
StreamEvent::Error { error } => {
    warn!("[SubAgent] Stream error: {}", error);
    if output.is_empty() {
        output = format!("Sub-agent stream error: {}", error);
    }
    break 'agent_loop;   // 修 Bug 4:从 break 改成 break 'agent_loop
}
```

### 6.3 改动 C · 空/非 ToolUse 路径替换(line 386-394)

```rust
if stop_reason != StopReason::ToolUse || tool_calls.is_empty() {
    last_stop_reason = Some(stop_reason);
    let had_content = !iter_content.is_empty();
    let had_tools = !tool_calls.is_empty();

    // audit:即便空 content 也要 push,留 transcript 痕迹。空时用占位文案,
    // 防止未来 retry / compaction 拿这个 messages 续写时被
    // Anthropic API "text block cannot be empty" reject
    let assistant_text = if had_content {
        iter_content.clone()
    } else {
        format!("[empty turn: stop_reason={:?}]", stop_reason)
    };
    request.messages.push(ChatMessage::text("assistant", assistant_text));

    match recovery.decide(
        stop_reason,
        had_content,
        had_tools,
        max_tokens,
        iterations_used as u32,
    ) {
        RecoveryDecision::Retry { hint_message } => {
            info!(
                "[SubAgent] empty-response recovery attempt {} (stop={:?})",
                recovery.attempts_used(),
                stop_reason
            );
            request
                .messages
                .push(ChatMessage::text("user", hint_message.to_string()));
            continue 'agent_loop;
        }
        RecoveryDecision::Surface { fallback_output } => {
            output = fallback_output;
            break;
        }
        RecoveryDecision::NoRecovery => {
            // 有 content 但 stop != ToolUse,正常结束
            output = iter_content;
            break;
        }
    }
}
```

### 6.4 改动 D · loop 外兜底保持不变(line 619-628)

- iter-limit:仍用 `"Sub-agent reached iteration limit."`
- cancelled:仍用 `"Sub-agent cancelled."`
- 空 + 非以上:**不会进入这里**——helper 在 break 前已经填好 output

### 6.5 改动 E · envelope 构造(line 662-670)

```rust
let envelope = SubAgentResultEnvelope {
    schema_version: 1,
    output: output.clone(),
    iterations_used,
    generated_files: generated_files.clone(),
    terminal_tool_results,
    transcript_snapshot,
    transcript_ref: Some(transcript_ref.clone()),
    // 用 serde 的 snake_case rename 而非 Debug derive,保证字符串稳定
    terminal_stop_reason: last_stop_reason.as_ref().and_then(|r| {
        serde_json::to_value(r).ok().and_then(|v| v.as_str().map(String::from))
    }),
    max_tokens_recovery_attempts: recovery.attempts_used(),
};
```

---

## 7. 测试方案

`LlmGateway` 是 concrete struct 不是 trait,无法直接 mock 注入做端到端 behavioral test。所以采用:
- **helper state machine 行为单测**(7 条,覆盖所有 decide 分支)
- **review-style 源码断言**(本仓库 tests/review_*.rs 惯例,验证接入正确)
- **envelope 行为单测**(JSON 往返、向后兼容)

### 7.1 Helper 行为单测 · `tests/empty_response_recovery_test.rs`

```rust
use app_lib::llm::streaming::StopReason;
use app_lib::runtime::agent::empty_response_recovery::{
    EmptyResponseRecoveryConfig, EmptyResponseRecoveryState, RecoveryDecision,
};

#[test]
fn no_recovery_when_content_present() {
    let mut s = EmptyResponseRecoveryState::new(Default::default());
    let d = s.decide(StopReason::EndTurn, true, false, 64000, 1);
    assert!(matches!(d, RecoveryDecision::NoRecovery));
    assert_eq!(s.attempts_used(), 0);
}

#[test]
fn no_recovery_when_tool_calls_present() {
    let mut s = EmptyResponseRecoveryState::new(Default::default());
    let d = s.decide(StopReason::ToolUse, false, true, 64000, 1);
    assert!(matches!(d, RecoveryDecision::NoRecovery));
}

#[test]
fn max_tokens_first_two_attempts_retry() {
    let mut s = EmptyResponseRecoveryState::new(Default::default());
    let d1 = s.decide(StopReason::MaxTokens, false, false, 64000, 1);
    assert!(matches!(d1, RecoveryDecision::Retry { .. }));
    assert_eq!(s.attempts_used(), 1);
    let d2 = s.decide(StopReason::MaxTokens, false, false, 64000, 2);
    assert!(matches!(d2, RecoveryDecision::Retry { .. }));
    assert_eq!(s.attempts_used(), 2);
}

#[test]
fn max_tokens_third_attempt_surfaces() {
    let mut s = EmptyResponseRecoveryState::new(Default::default());
    s.decide(StopReason::MaxTokens, false, false, 64000, 1);
    s.decide(StopReason::MaxTokens, false, false, 64000, 2);
    let d3 = s.decide(StopReason::MaxTokens, false, false, 64000, 3);
    match d3 {
        RecoveryDecision::Surface { fallback_output } => {
            assert!(fallback_output.contains("max_tokens"));
            assert!(fallback_output.contains("内部重试"));
        }
        _ => panic!("expected Surface after exhausted attempts"),
    }
    assert_eq!(s.attempts_used(), 2);  // counter 不该再 +1
}

#[test]
fn content_filter_surfaces_immediately_no_retry() {
    let mut s = EmptyResponseRecoveryState::new(Default::default());
    let d = s.decide(StopReason::ContentFilter, false, false, 64000, 1);
    match d {
        RecoveryDecision::Surface { fallback_output } => {
            assert!(fallback_output.contains("内容过滤"));
        }
        _ => panic!("ContentFilter must Surface immediately"),
    }
    assert_eq!(s.attempts_used(), 0);
}

#[test]
fn end_turn_with_empty_surfaces_immediately() {
    let mut s = EmptyResponseRecoveryState::new(Default::default());
    let d = s.decide(StopReason::EndTurn, false, false, 64000, 1);
    match d {
        RecoveryDecision::Surface { fallback_output } => {
            assert!(fallback_output.contains("没有产生任何输出"));
            assert!(fallback_output.contains("EndTurn"));
        }
        _ => panic!("EndTurn empty must Surface immediately"),
    }
}

#[test]
fn custom_max_attempts_respected() {
    let mut s = EmptyResponseRecoveryState::new(EmptyResponseRecoveryConfig {
        max_attempts: 1,
    });
    let d1 = s.decide(StopReason::MaxTokens, false, false, 64000, 1);
    assert!(matches!(d1, RecoveryDecision::Retry { .. }));
    let d2 = s.decide(StopReason::MaxTokens, false, false, 64000, 2);
    assert!(matches!(d2, RecoveryDecision::Surface { .. }));
}
```

### 7.2 子 agent 接入断言 · `tests/review_subagent_empty_response_handling.rs`

```rust
use std::fs;

#[test]
fn worker_runtime_uses_empty_response_recovery_helper() {
    let src = fs::read_to_string("src/runtime/agent/worker_runtime.rs").unwrap();
    assert!(
        src.contains("EmptyResponseRecoveryState::new"),
        "worker_runtime must instantiate EmptyResponseRecoveryState"
    );
    assert!(
        src.contains("recovery.decide("),
        "worker_runtime must call recovery.decide(...)"
    );
    assert!(
        src.contains("RecoveryDecision::Retry"),
        "worker_runtime must handle Retry branch"
    );
    assert!(
        src.contains("RecoveryDecision::Surface"),
        "worker_runtime must handle Surface branch"
    );
    assert!(
        src.contains("continue 'agent_loop"),
        "Retry branch must continue outer loop, not break"
    );
}

#[test]
fn worker_runtime_pushes_audit_assistant_turn_for_empty_content() {
    let src = fs::read_to_string("src/runtime/agent/worker_runtime.rs").unwrap();
    assert!(
        src.contains("[empty turn: stop_reason="),
        "empty-turn placeholder must be present"
    );
    assert!(
        !src.contains("if !iter_content.is_empty() {\n                    request"),
        "old guard `if !iter_content.is_empty()` must be removed"
    );
}

#[test]
fn worker_runtime_stream_error_breaks_outer_loop() {
    let src = fs::read_to_string("src/runtime/agent/worker_runtime.rs").unwrap();
    let idx = src
        .find("StreamEvent::Error { error }")
        .expect("stream error branch must exist");
    let tail = &src[idx..(idx + 400).min(src.len())];
    assert!(
        tail.contains("break 'agent_loop"),
        "stream error must break outer agent_loop to avoid output overwrite"
    );
}

#[test]
fn envelope_has_recovery_audit_fields() {
    let src = fs::read_to_string("src/runtime/agent/subagent_result_envelope.rs").unwrap();
    assert!(
        src.contains("pub terminal_stop_reason: Option<String>"),
        "envelope must expose terminal_stop_reason"
    );
    assert!(
        src.contains("pub max_tokens_recovery_attempts: u32"),
        "envelope must expose max_tokens_recovery_attempts"
    );
    let field_idx = src.find("pub terminal_stop_reason").unwrap();
    let preceding = &src[field_idx.saturating_sub(120)..field_idx];
    assert!(
        preceding.contains("#[serde(default"),
        "terminal_stop_reason must have #[serde(default)] for backward compat"
    );
}
```

### 7.3 Envelope 行为单测(加到 `subagent_result_envelope.rs` 末尾)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_envelope_json_deserializes_with_defaults() {
        let old_json = r#"{
            "schemaVersion": 1,
            "output": "hello",
            "iterationsUsed": 3,
            "transcriptRef": "subagent://abc"
        }"#;
        let env: SubAgentResultEnvelope =
            serde_json::from_str(old_json).expect("parse old json");
        assert_eq!(env.output, "hello");
        assert_eq!(env.iterations_used, 3);
        assert_eq!(env.terminal_stop_reason, None);
        assert_eq!(env.max_tokens_recovery_attempts, 0);
    }

    #[test]
    fn new_envelope_roundtrip_preserves_recovery_fields() {
        let env = SubAgentResultEnvelope {
            schema_version: 1,
            output: "ok".to_string(),
            iterations_used: 1,
            generated_files: vec![],
            terminal_tool_results: vec![],
            transcript_snapshot: vec![],
            transcript_ref: None,
            terminal_stop_reason: Some("max_tokens".to_string()),
            max_tokens_recovery_attempts: 2,
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"terminalStopReason\":\"max_tokens\""));
        assert!(json.contains("\"maxTokensRecoveryAttempts\":2"));
        let back: SubAgentResultEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back.terminal_stop_reason, Some("max_tokens".to_string()));
        assert_eq!(back.max_tokens_recovery_attempts, 2);
    }

    #[test]
    fn zero_recovery_attempts_skipped_in_serialization() {
        // counter=0 时应该 skip,保持老 JSON shape
        let env = SubAgentResultEnvelope {
            schema_version: 1,
            output: "ok".to_string(),
            iterations_used: 1,
            generated_files: vec![],
            terminal_tool_results: vec![],
            transcript_snapshot: vec![],
            transcript_ref: None,
            terminal_stop_reason: None,
            max_tokens_recovery_attempts: 0,
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(!json.contains("maxTokensRecoveryAttempts"));
        assert!(!json.contains("terminalStopReason"));
    }

    #[test]
    fn storage_summary_accepts_legacy_payload() {
        let legacy = format!(
            "{ENVELOPE_PREFIX}{}",
            r#"{"schemaVersion":1,"output":"x","iterationsUsed":0}"#
        );
        let env =
            SubAgentResultEnvelope::from_storage_summary(&legacy).expect("parse legacy");
        assert_eq!(env.terminal_stop_reason, None);
        assert_eq!(env.max_tokens_recovery_attempts, 0);
    }
}
```

### 7.4 回归命令

```bash
cd src-tauri

# 新测试
cargo test --test empty_response_recovery_test
cargo test --test review_subagent_empty_response_handling
cargo test --lib runtime::agent::subagent_result_envelope

# 全量 review 回归(约束没破)
cargo test review_ --tests --no-fail-fast

# lib 单测
cargo test --lib
```

---

## 8. 实施顺序(TDD)

| 步骤 | 动作 | 期望状态 |
|---|---|---|
| 1 | 新建 `tests/empty_response_recovery_test.rs`(7 条) | 编译过、7 条全 RED |
| 2 | 新建 `runtime/agent/empty_response_recovery.rs` + `mod.rs` 导出 | step 1 全绿 |
| 3 | 新建 `tests/review_subagent_empty_response_handling.rs`(4 条) | 4 条全 RED |
| 4 | 在 `subagent_result_envelope.rs` 末尾加 `mod tests`(4 条单测)+ 加 2 个字段 + helper fn | envelope 单测全绿;step 3 仍 1 红 |
| 5 | 改 `worker_runtime.rs`:改动 A(loop 前变量) | step 3 减 1 红 |
| 6 | 改 `worker_runtime.rs`:改动 B(stream error `break 'agent_loop`) | step 3 减 1 红 |
| 7 | 改 `worker_runtime.rs`:改动 C(去守卫 + 占位 push + match recovery) | step 3 减 1 红 |
| 8 | 改 `worker_runtime.rs`:改动 E(envelope 字段填充) | step 3 全绿 |
| 9 | 跑 `cargo test review_ --tests --no-fail-fast` | 全量 review 回归绿 |
| 10 | 跑 `cargo test --lib` | lib 单测绿 |

---

## 9. 风险与缓解

| 风险 | 缓解 |
|---|---|
| 续写第二轮仍被推理 token 烧光 → 浪费 2 × 90s | `max_attempts=2` 硬上限,上线后观察命中率,< 20% 就降到 1 |
| 续写期间用户感知耗时变长(最坏 3 × 90s) | 接入 diagnostics 日志(下个 PR),让 UI 至少能给"重试中"提示;本次 PR 仅 `info!` 日志 |
| 空 assistant turn 被未来 retry / compaction 发回 Anthropic 被 reject | 用 `[empty turn: stop_reason=...]` 占位文案而非空串 |
| `last_stop_reason` 在 LLM-error / stream-error 路径上仍为 None,兜底分支可能用错文案 | LLM-error 路径在 line 335 已设 `output`;stream-error 改成 `break 'agent_loop` 后也已设 `output`——两路都不会进入新 helper 兜底分支(`output.is_empty()` 不成立) |
| 老会话 JSON 反序列化 | 字段 `#[serde(default)]` + `is_zero_u32`,单测 7.3 覆盖 |
| schema_version 冲突(`chat.rs:1011/1066` 也有 `schema_version: Some(2)`) | 那是 `StoredMessage.schema_version`,与 envelope 是不同 namespace,不冲突;envelope 不 bump,保持 1 |
| `terminal_stop_reason` 字符串格式漂移 | 用 serde 的 `snake_case` rename 而非 `Debug`,与 Anthropic 协议字段名一致 |
| `safe_truncate("[empty turn: ...]", 800)` panic | 占位文案 < 800 字节,空串/短串走 fast path return |
| recovery 注入的 user message 进 transcript 增加噪音 | 这是预期效果——transcript 反映真实对话流(包含 worker 内部 retry),否则 debug 无法还原 |

---

## 10. 不在本次范围

- **父 agent(`chat_turn_driver.rs`)接入 helper** —— 留下个 PR,等子 agent 上线 1-2 周观察 helper 设计无问题再无风险复用
- **diagnostics 日志事件接入**(参照 `docs/harness/diagnostics-log-debugging-guide.md`)—— 本次只用 `info!`,下个 PR 子父一起接入
- **lotus gateway 排查为什么 64k 预算被推理 token 烧光** —— gateway 仓库的事
- **`max_tokens` 数值调整** —— 64k 已是协议天花板

---

## 11. 验证 helper 没问题的标准(决定何时可以推父 agent)

- `cargo test --test empty_response_recovery_test` 7/7 绿
- `cargo test --test review_subagent_empty_response_handling` 4/4 绿
- 子 agent 上线 1-2 周内真实 case 触发 recovery 至少 1 次,日志能看到 `[SubAgent] empty-response recovery attempt N` + envelope `max_tokens_recovery_attempts > 0`
- recovery 成功率(完整流程后 `output` 非兜底文案的比例)≥ 40%,否则考虑调整 hint 文案或降 `max_attempts`
- 父 agent 观察期内**没有**独立的 max_tokens 哑跑报告(说明子 agent 的接入方式覆盖了主要场景)

---

## 12. 影响面

- 后端改动:`worker_runtime.rs`(+~30 行) + `subagent_result_envelope.rs`(+10 行字段 +~60 行 unit test) + `empty_response_recovery.rs`(新 +~80 行) + `mod.rs`(+1 行)
- 新增测试文件:`tests/empty_response_recovery_test.rs`(~120 行) + `tests/review_subagent_empty_response_handling.rs`(~70 行)
- envelope JSON shape 向后兼容(纯 additive 字段,counter=0 时 skip 序列化)
- 不涉及 LLM 协议、网关、prompt 体系、存储路径
- 客户端单体应用,无前端展示需求(`terminal_stop_reason` 与 `max_tokens_recovery_attempts` 只供父 agent LLM 消费 + transcript audit + 日志)

---

## 13. 修复前后效果对比

### 场景:实际遇到的 case(推理 token 烧光 64k,续写后成功)

**修之前**:父 agent 拿到 `"Tool ran without output or errors"`,transcript 只有 user 一条 → 父 agent 摸黑,事后 debug 误判方向。

**修之后**:
- 子 agent 内部:第 1 次 `stop=MaxTokens + content=""` → 注入中文 hint → 第 2 次 LLM 调用 → 模型避开推理直接调 `write_file` 写第一块 → 后续正常完成
- 父 agent 看到:正常的 `output` + envelope `max_tokens_recovery_attempts=1, terminal_stop_reason=end_turn` 知道这次内部用了 1 次续写但成功了
- transcript 留痕:`[user] 帮我生成 HTML` + `[assistant] [empty turn: stop_reason=MaxTokens]` + `[user] 上一轮被 max_tokens 截断...` + `[assistant] <实际内容>` —— 完整还原

### 场景:续写 2 次仍失败

**修之前**:同上,父 agent 完全没线索。

**修之后**:父 agent 拿到中文兜底文案 `"子代理在 2 次内部重试后,仍以 stop_reason=max_tokens 结束..."` + envelope `max_tokens_recovery_attempts=2, terminal_stop_reason=max_tokens`,LLM 看懂"哑跑 + 建议拆小"会主动拆任务重派,而不是机械重试。
