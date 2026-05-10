//! Skill-Smith (小程) — 对话式创建 SKILL.md 的运行时工具集。
//!
//! 5 个 RuntimeTool 共同支撑「小程」数字员工的对话流程：
//!
//! 1. `skill_create_draft` — 在 `~/.renlijia/users/{scope}/skill-drafts/<draft_id>/` 创建草稿目录
//! 2. `skill_write_md`     — 整体写入 SKILL.md（YAML frontmatter + Markdown body）
//! 3. `skill_add_file`     — 写入额外的 scripts/ 或 references/ 文件
//! 4. `skill_validate`     — 6 项校验，errors[].fix_hint 中文友好让 LLM 自修复
//! 5. `skill_install`      — 把草稿复制到 `~/.renlijia/users/{scope}/skills/<name>/`
//!
//! 安全边界：
//! - draft_id / 额外文件路径走 `safe_filename` 校验
//! - scripts/references 仅允许一级子目录
//! - 安装时同名冲突默认拒绝（force=true 才覆盖）

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::{ToolDefinition, ToolKind};
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;
use crate::storage::skill_draft_store::{DraftMeta, SkillDraftStore};
use crate::storage::{AiJiaHome, UserScope};

// ============================================================================
// Shared dependencies
// ============================================================================

/// 把 SkillDraftStore + 当前会话作用域注入到工具构造时。
#[derive(Clone)]
pub struct SkillSmithDeps {
    pub store: Arc<SkillDraftStore>,
    pub home: Arc<AiJiaHome>,
    pub scope: UserScope,
    pub conversation_id: String,
}

impl SkillSmithDeps {
    pub fn new(
        store: Arc<SkillDraftStore>,
        home: Arc<AiJiaHome>,
        scope: UserScope,
        conversation_id: String,
    ) -> Self {
        Self {
            store,
            home,
            scope,
            conversation_id,
        }
    }
}

// ============================================================================
// 1. skill_create_draft
// ============================================================================

#[derive(Deserialize)]
struct CreateDraftInput {
    name: String,
    description: String,
}

pub struct SkillCreateDraftTool {
    deps: SkillSmithDeps,
}

impl SkillCreateDraftTool {
    pub fn new(deps: SkillSmithDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl RuntimeTool for SkillCreateDraftTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "skill_create_draft",
            "创建一个新的技能草稿目录。draft_id 自动绑定当前会话。后续 skill_write_md / skill_add_file / skill_install 都用返回的 draft_id 操作。",
        )
        .with_kind(ToolKind::Power)
        .with_destructive(false)
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let CreateDraftInput { name, description } = serde_json::from_value(input)
            .map_err(|e| ToolError::InputValidationError {
                tool_name: "skill_create_draft".into(),
                message: format!("invalid input: {}", e),
            })?;

        validate_skill_name(&name)
            .map_err(|e| ToolError::InputValidationError {
                tool_name: "skill_create_draft".into(),
                message: e.to_string(),
            })?;

        let draft_id = self.deps.conversation_id.clone();
        // 同会话已有草稿则视为继续编辑（返回 existing）
        let meta = match self.deps.store.read_meta(&self.deps.scope, &draft_id) {
            Ok(existing) => existing,
            Err(_) => self
                .deps
                .store
                .create(
                    &self.deps.scope,
                    &draft_id,
                    Some(self.deps.conversation_id.clone()),
                    &name,
                    &description,
                )
                .map_err(|e| ToolError::ExecutionFailed(format!("create draft: {}", e)))?,
        };
        let draft_dir = self
            .deps
            .store
            .draft_dir(&self.deps.scope, &draft_id)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let data = json!({
            "draft_id": meta.draft_id,
            "name": meta.name,
            "draft_dir": draft_dir.to_string_lossy(),
        });
        let content = format!(
            "✅ 草稿已创建：{}\n   draft_id: {}\n   目录：{}\n\n下一步：用 skill_write_md 写入 SKILL.md。",
            meta.name,
            meta.draft_id,
            draft_dir.display()
        );
        Ok(ToolResult::new("skill_create_draft", content, Some(data)))
    }
}

// ============================================================================
// 2. skill_write_md
// ============================================================================

#[derive(Deserialize)]
struct WriteMdInput {
    draft_id: String,
    /// 完整 SKILL.md 文件内容（包含 `---\n…\n---\nbody`）。
    content: String,
}

pub struct SkillWriteMdTool {
    deps: SkillSmithDeps,
}

