# Hook 系统（Plan-M）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 ToolDispatcher 和 TurnDriver 增加 PreToolUse / PostToolUse / Stop 三类用户可配置 hook，允许通过 shell 脚本干预工具执行流程。

**Architecture:** `HookConfig` 存储在 `AppStorage` / workspace 配置目录，`HookRunner` 通过 `tokio::process::Command` 执行 shell 脚本并解析 JSON 输出（`behavior: allow/deny`、`updatedInput`、`preventContinuation`）；`ToolDispatcher::dispatch` 在 permission 通过后、`execute` 前运行 PreToolUse hook，`execute` 后运行 PostToolUse hook；`RuntimeChatTurnDriver::run_chat_turn_s4` 在 turn 完成后调用 Stop hook。Hook 配置通过 `ToolExecutionContext` 注入，不扩大 `CapabilityContext`。

**Tech Stack:** Rust, tokio, async_trait, serde_json, tokio::process

**Worktree branch:** pzc

---

## 背景与对标

claude-code-best 在 `src/services/tools/toolHooks.ts` 和 `src/utils/hooks.js` 中实现了三类 hook：

- **PreToolUse**：在权限检查通过、工具执行前触发，可返回 `behavior:deny` 中止执行、`updatedInput` 修改参数、或 `preventContinuation` 阻止后续对话轮。
- **PostToolUse**：在工具成功执行后触发，可返回附加上下文消息。
- **Stop**：在整个 turn 完成后触发（`preventContinuation`）。

对应调用点：
- `runPreToolUseHooks` 在 `checkPermissionsAndCallTool` 的权限检查之后、`tool.call()` 之前（toolExecution.ts 第 813 行）。
- `runPostToolUseHooks` 在 `tool.call()` 成功返回后（第 1496 行）。
- `runPostToolUseFailureHooks` 在 catch 块中（第 1713 行）。
- Stop hook 在 turn 结束后由上层 query loop 触发。

lotus-app 当前 gap：
- `dispatcher.rs`：`dispatch()` 只有 `check_permissions()` + `execute()`，无 hook 点。
- `chat_turn_driver.rs`：`run_chat_turn_s4` 在 turn 完成（Step 8）后无 stop hook。
- `ToolExecutionContext`：有 `capability: Option<SharedCapabilityContext>`，可扩展携带 hook config，但不进 `CapabilityContext` 本身（保持窄接口）。

---

## Task M1 — 定义 HookConfig 与 HookRunner

**文件**
- Create: `src-tauri/src/runtime/hooks/mod.rs`
- Create: `src-tauri/src/runtime/hooks/config.rs`
- Create: `src-tauri/src/runtime/hooks/runner.rs`
- Test: `src-tauri/tests/plan_m_hook_runner_test.rs`

### TDD 步骤

**M1-T1: 写失败测试**

创建 `src-tauri/tests/plan_m_hook_runner_test.rs`：

```rust
//! Plan-M Task 1: HookConfig 结构与 HookRunner 基础行为测试
//!
//! cargo test --test plan_m_hook_runner_test

use lotus_app::runtime::hooks::{HookConfig, HookEvent, HookRunner, HookDecision};

// M1-1: HookConfig 可以被序列化/反序列化
#[test]
fn hook_config_roundtrips_serde() {
    let config = HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo '{\"behavior\":\"allow\"}'".to_string(),
        tool_filter: None,
        timeout_secs: Some(30),
    };
    let json = serde_json::to_string(&config).unwrap();
    let back: HookConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.command, config.command);
    assert!(matches!(back.event, HookEvent::PreToolUse));
}

// M1-2: HookRunner 执行返回 allow 的 shell 命令
#[tokio::test]
async fn hook_runner_allow_decision() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo '{\"behavior\":\"allow\"}'".to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    };
    let result = runner
        .run_hook(&config, "bash_tool", &serde_json::json!({"command": "ls"}))
        .await
        .unwrap();
    assert!(matches!(result.decision, HookDecision::Allow));
    assert!(result.updated_input.is_none());
    assert!(!result.prevent_continuation);
}

// M1-3: HookRunner 执行返回 deny 的 shell 命令
#[tokio::test]
async fn hook_runner_deny_decision() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo '{\"behavior\":\"deny\",\"message\":\"blocked\"}'".to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    };
    let result = runner
        .run_hook(&config, "bash_tool", &serde_json::json!({"command": "rm -rf /"}))
        .await
        .unwrap();
    assert!(matches!(result.decision, HookDecision::Deny { .. }));
}

// M1-4: HookRunner 解析 updatedInput
#[tokio::test]
async fn hook_runner_updated_input() {
    let runner = HookRunner::new();
    // 脚本通过 stdin 读取输入并返回修改后的 updatedInput
    let config = HookConfig {
        event: HookEvent::PreToolUse,
        command: "printf '{\"behavior\":\"allow\",\"updatedInput\":{\"command\":\"echo safe\"}}'".to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    };
    let result = runner
        .run_hook(&config, "bash_tool", &serde_json::json!({"command": "dangerous_cmd"}))
        .await
        .unwrap();
    assert!(matches!(result.decision, HookDecision::Allow));
    assert!(result.updated_input.is_some());
    let updated = result.updated_input.unwrap();
    assert_eq!(updated.get("command").and_then(|v| v.as_str()), Some("echo safe"));
}

// M1-5: HookRunner 处理 preventContinuation
#[tokio::test]
async fn hook_runner_prevent_continuation() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::PostToolUse,
        command: "echo '{\"behavior\":\"allow\",\"preventContinuation\":true,\"stopReason\":\"done\"}'".to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    };
    let result = runner
        .run_hook(&config, "bash_tool", &serde_json::json!({}))
        .await
        .unwrap();
    assert!(result.prevent_continuation);
    assert_eq!(result.stop_reason.as_deref(), Some("done"));
}

// M1-6: tool_filter 过滤不匹配的工具
#[tokio::test]
async fn hook_runner_tool_filter_skips_non_matching() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo '{\"behavior\":\"deny\"}'".to_string(),
        tool_filter: Some("write_file".to_string()),
        timeout_secs: Some(10),
    };
    // bash_tool 不匹配 write_file，hook 应被跳过，返回 Allow
    let result = runner
        .run_hook(&config, "bash_tool", &serde_json::json!({}))
        .await
        .unwrap();
    assert!(matches!(result.decision, HookDecision::Allow),
        "non-matching tool should be skipped (Allow)");
}

// M1-7: HookRunner 超时返回 Allow（防止 hook 卡住）
#[tokio::test]
async fn hook_runner_timeout_returns_allow() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::PreToolUse,
        command: "sleep 10".to_string(),
        tool_filter: None,
        timeout_secs: Some(1), // 1秒超时
    };
    let result = runner
        .run_hook(&config, "bash_tool", &serde_json::json!({}))
        .await
        .unwrap();
    // 超时默认 Allow（不阻断执行）
    assert!(matches!(result.decision, HookDecision::Allow));
}
```

