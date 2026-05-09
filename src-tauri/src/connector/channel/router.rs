use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::runtime::store::ConversationStore;

use super::types::ConversationType;

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
    fn parse_key(key: &str) -> Option<(ConversationType, String, String)> {
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

impl super::ask_coordinator::ChannelSessionRegistry for ChannelSessionRouter {
    fn is_channel_session(&self, session_id: &crate::runtime::ids::SessionId) -> bool {
        self.session_ids.contains(session_id.as_str())
    }
}

/// Shared registry backed by a plain sync RwLock<HashSet> so the IM worker can
/// insert new session IDs from its async context without holding an async lock.
impl super::ask_coordinator::ChannelSessionRegistry for std::sync::RwLock<std::collections::HashSet<String>> {
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
        conv_store.create_conversation("sess-old-1", "old1").unwrap();
        conv_store.create_conversation("sess-old-2", "old2").unwrap();

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

        assert_ne!(s_a, s_b, "same external_id under different robot_code must produce different sessions");
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
        assert!(matches!(entries[0].conversation_type, ConversationType::Group));

        assert_eq!(entries[1].session_id, "sess-2");
        assert_eq!(entries[1].robot_code, "robot-B");
        assert_eq!(entries[1].external_id, "user2");
        assert!(matches!(entries[1].conversation_type, ConversationType::Private));
    }
}
