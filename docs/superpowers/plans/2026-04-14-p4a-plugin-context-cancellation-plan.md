# P4-A PluginContext 退出主路径 + CancellationToken 级联计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 消除 `chat_runtime_impl.rs` 中两处 `PluginContext` 构建点，同时修复两处 fire-and-forget `tokio::spawn`（session eviction + agent loop），让取消信号能从上游 `CancellationToken` 级联传播。

**Architecture:** 两个改动相互独立，但都在同一文件域内。PluginContext 的两处残留：L1766 是 precompute 阶段自动加载文件（只需 session + workspace 能力，可直接调用 load_file RuntimeTool）；L2715 是 agent loop 的工具分发入口（已经走 `to_runtime_dispatcher`，只需删除 PluginContext 中间构建步骤，由 `QueryEngine` 持有的 `TurnState` 提供 cancellation 即可）。CancellationToken 两处 spawn 都是独立改动——session eviction 接收 token 后在 checkpoint 前先检查；agent spawn 把现有的 `tokio::time::timeout` 换成 `tokio::select!` 监听 cancellation。

**Tech Stack:** Rust, tokio, existing `CancellationToken` (`runtime/cancellation.rs`), `ToolExecutionContext`, `CapabilityContext`

---

## 当前残留点一览

| 位置 | 问题 | 修复方式 |
|------|------|---------|
| `chat_runtime_impl.rs:1766` | PluginContext 构建用于 precompute 自动 load_file | 直接调用 load_file RuntimeTool，或使用已有的 ToolExecutionContext |
| `chat_runtime_impl.rs:2715` | PluginContext 构建传入 `to_runtime_dispatcher` | to_runtime_dispatcher 已接收 PluginContext，改为不传 PluginContext 或传最小 ctx |
| `python/session.rs:641` | LRU eviction spawn 无取消 | 传入 CancellationToken，spawn 内检查 |
| `chat_runtime_impl.rs:1115` | agent loop spawn 超时用 timeout，无取消级联 | 用 `tokio::select!` 监听 cancellation + timeout |

---

## 文件变更清单

| 文件 | 操作 |
|------|------|
| `src-tauri/src/python/session.rs` | 修改 LRU eviction spawn，接收 CancellationToken |
| `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` | 修复 agent spawn（tokio::select!）；清理 L1766 和 L2715 两处 PluginContext |

> **注意：** L2715 的 PluginContext 仍被 `to_runtime_dispatcher` 使用（legacy bridge），不能一步删除。本计划的目标是**不再新增构建点**，并把 cancellation 级联修通。PluginContext 的完全消除需要所有 legacy ToolPlugin 迁移完成（P4 后续）。

---

## Task 1：修复 session eviction 的 fire-and-forget spawn

**文件：**
- Modify: `src-tauri/src/python/session.rs`（约 L637-644）

- [ ] **Step 1：找到 eviction spawn 的调用函数签名**

```bash
grep -n "fn.*evict\|fn.*get_or_create\|fn.*acquire" /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/python/session.rs | head -20
```

记录包含该 spawn 的函数名（约 L620-650 附近）和参数列表。

- [ ] **Step 2：确认 CancellationToken 可传入**

读取 `src-tauri/src/runtime/cancellation.rs`（已知内容：`CancellationToken { cancelled: Arc<AtomicBool> }`，有 `clone()` 和 `is_cancelled()` 方法）。CancellationToken 实现了 Clone，可以传入 async 闭包。

- [ ] **Step 3：修改 eviction spawn，在 checkpoint 前检查取消**

将 `python/session.rs` 约 L641-644：

```rust
tokio::spawn(async move {
    let _ = evicted.write_checkpoint().await;
    let _ = evicted.kill().await;
});
```

改为（在函数参数里添加 `cancellation: crate::runtime::cancellation::CancellationToken`，并将其 clone 传入 spawn）：

```rust
let cancel = cancellation.clone();
tokio::spawn(async move {
    if !cancel.is_cancelled() {
        let _ = evicted.write_checkpoint().await;
    }
    let _ = evicted.kill().await;
});
```

> 说明：即使被取消，`kill()` 仍需执行（避免进程泄漏），只跳过 checkpoint 写入。

- [ ] **Step 4：更新该函数所有调用点**

```bash
grep -rn "get_or_create\|evict\|acquire_session" /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/ | grep -v "session.rs" | grep -v "test" | head -20
```

