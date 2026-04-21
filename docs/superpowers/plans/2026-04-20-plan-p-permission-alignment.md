# Permission 对标改造（Plan-P）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ] `) syntax for tracking.

**Goal:** 把 lotus-app 的权限模型升级为三层规则 + 完整 permissionMode 语义 + subagent Ask 闭环，对齐 claude-code-best 的 permission model。

**Architecture:** PermissionStore 已有三层结构（session/workspace/user），本计划补齐 apply_permission_mode 的 Plan 模式语义、WorkerRunConfig 的独立 permission_mode 字段、worker_runtime 的 Ask 冒泡与父 run control plane 的接线、Bash/文件/MCP 的细粒度匹配。

**Tech Stack:** Rust, Tauri v2, React/TypeScript, Zustand

**Worktree branch:** pzc

---

## 现状速查

| 文件 | 现状 | 缺口 |
|---|---|---|
| `src-tauri/src/runtime/tools/permission.rs` | `apply_permission_mode` 的 Plan 模式只改 reason，不改为 Deny | Plan 模式下 Ask 应转为 Deny，不再向上冒泡 |
| `src-tauri/src/runtime/store/permission_store.rs` | 三层结构已有，`get_for_glob` / `get_for_command_pattern` 缺失 | PathGlob / CommandPattern scope 没有匹配逻辑 |
| `src-tauri/src/runtime/agent/worker_runtime.rs` | `WorkerRunConfig` 无 `permission_mode` 字段 | subagent 无法继承父 run 的 permission mode |
| `src-tauri/src/runtime/agent/worker_runtime.rs` | AskRequired 冒泡后直接 break，父 run 无 control plane 接收 | Ask 无法在父 run ChatTurnDriver 中被解析 |
| `src-tauri/src/runtime/tools/builtin/bash.rs` | `check_permissions` 只做危险模式 Deny，不查 CommandPattern 规则 | 用户的 CommandPattern 规则无法生效 |
| `src-tauri/src/runtime/tools/builtin/file.rs` | 无 `check_permissions` override | 用户的 PathGlob 规则无法生效 |
| `src-tauri/src/runtime/mcp/runtime_tool.rs` | `execute` 内无 authorize 调用（依赖 dispatcher 中的 pipeline） | 需确认 dispatcher 路径完整通过 StorePolicyPipeline |
| `src-tauri/src/runtime/agent/invocation.rs` | `AgentStatus` 只有 4 种，无 `Failed` | 无法区分 cancelled 与 error 结束 |

---

## Task 1 — plan 模式语义修正：Ask → Deny

- [ ] **1-a 先写失败测试**

新建 `src-tauri/tests/review_permission_plan_mode_test.rs`：

```rust
//! Plan mode 下 Ask 应被转为 Deny，而非保留为 Ask（原来只改 reason）。

use app_lib::runtime::tools::permission::{
    apply_permission_mode, default_permission_ask, PermissionDecision, PermissionDestination,
    PermissionMode, PermissionReason,
};

fn make_ask() -> PermissionDecision {
    let (remember_options, default_destination) = default_permission_ask();
    PermissionDecision::Ask {
        message: "Run this tool?".into(),
        suggestions: vec!["Allow once".into()],
        remember_options,
        default_destination,
        reason: PermissionReason::UnknownScope,
    }
}

#[test]
fn review_plan_mode_ask_becomes_deny() {
    let result = apply_permission_mode(make_ask(), "bash", PermissionMode::Plan);
    assert!(
        matches!(result, PermissionDecision::Deny { .. }),
        "Plan mode must convert Ask to Deny, got: {:?}",
        result
    );
    if let PermissionDecision::Deny { reason, .. } = result {
        assert!(
            matches!(reason, PermissionReason::Mode(ref m) if m == "plan"),
            "Deny reason should be Mode(plan)"
        );
    }
}

#[test]
fn review_plan_mode_allow_passes_through() {
    let allow = PermissionDecision::Allow {
        updated_input: None,
        reason: PermissionReason::StoredPolicy,
    };
    let result = apply_permission_mode(allow, "bash", PermissionMode::Plan);
    assert!(matches!(result, PermissionDecision::Allow { .. }));
}

#[test]
fn review_plan_mode_deny_passes_through() {
    let deny = PermissionDecision::Deny {
        message: "blocked".into(),
        reason: PermissionReason::StoredPolicy,
    };
    let result = apply_permission_mode(deny, "bash", PermissionMode::Plan);
    assert!(matches!(result, PermissionDecision::Deny { .. }));
}

#[test]
fn review_dont_ask_mode_ask_becomes_deny() {
    let result = apply_permission_mode(make_ask(), "bash", PermissionMode::DontAsk);
    assert!(
        matches!(result, PermissionDecision::Deny { .. }),
        "DontAsk mode must also convert Ask to Deny"
    );
}
```

验证命令（应当失败 — `review_plan_mode_ask_becomes_deny` 失败）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_permission_plan_mode_test -- --nocapture 2>&1 | tail -30
```

- [ ] **1-b 修复实现**

修改 `src-tauri/src/runtime/tools/permission.rs` 中 `apply_permission_mode`（`:97-113`），将：

```rust
        (
            PermissionMode::Plan,
            PermissionDecision::Ask {
                message,
                suggestions,
                remember_options,
                default_destination,
                ..
            },
        ) => PermissionDecision::Ask {
            message,
            suggestions,
            remember_options,
            default_destination,
            reason: PermissionReason::Mode("plan".into()),
        },
```

替换为：

```rust
        (PermissionMode::Plan, PermissionDecision::Ask { .. }) => PermissionDecision::Deny {
            message: format!(
                "Tool '{}' requires permission, but current mode is plan (read-only planning phase).",
                tool_name
            ),
            reason: PermissionReason::Mode("plan".into()),
        },
```

同时删除 `permission.rs` 内单元测试 `review_apply_permission_mode_marks_plan_reason`（断言的是旧行为）。

验证命令（应当通过）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_permission_plan_mode_test -- --nocapture
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test permission -- --nocapture 2>&1 | tail -30
```

- [ ] **1-c Commit**

