//! Plugin health-check regression tests (专项 3: Skill / Workflow 历史债务清理)
//!
//! 覆盖目标：
//! A1) 全量 workflow.toml 语法审计 — 所有 plugins/* 里的 workflow.toml 都能被解析
//! A2) 历史坏 skill 真实加载审计 — 5 个 skill 走完整路径:
//!       plugin.toml → parse_plugin_manifest → DeclarativeSkill::load
//! B)  skill 模板生成的 workflow 可解析（引用真实常量，无复制字符串）
//! C)  optional extract prompt 缺失不失败 — 没有 base_extract.md 时仍能构造 DeclarativeSkill

use app_lib::plugin::manifest::parse_workflow_manifest;

/// ─── 测试 A + D：全量 workflow.toml 解析（含 5 个历史坏 skill 回归）──────────
///
/// 扫描 src-tauri/plugins/ 下所有存在 workflow.toml 的插件目录，
/// 断言每一个都能被 parse_workflow_manifest 成功解析。
/// 5 个历史破坏的 skill 必须在此测试中通过（不能再悄悄失败）。
#[test]
fn all_plugin_workflow_toml_files_parse_successfully() {
    let plugins_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins");
    assert!(
        plugins_dir.exists(),
        "plugins/ directory not found at {:?}",
        plugins_dir
    );

    // 5 个历史坏 skill，明确点名进入回归名单
    let historically_broken = [
        "customer-segmentation",
        "user-behavior",
        "survey-analysis",
        "ops-analysis",
        "sales-analysis",
    ];

    let entries = std::fs::read_dir(&plugins_dir)
        .expect("failed to read plugins/")
        .flatten()
        .collect::<Vec<_>>();

    let mut checked = 0usize;
    let mut historically_broken_checked = std::collections::HashSet::new();
    let mut failures: Vec<String> = Vec::new();

    for entry in &entries {
        let plugin_dir = entry.path();
        if !plugin_dir.is_dir() {
            continue;
        }
        let workflow_path = plugin_dir.join("workflow.toml");
        if !workflow_path.exists() {
            continue;
        }

        let plugin_name = plugin_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let content = std::fs::read_to_string(&workflow_path)
            .unwrap_or_else(|e| panic!("cannot read {:?}: {}", workflow_path, e));

        match parse_workflow_manifest(&content) {
            Ok(_) => {
                checked += 1;
                if historically_broken.contains(&plugin_name.as_str()) {
                    historically_broken_checked.insert(plugin_name.clone());
                }
            }
            Err(e) => {
                failures.push(format!(
                    "[{}] workflow.toml parse error: {}",
                    plugin_name, e
                ));
            }
        }
    }

    // 确保我们确实扫描到了文件（不允许零文件通过）
    assert!(
        checked > 0,
        "No workflow.toml files were found and checked — something is wrong with the test setup"
    );

    // 全量 parse 失败报告
    assert!(
        failures.is_empty(),
        "workflow.toml parse failures detected:\n{}",
        failures.join("\n")
    );

    // 5 个历史坏 skill 都必须存在且通过
    for broken_name in &historically_broken {
        assert!(
            historically_broken_checked.contains(*broken_name),
            "Historically broken skill '{}' was NOT found/checked — \
             either the plugin directory is missing or workflow.toml is absent",
            broken_name
        );
    }
}

/// ─── 测试 B：skill 脚手架模板生成的 workflow 可解析 ────────────────────────
///
/// 直接引用 skill_management::SCAFFOLD_WORKFLOW_TOML 常量，
/// 保证测试与生产模板永远指向同一份源，无需手工同步复制字符串。
#[test]
fn skill_scaffold_template_workflow_parses_correctly() {
    use app_lib::commands::skill_management::SCAFFOLD_WORKFLOW_TOML;

    let result = parse_workflow_manifest(SCAFFOLD_WORKFLOW_TOML);
    assert!(
        result.is_ok(),
        "skill scaffold template workflow.toml failed to parse: {:?}",
        result.err()
    );

    let manifest = result.unwrap();
    assert_eq!(manifest.steps.len(), 3, "scaffold should have 3 steps");
    assert_eq!(manifest.steps[0].id, "step0");
    assert_eq!(manifest.steps[1].id, "step1");
    assert_eq!(manifest.steps[2].id, "step2");

    // step1 应有 tools_on_feedback 作为数组（而非嵌套 table）
    let step1 = &manifest.steps[1];
    assert!(
        step1.tools_on_feedback.is_some(),
        "step1 should have tools_on_feedback"
    );
    assert_eq!(
        step1.tools_on_feedback.as_ref().unwrap(),
        &["execute_python", "export_data"]
    );
    assert_eq!(step1.max_iterations_feedback, Some(3));
}

