use anyhow::Result;

#[derive(Clone, Debug, Default)]
pub struct PythonRunArtifacts {
    pub loaded_manifest_path: Option<String>,
    pub analysis_snapshot_path: Option<String>,
    pub precompute_cache_paths: Vec<String>,
    pub generated_artifact_refs: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct PythonRecoveryInput {
    pub loaded_manifest_path: Option<String>,
    pub analysis_snapshot_path: Option<String>,
    pub precompute_cache_paths: Vec<String>,
    pub generated_artifact_refs: Vec<String>,
    pub degraded_restore: bool,
}

pub fn build_python_recovery_input_from_run_artifacts(
    artifacts: &PythonRunArtifacts,
) -> Result<PythonRecoveryInput> {
    let degraded_restore =
        artifacts.loaded_manifest_path.is_none() || artifacts.analysis_snapshot_path.is_none();
    Ok(PythonRecoveryInput {
        loaded_manifest_path: artifacts.loaded_manifest_path.clone(),
        analysis_snapshot_path: artifacts.analysis_snapshot_path.clone(),
        precompute_cache_paths: artifacts.precompute_cache_paths.clone(),
        generated_artifact_refs: artifacts.generated_artifact_refs.clone(),
        degraded_restore,
    })
}
