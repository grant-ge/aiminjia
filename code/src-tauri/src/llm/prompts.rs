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
const BASE_FALLBACK: &str = "你是 AI小家 — 组织咨询专家和智能工作助手。";

/// All recognized prompt names.
const PROMPT_NAMES: &[&str] = &[
    "base", "daily", "step0", "step1", "step2", "step3", "step4", "step5",
];

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
        self.prompts
            .get(name)
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
    let mut guard = PROMPT_STORE.write().expect("PromptStore write lock poisoned");
    *guard = store;
}

/// Reload all prompts from disk (for future hot-reload from settings UI).
#[allow(dead_code)]
pub fn reload_prompts() {
    let mut guard = PROMPT_STORE.write().expect("PromptStore write lock poisoned");
    guard.reload();
}

/// Compose the full system prompt by combining BASE + mode-specific prompt.
///
/// - `step = None` → daily consultation mode (BASE + DAILY)
/// - `step = Some(0..=5)` → analysis step mode (BASE + STEP_N)
pub fn get_system_prompt(step: Option<u32>) -> String {
    let guard = PROMPT_STORE.read().expect("PromptStore read lock poisoned");

    let base = guard.get("base");
    let mode_key = match step {
        None => "daily",
        Some(0) => "step0",
        Some(1) => "step1",
        Some(2) => "step2",
        Some(3) => "step3",
        Some(4) => "step4",
        Some(5) => "step5",
        Some(_) => "daily", // fallback
    };
    let mode_prompt = guard.get(mode_key);

    if mode_prompt.is_empty() {
        base.to_string()
    } else {
        format!("{}\n\n{}", base, mode_prompt)
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
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        fs::create_dir_all(&bundled).unwrap();
        fs::create_dir_all(&user).unwrap();

        setup_prompts(&bundled, &[
            ("base", "Test base prompt"),
            ("daily", "Test daily prompt"),
        ]);

        init_prompts(&bundled, &user);

        let prompt = get_system_prompt(None);
        assert!(prompt.contains("Test base prompt"));
        assert!(prompt.contains("Test daily prompt"));
    }

    #[test]
    fn test_user_override_priority() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");

        setup_prompts(&bundled, &[
            ("base", "Bundled base"),
            ("daily", "Bundled daily"),
        ]);
        setup_prompts(&user, &[
            ("base", "Custom base"),
        ]);

        init_prompts(&bundled, &user);

        let prompt = get_system_prompt(None);
        assert!(prompt.contains("Custom base"), "User override should take priority");
        assert!(prompt.contains("Bundled daily"), "Non-overridden should use bundled");
    }

    #[test]
    fn test_empty_file_falls_through() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");

        setup_prompts(&bundled, &[
            ("base", "Bundled base"),
        ]);
        // Empty override file should be ignored
        setup_prompts(&user, &[
            ("base", "   "),
        ]);

        init_prompts(&bundled, &user);

        let prompt = get_system_prompt(None);
        assert!(prompt.contains("Bundled base"), "Empty override should fall through to bundled");
    }

    #[test]
    fn test_fallback_base() {
        let tmp = tempfile::tempdir().unwrap();
        let empty_bundled = tmp.path().join("empty_bundled");
        let empty_user = tmp.path().join("empty_user");
        fs::create_dir_all(&empty_bundled).unwrap();
        fs::create_dir_all(&empty_user).unwrap();

        init_prompts(&empty_bundled, &empty_user);

        let prompt = get_system_prompt(None);
        assert!(prompt.contains("AI小家"), "Should fall back to hardcoded base");
    }

    #[test]
    fn test_api_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");

        setup_prompts(&bundled, &[
            ("base", "AI小家 base"),
            ("daily", "日常工作助手"),
            ("step0", "分析方向确认"),
            ("step1", "数据清洗"),
            ("step2", "岗位归一化"),
            ("step3", "职级推断"),
            ("step4", "公平性诊断"),
            ("step5", "行动方案"),
        ]);
        fs::create_dir_all(&user).unwrap();

        init_prompts(&bundled, &user);

        // All step variants work
        assert!(get_system_prompt(None).contains("日常工作助手"));
        assert!(get_system_prompt(Some(0)).contains("分析方向确认"));
        assert!(get_system_prompt(Some(1)).contains("数据清洗"));
        assert!(get_system_prompt(Some(2)).contains("岗位归一化"));
        assert!(get_system_prompt(Some(3)).contains("职级推断"));
        assert!(get_system_prompt(Some(4)).contains("公平性诊断"));
        assert!(get_system_prompt(Some(5)).contains("行动方案"));

        // Invalid step falls back to daily
        assert!(get_system_prompt(Some(99)).contains("日常工作助手"));

        // Base always included
        for step in [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)] {
            assert!(
                get_system_prompt(step).contains("AI小家 base"),
                "Step {:?} should include base prompt",
                step,
            );
        }
    }

    #[test]
    fn test_reload() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");

        setup_prompts(&bundled, &[
            ("base", "Original base"),
        ]);
        fs::create_dir_all(&user).unwrap();

        // Test reload on a standalone PromptStore instance to avoid global state races
        let mut store = PromptStore::new(&bundled, &user);
        assert!(store.get("base").contains("Original base"));

        // Write user override
        setup_prompts(&user, &[
            ("base", "Updated base"),
        ]);

        store.reload();
        assert!(store.get("base").contains("Updated base"));
    }
}
