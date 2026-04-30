use app_lib::runtime::project_memory::{
    ProjectMemoryEntryDraft, ProjectMemoryService, ProjectMemoryType,
};
use tempfile::TempDir;

fn make_service(dir: &std::path::Path) -> ProjectMemoryService {
    let workspace = dir.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    ProjectMemoryService::new(dir, &workspace)
}

// ── 意图 1：保存记忆时写入独立 entry 文件并重建索引 ──────────────────────────

#[test]
fn memory_save_writes_entry_file_with_correct_frontmatter_and_content() {
    let dir = TempDir::new().unwrap();
    let service = make_service(dir.path());

    let saved = service
        .save_memory(ProjectMemoryEntryDraft {
            memory_type: ProjectMemoryType::UserPreference,
            name: "薪资分析偏好箱线图".to_string(),
            description: "用户偏好用箱线图展示薪资分布".to_string(),
            content: "遇到薪资分析时优先建议箱线图。".to_string(),
            source: Some("intent-test".to_string()),
        })
        .unwrap();

    assert!(saved.path.exists(), "entry file must be written to disk");
    assert!(
        saved.path.starts_with(service.memory_root()),
        "entry file must be inside this workspace's memory bucket"
    );

    let content = std::fs::read_to_string(&saved.path).unwrap();
    assert!(content.contains("type: user_preference"));
    assert!(content.contains("name: 薪资分析偏好箱线图"));
    assert!(content.contains("description: 用户偏好用箱线图展示薪资分布"));
    assert!(content.contains("source: intent-test"));
    assert!(content.contains("遇到薪资分析时优先建议箱线图。"));
}

#[test]
fn memory_save_rebuilds_memory_index_with_entry_link_and_description() {
    let dir = TempDir::new().unwrap();
    let service = make_service(dir.path());

    service
        .save_memory(ProjectMemoryEntryDraft {
            memory_type: ProjectMemoryType::UserPreference,
            name: "薪资分析偏好箱线图".to_string(),
            description: "用户偏好用箱线图展示薪资分布".to_string(),
            content: "遇到薪资分析时优先建议箱线图。".to_string(),
            source: None,
        })
        .unwrap();

    let index = std::fs::read_to_string(service.entrypoint_path()).unwrap();
    assert!(
        index.contains("薪资分析偏好箱线图"),
        "MEMORY.md must contain entry name"
    );
    assert!(
        index.contains("用户偏好用箱线图展示薪资分布"),
        "MEMORY.md must contain entry description"
    );
}

// ── 意图 2：不同 workspace 的记忆互相隔离 ──────────────────────────────────

#[test]
fn memory_different_workspaces_use_different_buckets_and_do_not_share_entries() {
    let dir = TempDir::new().unwrap();
    let ws_a = dir.path().join("ws_a");
    let ws_b = dir.path().join("ws_b");
    std::fs::create_dir_all(&ws_a).unwrap();
    std::fs::create_dir_all(&ws_b).unwrap();

    let service_a = ProjectMemoryService::new(dir.path(), &ws_a);
    let service_b = ProjectMemoryService::new(dir.path(), &ws_b);

    service_a
        .save_memory(ProjectMemoryEntryDraft {
            memory_type: ProjectMemoryType::ProjectConstraint,
            name: "项目A专属记忆".to_string(),
            description: "只属于项目A".to_string(),
            content: "项目A的专属内容。".to_string(),
            source: None,
        })
        .unwrap();

    assert_ne!(
        service_a.memory_root(),
        service_b.memory_root(),
        "different workspaces must have different memory buckets"
    );

    let ctx_b = service_b.load_context("项目A专属记忆").unwrap();
    assert_eq!(
        ctx_b.recalled_entries.len(),
        0,
        "workspace B must not recall workspace A's memories"
    );

    let index_b_path = service_b.entrypoint_path();
    if index_b_path.exists() {
        let index_b = std::fs::read_to_string(&index_b_path).unwrap();
        assert!(
            !index_b.contains("项目A专属记忆"),
            "workspace B's MEMORY.md must not contain workspace A's entries"
        );
    }
}

// ── 意图 3：加载上下文时只返回与 query 相关的 entries ─────────────────────