```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app
git add src-tauri/src/runtime/tools/permission.rs src-tauri/tests/review_permission_plan_mode_test.rs
git commit -m "$(cat <<'EOF'
fix(permission): plan mode converts Ask to Deny instead of re-asking

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2 — PermissionStore 补 PathGlob / CommandPattern 匹配

- [ ] **2-a 先写失败测试**

新建 `src-tauri/tests/review_permission_store_glob_test.rs`：

```rust
//! PermissionStore PathGlob / CommandPattern 匹配路径测试。

use app_lib::runtime::store::permission_store::{
    PermissionRule, PermissionScope, PermissionSource, PermissionStore, PolicyDecision,
};
use app_lib::runtime::tools::permission::PermissionDestination;

#[test]
fn review_path_glob_allow_matches_file_inside_workspace() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::Session,
        PermissionRule::simple(
            "write_file",
            PermissionScope::PathGlob("/tmp/ws/**".into()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::Session,
        ),
    );
    let result = store.get_for_path("write_file", "/tmp/ws/data/output.csv");
    assert_eq!(result, Some(PolicyDecision::AlwaysAllow));
}

#[test]
fn review_path_glob_does_not_match_outside_workspace() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::Session,
        PermissionRule::simple(
            "write_file",
            PermissionScope::PathGlob("/tmp/ws/**".into()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::Session,
        ),
    );
    let result = store.get_for_path("write_file", "/etc/passwd");
    assert_eq!(result, None);
}

#[test]
fn review_command_pattern_matches_exact_prefix() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::Workspace,
        PermissionRule::simple(
            "bash",
            PermissionScope::CommandPattern("git ".into()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::Workspace,
        ),
    );
    let result = store.get_for_command("bash", "git status --short");
    assert_eq!(result, Some(PolicyDecision::AlwaysAllow));
}

#[test]
fn review_command_pattern_does_not_match_different_command() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::Workspace,
        PermissionRule::simple(
            "bash",
            PermissionScope::CommandPattern("git ".into()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::Workspace,
        ),
    );
    let result = store.get_for_command("bash", "rm -rf /tmp/old");
    assert_eq!(result, None);
}

#[test]
fn review_path_glob_session_overrides_workspace() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::Workspace,
        PermissionRule::simple(
            "write_file",
            PermissionScope::PathGlob("/tmp/ws/**".into()),
            PolicyDecision::AlwaysDeny,
            PermissionSource::Workspace,
        ),
    );
    store.record_to(
        PermissionDestination::Session,
        PermissionRule::simple(
            "write_file",
            PermissionScope::PathGlob("/tmp/ws/**".into()),
            PolicyDecision::Allow,
            PermissionSource::Session,
        ),
    );
    let result = store.get_for_path("write_file", "/tmp/ws/out.csv");
    assert_eq!(result, Some(PolicyDecision::Allow));
}
```

验证命令（应当失败）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_permission_store_glob_test 2>&1 | tail -30
```

- [ ] **2-b 实现匹配逻辑**

在 `src-tauri/src/runtime/store/permission_store.rs` 文件顶部（use 块之后）追加私有 glob 辅助函数：

```rust
/// Minimal glob matching supporting `**` (any path segments) and `*` (within one segment).
fn glob_matches(glob: &str, path: &str) -> bool {
    glob_matches_inner(glob.as_bytes(), path.as_bytes())
}

fn glob_matches_inner(pattern: &[u8], text: &[u8]) -> bool {
    match (pattern, text) {
        ([], []) => true,
        ([], _) => false,
        ([b'*', b'*', rest @ ..], _) => {
            // `**` matches zero or more path components (including slashes)
            for i in 0..=text.len() {
                if glob_matches_inner(rest, &text[i..]) {
                    return true;
                }
            }
            false
        }
        ([b'*', rest @ ..], _) => {
            // `*` matches within a single segment (stops at '/')
            for i in 0..=text.len() {
                if i > 0 && text[i - 1] == b'/' {
                    break;
                }
                if glob_matches_inner(rest, &text[i..]) {
                    return true;
                }
            }
            false
        }
        ([p, p_rest @ ..], [t, t_rest @ ..]) if p == t => glob_matches_inner(p_rest, t_rest),
        _ => false,
    }
}
```

在 `PermissionLayer` impl 块末尾追加：

```rust
    fn get_for_glob_path(&self, tool_name: &str, path: &str) -> Option<PolicyDecision> {
        for rule in &self.rules {
            if rule.tool_name != tool_name {
                continue;
            }
            if let PermissionScope::PathGlob(glob) = &rule.scope {
                if glob_matches(glob, path) {
                    return Some(rule.decision.clone());
                }
            }
        }
        None
    }

    fn get_for_command_pattern(&self, tool_name: &str, command: &str) -> Option<PolicyDecision> {
        for rule in &self.rules {
            if rule.tool_name != tool_name {
                continue;
            }
            if let PermissionScope::CommandPattern(pattern) = &rule.scope {
                if command.starts_with(pattern.as_str()) || command.contains(pattern.as_str()) {
                    return Some(rule.decision.clone());
                }
            }
        }
        None
    }
```

在 `PermissionStore` impl 块（`flush_user` 之前）追加两个公开方法：

```rust
    /// 按路径查找匹配的 PathGlob 规则。优先级：session > workspace > user。
    pub fn get_for_path(&self, tool_name: &str, path: &str) -> Option<PolicyDecision> {
        self.session
            .read()
            .unwrap()
            .get_for_glob_path(tool_name, path)
            .or_else(|| {
                self.workspace
                    .read()
                    .unwrap()
                    .get_for_glob_path(tool_name, path)
            })
            .or_else(|| {
                self.user
                    .read()
                    .unwrap()
                    .get_for_glob_path(tool_name, path)
            })
    }

    /// 按命令字符串查找匹配的 CommandPattern 规则。优先级：session > workspace > user。
    pub fn get_for_command(&self, tool_name: &str, command: &str) -> Option<PolicyDecision> {
        self.session
            .read()
            .unwrap()
            .get_for_command_pattern(tool_name, command)
            .or_else(|| {
                self.workspace
                    .read()
                    .unwrap()
                    .get_for_command_pattern(tool_name, command)
            })
            .or_else(|| {
                self.user
                    .read()
                    .unwrap()
                    .get_for_command_pattern(tool_name, command)
            })
    }
```

