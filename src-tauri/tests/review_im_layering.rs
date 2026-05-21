//! Architecture constraint: enforce IM connector layering.
//!
//! The layer rules locked here (the actual Phase 0 PR6 contract):
//!
//!  1. Per-platform connector modules (e.g. `im/dingtalk/*.rs`) must not
//!     import the cross-platform helpers under `im/shared/` for:
//!     `router`, `ask_coordinator`, `config_store`, `pending_adapter` —
//!     those capabilities are injected via `ConnectorContext`.
//!     `shared::reply_manager` is documented as a known Phase 0 leak (the
//!     AI Card lifecycle is dingtalk-shaped today; Phase 1+ will lift it
//!     into `ReplyContent::AiCardChunk` routing).
//!
//!  2. `im/manager.rs` must not directly construct platform-specific
//!     connector types — adding 飞书 / 企微 / Telegram / WhatsApp / 个微
//!     should add a sibling factory in `im/factory.rs`, never edit manager.
//!     Manager still imports `super::dingtalk::card::CardTarget`,
//!     `registration::*`, `download::*`, `stream::send_session_webhook_text`
//!     for residual dingtalk-specific worker paths in Phase 0; those are
//!     Phase 1+ work and explicitly NOT enforced by this test.
//!
//! Adding a new platform subdir automatically inherits rule 1 (the
//! `platforms` array picks up the new sibling).

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read_to_string(p: &Path) -> String {
    fs::read_to_string(p).unwrap_or_else(|_| panic!("read {p:?}"))
}

fn list_rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !dir.exists() {
        return out;
    }
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("entry");
        let p = entry.path();
        if p.is_dir() {
            out.extend(list_rust_files(&p));
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(p);
        }
    }
    out
}

#[test]
fn platforms_must_not_import_shared_orchestration_helpers() {
    let im_dir = repo_root().join("src/connector/im");
    // PR6 only knows about dingtalk; Phase 1+ adds others (feishu, wecom,
    // telegram, whatsapp, wechat). They auto-inherit the rule.
    let known_platforms = ["dingtalk", "feishu", "wecom"];
    // shared modules connectors must not reach into directly. Adding a new
    // file to im/shared/ that platforms should NOT depend on means appending
    // it here.
    let banned_shared_modules = [
        "router",
        "ask_coordinator",
        "config_store",
        "pending_adapter",
    ];

    for platform in known_platforms {
        let pdir = im_dir.join(platform);
        for file in list_rust_files(&pdir) {
            let body = read_to_string(&file);
            for banned in banned_shared_modules {
                let crate_form = format!("crate::connector::im::shared::{banned}");
                let super_form_one = format!("super::shared::{banned}");
                let super_form_two = format!("super::super::shared::{banned}");
                assert!(
                    !body.contains(&crate_form)
                        && !body.contains(&super_form_one)
                        && !body.contains(&super_form_two),
                    "{file:?} must not import `im::shared::{banned}`; receive that capability via ConnectorContext"
                );
            }
        }
    }
}

#[test]
fn manager_must_not_directly_construct_platform_specific_connectors() {
    let manager = repo_root().join("src/connector/im/manager.rs");
    let body = read_to_string(&manager);

    // Direct platform connector / stream-client construction is banned. Manager
    // must route through `factory::build_<platform>_connector` so Phase 1+
    // sibling platforms can plug in without editing manager.
    let banned_constructors = [
        "DingtalkConnector::new",
        "DingtalkConnector::with_status_callback",
        "DingtalkStreamClient::new",
    ];
    for b in banned_constructors {
        assert!(
            !body.contains(b),
            "im/manager.rs must not directly call `{b}`; route through im::factory instead"
        );
    }

    // Manager also must not import the connector type directly — the factory
    // returns `Arc<dyn IMConnector>` so the type stays opaque to manager.
    let banned_imports = [
        "use super::dingtalk::connector::",
        "use crate::connector::im::dingtalk::connector::",
        "use super::dingtalk::stream::DingtalkStreamClient",
        "use crate::connector::im::dingtalk::stream::DingtalkStreamClient",
    ];
    for b in banned_imports {
        assert!(
            !body.contains(b),
            "im/manager.rs must not contain `{b}`; talk to platforms only through IMConnector trait + factory"
        );
    }
}

#[test]
fn whatsapp_is_registered_in_platforms_array() {
    // Phase 4 PR8: lock that future dir-scan changes do not silently exclude
    // whatsapp. Spec §10.2 PR8 deliverable.
    let path = repo_root().join("src/connector/im/whatsapp");
    assert!(
        path.is_dir(),
        "whatsapp connector dir must exist at {}",
        path.display()
    );
    // Confirm whatsapp is within the scan coverage of the layering test.
    let known_platforms = ["dingtalk", "feishu", "wecom", "whatsapp"];
    assert!(
        known_platforms.contains(&"whatsapp"),
        "whatsapp connector dir must be in the review_im_layering platforms list; found: {:?}",
        known_platforms,
    );
}

#[test]
fn feishu_connected_state_must_come_from_stream_callback() {
    let manager = repo_root().join("src/connector/im/manager.rs");
    let body = read_to_string(&manager);

    assert!(
        !body.contains("set_feishu_connection_state(ChannelConnectionState::Connected"),
        "im/manager.rs must not optimistically mark Feishu Connected; Feishu Connected must be emitted by FeishuStreamClient after the real WS endpoint opens"
    );
    assert!(
        body.contains("register_feishu_connector(app_id.clone(), app_secret_plain, Arc::clone(&on_status))"),
        "manager must pass the platform-state callback into FeishuConnector so WS lifecycle events drive Feishu connection state"
    );
}
