#[cfg(not(target_os = "windows"))]
use crate::storage::process_ext::NoWindowExt;

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
    managed_runtime_enabled: bool,
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

    let system_info =
        SystemRuntimeEnvInfo::detect(runtime_info.map(|info| info.runtime_root.as_path()));
    parts.push(system_info.format_for_env_info(managed_runtime_enabled));

    if managed_runtime_enabled {
        match runtime_info {
            Some(runtime_info) => parts.push(runtime_info.format_enabled_for_env_info()),
            None => parts.push(
                "AIjia 托管运行时：已开启，但当前没有检测到可用的 AIjia Runtime。\n\
                 当前不要假设裸 `node` / `python` / `uv` 已经命中 AIjia 托管运行时；如系统环境检测可用，可按用户意图使用系统环境。"
                    .to_string(),
            ),
        }
    } else {
        parts.push(
            "AIjia 托管运行时：已关闭（默认使用系统环境）\n\
             规则:\n\
             1. 本地 Bash / PowerShell / Skill / MCP 子进程不会注入 AIjia 托管运行时。\n\
             2. 裸 `node`、`npm`、`npx`、`python`、`python3`、`uv`、`uvx` 来自系统 PATH；如系统环境检测未发现对应命令，需要说明系统环境不可用。\n\
             3. 工具没有 `runtime_env` 参数，不要在工具调用里传这个字段。"
                .to_string(),
        );
    }

    format!("\n\n[当前环境]\n{}", parts.join("\n"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemRuntimeEnvInfo {
    pub node: Vec<std::path::PathBuf>,
    pub npm: Vec<std::path::PathBuf>,
    pub npx: Vec<std::path::PathBuf>,
    pub python: Vec<std::path::PathBuf>,
    pub uv: Vec<std::path::PathBuf>,
    pub uvx: Vec<std::path::PathBuf>,
}

impl SystemRuntimeEnvInfo {
    pub fn detect(managed_runtime_root: Option<&std::path::Path>) -> Self {
        Self {
            node: detect_system_command_paths(&["node"], managed_runtime_root),
            npm: detect_system_command_paths(&["npm"], managed_runtime_root),
            npx: detect_system_command_paths(&["npx"], managed_runtime_root),
            python: if cfg!(target_os = "windows") {
                detect_system_command_paths(&["python", "py"], managed_runtime_root)
            } else {
                detect_system_command_paths(&["python3", "python"], managed_runtime_root)
            },
            uv: detect_system_command_paths(&["uv"], managed_runtime_root),
            uvx: detect_system_command_paths(&["uvx"], managed_runtime_root),
        }
    }

    pub fn format_for_env_info(&self, managed_runtime_enabled: bool) -> String {
        let explicit_system_rule = if managed_runtime_enabled {
            "用户明确说“用系统自带 / 我电脑上的 / 不要用你自带”时，请直接使用这里检测到的系统绝对路径；不要在工具里用裸 `where node` / `Get-Command node` / `which node` 的第一条结果当系统路径，因为当前工具环境会优先看到 AIjia 托管运行时。确需复核时，只能验证这里列出的系统绝对路径，或过滤掉 AIjia Runtime 路径后再判断。"
        } else {
            "当前默认就是系统环境；如果这里未发现对应命令，说明系统环境不可用，不要假设 AIjia 托管运行时已注入。"
        };
        format!(
            "系统环境检测（未注入 AIjia 托管运行时）:\n\
             {}\n\
             {}\n\
             {}\n\
             {}\n\
             {}\n\
             {}\n\
             说明: {explicit_system_rule}",
            format_paths("node", &self.node),
            format_paths("npm", &self.npm),
            format_paths("npx", &self.npx),
            format_paths("python", &self.python),
            format_paths("uv", &self.uv),
            format_paths("uvx", &self.uvx),
        )
    }
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
    pub fn from_workspace_dependencies(
        deps: &crate::runtime::dependencies::WorkspaceDependencies,
    ) -> Self {
        Self {
            runtime_root: infer_runtime_root(&deps.python),
            python_path: deps.python.clone(),
            node_path: deps.node.clone(),
            npm_path: deps.npm.clone(),
            npx_path: deps.npx.clone(),
            uv_path: deps.uv.clone(),
            uvx_path: deps.uvx.clone(),
        }
    }

    pub fn format_enabled_for_env_info(&self) -> String {
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

        let python_command_name = if cfg!(target_os = "windows") {
            "python"
        } else {
            "python3"
        };
        let python_install_template =
            format!("uv pip install <包名> --python {python_command_name} --quiet");
        let node_install_template = "npm install -g <包名> --silent";
        let node_require_template = r#"node -e "require('<包名>'); console.log('ok')""#;
        let node_cli_template = "命令名 <参数>";
        format!(
            r#"AIjia 托管运行时：已开启（默认优先）
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
1. Bash / PowerShell / Skill / MCP 本地子进程默认会把 AIjia 托管运行时放到 PATH 前面；普通任务直接使用裸 `node`、`npm`、`npx`、`{python_command_name}`、`uv`、`uvx` 即可命中上面的 Runtime。
2. 不要为了使用默认 Runtime 而手写上面这些可执行文件的绝对路径；这些路径主要用于诊断、核对和故障说明。
3. 工具没有 `runtime_env` 参数，不要在工具调用里传这个字段。
4. 用户明确要求系统 Node / Python / npm / uv 时，直接使用上方“系统环境检测”里的系统绝对路径验证和执行；不要先运行裸 `node` / `python` / `where node` / `Get-Command node` 来判断系统环境，因为这些会优先命中 AIjia 托管运行时。系统环境未发现时说明不可用，不要静默切回 AIjia 托管运行时。
5. 安装第三方 Python 包使用以下模板（替换包名即可）：

   {python_install_template}

6. 禁止使用 --system / 裸 pip / python -m pip / pip install 后省略 --python。
7. uv 装包是幂等的：已安装的包会秒过（< 1s），不会重复下载，所以可以放心在每次需要时直接调用上述模板。
8. 安装第三方 Node 包使用以下模板（替换包名即可），不要安装到当前工作目录：

   {node_install_template}

9. 检查或 require Runtime Node 全局包时，直接使用裸 node；NODE_PATH 已由工具环境注入：

   {node_require_template}

10. 已安装的 Node CLI 可直接用命令名执行，例如：

   {node_cli_template}

   不要用 npx 运行已安装的包；npx 只在需要临时执行未安装包时使用。"#,
            runtime_root = self.runtime_root.display(),
            python = self.python_path.display(),
            node = self.node_path.display(),
            npm = self.npm_path.display(),
            npx = self.npx_path.display(),
            uv = self.uv_path.display(),
            uvx = self.uvx_path.display(),
            node_global_modules = node_global_modules.display(),
            node_cli_dir = node_cli_dir.display(),
            python_command_name = python_command_name,
            python_install_template = python_install_template,
            node_install_template = node_install_template,
            node_require_template = node_require_template,
            node_cli_template = node_cli_template,
        )
    }
}

fn infer_runtime_root(path: &std::path::Path) -> std::path::PathBuf {
    let mut current = path.parent();
    while let Some(dir) = current {
        let name = dir.file_name().and_then(|value| value.to_str());
        if matches!(name, Some("node" | "python" | "uv")) {
            return dir.parent().unwrap_or(dir).to_path_buf();
        }
        current = dir.parent();
    }
    path.parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| path.to_path_buf())
}

fn format_paths(label: &str, paths: &[std::path::PathBuf]) -> String {
    if paths.is_empty() {
        return format!("- {label}: 未发现");
    }
    let joined = paths
        .iter()
        .take(3)
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("; ");
    if paths.len() > 3 {
        format!("- {label}: {joined}; ...")
    } else {
        format!("- {label}: {joined}")
    }
}

fn detect_system_command_paths(
    commands: &[&str],
    managed_runtime_root: Option<&std::path::Path>,
) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for command in commands {
        for path in detect_command_paths(command) {
            if managed_runtime_root.is_some_and(|root| path_is_inside_managed_runtime(&path, root))
            {
                continue;
            }
            if !paths.iter().any(|existing| paths_equal(existing, &path)) {
                paths.push(path);
            }
        }
    }
    paths
}