验证命令（应当通过）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_permission_store_glob_test -- --nocapture
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test permission_store -- --nocapture 2>&1 | tail -30
```

- [ ] **2-c Commit**

```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app
git add src-tauri/src/runtime/store/permission_store.rs src-tauri/tests/review_permission_store_glob_test.rs
git commit -m "$(cat <<'EOF'
feat(permission): add PathGlob and CommandPattern matching to PermissionStore

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3 — WorkerRunConfig 新增 permission_mode 字段

- [ ] **3-a 先写失败测试**

新建 `src-tauri/tests/review_worker_run_config_permission_mode_test.rs`：

```rust
//! WorkerRunConfig 必须携带 permission_mode 字段。

use app_lib::runtime::agent::worker_runtime::WorkerRunConfig;
use app_lib::runtime::tools::permission::PermissionMode;

#[test]
fn review_worker_run_config_has_permission_mode_field() {
    let config = WorkerRunConfig {
        allowed_tools: vec![],
        conversation_id: "conv-test".into(),
        parent_run_id: None,
        background: false,
        app_handle: None,
        cancel_token: None,
        permission_mode: PermissionMode::Plan,
    };
    assert_eq!(config.permission_mode, PermissionMode::Plan);
}

#[test]
fn review_worker_run_config_default_permission_mode_is_default() {
    let config = WorkerRunConfig {
        allowed_tools: vec![],
        conversation_id: "conv-test".into(),
        parent_run_id: None,
        background: false,
        app_handle: None,
        cancel_token: None,
        permission_mode: PermissionMode::Default,
    };
    assert_eq!(config.permission_mode, PermissionMode::Default);
}
```

验证命令（应当失败）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_worker_run_config_permission_mode_test 2>&1 | tail -20
```

- [ ] **3-b 修改 WorkerRunConfig 与调用链**

在 `src-tauri/src/runtime/agent/worker_runtime.rs` 中追加导入并修改 `WorkerRunConfig`：

```rust
use crate::runtime::tools::permission::PermissionMode;

pub struct WorkerRunConfig {
    pub allowed_tools: Vec<String>,
    pub conversation_id: String,
    pub parent_run_id: Option<RunId>,
    pub background: bool,
    pub app_handle: Option<tauri::AppHandle>,
    pub cancel_token: Option<CancellationToken>,
    /// 权限模式，从父 run 传入。Default 时行为不变。
    pub permission_mode: PermissionMode,
}
```

在 `SubagentWorkerRuntime::run` 构造 `WorkerRunConfig` 时追加字段（`config.permission_mode` 来自 `SubAgentConfig`）：

```rust
        let run_config = WorkerRunConfig {
            allowed_tools: config.allowed_tools.clone(),
            conversation_id: config.conversation_id,
            parent_run_id: config.parent_run_id,
            background: config.background,
            app_handle: config.app_handle,
            cancel_token: config.cancel_token,
            permission_mode: config.permission_mode,
        };
```

在 `src-tauri/src/llm/sub_agent.rs` 的 `SubAgentConfig` 结构体末尾追加：

```rust
    /// 继承父 run 的权限模式。默认为 Default。
    #[serde(default)]
    pub permission_mode: crate::runtime::tools::permission::PermissionMode,
```

验证命令（应当通过）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_worker_run_config_permission_mode_test -- --nocapture
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo build 2>&1 | grep "^error" | head -20
```

- [ ] **3-c Commit**

```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app
git add src-tauri/src/runtime/agent/worker_runtime.rs src-tauri/src/llm/sub_agent.rs src-tauri/tests/review_worker_run_config_permission_mode_test.rs
git commit -m "$(cat <<'EOF'
feat(permission): add permission_mode to WorkerRunConfig and SubAgentConfig

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4 — Ask 闭环：worker_runtime AskRequired 接入父 run control plane

- [ ] **4-a 先写失败测试**

新建 `src-tauri/tests/review_worker_ask_control_plane_test.rs`：

```rust
//! WorkerRunConfig 必须有 control_plane 字段，可接受 Option<Arc<dyn PendingPermissionControlPlane>>。

use std::sync::Arc;
use app_lib::runtime::agent::worker_runtime::WorkerRunConfig;
use app_lib::runtime::store::PendingPermissionControlPlane;
use app_lib::runtime::tools::permission::PermissionMode;

#[test]
fn review_worker_run_config_accepts_optional_control_plane() {
    let _config = WorkerRunConfig {
        allowed_tools: vec![],
        conversation_id: "c".into(),
        parent_run_id: None,
        background: false,
        app_handle: None,
        cancel_token: None,
        permission_mode: PermissionMode::Default,
        control_plane: None::<Arc<dyn PendingPermissionControlPlane>>,
    };
    // 若能编译即通过
}
```

验证命令（应当失败）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_worker_ask_control_plane_test 2>&1 | tail -20
```

- [ ] **4-b 在 WorkerRunConfig 追加 control_plane 字段**

在 `src-tauri/src/runtime/agent/worker_runtime.rs` 的 `WorkerRunConfig` 中追加：

```rust
use crate::runtime::store::PendingPermissionControlPlane;

pub struct WorkerRunConfig {
    // ... 已有字段 ...
    pub permission_mode: PermissionMode,
    /// 父 run 的 pending permission control plane。
    /// Some 时：worker 内 AskRequired 转发给 control plane 等待解析，不冒泡给父 turn。
    /// None 时：保留原有冒泡行为（subagent caller 处理）。
    pub control_plane: Option<Arc<dyn PendingPermissionControlPlane>>,
}
```

在 `SubAgentConfig` 中追加：

```rust
    pub control_plane: Option<Arc<dyn crate::runtime::store::PendingPermissionControlPlane>>,
```

在 `SubagentWorkerRuntime::run` 中构造 `WorkerRunConfig` 追加：

```rust
            control_plane: config.control_plane.clone(),
```

- [ ] **4-c 修改 AskRequired 处理逻辑**

