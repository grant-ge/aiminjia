use app_lib::runtime::employee::knowledge::{chunk_markdown, KnowledgeChunk};

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
