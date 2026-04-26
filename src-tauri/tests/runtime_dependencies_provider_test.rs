use app_lib::runtime::dependencies::{RuntimeArtifactProviderKind, RuntimeArtifactProviderPolicy};

#[test]
fn official_source_policy_requires_manifest_and_disallows_hardcoded_urls() {
    let policy = RuntimeArtifactProviderPolicy::new(RuntimeArtifactProviderKind::OfficialSource);

    assert_eq!(policy.kind(), RuntimeArtifactProviderKind::OfficialSource);
    assert!(policy.requires_manifest_url());
    assert!(!policy.allows_hardcoded_upstream_urls());
}

#[test]
fn renlijia_bundle_policy_requires_manifest_and_disallows_hardcoded_urls() {
    let policy = RuntimeArtifactProviderPolicy::new(RuntimeArtifactProviderKind::RenlijiaBundle);

    assert_eq!(policy.kind(), RuntimeArtifactProviderKind::RenlijiaBundle);
    assert!(policy.requires_manifest_url());
    assert!(!policy.allows_hardcoded_upstream_urls());
}

#[test]
fn production_default_uses_renlijia_bundle() {
    let policy = RuntimeArtifactProviderPolicy::production_default();

    assert_eq!(policy.kind(), RuntimeArtifactProviderKind::RenlijiaBundle);
}
