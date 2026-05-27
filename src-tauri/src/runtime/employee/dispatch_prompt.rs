//! Prompt assembly for employee dispatch.
//!
//! Pure string construction over `EmployeeRecord` + the per-trigger inputs
//! (trigger label, catchup info, optional user request). Lives outside the
//! transport layer so it can be unit-tested in isolation — the prompt format
//! is the load-bearing piece that drives whether the LLM starts working
//! immediately or waits for further instructions.

use std::path::Path;

use crate::runtime::employee::store::EmployeeRecord;
use crate::runtime::employee::template_store::{
    effective_default_skill_id, effective_requires_attachment, effective_skill_ids,
    effective_system_prompt_extra,
};

/// Build the user-facing prompt sent on employee dispatch.
///
/// `employees_root` is the directory that contains per-employee dirs
/// (`<employees_root>/<employee_id>/template/template.json`). When `Some`
/// and a snapshot exists, the snapshot's `system_prompt_extra` and
/// `default_skill_id` take precedence over the record's matching fields.
/// When `None` (tests, or pre-PR3 records with no snapshot yet), the
/// record fields are used as-is.
///
/// Layout:
/// ```text
/// {identity}{system_prompt_extra}
/// {trigger_label}{catchup}                       # catchup is "\n{info}" or empty
/// {user_request}                                 # blank line + content, or empty
///
/// 【本次工作配置】                                 # whole block omitted when both lines empty
/// - 默认技能：…                                   # only when default_skill_id is Some(non-empty)
/// - 资源配置：{json}                              # only when resource_config is non-empty object
///
/// 请立即开始按职责执行，不要等待用户额外指示。       # mandatory
/// ```
pub fn build_dispatch_prompt(
    employee: &EmployeeRecord,
    trigger_label: &str,
    catchup_info: Option<&str>,
    prompt_override: Option<&str>,
    employees_root: Option<&Path>,
) -> String {
    let identity_block = format!(
        "你现在是「{}」（{}）。\n{}\n",
        employee.name, employee.role, employee.description
    );
    // Snapshot-first lookup: when an instance has a `template/template.json`
    // (always true for post-PR3 hires + back-filled legacy records), the
    // snapshot wins. Pre-PR3 records with no snapshot fall back to the
    // record field. PR6 deletes the record field; this helper's fallback
    // branch goes away then.
    let extra = employees_root
        .and_then(|root| {
            effective_system_prompt_extra(
                root,
                &employee.id,
                employee.system_prompt_extra.as_deref(),
            )
        })
        .or_else(|| employee.system_prompt_extra.clone())
        .unwrap_or_default();

    let catchup = catchup_info.map(|s| format!("\n{s}")).unwrap_or_default();

    let mut config_lines: Vec<String> = Vec::new();

    // Collect all configured skills: default first, then additional skill_ids.
    let effective_default = employees_root
        .and_then(|root| {
            effective_default_skill_id(root, &employee.id, employee.default_skill_id.as_deref())
        })
        .or_else(|| employee.default_skill_id.clone());
    let extra_skills = employees_root
        .map(|root| effective_skill_ids(root, &employee.id, &employee.skill_ids))
        .unwrap_or_else(|| employee.skill_ids.clone());

    // Merge: default skill + extra skills, dedup
    let mut all_skills: Vec<String> = Vec::new();
    if let Some(ref sid) = effective_default {
        if !sid.is_empty() {
            all_skills.push(sid.clone());
        }
    }
    for sid in &extra_skills {
        if !sid.is_empty() && !all_skills.iter().any(|s| s == sid) {
            all_skills.push(sid.clone());
        }
    }

    if all_skills.len() == 1 {
        // Single skill — keep the original concise format. Use imperative
        // tool-call phrasing ("使用 X 工具") rather than function-call
        // syntax ("Skill(skill_id='...')") so the LLM doesn't mistake the
        // hint for a reasoning step it can satisfy with prose.
        config_lines.push(format!(
            "- 默认技能：{} —— 第一步请使用 Skill 工具（参数 skill_id=\"{}\"）加载工作流",
            all_skills[0], all_skills[0]
        ));
    } else if all_skills.len() > 1 {
        // Multiple skills — list them, instruct LLM to load as needed.
        config_lines.push("- 可用技能：".to_string());
        for (i, sid) in all_skills.iter().enumerate() {
            if i == 0 {
                config_lines.push(format!("  · {sid}（默认，第一步请加载）"));
            } else {
                config_lines.push(format!("  · {sid}"));
            }
        }
        config_lines.push(format!(
            "  请用 Skill 工具按需加载（参数 skill_id），首先加载 {}。",
            all_skills[0]
        ));
    }
    let resource_config_is_empty = match &employee.resource_config {
        serde_json::Value::Null => true,
        serde_json::Value::Object(map) => map.is_empty(),
        _ => false,
    };
    if !resource_config_is_empty {
        let template_id = employee.template_id.as_deref().unwrap_or("");
        let summary_lines = summarize_resource_config(template_id, &employee.resource_config);
        if summary_lines.is_empty() {
            // Unknown / custom template — fall back to raw JSON so the LLM
            // still has the data, even if the user-visible form is ugly.
            let json = serde_json::to_string(&employee.resource_config)
                .unwrap_or_else(|_| "{}".to_string());
            config_lines.push(format!("- 资源配置：{json}"));
        } else {
            config_lines.extend(summary_lines);
        }
    }
    if let Some(hint) = knowledge_memory_hint(employee) {
        config_lines.push(hint);
    }
    let config_block = if config_lines.is_empty() {
        String::new()
    } else {
        format!("\n\n【本次工作配置】\n{}", config_lines.join("\n"))
    };

    // PR-10: when the template declares `requires_attachment`, we used to
    // pop a native file picker before dispatch. The picker felt jarring
    // (came out of nowhere when clicking 派活). Now we open the chat
    // first and let the LLM ask for the files in its first turn. This
    // hint goes outside the 【本次工作配置】 block because it's an
    // instruction to the agent about what to do first, not a fact about
    // the workspace.
    let attachment_hint = build_attachment_hint(employees_root, &employee.id);

    let user_request = prompt_override.unwrap_or("").trim();
    let user_block = if user_request.is_empty() {
        String::new()
    } else {
        format!("\n\n{user_request}")
    };

    format!(
        "{identity_block}{extra}\n{trigger_label}{catchup}{user_block}{config_block}{attachment_hint}\n\n请立即开始按职责执行，不要等待用户额外指示。"
    )
}

