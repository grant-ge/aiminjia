//! Smart model router — selects optimal model based on task type and settings.
//!
//! The router inspects the latest user message to infer a [`TaskType`], then
//! consults [`AppSettings`] to decide which provider and API key to use.
//!
//! Each provider has known model capabilities. When `auto_model_routing` is
//! enabled, the router automatically selects the reasoning variant (e.g.
//! DeepSeek-R1) for reasoning tasks using the same API key.
//!
//! **Important**: Analysis tasks always use the primary model with tools
#![allow(dead_code)]
//! enabled, because the 5-step analysis workflow requires tool calls.

use crate::llm::streaming::ChatMessage;
use crate::models::settings::AppSettings;

/// Known model capabilities for a provider.
///
/// The system uses this to auto-select the best model for each task type
/// without requiring separate configuration per model.
#[derive(Debug, Clone)]
pub struct ProviderCapabilities {
    /// Provider ID used for primary tasks (dispatch_stream key)
    pub primary_provider: &'static str,
    /// Provider ID for reasoning tasks (same API key), None if no reasoning variant
    pub reasoning_provider: Option<&'static str>,
    /// Human-readable description of available models (for UI display)
    pub models_desc: &'static str,
}

/// Get the known model capabilities for a provider.
///
/// This is the central registry of what models each provider offers.
/// The same API key works for both primary and reasoning models within
/// a single provider.
pub fn get_provider_capabilities(provider: &str) -> ProviderCapabilities {
    match provider {
        "deepseek-v3" => ProviderCapabilities {
            primary_provider: "deepseek-v3",
            reasoning_provider: Some("deepseek-r1"),
            models_desc: "主力: deepseek-chat | 推理: deepseek-reasoner",
        },
        "qwen-plus" => ProviderCapabilities {
            primary_provider: "qwen-plus",
            reasoning_provider: None,
            models_desc: "主力: qwen-plus",
        },
        "openai" => ProviderCapabilities {
            primary_provider: "openai",
            reasoning_provider: None, // TODO: add o1 support
            models_desc: "主力: GPT-4o",
        },
        "claude" => ProviderCapabilities {
            primary_provider: "claude",
            reasoning_provider: None,
            models_desc: "主力: Claude Sonnet",
        },
        "volcano" => ProviderCapabilities {
            primary_provider: "volcano",
            reasoning_provider: None,
            models_desc: "主力: 字节跳动大模型",
        },
        _ => ProviderCapabilities {
            primary_provider: "deepseek-v3",
            reasoning_provider: None,
            models_desc: "",
        },
    }
}

/// Task categories that influence model selection.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskType {
    /// General conversation / Q&A
    General,
    /// Deep analysis requiring reasoning (compensation fairness, statistics)
    /// NOTE: Always routed to primary model WITH tools enabled.
    Analysis,
    /// Code generation (Python scripts for data processing)
    CodeGen,
    /// Web search synthesis
    Search,
    /// Pure reasoning task (explicitly requested, no tools needed)
    Reasoning,
}

/// Result of routing: which provider + model to use.
#[derive(Debug, Clone)]
pub struct RouteResult {
    /// Provider identifier, e.g. "deepseek-v3", "openai", "claude", "volcano"
    pub provider: String,
    /// API key for the selected provider
    pub api_key: String,
    /// Specific model ID hint (used by providers like Volcano that need it)
    pub model_hint: String,
    /// Whether this route supports tool use
    pub use_tools: bool,
}

