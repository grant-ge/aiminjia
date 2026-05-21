use app_lib::plugin::skill::frontmatter::parse_skill_md;

#[test]
fn parses_required_skill_md_frontmatter_and_body() {
    let input = r#"---
name: salary-query
description: 薪酬查询
metadata:
  label: 薪酬市场数据查询助手
---

# Body
Use `${AIJIA_SKILL_DIR}/scripts/call.py`.
"#;

    let parsed = parse_skill_md(input).expect("valid SKILL.md should parse");
    assert_eq!(parsed.frontmatter.name, "salary-query");
    assert_eq!(parsed.frontmatter.description, "薪酬查询");
    assert_eq!(
        parsed.frontmatter.metadata.label.as_deref(),
        Some("薪酬市场数据查询助手")
    );
    assert!(parsed.body.contains("# Body"));
}

#[test]
fn rejects_missing_frontmatter() {
    let err = parse_skill_md("# Body only").unwrap_err().to_string();
    assert!(err.contains("frontmatter"), "unexpected error: {err}");
}

#[test]
fn rejects_missing_required_name_or_description() {
    let missing_name = "---\ndescription: x\n---\nbody";
    assert!(parse_skill_md(missing_name)
        .unwrap_err()
        .to_string()
        .contains("name"));

    let missing_desc = "---\nname: x\n---\nbody";
    assert!(parse_skill_md(missing_desc)
        .unwrap_err()
        .to_string()
        .contains("description"));
}

#[test]
fn accepts_all_claude_code_fields() {
    let input = r#"---
name: code-review
description: Review code
when_to_use: user asks for code review
allowed-tools:
  - read_file
  - Bash
argument-hint: <path>
arguments: path severity
model: opus
effort: high
context: fork
agent: code-reviewer
user-invocable: false
disable-model-invocation: true
version: "1.0"
paths:
  - "src/**/*.rs"
hooks:
  PreToolUse:
    - command: ["echo", "hi"]
shell: bash
metadata:
  label: Code Review
unknown-field: ignored
---
body
"#;
    let parsed = parse_skill_md(input).expect("all supported fields should parse");
    assert_eq!(parsed.frontmatter.context.as_deref(), Some("fork"));
    assert_eq!(parsed.frontmatter.allowed_tools, vec!["read_file", "Bash"]);
    assert_eq!(parsed.frontmatter.arguments, vec!["path", "severity"]);
    assert!(parsed.frontmatter.disable_model_invocation);
}
