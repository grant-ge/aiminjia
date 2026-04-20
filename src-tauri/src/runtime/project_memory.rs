use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const ENTRYPOINT_NAME: &str = "MEMORY.md";
const ENTRIES_DIR: &str = "entries";
const LEGACY_CORE_MEMORY_REL_PATH: &str = "shared/cognitive/mem.md";
const LEGACY_MIGRATION_ENTRY_NAME: &str = "legacy-core-memory";
const MAX_RECALLED_ENTRIES: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProjectMemoryType {
    UserPreference,
    ProjectConstraint,
    ReferenceInfo,
}

impl ProjectMemoryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UserPreference => "user_preference",
            Self::ProjectConstraint => "project_constraint",
            Self::ReferenceInfo => "reference_info",
        }
    }

    fn from_str(raw: &str) -> Option<Self> {
        match raw.trim() {
            "user_preference" => Some(Self::UserPreference),
            "project_constraint" => Some(Self::ProjectConstraint),
            "reference_info" => Some(Self::ReferenceInfo),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectMemoryContext {
    pub index_text: String,
    pub recalled_entries: Vec<ProjectMemoryEntry>,
}

impl ProjectMemoryContext {
    pub fn is_empty(&self) -> bool {
        self.index_text.trim().is_empty() && self.recalled_entries.is_empty()
    }

    pub fn render_for_prompt(&self) -> String {
        if !self.recalled_entries.is_empty() {
            let recalled = self
                .recalled_entries
                .iter()
                .map(ProjectMemoryEntry::render_for_prompt)
                .collect::<Vec<_>>()
                .join("\n\n");
            return format!("[相关记忆]\n{}", recalled);
        }

        self.index_text.trim().to_string()
    }

    pub fn from_legacy_core_memory(content: impl Into<String>) -> Self {
        Self {
            index_text: content.into(),
            recalled_entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMemoryEntry {
    pub name: String,
    pub description: String,
    pub memory_type: ProjectMemoryType,
    pub content: String,
    pub path: PathBuf,
    pub relative_path: PathBuf,
}

impl ProjectMemoryEntry {
    fn render_index_line(&self) -> String {
        format!(
            "- [{}]({}) - {}",
            self.name,
            self.relative_path.display(),
            self.description
        )
    }

    fn render_for_prompt(&self) -> String {
        format!(
            "### {}\n- 类型: {}\n- 文件: {}\n- 说明: {}\n\n{}",
            self.name,
            self.memory_type.as_str(),
            self.relative_path.display(),
            self.description,
            self.content.trim()
        )
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectMemoryEntryDraft {
    pub memory_type: ProjectMemoryType,
    pub name: String,
    pub description: String,
    pub content: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SavedProjectMemory {
    pub path: PathBuf,
    pub relative_path: PathBuf,
}

pub struct ProjectMemoryService {
    app_data_dir: PathBuf,
    workspace_path: PathBuf,
    memory_root: PathBuf,
}

impl ProjectMemoryService {
    pub fn new(
        app_data_dir: impl AsRef<Path>,
        workspace_path: impl AsRef<Path>,
    ) -> Self {
        let app_data_dir = app_data_dir.as_ref().to_path_buf();
        let workspace_path = workspace_path.as_ref().to_path_buf();
        let canonical_workspace = canonical_workspace_key_path(&workspace_path);
        let bucket = format!(
            "{}-{:016x}",
            slugify_path_tail(&canonical_workspace),
            stable_hash(&canonical_workspace.to_string_lossy())
        );
        let memory_root = app_data_dir.join("project_memories").join(bucket);

        Self {
            app_data_dir,
            workspace_path,
            memory_root,
        }
    }

    pub fn memory_root(&self) -> &Path {
        &self.memory_root
    }

    pub fn entrypoint_path(&self) -> PathBuf {
        self.memory_root.join(ENTRYPOINT_NAME)
    }

    pub fn save_memory(&self, draft: ProjectMemoryEntryDraft) -> Result<SavedProjectMemory> {
        self.ensure_dirs()?;

        let file_stem = format!(
            "{}-{:016x}",
            slugify(&draft.name),
            stable_hash(format!("{}::{}", draft.name, draft.description))
        );
        let relative_path = PathBuf::from(ENTRIES_DIR).join(format!("{}.md", file_stem));
        let full_path = self.memory_root.join(&relative_path);
        let payload = render_entry_file(&draft);
        fs::write(&full_path, payload)
            .with_context(|| format!("Failed to write project memory '{}'", full_path.display()))?;

        self.rebuild_index()?;

        Ok(SavedProjectMemory {
            path: full_path,
            relative_path,
        })
    }

    pub fn load_context(&self, query: &str) -> Result<ProjectMemoryContext> {
        self.ensure_dirs()?;
        self.ensure_legacy_migrated()?;

        let entries = self.load_entries()?;
        let index_text = fs::read_to_string(self.entrypoint_path()).unwrap_or_default();
        let recalled_entries = select_relevant_entries(&entries, query, MAX_RECALLED_ENTRIES);

        Ok(ProjectMemoryContext {
            index_text,
            recalled_entries,
        })
    }

    pub fn distill_index(&self) -> Result<usize> {
        self.ensure_dirs()?;
        self.ensure_legacy_migrated()?;
        let entries = self.load_entries()?;
        self.rebuild_index()?;
        Ok(entries.len())
    }

    fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(self.memory_root.join(ENTRIES_DIR)).with_context(|| {
            format!(
                "Failed to create project memory directory '{}'",
                self.memory_root.display()
            )
        })?;
        Ok(())
    }

    fn ensure_legacy_migrated(&self) -> Result<()> {
        let legacy_path = self.app_data_dir.join(LEGACY_CORE_MEMORY_REL_PATH);
        if !legacy_path.exists() {
            return Ok(());
        }

        let legacy_content = fs::read_to_string(&legacy_path).unwrap_or_default();
        if legacy_content.trim().is_empty() {
            return Ok(());
        }

        self.ensure_dirs()?;
        let relative_path = PathBuf::from(ENTRIES_DIR).join(format!("{LEGACY_MIGRATION_ENTRY_NAME}.md"));
        let full_path = self.memory_root.join(&relative_path);
        if full_path.exists() {
            if !self.entrypoint_path().exists() {
                self.rebuild_index()?;
            }
            return Ok(());
        }

        let draft = ProjectMemoryEntryDraft {
            memory_type: ProjectMemoryType::ProjectConstraint,
            name: LEGACY_MIGRATION_ENTRY_NAME.to_string(),
            description: "从 legacy core_memory 懒迁移而来的历史项目记忆".to_string(),
            content: legacy_content,
            source: Some("legacy-core-memory".to_string()),
        };
        fs::write(&full_path, render_entry_file(&draft)).with_context(|| {
            format!(
                "Failed to migrate legacy core memory into '{}'",
                full_path.display()
            )
        })?;
        self.rebuild_index()?;
        Ok(())
    }

    fn load_entries(&self) -> Result<Vec<ProjectMemoryEntry>> {
        let entries_dir = self.memory_root.join(ENTRIES_DIR);
        if !entries_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        for dir_entry in fs::read_dir(&entries_dir)
            .with_context(|| format!("Failed to read '{}'", entries_dir.display()))?
        {
            let dir_entry = dir_entry?;
            let path = dir_entry.path();
            if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            if let Some(entry) = parse_entry_file(&self.memory_root, &path)? {
                entries.push(entry);
            }
        }

        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn rebuild_index(&self) -> Result<()> {
        let entries = self.load_entries()?;
        let mut index = String::from("# MEMORY.md\n\n");
        index.push_str(&format!(
            "<!-- workspace: {} -->\n\n",
            canonical_workspace_key_path(&self.workspace_path).display()
        ));

        if entries.is_empty() {
            index.push_str("_暂无项目记忆条目。_\n");
        } else {
            for entry in &entries {
                index.push_str(&entry.render_index_line());
                index.push('\n');
            }
        }

        fs::write(self.entrypoint_path(), index)
            .with_context(|| format!("Failed to write '{}'", self.entrypoint_path().display()))?;
        Ok(())
    }
}

fn render_entry_file(draft: &ProjectMemoryEntryDraft) -> String {
    let mut content = String::new();
    content.push_str("---\n");
    content.push_str(&format!("type: {}\n", draft.memory_type.as_str()));
    content.push_str(&format!("name: {}\n", draft.name.trim()));
    content.push_str(&format!("description: {}\n", draft.description.trim()));
    if let Some(source) = draft.source.as_deref() {
        content.push_str(&format!("source: {}\n", source.trim()));
    }
    content.push_str("---\n\n");
    content.push_str(draft.content.trim());
    content.push('\n');
    content
}

fn parse_entry_file(memory_root: &Path, path: &Path) -> Result<Option<ProjectMemoryEntry>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read project memory '{}'", path.display()))?;
    let Some((frontmatter, body)) = split_frontmatter(&raw) else {
        return Ok(None);
    };

    let mut name = String::new();
    let mut description = String::new();
    let mut memory_type = None;
    for line in frontmatter.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().to_string();
        match key.trim() {
            "name" => name = value,
            "description" => description = value,
            "type" => memory_type = ProjectMemoryType::from_str(&value),
            _ => {}
        }
    }

    let Some(memory_type) = memory_type else {
        return Ok(None);
    };
    if name.is_empty() || description.is_empty() {
        return Ok(None);
    }

    Ok(Some(ProjectMemoryEntry {
        name,
        description,
        memory_type,
        content: body.trim().to_string(),
        path: path.to_path_buf(),
        relative_path: path
            .strip_prefix(memory_root)
            .unwrap_or(path)
            .to_path_buf(),
    }))
}

fn split_frontmatter(raw: &str) -> Option<(String, String)> {
    let trimmed = raw.trim_start();
    let rest = trimmed.strip_prefix("---\n")?;
    let (frontmatter, body) = rest.split_once("\n---\n")?;
    Some((frontmatter.to_string(), body.to_string()))
}

fn select_relevant_entries(
    entries: &[ProjectMemoryEntry],
    query: &str,
    limit: usize,
) -> Vec<ProjectMemoryEntry> {
    let tokens = query_tokens(query);
    if tokens.is_empty() {
        return Vec::new();
    }

    let mut scored = entries
        .iter()
        .filter_map(|entry| {
            let haystack = format!(
                "{}\n{}\n{}",
                entry.name.to_lowercase(),
                entry.description.to_lowercase(),
                entry.content.to_lowercase()
            );
            let score = tokens.iter().filter(|token| haystack.contains(token.as_str())).count();
            (score > 0).then(|| (score, entry.clone()))
        })
        .collect::<Vec<_>>();

    scored.sort_by(|(score_a, entry_a), (score_b, entry_b)| {
        score_b
            .cmp(score_a)
            .then_with(|| entry_a.name.cmp(&entry_b.name))
    });
    scored.truncate(limit);
    scored.into_iter().map(|(_, entry)| entry).collect()
}

fn query_tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut seen = HashSet::new();
    for token in query
        .to_lowercase()
        .split(|ch: char| !(ch.is_alphanumeric() || ch >= '\u{4e00}' && ch <= '\u{9fff}'))
    {
        let token = token.trim();
        if token.len() < 2 {
            continue;
        }
        if seen.insert(token.to_string()) {
            tokens.push(token.to_string());
        }
    }
    tokens
}

fn canonical_workspace_key_path(workspace_path: &Path) -> PathBuf {
    resolve_git_toplevel(workspace_path)
        .or_else(|| workspace_path.canonicalize().ok())
        .unwrap_or_else(|| workspace_path.to_path_buf())
}

fn resolve_git_toplevel(workspace_path: &Path) -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(workspace_path)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        return None;
    }
    let root_path = PathBuf::from(&root);
    root_path.canonicalize().ok().or(Some(root_path))
}

fn slugify_path_tail(path: &Path) -> String {
    path.file_name()
        .map(|name| slugify(&name.to_string_lossy()))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "workspace".to_string())
}

fn slugify(raw: &str) -> String {
    let mut result = String::new();
    let mut last_dash = false;
    for ch in raw.chars() {
        let lowered = ch.to_ascii_lowercase();
        if lowered.is_ascii_alphanumeric() {
            result.push(lowered);
            last_dash = false;
        } else if !last_dash {
            result.push('-');
            last_dash = true;
        }
    }
    result.trim_matches('-').to_string()
}

fn stable_hash(input: impl AsRef<str>) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in input.as_ref().as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}