/// Infer the task type from the conversation messages.
///
/// Looks at the latest user message for domain-specific keywords in both
/// Chinese and English. Returns [`TaskType::General`] if no keywords match
/// or if there are no user messages.
pub fn infer_task_type(messages: &[ChatMessage]) -> TaskType {
    // Get the last user message
    let last_user = messages.iter().rev().find(|m| m.role == "user");
    let text = match last_user {
        Some(msg) => msg.content.to_lowercase(),
        None => return TaskType::General,
    };

    // Analysis keywords (Chinese + English)
    let analysis_keywords = [
        "分析", "诊断", "公平性", "薪酬", "对比", "回归", "标准差",
        "analyze", "diagnosis", "fairness", "regression", "statistics",
        "correlation", "deviation", "相关性", "偏差", "显著性",
    ];
    if analysis_keywords.iter().any(|kw| text.contains(kw)) {
        return TaskType::Analysis;
    }

    // Code generation keywords
    let code_keywords = [
        "代码", "脚本", "python", "计算", "code", "script", "compute",
        "函数", "function", "算法", "algorithm",
    ];
    if code_keywords.iter().any(|kw| text.contains(kw)) {
        return TaskType::CodeGen;
    }

    // Search keywords
    let search_keywords = [
        "搜索", "查找", "市场数据", "search", "lookup", "benchmark",
        "行业数据", "薪酬报告", "market data", "salary survey",
    ];
    if search_keywords.iter().any(|kw| text.contains(kw)) {
        return TaskType::Search;
    }

    TaskType::General
}

