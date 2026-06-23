//! Tests for P6.2: async path of `spawn_subagent` tool.
//!
//! Verifies:
//! 1. `run_in_background=true, name=Some` → result JSON contains
//!    `status="async_launched"`, `agent_id`, `name` fields.
//! 2. `run_in_background=true, name=None` → result JSON `name` field is null.
//! 3. Async path calls `launch_async`, not `launch_sync`.
//! 4. Sync path (run_in_background=false or absent) calls `launch_sync`, not `launch_async`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use app_lib::runtime::agent::registry::AgentRegistry;
use app_lib::runtime::ids::AgentId;
use app_lib::runtime::tools::builtin::spawn_subagent::{
    SpawnAsyncOutcome, SpawnForegroundAutoOutcome, SpawnSubagentContext, SpawnSubagentLauncher,
    SpawnSubagentRequest, SpawnSubagentRuntimeTool,
};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::RuntimeTool;

// ─── Stub that records calls ──────────────────────────────────────────────────

struct RecordingLauncher {
    sync_calls: Arc<AtomicU32>,
    async_calls: Arc<AtomicU32>,
    foreground_auto_calls: Arc<AtomicU32>,
    /// If Some, returned as the agent_id in async outcomes.
    agent_id: String,
    foreground_auto_backgrounded: Option<String>,
}

impl RecordingLauncher {
    fn new(agent_id: impl Into<String>) -> Self {
        Self {
            sync_calls: Arc::new(AtomicU32::new(0)),
            async_calls: Arc::new(AtomicU32::new(0)),
            foreground_auto_calls: Arc::new(AtomicU32::new(0)),
            agent_id: agent_id.into(),
            foreground_auto_backgrounded: None,
        }
    }

    fn with_foreground_auto_backgrounded(mut self, agent_id: impl Into<String>) -> Self {
        self.foreground_auto_backgrounded = Some(agent_id.into());
        self
    }
}

#[async_trait]
impl SpawnSubagentLauncher for RecordingLauncher {
    async fn launch_sync(
        &self,
        _request: SpawnSubagentRequest,
        _context: SpawnSubagentContext,
    ) -> Result<String> {
        self.sync_calls.fetch_add(1, Ordering::SeqCst);
        Ok("sync-output".into())
    }

    async fn launch_async(
        &self,
        request: SpawnSubagentRequest,
        _context: SpawnSubagentContext,
    ) -> Result<SpawnAsyncOutcome> {
        self.async_calls.fetch_add(1, Ordering::SeqCst);
        Ok(SpawnAsyncOutcome {
            agent_id: AgentId::new(self.agent_id.clone()),
            name: request.name.clone(),
        })
    }

    async fn launch_foreground_auto_background(
        &self,
        request: SpawnSubagentRequest,
        _context: SpawnSubagentContext,
    ) -> Result<SpawnForegroundAutoOutcome> {
        self.foreground_auto_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(agent_id) = self.foreground_auto_backgrounded.as_ref() {
            Ok(SpawnForegroundAutoOutcome::Backgrounded {
                agent_id: AgentId::new(agent_id.clone()),
                name: request.name.clone(),
                auto_background_after_ms: request.auto_background_after_ms.unwrap_or(15_000),
            })
        } else {
            Ok(SpawnForegroundAutoOutcome::Completed("sync-output".into()))
        }
    }
}

// ─── Helper ───────────────────────────────────────────────────────────────────

fn build_tool_with_launcher(launcher: Arc<dyn SpawnSubagentLauncher>) -> SpawnSubagentRuntimeTool {
    let registry = Arc::new(AgentRegistry::with_builtins());
    SpawnSubagentRuntimeTool::new(launcher, registry)
}

// ─── Test 1: async_launched shape with name ───────────────────────────────────

#[tokio::test]
async fn async_launched_returns_immediately_with_agent_id() {
    let launcher = Arc::new(RecordingLauncher::new("a1"));
    let tool = build_tool_with_launcher(launcher);
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");

    let result = tool
        .execute(
            json!({
                "subagent_type": "explore",
                "prompt": "do work",
                "description": "async test",
                "run_in_background": true,
                "name": "w1"
            }),
            ctx,
        )
        .await
        .expect("async path must not Err");

    let parsed: serde_json::Value =
        serde_json::from_str(&result.content).expect("result must be valid JSON");

    assert_eq!(
        parsed.get("status").and_then(|v| v.as_str()),
        Some("async_launched"),
        "status must be async_launched, got: {}",
        result.content
    );
    assert_eq!(
        parsed.get("agent_id").and_then(|v| v.as_str()),
        Some("a1"),
        "agent_id must match stub's returned id, got: {}",
        result.content
    );
    assert_eq!(
        parsed.get("name").and_then(|v| v.as_str()),
        Some("w1"),
        "name must match input name, got: {}",
        result.content
    );
}

// ─── Test 2: name=None → JSON name field is null ─────────────────────────────

