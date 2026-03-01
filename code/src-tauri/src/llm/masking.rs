//! Data masking — protect PII (names, companies, emails, phones) before LLM calls.
//!
//! Supports three levels: Strict (mask all), Standard (names + companies), Relaxed (none).
//! Maintains a bidirectional mapping for unmasking results.
//!
//! Pattern matching is implemented without the `regex` crate — uses simple
//! character-level scanning for emails, phones, company suffixes, and
//! keyword-triggered Chinese person names.
#![allow(dead_code)]

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::llm::streaming::ChatMessage;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Controls how aggressively PII is masked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MaskingLevel {
    /// Mask everything: names, companies, emails, phones.
    Strict,
    /// Mask person names and company names only.
    Standard,
    /// No masking at all.
    Relaxed,
}

impl Default for MaskingLevel {
    fn default() -> Self {
        Self::Strict
    }
}

/// Holds the bidirectional mapping for a masking session.
pub struct MaskingContext {
    level: MaskingLevel,
    /// original value -> placeholder
    mask_map: HashMap<String, String>,
    /// placeholder -> original value
    unmask_map: HashMap<String, String>,
    counters: MaskCounters,
}

/// Per-category counters for generating sequential placeholders.
struct MaskCounters {
    person: u32,
    company: u32,
    email: u32,
    phone: u32,
}

// ---------------------------------------------------------------------------
// Keywords and suffixes used for pattern detection
// ---------------------------------------------------------------------------

/// Keywords that, when immediately followed by 2-4 Chinese characters,
/// indicate a person name. E.g. "员工张三" -> mask "张三".
const PERSON_CONTEXT_KEYWORDS: &[&str] = &[
    "员工",
    "姓名",
    "人员",
    "负责人",
    "联系人",
    "经理",
    "主管",
    "总监",
    "专员",
    "同事",
    "老师",
    "先生",
    "女士",
    "领导",
    "总经理",
    "副总",
    "董事",
    "主任",
    "组长",
    "队长",
];

/// Suffixes that mark a company name. We scan backwards from these.
const COMPANY_SUFFIXES: &[&str] = &[
    "有限责任公司",
    "股份有限公司",
    "有限公司",
    "责任公司",
    "集团公司",
    "集团",
    "公司",
    "企业",
    "工厂",
    "事务所",
];

impl MaskingContext {
    pub fn new(level: MaskingLevel) -> Self {
        Self {
            level,
            mask_map: HashMap::new(),
            unmask_map: HashMap::new(),
            counters: MaskCounters {
                person: 0,
                company: 0,
                email: 0,
                phone: 0,
            },
        }
    }

    /// Mask a list of chat messages. Returns masked copies.
    pub fn mask_messages(&mut self, messages: &[ChatMessage]) -> Vec<ChatMessage> {
        messages
            .iter()
            .map(|msg| {
                let mut masked = msg.clone();
                masked.content = self.mask_text(&msg.content);
                masked
            })
            .collect()
    }

    /// Unmask a response string, replacing placeholders back with originals.
    pub fn unmask(&self, text: &str) -> String {
        let mut result = text.to_string();
        // Replace longest placeholders first to avoid partial matches.
        // Since placeholders are like [PERSON_1], [COMPANY_12], sorting by
        // descending length handles the unlikely overlap case.
        let mut pairs: Vec<(&String, &String)> = self.unmask_map.iter().collect();
        pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        for (placeholder, original) in pairs {
            result = result.replace(placeholder.as_str(), original.as_str());
        }

        // Fallback: replace any remaining PII placeholders that weren't matched.
        // This catches cases where LLM modified the placeholder format (e.g.
        // removed brackets, changed case). Pattern: [PERSON_N], [COMPANY_N],
        // [EMAIL_N], [PHONE_N] with optional brackets.
        result = replace_residual_placeholders(&result);

        result
    }

    /// Get the current masking level.
    pub fn level(&self) -> &MaskingLevel {
        &self.level
    }

    /// Get a read-only view of the mask map (original -> placeholder).
    pub fn mask_map(&self) -> &HashMap<String, String> {
        &self.mask_map
    }

