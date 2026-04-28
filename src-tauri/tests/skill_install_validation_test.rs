use std::fs;
use tempfile::TempDir;

use app_lib::commands::skill_management::{
    install_custom_skill_to_dir_with_force, validate_skill_directory, InstallSkillError,
    SkillValidationError,
};

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

#[test]
fn install_succeeds_when_target_missing() {
    let staging = TempDir::new().unwrap();
    let src = staging.path().join("my-skill-src");
    fs::create_dir(&src).unwrap();
    write(&src, "SKILL.md", "---\nname: my-skill\ndescription: ok\n---\nbody\n");

    let dst_parent = TempDir::new().unwrap();
    let result = install_custom_skill_to_dir_with_force(&src, dst_parent.path(), false).unwrap();
    assert!(result.contains("my-skill-src"));
    assert!(dst_parent.path().join("my-skill-src/SKILL.md").is_file());
}

#[test]
fn install_returns_already_exists_when_target_present_and_force_false() {
    let src_parent = TempDir::new().unwrap();
    let src = src_parent.path().join("dup-skill");
    fs::create_dir(&src).unwrap();
    write(&src, "SKILL.md", "---\nname: dup-skill\ndescription: ok\n---\n");

    let dst_parent = TempDir::new().unwrap();
    fs::create_dir(dst_parent.path().join("dup-skill")).unwrap();
    fs::write(dst_parent.path().join("dup-skill/SKILL.md"), "old").unwrap();

    let err = install_custom_skill_to_dir_with_force(&src, dst_parent.path(), false).unwrap_err();
    assert!(matches!(err, InstallSkillError::AlreadyExists(_)));
    assert_eq!(
        fs::read_to_string(dst_parent.path().join("dup-skill/SKILL.md")).unwrap(),
        "old"
    );
}

#[test]
fn install_overwrites_when_force_true() {
    let src_parent = TempDir::new().unwrap();
    let src = src_parent.path().join("dup-skill");
    fs::create_dir(&src).unwrap();
    write(&src, "SKILL.md", "---\nname: dup-skill\ndescription: ok\n---\nNEW\n");

    let dst_parent = TempDir::new().unwrap();
    fs::create_dir(dst_parent.path().join("dup-skill")).unwrap();
    fs::write(dst_parent.path().join("dup-skill/SKILL.md"), "old").unwrap();

    install_custom_skill_to_dir_with_force(&src, dst_parent.path(), true).unwrap();
    let new_content = fs::read_to_string(dst_parent.path().join("dup-skill/SKILL.md")).unwrap();
    assert!(new_content.contains("NEW"));
}
