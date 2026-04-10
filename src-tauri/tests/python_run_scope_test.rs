use app_lib::python::session::{migrate_loaded_keys_to_run_scope, session_key_for_run};
use app_lib::runtime::ids::RunId;

#[test]
fn python_sessions_are_scoped_by_run_id_and_legacy_loaded_keys_are_migrated() {
    let parent_key = session_key_for_run(&RunId::new("run-parent"));
    let child_key = session_key_for_run(&RunId::new("run-child"));
    let migrated = migrate_loaded_keys_to_run_scope("conv-1", &RunId::new("run-child"));
    assert_ne!(parent_key, child_key);
    assert_eq!(migrated.source_prefix, "loaded:conv-1");
    assert_eq!(migrated.target_prefix, "loaded:run-child");
}
