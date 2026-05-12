//! Architectural review test:
//! The Tauri command `send_message` body in `TauriChatCommandAdapter` MUST
//! invoke `PendingQueueManager::enqueue_or_send`. This prevents future edits
//! from silently regressing the queue integration.

use std::path::Path;

#[test]
fn send_message_routes_through_pending_manager() {
    let content = std::fs::read_to_string(Path::new(
        "src/transport/tauri_commands/chat.rs",
    ))
    .expect("read chat.rs");
    assert!(
        content.contains("enqueue_or_send"),
        "TauriChatCommandAdapter::send_message must call PendingQueueManager::enqueue_or_send"
    );
    assert!(
        content.contains("PendingQueueManager"),
        "chat.rs must reference PendingQueueManager"
    );
}
