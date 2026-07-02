//! System prompt library — externalized to .md files with runtime loading.
//!
//! Prompts are loaded from external .md files with a priority chain:
//! 1. User override: `{data_root}/prompts/{name}.md`
//! 2. Bundled default: `{resource_dir}/prompts/{name}.md`
//! 3. Legacy fallback: `base.md` is accepted only when loading `system`
//! 4. Source-bundled fallback for `system.md`
//! 5. Hardcoded fallback for `system`
//!
//! The public API (`get_system_prompt`) remains unchanged. This module is a raw
//! prompt store plus compatibility shim; new production assembly lives under
//! `runtime::chat::prompt::PromptAssembler`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::Mutex;
use std::sync::{LazyLock, RwLock};

use crate::runtime::chat::prompt::{PromptAssembler, PromptBuildContext, PromptCachePolicy};

#[cfg(test)]
pub(crate) static PROMPT_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Minimal hardcoded fallback for `system` — used only when all file fallbacks
/// are missing.
const SYSTEM_FALLBACK: &str = "你是 AI小家 — 智能工作助手。";

/// All recognized prompt names.
const PROMPT_NAMES: &[&str] = &["system"];

fn bundled_prompt_fallback(name: &str) -> Option<&'static str> {
    match name {
        "system" => Some(include_str!("../../prompts/system.md")),
        _ => None,
    }
}

/// System prompt 的分层结构。
///
/// `static_section`：可跨会话复用的稳定内容（system.md）。
/// `dynamic_section`：会话级动态内容（persona 等运行时注入）。
#[derive(Debug, Clone)]
pub struct SystemPromptParts {
    /// 稳定前缀（system.md 品牌替换后）
    pub static_section: String,
    /// 动态后缀（persona 段等运行时内容）
    pub dynamic_section: String,
}

/// Raw prompt fragments captured under one PromptStore read lock.
#[derive(Debug, Clone)]
pub struct PromptFragmentSnapshot {
    pub system: String,
}

/// Source from which a prompt was loaded (for logging).
#[derive(Debug, Clone, Copy)]
enum PromptSource {
    Override,
    Bundled,
    Fallback,
}

impl std::fmt::Display for PromptSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PromptSource::Override => write!(f, "override"),
            PromptSource::Bundled => write!(f, "bundled"),
            PromptSource::Fallback => write!(f, "fallback"),
        }
    }
}

/// PromptStore 是纯文本片段仓库。
///
/// 它只负责按名字加载/缓存 system.md 等原始 prompt
/// 片段，不承担 system prompt 组装逻辑。新增静态 prompt 入口时修改这里；
/// 组装策略统一放在 `build_system_prompt_parts`。
struct PromptStore {
    prompts: HashMap<String, String>,
    #[allow(dead_code)]
    bundled_dir: PathBuf,
    #[allow(dead_code)]
    override_dir: PathBuf,
}

impl PromptStore {
    fn new(resource_dir: &Path, data_root: &Path) -> Self {
        let bundled_dir = resource_dir.join("prompts");
        let override_dir = data_root.join("prompts");

        let mut prompts = HashMap::new();

        for &name in PROMPT_NAMES {
            let (content, source) = Self::load_one(name, &override_dir, &bundled_dir);
            log::info!(
                "Loaded prompt '{}': {} chars (source: {})",
                name,
                content.len(),
                source,
            );
            prompts.insert(name.to_string(), content);
        }

        Self {
            prompts,
            bundled_dir,
            override_dir,
        }
    }