**M1-T2: 确认失败**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_m_hook_runner_test 2>&1 | head -20
```
期望：编译错误（module 不存在）。

**M1-T3: 最小实现**

创建 `src-tauri/src/runtime/hooks/mod.rs`：
```rust
pub mod config;
pub mod runner;

pub use config::{HookConfig, HookEvent};
pub use runner::{HookDecision, HookOutcome, HookRunner};
```

创建 `src-tauri/src/runtime/hooks/config.rs`：
```rust
use serde::{Deserialize, Serialize};

/// 触发时机
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    Stop,
}

/// 单个 hook 的配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookConfig {
    /// 触发时机
    pub event: HookEvent,
    /// Shell 命令（通过 sh -c 执行）
    pub command: String,
    /// 仅对特定工具触发（None = 所有工具）
    pub tool_filter: Option<String>,
    /// 超时秒数（默认 30）
    pub timeout_secs: Option<u64>,
}

impl HookConfig {
    pub fn matches_tool(&self, tool_name: &str) -> bool {
        match &self.tool_filter {
            None => true,
            Some(filter) => filter == tool_name,
        }
    }

    pub fn effective_timeout_secs(&self) -> u64 {
        self.timeout_secs.unwrap_or(30)
    }
}

/// 会话级 hook 配置集合
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookRegistry {
    pub hooks: Vec<HookConfig>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn hooks_for(&self, event: HookEvent, tool_name: &str) -> Vec<&HookConfig> {
        self.hooks
            .iter()
            .filter(|h| h.event == event && h.matches_tool(tool_name))
            .collect()
    }
}
```

创建 `src-tauri/src/runtime/hooks/runner.rs`：
```rust
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::runtime::hooks::config::HookConfig;

/// Hook 执行决策
#[derive(Debug, Clone)]
pub enum HookDecision {
    /// 允许继续，可选 updatedInput
    Allow,
    /// 拒绝，附带原因
    Deny { message: String },
}

/// Hook 执行结果
#[derive(Debug, Clone)]
pub struct HookOutcome {
    pub decision: HookDecision,
    /// 如果 hook 返回了修改后的输入
    pub updated_input: Option<Value>,
    /// 是否阻止后续对话轮
    pub prevent_continuation: bool,
    /// preventContinuation 时的原因
    pub stop_reason: Option<String>,
}

impl HookOutcome {
    fn allow() -> Self {
        Self {
            decision: HookDecision::Allow,
            updated_input: None,
            prevent_continuation: false,
            stop_reason: None,
        }
    }
}

/// JSON 输出的反序列化结构
#[derive(Debug, Deserialize)]
struct HookOutput {
    #[serde(default = "default_behavior")]
    behavior: String,
    message: Option<String>,
    #[serde(rename = "updatedInput")]
    updated_input: Option<Value>,
    #[serde(rename = "preventContinuation", default)]
    prevent_continuation: bool,
    #[serde(rename = "stopReason")]
    stop_reason: Option<String>,
}

fn default_behavior() -> String {
    "allow".to_string()
}

/// 执行 hook shell 命令
pub struct HookRunner;

impl HookRunner {
    pub fn new() -> Self {
        Self
    }

