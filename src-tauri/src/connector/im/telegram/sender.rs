//! Telegram outbound: sendMessage HTML + 429 retry + 400 fallback to plain.
//!
//! 输入是 AI 生成的 GFM markdown 子集。出站用 Telegram HTML parse_mode
//! （比 MarkdownV2 宽松：只需 escape `<>&` 三个字符，没有 `*`、`_`、`.`、`!` 等
//! 杂乱限制；可直接渲染 `<b><i><code><pre><a><s>`），通过 `markdown_to_telegram_html`
//! 把 markdown 标记转成对应 tag。
//!
//! 401 / 403 错误向上抛由 connector 处理（403 触发 allowlist 移除）。

use std::sync::Arc;
use std::time::Duration;

use super::api::{TelegramApi, TelegramApiError};

/// Telegram sendMessage 4096 字符上限；按 byte 保留 96 byte 给 HTML 实体展开余量。
pub const MAX_MESSAGE_BYTES: usize = 4000;

pub struct TelegramSender {
    api: Arc<TelegramApi>,
}

#[derive(Debug, thiserror::Error)]
pub enum SenderError {
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("forbidden by recipient: {0}")]
    Forbidden(String),
    #[error("transport: {0}")]
    Transport(String),
}

impl TelegramSender {
    pub fn new(api: Arc<TelegramApi>) -> Self {
        Self { api }
    }

    /// markdown → Telegram HTML send；不带 reply_to。对外 default。
    pub async fn send_markdown(&self, chat_id: i64, raw_markdown: &str) -> Result<(), SenderError> {
        self.send_markdown_with_reply(chat_id, raw_markdown, None)
            .await
    }

    /// markdown → Telegram HTML 分片 send；可指定 reply_to_message_id（**仅首条** chunk 带）。
    /// 同时从 markdown 提取本地路径，文本发完后串行 send_document 附件，附件 reply 到首条文本。
    pub async fn send_markdown_with_reply(
        &self,
        chat_id: i64,
        raw_markdown: &str,
        reply_to_message_id: Option<i64>,
    ) -> Result<(), SenderError> {
        // 提取附件并把 markdown 中的占位符替换成「📎 label」
        let attachments = extract_local_paths(raw_markdown);
        let mut clean_markdown = raw_markdown.to_string();
        for a in &attachments {
            let placeholder = format!("📎 {}", a.display_label);
            clean_markdown = clean_markdown.replace(&a.original_segment, &placeholder);
        }

        let html = markdown_to_telegram_html(&clean_markdown);
        let chunks = split_telegram_html(&html, MAX_MESSAGE_BYTES);
        let mut first_chunk_id: Option<i64> = None;
        let mut is_first = true;
        for chunk in chunks {
            let reply = if is_first { reply_to_message_id } else { None };
            match self
                .send_html_chunk_with_reply(chat_id, &chunk, reply)
                .await
            {
                Ok(sent_id) => {
                    if is_first {
                        first_chunk_id = Some(sent_id);
                    }
                }
                Err(SenderError::Transport(desc)) if desc.starts_with("parse error:") => {
                    // 整段回 plain text fallback；不再尝试后续 chunks。
                    let plain = strip_markdown(&clean_markdown);
                    return self
                        .api
                        .send_message_with_reply(chat_id, &plain, None, reply)
                        .await
                        .map(|_| ())
                        .map_err(map_err);
                }
                Err(e) => return Err(e),
            }
            is_first = false;
        }

        // 文本 chunks 已发完，串行发附件。reply 到首条文本，形成视觉聚合。
        for a in &attachments {
            match self
                .api
                .send_document(chat_id, &a.absolute_path, None, first_chunk_id)
                .await
            {
                Ok(_) => {}
                Err(TelegramApiError::FileTooBig { size, limit }) => {
                    let hint = format!(
                        "📎 {} 太大未上传（{:.1}MB > {:.0}MB 上限）",
                        a.absolute_path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("文件"),
                        size as f64 / 1024.0 / 1024.0,
                        limit as f64 / 1024.0 / 1024.0,
                    );
                    let _ = self
                        .api
                        .send_message_with_reply(chat_id, &hint, None, first_chunk_id)
                        .await;
                }
                Err(e) => {
                    log::warn!(
                        "[telegram-sender] send_document failed for {}: {e:?}",
                        a.absolute_path.display()
                    );
                    let hint = format!(
                        "📎 {} 发送失败",
                        a.absolute_path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("文件")
                    );
                    let _ = self
                        .api
                        .send_message_with_reply(chat_id, &hint, None, first_chunk_id)
                        .await;
                }
            }
        }
        Ok(())
    }

