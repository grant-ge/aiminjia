use std::sync::{Arc, Mutex};

use anyhow::Result;

use crate::runtime::agent::{
    AgentNameRegistry, CancellationRegistry, InboxRegistry, LeadIdleSupervisor, TeamRegistry,
};
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

    fn team_registry(&self) -> Arc<TeamRegistry> {
        unimplemented!("test mock — call site doesn't exercise team_registry")
    }

    fn agent_names(&self) -> Arc<AgentNameRegistry> {
        unimplemented!("test mock — call site doesn't exercise agent_names")
    }

    fn inbox_registry(&self) -> Arc<InboxRegistry> {
        unimplemented!("test mock — call site doesn't exercise inbox_registry")
    }

    fn lead_idle_supervisor(&self) -> Arc<LeadIdleSupervisor> {
        unimplemented!("test mock — call site doesn't exercise lead_idle_supervisor")
    }

    fn cancellation_registry(&self) -> Arc<CancellationRegistry> {
        unimplemented!("test mock — call site doesn't exercise cancellation_registry")
    }
}

#[derive(Default)]
pub struct NoopRuntimeHost;

impl RuntimeHost for NoopRuntimeHost {
    fn emit_legacy_event(&self, _name: &str, _payload: serde_json::Value) -> Result<()> {
        Ok(())
    }

    fn team_registry(&self) -> Arc<TeamRegistry> {
        unimplemented!("test mock — call site doesn't exercise team_registry")
    }

    fn agent_names(&self) -> Arc<AgentNameRegistry> {
        unimplemented!("test mock — call site doesn't exercise agent_names")
    }

    fn inbox_registry(&self) -> Arc<InboxRegistry> {
        unimplemented!("test mock — call site doesn't exercise inbox_registry")
    }

    fn lead_idle_supervisor(&self) -> Arc<LeadIdleSupervisor> {
        unimplemented!("test mock — call site doesn't exercise lead_idle_supervisor")
    }

    fn cancellation_registry(&self) -> Arc<CancellationRegistry> {
        unimplemented!("test mock — call site doesn't exercise cancellation_registry")
    }
}
