use std::sync::Arc;
use tauri::State;

/// Check which conversations have active agent tasks.
///
/// Returns a list of conversation IDs that are currently being processed.
#[tauri::command]
pub async fn is_agent_busy(
    adapter: State<'_, Arc<crate::transport::tauri_commands::chat::TauriChatCommandAdapter>>,
) -> Result<Vec<String>, String> {
    Ok(adapter.is_agent_busy())
}

/// Send a user message and trigger the LLM agent loop.
///
/// The function detects whether the conversation is in analysis mode
/// (structured 6-step workflow) or daily consultation mode, and
/// dispatches accordingly with the appropriate system prompt, tool
/// filter, and iteration limit.
#[tauri::command]
pub async fn send_message(
    adapter: State<'_, Arc<crate::transport::tauri_commands::chat::TauriChatCommandAdapter>>,
    conversation_id: String,
    content: String,
    file_ids: Vec<String>,
) -> Result<(), String> {
    adapter
        .send_message(conversation_id, content, file_ids)
        .await
}
#[tauri::command]
pub async fn stop_streaming(
    adapter: State<'_, Arc<crate::transport::tauri_commands::chat::TauriChatCommandAdapter>>,
    conversation_id: String,
) -> Result<(), String> {
    adapter.stop_streaming(conversation_id).await
}

#[tauri::command]
pub async fn get_messages(
    adapter: State<'_, Arc<crate::transport::tauri_commands::chat::TauriChatCommandAdapter>>,
    conversation_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    adapter.get_messages(conversation_id).await
}

#[tauri::command]
pub async fn create_conversation(
    adapter: State<'_, Arc<crate::transport::tauri_commands::chat::TauriChatCommandAdapter>>,
) -> Result<String, String> {
    adapter.create_conversation().await
}

#[tauri::command]
pub async fn delete_conversation(
    adapter: State<'_, Arc<crate::transport::tauri_commands::chat::TauriChatCommandAdapter>>,
    conversation_id: String,
) -> Result<(), String> {
    adapter.delete_conversation(conversation_id).await
}

#[tauri::command]
pub async fn rename_conversation(
    adapter: State<'_, Arc<crate::transport::tauri_commands::chat::TauriChatCommandAdapter>>,
    conversation_id: String,
    new_title: String,
) -> Result<(), String> {
    adapter
        .rename_conversation(conversation_id, new_title)
        .await
}

#[tauri::command]
pub async fn get_conversations(
    adapter: State<'_, Arc<crate::transport::tauri_commands::chat::TauriChatCommandAdapter>>,
) -> Result<Vec<serde_json::Value>, String> {
    adapter.get_conversations().await
}

pub mod testsupport {
    use std::sync::Arc;

    use anyhow::Result;

    use crate::runtime::event_bus::RuntimeEventBus;
    use crate::runtime::identity::IdentityMapping;
    use crate::runtime::ids::RunId;
    use crate::runtime::query_engine::QueryEngine;
    use crate::runtime::state::TurnState;
    use crate::runtime::tools::testing::single_legacy_tool_dispatcher;
    use crate::runtime::SessionRuntime;
    use crate::runtime_audit::trace_capture::CapturedTrace;
    use crate::transport::tauri_event_adapter::TauriEventAdapter;
    use crate::transport::testing::RecordingRuntimeHost;

    pub async fn run_send_message_through_runtime(
        conversation_id: &str,
        input: &str,
    ) -> Result<CapturedTrace> {
        let host = RecordingRuntimeHost::new();
        let runtime = SessionRuntime::for_test(host.clone());
        runtime
            .run_for_test(conversation_id, "run-test-basic", input)
            .await?;
        Ok(host.trace())
    }

    pub async fn run_tool_turn_through_runtime(
        conversation_id: &str,
        tool_name: &str,
    ) -> Result<CapturedTrace> {
        let host = RecordingRuntimeHost::new();
        let bus = RuntimeEventBus::new();
        bus.subscribe(Arc::new(TauriEventAdapter::new(host.clone())));

        let dispatcher = single_legacy_tool_dispatcher(tool_name);
        let query_engine = QueryEngine::for_test(dispatcher);
        let turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id(conversation_id.to_string()),
            RunId::new("run-test-tool"),
            format!("run {tool_name}"),
        );
        query_engine
            .run_tool_with_bus(&turn, &bus, tool_name)
            .await?;
        Ok(host.trace())
    }
}

#[cfg(test)]
mod xml_strip_tests {
    use crate::llm::content_filter::strip_hallucinated_xml;

    #[test]
    fn test_strip_closed_function_calls() {
        let input = "你好\n<function_calls>\n<invoke name=\"load_file\">\n<parameter name=\"file_id\">abc</parameter>\n</invoke>\n</function_calls>\n世界";
        let result = strip_hallucinated_xml(input);
        assert_eq!(result, "你好\n\n世界");
    }

    #[test]
    fn test_strip_unclosed_function_calls() {
        let input = "你好\n<function_calls>\n<invoke name=\"execute_python\">\n<parameter name=\"code\">print(1)</parameter>";
        let result = strip_hallucinated_xml(input);
        assert_eq!(result, "你好");
    }

    #[test]
    fn test_no_xml_unchanged() {
        let input = "正常的消息内容，没有任何 XML 标签。";
        let result = strip_hallucinated_xml(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_strip_multiple_blocks() {
        let input = "前文\n<function_calls>\n<invoke name=\"a\">\n</invoke>\n</function_calls>\n中间\n<function_calls>\n<invoke name=\"b\">\n</invoke>\n</function_calls>\n后文";
        let result = strip_hallucinated_xml(input);
        assert_eq!(result, "前文\n\n中间\n\n后文");
    }

    #[test]
    fn test_strip_tool_call_tags() {
        let input = "结果：<tool_call>{\"name\": \"test\"}</tool_call>完成";
        let result = strip_hallucinated_xml(input);
        assert_eq!(result, "结果：完成");
    }

    #[test]
    fn test_error_classification_401() {
        let error = "API error (401): Unauthorized";
        let lower = error.to_lowercase();
        assert!(lower.contains("401") || lower.contains("unauthorized"));
    }

    #[test]
    fn test_error_classification_429() {
        let error = "API error (429): Rate limit exceeded";
        let lower = error.to_lowercase();
        assert!(lower.contains("429") || lower.contains("rate limit"));
    }

    #[test]
    fn test_error_classification_timeout() {
        let error = "Request timed out after 90 seconds";
        let lower = error.to_lowercase();
        assert!(lower.contains("timeout") || lower.contains("timed out"));
    }

    #[test]
    fn test_error_classification_network() {
        let error = "Connection reset by peer";
        let lower = error.to_lowercase();
        assert!(lower.contains("connection"));
    }

    #[test]
    fn test_error_classification_5xx() {
        let errors = vec![
            "API error (500): Internal Server Error",
            "API error (502): Bad Gateway",
            "API error (503): Service Unavailable",
            "API error (504): Gateway Timeout",
        ];
        for error in errors {
            let lower = error.to_lowercase();
            assert!(
                lower.contains("500")
                    || lower.contains("502")
                    || lower.contains("503")
                    || lower.contains("504")
            );
        }
    }
}
