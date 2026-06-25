use app_lib::runtime::tools::definition::{ToolDefinition, ToolKind};

#[test]
fn tool_definition_has_kind_field() {
    let def = ToolDefinition::new("WebSearch", "Search the web").with_kind(ToolKind::Primitive);
    assert!(matches!(def.kind, ToolKind::Primitive));
}

#[test]
fn tool_kind_default_is_primitive() {
    let def = ToolDefinition::new("echo", "Echo test");
    assert!(matches!(def.kind, ToolKind::Primitive));
}

#[test]
fn spawn_subagent_kind_is_composite() {
    let def = ToolDefinition::new("Agent", "Launch sub-agent").with_kind(ToolKind::Composite);
    assert!(matches!(def.kind, ToolKind::Composite));
}

#[test]
fn all_new_plan_c_tools_are_in_catalog() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    for id in &["Write", "Edit", "Bash", "Grep"] {
        assert!(
            TOOL_CATALOG.get(id).is_some(),
            "Tool '{id}' should be registered in TOOL_CATALOG"
        );
    }
}

#[test]
fn ask_user_question_catalog_forbids_model_supplied_other_option() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    let entry = TOOL_CATALOG
        .get_entry("AskUserQuestion")
        .expect("AskUserQuestion should be registered in TOOL_CATALOG");

    let description = &entry.definition.description;
    assert!(
        description.contains("不要在 options 中添加")
            && description.contains("其他")
            && description.contains("Other"),
        "AskUserQuestion description must tell the model not to add custom/Other options: {description}"
    );

    let options_description = entry.json_schema["properties"]["questions"]["items"]["properties"]
        ["options"]["description"]
        .as_str()
        .expect("AskUserQuestion options schema should describe option constraints");
    assert!(
        options_description.contains("不要添加")
            && options_description.contains("其他")
            && options_description.contains("Other"),
        "AskUserQuestion options schema must forbid model-supplied Other options: {options_description}"
    );
}

#[tokio::test]
async fn get_all_schemas_returns_builtin_then_mcp_partitions() {
    use app_lib::plugin::registry::ToolRegistry;
    let registry = ToolRegistry::new();
    let schemas = registry.get_all_schemas().await;
    let names: Vec<_> = schemas.iter().map(|s| s.name.clone()).collect();
    let builtin: Vec<_> = names
        .iter()
        .filter(|name| !name.starts_with("mcp__"))
        .cloned()
        .collect();
    let mcp: Vec<_> = names
        .iter()
        .filter(|name| name.starts_with("mcp__"))
        .cloned()
        .collect();
    let mut builtin_sorted = builtin.clone();
    builtin_sorted.sort();
    let mut mcp_sorted = mcp.clone();
    mcp_sorted.sort();
    assert_eq!(builtin, builtin_sorted, "built-in partition must be sorted");
    assert_eq!(mcp, mcp_sorted, "MCP partition must be sorted");
}

#[tokio::test]
async fn get_schemas_filtered_returns_sorted_by_name() {
    use app_lib::plugin::registry::ToolRegistry;
    use app_lib::plugin::skill_trait::ToolFilter;
    let registry = ToolRegistry::new();
    let ctx = app_lib::runtime::tools::ToolDescriptionContext::default();
    let schemas = registry
        .get_schemas_filtered(
            &ToolFilter::Only(vec!["WebSearch".to_string(), "WriteMemory".to_string()]),
            &ctx,
            &std::collections::HashMap::new(),
        )
        .await;
    let names: Vec<_> = schemas.iter().map(|s| s.name.clone()).collect();
    // Both are in REQUEST_SCOPED_RUNTIME_TOOL_NAMES so should appear when filtered
    let builtin: Vec<_> = names
        .iter()
        .filter(|name| !name.starts_with("mcp__"))
        .cloned()
        .collect();
    let mcp: Vec<_> = names
        .iter()
        .filter(|name| name.starts_with("mcp__"))
        .cloned()
        .collect();
    let mut builtin_sorted = builtin.clone();
    builtin_sorted.sort();
    let mut mcp_sorted = mcp.clone();
    mcp_sorted.sort();
    assert_eq!(
        builtin, builtin_sorted,
        "filtered built-in partition must be sorted"
    );
    assert_eq!(mcp, mcp_sorted, "filtered MCP partition must be sorted");
}