    /// Load a single prompt file with priority chain.
    fn load_one(name: &str, override_dir: &Path, bundled_dir: &Path) -> (String, PromptSource) {
        // 1. User override
        let override_path = override_dir.join(format!("{}.md", name));
        if let Some(content) = Self::read_non_empty(&override_path) {
            return (content, PromptSource::Override);
        }

        // 2. Bundled default
        let bundled_path = bundled_dir.join(format!("{}.md", name));
        if let Some(content) = Self::read_non_empty(&bundled_path) {
            return (content, PromptSource::Bundled);
        }

        // 3. Legacy compatibility: old installs may still override base.md.
        if name == "system" {
            let legacy_override = override_dir.join("base.md");
            if let Some(content) = Self::read_non_empty(&legacy_override) {
                return (content, PromptSource::Override);
            }

            let legacy_bundled = bundled_dir.join("base.md");
            if let Some(content) = Self::read_non_empty(&legacy_bundled) {
                return (content, PromptSource::Bundled);
            }
        }

        // 4. Bundled source fallback for prompt fragments that live in markdown.
        if let Some(content) = bundled_prompt_fallback(name) {
            return (content.to_string(), PromptSource::Fallback);
        }

        // 5. Hardcoded fallback (system only)
        if name == "system" {
            return (SYSTEM_FALLBACK.to_string(), PromptSource::Fallback);
        }

        // Other prompts: empty string (prompt will just have BASE)
        (String::new(), PromptSource::Fallback)
    }

    /// Read a file if it exists and is non-empty.
    fn read_non_empty(path: &Path) -> Option<String> {
        match std::fs::read_to_string(path) {
            Ok(content) if !content.trim().is_empty() => Some(content),
            _ => None,
        }
    }

