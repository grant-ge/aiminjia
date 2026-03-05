//! Content filtering utilities for LLM output sanitization.
//!
//! Strips hallucinated XML function-call blocks that some models emit
//! in their text output. These are not real tool calls and must be
//! removed before displaying to users or saving to DB.

/// Strip hallucinated XML function-call blocks from LLM content.
///
/// Some models (especially OpenAI-compatible providers) occasionally
/// hallucinate `<function_calls>` blocks, `<invoke>` tags, or similar
/// XML tool-call patterns in their text output. These are not real
/// tool calls and must be stripped.
///
/// Also strips `<tool_call>` / `<tool_calls>` patterns.
pub fn strip_hallucinated_xml(text: &str) -> String {
    let mut result = text.to_string();

    // Tags to strip (with their content between open and close)
    let tag_pairs: &[(&str, &str)] = &[
        ("<function_calls>", "</function_calls>"),
        ("<invoke ", "</invoke>"),
        ("<invoke>", "</invoke>"),
        ("<tool_call>", "</tool_call>"),
        ("<tool_calls>", "</tool_calls>"),
    ];

    for &(open, close) in tag_pairs {
        loop {
            if let Some(start) = result.find(open) {
                if let Some(end_offset) = result[start..].find(close) {
                    // Remove the entire block including closing tag
                    result.replace_range(start..start + end_offset + close.len(), "");
                } else {
                    // Unclosed tag — remove from open tag to end of string
                    result.truncate(start);
                }
            } else {
                break;
            }
        }
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_xml_tags_unchanged() {
        let input = "Hello, this is a normal response.";
        assert_eq!(strip_hallucinated_xml(input), input);
    }

    #[test]
    fn strips_function_calls_block() {
        let input = "Some text <function_calls><invoke name=\"foo\"><parameter name=\"bar\">baz</parameter></invoke></function_calls> more text";
        assert_eq!(strip_hallucinated_xml(input), "Some text  more text");
    }

    #[test]
    fn strips_unclosed_function_calls() {
        let input = "Some text <function_calls><invoke name=\"foo\">";
        assert_eq!(strip_hallucinated_xml(input), "Some text");
    }

    #[test]
    fn strips_tool_call_tags() {
        let input = "Result: <tool_call>{\"name\": \"test\"}</tool_call> done";
        assert_eq!(strip_hallucinated_xml(input), "Result:  done");
    }

    #[test]
    fn strips_multiple_blocks() {
        let input = "A <function_calls>block1</function_calls> B <tool_call>block2</tool_call> C";
        assert_eq!(strip_hallucinated_xml(input), "A  B  C");
    }

    #[test]
    fn preserves_normal_xml() {
        let input = "Use <b>bold</b> and <i>italic</i> formatting.";
        assert_eq!(strip_hallucinated_xml(input), input);
    }

    #[test]
    fn strips_invoke_with_attributes() {
        let input = "Text <invoke name=\"execute_python\"><parameter>code</parameter></invoke> end";
        assert_eq!(strip_hallucinated_xml(input), "Text  end");
    }

    #[test]
    fn empty_input() {
        assert_eq!(strip_hallucinated_xml(""), "");
    }

    #[test]
    fn whitespace_only_after_strip() {
        let input = "  <function_calls>junk</function_calls>  ";
        assert_eq!(strip_hallucinated_xml(input), "");
    }
}
