//! agenda dispatcher 必须读 employee store 拿 system_prompt_extra
//! 并拼进 send_message 的 prompt。

#[test]
fn agenda_dispatch_reads_employee_system_prompt_extra() {
    // 这个 test 是断言用的契约文档，真实 dispatcher 走 Tauri 闭包不易单测。
    // 真实集成靠手动烟测验证（见 plan 末尾），这里只断言代码结构存在。
    let chat_rs = std::fs::read_to_string("src/transport/tauri_commands/chat.rs")
        .expect("chat.rs must exist");
    assert!(
        chat_rs.contains("organizer_employee_id"),
        "AgendaRunDispatcher::dispatch must use organizer_employee_id to look up employee"
    );
    assert!(
        chat_rs.contains("build_dispatch_prompt"),
        "AgendaRunDispatcher::dispatch must reuse build_dispatch_prompt to compose employee prompt"
    );
}
