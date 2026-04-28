use std::fs;
use tempfile::TempDir;

use app_lib::commands::skill_management::{validate_skill_directory, SkillValidationError};

fn write(dir: &std::path::Path, rel: &str, body: &str) {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(p, body).unwrap();
}

fn skill_dir(parent: &TempDir) -> std::path::PathBuf {
    let dir = parent.path().join("test-skill");
    fs::create_dir(&dir).unwrap();
    dir
}

#[test]
fn rejects_missing_skill_md() {
    let tmp = TempDir::new().unwrap();
    let dir = skill_dir(&tmp);
    let err = validate_skill_directory(&dir).unwrap_err();
    assert!(matches!(err, SkillValidationError::MissingSkillMd));
}

#[test]
fn rejects_invalid_frontmatter_yaml() {
    let tmp = TempDir::new().unwrap();
    let dir = skill_dir(&tmp);
    write(
        &dir,
        "SKILL.md",
        "---\nname: [unterminated\n---\nbody\n",
    );
    let err = validate_skill_directory(&dir).unwrap_err();
    assert!(matches!(err, SkillValidationError::ParseFailed(_)));
}

#[test]
fn rejects_invalid_directory_basename() {
    let parent = TempDir::new().unwrap();
    // basename "Bad Name!" violates is_valid_skill_id (uppercase, space, !)
    let dir = parent.path().join("Bad Name!");
    fs::create_dir(&dir).unwrap();
    write(
        &dir,
        "SKILL.md",
        "---\nname: my-skill\ndescription: x\n---\nbody\n",
    );
    let err = validate_skill_directory(&dir).unwrap_err();
    assert!(matches!(err, SkillValidationError::InvalidName(_)));
}

#[test]
fn rejects_empty_description() {
    let tmp = TempDir::new().unwrap();
    let dir = skill_dir(&tmp);
    write(
        &dir,
        "SKILL.md",
        "---\nname: my-skill\ndescription: \"\"\n---\nbody\n",
    );
    let err = validate_skill_directory(&dir).unwrap_err();
    assert!(matches!(err, SkillValidationError::ParseFailed(_)));
}

#[test]
fn accepts_minimal_valid_skill() {
    let tmp = TempDir::new().unwrap();
    let dir = skill_dir(&tmp);
    write(
        &dir,
        "SKILL.md",
        "---\nname: my-skill\ndescription: A test skill\n---\nHello\n",
    );
    validate_skill_directory(&dir).expect("should pass validation");
}
