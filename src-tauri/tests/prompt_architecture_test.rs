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