/// Build the in-chat "please ask the user to attach files" hint for templates
/// with `requires_attachment`. Returns an empty string when the snapshot has
/// no attachment requirement (most templates).
///
/// Format produced (when active):
///   \n\n【附件】用户尚未在派活时上传文件。请第一步用友好的语气引导用户
///   把所需的 PDF/DOCX 等文件拖入对话框或粘贴附件后，再开始正式工作。
fn build_attachment_hint(employees_root: Option<&Path>, employee_id: &str) -> String {
    let Some(root) = employees_root else {
        return String::new();
    };
    let Some(spec) = effective_requires_attachment(root, employee_id) else {
        return String::new();
    };
    // Best-effort extraction of accept / min / max for a friendlier hint.
    let accept = spec.get("accept").and_then(|v| v.as_str()).unwrap_or("");
    let min = spec.get("min").and_then(|v| v.as_i64()).unwrap_or(1);
    let max = spec.get("max").and_then(|v| v.as_i64()).unwrap_or(1);
    let count_phrase = if min == max {
        format!("{min} 份")
    } else {
        format!("{min}–{max} 份")
    };
    let file_phrase = if accept.is_empty() {
        "所需文件".to_string()
    } else {
        format!("{accept} 格式的文件")
    };
    format!(
        "\n\n【附件】本次派活没有附带文件。\
请第一步用友好的语气引导用户把 {count_phrase} {file_phrase} 拖入对话框（或粘贴附件、点击 + 按钮），\
看到附件成功显示后再开始正式工作。引导时不要让用户感到突兀，先简短问候并说明你将做什么。"
    )
}