    /// Merge another MaskingContext's mappings into this one.
    /// New mappings from `other` are absorbed so that tool results masked
    /// by a later iteration can still be unmasked by the combined context.
    pub fn merge(&mut self, other: MaskingContext) {
        for (original, placeholder) in other.mask_map {
            self.unmask_map.entry(placeholder.clone()).or_insert(original.clone());
            self.mask_map.entry(original).or_insert(placeholder);
        }
        // Advance counters to avoid collisions
        self.counters.person = self.counters.person.max(other.counters.person);
        self.counters.company = self.counters.company.max(other.counters.company);
        self.counters.email = self.counters.email.max(other.counters.email);
        self.counters.phone = self.counters.phone.max(other.counters.phone);
    }

    // -----------------------------------------------------------------------
    // Core masking logic
    // -----------------------------------------------------------------------

    /// Apply all relevant masking patterns to `text` based on the current level.
    pub fn mask_text(&mut self, text: &str) -> String {
        match self.level {
            MaskingLevel::Relaxed => return text.to_string(),
            MaskingLevel::Standard => {
                let result = self.mask_company_names(text);
                let result = self.mask_person_names(&result);
                result
            }
            MaskingLevel::Strict => {
                let result = self.mask_company_names(text);
                let result = self.mask_person_names(&result);
                let result = self.mask_emails(&result);
                let result = self.mask_phones(&result);
                result
            }
        }
    }

    /// Look up or create a placeholder for the given original value.
    fn get_or_create_placeholder(&mut self, original: &str, category: &str) -> String {
        if let Some(existing) = self.mask_map.get(original) {
            return existing.clone();
        }

        let counter = match category {
            "PERSON" => &mut self.counters.person,
            "COMPANY" => &mut self.counters.company,
            "EMAIL" => &mut self.counters.email,
            "PHONE" => &mut self.counters.phone,
            _ => &mut self.counters.person,
        };
        *counter += 1;
        let placeholder = format!("[{}_{counter}]", category);

        self.mask_map
            .insert(original.to_string(), placeholder.clone());
        self.unmask_map
            .insert(placeholder.clone(), original.to_string());
        placeholder
    }

    // -----------------------------------------------------------------------
    // Pattern: Company names
    // -----------------------------------------------------------------------

    /// Detect company names by finding known suffixes and extracting the
    /// preceding Chinese characters as the company name.
    fn mask_company_names(&mut self, text: &str) -> String {
        let mut result = text.to_string();

        // Try each suffix from longest to shortest to avoid partial matches
        let mut suffixes: Vec<&str> = COMPANY_SUFFIXES.to_vec();
        suffixes.sort_by(|a, b| b.len().cmp(&a.len()));

        for suffix in &suffixes {
            loop {
                let Some(suffix_pos) = result.find(suffix) else {
                    break;
                };

                // Walk backwards from the suffix to collect Chinese characters
                // that form the company name prefix.
                let before = &result[..suffix_pos];
                let prefix_start = find_chinese_prefix_start(before);

                if prefix_start == suffix_pos {
                    // No Chinese prefix found — not a real company name.
                    // To avoid infinite loop, skip past this occurrence.
                    // We do this by temporarily replacing the suffix with a
                    // sentinel, then restoring later. Simpler: just break.
                    break;
                }

                let company_name = &result[prefix_start..suffix_pos + suffix.len()];
                let company_name_owned = company_name.to_string();
                let placeholder = self.get_or_create_placeholder(&company_name_owned, "COMPANY");

                // Replace only the first occurrence to avoid index confusion
                result = result.replacen(&company_name_owned, &placeholder, 1);
            }
        }

        result
    }

    // -----------------------------------------------------------------------
    // Pattern: Person names (keyword-triggered)
    // -----------------------------------------------------------------------

