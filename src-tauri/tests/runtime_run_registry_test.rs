use app_lib::runtime::{RunId, RuntimeRunRegistry};

#[test]
fn runtime_run_registry_tracks_active_run_identity_and_cancel_state() {
    let registry = RuntimeRunRegistry::new();
    registry.reserve("conv-1", RunId::new("run-1")).unwrap();

    assert_eq!(
        registry.run_id_for_session("conv-1").unwrap().as_str(),
        "run-1"
    );
    assert!(registry.is_session_busy("conv-1"));
    assert!(!registry.is_cancelled("conv-1"));

    registry.cancel("conv-1");
    assert!(registry.is_cancelled("conv-1"));

    let cleared = registry.clear("conv-1").unwrap();
    assert_eq!(cleared.as_str(), "run-1");
    assert!(!registry.is_session_busy("conv-1"));
}
