//! Production `ConvDirResolver` backed by `AiJiaHome` + `ConversationStore`.
//!
//! This resolver uses the AiJiaHome path layout and reads `conv.json` directly
//! to determine archive status, since `ConversationStore` trait has no `is_archived`
//! query method.

use std::path::PathBuf;

use crate::runtime::ids::SessionId;
use crate::storage::file_store::types::ConversationMeta;
use crate::storage::file_store::io::read_json_optional;
use crate::storage::AiJiaHome;
use crate::storage::UserScope;

use super::queue_manager::ConvDirResolver;

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
