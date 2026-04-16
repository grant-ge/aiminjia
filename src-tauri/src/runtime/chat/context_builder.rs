/// Build the dynamic context injected into the system prompt on each iteration.
///
/// This is kept separate from the static system prompt so that KV-cache prefix stability is
/// preserved: only the dynamic user message changes, not the system message.
///
/// The concatenation order and separators intentionally mirror Block 13 of `chat_runtime_impl.rs`
/// (lines ~2230-2291) so that the LLM sees an identical context layout regardless of which code
/// path produces the string.
pub fn build_iteration_context(
    core_memory: &str,
    workspace_context: &str,
    file_context: &str,
    analysis_notes: &str,
    precompute_result: Option<&str>,
    connector_context: Option<&str>,
    analysis_ctx_prompt: Option<&str>,
) -> String {
    let mut ctx = String::from("[动态上下文 — 请勿回复此消息]\n");

    // 1. Cognitive core memory — always loaded (cross-session knowledge base)
    if !core_memory.is_empty() {
        ctx.push_str("\n[核心记忆]\n");
        ctx.push_str(core_memory);
        ctx.push_str("\n");
    }

    // 2. Workspace context (file listing, project summary, etc.)
    if !workspace_context.is_empty() {
        ctx.push_str(workspace_context);
    }

    // 3. File context (contents of uploaded / referenced files)
    if !file_context.is_empty() {
        ctx.push_str(file_context);
    }

    // 4. Analysis notes (accumulated observations from previous iterations)
    if !analysis_notes.is_empty() {
        ctx.push_str(analysis_notes);
    }

    // 5. Precompute result — tagged block so the LLM treats it as pre-computed data
    if let Some(pc_result) = precompute_result {
        ctx.push_str("\n\n[precompute_result]\n");
        ctx.push_str("以下是系统自动计算的结果，请基于这些数据向用户展示分析结论。\n");
        ctx.push_str(pc_result);
        ctx.push_str("\n[/precompute_result]");
    }

    // 6. Internal connector context (browsing sessions / legacy app integrations)
    if let Some(connector) = connector_context {
        ctx.push_str("\n\n[内部系统浏览]\n");
        ctx.push_str(
            "你可以通过 browse_navigate 工具直接打开浏览器访问任意内部系统 URL，\
             用户在 Chrome 中登录后即可读取数据。\n\n",
        );
        ctx.push_str(connector);
        ctx.push_str("\n[/内部系统浏览]");
    }

    // 7. Analysis-mode context prompt (AnalysisContext::format_for_prompt())
    //    and step plan are injected by the caller and forwarded here as a single
    //    pre-formatted string.
    //
    //    NOTE: The original Block 13 also reads `_plan.md` from disk inline when
    //    `is_analysis` is true (workspace_path/analysis/{conversation_id}/_plan.md).
    //    That file-I/O is kept in the call-site for now so this function stays pure.
    //    TODO(S4-T5): lift plan-file reading into this function once the workspace-path
    //    is threaded through the ChatTurnRequest / TurnConfig.
    if let Some(ctx_prompt) = analysis_ctx_prompt {
        if !ctx_prompt.is_empty() {
            ctx.push_str(ctx_prompt);
        }
    }

    ctx
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_HEADER: &str = "[动态上下文 — 请勿回复此消息]\n";

    #[test]
    fn test_empty_inputs_yields_only_header() {
        let result = build_iteration_context("", "", "", "", None, None, None);
        assert_eq!(result, EMPTY_HEADER);
    }

    #[test]
    fn test_core_memory_block() {
        let result = build_iteration_context("mem content", "", "", "", None, None, None);
        assert!(result.contains("\n[核心记忆]\n"));
        assert!(result.contains("mem content"));
        assert!(result.contains("\n[核心记忆]\nmem content\n"));
    }

    #[test]
    fn test_precompute_result_block() {
        let result =
            build_iteration_context("", "", "", "", Some("computed data"), None, None);
        assert!(result.contains("[precompute_result]\n"));
        assert!(result.contains("computed data"));
        assert!(result.contains("[/precompute_result]"));
    }

    #[test]
    fn test_connector_context_block() {
        let result =
            build_iteration_context("", "", "", "", None, Some("connector info"), None);
        assert!(result.contains("[内部系统浏览]\n"));
        assert!(result.contains("connector info"));
        assert!(result.contains("[/内部系统浏览]"));
    }

    #[test]
    fn test_concatenation_order() {
        let result = build_iteration_context(
            "CORE",
            "WORKSPACE",
            "FILES",
            "NOTES",
            Some("PRECOMPUTE"),
            Some("CONNECTOR"),
            Some("ANALYSIS"),
        );

        let core_pos = result.find("CORE").expect("CORE missing");
        let ws_pos = result.find("WORKSPACE").expect("WORKSPACE missing");
        let file_pos = result.find("FILES").expect("FILES missing");
        let notes_pos = result.find("NOTES").expect("NOTES missing");
        let pre_pos = result.find("PRECOMPUTE").expect("PRECOMPUTE missing");
        let conn_pos = result.find("CONNECTOR").expect("CONNECTOR missing");
        let ana_pos = result.find("ANALYSIS").expect("ANALYSIS missing");

        assert!(core_pos < ws_pos);
        assert!(ws_pos < file_pos);
        assert!(file_pos < notes_pos);
        assert!(notes_pos < pre_pos);
        assert!(pre_pos < conn_pos);
        assert!(conn_pos < ana_pos);
    }
}