/// Render the employee's `resource_config` JSON into one or more
/// human-readable bullet lines for the dispatch prompt.
///
/// Returns an empty Vec when the template_id is unknown (caller falls back to
/// raw JSON) or when every field is empty.
///
/// Why: the previous implementation dumped the entire `resource_config` JSON
/// inline as `- 资源配置：{"groupMatch":{...}}`. Two problems:
/// - The LLM has to parse it; tokens are wasted and field names can be
///   missed.
/// - The user re-reading the chat log sees a wall of JSON instead of the
///   keywords / URLs / templates they actually filled in.
///
/// Each `case` corresponds to a `ResourceConfigKind` defined in the frontend
/// `templates.ts`. New built-in templates that ship a `resourceConfigKind`
/// must add a branch here; new custom templates (via OPS portal `resourceConfigSchema`)
/// fall through to the raw-JSON fallback in `build_dispatch_prompt`.
fn summarize_resource_config(template_id: &str, cfg: &serde_json::Value) -> Vec<String> {
    let obj = match cfg.as_object() {
        Some(o) => o,
        None => return Vec::new(),
    };
    match template_id {
        // monitoring-urls (小研 / xiaoyuan)
        "builtin:xiaoyuan" => summarize_monitoring_urls(obj),
        // sales-table (小标 / xiaobiao)
        "builtin:xiaobiao" => summarize_sales_table(obj),
        // tech-support (小工 / xiaogong) + customer-support (小客 / xiaoke)
        // share the GroupMatch + responseStyle + summaryCron + knowledgeSources
        // shape; xiaoke also has greeting/closing/escalation/tech keywords.
        "builtin:xiaogong" => summarize_group_match_form(obj, /* full */ false),
        "builtin:xiaoke" => summarize_group_match_form(obj, /* full */ true),
        // weekly-report (小周 / xiaozhou)
        "builtin:xiaozhou" => summarize_weekly_report(obj),
        _ => Vec::new(),
    }
}

fn join_chinese(parts: &[&str]) -> String {
    parts.join("、")
}

fn pluck_str_array<'a>(
    obj: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Vec<&'a str> {
    obj.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default()
}

fn summarize_monitoring_urls(obj: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    let targets = match obj.get("monitoringTargets").and_then(|v| v.as_array()) {
        Some(t) if !t.is_empty() => t,
        _ => return vec!["- 监听目标：（未配置，将在对话中由你协助补全）".to_string()],
    };
    let rows: Vec<String> = targets
        .iter()
        .filter_map(|t| {
            let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
            let url = t.get("url").and_then(|v| v.as_str()).unwrap_or("").trim();
            if name.is_empty() && url.is_empty() {
                None
            } else if url.is_empty() {
                Some(name.to_string())
            } else if name.is_empty() {
                Some(url.to_string())
            } else {
                Some(format!("{name}（{url}）"))
            }
        })
        .collect();
    if rows.is_empty() {
        return vec!["- 监听目标：（未配置，将在对话中由你协助补全）".to_string()];
    }
    vec![format!(
        "- 监听目标（{} 个）：{}",
        rows.len(),
        rows.join("；")
    )]
}

fn summarize_sales_table(obj: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    let mut lines = Vec::new();
    let base_id = obj.get("baseId").and_then(|v| v.as_str()).unwrap_or("");
    let table_id = obj.get("tableId").and_then(|v| v.as_str()).unwrap_or("");
    let share_url = obj.get("shareUrl").and_then(|v| v.as_str()).unwrap_or("");
    if !base_id.is_empty() {
        if !table_id.is_empty() {
            lines.push(format!(
                "- 钉钉多维表：baseId={base_id} · tableId={table_id}"
            ));
        } else {
            lines.push(format!(
                "- 钉钉多维表：baseId={base_id}（tableId 未指定，请在对话中向用户确认子表名）"
            ));
        }
    } else if !share_url.is_empty() {
        lines.push(format!(
            "- 钉钉多维表链接：{share_url}（请解析 baseId/tableId）"
        ));
    } else {
        lines.push("- 钉钉多维表：（未配置，将在对话中由你协助补全分享链接）".to_string());
    }
    if let Some(scope) = obj.get("scope").and_then(|v| v.as_str()) {
        lines.push(format!("- 写入范围：{scope}"));
    }
    if let Some(mapping) = obj.get("fieldMapping").and_then(|v| v.as_object()) {
        if !mapping.is_empty() {
            let kvs: Vec<String> = mapping
                .iter()
                .map(|(k, v)| {
                    let val_str = match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    format!("{k}→{val_str}")
                })
                .collect();
            lines.push(format!("- 字段映射：{}", kvs.join("、")));
        }
    }
    lines
}

