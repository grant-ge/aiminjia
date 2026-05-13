//! Structured message envelope sent via the SendMessage tool.
//!
//! Wire format is JSON with a discriminating `type` field (snake_case):
//!
//! ```json
//! {"type":"text","content":"hello"}
//! {"type":"shutdown_request","reason":"task done"}
//! {"type":"shutdown_response","request_id":"abc","approve":true}
//! {"type":"plan_approval_request","request_id":"abc","plan":"..."}
//! {"type":"plan_approval_response","request_id":"abc","approve":false,"feedback":"missed edge case"}
//! ```
//!
//! Designed so the LLM can produce / consume these as natural JSON via the
//! SendMessage tool's `message` arg.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StructuredMessage {
    /// Free-form text from one agent to another.
    Text { content: String },

    /// Request the recipient terminate its idle loop.  P2.6 hands this to the
    /// recipient's LLM so it can produce a graceful summary before exit.
    ShutdownRequest {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },

    /// Reply to a `ShutdownRequest`.  When `approve` is true the recipient
    /// (originator of the request) treats it as the teammate's acknowledgement
    /// and proceeds with cleanup.
    ShutdownResponse {
        request_id: String,
        approve: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },

    /// Request the recipient approve a plan before the sender executes it.
    PlanApprovalRequest { request_id: String, plan: String },

    /// Reply to a `PlanApprovalRequest`.
    PlanApprovalResponse {
        request_id: String,
        approve: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feedback: Option<String>,
    },
}

impl StructuredMessage {
    /// Constructs a `Text` variant from anything String-like.
    pub fn text(content: impl Into<String>) -> Self {
        Self::Text {
            content: content.into(),
        }
    }

    /// Stable snake_case discriminator name; mirrors the `type` JSON field
    /// and is convenient for log prefixes / metrics labels.
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::Text { .. } => "text",
            Self::ShutdownRequest { .. } => "shutdown_request",
            Self::ShutdownResponse { .. } => "shutdown_response",
            Self::PlanApprovalRequest { .. } => "plan_approval_request",
            Self::PlanApprovalResponse { .. } => "plan_approval_response",
        }
    }

    /// Convenience accessor for the `Text` variant's content; returns `None`
    /// for all other variants.  Used by transcript writers / logging.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { content } => Some(content.as_str()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_helper_constructs_text_variant() {
        let m = StructuredMessage::text("hello");
        assert_eq!(m.variant_name(), "text");
        assert_eq!(m.as_text(), Some("hello"));
    }

    #[test]
    fn variant_name_is_stable_for_all_arms() {
        assert_eq!(
            StructuredMessage::ShutdownRequest { reason: None }.variant_name(),
            "shutdown_request"
        );
        assert_eq!(
            StructuredMessage::ShutdownResponse {
                request_id: "x".into(),
                approve: true,
                reason: None,
            }
            .variant_name(),
            "shutdown_response"
        );
        assert_eq!(
            StructuredMessage::PlanApprovalRequest {
                request_id: "x".into(),
                plan: "p".into(),
            }
            .variant_name(),
            "plan_approval_request"
        );
        assert_eq!(
            StructuredMessage::PlanApprovalResponse {
                request_id: "x".into(),
                approve: false,
                feedback: None,
            }
            .variant_name(),
            "plan_approval_response"
        );
    }
}
