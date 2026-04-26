#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeArtifactProviderKind {
    OfficialSource,
    RenlijiaBundle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeArtifactProviderPolicy {
    kind: RuntimeArtifactProviderKind,
}

impl RuntimeArtifactProviderPolicy {
    pub fn new(kind: RuntimeArtifactProviderKind) -> Self {
        Self { kind }
    }

    pub fn production_default() -> Self {
        Self::new(RuntimeArtifactProviderKind::RenlijiaBundle)
    }

    pub fn kind(&self) -> RuntimeArtifactProviderKind {
        self.kind
    }

    pub fn requires_manifest_url(&self) -> bool {
        true
    }

    pub fn allows_hardcoded_upstream_urls(&self) -> bool {
        false
    }
}
