//! System prompt library — externalized to .md files with runtime loading.
//!
//! Prompts are loaded from external .md files with a priority chain:
//! 1. User override: `{data_root}/prompts/{name}.md`
//! 2. Bundled default: `{resource_dir}/prompts/{name}.md`
//! 3. Hardcoded fallback (base only)
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

/// Minimal hardcoded fallback for `base` — used only when both
/// override and bundled files are missing.
const BASE_FALLBACK: &str = "你是 AI小家 — 智能工作助手。";

/// All recognized prompt names.
const PROMPT_NAMES: &[&str] = &["base", "daily", "browser_agent"];

// 工具选择偏好章节——静态内容，写入 static_section，所有 mode 均包含。
const TOOL_PREFERENCE_SECTION: &str = r#"

【工具选择偏好】
- 优先使用专用能力：遇到文件读取、搜索、分析等任务时，优先使用当前可用的专门能力，不要用脚本能力硬凑流程
- 文件操作：描述文件内容时先基于实际读取结果；需要批量计算、转换或生成衍生结果时，再使用脚本或计算能力
- 搜索：涉及外部事实、法规、政策、行情、时事或产品信息时，必须先执行真实搜索，不要伪造搜索结果
- 记忆：不要假设系统一定提供长期记忆能力；若上下文里没有明确事实，就基于当前对话和已加载内容作答
- 分析：任何数据分析、统计或结论都必须来自实际执行结果，不得编造"#;

/// Memory mechanics section — 对标 claude-code-best 的记忆写回/召回指导。
const MEMORY_MECHANICS_SECTION: &str = r#"

【记忆管理】
你拥有跨对话持久化记忆能力，可通过 `write_memory` 与 `search_memory` 操作项目记忆。

何时调用 `write_memory`
- 用户明确要求“记住”“下次也这样”
- 用户纠正你的行为或确认一个长期适用的做法
- 识别到稳定的用户偏好、项目约束、外部系统指针

何时调用 `search_memory`
- 当前问题明显依赖历史偏好、既有项目约束、过去确认过的做法
- 回答前需要确认是否已有相关记忆，避免重复追问

不要保存
- 可从当前代码、文件、git 历史直接推导的信息
- 只在本次对话临时有效的状态

类型约定
- `user_preference`：用户偏好
- `project_constraint`：项目约束
- `reference_info`：外部系统指针
- `feedback`：AI 行为纠正或确认

`feedback` 建议写法：规则本体 + `**Why:**` + `**How to apply:**`

引用记忆前要防漂移
- 涉及文件路径先确认文件仍存在
- 涉及函数名、变量名先在代码里确认仍存在
- 用户说“忽略记忆”时，视作当前没有可用记忆，不引用也不比较旧记忆
"#;

/// System prompt 构建模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMode {
    /// 日常助手模式（base + 工具偏好 + daily.md + persona）
    Daily,
    /// 浏览器子代理模式（base + 工具偏好 + browser_agent.md）
    BrowserAgent,
}

/// System prompt 的分层结构。
///
/// `static_section`：可跨会话复用的稳定内容（base + 工具偏好）。
/// `dynamic_section`：会话级动态内容（persona + mode-specific prompt）。
#[derive(Debug, Clone)]
pub struct SystemPromptParts {
    /// 稳定前缀（base.md 品牌替换后 + 工具选择偏好章节）
    pub static_section: String,
    /// 动态后缀（persona 段 + daily.md / browser_agent.md，Analysis 时为空）
    pub dynamic_section: String,
}

/// Raw prompt fragments captured under one PromptStore read lock.
#[derive(Debug, Clone)]
pub struct PromptFragmentSnapshot {
    pub base: String,
    pub daily: String,
    pub browser_agent: String,
    pub tool_preference: String,
    pub memory_mechanics: String,
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
/// 它只负责按名字加载/缓存 base、daily、browser_agent 等原始 prompt
/// 片段，不承担 system prompt 组装逻辑。新增 prompt 片段时修改这里；
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

        // 3. Hardcoded fallback (base only)
        if name == "base" {
            return (BASE_FALLBACK.to_string(), PromptSource::Fallback);
        }

        // Other prompts: empty string (mode will just have BASE)
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
        self.prompts.get(name).map(|s| s.as_str()).unwrap_or("")
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
        RwLock::new(PromptStore {
            prompts: {
                let mut m = HashMap::new();
                m.insert("base".to_string(), BASE_FALLBACK.to_string());
                m
            },
            bundled_dir: PathBuf::new(),
            override_dir: PathBuf::new(),
        })
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

/// Get the base prompt content (for plugin composition).
pub fn get_base_prompt() -> String {
    let guard = PROMPT_STORE.read().expect("PromptStore read lock poisoned");
    guard.get("base").to_string()
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
        base: guard.get("base").to_string(),
        daily: guard.get("daily").to_string(),
        browser_agent: guard.get("browser_agent").to_string(),
        tool_preference: TOOL_PREFERENCE_SECTION.to_string(),
        memory_mechanics: MEMORY_MECHANICS_SECTION.to_string(),
    }
}

