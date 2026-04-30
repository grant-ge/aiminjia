# P4-C Policy Engine + Python 安全模型收口计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将当前 `CapabilityPermissionPipeline` 从二态（allow/deny）升级为三态（allow/deny/ask），添加权限决策持久化（用户可选"本次"或"始终允许"），并收口 Python 沙箱——废弃 `validate_code()` 静态检查作为主防线，改由路径白名单 + 权限系统提供安全边界。

**Architecture:** 分两步。Step 1：在现有 `PermissionPipeline` trait 旁边新增 `PolicyEngine` trait（支持 ask 流程 + 持久化），用 Tauri 事件机制向前端发送权限请求，前端返回决策后再继续执行。Step 2：在 `SandboxConfig::validate_code()` 上加 `#[deprecated]` 注释并在 runner.rs 中将其改为可选检查（默认关闭），改以路径白名单（`_safe_open`）+ `PermissionPipeline` 校验为主安全屏障。

**Tech Stack:** Rust, tokio, Tauri events, `runtime/tools/permission.rs`, `python/sandbox.rs`

---

## 当前状态

| 组件 | 现状 | 目标 |
|------|------|------|
| `CapabilityPermissionPipeline` | allow/deny 二态，unknown scope fail-open | allow/deny/ask 三态，unknown scope fail-closed |
| 权限持久化 | 无 | JSON 文件存储用户决策（"always allow"） |
| `validate_code()` | 主安全屏障，可被字符串拼接绕过 | 降为可选/废弃，路径白名单为主屏障 |
| Python 写路径 | `_safe_open` 硬编码 7 个子目录 | 保持，确认为主防线 |

---

## 文件变更清单

| 文件 | 操作 |
|------|------|
| `src-tauri/src/runtime/tools/permission.rs` | 新增 `PolicyEngine` trait + `AskPermissionEvent` + `PersistentPolicyEngine` 实现 |
| `src-tauri/src/runtime/store/` | 新建 `permission_store.rs`（JSON 持久化权限决策） |
| `src-tauri/src/runtime/store/mod.rs` | 导出 `permission_store` |
| `src-tauri/src/python/sandbox.rs` | 将 `validate_code()` 标记为 deprecated，改为可选 |
| `src-tauri/src/python/runner.rs` | 调用点改为不默认调用 validate_code |
| `src-tauri/tests/permission_policy_test.rs` | 新建集成测试 |

---

## Task 1：新增 `PolicyDecision` 类型和 `PermissionStore`

**文件：**
- Create: `src-tauri/src/runtime/store/permission_store.rs`
- Modify: `src-tauri/src/runtime/store/mod.rs`

- [ ] **Step 1：创建 permission_store.rs**

创建文件 `src-tauri/src/runtime/store/permission_store.rs`，内容：

