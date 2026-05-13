use std::collections::HashMap;
use std::sync::RwLock;

use crate::runtime::agent::builtin::{
    daily_assistant_agent::daily_assistant_agent_definition, explore::explore_agent_definition,
    general_purpose::general_purpose_agent_definition,
};
use crate::runtime::agent::definition::AgentDefinition;

/// Builtin 名字保护清单——`unregister` 拒绝删除这些项。
/// 派 Teammate 时这些是终极兜底，绝对不能被业务路径意外移除。
const PROTECTED_NAMES: &[&str] = &["general-purpose", "explore", "daily_assistant_agent"];

/// AgentRegistry 是 spawn_subagent 派活的唯一查询入口。
///
/// 内部用 `RwLock<HashMap>`：
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
    /// 语义同 `register`；分开命名让调用点意图清晰、便于 grep。
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

    /// 返 owned `AgentDefinition`（加 RwLock 后 lifetime 不能跨 read guard）。
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::agent::definition::{
        AgentModel, AgentPermissionMode, AgentPrompt, AgentSource,
    };

    fn mk_def(name: &str, source: AgentSource) -> AgentDefinition {
        AgentDefinition {
            name: name.into(),
            description: "".into(),
            allowed_tools: vec![],
            disallowed_tools: vec![],
            max_iterations: 10,
            model: AgentModel::Inherit,
            system_prompt: AgentPrompt::Inline("".into()),
            source,
            permission_mode: AgentPermissionMode::Bubble,
            background_default: false,
        }
    }

    #[test]
    fn register_dynamic_and_get() {
        let reg = AgentRegistry::with_builtins();
        reg.register_dynamic(mk_def("emp-x", AgentSource::Employee));
        let got = reg.get("emp-x").expect("should find emp-x");
        assert_eq!(got.source, AgentSource::Employee);
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
