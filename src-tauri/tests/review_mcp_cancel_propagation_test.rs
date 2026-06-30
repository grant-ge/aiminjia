use async_trait::async_trait;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use app_lib::runtime::cancellation::{CancellationReason, CancellationToken};
use app_lib::runtime::mcp::{
    McpConnection, McpResult, McpRuntimeTool, McpServerConfig, McpToolDefinition,
    SharedMcpConnection,
};
use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};

/// Stub connection whose `call_tool` future never resolves until the test
/// finishes — simulates an MCP server that hangs.
struct HangingConnection {
    config: McpServerConfig,
    disconnect_called: Arc<AtomicBool>,
}

#[async_trait]
impl McpConnection for HangingConnection {
    async fn connect(&self) -> McpResult<()> {
        Ok(())
    }
    async fn disconnect(&self) -> McpResult<()> {
        self.disconnect_called.store(true, Ordering::SeqCst);
        Ok(())
    }
    fn is_connected(&self) -> bool {
        true
    }
    fn server_name(&self) -> &str {
        "stub"
    }
    async fn list_tools(&self) -> McpResult<Vec<McpToolDefinition>> {
        Ok(vec![])
    }
    async fn call_tool(&self, _tool_name: &str, _arguments: Value) -> McpResult<Value> {
        // Simulates an MCP server that never responds.
        std::future::pending::<McpResult<Value>>().await
    }
    fn config(&self) -> &McpServerConfig {
        &self.config
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_runtime_tool_aborts_within_one_second_when_token_cancelled() {
    let config = McpServerConfig {
        name: "stub".into(),
        transport_type: "stdio".into(),
        endpoint: "noop".into(),
        env_vars: None,
    };
    let disconnect_called = Arc::new(AtomicBool::new(false));
    let conn: SharedMcpConnection = Arc::new(HangingConnection {
        config,
        disconnect_called: disconnect_called.clone(),
    });
    let tool_def = McpToolDefinition {
        server_name: "stub".into(),
        tool_name: "echo".into(),
        description: "stub tool".into(),
        input_schema: serde_json::json!({}),
    };
    let tool = McpRuntimeTool::new(tool_def, conn);

    let cancel = CancellationToken::new();
    let mut ctx = ToolExecutionContext::for_test("c1", "r1", "tc1");
    ctx.cancellation = cancel.clone();

    let exec = tokio::spawn(async move { tool.execute(serde_json::json!({}), ctx).await });

    // Let the execute() call get into the select! before we cancel.
    tokio::time::sleep(Duration::from_millis(50)).await;
    cancel.cancel_with_reason(CancellationReason::UserCancel);

    let result = tokio::time::timeout(Duration::from_secs(1), exec)
        .await
        .expect("must abort within 1s of cancel")
        .expect("task should not panic");

    assert!(
        result.is_err(),
        "expected ToolError after cancel, got: {:?}",
        result
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("cancelled"),
        "error message should mention 'cancelled', got: {err_msg}"
    );
    assert!(
        disconnect_called.load(Ordering::SeqCst),
        "disconnect_on_cancel must invoke disconnect() on the connection"
    );
}
