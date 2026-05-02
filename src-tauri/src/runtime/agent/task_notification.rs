//! Task notification 协议：async subagent 完成后向父 chat turn 注入完成信号
//!
//! 协议参考 Claude Code (claude-code-best 项目)
//! - subagent 完成时调 `enqueue_completion(...)` → 入队
//! - 父 chat turn 在 round 之间或下次 turn 开始时 `drain_for_session()` → 拿到 XML
//! - XML 作为 user message attachment 注入下一个 user turn，让父 LLM 看到 "子 agent 完成了"
//!
//! XML 形如：
//! ```xml
//! <task-notification>
//!   <task-id>agent-abc123</task-id>
//!   <tool-use-id>toolu_xyz</tool-use-id>
//!   <output-file>/path/to/agent-abc123.output</output-file>
//!   <status>completed</status>
//!   <summary>...</summary>
//!   <result>...</result>
//!   <usage><total_tokens>1234</total_tokens></usage>
//! </task-notification>
//! ```

use std::sync::{Arc, Mutex};

use crate::runtime::ids::{RunId, SessionId};

/// 构造 `<task-notification>` XML 字符串
///
/// # 必填
/// - `agent_id` — async agent 的 ID
/// - `output_file` — 该 agent 持久化输出文件路径（task_output 工具用此读取）
/// - `status` — "completed" | "failed" | "killed"
/// - `summary` — 简短描述 (e.g. "Agent \"explore\" completed")
///
/// # 可选
/// - `parent_tool_use_id` — 父调 spawn_subagent 时的 tool_use_id
/// - `result` — 子 agent 最终消息
/// - `total_tokens` — 用量统计
pub fn build_task_notification_xml(
    agent_id: &str,
    parent_tool_use_id: Option<&str>,
    output_file: &str,
    status: &str,
    summary: &str,
    result: Option<&str>,
    total_tokens: Option<u64>,
) -> String {
    let mut s = String::from("<task-notification>\n");
    s.push_str(&format!("  <task-id>{}</task-id>\n", xml_escape(agent_id)));
    if let Some(t) = parent_tool_use_id {
        s.push_str(&format!("  <tool-use-id>{}</tool-use-id>\n", xml_escape(t)));
    }
    s.push_str(&format!("  <output-file>{}</output-file>\n", xml_escape(output_file)));
    s.push_str(&format!("  <status>{}</status>\n", xml_escape(status)));
    s.push_str(&format!("  <summary>{}</summary>\n", xml_escape(summary)));
    if let Some(r) = result {
        s.push_str(&format!("  <result>{}</result>\n", xml_escape(r)));
    }
    if let Some(t) = total_tokens {
        s.push_str(&format!(
            "  <usage><total_tokens>{}</total_tokens></usage>\n",
            t
        ));
    }
    s.push_str("</task-notification>");
    s
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 内存级队列：async subagent 完成时入队，父 chat turn 之间按 session drain。
///
/// 队列是进程级共享实例；每条 notification 携带父 session/run，drain 时只
/// 消费匹配当前 session 的条目，避免并发会话之间串话。
#[derive(Clone, Default)]
pub struct TaskNotificationQueue {
    inner: Arc<Mutex<Vec<QueuedNotification>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedNotification {
    pub agent_id: String,
    pub xml: String,
    pub parent_session_id: SessionId,
    pub parent_run_id: Option<RunId>,
}

impl TaskNotificationQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue(
        &self,
        agent_id: impl Into<String>,
        xml: impl Into<String>,
        parent_session_id: SessionId,
        parent_run_id: Option<RunId>,
    ) {
        let mut guard = self.inner.lock().expect("notification queue poisoned");
        guard.push(QueuedNotification {
            agent_id: agent_id.into(),
            xml: xml.into(),
            parent_session_id,
            parent_run_id,
        });
    }

    /// Drain notifications belonging to `session_id`. Other sessions' items remain queued.
    pub fn drain_for_session(&self, session_id: &SessionId) -> Vec<QueuedNotification> {
        let mut guard = self.inner.lock().expect("notification queue poisoned");
        let all = std::mem::take(&mut *guard);
        let (matching, keep): (Vec<_>, Vec<_>) = all
            .into_iter()
            .partition(|n| &n.parent_session_id == session_id);
        *guard = keep;
        matching
    }

    /// Re-enqueue items in original order (used by cancel paths).
    pub fn re_enqueue(&self, items: Vec<QueuedNotification>) {
        if items.is_empty() {
            return;
        }
        let mut guard = self.inner.lock().expect("notification queue poisoned");
        // Items go to the FRONT to preserve enqueue order across drain/re_enqueue cycles.
        let mut combined = items;
        combined.append(&mut *guard);
        *guard = combined;
    }

    /// Peek count without draining (for diagnostics / tests).
    pub fn pending_count(&self) -> usize {
        self.inner.lock().expect("notification queue poisoned").len()
    }

    /// TEST-ONLY: drain everything regardless of session.
    #[cfg(test)]
    pub fn drain_all_for_test(&self) -> Vec<QueuedNotification> {
        let mut guard = self.inner.lock().expect("notification queue poisoned");
        std::mem::take(&mut *guard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_includes_required_fields() {
        let xml = build_task_notification_xml(
            "agent-123",
            None,
            "/tmp/agent-123.output",
            "completed",
            "Test agent done",
            None,
            None,
        );
        assert!(xml.starts_with("<task-notification>"));
        assert!(xml.ends_with("</task-notification>"));
        assert!(xml.contains("<task-id>agent-123</task-id>"));
        assert!(xml.contains("<output-file>/tmp/agent-123.output</output-file>"));
        assert!(xml.contains("<status>completed</status>"));
        assert!(xml.contains("<summary>Test agent done</summary>"));
        // 可选字段未提供时不出现
        assert!(!xml.contains("<tool-use-id>"));
        assert!(!xml.contains("<result>"));
        assert!(!xml.contains("<usage>"));
    }

    #[test]
    fn xml_includes_optional_fields_when_provided() {
        let xml = build_task_notification_xml(
            "agent-123",
            Some("toolu_abc"),
            "/tmp/x.output",
            "completed",
            "summary",
            Some("hello result"),
            Some(1234),
        );
        assert!(xml.contains("<tool-use-id>toolu_abc</tool-use-id>"));
        assert!(xml.contains("<result>hello result</result>"));
        assert!(xml.contains("<total_tokens>1234</total_tokens>"));
    }

    #[test]
    fn xml_escapes_special_characters() {
        let xml = build_task_notification_xml(
            "agent-1",
            None,
            "/tmp/a.output",
            "completed",
            "danger: <script>&amp;</script>",
            Some("3 < 5 && 5 > 2"),
            None,
        );
        // < 应被转义为 &lt;
        assert!(xml.contains("&lt;script&gt;"));
        assert!(xml.contains("3 &lt; 5"));
        // 但 XML 标签本身仍然是 < 开头（不被转义）
        assert!(xml.contains("<task-notification>"));
    }

    fn test_session(id: &str) -> SessionId {
        SessionId::new(id)
    }

    #[test]
    fn queue_enqueue_drain_in_order() {
        let q = TaskNotificationQueue::new();
        let session_id = test_session("session-queue-order");
        q.enqueue("agent-1", "xml-1", session_id.clone(), None);
        q.enqueue("agent-2", "xml-2", session_id.clone(), None);
        assert_eq!(q.pending_count(), 2);

        let drained = q.drain_for_session(&session_id);
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].agent_id, "agent-1");
        assert_eq!(drained[0].xml, "xml-1");
        assert_eq!(drained[1].agent_id, "agent-2");

        // 第二次 drain 应为空
        assert!(q.drain_for_session(&session_id).is_empty());
        assert_eq!(q.pending_count(), 0);
    }

    #[test]
    fn queue_clones_share_state() {
        let q1 = TaskNotificationQueue::new();
        let q2 = q1.clone();
        let session_id = test_session("session-clone");
        q1.enqueue("a", "x", session_id.clone(), None);
        // q2 是 Arc 克隆，应该看到同一份内容
        assert_eq!(q2.pending_count(), 1);
        let drained = q2.drain_for_session(&session_id);
        assert_eq!(drained.len(), 1);
        // q1 也已被 drain
        assert_eq!(q1.pending_count(), 0);
    }

    #[test]
    fn drain_for_session_only_returns_matching() {
        let q = TaskNotificationQueue::new();
        let sa = test_session("sess-A");
        let sb = test_session("sess-B");
        q.enqueue("a1", "x1", sa.clone(), None);
        q.enqueue("b1", "y1", sb, None);

        let drained = q.drain_for_session(&sa);

        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].agent_id, "a1");
        assert_eq!(q.pending_count(), 1, "B's notification should remain queued");
    }

    #[test]
    fn re_enqueue_preserves_order_at_front() {
        let q = TaskNotificationQueue::new();
        let s = test_session("session-re-enqueue");
        q.enqueue("a", "x", s.clone(), None);
        let drained = q.drain_for_session(&s);
        q.enqueue("b", "y", s.clone(), None);

        q.re_enqueue(drained);

        let final_drain = q.drain_for_session(&s);
        assert_eq!(final_drain.len(), 2);
        assert_eq!(final_drain[0].agent_id, "a", "re-enqueued items should come first");
        assert_eq!(final_drain[1].agent_id, "b");
    }
}
