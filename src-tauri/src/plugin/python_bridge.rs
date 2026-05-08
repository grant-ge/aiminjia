//! Python bridge stub — Python tools have been deleted.
//!
//! This module previously adapted Python scripts into ToolPlugin implementations.
//! All Python-based tools have been removed in Phase 3 of the tool cleanup.
//! This stub preserves the module declaration so that `plugin/mod.rs` continues
//! to compile without modification.
#![allow(deprecated)]

use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;

use super::context::PluginContext;
use super::tool_trait::{ToolError, ToolOutput, ToolPlugin};

/// Stub for the former Python-based tool plugin bridge.
///
/// All Python execution tools have been removed. This type is retained only for
/// compilation compatibility; it cannot be instantiated in production code.
pub struct PythonToolBridge {
    id: String,
    description: String,
    schema: Value,
    plugin_dir: PathBuf,
    handler_file: String,
}

impl PythonToolBridge {
    /// Create from a parsed plugin manifest and its directory.
    #[allow(dead_code)]
    pub fn from_manifest(_id: &str, _handler: &str, _plugin_dir: PathBuf) -> Result<Self, String> {
        unimplemented!("Python plugin bridge removed in Phase 3 tool cleanup")
    }

    /// Load schema (stub — always returns error).
    pub async fn load_schema(&mut self, _workspace_path: &std::path::Path) -> Result<(), String> {
        Err("Python bridge tools have been removed".to_string())
    }
}

#[async_trait]
impl ToolPlugin for PythonToolBridge {
    fn name(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.schema.clone()
    }

    async fn execute(&self, _ctx: &PluginContext, _input: Value) -> Result<ToolOutput, ToolError> {
        Err(ToolError::ExecutionFailed(
            "Python bridge tools have been removed".to_string(),
        ))
    }
}
