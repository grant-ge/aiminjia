use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::runtime::ids::SessionId;
use crate::storage::file_store::AppStorage;

/// A user-authorized local directory that tools may access within a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizedWorkspace {
    pub id: String,
    pub session_id: SessionId,
    pub root_path: PathBuf,
    pub display_name: String,
    pub authorized_at: String,
}

/// Lightweight reference injected into PluginContext and CapabilityContext.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizedWorkspaceRef {
    pub id: String,
    pub root_path: PathBuf,
    pub display_name: String,
}

/// Store trait: each session holds at most one authorized directory.
/// Writing again replaces the previous value (upsert / single-value semantics).
pub trait AuthorizedWorkspaceStore: Send + Sync {
    fn replace_for_session(
        &self,
        conversation_id: &str,
        ws: &AuthorizedWorkspace,
    ) -> Result<()>;
    fn get_current_for_session(
        &self,
        conversation_id: &str,
        session_id: &SessionId,
    ) -> Result<Option<AuthorizedWorkspace>>;
    fn clear_for_session(
        &self,
        conversation_id: &str,
        session_id: &SessionId,
    ) -> Result<()>;
}

// ─────────────────────────────────────────────────────────────────────────────
// File-backed production implementation
// ─────────────────────────────────────────────────────────────────────────────

/// Persists the authorized workspace using `AppStorage::set_memory` /
/// `get_memory` with the key `"authorized_workspace:{session_id}"`.
///
/// AppStorage has no dedicated `delete_memory` method, so `clear_for_session`
/// writes an empty string and `get_current_for_session` treats an empty value
/// as "absent".
pub struct FileAuthorizedWorkspaceStore {
    pub storage: Arc<AppStorage>,
}

impl AuthorizedWorkspaceStore for FileAuthorizedWorkspaceStore {
    fn replace_for_session(
        &self,
        _conversation_id: &str,
        ws: &AuthorizedWorkspace,
    ) -> Result<()> {
        let key = format!("authorized_workspace:{}", ws.session_id.as_str());
        let value = serde_json::to_string(ws)?;
        self.storage
            .set_memory(&key, &value, Some("authorized_workspace"))
    }

    fn get_current_for_session(
        &self,
        _conversation_id: &str,
        session_id: &SessionId,
    ) -> Result<Option<AuthorizedWorkspace>> {
        let key = format!("authorized_workspace:{}", session_id.as_str());
        match self.storage.get_memory(&key)? {
            Some(v) if !v.is_empty() => Ok(Some(serde_json::from_str(&v)?)),
            _ => Ok(None),
        }
    }

