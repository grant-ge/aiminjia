use app_lib::runtime::hooks::{HookConfig, HookDecision, HookEvent, HookRunner};

#[test]
fn hook_config_roundtrips_serde() {
    let config = HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo '{\"behavior\":\"allow\"}'".to_string(),
        tool_filter: None,
        timeout_secs: Some(30),
    };
    let json = serde_json::to_string(&config).unwrap();
    let back: HookConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.command, config.command);
    assert!(matches!(back.event, HookEvent::PreToolUse));
}

#[tokio::test]
async fn hook_runner_allow_decision() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo '{\"behavior\":\"allow\"}'".to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    };
    let result = runner
        .run_hook(&config, "bash_tool", &serde_json::json!({"command": "ls"}))
        .await
        .unwrap();
    assert!(matches!(result.decision, HookDecision::Allow));
    assert!(result.updated_input.is_none());
    assert!(!result.prevent_continuation);
}

#[tokio::test]
async fn hook_runner_deny_decision() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo '{\"behavior\":\"deny\",\"message\":\"blocked\"}'".to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    };
    let result = runner
        .run_hook(
            &config,
            "bash_tool",
            &serde_json::json!({"command": "rm -rf /"}),
        )
        .await
        .unwrap();
    assert!(matches!(result.decision, HookDecision::Deny { .. }));
}

#[tokio::test]
async fn hook_runner_updated_input() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::PreToolUse,
        command: "printf '{\"behavior\":\"allow\",\"updatedInput\":{\"command\":\"echo safe\"}}'"
            .to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    };
    let result = runner
        .run_hook(
            &config,
            "bash_tool",
            &serde_json::json!({"command": "dangerous_cmd"}),
        )
        .await
        .unwrap();
    assert!(matches!(result.decision, HookDecision::Allow));
    assert!(result.updated_input.is_some());
    let updated = result.updated_input.unwrap();
    assert_eq!(
        updated.get("command").and_then(serde_json::Value::as_str),
        Some("echo safe")
    );
}

#[tokio::test]
async fn hook_runner_prevent_continuation() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::PostToolUse,
        command:
            "echo '{\"behavior\":\"allow\",\"preventContinuation\":true,\"stopReason\":\"done\"}'"
                .to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    };
    let result = runner
        .run_hook(&config, "bash_tool", &serde_json::json!({}))
        .await
        .unwrap();
    assert!(result.prevent_continuation);
    assert_eq!(result.stop_reason.as_deref(), Some("done"));
}

#[tokio::test]
async fn hook_runner_tool_filter_skips_non_matching() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo '{\"behavior\":\"deny\"}'".to_string(),
        tool_filter: Some("write_file".to_string()),
        timeout_secs: Some(10),
    };
    let result = runner
        .run_hook(&config, "bash_tool", &serde_json::json!({}))
        .await
        .unwrap();
    assert!(matches!(result.decision, HookDecision::Allow));
}

#[tokio::test]
async fn hook_runner_timeout_returns_allow() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::PreToolUse,
        command: "sleep 10".to_string(),
        tool_filter: None,
        timeout_secs: Some(1),
    };
    let result = runner
        .run_hook(&config, "bash_tool", &serde_json::json!({}))
        .await
        .unwrap();
    assert!(matches!(result.decision, HookDecision::Allow));
}
