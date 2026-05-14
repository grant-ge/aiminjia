//! Try each resolver in order. First success wins. If all fail, return the
//! last error so callers see the deepest-fallback diagnostic.

use std::sync::Arc;

use super::{RuntimeDependencyError, RuntimeDependencyResult, RuntimeResolver, WorkspaceDependencies};

#[derive(Clone)]
pub struct ChainResolver {
    resolvers: Vec<Arc<dyn RuntimeResolver>>,
}

impl ChainResolver {
    pub fn new(resolvers: Vec<Arc<dyn RuntimeResolver>>) -> Self {
        Self { resolvers }
    }
}

impl std::fmt::Debug for ChainResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChainResolver")
            .field("len", &self.resolvers.len())
            .finish()
    }
}

impl RuntimeResolver for ChainResolver {
    fn workspace_dependencies(&self) -> RuntimeDependencyResult<WorkspaceDependencies> {
        let mut last_err: Option<RuntimeDependencyError> = None;
        for (idx, resolver) in self.resolvers.iter().enumerate() {
            match resolver.workspace_dependencies() {
                Ok(deps) => {
                    log::info!("[runtime] chain resolved via resolver[{idx}]");
                    return Ok(deps);
                }
                Err(e) => {
                    log::debug!("[runtime] resolver[{idx}] miss: {e}");
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            RuntimeDependencyError::ResolverUnavailable(
                "chain resolver has no entries".to_string(),
            )
        }))
    }
}
