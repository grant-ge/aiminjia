use app_lib::llm::context_decay::{
    context_window_for_provider, estimate_context_tokens, estimate_tokens,
    estimate_tokens_from_json, CONTEXT_OVERFLOW_THRESHOLD,
};
use app_lib::llm::gateway::thinking_config_for_route;
use app_lib::llm::providers::claude::ClaudeProvider;
use app_lib::llm::router::RouteResult;
use app_lib::llm::streaming::{ChatMessage, LlmRequest, ThinkingConfig, ToolCall};
use app_lib::models::settings::AppSettings;
use app_lib::runtime::chat::{LlmStepInput, ResolvedLlmSettings};
use serde_json::json;

fn claude_provider() -> ClaudeProvider {
    ClaudeProvider::new("test-key".to_string(), None)
}

#[test]
fn ad1_estimate_tokens_counts_text_and_tool_calls() {
    let messages = vec![
        ChatMessage::text("user", "1234"),
        ChatMessage::assistant_with_tool_calls(
            "abcd".to_string(),
            vec![ToolCall {
                id: "call_1".to_string(),
                name: "grep".to_string(),
                arguments: json!({"pattern": "needle", "limit": 3}),
            }],
        ),
    ];

    let estimated = estimate_tokens(&messages);
    assert!(
        estimated > 2,
        "tool_calls json should be included, got {}",
        estimated
    );
}

#[test]
fn ad1_estimate_context_tokens_includes_system_prompt() {
    let messages = vec![ChatMessage::text("user", "1234")];
    let with_system = estimate_context_tokens("system text", &messages);
    let without_system = estimate_tokens(&messages);
    assert!(with_system > without_system);
}

#[test]
fn ad1_estimate_tokens_from_json_uses_serialized_size() {
    let messages = vec![
        json!({"role": "user", "content": "1234"}),
        json!({"role": "assistant", "content": "abcd", "tool_calls": [{"id": "1", "name": "calc", "arguments": {"x": 1}}]}),
    ];
    let estimated = estimate_tokens_from_json(&messages);
    assert!(estimated > 2);
}

#[test]
fn ad2_context_window_for_provider_matches_expected_defaults() {
    assert_eq!(context_window_for_provider("claude"), 200_000);
    assert_eq!(context_window_for_provider("deepseek-v3"), 128_000);
    assert_eq!(context_window_for_provider("deepseek-r1"), 128_000);
    assert_eq!(context_window_for_provider("openai"), 100_000);
    assert!((CONTEXT_OVERFLOW_THRESHOLD - 0.8).abs() < f64::EPSILON);
}

#[test]
fn ad4_gateway_only_enables_thinking_for_claude_route() {
    let settings = AppSettings {
        thinking_type: "enabled".to_string(),
        thinking_budget_tokens: 4096,
        ..AppSettings::default()
    };
    let claude_route = RouteResult {
        provider: "claude".to_string(),
        api_key: "k".to_string(),
        model_hint: String::new(),
        use_tools: true,
        endpoint_url: String::new(),
        model_type: String::new(),
    };
    let deepseek_route = RouteResult {
        provider: "deepseek-v3".to_string(),
        ..claude_route.clone()
    };

    assert!(matches!(
        thinking_config_for_route(&claude_route, &settings),
        Some(ThinkingConfig::Enabled {
            budget_tokens: 4096
        })
    ));
    assert!(thinking_config_for_route(&deepseek_route, &settings).is_none());
}

#[test]
fn ad5_thinking_headers_only_added_when_thinking_is_active() {
    let provider = claude_provider();
    let disabled = LlmRequest {
        messages: vec![ChatMessage::text("user", "Hello")],
        max_tokens: 1024,
        temperature: 0.3,
        stream: false,
        thinking_config: Some(ThinkingConfig::Disabled),
        ..LlmRequest::default()
    };
    let enabled = LlmRequest {
        thinking_config: Some(ThinkingConfig::Enabled {
            budget_tokens: 1024,
        }),
        ..disabled.clone()
    };

    let disabled_headers = provider.build_request_headers_for_test(&disabled);
    assert!(disabled_headers.get("anthropic-beta").is_none());

    let enabled_headers = provider.build_request_headers_for_test(&enabled);
    assert_eq!(
        enabled_headers.get("anthropic-beta").unwrap(),
        "interleaved-thinking-2025-05-14"
    );
}