#[test]
fn memory_load_context_returns_only_relevant_entries_for_query() {
    let dir = TempDir::new().unwrap();
    let service = make_service(dir.path());

    service
        .save_memory(ProjectMemoryEntryDraft {
            memory_type: ProjectMemoryType::UserPreference,
            name: "薪资分析偏好箱线图".to_string(),
            description: "用户偏好用箱线图展示薪资分布".to_string(),
            content: "遇到薪资分析时优先建议箱线图。".to_string(),
            source: None,
        })
        .unwrap();
    service
        .save_memory(ProjectMemoryEntryDraft {
            memory_type: ProjectMemoryType::ProjectConstraint,
            name: "移动端发版冻结".to_string(),
            description: "2026-04-25 起非关键改动暂停合并".to_string(),
            content: "发版冻结期间非关键 PR 不建议合并。".to_string(),
            source: None,
        })
        .unwrap();

    let ctx = service.load_context("薪资分析").unwrap();

    assert_eq!(ctx.recalled_entries.len(), 1);
    assert_eq!(ctx.recalled_entries[0].name, "薪资分析偏好箱线图");

    let rendered = ctx.render_for_prompt();
    assert!(rendered.contains("[相关记忆]"));
    assert!(rendered.contains("箱线图"));
    assert!(!rendered.contains("移动端发版冻结"));
}

#[test]
fn memory_load_context_falls_back_to_index_when_no_entries_match() {
    let dir = TempDir::new().unwrap();
    let service = make_service(dir.path());

    service
        .save_memory(ProjectMemoryEntryDraft {
            memory_type: ProjectMemoryType::UserPreference,
            name: "薪资分析偏好箱线图".to_string(),
            description: "用户偏好用箱线图展示薪资分布".to_string(),
            content: "遇到薪资分析时优先建议箱线图。".to_string(),
            source: None,
        })
        .unwrap();

    let ctx = service.load_context("完全无关的查询词").unwrap();

    assert_eq!(ctx.recalled_entries.len(), 0);
    let rendered = ctx.render_for_prompt();
    assert!(
        !rendered.contains("[相关记忆]"),
        "must not show recall block when no match"
    );
    assert!(
        rendered.contains("MEMORY.md"),
        "must fall back to index text"
    );
}

// ── 意图 4：legacy core memory 被懒迁移且迁移幂等 ─────────────────────────

#[test]
fn memory_legacy_core_memory_is_lazily_migrated_on_first_load() {
    let dir = TempDir::new().unwrap();
    let legacy_dir = dir.path().join("shared").join("cognitive");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::write(
        legacy_dir.join("mem.md"),
        "旧版核心记忆：优先用 cargo test 验证 Rust 改动。",
    )
    .unwrap();

    let service = make_service(dir.path());
    let ctx = service.load_context("cargo test").unwrap();

    let legacy_entry = service
        .memory_root()
        .join("entries")
        .join("legacy-core-memory.md");
    assert!(
        legacy_entry.exists(),
        "legacy entry file must be created on first load"
    );

    let content = std::fs::read_to_string(&legacy_entry).unwrap();
    assert!(content.contains("type: project_constraint"));
    assert!(content.contains("source: legacy-core-memory"));
    assert!(content.contains("旧版核心记忆"));

    let index = std::fs::read_to_string(service.entrypoint_path()).unwrap();
    assert!(index.contains("legacy-core-memory"));

    assert!(
        ctx.render_for_prompt().contains("旧版核心记忆"),
        "migrated legacy memory must be available in prompt"
    );
}

#[test]
fn memory_legacy_migration_is_idempotent_on_repeated_load() {
    let dir = TempDir::new().unwrap();
    let legacy_dir = dir.path().join("shared").join("cognitive");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::write(
        legacy_dir.join("mem.md"),
        "旧版核心记忆：优先用 cargo test 验证 Rust 改动。",
    )
    .unwrap();

    let service = make_service(dir.path());
    service.load_context("cargo test").unwrap();
    service.load_context("cargo test").unwrap();

    let entries_dir = service.memory_root().join("entries");
    let legacy_entries: Vec<_> = std::fs::read_dir(&entries_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("legacy-core-memory")
        })
        .collect();

    assert_eq!(
        legacy_entries.len(),
        1,
        "legacy migration must only run once"
    );
}

// ── 意图 5：distill_index 从现有 entry 文件重建 MEMORY.md ─────────────────

