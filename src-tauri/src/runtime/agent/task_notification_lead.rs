//! Task notification emitter (P2.5) — when a Teammate's TaskCreate /
//! TaskUpdate / TaskClaim succeeds in Team mode, this module synthesises a
//! `<task-notification>` XML envelope and pushes it into the Lead's inbox so
//! the Lead is informed without polling.
//!
//! Boundaries:
//! - Only fires in Team mode (TeamRegistry has a team for the session).
//! - Never echoes back to the actor — if the Lead itself made the change,
//!   no notification is emitted (avoids self-feedback loops).
//! - Wakes the Lead via [`LeadIdleSupervisor::enqueue`] (P2.4).  Currently
//!   the Path-C continuation spawn is logging-only; P2.5 still records the
//!   pending bit so Path A picks it up at the next turn boundary.

use std::sync::Arc;

use crate::runtime::agent::inbox::{InboxItem, MessageSource};
use crate::runtime::agent::{
    AgentNameRegistry, InboxRegistry, LeadIdleSupervisor, TeamRegistry,
};
use crate::runtime::ids::SessionId;
use crate::runtime::messaging::StructuredMessage;

/// Action labels stamped onto the `<task-notification action="...">`
/// attribute.  Kept as a small enum so callers can't typo a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskAction {
    Created,
    Updated,
    Claimed,
}

impl TaskAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
            Self::Claimed => "claimed",
        }
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Build the XML payload that lands in the Lead's inbox.
pub fn build_envelope(
    task_id: &str,
    actor_name: &str,
    action: TaskAction,
    subject: &str,
    status: &str,
) -> String {
    format!(
        "<task-notification id=\"{}\" actor=\"{}\" action=\"{}\">\n  \
         <subject>{}</subject>\n  \
         <status>{}</status>\n\
         </task-notification>",
        xml_escape(task_id),
        xml_escape(actor_name),
        action.as_str(),
        xml_escape(subject),
        xml_escape(status),
    )
}

/// Async resources needed to deliver a task-notification.  Bundled so the
/// task-tools call site only has one optional handle to thread through.
#[derive(Clone)]
pub struct TaskNotificationDeps {
    pub team_registry: Arc<TeamRegistry>,
    pub agent_names: Arc<AgentNameRegistry>,
    pub inbox_registry: Arc<InboxRegistry>,
    pub lead_idle: Option<Arc<LeadIdleSupervisor>>,
}

/// Outcome reported by [`emit_to_lead`] for diagnostics / tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitOutcome {
    /// Notification successfully pushed to the Lead's inbox.
    Delivered,
    /// No team for this session — task isn't part of an LTR run; skipped.
    NoTeam,
    /// Actor IS the Lead — would create a feedback loop; skipped.
    SkippedSelfActor,
    /// Lead's name resolved to no agent (registry race / cleanup); skipped.
    LeadNotResolved,
    /// Lead has no inbox (rare race); skipped.
    LeadNoInbox,
    /// Inbox closed mid-flight; skipped.
    InboxClosed,
}

/// Push a `<task-notification>` user-message into the Lead's inbox.
///
/// `actor_name` should be the AgentNameRegistry name of the agent that
/// triggered the change (typically the Teammate that owns the calling tool
/// call).  When the actor IS the Lead, the function is a no-op.
pub async fn emit_to_lead(
    deps: &TaskNotificationDeps,
    session: &SessionId,
    actor_name: &str,
    task_id: &str,
    action: TaskAction,
    subject: &str,
    status: &str,
) -> EmitOutcome {
    use crate::runtime::tools::builtin::team_tools::LEAD_NAME;

    // PR2 compat: check if any team exists for this session.
    // PR4 will add team_name parameter for precise team lookup.
    if deps.team_registry.list(session).await.is_empty() {
        return EmitOutcome::NoTeam;
    }
    if actor_name == LEAD_NAME {
        return EmitOutcome::SkippedSelfActor;
    }

    let lead_id = match deps.agent_names.resolve(session, LEAD_NAME).await {
        Some(id) => id,
        None => return EmitOutcome::LeadNotResolved,
    };
    let inbox = match deps.inbox_registry.get(session, &lead_id).await {
        Some(i) => i,
        None => return EmitOutcome::LeadNoInbox,
    };

    let xml = build_envelope(task_id, actor_name, action, subject, status);
    let send_result = inbox
        .send(InboxItem::ChatMessage {
            message: StructuredMessage::text(xml),
            source: MessageSource::System,
        })
        .await;
    if send_result.is_err() {
        return EmitOutcome::InboxClosed;
    }

    if let Some(sup) = deps.lead_idle.as_ref() {
        let key = (session.clone(), lead_id);
        let _wake = sup.enqueue(&key).await;
        // Wake-spawn wiring lands with the chat_turn_driver follow-up; here
        // we only need the supervisor to record the pending bit.
    }

    EmitOutcome::Delivered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_escapes_special_chars_and_uses_action_attr() {
        let xml = build_envelope("t-1", "Alice & Bob", TaskAction::Claimed, "<plot>", "in_progress");
        assert!(xml.contains(r#"id="t-1""#));
        assert!(xml.contains(r#"action="claimed""#));
        assert!(xml.contains("Alice &amp; Bob"));
        assert!(xml.contains("&lt;plot&gt;"));
        assert!(xml.contains("<status>in_progress</status>"));
    }

    #[test]
    fn action_labels_are_stable() {
        assert_eq!(TaskAction::Created.as_str(), "created");
        assert_eq!(TaskAction::Updated.as_str(), "updated");
        assert_eq!(TaskAction::Claimed.as_str(), "claimed");
    }
}
