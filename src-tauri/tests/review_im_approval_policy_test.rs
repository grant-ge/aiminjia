use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn ordinary_im_text_is_not_routed_to_approval_specific_queue() {
    let manager = fs::read_to_string(repo_root().join("src/connector/im/manager.rs"))
        .expect("manager.rs should be readable");
    let ask_coordinator =
        fs::read_to_string(repo_root().join("src/connector/im/shared/ask_coordinator.rs"))
            .expect("ask_coordinator.rs should be readable");

    assert!(
        !ask_coordinator.contains("QueuedBehindApproval"),
        "ordinary IM text must fall through instead of becoming an approval-specific outcome"
    );
    assert!(
        !manager.contains("QueuedBehindApproval")
            && !manager.contains("queued_behind_approval")
            && !manager.contains("enqueue_behind_pending_approval")
            && !manager.contains("message queued behind approval"),
        "IM manager must not keep approval-specific queue branches for ordinary text"
    );
    assert!(
        manager.contains("enqueue_or_send"),
        "ordinary IM messages still need to flow through PendingQueueManager"
    );
}

#[test]
fn every_im_worker_has_pending_approval_pre_dispatch_gate() {
    let manager = fs::read_to_string(repo_root().join("src/connector/im/manager.rs"))
        .expect("manager.rs should be readable");

    for marker in [
        "[channel/dingtalk]",
        "[channel/feishu]",
        "[channel/wecom]",
        "[channel/wechat]",
        "[channel/telegram]",
        "[channel/whatsapp]",
    ] {
        let matched_worker = worker_tail_for(&manager, marker)
            .map(|tail| {
                tail.contains("handle_pending_action_pre_dispatch")
                    && tail.contains("HandleOutcome::InvalidApprovalAction")
                    && tail.contains("enqueue_or_send")
            })
            .unwrap_or(false);
        assert!(
            matched_worker,
            "{marker} must handle explicit pending actions before normal dispatch and queue ordinary text normally"
        );
    }
}

#[test]
fn normal_pending_queue_preserves_channel_attachments_and_sources() {
    let manager = fs::read_to_string(repo_root().join("src/connector/im/manager.rs"))
        .expect("manager.rs should be readable");

    for (marker, builder) in [
        ("[channel/dingtalk]", "build_pending_item_from_dingtalk"),
        ("[channel/feishu]", "build_pending_item_from_feishu"),
        ("[channel/wecom]", "build_pending_item_from_wecom"),
        ("[channel/wechat]", "build_pending_item_from_wechat"),
        ("[channel/telegram]", "build_pending_item_from_telegram"),
        ("[channel/whatsapp]", "build_pending_item_from_whatsapp"),
    ] {
        let matched_worker = worker_tail_with(&manager, marker, builder)
            .map(|tail| {
                tail.contains("handle_pending_action_pre_dispatch")
                    && tail.contains("enqueue_or_send")
                    && tail.contains("chat_attachments")
                    && tail.contains("download_failures")
            })
            .unwrap_or(false);
        assert!(
            matched_worker,
            "{marker} normal pending queue path must enqueue with {builder} after real attachment conversion"
        );

        let worker_tail = manager
            .match_indices(marker)
            .map(|(marker_index, _)| bounded_tail(&manager, marker_index, 30_000))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !worker_tail.contains("vec![],\n                                &[],")
                && !worker_tail.contains("Vec::new(),\n                                &[],"),
            "{marker} normal pending queue path must not build pending item with empty attachment placeholders"
        );
    }

    let wechat_tail = worker_tail_with(
        &manager,
        "[channel/wechat]",
        "build_pending_item_from_wechat",
    )
    .expect("WeChat worker tail should exist");
    assert!(
        wechat_tail.contains("build_pending_item_from_wechat"),
        "WeChat must use its own pending source builder"
    );
    assert!(
        !wechat_tail.contains("build_pending_item_from_wecom"),
        "WeChat must not reuse WeCom pending source builder"
    );

    let whatsapp_tail = worker_tail_with(
        &manager,
        "[channel/whatsapp]",
        "build_pending_item_from_whatsapp",
    )
    .expect("WhatsApp worker tail should exist");
    assert!(
        whatsapp_tail.contains("build_pending_item_from_whatsapp"),
        "WhatsApp must use its own pending source builder"
    );
    assert!(
        !whatsapp_tail.contains("build_pending_item_from_telegram"),
        "WhatsApp must not reuse Telegram pending source builder"
    );
}

fn worker_tail_for<'a>(manager: &'a str, marker: &str) -> Option<&'a str> {
    manager.match_indices(marker).find_map(|(index, _)| {
        let tail = bounded_tail(manager, index, 80_000);
        tail.contains("handle_pending_action_pre_dispatch")
            .then_some(tail)
    })
}

fn worker_tail_with<'a>(manager: &'a str, marker: &str, needle: &str) -> Option<&'a str> {
    manager.match_indices(marker).find_map(|(index, _)| {
        let tail = bounded_tail(manager, index, 30_000);
        (tail.contains("handle_pending_action_pre_dispatch") && tail.contains(needle))
            .then_some(tail)
    })
}

fn bounded_tail(text: &str, start: usize, max_bytes: usize) -> &str {
    let mut end = text.len().min(start + max_bytes);
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[start..end]
}
