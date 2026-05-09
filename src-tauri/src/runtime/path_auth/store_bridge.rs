use std::collections::HashMap;
use std::path::PathBuf;

use crate::runtime::store::permission_store::{PermissionSource, PermissionStore, StoredPathOp};
use super::context::{PermissionRule, RuleSource};
use super::op::PathOp;

pub struct PathAuthEntries {
    pub working_dirs: HashMap<PathBuf, RuleSource>,
    pub allow_rules: Vec<PermissionRule>,
}

fn source_to_rule_source(source: PermissionSource) -> RuleSource {
    match source {
        PermissionSource::Session => RuleSource::Session,
        // Why: spec §4.1 defines only two RuleSource values; both Workspace and User
        // map to UserSettings on the path_auth side.
        PermissionSource::Workspace | PermissionSource::User => RuleSource::UserSettings,
    }
}

pub fn load_path_auth_entries(store: &PermissionStore) -> PathAuthEntries {
    let path_auth_data = store.path_auth_data();
    let mut working_dirs: HashMap<PathBuf, RuleSource> = HashMap::new();
    let mut allow_rules: Vec<PermissionRule> = Vec::new();

    // working_dirs: process user → workspace → session so that more ephemeral layers win.
    for (entries, source) in &[
        (path_auth_data.user_working_dirs, PermissionSource::User),
        (path_auth_data.workspace_working_dirs, PermissionSource::Workspace),
        (path_auth_data.session_working_dirs, PermissionSource::Session),
    ] {
        let rule_source = source_to_rule_source(*source);
        for entry in entries {
            working_dirs.insert(entry.path.clone(), rule_source.clone());
        }
    }

    // allow_rules: session → workspace → user to match step-5 precedence in decide.rs.
    for (entries, source) in &[
        (path_auth_data.session_allow_rules, PermissionSource::Session),
        (path_auth_data.workspace_allow_rules, PermissionSource::Workspace),
        (path_auth_data.user_allow_rules, PermissionSource::User),
    ] {
        let rule_source = source_to_rule_source(*source);
        for entry in entries {
            allow_rules.push(PermissionRule {
                pattern: entry.pattern.clone(),
                op: entry.op.map(PathOp::from),
                source: rule_source.clone(),
            });
        }
    }

    PathAuthEntries { working_dirs, allow_rules }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::path_auth::PathOp;
    use crate::runtime::store::permission_store::PermissionStore;
    use crate::runtime::tools::permission::PermissionDestination;

    #[test]
    fn bridge_maps_session_source_to_session() {
        let store = PermissionStore::in_memory();
        let p = PathBuf::from("/tmp/session-dir");
        store.append_working_dir(PermissionDestination::Session, p.clone()).unwrap();

        let entries = load_path_auth_entries(&store);
        assert_eq!(entries.working_dirs.get(&p), Some(&RuleSource::Session));
    }

    #[test]
    fn bridge_maps_workspace_to_user_settings() {
        let store = PermissionStore::in_memory();
        let p = PathBuf::from("/tmp/workspace-dir");
        store.append_working_dir(PermissionDestination::Workspace, p.clone()).unwrap();

        let entries = load_path_auth_entries(&store);
        assert_eq!(entries.working_dirs.get(&p), Some(&RuleSource::UserSettings));
    }

    #[test]
    fn bridge_maps_user_to_user_settings() {
        let store = PermissionStore::in_memory();
        let p = PathBuf::from("/tmp/user-dir");
        store.append_working_dir(PermissionDestination::User, p.clone()).unwrap();

        let entries = load_path_auth_entries(&store);
        assert_eq!(entries.working_dirs.get(&p), Some(&RuleSource::UserSettings));
    }

    #[test]
    fn bridge_aggregates_three_layers() {
        let store = PermissionStore::in_memory();
        let p_session = PathBuf::from("/tmp/bridge-session");
        let p_workspace = PathBuf::from("/tmp/bridge-workspace");
        let p_user = PathBuf::from("/tmp/bridge-user");
        store.append_working_dir(PermissionDestination::Session, p_session.clone()).unwrap();
        store.append_working_dir(PermissionDestination::Workspace, p_workspace.clone()).unwrap();
        store.append_working_dir(PermissionDestination::User, p_user.clone()).unwrap();

        let entries = load_path_auth_entries(&store);
        assert!(entries.working_dirs.contains_key(&p_session));
        assert!(entries.working_dirs.contains_key(&p_workspace));
        assert!(entries.working_dirs.contains_key(&p_user));
    }

    #[test]
    fn bridge_session_layer_wins_over_workspace_for_duplicate_path() {
        let store = PermissionStore::in_memory();
        let p = PathBuf::from("/tmp/dup-path");
        store.append_working_dir(PermissionDestination::Workspace, p.clone()).unwrap();
        store.append_working_dir(PermissionDestination::Session, p.clone()).unwrap();

        let entries = load_path_auth_entries(&store);
        assert_eq!(entries.working_dirs.get(&p), Some(&RuleSource::Session));
    }

    #[test]
    fn bridge_allow_rules_preserve_layer_order_session_first() {
        let store = PermissionStore::in_memory();
        store
            .append_path_allow_rule(
                PermissionDestination::Session,
                "/tmp/session/**".to_string(),
                Some(PathOp::Read),
            )
            .unwrap();
        store
            .append_path_allow_rule(
                PermissionDestination::Workspace,
                "/tmp/workspace/**".to_string(),
                Some(PathOp::Write),
            )
            .unwrap();
        store
            .append_path_allow_rule(
                PermissionDestination::User,
                "/tmp/user/**".to_string(),
                None,
            )
            .unwrap();

        let entries = load_path_auth_entries(&store);
        assert_eq!(entries.allow_rules.len(), 3);
        assert_eq!(entries.allow_rules[0].pattern, "/tmp/session/**");
        assert_eq!(entries.allow_rules[0].source, RuleSource::Session);
        assert_eq!(entries.allow_rules[1].pattern, "/tmp/workspace/**");
        assert_eq!(entries.allow_rules[1].source, RuleSource::UserSettings);
        assert_eq!(entries.allow_rules[2].pattern, "/tmp/user/**");
        assert_eq!(entries.allow_rules[2].source, RuleSource::UserSettings);
    }
}
