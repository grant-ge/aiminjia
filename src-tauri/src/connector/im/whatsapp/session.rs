//! WhatsApp 凭证文件路径计算 + 备份/删除。spec v3 §3.1-§3.3。
//!
//! 抄 OpenClaw 的 oauth/whatsapp/{accountId}/ + creds.json + creds.json.bak
//! 模式。单账号下连 `default/` 子目录也省，直接 channels/whatsapp/。
//!
//! ```text
//! channels/whatsapp/
//! ├── session.db          # wa-rs SqliteStore
//! ├── session.db.bak      # 启动前自动备份；wa-rs 启动失败时手动恢复
//! └── config.json         # AIjia 元数据：jid / push_name / paired_at
//! ```

use std::path::{Path, PathBuf};

/// 路径计算 helper。`base` 是 channels/whatsapp/ 目录（来自
/// `ChannelConfigStore::platform_dir(Platform::Whatsapp)`）。
#[derive(Debug, Clone)]
pub struct WhatsAppPaths {
    base: PathBuf,
}

impl WhatsAppPaths {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    /// `channels/whatsapp/`
    pub fn base(&self) -> &Path {
        &self.base
    }

    /// `channels/whatsapp/session.db` —— wa-rs SqliteStore 的文件
    pub fn session_db(&self) -> PathBuf {
        self.base.join("session.db")
    }

    /// `channels/whatsapp/session.db.bak` —— 启动前备份
    pub fn session_db_bak(&self) -> PathBuf {
        self.base.join("session.db.bak")
    }

    /// `channels/whatsapp/config.json` —— AIjia 元数据
    /// （也是 `ChannelConfigStore::platform_config_path` 的对应路径）
    pub fn config_path(&self) -> PathBuf {
        self.base.join("config.json")
    }

    /// 确保 base 目录存在。在 PR3 begin_registration 第一次写文件前调一次即可。
    pub fn ensure_base_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.base)
    }
}

/// 启动前备份：如果 session.db 存在且非空，复制一份到 session.db.bak（覆盖旧 bak）。
///
/// spec v3 §3.3。**不**判断 wa-rs 启动是否能读 session.db；wa-rs 自己会报错，
/// 上层在 PR4 集成测试发现实际损坏概率后决定要不要做自动回滚。
///
/// 返回 `Ok(true)` 表示备份发生了，`Ok(false)` 表示无 session.db 或文件空 → 跳过。
#[allow(dead_code)] // PR3 first consumer
pub fn backup_session_db_if_present(paths: &WhatsAppPaths) -> std::io::Result<bool> {
    let src = paths.session_db();
    if !src.exists() {
        return Ok(false);
    }
    let meta = std::fs::metadata(&src)?;
    if meta.len() == 0 {
        return Ok(false);
    }
    let dst = paths.session_db_bak();
    std::fs::copy(&src, &dst)?;
    Ok(true)
}

/// 重新扫码用：删 session.db + config.json，**保留** session.db.bak。
///
/// spec v3 §3.9。如果删除失败（文件本不存在），不返回错——重新扫码语义
/// 是"清掉登录态"，已不在的文件就当成功。
#[allow(dead_code)] // PR3 first consumer
pub fn delete_for_reauth(paths: &WhatsAppPaths) -> std::io::Result<()> {
    for p in [paths.session_db(), paths.config_path()] {
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tmp_paths() -> (TempDir, WhatsAppPaths) {
        let dir = TempDir::new().unwrap();
        let base = dir.path().join("channels").join("whatsapp");
        let paths = WhatsAppPaths::new(&base);
        paths.ensure_base_dir().unwrap();
        (dir, paths)
    }

    #[test]
    fn path_helpers_compose_under_base() {
        let p = WhatsAppPaths::new("/tmp/foo");
        assert_eq!(p.session_db(), PathBuf::from("/tmp/foo/session.db"));
        assert_eq!(p.session_db_bak(), PathBuf::from("/tmp/foo/session.db.bak"));
        assert_eq!(p.config_path(), PathBuf::from("/tmp/foo/config.json"));
    }

    #[test]
    fn ensure_base_dir_creates_nested_dirs() {
        let dir = TempDir::new().unwrap();
        let base = dir.path().join("a").join("b").join("whatsapp");
        let paths = WhatsAppPaths::new(&base);
        paths.ensure_base_dir().unwrap();
        assert!(base.exists());
    }

    #[test]
    fn backup_skips_when_session_db_missing() {
        let (_dir, paths) = tmp_paths();
        let did = backup_session_db_if_present(&paths).unwrap();
        assert!(!did, "no session.db → no backup");
        assert!(!paths.session_db_bak().exists());
    }

    #[test]
    fn backup_skips_when_session_db_empty() {
        let (_dir, paths) = tmp_paths();
        std::fs::write(paths.session_db(), b"").unwrap();
        let did = backup_session_db_if_present(&paths).unwrap();
        assert!(!did, "empty session.db → no backup");
        assert!(!paths.session_db_bak().exists());
    }

    #[test]
    fn backup_copies_session_db_to_bak() {
        let (_dir, paths) = tmp_paths();
        std::fs::write(paths.session_db(), b"SQLite payload").unwrap();
        let did = backup_session_db_if_present(&paths).unwrap();
        assert!(did);
        assert_eq!(
            std::fs::read(paths.session_db_bak()).unwrap(),
            b"SQLite payload"
        );
    }

    #[test]
    fn backup_overwrites_existing_bak() {
        let (_dir, paths) = tmp_paths();
        std::fs::write(paths.session_db_bak(), b"OLD").unwrap();
        std::fs::write(paths.session_db(), b"NEW").unwrap();
        let did = backup_session_db_if_present(&paths).unwrap();
        assert!(did);
        assert_eq!(std::fs::read(paths.session_db_bak()).unwrap(), b"NEW");
    }

    #[test]
    fn delete_for_reauth_removes_db_and_config_keeps_bak() {
        let (_dir, paths) = tmp_paths();
        std::fs::write(paths.session_db(), b"db").unwrap();
        std::fs::write(paths.session_db_bak(), b"bak").unwrap();
        std::fs::write(paths.config_path(), b"{}").unwrap();

        delete_for_reauth(&paths).unwrap();

        assert!(!paths.session_db().exists());
        assert!(!paths.config_path().exists());
        assert!(
            paths.session_db_bak().exists(),
            "bak must be preserved as recovery anchor"
        );
    }

    #[test]
    fn delete_for_reauth_idempotent_when_files_missing() {
        let (_dir, paths) = tmp_paths();
        // Files don't exist; should not error
        delete_for_reauth(&paths).unwrap();
        delete_for_reauth(&paths).unwrap();
    }
}
