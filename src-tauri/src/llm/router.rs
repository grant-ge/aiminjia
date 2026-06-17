//! Smart model router — selects optimal model based on task type and settings.
//!
//! The router inspects the latest user message to infer a [`TaskType`], then
//! consults [`AppSettings`] to decide which provider and API key to use.
//!
//! Each provider has known model capabilities. When `auto_model_routing` is
//! enabled, the router automatically selects the reasoning variant (e.g.
//! DeepSeek-R1) for reasoning tasks using the same API key.
//!
//! **Important**: Reasoning routes can keep tools enabled; the cloud gateway
//! validates `reasoning + tool_calling` against configured route capabilities.
#![allow(dead_code)]

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
        "openai" => ProviderCapabilities {
            primary_provider: "openai",
            reasoning_provider: None, // TODO: add o1 support
            models_desc: "主力: GPT-4o",
        },
        "custom" => ProviderCapabilities {
            primary_provider: "custom",
            reasoning_provider: None,
            models_desc: "自定义 OpenAI 兼容模型",
        },
        "aijia-v2" => ProviderCapabilities {
            primary_provider: "aijia-v2",
            reasoning_provider: None,
            models_desc: "云端模型（登录后可用）",
        },
        _ => ProviderCapabilities {
            primary_provider: "aijia-v2",
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
    /// Deep analysis requiring reasoning (compensation fairness, statistics).
    Analysis,
    /// Code generation (Python scripts for data processing)
    CodeGen,
    /// Web search synthesis
    Search,
    /// Pure reasoning task (explicitly requested).
    Reasoning,
}

/// Result of routing: which provider + model to use.
#[derive(Debug, Clone)]
pub struct RouteResult {
    /// Provider identifier, e.g. "openai", "custom", "aijia-v2"
    pub provider: String,
    /// API key for the selected provider
    pub api_key: String,
    /// Specific model ID hint (used by providers like Volcano that need it)
    pub model_hint: String,
    /// Whether this route supports tool use
    pub use_tools: bool,
    /// Custom endpoint URL (only used by "custom" provider)
    pub endpoint_url: String,
    /// Model type for AIjia Gateway V2 routing: "chat" or "reasoner"
    pub model_type: String,
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
    // NOTE: Avoid overly broad keywords like "分析" or "诊断" alone — they
    // appear in everyday conversation (e.g. "分析下伊朗局势"). Use compound
    // domain-specific terms that reliably indicate a data analysis task.
    let analysis_keywords = [
        "薪酬分析",
        "薪资分析",
        "薪酬诊断",
        "薪资诊断",
        "公平性",
        "薪酬",
        "薪资",
        "回归分析",
        "标准差",
        "salary analysis",
        "pay equity",
        "compensation",
        "regression",
        "statistics",
        "standard deviation",
        "相关性分析",
        "显著性",
        "偏差分析",
    ];
    if analysis_keywords.iter().any(|kw| text.contains(kw)) {
        return TaskType::Analysis;
    }

    // Code generation keywords
    let code_keywords = [
        "代码",
        "脚本",
        "python",
        "计算",
        "code",
        "script",
        "compute",
        "函数",
        "function",
        "算法",
        "algorithm",
    ];
    if code_keywords.iter().any(|kw| text.contains(kw)) {
        return TaskType::CodeGen;
    }

