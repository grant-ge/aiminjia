//! Verify: when a parent LLM emits N concurrency-safe tool_use blocks in one
//! turn (N `spawn_subagent` calls), they execute **in parallel** — total
//! elapsed ≈ max(individual times), not sum.
//!
//! Strategy: inject a `DelayLauncher` (sleep 50 ms), run 4 tool executions
//! concurrently via `futures::future::try_join_all`, assert total < 150 ms
//! (true parallel) rather than > 200 ms (sequential).
//!
//! This also exercises the `is_concurrency_safe = true` path that the
//! dispatcher's `dispatch_batch` uses to group same-batch calls into a
//! `join_all` bucket (see `dispatcher.rs` `dispatch_batch`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::time::sleep;

use app_lib::runtime::agent::registry::AgentRegistry;
use app_lib::runtime::ids::AgentId;
use app_lib::runtime::tools::builtin::spawn_subagent::{
    SpawnAsyncOutcome, SpawnSubagentContext, SpawnSubagentLauncher, SpawnSubagentRequest, SpawnSubagentRuntimeTool,
};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::RuntimeTool;

// ─── Stub launcher with deterministic delay ───────────────────────────────────

struct DelayLauncher {
    delay_ms: u64,
}

#[async_trait]
impl SpawnSubagentLauncher for DelayLauncher {
    async fn launch_sync(
        &self,
        request: SpawnSubagentRequest,
        _context: SpawnSubagentContext,
    ) -> Result<String> {
        sleep(Duration::from_millis(self.delay_ms)).await;
        Ok(format!("done: {}", request.subagent_type))
    }

    async fn launch_async(
        &self,
        request: SpawnSubagentRequest,
        _context: SpawnSubagentContext,
    ) -> Result<SpawnAsyncOutcome> {
        sleep(Duration::from_millis(self.delay_ms)).await;
        Ok(SpawnAsyncOutcome {
            agent_id: AgentId::new("stub-async-id"),
            name: request.name.clone(),
        })
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// Sanity: the tool correctly reports itself as concurrency-safe.
/// If this breaks it means P2.3 was inadvertently reverted.
#[test]
fn spawn_subagent_is_concurrency_safe() {
    let registry = Arc::new(AgentRegistry::with_builtins());
    let launcher = Arc::new(DelayLauncher { delay_ms: 0 });
    let tool = SpawnSubagentRuntimeTool::new(launcher, registry);
    assert!(
        tool.is_concurrency_safe(&serde_json::Value::Null),
        "spawn_subagent must have is_concurrency_safe=true (P2.3)"
    );
}

/// Timing proof: 4 × 50 ms parallel ≈ 50 ms, not 200 ms.
///
/// This test mimics exactly what the dispatcher's `dispatch_batch` does for a
/// batch of `is_concurrency_safe=true` calls: it collects futures and drives
/// them with `futures::future::join_all` (or `try_join_all` here).  The
/// launcher sleep(50 ms) represents real sub-agent launch latency.
#[tokio::test]
async fn parallel_spawn_subagent_calls_run_concurrently() {
    let registry = Arc::new(AgentRegistry::with_builtins());
    let launcher = Arc::new(DelayLauncher { delay_ms: 50 });
    let tool = Arc::new(SpawnSubagentRuntimeTool::new(launcher, registry));

    // Confirm is_concurrency_safe so the test reflects production routing.
    assert!(
        tool.is_concurrency_safe(&serde_json::Value::Null),
        "precondition: spawn_subagent must be concurrency-safe"
    );

    let inputs: Vec<serde_json::Value> = (0..4)
        .map(|i| {
            json!({
                "subagent_type": "explore",
                "prompt": format!("task-{i}"),
                "description": format!("desc-{i}"),
            })
        })
        .collect();

    let start = Instant::now();

    // Build 4 futures and race them concurrently — mirrors dispatcher join_all.
    let futs = inputs.into_iter().map(|input| {
        let tool = tool.clone();
        let ctx = ToolExecutionContext::for_test(
            "sess-parallel",
            "run-parallel",
            format!("tc-{}", uuid::Uuid::new_v4()),
        );
        async move { tool.execute(input, ctx).await }
    });

    let results = futures::future::try_join_all(futs)
        .await
        .expect("all 4 spawn_subagent calls must succeed");

    let elapsed = start.elapsed();
    eprintln!("tool-level parallel elapsed: {:?}", elapsed);

    // All 4 must have returned output.
    assert_eq!(results.len(), 4);
    for r in &results {
        assert!(
            r.content.contains("done: explore"),
            "unexpected launcher output: {}",
            r.content
        );
    }

    // Timing invariant:
    //   Sequential would take ≥ 4 × 50 ms = 200 ms.
    //   Parallel should take ≈ 50 ms.  Allow 150 ms for CI scheduler jitter.
    assert!(
        elapsed < Duration::from_millis(150),
        "expected parallel execution (~50 ms for 4 tasks), got {:?} — \
         check that spawn_subagent.execute() does not hold any blocking lock \
         across the launcher await",
        elapsed
    );
}

/// Dispatcher batch-level verification: `dispatch_batch` groups all
/// `is_concurrency_safe=true` calls into a single concurrent batch.
///
/// We call `dispatch_batch` with 4 `spawn_subagent` inputs and assert that
/// the total wall time is still ~50 ms, not 200 ms.
#[tokio::test]
async fn dispatch_batch_runs_concurrency_safe_tools_in_parallel() {
    use app_lib::runtime::tools::{AllowAllPermissionPipeline, ToolDispatcher};

    let registry = Arc::new(AgentRegistry::with_builtins());
    let launcher = Arc::new(DelayLauncher { delay_ms: 50 });
    let tool = Arc::new(SpawnSubagentRuntimeTool::new(launcher, registry));

    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(tool);

    let calls: Vec<_> = (0..4)
        .map(|i| {
            let ctx = ToolExecutionContext::for_test(
                "sess-batch",
                "run-batch",
                format!("tc-batch-{i}"),
            );
            let input = json!({
                "subagent_type": "explore",
                "prompt": format!("batch-task-{i}"),
                "description": format!("batch-desc-{i}"),
            });
            ("Agent".to_string(), input, ctx)
        })
        .collect();

    let start = Instant::now();
    let results = dispatcher.dispatch_batch(calls).await;
    let elapsed = start.elapsed();
    eprintln!("dispatch_batch parallel elapsed: {:?}", elapsed);

    assert_eq!(results.len(), 4, "all 4 calls must produce a result");
    for r in &results {
        assert!(r.is_ok(), "dispatch_batch result must be Ok, got: {:?}", r);
    }

    assert!(
        elapsed < Duration::from_millis(150),
        "dispatch_batch with is_concurrency_safe=true tools must run in parallel \
         (~50 ms), got {:?}",
        elapsed
    );
}
