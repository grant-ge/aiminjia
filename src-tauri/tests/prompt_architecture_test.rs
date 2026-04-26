use app_lib::runtime::chat::prompt::{
    PromptAssembly, PromptBlock, PromptCachePolicy, PromptSectionId,
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