#[test]
fn memory_distill_rebuilds_index_from_valid_entries_and_skips_corrupt_ones() {
    let dir = TempDir::new().unwrap();
    let service = make_service(dir.path());
    let entries_dir = service.memory_root().join("entries");
    std::fs::create_dir_all(&entries_dir).unwrap();

    // 合法 entry 1
    std::fs::write(
        entries_dir.join("valid-a.md"),
        "---\ntype: user_preference\nname: 合法记忆A\ndescription: 描述A\n---\n\n内容A\n",
    )
    .unwrap();
    // 合法 entry 2
    std::fs::write(
        entries_dir.join("valid-b.md"),
        "---\ntype: feedback\nname: 合法记忆B\ndescription: 描述B\n---\n\n内容B\n",
    )
    .unwrap();
    // 损坏 entry（无 frontmatter）
    std::fs::write(
        entries_dir.join("corrupt.md"),
        "这是没有 frontmatter 的内容\n",
    )
    .unwrap();
    // 清空 MEMORY.md
    std::fs::write(service.entrypoint_path(), "").unwrap();

    let count = service.distill_index().unwrap();

    assert_eq!(count, 2, "only valid entries should be counted");

    let index = std::fs::read_to_string(service.entrypoint_path()).unwrap();
    assert!(index.contains("合法记忆A"));
    assert!(index.contains("合法记忆B"));
    assert!(!index.contains("这是没有 frontmatter"));
}

// ── 意图 6：同一条记忆重复保存时更新而不是复制 ─────────────────────────────

