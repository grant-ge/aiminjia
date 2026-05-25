use app_lib::connector::im::ask_coordinator::{format_pending_ask_markdown, PendingAskKind};
use app_lib::runtime::ids::ToolCallId;

#[test]
fn permission_ask_markdown_is_plain_im_text() {
    let markdown = format_pending_ask_markdown(&PendingAskKind::Permission {
        tool_call_id: ToolCallId::new("tool-1"),
        tool_name: "bash".into(),
        message: "命令：`ls /tmp`".into(),
        suggestions: vec!["只读命令".into()],
    });
    assert!(markdown.contains("我需要你的确认"));
    assert!(markdown.contains("bash"));
    assert!(markdown.contains("ls /tmp"));
}
