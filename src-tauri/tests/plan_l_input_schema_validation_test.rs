#[test]
fn l1_input_validation_error_formats_tool_name_and_message() {
    use app_lib::runtime::tools::executor::ToolError;

    let err = ToolError::InputValidationError {
        tool_name: "bash".to_string(),
        message: "Missing required field: command".to_string(),
    };

    let display = err.to_string();
    assert!(display.contains("bash"), "got: {display}");
    assert!(
        display.contains("Missing required field: command"),
        "got: {display}"
    );
}

#[test]
fn l1_input_validation_error_is_retriable_distinguishable_from_execution_failed() {
    use app_lib::runtime::tools::executor::ToolError;

    let validation_err = ToolError::InputValidationError {
        tool_name: "write_file".to_string(),
        message: "field path is required".to_string(),
    };
    let exec_err = ToolError::ExecutionFailed("disk full".to_string());

    let is_validation = matches!(validation_err, ToolError::InputValidationError { .. });
    let is_exec = matches!(exec_err, ToolError::ExecutionFailed(_));

    assert!(is_validation);
    assert!(is_exec);
}

mod l2_validate_input_trait {
    use async_trait::async_trait;
    use serde_json::{json, Value};

    use app_lib::runtime::tools::definition::ToolDefinition;
    use app_lib::runtime::tools::executor::{ToolError, ToolResult};
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};

    struct StrictTool;

    #[async_trait]
    impl RuntimeTool for StrictTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("strict_tool", "test tool with validation")
        }

        fn validate_input(&self, input: &Value) -> Option<ToolError> {
            if input.get("command").is_none() {
                return Some(ToolError::InputValidationError {
                    tool_name: "strict_tool".to_string(),
                    message: "Missing required field: command".to_string(),
                });
            }
            None
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("strict_tool", "ok", None))
        }
    }

    struct PermissiveTool;

    #[async_trait]
    impl RuntimeTool for PermissiveTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("permissive_tool", "test tool without validation")
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("permissive_tool", "ok", None))
        }
    }

    #[test]
    fn l2_default_validate_input_returns_none() {
        let tool = PermissiveTool;
        let input = json!({"anything": "goes"});
        assert!(tool.validate_input(&input).is_none());
    }

    #[test]
    fn l2_override_validate_input_returns_error_when_missing_field() {
        let tool = StrictTool;
        let bad_input = json!({"not_command": "ls"});
        let result = tool.validate_input(&bad_input);
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), ToolError::InputValidationError { .. }));
    }

    #[test]
    fn l2_override_validate_input_returns_none_when_valid() {
        let tool = StrictTool;
        let good_input = json!({"command": "ls -la"});
        let result = tool.validate_input(&good_input);
        assert!(result.is_none());
    }
}

mod l3_dispatcher_validation_gate {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use async_trait::async_trait;
    use serde_json::{json, Value};

    use app_lib::runtime::tools::definition::ToolDefinition;
    use app_lib::runtime::tools::executor::{ToolError, ToolResult};
    use app_lib::runtime::tools::{
        AllowAllPermissionPipeline, RuntimeTool, ToolDispatcher, ToolExecutionContext,
    };

    struct ValidatingTool {
        executed: Arc<AtomicBool>,
    }

