#[test]
fn powershell_tool_uses_best_effort_encoding_prologue_for_constrained_language() {
    let source = std::fs::read_to_string("src/runtime/tools/builtin/powershell.rs")
        .expect("read powershell tool source");

    assert!(
        source.contains("& {{ try {{ [Console]::OutputEncoding")
            && source.contains("catch {{ }} }} 2>$null"),
        "PowerShell UTF-8 prologue must suppress ConstrainedLanguage property-set failures"
    );
    assert!(
        source.contains("2>$null"),
        "PowerShell UTF-8 prologue must suppress best-effort setup stderr"
    );
    assert!(
        !source.contains("[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \\\n             $OutputEncoding = [System.Text.Encoding]::UTF8;"),
        "raw OutputEncoding assignments before the user command regress Windows ConstrainedLanguage sessions"
    );
}

#[test]
fn hooks_runner_uses_best_effort_encoding_prologue_for_constrained_language() {
    let source =
        std::fs::read_to_string("src/runtime/hooks/runner.rs").expect("read hook runner source");

    assert!(
        source.contains("& {{ try {{ [Console]::OutputEncoding")
            && source.contains("catch {{ }} }} 2>$null"),
        "hook Windows PowerShell prologue must suppress ConstrainedLanguage property-set failures"
    );
    assert!(
        source.contains("2>$null"),
        "hook Windows PowerShell prologue must suppress best-effort setup stderr"
    );
    assert!(
        !source.contains("[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \\\n                     $OutputEncoding = [System.Text.Encoding]::UTF8;"),
        "raw hook OutputEncoding assignments can break ConstrainedLanguage sessions before hook JSON is emitted"
    );
}

#[test]
fn shell_common_decodes_collected_bytes_through_console_decoder() {
    let source = std::fs::read_to_string("src/runtime/tools/builtin/shell_common.rs")
        .expect("read shell_common source");

    assert!(
        source.contains("decode_console_bytes(&bytes)"),
        "PowerShell output collection must use console_decode so Windows GBK output does not become mojibake"
    );
    assert!(
        !source.contains("String::from_utf8_lossy(&bytes).to_string()"),
        "direct UTF-8 lossy decoding regresses zh-CN Windows console output"
    );
}

#[test]
fn context_builder_hides_command_path_probe_windows() {
    let source = std::fs::read_to_string("src/runtime/chat/context_builder.rs")
        .expect("read context_builder source");

    assert!(
        !source.contains("Command::new(\"where.exe\")")
            && !source.contains("std::process::Command::new(\"where.exe\")"),
        "context_builder must not spawn where.exe because it can create a visible conhost flash"
    );
    assert!(
        source.contains("fn detect_windows_command_paths(command: &str)")
            && source.contains("std::env::split_paths(&path_var)")
            && source.contains("windows_command_candidate_names(command)"),
        "Windows command path detection must use in-process PATH/PATHEXT scanning"
    );
}

#[test]
fn dingtalk_bridge_hides_dws_path_probe_windows() {
    let source =
        std::fs::read_to_string("src/connector/dingtalk.rs").expect("read dingtalk source");

    assert!(
        !source.contains("Command::new(\"where.exe\")")
            && !source.contains("std::process::Command::new(\"where.exe\")"),
        "dingtalk bridge must not spawn where.exe because it can create a visible conhost flash"
    );
    assert!(
        source.contains("fn find_windows_command_path(command: &str)")
            && source.contains("std::env::split_paths(&path_var)")
            && source.contains("windows_command_candidate_names(command)"),
        "Windows dws path detection must use in-process PATH/PATHEXT scanning"
    );
}
