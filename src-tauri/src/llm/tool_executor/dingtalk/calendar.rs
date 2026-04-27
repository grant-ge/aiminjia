//! Calendar handlers — list events, create event, free/busy.
//!
//! Response structure TBD (needs real API test), using flexible parsing.

use anyhow::Result;
use serde_json::Value;

use crate::plugin::context::PluginContext;
use super::super::{require_str, optional_str};
use super::get_bridge;

/// Try to extract events array from dws calendar response.
/// Tries: result[] → data.events[] → data[] → events[] → top-level array
fn extract_events(result: &Value) -> Option<&Vec<Value>> {
    // Try result[] (common dws pattern)
    if let Some(arr) = result.get("result").and_then(|r| r.as_array()) {
        return Some(arr);
    }
    // Try data.events[] or data[]
    if let Some(data) = result.get("data") {
        if let Some(events) = data.get("events").and_then(|e| e.as_array()) {
            return Some(events);
        }
        if let Some(arr) = data.as_array() {
            return Some(arr);
        }
    }
    // Try events[]
    if let Some(events) = result.get("events").and_then(|e| e.as_array()) {
        return Some(events);
    }
    // Top-level array
    result.as_array()
}

fn format_event(e: &Value) -> String {
    let summary = e.get("summary").or_else(|| e.get("title"))
        .and_then(|v| v.as_str()).unwrap_or("Untitled");
    let start = e.get("start").and_then(|v| {
        v.get("dateTime").or_else(|| v.get("date")).and_then(|d| d.as_str())
    }).or_else(|| e.get("startTime").and_then(|v| v.as_str()))
        .unwrap_or("?");
    let end = e.get("end").and_then(|v| {
        v.get("dateTime").or_else(|| v.get("date")).and_then(|d| d.as_str())
    }).or_else(|| e.get("endTime").and_then(|v| v.as_str()))
        .unwrap_or("?");
    let location = e.get("location").and_then(|v| {
        v.get("displayName").and_then(|d| d.as_str()).or_else(|| v.as_str())
    }).unwrap_or("");
    let eid = e.get("id").or_else(|| e.get("eventId"))
        .and_then(|v| v.as_str()).unwrap_or("?");

    let mut line = format!("- **{}** (event_id: `{}`)\n  {} -> {}", summary, eid, start, end);
    if !location.is_empty() {
        line.push_str(&format!(" @ {}", location));
    }
    line.push('\n');
    line
}

/// List calendar events. dws: calendar event list --start X --end Y (both required)
pub async fn handle_dingtalk_list_events(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let start_time = require_str(args, "start_time")?;
    let end_time = require_str(args, "end_time")?;

    let result = bridge.query(&["calendar", "event", "list", "--start", start_time, "--end", end_time]).await?;

    if let Some(events) = extract_events(&result) {
        if events.is_empty() {
            return Ok(format!("No events found between {} and {}.", start_time, end_time));
        }
        let mut output = format!("Found {} event(s):\n\n", events.len());
        for e in events {
            output.push_str(&format_event(e));
        }
        Ok(output)
    } else {
        Ok(format!("Events:\n```json\n{}\n```", serde_json::to_string_pretty(&result)?))
    }
}

/// Create calendar event. dws: calendar event create --title X --start Y --end Z
pub async fn handle_dingtalk_create_event(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let summary = require_str(args, "summary")?;
    let start_time = require_str(args, "start_time")?;
    let end_time = require_str(args, "end_time")?;
    let description = optional_str(args, "description");
    let attendees = optional_str(args, "attendee_user_ids");

    let mut cmd_args = vec![
        "calendar", "event", "create",
        "--title", summary,
        "--start", start_time,
        "--end", end_time,
    ];

    if let Some(desc) = description {
        cmd_args.extend(["--desc", desc]);
    }
    if let Some(att) = attendees {
        cmd_args.extend(["--attendees", att]);
    }

    let result = bridge.mutate(&cmd_args).await?;

    // Try to extract event ID from response
    let event_id = result.get("result")
        .or_else(|| result.get("data"))
        .and_then(|d| d.get("id").or_else(|| d.get("eventId")))
        .and_then(|v| v.as_str())
        .unwrap_or("created");

    Ok(format!(
        "Event created (event_id: `{}`).\n\n**{}**\n{} -> {}\n{}",
        event_id, summary, start_time, end_time,
        attendees.map(|a| format!("Attendees: {}\n", a)).unwrap_or_default(),
    ))
}

/// Free/busy check — lists events in range as a workaround (no dedicated free-busy API).
pub async fn handle_dingtalk_free_busy(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let start_time = require_str(args, "start_time")?;
    let end_time = require_str(args, "end_time")?;

    let result = bridge.query(&[
        "calendar", "event", "list",
        "--start", start_time,
        "--end", end_time,
    ]).await?;

    if let Some(events) = extract_events(&result) {
        if events.is_empty() {
            return Ok(format!("No events in {} -> {}. The time slot appears free.", start_time, end_time));
        }
        let mut output = format!(
            "Found {} event(s) in {} -> {} (busy times):\n\n",
            events.len(), start_time, end_time
        );
        for e in events {
            output.push_str(&format_event(e));
        }
        Ok(output)
    } else {
        Ok(format!("Events in range:\n```json\n{}\n```", serde_json::to_string_pretty(&result)?))
    }
}

#[cfg(test)]
mod tests {
    use crate::llm::tool_executor::require_str;
    use serde_json::json;

    #[test]
    fn test_require_event_fields() {
        let args = json!({"summary": "Meeting", "start_time": "2026-04-27T14:00:00", "end_time": "2026-04-27T15:00:00"});
        assert_eq!(require_str(&args, "summary").unwrap(), "Meeting");
        assert_eq!(require_str(&args, "start_time").unwrap(), "2026-04-27T14:00:00");
    }
}
