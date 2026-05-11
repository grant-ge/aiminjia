use std::sync::Arc;

use crate::auth::state::{CloudAuthInfo, CloudModelInfo};
use crate::auth::AuthManager;

#[derive(Clone)]
pub struct TauriAuthCommandAdapter {
    auth: Arc<AuthManager>,
}

impl TauriAuthCommandAdapter {
    pub fn new(auth: Arc<AuthManager>) -> Self {
        Self { auth }
    }

    pub async fn cloud_login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<CloudAuthInfo, String> {
        let username = username.trim();
        if username.is_empty() || password.is_empty() {
            return Err("请输入用户名和密码".to_string());
        }
        self.auth
            .login(username, password)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn cloud_logout(&self) {
        self.auth.logout().await;
    }

    pub async fn get_cloud_auth(&self) -> CloudAuthInfo {
        self.auth.refresh_auth_info().await
    }

    pub async fn get_cloud_models(&self) -> Result<Vec<CloudModelInfo>, String> {
        self.auth
            .get_available_models()
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn cloud_change_password(
        &self,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), String> {
        if old_password.is_empty() || new_password.is_empty() {
            return Err("请输入旧密码和新密码".to_string());
        }
        if new_password.len() < 8 {
            return Err("新密码长度至少 8 个字符".to_string());
        }
        self.auth
            .change_password(old_password, new_password)
            .await
            .map_err(|e| e.to_string())
    }
}
