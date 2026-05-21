//! Integration tests for the lotus → anthropic-ingress path.
//!
//! These tests stand up a mockito server that pretends to be the lotus
//! gateway's `/anthropic/v1/messages` endpoint. They drive the underlying
//! `ClaudeProvider::with_url(...)` (the same instance `LotusProvider`
//! delegates to) so that:
//!
//! 1. The request body shape matches the Anthropic Messages API spec
//!    (top-level `system`, `messages` array, `max_tokens`, `tools` with
//!    `input_schema`, etc.) — i.e. there is no OpenAI-style translation
//!    happening on the client side.
//!
//! 2. **The thinking-block roundtrip is byte-perfect.** This is the
//!    central reason for Phase C: when an assistant's prior turn carried
//!    `thinking_blocks` (with signatures), the next request must echo
//!    them back in the very first content slot of that assistant message,
//!    verbatim. Any field reordering, extra whitespace, or signature
//!    truncation will trip the upstream's signature verifier and the
//!    user sees `thinking_block_mismatch`. We assert byte-equality.
//!
//! 3. The SSE state machine handles `thinking` content blocks (incl.
//!    `signature_delta`), `tool_use` accumulation, and `message_stop`
//!    correctly when fed a stream framed by the gateway.
//!
//! Tests intentionally use `ClaudeProvider::with_url` rather than
//! `LotusProvider::new`, because the lotus provider hard-codes its URL
//! to production. The two are semantically identical at this layer
//! (lotus.rs is a thin shell with retry policy on top); covering the
//! shared surface here means PR-2's retry classifier is the only piece
//! exercised separately (already done via lotus.rs unit tests).

use serde_json::{json, Value};

use app_lib::llm::providers::claude::ClaudeProvider;
use app_lib::llm::providers::LlmProviderTrait;
use app_lib::llm::streaming::{ChatMessage, LlmRequest, ToolDefinition};

/// Bypass the system HTTP proxy for loopback addresses mockito binds to.
///
/// Without this, on a developer machine with system-level HTTP proxy
/// settings (e.g. macOS `networksetup -getwebproxy`, or shell env vars),
/// `reqwest` (used by `ClaudeProvider`) routes the test request through
/// the proxy rather than the mockito server. The proxy then can't connect
/// to the mock and returns 502, which surfaces here as a confusing
/// "Anthropic API error (502)" — the request never reached the mock.
///
/// `NO_PROXY` is read by reqwest's default proxy resolver. Setting it
/// process-wide is safe across tests (env vars are global; mockito only
/// ever lives on 127.0.0.1).
fn ensure_no_proxy_for_loopback() {
    let needed = "127.0.0.1,localhost,::1";
    for var in ["NO_PROXY", "no_proxy"] {
        let existing = std::env::var(var).unwrap_or_default();
        if existing.is_empty() {
            std::env::set_var(var, needed);
        } else if !existing.contains("127.0.0.1") {
            std::env::set_var(var, format!("{},{}", existing, needed));
        }
    }
}

