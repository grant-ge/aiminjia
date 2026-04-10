use crate::runtime::events::RuntimeEventKind;
use crate::runtime::ids::AgentId;

/// Build the `AgentIdle` event kind for a completed sub-agent.
/// This event signals the UI that the background sub-agent has finished.
pub fn bridge_agent_summary(agent_id: AgentId) -> RuntimeEventKind {
    RuntimeEventKind::AgentIdle { agent_id }
}

/// Summarise a sub-agent result for storage/transport.
///
/// Returns a concise string that is persisted in the invocation record and
/// can later be retrieved by the parent run via `AgentRuntime::get_summary`.
pub fn format_sub_agent_summary(output: &str, iterations_used: usize, files_count: usize) -> String {
    let short = if output.len() > 500 {
        let end = output
            .char_indices()
            .take_while(|(i, _)| *i < 500)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(500.min(output.len()));
        format!("{}...", &output[..end])
    } else {
        output.to_owned()
    };
    format!(
        "iterations={} files={} output={}",
        iterations_used, files_count, short
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::ids::AgentId;

    #[test]
    fn bridge_agent_summary_produces_agent_idle() {
        let id = AgentId::new("agent-42");
        let kind = bridge_agent_summary(id.clone());
        assert!(
            matches!(kind, RuntimeEventKind::AgentIdle { agent_id } if agent_id == id),
            "expected AgentIdle for {id:?}"
        );
    }

    #[test]
    fn format_sub_agent_summary_truncates_long_output() {
        let long = "x".repeat(1000);
        let s = format_sub_agent_summary(&long, 3, 2);
        assert!(s.contains("iterations=3"));
        assert!(s.contains("files=2"));
        assert!(s.len() < 600, "summary should be truncated: len={}", s.len());
    }

    #[test]
    fn format_sub_agent_summary_keeps_short_output() {
        let s = format_sub_agent_summary("done", 1, 0);
        assert_eq!(s, "iterations=1 files=0 output=done");
    }
}