fn summarize_group_match_form(
    obj: &serde_json::Map<String, serde_json::Value>,
    include_extras: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(gm) = obj.get("groupMatch").and_then(|v| v.as_object()) {
        let keywords = pluck_str_array(gm, "keywords");
        if !keywords.is_empty() {
            lines.push(format!("- 群关键词：{}", join_chinese(&keywords)));
        }
        let exclude = pluck_str_array(gm, "exclude");
        if !exclude.is_empty() {
            lines.push(format!("- 排除关键词：{}", join_chinese(&exclude)));
        }
        if let Some(max) = gm.get("maxGroups").and_then(|v| v.as_i64()) {
            if max > 0 && max != 50 {
                lines.push(format!("- 最多监听群数：{max}"));
            }
        }
    }
    if let Some(style) = obj.get("responseStyle").and_then(|v| v.as_str()) {
        let label = match style {
            "professional" => "专业",
            "friendly" => "亲切",
            "concise" => "简洁",
            other => other,
        };
        lines.push(format!("- 响应风格：{label}"));
    }
    if let Some(cron) = obj.get("summaryCron").and_then(|v| v.as_str()) {
        let label = match cron {
            "daily" => "每日",
            "weekly" => "每周",
            "off" => "不汇总",
            other => other,
        };
        lines.push(format!("- 汇总频率：{label}"));
    }
    if let Some(sources) = obj.get("knowledgeSources").and_then(|v| v.as_array()) {
        let names: Vec<&str> = sources
            .iter()
            .filter_map(|s| {
                s.get("originalName")
                    .and_then(|v| v.as_str())
                    .or_else(|| s.get("path").and_then(|v| v.as_str()))
            })
            .collect();
        if !names.is_empty() {
            let preview = if names.len() <= 3 {
                names.join("、")
            } else {
                format!("{}…", names[..3].join("、"))
            };
            lines.push(format!("- 知识库（{} 份）：{}", names.len(), preview));
        }
    }
    if include_extras {
        if let Some(greeting) = obj.get("greeting").and_then(|v| v.as_str()) {
            if !greeting.trim().is_empty() {
                lines.push(format!("- 开场白：{}", greeting.trim()));
            }
        }
        if let Some(closing) = obj.get("closing").and_then(|v| v.as_str()) {
            if !closing.trim().is_empty() {
                lines.push(format!("- 结束语：{}", closing.trim()));
            }
        }
        let esc = pluck_str_array(obj, "escalationKeywords");
        if !esc.is_empty() {
            lines.push(format!("- 转人工关键词：{}", join_chinese(&esc)));
        }
        let tech = pluck_str_array(obj, "techKeywords");
        if !tech.is_empty() {
            lines.push(format!("- 技术问题关键词：{}", join_chinese(&tech)));
        }
    }
    lines
}

fn summarize_weekly_report(obj: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(template) = obj.get("template").and_then(|v| v.as_str()) {
        if !template.trim().is_empty() {
            let preview: String = template.chars().take(60).collect();
            let suffix = if template.chars().count() > 60 {
                "…"
            } else {
                ""
            };
            lines.push(format!("- 周报模板：{preview}{suffix}"));
        }
    }
    if let Some(groups) = obj.get("watchGroups").and_then(|v| v.as_array()) {
        let names: Vec<&str> = groups.iter().filter_map(|v| v.as_str()).collect();
        if !names.is_empty() {
            lines.push(format!("- 监听群：{}", join_chinese(&names)));
        }
    }
    if let Some(scope) = obj.get("scope").and_then(|v| v.as_str()) {
        lines.push(format!("- 汇总范围：{scope}"));
    }
    if let Some(lang) = obj.get("language").and_then(|v| v.as_str()) {
        if lang != "zh" {
            lines.push(format!("- 输出语言：{lang}"));
        }
    }
    lines
}

