//! Python bridge — adapts a Python script into a ToolPlugin.
//!
//! Python tools live in `{resource_dir}/plugins/{id}/` and expose
//! `schema()` and `handle(args, context)` functions.

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::Value;

use crate::python::runner::PythonRunner;

use super::context::PluginContext;
use super::manifest::PluginManifest;
use super::tool_trait::{ToolError, ToolOutput, ToolPlugin};

/// A Python-based tool plugin.
pub struct PythonToolBridge {
    id: String,
    name: String,
    description: String,
    schema: Value,
    plugin_dir: PathBuf,
    handler_file: String,
}

impl PythonToolBridge {
    /// Create from a parsed plugin manifest and its directory.
    pub fn from_manifest(manifest: &PluginManifest, plugin_dir: PathBuf) -> Result<Self, String> {
        let handler = manifest.plugin.handler.as_deref()
            .ok_or("Python tool plugin must specify 'handler' in plugin.toml")?;

        let handler_path = plugin_dir.join(handler);
        if !handler_path.exists() {
            return Err(format!("Handler file not found: {:?}", handler_path));
        }

        // Read the schema from the Python handler by calling schema()
        // For now, use a placeholder — actual schema loading happens at registration time
        Ok(Self {
            id: manifest.plugin.id.clone(),
            name: manifest.plugin.name.clone(),
            description: String::new(),
            schema: Value::Object(serde_json::Map::new()),
            plugin_dir,
            handler_file: handler.to_string(),
        })
    }

    /// Load schema by executing the Python handler's schema() function.
    pub async fn load_schema(&mut self, workspace_path: &std::path::Path) -> Result<(), String> {
        let handler_path = self.plugin_dir.join(&self.handler_file);

        // Pass paths via environment variables to avoid string interpolation injection.
        // The Python code reads them with os.environ.
        let code = [
            "import json, sys, os",
            "sys.path.insert(0, os.environ['_PLUGIN_DIR'])",
            "import importlib.util",
            "spec = importlib.util.spec_from_file_location('handler', os.environ['_HANDLER_PATH'])",
            "mod = importlib.util.module_from_spec(spec)",
            "spec.loader.exec_module(mod)",
            "result = mod.schema()",
            "print(json.dumps(result))",
        ].join("\n");

        // Write env vars into temp files that the runner can pick up
        let temp_dir = workspace_path.join("temp");
        std::fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;
        let env_file = temp_dir.join(format!("plugin_env_{}.json", uuid::Uuid::new_v4()));
        let env_data = serde_json::json!({
            "_PLUGIN_DIR": self.plugin_dir.to_string_lossy(),
            "_HANDLER_PATH": handler_path.to_string_lossy(),
        });
        std::fs::write(&env_file, env_data.to_string()).map_err(|e| e.to_string())?;

        // Wrap the code: read env vars from the temp file at the top
        let bootstrap = format!(
            "import json as _json, os as _os\n\
            with open({:?}) as _f: _env = _json.load(_f)\n\
            for _k, _v in _env.items(): _os.environ[_k] = _v\n\
            _os.remove({:?})\n",
            env_file.to_string_lossy(),
            env_file.to_string_lossy(),
        );
        let full_code = format!("{}\n{}", bootstrap, code);

        let runner = PythonRunner::new(workspace_path.to_path_buf(), None);
        let result = runner.execute(&full_code).await.map_err(|e| e.to_string())?;

        // Clean up env file in case Python didn't delete it
        let _ = std::fs::remove_file(&env_file);

        if result.exit_code != 0 {
            return Err(format!(
                "Failed to load schema from {}: {}",
                self.handler_file, result.stderr
            ));
        }

        let schema: Value = serde_json::from_str(result.stdout.trim())
            .map_err(|e| format!("Invalid schema JSON: {}", e))?;

        self.description = schema.get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if let Some(input_schema) = schema.get("input_schema") {
            self.schema = input_schema.clone();
        }

        if let Some(name) = schema.get("name").and_then(|v| v.as_str()) {
            self.id = name.to_string();
        }

        Ok(())
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

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        let handler_path = self.plugin_dir.join(&self.handler_file);
        let args_json = serde_json::to_string(&input)
            .map_err(|e| ToolError::InvalidArgument(e.to_string()))?;

        let context_json = serde_json::to_string(&serde_json::json!({
            "workspace_path": ctx.workspace_path.to_string_lossy(),
            "conversation_id": ctx.conversation_id,
        })).unwrap_or_default();

        // Pass all data via a temp JSON file to avoid code injection
        let temp_dir = ctx.workspace_path.join("temp");
        std::fs::create_dir_all(&temp_dir)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let data_file = temp_dir.join(format!("plugin_data_{}.json", uuid::Uuid::new_v4()));
        let data = serde_json::json!({
            "plugin_dir": self.plugin_dir.to_string_lossy(),
            "handler_path": handler_path.to_string_lossy(),
            "args": args_json,
            "context": context_json,
        });
        std::fs::write(&data_file, data.to_string())
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let code = format!(
            "import json, sys, os\n\
            with open({data_file:?}) as _f: _data = json.load(_f)\n\
            os.remove({data_file:?})\n\
            sys.path.insert(0, _data['plugin_dir'])\n\
            import importlib.util\n\
            spec = importlib.util.spec_from_file_location('handler', _data['handler_path'])\n\
            mod = importlib.util.module_from_spec(spec)\n\
            spec.loader.exec_module(mod)\n\
            args = json.loads(_data['args'])\n\
            context = json.loads(_data['context'])\n\
            result = mod.handle(args, context)\n\
            print(json.dumps(result))",
            data_file = data_file.to_string_lossy(),
        );

        let runner = PythonRunner::new(
            ctx.workspace_path.clone(),
            ctx.app_handle.as_ref(),
        );
        let result = runner.execute(&code).await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        // Clean up data file in case Python didn't delete it
        let _ = std::fs::remove_file(&data_file);

        if result.exit_code != 0 {
            return Ok(ToolOutput::error(format!(
                "Python tool error: {}",
                if result.stderr.is_empty() { &result.stdout } else { &result.stderr }
            )));
        }

        // Parse the JSON result from the handler
        match serde_json::from_str::<Value>(result.stdout.trim()) {
            Ok(parsed) => {
                let content = parsed.get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&result.stdout)
                    .to_string();
                let is_error = parsed.get("is_error")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                Ok(ToolOutput {
                    content,
                    is_error,
                    data: parsed.get("data").cloned(),
                    generated_files: Vec::new(),
                })
            }
            Err(_) => {
                // If output isn't JSON, treat stdout as content
                Ok(ToolOutput::success(result.stdout))
            }
        }
    }
}
