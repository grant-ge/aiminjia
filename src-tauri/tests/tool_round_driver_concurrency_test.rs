use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use app_lib::runtime::chat::tool_round_driver::ToolRoundDriver;
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::chat::ToolRoundResult;
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::tools::dispatcher::RuntimeTool;
use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::{
    AllowAllPermissionPipeline, ToolDefinition, ToolDispatcher, ToolError, ToolExecutionContext,
    ToolResult,
};
use async_trait::async_trait;
use serde_json::{json, Value};

#[derive(Default)]
struct ConcurrencyProbe {
    current: AtomicI32,
    max: AtomicI32,
}

impl ConcurrencyProbe {
    fn enter(&self) {
        let now = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        let mut observed = self.max.load(Ordering::SeqCst);
        while now > observed {
            match self
                .max
                .compare_exchange(observed, now, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => break,
                Err(actual) => observed = actual,
            }
        }
    }

    fn exit(&self) {
        self.current.fetch_sub(1, Ordering::SeqCst);
    }

    fn max(&self) -> i32 {
        self.max.load(Ordering::SeqCst)
    }
}

struct ProbeTool {
    name: &'static str,
    concurrency_safe: bool,
    probe: Arc<ConcurrencyProbe>,
}

#[async_trait]
impl RuntimeTool for ProbeTool {
    fn id(&self) -> &str {

        self.name

    }


    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new(self.name, "concurrency probe")
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        self.concurrency_safe
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        self.probe.enter();
        tokio::time::sleep(Duration::from_millis(50)).await;
        self.probe.exit();
        Ok(ToolResult::new(self.name, self.name, None))
    }
}

fn make_turn() -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id("tool-round-concurrency-test");
    TurnState::new(
        mapping,
        RunId::new("run-tool-round-concurrency"),
        "test".to_string(),
    )
}

fn call(id: &str, tool_name: &str) -> RuntimeToolCallRequest {
    RuntimeToolCallRequest {
        tool_call_id: id.to_string(),
        tool_name: tool_name.to_string(),
        args: json!({}),
        purpose: None,
    }
}

#[tokio::test]
async fn concurrency_safe_tools_dispatch_in_parallel() {
    let probe = Arc::new(ConcurrencyProbe::default());
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(ProbeTool {
        name: "safe_a",
        concurrency_safe: true,
        probe: probe.clone(),
    }));
    dispatcher.register(Arc::new(ProbeTool {
        name: "safe_b",
        concurrency_safe: true,
        probe: probe.clone(),
    }));

    let driver = ToolRoundDriver::new(QueryEngine::with_dispatcher(dispatcher));
    let results = driver
        .execute_round(
            &make_turn(),
            &RuntimeEventBus::new(),
            vec![call("tc-safe-a", "safe_a"), call("tc-safe-b", "safe_b")],
        )
        .await;

    assert_eq!(results.len(), 2);
    assert!(results
        .iter()
        .all(|r| matches!(r, ToolRoundResult::Ok(o) if !o.is_error())));
    assert_eq!(probe.max(), 2, "concurrency-safe tools must overlap");
}

#[tokio::test]
async fn non_safe_tools_dispatch_serially() {
    let probe = Arc::new(ConcurrencyProbe::default());
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(ProbeTool {
        name: "unsafe_a",
        concurrency_safe: false,
        probe: probe.clone(),
    }));
    dispatcher.register(Arc::new(ProbeTool {
        name: "unsafe_b",
        concurrency_safe: false,
        probe: probe.clone(),
    }));

    let driver = ToolRoundDriver::new(QueryEngine::with_dispatcher(dispatcher));
    let results = driver
        .execute_round(
            &make_turn(),
            &RuntimeEventBus::new(),
            vec![
                call("tc-unsafe-a", "unsafe_a"),
                call("tc-unsafe-b", "unsafe_b"),
            ],
        )
        .await;

    assert_eq!(results.len(), 2);
    assert!(results
        .iter()
        .all(|r| matches!(r, ToolRoundResult::Ok(o) if !o.is_error())));
    assert_eq!(
        probe.max(),
        1,
        "non-concurrency-safe tools must run serially"
    );
}

#[tokio::test]
async fn mixed_safe_unsafe_preserves_order() {
    let safe_probe = Arc::new(ConcurrencyProbe::default());
    let unsafe_probe = Arc::new(ConcurrencyProbe::default());
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(ProbeTool {
        name: "unsafe_a",
        concurrency_safe: false,
        probe: unsafe_probe.clone(),
    }));
    dispatcher.register(Arc::new(ProbeTool {
        name: "safe_a",
        concurrency_safe: true,
        probe: safe_probe.clone(),
    }));
    dispatcher.register(Arc::new(ProbeTool {
        name: "unsafe_b",
        concurrency_safe: false,
        probe: unsafe_probe,
    }));
    dispatcher.register(Arc::new(ProbeTool {
        name: "safe_b",
        concurrency_safe: true,
        probe: safe_probe,
    }));

    let driver = ToolRoundDriver::new(QueryEngine::with_dispatcher(dispatcher));
    let results = driver
        .execute_round(
            &make_turn(),
            &RuntimeEventBus::new(),
            vec![
                call("tc-0", "unsafe_a"),
                call("tc-1", "safe_a"),
                call("tc-2", "unsafe_b"),
                call("tc-3", "safe_b"),
            ],
        )
        .await;

    let ids: Vec<_> = results
        .iter()
        .map(|result| match result {
            ToolRoundResult::Ok(outcome) => outcome.tool_call_id().to_string(),
            ToolRoundResult::Blocked(blocked) => blocked.tool_call_id.clone(),
        })
        .collect();

    assert_eq!(ids, vec!["tc-0", "tc-1", "tc-2", "tc-3"]);
}