/// Get the static tool preference guidance section.
pub fn tool_preference_section() -> &'static str {
    TOOL_PREFERENCE_SECTION
}

/// Get the static memory mechanics guidance section.
pub fn memory_mechanics_section() -> &'static str {
    MEMORY_MECHANICS_SECTION
}

/// Get the raw browser agent prompt fragment.
pub fn get_browser_agent_prompt_fragment() -> String {
    get_prompt_fragment("browser_agent")
}

/// Get the raw browser agent prompt fragment (legacy accessor).
pub fn get_browser_agent_prompt() -> String {
    get_browser_agent_prompt_fragment()
}

/// Compatibility shim for old callers.
/// New production code must use `runtime::chat::prompt::PromptAssembler`.
///
/// 构建分层 system prompt（section 化版本）。
///
/// - `static_section` = base.md（品牌替换后）+ TOOL_PREFERENCE_SECTION
/// - `dynamic_section` = persona 段 + mode-specific prompt（Analysis 时无 daily.md）
///
/// **注意：** 不再注入当前日期——日期改为首条 user message `<system-reminder>` 注入。
/// build_system_prompt_parts 是 system prompt 的唯一组装入口；
/// 其他调用方若需要完整字符串，应通过 `get_system_prompt` 这个兼容 shim
/// 间接调用，而不是自行拼接 base / daily / browser_agent 片段。
pub fn build_system_prompt_parts(
    mode: PromptMode,
    persona: Option<&crate::storage::file_store::persona::Persona>,
    product_name: Option<&str>,
) -> SystemPromptParts {
    let assembly = PromptAssembler::default().build_system_prompt(PromptBuildContext {
        mode,
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
/// `step` 参数保留仅为兼容旧调用点；现在所有调用都走统一 daily prompt。
pub fn get_system_prompt(
    _step: Option<u32>,
    persona: Option<&crate::storage::file_store::persona::Persona>,
    product_name: Option<&str>,
) -> String {
    let parts = build_system_prompt_parts(PromptMode::Daily, persona, product_name);
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

        setup_prompts(
            &bundled,
            &[("base", "Test base prompt"), ("daily", "Test daily prompt")],
        );

        init_prompts(&bundled, &user);

        let prompt = get_system_prompt(None, None, None);
        assert!(prompt.contains("Test base prompt"));
        assert!(prompt.contains("Test daily prompt"));
    }

    #[test]
    fn test_user_override_priority() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");

        setup_prompts(
            &bundled,
            &[("base", "Bundled base"), ("daily", "Bundled daily")],
        );
        setup_prompts(&user, &[("base", "Custom base")]);

        init_prompts(&bundled, &user);

        let prompt = get_system_prompt(None, None, None);
        assert!(
            prompt.contains("Custom base"),
            "User override should take priority"
        );
        assert!(
            prompt.contains("Bundled daily"),
            "Non-overridden should use bundled"
        );
    }

    #[test]
    fn test_empty_file_falls_through() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");

        setup_prompts(&bundled, &[("base", "Bundled base")]);
        // Empty override file should be ignored
        setup_prompts(&user, &[("base", "   ")]);

        init_prompts(&bundled, &user);

        let prompt = get_system_prompt(None, None, None);
        assert!(
            prompt.contains("Bundled base"),
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
            "Should fall back to hardcoded base"
        );
    }

    #[test]
    fn test_api_unchanged() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");

        setup_prompts(
            &bundled,
            &[("base", "AI小家 base"), ("daily", "日常工作助手")],
        );
        fs::create_dir_all(&user).unwrap();

        init_prompts(&bundled, &user);

        // Daily mode works
        assert!(get_system_prompt(None, None, None).contains("日常工作助手"));

        // Step variants now reuse the same unified daily prompt.
        let step0 = get_system_prompt(Some(0), None, None);
        assert!(step0.contains("AI小家 base"));
        assert!(step0.contains("日常工作助手"));

        // Invalid step also returns the same unified prompt
        let step99 = get_system_prompt(Some(99), None, None);
        assert!(step99.contains("AI小家 base"));
        assert!(step99.contains("日常工作助手"));

        // Base always included
        for step in [None, Some(0), Some(1), Some(5)] {
            assert!(
                get_system_prompt(step, None, None).contains("AI小家 base"),
                "Step {:?} should include base prompt",
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

        setup_prompts(&bundled, &[("base", "Original base")]);
        fs::create_dir_all(&user).unwrap();

        // Test reload on a standalone PromptStore instance to avoid global state races
        let mut store = PromptStore::new(&bundled, &user);
        assert!(store.get("base").contains("Original base"));

        // Write user override
        setup_prompts(&user, &[("base", "Updated base")]);

        store.reload();
        assert!(store.get("base").contains("Updated base"));
    }

    #[test]
    fn test_build_system_prompt_parts_daily_has_static_and_dynamic() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        setup_prompts(
            &bundled,
            &[("base", "AI小家 base"), ("daily", "日常工作助手")],
        );
        fs::create_dir_all(&user).unwrap();
        init_prompts(&bundled, &user);

        let parts = build_system_prompt_parts(PromptMode::Daily, None, None);
        assert!(
            parts.static_section.contains("AI小家 base"),
            "static_section must contain base prompt"
        );
        assert!(
            parts.static_section.contains("工具选择偏好"),
            "static_section must contain tool preference section"
        );
        assert!(
            !parts.static_section.contains("今天是"),
            "static_section must NOT contain date"
        );
        assert!(
            parts.dynamic_section.contains("日常工作助手"),
            "dynamic_section must contain daily prompt"
        );
        assert!(
            !parts.dynamic_section.contains("AI小家 base"),
            "dynamic_section must NOT repeat base prompt"
        );
    }

    #[test]
    fn test_build_system_prompt_parts_browser_agent_no_daily() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        setup_prompts(
            &bundled,
            &[
                ("base", "AI小家 base"),
                ("daily", "日常工作助手"),
                ("browser_agent", "浏览器代理"),
            ],
        );
        fs::create_dir_all(&user).unwrap();
        init_prompts(&bundled, &user);

        let parts = build_system_prompt_parts(PromptMode::BrowserAgent, None, None);
        assert!(
            !parts.dynamic_section.contains("日常工作助手"),
            "BrowserAgent dynamic_section must NOT contain daily prompt"
        );
        assert!(
            parts.dynamic_section.contains("浏览器代理"),
            "BrowserAgent dynamic_section must contain browser prompt"
        );
    }

    #[test]
    fn test_build_system_prompt_parts_product_name_replacement() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        setup_prompts(&bundled, &[("base", "你是 AI小家"), ("daily", "")]);
        fs::create_dir_all(&user).unwrap();
        init_prompts(&bundled, &user);

        let parts = build_system_prompt_parts(PromptMode::Daily, None, Some("智能办公"));
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
        setup_prompts(&bundled, &[("base", "AI小家"), ("daily", "日常")]);
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

        let parts = build_system_prompt_parts(PromptMode::Daily, Some(&persona), None);
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
        setup_prompts(
            &bundled,
            &[("base", "AI小家 base"), ("daily", "日常工作助手")],
        );
        fs::create_dir_all(&user).unwrap();
        init_prompts(&bundled, &user);

        let prompt = get_system_prompt(None, None, None);
        assert!(prompt.contains("AI小家 base"), "shim: base must be present");
        assert!(
            prompt.contains("日常工作助手"),
            "shim: daily must be present for step=None"
        );
        let prompt_step = get_system_prompt(Some(0), None, None);
        assert!(
            prompt_step.contains("AI小家 base"),
            "shim: base must be present for step"
        );
        assert!(
            prompt_step.contains("日常工作助手"),
            "shim: step=Some must reuse unified daily prompt"
        );
        assert!(
            !prompt.contains("今天是"),
            "shim: date must NOT be in system prompt"
        );
    }

    #[test]
    fn test_tool_preference_section_content() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        setup_prompts(&bundled, &[("base", "AI小家"), ("daily", "")]);
        fs::create_dir_all(&user).unwrap();
        init_prompts(&bundled, &user);

        let parts = build_system_prompt_parts(PromptMode::Daily, None, None);
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
    }

    #[test]
    fn test_tool_preference_section_omits_retired_tool_names() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        setup_prompts(&bundled, &[("base", "AI小家"), ("daily", "")]);
        fs::create_dir_all(&user).unwrap();
        init_prompts(&bundled, &user);

        let parts = build_system_prompt_parts(PromptMode::Daily, None, None);
        for retired_tool in [
            "save_memory",
            "load_core_memory",
            "distill_memories",
            "plan_update",
            "progress_update",
            "save_analysis_note",
            "hypothesis_test",
            "detect_anomalies",
            "slides_gen",
            "export_data",
        ] {
            assert!(
                !parts.static_section.contains(retired_tool),
                "system prompt must not mention retired tool name {}",
                retired_tool
            );
        }
    }

    #[test]
    fn test_memory_mechanics_section_mentions_runtime_memory_tools_only() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        setup_prompts(&bundled, &[("base", "AI小家"), ("daily", "")]);
        fs::create_dir_all(&user).unwrap();
        init_prompts(&bundled, &user);

        let parts = build_system_prompt_parts(PromptMode::Daily, None, None);
        assert!(
            parts.static_section.contains("write_memory"),
            "static section must mention write_memory guidance"
        );
        assert!(
            parts.static_section.contains("search_memory"),
            "static section must mention search_memory guidance"
        );
        assert!(
            !parts.static_section.contains("save_memory"),
            "static section must not mention retired legacy memory tool names"
        );
    }
}