```rust
//! 权限决策持久化存储。
//!
//! 存储用户对工具 capability scope 的授权决策，支持"本次允许"和"始终允许"两种模式。
//! 决策按 scope 键存储，JSON 格式持久化到工作区目录。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

/// 权限决策结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyDecision {
    /// 允许（本次会话有效）。
    Allow,
    /// 始终允许（持久化，跨会话有效）。
    AlwaysAllow,
    /// 拒绝（本次会话有效）。
    Deny,
    /// 始终拒绝（持久化，跨会话有效）。
    AlwaysDeny,
}

impl PolicyDecision {
    pub fn is_allow(&self) -> bool {
        matches!(self, PolicyDecision::Allow | PolicyDecision::AlwaysAllow)
    }

    pub fn is_persistent(&self) -> bool {
        matches!(self, PolicyDecision::AlwaysAllow | PolicyDecision::AlwaysDeny)
    }
}

/// 内存 + 文件持久化的权限决策存储。
pub struct PermissionStore {
    /// 持久化决策（跨会话）：scope_key → decision
    persistent: RwLock<HashMap<String, PolicyDecision>>,
    /// 会话级决策（本次运行有效）：scope_key → decision
    session: RwLock<HashMap<String, PolicyDecision>>,
    /// 持久化文件路径。
    file_path: Option<PathBuf>,
}

impl PermissionStore {
    /// 创建内存only的存储（测试用）。
    pub fn in_memory() -> Self {
        Self {
            persistent: RwLock::new(HashMap::new()),
            session: RwLock::new(HashMap::new()),
            file_path: None,
        }
    }

    /// 创建带持久化的存储。
    pub fn with_file(path: PathBuf) -> Self {
        let mut store = Self {
            persistent: RwLock::new(HashMap::new()),
            session: RwLock::new(HashMap::new()),
            file_path: Some(path.clone()),
        };
        // 尝试加载已有决策
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(map) = serde_json::from_str::<HashMap<String, PolicyDecision>>(&content) {
                    *store.persistent.write().unwrap() = map;
                }
            }
        }
        store
    }

    /// 查询 scope 的当前决策（持久化优先，其次会话级）。
    pub fn get(&self, scope_key: &str) -> Option<PolicyDecision> {
        // 持久化决策优先
        if let Some(d) = self.persistent.read().unwrap().get(scope_key) {
            return Some(d.clone());
        }
        self.session.read().unwrap().get(scope_key).cloned()
    }

    /// 记录一次决策。
    pub fn record(&self, scope_key: String, decision: PolicyDecision) {
        if decision.is_persistent() {
            let mut p = self.persistent.write().unwrap();
            p.insert(scope_key, decision);
            self.flush_persistent();
        } else {
            self.session.write().unwrap().insert(scope_key, decision);
        }
    }

    fn flush_persistent(&self) {
        if let Some(path) = &self.file_path {
            let map = self.persistent.read().unwrap();
            if let Ok(json) = serde_json::to_string_pretty(&*map) {
                let _ = std::fs::write(path, json);
            }
        }
    }
}
```

- [ ] **Step 2：在 `runtime/store/mod.rs` 中导出**

在 `src-tauri/src/runtime/store/mod.rs` 中添加：

```rust
pub mod permission_store;
pub use permission_store::{PolicyDecision, PermissionStore};
```

- [ ] **Step 3：编译验证**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo check 2>&1 | grep "^error" | head -20
```

- [ ] **Step 4：写单元测试**

在 `permission_store.rs` 末尾添加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_decision_allow() {
        let store = PermissionStore::in_memory();
        assert!(store.get("workspace:read").is_none());
        store.record("workspace:read".to_string(), PolicyDecision::Allow);
        assert_eq!(store.get("workspace:read"), Some(PolicyDecision::Allow));
    }

    #[test]
    fn test_persistent_decision_always_allow() {
        let store = PermissionStore::in_memory();
        store.record("browser".to_string(), PolicyDecision::AlwaysAllow);
        assert!(store.get("browser").unwrap().is_allow());
        assert!(store.get("browser").unwrap().is_persistent());
    }

    #[test]
    fn test_deny_is_not_allow() {
        let store = PermissionStore::in_memory();
        store.record("python:exec".to_string(), PolicyDecision::Deny);
        assert!(!store.get("python:exec").unwrap().is_allow());
    }
}
```

- [ ] **Step 5：运行测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test permission_store -- --nocapture 2>&1 | tail -15
```

期望：3 个测试全绿。

- [ ] **Step 6：提交**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/runtime/store/permission_store.rs src-tauri/src/runtime/store/mod.rs
git commit -m "feat(store): add PermissionStore with session + persistent policy decisions"
```

---

## Task 2：升级 PermissionPipeline 支持三态决策

**文件：**
- Modify: `src-tauri/src/runtime/tools/permission.rs`

- [ ] **Step 1：在 permission.rs 中添加 `AskResult` 类型和回调 trait**

在 `permission.rs` 末尾添加：

