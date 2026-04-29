//! DingTalk tool handlers — 5 product domains via dws CLI.
//!
//! Each submodule covers one DingTalk product:
//! - aitable: AI Table CRUD (base/table/record/field)
//! - contact: Contacts (search/user/department)
//! - chat:    Group messaging (list/send/search)
//! - calendar: Calendar events (list/create/free-busy)
//! - todo:    Task management (list/create/complete)

mod aitable;
mod contact;
mod chat;
mod calendar;
mod todo;

use anyhow::{anyhow, Result};
use crate::connector::dingtalk::DingtalkBridge;
use crate::plugin::context::PluginContext;

/// Shared helper: get DingtalkBridge from PluginContext with auth guard.
pub(crate) async fn get_bridge(ctx: &PluginContext) -> Result<&DingtalkBridge> {
    let bridge = ctx.dingtalk_bridge.as_ref()
        .ok_or_else(|| anyhow!("DingTalk integration not initialized"))?;

    if !bridge.is_connected().await {
        return Err(anyhow!(
            "DingTalk not connected. Please authorize in Settings → DingTalk Account first."
        ));
    }

    Ok(bridge)
}

// ── Re-exports ──────────────────────────────────────────────────

// AI Table (6 tools)
pub use aitable::handle_dingtalk_list_bases;
pub use aitable::handle_dingtalk_schema;
pub use aitable::handle_dingtalk_query_records;
pub use aitable::handle_dingtalk_create_record;
pub use aitable::handle_dingtalk_update_record;
pub use aitable::handle_dingtalk_delete_record;

// Contacts (3 tools)
pub use contact::handle_dingtalk_search_contacts;
pub use contact::handle_dingtalk_get_user;
pub use contact::handle_dingtalk_get_department;

// Chat (3 tools)
pub use chat::handle_dingtalk_list_groups;
pub use chat::handle_dingtalk_send_message;
pub use chat::handle_dingtalk_search_chat;

// Calendar (3 tools)
pub use calendar::handle_dingtalk_list_events;
pub use calendar::handle_dingtalk_create_event;
pub use calendar::handle_dingtalk_free_busy;

// Todo (3 tools)
pub use todo::handle_dingtalk_list_todos;
pub use todo::handle_dingtalk_create_todo;
pub use todo::handle_dingtalk_complete_todo;