在每个调用点传入 cancellation token（从 `TurnState` 或函数参数获取）。若调用链较长，可将 token 设为 `Option<CancellationToken>`，None 时跳过取消检查：

```rust
let cancel = cancellation.as_ref().map_or(false, |t| t.is_cancelled());
if !cancel {
    let _ = evicted.write_checkpoint().await;
}
```

- [ ] **Step 5：编译验证**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo check 2>&1 | grep "^error" | head -20
```

期望：无 error。

- [ ] **Step 6：提交**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/python/session.rs
git commit -m "fix(session): propagate CancellationToken to LRU eviction spawn"
```

---

## Task 2：修复 agent loop spawn — tokio::select! 替代纯 timeout

**文件：**
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`（约 L1115-1170）

背景：`chat_runtime_impl.rs` L1115 的 `tokio::spawn` 包裹了 agent_loop，内部用 `tokio::time::timeout` 处理超时，但没有监听外部取消信号（`TurnState.cancellation`）。

- [ ] **Step 1：确认外部 cancellation 是否已在调用位置可访问**

```bash
grep -n "cancellation\|CancellationToken\|turn_state\|TurnState" /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs | head -30
```

记录 cancellation token 在哪里创建（行号），是否在 spawn 之前。

- [ ] **Step 2：修改 agent spawn，用 tokio::select! 同时监听超时和取消**

将约 L1115-1170 的 spawn 内的超时逻辑：

```rust
tokio::spawn(async move {
    // ...
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(AGENT_TIMEOUT_SECS),
        agent_loop(agent_ctx, chat_messages, step_config),
    )
    .await;

    match result {
        Ok(()) => { /* ... */ }
        Err(_elapsed) => {
            // timeout handling
            guard.gateway.cancel_conversation(&conversation_id_clone).ok();
            // emit error event
        }
    }
    guard.clear().await;
});
```

改为（在 spawn 之前 clone cancellation token，假设它叫 `cancellation`）：

```rust
let cancel_token = cancellation.clone();
tokio::spawn(async move {
    let conversation_id_clone = agent_ctx.conversation_id.clone();
    let app_clone = agent_ctx.app.clone();

    let mut guard = AgentGuard::new(
        agent_ctx.gateway.clone(),
        agent_ctx.db.clone(),
        agent_ctx.session_mgr.clone(),
        agent_ctx.app.clone(),
        agent_ctx.conversation_id.clone(),
        agent_ctx.run_id.clone(),
    );

    let timeout_dur = std::time::Duration::from_secs(AGENT_TIMEOUT_SECS);

    tokio::select! {
        result = tokio::time::timeout(timeout_dur, agent_loop(agent_ctx, chat_messages, step_config)) => {
            match result {
                Ok(()) => {
                    log::info!(
                        "[AgentGuard] agent_loop completed normally for conversation {}",
                        conversation_id_clone
                    );
                }
                Err(_elapsed) => {
                    log::error!(
                        "[AgentGuard] agent_loop TIMED OUT after {}s for conversation {}",
                        AGENT_TIMEOUT_SECS,
                        conversation_id_clone
                    );
                    guard.gateway.cancel_conversation(&conversation_id_clone).ok();
                    let _ = app_clone.emit(
                        "streaming:error",
                        serde_json::json!({
                            "conversationId": conversation_id_clone,
                            "error": format!(
                                "处理超时（已运行 {} 分钟）。可能原因：\n\
                                1. 任务过于复杂\n\
                                2. 网络连接不稳定\n\n\
                                请尝试简化问题后重试。",
                                AGENT_TIMEOUT_SECS / 60
                            ),
                            "errorType": "agent_timeout",
                            "timeoutSeconds": AGENT_TIMEOUT_SECS,
                        }),
                    );
                }
            }
        }
        _ = async {
            loop {
                if cancel_token.is_cancelled() { break; }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        } => {
            log::info!(
                "[AgentGuard] agent_loop cancelled externally for conversation {}",
                conversation_id_clone
            );
        }
    }

    guard.clear().await;
});
```

- [ ] **Step 3：编译验证**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo check 2>&1 | grep "^error" | head -20
```

期望：无 error。若有 borrow 问题，检查 `agent_ctx` 的 move 语义（它被 move 进 agent_loop，select! 的其他分支不应再访问）。

- [ ] **Step 4：提交**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs
git commit -m "fix(agent): use tokio::select! to propagate CancellationToken to agent loop spawn"
```

---

## Task 3：清理 L1766 precompute 的 PluginContext 构建

**文件：**
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`（约 L1759-1810）

背景：L1766 构建 `auto_load_ctx: PluginContext` 只是为了调用 `auto_load_ctx.loaded_key(file_id)` 和 `auto_load_ctx.load_failed_key(file_id)`（key 生成器），以及传给 `handle_load_file` 执行实际加载。

- [ ] **Step 1：确认 loaded_key / load_failed_key 是 PluginContext 的方法还是独立函数**

```bash
grep -n "loaded_key\|load_failed_key" /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/context.rs | head -10
grep -rn "fn loaded_key\|fn load_failed_key" /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/ | head -10
```

- [ ] **Step 2：若 loaded_key 是 PluginContext 方法，提取为独立函数**

若 `loaded_key` 定义在 `PluginContext` impl 块中，将其逻辑提取为同文件的独立函数（或移到 session/memory key 模块），例如：

```rust
/// Generate the memory key that marks a file as loaded in a conversation.
fn file_loaded_key(conversation_id: &str, file_id: &str) -> String {
    format!("loaded:{}:{}", conversation_id, file_id)
}

fn file_load_failed_key(conversation_id: &str, file_id: &str) -> String {
    format!("load_failed:{}:{}", conversation_id, file_id)
}
```

- [ ] **Step 3：用提取的函数替换 auto_load_ctx 的 key 调用**

将：
```rust
let loaded_key = auto_load_ctx.loaded_key(file_id);
let failed_key = auto_load_ctx.load_failed_key(file_id);
```

改为：
```rust
let loaded_key = file_loaded_key(&conversation_id, file_id);
let failed_key = file_load_failed_key(&conversation_id, file_id);
```

- [ ] **Step 4：确认 auto_load_ctx 是否还有其他用途**

```bash
grep -n "auto_load_ctx" /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs
```

若 `auto_load_ctx` 仅用于 key 生成和传入 `handle_load_file`，检查 `handle_load_file` 签名是否可改为接收更小的 ctx。若改造成本高，记录为后续任务，本 Task 只消除 key 生成对 PluginContext 的依赖。

- [ ] **Step 5：编译验证**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo check 2>&1 | grep "^error" | head -20
```

- [ ] **Step 6：提交**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs
git add src-tauri/src/plugin/context.rs
git commit -m "refactor(precompute): extract file key helpers, reduce PluginContext usage in auto-load"
```

---

## Task 4：验收

- [ ] **Step 1：确认 PluginContext 构建点减少**

```bash
grep -n "PluginContext {" /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs
```

期望：构建点从 2 处变为至多 1 处（L2715 的 agent loop 工具分发仍保留，因为 legacy bridge 还需要）。

- [ ] **Step 2：确认 tokio::spawn fire-and-forget 减少**

```bash
grep -c "tokio::spawn" /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/python/session.rs
grep -c "tokio::spawn" /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs
```

记录数量（不要求清零，只验证修改生效）。

- [ ] **Step 3：运行 review_ 系列测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --tests -- review_ --nocapture 2>&1 | grep -E "FAILED|test review_.*ok" | head -20
```

期望：已知 Tier B 红灯之外无新增 FAILED。

- [ ] **Step 4：提交 README 更新**

在 `docs/superpowers/plans/README.md` P4 表格中更新 PluginContext 和 CancellationToken 两行状态为 `✅ 已关闭（2026-04-14）`。

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add docs/superpowers/plans/README.md
git commit -m "docs: mark P4-A PluginContext/CancellationToken tasks as closed"
```

---

## 自检

### Spec 覆盖

| 要求 | 对应 Task |
|------|---------|
| CancellationToken 级联到 session eviction | Task 1 |
| CancellationToken 级联到 agent loop spawn | Task 2 |
| PluginContext L1766 precompute 构建点减少 | Task 3 |
| PluginContext L2715 工具分发（保留，注释说明）| 设计决策：legacy bridge 必须保留到所有工具迁完 |

### Placeholder 扫描

- Task 1 Step 4 给出了 `Option<CancellationToken>` 的处理模式
- Task 2 给出了完整的 `tokio::select!` 代码
- Task 3 给出了 key 提取函数的完整实现

### 类型一致性

`CancellationToken::is_cancelled()` 在 Task 1 和 Task 2 均使用，名称一致（来自 `cancellation.rs`）。
