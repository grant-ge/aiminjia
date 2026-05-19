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
    // Back-compat shim: legacy callers pass an ordered list and expect
    // index 0 = User, the rest = Global. Forward to the explicit variant
    // so all the dedup / warn-on-conflict logic lives in one place.
    let tagged: Vec<(PathBuf, SkillSource)> = roots
        .iter()
        .enumerate()
        .map(|(idx, root)| {
            let src = if idx == 0 { SkillSource::User } else { SkillSource::Global };
            (root.clone(), src)
        })
        .collect();
    load_skill_roots_tagged(&tagged)
}

/// Load skills from `(root, source)` pairs in priority order. Earlier entries
/// win on id collisions, with a warn-level log naming the loser so operators
/// can detect when a local upload shadows a tenant-pushed skill.
pub fn load_skill_roots_tagged(roots: &[(PathBuf, SkillSource)]) -> Result<HashMap<String, DiskSkill>> {
    let mut loaded = HashMap::new();
    for (root, source) in roots {
        load_one_root(root, *source, &mut loaded)?;
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
        if let Some(existing) = loaded.get(name) {
            // Don't overwrite — first-loaded (higher priority) wins. But surface
            // the collision so users can diagnose "this skill shows the old
            // local version, not the tenant push I just got."
            log::warn!(
                "skill id collision: '{}' kept from {:?} ({}), ignored {:?} ({})",
                name,
                existing.source,
                existing.root.display(),
                source,
                path.display(),
            );
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
