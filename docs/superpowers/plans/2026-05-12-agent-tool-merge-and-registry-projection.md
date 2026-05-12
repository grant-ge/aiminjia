# Agent 工具协议合并 & AgentRegistry 索引 Employee 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 spawn_subagent 的 `subagent_type` / `employee_id` 互斥参数合并为单一 `subagent_type`（emp-id 也是一种 subagent_type）；EmployeeStore 索引进 AgentRegistry 让派活只查一处；AgentDefinition 加 `origin` 枚举显式区分来源（不再依赖任何字符串前缀启发式）。彻底解决 LLM 用 `subagent_type:"general-purpose" + team_name` 派"伪 Teammate"的幻觉问题。

**Architecture:**
- **AgentDefinition 加 `origin: AgentSource` 字段**（Builtin / UserMarkdown / Employee），是"这个 agent 来自哪里"的唯一真值
- **AgentRegistry 是派活唯一查询入口**：内部 RwLock，启动时把 builtin + user markdown + 所有 Active Employee（投影后）灌进去；employee hire/update/archive 通过 `EmployeeAgentSync` 钩子实时同步
- **EmployeeStore 仍是 employee 业务实体的 source of truth**——cron / lifecycle / resource_config / last_run_at 留在它，AgentRegistry 是它的"派活视图"。**EmployeeStore 不删，不做数据迁移**
- **spawn_subagent 简化**：删 `employee_id` 字段、删 `AgentSource` 枚举、删 `employee_store` 字段；只 `registry.get(&subagent_type)`；派 Teammate 时用 `origin == Employee` 判定要不要写 `transcript.employee_id`
- **未知 subagent_type 错误信息回灌当前可选清单**给 LLM（对齐 claude-code-best 的 `AgentTool.tsx:532-536`）

**Tech Stack:** Rust 2021，tokio + async_trait，serde_json，tempfile（测试）。Tauri 后端，前端 React/TS 不需要改动（前端走 `runEmployeeOnDemand` 等专用 Tauri 命令，不直接调 Agent 工具）。

---

## 文件结构

- **修改** `src-tauri/src/runtime/agent/definition.rs` — 新增 `AgentSource` 枚举 + `AgentDefinition.origin` 字段
- **修改** `src-tauri/src/runtime/agent/builtin/*.rs`（3 个）— 构造点补 `source: AgentSource::Builtin`
- **修改** `src-tauri/src/runtime/agent/markdown_loader.rs:109` — 构造点补 `source: AgentSource::User`
- **修改** `src-tauri/src/runtime/agent/registry.rs` — 内部 RwLock；新增 `register_dynamic` / `unregister`（builtin 保护）；`get` / `list` 返 owned
- **修改** `src-tauri/src/runtime/agent/registry_loader.rs` — 跟随 registry 接口变化（`&mut` → `&`）
- **新增** `src-tauri/src/runtime/agent/employee_projection.rs` — `project_employee_to_agent` 投影函数 + `EmployeeAgentSync` trait + `AgentRegistrySync` / `NoopSync` 实现 + `seed_registry_from_employees`
- **修改** `src-tauri/src/runtime/agent/mod.rs` — `pub mod employee_projection;`
- **修改** `src-tauri/src/runtime/employee/store.rs` — 加 `sync: RwLock<Option<Arc<dyn EmployeeAgentSync>>>` 字段；`set_sync` 方法；`create / update / purge / purge_if_archived_older_than / purge_old_archived` 末尾调钩子
- **修改** `src-tauri/src/lib.rs:349-380` — 启动时 `Arc<EmployeeStore>`，seed AgentRegistry，wire sync 钩子，`app.manage(employee_store_arc)`
- **修改** `src-tauri/src/runtime/tools/builtin/spawn_subagent.rs` — 删 `employee_id` 解析、`AgentSource`、`employee_store` 字段；`origin == Employee` 判定 `employee_id`；新增 `build_unknown_subagent_type_error`；`render_dispatch_catalog` 改单段
- **修改** `src-tauri/src/runtime/tools/catalog.rs:330-380` — Agent input_schema 删 `employee_id`，更新 description
- **修改** `src-tauri/src/runtime/tools/description_context.rs` — 删 `EmployeeSummary` / `ToolDescriptionContext.employees`；`AgentDefSummary` 加 `origin` 字段
- **修改** `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:88-155, 188-238` — `build_tool_description_context` 不再单独读 EmployeeStore；`build_request_scoped_tool_overrides` 用单例
- **修改** `src-tauri/src/transport/tauri_commands/chat.rs:1438` — 注释/diagnostic 标记同步改名
- **修改** `src-tauri/src/llm/providers/claude.rs:383, 408` — diagnostic 标记同步改名
- **修改** `src-tauri/src/llm/tool_executor/spawn_subagent.rs:90-99` — launcher 兜底错误回灌清单
- **修改** `src-tauri/tests/spawn_teammate_via_employee_test.rs` — 删互斥测试，改 emp-id 当 subagent_type 的 happy path
- **修改** `src-tauri/src/transport/tauri_commands/employee*.rs` 等 — 把 `EmployeeStore::new(...)` 替换为从 app state 拿 `Arc<EmployeeStore>`
- **修改** `CLAUDE.md` + `docs/superpowers/plans/README.md` — 文档更新

---

## Task 1: AgentDefinition 加 `origin: AgentSource` 字段

**Files:**
- Modify: `src-tauri/src/runtime/agent/definition.rs`
- Modify: `src-tauri/src/runtime/agent/builtin/daily_assistant_agent.rs`
- Modify: `src-tauri/src/runtime/agent/builtin/explore.rs`
- Modify: `src-tauri/src/runtime/agent/builtin/general_purpose.rs`
- Modify: `src-tauri/src/runtime/agent/markdown_loader.rs:109`

- [ ] **Step 1: 写失败测试 — origin 字段存在且 builtin 正确**

在 `src-tauri/src/runtime/agent/definition.rs` 末尾追加测试：

```rust
    #[test]
    fn agent_origin_distinguishes_three_sources() {
        let b = AgentDefinition {
            name: "x".into(),
            description: "y".into(),
            allowed_tools: vec![],
            disallowed_tools: vec![],
            max_iterations: 10,
            model: AgentModel::Inherit,
            system_prompt: AgentPrompt::Inline("p".into()),
            source: AgentSource::Builtin,
            permission_mode: AgentPermissionMode::Bubble,
            background_default: false,
            source: AgentSource::Builtin,
        };
        assert_eq!(b.origin, AgentSource::Builtin);
        assert_ne!(b.origin, AgentSource::User);
        assert_ne!(b.origin, AgentSource::Employee);
    }
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cd src-tauri && cargo test --lib runtime::agent::definition -- --nocapture`

Expected: FAIL（编译错误"`AgentSource` not found" / "`origin` field missing"）。

- [ ] **Step 3: 实现 `AgentSource` 枚举 + 字段**

在 `definition.rs` 现有 `AgentSource` 枚举下方追加：

```rust
/// 派活时区分 agent 来源——builtin / 用户 markdown / 由 EmployeeStore 投影。
///
/// 用途：spawn_subagent 派 Teammate 时根据 origin 决定是否在 transcript meta
/// 中带上 `employee_id` 字段，下游 agenda runner / Team 视图 / inbox 仍按
/// employee_id 反查 EmployeeStore。**这个枚举是"是不是 employee"的唯一真值**，
/// 取代之前用 emp- 前缀启发式或 employee_id 互斥参数的方案。
///
/// 与 `AgentSource` 的区别：`AgentSource` 仅记录"配置文件来自 builtin/user 哪里"，
/// 是历史字段；`AgentSource` 是 dispatch 时的语义分类。两者并存以保持向后兼容。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentSource {
    Builtin,
    UserMarkdown,
    Employee,
}
```

`AgentDefinition` 末尾追加字段：

```rust
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub max_iterations: usize,
    pub model: AgentModel,
    pub system_prompt: AgentPrompt,
    pub source: AgentSource,
    pub permission_mode: AgentPermissionMode,
    pub background_default: bool,
    pub source: AgentSource,
}
```

并在文件顶部已有的 `agent_definition_supports_disallowed_tools_and_permission_mode` 测试里补 `source: AgentSource::Builtin`。

- [ ] **Step 4: 修 3 个 builtin 构造点**

`runtime/agent/builtin/daily_assistant_agent.rs`、`explore.rs`、`general_purpose.rs` 每个 `AgentDefinition { ... }` 字面量末尾追加：

```rust
        origin: crate::runtime::agent::definition::AgentSource::Builtin,
```

- [ ] **Step 5: 修 markdown_loader 构造点**

`runtime/agent/markdown_loader.rs:109` 找到 `Ok(AgentDefinition { ... })`，末尾追加：