    fn get(&self, name: &str) -> &str {
        let normalized = if name == "base" { "system" } else { name };
        self.prompts
            .get(normalized)
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// Reload all prompts from disk.
    fn reload(&mut self) {
        for &name in PROMPT_NAMES {
            let (content, source) = Self::load_one(name, &self.override_dir, &self.bundled_dir);
            log::info!(
                "Reloaded prompt '{}': {} chars (source: {})",
                name,
                content.len(),
                source,
            );
            self.prompts.insert(name.to_string(), content);
        }
    }
}

static PROMPT_STORE: LazyLock<RwLock<PromptStore>> = LazyLock::new(|| {
    // Auto-detect prompts from the source tree (for tests and dev without init_prompts).
    // In production, init_prompts() is called explicitly and overwrites this.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_default();
    let empty = PathBuf::new();
    if manifest_dir.join("prompts").is_dir() {
        RwLock::new(PromptStore::new(&manifest_dir, &empty))
    } else {
        RwLock::new(PromptStore::new(&empty, &empty))
    }
});

/// Initialize the prompt store. Must be called once at app startup.
pub fn init_prompts(resource_dir: &Path, data_root: &Path) {
    let store = PromptStore::new(resource_dir, data_root);
    let mut guard = PROMPT_STORE
        .write()
        .expect("PromptStore write lock poisoned");
    *guard = store;
}

/// Reload all prompts from disk (for future hot-reload from settings UI).
#[allow(dead_code)]
pub fn reload_prompts() {
    let mut guard = PROMPT_STORE
        .write()
        .expect("PromptStore write lock poisoned");
    guard.reload();
}

/// Get the system prompt content (legacy name kept for plugin composition).
pub fn get_base_prompt() -> String {
    let guard = PROMPT_STORE.read().expect("PromptStore read lock poisoned");
    guard.get("system").to_string()
}

/// Get any raw prompt fragment by name.
pub fn get_prompt_fragment(name: &str) -> String {
    let guard = PROMPT_STORE.read().expect("PromptStore read lock poisoned");
    guard.get(name).to_string()
}

/// Get all raw fragments needed for system prompt assembly in a single snapshot.
pub fn get_prompt_fragment_snapshot() -> PromptFragmentSnapshot {
    let guard = PROMPT_STORE.read().expect("PromptStore read lock poisoned");
    PromptFragmentSnapshot {
        system: guard.get("system").to_string(),
    }
}

/// Compatibility shim for old callers.
/// New production code must use `runtime::chat::prompt::PromptAssembler`.
///
/// 构建分层 system prompt（section 化版本）。
///
/// - `static_section` = system.md（品牌替换后）
/// - `dynamic_section` = persona 段等运行时动态内容
///
/// **注意：** 不再注入当前日期——日期改为首条 user message `<system-reminder>` 注入。
/// build_system_prompt_parts 是 system prompt 的唯一组装入口；
/// 其他调用方若需要完整字符串，应通过 `get_system_prompt` 这个兼容 shim
/// 间接调用，而不是自行拼接 prompt 片段。
pub fn build_system_prompt_parts(
    persona: Option<&crate::storage::file_store::persona::Persona>,
    product_name: Option<&str>,
) -> SystemPromptParts {
    let assembly = PromptAssembler::default().build_system_prompt(PromptBuildContext {
        persona,
        product_name,
    });

    let mut static_parts = Vec::new();
    let mut dynamic_parts = Vec::new();

    for block in assembly.blocks() {
        if block.text.trim().is_empty() {
            continue;
        }

        match block.cache_policy {
            PromptCachePolicy::StaticPrefix => static_parts.push(block.text.clone()),
            PromptCachePolicy::SessionDynamic | PromptCachePolicy::Volatile => {
                dynamic_parts.push(block.text.clone())
            }
        }
    }

    SystemPromptParts {
        static_section: static_parts.join("\n\n"),
        dynamic_section: dynamic_parts.join("\n\n"),
    }
}

/// Compatibility shim for old callers.
/// New production code must use `runtime::chat::prompt::PromptAssembler`.
///
/// Compose the full system prompt (backward-compatible shim).
///
/// 调用 `build_system_prompt_parts` 后拼接 static + dynamic。
/// **不再注入当前日期**——日期改为 `run_chat_turn_s4` 的首条 user message 注入。
///
/// `step` 参数保留仅为兼容旧调用点；现在所有调用都走统一 system prompt。
pub fn get_system_prompt(
    _step: Option<u32>,
    persona: Option<&crate::storage::file_store::persona::Persona>,
    product_name: Option<&str>,
) -> String {
    let parts = build_system_prompt_parts(persona, product_name);
    if parts.dynamic_section.is_empty() {
        parts.static_section
    } else {
        format!("{}\n\n{}", parts.static_section, parts.dynamic_section)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper: create a temp prompt directory with given files.
    fn setup_prompts(dir: &Path, files: &[(&str, &str)]) {
        let prompts_dir = dir.join("prompts");
        fs::create_dir_all(&prompts_dir).unwrap();
        for (name, content) in files {
            fs::write(prompts_dir.join(format!("{}.md", name)), content).unwrap();
        }
    }

    #[test]
    fn test_bundled_loading() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        fs::create_dir_all(&bundled).unwrap();
        fs::create_dir_all(&user).unwrap();

        setup_prompts(&bundled, &[("system", "Test system prompt")]);

        init_prompts(&bundled, &user);

        let prompt = get_system_prompt(None, None, None);
        assert!(prompt.contains("Test system prompt"));
    }

    #[test]
    fn test_user_override_priority() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");

        setup_prompts(&bundled, &[("system", "Bundled system")]);
        setup_prompts(&user, &[("system", "Custom system")]);

        init_prompts(&bundled, &user);

        let prompt = get_system_prompt(None, None, None);
        assert!(
            prompt.contains("Custom system"),
            "User override should take priority"
        );
        assert!(
            !prompt.contains("Bundled system"),
            "Overridden system prompt should not include bundled system prompt"
        );
    }

    #[test]
    fn test_empty_file_falls_through() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");

        setup_prompts(&bundled, &[("system", "Bundled system")]);
        // Empty override file should be ignored
        setup_prompts(&user, &[("system", "   ")]);

        init_prompts(&bundled, &user);

