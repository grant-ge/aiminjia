use app_lib::llm::prompts::{self, PromptMode};
use app_lib::runtime::chat::prompt::{PromptAssembler, PromptBuildContext};

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use app_lib::runtime::chat::prompt::{
    PromptAssembly, PromptBlock, PromptCachePolicy, PromptSectionCache, PromptSectionId,
    PromptSectionSpec,
};

#[test]
fn prompt_assembly_keeps_static_blocks_before_dynamic_blocks() {
    let assembly = PromptAssembly::new(vec![
        PromptBlock::static_block(PromptSectionId::new("intro"), "static intro"),
        PromptBlock::dynamic_block(PromptSectionId::new("persona"), "dynamic persona"),
    ]);

    let payload = assembly.to_system_view();
    assert_eq!(payload.blocks.len(), 2);
    assert_eq!(payload.blocks[0].text, "static intro");
    assert_eq!(
        payload.blocks[0].cache_policy,
        PromptCachePolicy::StaticPrefix
    );
    assert_eq!(payload.blocks[1].text, "dynamic persona");
    assert_eq!(
        payload.blocks[1].cache_policy,
        PromptCachePolicy::SessionDynamic
    );
}

#[test]
fn prompt_section_cache_reuses_session_dynamic_sections() {
    let cache = PromptSectionCache::new();
    let section_id = PromptSectionId::new("env_info_simple");

    let first = cache.get_or_insert(section_id.clone(), || "env-v1".to_string());
    let second = cache.get_or_insert(section_id.clone(), || "env-v2".to_string());

    assert_eq!(first, "env-v1");
    assert_eq!(second, "env-v1");

    cache.clear();
    let third = cache.get_or_insert(section_id, || "env-v3".to_string());
    assert_eq!(third, "env-v3");
}

#[test]
fn prompt_section_cache_does_not_compute_cache_hits() {
    let cache = PromptSectionCache::new();
    let section_id = PromptSectionId::new("env_info_simple");
    let compute_count = Arc::new(AtomicUsize::new(0));

    let first_count = Arc::clone(&compute_count);
    let first = cache.get_or_insert(section_id.clone(), || {
        first_count.fetch_add(1, Ordering::SeqCst);
        "env-v1".to_string()
    });

    let second_count = Arc::clone(&compute_count);
    let second = cache.get_or_insert(section_id, || {
        second_count.fetch_add(1, Ordering::SeqCst);
        "env-v2".to_string()
    });

    assert_eq!(first, "env-v1");
    assert_eq!(second, "env-v1");
    assert_eq!(compute_count.load(Ordering::SeqCst), 1);
}

#[test]
fn volatile_section_spec_requires_reason() {
    let spec = PromptSectionSpec::volatile(
        PromptSectionId::new("mcp_instructions_delta"),
        "MCP servers connect and disconnect between turns",
    );

    assert_eq!(spec.cache_policy, PromptCachePolicy::Volatile);
    assert_eq!(
        spec.cache_break_reason.as_deref(),
        Some("MCP servers connect and disconnect between turns")
    );
}

#[test]
fn prompt_assembler_places_base_before_dynamic_daily_prompt() {
    let tmp = tempfile::tempdir().unwrap();
    let bundled = tmp.path().join("bundled");
    let user = tmp.path().join("user");
    std::fs::create_dir_all(bundled.join("prompts")).unwrap();
    std::fs::create_dir_all(&user).unwrap();
    std::fs::write(bundled.join("prompts/base.md"), "AI小家 base").unwrap();
    std::fs::write(bundled.join("prompts/daily.md"), "daily prompt").unwrap();
    std::fs::write(bundled.join("prompts/browser_agent.md"), "browser prompt").unwrap();
    prompts::init_prompts(&bundled, &user);

    let assembler = PromptAssembler::default();
    let assembly = assembler.build_system_prompt(PromptBuildContext {
        mode: PromptMode::Daily,
        persona: None,
        product_name: None,
    });

    let blocks = assembly.blocks();
    assert!(blocks[0].text.contains("AI小家 base"));
    assert_eq!(blocks[0].cache_policy, PromptCachePolicy::StaticPrefix);
    assert!(blocks
        .iter()
        .any(|block| block.text.contains("daily prompt")));
    assert!(blocks
        .iter()
        .any(|block| block.cache_policy == PromptCachePolicy::SessionDynamic));
}