impl SkillWriteMdTool {
    pub fn new(deps: SkillSmithDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl RuntimeTool for SkillWriteMdTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "skill_write_md",
            "把完整的 SKILL.md 内容（YAML frontmatter + Markdown body）整体写入草稿。\n要求 content 必须以 '---' 开头并包含 frontmatter（至少 name + description 字段）。",
        )
        .with_kind(ToolKind::Power)
        .with_destructive(true)
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let WriteMdInput { draft_id, content } =
            serde_json::from_value(input).map_err(|e| ToolError::InputValidationError {
                tool_name: "skill_write_md".into(),
                message: format!("invalid input: {}", e),
            })?;

        self.deps
            .store
            .write_skill_md(&self.deps.scope, &draft_id, &content)
            .map_err(|e| ToolError::ExecutionFailed(format!("write SKILL.md: {}", e)))?;

        let bytes = content.len();
        Ok(ToolResult::new(
            "skill_write_md",
            format!("✅ 已写入 SKILL.md（{} 字节）。下一步建议：调用 skill_validate 验证。", bytes),
            Some(json!({"draft_id": draft_id, "bytes": bytes})),
        ))
    }
}

// ============================================================================
// 3. skill_add_file
// ============================================================================

#[derive(Deserialize)]
struct AddFileInput {
    draft_id: String,
    /// 仅允许 `scripts/<name>` 或 `references/<name>` 形式。
    path: String,
    content: String,
}

pub struct SkillAddFileTool {
    deps: SkillSmithDeps,
}

impl SkillAddFileTool {
    pub fn new(deps: SkillSmithDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl RuntimeTool for SkillAddFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "skill_add_file",
            "在草稿目录下写入额外文件（scripts/ 或 references/ 子目录），用于附加 Python 脚本或参考资料。\npath 格式必须是 'scripts/<filename>' 或 'references/<filename>'，仅一级子目录。",
        )
        .with_kind(ToolKind::Power)
        .with_destructive(true)
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let AddFileInput {
            draft_id,
            path,
            content,
        } = serde_json::from_value(input).map_err(|e| ToolError::InputValidationError {
            tool_name: "skill_add_file".into(),
            message: format!("invalid input: {}", e),
        })?;

        self.deps
            .store
            .write_extra_file(&self.deps.scope, &draft_id, &path, &content)
            .map_err(|e| ToolError::ExecutionFailed(format!("write extra file: {}", e)))?;

        Ok(ToolResult::new(
            "skill_add_file",
            format!("✅ 已写入 {}", path),
            Some(json!({"draft_id": draft_id, "path": path})),
        ))
    }
}

// ============================================================================
// 4. skill_validate
// ============================================================================

#[derive(Deserialize)]
struct ValidateInput {
    draft_id: String,
}

pub struct SkillValidateTool {
    deps: SkillSmithDeps,
}

impl SkillValidateTool {
    pub fn new(deps: SkillSmithDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl RuntimeTool for SkillValidateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "skill_validate",
            "对草稿做 6 项校验：① frontmatter 必填字段（name/description）② name kebab-case ③ frontmatter YAML 合法 ④ body 非空 ⑤ scripts/references 引用存在 ⑥ allowed_tools 引用工具存在。\n返回 errors[].fix_hint 中文提示，便于 LLM 自动修复。",
        )
        .with_kind(ToolKind::Support)
        .with_read_only(true)
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let ValidateInput { draft_id } = serde_json::from_value(input).map_err(|e| {
            ToolError::InputValidationError {
                tool_name: "skill_validate".into(),
                message: format!("invalid input: {}", e),
            }
        })?;

        let body = self
            .deps
            .store
            .read_skill_md(&self.deps.scope, &draft_id)
            .map_err(|e| ToolError::ExecutionFailed(format!("read SKILL.md: {}", e)))?;
        let draft_dir = self
            .deps
            .store
            .draft_dir(&self.deps.scope, &draft_id)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let meta = self
            .deps
            .store
            .read_meta(&self.deps.scope, &draft_id)
            .ok();

