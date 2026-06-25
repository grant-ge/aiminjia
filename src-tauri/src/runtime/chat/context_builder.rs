/// Build the per-iteration user/dynamic context carrying runtime deltas.
///
/// This is kept separate from the static system prompt so that KV-cache prefix stability is
/// preserved: only the dynamic user message changes, not the system message.
///
/// The concatenation order and separators intentionally mirror Block 13 of `chat_runtime_impl.rs`
/// (lines ~2230-2291) so that the LLM sees an identical context layout regardless of which code
/// path produces the string.
pub fn build_iteration_context(
    core_memory: &str,
    project_memory: &str,
    workspace_context: &str,
    file_context: &str,
    analysis_notes: &str,
    connector_context: Option<&str>,
    analysis_ctx_prompt: Option<&str>,
    skill_catalog: &str,
) -> String {
    let mut ctx = String::from("[动态上下文 — 请勿回复此消息]\n");

    // 1. Cognitive core memory — always loaded (cross-session knowledge base)
    if !core_memory.is_empty() {
        ctx.push_str("\n[核心记忆]\n");
        ctx.push_str(core_memory);
        ctx.push_str("\n");
    }

    // 2. Project memory (workspace-scoped persistent recall)
    if !project_memory.is_empty() {
        ctx.push_str("\n[项目记忆]\n");
        ctx.push_str(project_memory);
        ctx.push_str("\n");
    }

    // 3. Workspace context (file listing, project summary, etc.)
    if !workspace_context.is_empty() {
        ctx.push_str(workspace_context);
    }

    // 4. File context (contents of uploaded / referenced files)
    if !file_context.is_empty() {
        ctx.push_str(file_context);
    }

    // 5. Analysis notes (accumulated observations from previous iterations)
    if !analysis_notes.is_empty() {
        ctx.push_str(analysis_notes);
    }

    // 6. Internal connector context (browsing sessions / legacy app integrations)
    if let Some(connector) = connector_context {
        ctx.push_str("\n\n[内部系统浏览]\n");
        ctx.push_str(connector);
        ctx.push_str("\n[/内部系统浏览]");
    }

    // 7. Optional step context prompt (AnalysisContext::format_for_prompt())
    //    and step plan are injected by the caller and forwarded here as a single
    //    pre-formatted string.
    //
    //    NOTE: The original Block 13 also reads `_plan.md` from disk inline when a
    //    step/workflow context is active (workspace_path/analysis/{conversation_id}/_plan.md).
    //    That file-I/O is kept in the call-site for now so this function stays pure.
    //    TODO(S4-T5): lift plan-file reading into this function once the workspace-path
    //    is threaded through the ChatTurnRequest / TurnConfig.
    if let Some(ctx_prompt) = analysis_ctx_prompt {
        if !ctx_prompt.is_empty() {
            ctx.push_str(ctx_prompt);
        }
    }

    // 8. Skill catalog — dynamic LLM-driven skill discovery.
    if !skill_catalog.is_empty() {
        ctx.push_str("\n\n<system-reminder>\n");
        ctx.push_str(skill_catalog);
        ctx.push_str("\n</system-reminder>");
    }

    ctx
}

pub fn build_local_skill_context(
    workspace_path: &std::path::Path,
    authorized_workspace: Option<&crate::runtime::store::AuthorizedWorkspaceRef>,
) -> String {
    let root = authorized_workspace
        .map(|ws| ws.root_path.as_path())
        .unwrap_or(workspace_path);
    let mut paths = Vec::new();
    push_skill_entry_if_file(root, root.join("SKILL.md"), &mut paths);
    push_one_level_skill_entries(root, root.join(".agents").join("skills"), &mut paths);
    push_one_level_skill_entries(root, root.join("skills"), &mut paths);
    if paths.is_empty() {
        return String::new();
    }
    paths.sort();
    paths.dedup();
    paths.truncate(12);

    let mut out = String::from(
        "\n\n[本地技能入口]\n当前工作区存在本地 SKILL.md。非平凡文件/数据/代码任务在手写实现前，先用 Read 读取相关入口；读取后先提取环境/依赖、输入输出、helper/脚本、公式阈值、禁止事项和验证口径，再按其方法执行。这些不是 Skill(find-skills) 的 skill_id。这里只表示当前工作区已有的只读方法入口，不授权创建、安装、克隆或写入任何会被自动加载的 skills 目录。\n",
    );
    for path in paths {
        out.push_str("- ");
        out.push_str(&path);
        out.push('\n');
    }
    out
}

