//! Architecture review: agenda dispatcher must use SessionId / RunId from
//! runtime/ids (spec §4.4). Locks in the rule that future refactors don't
//! drop run_id off Occurrence or replace `RunId::new` with raw strings.

#[test]
fn occurrence_struct_uses_session_id_and_run_id() {
    let source = std::fs::read_to_string("src/runtime/agenda/occurrence.rs").unwrap();
    assert!(
        source.contains("session_id: SessionId"),
        "Occurrence must record session_id: SessionId"
    );
    assert!(
        source.contains("run_id: RunId"),
        "Occurrence must record run_id: RunId"
    );
    assert!(
        source.contains("use crate::runtime::ids::"),
        "Occurrence must import ids from runtime::ids module"
    );
}

#[test]
fn agenda_dispatcher_wires_run_id_into_occurrence() {
    let chat = std::fs::read_to_string("src/transport/tauri_commands/chat.rs").unwrap();
    let in_impl = chat
        .split("impl crate::runtime::agenda::AgendaRunDispatcher")
        .nth(1)
        .expect("AgendaRunDispatcher impl block not found");
    assert!(
        in_impl.contains("RunId::new"),
        "AgendaRunDispatcher impl must construct RunId explicitly"
    );
    assert!(
        in_impl.contains("session_id"),
        "AgendaRunDispatcher impl must record session_id on Occurrence"
    );
}