/// ─── 测试 C：missing optional extract prompt 不应导致 DeclarativeSkill 构造失败 ──
///
/// 创建一个最小的临时插件目录（无 prompts/extract/ 目录），
/// 验证 DeclarativeSkill::load 成功，且 extract_prompt() 返回两个空字符串。
#[test]
fn missing_optional_extract_prompt_does_not_fail_skill_load() {
    use app_lib::plugin::declarative_skill::DeclarativeSkill;
    use app_lib::plugin::manifest::parse_plugin_manifest;
    use app_lib::plugin::skill_trait::Skill;

    let tmp = tempfile::tempdir().expect("failed to create tempdir");
    let plugin_dir = tmp.path();

    // 写 plugin.toml
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        r#"[plugin]
id = "test-no-extract"
name = "Test No Extract"
type = "skill"

[trigger]
keywords = ["test"]
"#,
    )
    .unwrap();

    // 写 workflow.toml（合法格式）
    std::fs::write(
        plugin_dir.join("workflow.toml"),
        r#"[[steps]]
id = "step0"
name = "Step 0"
tools_only = ["save_analysis_note"]
max_iterations = 3
advance_on = "any"
"#,
    )
    .unwrap();

    // 故意不创建 prompts/extract/ 目录或 base_extract.md

    let plugin_toml = std::fs::read_to_string(plugin_dir.join("plugin.toml")).unwrap();
    let manifest = parse_plugin_manifest(&plugin_toml).unwrap();

    // 构造 DeclarativeSkill 不应失败
    let skill = DeclarativeSkill::load(&manifest, plugin_dir)
        .expect("DeclarativeSkill::load should succeed even without extract prompts");

    // extract_prompt 应返回两个空字符串，而非 panic 或错误
    let (base, step_specific) = skill.extract_prompt("step0");
    assert!(
        base.is_empty(),
        "extract_prompt base should be empty when base_extract.md absent, got: {:?}",
        base
    );
    assert!(
        step_specific.is_empty(),
        "extract_prompt step_specific should be empty when extract_step0.md absent, got: {:?}",
        step_specific
    );
}

/// ─── 测试 A2：历史坏 skill 真实加载审计 ────────────────────────────────────
///
/// 对 5 个历史坏 skill 逐一执行完整加载路径：
///   1. 读取 plugin.toml
///   2. parse_plugin_manifest(...)
///   3. DeclarativeSkill::load(...)
///   4. smoke-assert: id() 正确、workflow() 存在且至少有 4 步、
///      allowed_tool_names() 在 step0 可调用（不 panic）
///
/// 这比仅解析 workflow.toml 强：真实 loader 路径上的任何回归
/// （plugin.toml 格式、prompt 构造、step config 解析）都会在此暴露。
#[test]
fn historically_broken_skills_load_successfully_via_declarative_skill() {
    use app_lib::plugin::declarative_skill::DeclarativeSkill;
    use app_lib::plugin::manifest::parse_plugin_manifest;
    use app_lib::plugin::skill_trait::{Skill, SkillState};

    let plugins_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins");

    // 5 个历史坏 skill — 全部点名，必须真实加载
    let historically_broken = [
        ("customer-segmentation", "客户细分分析"),
        ("user-behavior", "用户行为分析"),
        ("survey-analysis", "问卷调研分析"),
        ("ops-analysis", "运营数据分析"),
        ("sales-analysis", "销售数据分析"),
    ];

    for (plugin_id, expected_name_fragment) in &historically_broken {
        let plugin_dir = plugins_dir.join(plugin_id);
        assert!(
            plugin_dir.is_dir(),
            "[{}] plugin directory not found at {:?}",
            plugin_id,
            plugin_dir
        );

        // Step 1: 读取并解析 plugin.toml
        let plugin_toml_path = plugin_dir.join("plugin.toml");
        assert!(
            plugin_toml_path.exists(),
            "[{}] plugin.toml not found",
            plugin_id
        );
        let plugin_toml = std::fs::read_to_string(&plugin_toml_path)
            .unwrap_or_else(|e| panic!("[{}] cannot read plugin.toml: {}", plugin_id, e));

        let manifest = parse_plugin_manifest(&plugin_toml)
            .unwrap_or_else(|e| panic!("[{}] parse_plugin_manifest failed: {}", plugin_id, e));

        assert_eq!(
            manifest.plugin.id, *plugin_id,
            "[{}] plugin.id mismatch in plugin.toml",
            plugin_id
        );

        // Step 2: DeclarativeSkill::load — 真实 loader 路径
        let skill = DeclarativeSkill::load(&manifest, &plugin_dir)
            .unwrap_or_else(|e| panic!("[{}] DeclarativeSkill::load failed: {}", plugin_id, e));

        // Step 3: smoke asserts
        assert_eq!(
            skill.id(),
            *plugin_id,
            "[{}] skill.id() mismatch after load",
            plugin_id
        );

        assert!(
            skill.display_name().contains(expected_name_fragment),
            "[{}] display_name '{}' should contain '{}'",
            plugin_id,
            skill.display_name(),
            expected_name_fragment
        );

        // workflow 必须存在且有步骤（这些 skill 都是多步工作流）
        let wf = skill.workflow().unwrap_or_else(|| {
            panic!(
                "[{}] workflow() returned None — plugin should have a workflow",
                plugin_id
            )
        });
        assert!(
            wf.steps.len() >= 4,
            "[{}] workflow should have at least 4 steps, got {}",
            plugin_id,
            wf.steps.len()
        );

        // allowed_tool_names 在 step0 应返回 Some（step0 均定义了 tools_only）
        let state0 = SkillState {
            current_step: Some("step0".into()),
            ..SkillState::new(plugin_id)
        };
        assert!(
            skill.allowed_tool_names(&state0).is_some(),
            "[{}] allowed_tool_names(step0) should be Some — step0 has tools_only in workflow",
            plugin_id
        );

        // step1 应有 tools_on_feedback（这正是历史上配错的字段）
        let state1 = SkillState {
            current_step: Some("step1".into()),
            ..SkillState::new(plugin_id)
        };
        let fb = skill.feedback_config(&state1).unwrap_or_else(|| {
            panic!(
                "[{}] feedback_config(step1) returned None — \
                 tools_on_feedback should be present after the fix",
                plugin_id
            )
        });
        assert!(
            !fb.tools.is_empty(),
            "[{}] feedback_config(step1).tools should not be empty",
            plugin_id
        );
    }
}
