//! 权限规则存储。
//!
//! Plan-U2 的最小落地版本采用三层本地来源：
//! - `session`：只在当前进程内有效
//! - `workspace`：当前工作区配置
//! - `user`：当前用户全局配置
//!
//! 兼容旧格式 `tool:scope -> PolicyDecision`，避免现有测试与历史数据立即失效。

use crate::storage::file_store::io::atomic_write_json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

/// Minimal glob matching supporting `**` (any path segments) and `*` (within one segment).
fn glob_matches(glob: &str, path: &str) -> bool {
    glob_matches_inner(glob.as_bytes(), path.as_bytes())
}

fn glob_matches_inner(pattern: &[u8], text: &[u8]) -> bool {
    match (pattern, text) {
        ([], []) => true,
        ([], _) => false,
        ([b'*', b'*', rest @ ..], _) => {
            for i in 0..=text.len() {
                if glob_matches_inner(rest, &text[i..]) {
                    return true;
                }
            }
            false
        }
        ([b'*', rest @ ..], _) => {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionSource {
    Session,
    Workspace,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum PermissionScope {
    Scope(String),
    PathGlob(String),
    CommandPattern(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRule {
    pub tool_name: String,
    pub scope: PermissionScope,
    pub decision: PolicyDecision,
    pub source: PermissionSource,
}

impl PermissionRule {
    pub fn simple(
        tool_name: impl Into<String>,
        scope: PermissionScope,
        decision: PolicyDecision,
        source: PermissionSource,
    ) -> Self {
        Self {
            tool_name: tool_name.into(),
            scope,
            decision,
            source,
        }
    }

    pub fn legacy_key(&self) -> Option<String> {
        match &self.scope {
            PermissionScope::Scope(scope) => Some(format!("{}:{}", self.tool_name, scope)),
            PermissionScope::PathGlob(_) | PermissionScope::CommandPattern(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStoreSnapshot {
    pub rules: Vec<PermissionRule>,
    #[serde(default)]
    pub legacy: HashMap<String, PolicyDecision>,
}

impl Default for PermissionStoreSnapshot {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            legacy: HashMap::new(),
        }
    }
}

/// 权限决策结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyDecision {
    Allow,
    AlwaysAllow,
    Deny,
    AlwaysDeny,
}

impl PolicyDecision {
    pub fn is_allow(&self) -> bool {
        matches!(self, PolicyDecision::Allow | PolicyDecision::AlwaysAllow)
    }

    pub fn is_persistent(&self) -> bool {
        matches!(
            self,
            PolicyDecision::AlwaysAllow | PolicyDecision::AlwaysDeny
        )
    }
}

#[derive(Default)]
struct PermissionLayer {
    rules: Vec<PermissionRule>,
    legacy: HashMap<String, PolicyDecision>,
}

impl PermissionLayer {
    fn upsert_rule(&mut self, rule: PermissionRule) {
        if let Some(existing) = self.rules.iter_mut().find(|candidate| {
            candidate.tool_name == rule.tool_name && candidate.scope == rule.scope
        }) {
            *existing = rule.clone();
        } else {
            self.rules.push(rule.clone());
        }

        if let Some(key) = rule.legacy_key() {
            self.legacy.insert(key, rule.decision);
        }
    }

    fn record_legacy(
        &mut self,
        scope_key: String,
        decision: PolicyDecision,
        source: PermissionSource,
    ) {
        self.legacy.insert(scope_key.clone(), decision.clone());

        if let Some((tool_name, scope)) = split_legacy_key(&scope_key) {
            self.upsert_rule(PermissionRule::simple(
                tool_name,
                PermissionScope::Scope(scope),
                decision,
                source,
            ));
        }
    }

    fn get_rule(&self, tool_name: &str, scope: &PermissionScope) -> Option<PolicyDecision> {
        self.rules
            .iter()
            .find(|rule| rule.tool_name == tool_name && &rule.scope == scope)
            .map(|rule| rule.decision.clone())
    }

    fn get_legacy(&self, scope_key: &str) -> Option<PolicyDecision> {
        self.legacy.get(scope_key).cloned()
    }

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

    fn snapshot_with_source(&self, source: PermissionSource) -> PermissionStoreSnapshot {
        let rules = self
            .rules
            .iter()
            .cloned()
            .map(|mut rule| {
                rule.source = source;
                rule
            })
            .collect();
        PermissionStoreSnapshot {
            rules,
            legacy: self.legacy.clone(),
        }
    }
}

fn split_legacy_key(scope_key: &str) -> Option<(String, String)> {
    let (tool_name, scope) = scope_key.split_once(':')?;
    Some((tool_name.to_string(), scope.to_string()))
}

fn load_snapshot(path: &PathBuf, source: PermissionSource) -> PermissionLayer {
    let mut layer = PermissionLayer::default();
    if !path.exists() {
        return layer;
    }

    let Ok(content) = std::fs::read_to_string(path) else {
        return layer;
    };

    if let Ok(snapshot) = serde_json::from_str::<PermissionStoreSnapshot>(&content) {
        layer.rules = snapshot
            .rules
            .into_iter()
            .map(|mut rule| {
                rule.source = source;
                rule
            })
            .collect();
        layer.legacy = snapshot.legacy;
        return layer;
    }

    if let Ok(legacy) = serde_json::from_str::<HashMap<String, PolicyDecision>>(&content) {
        for (key, decision) in legacy {
            layer.record_legacy(key, decision, source);
        }
    }

    layer
}

pub struct PermissionStore {
    session: RwLock<PermissionLayer>,
    workspace: RwLock<PermissionLayer>,
    user: RwLock<PermissionLayer>,
    workspace_file_path: Option<PathBuf>,
    user_file_path: Option<PathBuf>,
}

impl PermissionStore {
    pub fn in_memory() -> Self {
        Self {
            session: RwLock::new(PermissionLayer::default()),
            workspace: RwLock::new(PermissionLayer::default()),
            user: RwLock::new(PermissionLayer::default()),
            workspace_file_path: None,
            user_file_path: None,
        }
    }

    pub fn with_file(path: PathBuf) -> Self {
        Self::with_layer_files(Some(path), None)
    }

    pub fn with_layer_files(
        workspace_file_path: Option<PathBuf>,
        user_file_path: Option<PathBuf>,
    ) -> Self {
        let workspace = workspace_file_path
            .as_ref()
            .map(|path| load_snapshot(path, PermissionSource::Workspace))
            .unwrap_or_default();
        let user = user_file_path
            .as_ref()
            .map(|path| load_snapshot(path, PermissionSource::User))
            .unwrap_or_default();

        Self {
            session: RwLock::new(PermissionLayer::default()),
            workspace: RwLock::new(workspace),
            user: RwLock::new(user),
            workspace_file_path,
            user_file_path,
        }
    }

    pub fn get(&self, scope_key: &str) -> Option<PolicyDecision> {
        self.session
            .read()
            .unwrap()
            .get_legacy(scope_key)
            .or_else(|| self.workspace.read().unwrap().get_legacy(scope_key))
            .or_else(|| self.user.read().unwrap().get_legacy(scope_key))
    }

    pub fn get_for_scope(&self, tool_name: &str, scope: &str) -> Option<PolicyDecision> {
        let permission_scope = PermissionScope::Scope(scope.to_string());
        self.session
            .read()
            .unwrap()
            .get_rule(tool_name, &permission_scope)
            .or_else(|| {
                self.workspace
                    .read()
                    .unwrap()
                    .get_rule(tool_name, &permission_scope)
            })
            .or_else(|| {
                self.user
                    .read()
                    .unwrap()
                    .get_rule(tool_name, &permission_scope)
            })
            .or_else(|| self.get(&format!("{}:{}", tool_name, scope)))
    }

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
            .or_else(|| self.user.read().unwrap().get_for_glob_path(tool_name, path))
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

    pub fn record(&self, scope_key: String, decision: PolicyDecision) {
        let destination = if decision.is_persistent() {
            PermissionSource::Workspace
        } else {
            PermissionSource::Session
        };
        self.record_legacy_to(destination, scope_key, decision);
    }

    pub fn record_to(
        &self,
        destination: crate::runtime::tools::permission::PermissionDestination,
        rule: PermissionRule,
    ) {
        match destination {
            crate::runtime::tools::permission::PermissionDestination::Session => {
                self.session.write().unwrap().upsert_rule(rule);
            }
            crate::runtime::tools::permission::PermissionDestination::Workspace => {
                self.workspace.write().unwrap().upsert_rule(rule);
                self.flush_workspace();
            }
            crate::runtime::tools::permission::PermissionDestination::User => {
                self.user.write().unwrap().upsert_rule(rule);
                self.flush_user();
            }
        }
    }

    pub fn record_legacy_to(
        &self,
        source: PermissionSource,
        scope_key: String,
        decision: PolicyDecision,
    ) {
        match source {
            PermissionSource::Session => {
                self.session
                    .write()
                    .unwrap()
                    .record_legacy(scope_key, decision, source);
            }
            PermissionSource::Workspace => {
                self.workspace
                    .write()
                    .unwrap()
                    .record_legacy(scope_key, decision, source);
                self.flush_workspace();
            }
            PermissionSource::User => {
                self.user
                    .write()
                    .unwrap()
                    .record_legacy(scope_key, decision, source);
                self.flush_user();
            }
        }
    }

    fn flush_workspace(&self) {
        if let Some(path) = &self.workspace_file_path {
            let snapshot = self
                .workspace
                .read()
                .unwrap()
                .snapshot_with_source(PermissionSource::Workspace);
            flush_snapshot(path, &snapshot);
        }
    }

    fn flush_user(&self) {
        if let Some(path) = &self.user_file_path {
            let snapshot = self
                .user
                .read()
                .unwrap()
                .snapshot_with_source(PermissionSource::User);
            flush_snapshot(path, &snapshot);
        }
    }
}

fn flush_snapshot(path: &PathBuf, snapshot: &PermissionStoreSnapshot) {
    if let Err(err) = atomic_write_json(path, snapshot) {
        log::warn!(
            "[PermissionStore] Failed to flush permission snapshot to {:?}: {}",
            path,
            err
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tools::permission::PermissionDestination;
    use tempfile::TempDir;

    #[test]
    fn test_session_decision_allow() {
        let store = PermissionStore::in_memory();
        assert!(store.get("workspace:read").is_none());
        store.record("workspace:read".to_string(), PolicyDecision::Allow);
        assert_eq!(store.get("workspace:read"), Some(PolicyDecision::Allow));
    }

    #[test]
    fn test_workspace_overrides_user() {
        let store = PermissionStore::in_memory();
        store.record_to(
            PermissionDestination::User,
            PermissionRule::simple(
                "browser",
                PermissionScope::Scope("network".to_string()),
                PolicyDecision::AlwaysAllow,
                PermissionSource::User,
            ),
        );
        store.record_to(
            PermissionDestination::Workspace,
            PermissionRule::simple(
                "browser",
                PermissionScope::Scope("network".to_string()),
                PolicyDecision::AlwaysDeny,
                PermissionSource::Workspace,
            ),
        );

        assert_eq!(
            store.get_for_scope("browser", "network"),
            Some(PolicyDecision::AlwaysDeny)
        );
    }

    #[test]
    fn test_session_overrides_workspace_and_user() {
        let store = PermissionStore::in_memory();
        store.record_to(
            PermissionDestination::User,
            PermissionRule::simple(
                "browser",
                PermissionScope::Scope("network".to_string()),
                PolicyDecision::AlwaysDeny,
                PermissionSource::User,
            ),
        );
        store.record_to(
            PermissionDestination::Workspace,
            PermissionRule::simple(
                "browser",
                PermissionScope::Scope("network".to_string()),
                PolicyDecision::AlwaysDeny,
                PermissionSource::Workspace,
            ),
        );
        store.record_to(
            PermissionDestination::Session,
            PermissionRule::simple(
                "browser",
                PermissionScope::Scope("network".to_string()),
                PolicyDecision::Allow,
                PermissionSource::Session,
            ),
        );

        assert_eq!(
            store.get_for_scope("browser", "network"),
            Some(PolicyDecision::Allow)
        );
    }

    #[test]
    fn test_with_layer_files_reads_legacy_and_snapshot() {
        let temp = TempDir::new().expect("tempdir");
        let workspace_path = temp.path().join("workspace.json");
        let user_path = temp.path().join("user.json");

        std::fs::write(
            &workspace_path,
            serde_json::to_string(&HashMap::from([(
                "bash:workspace:write".to_string(),
                PolicyDecision::AlwaysDeny,
            )]))
            .expect("serialize workspace"),
        )
        .expect("write workspace");

        std::fs::write(
            &user_path,
            serde_json::to_string(&PermissionStoreSnapshot {
                rules: vec![PermissionRule::simple(
                    "WebSearch",
                    PermissionScope::Scope("network".to_string()),
                    PolicyDecision::AlwaysAllow,
                    PermissionSource::User,
                )],
                legacy: HashMap::new(),
            })
            .expect("serialize user"),
        )
        .expect("write user");

        let store = PermissionStore::with_layer_files(Some(workspace_path), Some(user_path));
        assert_eq!(
            store.get("bash:workspace:write"),
            Some(PolicyDecision::AlwaysDeny)
        );
        assert_eq!(
            store.get_for_scope("WebSearch", "network"),
            Some(PolicyDecision::AlwaysAllow)
        );
    }
}