在 `run_worker_turn` 中定位 `ToolRoundResult::Ok(RuntimeToolCallOutcome::AskRequired { .. })` 分支（约 `:400`），在该分支开头插入 control plane 路径：

```rust
ToolRoundResult::Ok(RuntimeToolCallOutcome::AskRequired {
    tool_call_id,
    tool_name,
    capability_scopes,
    original_request,
    decision,
}) => {
    if let Some(cp) = config.control_plane.as_ref() {
        use crate::runtime::store::{PendingPermissionRequest, PendingPermissionResolution};
        use crate::runtime::tools::permission::PermissionDecision;

        let (message, suggestions, remember_options, default_destination) = match &decision {
            PermissionDecision::Ask {
                message,
                suggestions,
                remember_options,
                default_destination,
                ..
            } => (message.clone(), suggestions.clone(), remember_options.clone(), *default_destination),
            _ => {
                warn!("[SubAgent] AskRequired has non-Ask decision, falling back to bubble");
                let bubbled = annotate_subagent_ask_decision(&tool_name, &tool_call_id, decision);
                pending_ask = Some(bubbled);
                break 'agent_loop;
            }
        };

        let pending_request = PendingPermissionRequest {
            tool_call_id: tool_call_id.clone().into(),
            session_id: turn.session_id().clone(),
            run_id: child_run_id.clone(),
            tool_name: tool_name.clone(),
            capability_scopes: capability_scopes.clone(),
            message,
            suggestions,
            mode: config.permission_mode,
            remember_options,
            default_destination,
            original_request: original_request.clone(),
        };

        match cp.insert_pending_request(pending_request) {
            Ok(rx) => {
                // 最多等待 5 分钟
                let resolution = tokio::time::timeout(
                    std::time::Duration::from_secs(300),
                    rx,
                )
                .await
                .ok()
                .and_then(|r| r.ok())
                .unwrap_or(PendingPermissionResolution::Cancel {
                    message: "Worker permission ask timed out".into(),
                });

                match resolution {
                    PendingPermissionResolution::Allow { updated_input, .. } => {
                        let replay_args = updated_input.unwrap_or_else(|| original_request.args.clone());
                        let replay = RuntimeToolCallRequest {
                            tool_call_id: tool_call_id.clone(),
                            tool_name: tool_name.clone(),
                            args: replay_args,
                            purpose: original_request.purpose.clone(),
                        };
                        let replayed = round_driver
                            .execute_round(&turn, &tool_event_bus, vec![replay])
                            .await;
                        for rr in replayed {
                            if let ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
                                tool_call_id: rid,
                                tool_name: rname,
                                content,
                                max_result_size_chars,
                                ..
                            }) = rr
                            {
                                let c = truncate_tool_content(&content, max_result_size_chars);
                                request.messages.push(ChatMessage::tool_result(&rid, &rname, c));
                            }
                        }
                    }
                    PendingPermissionResolution::Deny { message, .. }
                    | PendingPermissionResolution::Cancel { message } => {
                        request.messages.push(ChatMessage::tool_result(
                            &tool_call_id,
                            &tool_name,
                            message,
                        ));
                    }
                }
            }
            Err(e) => {
                warn!("[SubAgent] insert_pending_request failed: {e}, bubbling");
                let bubbled = annotate_subagent_ask_decision(&tool_name, &tool_call_id, decision);
                pending_ask = Some(bubbled);
                break 'agent_loop;
            }
        }
    } else {
        // 保留原有冒泡行为
        let bubbled = annotate_subagent_ask_decision(&tool_name, &tool_call_id, decision);
        terminal_tool_results.push(SubAgentTerminalToolResult {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            success: false,
            summary: "Permission Ask required".to_string(),
            generated_files: Vec::new(),
        });
        emit_tool_completed(
            config.app_handle.as_ref(),
            &config.conversation_id,
            &tool_call_id,
            false,
            Some("Permission Ask required"),
        );
        request.messages.push(ChatMessage::tool_result(
            &tool_call_id,
            &tool_name,
            "Permission Ask required".to_string(),
        ));
        warn!(
            "[SubAgent] Tool '{}' AskRequired; bubbling to parent: {}",
            tool_name, bubbled
        );
        pending_ask = Some(bubbled);
        break 'agent_loop;
    }
}
```

验证命令（应当通过）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_worker_ask_control_plane_test -- --nocapture
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo build 2>&1 | grep "^error" | head -20
```

- [ ] **4-d Commit**

```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app
git add src-tauri/src/runtime/agent/worker_runtime.rs src-tauri/src/llm/sub_agent.rs src-tauri/tests/review_worker_ask_control_plane_test.rs
git commit -m "$(cat <<'EOF'
feat(permission): wire worker_runtime Ask to parent control_plane instead of always bubbling

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5 — BashTool 接入 CommandPattern 规则

- [ ] **5-a 先写失败测试**

新建 `src-tauri/tests/review_bash_command_pattern_permission_test.rs`：