    fn clear_for_session(
        &self,
        _conversation_id: &str,
        session_id: &SessionId,
    ) -> Result<()> {
        let key = format!("authorized_workspace:{}", session_id.as_str());
        // AppStorage has no delete_memory; write empty string as sentinel.
        self.storage
            .set_memory(&key, "", Some("authorized_workspace_cleared"))?;
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// conv.json-backed implementation (production successor to FileAuthorizedWorkspaceStore)
// ─────────────────────────────────────────────────────────────────────────────

/// 把授权目录持久化到 `conversations/{id}/conv.json` 的 `authorizedWorkspace` 字段，
/// 同时 mirror displayName 到 `index.json`。
///
/// 替代 `FileAuthorizedWorkspaceStore`（走 memory.jsonl 那条线）。
pub struct ConvJsonAuthorizedWorkspaceStore {
    pub storage: Arc<AppStorage>,
}

impl AuthorizedWorkspaceStore for ConvJsonAuthorizedWorkspaceStore {
    fn replace_for_session(
        &self,
        conversation_id: &str,
        ws: &AuthorizedWorkspace,
    ) -> Result<()> {
        let persisted = crate::storage::file_store::types::PersistedAuthorizedWorkspace {
            id: ws.id.clone(),
            root_path: ws.root_path.clone(),
            display_name: ws.display_name.clone(),
            authorized_at: ws.authorized_at.clone(),
        };
        crate::storage::file_store::conversations::set_conversation_workspace(
            self.storage.base_dir(),
            conversation_id,
            Some(&persisted),
        )?;
        Ok(())
    }

    fn get_current_for_session(
        &self,
        conversation_id: &str,
        session_id: &SessionId,
    ) -> Result<Option<AuthorizedWorkspace>> {
        let persisted = crate::storage::file_store::conversations::read_conversation_workspace(
            self.storage.base_dir(),
            conversation_id,
        )?;
        Ok(persisted.map(|p| AuthorizedWorkspace {
            id: p.id,
            session_id: session_id.clone(),
            root_path: p.root_path,
            display_name: p.display_name,
            authorized_at: p.authorized_at,
        }))
    }

    fn clear_for_session(
        &self,
        conversation_id: &str,
        _session_id: &SessionId,
    ) -> Result<()> {
        crate::storage::file_store::conversations::set_conversation_workspace(
            self.storage.base_dir(),
            conversation_id,
            None,
        )?;
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// In-memory implementation (tests)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct InMemoryAuthorizedWorkspaceStore {
    data: Mutex<HashMap<String, AuthorizedWorkspace>>,
}

impl AuthorizedWorkspaceStore for InMemoryAuthorizedWorkspaceStore {
    fn replace_for_session(
        &self,
        conversation_id: &str,
        ws: &AuthorizedWorkspace,
    ) -> Result<()> {
        self.data
            .lock()
            .unwrap()
            .insert(conversation_id.to_string(), ws.clone());
        Ok(())
    }

    fn get_current_for_session(
        &self,
        conversation_id: &str,
        _session_id: &SessionId,
    ) -> Result<Option<AuthorizedWorkspace>> {
        Ok(self.data.lock().unwrap().get(conversation_id).cloned())
    }

    fn clear_for_session(
        &self,
        conversation_id: &str,
        _session_id: &SessionId,
    ) -> Result<()> {
        self.data.lock().unwrap().remove(conversation_id);
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_ws(session_id: &str, path: &str, name: &str) -> AuthorizedWorkspace {
        AuthorizedWorkspace {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: SessionId::new(session_id),
            root_path: PathBuf::from(path),
            display_name: name.to_string(),
            authorized_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn test_replace_and_get_for_session() {
        let store = InMemoryAuthorizedWorkspaceStore::default();
        let sid = SessionId::new("session-a");
        let ws = make_ws("session-a", "/tmp/project", "project");

        store.replace_for_session(sid.as_str(), &ws).unwrap();
        let got = store.get_current_for_session(sid.as_str(), &sid).unwrap();
        assert!(got.is_some());
        let got = got.unwrap();
        assert_eq!(got.display_name, "project");
        assert_eq!(got.root_path, PathBuf::from("/tmp/project"));
    }

    #[test]
    fn test_replace_overwrites_previous() {
        let store = InMemoryAuthorizedWorkspaceStore::default();
        let sid = SessionId::new("session-b");

        let ws1 = make_ws("session-b", "/tmp/old", "old");
        let ws2 = make_ws("session-b", "/tmp/new", "new");

        store.replace_for_session(sid.as_str(), &ws1).unwrap();
        store.replace_for_session(sid.as_str(), &ws2).unwrap();

        let got = store
            .get_current_for_session(sid.as_str(), &sid)
            .unwrap()
            .unwrap();
        assert_eq!(got.display_name, "new");
        assert_eq!(got.root_path, PathBuf::from("/tmp/new"));
    }

    #[test]
    fn test_session_isolation() {
        let store = InMemoryAuthorizedWorkspaceStore::default();
        let sid_a = SessionId::new("session-c");
        let sid_b = SessionId::new("session-d");

        let ws_a = make_ws("session-c", "/tmp/a", "a");
        store.replace_for_session(sid_a.as_str(), &ws_a).unwrap();

        // Session B should find nothing
        let got_b = store
            .get_current_for_session(sid_b.as_str(), &sid_b)
            .unwrap();
        assert!(got_b.is_none());

        // Session A should still find its entry
        let got_a = store
            .get_current_for_session(sid_a.as_str(), &sid_a)
            .unwrap();
        assert!(got_a.is_some());
    }

    #[test]
    fn test_clear_for_session() {
        let store = InMemoryAuthorizedWorkspaceStore::default();
        let sid = SessionId::new("session-e");
        let ws = make_ws("session-e", "/tmp/x", "x");

        store.replace_for_session(sid.as_str(), &ws).unwrap();
        store.clear_for_session(sid.as_str(), &sid).unwrap();

        let got = store.get_current_for_session(sid.as_str(), &sid).unwrap();
        assert!(got.is_none());
    }
}