#[tokio::test]
async fn async_launched_without_name_returns_null() {
    let launcher = Arc::new(RecordingLauncher::new("anon-agent"));
    let tool = build_tool_with_launcher(launcher);
    let ctx = ToolExecutionContext::for_test("conv-2", "run-2", "tc-2");

    let result = tool
        .execute(
            json!({
                "subagent_type": "explore",
                "prompt": "do anonymous work",
                "description": "no name test",
                "run_in_background": true
            }),
            ctx,
        )
        .await
        .expect("async path without name must not Err");

    let parsed: serde_json::Value =
        serde_json::from_str(&result.content).expect("result must be valid JSON");

    assert_eq!(
        parsed.get("status").and_then(|v| v.as_str()),
        Some("async_launched"),
    );
    assert!(
        parsed.get("name").map(|v| v.is_null()).unwrap_or(false),
        "name field must be null when not provided, got: {}",
        result.content
    );
}

// ─── Test 3: run_in_background=true → launch_async called, launch_sync NOT ───

#[tokio::test]
async fn launch_async_called_when_run_in_background_true() {
    let sync_calls = Arc::new(AtomicU32::new(0));
    let async_calls = Arc::new(AtomicU32::new(0));
    let foreground_auto_calls = Arc::new(AtomicU32::new(0));
    let launcher = Arc::new(RecordingLauncher {
        sync_calls: sync_calls.clone(),
        async_calls: async_calls.clone(),
        foreground_auto_calls: foreground_auto_calls.clone(),
        agent_id: "bg-agent".into(),
        foreground_auto_backgrounded: None,
    });
    let tool = build_tool_with_launcher(launcher);
    let ctx = ToolExecutionContext::for_test("conv-3", "run-3", "tc-3");

    tool.execute(
        json!({
            "subagent_type": "explore",
            "prompt": "background job",
            "description": "routing test",
            "run_in_background": true
        }),
        ctx,
    )
    .await
    .expect("must succeed");

    assert_eq!(
        async_calls.load(Ordering::SeqCst),
        1,
        "launch_async must have been called exactly once"
    );
    assert_eq!(
        sync_calls.load(Ordering::SeqCst),
        0,
        "launch_sync must NOT have been called for run_in_background=true"
    );
    assert_eq!(
        foreground_auto_calls.load(Ordering::SeqCst),
        0,
        "foreground auto path must NOT have been called for run_in_background=true"
    );
}

// ─── Test 4a: run_in_background=false → foreground auto path called ──────────

#[tokio::test]
async fn run_in_background_false_calls_foreground_auto_background() {
    let sync_calls = Arc::new(AtomicU32::new(0));
    let async_calls = Arc::new(AtomicU32::new(0));
    let foreground_auto_calls = Arc::new(AtomicU32::new(0));
    let launcher = Arc::new(RecordingLauncher {
        sync_calls: sync_calls.clone(),
        async_calls: async_calls.clone(),
        foreground_auto_calls: foreground_auto_calls.clone(),
        agent_id: "fg-agent".into(),
        foreground_auto_backgrounded: None,
    });
    let tool = build_tool_with_launcher(launcher);
    let ctx = ToolExecutionContext::for_test("conv-4a", "run-4a", "tc-4a");

    let result = tool
        .execute(
            json!({
                "subagent_type": "explore",
                "prompt": "sync job",
                "description": "sync routing test",
                "run_in_background": false
            }),
            ctx,
        )
        .await
        .expect("must succeed");

    assert_eq!(result.content, "sync-output");
    assert_eq!(
        foreground_auto_calls.load(Ordering::SeqCst),
        1,
        "launch_foreground_auto_background must have been called exactly once for run_in_background=false"
    );
    assert_eq!(
        sync_calls.load(Ordering::SeqCst),
        0,
        "launch_sync must NOT have been called for run_in_background=false"
    );
    assert_eq!(
        async_calls.load(Ordering::SeqCst),
        0,
        "launch_async must NOT have been called for run_in_background=false"
    );
}

// ─── Test 4b: foreground auto path may return async_launched JSON ────────────

#[tokio::test]
async fn foreground_auto_backgrounded_returns_task_json() {
    let launcher = Arc::new(
        RecordingLauncher::new("fg-agent-default")
            .with_foreground_auto_backgrounded("agent-auto-1"),
    );
    let tool = build_tool_with_launcher(launcher);
    let ctx = ToolExecutionContext::for_test("conv-4b", "run-4b", "tc-4b");

    let result = tool
        .execute(
            json!({
                "subagent_type": "explore",
                "prompt": "default routing",
                "description": "omitted field test",
                "_auto_background_after_ms": 5
            }),
            ctx,
        )
        .await
        .expect("must succeed");
    let body: Value = serde_json::from_str(&result.content).expect("json body");

    assert_eq!(
        body["status"], "async_launched",
        "status must be async_launched when foreground path auto-backgrounds"
    );
    assert_eq!(
        body["agent_id"], "agent-auto-1",
        "agent_id must match promoted agent id"
    );
    assert_eq!(body["task_id"], "agent-auto-1");
    assert_eq!(body["task_type"], "local_agent");
    assert_eq!(body["assistant_auto_backgrounded"], true);
    assert_eq!(body["auto_background_after_ms"], 5);
    assert_eq!(
        body.get("name").map(Value::is_null),
        Some(true),
        "name should stay null when omitted"
    );
}