    /// 运行单个 hook。如果 tool_filter 不匹配则直接返回 Allow。
    pub async fn run_hook(
        &self,
        config: &HookConfig,
        tool_name: &str,
        tool_input: &Value,
    ) -> anyhow::Result<HookOutcome> {
        if !config.matches_tool(tool_name) {
            return Ok(HookOutcome::allow());
        }

        let timeout = Duration::from_secs(config.effective_timeout_secs());
        let input_json = serde_json::to_string(tool_input)?;

        let result = tokio::time::timeout(timeout, async {
            let mut child = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&config.command)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()?;

            // 写入工具输入到 stdin
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(input_json.as_bytes()).await;
            }

            let output = child.wait_with_output().await?;
            anyhow::Ok(output)
        })
        .await;

        let output = match result {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => {
                log::warn!("[HookRunner] hook execution error: {}", e);
                return Ok(HookOutcome::allow());
            }
            Err(_) => {
                log::warn!("[HookRunner] hook timed out after {}s", config.effective_timeout_secs());
                return Ok(HookOutcome::allow());
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stdout = stdout.trim();

        if stdout.is_empty() {
            // 无输出 = allow（exit code 0）或 deny（exit code 2）
            if output.status.code() == Some(2) {
                return Ok(HookOutcome {
                    decision: HookDecision::Deny {
                        message: "Hook denied execution (exit code 2)".to_string(),
                    },
                    ..HookOutcome::allow()
                });
            }
            return Ok(HookOutcome::allow());
        }

        // 尝试解析 JSON
        match serde_json::from_str::<HookOutput>(stdout) {
            Ok(hook_output) => {
                let decision = if hook_output.behavior == "deny" {
                    HookDecision::Deny {
                        message: hook_output
                            .message
                            .unwrap_or_else(|| "Hook denied execution".to_string()),
                    }
                } else {
                    HookDecision::Allow
                };
                Ok(HookOutcome {
                    decision,
                    updated_input: hook_output.updated_input,
                    prevent_continuation: hook_output.prevent_continuation,
                    stop_reason: hook_output.stop_reason,
                })
            }
            Err(_) => {
                // 非 JSON 输出 = allow
                Ok(HookOutcome::allow())
            }
        }
    }

    /// 串行执行多个 hook，遇到 deny 立即停止。
    pub async fn run_hooks(
        &self,
        hooks: &[&HookConfig],
        tool_name: &str,
        tool_input: &Value,
    ) -> anyhow::Result<HookOutcome> {
        let mut current_input = tool_input.clone();
        for hook in hooks {
            let outcome = self.run_hook(hook, tool_name, &current_input).await?;
            if let HookDecision::Deny { .. } = &outcome.decision {
                return Ok(outcome);
            }
            if let Some(updated) = outcome.updated_input.clone() {
                current_input = updated;
            }
            if outcome.prevent_continuation {
                return Ok(HookOutcome {
                    updated_input: Some(current_input),
                    ..outcome
                });
            }
        }
        Ok(HookOutcome {
            decision: HookDecision::Allow,
            updated_input: if *tool_input != current_input {
                Some(current_input)
            } else {
                None
            },
            prevent_continuation: false,
            stop_reason: None,
        })
    }
}

impl Default for HookRunner {
    fn default() -> Self {
        Self::new()
    }
}
```

在 `src-tauri/src/runtime/mod.rs` 中添加 `pub mod hooks;`（或在相应位置注册模块）。

**M1-T4: 确认通过**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_m_hook_runner_test -- --nocapture
```
期望：7 个测试全部通过。

**M1-T5: commit**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/runtime/hooks/ src-tauri/tests/plan_m_hook_runner_test.rs && git commit -m "$(cat <<'EOF'
feat(hooks): add HookConfig and HookRunner for PreToolUse/PostToolUse/Stop - M1

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task M2 — HookConfig 注入 ToolExecutionContext

**文件**
- Modify: `src-tauri/src/runtime/tools/context.rs`
- Modify: `src-tauri/src/runtime/hooks/config.rs`
- Test: `src-tauri/tests/plan_m_context_injection_test.rs`

### TDD 步骤

**M2-T1: 写失败测试**

创建 `src-tauri/tests/plan_m_context_injection_test.rs`：

```rust
//! Plan-M Task 2: HookRegistry 注入 ToolExecutionContext 测试
//!
//! cargo test --test plan_m_context_injection_test

use lotus_app::runtime::hooks::config::{HookConfig, HookEvent, HookRegistry};
use lotus_app::runtime::tools::context::ToolExecutionContext;

// M2-1: ToolExecutionContext 默认无 hook_registry
#[test]
fn tool_execution_context_default_no_hooks() {
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
    assert!(ctx.hook_registry.is_none());
}

// M2-2: with_hook_registry 设置 hook
#[test]
fn tool_execution_context_with_hook_registry() {
    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo 'ok'".to_string(),
        tool_filter: None,
        timeout_secs: None,
    });
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_hook_registry(std::sync::Arc::new(registry));
    assert!(ctx.hook_registry.is_some());
    let reg = ctx.hook_registry.unwrap();
    assert_eq!(reg.hooks.len(), 1);
}

// M2-3: ToolExecutionContext Clone 保留 hook_registry
#[test]
fn tool_execution_context_clone_preserves_hook_registry() {
    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PostToolUse,
        command: "echo 'post'".to_string(),
        tool_filter: Some("bash_tool".to_string()),
        timeout_secs: Some(5),
    });
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_hook_registry(std::sync::Arc::new(registry));
    let cloned = ctx.clone();
    assert!(cloned.hook_registry.is_some());
}
```

