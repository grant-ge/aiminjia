use std::sync::{Arc, Mutex};

use anyhow::Result;

use crate::runtime_audit::trace_capture::{CapturedEvent, CapturedTrace};
use crate::transport::runtime_host::RuntimeHost;

#[derive(Default)]
pub struct RecordingRuntimeHost {
    events: Mutex<Vec<CapturedEvent>>,
}

impl RecordingRuntimeHost {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn trace(&self) -> CapturedTrace {
        CapturedTrace::new(self.events.lock().unwrap().clone())
    }
}

impl RuntimeHost for RecordingRuntimeHost {
    fn emit_legacy_event(&self, name: &str, payload: serde_json::Value) -> Result<()> {
        self.events.lock().unwrap().push(CapturedEvent {
            name: name.to_string(),
            payload,
        });
        Ok(())
    }
}

#[derive(Default)]
pub struct NoopRuntimeHost;

impl RuntimeHost for NoopRuntimeHost {
    fn emit_legacy_event(&self, _name: &str, _payload: serde_json::Value) -> Result<()> {
        Ok(())
    }
}
