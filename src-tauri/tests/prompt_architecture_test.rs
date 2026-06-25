use app_lib::llm::prompts;
use app_lib::runtime::chat::context_builder::build_iteration_context;
use app_lib::runtime::chat::prompt::{PromptAssembler, PromptBuildContext, ReminderBuilder};
use app_lib::runtime::tools::catalog::ToolCatalog;

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, LazyLock, Mutex,
};

use app_lib::runtime::chat::prompt::{
    PromptAssembly, PromptBlock, PromptCachePolicy, PromptDiagnostics, PromptSectionCache,
    PromptSectionId, PromptSectionSpec, TurnPromptSnapshot,
};

static PROMPT_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[test]
fn prompt_assembly_keeps_static_blocks_before_dynamic_blocks() {
    let assembly = PromptAssembly::new(vec![
        PromptBlock::static_block(PromptSectionId::new("intro"), "static intro"),
        PromptBlock::dynamic_block(PromptSectionId::new("persona"), "dynamic persona"),
    ]);

    let payload = assembly.to_system_view();
    assert_eq!(payload.blocks.len(), 2);
    assert_eq!(payload.blocks[0].text, "static intro");
    assert_eq!(
        payload.blocks[0].cache_policy,
        PromptCachePolicy::StaticPrefix
    );
    assert_eq!(payload.blocks[1].text, "dynamic persona");
    assert_eq!(
        payload.blocks[1].cache_policy,
        PromptCachePolicy::SessionDynamic
    );
}

#[test]
fn prompt_section_cache_reuses_session_dynamic_sections() {
    let cache = PromptSectionCache::new();
    let section_id = PromptSectionId::new("env_info_simple");

    let first = cache.get_or_insert(section_id.clone(), || "env-v1".to_string());
    let second = cache.get_or_insert(section_id.clone(), || "env-v2".to_string());

    assert_eq!(first, "env-v1");
    assert_eq!(second, "env-v1");

    cache.clear();
    let third = cache.get_or_insert(section_id, || "env-v3".to_string());
    assert_eq!(third, "env-v3");
}

#[test]
fn prompt_section_cache_does_not_compute_cache_hits() {
    let cache = PromptSectionCache::new();
    let section_id = PromptSectionId::new("env_info_simple");
    let compute_count = Arc::new(AtomicUsize::new(0));

    let first_count = Arc::clone(&compute_count);
    let first = cache.get_or_insert(section_id.clone(), || {
        first_count.fetch_add(1, Ordering::SeqCst);
        "env-v1".to_string()
    });

    let second_count = Arc::clone(&compute_count);
    let second = cache.get_or_insert(section_id, || {
        second_count.fetch_add(1, Ordering::SeqCst);
        "env-v2".to_string()
    });

    assert_eq!(first, "env-v1");
    assert_eq!(second, "env-v1");
    assert_eq!(compute_count.load(Ordering::SeqCst), 1);
}

#[test]
fn volatile_section_spec_requires_reason() {
    let spec = PromptSectionSpec::volatile(
        PromptSectionId::new("mcp_instructions_delta"),
        "MCP servers connect and disconnect between turns",
    );

    assert_eq!(spec.cache_policy, PromptCachePolicy::Volatile);
    assert_eq!(
        spec.cache_break_reason.as_deref(),
        Some("MCP servers connect and disconnect between turns")
    );
}

#[test]
fn prompt_diagnostics_reports_section_lengths_and_cache_policy() {
    let assembly = PromptAssembly::new(vec![
        PromptBlock::static_block(PromptSectionId::new("base"), "abc"),
        PromptBlock::dynamic_block(PromptSectionId::new("daily"), "defg"),
    ]);

    let report = PromptDiagnostics::from_assembly(&assembly);
    assert_eq!(report.total_chars, 7);
    assert_eq!(report.sections.len(), 2);
    assert_eq!(report.sections[0].section_id, "base");
    assert_eq!(report.sections[0].chars, 3);
    assert_eq!(report.sections[0].cache_policy, "static_prefix");
    assert_eq!(report.sections[1].cache_break_reason, None);
}