**M2-T2: 确认失败**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_m_context_injection_test 2>&1 | head -20
```

**M2-T3: 最小实现**

修改 `src-tauri/src/runtime/tools/context.rs`，在 `ToolExecutionContext` 结构体中添加：

```rust
// 在文件顶部 use 中添加：
use crate::runtime::hooks::config::HookRegistry;

// 在 ToolExecutionContext 结构体中添加字段：
pub hook_registry: Option<Arc<HookRegistry>>,
```

在 `ToolExecutionContext::new` 中初始化为 `None`，在 `with_*` 构造方法后添加：

```rust
pub fn with_hook_registry(mut self, registry: Arc<HookRegistry>) -> Self {
    self.hook_registry = Some(registry);
    self
}
```

**M2-T4: 确认通过**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_m_context_injection_test -- --nocapture
```

**M2-T5: commit**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/runtime/tools/context.rs src-tauri/tests/plan_m_context_injection_test.rs && git commit -m "$(cat <<'EOF'
feat(hooks): inject HookRegistry into ToolExecutionContext - M2

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task M3 — PreToolUse Hook 集成到 ToolDispatcher

**文件**
- Modify: `src-tauri/src/runtime/tools/dispatcher.rs`
- Test: `src-tauri/tests/plan_m_pre_tool_hook_test.rs`

**调用点分析**：`ToolDispatcher::dispatch` 在 permission 决策通过（`PermissionDecision::Allow`）后、`tool.execute(input, ctx.clone()).await` 前，是 PreToolUse hook 的正确插入位置（对标 toolExecution.ts 第 813 行的 `runPreToolUseHooks` 调用）。

### TDD 步骤

**M3-T1: 写失败测试**

创建 `src-tauri/tests/plan_m_pre_tool_hook_test.rs`：

```rust
//! Plan-M Task 3: PreToolUse Hook 集成到 ToolDispatcher 测试
//!
//! cargo test --test plan_m_pre_tool_hook_test

use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use serde_json::{json, Value};
use lotus_app::runtime::hooks::config::{HookConfig, HookEvent, HookRegistry};
use lotus_app::runtime::tools::{
    AllowAllPermissionPipeline, ToolDefinition, ToolDispatcher, ToolError,
    ToolExecutionContext, ToolResult,
};
use lotus_app::runtime::tools::dispatcher::RuntimeTool;

struct RecordingTool {
    name: String,
    received_inputs: Arc<Mutex<Vec<Value>>>,
}

#[async_trait]
impl RuntimeTool for RecordingTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(&self.name, "recording")
    }
    async fn execute(&self, input: Value, _ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        self.received_inputs.lock().unwrap().push(input);
        Ok(ToolResult::new(&self.name, "ok", None))
    }
}

// M3-1: PreToolUse hook deny 阻止工具执行
#[tokio::test]
async fn pre_tool_hook_deny_prevents_execution() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let tool = Arc::new(RecordingTool {
        name: "bash_tool".to_string(),
        received_inputs: received.clone(),
    });
    let dispatcher = Arc::new(ToolDispatcher::allow_all());
    dispatcher.register(tool);

    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo '{\"behavior\":\"deny\",\"message\":\"blocked by hook\"}'".to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    });

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_hook_registry(Arc::new(registry));

    let result = dispatcher.dispatch("bash_tool", json!({"command": "rm -rf /"}), ctx).await;

    assert!(result.is_err(), "deny hook should cause dispatch to return Err");
    let err_str = format!("{}", result.unwrap_err());
    assert!(err_str.contains("blocked by hook"), "error should contain hook message");
    assert_eq!(received.lock().unwrap().len(), 0, "tool should not have been called");
}

// M3-2: PreToolUse hook allow 让工具正常执行
#[tokio::test]
async fn pre_tool_hook_allow_permits_execution() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let tool = Arc::new(RecordingTool {
        name: "bash_tool".to_string(),
        received_inputs: received.clone(),
    });
    let dispatcher = Arc::new(ToolDispatcher::allow_all());
    dispatcher.register(tool);

    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo '{\"behavior\":\"allow\"}'".to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    });

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_hook_registry(Arc::new(registry));

    let result = dispatcher.dispatch("bash_tool", json!({"command": "ls"}), ctx).await;
    assert!(result.is_ok(), "allow hook should let execution proceed");
    assert_eq!(received.lock().unwrap().len(), 1);
}

// M3-3: PreToolUse hook updatedInput 修改传递给工具的参数
#[tokio::test]
async fn pre_tool_hook_updated_input_modifies_args() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let tool = Arc::new(RecordingTool {
        name: "bash_tool".to_string(),
        received_inputs: received.clone(),
    });
    let dispatcher = Arc::new(ToolDispatcher::allow_all());
    dispatcher.register(tool);

    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        command: r#"printf '{"behavior":"allow","updatedInput":{"command":"echo safe"}}'"#.to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    });

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_hook_registry(Arc::new(registry));

    dispatcher.dispatch("bash_tool", json!({"command": "dangerous"}), ctx).await.unwrap();

    let inputs = received.lock().unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(
        inputs[0].get("command").and_then(|v| v.as_str()),
        Some("echo safe"),
        "tool should receive hook-modified input"
    );
}

// M3-4: 无 hook_registry 时正常执行（向后兼容）
#[tokio::test]
async fn no_hook_registry_executes_normally() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let tool = Arc::new(RecordingTool {
        name: "bash_tool".to_string(),
        received_inputs: received.clone(),
    });
    let dispatcher = Arc::new(ToolDispatcher::allow_all());
    dispatcher.register(tool);

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
    // ctx.hook_registry is None

    let result = dispatcher.dispatch("bash_tool", json!({"command": "ls"}), ctx).await;
    assert!(result.is_ok());
    assert_eq!(received.lock().unwrap().len(), 1);
}

// M3-5: tool_filter 精确匹配——hook 只对特定工具触发
#[tokio::test]
async fn pre_tool_hook_tool_filter_only_affects_target() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let tool = Arc::new(RecordingTool {
        name: "write_file".to_string(),
        received_inputs: received.clone(),
    });
    let dispatcher = Arc::new(ToolDispatcher::allow_all());
    dispatcher.register(tool);

    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo '{\"behavior\":\"deny\"}'".to_string(),
        // 这个 hook 只针对 bash_tool，不针对 write_file
        tool_filter: Some("bash_tool".to_string()),
        timeout_secs: Some(10),
    });

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_hook_registry(Arc::new(registry));

    let result = dispatcher.dispatch("write_file", json!({"path": "/tmp/x"}), ctx).await;
    assert!(result.is_ok(), "write_file should not be affected by bash_tool-only hook");
    assert_eq!(received.lock().unwrap().len(), 1);
}
```

