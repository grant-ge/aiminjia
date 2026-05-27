use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::runtime::store::ConversationStore;

use super::super::types::{ConversationType, Platform};

const CURRENT_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Default, Serialize, Deserialize)]
struct SessionsState {
    #[serde(default, rename = "schemaVersion")]
    schema_version: u32,
    /// key: "group:{robot_code}:{external_id}" 或 "private:{robot_code}:{external_id}"
    #[serde(default)]
    sessions: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct RouterEntry {
    pub conversation_type: ConversationType,
    pub robot_code: String,
    pub external_id: String,
    pub session_id: String,
}

pub struct ChannelSessionRouter {
    sessions_path: PathBuf,
    state: SessionsState,
    session_ids: HashSet<String>,
}

impl ChannelSessionRouter {
    fn build_session_ids(state: &SessionsState) -> HashSet<String> {
        state.sessions.values().cloned().collect()
    }

    /// 启动入口：检测到 v1（缺失 schema_version 或 != 2）时清空所有指向的 conversation 目录
    /// 后写入空 v2 文件，返回空 router。已是 v2 则正常加载。
    pub fn migrate_or_load(
        sessions_path: &Path,
        conversation_store: &dyn ConversationStore,
    ) -> Result<Self> {
        let state = if sessions_path.exists() {
            let content = std::fs::read_to_string(sessions_path)?;
            serde_json::from_str::<SessionsState>(&content).unwrap_or_default()
        } else {
            SessionsState::default()
        };

        if state.schema_version == CURRENT_SCHEMA_VERSION {
            let session_ids = Self::build_session_ids(&state);
            return Ok(Self {
                sessions_path: sessions_path.to_path_buf(),
                state,
                session_ids,
            });
        }

        let legacy_count = state.sessions.len();
        if legacy_count > 0 {
            log::info!(
                "[channel] migrating sessions.json v{} → v{}, dropping {} legacy conversations",
                state.schema_version,
                CURRENT_SCHEMA_VERSION,
                legacy_count
            );
        }
        for session_id in state.sessions.values() {
            if let Err(e) = conversation_store.delete_conversation(session_id) {
                log::warn!(
                    "[channel] failed to delete legacy conversation {}: {:#}",
                    session_id,
                    e
                );
            }
        }

        let state = SessionsState {
            schema_version: CURRENT_SCHEMA_VERSION,
            sessions: HashMap::new(),
        };
        let router = Self {
            sessions_path: sessions_path.to_path_buf(),
            session_ids: Self::build_session_ids(&state),
            state,
        };
        router.persist()?;
        Ok(router)
    }

    /// Public test-only entry: 不做迁移，纯加载。生产代码请用 `migrate_or_load`。
    pub fn load_for_test(sessions_path: &Path) -> Result<Self> {
        let state = if sessions_path.exists() {
            let content = std::fs::read_to_string(sessions_path)?;
            serde_json::from_str::<SessionsState>(&content).unwrap_or_default()
        } else {
            SessionsState {
                schema_version: CURRENT_SCHEMA_VERSION,
                sessions: std::collections::HashMap::new(),
            }
        };
        Ok(Self {
            sessions_path: sessions_path.to_path_buf(),
            session_ids: Self::build_session_ids(&state),
            state,
        })
    }

    /// 查询或新建 session_id。新建时持久化到磁盘。
    /// Returns the existing session for the given key, or creates a new one
    /// via `create_session`. When `ensure_fn` is provided, it is called with
    /// the existing session_id so the caller can ensure the backing conv.json
    /// exists in the current user scope (needed after account switch).
    pub fn get_or_create_session(
        &mut self,
        conversation_type: &ConversationType,
        robot_code: &str,
        external_id: &str,
        create_session: impl FnOnce() -> Result<String>,
    ) -> Result<String> {
        let key = Self::make_key(conversation_type, robot_code, external_id);
        if let Some(session_id) = self.state.sessions.get(&key) {
            return Ok(session_id.clone());
        }
        let session_id = create_session()?;
        self.state.sessions.insert(key, session_id.clone());
        self.session_ids.insert(session_id.clone());
        self.persist()?;
        Ok(session_id)
    }

