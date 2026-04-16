//! System prompt library — externalized to .md files with runtime loading.
//!
//! Prompts are loaded from external .md files with a priority chain:
//! 1. User override: `{app_data_dir}/prompts/{name}.md`
//! 2. Bundled default: `{resource_dir}/prompts/{name}.md`
//! 3. Hardcoded fallback (base only)
//!
//! The public API (`get_system_prompt`) remains unchanged.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, RwLock};

/// Minimal hardcoded fallback for `base` — used only when both
/// override and bundled files are missing.
const BASE_FALLBACK: &str = "你是 AI小家 — 智能工作助手。";

/// All recognized prompt names.
const PROMPT_NAMES: &[&str] = &["base", "daily", "browser_agent"];

// 工具选择偏好章节——静态内容，写入 static_section
const TOOL_PREFERENCE_SECTION: &str = r#"

【工具选择偏好】
- 优先使用专用工具：有专用工具时，不要改用 execute_python 模拟
- 文件操作：读文件用 load_file/read_workspace_file，不要用 execute_python 读文件
- 搜索：信息查询用 web_search，不要伪造搜索结果
- 内存：需要记忆时用 save_memory/search_memory，不要依赖对话历史
- 分析：数据分析用 execute_python，结果必须来自实际执行"#;

/// System prompt 构建模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMode {
    /// 日常助手模式（base + 工具偏好 + daily.md + persona）
    Daily,
    /// 分析步骤模式（base + 工具偏好，无 daily.md，无 persona）
    Analysis,
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

struct PromptStore {
    prompts: HashMap<String, String>,
    #[allow(dead_code)]
    bundled_dir: PathBuf,
    #[allow(dead_code)]
    override_dir: PathBuf,
}

