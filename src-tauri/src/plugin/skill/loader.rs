use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::frontmatter::parse_skill_md;
use super::types::{DiskSkill, SkillSource};

pub fn is_valid_skill_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() => {}
        _ => return false,
    }
    id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

pub fn load_skill_roots(roots: &[PathBuf]) -> Result<HashMap<String, DiskSkill>> {
    let mut loaded = HashMap::new();
    for (idx, root) in roots.iter().enumerate() {
        let source = if idx == 0 { SkillSource::User } else { SkillSource::Global };
        load_one_root(root, source, &mut loaded)?;
    }
    Ok(loaded)
}

fn load_one_root(
    root: &Path,
    source: SkillSource,
    loaded: &mut HashMap<String, DiskSkill>,
) -> Result<()> {
    if !root.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('_') || name.starts_with('.') || !is_valid_skill_id(name) {
            continue;
        }
        if loaded.contains_key(name) {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        let content = fs::read_to_string(&skill_md)?;
        let parsed = match parse_skill_md(&content) {
            Ok(parsed) => parsed,
            Err(err) => {
                log::error!("Failed to parse skill {} at {}: {}", name, skill_md.display(), err);
                continue;
            }
        };
        loaded.insert(
            name.to_string(),
            DiskSkill {
                id: name.to_string(),
                root: path,
                frontmatter: parsed.frontmatter,
                body: parsed.body,
                source,
            },
        );
    }
    Ok(())
}
