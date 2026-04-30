use anyhow::Result;

use crate::runtime::agent::invocation::AgentInvocation;
use crate::runtime::cancellation::CancellationToken;

pub async fn run_child(
    _invocation: &AgentInvocation,
    cancellation: CancellationToken,
) -> Result<()> {
    if cancellation.is_cancelled() {
        anyhow::bail!("cancelled")
    }
    Ok(())
}