```rust
//! BashTool 应查询 PermissionStore 的 CommandPattern 规则并返回正确决策。

use std::sync::Arc;
use app_lib::runtime::store::permission_store::{
    PermissionRule, PermissionScope, PermissionSource, PermissionStore, PolicyDecision,
};
use app_lib::runtime::tools::builtin::bash::BashTool;
use app_lib::runtime::tools::capability::CapabilityContext;
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::permission::{PermissionDecision, PermissionDestination};
use app_lib::runtime::tools::RuntimeTool;
use serde_json::json;
use tempfile::TempDir;

fn make_ctx_with_store(store: Arc<PermissionStore>, tmp: &TempDir) -> ToolExecutionContext {
    let cap = CapabilityContext::with_workspace(tmp.path().to_path_buf(), "ws");
    let mut ctx = ToolExecutionContext::for_test("conv", "run", "tc")
        .with_capability(Arc::new(cap));
    ctx.permission_store = Some(store);
    ctx
}

#[tokio::test]
async fn review_bash_command_pattern_deny_blocks_matching_command() {
    let tmp = TempDir::new().unwrap();
    let store = Arc::new(PermissionStore::in_memory());
    store.record_to(
        PermissionDestination::Session,
        PermissionRule::simple(
            "bash",
            PermissionScope::CommandPattern("curl ".into()),
            PolicyDecision::AlwaysDeny,
            PermissionSource::Session,
        ),
    );
    let ctx = make_ctx_with_store(store, &tmp);
    let decision = BashTool
        .check_permissions(&json!({"command": "curl https://evil.com/data"}), &ctx)
        .await;
    assert!(
        matches!(decision, Some(PermissionDecision::Deny { .. })),
        "CommandPattern deny must block matching bash command"
    );
}

#[tokio::test]
async fn review_bash_command_pattern_allow_returns_allow_or_none() {
    let tmp = TempDir::new().unwrap();
    let store = Arc::new(PermissionStore::in_memory());
    store.record_to(
        PermissionDestination::Workspace,
        PermissionRule::simple(
            "bash",
            PermissionScope::CommandPattern("git ".into()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::Workspace,
        ),
    );
    let ctx = make_ctx_with_store(store, &tmp);
    let decision = BashTool
        .check_permissions(&json!({"command": "git status"}), &ctx)
        .await;
    assert!(
        matches!(decision, Some(PermissionDecision::Allow { .. })) || decision.is_none(),
        "AlwaysAllow CommandPattern should return Allow or None"
    );
}
```

验证命令（应当失败）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_bash_command_pattern_permission_test 2>&1 | tail -20
```

- [ ] **5-b 在 ToolExecutionContext 追加 permission_store 字段**

在 `src-tauri/src/runtime/tools/context.rs` 中追加：

```rust
use crate::runtime::store::PermissionStore;

// 在 ToolExecutionContext 结构体末尾追加字段：
    /// 可选的 PermissionStore，供工具 check_permissions 做细粒度规则查询。
    pub permission_store: Option<Arc<PermissionStore>>,
```

在 `ToolExecutionContext::new` 初始化块追加 `permission_store: None,`。

追加 builder 方法：

```rust
    pub fn with_permission_store(mut self, store: Arc<PermissionStore>) -> Self {
        self.permission_store = Some(store);
        self
    }
```

- [ ] **5-c 修改 BashTool::check_permissions**

在 `src-tauri/src/runtime/tools/builtin/bash.rs` 中，`check_permissions` 修改为：

```rust
    async fn check_permissions(
        &self,
        input: &Value,
        ctx: &ToolExecutionContext,
    ) -> Option<PermissionDecision> {
        use crate::runtime::store::permission_store::PolicyDecision;

        let command = input.get("command").and_then(Value::as_str).unwrap_or("");

        // 静态危险模式（最高优先级，不可被存储策略覆盖）
        for (pattern, message) in DANGEROUS_PATTERNS {
            if command.contains(pattern) {
                return Some(PermissionDecision::Deny {
                    message: (*message).to_string(),
                    reason: PermissionReason::Other("dangerous_pattern".to_string()),
                });
            }
        }

        // CommandPattern 规则查询
        if let Some(store) = ctx.permission_store.as_ref() {
            match store.get_for_command("bash", command) {
                Some(PolicyDecision::AlwaysDeny) | Some(PolicyDecision::Deny) => {
                    return Some(PermissionDecision::Deny {
                        message: format!(
                            "Command blocked by stored CommandPattern policy: {}",
                            command.chars().take(80).collect::<String>()
                        ),
                        reason: PermissionReason::StoredPolicy,
                    });
                }
                Some(PolicyDecision::AlwaysAllow) | Some(PolicyDecision::Allow) => {
                    return Some(PermissionDecision::Allow {
                        updated_input: None,
                        reason: PermissionReason::StoredPolicy,
                    });
                }
                None => {}
            }
        }

        None
    }
```

验证命令（应当通过）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_bash_command_pattern_permission_test -- --nocapture
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test bash -- --nocapture 2>&1 | tail -30
```

- [ ] **5-d Commit**

```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app
git add src-tauri/src/runtime/tools/context.rs src-tauri/src/runtime/tools/builtin/bash.rs src-tauri/tests/review_bash_command_pattern_permission_test.rs
git commit -m "$(cat <<'EOF'
feat(permission): BashTool checks CommandPattern rules from PermissionStore

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6 — 文件类工具接入 PathGlob 规则

- [ ] **6-a 先写失败测试**

新建 `src-tauri/tests/review_file_path_glob_permission_test.rs`：

```rust
//! 文件写工具应查询 PermissionStore PathGlob 规则，Deny 路径应被拒绝。

use std::sync::Arc;
use app_lib::runtime::store::permission_store::{
    PermissionRule, PermissionScope, PermissionSource, PermissionStore, PolicyDecision,
};
use app_lib::runtime::tools::capability::CapabilityContext;
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::permission::{PermissionDecision, PermissionDestination};
use app_lib::runtime::tools::RuntimeTool;
use serde_json::json;
use tempfile::TempDir;

// 注：WriteFileTool 的实际模块路径需根据当前代码确认（可能在 builtin/write_file.rs 或 builtin/mod.rs 导出）
use app_lib::runtime::tools::builtin::WriteFileTool;

fn make_ctx(store: Arc<PermissionStore>, tmp: &TempDir) -> ToolExecutionContext {
    let cap = CapabilityContext::with_workspace(tmp.path().to_path_buf(), "ws");
    let mut ctx = ToolExecutionContext::for_test("conv", "run", "tc")
        .with_capability(Arc::new(cap));
    ctx.permission_store = Some(store);
    ctx
}

#[tokio::test]
async fn review_write_file_path_glob_deny_blocks_matching_path() {
    let tmp = TempDir::new().unwrap();
    let store = Arc::new(PermissionStore::in_memory());
    store.record_to(
        PermissionDestination::User,
        PermissionRule::simple(
            "write_file",
            PermissionScope::PathGlob("/etc/**".into()),
            PolicyDecision::AlwaysDeny,
            PermissionSource::User,
        ),
    );
    let ctx = make_ctx(store, &tmp);
    let decision = WriteFileTool
        .check_permissions(&json!({"path": "/etc/cron.d/evil", "content": "hack"}), &ctx)
        .await;
    assert!(
        matches!(decision, Some(PermissionDecision::Deny { .. })),
        "PathGlob deny must block write to /etc/"
    );
}

