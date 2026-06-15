use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Deserialize;

use super::frontmatter::parse_skill_md;
use super::required_builtin::is_required_builtin_skill;
use super::types::{DiskSkill, SkillDisplayI18nText, SkillSource};

/// Sidecar metadata written by the lotus skill-sync path. Used to overlay
/// fields onto the SKILL.md frontmatter when the in-package values are
/// missing — primarily category, for legacy packages uploaded before the
/// strict-frontmatter contract. Leaving SKILL.md untouched preserves the
/// sha256 integrity check.
#[derive(Debug, Default, Deserialize)]
struct LotusMeta {
    #[serde(default)]
    category: Option<String>,
    #[serde(default, rename = "displayI18n", alias = "display_i18n")]
    display_i18n: HashMap<String, SkillDisplayI18nText>,
}

fn read_lotus_meta(skill_dir: &Path) -> LotusMeta {
    let path = skill_dir.join(".lotus-meta.json");
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(_) => return LotusMeta::default(),
    };
    match serde_json::from_slice::<LotusMeta>(&bytes) {
        Ok(meta) => meta,
        Err(err) => {
            log::warn!(
                "parse .lotus-meta.json at {} failed: {} (ignoring sidecar)",
                path.display(),
                err
            );
            LotusMeta::default()
        }
    }
}

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
            let src = if idx == 0 {
                SkillSource::User
            } else {
                SkillSource::Global
            };
            (root.clone(), src)
        })
        .collect();
    load_skill_roots_tagged(&tagged)
}