        let errors = run_validation(&body, draft_dir, meta.as_ref());
        let ok = errors.is_empty();
        let summary = if ok {
            "✅ 校验通过，可以 skill_install 安装。".to_string()
        } else {
            format!(
                "❌ 校验失败，共 {} 处问题：\n{}",
                errors.len(),
                errors
                    .iter()
                    .enumerate()
                    .map(|(i, e)| format!(
                        "  {}. [{}] {}\n     建议：{}",
                        i + 1,
                        e.code,
                        e.message,
                        e.fix_hint
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };
        let data = json!({
            "draft_id": draft_id,
            "ok": ok,
            "errors": errors.iter().map(|e| json!({
                "code": e.code,
                "message": e.message,
                "fix_hint": e.fix_hint,
            })).collect::<Vec<_>>(),
        });
        Ok(ToolResult::new("skill_validate", summary, Some(data)))
    }
}

// ============================================================================
// 5. skill_install
// ============================================================================

#[derive(Deserialize)]
struct InstallInput {
    draft_id: String,
    #[serde(default)]
    force: bool,
}

pub struct SkillInstallTool {
    deps: SkillSmithDeps,
}

impl SkillInstallTool {
    pub fn new(deps: SkillSmithDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl RuntimeTool for SkillInstallTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "skill_install",
            "把草稿复制到 ~/.renlijia/users/{scope}/skills/<name>/，使其在新对话中可用。\n同名冲突时返回 status='conflict'，除非传 force=true 强制覆盖。",
        )
        .with_kind(ToolKind::Power)
        .with_destructive(true)
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let InstallInput { draft_id, force } = serde_json::from_value(input).map_err(|e| {
            ToolError::InputValidationError {
                tool_name: "skill_install".into(),
                message: format!("invalid input: {}", e),
            }
        })?;

        let body = self
            .deps
            .store
            .read_skill_md(&self.deps.scope, &draft_id)
            .map_err(|e| ToolError::ExecutionFailed(format!("read SKILL.md: {}", e)))?;
        let frontmatter = parse_frontmatter(&body).map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "frontmatter 解析失败：{}。请先调用 skill_validate。",
                e
            ))
        })?;
        let name = frontmatter
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("frontmatter 缺少 name 字段".into()))?
            .to_string();
        validate_skill_name(&name).map_err(|e| {
            ToolError::ExecutionFailed(format!("name 无效：{}", e))
        })?;

        let target = self.deps.home.user_skills_dir(&self.deps.scope).join(&name);
        if target.exists() && !force {
            let data = json!({
                "draft_id": draft_id,
                "status": "conflict",
                "name": name,
                "existing_path": target.to_string_lossy(),
            });
            return Ok(ToolResult::new(
                "skill_install",
                format!(
                    "⚠️ 同名技能 '{}' 已存在于 {}\n   - 让用户决定：覆盖 / 改名 / 取消（用 ask_user_question）\n   - 覆盖时再次调用 skill_install 并传 force=true",
                    name,
                    target.display()
                ),
                Some(data),
            ));
        }

        if target.exists() {
            std::fs::remove_dir_all(&target).map_err(|e| {
                ToolError::ExecutionFailed(format!("rm existing target: {}", e))
            })?;
        }

        self.deps
            .store
            .copy_to(&self.deps.scope, &draft_id, &target)
            .map_err(|e| ToolError::ExecutionFailed(format!("install (copy): {}", e)))?;
        self.deps
            .store
            .mark_installed(&self.deps.scope, &draft_id, &target)
            .ok();

        let data = json!({
            "draft_id": draft_id,
            "status": "installed",
            "name": name,
            "installed_to": target.to_string_lossy(),
        });
        Ok(ToolResult::new(
            "skill_install",
            format!(
                "✅ 已安装到 {}\n   下次新对话中可以让 AI 调用 load_skill 加载，或者你直接用 /{} 触发。",
                target.display(),
                name
            ),
            Some(data),
        ))
    }
}

// ============================================================================
// Validation
// ============================================================================

#[derive(Debug, Clone)]
pub struct ValidationError {
    pub code: &'static str,
    pub message: String,
    pub fix_hint: String,
}