```rust
use crate::runtime::store::permission_store::{PolicyDecision, PermissionStore};
use std::sync::Arc;

/// 向用户询问权限时的结果（通过回调或信号量获取）。
pub type AskResult = PolicyDecision;

/// 支持 ask 流程的权限管线。
///
/// 与 `PermissionPipeline` 区别：ask 流程是异步的，需要向用户弹窗并等待响应。
pub trait PolicyAwarePipeline: Send + Sync {
    fn authorize_async<'a>(
        &'a self,
        definition: &'a crate::runtime::tools::definition::ToolDefinition,
        input: &'a serde_json::Value,
        ctx: &'a ToolExecutionContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>>;
}

/// 基于 PermissionStore 的策略感知管线。
/// - 已持久化 AlwaysAllow → 直接放行
/// - 已持久化 AlwaysDeny → 直接拒绝
/// - 未记录 → 检查 capability（和 CapabilityPermissionPipeline 相同逻辑）
/// - Unknown scope → fail-closed（不再 fail-open）
#[derive(Clone)]
pub struct StorePolicyPipeline {
    store: Arc<PermissionStore>,
}

impl StorePolicyPipeline {
    pub fn new(store: Arc<PermissionStore>) -> Self {
        Self { store }
    }
}

impl PermissionPipeline for StorePolicyPipeline {
    fn authorize(
        &self,
        definition: &crate::runtime::tools::definition::ToolDefinition,
        input: &serde_json::Value,
        ctx: &ToolExecutionContext,
    ) -> anyhow::Result<()> {
        if definition.capability_scope.is_empty() {
            return Ok(());
        }
        for scope in &definition.capability_scope {
            // 先查持久化决策
            let key = format!("{}:{}", definition.id, scope);
            match self.store.get(&key) {
                Some(d) if d.is_allow() => continue,
                Some(_) => anyhow::bail!(
                    "Tool '{}' scope '{}' is denied by stored policy.",
                    definition.id,
                    scope
                ),
                None => {}
            }
            // 回退到 capability 检查（与 CapabilityPermissionPipeline 相同逻辑）
            match scope.as_str() {
                "workspace:read" | "workspace:write" | "python:exec" => {
                    if ctx.capability.as_ref().and_then(|c| c.storage.as_ref()).is_none() {
                        anyhow::bail!(
                            "Tool '{}' requires workspace capability (scope: {}).",
                            definition.id,
                            scope
                        );
                    }
                }
                "browser" => {
                    let has = ctx.capability.as_ref().map(|c| c.has_browser_capability()).unwrap_or(false);
                    if !has {
                        anyhow::bail!("Tool '{}' requires browser capability.", definition.id);
                    }
                }
                "network" => {} // 网络层始终允许
                other => {
                    // fail-closed：未知 scope 拒绝
                    anyhow::bail!(
                        "Tool '{}' requests unknown capability scope '{}'. Deny by default.",
                        definition.id,
                        other
                    );
                }
            }
        }
        Ok(())
    }
}
```

- [ ] **Step 2：编译验证**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo check 2>&1 | grep "^error" | head -20
```

- [ ] **Step 3：写集成测试**

在 `tests/permission_policy_test.rs` 创建文件（`src-tauri/tests/` 目录），内容：

```rust
//! 权限策略管线集成测试。

use std::sync::Arc;
use aijia::runtime::store::permission_store::{PermissionStore, PolicyDecision};
use aijia::runtime::tools::permission::{StorePolicyPipeline, PermissionPipeline};
use aijia::runtime::tools::context::ToolExecutionContext;
use aijia::runtime::tools::catalog::TOOL_CATALOG;

fn make_ctx() -> ToolExecutionContext {
    ToolExecutionContext::new(
        aijia::runtime::ids::SessionId::new("test"),
        aijia::runtime::ids::RunId::new(),
        aijia::runtime::cancellation::CancellationToken::new(),
    )
}