#[tokio::test]
async fn review_write_file_no_matching_glob_returns_none() {
    let tmp = TempDir::new().unwrap();
    let store = Arc::new(PermissionStore::in_memory());
    let ctx = make_ctx(store, &tmp);
    let decision = WriteFileTool
        .check_permissions(&json!({"path": "output/result.csv", "content": "a,b"}), &ctx)
        .await;
    assert!(decision.is_none(), "No glob rule should return None");
}
```

验证命令（应当失败）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_file_path_glob_permission_test 2>&1 | tail -20
```

- [ ] **6-b 为 WriteFileTool / EditFileTool 实现 check_permissions**

在相应工具的 `RuntimeTool` impl 中追加（以 WriteFileTool 为例，EditFileTool 同理）：

```rust
    async fn check_permissions(
        &self,
        input: &Value,
        ctx: &ToolExecutionContext,
    ) -> Option<crate::runtime::tools::permission::PermissionDecision> {
        use crate::runtime::store::permission_store::PolicyDecision;
        use crate::runtime::tools::permission::{PermissionDecision, PermissionReason};

        let path = input.get("path").and_then(Value::as_str).unwrap_or("");
        if path.is_empty() {
            return None;
        }

        if let Some(store) = ctx.permission_store.as_ref() {
            match store.get_for_path("write_file", path) {
                Some(PolicyDecision::AlwaysDeny) | Some(PolicyDecision::Deny) => {
                    return Some(PermissionDecision::Deny {
                        message: format!(
                            "Write to '{}' is blocked by stored PathGlob policy.",
                            path
                        ),
                        reason: PermissionReason::StoredPolicy,
                    });
                }
                Some(PolicyDecision::AlwaysAllow) | Some(PolicyDecision::Allow) => {
                    return Some(PermissionDecision::Allow {
                        updated_input: None,
                        reason: PermissionReason::StoredPolicy,
                    });
                }
                None => {}
            }
        }

        None
    }
```

EditFileTool 中 `tool_name` 改为 `"edit_file"` 并从 `input.get("path")` 读取路径（或 `"file_path"` 字段，根据实际 schema 确认）。

验证命令（应当通过）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_file_path_glob_permission_test -- --nocapture
```

- [ ] **6-c Commit**

```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app
git add src-tauri/src/runtime/tools/builtin/ src-tauri/tests/review_file_path_glob_permission_test.rs
git commit -m "$(cat <<'EOF'
feat(permission): WriteFileTool and EditFileTool check PathGlob rules from PermissionStore

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7 — 确认 MCP 工具不绕过 authorize（回归测试）

- [ ] **7-a 写回归测试**

新建 `src-tauri/tests/review_mcp_no_bypass_permission_test.rs`：

```rust
//! McpRuntimeTool 不能自行绕过 ToolDispatcher 的 permission pipeline。
//! 通过 ToolDispatcher 调用 MCP 工具时，若 pipeline 返回 Deny，execute 不应被调用。

use std::sync::Arc;
use async_trait::async_trait;
use serde_json::{json, Value};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::definition::ToolDefinition;
use app_lib::runtime::tools::executor::{ToolError, ToolResult};
use app_lib::runtime::tools::permission::{
    PermissionDecision, PermissionPipeline, PermissionReason,
};
use app_lib::runtime::tools::{RuntimeTool, ToolDispatcher};

struct AlwaysDenyPipeline;

impl PermissionPipeline for AlwaysDenyPipeline {
    fn authorize(
        &self,
        _: &ToolDefinition,
        _: &Value,
        _: &ToolExecutionContext,
    ) -> PermissionDecision {
        PermissionDecision::Deny {
            message: "deny_all_for_test".into(),
            reason: PermissionReason::Other("test".into()),
        }
    }
}

struct PanickingMcpTool;

#[async_trait]
impl RuntimeTool for PanickingMcpTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("mcp__srv__panic_tool", "should not be called")
            .with_capability_scope(["mcp"])
    }

    async fn execute(&self, _: Value, _: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        panic!("execute() must not be called when permission is denied");
    }
}

#[tokio::test]
async fn review_mcp_tool_pipeline_deny_prevents_execute() {
    let dispatcher = ToolDispatcher::new(Arc::new(AlwaysDenyPipeline));
    dispatcher.register(Arc::new(PanickingMcpTool));

    let ctx = ToolExecutionContext::for_test("conv", "run", "tc");
    let result = dispatcher
        .dispatch("mcp__srv__panic_tool", json!({}), ctx)
        .await;

    assert!(
        matches!(result, Err(ToolError::PermissionDenied(_))),
        "Denied MCP tool must return PermissionDenied, not execute"
    );
}
```

验证命令（应当通过）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_mcp_no_bypass_permission_test -- --nocapture
```

- [ ] **7-b Commit**

```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app
git add src-tauri/tests/review_mcp_no_bypass_permission_test.rs
git commit -m "$(cat <<'EOF'
test(permission): add review test confirming MCP tools cannot bypass dispatcher permission pipeline

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8 — AgentStatus 补 Failed 变体

- [ ] **8-a 先写失败测试**

新建 `src-tauri/tests/review_agent_status_failed_test.rs`：

```rust
//! AgentStatus 必须有 Failed 变体，区分 Cancelled（用户取消）与 Failed（内部错误）。

use app_lib::runtime::agent::invocation::AgentStatus;

#[test]
fn review_agent_status_has_failed_variant() {
    let status = AgentStatus::Failed;
    assert!(matches!(status, AgentStatus::Failed));
}

#[test]
fn review_agent_status_cancelled_is_not_failed() {
    assert!(!matches!(AgentStatus::Cancelled, AgentStatus::Failed));
    assert!(!matches!(AgentStatus::Failed, AgentStatus::Cancelled));
}

#[test]
fn review_agent_status_failed_serializes() {
    let serialized = serde_json::to_string(&AgentStatus::Failed).expect("serialize");
    assert!(serialized.contains("failed") || serialized.contains("Failed"));
}
```

