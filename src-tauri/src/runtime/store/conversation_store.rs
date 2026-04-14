use anyhow::Result;
use std::collections::HashMap;
use std::sync::Mutex;

/// Domain trait for conversation lifecycle operations.
///
/// Replaces direct `AppStorage` calls in runtime/command code, keeping the
/// runtime layer decoupled from the file-store implementation details.
pub trait ConversationStore: Send + Sync {
    /// Create a new conversation with the given id and initial title.
    fn create_conversation(&self, id: &str, title: &str) -> Result<()>;
    /// List the ids of all known conversations.
    fn list_conversation_ids(&self) -> Result<Vec<String>>;
    /// Return all conversations as JSON values (same shape as AppStorage::get_conversations).
    fn get_conversations(&self) -> Result<Vec<serde_json::Value>>;
    /// Delete a conversation and all its associated data.
    fn delete_conversation(&self, id: &str) -> Result<()>;
    /// Rename an existing conversation.
    fn rename_conversation(&self, id: &str, new_title: &str) -> Result<()>;
    /// Write the run.lock file to signal an active agent task.
    fn insert_active_task(&self, conversation_id: &str) -> Result<()>;
    /// Remove the run.lock file (idempotent — succeeds even if already absent).
    fn remove_active_task(&self, conversation_id: &str) -> Result<()>;
    /// Retrieve all messages for a conversation as JSON values.
    fn get_messages(&self, conversation_id: &str) -> Result<Vec<serde_json::Value>>;
}

// ──────────────────────────────────────────────────────────────────────────────
// In-memory implementation (used in tests)
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct InMemoryConversationStore {
    conversations: Mutex<HashMap<String, String>>,
    messages: Mutex<HashMap<String, Vec<serde_json::Value>>>,
    active_tasks: Mutex<std::collections::HashSet<String>>,
}

impl InMemoryConversationStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ConversationStore for InMemoryConversationStore {
    fn create_conversation(&self, id: &str, title: &str) -> Result<()> {
        self.conversations
            .lock()
            .unwrap()
            .insert(id.to_string(), title.to_string());
        Ok(())
    }

    fn list_conversation_ids(&self) -> Result<Vec<String>> {
        Ok(self.conversations.lock().unwrap().keys().cloned().collect())
    }

    fn get_conversations(&self) -> Result<Vec<serde_json::Value>> {
        Ok(self
            .conversations
            .lock()
            .unwrap()
            .iter()
            .map(|(id, title)| {
                serde_json::json!({
                    "id": id,
                    "title": title,
                })
            })
            .collect())
    }

    fn delete_conversation(&self, id: &str) -> Result<()> {
        self.conversations.lock().unwrap().remove(id);
        self.messages.lock().unwrap().remove(id);
        self.active_tasks.lock().unwrap().remove(id);
        Ok(())
    }

    fn rename_conversation(&self, id: &str, new_title: &str) -> Result<()> {
        let mut convs = self.conversations.lock().unwrap();
        if let Some(entry) = convs.get_mut(id) {
            *entry = new_title.to_string();
            Ok(())
        } else {
            anyhow::bail!("conversation '{}' not found", id)
        }
    }

    fn insert_active_task(&self, conversation_id: &str) -> Result<()> {
        self.active_tasks
            .lock()
            .unwrap()
            .insert(conversation_id.to_string());
        Ok(())
    }

    fn remove_active_task(&self, conversation_id: &str) -> Result<()> {
        self.active_tasks.lock().unwrap().remove(conversation_id);
        Ok(())
    }

    fn get_messages(&self, conversation_id: &str) -> Result<Vec<serde_json::Value>> {
        Ok(self
            .messages
            .lock()
            .unwrap()
            .get(conversation_id)
            .cloned()
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inmemory_rename_conversation() {
        let store = InMemoryConversationStore::new();
        store.create_conversation("c1", "Old Title").unwrap();
        store.rename_conversation("c1", "New Title").unwrap();
        let convs = store.get_conversations().unwrap();
        assert!(!convs.is_empty());
    }

    #[test]
    fn test_inmemory_rename_nonexistent_fails() {
        let store = InMemoryConversationStore::new();
        let result = store.rename_conversation("nonexistent", "Title");
        assert!(result.is_err());
    }
}
