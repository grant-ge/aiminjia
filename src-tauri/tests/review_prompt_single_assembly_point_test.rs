use std::fs;
use std::path::PathBuf;

use app_lib::llm::prompts::{build_system_prompt_parts, get_system_prompt};
use app_lib::runtime::chat::context_builder::build_iteration_context;

fn prompts_rs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/llm/prompts.rs")
}

fn tauri_chat_command_rs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/transport/tauri_commands/chat.rs")
}

#[test]
fn review_context_builder_remains_dynamic_only() {
    let result = build_iteration_context(
        "mem",
        "project-memory",
        "workspace",
        "files",
        "notes",
        None,
        None,
        "",
    );

    assert!(
        result.starts_with("[动态上下文"),
        "context_builder output must stay in dynamic context space"
    );
    assert!(
        !result.contains("工具选择偏好"),
        "context_builder must not assemble system prompt static sections"
    );
}

#[test]
fn review_get_system_prompt_is_a_compatibility_shim_over_parts() {
    let daily_parts = build_system_prompt_parts(None, None);
    let expected_daily = format!(
        "{}\n\n{}",
        daily_parts.static_section, daily_parts.dynamic_section
    );
    assert_eq!(get_system_prompt(None, None, None), expected_daily);
    assert_eq!(get_system_prompt(Some(0), None, None), expected_daily);
}

#[test]
fn review_prompts_module_documents_single_assembly_boundary() {
    let source = fs::read_to_string(prompts_rs()).expect("read prompts.rs");

    assert!(
        source.contains("PromptStore 是纯文本片段仓库"),
        "prompts.rs must document PromptStore as a fragment-only store"
    );
    assert!(
        source.contains("build_system_prompt_parts 是 system prompt 的唯一组装入口"),
        "prompts.rs must document build_system_prompt_parts as the single assembly entrypoint"
    );
    assert!(
        source.contains("工具选择偏好章节——静态内容，写入 static_section"),
        "TOOL_PREFERENCE_SECTION comment should explain it is a static section"
    );
}

#[test]
fn review_prompt_mode_variants_are_removed_from_prompt_chain() {
    let prompts_source = fs::read_to_string(prompts_rs()).expect("read prompts.rs");
    let chat_source = fs::read_to_string(tauri_chat_command_rs()).expect("read chat.rs");
    let retired_marker = ["Prompt", "Mode"].concat();

    assert!(
        !prompts_source.contains(&retired_marker),
        "prompts.rs should not reference the retired prompt mode type"
    );
    assert!(
        !chat_source.contains(&retired_marker),
        "chat.rs should not reference the retired prompt mode type"
    );
}
