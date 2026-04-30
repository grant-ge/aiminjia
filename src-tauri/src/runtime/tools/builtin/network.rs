//! Web search as RuntimeTool.
//!
//! `WebSearchRuntimeTool` does NOT accept a `PluginContext`.  All search
//! dependencies are injected at construction time via `SearchDeps`.
//! The actual search logic lives in `llm::tool_executor::execute_web_search_core`
//! and is shared with the legacy `handle_web_search` path.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::auth::AuthManager;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

// ── SearchDeps ───────────────────────────────────────────────────────────────

/// Narrow search dependencies — injected at construction, not from CapabilityContext.
///
/// Only the fields that `web_search` actually needs are present here.
/// `CapabilityContext` is intentionally NOT extended with these fields.
pub struct SearchDeps {
    pub tavily_api_key: Option<String>,
    pub bocha_api_key: Option<String>,
    pub use_cloud: bool,
    pub auth_manager: Option<Arc<AuthManager>>,
}

// ── WebSearchRuntimeTool ─────────────────────────────────────────────────────

pub struct WebSearchRuntimeTool {
    deps: Arc<SearchDeps>,
}

impl WebSearchRuntimeTool {
    pub fn new(deps: SearchDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }
}

#[async_trait]
impl RuntimeTool for WebSearchRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("web_search")
            .unwrap_or_else(|| ToolDefinition::new("web_search", "搜索互联网获取最新信息"))
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let query = input
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: query".into()))?;
        let max_results = input
            .get("max_results")
            .and_then(Value::as_u64)
            .unwrap_or(5) as u32;

        let result = crate::llm::tool_executor::execute_web_search_core(
            query,
            max_results,
            self.deps.use_cloud,
            self.deps.auth_manager.as_ref(),
            self.deps.bocha_api_key.as_deref(),
            self.deps.tavily_api_key.as_deref(),
        )
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(ToolResult {
            tool_name: "web_search".to_string(),
            content: result.clone(),
            data: Some(Value::String(result)),
            file_meta: None,
            is_degraded: false,
            degradation_notice: None,
        })
    }
}
