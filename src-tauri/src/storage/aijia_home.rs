use std::path::{Path, PathBuf};

/// `~/.renlijia/` — AIjia 的单一数据根目录。
#[derive(Debug, Clone)]
pub struct AiJiaHome {
    root: PathBuf,
}

impl AiJiaHome {
    /// 从用户 home dir 构建，默认 `~/.renlijia/`。
    pub fn from_home() -> Self {
        let root = dirs::home_dir()
            .map(|home| home.join(".renlijia"))
            .expect("Cannot determine home directory");
        Self { root }
    }

    /// 用于测试，传入任意路径。
    #[cfg(test)]
    pub fn from_path(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
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

    pub fn playwright_profile_dir(&self) -> PathBuf {
        self.root.join("playwright-profile")
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

    pub fn drafts_dir(&self) -> PathBuf {
        self.skills_dir().join("_drafts")
    }

    /// 确保所有必需子目录存在。
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::create_dir_all(self.skills_dir())?;
        std::fs::create_dir_all(self.subagent_transcripts_dir())?;
        std::fs::create_dir_all(self.playwright_profile_dir())?;
        std::fs::create_dir_all(self.api_data_dir())?;
        std::fs::create_dir_all(self.screenshots_dir())?;
        std::fs::create_dir_all(self.crypto_dir())?;
        std::fs::create_dir_all(self.site_profiles_dir())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_paths_under_root() {
        let tmp = TempDir::new().unwrap();
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());

        assert_eq!(home.skills_dir(), tmp.path().join("skills"));
        assert_eq!(home.mcp_config_path(), tmp.path().join("mcp_servers.json"));
        assert_eq!(home.permissions_path(), tmp.path().join("permissions.json"));
        assert_eq!(home.agent_invocations_path(), tmp.path().join("agent_invocations.json"));
        assert_eq!(home.crypto_dir(), tmp.path().join("crypto"));
    }

    #[test]
    fn test_ensure_dirs_creates_subdirs() {
        let tmp = TempDir::new().unwrap();
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());

        home.ensure_dirs().unwrap();

        assert!(home.root().exists());
        assert!(home.skills_dir().exists());
        assert!(home.subagent_transcripts_dir().exists());
        assert!(home.playwright_profile_dir().exists());
        assert!(home.api_data_dir().exists());
        assert!(home.screenshots_dir().exists());
        assert!(home.crypto_dir().exists());
        assert!(home.site_profiles_dir().exists());
    }
}