    /// Like `get_or_create_session` but also calls `ensure_fn` for existing
    /// sessions. Use this to guarantee conv.json exists after account switch.
    pub fn get_or_create_session_with_ensure(
        &mut self,
        conversation_type: &ConversationType,
        robot_code: &str,
        external_id: &str,
        create_session: impl FnOnce() -> Result<String>,
        ensure_fn: impl FnOnce(&str) -> Result<()>,
    ) -> Result<String> {
        let key = Self::make_key(conversation_type, robot_code, external_id);
        if let Some(session_id) = self.state.sessions.get(&key) {
            let _ = ensure_fn(session_id);
            return Ok(session_id.clone());
        }
        let session_id = create_session()?;
        self.state.sessions.insert(key, session_id.clone());
        self.session_ids.insert(session_id.clone());
        self.persist()?;
        Ok(session_id)
    }

    /// 返回所有现存条目，供 hydrate 用。
    pub fn entries(&self) -> Vec<RouterEntry> {
        self.state
            .sessions
            .iter()
            .filter_map(|(key, session_id)| {
                let parsed = Self::parse_key(key)?;
                Some(RouterEntry {
                    conversation_type: parsed.0,
                    robot_code: parsed.1,
                    external_id: parsed.2,
                    session_id: session_id.clone(),
                })
            })
            .collect()
    }

    fn make_key(
        conversation_type: &ConversationType,
        robot_code: &str,
        external_id: &str,
    ) -> String {
        let prefix = match conversation_type {
            ConversationType::Group => "group",
            ConversationType::Private => "private",
        };
        format!("{}:{}:{}", prefix, robot_code, external_id)
    }

    /// 反向解析：把 `group:{robot_code}:{external_id}` 拆出来。
    /// 只识别 v2 格式（包含 robot_code 段）；v1 格式（缺 robot_code）返回 None，由 entries 过滤。
    pub(crate) fn parse_key(key: &str) -> Option<(ConversationType, String, String)> {
        let (prefix, rest) = key.split_once(':')?;
        let conv_type = match prefix {
            "group" => ConversationType::Group,
            "private" => ConversationType::Private,
            _ => return None,
        };
        let (robot_code, external_id) = rest.split_once(':')?;
        if robot_code.is_empty() || external_id.is_empty() {
            return None;
        }
        Some((conv_type, robot_code.to_string(), external_id.to_string()))
    }

    fn persist(&self) -> Result<()> {
        if let Some(parent) = self.sessions_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&self.state)?;
        std::fs::write(&self.sessions_path, content)?;
        Ok(())
    }
}

/// 推断 router entry 的归属平台。早期版本所有平台共用 dingtalk 的 sessions.json，
/// 落进同一个文件里。这个 helper 根据 robot_code 的命名规则把它们重新分配回
/// 各自的平台 sessions.json（见 `split_legacy_shared_sessions`）。
///
/// 命名规则：
/// - 飞书 device-code 注册得到的 app_id 都是 `cli_` 开头（open.feishu.cn 强制）
/// - 企微 aibot 的 bot_id 是 UUID 形式（含 `-`），无固定前缀
/// - 钉钉的 robot_code 是 `ding`/`dingaf` 前缀或纯数字
///
/// 任何不认识的格式默认归 dingtalk（与旧代码 hydrate 默认行为一致），
/// 避免把已知的钉钉会话错分类。
pub fn classify_robot_code(robot_code: &str) -> Platform {
    if robot_code.starts_with("cli_") {
        Platform::Feishu
    } else if is_uuid_like(robot_code) {
        Platform::Wecom
    } else {
        Platform::Dingtalk
    }
}

fn is_uuid_like(s: &str) -> bool {
    // 企微 aibot bot_id 是 36 位 UUID（8-4-4-4-12），含 4 个 `-`。
    let dash_count = s.chars().filter(|&c| c == '-').count();
    s.len() == 36 && dash_count == 4
}

