//! fetch_saas_data handler — delegates to ConnectorEngine.

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::plugin::context::PluginContext;
use super::require_str;

/// Handle fetch_saas_data tool invocations.
pub(crate) async fn handle_fetch_saas_data(ctx: &PluginContext, args: &Value) -> Result<String> {
    let action = require_str(args, "action")?;
    let app_name = require_str(args, "app_name")?;

    let engine = ctx.connector_engine.as_ref()
        .ok_or_else(|| anyhow!("SaaS connector not initialized"))?;

    match action {
        "get_sample" => {
            let capability_name = require_str(args, "capability_name")?;
            let sample = engine.get_capability_sample(app_name, capability_name).await
                .map_err(|e| anyhow!(e))?;
            Ok(serde_json::to_string_pretty(&sample)?)
        }
        "execute" => {
            let capability_name = require_str(args, "capability_name")?;
            let parameters = args.get("parameters").cloned();
            let result = engine.execute_request(app_name, capability_name, parameters).await
                .map_err(|e| anyhow!(e))?;
            Ok(result)
        }
        "set_credential" => {
            let api_credential = require_str(args, "api_credential")?;
            let result = engine.set_credential_by_name(app_name, api_credential).await
                .map_err(|e| anyhow!(e))?;
            Ok(result)
        }
        _ => Err(anyhow!("Invalid action: {}. Use 'get_sample', 'execute', or 'set_credential'.", action))
    }
}