fn push_one_level_skill_entries(
    root: &std::path::Path,
    dir: std::path::PathBuf,
    paths: &mut Vec<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        push_skill_entry_if_file(root, entry.path().join("SKILL.md"), paths);
        if paths.len() >= 12 {
            break;
        }
    }
}

fn push_skill_entry_if_file(
    root: &std::path::Path,
    path: std::path::PathBuf,
    paths: &mut Vec<String>,
) {
    if !path.is_file() {
        return;
    }
    let display = path
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));
    paths.push(display);
}

/// 构建会话级环境信息段落，注入到 dynamic context。
///
/// 输出内容：
/// - 当前工作目录 / 已授权目录
/// - 操作系统平台
/// - Runtime 工具绝对路径（可选）
///
/// Contract:
/// - `authorized = Some((root_path_str, display_name))` 表示用户已连接本地目录，且它是
///   LLM 应看到的“当前工作目录”的权威来源。
/// - 在该场景下，输出只展示 `authorized` 对应的目录信息；`workspace_path` 仅作为
///   fallback 输入保留，不出现在输出里。
/// - 当 `authorized = None` 时，退回使用 `workspace_path` 作为工作目录展示。
pub async fn build_env_info(
    workspace_path: &std::path::PathBuf,
    authorized: Option<(&str, &str)>,
    runtime_info: Option<&ManagedRuntimeEnvInfo>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    // 1. 工作目录 / 已授权目录
    match authorized {
        Some((root_path, display_name)) => {
            parts.push(format!("已连接目录: {} ({})", display_name, root_path));
        }
        None => {
            parts.push(format!("工作目录: {}", workspace_path.display()));
        }
    }

    // 2. 平台信息
    let platform = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    };
    parts.push(format!("Platform: {}", platform));

    if let Some(runtime_info) = runtime_info {
        parts.push(runtime_info.format_for_env_info());
    }

    format!("\n\n[当前环境]\n{}", parts.join("\n"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedRuntimeEnvInfo {
    pub runtime_root: std::path::PathBuf,
    pub python_path: std::path::PathBuf,
    pub node_path: std::path::PathBuf,
    pub npm_path: std::path::PathBuf,
    pub npx_path: std::path::PathBuf,
    pub uv_path: std::path::PathBuf,
    pub uvx_path: std::path::PathBuf,
}

impl ManagedRuntimeEnvInfo {
    pub fn format_for_env_info(&self) -> String {
        let node_prefix = self
            .npm_path
            .parent()
            .and_then(|bin| bin.parent())
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| self.runtime_root.join("node"));
        let node_bin_dir = self
            .npm_path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| node_prefix.join("bin"));
        let node_cli_dir = if cfg!(target_os = "windows") {
            // npm global shims are written to the prefix root on Windows when
            // installing with `npm install -g --prefix <prefix>`.
            node_prefix.clone()
        } else {
            node_bin_dir.clone()
        };
        let node_global_modules = if cfg!(target_os = "windows") {
            node_prefix.join("node_modules")
        } else {
            node_prefix.join("lib").join("node_modules")
        };

        // Shell 语法因平台而异：Windows 走 powershell，需要 `& "exe"` call operator；
        // macOS / Linux 走 /bin/sh，必须裸命令（路径用引号包住即可）。
        // 给 LLM 的模板写错平台 → 工具执行直接 syntax error。
        let (
            python_install_template,
            node_install_template,
            node_require_template,
            node_cli_template,
        ) = if cfg!(target_os = "windows") {
            (
                format!(
                    r#"& "{uv}" pip install <包名> --python "{python}" --quiet"#,
                    uv = self.uv_path.display(),
                    python = self.python_path.display(),
                ),
                format!(
                    r#"& "{npm}" install -g <包名> --prefix "{node_prefix}" --silent"#,
                    npm = self.npm_path.display(),
                    node_prefix = node_prefix.display(),
                ),
                format!(
                    r#"$env:NODE_PATH="{node_global_modules}"; & "{node}" -e "require('<包名>'); console.log('ok')""#,
                    node_global_modules = node_global_modules.display(),
                    node = self.node_path.display(),
                ),
                format!(
                    r#"& "{node_cli_dir}\命令名.cmd" <参数>"#,
                    node_cli_dir = node_cli_dir.display(),
                ),
            )
        } else {
            (
                format!(
                    r#""{uv}" pip install <包名> --python "{python}" --quiet"#,
                    uv = self.uv_path.display(),
                    python = self.python_path.display(),
                ),
                format!(
                    r#""{npm}" install -g <包名> --prefix "{node_prefix}" --silent"#,
                    npm = self.npm_path.display(),
                    node_prefix = node_prefix.display(),
                ),
                format!(
                    r#"NODE_PATH="{node_global_modules}" "{node}" -e "require('<包名>'); console.log('ok')""#,
                    node_global_modules = node_global_modules.display(),
                    node = self.node_path.display(),
                ),
                format!(
                    r#""{node_cli_dir}/命令名" <参数>"#,
                    node_cli_dir = node_cli_dir.display(),
                ),
            )
        };

        format!(
            r#"Runtime: 已安装
Runtime 当前目录: {runtime_root}
Python: {python}
Node: {node}
npm: {npm}
npx: {npx}
uv: {uv}
uvx: {uvx}
Node 全局包目录: {node_global_modules}
Node 命令目录: {node_cli_dir}

规则:
1. 运行 Python / Node / npm / npx / uv 命令时，默认使用上面列出的绝对路径；只有用户明确要求系统环境时，才使用系统 PATH 中的命令。
2. 安装第三方 Python 包必须使用以下模板（替换包名即可），禁止任何变体：

   {python_install_template}

3. 禁止使用 --system / 裸 pip / python -m pip / pip install 后省略 --python。
4. uv 装包是幂等的：已安装的包会秒过（< 1s），不会重复下载，所以可以放心在每次需要时直接调用上述模板。
5. 安装第三方 Node 包必须使用以下模板（替换包名即可），禁止安装到当前工作目录：

   {node_install_template}

6. 检查或 require Runtime Node 全局包时，必须带 NODE_PATH：

   {node_require_template}

7. 已安装的 Node CLI 要从 Node 命令目录用绝对路径执行，例如：

   {node_cli_template}

   不要用 npx 运行已安装的包；npx 可能触发临时下载或重复安装。"#,
            runtime_root = self.runtime_root.display(),
            python = self.python_path.display(),
            node = self.node_path.display(),
            npm = self.npm_path.display(),
            npx = self.npx_path.display(),
            uv = self.uv_path.display(),
            uvx = self.uvx_path.display(),
            node_global_modules = node_global_modules.display(),
            node_cli_dir = node_cli_dir.display(),
            python_install_template = python_install_template,
            node_install_template = node_install_template,
            node_require_template = node_require_template,
            node_cli_template = node_cli_template,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_HEADER: &str = "[动态上下文 — 请勿回复此消息]\n";

    #[test]
    fn test_empty_inputs_yields_only_header() {
        let result = build_iteration_context("", "", "", "", "", None, None, "");
        assert_eq!(result, EMPTY_HEADER);
    }

    #[test]
    fn test_core_memory_block() {
        let result = build_iteration_context("mem content", "", "", "", "", None, None, "");
        assert!(result.contains("\n[核心记忆]\n"));
        assert!(result.contains("mem content"));
        assert!(result.contains("\n[核心记忆]\nmem content\n"));
    }

    #[test]
    fn test_project_memory_block() {
        let result = build_iteration_context("", "memory index", "", "", "", None, None, "");
        assert!(result.contains("\n[项目记忆]\n"));
        assert!(result.contains("memory index"));
        assert!(result.contains("\n[项目记忆]\nmemory index\n"));
    }

    // Note: skill precompute block test removed in Phase B Task 7 (precompute pipeline deleted).

    #[test]
    fn test_connector_context_block() {
        let result = build_iteration_context("", "", "", "", "", Some("connector info"), None, "");
        assert!(result.contains("[内部系统浏览]\n"));
        assert!(result.contains("connector info"));
        assert!(result.contains("[/内部系统浏览]"));
    }

    #[test]
    fn test_skill_catalog_block() {
        let catalog = "## 可用专项技能\n- `biz-writing` — 商务写作";
        let result = build_iteration_context("", "", "", "", "", None, None, catalog);
        assert!(result.contains("<system-reminder>\n## 可用专项技能"));
        assert!(result.contains("biz-writing"));
        assert!(result.contains("\n</system-reminder>"));
    }

    #[test]
    fn test_empty_skill_catalog_not_injected() {
        let result = build_iteration_context("", "", "", "", "", None, None, "");
        assert!(!result.contains("可用专项技能"));
    }

    #[test]
    fn test_concatenation_order() {
        let result = build_iteration_context(
            "CORE",
            "MEMORY",
            "WORKSPACE",
            "FILES",
            "NOTES",
            Some("CONNECTOR"),
            Some("ANALYSIS"),
            "## 可用专项技能\n- `biz-writing` — 商务写作",
        );

        let core_pos = result.find("CORE").expect("CORE missing");
        let mem_pos = result.find("MEMORY").expect("MEMORY missing");
        let ws_pos = result.find("WORKSPACE").expect("WORKSPACE missing");
        let file_pos = result.find("FILES").expect("FILES missing");
        let notes_pos = result.find("NOTES").expect("NOTES missing");
        let conn_pos = result.find("CONNECTOR").expect("CONNECTOR missing");
        let ana_pos = result.find("ANALYSIS").expect("ANALYSIS missing");
        let skill_pos = result.find("biz-writing").expect("skill catalog missing");

        assert!(core_pos < mem_pos);
        assert!(mem_pos < ws_pos);
        assert!(ws_pos < file_pos);
        assert!(file_pos < notes_pos);
        assert!(notes_pos < conn_pos);
        assert!(conn_pos < ana_pos);
        assert!(ana_pos < skill_pos);
    }

    #[test]
    fn test_build_local_skill_context_lists_workspace_skill_entrypoints() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("SKILL.md"), "root").unwrap();
        std::fs::create_dir_all(tmp.path().join("skills").join("mesh-analysis")).unwrap();
        std::fs::write(
            tmp.path()
                .join("skills")
                .join("mesh-analysis")
                .join("SKILL.md"),
            "mesh",
        )
        .unwrap();

        let result = build_local_skill_context(tmp.path(), None);

        assert!(result.contains("[本地技能入口]"));
        assert!(result.contains("- SKILL.md"));
        assert!(result.contains("- skills/mesh-analysis/SKILL.md"));
        assert!(result.contains("环境/依赖"));
        assert!(result.contains("helper/脚本"));
        assert!(result.contains("验证口径"));
        assert!(result.contains("Skill(find-skills)"));
        assert!(result.contains("不授权创建、安装、克隆或写入任何会被自动加载的 skills 目录"));
    }

    #[tokio::test]
    async fn test_build_env_info_with_authorized_workspace() {
        let workspace_path = std::path::PathBuf::from("/tmp/test-workspace");
        let authorized = Some((
            "/tmp/test-workspace/my-project".to_string(),
            "我的项目".to_string(),
        ));
        let result = build_env_info(
            &workspace_path,
            authorized.as_ref().map(|(p, n)| (p.as_str(), n.as_str())),
            None,
        )
        .await;
        assert!(
            result.contains("[当前环境]"),
            "must have env section header"
        );
        assert!(result.contains("已连接目录"), "must mention authorized dir");
        assert!(
            result.contains("my-project") || result.contains("我的项目"),
            "must include dir name"
        );
        assert!(result.contains("Platform:"), "must include platform");
    }

    #[tokio::test]
    async fn test_build_env_info_without_authorized_workspace() {
        let workspace_path = std::path::PathBuf::from("/tmp/test-workspace");
        let result = build_env_info(&workspace_path, None, None).await;
        assert!(
            result.contains("[当前环境]"),
            "must have env section header"
        );
        assert!(result.contains("工作目录"), "must include working dir");
        assert!(result.contains("Platform:"), "must include platform");
        assert!(
            !result.contains("已连接目录"),
            "must NOT mention authorized dir when absent"
        );
    }

    #[tokio::test]
    async fn test_build_env_info_platform_info() {
        let workspace_path = std::path::PathBuf::from("/tmp");
        let result = build_env_info(&workspace_path, None, None).await;
        let has_platform =
            result.contains("darwin") || result.contains("windows") || result.contains("linux");
        assert!(has_platform, "must include OS type, got: {}", result);
    }

    #[tokio::test]
    async fn test_build_env_info_includes_runtime_paths_and_natural_language_rule() {
        let workspace_path = std::path::PathBuf::from("/tmp/test-workspace");
        let runtime_info = ManagedRuntimeEnvInfo {
            runtime_root: "/cache/renlijia/current".into(),
            python_path: "/cache/renlijia/python/bin/python3".into(),
            node_path: "/cache/renlijia/node/bin/node".into(),
            npm_path: "/cache/renlijia/node/bin/npm".into(),
            npx_path: "/cache/renlijia/node/bin/npx".into(),
            uv_path: "/cache/renlijia/uv/bin/uv".into(),
            uvx_path: "/cache/renlijia/uv/bin/uvx".into(),
        };

        let result = build_env_info(&workspace_path, None, Some(&runtime_info)).await;

        assert!(result.contains("Runtime: 已安装"));
        assert!(result.contains("Runtime 当前目录: /cache/renlijia/current"));
        assert!(result.contains("Python: /cache/renlijia/python/bin/python3"));
        assert!(result.contains("Node: /cache/renlijia/node/bin/node"));
        assert!(result.contains("npm: /cache/renlijia/node/bin/npm"));
        assert!(result.contains("npx: /cache/renlijia/node/bin/npx"));
        assert!(result.contains("uv: /cache/renlijia/uv/bin/uv"));
        assert!(result.contains("uvx: /cache/renlijia/uv/bin/uvx"));
        assert!(result.contains("Node 全局包目录: /cache/renlijia/node/lib/node_modules"));
        let expected_node_command_dir = if cfg!(target_os = "windows") {
            "Node 命令目录: /cache/renlijia/node"
        } else {
            "Node 命令目录: /cache/renlijia/node/bin"
        };
        assert!(
            result.contains(expected_node_command_dir),
            "must include the platform-correct Node command dir, got:\n{result}"
        );
        assert!(result.contains("默认使用上面列出的绝对路径"));
        assert!(result.contains("只有用户明确要求系统环境时"));
        let expected_python_template = if cfg!(target_os = "windows") {
            r#"& "/cache/renlijia/uv/bin/uv" pip install <包名> --python "/cache/renlijia/python/bin/python3" --quiet"#
        } else {
            r#""/cache/renlijia/uv/bin/uv" pip install <包名> --python "/cache/renlijia/python/bin/python3" --quiet"#
        };
        assert!(
            result.contains(expected_python_template),
            "must include uv pip install template with concrete absolute paths, got:\n{result}"
        );
        let expected_node_template = if cfg!(target_os = "windows") {
            r#"& "/cache/renlijia/node/bin/npm" install -g <包名> --prefix "/cache/renlijia/node" --silent"#
        } else {
            r#""/cache/renlijia/node/bin/npm" install -g <包名> --prefix "/cache/renlijia/node" --silent"#
        };
        assert!(
            result.contains(expected_node_template),
            "must include npm install template with concrete absolute paths, got:\n{result}"
        );
        assert!(
            result.contains(r#"NODE_PATH="/cache/renlijia/node/lib/node_modules""#),
            "must teach Node global package resolution, got:\n{result}"
        );
        let expected_node_cli_template = if cfg!(target_os = "windows") {
            r#"& "/cache/renlijia/node\命令名.cmd" <参数>"#
        } else {
            r#""/cache/renlijia/node/bin/命令名" <参数>"#
        };
        assert!(
            result.contains(expected_node_cli_template),
            "must teach absolute CLI execution instead of npx, got:\n{result}"
        );
        assert!(result.contains("不要用 npx 运行已安装的包"));
        assert!(result.contains("禁止使用 --system"));
        assert!(result.contains("uv 装包是幂等的"));
        assert!(!result.contains("仁励家 Runtime"));
    }
}
