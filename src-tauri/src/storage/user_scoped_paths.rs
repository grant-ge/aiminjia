use std::path::{Path, PathBuf};

/// Immutable snapshot of all paths for the current user scope.
/// Constructed once per scope activation, cloned freely, never mutated.
#[derive(Debug, Clone)]
pub struct UserScopedPaths {
    base: PathBuf,
}

impl UserScopedPaths {
    pub fn new(root: &Path, scope_key: &str) -> Self {
        Self {
            base: root.join("users").join(scope_key),
        }
    }

    pub fn base_dir(&self) -> PathBuf {
        self.base.clone()
    }
    pub fn config_path(&self) -> PathBuf {
        self.base.join("config.json")
    }
    pub fn scope_json_path(&self) -> PathBuf {
        self.base.join("scope.json")
    }
    pub fn index_path(&self) -> PathBuf {
        self.base.join("index.json")
    }
    pub fn conversations_dir(&self) -> PathBuf {
        self.base.join("conversations")
    }
    pub fn shared_dir(&self) -> PathBuf {
        self.base.join("shared")
    }
    pub fn memory_dir(&self) -> PathBuf {
        self.base.join("shared").join("memory")
    }
    pub fn cognitive_dir(&self) -> PathBuf {
        self.base.join("shared").join("cognitive")
    }
    pub fn cache_dir(&self) -> PathBuf {
        self.base.join("shared").join("cache")
    }
    pub fn schedules_dir(&self) -> PathBuf {
        self.base.join("schedules")
    }
    pub fn agenda_dir(&self) -> PathBuf {
        self.base.join("agenda")
    }
    pub fn permissions_path(&self) -> PathBuf {
        self.base.join("permissions.json")
    }
    pub fn mcp_config_path(&self) -> PathBuf {
        self.base.join("mcp_servers.json")
    }
    pub fn skills_dir(&self) -> PathBuf {
        self.base.join("skills")
    }
    pub fn agents_dir(&self) -> PathBuf {
        self.base.join("agents")
    }
    pub fn agent_invocations_path(&self) -> PathBuf {
        self.base.join("agent_invocations.json")
    }
    pub fn subagent_transcripts_dir(&self) -> PathBuf {
        self.base.join("subagent_transcripts")
    }
    pub fn project_memories_dir(&self) -> PathBuf {
        self.base.join("project_memories")
    }
    pub fn api_data_dir(&self) -> PathBuf {
        self.base.join("api-data")
    }
    pub fn screenshots_dir(&self) -> PathBuf {
        self.base.join("screenshots")
    }
    pub fn site_profiles_dir(&self) -> PathBuf {
        self.base.join("site-profiles")
    }
    pub fn audit_dir(&self) -> PathBuf {
        self.base.join("audit")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.base.join("logs")
    }
    pub fn downloads_dir(&self) -> PathBuf {
        self.base.join("downloads")
    }
    pub fn employees_dir(&self) -> PathBuf {
        self.base.join("employees")
    }
}

/// Trait for services that need user-scoped paths.
/// Services depend on this trait, not on AiJiaHome or UserScope directly.
pub trait UserScopedPathResolver: Send + Sync {
    /// Returns a paths snapshot if a user is logged in, None otherwise.
    fn resolve_paths(&self) -> Option<UserScopedPaths>;

    /// Returns a paths snapshot or error if not logged in.
    fn require_paths(&self) -> anyhow::Result<UserScopedPaths> {
        self.resolve_paths()
            .ok_or_else(|| anyhow::anyhow!("未登录"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn paths_snapshot_consistent() {
        let root = PathBuf::from("/tmp/test-renlijia");
        let paths = UserScopedPaths::new(&root, "t_1__u_2");

        assert_eq!(paths.base_dir(), root.join("users/t_1__u_2"));
        assert_eq!(
            paths.conversations_dir(),
            root.join("users/t_1__u_2/conversations")
        );
        assert_eq!(
            paths.mcp_config_path(),
            root.join("users/t_1__u_2/mcp_servers.json")
        );
        assert_eq!(paths.schedules_dir(), root.join("users/t_1__u_2/schedules"));
        assert_eq!(
            paths.permissions_path(),
            root.join("users/t_1__u_2/permissions.json")
        );
        assert_eq!(paths.skills_dir(), root.join("users/t_1__u_2/skills"));
        assert_eq!(paths.agents_dir(), root.join("users/t_1__u_2/agents"));
        assert_eq!(
            paths.agent_invocations_path(),
            root.join("users/t_1__u_2/agent_invocations.json")
        );
        assert_eq!(
            paths.subagent_transcripts_dir(),
            root.join("users/t_1__u_2/subagent_transcripts")
        );
    }

    #[test]
    fn paths_snapshot_all_directories() {
        let root = PathBuf::from("/data/renlijia");
        let paths = UserScopedPaths::new(&root, "t_5__u_6");
        let base = root.join("users/t_5__u_6");

        assert_eq!(paths.config_path(), base.join("config.json"));
        assert_eq!(paths.scope_json_path(), base.join("scope.json"));
        assert_eq!(paths.index_path(), base.join("index.json"));
        assert_eq!(paths.shared_dir(), base.join("shared"));
        assert_eq!(paths.memory_dir(), base.join("shared/memory"));
        assert_eq!(paths.cognitive_dir(), base.join("shared/cognitive"));
        assert_eq!(paths.cache_dir(), base.join("shared/cache"));
        assert_eq!(paths.project_memories_dir(), base.join("project_memories"));
        assert_eq!(paths.api_data_dir(), base.join("api-data"));
        assert_eq!(paths.screenshots_dir(), base.join("screenshots"));
        assert_eq!(paths.site_profiles_dir(), base.join("site-profiles"));
        assert_eq!(paths.audit_dir(), base.join("audit"));
        assert_eq!(paths.logs_dir(), base.join("logs"));
        assert_eq!(paths.downloads_dir(), base.join("downloads"));
    }

    #[test]
    fn agenda_dir_under_base() {
        use tempfile::TempDir;
        let dir = TempDir::new().unwrap();
        let paths = UserScopedPaths::new(dir.path(), "t_1__u_2");
        assert_eq!(paths.agenda_dir(), dir.path().join("users/t_1__u_2/agenda"));
    }

}
