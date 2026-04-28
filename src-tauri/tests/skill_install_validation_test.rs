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

#[test]
fn rejects_missing_skill_md() {
    let tmp = TempDir::new().unwrap();
    let err = validate_skill_directory(tmp.path()).unwrap_err();
    assert!(matches!(err, SkillValidationError::MissingSkillMd));
}

#[test]
fn rejects_invalid_frontmatter_yaml() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "SKILL.md",
        "---\nname: [unterminated\n---\nbody\n",
    );
    let err = validate_skill_directory(tmp.path()).unwrap_err();
    assert!(matches!(err, SkillValidationError::ParseFailed(_)));
}

#[test]
fn rejects_invalid_skill_id_in_frontmatter_name() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "SKILL.md",
        "---\nname: Invalid Name!\ndescription: x\n---\nbody\n",
    );
    let err = validate_skill_directory(tmp.path()).unwrap_err();
    assert!(matches!(err, SkillValidationError::InvalidName(_)));
}

#[test]
fn rejects_empty_description() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "SKILL.md",
        "---\nname: my-skill\ndescription: \"\"\n---\nbody\n",
    );
    let err = validate_skill_directory(tmp.path()).unwrap_err();
    assert!(matches!(err, SkillValidationError::EmptyDescription));
}

#[test]
fn accepts_minimal_valid_skill() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "SKILL.md",
        "---\nname: my-skill\ndescription: A test skill\n---\nHello\n",
    );
    validate_skill_directory(tmp.path()).expect("should pass validation");
}
