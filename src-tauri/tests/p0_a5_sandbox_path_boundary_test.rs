use app_lib::python::sandbox::SandboxConfig;

#[test]
fn safe_open_uses_path_separator_boundary_for_allowed_write_paths() {
    let config = SandboxConfig::default();
    let preamble = config.preamble();

    assert!(
        preamble.contains("abs_path == real_root or abs_path.startswith(real_root + os.sep)"),
        "sandbox preamble must require exact root match or root + os.sep boundary"
    );
    assert!(
        !preamble.contains("abs_path.startswith(os.path.realpath(p))"),
        "sandbox preamble must not use raw prefix matching without path boundary"
    );
}
