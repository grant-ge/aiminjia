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
    AgentDefinition, AgentModel, AgentPermissionMode, AgentPrompt, AgentSource,
};
use crate::runtime::agent::registry::AgentRegistry;
use crate::runtime::employee::store::{EmployeeLifecycle, EmployeeRecord};

/// 用 EmployeeRecord 派生一个 AgentDefinition。
///
/// `name` 取 employee.id（emp-…）让 LLM 看到的 subagent_type 直接就是
/// employee id；`source` 设为 `Employee` 让 spawn_subagent 派 Teammate 时
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
        source: AgentSource::Employee,
        permission_mode: AgentPermissionMode::AutoDeny,
        background_default: true,
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

    fn mk_req(name: &str, lifecycle: Option<EmployeeLifecycle>) -> CreateEmployeeRequest {
        CreateEmployeeRequest {
            name: name.into(),
            role: "r".into(),
            description: "".into(),
            avatar: "".into(),
            template_id: None,
            tool_whitelist: None,
            cron: None,
            timezone: None,
            lifecycle,
            cron_enabled: None,
            resource_config: None,
            system_prompt_extra: None,
            default_skill_id: None,
        }
    }

    #[test]
    fn project_carries_tool_whitelist_and_prompt_and_source() {
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
        assert_eq!(def.source, AgentSource::Employee);
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
        let active = store.create(mk_req("n1", Some(EmployeeLifecycle::Active))).unwrap();
        let paused = store.create(mk_req("n2", Some(EmployeeLifecycle::Paused))).unwrap();
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
        let rec = store.create(mk_req("n", None)).unwrap();
        sync.on_active(&rec);
        assert!(registry.get(&rec.id).is_some());
        sync.on_inactive(&rec.id);
        assert!(registry.get(&rec.id).is_none());
    }
}