/// 当员工已配置 knowledgeSources（且模板为 xiaoke/xiaogong）时，告诉 LLM
/// 知识库已切片入 cognitive memory，应用 memory_search 检索而不是 load_file 全文。
fn knowledge_memory_hint(employee: &EmployeeRecord) -> Option<String> {
    let template_id = employee.template_id.as_deref()?;
    if !matches!(template_id, "builtin:xiaoke" | "builtin:xiaogong") {
        return None;
    }
    let sources = employee
        .resource_config
        .get("knowledgeSources")
        .and_then(|v| v.as_array())?;
    if sources.is_empty() {
        return None;
    }
    Some(format!(
        "- 知识库：已切片入 cognitive memory。请用 `memory_search` 检索答案，\
         必须传 `category=\"fact\"` 和 `tag=\"knowledge:{}\"` 两个参数以仅检索本员工的知识，\
         不要直接读 knowledgeSources[].path 全文。",
        employee.id
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn employee(skill: Option<&str>, resource: serde_json::Value) -> EmployeeRecord {
        EmployeeRecord {
            id: "emp-test".into(),
            name: "小研".into(),
            role: "竞品调研员".into(),
            description: "每周汇总竞品动态".into(),
            avatar: "🔍".into(),
            template_id: Some("builtin:xiaoyuan".into()),
            tool_whitelist: vec![],
            cron: None,
            timezone: "Asia/Shanghai".into(),
            lifecycle: crate::runtime::employee::store::EmployeeLifecycle::Active,
            cron_enabled: true,
            resource_config: resource,
            system_prompt_extra: Some("聚焦事实".into()),
            default_skill_id: skill.map(|s| s.to_string()),
            skill_ids: vec![],
            template_ref: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_run_at: None,
            next_run_at: None,
        }
    }

    #[test]
    fn includes_mandatory_immediate_start_suffix() {
        let e = employee(None, serde_json::json!({}));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(
            p.ends_with("请立即开始按职责执行，不要等待用户额外指示。"),
            "prompt did not end with mandatory suffix: {p}"
        );
    }

    #[test]
    fn omits_skill_line_when_default_skill_id_is_none() {
        let e = employee(None, serde_json::json!({}));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(
            !p.contains("默认技能"),
            "prompt unexpectedly mentioned 默认技能: {p}"
        );
    }

    #[test]
    fn omits_skill_line_when_default_skill_id_is_empty_string() {
        let e = employee(Some(""), serde_json::json!({}));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(
            !p.contains("默认技能"),
            "prompt unexpectedly mentioned 默认技能: {p}"
        );
    }

    #[test]
    fn includes_skill_line_when_default_skill_id_is_set() {
        let e = employee(Some("competitive-intelligence"), serde_json::json!({}));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(
            p.contains("skill_id=\"competitive-intelligence\""),
            "missing skill hint: {p}"
        );
    }

    #[test]
    fn multi_skill_lists_all_with_default_first() {
        let mut e = employee(Some("dingtalk-workspace"), serde_json::json!({}));
        e.skill_ids = vec![
            "sales-followup-rules".to_string(),
            "competitive-intelligence".to_string(),
        ];
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(p.contains("可用技能"), "should use multi-skill format: {p}");
        assert!(
            p.contains("dingtalk-workspace（默认，第一步请加载）"),
            "default should be marked: {p}"
        );
        assert!(
            p.contains("· sales-followup-rules"),
            "extra skill missing: {p}"
        );
        assert!(
            p.contains("· competitive-intelligence"),
            "extra skill missing: {p}"
        );
        assert!(
            p.contains("首先加载 dingtalk-workspace"),
            "instruction missing: {p}"
        );
        // Should NOT use single-skill format
        assert!(
            !p.contains("默认技能："),
            "should not use single-skill format: {p}"
        );
    }

    #[test]
    fn multi_skill_deduplicates_default_from_skill_ids() {
        let mut e = employee(Some("dingtalk-workspace"), serde_json::json!({}));
        // default_skill_id is also in skill_ids — should not be listed twice
        e.skill_ids = vec![
            "dingtalk-workspace".to_string(),
            "sales-followup-rules".to_string(),
        ];
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        let count = p.matches("dingtalk-workspace").count();
        // Appears in: "· dingtalk-workspace（默认…）" + "首先加载 dingtalk-workspace"
        assert_eq!(
            count, 2,
            "default should appear exactly twice (list + instruction), got {count}: {p}"
        );
    }

    #[test]
    fn skill_ids_only_without_default_uses_multi_format() {
        let mut e = employee(None, serde_json::json!({}));
        e.skill_ids = vec![
            "sales-followup-rules".to_string(),
            "competitive-intelligence".to_string(),
        ];
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(p.contains("可用技能"), "should use multi-skill format: {p}");
        assert!(
            p.contains("sales-followup-rules（默认，第一步请加载）"),
            "first skill should be marked as default: {p}"
        );
    }

    #[test]
    fn single_skill_in_skill_ids_uses_single_format() {
        let mut e = employee(None, serde_json::json!({}));
        e.skill_ids = vec!["sales-followup-rules".to_string()];
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(
            p.contains("默认技能：sales-followup-rules"),
            "single skill_ids entry should use single-skill format: {p}"
        );
    }

    #[test]
    fn omits_resource_config_line_when_value_is_null_or_empty_object() {
        let e1 = employee(None, serde_json::Value::Null);
        let e2 = employee(None, serde_json::json!({}));
        for e in [e1, e2] {
            let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
            assert!(!p.contains("资源配置"), "should omit resource line: {p}");
        }
    }

    #[test]
    fn includes_resource_config_line_when_object_is_non_empty() {
        let e = employee(
            None,
            serde_json::json!({"monitoringTargets": [{"name": "A", "url": "https://a"}]}),
        );
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        // xiaoyuan template now renders as a "监听目标" line, not raw JSON.
        assert!(p.contains("监听目标"), "missing monitoring summary: {p}");
        assert!(p.contains("https://a"), "monitoring url missing: {p}");
        assert!(p.contains("A（https://a）"), "name+url format missing: {p}");
        // Ensure we are NOT dumping the raw JSON key.
        assert!(
            !p.contains("monitoringTargets"),
            "raw JSON key leaked into prompt: {p}"
        );
    }

    #[test]
    fn omits_config_block_entirely_when_no_lines() {
        let e = employee(None, serde_json::json!({}));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(
            !p.contains("【本次工作配置】"),
            "config block should be omitted: {p}"
        );
    }

    #[test]
    fn user_block_skipped_when_prompt_override_is_whitespace() {
        let e = employee(None, serde_json::json!({}));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, Some("   \n\t  "), None);
        // The prompt should not contain the (whitespace-only) override anywhere.
        // The cheapest check: there should be exactly one "\n\n" separator between
        // the trigger label and the suffix (no extra blank lines for an empty user block).
        assert!(
            p.contains("[按需派活]\n\n请立即"),
            "expected immediate suffix after trigger label: {p}"
        );
    }

    #[test]
    fn user_block_included_when_prompt_override_has_content() {
        let e = employee(None, serde_json::json!({}));
        let p = build_dispatch_prompt(
            &e,
            "[按需派活]",
            None,
            Some("帮我查一下 Anthropic 的最新动态"),
            None,
        );
        assert!(
            p.contains("帮我查一下 Anthropic"),
            "user request not included: {p}"
        );
    }

    #[test]
    fn catchup_appended_to_trigger_label_with_newline() {
        let e = employee(None, serde_json::json!({}));
        let p = build_dispatch_prompt(&e, "[定时触发]", Some("（补跑，跳过了 2 次）"), None, None);
        assert!(
            p.contains("[定时触发]\n（补跑，跳过了 2 次）"),
            "catchup not properly joined: {p}"
        );
    }

    fn xiaoke(resource: serde_json::Value) -> EmployeeRecord {
        let mut e = employee(None, resource);
        e.template_id = Some("builtin:xiaoke".into());
        e.id = "emp-xk".into();
        e
    }

    #[test]
    fn knowledge_memory_hint_included_for_xiaoke_with_sources() {
        let e = xiaoke(serde_json::json!({
            "knowledgeSources": [
                { "path": "/tmp/faq.md", "originalName": "faq.md", "status": "done", "slicedCount": 12 }
            ]
        }));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(
            p.contains("memory_search"),
            "missing memory_search hint: {p}"
        );
        assert!(p.contains("knowledge:emp-xk"), "missing tag hint: {p}");
    }

    #[test]
    fn knowledge_memory_hint_omitted_when_no_sources() {
        let e = xiaoke(serde_json::json!({}));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(!p.contains("memory_search"), "unexpected memory hint: {p}");
    }

    #[test]
    fn knowledge_memory_hint_omitted_for_unrelated_template() {
        let e = employee(
            None,
            serde_json::json!({
                "knowledgeSources": [
                    { "path": "/tmp/x.md", "originalName": "x.md", "status": "done", "slicedCount": 1 }
                ]
            }),
        );
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(
            !p.contains("memory_search"),
            "hint leaked to wrong template: {p}"
        );
    }

    #[test]
    fn snapshot_system_prompt_extra_wins_over_record_field() {
        use crate::runtime::employee::template_store::{
            ensure_instance_snapshot, TemplateSnapshot,
        };
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Record says "A"; snapshot says "B" — snapshot must win.
        let mut e = employee(None, serde_json::json!({}));
        e.id = "emp-snap-wins".to_string();
        e.system_prompt_extra = Some("record-extra".into());
        let snap = TemplateSnapshot {
            template_id: "builtin:xiaoyuan".into(),
            version: "1.0.0".into(),
            name: e.name.clone(),
            avatar: e.avatar.clone(),
            role: e.role.clone(),
            description: e.description.clone(),
            badge: "".into(),
            system_prompt_extra: "SNAPSHOT-WINS".into(),
            tool_whitelist: vec![],
            cron: "".into(),
            default_skill_id: "".into(),
            skill_ids: vec![],
            requires_dingtalk: false,
            requires_attachment: serde_json::Value::Null,
            resource_config_schema: serde_json::Value::Null,
            resource_config_ui: serde_json::Value::Null,
            extra: std::collections::BTreeMap::new(),
        };
        ensure_instance_snapshot(&root.join(&e.id), &snap, "bootstrap").unwrap();

        let with_snap = build_dispatch_prompt(&e, "[按需派活]", None, None, Some(root));
        assert!(
            with_snap.contains("SNAPSHOT-WINS"),
            "snapshot not applied: {with_snap}"
        );
        assert!(
            !with_snap.contains("record-extra"),
            "record field leaked through: {with_snap}"
        );

        // Without the employees_root hint, we must fall back to the record field.
        let without_snap = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(
            without_snap.contains("record-extra"),
            "fallback to record field broken: {without_snap}"
        );
    }

    // ── PR-3: summarize_resource_config tests ─────────────────────────────

    fn emp_with_template(tid: &str, cfg: serde_json::Value) -> EmployeeRecord {
        let mut e = employee(None, cfg);
        e.template_id = Some(tid.into());
        e.id = format!("emp-{}", tid.replace(':', "-"));
        e
    }

    #[test]
    fn summarize_xiaogong_renders_group_keywords_as_plain_text() {
        let e = emp_with_template(
            "builtin:xiaogong",
            serde_json::json!({
                "groupMatch": {"keywords": ["技术", "对接", "集成"], "exclude": ["内部"], "maxGroups": 50},
                "responseStyle": "professional",
                "summaryCron": "weekly",
            }),
        );
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(
            p.contains("群关键词：技术、对接、集成"),
            "keywords missing: {p}"
        );
        assert!(p.contains("排除关键词：内部"), "exclude missing: {p}");
        assert!(p.contains("响应风格：专业"), "style label missing: {p}");
        assert!(p.contains("汇总频率：每周"), "summary cron missing: {p}");
        // raw JSON keys must not leak
        assert!(!p.contains("groupMatch"), "raw key groupMatch leaked: {p}");
        assert!(
            !p.contains("responseStyle"),
            "raw key responseStyle leaked: {p}"
        );
    }

    #[test]
    fn summarize_xiaoke_includes_extras() {
        let e = emp_with_template(
            "builtin:xiaoke",
            serde_json::json!({
                "groupMatch": {"keywords": ["售后"], "exclude": [], "maxGroups": 50},
                "responseStyle": "friendly",
                "greeting": "您好，我是 AI 客服",
                "closing": "祝您生活愉快",
                "escalationKeywords": ["投诉", "退款"],
                "techKeywords": ["bug"],
            }),
        );
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(p.contains("群关键词：售后"), "keywords missing: {p}");
        assert!(p.contains("响应风格：亲切"), "style missing: {p}");
        assert!(
            p.contains("开场白：您好，我是 AI 客服"),
            "greeting missing: {p}"
        );
        assert!(p.contains("结束语：祝您生活愉快"), "closing missing: {p}");
        assert!(
            p.contains("转人工关键词：投诉、退款"),
            "escalation missing: {p}"
        );
        assert!(p.contains("技术问题关键词：bug"), "tech kw missing: {p}");
    }

    #[test]
    fn summarize_xiaoyuan_renders_monitoring_targets() {
        let e = emp_with_template(
            "builtin:xiaoyuan",
            serde_json::json!({
                "monitoringTargets": [
                    {"name": "Anthropic", "url": "https://anthropic.com"},
                    {"name": "OpenAI", "url": "https://openai.com"},
                ]
            }),
        );
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(p.contains("监听目标（2 个）"), "count missing: {p}");
        assert!(
            p.contains("Anthropic（https://anthropic.com）"),
            "first target missing: {p}"
        );
        assert!(
            p.contains("OpenAI（https://openai.com）"),
            "second target missing: {p}"
        );
    }

    #[test]
    fn summarize_xiaobiao_renders_dingtalk_table() {
        let e = emp_with_template(
            "builtin:xiaobiao",
            serde_json::json!({
                "baseId": "BASE123",
                "tableId": "TBL456",
                "scope": "all",
                "fieldMapping": {"客户名": "name", "金额": "amount"},
            }),
        );
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(p.contains("baseId=BASE123"), "base missing: {p}");
        assert!(p.contains("tableId=TBL456"), "table missing: {p}");
        assert!(p.contains("写入范围：all"), "scope missing: {p}");
        assert!(p.contains("客户名→name"), "mapping missing: {p}");
    }

    #[test]
    fn summarize_xiaobiao_with_only_base_id_hints_user_followup() {
        let e = emp_with_template("builtin:xiaobiao", serde_json::json!({"baseId": "BASE123"}));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(
            p.contains("tableId 未指定"),
            "expected user-followup hint when tableId missing: {p}"
        );
    }

    #[test]
    fn summarize_xiaozhou_renders_weekly_report_config() {
        let e = emp_with_template(
            "builtin:xiaozhou",
            serde_json::json!({
                "template": "本周完成事项 / 下周计划 / 风险",
                "watchGroups": ["研发周报", "产品周报"],
                "scope": "team",
                "language": "zh",
            }),
        );
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        assert!(
            p.contains("周报模板：本周完成事项"),
            "template missing: {p}"
        );
        assert!(
            p.contains("监听群：研发周报、产品周报"),
            "groups missing: {p}"
        );
        assert!(p.contains("汇总范围：team"), "scope missing: {p}");
        // language=zh is default → not rendered
        assert!(
            !p.contains("输出语言"),
            "default language should not be rendered: {p}"
        );
    }

    #[test]
    fn unknown_template_falls_back_to_raw_json() {
        let e = emp_with_template("custom:org-xyz", serde_json::json!({"custom_field": 42}));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, None);
        // Fallback must keep the JSON so the LLM still has the data.
        assert!(
            p.contains("资源配置") && p.contains("custom_field"),
            "fallback to raw JSON broken: {p}"
        );
    }

    // ── PR-10: attachment hint ─────────────────────────────────────────────

    #[test]
    fn attachment_hint_injected_when_snapshot_requires_attachment() {
        use crate::runtime::employee::template_store::{
            ensure_instance_snapshot, TemplateSnapshot,
        };
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let mut e = employee(None, serde_json::json!({}));
        e.id = "emp-xiaofa".into();
        e.template_id = Some("builtin:xiaofa".into());
        let snap = TemplateSnapshot {
            template_id: "builtin:xiaofa".into(),
            version: "1.0.0".into(),
            name: e.name.clone(),
            avatar: e.avatar.clone(),
            role: e.role.clone(),
            description: e.description.clone(),
            badge: "".into(),
            system_prompt_extra: "".into(),
            tool_whitelist: vec![],
            cron: "".into(),
            default_skill_id: "".into(),
            skill_ids: vec![],
            requires_dingtalk: false,
            requires_attachment: serde_json::json!({
                "accept": ".pdf,.docx",
                "min": 1,
                "max": 5,
            }),
            resource_config_schema: serde_json::Value::Null,
            resource_config_ui: serde_json::Value::Null,
            extra: std::collections::BTreeMap::new(),
        };
        ensure_instance_snapshot(&root.join(&e.id), &snap, "bootstrap").unwrap();
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, Some(root));
        assert!(p.contains("【附件】"), "missing attachment block: {p}");
        assert!(p.contains(".pdf,.docx"), "accept missing: {p}");
        assert!(
            p.contains("1–5 份") || p.contains("1-5 份"),
            "count phrase missing: {p}"
        );
        assert!(p.contains("拖入对话框"), "guide phrasing missing: {p}");
    }

    #[test]
    fn attachment_hint_absent_when_snapshot_has_no_requirement() {
        use crate::runtime::employee::template_store::{
            ensure_instance_snapshot, TemplateSnapshot,
        };
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let mut e = employee(None, serde_json::json!({}));
        e.id = "emp-no-attach".into();
        let snap = TemplateSnapshot {
            template_id: "builtin:xiaoyuan".into(),
            version: "1.0.0".into(),
            name: e.name.clone(),
            avatar: e.avatar.clone(),
            role: e.role.clone(),
            description: e.description.clone(),
            badge: "".into(),
            system_prompt_extra: "".into(),
            tool_whitelist: vec![],
            cron: "".into(),
            default_skill_id: "".into(),
            skill_ids: vec![],
            requires_dingtalk: false,
            requires_attachment: serde_json::Value::Null,
            resource_config_schema: serde_json::Value::Null,
            resource_config_ui: serde_json::Value::Null,
            extra: std::collections::BTreeMap::new(),
        };
        ensure_instance_snapshot(&root.join(&e.id), &snap, "bootstrap").unwrap();
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None, Some(root));
        assert!(
            !p.contains("【附件】"),
            "attachment block should NOT appear when no requirement: {p}"
        );
    }
}