fn detect_command_paths(command: &str) -> Vec<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        return detect_windows_command_paths(command);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut detect = std::process::Command::new("which");
        detect.arg("-a").arg(command);
        let mut paths = command_output_lines(&mut detect);

        #[cfg(target_os = "macos")]
        {
            if let Some(shell_paths) = detect_command_paths_from_login_shell(command) {
                paths.extend(shell_paths);
            }
        }

        paths
            .into_iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| std::path::PathBuf::from(line.trim()))
            .collect()
    }
}

#[cfg(target_os = "macos")]
fn detect_command_paths_from_login_shell(command: &str) -> Option<Vec<String>> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let output = std::process::Command::new(shell)
        .arg("-lc")
        .arg(format!("command -v {command}"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        crate::storage::console_decode::decode_console_bytes(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect(),
    )
}

#[cfg(not(target_os = "windows"))]
fn command_output_lines(command: &mut std::process::Command) -> Vec<String> {
    command.no_window();
    let Ok(output) = command.output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    crate::storage::console_decode::decode_console_bytes(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect()
}

#[cfg(target_os = "windows")]
fn detect_windows_command_paths(command: &str) -> Vec<std::path::PathBuf> {
    let Some(path_var) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    let names = windows_command_candidate_names(command);
    let mut paths: Vec<std::path::PathBuf> = Vec::new();

    for dir in std::env::split_paths(&path_var) {
        for name in &names {
            let candidate = dir.join(name);
            if candidate.is_file()
                && !paths
                    .iter()
                    .any(|existing| paths_equal(existing, &candidate))
            {
                paths.push(candidate);
            }
        }
    }

    paths
}

#[cfg(target_os = "windows")]
fn windows_command_candidate_names(command: &str) -> Vec<String> {
    let mut names = Vec::new();
    push_unique_candidate(&mut names, command.to_string());

    if std::path::Path::new(command).extension().is_some() {
        return names;
    }

    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    for ext in pathext
        .split(';')
        .map(str::trim)
        .filter(|ext| !ext.is_empty())
    {
        let ext = if ext.starts_with('.') {
            ext.to_string()
        } else {
            format!(".{ext}")
        };
        push_unique_candidate(&mut names, format!("{command}{ext}"));
    }

    names
}

#[cfg(target_os = "windows")]
fn push_unique_candidate(candidates: &mut Vec<String>, value: String) {
    if !candidates
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&value))
    {
        candidates.push(value);
    }
}

