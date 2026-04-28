use app_lib::runtime::tools::builtin::load_skill::format_fork_result;

#[test]
fn format_fork_result_assembles_expected_text() {
    let out = format_fork_result("biz-writing", "completed-output");
    assert!(out.contains("Skill \"biz-writing\" completed (forked execution)."));
    assert!(out.contains("Result:"));
    assert!(out.contains("completed-output"));
}
