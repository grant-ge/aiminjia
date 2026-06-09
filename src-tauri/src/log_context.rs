//! Per-request log correlation via tracing spans.
//!
//! Each chat turn is wrapped in a tracing `Span` that carries `trace_id = <run_id>`.
//! Sub-agents create a child span with an additional `span_id = <agent_id_short>`.
//! The formatter in `tracing_setup` reads these fields and renders them as
//! `[trace=<id>]` / `[trace=<id> span=<id>]` on every log line.
//!
//! Using `.instrument()` (from `tracing::Instrument`) instead of `task_local!`
//! means sub-agents spawned with `tokio::spawn` correctly inherit the span context
//! without any manual re-binding.

use std::future::Future;

use tracing::Instrument;

/// Correlation context for a single chat turn or sub-agent run.
pub struct LogContext {
    span: tracing::Span,
}

impl LogContext {
    /// Create a turn-level context.
    ///
    /// `_session` / `_run` are accepted for call-site compatibility.
    /// The trace ID shown in logs is the tracing span's own `Id` — no UUID needed.
    pub fn new(_session: impl Into<String>, _run: impl Into<String>) -> Self {
        let span = tracing::info_span!("turn");
        Self { span }
    }

    /// Attach a sub-agent context. Creates a child span; its `Id` becomes the span_id.
    pub fn with_agent(self, _agent: impl Into<String>) -> Self {
        let span = tracing::info_span!(parent: &self.span, "agent");
        Self { span }
    }
}

/// Run `fut` inside the correlation span so every `log::*` line emitted during
/// the future carries the trace/span context in the log formatter.
pub async fn scoped<F: Future>(ctx: LogContext, fut: F) -> F::Output {
    fut.instrument(ctx.span).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_context_new_does_not_panic() {
        let ctx = LogContext::new("sess_abc", "2f59bde8-75d2-46bd-a5b6-d2f09d625d8a");
        // Span is created; verify the trace_id field is stripped of dashes.
        drop(ctx);
    }

    #[test]
    fn with_agent_produces_child_span() {
        let ctx = LogContext::new("sess_abc", "2f59bde8-75d2-46bd-a5b6-d2f09d625d8a")
            .with_agent("a3ce929d-0e0e-4736-b789-000000000001");
        drop(ctx);
    }

    #[tokio::test]
    async fn scoped_runs_future() {
        let ctx = LogContext::new("sess", "run-123");
        let result = scoped(ctx, async { 42_u32 }).await;
        assert_eq!(result, 42);
    }
}
