use app_lib::runtime::tools::catalog::TOOL_CATALOG;

#[test]
fn review_long_running_tools_declare_timeout() {
    for id in [
        "bash",
        "load_file",
        "execute_python",
        "generate_report",
        "generate_chart",
    ] {
        let def = TOOL_CATALOG.get(id).unwrap();
        assert!(
            def.default_timeout_secs.is_some(),
            "{id} should declare a default timeout in TOOL_CATALOG"
        );
    }
}

#[test]
fn review_bash_timeout_in_catalog_matches_tool_constant() {
    let def = TOOL_CATALOG.get("bash").unwrap();
    assert_eq!(def.default_timeout_secs, Some(120));
}