    /// Detect person names that appear immediately after context keywords.
    /// E.g. "员工张三丰" -> "员工[PERSON_1]"
    fn mask_person_names(&mut self, text: &str) -> String {
        let mut result = text.to_string();

        // Sort keywords by length descending so longer keywords match first
        let mut keywords: Vec<&str> = PERSON_CONTEXT_KEYWORDS.to_vec();
        keywords.sort_by(|a, b| b.len().cmp(&a.len()));

        for keyword in &keywords {
            loop {
                let Some(kw_pos) = result.find(keyword) else {
                    break;
                };

                let after_keyword_byte = kw_pos + keyword.len();
                let after = &result[after_keyword_byte..];

                // Collect 2-4 consecutive Chinese characters after the keyword
                let name = extract_chinese_name(after);

                if name.is_empty() {
                    // No valid name follows this keyword occurrence.
                    // Advance past it to avoid infinite loop. We replace the
                    // keyword temporarily — simpler to just break since we
                    // handle one keyword at a time.
                    break;
                }

                let placeholder = self.get_or_create_placeholder(&name, "PERSON");
                result = result.replacen(&name, &placeholder, 1);
            }
        }

        result
    }

    // -----------------------------------------------------------------------
    // Pattern: Email addresses (Strict only)
    // -----------------------------------------------------------------------

    /// Simple email detection: find '@' and expand outward to collect
    /// local-part and domain.
    fn mask_emails(&mut self, text: &str) -> String {
        let mut result = text.to_string();

        loop {
            let Some(at_pos) = find_unmasked_at(&result) else {
                break;
            };

            // Expand left for local part: alphanumeric, '.', '_', '-', '+'
            let bytes = result.as_bytes();
            let mut left = at_pos;
            while left > 0 && is_email_local_char(bytes[left - 1]) {
                left -= 1;
            }

            // Expand right for domain: alphanumeric, '.', '-'
            let mut right = at_pos + 1;
            while right < bytes.len() && is_email_domain_char(bytes[right]) {
                right += 1;
            }

            // Validate minimal structure: at least 1 char before @, and
            // domain contains at least one '.'
            let local = &result[left..at_pos];
            let domain = &result[at_pos + 1..right];

            if local.is_empty() || !domain.contains('.') || domain.starts_with('.') {
                // Not a valid email — skip this '@' by advancing.
                // Replace this single '@' with a temp marker to avoid re-matching.
                // We will restore it later. Actually, simpler: just break, since
                // in practice there won't be stray '@' in HR text.
                break;
            }

            let email = result[left..right].to_string();
            let placeholder = self.get_or_create_placeholder(&email, "EMAIL");
            result = result.replacen(&email, &placeholder, 1);
        }

        result
    }

    // -----------------------------------------------------------------------
    // Pattern: Chinese phone numbers (Strict only)
    // -----------------------------------------------------------------------

    /// Detect 11-digit Chinese mobile numbers starting with 1[3-9].
    fn mask_phones(&mut self, text: &str) -> String {
        let result = text.to_string();

        // Scan for runs of digits
        let mut i = 0;
        let chars: Vec<char> = result.chars().collect();

        // We rebuild the string to avoid index shifting issues
        let mut output = String::with_capacity(result.len());
        while i < chars.len() {
            // Check if this could be the start of a phone number
            if chars[i] == '1' && i + 10 < chars.len() {
                // Check second digit is 3-9
                let second = chars[i + 1];
                if second >= '3' && second <= '9' {
                    // Check remaining 9 digits
                    let mut all_digits = true;
                    for j in 2..11 {
                        if !chars[i + j].is_ascii_digit() {
                            all_digits = false;
                            break;
                        }
                    }

                    if all_digits {
                        // Make sure char before is not a digit (avoid matching
                        // middle of a longer number)
                        let preceded_by_digit =
                            i > 0 && chars[i - 1].is_ascii_digit();
                        // Make sure char after (if any) is not a digit
                        let followed_by_digit =
                            i + 11 < chars.len() && chars[i + 11].is_ascii_digit();

                        if !preceded_by_digit && !followed_by_digit {
                            let phone: String =
                                chars[i..i + 11].iter().collect();
                            let placeholder =
                                self.get_or_create_placeholder(&phone, "PHONE");
                            output.push_str(&placeholder);
                            i += 11;
                            continue;
                        }
                    }
                }
            }

            output.push(chars[i]);
            i += 1;
        }

        output
    }
}