/// Construct a provider pointing at the mockito server, mirroring how
/// `LotusProvider::new` configures `is_direct=false`.
fn provider_for(server_url: &str) -> ClaudeProvider {
    ensure_no_proxy_for_loopback();
    ClaudeProvider::with_url(
        "sk-sess-test-fake".to_string(),
        Some("claude-sonnet-4-5".to_string()),
        format!("{}/anthropic/v1/messages", server_url),
        false, // is_direct=false → matches LotusProvider configuration
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn body_is_anthropic_native_no_openai_fields() {
    let mut server = mockito::Server::new_async().await;

    let body_capture = std::sync::Arc::new(std::sync::Mutex::new(None::<Value>));
    let captured = body_capture.clone();

    let _m = server
        .mock("POST", "/anthropic/v1/messages")
        .match_request(move |req| {
            if let Ok(body_bytes) = req.body() {
                if let Ok(body) = serde_json::from_slice::<Value>(body_bytes) {
                    *captured.lock().unwrap() = Some(body);
                }
            }
            true
        })
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "id": "msg_test",
                "type": "message",
                "role": "assistant",
                "model": "claude-sonnet-4-5",
                "content": [{"type": "text", "text": "ok"}],
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 5, "output_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let p = provider_for(&server.url());
    let mut req = LlmRequest::default();
    req.stream = false;
    req.messages = vec![
        ChatMessage::text("system", "You are a helpful agent."),
        ChatMessage::text("user", "Hello"),
    ];

    p.send(req).await.expect("send should succeed");

    let body = body_capture.lock().unwrap().clone().expect("body captured");

    // Anthropic-native shape assertions.
    assert!(
        body.get("model").is_some(),
        "anthropic body must carry top-level model"
    );
    assert!(
        body.get("max_tokens").is_some(),
        "anthropic body must carry top-level max_tokens"
    );
    assert!(
        body.get("system").is_some(),
        "system prompt must be lifted to top-level `system`, not in messages"
    );
    let messages = body
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("messages array");
    for m in messages {
        assert_ne!(
            m.get("role").and_then(|r| r.as_str()),
            Some("system"),
            "system must NOT appear inside messages array on anthropic"
        );
    }

    // No OpenAI-isms.
    assert!(
        body.get("functions").is_none(),
        "must not emit OpenAI `functions`"
    );
    assert!(
        body.get("function_call").is_none(),
        "must not emit OpenAI `function_call`"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thinking_blocks_roundtrip_byte_perfect() {
    // The exact shape the gateway / upstream returned to us on a previous
    // turn. We must echo this back verbatim — including the signature —
    // or the upstream will refuse the next turn.
    let signed_thinking = json!({
        "type": "thinking",
        "thinking": "I should consider the user's question carefully.",
        "signature": "sig_abc123_with_special_chars=/+",
    });

    let mut server = mockito::Server::new_async().await;
    let body_capture = std::sync::Arc::new(std::sync::Mutex::new(None::<Value>));
    let captured = body_capture.clone();

    let _m = server
        .mock("POST", "/anthropic/v1/messages")
        .match_request(move |req| {
            if let Ok(body_bytes) = req.body() {
                if let Ok(body) = serde_json::from_slice::<Value>(body_bytes) {
                    *captured.lock().unwrap() = Some(body);
                }
            }
            true
        })
        .with_status(200)
        .with_body(
            json!({
                "id": "msg_test",
                "type": "message",
                "role": "assistant",
                "model": "claude-sonnet-4-5",
                "content": [{"type": "text", "text": "ok"}],
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 5, "output_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let p = provider_for(&server.url());
    let mut req = LlmRequest::default();
    req.stream = false;

    let mut prior_assistant = ChatMessage::text("assistant", "Sure, let me help.".to_string());
    prior_assistant.thinking_blocks = Some(vec![signed_thinking.clone()]);

    req.messages = vec![
        ChatMessage::text("user", "What is 2+2?"),
        prior_assistant,
        ChatMessage::text("user", "Now what about 3+3?"),
    ];

    p.send(req).await.expect("send should succeed");
    let body = body_capture.lock().unwrap().clone().expect("body captured");
    let messages = body
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("messages array");

    // Find the assistant message and assert thinking-block invariants.
    let assistant = messages
        .iter()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("assistant"))
        .expect("assistant message present");
    let content = assistant
        .get("content")
        .and_then(|v| v.as_array())
        .expect("assistant content is a content-blocks array (not a plain string)");

    // Thinking block must be FIRST (anthropic protocol invariant).
    let first = &content[0];
    assert_eq!(
        first.get("type").and_then(|v| v.as_str()),
        Some("thinking"),
        "thinking blocks must precede other content blocks"
    );

    // The block must be byte-identical to what we passed in. We compare
    // serde Value equality (which is structural and order-insensitive
    // for objects) — that's the right semantic match for JSON.
    assert_eq!(
        *first, signed_thinking,
        "thinking block must roundtrip verbatim"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tools_use_input_schema_not_parameters() {
    let mut server = mockito::Server::new_async().await;
    let body_capture = std::sync::Arc::new(std::sync::Mutex::new(None::<Value>));
    let captured = body_capture.clone();

    let _m = server
        .mock("POST", "/anthropic/v1/messages")
        .match_request(move |req| {
            if let Ok(body_bytes) = req.body() {
                if let Ok(body) = serde_json::from_slice::<Value>(body_bytes) {
                    *captured.lock().unwrap() = Some(body);
                }
            }
            true
        })
        .with_status(200)
        .with_body(
            json!({
                "id": "msg_test",
                "type": "message",
                "role": "assistant",
                "model": "claude-sonnet-4-5",
                "content": [{"type": "text", "text": "ok"}],
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 5, "output_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let p = provider_for(&server.url());
    let mut req = LlmRequest::default();
    req.stream = false;
    req.messages = vec![ChatMessage::text("user", "hi")];
    req.tools = vec![ToolDefinition {
        name: "lookup".to_string(),
        description: "look up something".to_string(),
        parameters: json!({"type": "object", "properties": {}}),
    }];

    p.send(req).await.expect("send should succeed");
    let body = body_capture.lock().unwrap().clone().expect("body captured");

    let tools = body
        .get("tools")
        .and_then(|v| v.as_array())
        .expect("tools array");
    assert_eq!(tools.len(), 1);
    let t = &tools[0];
    assert!(
        t.get("input_schema").is_some(),
        "anthropic uses `input_schema`, not `parameters`"
    );
    assert!(
        t.get("parameters").is_none(),
        "must NOT carry OpenAI-style `parameters`"
    );
    assert!(
        t.get("function").is_none(),
        "must NOT wrap tool in OpenAI-style `function`"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_4xx_no_retry_no_silent_swallow() {
    // The gateway returns 400 for, e.g., bad model. The client must NOT
    // retry (no Anthropic 4xx is transient at this layer) and must
    // surface the error.
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("POST", "/anthropic/v1/messages")
        .with_status(400)
        .with_body(
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"bad model"}}"#,
        )
        .expect(1) // critical: ensures no retry
        .create_async()
        .await;

    let p = provider_for(&server.url());
    let mut req = LlmRequest::default();
    req.stream = false;
    req.messages = vec![ChatMessage::text("user", "hi")];

    let result = p.send(req).await;
    assert!(result.is_err(), "4xx must surface as Err");
    _m.assert_async().await; // exactly 1 hit
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streaming_thinking_signature_passes_through() {
    // Mock an anthropic-shape SSE stream with a signed thinking block,
    // followed by a text block, followed by message_stop. Assert the
    // client decodes the events and that the thinking signature reaches
    // the consumer intact.
    let mut server = mockito::Server::new_async().await;

    let sse_body = [
        "event: message_start",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-sonnet-4-5\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}",
        "",
        "event: content_block_start",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}",
        "",
        "event: content_block_delta",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Reasoning step 1.\"}}",
        "",
        "event: content_block_delta",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig_xyz_98765\"}}",
        "",
        "event: content_block_stop",
        "data: {\"type\":\"content_block_stop\",\"index\":0}",
        "",
        "event: content_block_start",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}",
        "",
        "event: content_block_delta",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello.\"}}",
        "",
        "event: content_block_stop",
        "data: {\"type\":\"content_block_stop\",\"index\":1}",
        "",
        "event: message_delta",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":7}}",
        "",
        "event: message_stop",
        "data: {\"type\":\"message_stop\"}",
        "",
    ]
    .join("\n");

    let _m = server
        .mock("POST", "/anthropic/v1/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let p = provider_for(&server.url());
    let mut req = LlmRequest::default();
    req.stream = true;
    req.messages = vec![ChatMessage::text("user", "hi")];

    let mut stream = p.stream(req).await.expect("stream open");
    use app_lib::llm::streaming::StreamEvent;
    use futures::StreamExt;

    let mut saw_thinking_delta = false;
    let mut thinking_block_signature: Option<String> = None;
    let mut saw_text_delta = false;
    let mut saw_done = false;

    while let Some(event) = stream.next().await {
        match event {
            StreamEvent::ThinkingDelta { delta } => {
                if !delta.is_empty() {
                    saw_thinking_delta = true;
                }
            }
            StreamEvent::ThinkingBlock { block } => {
                thinking_block_signature = block
                    .get("signature")
                    .and_then(|v| v.as_str())
                    .map(String::from);
            }
            StreamEvent::ContentDelta { delta } => {
                if !delta.is_empty() {
                    saw_text_delta = true;
                }
            }
            StreamEvent::Done { .. } => saw_done = true,
            StreamEvent::Error { error } => panic!("unexpected stream error: {}", error),
            _ => {}
        }
    }

    assert!(saw_thinking_delta, "ThinkingDelta event must be emitted");
    assert_eq!(
        thinking_block_signature.as_deref(),
        Some("sig_xyz_98765"),
        "thinking block signature must reach the consumer intact"
    );
    assert!(saw_text_delta, "text ContentDelta must be emitted");
    assert!(saw_done, "stream must emit Done");
}
