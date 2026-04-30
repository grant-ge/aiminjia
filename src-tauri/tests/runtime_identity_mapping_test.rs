use app_lib::runtime::identity::{IdentityMapping, RuntimeIdentity};
use app_lib::runtime::ids::{RunId, SessionId};

#[test]
fn phase1_reuses_legacy_conversation_id_as_session_id() {
    let mapping = IdentityMapping::from_legacy_conversation_id("conv-1".to_string());
    assert_eq!(mapping.session_id.as_str(), "conv-1");
    assert_eq!(mapping.legacy_conversation_id.as_deref(), Some("conv-1"));
}

#[test]
fn runtime_identity_uses_session_id_as_primary_key() {
    let identity = RuntimeIdentity::new(SessionId::new("conv-1"), RunId::new("run-1"));
    assert_eq!(identity.session_id().as_str(), "conv-1");
    assert_eq!(identity.run_id().as_str(), "run-1");
}
