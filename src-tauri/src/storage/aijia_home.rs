use std::path::{Path, PathBuf};

use crate::storage::UserScope;

/// `~/.renlijia/` — AIjia 的单一数据根目录。
#[derive(Debug, Clone)]
pub struct AiJiaHome {
    root: PathBuf,
    #[cfg(test)]
    runtime_cache_root: Option<PathBuf>,
}

impl AiJiaHome {
    /// 从用户 home dir 构建，默认 `~/.renlijia/`。
    pub fn from_home() -> Self {
        let root = dirs::home_dir()
            .map(|home| home.join(".renlijia"))
            .expect("Cannot determine home directory");
        Self {
            root,
            #[cfg(test)]
            runtime_cache_root: None,
        }
    }

    /// 用于测试，传入任意路径。仅供测试使用，勿在生产代码中调用。
    pub fn from_path(root: PathBuf) -> Self {
        Self {
            root,
            #[cfg(test)]
            runtime_cache_root: None,
        }
    }

    /// 用于测试，避免 runtimes 目录写入真实用户 cache。
    #[cfg(test)]
    pub fn from_path_with_runtime_cache(root: PathBuf, runtime_cache_root: PathBuf) -> Self {
        Self {
            root,
            runtime_cache_root: Some(runtime_cache_root),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn global_dir(&self) -> PathBuf {
        self.root.join("global")
    }

    pub fn global_config_path(&self) -> PathBuf {
        self.global_dir().join("config.json")
    }

    pub fn global_state_path(&self) -> PathBuf {
        self.global_dir().join("state.json")
    }

    pub fn auth_dir(&self) -> PathBuf {
        self.global_dir().join("auth")
    }

    pub fn cloud_auth_path(&self) -> PathBuf {
        self.auth_dir().join("cloud_auth")
    }

    pub fn active_account_path(&self) -> PathBuf {
        self.auth_dir().join("active_account.json")
    }

    pub fn users_dir(&self) -> PathBuf {
        self.root.join("users")
    }

    /// `~/.renlijia/employee-templates-cache/` — global, content-addressed
    /// cache for digital-employee templates downloaded from lotus OPS. Not
    /// scoped to user — all users on this machine share the same immutable
    /// template versions (cf. `lotus/docs/superpowers/specs/2026-05-10-employee-templates-as-a-service.md` §5).
    pub fn employee_templates_cache_dir(&self) -> PathBuf {
        self.root.join("employee-templates-cache")
    }

    pub fn user_dir(&self, scope: &UserScope) -> PathBuf {
        self.users_dir().join(scope.key())
    }

    pub fn user_config_path(&self, scope: &UserScope) -> PathBuf {
        self.user_dir(scope).join("config.json")
    }

    pub fn user_scope_json_path(&self, scope: &UserScope) -> PathBuf {
        self.user_dir(scope).join("scope.json")
    }

    pub fn user_conversations_dir(&self, scope: &UserScope) -> PathBuf {
        self.user_dir(scope).join("conversations")
    }

    pub fn user_schedules_dir(&self, scope: &UserScope) -> PathBuf {
        self.user_dir(scope).join("schedules")
    }

    pub fn user_permissions_path(&self, scope: &UserScope) -> PathBuf {
        self.user_dir(scope).join("permissions.json")
    }

    pub fn user_mcp_config_path(&self, scope: &UserScope) -> PathBuf {
        self.user_dir(scope).join("mcp_servers.json")
    }

    pub fn user_agent_invocations_path(&self, scope: &UserScope) -> PathBuf {
        self.user_dir(scope).join("agent_invocations.json")
    }

    pub fn user_subagent_transcripts_dir(&self, scope: &UserScope) -> PathBuf {
        self.user_dir(scope).join("subagent_transcripts")
    }

    pub fn user_skills_dir(&self, scope: &UserScope) -> PathBuf {
        self.user_dir(scope).join("skills")
    }

    /// `~/.renlijia/users/{scope}/skill-drafts/` — Skill-Smith (小程) 草稿区。
    pub fn user_skill_drafts_dir(&self, scope: &UserScope) -> PathBuf {
        self.user_dir(scope).join("skill-drafts")
    }

    pub fn user_agents_dir(&self, scope: &UserScope) -> PathBuf {
        self.user_dir(scope).join("agents")
    }

    pub fn user_api_data_dir(&self, scope: &UserScope) -> PathBuf {
        self.user_dir(scope).join("api-data")
    }

    pub fn user_screenshots_dir(&self, scope: &UserScope) -> PathBuf {
        self.user_dir(scope).join("screenshots")
    }

    pub fn user_site_profiles_dir(&self, scope: &UserScope) -> PathBuf {
        self.user_dir(scope).join("site-profiles")
    }

    pub fn user_audit_dir(&self, scope: &UserScope) -> PathBuf {
        self.user_dir(scope).join("audit")
    }

    pub fn user_logs_dir(&self, scope: &UserScope) -> PathBuf {
        self.user_dir(scope).join("logs")
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }

    #[allow(dead_code)]
    pub fn mcp_config_path(&self) -> PathBuf {
        self.root.join("mcp_servers.json")
    }

    #[allow(dead_code)]
    pub fn permissions_path(&self) -> PathBuf {
        self.root.join("permissions.json")
    }

    #[allow(dead_code)]
    pub fn agent_invocations_path(&self) -> PathBuf {
        self.root.join("agent_invocations.json")
    }

    pub fn subagent_transcripts_dir(&self) -> PathBuf {
        self.root.join("subagent_transcripts")
    }

    pub fn api_data_dir(&self) -> PathBuf {
        self.root.join("api-data")
    }

    pub fn screenshots_dir(&self) -> PathBuf {
        self.root.join("screenshots")
    }

    pub fn crypto_dir(&self) -> PathBuf {
        self.root.join("crypto")
    }

    pub fn site_profiles_dir(&self) -> PathBuf {
        self.root.join("site-profiles")
    }

    pub fn runtimes_dir(&self) -> PathBuf {
        #[cfg(test)]
        if let Some(runtime_cache_root) = &self.runtime_cache_root {
            return runtime_cache_root.join("renlijia-runtimes");
        }

        dirs::cache_dir()
            .unwrap_or_else(|| self.root.join("cache"))
            .join("renlijia-runtimes")
    }

    pub fn drafts_dir(&self) -> PathBuf {
        self.skills_dir().join("_drafts")
    }

    /// 未绑定工作目录的对话使用的默认文件夹 `~/.renlijia/defaultFolder/`。
    pub fn default_folder(&self) -> PathBuf {
        self.root.join("defaultFolder")
    }

    /// 临时文件根目录 `~/.renlijia/tmp/`。剪贴板图片、IM 渠道附件下载等
    /// "用户没主动产生、可重新生成"的内容都丢这里，方便统一清理。
    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// `~/.renlijia/turn_stages/` — ephemeral per-turn stage snapshots written
    /// by `TurnStageEmitter` (spec 2026-05-17-turn-stages §5).  The frontend
    /// reads these to hydrate in-flight turn status after a webview reload.
    pub fn turn_stages_dir(&self) -> PathBuf {
        self.root.join("turn_stages")
    }

    /// Per-conversation active-turn-stage file.  Flat layout (no user scope
    /// in the path) lets the emitter write without resolving the user scope
    /// at every transition.
    pub fn turn_stage_path(&self, conversation_id: &str) -> PathBuf {
        self.turn_stages_dir().join(format!("{conversation_id}.json"))
    }

    /// 剪贴板贴图保存目录 `~/.renlijia/tmp/clipboard/`。
    pub fn tmp_clipboard_dir(&self) -> PathBuf {
        self.tmp_dir().join("clipboard")
    }

    /// 钉钉附件下载目录 `~/.renlijia/tmp/dingtalk_downloads/`。
    pub fn tmp_dingtalk_downloads_dir(&self) -> PathBuf {
        self.tmp_dir().join("dingtalk_downloads")
    }

    /// 确保全局层目录存在，供 auth restore 等登录前流程使用。
    pub fn ensure_global_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.global_dir())?;
        std::fs::create_dir_all(self.auth_dir())?;
        std::fs::create_dir_all(self.users_dir())?;
        Ok(())
    }