验证命令（应当失败）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_agent_status_failed_test 2>&1 | tail -20
```

- [ ] **8-b 修改 AgentStatus**

修改 `src-tauri/src/runtime/agent/invocation.rs`：

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Pending,
    Running,
    Completed,
    Cancelled,
    /// 子代理因内部错误（LLM 失败、工具错误等）提前终止，区别于 Cancelled（用户主动取消）。
    Failed,
}
```

在 `worker_runtime.rs` 中 LLM 错误分支（`output = format!("Sub-agent LLM error...")`）之后，若有 `agent_runtime.complete_run(...)` 调用，改为传入 `AgentStatus::Failed` 而非 `AgentStatus::Completed`。

验证命令（应当通过）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_agent_status_failed_test -- --nocapture
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo build 2>&1 | grep "^error" | head -20
```

- [ ] **8-c Commit**

```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app
git add src-tauri/src/runtime/agent/invocation.rs src-tauri/tests/review_agent_status_failed_test.rs
git commit -m "$(cat <<'EOF'
feat(agent): add AgentStatus::Failed to distinguish error termination from cancel

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9 — QueryEngine 注入 PermissionStore 到 ToolExecutionContext

- [ ] **9-a 先写失败测试**

新建 `src-tauri/tests/review_query_engine_permission_store_injection_test.rs`：

```rust
//! QueryEngine 必须能接受并向 ToolExecutionContext 注入 PermissionStore。

use std::sync::Arc;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::store::PermissionStore;

#[test]
fn review_query_engine_accepts_permission_store() {
    let store = Arc::new(PermissionStore::in_memory());
    // 如果 QueryEngine 没有 with_permission_store 方法，编译失败
    let _engine = QueryEngine::new().with_permission_store(store);
}
```

验证命令（应当失败）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_query_engine_permission_store_injection_test 2>&1 | tail -20
```

- [ ] **9-b 在 QueryEngine 追加 permission_store 字段与注入逻辑**

在 `src-tauri/src/runtime/query_engine.rs` 的 `QueryEngine` 结构体追加：

```rust
    /// 注入到每次 ToolExecutionContext 以支持 BashTool/FileTool 的细粒度规则查询。
    permission_store: Option<Arc<crate::runtime::store::PermissionStore>>,
```

在 `QueryEngine::new()` / `with_dispatcher()` 中初始化为 `None`，`clone_with_fresh_session_state` 中传递 `permission_store: self.permission_store.clone()`。

追加 builder 方法：

```rust
    pub fn with_permission_store(
        mut self,
        store: Arc<crate::runtime::store::PermissionStore>,
    ) -> Self {
        self.permission_store = Some(store);
        self
    }
```

在构造 `ToolExecutionContext` 的位置（搜索 `ToolExecutionContext::new` 或 `for_test` 的生产调用点），追加：

```rust
    .with_permission_store(store.clone())
```

（仅在 `self.permission_store.is_some()` 时调用。）

在 `session_runtime.rs` 的 `build_driver_for_turn` 或 `query_engine_for_session` 末尾，若 `self.permission_store.is_some()` 则：

```rust
    let session_engine = if let Some(ref store) = self.permission_store {
        session_engine.with_permission_store(store.clone())
    } else {
        session_engine
    };
```

验证命令（应当通过）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_query_engine_permission_store_injection_test -- --nocapture
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test 2>&1 | tail -30
```

- [ ] **9-c Commit**

```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app
git add src-tauri/src/runtime/query_engine.rs src-tauri/src/runtime/session_runtime.rs src-tauri/tests/review_query_engine_permission_store_injection_test.rs
git commit -m "$(cat <<'EOF'
feat(permission): QueryEngine injects PermissionStore into ToolExecutionContext

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10 — 全量回归与 review lock

- [ ] **10-a 综合约束测试**

新建 `src-tauri/tests/review_permission_alignment_constraints_test.rs`：

```rust
//! Plan-P Permission Alignment 综合约束回归测试。
//! 验证整体权限模型不变量，任何重构后必须继续通过。

use std::sync::Arc;
use app_lib::runtime::store::permission_store::{
    PermissionRule, PermissionScope, PermissionSource, PermissionStore, PolicyDecision,
};
use app_lib::runtime::tools::permission::{
    apply_permission_mode, default_permission_ask, PermissionDecision, PermissionDestination,
    PermissionMode, PermissionReason,
};

// ─── P-1: 三层规则优先级 ───────────────────────────────────────────────────────

#[test]
fn review_permission_session_overrides_all_layers() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::User,
        PermissionRule::simple(
            "bash",
            PermissionScope::Scope("workspace:write".into()),
            PolicyDecision::AlwaysDeny,
            PermissionSource::User,
        ),
    );
    store.record_to(
        PermissionDestination::Workspace,
        PermissionRule::simple(
            "bash",
            PermissionScope::Scope("workspace:write".into()),
            PolicyDecision::AlwaysDeny,
            PermissionSource::Workspace,
        ),
    );
    store.record_to(
        PermissionDestination::Session,
        PermissionRule::simple(
            "bash",
            PermissionScope::Scope("workspace:write".into()),
            PolicyDecision::Allow,
            PermissionSource::Session,
        ),
    );
    assert_eq!(
        store.get_for_scope("bash", "workspace:write"),
        Some(PolicyDecision::Allow),
        "session layer must override workspace and user"
    );
}

#[test]
fn review_permission_workspace_overrides_user_layer() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::User,
        PermissionRule::simple(
            "execute_python",
            PermissionScope::Scope("python:exec".into()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::User,
        ),
    );
    store.record_to(
        PermissionDestination::Workspace,
        PermissionRule::simple(
            "execute_python",
            PermissionScope::Scope("python:exec".into()),
            PolicyDecision::AlwaysDeny,
            PermissionSource::Workspace,
        ),
    );
    assert_eq!(
        store.get_for_scope("execute_python", "python:exec"),
        Some(PolicyDecision::AlwaysDeny),
        "workspace layer must override user layer"
    );
}

// ─── P-2: PermissionMode 语义 ─────────────────────────────────────────────────