```rust
        origin: crate::runtime::agent::definition::AgentSource::User,
```

- [ ] **Step 6: 跑全部 lib 测试，确认编译通过、新测试 PASS**

Run: `cd src-tauri && cargo test --lib runtime::agent::definition runtime::agent::builtin runtime::agent::markdown_loader -- --nocapture`

Expected: 全 PASS。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/runtime/agent/definition.rs src-tauri/src/runtime/agent/builtin/ src-tauri/src/runtime/agent/markdown_loader.rs
git commit -m "feat(agent): add AgentSource enum to AgentDefinition"
```

---

## Task 2: AgentRegistry 改 RwLock + 加 dynamic 注册接口

**Files:**
- Modify: `src-tauri/src/runtime/agent/registry.rs`
- Modify: `src-tauri/src/runtime/agent/registry_loader.rs`

- [ ] **Step 1: 写失败测试 — register_dynamic / unregister + builtin 保护**

在 `src-tauri/src/runtime/agent/registry.rs` 文件末尾添加 `#[cfg(test)] mod tests`：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::agent::definition::{
        AgentModel, AgentSource, AgentPermissionMode, AgentPrompt,
    };

    fn mk_def(name: &str, origin: AgentSource) -> AgentDefinition {
        AgentDefinition {
            name: name.into(),
            description: "".into(),
            allowed_tools: vec![],
            disallowed_tools: vec![],
            max_iterations: 10,
            model: AgentModel::Inherit,
            system_prompt: AgentPrompt::Inline("".into()),
            source: AgentSource::User,
            permission_mode: AgentPermissionMode::Bubble,
            background_default: false,
            origin,
        }
    }

    #[test]
    fn register_dynamic_and_get() {
        let reg = AgentRegistry::with_builtins();
        reg.register_dynamic(mk_def("emp-x", AgentSource::Employee));
        let got = reg.get("emp-x").expect("should find emp-x");
        assert_eq!(got.origin, AgentSource::Employee);
    }

    #[test]
    fn unregister_removes_dynamic_entries() {
        let reg = AgentRegistry::with_builtins();
        reg.register_dynamic(mk_def("emp-y", AgentSource::Employee));
        assert!(reg.get("emp-y").is_some());
        reg.unregister("emp-y");
        assert!(reg.get("emp-y").is_none());
    }

    #[test]
    fn unregister_protects_builtins() {
        let reg = AgentRegistry::with_builtins();
        reg.unregister("general-purpose");
        assert!(
            reg.get("general-purpose").is_some(),
            "general-purpose must not be removable"
        );
    }

    #[test]
    fn list_returns_sorted_owned_defs() {
        let reg = AgentRegistry::with_builtins();
        reg.register_dynamic(mk_def("emp-a", AgentSource::Employee));
        let names: Vec<String> = reg.list().iter().map(|d| d.name.clone()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }
}
```

注意：`AgentSource` 这里参考自 `super::*`（`registry.rs` 已有 `use crate::runtime::agent::definition::AgentDefinition;`，需补 `AgentSource`）。

- [ ] **Step 2: 验证测试失败**

Run: `cd src-tauri && cargo test --lib runtime::agent::registry::tests -- --nocapture`

Expected: 编译错误（`register_dynamic` / `unregister` 不存在）。

- [ ] **Step 3: 重写 `registry.rs`**

整体替换文件内容：

```rust
use std::collections::HashMap;
use std::sync::RwLock;

use crate::runtime::agent::builtin::{
    daily_assistant_agent::daily_assistant_agent_definition, explore::explore_agent_definition,
    general_purpose::general_purpose_agent_definition,
};
use crate::runtime::agent::definition::{AgentDefinition, AgentSource};

/// Builtin 名字保护清单——`unregister` 会拒绝删除这些项。
/// 派 Teammate 时这些是终极兜底，绝对不能在运行时被业务路径意外移除。
const PROTECTED_NAMES: &[&str] = &["general-purpose", "explore", "daily_assistant_agent"];

/// AgentRegistry 是 spawn_subagent 派活的唯一查询入口。
///
/// 内部用 `RwLock<HashMap>` 是因为：
/// - 启动时 seed builtin + user markdown + Active Employee 投影
/// - 运行时 hire/update/archive 通过 `EmployeeAgentSync` 钩子动态增删
/// - 读多写少（每轮 turn 调 `list` 渲染 catalog；employee 变更频率远低）
pub struct AgentRegistry {
    inner: RwLock<HashMap<String, AgentDefinition>>,
}

impl AgentRegistry {
    pub fn with_builtins() -> Self {
        let mut map = HashMap::new();
        for def in [
            daily_assistant_agent_definition(),
            general_purpose_agent_definition(),
            explore_agent_definition(),
        ] {
            map.insert(def.name.clone(), def);
        }
        Self {
            inner: RwLock::new(map),
        }
    }

    /// 静态注册：启动时加载（builtins + user_dir markdown）。
    /// 同名后覆盖前，由调用方保证 namespace 不冲突。
    pub fn register(&self, def: AgentDefinition) {
        let mut g = self.inner.write().expect("registry write poisoned");
        g.insert(def.name.clone(), def);
    }

    /// 由 EmployeeStore 投影而来的动态条目。
    /// 语义同 `register`；分开命名是为了让调用点意图清晰、便于 grep。
    pub fn register_dynamic(&self, def: AgentDefinition) {
        self.register(def);
    }

    /// 删除一个 dynamic agent（employee archived / paused / purged 时调用）。
    /// 拒绝删除 `PROTECTED_NAMES` 里的 builtin，避免业务路径意外移除核心 agent。
    pub fn unregister(&self, name: &str) {
        if PROTECTED_NAMES.contains(&name) {
            log::warn!(
                "[agent-registry] refused to unregister protected builtin: {}",
                name
            );
            return;
        }
        let mut g = self.inner.write().expect("registry write poisoned");
        g.remove(name);
    }

    /// 返回 owned `AgentDefinition`（之前返 `&` 是因为内部是裸 HashMap；
    /// 加 RwLock 后 lifetime 不能跨 read guard，只能 clone 出来）。
    pub fn get(&self, name: &str) -> Option<AgentDefinition> {
        let g = self.inner.read().expect("registry read poisoned");
        g.get(name).cloned()
    }

    pub fn list(&self) -> Vec<AgentDefinition> {
        let g = self.inner.read().expect("registry read poisoned");
        let mut list: Vec<AgentDefinition> = g.values().cloned().collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }
}
```

- [ ] **Step 4: 修 registry_loader.rs**

`registry_loader.rs:25-35` 把：

```rust
pub fn load_registry_with_user_dir(
    user_dir: Option<&Path>,
    project_dir: Option<&Path>,
) -> AgentRegistry {
    let mut reg = AgentRegistry::with_builtins();
    if let Some(dir) = user_dir {
        merge_dir(&mut reg, dir, "user");
    }
    if let Some(dir) = project_dir {
        merge_dir(&mut reg, dir, "project");
    }
    reg
}