// Task 2.1 tests

#[test]
fn tool_definition_default_read_only_is_false() {
    let def = ToolDefinition::new("test_tool", "desc");
    assert!(!def.default_read_only);
    assert!(!def.default_destructive);
}

#[test]
fn tool_definition_with_read_only_flag() {
    let def = ToolDefinition::new("read_tool", "desc").with_read_only(true);
    assert!(def.default_read_only);
}

#[test]
fn tool_definition_with_destructive_flag() {
    let def = ToolDefinition::new("write_tool", "desc").with_destructive(true);
    assert!(def.default_destructive);
}

#[test]
fn runtime_tool_default_predicates_follow_definition_flags() {
    use app_lib::runtime::tools::description_context::ToolDescriptionContext;
    use app_lib::runtime::tools::{RuntimeTool, ToolError, ToolExecutionContext, ToolResult};
    use async_trait::async_trait;
    use serde_json::{json, Value};

    struct PredicateTool(ToolDefinition);

    #[async_trait]
    impl RuntimeTool for PredicateTool {
        fn id(&self) -> &str {
            &self.0.id
        }

        // The trait's default `is_read_only` / `is_destructive` call
        // `default_read_only()` / `default_destructive()`.  Tools whose
        // static flags live in `ToolDefinition` plumb them through here so
        // the predicates honor the definition without a per-call async
        // round-trip into `definition()`.
        fn default_read_only(&self) -> bool {
            self.0.default_read_only
        }

        fn default_destructive(&self) -> bool {
            self.0.default_destructive
        }

        async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
            self.0.clone()
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new(self.0.id.clone(), "ok", None))
        }
    }

    let default_tool = PredicateTool(ToolDefinition::new("default_tool", "desc"));
    assert!(!default_tool.is_concurrency_safe(&json!({})));
    assert!(!default_tool.is_read_only(&json!({})));
    assert!(!default_tool.is_destructive(&json!({})));

    let flagged_tool = PredicateTool(
        ToolDefinition::new("flagged_tool", "desc")
            .with_read_only(true)
            .with_destructive(true),
    );
    assert!(flagged_tool.is_read_only(&json!({})));
    assert!(flagged_tool.is_destructive(&json!({})));
}

// ── Plan-D1: ToolDefinition.default_max_result_size_chars ─────────────────

#[test]
fn tool_definition_default_max_result_size_chars_is_8000() {
    let def = ToolDefinition::new("some_tool", "desc");
    assert_eq!(def.default_max_result_size_chars, 8_000);
}

#[test]
fn tool_definition_with_max_result_size_chars_sets_field() {
    let def =
        ToolDefinition::new("some_tool_with_limit", "desc").with_max_result_size_chars(32_000);
    assert_eq!(def.default_max_result_size_chars, 32_000);
}

#[test]
fn catalog_read_workspace_file_has_16000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;
    let def = TOOL_CATALOG.get("Read").unwrap();
    assert_eq!(def.default_max_result_size_chars, 16_000);
}

#[test]
fn catalog_read_guides_binary_data_to_parsers_and_artifacts() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;
    let def = TOOL_CATALOG.get("Read").unwrap();
    assert!(def.description.contains("STL"));
    assert!(def.description.contains("解析脚本"));
    assert!(def.description.contains("目标产物"));
    assert!(def.description.contains("不要先 Read 二进制文件"));
    assert!(def.description.contains("预览不完整"));
    assert!(def.description.contains("写入该命名目标文件"));
    assert!(def.description.contains("不要继续只扩大阅读范围"));
}

#[test]
fn catalog_search_files_has_4000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;
    let def = TOOL_CATALOG.get("Glob").unwrap();
    assert_eq!(def.default_max_result_size_chars, 4_000);
}

#[test]
fn catalog_other_tools_default_to_8000_when_not_overridden() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    for id in ["WebSearch"] {
        let def = TOOL_CATALOG.get(id).unwrap();
        assert_eq!(
            def.default_max_result_size_chars, 8_000,
            "{} should default to 8000",
            id
        );
    }
}

#[test]
fn catalog_long_running_tools_have_declared_default_timeouts() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    for (id, expected) in [("Bash", Some(120))] {
        let def = TOOL_CATALOG.get(id).unwrap();
        assert_eq!(
            def.default_timeout_secs, expected,
            "{id} should declare the expected default timeout"
        );
    }
}