#[test]
fn ad4_thinking_config_serializes_adaptive_and_enabled() {
    let adaptive = serde_json::to_value(&ThinkingConfig::Adaptive).unwrap();
    assert_eq!(adaptive["type"], "adaptive");

    let enabled = serde_json::to_value(&ThinkingConfig::Enabled {
        budget_tokens: 2048,
    })
    .unwrap();
    assert_eq!(enabled["type"], "enabled");
    assert_eq!(enabled["budget_tokens"], 2048);
}

#[test]
fn ad4_llm_request_default_has_no_thinking() {
    let request = LlmRequest::default();
    assert!(request.thinking_config.is_none());
}

#[test]
fn ad4_app_settings_default_disables_thinking() {
    let settings = AppSettings::default();
    assert_eq!(settings.thinking_type, "disabled");
    assert_eq!(settings.thinking_budget_tokens, 8000);
}

#[test]
fn ad4_resolved_llm_settings_default_disables_thinking() {
    let settings = ResolvedLlmSettings::default();
    assert_eq!(settings.thinking_type, "disabled");
    assert_eq!(settings.thinking_budget_tokens, 8000);
}

#[test]
fn ad5_build_request_body_with_thinking_enabled_clamps_budget_and_drops_temperature() {
    let provider = claude_provider();
    let request = LlmRequest {
        messages: vec![ChatMessage::text("user", "Hello")],
        max_tokens: 1024,
        temperature: 0.3,
        stream: false,
        thinking_config: Some(ThinkingConfig::Enabled {
            budget_tokens: 4096,
        }),
        ..LlmRequest::default()
    };

    let body = provider.build_request_body_for_test(&request);
    assert_eq!(body["thinking"]["type"], "enabled");
    assert_eq!(body["thinking"]["budget_tokens"], 1023);
    assert!(body.get("temperature").is_none());
}

#[test]
fn ad5_build_request_body_with_thinking_adaptive() {
    let provider = claude_provider();
    let request = LlmRequest {
        messages: vec![ChatMessage::text("user", "Hello")],
        max_tokens: 1024,
        temperature: 0.3,
        stream: false,
        thinking_config: Some(ThinkingConfig::Adaptive),
        ..LlmRequest::default()
    };

    let body = provider.build_request_body_for_test(&request);
    assert_eq!(body["thinking"]["type"], "adaptive");
    assert!(body.get("temperature").is_none());
}

#[test]
fn ad5_build_request_body_with_thinking_disabled_omits_thinking_and_keeps_temperature() {
    let provider = claude_provider();
    let request = LlmRequest {
        messages: vec![ChatMessage::text("user", "Hello")],
        max_tokens: 1024,
        temperature: 0.3,
        stream: false,
        thinking_config: Some(ThinkingConfig::Disabled),
        ..LlmRequest::default()
    };

    let body = provider.build_request_body_for_test(&request);
    assert!(body.get("thinking").is_none());
    let temp = body["temperature"].as_f64().unwrap();
    assert!((temp - 0.3).abs() < 0.001);
}

#[test]
fn ad3_llm_step_input_exposes_estimated_tokens() {
    let llm_settings = ResolvedLlmSettings::default();
    let input = LlmStepInput {
        system_prompt: "sys",
        openai_system_message: None,
        dynamic_context: "",
        messages: vec![],
        tool_defs: &[],
        token_budget: 4096,
        chunk_timeout_secs: 30,
        masking_level: "strict",
        force_no_tools: false,
        llm_settings: &llm_settings,
        conversation_id: "conv",
        run_id: "run",
        estimated_tokens: 123,
    };

    assert_eq!(input.estimated_tokens, 123);
}