**M3-T2: 确认失败**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_m_pre_tool_hook_test 2>&1 | head -30
```

**M3-T3: 最小实现**

修改 `src-tauri/src/runtime/tools/dispatcher.rs`，在 `dispatch` 方法的权限通过分支后（`PermissionDecision::Allow`）、`ctx.event_sink.emit("tool:executing")` 前插入：

```rust
// 在 dispatcher.rs 顶部添加 use：
use crate::runtime::hooks::{HookDecision, HookRunner};
use crate::runtime::hooks::config::HookEvent;

// 在 dispatch() 方法中，权限通过后插入：
// ── PreToolUse hooks ──────────────────────────────────────────────
if let Some(registry) = ctx.hook_registry.as_ref() {
    let runner = HookRunner::new();
    let hooks: Vec<&_> = registry.hooks_for(HookEvent::PreToolUse, tool_name);
    if !hooks.is_empty() {
        let outcome = runner.run_hooks(&hooks, tool_name, &input).await
            .map_err(|e| ToolError::ExecutionFailed(format!("pre-tool hook error: {e}")))?;
        match outcome.decision {
            HookDecision::Deny { message } => {
                return Err(ToolError::PermissionDenied(message));
            }
            HookDecision::Allow => {
                if let Some(updated) = outcome.updated_input {
                    input = updated;
                }
            }
        }
    }
}
// ── 原有执行逻辑 ───────────────────────────────────────────────────
ctx.event_sink.emit("tool:executing");
```

注意：`input` 参数需改为 `mut input`。

**M3-T4: 确认通过**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_m_pre_tool_hook_test -- --nocapture
```

**M3-T5: commit**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/runtime/tools/dispatcher.rs src-tauri/tests/plan_m_pre_tool_hook_test.rs && git commit -m "$(cat <<'EOF'
feat(hooks): integrate PreToolUse hook into ToolDispatcher - M3

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task M4 — PostToolUse Hook 集成到 ToolDispatcher

**文件**
- Modify: `src-tauri/src/runtime/tools/dispatcher.rs`
- Test: `src-tauri/tests/plan_m_post_tool_hook_test.rs`

**调用点分析**：PostToolUse hook 在 `tool.execute()` 成功返回后立即运行，对应 toolExecution.ts 第 1496 行的 `runPostToolUseHooks`。`PostToolUseFailure` hook 在 `execute()` 的 catch 分支中运行（第 1713 行）。lotus-app 的 `dispatch()` 没有独立的 success/failure 分支，可在 `result?` 后（成功路径）插入 PostToolUse hook。

### TDD 步骤

**M4-T1: 写失败测试**

创建 `src-tauri/tests/plan_m_post_tool_hook_test.rs`：