fn run_validation(body: &str, draft_dir: PathBuf, _meta: Option<&DraftMeta>) -> Vec<ValidationError> {
    let mut errors = vec![];

    // ① frontmatter 存在
    let (frontmatter, body_text) = match split_frontmatter(body) {
        Ok(v) => v,
        Err(e) => {
            errors.push(ValidationError {
                code: "frontmatter.missing",
                message: format!("frontmatter 缺失或格式错误：{}", e),
                fix_hint: "SKILL.md 必须以 '---' 开头，包含至少 name 和 description 两个 YAML 字段，再用 '---' 闭合。".into(),
            });
            return errors;
        }
    };

    // ② frontmatter 是合法 YAML
    let fm: serde_yaml::Value = match serde_yaml::from_str(&frontmatter) {
        Ok(v) => v,
        Err(e) => {
            errors.push(ValidationError {
                code: "frontmatter.yaml_invalid",
                message: format!("YAML 解析失败：{}", e),
                fix_hint: "检查 YAML 缩进、冒号后空格、列表用 '- ' 起头。".into(),
            });
            return errors;
        }
    };
    let fm_map = fm.as_mapping();

    // ③ 必填字段 name
    let name = fm_map
        .and_then(|m| m.get(serde_yaml::Value::String("name".into())))
        .and_then(|v| v.as_str());
    match name {
        None => errors.push(ValidationError {
            code: "frontmatter.missing_name",
            message: "frontmatter 缺少 name 字段".into(),
            fix_hint: "在 frontmatter 加一行 'name: my-skill'（kebab-case）。".into(),
        }),
        Some(n) => {
            if let Err(e) = validate_skill_name(n) {
                errors.push(ValidationError {
                    code: "frontmatter.bad_name",
                    message: format!("name 不合法：{}", e),
                    fix_hint: "name 必须是 kebab-case（小写字母/数字/连字符），3-40 字符，例如 'resume-jd-match'。".into(),
                });
            }
        }
    }

    // ③ 必填字段 description
    if fm_map
        .and_then(|m| m.get(serde_yaml::Value::String("description".into())))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        errors.push(ValidationError {
            code: "frontmatter.missing_description",
            message: "frontmatter 缺少 description 字段".into(),
            fix_hint: "用一句话描述这个技能在什么场景下被 AI 选用，例如 'description: 当用户上传简历和 JD 想做匹配评分时使用'。".into(),
        });
    }

    // ④ body 非空
    if body_text.trim().is_empty() {
        errors.push(ValidationError {
            code: "body.empty",
            message: "SKILL.md body 为空".into(),
            fix_hint: "在 frontmatter 之后写明：技能要做什么、输入是什么、输出格式、需要调用哪些工具。".into(),
        });
    }

    // ⑤ scripts/references 引用文件存在（粗略：扫 body 里的 scripts/ references/ 路径）
    for (subdir, kind) in &[("scripts", "script"), ("references", "reference")] {
        for cap in find_referenced_paths(&body_text, subdir) {
            let path = draft_dir.join(subdir).join(&cap);
            if !path.exists() {
                errors.push(ValidationError {
                    code: "ref.missing_file",
                    message: format!("引用了 {}/{} 但文件不存在", subdir, cap),
                    fix_hint: format!(
                        "调用 skill_add_file 写入 path='{}/{}'，或从 SKILL.md body 中删除该 {} 引用。",
                        subdir, cap, kind
                    ),
                });
            }
        }
    }

    // ⑥ allowed_tools 引用的工具存在（用静态白名单验证常见工具，避免硬依赖 catalog）
    if let Some(tools) = fm_map
        .and_then(|m| m.get(serde_yaml::Value::String("allowed_tools".into())))
        .and_then(|v| v.as_sequence())
    {
        for tool in tools {
            if let Some(name) = tool.as_str() {
                if !is_known_tool(name) {
                    errors.push(ValidationError {
                        code: "frontmatter.unknown_tool",
                        message: format!("allowed_tools 引用了未知工具 '{}'", name),
                        fix_hint: format!(
                            "把 '{}' 从 allowed_tools 移除，或检查拼写。常见工具：workspace, ask_user_question, memory, network, load_skill, skill_create_draft, skill_write_md, skill_add_file, skill_validate, skill_install。",
                            name
                        ),
                    });
                }
            }
        }
    }

    errors
}

/// 分离 frontmatter 与 body：
/// 输入须以 `---\n` 开头，再有一行 `---` 作为结束，否则视为缺失。
fn split_frontmatter(s: &str) -> Result<(String, String)> {
    let s = s.trim_start_matches('\u{feff}');
    let s = s.strip_prefix("---").ok_or_else(|| anyhow!("missing leading ---"))?;
    let s = s.trim_start_matches(|c: char| c == '\r' || c == '\n');
    let end = s.find("\n---").ok_or_else(|| anyhow!("missing closing ---"))?;
    let frontmatter = s[..end].to_string();
    let after = &s[end + 4..]; // skip "\n---"
    let body = after.trim_start_matches(|c: char| c == '\r' || c == '\n').to_string();
    Ok((frontmatter, body))
}