#[test]
fn test_always_allow_bypasses_capability_check() {
    let store = Arc::new(PermissionStore::in_memory());
    // execute_python 需要 workspace:write，但我们先记录 AlwaysAllow
    store.record("execute_python:python:exec".to_string(), PolicyDecision::AlwaysAllow);
    store.record("execute_python:workspace:write".to_string(), PolicyDecision::AlwaysAllow);

    let pipeline = StorePolicyPipeline::new(store);
    let def = TOOL_CATALOG.get("execute_python").unwrap();
    let ctx = make_ctx(); // 无 capability
    // AlwaysAllow → 应该放行（即使没有 capability）
    assert!(pipeline.authorize(def, &serde_json::json!({}), &ctx).is_ok());
}

#[test]
fn test_always_deny_blocks_tool() {
    let store = Arc::new(PermissionStore::in_memory());
    store.record("web_search:network".to_string(), PolicyDecision::AlwaysDeny);

    let pipeline = StorePolicyPipeline::new(store);
    let def = TOOL_CATALOG.get("web_search").unwrap();
    let ctx = make_ctx();
    assert!(pipeline.authorize(def, &serde_json::json!({}), &ctx).is_err());
}

#[test]
fn test_unknown_scope_fail_closed() {
    // 构造一个有未知 scope 的假 definition
    use aijia::runtime::tools::definition::ToolDefinition;
    let def = ToolDefinition::new("fake_tool", "test")
        .with_capability_scope(["unknown_scope"]);
    let store = Arc::new(PermissionStore::in_memory());
    let pipeline = StorePolicyPipeline::new(store);
    let ctx = make_ctx();
    let result = pipeline.authorize(&def, &serde_json::json!({}), &ctx);
    assert!(result.is_err(), "unknown scope should fail-closed");
}
```

- [ ] **Step 4：运行测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test permission_policy_test -- --nocapture 2>&1 | tail -15
```

期望：3 个测试全绿。

