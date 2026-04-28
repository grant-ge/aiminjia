//! Phase F Task 22 review test: forbid legacy stateful skill markers in production source.

use std::fs;
use std::path::Path;

fn check_no_forbidden_markers(repo_root: &Path, root: &str, forbidden: &[&str]) {
    let scan_dir = repo_root.join(root);
    let mut hits: Vec<String> = Vec::new();
    walk(&scan_dir, &mut |path: &Path| {
        // 跳过非源代码扩展名
        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(e) => e,
            None => return,
        };
        if !matches!(ext, "rs" | "ts" | "tsx" | "js" | "jsx") {
            return;
        }
        // 跳过本测试自身
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == "review_skill_system_no_legacy_test.rs")
            .unwrap_or(false)
        {
            return;
        }
        let content = fs::read_to_string(path).unwrap_or_default();
        for needle in forbidden {
            if content.contains(needle) {
                hits.push(format!(
                    "legacy marker `{}` found in {}",
                    needle,
                    path.display()
                ));
            }
        }
    });
    assert!(
        hits.is_empty(),
        "Legacy skill markers must not appear in production source:\n{}",
        hits.join("\n")
    );
}

fn walk(dir: &Path, action: &mut dyn FnMut(&Path)) {
    if dir.is_file() {
        action(dir);
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // 跳过 target / node_modules / .git / 文档
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(
                name,
                "target" | "node_modules" | ".git" | "dist" | "build" | "docs"
            ) {
                continue;
            }
            walk(&path, action);
        } else if path.is_file() {
            action(&path);
        }
    }
}

#[test]
fn production_source_has_no_legacy_skill_markers() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .unwrap();

    let forbidden = &[
        "SkillSessionStore",
        "SkillRuntimePatch",
        "selected_skill_id",
        "selectedSkillId",
        "selected_skill_label",
        "selectedSkillLabel",
        "extract_skill_runtime_patch",
        "apply_skill_runtime_patch",
        "skill_runtime_patch",
        "is_analysis",
        "precompute_result",
    ];

    // 后端只扫 src/，跳过 tests/（tests 中可能含历史断言字符串）
    check_no_forbidden_markers(&repo_root, "src-tauri/src", forbidden);

    // 前端只扫 src/
    check_no_forbidden_markers(&repo_root, "src", forbidden);
}

#[test]
fn production_source_has_no_legacy_filename_references() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .unwrap();

    // 注意：plugin.toml / workflow.toml / prompts/step 在 skill_smith 测试 fixture 中仍有，
    // 但生产代码（src 目录）应零引用。我们扫 src-tauri/src + src，跳过 tests/。
    let forbidden = &[
        "plugin.toml",
        "workflow.toml",
        "DeclarativeSkill",
        "PluginManifest",
        "WorkflowManifest",
        "switch_skill",
    ];

    check_no_forbidden_markers(&repo_root, "src-tauri/src", forbidden);
    check_no_forbidden_markers(&repo_root, "src", forbidden);
}
