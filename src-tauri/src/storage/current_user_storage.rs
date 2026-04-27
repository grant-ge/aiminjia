use std::sync::{Arc, RwLock};

use crate::storage::file_store::AppStorage;
use crate::storage::{AiJiaHome, UserScope, UserScopedPathResolver, UserScopedPaths};

struct Inner {
    scope: UserScope,
    paths: UserScopedPaths,
    storage: Arc<AppStorage>,
}

pub struct CurrentUserStorage {
    home: Arc<AiJiaHome>,
    inner: RwLock<Option<Inner>>,
}

impl CurrentUserStorage {
    pub fn new(home: Arc<AiJiaHome>) -> Self {
        Self {
            home,
            inner: RwLock::new(None),
        }
    }

    /// Activate a user scope: create UserScopedPaths snapshot + AppStorage at user base dir.
    pub fn activate_scope(&self, scope: UserScope) -> anyhow::Result<()> {
        self.home.ensure_user_dirs(&scope)?;
        let paths = UserScopedPaths::new(self.home.root(), &scope.key());
        let storage = Arc::new(AppStorage::new(&paths.base_dir())?);
        let mut guard = self.inner.write().unwrap();
        *guard = Some(Inner {
            scope,
            paths,
            storage,
        });
        Ok(())
    }

    /// Deactivate on logout.
    pub fn deactivate(&self) {
        let mut guard = self.inner.write().unwrap();
        *guard = None;
    }

    /// Get current AppStorage if logged in.
    pub fn get(&self) -> Option<Arc<AppStorage>> {
        self.inner
            .read()
            .unwrap()
            .as_ref()
            .map(|inner| inner.storage.clone())
    }

    /// Get current AppStorage or error.
    pub fn require(&self) -> anyhow::Result<Arc<AppStorage>> {
        self.get().ok_or_else(|| anyhow::anyhow!("未登录"))
    }

    /// Get current UserScope if logged in.
    pub fn scope(&self) -> Option<UserScope> {
        self.inner
            .read()
            .unwrap()
            .as_ref()
            .map(|inner| inner.scope.clone())
    }

    /// Get AiJiaHome reference for global/root paths only.
    pub fn home(&self) -> &AiJiaHome {
        &self.home
    }
}

impl UserScopedPathResolver for CurrentUserStorage {
    fn resolve_paths(&self) -> Option<UserScopedPaths> {
        self.inner
            .read()
            .unwrap()
            .as_ref()
            .map(|inner| inner.paths.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn not_logged_in_returns_none() {
        let tmp = TempDir::new().unwrap();
        let home = crate::storage::AiJiaHome::from_path(tmp.path().to_path_buf());
        let cus = CurrentUserStorage::new(Arc::new(home));
        assert!(cus.get().is_none());
    }

    #[test]
    fn activate_scope_creates_storage() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(crate::storage::AiJiaHome::from_path(tmp.path().to_path_buf()));
        let cus = CurrentUserStorage::new(home.clone());
        let scope = UserScope::new(1, 2);
        cus.activate_scope(scope.clone()).unwrap();
        assert!(cus.get().is_some());
    }

    #[test]
    fn deactivate_clears_storage() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(crate::storage::AiJiaHome::from_path(tmp.path().to_path_buf()));
        let cus = CurrentUserStorage::new(home);
        let scope = UserScope::new(1, 2);
        cus.activate_scope(scope).unwrap();
        cus.deactivate();
        assert!(cus.get().is_none());
    }

    #[test]
    fn resolve_paths_none_when_not_logged_in() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(crate::storage::AiJiaHome::from_path(tmp.path().to_path_buf()));
        let cus = CurrentUserStorage::new(home);
        assert!(cus.resolve_paths().is_none());
    }

    #[test]
    fn resolve_paths_returns_correct_base_after_activate() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(crate::storage::AiJiaHome::from_path(tmp.path().to_path_buf()));
        let cus = CurrentUserStorage::new(home.clone());
        let scope = UserScope::new(1, 2);
        cus.activate_scope(scope).unwrap();
        let paths = cus.resolve_paths().expect("should be Some after activate");
        assert_eq!(paths.base_dir(), tmp.path().join("users").join("t_1__u_2"));
    }

    #[test]
    fn resolve_paths_none_after_deactivate() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(crate::storage::AiJiaHome::from_path(tmp.path().to_path_buf()));
        let cus = CurrentUserStorage::new(home);
        cus.activate_scope(UserScope::new(1, 2)).unwrap();
        cus.deactivate();
        assert!(cus.resolve_paths().is_none());
    }

    #[test]
    fn require_errors_when_not_logged_in() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(crate::storage::AiJiaHome::from_path(tmp.path().to_path_buf()));
        let cus = CurrentUserStorage::new(home);
        assert!(cus.require().is_err());
    }

    #[test]
    fn scope_returns_correct_value() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(crate::storage::AiJiaHome::from_path(tmp.path().to_path_buf()));
        let cus = CurrentUserStorage::new(home);
        assert!(cus.scope().is_none());
        cus.activate_scope(UserScope::new(3, 4)).unwrap();
        let s = cus.scope().unwrap();
        assert_eq!(s.tenant_id, 3);
        assert_eq!(s.user_id, 4);
    }

    #[test]
    fn activate_scope_creates_user_dirs_on_disk() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(crate::storage::AiJiaHome::from_path(tmp.path().to_path_buf()));
        let cus = CurrentUserStorage::new(home);
        cus.activate_scope(UserScope::new(1, 2)).unwrap();
        let user_dir = tmp.path().join("users").join("t_1__u_2");
        assert!(user_dir.join("conversations").is_dir());
        assert!(user_dir.join("shared").join("memory").is_dir());
        assert!(user_dir.join("shared").join("cognitive").is_dir());
        assert!(user_dir.join("shared").join("cache").is_dir());
        assert!(user_dir.join("schedules").is_dir());
        assert!(user_dir.join("skills").is_dir());
    }
}