impl PromptStore {
    fn new(resource_dir: &Path, app_data_dir: &Path) -> Self {
        let bundled_dir = resource_dir.join("prompts");
        let override_dir = app_data_dir.join("prompts");

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
pub fn init_prompts(resource_dir: &Path, app_data_dir: &Path) {
    let store = PromptStore::new(resource_dir, app_data_dir);
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

/// Get the browser agent prompt (for SubAgent).
pub fn get_browser_agent_prompt() -> String {
    let guard = PROMPT_STORE.read().expect("PromptStore read lock poisoned");
    guard.get("browser_agent").to_string()
}

/// 构建分层 system prompt（section 化版本）。
///
/// - `static_section` = base.md（品牌替换后）+ TOOL_PREFERENCE_SECTION
/// - `dynamic_section` = persona 段 + mode-specific prompt（Analysis 时无 daily.md）
///
/// **注意：** 不再注入当前日期——日期改为首条 user message `<system-reminder>` 注入。
pub fn build_system_prompt_parts(
    mode: PromptMode,
    persona: Option<&crate::storage::file_store::persona::Persona>,
    product_name: Option<&str>,
) -> SystemPromptParts {
    let guard = PROMPT_STORE.read().expect("PromptStore read lock poisoned");

    // ── static_section: base.md + 品牌替换 + 工具偏好 ───────────────
    let base_raw = guard.get("base");
    let base = match product_name {
        Some(name) if !name.is_empty() && name != "AI小家" => base_raw.replace("AI小家", name),
        _ => base_raw.to_string(),
    };
    let static_section = format!("{}{}", base, TOOL_PREFERENCE_SECTION);

    // ── dynamic_section: persona + mode prompt ───────────────────────
    let mut dynamic_parts: Vec<String> = Vec::new();

    // Daily 和 BrowserAgent 模式注入 persona
    if matches!(mode, PromptMode::Daily | PromptMode::BrowserAgent) {
        if let Some(p) = persona {
            if !p.identity.is_empty() {
                dynamic_parts.push(format!("【角色设定】{}", p.identity));
            }
            if !p.expertise.is_empty() {
                dynamic_parts.push(format!("【专业领域】{}", p.expertise.join("、")));
            }
            if !p.memory_hints.is_empty() {
                let hints = p.memory_hints.iter()
                    .map(|h| format!("- {}", h))
                    .collect::<Vec<_>>()
                    .join("\n");
                dynamic_parts.push(format!("【记忆管理（白名单制）】\n{}", hints));
            }
        }
    }

    // Mode-specific prompt
    match mode {
        PromptMode::Daily => {
            let daily = guard.get("daily");
            if !daily.is_empty() {
                let has_persona_memory = persona.map_or(false, |p| !p.memory_hints.is_empty());
                let final_daily = if has_persona_memory {
                    strip_memory_section(daily)
                } else {
                    daily.to_string()
                };
                if !final_daily.trim().is_empty() {
                    dynamic_parts.push(final_daily);
                }
            }
        }
        PromptMode::Analysis => {
            // analysis 模式不加 daily.md
        }
        PromptMode::BrowserAgent => {
            let browser = guard.get("browser_agent");
            if !browser.is_empty() {
                dynamic_parts.push(browser.to_string());
            }
        }
    }

    let dynamic_section = dynamic_parts.join("\n\n");

    SystemPromptParts { static_section, dynamic_section }
}

/// Compose the full system prompt (backward-compatible shim).
///
/// 调用 `build_system_prompt_parts` 后拼接 static + dynamic。
/// **不再注入当前日期**——日期改为 `run_chat_turn_s4` 的首条 user message 注入。
///
/// - `step = None` → PromptMode::Daily
/// - `step = Some(_)` → PromptMode::Analysis
pub fn get_system_prompt(
    step: Option<u32>,
    persona: Option<&crate::storage::file_store::persona::Persona>,
    product_name: Option<&str>,
) -> String {
    let mode = match step {
        None => PromptMode::Daily,
        Some(_) => PromptMode::Analysis,
    };
    let parts = build_system_prompt_parts(mode, persona, product_name);
    if parts.dynamic_section.is_empty() {
        parts.static_section
    } else {
        format!("{}\n\n{}", parts.static_section, parts.dynamic_section)
    }
}

/// Strip the "记忆管理" section from a mode prompt (daily.md).
/// Removes everything from "记忆管理" header to the end of the memory list.
fn strip_memory_section(prompt: &str) -> String {
    let mut result = Vec::new();
    let mut skip = false;

    for line in prompt.lines() {
        // Start skipping when we hit the memory management header
        if line.contains("记忆管理") && line.contains("白名单") {
            skip = true;
            continue;
        }

        if skip {
            // Stop skipping when we hit a non-list, non-blank line
            if !line.trim().is_empty() && !line.trim().starts_with("- ") {
                skip = false;
            } else {
                continue;
            }
        }

        result.push(line);
    }

    result.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{LazyLock, Mutex};

    static PROMPT_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

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

        // Step variants return base + date only (step prompts are in plugins now)
        let step0 = get_system_prompt(Some(0), None, None);
        assert!(step0.contains("AI小家 base"));
        assert!(!step0.contains("日常工作助手"));

        // Invalid step also returns base only
        let step99 = get_system_prompt(Some(99), None, None);
        assert!(step99.contains("AI小家 base"));
        assert!(!step99.contains("日常工作助手"));

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
        assert!(parts.static_section.contains("AI小家 base"),
            "static_section must contain base prompt");
        assert!(parts.static_section.contains("工具选择偏好"),
            "static_section must contain tool preference section");
        assert!(!parts.static_section.contains("今天是"),
            "static_section must NOT contain date");
        assert!(parts.dynamic_section.contains("日常工作助手"),
            "dynamic_section must contain daily prompt");
        assert!(!parts.dynamic_section.contains("AI小家 base"),
            "dynamic_section must NOT repeat base prompt");
    }

    #[test]
    fn test_build_system_prompt_parts_analysis_no_daily() {
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

        let parts = build_system_prompt_parts(PromptMode::Analysis, None, None);
        assert!(!parts.dynamic_section.contains("日常工作助手"),
            "Analysis dynamic_section must NOT contain daily prompt");
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
        assert!(parts.static_section.contains("智能办公"),
            "product_name replacement must work in static_section");
        assert!(!parts.static_section.contains("AI小家"),
            "original brand name must be replaced");
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
        assert!(parts.dynamic_section.contains("你是专业 HR 顾问"),
            "persona identity must appear in dynamic_section");
        assert!(parts.dynamic_section.contains("薪酬分析"),
            "persona expertise must appear in dynamic_section");
        assert!(!parts.static_section.contains("你是专业 HR 顾问"),
            "persona must NOT be in static_section");
    }

    #[test]
    fn test_get_system_prompt_shim_backward_compatible() {
        let _guard = PROMPT_TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        setup_prompts(&bundled, &[("base", "AI小家 base"), ("daily", "日常工作助手")]);
        fs::create_dir_all(&user).unwrap();
        init_prompts(&bundled, &user);

        let prompt = get_system_prompt(None, None, None);
        assert!(prompt.contains("AI小家 base"), "shim: base must be present");
        assert!(prompt.contains("日常工作助手"), "shim: daily must be present for step=None");
        let prompt_step = get_system_prompt(Some(0), None, None);
        assert!(prompt_step.contains("AI小家 base"), "shim: base must be present for step");
        assert!(!prompt_step.contains("日常工作助手"), "shim: daily must be absent for step=Some");
        assert!(!prompt.contains("今天是"), "shim: date must NOT be in system prompt");
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
        assert!(parts.static_section.contains("优先使用专用工具"),
            "must mention prefer dedicated tools");
        assert!(parts.static_section.contains("execute_python"),
            "must mention execute_python in context");
        assert!(parts.static_section.contains("web_search"),
            "must mention web_search in context");
        assert!(parts.static_section.contains("save_memory"),
            "must mention save_memory in context");
    }
}