    /// 确保已知用户 scope 下的业务目录存在。
    pub fn ensure_user_dirs(&self, scope: &UserScope) -> std::io::Result<()> {
        let user_dir = self.user_dir(scope);
        std::fs::create_dir_all(self.user_conversations_dir(scope))?;
        std::fs::create_dir_all(user_dir.join("shared").join("cognitive"))?;
        std::fs::create_dir_all(user_dir.join("shared").join("cache"))?;
        std::fs::create_dir_all(self.user_audit_dir(scope))?;
        std::fs::create_dir_all(self.user_schedules_dir(scope))?;
        let agenda_dir = self.user_dir(scope).join("agenda");
        std::fs::create_dir_all(agenda_dir.join("items"))?;
        std::fs::create_dir_all(agenda_dir.join("occurrences"))?;
        std::fs::create_dir_all(self.user_skills_dir(scope))?;
        std::fs::create_dir_all(self.user_agents_dir(scope))?;
        std::fs::create_dir_all(self.user_subagent_transcripts_dir(scope))?;
        std::fs::create_dir_all(self.user_api_data_dir(scope))?;
        std::fs::create_dir_all(self.user_screenshots_dir(scope))?;
        std::fs::create_dir_all(self.user_site_profiles_dir(scope))?;
        std::fs::create_dir_all(self.user_logs_dir(scope))?;
        std::fs::create_dir_all(user_dir.join("channels"))?;
        Ok(())
    }