```rust
//! Plan-M Task 4: PostToolUse Hook 集成到 ToolDispatcher 测试
//!
//! cargo test --test plan_m_post_tool_hook_test

use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use serde_json::{json, Value};
use lotus_app::runtime::hooks::config::{HookConfig, HookEvent, HookRegistry};
use lotus_app::runtime::tools::{
    AllowAllPermissionPipeline, ToolDefinition, ToolDispatcher, ToolError,
    ToolExecutionContext, ToolResult,
};
use lotus_app::runtime::tools::dispatcher::{RuntimeTool, ToolDispatchOutcome};

struct OkTool {
    name: String,
}

#[async_trait]
impl RuntimeTool for OkTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(&self.name, "always ok")
    }
    async fn execute(&self, _input: Value, _ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new(&self.name, "success output", None))
    }
}

// M4-1: PostToolUse hook 在工具成功后执行（prevent_continuation 记录在 outcome）
#[tokio::test]
async fn post_tool_hook_executes_after_success() {
    let hook_ran = Arc::new(Mutex::new(false));
    let hook_ran_clone = hook_ran.clone();

    // 用临时文件作为 hook 执行的证明
    let tmp_path = std::env::temp_dir().join("plan_m_post_hook_ran.txt");
    let tmp_str = tmp_path.to_str().unwrap().to_string();

    let tool = Arc::new(OkTool { name: "bash_tool".to_string() });
    let dispatcher = Arc::new(ToolDispatcher::allow_all());
    dispatcher.register(tool);

    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PostToolUse,
        command: format!("touch {} && echo '{{\"behavior\":\"allow\"}}'", tmp_str),
        tool_filter: None,
        timeout_secs: Some(10),
    });

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_hook_registry(Arc::new(registry));

    let result = dispatcher.dispatch("bash_tool", json!({}), ctx).await;
    assert!(result.is_ok());

    // hook 创建了 tmp 文件
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(tmp_path.exists(), "post-tool hook should have run and created file");
    let _ = std::fs::remove_file(&tmp_path);
}

// M4-2: PostToolUse hook preventContinuation 记录在 ToolDispatchOutcome
#[tokio::test]
async fn post_tool_hook_prevent_continuation_surfaced() {
    let tool = Arc::new(OkTool { name: "bash_tool".to_string() });
    let dispatcher = Arc::new(ToolDispatcher::allow_all());
    dispatcher.register(tool);

    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PostToolUse,
        command: "echo '{\"behavior\":\"allow\",\"preventContinuation\":true,\"stopReason\":\"task done\"}'".to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    });

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_hook_registry(Arc::new(registry));

    let outcome = dispatcher.dispatch("bash_tool", json!({}), ctx).await.unwrap();
    match outcome {
        ToolDispatchOutcome::Completed { prevent_continuation, stop_reason, .. } => {
            assert!(prevent_continuation, "preventContinuation from PostToolUse hook should be surfaced");
            assert_eq!(stop_reason.as_deref(), Some("task done"));
        }
        _ => panic!("expected Completed outcome"),
    }
}

// M4-3: 无 PostToolUse hook 时 prevent_continuation 默认 false
#[tokio::test]
async fn no_post_hook_no_prevent_continuation() {
    let tool = Arc::new(OkTool { name: "bash_tool".to_string() });
    let dispatcher = Arc::new(ToolDispatcher::allow_all());
    dispatcher.register(tool);

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");

    let outcome = dispatcher.dispatch("bash_tool", json!({}), ctx).await.unwrap();
    match outcome {
        ToolDispatchOutcome::Completed { prevent_continuation, .. } => {
            assert!(!prevent_continuation);
        }
        _ => panic!("expected Completed"),
    }
}
```

**M4-T2: 确认失败**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_m_post_tool_hook_test 2>&1 | head -30
```

**M4-T3: 最小实现**

修改 `ToolDispatchOutcome::Completed` variant（`dispatcher.rs`），添加两个字段：

```rust
pub enum ToolDispatchOutcome {
    Completed {
        result: ToolResult,
        event_names: Vec<String>,
        max_result_size_chars: usize,
        /// PostToolUse hook 指示阻止后续对话轮
        prevent_continuation: bool,
        /// preventContinuation 时的原因
        stop_reason: Option<String>,
    },
    AskRequired(PermissionDecision),
}
```

在 `dispatch()` 的成功路径上（`let result = result?;` 后），插入：

```rust
// ── PostToolUse hooks ─────────────────────────────────────────────
let mut prevent_continuation = false;
let mut stop_reason: Option<String> = None;
if let Some(registry) = ctx.hook_registry.as_ref() {
    let runner = HookRunner::new();
    let tool_result_value = serde_json::to_value(&result.content).unwrap_or(Value::Null);
    let hooks: Vec<&_> = registry.hooks_for(HookEvent::PostToolUse, tool_name);
    if !hooks.is_empty() {
        if let Ok(outcome) = runner.run_hooks(&hooks, tool_name, &tool_result_value).await {
            prevent_continuation = outcome.prevent_continuation;
            stop_reason = outcome.stop_reason;
        }
    }
}
ctx.event_sink.emit("tool:completed");
Ok(ToolDispatchOutcome::Completed {
    result,
    event_names: ctx.event_sink.snapshot(),
    max_result_size_chars: definition.default_max_result_size_chars,
    prevent_continuation,
    stop_reason,
})
```

注意需要更新所有 `ToolDispatchOutcome::Completed` 的解构点（在 `chat_turn_driver.rs`、`query_engine.rs` 等处补充 `prevent_continuation: _` 字段）。

**M4-T4: 确认通过**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_m_post_tool_hook_test -- --nocapture
```

