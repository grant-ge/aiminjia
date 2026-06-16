use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::storage::fs_atomic::write_atomic;
use crate::storage::{CurrentUserStorage, UserScopedPathResolver};

#[derive(Clone)]
pub struct SkillEnablementStore {
    current_user: Arc<CurrentUserStorage>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillEnablementState {
    #[serde(default)]
    pub disabled_skill_ids: BTreeSet<String>,
}

impl SkillEnablementState {
    pub fn is_enabled(&self, skill_id: &str) -> bool {
        !self.disabled_skill_ids.contains(skill_id)
    }
}

impl SkillEnablementStore {
    pub fn new(current_user: Arc<CurrentUserStorage>) -> Self {
        Self { current_user }
    }

    pub fn load(&self) -> Result<SkillEnablementState> {
        let path = self.state_path()?;
        if !path.is_file() {
            return Ok(SkillEnablementState::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read skill enablement '{}'", path.display()))?;
        serde_json::from_str(&text)
            .with_context(|| format!("parse skill enablement '{}'", path.display()))
    }

    pub fn load_or_default(&self) -> SkillEnablementState {
        match self.load() {
            Ok(state) => state,
            Err(error) => {
                log::warn!("[skill-enablement] load failed, default enabled: {error}");
                SkillEnablementState::default()
            }
        }
    }

    pub fn set_enabled(&self, skill_id: &str, enabled: bool) -> Result<SkillEnablementState> {
        let mut state = self.load_or_default();
        if enabled {
            state.disabled_skill_ids.remove(skill_id);
        } else {
            state.disabled_skill_ids.insert(skill_id.to_string());
        }
        self.save(&state)?;
        Ok(state)
    }

    pub fn clear_override(&self, skill_id: &str) -> Result<SkillEnablementState> {
        let mut state = self.load_or_default();
        state.disabled_skill_ids.remove(skill_id);
        self.save(&state)?;
        Ok(state)
    }

    fn save(&self, state: &SkillEnablementState) -> Result<()> {
        let path = self.state_path()?;
        let bytes = serde_json::to_vec_pretty(state).context("encode skill enablement")?;
        write_atomic(&path, &bytes)
    }

    fn state_path(&self) -> Result<PathBuf> {
        self.current_user
            .resolve_paths()
            .map(|paths| paths.skills_config_path())
            .ok_or_else(|| anyhow::anyhow!("未登录，无法读写技能配置"))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::storage::{AiJiaHome, CurrentUserStorage, UserScope, UserScopedPathResolver};
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn enablement_defaults_to_enabled_when_file_missing() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(AiJiaHome::from_path(tmp.path().to_path_buf()));
        let current_user = Arc::new(CurrentUserStorage::new(home));
        current_user.activate_scope(UserScope::new(1, 2)).unwrap();

        let store = SkillEnablementStore::new(current_user);
        let state = store.load().unwrap();

        assert!(state.is_enabled("biz-plan"));
        assert!(state.disabled_skill_ids.is_empty());
    }

    #[test]
    fn enablement_persists_disabled_ids_under_user_scope() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(AiJiaHome::from_path(tmp.path().to_path_buf()));
        let current_user = Arc::new(CurrentUserStorage::new(home));
        current_user.activate_scope(UserScope::new(7, 9)).unwrap();

        let store = SkillEnablementStore::new(current_user.clone());
        store.set_enabled("biz-plan", false).unwrap();

        let path = current_user.resolve_paths().unwrap().skills_config_path();
        let raw = std::fs::read_to_string(path).unwrap();
        assert!(raw.contains("disabledSkillIds"));
        assert!(raw.contains("biz-plan"));

        let reloaded = store.load().unwrap();
        assert!(!reloaded.is_enabled("biz-plan"));
        assert!(reloaded.is_enabled("deep-research"));
    }

    #[test]
    fn enablement_requires_user_scope_for_writes() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(AiJiaHome::from_path(tmp.path().to_path_buf()));
        let current_user = Arc::new(CurrentUserStorage::new(home));
        let store = SkillEnablementStore::new(current_user);

        let err = store.set_enabled("local-only", false).unwrap_err();

        assert!(err.to_string().contains("未登录") || err.to_string().contains("not logged in"));
    }

    #[test]
    fn remove_override_re_enables_skill() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(AiJiaHome::from_path(tmp.path().to_path_buf()));
        let current_user = Arc::new(CurrentUserStorage::new(home));
        current_user.activate_scope(UserScope::new(3, 4)).unwrap();
        let store = SkillEnablementStore::new(current_user);

        store.set_enabled("docx", false).unwrap();
        store.set_enabled("docx", true).unwrap();

        assert!(store.load().unwrap().is_enabled("docx"));
    }

    #[test]
    fn corrupt_enablement_file_defaults_to_enabled() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(AiJiaHome::from_path(tmp.path().to_path_buf()));
        let current_user = Arc::new(CurrentUserStorage::new(home));
        current_user.activate_scope(UserScope::new(5, 6)).unwrap();
        let path = current_user.resolve_paths().unwrap().skills_config_path();
        std::fs::write(&path, b"{not-json").unwrap();

        let store = SkillEnablementStore::new(current_user);
        let state = store.load_or_default();

        assert!(state.is_enabled("biz-plan"));
        assert!(state.disabled_skill_ids.is_empty());
    }

    #[test]
    fn enablement_is_isolated_between_user_scopes() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(AiJiaHome::from_path(tmp.path().to_path_buf()));
        let current_user = Arc::new(CurrentUserStorage::new(home));
        let store = SkillEnablementStore::new(current_user.clone());

        current_user.activate_scope(UserScope::new(7, 9)).unwrap();
        store.set_enabled("biz-plan", false).unwrap();
        assert!(!store.load().unwrap().is_enabled("biz-plan"));

        current_user.activate_scope(UserScope::new(8, 10)).unwrap();
        assert!(store.load().unwrap().is_enabled("biz-plan"));
    }
}
