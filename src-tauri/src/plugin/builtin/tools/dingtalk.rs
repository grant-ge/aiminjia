//! DingTalk AI Table tool plugins — thin wrappers delegating to llm/tool_executor/dingtalk.
//!
//! 6 tools: list_bases, schema, query_records, create_record, update_record, delete_record.
//! Write operations (create/update/delete) set `requires_confirmation = true` in their output
//! so the agent loop presents a confirmation prompt before execution.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

// ── dingtalk_list_bases ─────────────────────────────────────────

pub struct DingtalkListBasesTool;

#[async_trait]
impl ToolPlugin for DingtalkListBasesTool {
    fn name(&self) -> &str { "dingtalk_list_bases" }

    fn description(&self) -> &str {
        "List all DingTalk AI Table bases (databases) the user has access to. \
         Use this first to discover available tables before querying data."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_list_bases(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

// ── dingtalk_schema ─────────────────────────────────────────────

pub struct DingtalkSchemaTool;

#[async_trait]
impl ToolPlugin for DingtalkSchemaTool {
    fn name(&self) -> &str { "dingtalk_schema" }

    fn description(&self) -> &str {
        "Get the schema (tables and fields) of a DingTalk AI Table base. \
         Provide base_id to list tables, or both base_id and table_id to get field definitions. \
         Always call this before querying records to understand the data structure."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "base_id": {
                    "type": "string",
                    "description": "The base ID to inspect"
                },
                "table_id": {
                    "type": "string",
                    "description": "Optional: specific table ID to get field definitions"
                }
            },
            "required": ["base_id"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_schema(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

// ── dingtalk_query_records ──────────────────────────────────────

pub struct DingtalkQueryRecordsTool;

#[async_trait]
impl ToolPlugin for DingtalkQueryRecordsTool {
    fn name(&self) -> &str { "dingtalk_query_records" }

    fn description(&self) -> &str {
        "Query records from a DingTalk AI Table. Supports filtering, sorting, and pagination. \
         Call dingtalk_schema first to understand the table structure before constructing queries."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "base_id": {
                    "type": "string",
                    "description": "The base ID containing the table"
                },
                "table_id": {
                    "type": "string",
                    "description": "The table ID to query"
                },
                "filter": {
                    "type": "string",
                    "description": "Optional filter expression (e.g., \"Status = 'Active'\")"
                },
                "sort": {
                    "type": "string",
                    "description": "Optional sort expression (e.g., \"CreatedAt DESC\")"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of records to return (default 50, max 500)",
                    "default": 50
                }
            },
            "required": ["base_id", "table_id"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_query_records(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

// ── dingtalk_create_record ──────────────────────────────────────

pub struct DingtalkCreateRecordTool;

#[async_trait]
impl ToolPlugin for DingtalkCreateRecordTool {
    fn name(&self) -> &str { "dingtalk_create_record" }

    fn description(&self) -> &str {
        "Create a new record in a DingTalk AI Table. \
         IMPORTANT: Before calling this tool, you MUST first describe the record you plan to create \
         in a text message and wait for the user to confirm. Only call this tool after user confirmation."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "base_id": {
                    "type": "string",
                    "description": "The base ID containing the table"
                },
                "table_id": {
                    "type": "string",
                    "description": "The table ID to add the record to"
                },
                "fields": {
                    "type": "object",
                    "description": "Field name-value pairs for the new record (e.g., {\"Name\": \"Alice\", \"Age\": 30})"
                }
            },
            "required": ["base_id", "table_id", "fields"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_create_record(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

// ── dingtalk_update_record ──────────────────────────────────────

pub struct DingtalkUpdateRecordTool;

#[async_trait]
impl ToolPlugin for DingtalkUpdateRecordTool {
    fn name(&self) -> &str { "dingtalk_update_record" }

    fn description(&self) -> &str {
        "Update an existing record in a DingTalk AI Table. \
         IMPORTANT: Before calling this tool, you MUST first describe the changes in a text message \
         and wait for the user to confirm. Only call this tool after user confirmation."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "base_id": {
                    "type": "string",
                    "description": "The base ID containing the table"
                },
                "table_id": {
                    "type": "string",
                    "description": "The table ID containing the record"
                },
                "record_id": {
                    "type": "string",
                    "description": "The record ID to update"
                },
                "fields": {
                    "type": "object",
                    "description": "Field name-value pairs to update (only changed fields needed)"
                }
            },
            "required": ["base_id", "table_id", "record_id", "fields"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_update_record(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

// ── dingtalk_delete_record ──────────────────────────────────────

pub struct DingtalkDeleteRecordTool;

#[async_trait]
impl ToolPlugin for DingtalkDeleteRecordTool {
    fn name(&self) -> &str { "dingtalk_delete_record" }

    fn description(&self) -> &str {
        "Delete a record from a DingTalk AI Table. This action cannot be undone. \
         IMPORTANT: Before calling this tool, you MUST first describe what will be deleted \
         in a text message and wait for the user to confirm. Only call this tool after user confirmation."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "base_id": {
                    "type": "string",
                    "description": "The base ID containing the table"
                },
                "table_id": {
                    "type": "string",
                    "description": "The table ID containing the record"
                },
                "record_id": {
                    "type": "string",
                    "description": "The record ID to delete"
                }
            },
            "required": ["base_id", "table_id", "record_id"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_delete_record(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// CONTACTS
// ═══════════════════════════════════════════════════════════════════

pub struct DingtalkSearchContactsTool;

#[async_trait]
impl ToolPlugin for DingtalkSearchContactsTool {
    fn name(&self) -> &str { "dingtalk_search_contacts" }

    fn description(&self) -> &str {
        "Search DingTalk contacts (organization members) by name, phone, or email. \
         Returns user IDs needed for other DingTalk operations."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "keyword": {
                    "type": "string",
                    "description": "Search keyword (name, phone number, or email)"
                }
            },
            "required": ["keyword"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_search_contacts(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

pub struct DingtalkGetUserTool;

#[async_trait]
impl ToolPlugin for DingtalkGetUserTool {
    fn name(&self) -> &str { "dingtalk_get_user" }

    fn description(&self) -> &str {
        "Get detailed info about a DingTalk user (name, department, title, email, phone). \
         If no user_id is provided, returns info about the current user."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "user_id": {
                    "type": "string",
                    "description": "The userId to look up. Omit to get current user's info."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_get_user(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

pub struct DingtalkGetDepartmentTool;

#[async_trait]
impl ToolPlugin for DingtalkGetDepartmentTool {
    fn name(&self) -> &str { "dingtalk_get_department" }

    fn description(&self) -> &str {
        "Get DingTalk department info and list sub-departments. \
         Omit dept_id to get the root organization structure."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "dept_id": {
                    "type": "string",
                    "description": "Department ID. Omit for root departments."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_get_department(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// CHAT / GROUP MESSAGING
// ═══════════════════════════════════════════════════════════════════

pub struct DingtalkListGroupsTool;

#[async_trait]
impl ToolPlugin for DingtalkListGroupsTool {
    fn name(&self) -> &str { "dingtalk_list_groups" }

    fn description(&self) -> &str {
        "Search DingTalk groups (conversations) by name. \
         Returns group IDs needed for sending messages. Provide a query to search."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search keyword to find groups by name. Use empty string to browse."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_list_groups(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

pub struct DingtalkSendMessageTool;

#[async_trait]
impl ToolPlugin for DingtalkSendMessageTool {
    fn name(&self) -> &str { "dingtalk_send_message" }

    fn description(&self) -> &str {
        "Send a message to a DingTalk group via bot. Requires a robot_code and group_id. \
         IMPORTANT: Before calling this tool, you MUST describe the message content \
         in a text message and wait for user confirmation."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "robot_code": {
                    "type": "string",
                    "description": "The bot's robot_code identifier"
                },
                "group_id": {
                    "type": "string",
                    "description": "The target group's openConversationId"
                },
                "title": {
                    "type": "string",
                    "description": "Optional message title (for Markdown messages)"
                },
                "text": {
                    "type": "string",
                    "description": "Message text content (supports Markdown)"
                }
            },
            "required": ["robot_code", "group_id", "text"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_send_message(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

pub struct DingtalkSearchChatTool;

#[async_trait]
impl ToolPlugin for DingtalkSearchChatTool {
    fn name(&self) -> &str { "dingtalk_search_chat" }

    fn description(&self) -> &str {
        "Search DingTalk conversations and messages by keyword."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search keyword"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum results (default 10)",
                    "default": 10
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_search_chat(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// CALENDAR
// ═══════════════════════════════════════════════════════════════════

pub struct DingtalkListEventsTool;

#[async_trait]
impl ToolPlugin for DingtalkListEventsTool {
    fn name(&self) -> &str { "dingtalk_list_events" }

    fn description(&self) -> &str {
        "List DingTalk calendar events in a time range. Both start_time and end_time are required."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "start_time": {
                    "type": "string",
                    "description": "Start of time range (ISO 8601, e.g., '2026-04-27T00:00:00+08:00')"
                },
                "end_time": {
                    "type": "string",
                    "description": "End of time range (ISO 8601, e.g., '2026-04-27T23:59:59+08:00')"
                }
            },
            "required": ["start_time", "end_time"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_list_events(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

pub struct DingtalkCreateEventTool;

#[async_trait]
impl ToolPlugin for DingtalkCreateEventTool {
    fn name(&self) -> &str { "dingtalk_create_event" }

    fn description(&self) -> &str {
        "Create a DingTalk calendar event. \
         IMPORTANT: Before calling this tool, you MUST describe the event details \
         in a text message and wait for user confirmation."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "Event title"
                },
                "start_time": {
                    "type": "string",
                    "description": "Start time (ISO 8601, e.g., '2026-04-27T14:00:00+08:00')"
                },
                "end_time": {
                    "type": "string",
                    "description": "End time (ISO 8601)"
                },
                "description": {
                    "type": "string",
                    "description": "Optional event description"
                },
                "attendee_user_ids": {
                    "type": "string",
                    "description": "Optional comma-separated userIds of attendees"
                }
            },
            "required": ["summary", "start_time", "end_time"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_create_event(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

pub struct DingtalkFreeBusyTool;

#[async_trait]
impl ToolPlugin for DingtalkFreeBusyTool {
    fn name(&self) -> &str { "dingtalk_free_busy" }

    fn description(&self) -> &str {
        "Check free/busy status for DingTalk users in a time range. \
         Use this before creating events to find available time slots."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "user_ids": {
                    "type": "string",
                    "description": "Comma-separated userIds to check availability"
                },
                "start_time": {
                    "type": "string",
                    "description": "Start of time range (ISO 8601)"
                },
                "end_time": {
                    "type": "string",
                    "description": "End of time range (ISO 8601)"
                }
            },
            "required": ["user_ids", "start_time", "end_time"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_free_busy(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// TODO / TASKS
// ═══════════════════════════════════════════════════════════════════

pub struct DingtalkListTodosTool;

#[async_trait]
impl ToolPlugin for DingtalkListTodosTool {
    fn name(&self) -> &str { "dingtalk_list_todos" }

    fn description(&self) -> &str {
        "List DingTalk todo tasks. Filter by status (open/done)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "description": "Filter: 'open' (not completed) or 'done' (completed). Omit for all.",
                    "enum": ["open", "done"]
                },
                "limit": {
                    "type": "integer",
                    "description": "Fetch count per page (default 20, auto-paginates if over 20)",
                    "default": 20
                }
            },
            "required": []
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_list_todos(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

pub struct DingtalkCreateTodoTool;

#[async_trait]
impl ToolPlugin for DingtalkCreateTodoTool {
    fn name(&self) -> &str { "dingtalk_create_todo" }

    fn description(&self) -> &str {
        "Create a new DingTalk todo task. \
         IMPORTANT: Before calling this tool, you MUST describe the task details \
         in a text message and wait for user confirmation."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "subject": {
                    "type": "string",
                    "description": "Task title"
                },
                "description": {
                    "type": "string",
                    "description": "Optional task description"
                },
                "due_time": {
                    "type": "string",
                    "description": "Optional deadline (ISO 8601)"
                },
                "executor_user_ids": {
                    "type": "string",
                    "description": "Optional comma-separated userIds of assignees"
                },
                "priority": {
                    "type": "integer",
                    "description": "Priority: 40=urgent, 30=high, 20=normal, 10=low, 0=none",
                    "default": 0
                }
            },
            "required": ["subject"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_create_todo(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

pub struct DingtalkCompleteTodoTool;

#[async_trait]
impl ToolPlugin for DingtalkCompleteTodoTool {
    fn name(&self) -> &str { "dingtalk_complete_todo" }

    fn description(&self) -> &str {
        "Mark a DingTalk todo task as complete. \
         IMPORTANT: Before calling this tool, confirm with the user which task to complete."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The task ID to mark as complete"
                }
            },
            "required": ["task_id"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_dingtalk_complete_todo(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