#[test]
fn memory_saving_same_name_and_description_twice_overwrites_not_duplicates() {
    let dir = TempDir::new().unwrap();
    let service = make_service(dir.path());

    service
        .save_memory(ProjectMemoryEntryDraft {
            memory_type: ProjectMemoryType::UserPreference,
            name: "回复风格偏好".to_string(),
            description: "用户希望回复简洁".to_string(),
            content: "v1 内容：回复要简洁直接。".to_string(),
            source: None,
        })
        .unwrap();
    service
        .save_memory(ProjectMemoryEntryDraft {
            memory_type: ProjectMemoryType::UserPreference,
            name: "回复风格偏好".to_string(),
            description: "用户希望回复简洁".to_string(),
            content: "v2 内容：回复要简洁，不要总结改动。".to_string(),
            source: None,
        })
        .unwrap();

    let entries_dir = service.memory_root().join("entries");
    let md_files: Vec<_> = std::fs::read_dir(&entries_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
        .collect();

    assert_eq!(
        md_files.len(),
        1,
        "same name+description must overwrite, not duplicate"
    );

    let content = std::fs::read_to_string(md_files[0].path()).unwrap();
    assert!(
        content.contains("v2 内容"),
        "file must contain latest content"
    );
    assert!(
        !content.contains("v1 内容"),
        "file must not contain stale content"
    );

    let index = std::fs::read_to_string(service.entrypoint_path()).unwrap();
    let count = index.matches("回复风格偏好").count();
    assert_eq!(count, 1, "MEMORY.md must only list entry once");
}

// ── 意图 7：四类 memory_type 都能正确持久化并被 recall ──────────────────────

#[test]
fn memory_all_four_memory_types_persist_and_recall_correctly() {
    let dir = TempDir::new().unwrap();
    let service = make_service(dir.path());

    let cases = vec![
        (
            ProjectMemoryType::UserPreference,
            "用户偏好记录",
            "user_preference",
            "偏好关键词unique1",
        ),
        (
            ProjectMemoryType::ProjectConstraint,
            "项目约束记录",
            "project_constraint",
            "约束关键词unique2",
        ),
        (
            ProjectMemoryType::ReferenceInfo,
            "参考信息记录",
            "reference_info",
            "参考关键词unique3",
        ),
        (
            ProjectMemoryType::Feedback,
            "反馈记录",
            "feedback",
            "反馈关键词unique4",
        ),
    ];

    for (memory_type, name, _type_str, keyword) in &cases {
        service
            .save_memory(ProjectMemoryEntryDraft {
                memory_type: memory_type.clone(),
                name: name.to_string(),
                description: format!("{}的描述", name),
                content: format!("内容中包含{}。", keyword),
                source: None,
            })
            .unwrap();
    }

    for (memory_type, name, type_str, keyword) in &cases {
        let ctx = service.load_context(keyword).unwrap();
        assert_eq!(
            ctx.recalled_entries.len(),
            1,
            "should recall exactly one entry for '{}'",
            keyword
        );
        assert_eq!(&ctx.recalled_entries[0].name, name);
        assert_eq!(ctx.recalled_entries[0].memory_type, *memory_type);

        let entry_content = std::fs::read_to_string(&ctx.recalled_entries[0].path).unwrap();
        assert!(entry_content.contains(&format!("type: {}", type_str)));
    }
}

// ── 意图 8：query 为空或过短时不做 recall，只回退 index ──────────────────────

#[test]
fn memory_empty_or_too_short_query_does_not_recall_and_falls_back_to_index() {
    let dir = TempDir::new().unwrap();
    let service = make_service(dir.path());

    service
        .save_memory(ProjectMemoryEntryDraft {
            memory_type: ProjectMemoryType::UserPreference,
            name: "某条记忆".to_string(),
            description: "随便一条记忆".to_string(),
            content: "内容无所谓。".to_string(),
            source: None,
        })
        .unwrap();

    for query in ["", "a", "我"] {
        let ctx = service.load_context(query).unwrap();
        assert_eq!(
            ctx.recalled_entries.len(),
            0,
            "query '{}' is too short, must not recall",
            query
        );
        let rendered = ctx.render_for_prompt();
        assert!(
            !rendered.contains("[相关记忆]"),
            "query '{}' must not produce recall block",
            query
        );
        assert!(
            rendered.contains("MEMORY.md"),
            "query '{}' must fall back to index",
            query
        );
    }
}

// ── 意图 9：相关性召回最多返回 5 条，且优先返回命中分更高的记忆 ───────────────

#[test]
fn memory_recall_is_capped_at_five_entries_and_prioritizes_higher_scoring_ones() {
    let dir = TempDir::new().unwrap();
    let service = make_service(dir.path());

    // 保存 5 条普通命中（只命中一次 "薪资"）
    for i in 1..=5 {
        service
            .save_memory(ProjectMemoryEntryDraft {
                memory_type: ProjectMemoryType::UserPreference,
                name: format!("普通记忆{}", i),
                description: format!("薪资相关描述{}", i),
                content: format!("关于薪资的内容{}。", i),
                source: None,
            })
            .unwrap();
    }
    // 保存 1 条高分命中（name + description + content 都含 "薪资"，但 name 唯一标识）
    service
        .save_memory(ProjectMemoryEntryDraft {
            memory_type: ProjectMemoryType::UserPreference,
            name: "高分薪资记忆".to_string(),
            description: "薪资分析薪资偏好".to_string(),
            content: "薪资薪资薪资反复出现。".to_string(),
            source: None,
        })
        .unwrap();

    let ctx = service.load_context("薪资 分析 偏好").unwrap();

    assert_eq!(ctx.recalled_entries.len(), 5, "recall must be capped at 5");
    assert!(
        ctx.recalled_entries
            .iter()
            .any(|e| e.name == "高分薪资记忆"),
        "highest-scoring entry must be in results"
    );
}

// ── 意图 10：损坏的 entry 文件不会污染 recall 和 index ──────────────────────

#[test]
fn memory_corrupt_entries_are_silently_skipped_in_recall_and_index() {
    let dir = TempDir::new().unwrap();
    let service = make_service(dir.path());
    let entries_dir = service.memory_root().join("entries");
    std::fs::create_dir_all(&entries_dir).unwrap();

    // 合法 entry
    std::fs::write(
        entries_dir.join("valid.md"),
        "---\ntype: user_preference\nname: 合法记忆\ndescription: 合法描述\n---\n\n合法关键词内容。\n",
    )
    .unwrap();
    // 无 frontmatter
    std::fs::write(
        entries_dir.join("no-frontmatter.md"),
        "没有 frontmatter 的原始内容\n",
    )
    .unwrap();
    // 缺少 type
    std::fs::write(
        entries_dir.join("missing-type.md"),
        "---\nname: 缺type记忆\ndescription: 无type描述\n---\n\n内容。\n",
    )
    .unwrap();
    // type 非法
    std::fs::write(
        entries_dir.join("invalid-type.md"),
        "---\ntype: unknown_type\nname: 非法type记忆\ndescription: 非法描述\n---\n\n内容。\n",
    )
    .unwrap();

    let ctx = service.load_context("合法关键词").unwrap();
    assert_eq!(ctx.recalled_entries.len(), 1);
    assert_eq!(ctx.recalled_entries[0].name, "合法记忆");

    let count = service.distill_index().unwrap();
    assert_eq!(count, 1, "only valid entry should be counted in distill");

    let index = std::fs::read_to_string(service.entrypoint_path()).unwrap();
    assert!(index.contains("合法记忆"));
    assert!(!index.contains("缺type记忆"));
    assert!(!index.contains("非法type记忆"));
    assert!(!index.contains("没有 frontmatter"));
}
