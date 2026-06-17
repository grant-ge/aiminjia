fn extract_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_index = source
        .find(start)
        .unwrap_or_else(|| panic!("missing start marker: {start}"));
    let remainder = &source[start_index..];
    let end_index = remainder
        .find(end)
        .unwrap_or_else(|| panic!("missing end marker after {start}: {end}"));
    &remainder[..end_index]
}

#[test]
fn review_dingtalk_reroute_must_reuse_pending_queue() {
    let source = include_str!("../src/connector/im/manager.rs");
    let worker = extract_between(
        source,
        "[channel/dingtalk] worker observed inactive flag",
        "self.platform_state_mutate(Platform::Dingtalk",
    );
    let branch = extract_between(
        worker,
        "let (chat_attachments, download_failures)",
        "Ok(crate::runtime::pending::EnqueueOutcome::Rejected",
    );

    assert!(
        worker.contains("deliver_pending_approval_ack"),
        "Dingtalk pending approval queue branch must ACK immediately before attachment download"
    );
    assert!(
        branch.contains("build_pending_item_from_dingtalk") && branch.contains("enqueue_or_send"),
        "Dingtalk pending approval queue branch must reuse PendingQueueManager::enqueue_or_send; direct send_chat_request drops messages when the session is busy"
    );
    let build_pending_index = branch
        .find("build_pending_item_from_dingtalk")
        .expect("Dingtalk branch must build a PendingItem after attachment conversion");
    let enqueue_index = branch
        .find("enqueue_or_send")
        .expect("Dingtalk branch must route through PendingQueueManager");
    assert!(
        build_pending_index < enqueue_index,
        "Dingtalk branch must construct the PendingItem before calling enqueue_or_send"
    );
    assert!(
        branch.contains("chat_attachments") && branch.contains("download_failures"),
        "Dingtalk pending approval queue branch must enqueue after attachment conversion with real attachment data"
    );
    assert!(
        !branch.contains("send_chat_request(request).await"),
        "Dingtalk pending approval queue branch must not directly call send_chat_request because it bypasses the busy-session pending queue"
    );
}

#[test]
fn review_dingtalk_stream_delta_must_not_await_card_put_inline() {
    let source = include_str!("../src/connector/im/shared/reply_manager.rs");
    let branch = extract_between(
        source,
        "RuntimeEventKind::StreamDelta { content } =>",
        "RuntimeEventKind::StreamDone =>",
    );

    assert!(
        !branch.contains("dingtalk_card::stream_card("),
        "StreamDelta handling runs inside RuntimeEventBus::emit; it must enqueue card updates instead of awaiting dingtalk_card::stream_card inline"
    );
    assert!(
        branch.contains("enqueue_card_update") || branch.contains("schedule_card_update"),
        "StreamDelta handling should hand off card updates to a background path so app streaming is not back-pressured by Dingtalk PUT"
    );
}
