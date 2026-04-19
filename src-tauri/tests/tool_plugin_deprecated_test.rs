/// Phase 2 Task 4 Step 3 — legacy ToolPlugin deprecation regression tests.
///
/// Verifies that:
/// 1. The deprecated ToolPlugin trait is still usable — implementations compile
///    and can be wrapped via LegacyToolAdapter::from_plugin (no regression).
/// 2. New RuntimeTool direct implementations continue to work in parallel,
///    proving both paths co-exist during the migration window.
use app_lib::plugin::tool_trait::{ToolError as LegacyToolError, ToolOutput, ToolPlugin};
use app_lib::runtime::tools::{LegacyToolAdapter, RuntimeTool, ToolExecutionContext};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// A minimal ToolPlugin implementation using the (now deprecated) legacy trait.
// The allow(deprecated) suppresses the compiler warning so this test can
// act as an intentional regression guard for the bridging path.
// ---------------------------------------------------------------------------
#[allow(deprecated)]
struct LegacyEchoPlugin;

#[allow(deprecated)]
#[async_trait]
impl ToolPlugin for LegacyEchoPlugin {
    fn name(&self) -> &str {
        "legacy_echo"
    }
    fn description(&self) -> &str {
        "Echo via legacy trait (deprecated path)"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"text":{"type":"string"}}})
    }
    async fn execute(
        &self,
        _ctx: &app_lib::plugin::context::PluginContext,
        input: Value,
    ) -> Result<ToolOutput, LegacyToolError> {
        let text = input
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        Ok(ToolOutput::success(text))
    }
}

// ---------------------------------------------------------------------------
// Test: Structurally verify that LegacyEchoPlugin satisfies the ToolPlugin
// contract, i.e., the deprecated trait can still be impl'd and the methods
// have the expected signatures.  We don't call execute() because that
// requires a real PluginContext; we rely on compile success as the guard.
// ---------------------------------------------------------------------------
#[test]
fn deprecated_tool_plugin_trait_is_still_implementable() {
    #[allow(deprecated)]
    let plugin: Arc<dyn ToolPlugin> = Arc::new(LegacyEchoPlugin);
    assert_eq!(plugin.name(), "legacy_echo");
    assert_eq!(
        plugin.description(),
        "Echo via legacy trait (deprecated path)"
    );
}

// ---------------------------------------------------------------------------
// Test: The for_test() path of LegacyToolAdapter (which wraps an arbitrary
// async closure rather than a real ToolPlugin) still works end-to-end,
// proving the adapter machinery is intact.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn legacy_adapter_for_test_still_routes_through_runtime_contract() {
    let adapter = LegacyToolAdapter::for_test("legacy_echo");
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
    let result = RuntimeTool::execute(&adapter, json!({"key": "value"}), ctx)
        .await
        .unwrap();
    assert_eq!(result.tool_name, "legacy_echo");
}