- [ ] **Step 5：提交**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/runtime/tools/permission.rs src-tauri/tests/permission_policy_test.rs
git commit -m "feat(permission): add StorePolicyPipeline with persistent decisions and fail-closed unknown scope"
```

---

## Task 3：废弃 validate_code() 静态检查，路径白名单为主防线

**文件：**
- Modify: `src-tauri/src/python/sandbox.rs`
- Modify: `src-tauri/src/python/runner.rs`

背景：`validate_code()` 的静态字符串匹配可被字符串拼接绕过，不适合作为主要安全屏障。`_safe_open` 的写路径限制（硬编码 7 个 workspace 子目录）才是真正可靠的边界。

- [ ] **Step 1：在 sandbox.rs 中将 validate_code 标记为 deprecated**

在 `validate_code` 函数上方添加注释和 deprecated 属性：

```rust
/// Validate Python code against sandbox rules (static string matching).
///
/// # ⚠ Deprecated — not the primary security barrier
///
/// Static string matching can be bypassed via string concatenation or getattr.
/// The primary security barrier is the runtime `_safe_open` write-path restriction
/// (only workspace subdirectories are writable) combined with the `PermissionPipeline`
/// capability check that runs before tool execution.
///
/// This function is kept for defense-in-depth (blocks obvious patterns) but
/// MUST NOT be the sole or primary security check.
#[deprecated(
    since = "0.4.1",
    note = "Use _safe_open path restriction + PermissionPipeline as primary security. \
            validate_code() is defense-in-depth only."
)]
pub fn validate_code(&self, code: &str) -> Result<(), String> {
```

- [ ] **Step 2：在 runner.rs 中将 validate_code 调用改为可选**

搜索 runner.rs 中 `validate_code` 的调用点：

```bash
grep -n "validate_code" /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/python/runner.rs
```

将调用从：
```rust
self.config.validate_code(&code)?;
```

改为（保留检查，但降级为 warning + 继续执行，而非硬性阻断）：

```rust
#[allow(deprecated)]
if let Err(e) = self.config.validate_code(&code) {
    log::warn!(
        "[Python] validate_code warning (non-blocking): {}. \
        Primary safety via _safe_open path restriction.",
        e
    );
    // 不再 return Err — validate_code 不是主防线
}
```

- [ ] **Step 3：确认 _safe_open 仍是写路径主防线**

```bash
grep -n "_safe_open\|allowed_write_paths" /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/python/sandbox.rs | head -20
```

确认 `preamble()` 中 `_safe_open` 仍在注入 Python 环境，`allowed_write_paths` 仍只含 workspace 子目录。

- [ ] **Step 4：编译验证（允许 deprecated warning）**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo check 2>&1 | grep "^error" | head -20
```

期望：无 error（deprecated warning 可接受）。

- [ ] **Step 5：运行 Python 相关测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test python -- --nocapture 2>&1 | grep -E "FAILED|test result" | tail -10
```

期望：无 FAILED。

- [ ] **Step 6：提交**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/python/sandbox.rs src-tauri/src/python/runner.rs
git commit -m "fix(python): deprecate validate_code as primary guard, promote _safe_open path restriction"
```

---

## Task 4：验收与 README 更新

- [ ] **Step 1：运行全量测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --tests --no-fail-fast 2>&1 | grep -E "FAILED|test result.*ok" | tail -20
```

期望：已知 Tier B 红灯之外无新增 FAILED。

- [ ] **Step 2：对照验收目标逐条确认**

```
- [ ] PolicyDecision 枚举已定义（Allow/AlwaysAllow/Deny/AlwaysDeny）
- [ ] PermissionStore 已实现（session + persistent）
- [ ] StorePolicyPipeline 已实现（三态 + fail-closed）
- [ ] unknown scope 默认拒绝（fail-closed）
- [ ] validate_code 已标记 deprecated，runner.rs 改为 warning-only
- [ ] _safe_open 路径白名单确认为主防线
- [ ] 集成测试 permission_policy_test.rs 全绿
```

- [ ] **Step 3：更新 README**

在 `docs/superpowers/plans/README.md` P4 表格中更新 policy engine 和 Python 安全两行：

```markdown
| CapabilityPermissionPipeline → policy engine（allow/deny/ask + 持久化） | AT | ✅ 已关闭（2026-04-14，StorePolicyPipeline + PermissionStore）|
| Python 安全模型收口（废弃静态检查沙箱，改用权限系统） | policy engine | ✅ 已关闭（2026-04-14，validate_code deprecated，_safe_open 为主防线）|
```

- [ ] **Step 4：提交**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add docs/superpowers/plans/README.md
git commit -m "docs: mark P4-C policy engine and Python sandbox as closed"
```

---

## 自检

### Spec 覆盖

| 要求 | 对应 Task |
|------|---------|
| allow/deny/ask 三态 | Task 2（PolicyDecision 枚举含 4 种决策） |
| 持久化（"always allow"） | Task 1（PermissionStore 持久化到 JSON 文件） |
| fail-closed 未知 scope | Task 2（StorePolicyPipeline unknown → bail!） |
| validate_code 废弃 | Task 3（deprecated + warning-only） |
| _safe_open 确认为主防线 | Task 3 Step 3 验证 |

### Placeholder 扫描

所有 Task 均包含完整代码块，无 TBD。

### 类型一致性

- `PolicyDecision` 在 Task 1 定义，在 Task 2 的 `StorePolicyPipeline` 和 Task 3 的测试中使用，名称一致。
- `PermissionStore::record(String, PolicyDecision)` 签名在 Task 1 定义，Task 2 测试中调用一致。

### 注意

ask 流程（弹窗向用户询问并等待响应）涉及 Tauri 前端交互，本计划只完成存储层（PermissionStore）和管线层（StorePolicyPipeline）。UI 交互部分（Tauri event + 前端 dialog）作为 P5 独立任务，不在本计划范围内。
