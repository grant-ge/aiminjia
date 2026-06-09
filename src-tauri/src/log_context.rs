//! Per-request log correlation context.
//!
//! Threads `SessionId` / `RunId` (and an optional sub-agent id) through every
//! `log::*` line emitted while a chat turn runs, so one request can be grepped
//! end-to-end across modules. Implemented as a `tokio::task_local!`: the chat
//! turn future is wrapped in [`scoped`], and the log formatter in `lib.rs`
//! reads [`prefix`] synchronously from the emitting task (fern dispatches the
//! format closure inline on the caller's task, so the task-local is visible).
//!
//! Sub-agents run on their own `tokio::spawn`ed task, which does NOT inherit
//! task-locals — re-establish the scope inside the spawn closure with the
//! parent ids plus the agent id.

use std::future::Future;

/// Correlation ids bound to the current task for the duration of a turn.
#[derive(Clone)]
pub struct LogContext {
    session: String,
    run: String,
    agent: Option<String>,
}

impl LogContext {
    pub fn new(session: impl Into<String>, run: impl Into<String>) -> Self {
        Self {
            session: session.into(),
            run: run.into(),
            agent: None,
        }
    }

    /// Attach a sub-agent id (rendered as `a=<id>` in the log prefix).
    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        self.agent = Some(agent.into());
        self
    }
}

tokio::task_local! {
    static CONTEXT: LogContext;
}

/// Run `fut` with `ctx` bound as the current task's correlation context.
pub async fn scoped<F: Future>(ctx: LogContext, fut: F) -> F::Output {
    CONTEXT.scope(ctx, fut).await
}

/// Compact correlation prefix for the current task, e.g. `[s=<sid> r=<rid>]`
/// (or `[s=<sid> r=<rid> a=<agent>]` inside a sub-agent). Returns an empty
/// string when called outside any turn scope (startup, idle, background
/// threads), so non-request logs stay clean.
pub fn prefix() -> String {
    CONTEXT
        .try_with(|c| match &c.agent {
            Some(agent) => format!("[s={} r={} a={}]", c.session, c.run, agent),
            None => format!("[s={} r={}]", c.session, c.run),
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn prefix_is_empty_outside_scope() {
        assert_eq!(prefix(), "");
    }

    #[tokio::test]
    async fn prefix_carries_session_and_run_inside_scope() {
        let out = scoped(LogContext::new("sess_abc", "run_def"), async { prefix() }).await;
        assert_eq!(out, "[s=sess_abc r=run_def]");
        // Scope is unwound after the future completes.
        assert_eq!(prefix(), "");
    }

    #[tokio::test]
    async fn prefix_includes_agent_when_present() {
        let ctx = LogContext::new("sess_abc", "run_def").with_agent("agent_1");
        let out = scoped(ctx, async { prefix() }).await;
        assert_eq!(out, "[s=sess_abc r=run_def a=agent_1]");
    }

    #[tokio::test]
    async fn scope_does_not_cross_spawn() {
        // tokio::spawn starts a fresh task that does not inherit the task-local,
        // mirroring the sub-agent path which re-binds the context explicitly.
        let inner = scoped(LogContext::new("sess_abc", "run_def"), async {
            tokio::spawn(async { prefix() }).await.unwrap()
        })
        .await;
        assert_eq!(inner, "");
    }
}
