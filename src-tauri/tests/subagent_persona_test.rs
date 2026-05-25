//! Wave 4 验收：3 个内置 subagent 都有独立中文人格 prompt，
//! 不继承主对话 "AI小家" 身份，并包含安全 / 输出格式 / 数据真实性条款。
//!
//! 注：browse_data_agent 已在 tool cleanup Phase 3 中删除（连同浏览器工具族）。

use app_lib::runtime::agent::builtin::{
    daily_assistant_agent::daily_assistant_agent_definition, explore::explore_agent_definition,
    general_purpose::general_purpose_agent_definition,
};
use app_lib::runtime::agent::definition::AgentPrompt;

fn extract_prompt(p: &AgentPrompt) -> String {
    match p {
        AgentPrompt::Inline(s) => s.clone(),
        _ => panic!("expected Inline prompt"),
    }
}

#[test]
fn general_purpose_persona_has_safety_and_output_clauses() {
    let def = general_purpose_agent_definition();
    let prompt = extract_prompt(&def.system_prompt);
    assert!(
        prompt.contains("绝不创建文件"),
        "general-purpose persona missing safety clause: {prompt}"
    );
    assert!(
        prompt.contains("path:line"),
        "general-purpose persona missing output format clause"
    );
    assert!(
        prompt.contains("不能编造"),
        "general-purpose persona missing data truthfulness clause"
    );
    // 不应包含主对话身份（子代理是独立人格）
    assert!(
        !prompt.contains("AI小家"),
        "subagent must NOT inherit main identity"
    );
    assert!(
        prompt.len() >= 200,
        "expected detailed persona, got {} chars",
        prompt.len()
    );
}

#[test]
fn explore_persona_has_strict_readonly_block() {
    let def = explore_agent_definition();
    let prompt = extract_prompt(&def.system_prompt);
    assert!(
        prompt.contains("严格只读"),
        "explore persona missing strict readonly declaration"
    );
    assert!(
        prompt.contains("严格禁止"),
        "explore persona missing prohibition list"
    );
    assert!(
        prompt.contains("不要捏造"),
        "explore persona missing anti-fabrication clause"
    );
    assert!(!prompt.contains("AI小家"));
    assert!(prompt.len() >= 300);
}

#[test]
fn daily_assistant_persona_has_professional_boundary() {
    let def = daily_assistant_agent_definition();
    let prompt = extract_prompt(&def.system_prompt);
    assert!(prompt.contains("专业资质"));
    assert!(prompt.contains("不替用户做决定"));
    assert!(!prompt.contains("AI小家"));
    assert!(prompt.len() >= 200);
}