/// Load skills from `(root, source)` pairs in priority order. Earlier entries
/// win on id collisions, with a warn-level log naming the loser so operators
/// can detect when a local upload shadows a tenant-pushed skill.
pub fn load_skill_roots_tagged(
    roots: &[(PathBuf, SkillSource)],
) -> Result<HashMap<String, DiskSkill>> {
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
        if matches!(source, SkillSource::Global) && !is_required_builtin_skill(name) {
            log::info!(
                "skip non-required global skill '{}' from {}",
                name,
                path.display()
            );
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
        let mut parsed = match parse_skill_md(&content) {
            Ok(parsed) => parsed,
            Err(err) => {
                log::error!(
                    "Failed to parse skill {} at {}: {}",
                    name,
                    skill_md.display(),
                    err
                );
                continue;
            }
        };
        // Overlay .lotus-meta.json sidecar fields onto the frontmatter. Only
        // applied when the frontmatter value is missing — packages with
        // explicit `category:` in SKILL.md remain authoritative.
        let meta = read_lotus_meta(&path);
        if !meta.display_i18n.is_empty() {
            parsed.frontmatter.metadata.display_i18n = meta.display_i18n;
        }
        let is_blank = parsed
            .frontmatter
            .category
            .as_deref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true);
        if is_blank {
            if let Some(cat) = meta.category {
                let cat_trimmed = cat.trim();
                if !cat_trimmed.is_empty() {
                    parsed.frontmatter.category = Some(cat_trimmed.to_string());
                }
            }
        }
        // If sync wrote a `.scope` marker, upgrade Global → Tenant when marker
        // says "tenant". User root never reads .scope (local uploads stay User).
        let effective_source = if matches!(source, SkillSource::Global) {
            match fs::read_to_string(path.join(".scope")) {
                Ok(s) if s.trim() == "tenant" => SkillSource::Tenant,
                _ => SkillSource::Global,
            }
        } else {
            source
        };
        loaded.insert(
            name.to_string(),
            DiskSkill {
                id: name.to_string(),
                root: path,
                frontmatter: parsed.frontmatter,
                body: parsed.body,
                source: effective_source,
            },
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_skill(root: &Path, id: &str, skill_md: &str, sidecar: Option<&str>) {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), skill_md).unwrap();
        if let Some(s) = sidecar {
            fs::write(dir.join(".lotus-meta.json"), s).unwrap();
        }
    }

    const MD_WITH_CATEGORY: &str =
        "---\nname: explicit\ndescription: x\nversion: \"1.0\"\ncategory: hr\n---\nbody\n";
    const MD_WITHOUT_CATEGORY: &str =
        "---\nname: legacy\ndescription: x\nversion: \"1.0\"\n---\nbody\n";

    #[test]
    fn sidecar_fills_missing_category() {
        let tmp = TempDir::new().unwrap();
        write_skill(
            tmp.path(),
            "legacy-skill",
            MD_WITHOUT_CATEGORY,
            Some(r#"{"category":"hr"}"#),
        );
        let loaded =
            load_skill_roots_tagged(&[(tmp.path().to_path_buf(), SkillSource::User)]).unwrap();
        let skill = loaded.get("legacy-skill").expect("loaded");
        assert_eq!(skill.frontmatter.category.as_deref(), Some("hr"));
    }

    #[test]
    fn sidecar_fills_display_i18n() {
        let tmp = TempDir::new().unwrap();
        write_skill(
            tmp.path(),
            "legacy-skill",
            MD_WITHOUT_CATEGORY,
            Some(
                r#"{"displayI18n":{"en-US":{"name":"Budget Analysis","description":"Analyze budget execution"}}}"#,
            ),
        );
        let loaded =
            load_skill_roots_tagged(&[(tmp.path().to_path_buf(), SkillSource::User)]).unwrap();
        let skill = loaded.get("legacy-skill").expect("loaded");
        let en = skill
            .frontmatter
            .metadata
            .display_i18n
            .get("en-US")
            .expect("en-US display");
        assert_eq!(en.name.as_deref(), Some("Budget Analysis"));
        assert_eq!(en.description.as_deref(), Some("Analyze budget execution"));
    }

    #[test]
    fn sidecar_accepts_go_field_name_display_i18n() {
        let tmp = TempDir::new().unwrap();
        write_skill(
            tmp.path(),
            "legacy-skill",
            MD_WITHOUT_CATEGORY,
            Some(
                r#"{"displayI18n":{"en-US":{"Name":"Budget Analysis","Description":"Analyze budget execution"}}}"#,
            ),
        );
        let loaded =
            load_skill_roots_tagged(&[(tmp.path().to_path_buf(), SkillSource::User)]).unwrap();
        let skill = loaded.get("legacy-skill").expect("loaded");
        let en = skill
            .frontmatter
            .metadata
            .display_i18n
            .get("en-US")
            .expect("en-US display");
        assert_eq!(en.name.as_deref(), Some("Budget Analysis"));
        assert_eq!(en.description.as_deref(), Some("Analyze budget execution"));
    }

    #[test]
    fn frontmatter_category_beats_sidecar() {
        let tmp = TempDir::new().unwrap();
        write_skill(
            tmp.path(),
            "explicit-skill",
            MD_WITH_CATEGORY,
            Some(r#"{"category":"finance"}"#),
        );
        let loaded =
            load_skill_roots_tagged(&[(tmp.path().to_path_buf(), SkillSource::User)]).unwrap();
        let skill = loaded.get("explicit-skill").expect("loaded");
        // SKILL.md wins; sidecar is fallback only
        assert_eq!(skill.frontmatter.category.as_deref(), Some("hr"));
    }

    #[test]
    fn missing_sidecar_leaves_category_unset() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "no-sidecar", MD_WITHOUT_CATEGORY, None);
        let loaded =
            load_skill_roots_tagged(&[(tmp.path().to_path_buf(), SkillSource::User)]).unwrap();
        let skill = loaded.get("no-sidecar").expect("loaded");
        assert!(skill.frontmatter.category.is_none());
    }

    #[test]
    fn malformed_sidecar_does_not_break_load() {
        let tmp = TempDir::new().unwrap();
        write_skill(
            tmp.path(),
            "bad-sidecar",
            MD_WITHOUT_CATEGORY,
            Some("not json"),
        );
        let loaded =
            load_skill_roots_tagged(&[(tmp.path().to_path_buf(), SkillSource::User)]).unwrap();
        let skill = loaded
            .get("bad-sidecar")
            .expect("still loads despite bad sidecar");
        assert!(skill.frontmatter.category.is_none());
    }

    #[test]
    fn sidecar_empty_category_does_not_overwrite() {
        let tmp = TempDir::new().unwrap();
        write_skill(
            tmp.path(),
            "blank-sidecar",
            MD_WITHOUT_CATEGORY,
            Some(r#"{"category":"   "}"#),
        );
        let loaded =
            load_skill_roots_tagged(&[(tmp.path().to_path_buf(), SkillSource::User)]).unwrap();
        let skill = loaded.get("blank-sidecar").expect("loaded");
        assert!(skill.frontmatter.category.is_none());
    }

    #[test]
    fn global_root_loads_required_builtin_only() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "market-only", MD_WITHOUT_CATEGORY, None);
        write_skill(tmp.path(), "dingtalk-workspace", MD_WITHOUT_CATEGORY, None);

        let loaded =
            load_skill_roots_tagged(&[(tmp.path().to_path_buf(), SkillSource::Global)]).unwrap();

        assert!(!loaded.contains_key("market-only"));
        assert!(loaded.contains_key("dingtalk-workspace"));
    }
}