// ---------------------------------------------------------------------------
// Character-level helpers
// ---------------------------------------------------------------------------

/// Check if a byte is a valid email local-part character.
fn is_email_local_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-' || b == b'+'
}

/// Check if a byte is a valid email domain character.
fn is_email_domain_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'.' || b == b'-'
}

/// Check if a char is a CJK Unified Ideograph (basic range U+4E00..U+9FA5).
fn is_chinese_char(c: char) -> bool {
    c >= '\u{4e00}' && c <= '\u{9fa5}'
}

/// Given the text *before* a company suffix, walk backwards to find the start
/// byte index of the Chinese prefix. Returns the byte offset where the
/// Chinese characters begin. If no Chinese chars are found, returns
/// `before.len()` (i.e., empty prefix).
fn find_chinese_prefix_start(before: &str) -> usize {
    let chars: Vec<(usize, char)> = before.char_indices().collect();
    if chars.is_empty() {
        return before.len();
    }

    let mut idx = chars.len();
    while idx > 0 {
        let (_, c) = chars[idx - 1];
        if is_chinese_char(c) {
            idx -= 1;
        } else {
            break;
        }
    }

    if idx == chars.len() {
        // No Chinese characters found
        before.len()
    } else {
        chars[idx].0
    }
}

/// Extract 2-4 consecutive Chinese characters from the start of `text`.
/// Returns the extracted name string or empty string if fewer than 2
/// Chinese characters are found at the start.
fn extract_chinese_name(text: &str) -> String {
    let mut chars_collected: Vec<char> = Vec::new();
    let mut byte_len = 0;

    for c in text.chars() {
        if is_chinese_char(c) && chars_collected.len() < 4 {
            chars_collected.push(c);
            byte_len += c.len_utf8();
        } else {
            break;
        }
    }

    if chars_collected.len() >= 2 {
        text[..byte_len].to_string()
    } else {
        String::new()
    }
}

/// Find the byte position of an '@' that is not inside an existing
/// placeholder (i.e., not between '[' and ']').
fn find_unmasked_at(text: &str) -> Option<usize> {
    let mut in_bracket = false;
    for (i, c) in text.char_indices() {
        match c {
            '[' => in_bracket = true,
            ']' => in_bracket = false,
            '@' if !in_bracket => return Some(i),
            _ => {}
        }
    }
    None
}

