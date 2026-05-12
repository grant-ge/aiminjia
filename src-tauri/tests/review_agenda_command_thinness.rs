//! Architecture review: tauri_commands/agenda.rs must be thin (no business logic).
//! Each #[tauri::command] body must only validate / convert / call store|dispatcher.

#[test]
fn agenda_commands_only_delegate_to_store_or_dispatcher() {
    let source = std::fs::read_to_string("src/transport/tauri_commands/agenda.rs").unwrap();
    let lines: Vec<&str> = source.lines().collect();
    let mut in_command = false;
    let mut current_fn = String::new();
    let mut body_lines: Vec<String> = Vec::new();

    for line in &lines {
        if line.contains("#[tauri::command]") {
            in_command = true;
            continue;
        }
        if in_command && line.contains("pub async fn ") {
            current_fn = line.trim().to_string();
            body_lines.clear();
            continue;
        }
        if in_command && line.starts_with("}") && !line.contains("=>") {
            // 命令体结束，断言函数体未超过 30 行（薄转发预算）
            assert!(
                body_lines.len() < 30,
                "command `{}` body has {} lines (limit 30, business logic should be in store/runtime)",
                current_fn,
                body_lines.len()
            );
            in_command = false;
            current_fn.clear();
            continue;
        }
        if in_command && !current_fn.is_empty() {
            body_lines.push(line.to_string());
        }
    }
}
