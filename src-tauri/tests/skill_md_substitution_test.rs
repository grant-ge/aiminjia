use app_lib::plugin::skill::substitution::{substitute_skill_body, SkillSubstitutionContext};
use tempfile::TempDir;

#[test]
fn substitutes_skill_dir_session_and_arguments() {
    let dir = TempDir::new().unwrap();
    let ctx = SkillSubstitutionContext {
        skill_dir: dir.path().to_path_buf(),
        session_id: "session-123".to_string(),
        args: "北京 工程师".to_string(),
        argument_names: vec!["city".to_string(), "role".to_string()],
        execute_shell: false,
    };
    let body = "Dir=${AIJIA_SKILL_DIR}\nSession=${AIJIA_SESSION_ID}\nArgs=$ARGUMENTS\nCity=$city\nRole=$role\nFirst=$1";
    let result = substitute_skill_body(body, &ctx).unwrap();
    assert!(result.contains(&format!("Dir={}", dir.path().display())));
    assert!(result.contains("Session=session-123"));
    assert!(result.contains("Args=北京 工程师"));
    assert!(result.contains("City=北京"));
    assert!(result.contains("Role=工程师"));
    assert!(result.contains("First=北京"));
}

#[test]
fn appends_arguments_when_placeholder_absent() {
    let dir = TempDir::new().unwrap();
    let ctx = SkillSubstitutionContext {
        skill_dir: dir.path().to_path_buf(),
        session_id: "s".to_string(),
        args: "raw args".to_string(),
        argument_names: vec![],
        execute_shell: false,
    };
    let result = substitute_skill_body("body", &ctx).unwrap();
    assert!(result.contains("ARGUMENTS: raw args"));
}

#[test]
fn leaves_unknown_placeholders_unchanged() {
    let dir = TempDir::new().unwrap();
    let ctx = SkillSubstitutionContext {
        skill_dir: dir.path().to_path_buf(),
        session_id: "s".to_string(),
        args: "".to_string(),
        argument_names: vec![],
        execute_shell: false,
    };
    let result = substitute_skill_body("$unknown ${AIJIA_UNKNOWN}", &ctx).unwrap();
    assert!(result.contains("$unknown"));
    assert!(result.contains("${AIJIA_UNKNOWN}"));
}

#[test]
fn executes_inline_shell_blocks_when_enabled() {
    let dir = tempfile::TempDir::new().unwrap();
    let ctx = SkillSubstitutionContext {
        skill_dir: dir.path().to_path_buf(),
        session_id: "s".to_string(),
        args: "".to_string(),
        argument_names: vec![],
        execute_shell: true,
    };
    let result = substitute_skill_body("before !`printf hello` after", &ctx).unwrap();
    assert_eq!(result, "before hello after");
}
