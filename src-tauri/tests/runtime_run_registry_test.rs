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

#[test]
fn cancelled_session_can_attach_again_after_clear() {
    let registry = RuntimeRunRegistry::new();
    registry.reserve("conv-reattach", RunId::new("run-cancelled")).unwrap();
    registry.cancel("conv-reattach");

    let err = registry
        .attach_stream("conv-reattach", "task-stale".to_string())
        .expect_err("stale cancelled slot should reject a new stream before cleanup");
    assert!(err
        .to_string()
        .contains("Conversation cancelled before stream started"));

    registry.clear("conv-reattach");

    let rx = registry
        .attach_stream("conv-reattach", "task-fresh".to_string())
        .expect("cleared session should accept a fresh stream");
    assert!(!*rx.borrow());
    assert!(registry.is_session_busy("conv-reattach"));
}


#[test]
fn clear_for_run_does_not_remove_newer_run_for_same_session() {
    let registry = RuntimeRunRegistry::new();
    registry.reserve("conv-race", RunId::new("run-old")).unwrap();
    registry.clear("conv-race");
    registry.reserve("conv-race", RunId::new("run-new")).unwrap();

    assert_eq!(registry.clear_for_run("conv-race", &RunId::new("run-old")), None);
    assert_eq!(
        registry.run_id_for_session("conv-race").unwrap().as_str(),
        "run-new"
    );

    let cleared = registry
        .clear_for_run("conv-race", &RunId::new("run-new"))
        .expect("matching run should clear");
    assert_eq!(cleared.as_str(), "run-new");
    assert!(!registry.is_session_busy("conv-race"));
}

#[test]
fn reserve_replaces_cancelled_stale_run_for_same_session() {
    let registry = RuntimeRunRegistry::new();
    registry.reserve("conv-replace", RunId::new("run-old")).unwrap();
    registry.cancel("conv-replace");

    registry
        .reserve("conv-replace", RunId::new("run-new"))
        .expect("cancelled stale run should not block a fresh turn");

    assert_eq!(
        registry.run_id_for_session("conv-replace").unwrap().as_str(),
        "run-new"
    );
    assert!(!registry.is_cancelled("conv-replace"));
}
