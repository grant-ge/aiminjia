//! Architecture review: phase-1 constraints declared in spec §1.9 must remain
//! enforced by `store::validate_phase1_constraints` + organizer-immutable
//! rule in `store::update`. Failing this test means a future PR loosened the
//! rules without a spec update.
//!
//! Exact error-message strings below come from store.rs:355-369 / store.rs:81.
//! If they drift, fix them in both places (and the spec) together.

#[test]
fn store_validates_all_five_phase1_constraints() {
    let source = std::fs::read_to_string("src/runtime/agenda/store.rs").unwrap();
    let assertions = [
        "phase1 constraint: participants.len() must be 1",
        "phase1 constraint: organizer must equal participants[0]",
        "phase1 constraint: override_of must be None",
        "phase1 constraint: rule.by_day / by_month_day must be empty",
        "phase1 constraint: skip_dates only valid when rule is Some",
    ];
    for needle in assertions {
        assert!(
            source.contains(needle),
            "phase-1 constraint missing in store.rs: '{}'",
            needle
        );
    }
}

#[test]
fn organizer_immutable_unless_orphaned_kept() {
    let source = std::fs::read_to_string("src/runtime/agenda/store.rs").unwrap();
    assert!(
        source.contains("organizer can only change when status was Orphaned"),
        "organizer-immutable rule missing in store update path"
    );
}
