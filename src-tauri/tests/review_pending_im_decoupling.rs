//! Architecture review: runtime/pending/ must not depend on Tauri,
//! and channel/manager.rs must only touch the public pending API.

use std::path::Path;

const PENDING_FILES: &[&str] = &[
    "src/runtime/pending/mod.rs",
    "src/runtime/pending/types.rs",
    "src/runtime/pending/store.rs",
    "src/runtime/pending/queue_manager.rs",
    "src/runtime/pending/aijia_resolver.rs",
];

#[test]
fn runtime_pending_does_not_use_tauri() {
    for f in PENDING_FILES {
        let c = std::fs::read_to_string(Path::new(f)).expect(f);
        assert!(!c.contains("use tauri::"), "{} uses tauri", f);
    }
}

#[test]
fn channel_manager_uses_only_public_pending_api() {
    let path = "src/connector/channel/manager.rs";
    let content = std::fs::read_to_string(Path::new(path)).expect(path);
    assert!(
        !content.contains("runtime::pending::queue_manager::"),
        "channel/manager.rs must not reach into queue_manager internals"
    );
    assert!(
        !content.contains("runtime::pending::store::"),
        "channel/manager.rs must not reach into store internals"
    );
}