#[test]
fn prompt_assembler_places_base_before_dynamic_daily_prompt() {
    let _guard = PROMPT_TEST_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let bundled = tmp.path().join("bundled");
    let user = tmp.path().join("user");
    std::fs::create_dir_all(bundled.join("prompts")).unwrap();
    std::fs::create_dir_all(&user).unwrap();
    std::fs::write(bundled.join("prompts/base.md"), "AI小家 base").unwrap();
    std::fs::write(bundled.join("prompts/daily.md"), "daily prompt").unwrap();
    prompts::init_prompts(&bundled, &user);

    let assembler = PromptAssembler::default();
    let assembly = assembler.build_system_prompt(PromptBuildContext {
        persona: None,
        product_name: None,
    });

    let blocks = assembly.blocks();
    assert!(blocks[0].text.contains("AI小家 base"));
    assert_eq!(blocks[0].cache_policy, PromptCachePolicy::StaticPrefix);
    assert!(blocks
        .iter()
        .any(|block| block.text.contains("daily prompt")));
    assert!(blocks
        .iter()
        .any(|block| block.cache_policy == PromptCachePolicy::SessionDynamic));
}

#[test]
fn prompt_assembler_strips_daily_memory_whitelist_when_persona_memory_hints_exist() {
    let _guard = PROMPT_TEST_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let bundled = tmp.path().join("bundled");
    let user = tmp.path().join("user");
    std::fs::create_dir_all(bundled.join("prompts")).unwrap();
    std::fs::create_dir_all(&user).unwrap();
    std::fs::write(bundled.join("prompts/base.md"), "AI小家 base").unwrap();
    std::fs::write(
        bundled.join("prompts/daily.md"),
        "daily intro\n记忆管理（白名单制）\n- old memory hint\n- another old hint\n\n后续章节\nkeep this section",
    )
    .unwrap();
    prompts::init_prompts(&bundled, &user);

    let persona = app_lib::storage::file_store::persona::Persona {
        id: "persona".to_string(),
        version: 1,
        builtin: false,
        name: "Persona".to_string(),
        icon: "P".to_string(),
        description: "desc".to_string(),
        name_en: String::new(),
        description_en: String::new(),
        identity: "persona identity".to_string(),
        expertise: vec![],
        memory_hints: vec!["new memory hint".to_string()],
        linked_categories: vec![],
        created_at: "2026-01-01".to_string(),
        updated_at: "2026-01-01".to_string(),
    };

    let parts = prompts::build_system_prompt_parts(Some(&persona), None);

    assert!(parts.dynamic_section.contains("daily intro"));
    assert!(parts.dynamic_section.contains("new memory hint"));
    assert!(!parts.dynamic_section.contains("old memory hint"));
    assert!(parts.dynamic_section.contains("后续章节"));
    assert!(parts.dynamic_section.contains("keep this section"));
}

#[test]
fn prompt_assembler_matches_legacy_daily_prompt_parts() {
    let _guard = PROMPT_TEST_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let bundled = tmp.path().join("bundled");
    let user = tmp.path().join("user");
    std::fs::create_dir_all(bundled.join("prompts")).unwrap();
    std::fs::create_dir_all(&user).unwrap();
    std::fs::write(bundled.join("prompts/base.md"), "AI小家 base").unwrap();
    std::fs::write(bundled.join("prompts/daily.md"), "daily prompt").unwrap();
    prompts::init_prompts(&bundled, &user);

    let assembler = PromptAssembler::default();
    let assembly = assembler.build_system_prompt(PromptBuildContext {
        persona: None,
        product_name: Some("Lotus"),
    });
    let parts = prompts::build_system_prompt_parts(None, Some("Lotus"));
    let legacy_prompt = if parts.dynamic_section.is_empty() {
        parts.static_section
    } else {
        format!("{}\n\n{}", parts.static_section, parts.dynamic_section)
    };

    assert_eq!(assembly.flatten(), legacy_prompt);
}

#[test]
fn reminder_builder_outputs_system_reminder_user_message() {
    let message = ReminderBuilder::date_message("2026年04月26日", "2026-04-26", "星期日");

    assert_eq!(
        message,
        serde_json::json!({
            "role": "user",
            "content": "<system-reminder>\n今天是 2026年04月26日 星期日（2026-04-26）。\n</system-reminder>",
        })
    );
}

#[test]
fn reminder_builder_outputs_date_time_system_reminder_user_message() {
    let message = ReminderBuilder::date_time_message("2026年04月26日", "2026-04-26", "14:23:45");

    assert_eq!(
        message,
        serde_json::json!({
            "role": "user",
            "content": "<system-reminder>\n当前本地时间是 2026年04月26日 14:23:45（2026-04-26 14:23:45）。\n</system-reminder>",
        })
    );
}