    /// 确保所有必需子目录存在。
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::create_dir_all(self.skills_dir())?;
        std::fs::create_dir_all(self.subagent_transcripts_dir())?;
        std::fs::create_dir_all(self.api_data_dir())?;
        std::fs::create_dir_all(self.screenshots_dir())?;
        std::fs::create_dir_all(self.crypto_dir())?;
        std::fs::create_dir_all(self.site_profiles_dir())?;
        std::fs::create_dir_all(self.runtimes_dir())?;
        std::fs::create_dir_all(self.default_folder())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::UserScope;
    use tempfile::TempDir;

    #[test]
    fn test_paths_under_root() {
        let tmp = TempDir::new().unwrap();
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());

        assert_eq!(home.skills_dir(), tmp.path().join("skills"));
        assert_eq!(home.mcp_config_path(), tmp.path().join("mcp_servers.json"));
        assert_eq!(home.permissions_path(), tmp.path().join("permissions.json"));
        assert_eq!(
            home.agent_invocations_path(),
            tmp.path().join("agent_invocations.json")
        );
        assert_eq!(home.crypto_dir(), tmp.path().join("crypto"));
        assert_eq!(
            home.runtimes_dir()
                .file_name()
                .and_then(|name| name.to_str()),
            Some("renlijia-runtimes")
        );
        assert!(!home.runtimes_dir().starts_with(tmp.path()));
    }

    #[test]
    fn test_ensure_dirs_creates_subdirs() {
        let tmp = TempDir::new().unwrap();
        let runtime_cache = TempDir::new().unwrap();
        let home = AiJiaHome::from_path_with_runtime_cache(
            tmp.path().to_path_buf(),
            runtime_cache.path().to_path_buf(),
        );

        home.ensure_dirs().unwrap();

        assert!(home.root().exists());
        assert!(home.skills_dir().exists());
        assert!(home.subagent_transcripts_dir().exists());
        assert!(home.api_data_dir().exists());
        assert!(home.screenshots_dir().exists());
        assert!(home.crypto_dir().exists());
        assert!(home.site_profiles_dir().exists());
        assert_eq!(
            home.runtimes_dir(),
            runtime_cache.path().join("renlijia-runtimes")
        );
        assert!(runtime_cache.path().join("renlijia-runtimes").exists());
        assert!(!tmp.path().join("cache").join("renlijia-runtimes").exists());
    }

    #[test]
    fn test_global_paths_under_temp_root() {
        let tmp = TempDir::new().unwrap();
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());

        assert_eq!(home.global_dir(), tmp.path().join("global"));
        assert_eq!(
            home.global_config_path(),
            tmp.path().join("global").join("config.json")
        );
        assert_eq!(
            home.global_state_path(),
            tmp.path().join("global").join("state.json")
        );
        assert_eq!(home.auth_dir(), tmp.path().join("global").join("auth"));
        assert_eq!(
            home.cloud_auth_path(),
            tmp.path().join("global").join("auth").join("cloud_auth")
        );
        assert_eq!(
            home.active_account_path(),
            tmp.path()
                .join("global")
                .join("auth")
                .join("active_account.json")
        );
        assert_eq!(home.users_dir(), tmp.path().join("users"));
    }

    #[test]
    fn test_user_scoped_paths_under_temp_root() {
        let tmp = TempDir::new().unwrap();
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());
        let scope = UserScope::new(1, 2);
        let user_dir = tmp.path().join("users").join("t_1__u_2");

        assert_eq!(home.user_dir(&scope), user_dir);
        assert_eq!(home.user_config_path(&scope), user_dir.join("config.json"));
        assert_eq!(
            home.user_scope_json_path(&scope),
            user_dir.join("scope.json")
        );
        assert_eq!(
            home.user_conversations_dir(&scope),
            user_dir.join("conversations")
        );
        assert_eq!(home.user_schedules_dir(&scope), user_dir.join("schedules"));
        assert_eq!(
            home.user_permissions_path(&scope),
            user_dir.join("permissions.json")
        );
        assert_eq!(
            home.user_mcp_config_path(&scope),
            user_dir.join("mcp_servers.json")
        );
        assert_eq!(
            home.user_agent_invocations_path(&scope),
            user_dir.join("agent_invocations.json")
        );
        assert_eq!(
            home.user_subagent_transcripts_dir(&scope),
            user_dir.join("subagent_transcripts")
        );
        assert_eq!(home.user_skills_dir(&scope), user_dir.join("skills"));
        assert_eq!(home.user_api_data_dir(&scope), user_dir.join("api-data"));
        assert_eq!(
            home.user_screenshots_dir(&scope),
            user_dir.join("screenshots")
        );
        assert_eq!(
            home.user_site_profiles_dir(&scope),
            user_dir.join("site-profiles")
        );
        assert_eq!(home.user_audit_dir(&scope), user_dir.join("audit"));
        assert_eq!(home.user_logs_dir(&scope), user_dir.join("logs"));
    }

    #[test]
    fn test_ensure_global_dirs_creates_layer_dirs() {
        let tmp = TempDir::new().unwrap();
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());

        home.ensure_global_dirs().unwrap();

        assert!(home.global_dir().exists());
        assert!(home.auth_dir().exists());
        assert!(home.users_dir().exists());
    }

    #[test]
    fn test_ensure_user_dirs_creates_user_subdirs() {
        let tmp = TempDir::new().unwrap();
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());
        let scope = UserScope::new(1, 2);
        let user_dir = home.user_dir(&scope);

        home.ensure_user_dirs(&scope).unwrap();

        assert!(home.user_conversations_dir(&scope).exists());
        assert!(user_dir.join("shared").join("cache").exists());
        assert!(home.user_audit_dir(&scope).exists());
        assert!(home.user_schedules_dir(&scope).exists());
        assert!(user_dir.join("agenda").join("items").exists());
        assert!(user_dir.join("agenda").join("occurrences").exists());
        assert!(home.user_skills_dir(&scope).exists());
        assert!(home.user_agents_dir(&scope).exists());
        assert!(home.user_subagent_transcripts_dir(&scope).exists());
        assert!(home.user_api_data_dir(&scope).exists());
        assert!(home.user_screenshots_dir(&scope).exists());
        assert!(home.user_site_profiles_dir(&scope).exists());
        assert!(home.user_logs_dir(&scope).exists());
    }
}