#[test]
fn shell_tool_descriptions_guard_auto_loaded_skill_directories() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    for id in ["Bash", "PowerShell"] {
        let def = TOOL_CATALOG.get(id).unwrap();
        assert!(
            def.description.contains("~/skills")
                && def.description.contains(".agents/skills")
                && def.description.contains("clone/install/write")
                && def.description.contains("隔离 review 目录"),
            "{id} description must warn against installing unreviewed code into auto-loaded skill directories: {}",
            def.description
        );
    }
}

#[test]
fn tool_descriptions_classify_recoverable_and_boundary_failures() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    for id in ["Bash", "PowerShell"] {
        let def = TOOL_CATALOG.get(id).unwrap();
        assert!(
            def.description.contains("错误类型")
                && def.description.contains("网络/5xx/429/超时")
                && def.description.contains("权限或安全拒绝")
                && def.description.contains("不要换写法绕过")
                && def.description.contains("optional/candidate/backup")
                && def.description.contains("按已安排/已分配处理")
                && def.description.contains("字段级校验")
                && def.description.contains("BEGIN:VEVENT")
                && def.description.contains("ATTENDEE")
                && def.description.contains("ATTENDEE;ROLE=OPT-PARTICIPANT")
                && def.description.contains("required+optional")
                && def.description.contains("完整邮箱")
                && def.description.contains("额外顶层章节")
                && def.description.contains("脚本退出成功")
                && def.description.contains("仍按交付失败处理")
                && def.description.contains("长 transcript")
                && def.description.contains("speaker dump")
                && def
                    .description
                    .contains("先把已确认事实和未确认缺口写入目标文件"),
            "{id} description must classify tool failures and boundary denials: {}",
            def.description
        );
    }

    let write_entry = TOOL_CATALOG.get_entry("Write").unwrap();
    let write = write_entry.json_schema["description"].as_str().unwrap();
    assert!(
        write.contains("文件已存在但未读取")
            && write.contains("不要改用 Bash/PowerShell 直接截断覆盖"),
        "Write description must recover from read-before-write denial without bypassing it: {}",
        write
    );
    assert!(
        write.contains("字段级断言")
            && write.contains("attendees")
            && write.contains("ATTENDEE")
            && write.contains("ATTENDEE;ROLE=OPT-PARTICIPANT")
            && write.contains("固定章节")
            && write.contains("required+optional")
            && write.contains("carol@company.com")
            && write.contains("立即 Edit/重写目标文件")
            && write.contains("写脚本、生成脚本或保存 helper 只是中间步骤")
            && write.contains("必须运行它生成用户命名的最终文件"),
        "Write description must require semantic validation of final hard-constraint outputs: {}",
        write
    );
    assert!(
        write.contains("agent、数字员工、persona")
            && write.contains("agent_config")
            && write.contains("不要用 TaskCreate 代替")
            && write.contains("未注册限制"),
        "Write description must guide product-entity fallback config artifacts: {}",
        write
    );

    let edit_entry = TOOL_CATALOG.get_entry("Edit").unwrap();
    let edit = edit_entry.json_schema["description"].as_str().unwrap();
    assert!(
        edit.contains("old_string 不存在")
            && edit.contains("old_string 不唯一")
            && edit.contains("不能只总结失败"),
        "Edit description must guide precise recovery after failed edits: {}",
        edit
    );
}

#[test]
fn task_tools_respect_strict_output_and_semantic_completion() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    let create = TOOL_CATALOG.get("TaskCreate").unwrap();
    assert!(
        create.description.contains("不要创建其它文件/目录")
            && create.description.contains("严格评分")
            && create.description.contains("内部清单")
            && create.description.contains("不是产品实体")
            && create
                .description
                .contains("不要用它来假装已经创建/配置了 agent")
            && create
                .description
                .contains("改用 Write/Edit 生成明确命名的配置产物"),
        "TaskCreate description must avoid persistent task artifacts when output set is strict: {}",
        create.description
    );

    let update = TOOL_CATALOG.get_entry("TaskUpdate").unwrap();
    let definition = &update.definition.description;
    let status = update.json_schema["properties"]["status"]["description"]
        .as_str()
        .unwrap();
    assert!(
        definition.contains("字段级断言")
            && definition.contains("文件存在")
            && status.contains("字段级断言")
            && status.contains("schema"),
        "TaskUpdate description must not allow shallow verification to mark hard-constraint tasks completed: def={definition}; status={status}",
    );
}

