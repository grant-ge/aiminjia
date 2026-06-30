use app_lib::runtime::tools::catalog::TOOL_CATALOG;

#[test]
fn shell_tools_do_not_expose_runtime_env_selector_in_catalog() {
    for tool_name in ["Bash", "PowerShell"] {
        let entry = TOOL_CATALOG
            .get_entry(tool_name)
            .expect("shell tool should be registered");
        assert!(entry
            .json_schema
            .pointer("/properties/runtime_env")
            .is_none());

        assert!(entry
            .definition
            .description
            .contains("没有 runtime_env 参数"));
    }
}