/// 一次性迁移：早期版本所有平台共用 `dingtalk/sessions.json`。本函数读这一个
/// 文件，按 robot_code 把每个 entry 重写到 `platform_paths[platform]`，最终
/// 让每个平台只看到属于自己的 session。
///
/// 行为：
/// - 文件不存在 → 视作"已是新结构"，直接返回 Ok（无 op）。
/// - 文件存在但不是 v2 schema → 跳过迁移，让后续 `migrate_or_load` 处理（它会清掉 v1）。
/// - 文件存在且是 v2 → 按 robot_code 分桶，把"不属于 dingtalk"的桶各写一份
///   `platform_paths[platform]` 文件，**只保留** dingtalk entry 在原文件里。
/// - 已经迁移过（其他平台的 sessions.json 已存在）→ skip，避免覆盖运行时新写入。
pub fn split_legacy_shared_sessions(
    dingtalk_sessions_path: &Path,
    platform_paths: &HashMap<Platform, PathBuf>,
) -> Result<()> {
    if !dingtalk_sessions_path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(dingtalk_sessions_path)?;
    let state: SessionsState = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Ok(()), // unreadable / corrupted → 留给 migrate_or_load 处理
    };
    if state.schema_version != CURRENT_SCHEMA_VERSION {
        return Ok(()); // v1 走原本的 wipe-and-load 路径
    }

    let mut buckets: HashMap<Platform, HashMap<String, String>> = HashMap::new();
    for (key, session_id) in &state.sessions {
        let Some((_, robot_code, _)) = ChannelSessionRouter::parse_key(key) else {
            continue;
        };
        let platform = classify_robot_code(&robot_code);
        buckets
            .entry(platform)
            .or_default()
            .insert(key.clone(), session_id.clone());
    }

    // 把非 dingtalk 桶写到各自的 sessions.json 里。已存在就 skip，保护运行时新写入。
    let mut migrated_platforms: Vec<Platform> = Vec::new();
    for (platform, sessions) in &buckets {
        if *platform == Platform::Dingtalk {
            continue;
        }
        let Some(target_path) = platform_paths.get(platform) else {
            continue;
        };
        if target_path.exists() {
            // 已经有该平台的 sessions.json 文件：可能是之前已迁移过，或者用户在
            // 这个 host 上重新配过该平台。无论哪种情况都不要覆盖；只是从 dingtalk
            // 这边把对应 entry 摘掉就够了。
            migrated_platforms.push(*platform);
            continue;
        }
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let new_state = SessionsState {
            schema_version: CURRENT_SCHEMA_VERSION,
            sessions: sessions.clone(),
        };
        let content = serde_json::to_string_pretty(&new_state)?;
        std::fs::write(target_path, content)?;
        log::info!(
            "[channel] migrated {} entries from dingtalk/sessions.json → {}/sessions.json",
            sessions.len(),
            platform.as_str()
        );
        migrated_platforms.push(*platform);
    }

    // 从 dingtalk 文件里删掉已搬走的 entry。如果没有任何东西被搬走，不动盘。
    if !migrated_platforms.is_empty() {
        let kept: HashMap<String, String> = state
            .sessions
            .iter()
            .filter(|(key, _)| {
                let Some((_, robot_code, _)) = ChannelSessionRouter::parse_key(key) else {
                    return true; // 保留无法解析的，让 migrate_or_load 处理
                };
                classify_robot_code(&robot_code) == Platform::Dingtalk
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if kept.len() != state.sessions.len() {
            let new_state = SessionsState {
                schema_version: CURRENT_SCHEMA_VERSION,
                sessions: kept,
            };
            let content = serde_json::to_string_pretty(&new_state)?;
            std::fs::write(dingtalk_sessions_path, content)?;
            log::info!("[channel] pruned non-dingtalk entries from dingtalk/sessions.json");
        }
    }

    Ok(())
}

impl super::ask_coordinator::ChannelSessionRegistry for ChannelSessionRouter {
    fn is_channel_session(&self, session_id: &crate::runtime::ids::SessionId) -> bool {
        self.session_ids.contains(session_id.as_str())
    }
}

/// Shared registry backed by a plain sync RwLock<HashSet> so the IM worker can
/// insert new session IDs from its async context without holding an async lock.
impl super::ask_coordinator::ChannelSessionRegistry
    for std::sync::RwLock<std::collections::HashSet<String>>
{
    fn is_channel_session(&self, session_id: &crate::runtime::ids::SessionId) -> bool {
        self.read()
            .map(|ids| ids.contains(session_id.as_str()))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn creates_new_session_for_group() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk/sessions.json");
        let mut router = ChannelSessionRouter::load_for_test(&path).unwrap();

        let session_id = router
            .get_or_create_session(&ConversationType::Group, "robot-1", "cid123", || {
                Ok("sess-abc".to_string())
            })
            .unwrap();

        assert_eq!(session_id, "sess-abc");
    }

    #[test]
    fn returns_existing_session_for_same_key() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk/sessions.json");
        let mut router = ChannelSessionRouter::load_for_test(&path).unwrap();

        router
            .get_or_create_session(&ConversationType::Group, "robot-1", "cid123", || {
                Ok("sess-abc".to_string())
            })
            .unwrap();

        let mut called = false;
        let session_id = router
            .get_or_create_session(&ConversationType::Group, "robot-1", "cid123", || {
                called = true;
                Ok("sess-xyz".to_string())
            })
            .unwrap();

        assert_eq!(session_id, "sess-abc");
        assert!(!called, "closure should not be called for existing session");
    }

    #[test]
    fn group_and_private_use_different_keys() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk/sessions.json");
        let mut router = ChannelSessionRouter::load_for_test(&path).unwrap();

        let group_sess = router
            .get_or_create_session(&ConversationType::Group, "robot-1", "id123", || {
                Ok("sess-group".to_string())
            })
            .unwrap();

        let private_sess = router
            .get_or_create_session(&ConversationType::Private, "robot-1", "id123", || {
                Ok("sess-private".to_string())
            })
            .unwrap();

        assert_ne!(group_sess, private_sess);
    }

    #[test]
    fn persists_and_reloads() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk/sessions.json");

        {
            let mut router = ChannelSessionRouter::load_for_test(&path).unwrap();
            router
                .get_or_create_session(&ConversationType::Private, "robot-1", "user42", || {
                    Ok("sess-persisted".to_string())
                })
                .unwrap();
        }

        let mut router2 = ChannelSessionRouter::load_for_test(&path).unwrap();
        let mut called = false;
        let session_id = router2
            .get_or_create_session(&ConversationType::Private, "robot-1", "user42", || {
                called = true;
                Ok("sess-new".to_string())
            })
            .unwrap();

        assert_eq!(session_id, "sess-persisted");
        assert!(!called, "should have loaded from disk, not created new");
    }

    #[test]
    fn migrate_or_load_drops_legacy_v1_data() {
        use crate::runtime::store::{ConversationStore, InMemoryConversationStore};
        use std::sync::Arc;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk/sessions.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        // 写一份 v1（无 schemaVersion 字段）
        let v1 = serde_json::json!({
            "sessions": {
                "group:cid-x": "sess-old-1",
                "private:user-y": "sess-old-2",
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&v1).unwrap()).unwrap();

        let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());
        conv_store
            .create_conversation("sess-old-1", "old1")
            .unwrap();
        conv_store
            .create_conversation("sess-old-2", "old2")
            .unwrap();

        let router = ChannelSessionRouter::migrate_or_load(&path, conv_store.as_ref()).unwrap();

        // 旧对话已删
        assert!(conv_store.list_conversation_ids().unwrap().is_empty());
        // sessions.json 重写为 v2 空文件
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["schemaVersion"], 2);
        assert!(parsed["sessions"].as_object().unwrap().is_empty());
        // entries 为空
        assert!(router.entries().is_empty());
    }

    #[test]
    fn migrate_or_load_preserves_v2_data() {
        use crate::runtime::store::{ConversationStore, InMemoryConversationStore};
        use std::sync::Arc;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk/sessions.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        let v2 = serde_json::json!({
            "schemaVersion": 2,
            "sessions": {
                "group:robot-A:cid1": "sess-1",
                "private:robot-A:user2": "sess-2",
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&v2).unwrap()).unwrap();

        let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());
        conv_store.create_conversation("sess-1", "t1").unwrap();
        conv_store.create_conversation("sess-2", "t2").unwrap();

        let router = ChannelSessionRouter::migrate_or_load(&path, conv_store.as_ref()).unwrap();

        assert_eq!(router.entries().len(), 2);
        assert_eq!(conv_store.list_conversation_ids().unwrap().len(), 2);
    }

    #[test]
    fn key_includes_robot_code() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk/sessions.json");
        let mut router = ChannelSessionRouter::load_for_test(&path).unwrap();

        let s_a = router
            .get_or_create_session(&ConversationType::Group, "robot-A", "cid1", || {
                Ok("sess-A".to_string())
            })
            .unwrap();
        let s_b = router
            .get_or_create_session(&ConversationType::Group, "robot-B", "cid1", || {
                Ok("sess-B".to_string())
            })
            .unwrap();

        assert_ne!(
            s_a, s_b,
            "same external_id under different robot_code must produce different sessions"
        );
    }

    #[test]
    fn entries_returns_parsed_tuples() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk/sessions.json");
        let mut router = ChannelSessionRouter::load_for_test(&path).unwrap();

        router
            .get_or_create_session(&ConversationType::Group, "robot-A", "cid1", || {
                Ok("sess-1".to_string())
            })
            .unwrap();
        router
            .get_or_create_session(&ConversationType::Private, "robot-B", "user2", || {
                Ok("sess-2".to_string())
            })
            .unwrap();

        let mut entries = router.entries();
        entries.sort_by(|a, b| a.session_id.cmp(&b.session_id));

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].session_id, "sess-1");
        assert_eq!(entries[0].robot_code, "robot-A");
        assert_eq!(entries[0].external_id, "cid1");
        assert!(matches!(
            entries[0].conversation_type,
            ConversationType::Group
        ));

        assert_eq!(entries[1].session_id, "sess-2");
        assert_eq!(entries[1].robot_code, "robot-B");
        assert_eq!(entries[1].external_id, "user2");
        assert!(matches!(
            entries[1].conversation_type,
            ConversationType::Private
        ));
    }

    #[test]
    fn classify_robot_code_distinguishes_platforms() {
        // 飞书 app_id 永远 cli_ 开头
        assert_eq!(
            classify_robot_code("cli_aa812b8928f8dcc9"),
            Platform::Feishu
        );
        // 企微 aibot bot_id 是 UUID
        assert_eq!(
            classify_robot_code("ab12cd34-ef56-7890-abcd-ef1234567890"),
            Platform::Wecom
        );
        // 钉钉应用 robot_code 是 ding* / dingaf* / 纯数字
        assert_eq!(
            classify_robot_code("dingaf79qt8carlhcwav"),
            Platform::Dingtalk
        );
        assert_eq!(classify_robot_code("ding12345"), Platform::Dingtalk);
        assert_eq!(classify_robot_code("12345"), Platform::Dingtalk);
        // 不认识的：归 dingtalk（保留原 hydrate 默认行为，避免错分类已知钉钉数据）
        assert_eq!(classify_robot_code(""), Platform::Dingtalk);
        assert_eq!(classify_robot_code("unknown-prefix"), Platform::Dingtalk);
    }

    fn make_platform_paths(root: &Path) -> HashMap<Platform, PathBuf> {
        let mut m = HashMap::new();
        m.insert(
            Platform::Dingtalk,
            root.join("channels/dingtalk/sessions.json"),
        );
        m.insert(Platform::Feishu, root.join("channels/feishu/sessions.json"));
        m.insert(Platform::Wecom, root.join("channels/wecom/sessions.json"));
        m
    }

    #[test]
    fn split_legacy_shared_sessions_handles_missing_file() {
        let dir = TempDir::new().unwrap();
        let paths = make_platform_paths(dir.path());
        // 文件不存在 → no op
        split_legacy_shared_sessions(&paths[&Platform::Dingtalk], &paths).unwrap();
        assert!(!paths[&Platform::Feishu].exists());
        assert!(!paths[&Platform::Wecom].exists());
    }

    #[test]
    fn split_legacy_shared_sessions_moves_feishu_entries() {
        let dir = TempDir::new().unwrap();
        let paths = make_platform_paths(dir.path());
        let dingtalk_path = &paths[&Platform::Dingtalk];
        std::fs::create_dir_all(dingtalk_path.parent().unwrap()).unwrap();

        // 模拟"飞书会话错落进了 dingtalk/sessions.json"的现场。
        let initial = serde_json::json!({
            "schemaVersion": 2,
            "sessions": {
                "private:dingaf79qt8carlhcwav:075919431222937233": "sess-ding",
                "private:cli_aa812b8928f8dcc9:oc_dc11f03e0d5ac22fb010f1b7cb9b023a": "sess-feishu",
            }
        });
        std::fs::write(
            dingtalk_path,
            serde_json::to_string_pretty(&initial).unwrap(),
        )
        .unwrap();

        split_legacy_shared_sessions(dingtalk_path, &paths).unwrap();

        // dingtalk 文件只剩钉钉条目
        let dt: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dingtalk_path).unwrap()).unwrap();
        let dt_sessions = dt["sessions"].as_object().unwrap();
        assert_eq!(dt_sessions.len(), 1);
        assert!(dt_sessions.contains_key("private:dingaf79qt8carlhcwav:075919431222937233"));

        // 飞书文件被创建出来，含飞书条目
        let fs_path = &paths[&Platform::Feishu];
        assert!(fs_path.exists());
        let fs: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(fs_path).unwrap()).unwrap();
        let fs_sessions = fs["sessions"].as_object().unwrap();
        assert_eq!(fs_sessions.len(), 1);
        assert!(fs_sessions
            .contains_key("private:cli_aa812b8928f8dcc9:oc_dc11f03e0d5ac22fb010f1b7cb9b023a"));
        assert_eq!(fs["schemaVersion"], 2);

        // 企微没有条目 → 不创建文件
        assert!(!paths[&Platform::Wecom].exists());
    }

    #[test]
    fn split_legacy_shared_sessions_preserves_existing_target_file() {
        // 已经迁移过 / 用户已重新配过飞书：feishu/sessions.json 已存在，不能被覆盖。
        let dir = TempDir::new().unwrap();
        let paths = make_platform_paths(dir.path());
        let dingtalk_path = &paths[&Platform::Dingtalk];
        let feishu_path = &paths[&Platform::Feishu];
        std::fs::create_dir_all(dingtalk_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(feishu_path.parent().unwrap()).unwrap();

        // 已存在的飞书 sessions.json（运行时新写入的，不能动）
        let pre_existing = serde_json::json!({
            "schemaVersion": 2,
            "sessions": {
                "private:cli_newfeishu:oc_new": "sess-new-feishu",
            }
        });
        std::fs::write(
            feishu_path,
            serde_json::to_string_pretty(&pre_existing).unwrap(),
        )
        .unwrap();

        // dingtalk 文件里残留一条旧飞书 entry（迁移前未清理）
        let initial = serde_json::json!({
            "schemaVersion": 2,
            "sessions": {
                "private:dingaf79qt8carlhcwav:075919431222937233": "sess-ding",
                "private:cli_oldfeishu:oc_old": "sess-old-feishu",
            }
        });
        std::fs::write(
            dingtalk_path,
            serde_json::to_string_pretty(&initial).unwrap(),
        )
        .unwrap();

        split_legacy_shared_sessions(dingtalk_path, &paths).unwrap();

        // 飞书文件未被覆盖
        let fs: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(feishu_path).unwrap()).unwrap();
        let fs_sessions = fs["sessions"].as_object().unwrap();
        assert_eq!(fs_sessions.len(), 1);
        assert!(fs_sessions.contains_key("private:cli_newfeishu:oc_new"));

        // dingtalk 仍然把那条旧飞书 entry 摘掉了（避免再读出来错分类）
        let dt: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dingtalk_path).unwrap()).unwrap();
        let dt_sessions = dt["sessions"].as_object().unwrap();
        assert_eq!(dt_sessions.len(), 1);
        assert!(dt_sessions.contains_key("private:dingaf79qt8carlhcwav:075919431222937233"));
    }

    #[test]
    fn split_legacy_shared_sessions_skips_v1_file() {
        // v1 schema: 留给 migrate_or_load 处理（它会清空所有 entry）。
        let dir = TempDir::new().unwrap();
        let paths = make_platform_paths(dir.path());
        let dingtalk_path = &paths[&Platform::Dingtalk];
        std::fs::create_dir_all(dingtalk_path.parent().unwrap()).unwrap();

        let v1 = serde_json::json!({
            "sessions": {
                "group:cid-old": "sess-old",
            }
        });
        std::fs::write(dingtalk_path, serde_json::to_string_pretty(&v1).unwrap()).unwrap();

        split_legacy_shared_sessions(dingtalk_path, &paths).unwrap();

        // v1 文件未动；其他平台文件未创建
        assert!(!paths[&Platform::Feishu].exists());
        assert!(!paths[&Platform::Wecom].exists());
        let raw = std::fs::read_to_string(dingtalk_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        // 没 schemaVersion 字段说明没被改写
        assert!(parsed.get("schemaVersion").is_none());
    }
}
