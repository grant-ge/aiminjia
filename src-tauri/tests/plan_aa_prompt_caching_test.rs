//! Plan-AA: Prompt Caching — architecture regression tests

use app_lib::llm::providers::claude::ClaudeProvider;
use app_lib::llm::providers::LlmProviderTrait;
use app_lib::llm::streaming::{ChatMessage, LlmRequest, ToolDefinition};
use serde_json::{json, Value};

fn test_provider() -> ClaudeProvider {
    ClaudeProvider::new("test-key".to_string(), None)
}

fn build_body(provider: &ClaudeProvider, request: &LlmRequest) -> Value {
    provider.build_request_body_for_test(request)
}

fn make_tool(name: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: format!("Tool {}", name),
        parameters: json!({"type": "object", "properties": {}}),
    }
}

#[test]
fn review_only_claude_supports_prompt_caching() {
    let claude = test_provider();
    assert!(
        claude.supports_prompt_caching(),
        "ClaudeProvider must return true for supports_prompt_caching()"
    );
}

#[test]
fn system_prompt_serialized_as_cache_control_block() {
    let provider = test_provider();
    let request = LlmRequest {
        messages: vec![
            ChatMessage::text("system", "You are a helpful assistant."),
            ChatMessage::text("user", "Hello"),
        ],
        tools: vec![],
        max_tokens: 1024,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    let system = &body["system"];
    assert!(
        system.is_array(),
        "system must be a content block array, got: {}",
        system
    );

    let blocks = system.as_array().unwrap();
    assert_eq!(blocks.len(), 1);

    let block = &blocks[0];
    assert_eq!(block["type"], "text");
    assert_eq!(block["text"], "You are a helpful assistant.");
    assert_eq!(block["cache_control"]["type"], "ephemeral");
}

#[test]
fn system_prompt_absent_when_no_system_message() {
    let provider = test_provider();
    let request = LlmRequest {
        messages: vec![ChatMessage::text("user", "Hello")],
        tools: vec![],
        max_tokens: 1024,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    assert!(body.get("system").is_none());
}

#[test]
fn last_tool_has_cache_control() {
    let provider = test_provider();
    let request = LlmRequest {
        messages: vec![ChatMessage::text("user", "go")],
        tools: vec![
            make_tool("tool_alpha"),
            make_tool("tool_beta"),
            make_tool("tool_gamma"),
        ],
        max_tokens: 1024,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    let tools = body["tools"].as_array().expect("tools must be an array");
    assert_eq!(tools.len(), 3);

    let last = &tools[2];
    assert_eq!(last["name"], "tool_gamma");
    assert_eq!(last["cache_control"]["type"], "ephemeral");

    for tool in &tools[..2] {
        assert!(tool.get("cache_control").is_none());
    }
}

#[test]
fn single_tool_has_cache_control() {
    let provider = test_provider();
    let request = LlmRequest {
        messages: vec![ChatMessage::text("user", "go")],
        tools: vec![make_tool("only_tool")],
        max_tokens: 1024,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["cache_control"]["type"], "ephemeral");
}

#[test]
fn no_tools_does_not_add_tools_key() {
    let provider = test_provider();
    let request = LlmRequest {
        messages: vec![ChatMessage::text("user", "go")],
        tools: vec![],
        max_tokens: 1024,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    assert!(body.get("tools").is_none());
}

fn count_cache_breakpoints(value: &Value) -> usize {
    match value {
        Value::Object(map) => {
            let self_count = usize::from(map.contains_key("cache_control"));
            let child_count: usize = map
                .iter()
                .filter(|(key, _)| key.as_str() != "cache_control")
                .map(|(_, value)| count_cache_breakpoints(value))
                .sum();
            self_count + child_count
        }
        Value::Array(arr) => arr.iter().map(count_cache_breakpoints).sum(),
        _ => 0,
    }
}

#[test]
fn cache_breakpoints_do_not_exceed_api_limit() {
    let provider = test_provider();
    let tools: Vec<ToolDefinition> = (0..20)
        .map(|i| make_tool(&format!("tool_{:02}", i)))
        .collect();

    let request = LlmRequest {
        messages: vec![
            ChatMessage::text("system", "You are an agent with many tools."),
            ChatMessage::text("user", "Use whatever tools you need."),
        ],
        tools,
        max_tokens: 4096,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    let breakpoint_count = count_cache_breakpoints(&body);
    assert!(breakpoint_count <= 4, "breakpoint count = {}", breakpoint_count);
}

#[test]
fn cache_breakpoints_count_system_plus_last_tool_equals_two() {
    let provider = test_provider();
    let request = LlmRequest {
        messages: vec![
            ChatMessage::text("system", "System prompt here."),
            ChatMessage::text("user", "Hello"),
        ],
        tools: vec![make_tool("alpha"), make_tool("beta")],
        max_tokens: 1024,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    assert_eq!(count_cache_breakpoints(&body), 2);
}

#[test]
fn cache_breakpoints_count_system_only_when_no_tools() {
    let provider = test_provider();
    let request = LlmRequest {
        messages: vec![
            ChatMessage::text("system", "System prompt."),
            ChatMessage::text("user", "Hi"),
        ],
        tools: vec![],
        max_tokens: 1024,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    assert_eq!(count_cache_breakpoints(&body), 1);
}

#[test]
fn cache_breakpoints_zero_when_no_system_no_tools() {
    let provider = test_provider();
    let request = LlmRequest {
        messages: vec![ChatMessage::text("user", "hello")],
        tools: vec![],
        max_tokens: 512,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    assert_eq!(count_cache_breakpoints(&body), 0);
}
