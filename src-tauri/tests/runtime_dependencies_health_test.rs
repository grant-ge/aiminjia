use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use app_lib::runtime::dependencies::{RuntimeHealthChecker, RuntimeHealthError, RuntimeToolProbe};
use tempfile::tempdir;

fn create_stub_executable(
    dir: &std::path::Path,
    name: &str,
    version_output: &str,
    exit_code: i32,
) -> PathBuf {
    let script_path = dir.join(name);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf '%s\\n' '{version_output}'\n  exit {exit_code}\nfi\nexit 99\n",
        );
        fs::write(&script_path, script).expect("write stub executable");
        let mut permissions = fs::metadata(&script_path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).expect("set permissions");
    }

    #[cfg(windows)]
    {
        let script = format!(
            "@echo off\r\nif \"%1\"==\"--version\" (\r\n  echo {version_output}\r\n  exit /b {exit_code}\r\n)\r\nexit /b 99\r\n",
        );
        let script_path = script_path.with_extension("cmd");
        fs::write(&script_path, script).expect("write stub executable");
        return script_path;
    }

    script_path
}

fn create_sleeping_stub_executable(dir: &std::path::Path, name: &str) -> PathBuf {
    let script_path = dir.join(name);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::write(&script_path, "#!/bin/sh\nsleep 2\necho never\n").expect("write sleeping stub");
        let mut permissions = fs::metadata(&script_path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).expect("set permissions");
    }

    #[cfg(windows)]
    {
        let script_path = script_path.with_extension("cmd");
        fs::write(
            &script_path,
            "@echo off\r\nping -n 3 127.0.0.1 >nul\r\necho never\r\n",
        )
        .expect("write sleeping stub");
        return script_path;
    }

    script_path
}

#[test]
fn check_reads_tool_versions_from_stub_executables() {
    let tempdir = tempdir().expect("tempdir");
    let node = create_stub_executable(tempdir.path(), "node", "v22.0.0", 0);
    let python = create_stub_executable(tempdir.path(), "python", "Python 3.12.3", 0);
    let uv = create_stub_executable(tempdir.path(), "uv", "uv 0.5.0", 0);

    let report = RuntimeHealthChecker::default()
        .check(&[
            RuntimeToolProbe::new("node", node),
            RuntimeToolProbe::new("python", python),
            RuntimeToolProbe::new("uv", uv),
        ])
        .expect("health check");

    assert_eq!(report.tool_version("node"), Some("v22.0.0"));
    assert_eq!(report.tool_version("python"), Some("Python 3.12.3"));
    assert_eq!(report.tool_version("uv"), Some("uv 0.5.0"));
    assert_eq!(report.tool_version("missing"), None);
}

#[test]
fn check_reports_tool_name_when_version_command_fails() {
    let tempdir = tempdir().expect("tempdir");
    let node = create_stub_executable(tempdir.path(), "node", "node 0.0.0", 2);

    let error = RuntimeHealthChecker::default()
        .check(&[RuntimeToolProbe::new("node", node)])
        .expect_err("health check should fail");

    match error {
        RuntimeHealthError::CommandFailed { name, message } => {
            assert_eq!(name, "node");
            assert!(!message.is_empty());
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn check_times_out_and_reports_tool_name_when_command_hangs() {
    let tempdir = tempdir().expect("tempdir");
    let node = create_sleeping_stub_executable(tempdir.path(), "node");

    let error = RuntimeHealthChecker::with_timeout(Duration::from_millis(50))
        .check(&[RuntimeToolProbe::new("node", node)])
        .expect_err("health check should time out");

    match error {
        RuntimeHealthError::CommandTimedOut { name, timeout_ms } => {
            assert_eq!(name, "node");
            assert_eq!(timeout_ms, 50);
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
