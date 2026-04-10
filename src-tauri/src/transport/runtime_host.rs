use anyhow::Result;

pub trait RuntimeHost: Send + Sync {
    fn emit_legacy_event(&self, name: &str, payload: serde_json::Value) -> Result<()>;
}
