/// Build the dynamic context injected into the system prompt on each iteration.
///
/// This is kept separate from the static system prompt so that KV-cache prefix stability is
/// preserved: only the dynamic user message changes, not the system message.
///
/// The concatenation order and separators intentionally mirror Block 13 of `chat_runtime_impl.rs`
/// (lines ~2230-2291) so that the LLM sees an identical context layout regardless of which code
/// path produces the string.
pub fn build_iteration_context(
    core_memory: &str,
    workspace_context: &str,
    file_context: &str,
    analysis_notes: &str,
    precompute_result: Option<&str>,
    connector_context: Option<&str>,
    analysis_ctx_prompt: Option<&str>,
) -> String {
    let mut ctx = String::from("[动态上下文 — 请勿回复此消息]\n");

    // 1. Cognitive core memory — always loaded (cross-session knowledge base)
    if !core_memory.is_empty() {
        ctx.push_str("\n[核心记忆]\n");
        ctx.push_str(core_memory);
        ctx.push_str("\n");
    }

    // 2. Workspace context (file listing, project summary, etc.)
    if !workspace_context.is_empty() {
        ctx.push_str(workspace_context);
    }

    // 3. File context (contents of uploaded / referenced files)
    if !file_context.is_empty() {
        ctx.push_str(file_context);
    }

    // 4. Analysis notes (accumulated observations from previous iterations)
    if !analysis_notes.is_empty() {
        ctx.push_str(analysis_notes);
    }

    // 5. Precompute result — tagged block so the LLM treats it as pre-computed data
    if let Some(pc_result) = precompute_result {
        ctx.push_str("\n\n[precompute_result]\n");
        ctx.push_str("以下是系统自动计算的结果，请基于这些数据向用户展示分析结论。\n");
        ctx.push_str(pc_result);
        ctx.push_str("\n[/precompute_result]");
    }

    // 6. Internal connector context (browsing sessions / legacy app integrations)
    if let Some(connector) = connector_context {
        ctx.push_str("\n\n[内部系统浏览]\n");
        ctx.push_str(
            "你可以通过 browse_navigate 工具直接打开浏览器访问任意内部系统 URL，\
             用户在 Chrome 中登录后即可读取数据。\n\n",
        );
        ctx.push_str(connector);
        ctx.push_str("\n[/内部系统浏览]");
    }

    // 7. Analysis-mode context prompt (AnalysisContext::format_for_prompt())
    //    and step plan are injected by the caller and forwarded here as a single
    //    pre-formatted string.
    //
    //    NOTE: The original Block 13 also reads `_plan.md` from disk inline when
    //    `is_analysis` is true (workspace_path/analysis/{conversation_id}/_plan.md).
    //    That file-I/O is kept in the call-site for now so this function stays pure.
    //    TODO(S4-T5): lift plan-file reading into this function once the workspace-path
    //    is threaded through the ChatTurnRequest / TurnConfig.
    if let Some(ctx_prompt) = analysis_ctx_prompt {
        if !ctx_prompt.is_empty() {
            ctx.push_str(ctx_prompt);
        }
    }

    ctx
}

/// 构建会话级环境信息段落，注入到 dynamic context。
///
/// 对齐 claude-code-best 的 `computeSimpleEnvInfo`：
/// - 当前工作目录 / 已授权目录
/// - git 状态摘要（失败时静默跳过）
/// - 操作系统平台
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
        .arg("-C")
        .arg(&effective_path)
        .arg("status")
        .arg("--short")
        .arg("--branch")
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

    format!("\n\n[当前环境]\n{}", parts.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_HEADER: &str = "[动态上下文 — 请勿回复此消息]\n";

    #[test]
    fn test_empty_inputs_yields_only_header() {
        let result = build_iteration_context("", "", "", "", None, None, None);
        assert_eq!(result, EMPTY_HEADER);
    }

    #[test]
    fn test_core_memory_block() {
        let result = build_iteration_context("mem content", "", "", "", None, None, None);
        assert!(result.contains("\n[核心记忆]\n"));
        assert!(result.contains("mem content"));
        assert!(result.contains("\n[核心记忆]\nmem content\n"));
    }

    #[test]
    fn test_precompute_result_block() {
        let result = build_iteration_context("", "", "", "", Some("computed data"), None, None);
        assert!(result.contains("[precompute_result]\n"));
        assert!(result.contains("computed data"));
        assert!(result.contains("[/precompute_result]"));
    }

    #[test]
    fn test_connector_context_block() {
        let result = build_iteration_context("", "", "", "", None, Some("connector info"), None);
        assert!(result.contains("[内部系统浏览]\n"));
        assert!(result.contains("connector info"));
        assert!(result.contains("[/内部系统浏览]"));
    }

    #[test]
    fn test_concatenation_order() {
        let result = build_iteration_context(
            "CORE",
            "WORKSPACE",
            "FILES",
            "NOTES",
            Some("PRECOMPUTE"),
            Some("CONNECTOR"),
            Some("ANALYSIS"),
        );

        let core_pos = result.find("CORE").expect("CORE missing");
        let ws_pos = result.find("WORKSPACE").expect("WORKSPACE missing");
        let file_pos = result.find("FILES").expect("FILES missing");
        let notes_pos = result.find("NOTES").expect("NOTES missing");
        let pre_pos = result.find("PRECOMPUTE").expect("PRECOMPUTE missing");
        let conn_pos = result.find("CONNECTOR").expect("CONNECTOR missing");
        let ana_pos = result.find("ANALYSIS").expect("ANALYSIS missing");

        assert!(core_pos < ws_pos);
        assert!(ws_pos < file_pos);
        assert!(file_pos < notes_pos);
        assert!(notes_pos < pre_pos);
        assert!(pre_pos < conn_pos);
        assert!(conn_pos < ana_pos);
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
        let result = build_env_info(&workspace_path, None).await;
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
        let result = build_env_info(&workspace_path, None).await;
        let has_platform =
            result.contains("darwin") || result.contains("windows") || result.contains("linux");
        assert!(has_platform, "must include OS type, got: {}", result);
    }

    #[tokio::test]
    async fn test_build_env_info_non_git_directory_skips_git_quietly() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let workspace_path = temp_dir.path().to_path_buf();

        let result = build_env_info(&workspace_path, None).await;

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
}