#[test]
fn memory_tool_description_requires_persisting_explicit_preferences() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    let write = TOOL_CATALOG.get("WriteMemory").unwrap();
    assert!(
        write.description.contains("不能只回复")
            && write.description.contains("保存成功后再确认")
            && write.description.contains("不要保存")
            && write.description.contains("临时任务进度"),
        "WriteMemory description must steer explicit preferences into durable memory: {}",
        write.description
    );
    let write_entry = TOOL_CATALOG.get_entry("WriteMemory").unwrap();
    let content_description = write_entry.json_schema["properties"]["content"]["description"]
        .as_str()
        .unwrap();
    assert!(
        content_description.contains("明文密码")
            && content_description.contains("令牌")
            && content_description.contains("大段原始数据"),
        "WriteMemory content schema must forbid sensitive or raw bulk memory: {content_description}"
    );

    let search = TOOL_CATALOG.get("SearchMemory").unwrap();
    assert!(
        search.description.contains("历史偏好")
            && search.description.contains("当前事实证明")
            && search.description.contains("没有命中时不要编造记忆"),
        "SearchMemory description must distinguish recall from current evidence: {}",
        search.description
    );
}

#[test]
fn agenda_tool_description_requires_absolute_time_and_verification() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    let entry = TOOL_CATALOG.get_entry("create_agenda_item").unwrap();
    let description = &entry.definition.description;
    assert!(
        description.contains("RFC3339")
            && description.contains("不要把“明早”")
            && description.contains("下一次未来触发时间")
            && description.contains("schema 没有 channel")
            && description.contains("list_agenda_items"),
        "create_agenda_item description must guide date parsing and verification: {description}"
    );

    let start_at = entry.json_schema["properties"]["start_at"]["description"]
        .as_str()
        .unwrap();
    assert!(
        start_at.contains("RFC3339") && start_at.contains("必须是未来触发点"),
        "start_at schema must require an absolute future timestamp: {start_at}"
    );
}

#[tokio::test]
async fn agenda_runtime_definition_matches_absolute_time_guidance() {
    use app_lib::runtime::tools::builtin::agenda::{AgendaToolDeps, CreateAgendaItemRuntimeTool};
    use app_lib::runtime::tools::{RuntimeTool, ToolDescriptionContext};
    use std::sync::Arc;

    let dir = tempfile::tempdir().unwrap();
    let tool = CreateAgendaItemRuntimeTool {
        deps: Arc::new(AgendaToolDeps::new(
            dir.path().to_path_buf(),
            "alice".to_string(),
        )),
    };
    let definition = tool.definition(&ToolDescriptionContext::empty()).await;
    assert!(definition.description.contains("RFC3339"));
    assert!(definition.description.contains("下一次未来触发时间"));
    assert!(definition.description.contains("schema 没有 channel"));
    assert!(definition.description.contains("list_agenda_items"));
}

#[test]
fn skill_market_tools_do_not_turn_no_match_into_task_completion() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    let search = TOOL_CATALOG.get("SkillMarketSearch").unwrap();
    assert!(
        search.description.contains("不代表用户任务结束")
            && search.description.contains("继续完成其它可执行交付物")
            && search
                .description
                .contains("不要用本工具替代本地 SKILL.md 发现"),
        "SkillMarketSearch description must keep no-match from stalling the task: {}",
        search.description
    );

    let install = TOOL_CATALOG.get("SkillMarketInstall").unwrap();
    assert!(
        install.description.contains("安装成功后调用 RefreshSkills")
            && install.description.contains("随后调用 Skill")
            && install.description.contains("不是最终交付"),
        "SkillMarketInstall description must describe post-install execution and failure recovery: {}",
        install.description
    );
}

#[test]
fn catalog_non_long_running_tools_keep_timeout_unset() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    for id in ["Read", "WebSearch"] {
        let def = TOOL_CATALOG.get(id).unwrap();
        assert_eq!(
            def.default_timeout_secs, None,
            "{id} should keep timeout declaration unset"
        );
    }
}
