use std::fs;
use std::path::PathBuf;

use crate::runtime::mcp::McpServerConfig;

/// Persists MCP server configurations to a JSON file.
#[derive(Debug, Clone)]
pub struct McpConfigStore {
    path: PathBuf,
}

impl McpConfigStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<Vec<McpServerConfig>, String> {
        if !self.path.exists() {
            return Ok(vec![]);
        }

        let content = fs::read_to_string(&self.path)
            .map_err(|err| format!("Failed to read MCP config file: {err}"))?;

        if content.trim().is_empty() {
            return Ok(vec![]);
        }

        serde_json::from_str(&content)
            .map_err(|err| format!("Failed to parse MCP config file: {err}"))
    }

    pub fn save(&self, configs: &[McpServerConfig]) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create MCP config directory: {err}"))?;
        }

        let content = serde_json::to_string_pretty(configs)
            .map_err(|err| format!("Failed to serialize MCP configs: {err}"))?;
        fs::write(&self.path, content)
            .map_err(|err| format!("Failed to write MCP config file: {err}"))?;
        Ok(())
    }

    pub fn add(&self, config: McpServerConfig) -> Result<(), String> {
        let mut configs = self.load()?;
        if configs.iter().any(|existing| existing.name == config.name) {
            return Err(format!("MCP server '{}' already exists", config.name));
        }
        configs.push(config);
        self.save(&configs)
    }

    pub fn remove(&self, name: &str) -> Result<(), String> {
        let mut configs = self.load()?;
        let before = configs.len();
        configs.retain(|config| config.name != name);
        if configs.len() == before {
            return Err(format!("MCP server '{}' not found", name));
        }
        self.save(&configs)
    }
}