#[test]
fn reminder_builder_context_message_preserves_legacy_meta_contract() {
    let message = ReminderBuilder::context_message("agentsMd", "- file.md: 摘要").unwrap();

    assert_eq!(message["role"], "user");
    assert_eq!(message["isMeta"], true);
    assert_eq!(
        message["content"],
        "<system-reminder>\nAs you answer the user's questions, you can use the following context:\n# agentsMd\n- file.md: 摘要\n\nIMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.\n</system-reminder>\n"
    );
}

#[test]
fn reminder_builder_context_message_omits_blank_body() {
    assert!(ReminderBuilder::context_message("agentsMd", "  \n\t").is_none());
}

#[test]
fn turn_prompt_snapshot_exposes_flattened_compat_prompt() {
    let assembly = PromptAssembly::new(vec![
        PromptBlock::static_block(PromptSectionId::new("base"), "base"),
        PromptBlock::dynamic_block(PromptSectionId::new("daily"), "daily"),
    ]);
    let snapshot = TurnPromptSnapshot::new(assembly, vec![]);

    assert_eq!(snapshot.compat_system_prompt(), "base\n\ndaily");
    assert_eq!(snapshot.system_view().blocks.len(), 2);
}

#[test]
fn iteration_context_contains_only_runtime_delta_sections() {
    let result = build_iteration_context(
        "",
        "",
        "\n\n[当前环境]\n工作目录: /tmp/project",
        "",
        "",
        None,
        None,
        "",
    );

    assert!(result.starts_with("[动态上下文 — 请勿回复此消息]"));
    assert!(result.contains("[当前环境]"));
    // Stateful workflow precompute pipeline removed in Phase B Task 7.
    assert!(!result.contains("[precompute_result]"));
    assert!(!result.contains("【工具选择偏好】"));
    assert!(!result.contains("【记忆管理】"));
}

#[test]
fn default_system_prompt_uses_base_prompt() {
    let base = app_lib::runtime::chat::base_prompt::DAILY_BASE_PROMPT;
    assert!(
        base.contains("你是 AI小家"),
        "base prompt must identify as AI小家"
    );
    assert!(
        !base.contains("daily-assistant"),
        "base prompt must not reference old daily-assistant"
    );
    assert!(
        !base.contains("switch_skill"),
        "base prompt must not reference switch_skill"
    );
}

#[test]
fn prompt_boundary_copy_does_not_expose_internal_mode_switching() {
    let parts = prompts::build_system_prompt_parts(None, None);
    let prompt = format!("{}\n\n{}", parts.static_section, parts.dynamic_section);
    let forbidden = [
        "当前模式",
        "隐藏模式",
        "模式切换",
        "进入模式",
        "退出模式",
        "切换模式",
        "需启动对应技能",
    ];

    for marker in forbidden {
        assert!(
            !prompt.contains(marker),
            "system prompt should not expose internal mode/switching wording: {marker}"
        );
    }
}

#[test]
fn prompt_identity_copy_keeps_digital_employee_internal() {
    let parts = prompts::build_system_prompt_parts(None, None);
    let prompt = format!("{}\n\n{}", parts.static_section, parts.dynamic_section);

    for marker in ["数字员工", "digital employee", "虚拟协作者"] {
        assert!(
            !prompt.contains(marker),
            "system prompt should not expose internal product identity wording: {marker}"
        );
    }
}

#[test]
fn prompt_states_internal_capabilities_as_user_facing_principle() {
    let parts = prompts::build_system_prompt_parts(None, None);
    let prompt = format!("{}\n\n{}", parts.static_section, parts.dynamic_section);

    assert!(
        prompt.contains("内部能力名称") && prompt.contains("普通用户"),
        "system prompt should keep internal capability names behind user-facing wording"
    );

    for marker in ["用户问你是谁", "你的角色定义", "用户询问模式", "不使用需要用户手动切换"] {
        assert!(
            !prompt.contains(marker),
            "system prompt should not hard-code scripted self-description answers: {marker}"
        );
    }
}

#[test]
fn skill_tool_description_keeps_skill_selection_internal() {
    let catalog = ToolCatalog::default_catalog();
    let skill = catalog.get("Skill").expect("Skill must exist in catalog");

    assert!(skill.description.contains("内部参考"));
    assert!(skill.description.contains("业务语言承接"));

    for marker in ["不改变系统提示", "模式", "切换"] {
        assert!(
            !skill.description.contains(marker),
            "Skill description should not invite user-visible routing wording: {marker}"
        );
    }
}
