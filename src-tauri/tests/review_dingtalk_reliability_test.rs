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
        "let card_target = match",
    );
    let branch = extract_between(
        worker,
        "if let Some(content) = queued_behind_approval",
        "if chat_attachments.is_empty()",
    );

    assert!(
        worker.contains("deliver_pending_approval_ack"),
        "Dingtalk pending approval queue branch must ACK immediately before attachment download"
    );
    assert!(
        branch.contains("enqueue_or_send") || branch.contains("enqueue_behind_pending_approval"),
        "Dingtalk pending approval queue branch must reuse PendingQueueManager::enqueue_or_send; direct send_chat_request drops messages when the session is busy"
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
