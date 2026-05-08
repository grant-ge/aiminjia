//! review: TOOL_CATALOG contains TaskStop and TaskGet with correct schemas.
//!
//! 5 cases:
//! 1. task_stop_is_in_catalog
//! 2. task_get_is_in_catalog
//! 3. task_stop_required_field_is_task_id
//! 4. task_stop_is_in_daily_allowed
//! 5. task_get_is_in_daily_allowed

use app_lib::runtime::tools::catalog::{DAILY_ALLOWED_TOOLS, TOOL_CATALOG};

// ─── Test 1 ──────────────────────────────────────────────────────────────────

#[test]
fn task_stop_is_in_catalog() {
    assert!(
        TOOL_CATALOG.get("TaskStop").is_some(),
        "TaskStop must be registered in TOOL_CATALOG"
    );
}

// ─── Test 2 ──────────────────────────────────────────────────────────────────

#[test]
fn task_get_is_in_catalog() {
    assert!(
        TOOL_CATALOG.get("TaskGet").is_some(),
        "TaskGet must be registered in TOOL_CATALOG"
    );
}

// ─── Test 3 ──────────────────────────────────────────────────────────────────

#[test]
fn task_stop_required_field_is_task_id() {
    let entry = TOOL_CATALOG
        .get_entry("TaskStop")
        .expect("TaskStop must have a catalog entry with json_schema");

    let schema = &entry.json_schema;

    // JSON Schema must list task_id in "required"
    let required = schema
        .get("required")
        .and_then(|v| v.as_array())
        .expect("TaskStop schema must have a 'required' array");

    let has_task_id = required.iter().any(|v| v.as_str() == Some("task_id"));
    assert!(
        has_task_id,
        "TaskStop schema 'required' array must contain 'task_id', got: {:?}",
        required
    );

    // Also verify the property is defined
    let props = schema
        .get("properties")
        .expect("TaskStop schema must have a 'properties' object");
    assert!(
        props.get("task_id").is_some(),
        "TaskStop schema properties must contain 'task_id', got: {:?}",
        props
    );
}

// ─── Test 4 ──────────────────────────────────────────────────────────────────

#[test]
fn task_stop_is_in_daily_allowed() {
    assert!(
        DAILY_ALLOWED_TOOLS.contains(&"TaskStop"),
        "TaskStop must be in DAILY_ALLOWED_TOOLS so the daily mode LLM can call it; \
         got: {:?}",
        DAILY_ALLOWED_TOOLS
    );
}

// ─── Test 5 ──────────────────────────────────────────────────────────────────

#[test]
fn task_get_is_in_daily_allowed() {
    assert!(
        DAILY_ALLOWED_TOOLS.contains(&"TaskGet"),
        "TaskGet must be in DAILY_ALLOWED_TOOLS; got: {:?}",
        DAILY_ALLOWED_TOOLS
    );
}
