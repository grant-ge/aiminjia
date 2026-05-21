//! Chat handlers — search conversations, bot messaging.
//!
//! Response shape: { result: { value: [{ title, openConversationId, memberCount }], total, hasMore } }

use anyhow::Result;
use serde_json::Value;

use super::super::{optional_str, require_str};
use super::get_bridge;
use crate::plugin::context::PluginContext;

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let end = s
            .char_indices()
            .nth(max_chars)
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        format!("{}...", &s[..end])
    }
}

/// List/search groups. Response: result.value[]
pub async fn handle_dingtalk_list_groups(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let query = optional_str(args, "query").unwrap_or("");

    let result = bridge.query(&["chat", "search", "--query", query]).await?;

    let groups = result
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_array());

    if let Some(groups) = groups {
        if groups.is_empty() {
            return Ok("No groups found.".into());
        }
        let total = result
            .get("result")
            .and_then(|r| r.get("total"))
            .and_then(|v| v.as_i64())
            .unwrap_or(groups.len() as i64);
        let mut output = format!(
            "Found {} conversation(s) (showing {}):\n\n",
            total,
            groups.len()
        );
        for g in groups {
            let name = g.get("title").and_then(|v| v.as_str()).unwrap_or("Unnamed");
            let gid = g
                .get("openConversationId")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let members = g.get("memberCount").and_then(|v| v.as_i64()).unwrap_or(0);
            output.push_str(&format!(
                "- **{}** ({} members, id: `{}`)\n",
                name, members, gid
            ));
        }
        Ok(output)
    } else {
        Ok(format!(
            "Groups:\n```json\n{}\n```",
            serde_json::to_string_pretty(&result)?
        ))
    }
}

/// Send message via bot. dws: chat message send-by-bot --robot-code X --group Y --text Z
pub async fn handle_dingtalk_send_message(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let robot_code = require_str(args, "robot_code")?;
    let group_id = require_str(args, "group_id")?;
    let title = optional_str(args, "title").unwrap_or("");
    let text = require_str(args, "text")?;

    let mut cmd_args = vec![
        "chat",
        "message",
        "send-by-bot",
        "--robot-code",
        robot_code,
        "--group",
        group_id,
        "--text",
        text,
    ];
    if !title.is_empty() {
        cmd_args.extend(["--title", title]);
    }

    bridge.mutate(&cmd_args).await?;

    Ok(format!(
        "Message sent to group `{}`.\n\nContent: {}",
        group_id,
        truncate_str(text, 100)
    ))
}

/// Search conversations. Response: result.value[]
pub async fn handle_dingtalk_search_chat(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let query = require_str(args, "query")?;

    let result = bridge.query(&["chat", "search", "--query", query]).await?;

    let groups = result
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_array());

    if let Some(groups) = groups {
        if groups.is_empty() {
            return Ok(format!("No conversations found for \"{}\".", query));
        }
        let mut output = format!(
            "Found {} result(s) matching \"{}\":\n\n",
            groups.len(),
            query
        );
        for (i, g) in groups.iter().enumerate().take(10) {
            let name = g.get("title").and_then(|v| v.as_str()).unwrap_or("?");
            let gid = g
                .get("openConversationId")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let members = g.get("memberCount").and_then(|v| v.as_i64()).unwrap_or(0);
            output.push_str(&format!(
                "{}. **{}** ({} members, id: `{}`)\n",
                i + 1,
                name,
                members,
                gid
            ));
        }
        Ok(output)
    } else {
        Ok(format!(
            "Search results:\n```json\n{}\n```",
            serde_json::to_string_pretty(&result)?
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::tool_executor::require_str;
    use serde_json::json;

    #[test]
    fn test_require_robot_code() {
        let args = json!({"robot_code": "bot123", "group_id": "grp456", "text": "hello"});
        assert_eq!(require_str(&args, "robot_code").unwrap(), "bot123");
    }

    #[test]
    fn test_truncate_chinese() {
        assert_eq!(truncate_str("你好世界测试", 4), "你好世界...");
    }

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate_str("hi", 10), "hi");
    }
}
