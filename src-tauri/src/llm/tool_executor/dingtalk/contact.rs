//! Contact handlers — search users, get user details, department search.
//!
//! Key dws response shapes:
//! - user search: { userId: ["id1", "id2"] } — only returns ID list
//! - user get: same as get-self
//! - user get-self: { result: [{ orgEmployeeModel: { orgUserName, userId, orgName, ... } }] }
//! - dept search: { deptList: [{ deptId: number, deptName: string }] }

use anyhow::Result;
use serde_json::Value;

use crate::plugin::context::PluginContext;
use super::super::{require_str, optional_str};
use super::get_bridge;

/// Search contacts. dws returns only userId list, so we batch-fetch details.
pub async fn handle_dingtalk_search_contacts(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let keyword = require_str(args, "keyword")?;

    // Step 1: search returns { userId: ["id1", "id2"] }
    let search_result = bridge.query(&[
        "contact", "user", "search",
        "--query", keyword,
    ]).await?;

    let user_ids = search_result.get("userId")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect::<Vec<_>>())
        .unwrap_or_default();

    if user_ids.is_empty() {
        return Ok(format!("No contacts found for \"{}\".", keyword));
    }

    // Step 2: batch get user details
    let ids_str = user_ids.join(",");
    let detail_result = bridge.query(&[
        "contact", "user", "get",
        "--ids", &ids_str,
    ]).await?;

    // get returns result[].orgEmployeeModel or similar
    let users = detail_result.get("result")
        .and_then(|r| r.as_array());

    if let Some(users) = users {
        let mut output = format!("Found {} contact(s) matching \"{}\":\n\n", users.len(), keyword);
        for u in users {
            let emp = u.get("orgEmployeeModel").unwrap_or(u);
            let name = emp.get("orgUserName").or_else(|| emp.get("name"))
                .and_then(|v| v.as_str()).unwrap_or("?");
            let uid = emp.get("userId").and_then(|v| v.as_str()).unwrap_or("?");
            let email = emp.get("orgAuthEmail").and_then(|v| v.as_str()).unwrap_or("");
            let depts: Vec<&str> = emp.get("depts")
                .and_then(|d| d.as_array())
                .map(|arr| arr.iter()
                    .filter_map(|d| d.get("deptName").and_then(|v| v.as_str()))
                    .collect())
                .unwrap_or_default();

            output.push_str(&format!("- **{}** (userId: `{}`)", name, uid));
            if !depts.is_empty() {
                output.push_str(&format!(" — {}", depts.join(", ")));
            }
            if !email.is_empty() {
                output.push_str(&format!(" | {}", email));
            }
            output.push('\n');
        }
        Ok(output)
    } else {
        // Fallback: just show user IDs
        let mut output = format!("Found {} userId(s) matching \"{}\":\n\n", user_ids.len(), keyword);
        for uid in &user_ids {
            output.push_str(&format!("- userId: `{}`\n", uid));
        }
        Ok(output)
    }
}

/// Get user info. dws: contact user get --ids "userId" or get-self
pub async fn handle_dingtalk_get_user(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let user_id = optional_str(args, "user_id");

    let result = if let Some(uid) = user_id {
        bridge.query(&["contact", "user", "get", "--ids", uid]).await?
    } else {
        bridge.query(&["contact", "user", "get-self"]).await?
    };

    // result[0].orgEmployeeModel
    let employee = result.get("result")
        .and_then(|r| r.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("orgEmployeeModel"));

    if let Some(emp) = employee {
        let name = emp.get("orgUserName").and_then(|v| v.as_str()).unwrap_or("?");
        let uid = emp.get("userId").and_then(|v| v.as_str()).unwrap_or("?");
        let org = emp.get("orgName").and_then(|v| v.as_str()).unwrap_or("");
        let email = emp.get("orgAuthEmail").and_then(|v| v.as_str()).unwrap_or("");
        let mobile = emp.get("orgUserMobile").and_then(|v| v.as_str()).unwrap_or("");
        let depts: Vec<&str> = emp.get("depts")
            .and_then(|d| d.as_array())
            .map(|arr| arr.iter()
                .filter_map(|d| d.get("deptName").and_then(|v| v.as_str()))
                .collect())
            .unwrap_or_default();

        let mut output = format!("**{}** (userId: `{}`)\n", name, uid);
        if !org.is_empty() { output.push_str(&format!("- Organization: {}\n", org)); }
        if !depts.is_empty() { output.push_str(&format!("- Department: {}\n", depts.join(", "))); }
        if !email.is_empty() { output.push_str(&format!("- Email: {}\n", email)); }
        if !mobile.is_empty() { output.push_str(&format!("- Mobile: {}\n", mobile)); }
        Ok(output)
    } else {
        Ok(format!("User info:\n```json\n{}\n```", serde_json::to_string_pretty(&result)?))
    }
}

/// Search departments. dws: contact dept search --query "keyword"
/// Response: { deptList: [{ deptId: number, deptName: string }] }
pub async fn handle_dingtalk_get_department(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let query = optional_str(args, "dept_id")
        .or_else(|| optional_str(args, "query"))
        .unwrap_or("");

    let result = bridge.query(&["contact", "dept", "search", "--query", query]).await?;

    let depts = result.get("deptList").and_then(|d| d.as_array());

    if let Some(depts) = depts {
        if depts.is_empty() {
            return Ok("No departments found.".into());
        }
        let mut output = format!("Found {} department(s):\n\n", depts.len());
        for d in depts {
            // deptName may contain <red>highlight</red> tags from search
            let raw_name = d.get("deptName").and_then(|v| v.as_str()).unwrap_or("?");
            let name = raw_name.replace("<red>", "").replace("</red>", "");
            let did = d.get("deptId").and_then(|v| v.as_i64()).unwrap_or(0);
            output.push_str(&format!("- **{}** (dept_id: `{}`)\n", name, did));
        }
        Ok(output)
    } else {
        Ok(format!("Departments:\n```json\n{}\n```", serde_json::to_string_pretty(&result)?))
    }
}

#[cfg(test)]
mod tests {
    use crate::llm::tool_executor::require_str;
    use serde_json::json;

    #[test]
    fn test_require_keyword() {
        let args = json!({"keyword": "张三"});
        assert_eq!(require_str(&args, "keyword").unwrap(), "张三");
    }
}
