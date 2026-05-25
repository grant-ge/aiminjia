/// Build the per-iteration user/dynamic context carrying runtime deltas.
///
/// This is kept separate from the static system prompt so that KV-cache prefix stability is
/// preserved: only the dynamic user message changes, not the system message.
///
/// The concatenation order and separators intentionally mirror Block 13 of `chat_runtime_impl.rs`
/// (lines ~2230-2291) so that the LLM sees an identical context layout regardless of which code
/// path produces the string.
use crate::storage::process_ext::NoWindowExt;
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

/// 构建会话级环境信息段落，注入到 dynamic context。
///
/// 对齐 claude-code-best 的 `computeSimpleEnvInfo`：
/// - 当前工作目录 / 已授权目录
/// - git 状态摘要（失败时静默跳过）
/// - 操作系统平台
/// - Runtime 工具绝对路径（可选）
///
/// Contract:
/// - `authorized = Some((root_path_str, display_name))` 表示用户已连接本地目录，且它是
///   LLM 应看到的“当前工作目录”的权威来源。
/// - 在该场景下，输出只展示 `authorized` 对应的目录信息；`workspace_path` 仅作为
///   fallback 输入保留，不出现在输出里。
/// - git status 也基于 `authorized` 路径执行，而不是 `workspace_path`。
/// - 当 `authorized = None` 时，退回使用 `workspace_path` 作为工作目录展示和 git
///   status 的执行路径。
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

    // 2. Git 状态（静默失败）
    let effective_path = authorized
        .map(|(p, _)| std::path::PathBuf::from(p))
        .unwrap_or_else(|| workspace_path.clone());

    if let Ok(output) = tokio::process::Command::new("git")
        .arg("-c")
        .arg("core.quotepath=false")
        .arg("-C")
        .arg(&effective_path)
        .arg("status")
        .arg("--short")
        .arg("--branch")
        .no_window()
        .output()
        .await
    {
        if output.status.success() {
            let status_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !status_str.is_empty() {
                let lines: Vec<&str> = status_str.lines().take(10).collect();
                parts.push(format!("Git: {}", lines.join(" | ")));
            }
        }
    }

    // 3. 平台信息
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
        // Shell 语法因平台而异：Windows 走 powershell，需要 `& "exe"` call operator；
        // macOS / Linux 走 /bin/sh，必须裸命令（路径用引号包住即可）。
        // 给 LLM 的模板写错平台 → 工具执行直接 syntax error。
        let install_template = if cfg!(target_os = "windows") {
            format!(
                r#"& "{uv}" pip install <包名> --python "{python}" --quiet"#,
                uv = self.uv_path.display(),
                python = self.python_path.display(),
            )
        } else {
            format!(
                r#""{uv}" pip install <包名> --python "{python}" --quiet"#,
                uv = self.uv_path.display(),
                python = self.python_path.display(),
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

规则:
1. 运行 Python / Node / npm / npx / uv 命令时，默认使用上面列出的绝对路径；只有用户明确要求系统环境时，才使用系统 PATH 中的命令。
2. 安装第三方 Python 包必须使用以下模板（替换包名即可），禁止任何变体：

   {install_template}

3. 禁止使用 --system / 裸 pip / python -m pip / pip install 后省略 --python。
4. uv 装包是幂等的：已安装的包会秒过（< 1s），不会重复下载，所以可以放心在每次需要时直接调用上述模板。"#,
            runtime_root = self.runtime_root.display(),
            python = self.python_path.display(),
            node = self.node_path.display(),
            npm = self.npm_path.display(),
            npx = self.npx_path.display(),
            uv = self.uv_path.display(),
            uvx = self.uvx_path.display(),
            install_template = install_template,
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
    async fn test_build_env_info_non_git_directory_skips_git_quietly() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let workspace_path = temp_dir.path().to_path_buf();

        let result = build_env_info(&workspace_path, None, None).await;

        assert!(
            result.contains("[当前环境]"),
            "must have env section header"
        );
        assert!(result.contains("工作目录:"), "must include working dir");
        assert!(
            !result.contains("Git:"),
            "must skip git section in non-git dir"
        );
    }

    #[tokio::test]
    async fn test_build_env_info_authorized_path_prefers_git_status() {
        let workspace_dir = tempfile::tempdir().expect("create workspace temp dir");
        let authorized_dir = tempfile::tempdir().expect("create authorized temp dir");
        let workspace_path = workspace_dir.path().to_path_buf();
        let authorized_root = authorized_dir.path().to_path_buf();

        let git_init = std::process::Command::new("git")
            .args(["init", authorized_root.to_string_lossy().as_ref()])
            .output()
            .expect("run git init");
        assert!(git_init.status.success(), "git init must succeed");

        std::fs::write(authorized_root.join("untracked.txt"), "hello")
            .expect("write untracked file");

        let result = build_env_info(
            &workspace_path,
            Some((authorized_root.to_string_lossy().as_ref(), "授权目录")),
            None,
        )
        .await;

        assert!(
            result.contains("已连接目录: 授权目录 ("),
            "must include authorized dir header"
        );
        assert!(
            result.contains("Git:"),
            "must include git status from authorized dir"
        );
        assert!(
            result.contains("untracked.txt") || result.contains("##"),
            "must reflect authorized git repo status, got: {}",
            result
        );
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
        assert!(result.contains("默认使用上面列出的绝对路径"));
        assert!(result.contains("只有用户明确要求系统环境时"));
        let expected_template = if cfg!(target_os = "windows") {
            r#"& "/cache/renlijia/uv/bin/uv" pip install <包名> --python "/cache/renlijia/python/bin/python3" --quiet"#
        } else {
            r#""/cache/renlijia/uv/bin/uv" pip install <包名> --python "/cache/renlijia/python/bin/python3" --quiet"#
        };
        assert!(
            result.contains(expected_template),
            "must include uv pip install template with concrete absolute paths, got:\n{result}"
        );
        assert!(result.contains("禁止使用 --system"));
        assert!(result.contains("uv 装包是幂等的"));
        assert!(!result.contains("仁励家 Runtime"));
    }
}