    #[async_trait]
    impl RuntimeTool for ValidatingTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("validating_tool", "requires 'path' field")
        }

        fn validate_input(&self, input: &Value) -> Option<ToolError> {
            if input.get("path").is_none() {
                return Some(ToolError::InputValidationError {
                    tool_name: "validating_tool".to_string(),
                    message: "Missing required field: path".to_string(),
                });
            }
            None
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            self.executed.store(true, Ordering::SeqCst);
            Ok(ToolResult::new("validating_tool", "executed", None))
        }
    }

    #[tokio::test]
    async fn l3_dispatcher_returns_validation_error_before_execute() {
        let executed = Arc::new(AtomicBool::new(false));
        let tool = ValidatingTool { executed: executed.clone() };
        let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
        dispatcher.register(Arc::new(tool));

        let ctx = ToolExecutionContext::for_test("sess-l3", "run-l3", "tc-l3");
        let bad_input = json!({"not_path": "/foo"});

        let result = dispatcher.dispatch("validating_tool", bad_input, ctx).await;

        match result {
            Err(ToolError::InputValidationError { .. }) => {}
            Err(other) => panic!("expected InputValidationError, got: {other}"),
            Ok(_) => panic!("expected validation error, got successful dispatch"),
        }
        assert!(!executed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn l3_dispatcher_executes_when_validation_passes() {
        let executed = Arc::new(AtomicBool::new(false));
        let tool = ValidatingTool { executed: executed.clone() };
        let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
        dispatcher.register(Arc::new(tool));

        let ctx = ToolExecutionContext::for_test("sess-l3b", "run-l3b", "tc-l3b");
        let good_input = json!({"path": "/workspace/file.txt"});

        let result = dispatcher.dispatch("validating_tool", good_input, ctx).await;

        assert!(result.is_ok());
        assert!(executed.load(Ordering::SeqCst));
    }
}

mod l4_query_engine_validation_error_encoding {
    use std::sync::Arc;

    use async_trait::async_trait;
    use serde_json::{json, Value};

    use app_lib::runtime::chat::tool_round_types::{RuntimeToolCallOutcome, RuntimeToolCallRequest};
    use app_lib::runtime::event_bus::RuntimeEventBus;
    use app_lib::runtime::identity::IdentityMapping;
    use app_lib::runtime::ids::RunId;
    use app_lib::runtime::query_engine::QueryEngine;
    use app_lib::runtime::state::TurnState;
    use app_lib::runtime::tools::definition::ToolDefinition;
    use app_lib::runtime::tools::executor::{ToolError, ToolResult};
    use app_lib::runtime::tools::{
        AllowAllPermissionPipeline, RuntimeTool, ToolDispatcher, ToolExecutionContext,
    };

    struct AlwaysFailsValidation;

    #[async_trait]
    impl RuntimeTool for AlwaysFailsValidation {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("always_invalid", "always fails validation")
        }

        fn validate_input(&self, _input: &Value) -> Option<ToolError> {
            Some(ToolError::InputValidationError {
                tool_name: "always_invalid".to_string(),
                message: "required field missing".to_string(),
            })
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            unreachable!("execute must not be called when validation fails")
        }
    }

    #[tokio::test]
    async fn l4_validation_error_encoded_as_retriable_tool_result() {
        let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
        dispatcher.register(Arc::new(AlwaysFailsValidation));

        let engine = QueryEngine::for_test(dispatcher);
        let bus = RuntimeEventBus::new();
        let mapping = IdentityMapping::from_legacy_conversation_id("sess-l4");
        let turn = TurnState::new(mapping, RunId::new("run-l4"), "test".to_string());

        let call = RuntimeToolCallRequest {
            tool_call_id: "tc-l4".to_string(),
            tool_name: "always_invalid".to_string(),
            args: json!({}),
            purpose: None,
        };

        let outcome = engine
            .run_tool_call_with_bus(&turn, &bus, call)
            .await
            .expect("run_tool_call_with_bus should not Err on validation failure");

        match outcome {
            RuntimeToolCallOutcome::Completed {
                is_error,
                content,
                ..
            } => {
                assert!(is_error);
                assert!(content.contains("InputValidationError"), "got: {content}");
                assert!(content.contains("required field missing"), "got: {content}");
            }
            other => panic!("expected Completed outcome, got {:?}", other),
        }
    }
}

mod l5_builtin_tool_validation {
    use serde_json::json;

    use app_lib::runtime::tools::builtin::bash::BashTool;
    use app_lib::runtime::tools::builtin::grep::GrepContentTool;
    use app_lib::runtime::tools::executor::ToolError;
    use app_lib::runtime::tools::RuntimeTool;

    #[test]
    fn l5_bash_validates_missing_command_field() {
        let tool = BashTool;
        let bad = json!({"timeout_secs": 30});
        let result = tool.validate_input(&bad);
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), ToolError::InputValidationError { .. }));
    }

    #[test]
    fn l5_bash_validates_command_must_be_string() {
        let tool = BashTool;
        let bad = json!({"command": 42});
        let result = tool.validate_input(&bad);
        assert!(result.is_some());
    }

    #[test]
    fn l5_bash_accepts_valid_input() {
        let tool = BashTool;
        let good = json!({"command": "ls -la"});
        assert!(tool.validate_input(&good).is_none());
    }

    #[test]
    fn l5_bash_accepts_valid_input_with_timeout() {
        let tool = BashTool;
        let good = json!({"command": "sleep 1", "timeout_secs": 5});
        assert!(tool.validate_input(&good).is_none());
    }

    #[test]
    fn l5_grep_validates_missing_pattern_field() {
        let tool = GrepContentTool;
        let bad = json!({"path": "/workspace"});
        let result = tool.validate_input(&bad);
        assert!(result.is_some());
    }

    #[test]
    fn l5_grep_accepts_valid_input() {
        let tool = GrepContentTool;
        let good = json!({"pattern": "fn main", "path": "/workspace"});
        assert!(tool.validate_input(&good).is_none());
    }
}
