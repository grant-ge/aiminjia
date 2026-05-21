//! Markdown → WhatsApp 受限格式。spec v3 §5.2。
//!
//! WhatsApp 文本格式仅支持：
//! - `*粗体*`（单星，不是双星）
//! - `_斜体_`（下划线，不是单星）
//! - `~删除~`（波浪号）—— 本 strip 不主动转换 markdown 删除线（不在 spec 表里）
//!
//! 8 行规则（spec §5.2 钉死，禁改）：
//! | `**粗体**`            → `*粗体*`     |
//! | `*斜体*` / `_斜体_`   → `_斜体_`     |
//! | `# 标题` / `## ...`   → 去前缀 `#+ ` |
//! | `` `code` ``          → `code`       |
//! | ` ```block``` `       → `block` 内容（保留换行）|
//! | `[link](url)`         → `link (url)` |
//! | `> 引用`              → 引用文字     |
//! | `- list` / `1. list`  → `• list`     |

/// 把 markdown 文本规整为 WhatsApp 可识别的最小子集。
pub fn strip_to_wa(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_fence = false;

    for line in input.split_inclusive('\n') {
        // 行尾的 \n 单独处理，主体走 trimmed
        let (body, has_nl) = match line.strip_suffix('\n') {
            Some(b) => (b, true),
            None => (line, false),
        };

        // ``` fence 边界
        let trimmed = body.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            // fence 那行整行去掉（不输出 ``` 标记 / 不输出 lang 标识）
            if has_nl {
                // consume the newline by not pushing it, maintaining correct line count
            }
            continue;
        }
        if in_fence {
            // fence 内：原样输出（带行尾换行如有）
            out.push_str(body);
            if has_nl {
                out.push('\n');
            }
            continue;
        }

        // 行级前缀：标题 / 引用 / 列表
        let stripped = strip_line_prefix(body);

        // inline 替换
        let inlined = strip_inline(&stripped);
        out.push_str(&inlined);
        if has_nl {
            out.push('\n');
        }
    }
    out
}

fn strip_line_prefix(line: &str) -> String {
    let trimmed = line.trim_start();
    let leading_ws = &line[..line.len() - trimmed.len()];

    // 标题 # / ## / ### …
    if let Some(after) = trimmed.strip_prefix('#') {
        let mut rest = after;
        while let Some(r) = rest.strip_prefix('#') {
            rest = r;
        }
        if let Some(after_space) = rest.strip_prefix(' ') {
            return format!("{leading_ws}{after_space}");
        }
        // `#word` 没空格不当标题（保持原样防误吃 hashtag）
        return line.to_string();
    }
    // 引用 `> text`
    if let Some(after) = trimmed.strip_prefix("> ") {
        return format!("{leading_ws}{after}");
    }
    if trimmed == ">" {
        return leading_ws.to_string();
    }
    // 无序列表：`- ` / `* ` / `+ `
    for marker in ["- ", "* ", "+ "] {
        if let Some(after) = trimmed.strip_prefix(marker) {
            return format!("{leading_ws}• {after}");
        }
    }
    // 有序列表：`1. ` / `12. ` …
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i + 2 <= bytes.len() && bytes[i] == b'.' && bytes[i + 1] == b' ' {
        let after = &trimmed[i + 2..];
        return format!("{leading_ws}• {after}");
    }
    line.to_string()
}

fn strip_inline(line: &str) -> String {
    let mut s = line.to_string();
    // 1. 反引号代码 `code`：去反引号（不嵌套，简单 first-match）
    s = replace_pairs(&s, '`', '`', |inner| inner.to_string());
    // 2. 链接 [text](url) → "text (url)"
    s = strip_links(&s);
    // 3. 粗体/斜体在一个 pass 处理，避免双星被先转成单星后被 italic pass 吃掉。
    //    **x** → *x*（WA 粗体）；*x* → _x_（WA 斜体）
    s = strip_bold_and_italic(&s);
    // 4. _x_ 已经是目标形态，保留不动
    s
}