fn merge_dir(reg: &mut AgentRegistry, dir: &Path, source_label: &str) {
```

改为：

```rust
pub fn load_registry_with_user_dir(
    user_dir: Option<&Path>,
    project_dir: Option<&Path>,
) -> AgentRegistry {
    let reg = AgentRegistry::with_builtins();
    if let Some(dir) = user_dir {
        merge_dir(&reg, dir, "user");
    }
    if let Some(dir) = project_dir {
        merge_dir(&reg, dir, "project");
    }
    reg
}

fn merge_dir(reg: &AgentRegistry, dir: &Path, source_label: &str) {
```

- [ ] **Step 5: 修 spawn_subagent.rs 既有调用点的 `&AgentDefinition` 用法**

Run: `cd src-tauri && cargo check --tests 2>&1 | grep -E '^error' | head -20`

可能的报错：
- `runtime/tools/builtin/spawn_subagent.rs:375` `let definition = self.registry.get(agent_type).ok_or_else(...)?` — 之前返 `&AgentDefinition`，现在返 owned，下面 `.clone()` 可以删；但因为 Task 6 会整段重写，**这里只需要确保编译通过**，临时改：

```rust
let definition = self.registry.get(agent_type).ok_or_else(|| {
    ToolError::ExecutionFailed(format!(
        "unknown subagent_type '{agent_type}'; \
         check ~/.renlijia/users/<scope>/agents/ or builtin agents"
    ))
})?;
```
（删掉原来的 `.clone()`，因为 `get` 已经返 owned）

`AgentRegistry::list()` 调用点（如 `chat_runtime_impl.rs:101`）原本是 `.list().into_iter().map(|def| ...)` 拿 `&def`，现在返 owned 也能跑——`into_iter().map(|def| AgentDefSummary { name: def.name.clone(), ...})` 改为 `name: def.name`（owned 直接 move）即可，但**这步 Task 10 会重写**，先保留 `.clone()` 让它编译通过。

- [ ] **Step 6: 跑测试**

Run: `cd src-tauri && cargo test --lib runtime::agent::registry -- --nocapture`

Expected: 4 个新测试 + 既有 PASS。

`cd src-tauri && cargo check --tests 2>&1 | tail -10`

Expected: 编译通过（warning 可容忍）。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/runtime/agent/registry.rs src-tauri/src/runtime/agent/registry_loader.rs src-tauri/src/runtime/tools/builtin/spawn_subagent.rs
git commit -m "refactor(agent): AgentRegistry → RwLock + register_dynamic/unregister"
```

---

## Task 3: EmployeeRecord → AgentDefinition 投影函数 + EmployeeAgentSync trait

**Files:**
- Create: `src-tauri/src/runtime/agent/employee_projection.rs`
- Modify: `src-tauri/src/runtime/agent/mod.rs`

- [ ] **Step 1: 写文件 + 测试**

新建 `src-tauri/src/runtime/agent/employee_projection.rs`：

```rust
//! `EmployeeRecord` → `AgentDefinition` 投影。
//!
//! 把 EmployeeStore 的"业务实体记录"映射成 AgentRegistry 的"派活定义"。
//! 只取派活相关字段（name / tool_whitelist / system_prompt_extra），业务态字段
//! （cron / lifecycle / resource_config / last_run_at）继续留在 EmployeeStore，
//! 不进 AgentDefinition——AgentDefinition 没有这些概念。
//!
//! 这是把 EmployeeStore 接入 AgentRegistry 单一查询入口的"夹层"，对齐
//! claude-code-best 的"AgentDefinition 是 dispatch 唯一维度"思路。

use std::sync::Arc;

use crate::runtime::agent::definition::{
    AgentDefinition, AgentModel, AgentSource, AgentPermissionMode, AgentPrompt, AgentSource,
};
use crate::runtime::agent::registry::AgentRegistry;
use crate::runtime::employee::store::{EmployeeLifecycle, EmployeeRecord};

/// 用 EmployeeRecord 派生一个 AgentDefinition。
///
/// `name` 取 employee.id（emp-…）让 LLM 看到的 subagent_type 直接就是
/// employee id；`origin` 设为 `Employee` 让 spawn_subagent 派 Teammate 时
/// 能正确决定是否往 transcript 写 employee_id。
pub fn project_employee_to_agent(rec: &EmployeeRecord) -> AgentDefinition {
    AgentDefinition {
        name: rec.id.clone(),
        description: format!("{}（{}，数字员工）", rec.name, rec.role),
        allowed_tools: rec.tool_whitelist.clone(),
        disallowed_tools: Vec::new(),
        max_iterations: 30,
        model: AgentModel::Inherit,
        system_prompt: AgentPrompt::Inline(
            rec.system_prompt_extra.clone().unwrap_or_default(),
        ),
        source: AgentSource::User,
        permission_mode: AgentPermissionMode::AutoDeny,
        background_default: true,
        source: AgentSource::Employee,
    }
}

/// 同步钩子 trait —— EmployeeStore.create / update / archive / purge 调用，
/// 让 AgentRegistry 跟上 Employee 的 lifecycle 变化。
///
/// 拆 trait 是为了让 EmployeeStore 不强依赖具体 Registry 类型（测试用
/// `NoopSync` 替身）。生产路径用 `AgentRegistrySync`。
pub trait EmployeeAgentSync: Send + Sync {
    /// Employee 变成 Active（雇佣 / 从 Paused 恢复 / 从 Archived 复活）。
    fn on_active(&self, rec: &EmployeeRecord);
    /// Employee 不再 Active（archive / purge / pause）；`name` 是 employee id。
    fn on_inactive(&self, name: &str);
}

/// 测试 / 早期 boot 用的空实现。
pub struct NoopSync;
impl EmployeeAgentSync for NoopSync {
    fn on_active(&self, _rec: &EmployeeRecord) {}
    fn on_inactive(&self, _name: &str) {}
}

/// 生产路径：把同步事件转发给 AgentRegistry。
pub struct AgentRegistrySync {
    pub registry: Arc<AgentRegistry>,
}

impl EmployeeAgentSync for AgentRegistrySync {
    fn on_active(&self, rec: &EmployeeRecord) {
        self.registry.register_dynamic(project_employee_to_agent(rec));
    }
    fn on_inactive(&self, name: &str) {
        self.registry.unregister(name);
    }
}

/// 启动时把所有 Active employee 灌进 AgentRegistry。返回实际注册的条目数。
/// `records` 是 `EmployeeStore::list()` 的产物；这里再做一次 lifecycle 过滤，
/// 避免误注册 Paused / Archived 项。
pub fn seed_registry_from_employees(
    registry: &AgentRegistry,
    records: &[EmployeeRecord],
) -> usize {
    let mut count = 0;
    for rec in records {
        if matches!(rec.lifecycle, EmployeeLifecycle::Active) {
            registry.register_dynamic(project_employee_to_agent(rec));
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::employee::store::{
        CreateEmployeeRequest, EmployeeLifecycle, EmployeeStore,
    };
    use tempfile::TempDir;

    fn mk_store() -> (EmployeeStore, TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        (EmployeeStore::new(tmp.path().to_path_buf()), tmp)
    }

    #[test]
    fn project_carries_tool_whitelist_and_prompt_and_origin() {
        let (store, _tmp) = mk_store();
        let rec = store
            .create(CreateEmployeeRequest {
                name: "小研".into(),
                role: "调研员".into(),
                description: "".into(),
                avatar: "🔬".into(),
                template_id: None,
                tool_whitelist: Some(vec!["Read".into(), "Grep".into()]),
                cron: None,
                timezone: None,
                lifecycle: None,
                cron_enabled: None,
                resource_config: None,
                system_prompt_extra: Some("你是小研".into()),
                default_skill_id: None,
            })
            .unwrap();
        let def = project_employee_to_agent(&rec);
        assert_eq!(def.name, rec.id);
        assert_eq!(def.allowed_tools, vec!["Read".to_string(), "Grep".to_string()]);
        assert_eq!(def.origin, AgentSource::Employee);
        match def.system_prompt {
            AgentPrompt::Inline(s) => assert!(s.contains("小研")),
            _ => panic!("expected Inline system prompt"),
        }
        assert!(def.description.contains("小研"));
        assert!(def.description.contains("数字员工"));
    }

    #[test]
    fn seed_skips_non_active_employees() {
        let registry = AgentRegistry::with_builtins();
        let (store, _tmp) = mk_store();
        let active = store
            .create(CreateEmployeeRequest {
                name: "n1".into(),
                role: "r".into(),
                description: "".into(),
                avatar: "".into(),
                template_id: None,
                tool_whitelist: None,
                cron: None,
                timezone: None,
                lifecycle: Some(EmployeeLifecycle::Active),
                cron_enabled: None,
                resource_config: None,
                system_prompt_extra: None,
                default_skill_id: None,
            })
            .unwrap();
        let paused = store
            .create(CreateEmployeeRequest {
                name: "n2".into(),
                role: "r".into(),
                description: "".into(),
                avatar: "".into(),
                template_id: None,
                tool_whitelist: None,
                cron: None,
                timezone: None,
                lifecycle: Some(EmployeeLifecycle::Paused),
                cron_enabled: None,
                resource_config: None,
                system_prompt_extra: None,
                default_skill_id: None,
            })
            .unwrap();
        let n = seed_registry_from_employees(&registry, &[active.clone(), paused]);
        assert_eq!(n, 1);
        assert!(registry.get(&active.id).is_some());
        assert_eq!(registry.list().len(), 4); // 3 builtin + 1 active
    }

    #[test]
    fn registry_sync_round_trip() {
        let registry = Arc::new(AgentRegistry::with_builtins());
        let sync = AgentRegistrySync {
            registry: registry.clone(),
        };
        let (store, _tmp) = mk_store();
        let rec = store
            .create(CreateEmployeeRequest {
                name: "n".into(),
                role: "r".into(),
                description: "".into(),
                avatar: "".into(),
                template_id: None,
                tool_whitelist: None,
                cron: None,
                timezone: None,
                lifecycle: None,
                cron_enabled: None,
                resource_config: None,
                system_prompt_extra: None,
                default_skill_id: None,
            })
            .unwrap();
        sync.on_active(&rec);
        assert!(registry.get(&rec.id).is_some());
        sync.on_inactive(&rec.id);
        assert!(registry.get(&rec.id).is_none());
    }
}
```

- [ ] **Step 2: 模块注册**

`src-tauri/src/runtime/agent/mod.rs` 加入：

```rust
pub mod employee_projection;
```

（按字母序插入到现有 `pub mod` 列表里）

- [ ] **Step 3: 跑单测**

Run: `cd src-tauri && cargo test --lib runtime::agent::employee_projection -- --nocapture`

Expected: 3 个 PASS。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/runtime/agent/employee_projection.rs src-tauri/src/runtime/agent/mod.rs
git commit -m "feat(agent): add EmployeeRecord→AgentDefinition projection + sync trait"
```

---

## Task 4: EmployeeStore 加 sync 字段 + 钩子 emission

**Files:**
- Modify: `src-tauri/src/runtime/employee/store.rs`

> **设计说明**：本 task 只装钩子接口，不要求 EmployeeStore 是单例。Task 5 把它升级成单例后钩子才在生产路径生效；本 task 已有的 employee 测试用 `None` sync 跑，行为零变化。

- [ ] **Step 1: 写失败测试 — sync 钩子触发**

在 `src-tauri/src/runtime/employee/store.rs` 文件末尾（如果已有 `#[cfg(test)] mod tests` 就追加；否则新建）加：

```rust
#[cfg(test)]
mod sync_hook_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use crate::runtime::agent::employee_projection::EmployeeAgentSync;

    #[derive(Default)]
    struct CountingSync {
        active: Mutex<Vec<String>>,
        inactive: Mutex<Vec<String>>,
    }
    impl EmployeeAgentSync for CountingSync {
        fn on_active(&self, rec: &EmployeeRecord) {
            self.active.lock().unwrap().push(rec.id.clone());
        }
        fn on_inactive(&self, name: &str) {
            self.inactive.lock().unwrap().push(name.to_string());
        }
    }

    fn mk(req_name: &str) -> CreateEmployeeRequest {
        CreateEmployeeRequest {
            name: req_name.into(),
            role: "r".into(),
            description: "".into(),
            avatar: "".into(),
            template_id: None,
            tool_whitelist: None,
            cron: None,
            timezone: None,
            lifecycle: None,
            cron_enabled: None,
            resource_config: None,
            system_prompt_extra: None,
            default_skill_id: None,
        }
    }

    #[test]
    fn create_calls_on_active_when_active() {
        let tmp = tempfile::tempdir().unwrap();
        let store = EmployeeStore::new(tmp.path().to_path_buf());
        let sync = Arc::new(CountingSync::default());
        store.set_sync(sync.clone());
        let rec = store.create(mk("n1")).unwrap();
        let active = sync.active.lock().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0], rec.id);
    }

    #[test]
    fn purge_calls_on_inactive() {
        let tmp = tempfile::tempdir().unwrap();
        let store = EmployeeStore::new(tmp.path().to_path_buf());
        let rec = store.create(mk("n2")).unwrap();
        let sync = Arc::new(CountingSync::default());
        store.set_sync(sync.clone());
        // archive 之后才能 purge（语义约束）
        store
            .update(
                &rec.id,
                UpdateEmployeeRequest {
                    lifecycle: Some(EmployeeLifecycle::Archived),
                    ..Default::default()
                },
            )
            .unwrap();
        store.purge(&rec.id).unwrap();
        let inactive = sync.inactive.lock().unwrap();
        // archive (update→on_inactive) + purge (on_inactive) — 至少 1 次
        assert!(inactive.iter().any(|n| n == &rec.id));
    }

    #[test]
    fn update_to_paused_calls_on_inactive() {
        let tmp = tempfile::tempdir().unwrap();
        let store = EmployeeStore::new(tmp.path().to_path_buf());
        let rec = store.create(mk("n3")).unwrap();
        let sync = Arc::new(CountingSync::default());
        store.set_sync(sync.clone());
        store
            .update(
                &rec.id,
                UpdateEmployeeRequest {
                    lifecycle: Some(EmployeeLifecycle::Paused),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(sync.inactive.lock().unwrap()[0], rec.id);
    }
}
```

- [ ] **Step 2: 验证测试失败**

Run: `cd src-tauri && cargo test --lib runtime::employee::store::sync_hook_tests -- --nocapture`

Expected: 编译错误（`set_sync` 不存在）。

- [ ] **Step 3: 实现 sync 字段 + 钩子 emission**

定位 `EmployeeStore` struct（约 line 145）：

```rust
#[derive(Debug)]
pub struct EmployeeStore {
    root: PathBuf,
    lock: Mutex<()>,
}
```

替换为：

```rust
pub struct EmployeeStore {
    root: PathBuf,
    lock: Mutex<()>,
    /// AgentRegistry 同步钩子：employee lifecycle 变化时通知。
    /// `None` = 不通知（测试 / 早期 boot），生产路径在 lib.rs 中 wire 进去。
    /// 用 RwLock interior mutability 让 `set_sync(&self)` 不要求 mut，方便 Arc 共享。
    sync: std::sync::RwLock<
        Option<std::sync::Arc<dyn crate::runtime::agent::employee_projection::EmployeeAgentSync>>,
    >,
}

impl std::fmt::Debug for EmployeeStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmployeeStore")
            .field("root", &self.root)
            .field("has_sync", &self.sync.read().map(|g| g.is_some()).unwrap_or(false))
            .finish()
    }
}
```

`pub fn new(...)`（约 line 153）改：

```rust
    pub fn new(employees_dir: PathBuf) -> Self {
        Self {
            root: employees_dir,
            lock: Mutex::new(()),
            sync: std::sync::RwLock::new(None),
        }
    }

    /// 设置 lifecycle 变化时的同步钩子。lib.rs 启动后调用一次注入
    /// `AgentRegistrySync`；之后 hire / update / archive / purge 自动通知。
    pub fn set_sync(
        &self,
        sync: std::sync::Arc<dyn crate::runtime::agent::employee_projection::EmployeeAgentSync>,
    ) {
        let mut g = self.sync.write().expect("sync write poisoned");
        *g = Some(sync);
    }

    fn notify_active(&self, rec: &EmployeeRecord) {
        if let Some(sync) = self.sync.read().expect("sync read poisoned").as_ref() {
            sync.on_active(rec);
        }
    }

    fn notify_inactive(&self, id: &str) {
        if let Some(sync) = self.sync.read().expect("sync read poisoned").as_ref() {
            sync.on_inactive(id);
        }
    }
```

- [ ] **Step 4: 在 create / update / purge 末尾加钩子调用**

定位 `pub fn create(&self, req: CreateEmployeeRequest) -> Result<EmployeeRecord>`（约 line 176），在 return Ok(record) 前：

```rust
        if matches!(record.lifecycle, EmployeeLifecycle::Active) {
            self.notify_active(&record);
        }
        Ok(record)
```

`pub fn update(...)`（约 line 282），return Ok(updated) 前：

```rust
        match updated.lifecycle {
            EmployeeLifecycle::Active => self.notify_active(&updated),
            EmployeeLifecycle::Paused | EmployeeLifecycle::Archived => {
                self.notify_inactive(&updated.id);
            }
        }
        Ok(updated)
```

`pub fn purge(&self, id: &str) -> Result<bool>`（约 line 402），删除成功后：

```rust
        if removed {
            self.notify_inactive(id);
        }
        Ok(removed)
```

`purge_if_archived_older_than`（约 line 420）：找到删除成功路径，加同样的 `self.notify_inactive(id);`。

`purge_old_archived`（约 line 457）：循环里每次成功删一个就 emit：

```rust
        for id in &purged {
            self.notify_inactive(id);
        }
```

（具体放置位置看现有 fn 怎么聚合 purged ids，可能是局部变量 `purged: Vec<String>`，emit 在循环外做。）

- [ ] **Step 5: 跑单测**

Run: `cd src-tauri && cargo test --lib runtime::employee::store -- --nocapture`

Expected: 既有 + sync_hook_tests 全 PASS。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/employee/store.rs
git commit -m "feat(employee): emit AgentRegistry sync events on lifecycle changes"
```

---

## Task 5: EmployeeStore 升级为 app 单例 + lib.rs seed

**Files:**
- Modify: `src-tauri/src/lib.rs:349-380`
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:113-148, 188-238`
- Modify: 其它 `EmployeeStore::new(...)` 调用点（grep 确认）

> **关键**：Task 4 写完后，钩子接口已有，但生产路径上 EmployeeStore 在每个 Tauri command 里都 `new()` 一个临时实例（参 `chat_runtime_impl.rs:120-122`），导致 `set_sync` 调过的实例出 scope 就丢。**本 task 把它升级为 `Arc<EmployeeStore>` 单例**，所有调用点改从 `app.try_state::<Arc<EmployeeStore>>()` 拿。

- [ ] **Step 1: lib.rs 启动段改造**

定位 lib.rs 现有：

```rust
            // Build agent registry: builtins + user-scope agents/*.md (if logged in).
            let user_agents_dir = current_user_storage
                .resolve_paths()
                .map(|paths| paths.agents_dir());
            let agent_registry = Arc::new(
                runtime::agent::registry_loader::load_registry_with_user_dir(
                    user_agents_dir.as_deref(),
                    None,
                ),
            );
            app.manage(agent_registry.clone());
```

替换为：

```rust
            // Build agent registry: builtins + user-scope agents/*.md (if logged in)
            // + dynamic projection of Active Employees.
            let user_agents_dir = current_user_storage
                .resolve_paths()
                .map(|paths| paths.agents_dir());
            let agent_registry = Arc::new(
                runtime::agent::registry_loader::load_registry_with_user_dir(
                    user_agents_dir.as_deref(),
                    None,
                ),
            );

            // EmployeeStore as app singleton: lib.rs creates one Arc, seeds AgentRegistry,
            // wires the sync hook; all Tauri commands pull this same Arc from app state.
            // Without singletization, set_sync below would only affect this transient
            // boot instance — every Tauri command that does EmployeeStore::new(...) would
            // miss the hook and AgentRegistry would drift.
            let employee_store_arc: Option<Arc<runtime::employee::store::EmployeeStore>> =
                current_user_storage.resolve_paths().map(|paths| {
                    Arc::new(runtime::employee::store::EmployeeStore::new(
                        paths.employees_dir(),
                    ))
                });

            if let Some(emp_store) = employee_store_arc.as_ref() {
                match emp_store.list() {
                    Ok(records) => {
                        let n = runtime::agent::employee_projection::seed_registry_from_employees(
                            &agent_registry,
                            &records,
                        );
                        log::info!(
                            "[agent-registry] seeded {} active employees into AgentRegistry",
                            n
                        );
                    }
                    Err(e) => log::warn!("[agent-registry] employee seed failed: {e}"),
                }
                let sync = Arc::new(
                    runtime::agent::employee_projection::AgentRegistrySync {
                        registry: agent_registry.clone(),
                    },
                );
                emp_store.set_sync(sync);
            }

            app.manage(agent_registry.clone());
            if let Some(store) = employee_store_arc {
                app.manage(store);
            }
```

- [ ] **Step 2: 找到所有生产代码里的 `EmployeeStore::new(...)` 调用**

Run: `cd src-tauri && grep -rn "EmployeeStore::new\b" src/ --include="*.rs"`

预计调用点（按 grep 结果调整）：
- `transport/tauri_commands/chat/chat_runtime_impl.rs:120` （`build_tool_description_context` 内）
- `transport/tauri_commands/chat.rs:3114` 附近（chat 内查找 organizer employee）
- `transport/tauri_commands/employee*.rs`（hire / update / archive / list 命令；可能有多个文件）
- `transport/tauri_commands/agenda.rs` 如有

每个生产代码调用点改为：

```rust
let store = app
    .try_state::<Arc<crate::runtime::employee::store::EmployeeStore>>()
    .map(|s| s.inner().clone())
    .ok_or_else(|| "EmployeeStore not initialized".to_string())?;
```

或如果调用点在 `&AppHandle` 之外（拿不到 app），保持 `new()` 暂时不动；写注释 `// FIXME: this path bypasses the app-singleton sync hook` 标记。

测试代码（`tests/*.rs` 和 `#[cfg(test)] mod`）保持 `new()` 不动。

- [ ] **Step 3: cargo check + 跑 employee 整套测试**

Run:

```bash
cd src-tauri && cargo check 2>&1 | tail -15
cd src-tauri && cargo test --lib runtime::employee:: -- --nocapture
cd src-tauri && cargo test --tests employee_ -- --no-fail-fast 2>&1 | tail -20
```

Expected: 全 PASS。如有 lifetime / Send 错误：通常是 `app.try_state` 在 async fn 里 hold 了 Tauri State 不能跨 await，需要 `let store = app.try_state::<...>().map(|s| s.inner().clone())?;` 这种"立刻 clone Arc"的写法。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/transport/
git commit -m "refactor(employee): EmployeeStore as app singleton, seed AgentRegistry on boot"
```

---

## Task 6: spawn_subagent 删 employee_id / AgentSource / employee_store 字段

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/spawn_subagent.rs`

- [ ] **Step 1: 写失败测试 — emp-id 当 subagent_type 直接命中 + 未知 subagent_type 回灌清单**

在 `mod tests` 末尾追加：

```rust
    // ── 单一 subagent_type 入口 ────────────────────────────────────────────

    #[tokio::test]
    async fn subagent_type_finds_employee_via_registry() {
        // employee 已经被 seed 进 registry，直接走 registry.get 命中
        let registry = Arc::new(AgentRegistry::with_builtins());
        let emp_def = AgentDefinition {
            name: "emp-test-001".to_string(),
            description: "小研（调研员，数字员工）".to_string(),
            allowed_tools: vec!["Read".to_string()],
            disallowed_tools: vec![],
            max_iterations: 30,
            model: AgentModel::Inherit,
            system_prompt: AgentPrompt::Inline("你是小研".to_string()),
            source: AgentSource::User,
            permission_mode: AgentPermissionMode::AutoDeny,
            background_default: true,
            source: AgentSource::Employee,
        };
        registry.register_dynamic(emp_def);

        let seen = Arc::new(Mutex::new(Vec::new()));
        let tool = SpawnSubagentRuntimeTool::new(
            Arc::new(RecordingLauncher {
                seen_requests: seen.clone(),
                async_seen: Arc::new(Mutex::new(Vec::new())),
            }),
            registry,
        );
        let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
        tool.execute(
            json!({
                "subagent_type": "emp-test-001",
                "prompt": "x",
                "description": "y"
            }),
            ctx,
        )
        .await
        .expect("emp-id should resolve via registry");
        let reqs = seen.lock().unwrap();
        assert_eq!(reqs[0].subagent_type, "emp-test-001");
    }

    #[tokio::test]
    async fn unknown_subagent_type_error_lists_registry_options() {
        let registry = Arc::new(AgentRegistry::with_builtins());
        let tool = SpawnSubagentRuntimeTool::new(
            Arc::new(RecordingLauncher {
                seen_requests: Arc::new(Mutex::new(Vec::new())),
                async_seen: Arc::new(Mutex::new(Vec::new())),
            }),
            registry,
        );
        let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
        let err = tool
            .execute(
                json!({
                    "subagent_type": "nonexistent-xyz",
                    "prompt": "x",
                    "description": "y"
                }),
                ctx,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::ExecutionFailed(msg) => {
                assert!(msg.contains("nonexistent-xyz"), "error: {msg}");
                assert!(
                    msg.contains("general-purpose") || msg.contains("explore"),
                    "should list builtins: {msg}"
                );
            }
            other => panic!("expected ExecutionFailed, got {:?}", other),
        }
    }
```

如果文件顶层 `use` 没引入 `AgentSource`，补 `use crate::runtime::agent::definition::{AgentDefinition, AgentModel, AgentSource, AgentPermissionMode, AgentPrompt, AgentSource};` 在测试 mod 顶部（`use super::*;` 下方）。

- [ ] **Step 2: 验证测试失败**

Run: `cd src-tauri && cargo test --lib runtime::tools::builtin::spawn_subagent::tests::subagent_type_finds_employee_via_registry runtime::tools::builtin::spawn_subagent::tests::unknown_subagent_type_error_lists_registry_options -- --nocapture`

Expected: FAIL（当前代码还要 `employee_id` 字段；找不到 `general-purpose` 在错误里）。

- [ ] **Step 3: 删 `AgentSource` 枚举和 `employee_store` 字段**

定位（约 line 117-124）：

```rust
enum AgentSource {
    Registry(String),
    Employee(String),
}
```

整段删除。

定位 struct（约 line 128-135）：

```rust
pub struct SpawnSubagentRuntimeTool {
    launcher: Arc<dyn SpawnSubagentLauncher>,
    registry: Arc<AgentRegistry>,
    employee_store: Option<Arc<EmployeeStore>>,
}
```

改为：

```rust
pub struct SpawnSubagentRuntimeTool {
    launcher: Arc<dyn SpawnSubagentLauncher>,
    registry: Arc<AgentRegistry>,
}
```

`impl` 块（约 line 137-158）只保留 `new`：

```rust
impl SpawnSubagentRuntimeTool {
    pub fn new(launcher: Arc<dyn SpawnSubagentLauncher>, registry: Arc<AgentRegistry>) -> Self {
        Self { launcher, registry }
    }
}
```

`new_with_employees` 整个删除。

文件顶部 `use crate::runtime::employee::store::EmployeeStore;` 也删除（不再依赖 EmployeeStore）。

- [ ] **Step 4: 改 `execute()` 输入解析 — 单 `subagent_type`**

定位（约 line 275-305）：

```rust
        // ── Parse optional source fields ───────────────────────────────────
        let subagent_type = input
            .get("subagent_type")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let employee_id = input
            .get("employee_id")
            ...
        let source = match (&subagent_type, &employee_id) {
            ...
        };
```

整段替换为：

```rust
        // ── Parse the single dispatch source field ────────────────────────
        let subagent_type = input
            .get("subagent_type")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                ToolError::ExecutionFailed("missing required field: subagent_type".into())
            })?;
```

- [ ] **Step 5: 改 def 解析 — 直接 registry.get + 用 origin 判 employee_id**

定位（约 line 372-413）：

```rust
        let (sys_prompt_extra, tool_whitelist, model_override) = match &source {
            AgentSource::Registry(agent_type) => {
                ...
            }
            AgentSource::Employee(eid) => {
                ...
            }
        };
```

整段替换为：

```rust
        // ── Resolve agent definition from registry (employees已在 boot 时投影进去) ──
        let definition = self.registry.get(&subagent_type).ok_or_else(|| {
            ToolError::ExecutionFailed(build_unknown_subagent_type_error(
                &subagent_type,
                &self.registry,
            ))
        })?;

        let sys_prompt_extra = match &definition.system_prompt {
            crate::runtime::agent::definition::AgentPrompt::Inline(s) if !s.is_empty() => {
                Some(s.clone())
            }
            _ => None,
        };
        let tool_whitelist = definition.allowed_tools.clone();
        let model_override = caller_model.clone().or_else(|| match &definition.model {
            crate::runtime::agent::definition::AgentModel::Fixed(m) => Some(m.clone()),
            crate::runtime::agent::definition::AgentModel::Inherit => None,
        });
```

`effective_model = model_override` 那行（约 line 417）保持不变。

- [ ] **Step 6: 改 Teammate 路径 employee_id 判定**

定位（约 line 491-500）：

```rust
        if let Some(team) = team_handle {
            ...
            let agent_id = teammate_agent_id.expect(...);
            let agent_name_str = name.clone().unwrap_or_default();
            let employee_id = if let AgentSource::Employee(ref eid) = source {
                Some(eid.clone())
            } else {
                None
            };
```

后两行替换为：

```rust
            let employee_id = if matches!(
                definition.origin,
                crate::runtime::agent::definition::AgentSource::Employee
            ) {
                Some(definition.name.clone())
            } else {
                None
            };
```

注：`definition` 在 Step 5 已经是 owned 了，能直接 `.name.clone()`。

- [ ] **Step 7: 改 SpawnSubagentRequest.subagent_type 直传**

定位（约 line 460-473）：

```rust
        let request = SpawnSubagentRequest {
            subagent_type: match &source {
                AgentSource::Registry(t) => t.clone(),
                AgentSource::Employee(eid) => format!("employee:{eid}"),
            },
            ...
        };
```

替换为：

```rust
        let request = SpawnSubagentRequest {
            subagent_type: subagent_type.clone(),
            ...
        };
```

（不再用 `employee:` sentinel — `DefaultSpawnSubagentLauncher` 现在直接 `registry.get` 就能命中。）

- [ ] **Step 8: 加 `build_unknown_subagent_type_error`**

在 `render_dispatch_catalog` 函数下方（约 line 213 后）新增：

```rust
/// 当 LLM 传入未知 `subagent_type` 时构造错误信息，把当前可选清单回灌给它。
/// 对齐 claude-code-best `AgentTool.tsx:532-536` 的 `Available agents: ...` 模式。
pub fn build_unknown_subagent_type_error(bad_name: &str, registry: &AgentRegistry) -> String {
    let available: Vec<String> = registry.list().iter().map(|d| d.name.clone()).collect();
    if available.is_empty() {
        format!("unknown subagent_type '{}'; no agents configured", bad_name)
    } else {
        format!(
            "unknown subagent_type '{}'. Available subagent_type values: {}",
            bad_name,
            available.join(", ")
        )
    }
}
```

- [ ] **Step 9: 改 `render_dispatch_catalog` 单段输出，按 origin 排序**

整体替换函数体（约 line 173-213）：

```rust
pub fn render_dispatch_catalog(ctx: &crate::runtime::tools::ToolDescriptionContext) -> String {
    use crate::runtime::agent::definition::AgentSource;
    use std::fmt::Write as _;

    let mut emp_lines: Vec<String> = Vec::new();
    let mut other_lines: Vec<String> = Vec::new();
    for a in &ctx.agents {
        let line = format!("- `{}` — {}", a.name, a.description);
        if matches!(a.origin, AgentSource::Employee) {
            emp_lines.push(line);
        } else {
            other_lines.push(line);
        }
    }
    if emp_lines.is_empty() && other_lines.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str(
        "**重要**：`subagent_type` 必须从下面清单中精确选择，禁止编造未列出的名字。\
         有匹配的数字员工（`emp-...`）时请优先选择它们——它们带有专属人设和工具白名单。",
    );
    let _ = write!(out, "\n\n<available_subagent_types>\n");
    for line in emp_lines.iter().chain(other_lines.iter()) {
        let _ = writeln!(out, "{line}");
    }
    let _ = write!(out, "</available_subagent_types>");
    out
}
```

注意：`ToolDescriptionContext.agents` 现在要含 origin 字段——Task 7 会把 `AgentDefSummary` 加上 origin。临时让函数编译通过，可以先把 `a.origin` 写法换成 `AgentSource::Builtin`（占位），Task 7 完成后改回。**或者把 Task 7 提前到这里和 Task 6 合并提交。** 我们选合并：见 Task 6 Step 11。

- [ ] **Step 10: 修 diagnostic event payload 字段名**

`definition()` 方法里 `record_diagnostic` 调用（约 line 229-244），把：

```rust
                "has_employee_section": dynamic.contains("<available_employee_ids>"),
                "has_subagent_section": dynamic.contains("<available_subagent_types>"),
                "employee_ids": ctx.employees.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
```

改为：

```rust
                "has_subagent_section": dynamic.contains("<available_subagent_types>"),
                "agent_names": ctx.agents.iter().map(|a| a.name.clone()).collect::<Vec<_>>(),
                "employee_count": ctx.agents.iter()
                    .filter(|a| matches!(a.origin, crate::runtime::agent::definition::AgentSource::Employee))
                    .count(),
```

（删除 `has_employee_section`、`employee_ids`；新增 `agent_names`、`employee_count`）

- [ ] **Step 11: 一并修 `ToolDescriptionContext`**

打开 `src-tauri/src/runtime/tools/description_context.rs`，整个文件改为：

```rust
//! [`ToolDescriptionContext`] — session-scoped context handed to
//! `RuntimeTool::definition()` so a tool can render a description that
//! depends on session state (which agents are registered, which MCP
//! servers are connected).
//!
//! Aligns with claude-code-best `tool.prompt({ tools, agents, ... })` —
//! the canonical approach: tool description is a function of session
//! context, not a compile-time constant.

use crate::runtime::agent::definition::AgentSource;

/// Compact summary of a registered/dispatchable agent — covers builtin
/// agents, user markdown agents, AND employees projected via
/// `EmployeeStore → AgentRegistry`. The `origin` field tells callers
/// which variant this is.
#[derive(Clone, Debug)]
pub struct AgentDefSummary {
    pub name: String,
    /// First sentence of the agent's description.
    pub description: String,
    pub source: AgentSource,
}

#[derive(Clone, Debug, Default)]
pub struct ToolDescriptionContext {
    pub agents: Vec<AgentDefSummary>,
    /// Connected MCP server names. Tools can reference these to advertise
    /// capability availability.
    pub mcp_servers: Vec<String>,
}

impl ToolDescriptionContext {
    pub fn empty() -> Self {
        Self::default()
    }
}
```

（删除 `EmployeeSummary` struct 和 `employees` 字段）

- [ ] **Step 12: 修 mod 导出**

`runtime/tools/mod.rs:17` 现状：

```rust
pub use description_context::{AgentDefSummary, EmployeeSummary, ToolDescriptionContext};
```

改为：

```rust
pub use description_context::{AgentDefSummary, ToolDescriptionContext};
```

- [ ] **Step 13: 修 `chat_runtime_impl::build_tool_description_context`**

定位 `build_tool_description_context`（约 line 92-155），整段替换为：

```rust
pub async fn build_tool_description_context(
    app: &AppHandle,
) -> crate::runtime::tools::ToolDescriptionContext {
    use crate::runtime::tools::{AgentDefSummary, ToolDescriptionContext};

    let agents: Vec<AgentDefSummary> = app
        .try_state::<Arc<crate::runtime::agent::registry::AgentRegistry>>()
        .map(|s| s.inner().clone())
        .map(|reg| {
            reg.list()
                .into_iter()
                .map(|def| AgentDefSummary {
                    name: def.name,
                    description: first_sentence(&def.description, 120),
                    origin: def.origin,
                })
                .collect()
        })
        .unwrap_or_default();

    ToolDescriptionContext {
        agents,
        mcp_servers: Vec::new(),
    }
}
```

- [ ] **Step 14: 修受影响的现有测试**

`spawn_subagent.rs` 测试中所有 `ToolDescriptionContext { agents, employees, mcp_servers }` 字面量改为 `{ agents, mcp_servers }`。

`render_catalog_lists_employees_with_id` 测试整段重写：

```rust
    #[test]
    fn render_catalog_lists_employees_first() {
        use crate::runtime::agent::definition::AgentSource;
        let ctx = ToolDescriptionContext {
            agents: vec![
                AgentDefSummary {
                    name: "general-purpose".into(),
                    description: "fallback".into(),
                    source: AgentSource::Builtin,
                },
                AgentDefSummary {
                    name: "emp-aaa-xiaoyan".into(),
                    description: "小研（调研员，数字员工）".into(),
                    source: AgentSource::Employee,
                },
                AgentDefSummary {
                    name: "emp-bbb-xiaosuan".into(),
                    description: "小算（数据分析员，数字员工）".into(),
                    source: AgentSource::Employee,
                },
            ],
            mcp_servers: vec![],
        };
        let out = render_dispatch_catalog(&ctx);
        let emp1 = out.find("emp-aaa-xiaoyan").expect("emp 1");
        let emp2 = out.find("emp-bbb-xiaosuan").expect("emp 2");
        let bi = out.find("general-purpose").expect("builtin");
        assert!(emp1 < bi && emp2 < bi, "employees should come before builtins");
        assert!(out.contains("<available_subagent_types>"));
        assert!(!out.contains("<available_employee_ids>"));
    }
```

`render_catalog_employee_section_appears_before_subagent_section` 整段删除（单段后无意义）。

`render_catalog_lists_agents_with_summary` 把 `agents` 字面量补 `source: AgentSource::Builtin`。

`definition_appends_dynamic_catalog_to_base` 把：
```rust
            employees: vec![EmployeeSummary { id, name, role }],
```
改为：
```rust
            agents: vec![AgentDefSummary {
                name: "emp-test-001".into(),
                description: "测试员（QA，数字员工）".into(),
                source: AgentSource::Employee,
            }],
```

`definition_with_empty_ctx_matches_static_catalog` 不动。

`rejects_both_subagent_type_and_employee_id` 删除（互斥语义不存在了）。

`missing_subagent_type_returns_execution_failed` 改成：

```rust
    #[tokio::test]
    async fn missing_subagent_type_returns_execution_failed() {
        let tool = build_tool_with_recorder(Arc::new(Mutex::new(Vec::new())));
        let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
        let err = tool
            .execute(
                json!({ "prompt": "x", "description": "y" }),
                ctx,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::ExecutionFailed(msg) => {
                assert!(msg.contains("subagent_type"), "got: {msg}");
            }
            other => panic!("expected ExecutionFailed, got {:?}", other),
        }
    }
```

- [ ] **Step 15: cargo check + 跑测试**

Run:

```bash
cd src-tauri && cargo check --tests 2>&1 | tail -20
cd src-tauri && cargo test --lib runtime::tools::builtin::spawn_subagent::tests -- --nocapture
```

Expected: 全 PASS。如果 `chat_runtime_impl.rs` 还有 `<available_employee_ids>` / `EmployeeSummary` 残留，按 cargo error 一起改了。

- [ ] **Step 16: Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/spawn_subagent.rs \
        src-tauri/src/runtime/tools/description_context.rs \
        src-tauri/src/runtime/tools/mod.rs \
        src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs
git commit -m "refactor(spawn_subagent): single subagent_type entry, drop employee_id/AgentSource"
```

---

## Task 7: catalog.rs 删 employee_id 字段

**Files:**
- Modify: `src-tauri/src/runtime/tools/catalog.rs:330-380`

- [ ] **Step 1: 改 description 文案**

定位 `c.insert(CatalogEntry::new(ToolDefinition::new("Agent", ...)`，把 description 字符串改为：

```rust
            "Agent",
            "【Composite 工具】启动一个子 Agent 执行聚焦任务。\
            \n\n适用场景：任务需要干净上下文、专属 Agent 类型或不同模型。`subagent_type` 取值范围在每轮 turn 的工具描述动态列表中给出，包含 builtin 类型、用户自定义 agent、以及当前用户已雇佣的数字员工 ID（`emp-...`）。\
            \n\n同步路径（run_in_background=false 或省略）：阻塞等待子 Agent 完成并返回最终输出文本。\
            \n\n异步路径（run_in_background=true）：立即返回 agent_id；子 Agent 在后台运行；用 TaskOutput(task_id=agent_id, offset=N) 增量读取 transcript；子 Agent 完成时父的下一轮会收到 <task-notification> XML。\
            \n\nTeammate 派活路径（subagent_type 选数字员工 + team_name + name）：从该 Employee 加载系统提示和工具白名单，加入当前 Session 的 Team 作为 Teammate 运行。`team_name` 非空时 `name` 为必填。",
```

- [ ] **Step 2: 改 input_schema**

把 `properties` 整段替换为：

```rust
        json!({
            "type": "object",
            "required": ["prompt", "description", "subagent_type"],
            "properties": {
                "subagent_type": {
                    "type": "string",
                    "description": "Agent 类型名称。必须从工具描述中 `<available_subagent_types>` 段列出的清单中精确选择（builtin 如 `general-purpose`、`explore`，或已雇佣的数字员工 ID `emp-…`）。"
                },
                "prompt": {
                    "type": "string",
                    "description": "子 Agent 应执行的完整任务指令。"
                },
                "description": {
                    "type": "string",
                    "description": "3-5 词任务描述，用于日志和 UI 展示。"
                },
                "model": {
                    "type": "string",
                    "description": "为该子 Agent 调用覆盖模型（如 'haiku'）。省略则继承父 Agent 的模型。"
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "若为 true，异步运行并立即返回 agent_id；后续用 TaskOutput 增量读 transcript，完成时父的下一轮收到 <task-notification>。",
                    "default": false
                },
                "name": {
                    "type": "string",
                    "description": "Agent 实例名。team_name 非空时必填（Teammate 派活）；异步子 Agent 也可选填以便 SendMessage 路由。"
                },
                "team_name": {
                    "type": "string",
                    "description": "目标 Team 名称。非空时将此 Agent 作为 Teammate 加入当前 Session 的 Team（Team 必须已通过 TeamCreate 创建）。此时 name 为必填。"
                }
            }
        }),
```

- [ ] **Step 3: 编译确认**

Run: `cd src-tauri && cargo check 2>&1 | tail -10`

Expected: 编译通过。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/runtime/tools/catalog.rs
git commit -m "feat(tool-schema): drop employee_id from Agent input schema"
```

---

## Task 8: Diagnostic / 注释字段名同步 + launcher 错误回灌

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:88, 220`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs:1438`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs:1076, 1127`
- Modify: `src-tauri/src/llm/providers/claude.rs:383, 408`
- Modify: `src-tauri/src/llm/tool_executor/spawn_subagent.rs:90-99`

- [ ] **Step 1: chat_runtime_impl.rs**

注释 line 88 附近：

```rust
///   `<available_employee_ids>` listing (Active employees only)
```

改为：

```rust
///   `<available_subagent_types>` listing (employees混排进单段，由 origin=Employee 区分)
```

`Agent rendered:` log 行（约 line 220）：

```rust
        rendered.description.contains("<available_employee_ids>"),
```

改为：

```rust
        rendered.description.contains("<available_subagent_types>"),
```

字段名 `contains_emp_section` / `contains_emp_id` 保持不变（仍然准确——含 employee 段）。

- [ ] **Step 1.5: chat_turn_driver.rs 两处 diagnostic 字面量**

定位：

```rust
// line 1076
.map(|d| d.contains("<available_employee_ids>"))
// line 1127
.map(|d| d.contains("<available_employee_ids>"))
```

两处都改为：

```rust
.map(|d| d.contains("<available_subagent_types>"))
```

字段名 `agent_desc_has_emp_section` 保持不变。

- [ ] **Step 2: chat.rs:1438**

```rust
                .map(|d| d.description.contains("<available_employee_ids>"))
```

改为：

```rust
                .map(|d| d.description.contains("<available_subagent_types>"))
```

字段名 `agent_desc_has_emp_section` 保持不变。

- [ ] **Step 3: claude.rs:383, 408**

两处 `<available_employee_ids>` 都改为 `<available_subagent_types>`。

- [ ] **Step 4: launcher 兜底错误回灌清单**

`src-tauri/src/llm/tool_executor/spawn_subagent.rs:90-99` 把：

```rust
        let definition = self
            .registry
            .get(&request.subagent_type)
            .ok_or_else(|| {
                anyhow!(
                    "unknown subagent_type '{}' in DefaultSpawnSubagentLauncher",
                    request.subagent_type
                )
            })?
            .clone();
```

改为：

```rust
        let definition = self.registry.get(&request.subagent_type).ok_or_else(|| {
            anyhow!(
                "DefaultSpawnSubagentLauncher: subagent_type '{}' not in AgentRegistry. \
                 Available: {}",
                request.subagent_type,
                self.registry
                    .list()
                    .iter()
                    .map(|d| d.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
```

注：`registry.get` 现在返 owned，删掉 `.clone()`。

- [ ] **Step 5: cargo check + commit**

Run: `cd src-tauri && cargo check --tests 2>&1 | tail -10`

Expected: 编译通过。

```bash
git add src-tauri/src/transport \
        src-tauri/src/runtime/chat/chat_turn_driver.rs \
        src-tauri/src/llm/providers/claude.rs \
        src-tauri/src/llm/tool_executor/spawn_subagent.rs
git commit -m "chore: rename available_employee_ids→subagent_types; launcher errors list available"
```

---

## Task 9: 修 spawn_teammate_via_employee_test 集成测试

**Files:**
- Modify: `src-tauri/tests/spawn_teammate_via_employee_test.rs`

- [ ] **Step 1: grep 找需要改的字段**

Run: `cd src-tauri && grep -n '"employee_id"\|new_with_employees\|EmployeeStore' tests/spawn_teammate_via_employee_test.rs`

- [ ] **Step 2: 把 `"employee_id":` 改为 `"subagent_type":`，删 builtin subagent_type 互斥案例**

每处 `json!({ "employee_id": <emp-id>, ... })` → `json!({ "subagent_type": <emp-id>, ... })`。

互斥测试（`rejects_both_subagent_type_and_employee_id` 类似名）整段删除——互斥语义已不存在。

- [ ] **Step 3: 把 `SpawnSubagentRuntimeTool::new_with_employees` 调用改为 `new` + 在测试内手动 register_dynamic**

定位 `new_with_employees(launcher, registry, store)` 调用处。改造模式：

```rust
let registry = Arc::new(AgentRegistry::with_builtins());
// 测试用：把 store 中的 employee 投影进 registry（模拟 boot 路径）
let records = store.list().unwrap();
for rec in &records {
    if matches!(rec.lifecycle, EmployeeLifecycle::Active) {
        registry.register_dynamic(
            app_lib::runtime::agent::employee_projection::project_employee_to_agent(rec),
        );
    }
}
let tool = SpawnSubagentRuntimeTool::new(launcher, registry);
```

- [ ] **Step 4: 跑测试**

Run: `cd src-tauri && cargo test --test spawn_teammate_via_employee_test -- --nocapture`

Expected: 非 ignore 测试 PASS；`#[ignore]` 维持 ignore。

- [ ] **Step 5: 全量回归**

Run:

```bash
cd src-tauri && cargo test --tests review_ employee_ team_ -- --no-fail-fast 2>&1 | tail -25
```

Expected: 全 PASS（与 baseline 已知 ignore 一致）。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/tests/spawn_teammate_via_employee_test.rs
git commit -m "test: migrate integration tests to single subagent_type field"
```

---

## Task 10: 端到端冒烟 + 文档更新

**Files:**
- Modify: `CLAUDE.md`
- Modify: `docs/superpowers/plans/README.md`

- [ ] **Step 1: 全套 cargo test**

Run:

```bash
cd src-tauri && cargo test --lib --no-fail-fast 2>&1 | tail -20
cd src-tauri && cargo test --tests --no-fail-fast 2>&1 | tail -30
```

Expected: 全 PASS 或与 ltr-mvp baseline 一致的 pre-existing failures。

- [ ] **Step 2: 手动跑 dev（如有 Tauri 环境）**

```bash
pnpm tauri:dev
```

操作流：
1. 雇佣一个 employee → grep `[agent-registry] seeded` 在 `~/.renlijia/logs/renlijia.log` 看到 register_dynamic 行
2. 开 Team 会话发"派小研做调研" → grep `tool-desc-trace` 看到 `Agent rendered: ... contains_emp_id=true`
3. 检查 LLM tool_call args：`subagent_type` 应该是 `emp-...` 而非 `general-purpose`
4. archive 这个 employee → 重派 → 错误 message 列表里不再含它

- [ ] **Step 3: CLAUDE.md 加段落**

在"Skill 系统（新）"段下方新增：

```markdown
### Agent 工具协议（2026-05-12 重构）

`spawn_subagent` 入口只暴露单一 `subagent_type` 字段（不再有 `employee_id` 互斥参数）。
取值范围在每轮 turn 由 `render_dispatch_catalog` 渲染进工具 description 的
`<available_subagent_types>` 段，emp- 开头的数字员工排前面，builtin / user_md 排后面。

来源识别由 `AgentDefinition.origin: AgentSource`（Builtin / UserMarkdown / Employee）
显式表达——这是"是不是 employee"的唯一真值，不依赖任何字符串前缀启发式。

启动时 `seed_registry_from_employees` 把所有 lifecycle=Active 的 employee 投影成
`AgentDefinition` 注册进 `AgentRegistry`；之后 hire / update / archive 通过
`EmployeeAgentSync` 钩子（生产路径 `AgentRegistrySync`）保持同步。
**EmployeeStore 仍是 employee 业务实体的 source of truth**——cron / lifecycle /
resource_config / last_run_at 不在 AgentDefinition 里。

未知 `subagent_type` 错误信息会列出当前可选清单（参考 claude-code-best 的
`AgentTool.tsx:532-536`），让 LLM 下一轮 retry 时立即看到正确选项。
```

- [ ] **Step 4: docs/superpowers/plans/README.md 加索引**

末尾追加：

```markdown
- 2026-05-12-agent-tool-merge-and-registry-projection.md — Agent 工具协议合并 + AgentRegistry 索引 Employee（已实施）
```

- [ ] **Step 5: Commit**

```bash
git add CLAUDE.md docs/superpowers/plans/README.md
git commit -m "docs: agent tool协议合并实施记录"
```

---

## Self-Review

### 1. Spec 覆盖检查

| 需求 | 任务覆盖 |
|---|---|
| 单一 `subagent_type` 入口（emp-id 当 subagent_type） | Task 6, 7 |
| AgentDefinition 加 origin 枚举 | Task 1 |
| AgentRegistry 索引 employee | Task 2, 3, 5 |
| EmployeeStore 不删，不数据迁移 | 全程不写迁移；Task 5 仅升级为单例 |
| 派 Teammate 时 employee_id 由 origin 判定 | Task 6 Step 6 |
| 未知 subagent_type 错误回灌清单 | Task 6 Step 8, Task 8 Step 4 |
| render_dispatch_catalog 单段，emp 在前 | Task 6 Step 9 |
| ToolDescriptionContext 删 employees | Task 6 Step 11-12 |
| 测试覆盖 | Task 1/2/3/4/6 各有针对性单测；Task 9 修集成测试 |

### 2. Placeholder 扫描

- ✅ 无 "TBD" / "TODO" / "fill in"
- ✅ 每个 step 都有具体代码或命令
- ⚠️ Task 6 Step 9 提到"agents 字段含 origin"——这个依赖 Task 6 Step 11 的 `AgentDefSummary.origin`。已经在同一个 task 内闭环（Step 9 与 Step 11 同 commit）。

### 3. 类型一致性

- `AgentSource::{Builtin, UserMarkdown, Employee}` — Task 1 定义；Task 3 投影、Task 6 判定、Task 11 catalog 都用同一 variant 名
- `EmployeeAgentSync` / `AgentRegistrySync` / `NoopSync` — Task 3 定义；Task 4 引用；Task 5 wire `AgentRegistrySync`
- `register_dynamic` / `unregister` — Task 2 定义；Task 3 钩子内调用；Task 5 seed 用 `register_dynamic`
- `build_unknown_subagent_type_error(name, registry)` — Task 6 定义 2 参数；Task 6 测试与 Step 5 调用一致；Task 8 launcher 路径走自己的 anyhow! 不调这个函数，签名独立无冲突
- `project_employee_to_agent(&EmployeeRecord) -> AgentDefinition` — Task 3 定义；Task 5 / Task 9 / `AgentRegistrySync.on_active` 调用一致
- `seed_registry_from_employees(&AgentRegistry, &[EmployeeRecord]) -> usize` — Task 3 定义；Task 5 调用一致
- `EmployeeStore.set_sync(&self, Arc<dyn EmployeeAgentSync>)` — Task 4 定义；Task 5 调用一致

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-05-12-agent-tool-merge-and-registry-projection.md`. Two execution options:**

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

**Which approach?**