    /// 单 chunk send + 429 重试 + connect 阶段失败重试。返回 sent message_id（首条用作附件 reply target）。
    async fn send_html_chunk_with_reply(
        &self,
        chat_id: i64,
        html: &str,
        reply_to_message_id: Option<i64>,
    ) -> Result<i64, SenderError> {
        match self
            .api
            .send_message_with_reply(chat_id, html, Some("HTML"), reply_to_message_id)
            .await
        {
            Ok(msg) => Ok(msg.message_id),
            Err(TelegramApiError::TooManyRequests { retry_after }) => {
                tokio::time::sleep(retry_after).await;
                self.api
                    .send_message_with_reply(chat_id, html, Some("HTML"), reply_to_message_id)
                    .await
                    .map(|m| m.message_id)
                    .map_err(map_err)
            }
            Err(TelegramApiError::TransportConnect(d)) => {
                log::warn!("[telegram-sender] connect failed: {d}, retrying once");
                tokio::time::sleep(Duration::from_millis(500)).await;
                self.api
                    .send_message_with_reply(chat_id, html, Some("HTML"), reply_to_message_id)
                    .await
                    .map(|m| m.message_id)
                    .map_err(map_err)
            }
            Err(TelegramApiError::BadRequest(desc)) if is_parse_error(&desc) => {
                Err(SenderError::Transport(format!("parse error: {desc}")))
            }
            Err(TelegramApiError::BadRequest(desc))
                if desc.to_lowercase().contains("replied message not found") =>
            {
                // 用户删了原消息 → 不带 reply 再发一次
                log::warn!("[telegram-sender] replied message not found, retry without reply");
                self.api
                    .send_message_with_reply(chat_id, html, Some("HTML"), None)
                    .await
                    .map(|m| m.message_id)
                    .map_err(map_err)
            }
            Err(TelegramApiError::BadRequest(desc)) => {
                Err(SenderError::Transport(format!("bad request: {desc}")))
            }
            Err(e) => Err(map_err(e)),
        }
    }

    /// 纯文本发送（pairing 提示语 / 欢迎语用）。
    pub async fn send_plain(&self, chat_id: i64, text: &str) -> Result<(), SenderError> {
        match self.api.send_message(chat_id, text, None).await {
            Ok(_) => Ok(()),
            Err(TelegramApiError::TooManyRequests { retry_after }) => {
                tokio::time::sleep(retry_after).await;
                self.api
                    .send_message(chat_id, text, None)
                    .await
                    .map(|_| ())
                    .map_err(map_err)
            }
            Err(e) => Err(map_err(e)),
        }
    }

    /// 给 long_poll task 复制一份共享同一个 `Arc<TelegramApi>` 的 sender 实例。
    pub fn clone_inner(&self) -> TelegramSender {
        TelegramSender {
            api: self.api.clone(),
        }
    }
}

fn map_err(e: TelegramApiError) -> SenderError {
    match e {
        TelegramApiError::Unauthorized(d) => SenderError::Unauthorized(d),
        TelegramApiError::Forbidden(d) => SenderError::Forbidden(d),
        other => SenderError::Transport(other.to_string()),
    }
}

fn is_parse_error(desc: &str) -> bool {
    let lower = desc.to_ascii_lowercase();
    lower.contains("parse")
        || lower.contains("entity")
        || lower.contains("html")
        || lower.contains("tag")
}

/// HTML escape 三件套：先转义 `&`，再转 `<` `>`，避免后续 markdown→HTML 转换
/// 注入的合法 tag 被反向当成文本。
fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
    out
}

/// 把 GFM markdown 子集转成 Telegram HTML。覆盖：
/// - 代码块 ```...``` → <pre><code>...</code></pre>
/// - 行内代码 `code` → <code>code</code>
/// - 粗体 **bold** / __bold__ → <b>bold</b>
/// - 斜体 *italic* / _italic_ → <i>italic</i>
/// - 删除线 ~~strike~~ → <s>strike</s>
/// - 链接 [text](url) → <a href="url">text</a>
/// - 标题 # H1 / ## H2 / ### H3 → <b>H</b>
/// - 列表 `- item` / `* item` / `1. item` → 文本保持，前缀转成 `• ` / `N. `
///
/// 不支持嵌套链接、HTML 透传（用户输入的 `<` 一律 escape）。
/// 解析顺序：先吃掉代码块（避免内部标记被误处理）→ 再 escape 普通段落 → 应用其它规则。
pub fn markdown_to_telegram_html(markdown: &str) -> String {
    // Phase 1: 把 fenced code blocks 拿出来用 placeholder 占位
    let (without_blocks, code_blocks) = extract_fenced_code_blocks(markdown);
    // Phase 2: 把 inline code 拿出来用 placeholder 占位
    let (without_inline, inline_codes) = extract_inline_code(&without_blocks);
    // Phase 3: 把普通文本 HTML escape
    let mut text = html_escape(&without_inline);
    // Phase 4: 行级处理（标题 / 列表前缀）
    text = transform_lines(&text);
    // Phase 5: inline 标记替换
    text = apply_inline_markup(&text);
    // Phase 6: placeholder 替换回真正的 <pre><code>...</code></pre> / <code>...</code>
    for (i, code) in code_blocks.iter().enumerate() {
        let escaped = html_escape(code);
        let lang = ""; // 暂不支持语言标记
        let wrapped = if lang.is_empty() {
            format!("<pre><code>{}</code></pre>", escaped)
        } else {
            format!(
                "<pre><code class=\"language-{}\">{}</code></pre>",
                lang, escaped
            )
        };
        text = text.replace(&format!("\u{E000}CB{i}\u{E001}"), &wrapped);
    }
    for (i, code) in inline_codes.iter().enumerate() {
        let escaped = html_escape(code);
        text = text.replace(
            &format!("\u{E000}IC{i}\u{E001}"),
            &format!("<code>{}</code>", escaped),
        );
    }
    text
}

