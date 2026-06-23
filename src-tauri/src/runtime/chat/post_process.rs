//! Post-processing of streaming content after an LLM turn completes.
//!
//! Extracted from `chat_runtime_impl.rs` Block 32 (L3246–L3285).
//!
//! Responsibilities:
//! 1. Append a max-iterations notice when the iteration cap was reached.
//! 2. Supply a user-visible fallback when the LLM returned empty content.
//! 3. Strip hallucinated XML function-call blocks from the final text.
//!
//! Note: `verify_file_claims` is intentionally **not** included here because it
//! depends on workspace-path and file-meta state owned by the call site; it will
//! be addressed in a later task.

/// Strip hallucinated XML function-call blocks from LLM content.
///
/// Some models (especially OpenAI-compatible providers) occasionally
/// hallucinate `<function_calls>` blocks, `<invoke>` tags, or similar
/// XML tool-call patterns in their text output. These are not real
/// tool calls and must be stripped.
///
/// Also strips `<tool_call>` / `<tool_calls>` patterns.
///
/// Re-exported from `llm::content_filter` for use within this module; callers
/// outside this module should prefer `llm::content_filter::strip_hallucinated_xml`.
pub(crate) fn strip_hallucinated_xml(text: &str) -> String {
    crate::llm::content_filter::strip_hallucinated_xml(text)
}

/// Perform all post-stream content transformations on `full_content`.
///
/// # Arguments
///
/// * `full_content`      – Accumulated text produced by the streaming LLM call.
///                         Modified in-place.
/// * `iteration_count`   – Number of tool-use iterations that occurred during
///                         this turn.
/// * `max_iterations`    – Configured ceiling for tool iterations.
/// * `stream_cancelled`  – `true` if the user (or a cancel token) interrupted
///                         the stream; used to suppress the empty-content
///                         fallback when cancellation is the expected cause.
///
/// # Transformations applied (in order)
///
/// 1. **Max-iterations notice** – if `iteration_count >= max_iterations`, a
///    Chinese-language notice is appended so the user understands the result
///    may be incomplete.
/// 2. **Empty-content fallback** – if the content is blank after trimming and
///    the stream was not cancelled, a user-visible error message is set.
/// 3. **Hallucinated-XML strip** – removes any stray `<function_calls>`,
///    `<invoke>`, `<tool_call>`, or `<tool_calls>` blocks.
pub fn finalize_content(
    full_content: &mut String,
    iteration_count: usize,
    max_iterations: usize,
    stream_cancelled: bool,
) {
    // 1. Append max-iterations notice when cap is reached.
    if iteration_count >= max_iterations {
        log::warn!("[post_process] Hit max_iterations ({})", max_iterations,);
        let notice = format!(
            "\n\n---\n⚠️ 已达处理上限（{} 次迭代）。以上内容可能是不完整交付，不代表所有命名文件、配置、脚本、报告或数据结果都已完成。\n\
            如果原请求包含具体产物，请继续补齐缺失项或明确说明阻塞原因；不要把未验证的分析当成最终完成结果。",
            max_iterations
        );
        full_content.push_str(&notice);
    }

    // 2. Provide a fallback when the LLM returned absolutely nothing.
    //    Two distinct cases:
    //    (a) user cancelled → write a 已取消 placeholder so the message row is
    //        not blank after refresh (in-memory TurnSummaryBadge shows the
    //        same state during the current session).
    //    (b) LLM truly returned empty → standard error fallback.
    if full_content.trim().is_empty() {
        if stream_cancelled {
            log::info!(
                "[post_process] empty content + stream_cancelled — writing cancel placeholder (iterations={})",
                iteration_count,
            );
            *full_content = "（已取消）".to_string();
        } else {
            log::warn!(
                "[post_process] LLM returned empty content (iterations={})",
                iteration_count,
            );
            *full_content =
                "抱歉，模型未能生成回复。可能原因：内容限制、网络问题或服务暂时不可用。请尝试换一种方式提问。"
                    .to_string();
        }
    }

    // 3. Strip hallucinated XML function-call blocks before saving.
    *full_content = strip_hallucinated_xml(full_content);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_change_for_normal_content() {
        let mut content = "normal response".to_string();
        finalize_content(&mut content, 3, 10, false);
        assert_eq!(content, "normal response");
    }

    #[test]
    fn appends_notice_when_at_max_iterations() {
        let mut content = "partial result".to_string();
        finalize_content(&mut content, 10, 10, false);
        assert!(content.starts_with("partial result"));
        assert!(content.contains("处理上限"));
        assert!(content.contains("10"));
        assert!(content.contains("不完整交付"));
        assert!(!content.contains("当前阶段的分析结果"));
    }

    #[test]
    fn sets_fallback_when_empty_and_not_cancelled() {
        let mut content = String::new();
        finalize_content(&mut content, 1, 10, false);
        assert!(!content.is_empty());
        assert!(content.contains("模型未能生成回复"));
    }

    #[test]
    fn cancel_placeholder_written_when_empty_and_cancelled() {
        let mut content = String::new();
        finalize_content(&mut content, 1, 10, true);
        // stream was cancelled with no content → write 已取消 placeholder so the
        // persisted final message is not blank after refresh.
        assert_eq!(content, "（已取消）");
    }

    #[test]
    fn cancel_does_not_overwrite_existing_content() {
        let mut content = "Partial response before cancel.".to_string();
        finalize_content(&mut content, 1, 10, true);
        // If LLM already produced text before cancel, keep it.
        assert_eq!(content, "Partial response before cancel.");
    }

    #[test]
    fn strips_hallucinated_xml() {
        let mut content = "Result: <function_calls>junk</function_calls> done".to_string();
        finalize_content(&mut content, 1, 10, false);
        assert_eq!(content, "Result:  done");
    }
}