        let prompt = get_system_prompt(None, None, None);
        assert!(
            prompt.contains("Bundled system"),
            "Empty override should fall through to bundled"
        );
    }

    #[test]
    fn test_fallback_base() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let empty_bundled = tmp.path().join("empty_bundled");
        let empty_user = tmp.path().join("empty_user");
        fs::create_dir_all(&empty_bundled).unwrap();
        fs::create_dir_all(&empty_user).unwrap();

        init_prompts(&empty_bundled, &empty_user);

        let prompt = get_system_prompt(None, None, None);
        assert!(
            prompt.contains("AI小家"),
            "Should fall back to hardcoded system prompt"
        );
    }

    #[test]
    fn test_api_unchanged() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");

        setup_prompts(&bundled, &[("system", "AI小家 system")]);
        fs::create_dir_all(&user).unwrap();

        init_prompts(&bundled, &user);

        // Unified system prompt works
        assert!(get_system_prompt(None, None, None).contains("AI小家 system"));

        // Step variants now reuse the same unified system prompt.
        let step0 = get_system_prompt(Some(0), None, None);
        assert!(step0.contains("AI小家 system"));

        // Invalid step also returns the same unified prompt
        let step99 = get_system_prompt(Some(99), None, None);
        assert!(step99.contains("AI小家 system"));

        // System always included
        for step in [None, Some(0), Some(1), Some(5)] {
            assert!(
                get_system_prompt(step, None, None).contains("AI小家 system"),
                "Step {:?} should include system prompt",
                step,
            );
        }
    }

    #[test]
    fn test_reload() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");

        setup_prompts(&bundled, &[("system", "Original system")]);
        fs::create_dir_all(&user).unwrap();

        // Test reload on a standalone PromptStore instance to avoid global state races
        let mut store = PromptStore::new(&bundled, &user);
        assert!(store.get("system").contains("Original system"));

        // Write user override
        setup_prompts(&user, &[("system", "Updated system")]);

        store.reload();
        assert!(store.get("system").contains("Updated system"));
    }

    #[test]
    fn test_build_system_prompt_parts_has_single_static_system() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        setup_prompts(
            &bundled,
            &[(
                "system",
                "AI小家 system\n\n<tool_use_protocol>\n优先使用专用能力\n</tool_use_protocol>",
            )],
        );
        fs::create_dir_all(&user).unwrap();
        init_prompts(&bundled, &user);

        let parts = build_system_prompt_parts(None, None);
        assert!(
            parts.static_section.contains("AI小家 system"),
            "static_section must contain system prompt"
        );
        assert!(
            parts.static_section.contains("优先使用专用能力"),
            "static_section must contain tool guidance"
        );
        assert!(
            !parts.static_section.contains("今天是"),
            "static_section must NOT contain date"
        );
        assert!(
            parts.dynamic_section.is_empty(),
            "dynamic_section must be empty without persona or runtime dynamic content"
        );
    }

    #[test]
    fn test_build_system_prompt_parts_product_name_replacement() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        setup_prompts(&bundled, &[("system", "你是 AI小家")]);
        fs::create_dir_all(&user).unwrap();
        init_prompts(&bundled, &user);

        let parts = build_system_prompt_parts(None, Some("智能办公"));
        assert!(
            parts.static_section.contains("智能办公"),
            "product_name replacement must work in static_section"
        );
        assert!(
            !parts.static_section.contains("AI小家"),
            "original brand name must be replaced"
        );
    }

    #[test]
    fn test_build_system_prompt_parts_persona_in_dynamic() {
        use crate::storage::file_store::persona::Persona;
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        setup_prompts(&bundled, &[("system", "AI小家")]);
        fs::create_dir_all(&user).unwrap();
        init_prompts(&bundled, &user);

        let persona = Persona {
            id: "test".to_string(),
            version: 1,
            builtin: false,
            name: "Test".to_string(),
            icon: "🧪".to_string(),
            description: "test".to_string(),
            name_en: "".to_string(),
            description_en: "".to_string(),
            identity: "你是专业 HR 顾问".to_string(),
            expertise: vec!["薪酬分析".to_string()],
            memory_hints: vec![],
            linked_categories: vec![],
            created_at: "2026-01-01".to_string(),
            updated_at: "2026-01-01".to_string(),
        };

        let parts = build_system_prompt_parts(Some(&persona), None);
        assert!(
            parts.dynamic_section.contains("你是专业 HR 顾问"),
            "persona identity must appear in dynamic_section"
        );
        assert!(
            parts.dynamic_section.contains("薪酬分析"),
            "persona expertise must appear in dynamic_section"
        );
        assert!(
            !parts.static_section.contains("你是专业 HR 顾问"),
            "persona must NOT be in static_section"
        );
    }

    #[test]
    fn test_get_system_prompt_shim_backward_compatible() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        setup_prompts(&bundled, &[("system", "AI小家 system")]);
        fs::create_dir_all(&user).unwrap();
        init_prompts(&bundled, &user);

        let prompt = get_system_prompt(None, None, None);
        assert!(
            prompt.contains("AI小家 system"),
            "shim: system must be present"
        );
        let prompt_step = get_system_prompt(Some(0), None, None);
        assert!(
            prompt_step.contains("AI小家 system"),
            "shim: step=Some must reuse unified system prompt"
        );
        assert!(
            !prompt.contains("今天是"),
            "shim: date must NOT be in system prompt"
        );
    }

    #[test]
    fn test_system_prompt_contains_tool_and_delivery_guidance() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        setup_prompts(
            &bundled,
            &[("system", include_str!("../../prompts/system.md"))],
        );
        fs::create_dir_all(&user).unwrap();
        init_prompts(&bundled, &user);

        let parts = build_system_prompt_parts(None, None);
        assert!(
            parts.static_section.contains("优先使用专用能力"),
            "must mention dedicated capabilities in context"
        );
        assert!(
            parts.static_section.contains("真实搜索"),
            "must mention real search in context"
        );
        assert!(
            parts.static_section.contains("长期记忆能力"),
            "must mention memory capability limits in context"
        );
        assert!(
            parts.static_section.contains("<file_creation_protocol>"),
            "must include deliverable discipline guidance"
        );
        assert!(
            parts.static_section.contains("必须创建或更新对应文件"),
            "must require file creation when the user asks for deliverables"
        );
        assert!(
            parts
                .static_section
                .contains("验证最终产物存在、非空、路径正确"),
            "must require final deliverable verification"
        );
        assert!(
            parts.static_section.contains("<sharing_files>"),
            "must include markdown link guidance for referenced files and URLs"
        );
        assert!(
            parts.static_section.contains("[名称](路径或URL)"),
            "must prefer Markdown links when referencing local files, URLs, or source documents"
        );
    }

    #[test]
    fn test_tool_call_communication_guidance_is_not_duplicated() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        setup_prompts(
            &bundled,
            &[("system", include_str!("../../prompts/system.md"))],
        );
        fs::create_dir_all(&user).unwrap();
        init_prompts(&bundled, &user);

        let parts = build_system_prompt_parts(None, None);
        let must_announce = "调用工具前，只简短说明当前具体动作或新的观察。";
        let no_colon = "不要用冒号引出工具调用。";

        assert_eq!(
            parts.static_section.matches(must_announce).count(),
            1,
            "tool-call announcement guidance must appear once"
        );
        assert_eq!(
            parts.static_section.matches(no_colon).count(),
            1,
            "tool-call colon guidance must appear once"
        );
    }

    #[test]
    fn test_system_prompt_omits_retired_tool_names() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        setup_prompts(
            &bundled,
            &[("system", include_str!("../../prompts/system.md"))],
        );
        fs::create_dir_all(&user).unwrap();
        init_prompts(&bundled, &user);

        let parts = build_system_prompt_parts(None, None);
        for retired_tool in ["save_memory", "load_core_memory", "distill_memories"] {
            assert!(
                !parts.static_section.contains(retired_tool),
                "system prompt must not mention retired tool name {}",
                retired_tool
            );
        }
    }

    #[test]
    fn test_system_prompt_mentions_runtime_memory_tools_only() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        setup_prompts(
            &bundled,
            &[("system", include_str!("../../prompts/system.md"))],
        );
        fs::create_dir_all(&user).unwrap();
        init_prompts(&bundled, &user);

        let parts = build_system_prompt_parts(None, None);
        assert!(
            parts.static_section.contains("WriteMemory"),
            "static section must mention WriteMemory guidance"
        );
        assert!(
            parts.static_section.contains("SearchMemory"),
            "static section must mention SearchMemory guidance"
        );
        assert!(
            parts
                .static_section
                .contains("项目记忆不能替代本地文件、命令或工具权限"),
            "static section must keep memory distinct from permission grants"
        );
        assert!(
            !parts.static_section.contains("save_memory"),
            "static section must not mention retired legacy memory tool names"
        );
    }
}