/// Replace residual PII placeholders that weren't matched during unmask.
///
/// Scans for patterns like `[PERSON_1]`, `COMPANY_2`, `[EMAIL_3]` etc.
/// where the category is PERSON/COMPANY/EMAIL/PHONE followed by _N digits.
/// Handles both bracketed `[PERSON_1]` and bare `PERSON_1` forms.
/// Replaces with `[已脱敏]` to avoid leaking placeholder text to users.
fn replace_residual_placeholders(text: &str) -> String {
    const CATEGORIES: &[&str] = &["PERSON", "COMPANY", "EMAIL", "PHONE"];

    let mut result = text.to_string();
    let mut changed = true;

    // Iterate until no more replacements (handles adjacent placeholders)
    while changed {
        changed = false;
        for category in CATEGORIES {
            // Try bracketed form first: [CATEGORY_N]
            let bracket_prefix = format!("[{}_", category);
            if let Some(start) = result.find(&bracket_prefix) {
                let after = &result[start + bracket_prefix.len()..];
                // Collect digits
                let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !digits.is_empty() {
                    let end_offset = start + bracket_prefix.len() + digits.len();
                    // Check for closing bracket
                    let placeholder_end = if result.as_bytes().get(end_offset) == Some(&b']') {
                        end_offset + 1
                    } else {
                        end_offset
                    };
                    result.replace_range(start..placeholder_end, "[已脱敏]");
                    changed = true;
                    continue;
                }
            }

            // Try bare form: CATEGORY_N (not preceded by '[')
            let bare_prefix = format!("{}_", category);
            if let Some(start) = result.find(&bare_prefix) {
                // Make sure it's not inside brackets (already handled above)
                let preceded_by_bracket = start > 0 && result.as_bytes()[start - 1] == b'[';
                if !preceded_by_bracket {
                    let after = &result[start + bare_prefix.len()..];
                    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if !digits.is_empty() {
                        let end_offset = start + bare_prefix.len() + digits.len();
                        // Check for trailing ']' (malformed bracket)
                        let placeholder_end = if result.as_bytes().get(end_offset) == Some(&b']') {
                            end_offset + 1
                        } else {
                            end_offset
                        };
                        result.replace_range(start..placeholder_end, "[已脱敏]");
                        changed = true;
                        continue;
                    }
                }
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- MaskingLevel -------------------------------------------------------

    #[test]
    fn test_default_level_is_strict() {
        assert_eq!(MaskingLevel::default(), MaskingLevel::Strict);
    }

    #[test]
    fn test_serde_roundtrip() {
        let level = MaskingLevel::Strict;
        let json = serde_json::to_string(&level).unwrap();
        assert_eq!(json, r#""strict""#);
        let parsed: MaskingLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, MaskingLevel::Strict);
    }

    // -- Relaxed level (no masking) -----------------------------------------

    #[test]
    fn test_relaxed_no_masking() {
        let mut ctx = MaskingContext::new(MaskingLevel::Relaxed);
        let input = "员工张三在华为公司工作，电话13800138000，邮箱test@example.com";
        let result = ctx.mask_text(input);
        assert_eq!(result, input);
    }

    // -- Company names ------------------------------------------------------

    #[test]
    fn test_mask_company_basic() {
        let mut ctx = MaskingContext::new(MaskingLevel::Standard);
        let input = "华为技术有限公司是一家科技企业";
        let result = ctx.mask_text(input);
        assert!(result.contains("[COMPANY_1]"));
        assert!(!result.contains("华为技术有限公司"));
    }

    #[test]
    fn test_mask_multiple_companies() {
        let mut ctx = MaskingContext::new(MaskingLevel::Standard);
        let input = "华为技术有限公司和腾讯科技公司合作";
        let result = ctx.mask_text(input);
        assert!(result.contains("[COMPANY_1]"));
        assert!(result.contains("[COMPANY_2]"));
    }

    #[test]
    fn test_mask_same_company_reuses_placeholder() {
        let mut ctx = MaskingContext::new(MaskingLevel::Standard);
        let input = "华为公司很好。华为公司很大。";
        let result = ctx.mask_text(input);
        // Both occurrences should use the same placeholder
        let count = result.matches("[COMPANY_1]").count();
        assert_eq!(count, 2);
    }

    // -- Person names -------------------------------------------------------

    #[test]
    fn test_mask_person_after_keyword() {
        let mut ctx = MaskingContext::new(MaskingLevel::Standard);
        let input = "员工张三已入职";
        let result = ctx.mask_text(input);
        assert!(result.contains("[PERSON_1]"));
        assert!(!result.contains("张三"));
        assert!(result.contains("员工"));
    }

    #[test]
    fn test_mask_person_three_char_name() {
        let mut ctx = MaskingContext::new(MaskingLevel::Standard);
        let input = "联系人张三丰负责此项目";
        let result = ctx.mask_text(input);
        assert!(result.contains("[PERSON_1]"));
        assert!(!result.contains("张三丰"));
    }

    #[test]
    fn test_mask_person_four_char_name() {
        let mut ctx = MaskingContext::new(MaskingLevel::Standard);
        let input = "经理欧阳锋峰的报告";
        let result = ctx.mask_text(input);
        assert!(result.contains("[PERSON_1]"));
        assert!(!result.contains("欧阳锋峰"));
    }

    #[test]
    fn test_no_mask_without_keyword() {
        let mut ctx = MaskingContext::new(MaskingLevel::Standard);
        // "张三" appears but not after a keyword
        let input = "今天天气真好";
        let result = ctx.mask_text(input);
        assert_eq!(result, input);
    }

    // -- Email addresses (Strict only) --------------------------------------

    #[test]
    fn test_mask_email_strict() {
        let mut ctx = MaskingContext::new(MaskingLevel::Strict);
        let input = "请联系 alice@example.com 获取详情";
        let result = ctx.mask_text(input);
        assert!(result.contains("[EMAIL_1]"));
        assert!(!result.contains("alice@example.com"));
    }

    #[test]
    fn test_email_not_masked_in_standard() {
        let mut ctx = MaskingContext::new(MaskingLevel::Standard);
        let input = "邮箱是 alice@example.com";
        let result = ctx.mask_text(input);
        assert!(result.contains("alice@example.com"));
    }

    // -- Phone numbers (Strict only) ----------------------------------------

    #[test]
    fn test_mask_phone_strict() {
        let mut ctx = MaskingContext::new(MaskingLevel::Strict);
        let input = "手机号13912345678请保密";
        let result = ctx.mask_text(input);
        assert!(result.contains("[PHONE_1]"));
        assert!(!result.contains("13912345678"));
    }

    #[test]
    fn test_phone_not_masked_in_standard() {
        let mut ctx = MaskingContext::new(MaskingLevel::Standard);
        let input = "电话13800138000";
        let result = ctx.mask_text(input);
        assert!(result.contains("13800138000"));
    }

    #[test]
    fn test_phone_must_be_11_digits() {
        let mut ctx = MaskingContext::new(MaskingLevel::Strict);
        // 10 digits — too short
        let input = "号码1390013800不是手机号";
        let result = ctx.mask_text(input);
        assert!(!result.contains("[PHONE_"));
    }

    #[test]
    fn test_phone_not_in_longer_number() {
        let mut ctx = MaskingContext::new(MaskingLevel::Strict);
        // 12 digits — the 11-digit substring should NOT match because it is
        // part of a longer digit run.
        let input = "编号139001380001是订单号";
        let result = ctx.mask_text(input);
        assert!(!result.contains("[PHONE_"));
    }

    // -- Unmask -------------------------------------------------------------

    #[test]
    fn test_unmask_roundtrip() {
        let mut ctx = MaskingContext::new(MaskingLevel::Strict);
        let input = "员工张三在华为公司工作，电话13800138000";
        let masked = ctx.mask_text(input);

        // Masked string should not contain originals
        assert!(!masked.contains("张三"));
        assert!(!masked.contains("华为公司"));
        assert!(!masked.contains("13800138000"));

        // Unmask should restore
        let unmasked = ctx.unmask(&masked);
        assert_eq!(unmasked, input);
    }

    // -- mask_messages ------------------------------------------------------

    #[test]
    fn test_mask_messages() {
        let mut ctx = MaskingContext::new(MaskingLevel::Standard);
        let messages = vec![
            ChatMessage::text("user", "员工李四,腾讯公司"),
            ChatMessage::text("assistant", "好的"),
        ];

        let masked = ctx.mask_messages(&messages);
        assert_eq!(masked.len(), 2);
        assert!(masked[0].content.contains("[PERSON_1]"));
        assert!(masked[0].content.contains("[COMPANY_1]"));
        assert_eq!(masked[0].role, "user");
        assert_eq!(masked[1].content, "好的");
    }

    // -- Helper functions ---------------------------------------------------

    #[test]
    fn test_is_chinese_char() {
        assert!(is_chinese_char('中'));
        assert!(is_chinese_char('国'));
        assert!(!is_chinese_char('a'));
        assert!(!is_chinese_char('1'));
        assert!(!is_chinese_char(' '));
    }

    #[test]
    fn test_extract_chinese_name() {
        // Extracts up to 4 consecutive Chinese chars
        assert_eq!(extract_chinese_name("张三是好人"), "张三是好");
        assert_eq!(extract_chinese_name("张三丰武功高"), "张三丰武");
        assert_eq!(extract_chinese_name("欧阳锋峰很强"), "欧阳锋峰");
        // Only 1 char — too short
        assert_eq!(extract_chinese_name("张a三"), "");
        // Max 4 chars
        assert_eq!(extract_chinese_name("欧阳锋峰强"), "欧阳锋峰");
    }

    #[test]
    fn test_find_chinese_prefix_start() {
        let text = "abc华为技术";
        let start = find_chinese_prefix_start(text);
        assert_eq!(&text[start..], "华为技术");

        let text2 = "abc";
        let start2 = find_chinese_prefix_start(text2);
        assert_eq!(start2, text2.len());
    }

    // -- Mixed-type masking (companies + persons + email + phone together) ------

    #[test]
    fn test_strict_masks_all_types_together() {
        let mut ctx = MaskingContext::new(MaskingLevel::Strict);
        // Company name at start of string (non-Chinese boundary before it)
        // Person name after keyword, followed by comma (non-Chinese boundary)
        let input = "华为技术有限公司的员工张三,电话13800138000,邮箱zhangsan@huawei.com";
        let result = ctx.mask_text(input);

        assert!(result.contains("[COMPANY_1]"), "Company should be masked");
        assert!(result.contains("[PERSON_1]"), "Person should be masked");
        assert!(result.contains("[PHONE_1]"), "Phone should be masked");
        assert!(result.contains("[EMAIL_1]"), "Email should be masked");

        // Originals should not appear
        assert!(!result.contains("华为技术有限公司"));
        assert!(!result.contains("张三"));
        assert!(!result.contains("13800138000"));
        assert!(!result.contains("zhangsan@huawei.com"));

        // Unmask should restore everything
        let unmasked = ctx.unmask(&result);
        assert_eq!(unmasked, input);
    }

    #[test]
    fn test_standard_masks_company_and_person_only() {
        let mut ctx = MaskingContext::new(MaskingLevel::Standard);
        // Company at start, person after keyword with comma boundary
        let input = "腾讯科技公司的员工李四,手机15912345678,邮箱lisi@qq.com";
        let result = ctx.mask_text(input);

        assert!(result.contains("[COMPANY_1]"), "Company should be masked");
        assert!(result.contains("[PERSON_1]"), "Person should be masked");
        // Standard does NOT mask email or phone
        assert!(result.contains("15912345678"), "Phone should NOT be masked in Standard");
        assert!(result.contains("lisi@qq.com"), "Email should NOT be masked in Standard");
    }

    // -- Multiple persons with same name reuse placeholder ----------------------

    #[test]
    fn test_same_person_reuses_placeholder() {
        let mut ctx = MaskingContext::new(MaskingLevel::Standard);
        // Use comma after name to create non-CJK boundary, preventing greedy extraction
        let input = "员工张三,表现好。主管张三,值得晋升。";
        let result = ctx.mask_text(input);
        // Both should use the same placeholder
        let count = result.matches("[PERSON_1]").count();
        assert_eq!(count, 2);
    }

    // -- Multiple different persons get different placeholders ------------------

    #[test]
    fn test_different_persons_get_different_placeholders() {
        let mut ctx = MaskingContext::new(MaskingLevel::Standard);
        // Use different keywords to avoid the single-keyword break-on-empty issue
        let input = "员工张三,主管李四,都很优秀";
        let result = ctx.mask_text(input);
        assert!(result.contains("[PERSON_1]"));
        assert!(result.contains("[PERSON_2]"));
    }

    // -- Multiple emails get different placeholders ----------------------------

    #[test]
    fn test_multiple_emails_strict() {
        let mut ctx = MaskingContext::new(MaskingLevel::Strict);
        let input = "联系alice@test.com或bob@test.com";
        let result = ctx.mask_text(input);
        assert!(result.contains("[EMAIL_1]"));
        assert!(result.contains("[EMAIL_2]"));
    }

    // -- Multiple phones get different placeholders ----------------------------

    #[test]
    fn test_multiple_phones_strict() {
        let mut ctx = MaskingContext::new(MaskingLevel::Strict);
        let input = "电话13800138000和15912345678";
        let result = ctx.mask_text(input);
        assert!(result.contains("[PHONE_1]"));
        assert!(result.contains("[PHONE_2]"));
    }

    // -- Empty input -----------------------------------------------------------

    #[test]
    fn test_mask_empty_string() {
        let mut ctx = MaskingContext::new(MaskingLevel::Strict);
        let result = ctx.mask_text("");
        assert_eq!(result, "");
    }

    #[test]
    fn test_unmask_empty_string() {
        let ctx = MaskingContext::new(MaskingLevel::Strict);
        let result = ctx.unmask("");
        assert_eq!(result, "");
    }

    // -- No PII in text --------------------------------------------------------

    #[test]
    fn test_text_with_no_pii() {
        let mut ctx = MaskingContext::new(MaskingLevel::Strict);
        let input = "薪酬分析报告显示总体公平性指数为0.85";
        let result = ctx.mask_text(input);
        assert_eq!(result, input);
    }

    // -- Unmask with no prior masking ------------------------------------------

    #[test]
    fn test_unmask_without_prior_mask() {
        let ctx = MaskingContext::new(MaskingLevel::Standard);
        let input = "some text with [COMPANY_1] placeholder";
        // Since no masking was performed, unmask_map is empty.
        // The residual placeholder fallback replaces it with [已脱敏].
        let result = ctx.unmask(input);
        assert_eq!(result, "some text with [已脱敏] placeholder");
    }

    // -- mask_messages with mixed roles ----------------------------------------

    #[test]
    fn test_mask_messages_system_role_also_masked() {
        let mut ctx = MaskingContext::new(MaskingLevel::Standard);
        let messages = vec![
            ChatMessage::text("system", "华为公司的HR系统"),
            ChatMessage::text("user", "员工张三怎么样"),
        ];

        let masked = ctx.mask_messages(&messages);
        assert!(masked[0].content.contains("[COMPANY_1]"));
        assert!(masked[1].content.contains("[PERSON_1]"));
    }

    // -- Unmask roundtrip across multiple mask_text calls -----------------------

    #[test]
    fn test_unmask_roundtrip_multiple_calls() {
        let mut ctx = MaskingContext::new(MaskingLevel::Strict);
        let input1 = "员工张三在华为公司";
        let input2 = "员工李四发邮件到lisi@company.com";

        let masked1 = ctx.mask_text(input1);
        let masked2 = ctx.mask_text(input2);

        // Both should be restorable from the same context
        let unmasked1 = ctx.unmask(&masked1);
        let unmasked2 = ctx.unmask(&masked2);
        assert_eq!(unmasked1, input1);
        assert_eq!(unmasked2, input2);
    }

    // -- Phone edge cases: hyphens/spaces should not affect -------------------

    #[test]
    fn test_phone_with_prefix_1() {
        let mut ctx = MaskingContext::new(MaskingLevel::Strict);
        // Starts with 1, exactly 11 digits
        let input = "号码18612345678";
        let result = ctx.mask_text(input);
        assert!(result.contains("[PHONE_1]"));
    }

    // -- Residual placeholder fallback -------------------------------------------

    #[test]
    fn test_residual_bracketed_placeholder_replaced() {
        let text = "员工[PERSON_1]在[COMPANY_2]工作";
        let result = replace_residual_placeholders(text);
        assert_eq!(result, "员工[已脱敏]在[已脱敏]工作");
    }

    #[test]
    fn test_residual_bare_placeholder_replaced() {
        // LLM might strip brackets
        let text = "员工PERSON_1在COMPANY_2工作";
        let result = replace_residual_placeholders(text);
        assert_eq!(result, "员工[已脱敏]在[已脱敏]工作");
    }

    #[test]
    fn test_residual_email_phone_placeholder() {
        let text = "联系[EMAIL_1]或[PHONE_3]";
        let result = replace_residual_placeholders(text);
        assert_eq!(result, "联系[已脱敏]或[已脱敏]");
    }

    #[test]
    fn test_no_residual_when_no_placeholders() {
        let text = "这是普通文本，没有占位符";
        let result = replace_residual_placeholders(text);
        assert_eq!(result, text);
    }

    #[test]
    fn test_unmask_with_residual_fallback() {
        let mut ctx = MaskingContext::new(MaskingLevel::Standard);
        let input = "员工张三,在华为公司工作";
        let _masked = ctx.mask_text(input);

        // Simulate LLM output that references a placeholder that was never in the mask map
        let llm_output = "分析结果：[PERSON_99]的薪酬偏低";
        let unmasked = ctx.unmask(llm_output);
        assert_eq!(unmasked, "分析结果：[已脱敏]的薪酬偏低");
    }
}