fn path_is_inside_managed_runtime(path: &std::path::Path, root: &std::path::Path) -> bool {
    let path = normalize_path_for_compare(path);
    let root = normalize_path_for_compare(root);
    path.starts_with(&root)
}

fn paths_equal(a: &std::path::Path, b: &std::path::Path) -> bool {
    normalize_path_for_compare(a) == normalize_path_for_compare(b)
}

fn normalize_path_for_compare(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if cfg!(target_os = "windows") {
        text.to_ascii_lowercase()
    } else {
        text
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
            true,
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
        let result = build_env_info(&workspace_path, None, None, true).await;
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
        let result = build_env_info(&workspace_path, None, None, true).await;
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

        let result = build_env_info(&workspace_path, None, Some(&runtime_info), true).await;

        assert!(result.contains("系统环境检测（未注入 AIjia 托管运行时）"));
        assert!(result.contains("AIjia 托管运行时：已开启（默认优先）"));
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
        assert!(result.contains("默认会把 AIjia 托管运行时放到 PATH 前面"));
        assert!(result.contains("直接使用裸 `node`"));
        assert!(result.contains("工具没有 `runtime_env` 参数"));
        assert!(result.contains("使用上方“系统环境检测”里的系统绝对路径"));
        let expected_python_template = if cfg!(target_os = "windows") {
            "uv pip install <包名> --python python --quiet"
        } else {
            "uv pip install <包名> --python python3 --quiet"
        };
        assert!(
            result.contains(expected_python_template),
            "must include uv pip install template with bare runtime commands, got:\n{result}"
        );
        assert!(
            result.contains("npm install -g <包名> --silent"),
            "must include npm install template with bare runtime commands, got:\n{result}"
        );
        assert!(
            result.contains(r#"node -e "require('<包名>'); console.log('ok')""#),
            "must teach Node global package resolution through injected NODE_PATH, got:\n{result}"
        );
        assert!(result.contains("不要用 npx 运行已安装的包"));
        assert!(result.contains("禁止使用 --system"));
        assert!(result.contains("uv 装包是幂等的"));
        assert!(!result.contains("默认使用上面列出的绝对路径"));
        assert!(!result.contains("必须带 NODE_PATH"));
        assert!(!result.contains("仁励家 Runtime"));
    }

    #[tokio::test]
    async fn test_build_env_info_switch_off_does_not_expose_runtime_paths() {
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

        let result = build_env_info(&workspace_path, None, Some(&runtime_info), false).await;

        assert!(result.contains("AIjia 托管运行时：已关闭（默认使用系统环境）"));
        assert!(result.contains("系统环境检测（未注入 AIjia 托管运行时）"));
        assert!(result.contains("不要假设 AIjia 托管运行时已注入"));
        assert!(result.contains("工具没有 `runtime_env` 参数"));
        assert!(!result.contains("Runtime 当前目录: /cache/renlijia/current"));
        assert!(!result.contains("默认会把 AIjia 托管运行时放到 PATH 前面"));
    }
}
