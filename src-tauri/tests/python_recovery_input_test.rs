use app_lib::runtime::agent::python_recovery::{
    build_python_recovery_input_from_run_artifacts, PythonRunArtifacts,
};

#[test]
fn builds_python_recovery_input_from_run_artifacts() {
    let artifacts = PythonRunArtifacts {
        loaded_manifest_path: Some("artifacts/run-1/loaded-files.json".into()),
        analysis_snapshot_path: Some("artifacts/run-1/analysis.json".into()),
        generated_artifact_refs: vec!["artifacts/run-1/output.csv".into()],
    };
    let recovery = build_python_recovery_input_from_run_artifacts(&artifacts).unwrap();
    assert_eq!(
        recovery.loaded_manifest_path.as_deref(),
        Some("artifacts/run-1/loaded-files.json")
    );
    assert_eq!(recovery.generated_artifact_refs.len(), 1);
}