/// 把已转好的 Telegram HTML 按 max_bytes 上限切成多片，尽量保留语义边界。
///
/// 切分优先级：
/// 1. `<pre><code>...</code></pre>` 代码块视为原子；单块超 max_bytes 则强切并各自外包
/// 2. 双换行（段落）
/// 3. 单换行（行）
/// 4. 字符兜底（utf-8 边界）
pub fn split_telegram_html(input: &str, max_bytes: usize) -> Vec<String> {
    if input.len() <= max_bytes {
        return vec![input.to_string()];
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    let segments = split_by_code_blocks(input);
    for seg in segments {
        match seg {
            Segment::CodeBlock(text) => {
                if !current.is_empty() && current.len() + text.len() > max_bytes {
                    chunks.push(std::mem::take(&mut current));
                }
                if text.len() > max_bytes {
                    if !current.is_empty() {
                        chunks.push(std::mem::take(&mut current));
                    }
                    chunks.extend(force_split_code_block(&text, max_bytes));
                } else {
                    current.push_str(&text);
                }
            }
            Segment::Text(text) => {
                push_text_with_paragraph_split(&text, max_bytes, &mut chunks, &mut current);
            }
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

enum Segment {
    CodeBlock(String),
    Text(String),
}

fn split_by_code_blocks(input: &str) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let open = "<pre><code";
    let close = "</code></pre>";
    let mut remaining = input;
    while let Some(open_idx) = remaining.find(open) {
        if open_idx > 0 {
            out.push(Segment::Text(remaining[..open_idx].to_string()));
        }
        let tail = &remaining[open_idx..];
        if let Some(close_idx) = tail.find(close) {
            let block_end = close_idx + close.len();
            out.push(Segment::CodeBlock(tail[..block_end].to_string()));
            remaining = &tail[block_end..];
        } else {
            out.push(Segment::Text(tail.to_string()));
            return out;
        }
    }
    if !remaining.is_empty() {
        out.push(Segment::Text(remaining.to_string()));
    }
    out
}

fn push_text_with_paragraph_split(
    text: &str,
    max_bytes: usize,
    chunks: &mut Vec<String>,
    current: &mut String,
) {
    for para in text.split("\n\n") {
        let to_add = if current.is_empty() {
            para.to_string()
        } else {
            format!("\n\n{para}")
        };
        if current.len() + to_add.len() <= max_bytes {
            current.push_str(&to_add);
            continue;
        }
        if !current.is_empty() {
            chunks.push(std::mem::take(current));
        }
        if para.len() > max_bytes {
            for line_chunk in split_by_lines(para, max_bytes) {
                if line_chunk.len() > max_bytes {
                    chunks.extend(force_split_chars(&line_chunk, max_bytes));
                } else {
                    chunks.push(line_chunk);
                }
            }
        } else {
            *current = para.to_string();
        }
    }
}

fn split_by_lines(text: &str, max_bytes: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in text.split('\n') {
        let to_add = if cur.is_empty() {
            line.to_string()
        } else {
            format!("\n{line}")
        };
        if cur.len() + to_add.len() <= max_bytes {
            cur.push_str(&to_add);
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            cur = line.to_string();
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn force_split_chars(text: &str, max_bytes: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if cur.len() + ch.len_utf8() > max_bytes && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(ch);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn force_split_code_block(block: &str, max_bytes: usize) -> Vec<String> {
    let open = "<pre><code>";
    let close = "</code></pre>";
    // split_by_code_blocks matches "<pre><code" (without '>') so we may receive
    // `<pre><code class="language-rust">...` — strip up to the first '>' to find
    // the inner start regardless of attribute.
    let inner = if let Some(stripped_close) = block.strip_suffix(close) {
        if let Some(open_attr_start) = stripped_close.find("<pre><code") {
            let after_open_tag = &stripped_close[open_attr_start..];
            after_open_tag
                .find('>')
                .map(|gt| &after_open_tag[gt + 1..])
                .unwrap_or(stripped_close)
        } else {
            stripped_close
        }
    } else {
        block.strip_prefix(open).unwrap_or(block)
    };
    let wrap_overhead = open.len() + close.len();
    let inner_max = max_bytes.saturating_sub(wrap_overhead).max(1);
    let inner_chunks = split_by_lines(inner, inner_max);
    inner_chunks
        .into_iter()
        .map(|c| format!("{open}{c}{close}"))
        .collect()
}

fn extract_fenced_code_blocks(input: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(input.len());
    let mut blocks: Vec<String> = Vec::new();
    let mut remaining = input;
    while let Some(start) = remaining.find("```") {
        out.push_str(&remaining[..start]);
        // skip past opening fence and the rest of the opening line (lang tag etc.)
        let after_open = &remaining[start + 3..];
        let body_start = after_open.find('\n').map(|n| n + 1).unwrap_or(0);
        let body = &after_open[body_start..];
        if let Some(end) = body.find("```") {
            let mut content = body[..end].to_string();
            // trim trailing newline before closing fence
            if content.ends_with('\n') {
                content.pop();
            }
            out.push_str(&format!("\u{E000}CB{}\u{E001}", blocks.len()));
            blocks.push(content);
            remaining = &body[end + 3..];
        } else {
            // no closing fence — treat as plain text
            out.push_str(&remaining[start..]);
            return (out, blocks);
        }
    }
    out.push_str(remaining);
    (out, blocks)
}

fn extract_inline_code(input: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(input.len());
    let mut codes: Vec<String> = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            // find matching `
            if let Some(end) = input[i + 1..].find('`') {
                let content = &input[i + 1..i + 1 + end];
                out.push_str(&format!("\u{E000}IC{}\u{E001}", codes.len()));
                codes.push(content.to_string());
                i = i + 1 + end + 1;
                continue;
            }
        }
        // safe because we only advance i on full-char boundaries via str indexing below
        let ch_end = next_char_boundary(input, i);
        out.push_str(&input[i..ch_end]);
        i = ch_end;
    }
    (out, codes)
}

fn next_char_boundary(s: &str, mut i: usize) -> usize {
    i += 1;
    while !s.is_char_boundary(i) && i < s.len() {
        i += 1;
    }
    i
}

fn transform_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut first = true;
    for line in text.split('\n') {
        if !first {
            out.push('\n');
        }
        first = false;
        let trimmed = line.trim_start();
        // headers
        if let Some(rest) = trimmed.strip_prefix("### ") {
            out.push_str(&format!("<b>{}</b>", rest));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            out.push_str(&format!("<b>{}</b>", rest));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            out.push_str(&format!("<b>{}</b>", rest));
            continue;
        }
        // bullet lists
        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            let indent = line.len() - trimmed.len();
            out.push_str(&" ".repeat(indent));
            out.push_str("• ");
            out.push_str(rest);
            continue;
        }
        // numeric lists: leave as-is ("1. text" stays "1. text"; readable enough)
        out.push_str(line);
    }
    out
}

fn apply_inline_markup(text: &str) -> String {
    let mut s = text.to_string();
    // Order: bold first (so we don't accidentally match a single asterisk inside **)
    s = replace_pair(&s, "**", "<b>", "</b>");
    s = replace_pair(&s, "__", "<b>", "</b>");
    s = replace_pair(&s, "~~", "<s>", "</s>");
    s = replace_pair(&s, "*", "<i>", "</i>");
    s = replace_pair(&s, "_", "<i>", "</i>");
    s = replace_links(&s);
    s
}

/// Replace pairs of `delim` with `open`/`close` tags. Matches non-greedily; if
/// no closing delim is found, leaves the text as-is. Skips delims that look
/// like they're inside already-tagged content (rare, but `<b>**foo**</b>` becomes
/// `<b><b>foo</b></b>` which Telegram will reject — we'd rather drop the inner
/// markers, but for v1 we tolerate redundant tags).
fn replace_pair(input: &str, delim: &str, open: &str, close: &str) -> String {
    let dl = delim.len();
    let mut out = String::with_capacity(input.len());
    let mut remaining = input;
    while let Some(start) = remaining.find(delim) {
        // For single-char delim ('*' or '_'), require it to be a "word boundary"
        // so that `snake_case_var` doesn't get split.
        if dl == 1 {
            let prev_is_alnum = start > 0
                && remaining[..start]
                    .chars()
                    .last()
                    .map(|c| c.is_alphanumeric() || c == '_' || c == '*')
                    .unwrap_or(false);
            if prev_is_alnum {
                // skip this delim
                let end = start + dl;
                out.push_str(&remaining[..end]);
                remaining = &remaining[end..];
                continue;
            }
        }
        let after = &remaining[start + dl..];
        if let Some(end) = after.find(delim) {
            // For single-char delim, also require the closing delim NOT be followed by alnum.
            if dl == 1 {
                let after_close_byte = start + dl + end + dl;
                let next_char_is_alnum = after_close_byte < remaining.len()
                    && remaining[after_close_byte..]
                        .chars()
                        .next()
                        .map(|c| c.is_alphanumeric() || c == '_')
                        .unwrap_or(false);
                if next_char_is_alnum {
                    let stop = start + dl;
                    out.push_str(&remaining[..stop]);
                    remaining = &remaining[stop..];
                    continue;
                }
            }
            out.push_str(&remaining[..start]);
            out.push_str(open);
            out.push_str(&after[..end]);
            out.push_str(close);
            remaining = &after[end + dl..];
        } else {
            // no closing — emit rest as plain
            out.push_str(remaining);
            return out;
        }
    }
    out.push_str(remaining);
    out
}

/// `[text](url)` → `<a href="url">text</a>`. URL inside is not validated; Telegram
/// will reject malformed URLs with a 400 → plain text fallback path.
fn replace_links(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(close_text) = input[i + 1..].find(']') {
                let after_text = i + 1 + close_text + 1;
                if after_text < input.len() && bytes[after_text] == b'(' {
                    if let Some(close_url) = input[after_text + 1..].find(')') {
                        let text = &input[i + 1..i + 1 + close_text];
                        let url = &input[after_text + 1..after_text + 1 + close_url];
                        // Only accept http(s)/tg/mailto URLs — anything else
                        // (`javascript:` etc.) gets stripped to text only.
                        if url.starts_with("http://")
                            || url.starts_with("https://")
                            || url.starts_with("tg://")
                            || url.starts_with("mailto:")
                        {
                            out.push_str(&format!("<a href=\"{}\">{}</a>", url, text));
                            i = after_text + 1 + close_url + 1;
                            continue;
                        }
                    }
                }
            }
        }
        let next = next_char_boundary(input, i);
        out.push_str(&input[i..next]);
        i = next;
    }
    out
}

/// Best-effort fallback when HTML parse fails: strip the most common markdown
/// markers so the user at least sees readable text.
fn strip_markdown(text: &str) -> String {
    let mut s = text.to_string();
    s = s.replace("**", "").replace("__", "").replace("~~", "");
    // single * / _ : leave as-is (likely punctuation or word internal)
    s
}

/// 一个从 markdown 文本中提取出来的本地附件引用。
#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentRef {
    /// 已展开的绝对路径（存在于文件系统）。
    pub absolute_path: std::path::PathBuf,
    /// 在原始 markdown 中对应的完整片段（用于替换为 `📎 label`）。
    pub original_segment: String,
    /// 展示给用户的标签（markdown 链接的 text 部分，或文件名）。
    pub display_label: String,
}

