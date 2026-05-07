//! Intent-test review: spec §6 signature narrowing for PR-2 closure.
//!
//! These tests lock the deviations from the original spec §6 table:
//! - `run_agenda_item_now` returns `String` (occurrence_id), not `Occurrence`.
//! - `list_agenda_occurrences` takes `(item_id, limit)`, no `before` cursor.
//!
//! Drift in either layer (Rust command return / TS invoke wrapper) fails this
//! test so any future re-widening is intentional, not accidental.

#[test]
fn run_agenda_item_now_backend_returns_string_occurrence_id() {
    let source =
        std::fs::read_to_string("src/transport/tauri_commands/agenda.rs").unwrap();
    let (_, after) = source
        .split_once("pub async fn run_agenda_item_now(")
        .expect("run_agenda_item_now command must exist in agenda.rs");
    let signature_end = after
        .find('{')
        .expect("run_agenda_item_now signature must end with body");
    let signature = &after[..signature_end];
    assert!(
        signature.contains("-> Result<String, String>"),
        "run_agenda_item_now must return Result<String, String>, got signature: {}",
        signature
    );
}

#[test]
fn run_agenda_item_now_frontend_wrapper_returns_promise_string() {
    let source = std::fs::read_to_string("../src/lib/tauri.ts").unwrap();
    let (_, after) = source
        .split_once("export function runAgendaItemNow(")
        .expect("runAgendaItemNow wrapper must exist in src/lib/tauri.ts");
    let signature_end = after
        .find('{')
        .expect("runAgendaItemNow signature must end with body");
    let signature = &after[..signature_end];
    assert!(
        signature.contains("Promise<string>"),
        "runAgendaItemNow must return Promise<string>, got signature: {}",
        signature
    );
}

#[test]
fn list_agenda_occurrences_backend_takes_only_item_id_and_limit() {
    let source =
        std::fs::read_to_string("src/transport/tauri_commands/agenda.rs").unwrap();
    let (_, after) = source
        .split_once("pub async fn list_agenda_occurrences(")
        .expect("list_agenda_occurrences command must exist in agenda.rs");
    let signature_end = after
        .find('{')
        .expect("list_agenda_occurrences signature must end with body");
    let signature = &after[..signature_end];
    assert!(
        signature.contains("item_id: String"),
        "list_agenda_occurrences must accept item_id: String, got signature: {}",
        signature
    );
    assert!(
        signature.contains("limit: Option<usize>"),
        "list_agenda_occurrences must accept limit: Option<usize>, got signature: {}",
        signature
    );
    assert!(
        !signature.contains("before"),
        "list_agenda_occurrences must NOT accept a `before` cursor in PR-2 (deferred to phase-2), got signature: {}",
        signature
    );
}

#[test]
fn list_agenda_occurrences_frontend_wrapper_takes_only_item_id_and_limit() {
    let source = std::fs::read_to_string("../src/lib/tauri.ts").unwrap();
    let (_, after) = source
        .split_once("export function listAgendaOccurrences(")
        .expect("listAgendaOccurrences wrapper must exist in src/lib/tauri.ts");
    let signature_end = after
        .find('{')
        .expect("listAgendaOccurrences signature must end with body");
    let signature = &after[..signature_end];
    assert!(
        signature.contains("itemId: string"),
        "listAgendaOccurrences must accept itemId: string, got signature: {}",
        signature
    );
    assert!(
        signature.contains("limit?: number"),
        "listAgendaOccurrences must accept limit?: number, got signature: {}",
        signature
    );
    assert!(
        !signature.contains("before"),
        "listAgendaOccurrences must NOT accept a `before` cursor in PR-2 (deferred to phase-2), got signature: {}",
        signature
    );
}
