//! AI Table handlers — base/table/record/field CRUD.

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::plugin::context::PluginContext;
use super::super::{require_str, optional_str, optional_i64};
use super::get_bridge;

/// List bases. Response: data.bases[].baseId/baseName
pub async fn handle_dingtalk_list_bases(ctx: &PluginContext, _args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let result = bridge.query(&["aitable", "base", "list"]).await?;

    let bases = result.get("data")
        .and_then(|d| d.get("bases"))
        .and_then(|b| b.as_array());

    if let Some(arr) = bases {
        if arr.is_empty() {
            return Ok("No AI Table bases found.".into());
        }
        let mut output = format!("Found {} AI Table base(s):\n\n", arr.len());
        for base in arr {
            let name = base.get("baseName").and_then(|v| v.as_str()).unwrap_or("Untitled");
            let id = base.get("baseId").and_then(|v| v.as_str()).unwrap_or("?");
            output.push_str(&format!("- **{}** (base_id: `{}`)\n", name, id));
        }
        Ok(output)
    } else {
        Ok(format!("Response:\n```json\n{}\n```", serde_json::to_string_pretty(&result)?))
    }
}

/// Get tables or fields.
/// Tables: data.tables[] ; Fields: data.fields[].fieldId/fieldName/type
pub async fn handle_dingtalk_schema(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let base_id = require_str(args, "base_id")?;
    let table_id = optional_str(args, "table_id");

    let result = if let Some(tid) = table_id {
        bridge.query(&["aitable", "field", "get", "--base-id", base_id, "--table-id", tid]).await?
    } else {
        bridge.query(&["aitable", "table", "get", "--base-id", base_id]).await?
    };

    let data = result.get("data").unwrap_or(&result);

    if table_id.is_some() {
        // Fields: data.fields[]
        if let Some(fields) = data.get("fields").and_then(|f| f.as_array()) {
            let mut output = format!("Table fields ({} columns):\n\n", fields.len());
            output.push_str("| Field Name | Type | Field ID |\n|---|---|---|\n");
            for f in fields {
                let name = f.get("fieldName").and_then(|v| v.as_str()).unwrap_or("?");
                let ftype = f.get("type").and_then(|v| v.as_str()).unwrap_or("?");
                let fid = f.get("fieldId").and_then(|v| v.as_str()).unwrap_or("?");
                output.push_str(&format!("| {} | {} | `{}` |\n", name, ftype, fid));
            }
            return Ok(output);
        }
    } else {
        // Tables: data.tables[]
        if let Some(tables) = data.get("tables").and_then(|t| t.as_array()) {
            let mut output = format!("Tables in base ({}):\n\n", tables.len());
            for t in tables {
                let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("Untitled");
                let tid = t.get("tableId").and_then(|v| v.as_str()).unwrap_or("?");
                output.push_str(&format!("- **{}** (table_id: `{}`)\n", name, tid));
            }
            return Ok(output);
        }
    }

    Ok(format!("Schema:\n```json\n{}\n```", serde_json::to_string_pretty(&data)?))
}