/// Select the route based on task type and app settings.
///
/// Routing logic:
/// - If `auto_model_routing` is disabled, always use the primary model.
/// - **Analysis tasks always use the primary model with tools enabled**,
///   because the 5-step analysis workflow requires tool calls.
/// - Only `Reasoning` tasks use the reasoning variant (if available).
/// - All other task types use the primary model with tools.
///
/// The reasoning model is auto-determined from provider capabilities.
/// No separate configuration is needed — the same API key is used.
pub fn select_route(task_type: &TaskType, settings: &AppSettings) -> RouteResult {
    let caps = get_provider_capabilities(&settings.primary_model);

    // If auto routing is disabled, always use primary model
    if !settings.auto_model_routing {
        return RouteResult {
            provider: settings.primary_model.clone(),
            api_key: settings.primary_api_key.clone(),
            model_hint: String::new(),
            use_tools: true,
        };
    }

    match task_type {
        // Analysis ALWAYS uses primary model with tools — this is critical
        // for the 5-step workflow that relies on execute_python, analyze_file, etc.
        TaskType::Analysis => RouteResult {
            provider: settings.primary_model.clone(),
            api_key: settings.primary_api_key.clone(),
            model_hint: String::new(),
            use_tools: true,
        },
        // Reasoning tasks use the reasoning variant if available (same API key)
        TaskType::Reasoning => {
            if let Some(reasoning) = caps.reasoning_provider {
                RouteResult {
                    provider: reasoning.to_string(),
                    api_key: settings.primary_api_key.clone(),
                    model_hint: String::new(),
                    use_tools: false,
                }
            } else {
                // No reasoning variant — use primary model
                RouteResult {
                    provider: settings.primary_model.clone(),
                    api_key: settings.primary_api_key.clone(),
                    model_hint: String::new(),
                    use_tools: true,
                }
            }
        }
        // All other tasks use primary model
        _ => RouteResult {
            provider: settings.primary_model.clone(),
            api_key: settings.primary_api_key.clone(),
            model_hint: String::new(),
            use_tools: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::settings::AppSettings;

    fn make_messages(texts: &[(&str, &str)]) -> Vec<ChatMessage> {
        texts
            .iter()
            .map(|(role, content)| ChatMessage::text(role, *content))
            .collect()
    }

    fn default_settings() -> AppSettings {
        AppSettings {
            auto_model_routing: true,
            primary_model: "deepseek-v3".to_string(),
            primary_api_key: "pk-test".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_infer_general() {
        let msgs = make_messages(&[("user", "Hello, how are you?")]);
        assert_eq!(infer_task_type(&msgs), TaskType::General);
    }

    #[test]
    fn test_infer_analysis_english() {
        let msgs = make_messages(&[("user", "Please analyze the salary regression data")]);
        assert_eq!(infer_task_type(&msgs), TaskType::Analysis);
    }

    #[test]
    fn test_infer_analysis_chinese() {
        let msgs = make_messages(&[("user", "请对薪酬公平性进行诊断")]);
        assert_eq!(infer_task_type(&msgs), TaskType::Analysis);
    }

    #[test]
    fn test_infer_codegen() {
        let msgs = make_messages(&[("user", "Write a Python script to compute averages")]);
        assert_eq!(infer_task_type(&msgs), TaskType::CodeGen);
    }

    #[test]
    fn test_infer_search() {
        let msgs = make_messages(&[("user", "Search for market data on software engineer salaries")]);
        assert_eq!(infer_task_type(&msgs), TaskType::Search);
    }

    #[test]
    fn test_infer_empty_messages() {
        let msgs: Vec<ChatMessage> = vec![];
        assert_eq!(infer_task_type(&msgs), TaskType::General);
    }

    #[test]
    fn test_infer_uses_last_user_message() {
        let msgs = make_messages(&[
            ("user", "Please analyze the data"),
            ("assistant", "Sure, I'll analyze it."),
            ("user", "Hello, how are you?"),
        ]);
        // Last user message is general, not analysis
        assert_eq!(infer_task_type(&msgs), TaskType::General);
    }

    #[test]
    fn test_route_auto_disabled() {
        let mut settings = default_settings();
        settings.auto_model_routing = false;

        let route = select_route(&TaskType::Analysis, &settings);
        assert_eq!(route.provider, "deepseek-v3");
        assert_eq!(route.api_key, "pk-test");
        assert!(route.use_tools);
    }

    #[test]
    fn test_route_analysis_uses_primary_with_tools() {
        let settings = default_settings();
        let route = select_route(&TaskType::Analysis, &settings);
        // Analysis MUST use primary model with tools enabled
        assert_eq!(route.provider, "deepseek-v3");
        assert_eq!(route.api_key, "pk-test");
        assert!(route.use_tools);
    }

    #[test]
    fn test_route_reasoning_uses_reasoning_model() {
        let settings = default_settings();
        let route = select_route(&TaskType::Reasoning, &settings);
        // DeepSeek has a reasoning variant (R1), auto-routed with same API key
        assert_eq!(route.provider, "deepseek-r1");
        assert_eq!(route.api_key, "pk-test");
        assert!(!route.use_tools);
    }

    #[test]
    fn test_route_analysis_fallback_no_reasoning() {
        // Qwen has no reasoning variant — reasoning tasks fallback to primary
        let mut settings = default_settings();
        settings.primary_model = "qwen-plus".to_string();

        let route = select_route(&TaskType::Reasoning, &settings);
        assert_eq!(route.provider, "qwen-plus");
        assert!(route.use_tools);
    }

    #[test]
    fn test_route_provider_capabilities() {
        let caps = get_provider_capabilities("deepseek-v3");
        assert_eq!(caps.reasoning_provider, Some("deepseek-r1"));

        let caps = get_provider_capabilities("qwen-plus");
        assert!(caps.reasoning_provider.is_none());
    }

    #[test]
    fn test_route_general_uses_primary() {
        let settings = default_settings();
        let route = select_route(&TaskType::General, &settings);
        assert_eq!(route.provider, "deepseek-v3");
        assert!(route.use_tools);
    }

    #[test]
    fn test_route_codegen_uses_primary() {
        let settings = default_settings();
        let route = select_route(&TaskType::CodeGen, &settings);
        assert_eq!(route.provider, "deepseek-v3");
        assert!(route.use_tools);
    }

    #[test]
    fn test_route_search_uses_primary() {
        let settings = default_settings();
        let route = select_route(&TaskType::Search, &settings);
        assert_eq!(route.provider, "deepseek-v3");
        assert!(route.use_tools);
    }
}