/// 扫描 markdown 源文本，提取所有指向本地文件系统中存在文件的路径。
///
/// 支持两种形式：
/// 1. markdown 链接 `[label](path)` 其中 path 是本地绝对路径
/// 2. 裸绝对路径（如 `/Users/foo/bar.xlsx` 或 `C:\Users\foo\bar.xlsx`）
///
/// 返回的 Vec 保证：每个 `absolute_path` 唯一（去重），路径在文件系统中存在。
pub fn extract_local_paths(markdown: &str) -> Vec<AttachmentRef> {
    let mut result: Vec<AttachmentRef> = Vec::new();
    let mut seen: std::collections::HashSet<std::path::PathBuf> = std::collections::HashSet::new();

    // Phase 1: 扫描 markdown 链接形式 [label](path)
    let mut i = 0;
    let bytes = markdown.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(close_text) = markdown[i + 1..].find(']') {
                let after_text = i + 1 + close_text + 1;
                if after_text < markdown.len() && bytes[after_text] == b'(' {
                    if let Some(close_url) = markdown[after_text + 1..].find(')') {
                        let label = &markdown[i + 1..i + 1 + close_text];
                        let path_str = &markdown[after_text + 1..after_text + 1 + close_url];
                        let segment = &markdown[i..after_text + 1 + close_url + 1];
                        if let Some(abs) = resolve_local_path(path_str) {
                            if !seen.contains(&abs) {
                                seen.insert(abs.clone());
                                result.push(AttachmentRef {
                                    absolute_path: abs,
                                    original_segment: segment.to_string(),
                                    display_label: if label.is_empty() {
                                        path_str
                                            .split(['/', '\\'])
                                            .next_back()
                                            .unwrap_or(path_str)
                                            .to_string()
                                    } else {
                                        label.to_string()
                                    },
                                });
                            }
                        }
                        i = after_text + 1 + close_url + 1;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }

    // Phase 2: 扫描裸路径（行中独立存在的绝对路径）
    for line in markdown.lines() {
        let trimmed = line.trim();
        // 快速排除非路径行
        if !is_absolute_path(trimmed) {
            continue;
        }
        // 取路径部分（到第一个空白前结束）
        let path_str = trimmed.split_whitespace().next().unwrap_or(trimmed);
        if let Some(abs) = resolve_local_path(path_str) {
            // 确认整行（trim 后）就是这个路径（避免误匹配 URL 里的路径部分）
            if !seen.contains(&abs) {
                seen.insert(abs.clone());
                let file_name = abs
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path_str)
                    .to_string();
                result.push(AttachmentRef {
                    absolute_path: abs,
                    original_segment: path_str.to_string(),
                    display_label: file_name,
                });
            }
        }
    }

    result
}

/// 判断一个字符串看起来是否是绝对路径（不做文件系统检查）。
fn is_absolute_path(s: &str) -> bool {
    if s.starts_with('/') {
        return true;
    }
    // Windows 卷根：`C:\` or `C:/`
    if s.len() >= 3 {
        let mut chars = s.chars();
        let c1 = chars.next().unwrap_or(' ');
        let c2 = chars.next().unwrap_or(' ');
        let c3 = chars.next().unwrap_or(' ');
        if c1.is_ascii_alphabetic() && c2 == ':' && (c3 == '\\' || c3 == '/') {
            return true;
        }
    }
    false
}

/// 尝试把路径字符串解析为存在于文件系统的绝对路径。
/// 支持 `~/` 展开；不接受 URL scheme（http/https/tg/mailto）。
fn resolve_local_path(path_str: &str) -> Option<std::path::PathBuf> {
    // 排除已知 scheme
    if path_str.starts_with("http://")
        || path_str.starts_with("https://")
        || path_str.starts_with("tg://")
        || path_str.starts_with("mailto:")
    {
        return None;
    }
    // 展开 ~/
    let expanded: std::path::PathBuf = if let Some(rest) = path_str.strip_prefix("~/") {
        dirs::home_dir()?.join(rest)
    } else {
        std::path::PathBuf::from(path_str)
    };
    // 必须是绝对路径 + 文件系统存在
    if !expanded.is_absolute() {
        return None;
    }
    if expanded.exists() {
        Some(expanded)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_unchanged() {
        assert_eq!(markdown_to_telegram_html("hello world"), "hello world");
        assert_eq!(markdown_to_telegram_html("你好 emoji 😀"), "你好 emoji 😀");
    }

    #[test]
    fn html_special_chars_escaped() {
        assert_eq!(
            markdown_to_telegram_html("1 < 2 && 3 > 1"),
            "1 &lt; 2 &amp;&amp; 3 &gt; 1"
        );
    }

    #[test]
    fn bold_double_star() {
        assert_eq!(markdown_to_telegram_html("**bold**"), "<b>bold</b>");
        assert_eq!(
            markdown_to_telegram_html("hello **world** end"),
            "hello <b>world</b> end"
        );
    }

    #[test]
    fn italic_single_star_with_word_boundary() {
        assert_eq!(markdown_to_telegram_html("*italic*"), "<i>italic</i>");
        // `snake_case_var` 不应被切碎
        assert_eq!(
            markdown_to_telegram_html("snake_case_var"),
            "snake_case_var"
        );
    }

    #[test]
    fn inline_code() {
        assert_eq!(
            markdown_to_telegram_html("call `foo()` here"),
            "call <code>foo()</code> here"
        );
        // 内部的 `**` 不应被解析为 bold
        assert_eq!(
            markdown_to_telegram_html("use `**raw**` here"),
            "use <code>**raw**</code> here"
        );
    }

    #[test]
    fn fenced_code_block() {
        let input = "```\nfn main() {\n    println!(\"hi\");\n}\n```";
        let expected = "<pre><code>fn main() {\n    println!(\"hi\");\n}</code></pre>";
        assert_eq!(markdown_to_telegram_html(input), expected);
    }

    #[test]
    fn fenced_code_block_with_lang_tag_strips_lang() {
        let input = "```rust\nlet x = 1;\n```";
        let expected = "<pre><code>let x = 1;</code></pre>";
        assert_eq!(markdown_to_telegram_html(input), expected);
    }

    #[test]
    fn link_http() {
        assert_eq!(
            markdown_to_telegram_html("see [docs](https://example.com) for details"),
            "see <a href=\"https://example.com\">docs</a> for details"
        );
    }

    #[test]
    fn link_javascript_stripped() {
        // javascript: URLs should NOT be rendered as link; leave bare
        let out = markdown_to_telegram_html("[bad](javascript:alert(1))");
        assert!(
            !out.contains("<a"),
            "javascript: link should be dropped: {out}"
        );
    }

    #[test]
    fn header_to_bold() {
        assert_eq!(markdown_to_telegram_html("# Title"), "<b>Title</b>");
        assert_eq!(markdown_to_telegram_html("## Subtitle"), "<b>Subtitle</b>");
    }

    #[test]
    fn bullet_list_prefix() {
        let input = "- first\n- second";
        let expected = "• first\n• second";
        assert_eq!(markdown_to_telegram_html(input), expected);
    }

    #[test]
    fn strikethrough() {
        assert_eq!(markdown_to_telegram_html("~~old~~"), "<s>old</s>");
    }

    #[test]
    fn mixed_markdown_kitchen_sink() {
        let input = "**bold** and *italic* with `code` and [link](https://x.com)";
        let expected = "<b>bold</b> and <i>italic</i> with <code>code</code> and <a href=\"https://x.com\">link</a>";
        assert_eq!(markdown_to_telegram_html(input), expected);
    }

    #[test]
    fn unmatched_delim_left_alone() {
        // `*foo` with no closing star should not produce a stray <i>
        assert_eq!(
            markdown_to_telegram_html("*foo without close"),
            "*foo without close"
        );
    }

    #[test]
    fn html_escape_inside_code_preserves_content() {
        assert_eq!(
            markdown_to_telegram_html("`<script>alert(1)</script>`"),
            "<code>&lt;script&gt;alert(1)&lt;/script&gt;</code>"
        );
    }

    #[test]
    fn strip_markdown_basic() {
        assert_eq!(strip_markdown("**bold** ~~strike~~"), "bold strike");
    }

    mod split_tests {
        use super::*;

        #[test]
        fn short_input_returns_single_chunk() {
            assert_eq!(split_telegram_html("hello", 4000), vec!["hello"]);
        }

        #[test]
        fn long_text_splits_on_double_newline() {
            let para = "a".repeat(1500);
            let input = format!("{para}\n\n{para}\n\n{para}\n\n{para}");
            let chunks = split_telegram_html(&input, 4000);
            assert!(chunks.len() >= 2);
            for c in &chunks {
                assert!(c.len() <= 4000);
            }
        }

        #[test]
        fn chinese_multibyte_never_cuts_inside_codepoint() {
            let chinese: String = "中".repeat(1500);
            let chunks = split_telegram_html(&chinese, 4000);
            assert!(chunks.len() >= 2);
            for c in &chunks {
                assert_eq!(c.len() % 3, 0);
            }
        }

        #[test]
        fn code_block_stays_intact_when_under_limit() {
            let prelude = "a".repeat(2000);
            let block = "<pre><code>fn main() {}</code></pre>";
            let suffix = "b".repeat(2000);
            let input = format!("{prelude}\n\n{block}\n\n{suffix}");
            let chunks = split_telegram_html(&input, 4000);
            let count_with_block = chunks
                .iter()
                .filter(|c| c.contains("<pre><code>fn main()"))
                .count();
            assert_eq!(count_with_block, 1);
        }

        #[test]
        fn oversized_code_block_is_force_split_and_rewrapped() {
            let inner_lines: String = (0..200)
                .map(|i| format!("line_{i}_with_some_content_to_fill_bytes"))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(inner_lines.len() > 4000);
            let block = format!("<pre><code>{inner_lines}</code></pre>");
            let chunks = split_telegram_html(&block, 4000);
            assert!(chunks.len() >= 2);
            for c in &chunks {
                assert!(c.starts_with("<pre><code>"));
                assert!(c.ends_with("</code></pre>"));
                assert!(c.len() <= 4000);
            }
        }

        #[test]
        fn oversized_code_block_with_lang_attr_strips_open_tag_correctly() {
            // 即便未来 markdown_to_telegram_html 启用 language-* class，
            // force_split_code_block 不会把 <pre><code class="..."> 也当成 inner 内容
            // 导致输出被双重 wrap。
            let inner_lines: String = (0..200)
                .map(|i| format!("line_{i}_with_some_content_to_fill_bytes"))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(inner_lines.len() > 4000);
            let block = format!("<pre><code class=\"language-rust\">{inner_lines}</code></pre>");
            let chunks = split_telegram_html(&block, 4000);
            assert!(chunks.len() >= 2);
            for c in &chunks {
                // 重新包装后只有一层 <pre><code>，inner 不应再含 class="
                assert!(c.starts_with("<pre><code>"));
                assert!(c.ends_with("</code></pre>"));
                assert!(
                    !c[11..c.len() - 13].contains("<pre><code"),
                    "inner contains nested <pre><code, double-wrapped: {c}"
                );
            }
        }

        #[test]
        fn empty_input_returns_single_empty_chunk() {
            assert_eq!(split_telegram_html("", 4000), vec![""]);
        }
    }

    mod extract_path_tests {
        use super::*;

        #[test]
        fn markdown_link_with_existing_file_is_extracted() {
            let dir = tempfile::TempDir::new().unwrap();
            let file = dir.path().join("report.xlsx");
            std::fs::write(&file, b"data").unwrap();
            let path_str = file.to_str().unwrap();
            let markdown = format!("[报告]({path_str})");
            let refs = extract_local_paths(&markdown);
            assert_eq!(refs.len(), 1);
            // Compare via canonicalize to handle macOS /var → /private/var symlink
            let expected = file.canonicalize().unwrap_or_else(|_| file.clone());
            let actual = refs[0]
                .absolute_path
                .canonicalize()
                .unwrap_or_else(|_| refs[0].absolute_path.clone());
            assert_eq!(actual, expected);
            assert_eq!(refs[0].display_label, "报告");
            assert_eq!(refs[0].original_segment, markdown);
        }

        #[test]
        fn bare_absolute_path_is_extracted() {
            let dir = tempfile::TempDir::new().unwrap();
            let file = dir.path().join("data.csv");
            std::fs::write(&file, b"a,b,c").unwrap();
            let path_str = file.to_str().unwrap().to_string();
            let markdown = format!("请查看文件\n{path_str}\n谢谢");
            let refs = extract_local_paths(&markdown);
            assert_eq!(refs.len(), 1, "bare path should be extracted: {refs:?}");
            assert_eq!(refs[0].display_label, "data.csv");
        }

        #[test]
        fn nonexistent_path_is_not_extracted() {
            let refs = extract_local_paths("[missing](/nonexistent/missing_file_xyz.xlsx)");
            assert_eq!(refs.len(), 0);
        }

        #[test]
        fn http_url_is_not_extracted() {
            let refs = extract_local_paths("[报告](https://example.com/report.xlsx)");
            assert_eq!(refs.len(), 0);
        }

        #[test]
        fn tilde_home_is_expanded() {
            // ~/ 展开到 home dir；如果 home 目录里没有 nonexistent_test_file.xyz 就 OK
            let refs = extract_local_paths("[x](~/definitely_nonexistent_test_file_abc123.xyz)");
            assert_eq!(
                refs.len(),
                0,
                "nonexistent tilde path should not be extracted"
            );
        }

        #[test]
        fn duplicate_paths_deduplicated() {
            let dir = tempfile::TempDir::new().unwrap();
            let file = dir.path().join("dup.txt");
            std::fs::write(&file, b"dup").unwrap();
            let path_str = file.to_str().unwrap();
            let markdown = format!("[A]({path_str}) and [B]({path_str})");
            let refs = extract_local_paths(&markdown);
            assert_eq!(refs.len(), 1, "duplicate paths should be deduplicated");
        }
    }

    mod send_with_attachment_tests {
        use super::*;
        use std::sync::Arc;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        #[tokio::test]
        async fn markdown_with_local_path_triggers_both_text_and_document() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/botT/sendMessage"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "ok": true,
                    "result": { "message_id": 100, "chat": { "id": 1, "type": "private" } }
                })))
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path("/botT/sendDocument"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "ok": true,
                    "result": { "message_id": 101, "chat": { "id": 1, "type": "private" } }
                })))
                .mount(&server)
                .await;

            let tmp = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(tmp.path(), b"hello attachment").unwrap();
            let path_str = tmp.path().to_string_lossy().to_string();

            let api = Arc::new(
                super::super::super::api::TelegramApi::new_with_api_base_for_tests(
                    "T".into(),
                    server.uri(),
                )
                .unwrap(),
            );
            let sender = TelegramSender::new(api);
            let markdown = format!("详见 [报告]({path_str}) 内容");
            sender.send_markdown(1, &markdown).await.unwrap();

            let calls = server.received_requests().await.unwrap();
            let send_msg = calls
                .iter()
                .filter(|r| r.url.path().ends_with("sendMessage"))
                .count();
            let send_doc = calls
                .iter()
                .filter(|r| r.url.path().ends_with("sendDocument"))
                .count();
            assert_eq!(send_msg, 1, "expected 1 sendMessage call, got {send_msg}");
            assert_eq!(send_doc, 1, "expected 1 sendDocument call, got {send_doc}");
        }
    }
}