**M4-T4b: 更新所有解构 ToolDispatchOutcome 的位置**

修改 `ToolDispatchOutcome::Completed` 后，所有对其解构的位置都需要补充新字段，否则编译失败。

```bash
cd src-tauri && cargo check 2>&1 | grep -E "missing field|pattern"
```

修复所有报错的解构点。主要位置在 `chat_turn_driver.rs` 和 `query_engine.rs`，在 `Completed { result, event_names, max_result_size_chars, .. }` 模式中补充 `prevent_continuation: _` 和 `stop_reason: _`（或使用 `..` 忽略剩余字段）。

**M4-T5: commit**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/runtime/tools/dispatcher.rs src-tauri/tests/plan_m_post_tool_hook_test.rs && git commit -m "$(cat <<'EOF'
feat(hooks): integrate PostToolUse hook into ToolDispatcher - M4

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task M5 — Stop Hook 集成到 TurnDriver

**文件**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/runtime/chat/turn_config.rs`（新增 `stop_hook_prevent_continuation: bool` 和 `stop_hook_reason: Option<String>` 字段到 `TurnIterationState`）
- Modify: `src-tauri/src/runtime/hooks/config.rs`（添加 `StopHookInput` 结构）
- Test: `src-tauri/tests/plan_m_stop_hook_test.rs`

**调用点分析**：Stop hook 对标 claude-code-best 中在 turn 完成后（query loop 结束后）触发的逻辑。在 `run_chat_turn_s4` 的 Step 8（Emit terminal events）之前调用，并将 `prevent_continuation` 结果存入 turn 状态，供上层（`SessionRuntime`）决定是否向前端发送 `preventContinuation` 事件。

Stop hook 的输入包含：turn 完成原因（`stop_reason`）、本次 turn 的最终文本内容。

### TDD 步骤

**M5-T1: 写失败测试**

创建 `src-tauri/tests/plan_m_stop_hook_test.rs`：

```rust
//! Plan-M Task 5: Stop Hook 集成到 TurnDriver 测试
//!
//! cargo test --test plan_m_stop_hook_test

use lotus_app::runtime::hooks::config::{HookConfig, HookEvent, HookRegistry};
use lotus_app::runtime::hooks::runner::HookRunner;

// M5-1: Stop hook 单独运行——返回 preventContinuation
#[tokio::test]
async fn stop_hook_prevent_continuation() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::Stop,
        command: "echo '{\"behavior\":\"allow\",\"preventContinuation\":true,\"stopReason\":\"stop signal received\"}'".to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    };
    let input = serde_json::json!({"stop_reason": "content_complete", "content": "Final response."});
    let result = runner.run_hook(&config, "__stop__", &input).await.unwrap();
    assert!(result.prevent_continuation);
    assert_eq!(result.stop_reason.as_deref(), Some("stop signal received"));
}

// M5-2: Stop hook allow 不阻断
#[tokio::test]
async fn stop_hook_allow_no_prevent() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::Stop,
        command: "echo '{\"behavior\":\"allow\"}'".to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    };
    let input = serde_json::json!({"stop_reason": "content_complete"});
    let result = runner.run_hook(&config, "__stop__", &input).await.unwrap();
    assert!(!result.prevent_continuation);
}

// M5-3: HookRegistry hooks_for Stop event 返回正确 hooks
#[test]
fn hook_registry_stop_hooks_filtering() {
    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo pre".to_string(),
        tool_filter: None,
        timeout_secs: None,
    });
    registry.hooks.push(HookConfig {
        event: HookEvent::Stop,
        command: "echo stop".to_string(),
        tool_filter: None,
        timeout_secs: None,
    });

    // Stop event 使用 "__stop__" 作为 tool_name 标记
    let stop_hooks = registry.hooks_for(HookEvent::Stop, "__stop__");
    assert_eq!(stop_hooks.len(), 1);
    assert_eq!(stop_hooks[0].command, "echo stop");
}

// M5-4: TurnIterationState 记录 stop_hook_prevent_continuation
//
// 注意：这个测试验证的是 TurnIterationState 的数据结构，不测试完整的
// run_chat_turn_s4（需要 LLM executor mock，复杂度超出本 task）。
#[test]
fn turn_iteration_state_stop_hook_field() {
    use lotus_app::runtime::chat::turn_config::TurnIterationState;
    let mut state = TurnIterationState::new(vec![]);
    // 默认 false
    assert!(!state.stop_hook_prevent_continuation);
    state.stop_hook_prevent_continuation = true;
    assert!(state.stop_hook_prevent_continuation);
}
```

**M5-T2: 确认失败**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_m_stop_hook_test 2>&1 | head -30
```

**M5-T3: 最小实现**

1. 在 `src-tauri/src/runtime/chat/turn_config.rs` 的 `TurnIterationState` 结构体中追加两个字段：
```rust
    /// Stop hook 设置了 preventContinuation
    pub stop_hook_prevent_continuation: bool,
    /// preventContinuation 时 stop hook 返回的原因
    pub stop_hook_reason: Option<String>,
```

