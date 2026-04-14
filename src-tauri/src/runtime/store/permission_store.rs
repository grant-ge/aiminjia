//! 权限决策持久化存储。
//!
//! 存储用户对工具 capability scope 的授权决策，支持“本次允许”和“始终允许”两种模式。
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
    /// 创建内存 only 的存储（测试用）。
    pub fn in_memory() -> Self {
        Self {
            persistent: RwLock::new(HashMap::new()),
            session: RwLock::new(HashMap::new()),
            file_path: None,
        }
    }

    /// 创建带持久化的存储。
    pub fn with_file(path: PathBuf) -> Self {
        let store = Self {
            persistent: RwLock::new(HashMap::new()),
            session: RwLock::new(HashMap::new()),
            file_path: Some(path.clone()),
        };
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
            drop(p);
            self.flush_persistent();
        } else {
            self.session.write().unwrap().insert(scope_key, decision);
        }
    }

    fn flush_persistent(&self) {
        if let Some(path) = &self.file_path {
            let map = self.persistent.read().unwrap();
            if let Ok(json) = serde_json::to_string_pretty(&*map) {
                if let Err(e) = std::fs::write(path, json) {
                    log::warn!(
                        "[PermissionStore] Failed to flush persistent decisions to {:?}: {}",
                        path,
                        e
                    );
                }
            }
        }
    }
}

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