/// 在一个左到右 pass 中处理 `**bold**` 和 `*italic*`：
/// - `**` 开头 → 找下一个 `**` 闭合 → 输出 `*inner*`
/// - 单 `*` 开头 → 找下一个单 `*` 闭合 → 输出 `_inner_`
///
/// 已知边界：嵌套 `**bold *italic* end**` 产生 `*bold _italic_ end*`
/// （简单贪婪，spec §5.2 没有 nested rule，可接受）。
fn strip_bold_and_italic(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '*' {
            if i + 1 < chars.len() && chars[i + 1] == '*' {
                // 双星：尝试找闭合 **
                if let Some(close) = find_double_star(&chars, i + 2) {
                    // **inner** → *inner*
                    out.push('*');
                    // 递归处理 inner（可能含斜体）
                    let inner: String = chars[i + 2..close].iter().collect();
                    out.push_str(&strip_bold_and_italic(&inner));
                    out.push('*');
                    i = close + 2;
                    continue;
                } else {
                    // 未闭合双星：原样输出
                    out.push('*');
                    out.push('*');
                    i += 2;
                    continue;
                }
            } else {
                // 单星：尝试找闭合 *（非双星）
                if let Some(close) = find_single_star(&chars, i + 1) {
                    // *inner* → _inner_
                    out.push('_');
                    let inner: String = chars[i + 1..close].iter().collect();
                    out.push_str(&inner);
                    out.push('_');
                    i = close + 1;
                    continue;
                } else {
                    // 未闭合单星：原样输出
                    out.push('*');
                    i += 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// 找从 `start` 起第一个不是双星开头的 `*`（单星闭合符）。
/// 跳过 `**`（双星）。
fn find_single_star(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    while i < chars.len() {
        if chars[i] == '*' {
            if i + 1 < chars.len() && chars[i + 1] == '*' {
                // 跳过 **（双星不是单星闭合符）
                i += 2;
                continue;
            }
            return Some(i);
        }
        i += 1;
    }
    None
}

fn find_double_star(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 1 < chars.len() {
        if chars[i] == '*' && chars[i + 1] == '*' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn replace_pairs<F>(s: &str, open: char, close: char, transform: F) -> String
where
    F: Fn(&str) -> String,
{
    let mut out = String::with_capacity(s.len());
    let mut buf = String::new();
    let mut in_pair = false;
    for c in s.chars() {
        if !in_pair && c == open {
            in_pair = true;
            buf.clear();
        } else if in_pair && c == close {
            out.push_str(&transform(&buf));
            in_pair = false;
        } else if in_pair {
            buf.push(c);
        } else {
            out.push(c);
        }
    }
    if in_pair {
        // 不闭合：原样吐 open + buf
        out.push(open);
        out.push_str(&buf);
    }
    out
}

fn strip_links(s: &str) -> String {
    // 简单 [text](url) 扫描；不处理 nested。
    // 使用字节索引定位 ASCII 标记，但始终切 UTF-8 slice 拼接，避免多字节字符错误。
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(rb) = find_byte(bytes, i + 1, b']') {
                if rb + 1 < bytes.len() && bytes[rb + 1] == b'(' {
                    if let Some(rp) = find_byte(bytes, rb + 2, b')') {
                        let text = &s[i + 1..rb];
                        let url = &s[rb + 2..rp];
                        out.push_str(text);
                        out.push(' ');
                        out.push('(');
                        out.push_str(url);
                        out.push(')');
                        i = rp + 1;
                        continue;
                    }
                }
            }
        }
        // 安全推进：找到下一个 UTF-8 字符边界，整块追加，避免多字节撕裂。
        let ch_len = utf8_char_len(bytes[i]);
        if i + ch_len <= bytes.len() {
            out.push_str(&s[i..i + ch_len]);
            i += ch_len;
        } else {
            // 残损字节（理论上不应出现）：原样推进
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// 返回以 `first_byte` 开头的 UTF-8 序列长度（1-4 字节）。
fn utf8_char_len(first_byte: u8) -> usize {
    if first_byte < 0x80 {
        1
    } else if first_byte < 0xE0 {
        2
    } else if first_byte < 0xF0 {
        3
    } else {
        4
    }
}

fn find_byte(bytes: &[u8], start: usize, b: u8) -> Option<usize> {
    bytes[start..]
        .iter()
        .position(|&x| x == b)
        .map(|p| p + start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_plain_text() {
        assert_eq!(strip_to_wa("hello"), "hello");
    }

    #[test]
    fn bold_double_star_to_single() {
        assert_eq!(strip_to_wa("**hi**"), "*hi*");
    }

    #[test]
    fn italic_star_to_underscore() {
        assert_eq!(strip_to_wa("*hi*"), "_hi_");
    }

    #[test]
    fn italic_underscore_passthrough() {
        assert_eq!(strip_to_wa("_hi_"), "_hi_");
    }

    #[test]
    fn code_inline_strip() {
        assert_eq!(strip_to_wa("a `code` b"), "a code b");
    }

    #[test]
    fn link_inline_to_paren() {
        assert_eq!(
            strip_to_wa("see [docs](https://x.y)!"),
            "see docs (https://x.y)!"
        );
    }

    #[test]
    fn heading_one_strip() {
        assert_eq!(strip_to_wa("# Title"), "Title");
    }

    #[test]
    fn heading_three_strip() {
        assert_eq!(strip_to_wa("### Sub Sub"), "Sub Sub");
    }

    #[test]
    fn quote_strip() {
        assert_eq!(strip_to_wa("> quoted"), "quoted");
    }

    #[test]
    fn dash_list_to_bullet() {
        assert_eq!(strip_to_wa("- item"), "• item");
    }

    #[test]
    fn ordered_list_to_bullet() {
        assert_eq!(strip_to_wa("1. first\n2. second"), "• first\n• second");
    }

    #[test]
    fn fenced_code_block_strips_fences_keeps_body() {
        assert_eq!(strip_to_wa("```\nlet x = 1;\n```"), "let x = 1;\n");
    }

    #[test]
    fn unclosed_bold_keeps_original() {
        assert_eq!(strip_to_wa("**hi"), "**hi");
    }

    #[test]
    fn nested_bold_italic() {
        // **bold *italic* end** —— 简单贪婪实现：bold 优先匹配双星，
        // 内部 *italic* 走后续单星替换 → _italic_。
        // spec §5.2 没有 nested rule，此行为是 best-effort，可接受。
        assert_eq!(strip_to_wa("**bold *italic* end**"), "*bold _italic_ end*");
    }

    #[test]
    fn multi_line_mixed() {
        let input = "# Title\n\n- a\n- b\n\n**bold** _italic_";
        assert_eq!(strip_to_wa(input), "Title\n\n• a\n• b\n\n*bold* _italic_");
    }
}