#[test]
fn review_permission_mode_dont_ask_converts_ask_to_deny() {
    let (ro, dd) = default_permission_ask();
    let ask = PermissionDecision::Ask {
        message: "run?".into(),
        suggestions: vec![],
        remember_options: ro,
        default_destination: dd,
        reason: PermissionReason::UnknownScope,
    };
    assert!(matches!(
        apply_permission_mode(ask, "tool", PermissionMode::DontAsk),
        PermissionDecision::Deny { .. }
    ));
}

#[test]
fn review_permission_mode_plan_converts_ask_to_deny() {
    let (ro, dd) = default_permission_ask();
    let ask = PermissionDecision::Ask {
        message: "run?".into(),
        suggestions: vec![],
        remember_options: ro,
        default_destination: dd,
        reason: PermissionReason::UnknownScope,
    };
    assert!(
        matches!(
            apply_permission_mode(ask, "tool", PermissionMode::Plan),
            PermissionDecision::Deny { .. }
        ),
        "Plan mode must also deny Ask"
    );
}

#[test]
fn review_permission_mode_default_preserves_ask() {
    let (ro, dd) = default_permission_ask();
    let ask = PermissionDecision::Ask {
        message: "run?".into(),
        suggestions: vec![],
        remember_options: ro,
        default_destination: dd,
        reason: PermissionReason::UnknownScope,
    };
    assert!(matches!(
        apply_permission_mode(ask, "tool", PermissionMode::Default),
        PermissionDecision::Ask { .. }
    ));
}

// ─── P-3: PathGlob 匹配 ─────────────────────────────────────────────────────

#[test]
fn review_path_glob_wildcard_matching() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::Session,
        PermissionRule::simple(
            "write_file",
            PermissionScope::PathGlob("/workspace/**".into()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::Session,
        ),
    );
    assert_eq!(
        store.get_for_path("write_file", "/workspace/reports/2026/q1.csv"),
        Some(PolicyDecision::AlwaysAllow)
    );
    assert_eq!(store.get_for_path("write_file", "/etc/shadow"), None);
}

// ─── P-4: CommandPattern 匹配 ────────────────────────────────────────────────

#[test]
fn review_command_pattern_prefix_matching() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::Workspace,
        PermissionRule::simple(
            "bash",
            PermissionScope::CommandPattern("npm ".into()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::Workspace,
        ),
    );
    assert_eq!(
        store.get_for_command("bash", "npm install --save-dev"),
        Some(PolicyDecision::AlwaysAllow)
    );
    assert_eq!(store.get_for_command("bash", "pip install requests"), None);
}

// ─── P-5: MCP scope 走 Ask 路径 ─────────────────────────────────────────────

#[test]
fn review_mcp_scope_triggers_ask_via_store_pipeline() {
    use app_lib::runtime::tools::definition::ToolDefinition;
    use app_lib::runtime::tools::permission::{PermissionPipeline, StorePolicyPipeline};
    use app_lib::runtime::tools::ToolExecutionContext;
    use serde_json::json;

    let store = Arc::new(PermissionStore::in_memory());
    let pipeline = StorePolicyPipeline::new(store);
    let def = ToolDefinition::new("mcp__srv__my_tool", "mcp tool")
        .with_capability_scope(["mcp"]);
    let ctx = ToolExecutionContext::for_test("conv", "run", "tc");
    let decision = pipeline.authorize(&def, &json!({}), &ctx);
    assert!(
        matches!(decision, PermissionDecision::Ask { .. }),
        "MCP tool with no stored policy must trigger Ask"
    );
}
```

验证命令（应当全部通过）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test --test review_permission_alignment_constraints_test -- --nocapture
```

- [ ] **10-b 全量 Rust 回归**

```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo test 2>&1 | tail -40
```

预期：所有测试通过，无编译错误。

- [ ] **10-c 前端回归**

```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app && pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts 2>&1 | tail -20
```

- [ ] **10-d Commit**

```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app
git add src-tauri/tests/review_permission_alignment_constraints_test.rs
git commit -m "$(cat <<'EOF'
test(permission): add review_permission_alignment_constraints invariant regression tests

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## 验收标准

| 标准 | 验证方式 |
|---|---|
| Plan 模式下 Ask → Deny | `review_permission_plan_mode_test.rs` 全绿 |
| PathGlob / CommandPattern 存储与匹配 | `review_permission_store_glob_test.rs` 全绿 |
| WorkerRunConfig 有 `permission_mode` 字段 | `review_worker_run_config_permission_mode_test.rs` 全绿 |
| WorkerRunConfig 有 `control_plane` 字段 | `review_worker_ask_control_plane_test.rs` 全绿 |
| BashTool 查询 CommandPattern 规则 | `review_bash_command_pattern_permission_test.rs` 全绿 |
| 文件写工具查询 PathGlob 规则 | `review_file_path_glob_permission_test.rs` 全绿 |
| MCP 工具不绕过 dispatcher authorize | `review_mcp_no_bypass_permission_test.rs` 全绿 |
| AgentStatus 有 `Failed` 变体 | `review_agent_status_failed_test.rs` 全绿 |
| QueryEngine 注入 PermissionStore | `review_query_engine_permission_store_injection_test.rs` 全绿 |
| 综合约束全绿，无 cargo test 回归 | `review_permission_alignment_constraints_test.rs` + `cargo test` |
| 前端关键测试无回归 | `pnpm exec vitest run ...` |

---

## 遗留 TODO（超出本 Plan-P 范围）

- `PermissionAskDialog` 的 remember / destination 选项 UI 已在 `2026-04-18-plan-p-permission-ask-frontend.md` 覆盖
- glob 匹配目前使用手写递归，后续可替换为 `globset` crate 提升精确性与性能
- `SubAgentConfig` 的 `control_plane` 字段注入链（caller 端）：各 subagent 入口需显式传入 `pending_permission_store` 作为 control plane，此部分依赖各入口的重构
- 国际化：Plan 模式 Deny 消息目前为英文硬编码，后续统一到 i18n 体系
- `WriteFileTool` 的工具名在 `get_for_path` 调用中硬编码为 `"write_file"`，应替换为 `self.definition().id` 以避免工具重命名后的漂移