/// Query records. Response: data.records[].cells/recordId
pub async fn handle_dingtalk_query_records(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let base_id = require_str(args, "base_id")?;
    let table_id = require_str(args, "table_id")?;
    let filters = optional_str(args, "filter");
    let sort = optional_str(args, "sort");
    let limit = optional_i64(args, "limit", 50);
    let query_keyword = optional_str(args, "query");

    let mut cmd_args = vec![
        "aitable", "record", "query",
        "--base-id", base_id,
        "--table-id", table_id,
    ];

    let limit_str = limit.to_string();
    cmd_args.extend(["--limit", &limit_str]);

    if let Some(f) = filters {
        cmd_args.extend(["--filters", f]);
    }
    if let Some(s) = sort {
        cmd_args.extend(["--sort", s]);
    }
    if let Some(q) = query_keyword {
        cmd_args.extend(["--query", q]);
    }

    let result = bridge.query(&cmd_args).await?;

    // data.records[]
    let records = result.get("data")
        .and_then(|d| d.get("records"))
        .and_then(|r| r.as_array());

    if let Some(records) = records {
        let count = records.len();
        if count == 0 {
            return Ok("No records found matching the query.".into());
        }
        if count > 20 {
            let sample: Vec<&Value> = records.iter().take(5).collect();
            let sample_json = serde_json::to_string_pretty(&sample)?;
            Ok(format!(
                "Query returned {} records. Showing first 5:\n\n```json\n{}\n```\n\n\
                 Use `load_file` with the full dataset for detailed analysis.",
                count, sample_json
            ))
        } else {
            Ok(format!(
                "Query returned {} record(s):\n\n```json\n{}\n```",
                count, serde_json::to_string_pretty(&records)?
            ))
        }
    } else {
        Ok(format!("Query result:\n```json\n{}\n```", serde_json::to_string_pretty(&result)?))
    }
}

/// Create record. dws: --records '[{"cells":{...}}]'
pub async fn handle_dingtalk_create_record(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let base_id = require_str(args, "base_id")?;
    let table_id = require_str(args, "table_id")?;
    let fields = args.get("fields")
        .ok_or_else(|| anyhow!("Missing required argument: fields"))?;

    let records = serde_json::json!([{"cells": fields}]);
    let records_json = serde_json::to_string(&records)?;

    let result = bridge.mutate(&[
        "aitable", "record", "create",
        "--base-id", base_id,
        "--table-id", table_id,
        "--records", &records_json,
    ]).await?;

    let record_id = result.get("data")
        .and_then(|d| d.get("records"))
        .and_then(|r| r.as_array())
        .and_then(|arr| arr.first())
        .and_then(|r| r.get("recordId"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    Ok(format!(
        "Record created successfully (record_id: `{}`).\n\nFields:\n```json\n{}\n```",
        record_id, serde_json::to_string_pretty(fields)?
    ))
}

/// Update record. dws: --records '[{"recordId":"X","cells":{...}}]'
pub async fn handle_dingtalk_update_record(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let base_id = require_str(args, "base_id")?;
    let table_id = require_str(args, "table_id")?;
    let record_id = require_str(args, "record_id")?;
    let fields = args.get("fields")
        .ok_or_else(|| anyhow!("Missing required argument: fields"))?;

    let records = serde_json::json!([{"recordId": record_id, "cells": fields}]);
    let records_json = serde_json::to_string(&records)?;

    bridge.mutate(&[
        "aitable", "record", "update",
        "--base-id", base_id,
        "--table-id", table_id,
        "--records", &records_json,
    ]).await?;

    Ok(format!(
        "Record `{}` updated.\n\nUpdated fields:\n```json\n{}\n```",
        record_id, serde_json::to_string_pretty(fields)?
    ))
}

/// Delete record. dws: --record-ids rec1,rec2
pub async fn handle_dingtalk_delete_record(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let base_id = require_str(args, "base_id")?;
    let table_id = require_str(args, "table_id")?;
    let record_id = require_str(args, "record_id")?;

    bridge.mutate(&[
        "aitable", "record", "delete",
        "--base-id", base_id,
        "--table-id", table_id,
        "--record-ids", record_id,
    ]).await?;

    Ok(format!("Record `{}` deleted.", record_id))
}

#[cfg(test)]
mod tests {
    use crate::llm::tool_executor::{require_str, optional_str, optional_i64};
    use serde_json::json;

    #[test]
    fn test_require_str() {
        let args = json!({"base_id": "abc123", "table_id": "tbl456"});
        assert_eq!(require_str(&args, "base_id").unwrap(), "abc123");
    }

    #[test]
    fn test_optional() {
        let args = json!({"base_id": "abc"});
        assert_eq!(optional_str(&args, "filter"), None);
        assert_eq!(optional_i64(&args, "limit", 50), 50);
    }
}