/// 在 body 文本里寻找形如 `scripts/foo.py` 或 `references/bar.md` 的相对路径引用。
fn find_referenced_paths(body: &str, subdir: &str) -> Vec<String> {
    let prefix = format!("{}/", subdir);
    let mut out = vec![];
    for token in body.split(|c: char| {
        c.is_whitespace() || matches!(c, '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '<' | '>' | ',' | ';')
    }) {
        if let Some(rest) = token.strip_prefix(&prefix) {
            // 仅一级文件名，不能含 / 或 ..
            if !rest.is_empty() && !rest.contains('/') && !rest.contains('\\') && rest != ".." {
                out.push(rest.trim_end_matches(|c: char| matches!(c, '.' | ',' | ';' | ':')).to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 是否是已知工具（白名单 — 内置 + skill_smith 自身工具）。
fn is_known_tool(name: &str) -> bool {
    matches!(
        name,
        "workspace"
            | "Glob"
            | "Read"
            | "Write"
            | "Edit"
            | "Bash"
            | "PowerShell"
            | "Grep"
            | "AskUserQuestion"
            | "ask_user_question"
            | "TaskCreate"
            | "TaskUpdate"
            | "TaskList"
            | "TaskGet"
            | "TaskOutput"
            | "TaskStop"
            | "Memory"
            | "memory"
            | "SaveMemory"
            | "SearchMemory"
            | "LoadCoreMemory"
            | "DistillMemory"
            | "WebSearch"
            | "Skill"
            | "load_skill"
            | "browse_data"
            | "browse_navigate"
            | "browse_and_extract"
            | "spawn_subagent"
            | "skill_create_draft"
            | "skill_write_md"
            | "skill_add_file"
            | "skill_validate"
            | "skill_install"
    )
}

/// 把 frontmatter YAML 解析成一个 Mapping。仅在 install 时用，validation 已自带更细的检查。
fn parse_frontmatter(body: &str) -> Result<serde_yaml::Mapping> {
    let (fm, _) = split_frontmatter(body)?;
    let v: serde_yaml::Value = serde_yaml::from_str(&fm)?;
    v.as_mapping()
        .cloned()
        .ok_or_else(|| anyhow!("frontmatter is not a YAML mapping"))
}

fn validate_skill_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("name 为空"));
    }
    if name.len() < 3 || name.len() > 40 {
        return Err(anyhow!("name 长度需在 3-40，当前 {}", name.len()));
    }
    let valid = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid {
        return Err(anyhow!(
            "name 仅允许小写字母 / 数字 / 连字符，当前 '{}'",
            name
        ));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(anyhow!("name 不能以连字符开头或结尾"));
    }
    if name.contains("--") {
        return Err(anyhow!("name 不能包含连续连字符"));
    }
    Ok(())
}

// ============================================================================
// Catalog JSON schemas (for LLM tool definitions)
// ============================================================================

/// 注册到 ToolCatalog 的 5 条 (id, schema) 元组。
pub fn catalog_entries() -> Vec<(ToolDefinition, Value)> {
    vec![
        (
            SkillCreateDraftTool::dummy().definition(),
            json!({
                "type": "object",
                "required": ["name", "description"],
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "技能 ID（kebab-case，小写+连字符，3-40 字符）",
                    },
                    "description": {
                        "type": "string",
                        "description": "技能一句话描述（描述什么场景下使用，驱动 LLM 选用）",
                    }
                }
            }),
        ),
        (
            SkillWriteMdTool::dummy().definition(),
            json!({
                "type": "object",
                "required": ["draft_id", "content"],
                "properties": {
                    "draft_id": { "type": "string" },
                    "content": {
                        "type": "string",
                        "description": "完整 SKILL.md 内容，必须以 '---' 开头包含 frontmatter（name + description 必填），再以 '---' 闭合 + Markdown body",
                    }
                }
            }),
        ),
        (
            SkillAddFileTool::dummy().definition(),
            json!({
                "type": "object",
                "required": ["draft_id", "path", "content"],
                "properties": {
                    "draft_id": { "type": "string" },
                    "path": {
                        "type": "string",
                        "description": "相对路径，仅允许 'scripts/<filename>' 或 'references/<filename>'，一级深度",
                    },
                    "content": { "type": "string" }
                }
            }),
        ),
        (
            SkillValidateTool::dummy().definition(),
            json!({
                "type": "object",
                "required": ["draft_id"],
                "properties": {
                    "draft_id": { "type": "string" }
                }
            }),
        ),
        (
            SkillInstallTool::dummy().definition(),
            json!({
                "type": "object",
                "required": ["draft_id"],
                "properties": {
                    "draft_id": { "type": "string" },
                    "force": {
                        "type": "boolean",
                        "default": false,
                        "description": "true 时覆盖同名已安装技能",
                    }
                }
            }),
        ),
    ]
}

// 让 catalog_entries() 不需要真正构造工具实例就能拿到 ToolDefinition。
trait DummyCtor {
    fn dummy() -> Self;
}
impl DummyCtor for SkillCreateDraftTool {
    fn dummy() -> Self {
        Self {
            deps: SkillSmithDeps {
                store: Arc::new(SkillDraftStore::new(Arc::new(AiJiaHome::from_path(
                    PathBuf::from("/tmp/__skill_smith_dummy__"),
                )))),
                home: Arc::new(AiJiaHome::from_path(PathBuf::from("/tmp/__skill_smith_dummy__"))),
                scope: UserScope::new(0, 0),
                conversation_id: String::new(),
            },
        }
    }
}
impl DummyCtor for SkillWriteMdTool {
    fn dummy() -> Self {
        Self {
            deps: SkillCreateDraftTool::dummy().deps,
        }
    }
}
impl DummyCtor for SkillAddFileTool {
    fn dummy() -> Self {
        Self {
            deps: SkillCreateDraftTool::dummy().deps,
        }
    }
}
impl DummyCtor for SkillValidateTool {
    fn dummy() -> Self {
        Self {
            deps: SkillCreateDraftTool::dummy().deps,
        }
    }
}
impl DummyCtor for SkillInstallTool {
    fn dummy() -> Self {
        Self {
            deps: SkillCreateDraftTool::dummy().deps,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, SkillSmithDeps) {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(AiJiaHome::from_path(tmp.path().to_path_buf()));
        let store = Arc::new(SkillDraftStore::new(home.clone()));
        let scope = UserScope::new(1, 1);
        let deps = SkillSmithDeps::new(store, home, scope, "conv-test-1".into());
        (tmp, deps)
    }

    fn ctx() -> ToolExecutionContext {
        use crate::runtime::cancellation::CancellationToken;
        use crate::runtime::ids::{RunId, SessionId};
        ToolExecutionContext::new(
            SessionId::new("s1"),
            RunId::new("r1"),
            None,
            "tc1",
            CancellationToken::new(),
        )
    }

    #[tokio::test]
    async fn create_draft_uses_conversation_id_as_draft_id() {
        let (_tmp, deps) = fixture();
        let tool = SkillCreateDraftTool::new(deps.clone());
        let result = tool
            .execute(
                json!({"name": "my-skill", "description": "x"}),
                ctx(),
            )
            .await
            .unwrap();
        let data = result.data.unwrap();
        assert_eq!(data["draft_id"], "conv-test-1");
        assert_eq!(data["name"], "my-skill");
    }

    #[tokio::test]
    async fn create_draft_idempotent_on_same_conversation() {
        let (_tmp, deps) = fixture();
        let tool = SkillCreateDraftTool::new(deps.clone());
        let r1 = tool
            .execute(json!({"name": "tmp-skill", "description": "y"}), ctx())
            .await
            .unwrap();
        let r2 = tool
            .execute(json!({"name": "tmp-skill", "description": "y"}), ctx())
            .await
            .unwrap();
        assert_eq!(r1.data.unwrap()["draft_id"], r2.data.unwrap()["draft_id"]);
    }

    #[tokio::test]
    async fn write_and_validate_round_trip() {
        let (_tmp, deps) = fixture();
        SkillCreateDraftTool::new(deps.clone())
            .execute(json!({"name": "tmp-skill", "description": "y"}), ctx())
            .await
            .unwrap();
        let body = "---\nname: my-skill\ndescription: 一段测试\n---\n# 标题\n\n说明";
        SkillWriteMdTool::new(deps.clone())
            .execute(
                json!({"draft_id": deps.conversation_id, "content": body}),
                ctx(),
            )
            .await
            .unwrap();
        let r = SkillValidateTool::new(deps.clone())
            .execute(json!({"draft_id": deps.conversation_id}), ctx())
            .await
            .unwrap();
        assert_eq!(r.data.unwrap()["ok"], true);
    }

    #[tokio::test]
    async fn validate_catches_missing_frontmatter() {
        let (_tmp, deps) = fixture();
        SkillCreateDraftTool::new(deps.clone())
            .execute(json!({"name": "tmp-skill", "description": "y"}), ctx())
            .await
            .unwrap();
        SkillWriteMdTool::new(deps.clone())
            .execute(
                json!({"draft_id": deps.conversation_id, "content": "no frontmatter"}),
                ctx(),
            )
            .await
            .unwrap();
        let r = SkillValidateTool::new(deps.clone())
            .execute(json!({"draft_id": deps.conversation_id}), ctx())
            .await
            .unwrap();
        let data = r.data.unwrap();
        assert_eq!(data["ok"], false);
        let codes: Vec<String> = data["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["code"].as_str().unwrap().to_string())
            .collect();
        assert!(codes.iter().any(|c| c == "frontmatter.missing"));
    }

    #[tokio::test]
    async fn validate_catches_empty_body_and_bad_name() {
        let (_tmp, deps) = fixture();
        SkillCreateDraftTool::new(deps.clone())
            .execute(json!({"name": "tmp-skill", "description": "y"}), ctx())
            .await
            .unwrap();
        let body = "---\nname: BadName--\ndescription: ok\n---\n";
        SkillWriteMdTool::new(deps.clone())
            .execute(
                json!({"draft_id": deps.conversation_id, "content": body}),
                ctx(),
            )
            .await
            .unwrap();
        let r = SkillValidateTool::new(deps.clone())
            .execute(json!({"draft_id": deps.conversation_id}), ctx())
            .await
            .unwrap();
        let data = r.data.unwrap();
        assert_eq!(data["ok"], false);
        let codes: Vec<String> = data["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["code"].as_str().unwrap().to_string())
            .collect();
        assert!(codes.iter().any(|c| c == "frontmatter.bad_name"));
        assert!(codes.iter().any(|c| c == "body.empty"));
    }

    #[tokio::test]
    async fn validate_catches_unknown_tool_in_allowed_tools() {
        let (_tmp, deps) = fixture();
        SkillCreateDraftTool::new(deps.clone())
            .execute(json!({"name": "tmp-skill", "description": "y"}), ctx())
            .await
            .unwrap();
        let body = "---\nname: my-skill\ndescription: ok\nallowed_tools:\n  - workspace\n  - non_existent_tool\n---\nbody";
        SkillWriteMdTool::new(deps.clone())
            .execute(
                json!({"draft_id": deps.conversation_id, "content": body}),
                ctx(),
            )
            .await
            .unwrap();
        let r = SkillValidateTool::new(deps.clone())
            .execute(json!({"draft_id": deps.conversation_id}), ctx())
            .await
            .unwrap();
        let data = r.data.unwrap();
        assert_eq!(data["ok"], false);
        let codes: Vec<String> = data["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["code"].as_str().unwrap().to_string())
            .collect();
        assert!(codes.iter().any(|c| c == "frontmatter.unknown_tool"));
    }

    #[tokio::test]
    async fn validate_catches_missing_referenced_script() {
        let (_tmp, deps) = fixture();
        SkillCreateDraftTool::new(deps.clone())
            .execute(json!({"name": "tmp-skill", "description": "y"}), ctx())
            .await
            .unwrap();
        let body = "---\nname: my-skill\ndescription: ok\n---\nrun scripts/missing.py to do magic";
        SkillWriteMdTool::new(deps.clone())
            .execute(
                json!({"draft_id": deps.conversation_id, "content": body}),
                ctx(),
            )
            .await
            .unwrap();
        let r = SkillValidateTool::new(deps.clone())
            .execute(json!({"draft_id": deps.conversation_id}), ctx())
            .await
            .unwrap();
        let data = r.data.unwrap();
        assert_eq!(data["ok"], false);
        let codes: Vec<String> = data["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["code"].as_str().unwrap().to_string())
            .collect();
        assert!(codes.iter().any(|c| c == "ref.missing_file"));
    }

    #[tokio::test]
    async fn add_file_then_validate_passes() {
        let (_tmp, deps) = fixture();
        SkillCreateDraftTool::new(deps.clone())
            .execute(json!({"name": "tmp-skill", "description": "y"}), ctx())
            .await
            .unwrap();
        SkillAddFileTool::new(deps.clone())
            .execute(
                json!({"draft_id": deps.conversation_id, "path": "scripts/foo.py", "content": "print(1)"}),
                ctx(),
            )
            .await
            .unwrap();
        let body = "---\nname: my-skill\ndescription: ok\n---\nuses scripts/foo.py";
        SkillWriteMdTool::new(deps.clone())
            .execute(
                json!({"draft_id": deps.conversation_id, "content": body}),
                ctx(),
            )
            .await
            .unwrap();
        let r = SkillValidateTool::new(deps.clone())
            .execute(json!({"draft_id": deps.conversation_id}), ctx())
            .await
            .unwrap();
        assert_eq!(r.data.unwrap()["ok"], true);
    }

    #[tokio::test]
    async fn install_creates_user_skill_dir() {
        let (tmp, deps) = fixture();
        SkillCreateDraftTool::new(deps.clone())
            .execute(json!({"name": "tmp-skill", "description": "y"}), ctx())
            .await
            .unwrap();
        let body = "---\nname: hello-world\ndescription: greet\n---\nsay hi";
        SkillWriteMdTool::new(deps.clone())
            .execute(
                json!({"draft_id": deps.conversation_id, "content": body}),
                ctx(),
            )
            .await
            .unwrap();
        let r = SkillInstallTool::new(deps.clone())
            .execute(json!({"draft_id": deps.conversation_id}), ctx())
            .await
            .unwrap();
        let data = r.data.unwrap();
        assert_eq!(data["status"], "installed");
        assert_eq!(data["name"], "hello-world");
        let installed_to = data["installed_to"].as_str().unwrap();
        assert!(tmp.path().join("users").join("t_1__u_1").join("skills").join("hello-world").exists());
        assert!(PathBuf::from(installed_to).join("SKILL.md").exists());
    }

    #[tokio::test]
    async fn install_conflict_returned_when_target_exists() {
        let (_tmp, deps) = fixture();
        SkillCreateDraftTool::new(deps.clone())
            .execute(json!({"name": "tmp-skill", "description": "y"}), ctx())
            .await
            .unwrap();
        let body = "---\nname: dup-skill\ndescription: ok\n---\nbody";
        SkillWriteMdTool::new(deps.clone())
            .execute(
                json!({"draft_id": deps.conversation_id, "content": body}),
                ctx(),
            )
            .await
            .unwrap();
        SkillInstallTool::new(deps.clone())
            .execute(json!({"draft_id": deps.conversation_id}), ctx())
            .await
            .unwrap();
        // re-install without force → conflict
        let r2 = SkillInstallTool::new(deps.clone())
            .execute(json!({"draft_id": deps.conversation_id}), ctx())
            .await
            .unwrap();
        assert_eq!(r2.data.unwrap()["status"], "conflict");
        // force=true should overwrite
        let r3 = SkillInstallTool::new(deps.clone())
            .execute(
                json!({"draft_id": deps.conversation_id, "force": true}),
                ctx(),
            )
            .await
            .unwrap();
        assert_eq!(r3.data.unwrap()["status"], "installed");
    }

    #[test]
    fn split_frontmatter_basic() {
        let (fm, body) = split_frontmatter("---\nname: x\n---\nbody").unwrap();
        assert_eq!(fm.trim(), "name: x");
        assert_eq!(body, "body");
    }

    #[test]
    fn split_frontmatter_with_bom_and_crlf() {
        let s = "\u{feff}---\r\nname: x\r\n---\r\nbody";
        let (fm, body) = split_frontmatter(s).unwrap();
        assert!(fm.contains("name: x"));
        assert_eq!(body, "body");
    }

    #[test]
    fn skill_name_validation() {
        assert!(validate_skill_name("my-skill").is_ok());
        assert!(validate_skill_name("a-b-c-1").is_ok());
        assert!(validate_skill_name("ab").is_err()); // too short
        assert!(validate_skill_name("My-Skill").is_err()); // uppercase
        assert!(validate_skill_name("my_skill").is_err()); // underscore
        assert!(validate_skill_name("-x").is_err()); // leading dash
        assert!(validate_skill_name("x-").is_err());
        assert!(validate_skill_name("x--y").is_err()); // double dash
    }

    #[test]
    fn find_referenced_paths_picks_basenames() {
        let body = "use scripts/foo.py and references/bar.md, also `scripts/baz.py`.";
        let scripts = find_referenced_paths(body, "scripts");
        let refs = find_referenced_paths(body, "references");
        assert_eq!(scripts, vec!["baz.py", "foo.py"]);
        assert_eq!(refs, vec!["bar.md"]);
    }

    #[test]
    fn catalog_entries_have_5_tools() {
        let entries = catalog_entries();
        assert_eq!(entries.len(), 5);
        let ids: Vec<String> = entries.iter().map(|(d, _)| d.id.clone()).collect();
        assert!(ids.contains(&"skill_create_draft".to_string()));
        assert!(ids.contains(&"skill_write_md".to_string()));
        assert!(ids.contains(&"skill_add_file".to_string()));
        assert!(ids.contains(&"skill_validate".to_string()));
        assert!(ids.contains(&"skill_install".to_string()));
    }
}