    // Search keywords
    let search_keywords = [
        "搜索",
        "查找",
        "市场数据",
        "search",
        "lookup",
        "benchmark",
        "行业数据",
        "薪酬报告",
        "market data",
        "salary survey",
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
///   because the 6-step analysis workflow requires tool calls.
/// - Only `Reasoning` tasks use the reasoning variant (if available).
/// - All other task types use the primary model with tools.
///
/// The reasoning model is auto-determined from provider capabilities.
/// No separate configuration is needed — the same API key is used.
pub fn select_route(task_type: &TaskType, settings: &AppSettings) -> RouteResult {
    // All chat routes through the AIjia v2 cloud gateway. Local-model and
    // custom-provider configuration was removed from the product, so there is
    // no non-cloud path. Reasoning tasks force the reasoner endpoint; every
    // other task uses the model_type implied by the user's selection
    // (default "chat"). Tools stay enabled so Lotus can require a route that
    // explicitly supports reasoning + tool_calling when the request needs both.
    // The session_key is injected by the gateway, so `api_key` here is just a
    // placeholder carrier.
    let model_type = if *task_type == TaskType::Reasoning {
        "reasoner"
    } else if settings.cloud_model_type.is_empty() {
        "chat"
    } else {
        &settings.cloud_model_type
    };
    RouteResult {
        provider: "aijia-v2".to_string(),
        api_key: settings.primary_api_key.clone(),
        model_hint: settings.cloud_model.clone(),
        use_tools: true,
        endpoint_url: String::new(),
        model_type: model_type.to_string(),
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
            primary_api_key: "sk-sess-test".to_string(),
            cloud_model: "claude-sonnet-4-5".to_string(),
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
        let msgs = make_messages(&[(
            "user",
            "Please analyze the salary regression data for pay equity",
        )]);
        assert_eq!(infer_task_type(&msgs), TaskType::Analysis);
    }

    #[test]
    fn test_infer_analysis_chinese() {
        let msgs = make_messages(&[("user", "请对薪酬公平性进行诊断")]);
        assert_eq!(infer_task_type(&msgs), TaskType::Analysis);
    }

    #[test]
    fn test_infer_general_with_analysis_word() {
        // "分析" alone should NOT trigger Analysis — it's too broad for everyday use
        let msgs = make_messages(&[("user", "分析下伊朗最新局势")]);
        assert_eq!(infer_task_type(&msgs), TaskType::General);
    }

    #[test]
    fn test_infer_codegen() {
        let msgs = make_messages(&[("user", "Write a Python script to compute averages")]);
        assert_eq!(infer_task_type(&msgs), TaskType::CodeGen);
    }

    #[test]
    fn test_infer_search() {
        let msgs = make_messages(&[(
            "user",
            "Search for market data on software engineer salaries",
        )]);
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
    fn test_route_general_uses_aijia_v2_with_tools() {
        let settings = default_settings();
        let route = select_route(&TaskType::General, &settings);
        assert_eq!(route.provider, "aijia-v2");
        assert_eq!(route.api_key, "sk-sess-test");
        assert_eq!(route.model_hint, "claude-sonnet-4-5");
        assert_eq!(route.model_type, "chat");
        assert!(route.use_tools);
    }

    #[test]
    fn test_route_analysis_uses_aijia_v2_with_tools() {
        let settings = default_settings();
        let route = select_route(&TaskType::Analysis, &settings);
        assert_eq!(route.provider, "aijia-v2");
        assert_eq!(route.model_type, "chat");
        assert!(route.use_tools);
    }

    #[test]
    fn test_route_codegen_uses_aijia_v2_with_tools() {
        let settings = default_settings();
        let route = select_route(&TaskType::CodeGen, &settings);
        assert_eq!(route.provider, "aijia-v2");
        assert!(route.use_tools);
    }

    #[test]
    fn test_route_search_uses_aijia_v2_with_tools() {
        let settings = default_settings();
        let route = select_route(&TaskType::Search, &settings);
        assert_eq!(route.provider, "aijia-v2");
        assert!(route.use_tools);
    }

    #[test]
    fn test_route_reasoning_forces_reasoner_endpoint() {
        let settings = default_settings();
        let route = select_route(&TaskType::Reasoning, &settings);
        assert_eq!(route.provider, "aijia-v2");
        assert_eq!(route.model_type, "reasoner");
        assert!(route.use_tools);
    }

    #[test]
    fn test_route_honors_explicit_cloud_model_type() {
        let mut settings = default_settings();
        settings.cloud_model_type = "chat".to_string();
        let route = select_route(&TaskType::General, &settings);
        assert_eq!(route.model_type, "chat");
        assert!(route.use_tools);
    }
}
