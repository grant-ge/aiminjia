//! Production `ConvDirResolver` backed by `AiJiaHome` + `ConversationStore`.
//!
//! This resolver uses the AiJiaHome path layout and reads `conv.json` directly
//! to determine archive status, since `ConversationStore` trait has no `is_archived`
//! query method.

use std::path::PathBuf;

use crate::runtime::ids::SessionId;
use crate::storage::file_store::io::read_json_optional;
use crate::storage::file_store::types::ConversationMeta;
use crate::storage::AiJiaHome;
use crate::storage::CurrentUserStorage;
use crate::storage::UserScope;

use super::queue_manager::ConvDirResolver;
use std::sync::Arc;

pub struct AiJiaPendingResolver {
    home: AiJiaHome,
    scope: UserScope,
}

impl AiJiaPendingResolver {
    pub fn new(home: AiJiaHome, scope: UserScope) -> Self {
        Self { home, scope }
    }
}

impl ConvDirResolver for AiJiaPendingResolver {
    fn conversation_dir(&self, session_id: &SessionId) -> Option<PathBuf> {
        let dir = self
            .home
            .user_conversations_dir(&self.scope)
            .join(session_id.as_str());
        if dir.exists() || std::fs::create_dir_all(&dir).is_ok() {
            Some(dir)
        } else {
            None
        }
    }

    fn is_archived(&self, session_id: &SessionId) -> bool {
        let conv_json = self
            .home
            .user_conversations_dir(&self.scope)
            .join(session_id.as_str())
            .join("conv.json");
        match read_json_optional::<ConversationMeta>(&conv_json) {
            Ok(Some(meta)) => meta.is_archived,
            _ => false,
        }
    }

    fn conversations_root(&self) -> PathBuf {
        self.home.user_conversations_dir(&self.scope)
    }
}

pub struct CurrentUserPendingResolver {
    home: AiJiaHome,
    cus: Arc<CurrentUserStorage>,
}

impl CurrentUserPendingResolver {
    pub fn new(home: AiJiaHome, cus: Arc<CurrentUserStorage>) -> Self {
        Self { home, cus }
    }

    fn active_conversations_root(&self) -> Option<PathBuf> {
        self.cus
            .scope()
            .map(|scope| self.home.user_conversations_dir(&scope))
    }
}

impl ConvDirResolver for CurrentUserPendingResolver {
    fn conversation_dir(&self, session_id: &SessionId) -> Option<PathBuf> {
        let dir = self.active_conversations_root()?.join(session_id.as_str());
        if dir.exists() || std::fs::create_dir_all(&dir).is_ok() {
            Some(dir)
        } else {
            None
        }
    }

    fn is_archived(&self, session_id: &SessionId) -> bool {
        let Some(root) = self.active_conversations_root() else {
            return true;
        };
        let conv_json = root.join(session_id.as_str()).join("conv.json");
        match read_json_optional::<ConversationMeta>(&conv_json) {
            Ok(Some(meta)) => meta.is_archived,
            _ => false,
        }
    }

    fn conversations_root(&self) -> PathBuf {
        self.active_conversations_root()
            .unwrap_or_else(|| self.home.user_conversations_dir(&UserScope::new(0, 0)))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::TempDir;

    use super::*;
    use crate::storage::CurrentUserStorage;

    #[test]
    fn current_user_resolver_follows_active_scope_changes() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(AiJiaHome::from_path(tmp.path().to_path_buf()));
        let cus = Arc::new(CurrentUserStorage::new(home.clone()));
        let resolver = CurrentUserPendingResolver::new(home.as_ref().clone(), cus.clone());
        let session = SessionId::new("conv-1");

        cus.activate_scope(UserScope::new(1, 10)).unwrap();
        let first = resolver.conversation_dir(&session).unwrap();
        assert!(first.starts_with(tmp.path().join("users").join("t_1__u_10")));

        cus.activate_scope(UserScope::new(2, 20)).unwrap();
        let second = resolver.conversation_dir(&session).unwrap();
        assert!(second.starts_with(tmp.path().join("users").join("t_2__u_20")));
    }
}
