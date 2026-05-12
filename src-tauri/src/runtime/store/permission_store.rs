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

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStoreSnapshot {
    pub rules: Vec<PermissionRule>,
    #[serde(default)]
    pub legacy: HashMap<String, PolicyDecision>,
    #[serde(default)]
    pub additional_working_dirs: Vec<AdditionalWorkingDirEntry>,
    #[serde(default)]
    pub path_allow_rules: Vec<PathAllowRuleEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdditionalWorkingDirEntry {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PathAllowRuleEntry {
    pub pattern: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<StoredPathOp>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StoredPathOp {
    Read,
    Write,
}

impl From<crate::runtime::path_auth::PathOp> for StoredPathOp {
    fn from(op: crate::runtime::path_auth::PathOp) -> Self {
        use crate::runtime::path_auth::PathOp;
        match op {
            PathOp::Read => StoredPathOp::Read,
            PathOp::Write => StoredPathOp::Write,
        }
    }
}

impl From<StoredPathOp> for crate::runtime::path_auth::PathOp {
    fn from(op: StoredPathOp) -> Self {
        use crate::runtime::path_auth::PathOp;
        match op {
            StoredPathOp::Read => PathOp::Read,
            StoredPathOp::Write => PathOp::Write,
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
    additional_working_dirs: Vec<AdditionalWorkingDirEntry>,
    path_allow_rules: Vec<PathAllowRuleEntry>,
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
            additional_working_dirs: self.additional_working_dirs.clone(),
            path_allow_rules: self.path_allow_rules.clone(),
        }
    }
}

fn split_legacy_key(scope_key: &str) -> Option<(String, String)> {
    let (tool_name, scope) = scope_key.split_once(':')?;
    Some((tool_name.to_string(), scope.to_string()))
}

pub struct PathAuthData {
    pub session_working_dirs: Vec<AdditionalWorkingDirEntry>,
    pub workspace_working_dirs: Vec<AdditionalWorkingDirEntry>,
    pub user_working_dirs: Vec<AdditionalWorkingDirEntry>,
    pub session_allow_rules: Vec<PathAllowRuleEntry>,
    pub workspace_allow_rules: Vec<PathAllowRuleEntry>,
    pub user_allow_rules: Vec<PathAllowRuleEntry>,
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
        layer.additional_working_dirs = snapshot.additional_working_dirs;
        layer.path_allow_rules = snapshot.path_allow_rules;
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

    pub(crate) fn path_auth_data(&self) -> PathAuthData {
        let session_working_dirs = self
            .session
            .read()
            .unwrap()
            .additional_working_dirs
            .clone();
        let session_allow_rules = self
            .session
            .read()
            .unwrap()
            .path_allow_rules
            .clone();

        let workspace_working_dirs = self
            .workspace
            .read()
            .unwrap()
            .additional_working_dirs
            .clone();
        let workspace_allow_rules = self
            .workspace
            .read()
            .unwrap()
            .path_allow_rules
            .clone();

        let user_working_dirs = self
            .user
            .read()
            .unwrap()
            .additional_working_dirs
            .clone();
        let user_allow_rules = self
            .user
            .read()
            .unwrap()
            .path_allow_rules
            .clone();

        PathAuthData {
            session_working_dirs,
            workspace_working_dirs,
            user_working_dirs,
            session_allow_rules,
            workspace_allow_rules,
            user_allow_rules,
        }
    }

    fn flush_workspace(&self) {
        if let Err(err) = self.flush_workspace_result() {
            log::warn!(
                "[PermissionStore] Failed to flush workspace permission snapshot: {}",
                err
            );
        }
    }

    fn flush_user(&self) {
        if let Err(err) = self.flush_user_result() {
            log::warn!(
                "[PermissionStore] Failed to flush user permission snapshot: {}",
                err
            );
        }
    }

    fn flush_workspace_result(&self) -> std::io::Result<()> {
        if let Some(path) = &self.workspace_file_path {
            let snapshot = self
                .workspace
                .read()
                .unwrap()
                .snapshot_with_source(PermissionSource::Workspace);
            atomic_write_json(path, &snapshot)?;
        }
        Ok(())
    }

    fn flush_user_result(&self) -> std::io::Result<()> {
        if let Some(path) = &self.user_file_path {
            let snapshot = self
                .user
                .read()
                .unwrap()
                .snapshot_with_source(PermissionSource::User);
            atomic_write_json(path, &snapshot)?;
        }
        Ok(())
    }

    pub fn append_working_dir(
        &self,
        destination: crate::runtime::tools::permission::PermissionDestination,
        path: PathBuf,
    ) -> std::io::Result<()> {
        match destination {
            crate::runtime::tools::permission::PermissionDestination::Session => {
                let mut layer = self.session.write().unwrap();
                if !layer.additional_working_dirs.iter().any(|e| e.path == path) {
                    layer.additional_working_dirs.push(AdditionalWorkingDirEntry { path });
                }
                Ok(())
            }
            crate::runtime::tools::permission::PermissionDestination::Workspace => {
                {
                    let mut layer = self.workspace.write().unwrap();
                    if !layer.additional_working_dirs.iter().any(|e| e.path == path) {
                        layer.additional_working_dirs.push(AdditionalWorkingDirEntry { path });
                    }
                }
                self.flush_workspace_result()
            }
            crate::runtime::tools::permission::PermissionDestination::User => {
                {
                    let mut layer = self.user.write().unwrap();
                    if !layer.additional_working_dirs.iter().any(|e| e.path == path) {
                        layer.additional_working_dirs.push(AdditionalWorkingDirEntry { path });
                    }
                }
                self.flush_user_result()
            }
        }
    }

    pub fn append_path_allow_rule(
        &self,
        destination: crate::runtime::tools::permission::PermissionDestination,
        pattern: String,
        op: Option<crate::runtime::path_auth::PathOp>,
    ) -> std::io::Result<()> {
        let stored_op = op.map(StoredPathOp::from);
        let entry = PathAllowRuleEntry { pattern, op: stored_op };
        match destination {
            crate::runtime::tools::permission::PermissionDestination::Session => {
                let mut layer = self.session.write().unwrap();
                if !layer.path_allow_rules.iter().any(|e| e.pattern == entry.pattern && e.op == entry.op) {
                    layer.path_allow_rules.push(entry);
                }
                Ok(())
            }
            crate::runtime::tools::permission::PermissionDestination::Workspace => {
                {
                    let mut layer = self.workspace.write().unwrap();
                    if !layer.path_allow_rules.iter().any(|e| e.pattern == entry.pattern && e.op == entry.op) {
                        layer.path_allow_rules.push(entry);
                    }
                }
                self.flush_workspace_result()
            }
            crate::runtime::tools::permission::PermissionDestination::User => {
                {
                    let mut layer = self.user.write().unwrap();
                    if !layer.path_allow_rules.iter().any(|e| e.pattern == entry.pattern && e.op == entry.op) {
                        layer.path_allow_rules.push(entry);
                    }
                }
                self.flush_user_result()
            }
        }
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
                "Bash",
                PermissionScope::Scope("network".to_string()),
                PolicyDecision::AlwaysAllow,
                PermissionSource::User,
            ),
        );
        store.record_to(
            PermissionDestination::Workspace,
            PermissionRule::simple(
                "Bash",
                PermissionScope::Scope("network".to_string()),
                PolicyDecision::AlwaysDeny,
                PermissionSource::Workspace,
            ),
        );

        assert_eq!(
            store.get_for_scope("Bash", "network"),
            Some(PolicyDecision::AlwaysDeny)
        );
    }

    #[test]
    fn test_session_overrides_workspace_and_user() {
        let store = PermissionStore::in_memory();
        store.record_to(
            PermissionDestination::User,
            PermissionRule::simple(
                "Bash",
                PermissionScope::Scope("network".to_string()),
                PolicyDecision::AlwaysDeny,
                PermissionSource::User,
            ),
        );
        store.record_to(
            PermissionDestination::Workspace,
            PermissionRule::simple(
                "Bash",
                PermissionScope::Scope("network".to_string()),
                PolicyDecision::AlwaysDeny,
                PermissionSource::Workspace,
            ),
        );
        store.record_to(
            PermissionDestination::Session,
            PermissionRule::simple(
                "Bash",
                PermissionScope::Scope("network".to_string()),
                PolicyDecision::Allow,
                PermissionSource::Session,
            ),
        );

        assert_eq!(
            store.get_for_scope("Bash", "network"),
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
                ..Default::default()
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

    #[test]
    fn path_auth_load_includes_additional_working_dirs() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("perms.json");
        let entry_path = PathBuf::from("/tmp/my-project");
        let snapshot = PermissionStoreSnapshot {
            additional_working_dirs: vec![AdditionalWorkingDirEntry { path: entry_path.clone() }],
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&snapshot).unwrap()).unwrap();

        let store = PermissionStore::with_file(path);
        let entries = crate::runtime::path_auth::store_bridge::load_path_auth_entries(&store);
        assert!(entries.working_dirs.contains_key(&entry_path));
    }

    #[test]
    fn path_auth_load_includes_path_allow_rules() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("perms.json");
        let snapshot = PermissionStoreSnapshot {
            path_allow_rules: vec![PathAllowRuleEntry {
                pattern: "/tmp/data/**".to_string(),
                op: Some(StoredPathOp::Read),
            }],
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&snapshot).unwrap()).unwrap();

        let store = PermissionStore::with_file(path);
        let entries = crate::runtime::path_auth::store_bridge::load_path_auth_entries(&store);
        assert_eq!(entries.allow_rules.len(), 1);
        assert_eq!(entries.allow_rules[0].pattern, "/tmp/data/**");
    }

    #[test]
    fn path_auth_write_atomic_on_user_grant_workingdir() {
        let temp = TempDir::new().expect("tempdir");
        let user_path = temp.path().join("user.json");
        let store = PermissionStore::with_layer_files(None, Some(user_path.clone()));
        let p = PathBuf::from("/tmp/user-project");
        store.append_working_dir(PermissionDestination::User, p.clone()).unwrap();

        let content = std::fs::read_to_string(&user_path).unwrap();
        assert!(
            content.contains("\"additionalWorkingDirs\""),
            "expected camelCase key in JSON, got: {}",
            content
        );
        let snapshot: PermissionStoreSnapshot = serde_json::from_str(&content).unwrap();
        assert!(snapshot.additional_working_dirs.iter().any(|e| e.path == p));
    }

    #[test]
    fn path_auth_write_atomic_on_user_grant_allow_rule() {
        let temp = TempDir::new().expect("tempdir");
        let user_path = temp.path().join("user.json");
        let store = PermissionStore::with_layer_files(None, Some(user_path.clone()));
        store
            .append_path_allow_rule(
                PermissionDestination::User,
                "/tmp/**".to_string(),
                Some(crate::runtime::path_auth::PathOp::Write),
            )
            .unwrap();

        let content = std::fs::read_to_string(&user_path).unwrap();
        assert!(
            content.contains("\"pathAllowRules\""),
            "expected camelCase key in JSON, got: {}",
            content
        );
        let snapshot: PermissionStoreSnapshot = serde_json::from_str(&content).unwrap();
        assert!(snapshot.path_allow_rules.iter().any(|e| e.pattern == "/tmp/**" && e.op == Some(StoredPathOp::Write)));
    }

    #[test]
    fn path_auth_backward_compat_no_new_fields() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("old.json");
        std::fs::write(&path, r#"{"rules":[],"legacy":{}}"#).unwrap();

        let store = PermissionStore::with_file(path);
        let entries = crate::runtime::path_auth::store_bridge::load_path_auth_entries(&store);
        assert!(entries.working_dirs.is_empty());
        assert!(entries.allow_rules.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn path_auth_write_failure_keeps_inmemory_grant() {
        use std::os::unix::fs::PermissionsExt;
        let temp = TempDir::new().expect("tempdir");
        let dir = temp.path().join("readonly");
        std::fs::create_dir(&dir).unwrap();
        let ws_path = dir.join("workspace.json");
        // make dir read-only so writes fail
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();

        let store = PermissionStore::with_layer_files(Some(ws_path), None);
        let p = PathBuf::from("/tmp/kept-in-memory");
        let result = store.append_working_dir(PermissionDestination::Workspace, p.clone());
        assert!(result.is_err());
        let entries = crate::runtime::path_auth::store_bridge::load_path_auth_entries(&store);
        assert!(entries.working_dirs.contains_key(&p));
    }

    #[test]
    fn path_auth_dedup_working_dir_idempotent() {
        let store = PermissionStore::in_memory();
        let p = PathBuf::from("/tmp/dedup-test");
        store.append_working_dir(PermissionDestination::Session, p.clone()).unwrap();
        store.append_working_dir(PermissionDestination::Session, p.clone()).unwrap();
        let entries = crate::runtime::path_auth::store_bridge::load_path_auth_entries(&store);
        let count = entries.working_dirs.keys().filter(|k| **k == p).count();
        assert_eq!(count, 1);
    }

    #[test]
    fn path_auth_dedup_allow_rule_idempotent() {
        let store = PermissionStore::in_memory();
        store
            .append_path_allow_rule(
                PermissionDestination::Session,
                "/tmp/**".to_string(),
                Some(crate::runtime::path_auth::PathOp::Read),
            )
            .unwrap();
        store
            .append_path_allow_rule(
                PermissionDestination::Session,
                "/tmp/**".to_string(),
                Some(crate::runtime::path_auth::PathOp::Read),
            )
            .unwrap();
        let entries = crate::runtime::path_auth::store_bridge::load_path_auth_entries(&store);
        let count = entries.allow_rules.iter().filter(|r| r.pattern == "/tmp/**").count();
        assert_eq!(count, 1);
    }
}
