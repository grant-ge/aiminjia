//! Prompt assembly for employee dispatch.
//!
//! Pure string construction over `EmployeeRecord` + the per-trigger inputs
//! (trigger label, catchup info, optional user request). Lives outside the
//! transport layer so it can be unit-tested in isolation — the prompt format
//! is the load-bearing piece that drives whether the LLM starts working
//! immediately or waits for further instructions.

use crate::runtime::employee::store::EmployeeRecord;

/// Build the user-facing prompt sent on employee dispatch.
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
) -> String {
    let identity_block = format!(
        "你现在是「{}」（{}）。\n{}\n",
        employee.name, employee.role, employee.description
    );
    let extra = employee.system_prompt_extra.as_deref().unwrap_or("");

    let catchup = catchup_info
        .map(|s| format!("\n{s}"))
        .unwrap_or_default();

    let mut config_lines: Vec<String> = Vec::new();
    if let Some(skill_id) = employee.default_skill_id.as_deref() {
        if !skill_id.is_empty() {
            config_lines.push(format!(
                "- 默认技能：{skill_id} —— 请第一步调用 load_skill('{skill_id}') 加载工作流"
            ));
        }
    }
    let resource_config_is_empty = match &employee.resource_config {
        serde_json::Value::Null => true,
        serde_json::Value::Object(map) => map.is_empty(),
        _ => false,
    };
    if !resource_config_is_empty {
        let json = serde_json::to_string(&employee.resource_config)
            .unwrap_or_else(|_| "{}".to_string());
        config_lines.push(format!("- 资源配置：{json}"));
    }
    let config_block = if config_lines.is_empty() {
        String::new()
    } else {
        format!("\n\n【本次工作配置】\n{}", config_lines.join("\n"))
    };

    let user_request = prompt_override.unwrap_or("").trim();
    let user_block = if user_request.is_empty() {
        String::new()
    } else {
        format!("\n\n{user_request}")
    };

    format!(
        "{identity_block}{extra}\n{trigger_label}{catchup}{user_block}{config_block}\n\n请立即开始按职责执行，不要等待用户额外指示。"
    )
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
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_run_at: None,
            next_run_at: None,
        }
    }

    #[test]
    fn includes_mandatory_immediate_start_suffix() {
        let e = employee(None, serde_json::json!({}));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None);
        assert!(
            p.ends_with("请立即开始按职责执行，不要等待用户额外指示。"),
            "prompt did not end with mandatory suffix: {p}"
        );
    }

    #[test]
    fn omits_skill_line_when_default_skill_id_is_none() {
        let e = employee(None, serde_json::json!({}));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None);
        assert!(!p.contains("默认技能"), "prompt unexpectedly mentioned 默认技能: {p}");
    }

    #[test]
    fn omits_skill_line_when_default_skill_id_is_empty_string() {
        let e = employee(Some(""), serde_json::json!({}));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None);
        assert!(!p.contains("默认技能"), "prompt unexpectedly mentioned 默认技能: {p}");
    }

    #[test]
    fn includes_skill_line_when_default_skill_id_is_set() {
        let e = employee(Some("competitive-intelligence"), serde_json::json!({}));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None);
        assert!(p.contains("load_skill('competitive-intelligence')"), "missing skill hint: {p}");
    }

    #[test]
    fn omits_resource_config_line_when_value_is_null_or_empty_object() {
        let e1 = employee(None, serde_json::Value::Null);
        let e2 = employee(None, serde_json::json!({}));
        for e in [e1, e2] {
            let p = build_dispatch_prompt(&e, "[按需派活]", None, None);
            assert!(!p.contains("资源配置"), "should omit resource line: {p}");
        }
    }

    #[test]
    fn includes_resource_config_line_when_object_is_non_empty() {
        let e = employee(
            None,
            serde_json::json!({"monitoringTargets": [{"name": "A", "url": "https://a"}]}),
        );
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None);
        assert!(p.contains("资源配置"), "missing resource line: {p}");
        assert!(p.contains("monitoringTargets"), "resource JSON missing field: {p}");
    }

    #[test]
    fn omits_config_block_entirely_when_no_lines() {
        let e = employee(None, serde_json::json!({}));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, None);
        assert!(!p.contains("【本次工作配置】"), "config block should be omitted: {p}");
    }

    #[test]
    fn user_block_skipped_when_prompt_override_is_whitespace() {
        let e = employee(None, serde_json::json!({}));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, Some("   \n\t  "));
        // The prompt should not contain the (whitespace-only) override anywhere.
        // The cheapest check: there should be exactly one "\n\n" separator between
        // the trigger label and the suffix (no extra blank lines for an empty user block).
        assert!(p.contains("[按需派活]\n\n请立即"), "expected immediate suffix after trigger label: {p}");
    }

    #[test]
    fn user_block_included_when_prompt_override_has_content() {
        let e = employee(None, serde_json::json!({}));
        let p = build_dispatch_prompt(&e, "[按需派活]", None, Some("帮我查一下 Anthropic 的最新动态"));
        assert!(p.contains("帮我查一下 Anthropic"), "user request not included: {p}");
    }

    #[test]
    fn catchup_appended_to_trigger_label_with_newline() {
        let e = employee(None, serde_json::json!({}));
        let p = build_dispatch_prompt(&e, "[定时触发]", Some("（补跑，跳过了 2 次）"), None);
        assert!(p.contains("[定时触发]\n（补跑，跳过了 2 次）"), "catchup not properly joined: {p}");
    }
}
