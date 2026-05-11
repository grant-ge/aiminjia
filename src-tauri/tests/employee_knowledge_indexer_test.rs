use app_lib::runtime::employee::knowledge::chunk_markdown;

#[test]
fn chunk_markdown_splits_on_h2_headings() {
    let src = "# 产品 FAQ\n\n## 怎么注册\n\n点击右上角注册按钮。\n\n## 怎么充值\n\n进入控制台 → 余额 → 充值。\n";
    let chunks = chunk_markdown(src);
    assert_eq!(chunks.len(), 2);
    assert!(chunks[0].content.contains("注册"));
    assert!(chunks[1].content.contains("充值"));
    assert_eq!(chunks[0].title.as_deref(), Some("怎么注册"));
}

#[test]
fn chunk_markdown_splits_q_a_pattern() {
    let src = "Q: 怎么找回密码？\nA: 在登录页点击\"忘记密码\"。\n\nQ: 客服电话？\nA: 400-123-4567\n";
    let chunks = chunk_markdown(src);
    assert_eq!(chunks.len(), 2);
    assert!(chunks[0].content.starts_with("Q: 怎么找回密码"));
}

#[test]
fn chunk_markdown_handles_long_paragraphs_by_double_newline() {
    let src = "段落一内容。\n\n段落二内容。\n\n段落三内容。\n";
    let chunks = chunk_markdown(src);
    assert_eq!(chunks.len(), 3);
}

#[test]
fn chunk_markdown_collapses_chunks_under_min_size() {
    let src = "短\n\n短2\n\n## 标题\n\n这是一个比较长的段落用于触发新分块的产生。";
    let chunks = chunk_markdown(src);
    // "短" 和 "短2" 太短被合并
    assert!(chunks.len() <= 2);
}

#[test]
fn chunk_markdown_respects_cognitive_memory_byte_limit() {
    // The cognitive memory store enforces CONTENT_MAX_LEN bytes per entry.
    // Chunks ultimately get prefixed with "【title】\n" before saving, so we
    // assert a conservative bound: every produced chunk content (without the
    // title prefix) must already fit within the limit, leaving headroom for
    // the prefix.
    use app_lib::storage::file_store::cognitive::CONTENT_MAX_LEN;

    // 4500-char Chinese paragraph (each char is 3 bytes → ~13.5KB).
    let huge_para: String = "这是一个非常长的段落用来测试硬切分策略。".repeat(120);
    let src = format!("## 大段\n\n{}\n", huge_para);
    let chunks = chunk_markdown(&src);
    for (i, c) in chunks.iter().enumerate() {
        // Title prefix worst case: "【…】\n" with title up to 100 bytes.
        let title_overhead = c.title.as_ref().map(|t| t.len() + 6).unwrap_or(0);
        let total_bytes = c.content.len() + title_overhead;
        assert!(
            total_bytes <= CONTENT_MAX_LEN,
            "chunk #{i} too large: {} bytes (limit {}). content head: {:?}",
            total_bytes,
            CONTENT_MAX_LEN,
            &c.content.chars().take(40).collect::<String>()
        );
    }
    assert!(chunks.len() > 1, "expected huge paragraph to be hard-split");
}

#[test]
fn chunk_markdown_short_chinese_paragraph_fits_byte_limit() {
    use app_lib::storage::file_store::cognitive::CONTENT_MAX_LEN;
    let src = "## 注册流程\n\n打开 App，点击右上角的注册按钮，输入手机号收到验证码后填写并提交即可。";
    let chunks = chunk_markdown(src);
    assert!(!chunks.is_empty());
    for c in &chunks {
        assert!(
            c.content.len() <= CONTENT_MAX_LEN,
            "chunk too large: {} bytes",
            c.content.len()
        );
    }
}