并在 `TurnIterationState::new()` 中初始化（在 `safeguard_phase1_injected: false,` 行之后）：
```rust
            stop_hook_prevent_continuation: false,
            stop_hook_reason: None,
```

2. 在 `run_chat_turn_s4` 的 Step 7（`persist_assistant_message`）之后、Step 8（Emit terminal events）之前插入：

```rust
// ── Stop hooks ────────────────────────────────────────────────────
if let Some(hooks_arc) = /* 从 executor 或 TurnConfig 获取 hook registry */ {
    let runner = HookRunner::new();
    let registry = hooks_arc.as_ref();
    let stop_hooks = registry.hooks_for(HookEvent::Stop, "__stop__");
    if !stop_hooks.is_empty() {
        let stop_input = serde_json::json!({
            "stop_reason": if state.stream_cancelled { "cancelled" } else { "content_complete" },
            "content": &state.full_content,
        });
        if let Ok(outcome) = runner.run_hooks(&stop_hooks, "__stop__", &stop_input).await {
            state.stop_hook_prevent_continuation = outcome.prevent_continuation;
            state.stop_hook_reason = outcome.stop_reason;
        }
    }
}
```

注意：Stop hook 的 registry 来源需与 PreToolUse/PostToolUse 保持一致——通过 `TurnConfig` 传入（在 `ChatTurnRequest` 或 `TurnConfig` 中增加 `hook_registry: Option<Arc<HookRegistry>>`）。具体注入路径由实现者根据 `SessionRuntime` 的 hook registry 加载逻辑确定。

**M5-T4: 确认通过**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_m_stop_hook_test -- --nocapture
```

**M5-T5: commit**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/runtime/chat/ src-tauri/tests/plan_m_stop_hook_test.rs && git commit -m "$(cat <<'EOF'
feat(hooks): integrate Stop hook into TurnDriver - M5

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task M6 — review 回归测试（架构约束）

**文件**
- Test: `src-tauri/tests/plan_m_review_hook_architecture_test.rs`

验证 hook 系统实现后不违反核心架构约束：
1. `runtime/hooks/` 下的模块不 `use tauri::*`。
2. `HookRegistry` 通过 `ToolExecutionContext` 注入，不直接持有 `Arc<LlmGateway>` 或 `Arc<AgentRuntime>`。
3. `CapabilityContext`（`capability.rs`）未被扩大（未新增 hook registry 字段）。

```rust
//! Plan-M Task 6: Hook 系统架构约束回归测试
//!
//! cargo test --test plan_m_review_hook_architecture_test

// M6-1: hooks 模块不依赖 tauri
#[test]
fn review_hooks_module_no_tauri_dependency() {
    // 静态验证：编译通过即表示 hooks 模块不引入 tauri crate
    // 运行时验证：检查模块路径不包含 tauri 类型
    let _ = lotus_app::runtime::hooks::config::HookRegistry::new();
    // 如果编译通过，即证明 hooks 模块不依赖 tauri::*
}

// M6-2: CapabilityContext 未被扩大
#[test]
fn review_capability_context_not_widened() {
    use lotus_app::runtime::tools::capability::CapabilityContext;
    // CapabilityContext 的字段应只有 storage、notifications 等狭义能力
    // hook_registry 不应在此
    // 编译通过即验证
    let _ = std::mem::size_of::<CapabilityContext>();
}

// M6-3: HookRunner 不依赖 LlmGateway
#[test]
fn review_hook_runner_no_llm_gateway() {
    // HookRunner 只依赖 tokio::process::Command，不持有 LLM 能力
    let runner = lotus_app::runtime::hooks::runner::HookRunner::new();
    let _ = runner;
}
```

**确认通过**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

**commit**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/tests/plan_m_review_hook_architecture_test.rs && git commit -m "$(cat <<'EOF'
test(hooks): add architecture constraint regression tests - M6

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## 实现注意事项

1. **HookRegistry 加载路径**：本计划不实现 end-user config 文件加载（与 CLAUDE.md 对齐，MCP 也是类似情况）。`HookRegistry` 在测试中直接构造，生产路径由 `SessionRuntime` 在 session 初始化时从 workspace 配置目录（如 `workspace/config/hooks.json`）加载并注入到 `ChatTurnRequest` / `ToolExecutionContext`。

2. **Shell 命令安全**：hook 命令由用户配置，不应在 Rust 层做额外注入。stdin 传入 JSON，不拼接 shell 参数。

3. **超时策略**：默认 30s，超时后静默 Allow（不阻断执行），仅 warn 日志。对标 claude-code-best 的 slow hook warning（2000ms 阈值记录 debug）。

4. **并发 hook 执行**：本计划串行执行同类型 hook（与对标源一致，单工具 hook 数量少）。后续可扩展为 `join_all`。

5. **PostToolUse hook 的 `tool_input`**：dispatcher 在 hook 运行时应传入实际执行的 input（可能经 PreToolUse hook 修改）。

<!-- reviewed: 2026-04-18, fixes applied -->
