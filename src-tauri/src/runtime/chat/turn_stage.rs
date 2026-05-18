//! Turn-stage emitter: single owner of the current `TurnStage` for a turn.
//!
//! Spec: `docs/superpowers/specs/2026-05-17-turn-stages.md` §3 + §8.
//!
//! Wraps a `RuntimeEventBus` + `SessionId` + `RunId` and exposes one method per
//! stage transition. The driver calls these at the 8 transition points listed
//! in spec §8. The emitter also owns the current stage in an `Arc<Mutex>` so
//! a future heartbeat task (PR2) can read it without racing the driver.
//!
//! ## Feature flag (PR1 → PR5)
//!
//! Controlled by env var `AIJIA_TURN_STAGES` (`"1"` / `"true"` → on; default
//! off). Settings.json plumbing is deferred to PR4; PR5 flips the default to
//! always-on and removes the env-var branch.
//!
//! When disabled, every emit is a no-op — zero events on the bus, zero cost.

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;

use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::events::{RunningTool, RuntimeEvent, TurnStage};
use crate::runtime::ids::{RunId, SessionId};

/// Read the dogfood feature flag.  See module docs.
pub fn turn_stages_enabled() -> bool {
    std::env::var("AIJIA_TURN_STAGES")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes"))
        .unwrap_or(false)
}

/// Wall-clock milliseconds since the unix epoch.  We use this instead of
/// `Instant` for stage_started_at_ms because the frontend needs a comparable
/// timestamp to compute "已 12s" elapsed labels relative to its own clock.
fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Shared snapshot of the current stage + its monotonic start instant.
/// Heartbeat task (PR2) reads this to derive `stage_elapsed_ms`.
#[derive(Clone, Debug)]
pub struct CurrentStage {
    pub stage: TurnStage,
    pub stage_started_at_ms: u64,
    pub stage_started_at_mono: Instant,
}

/// Stage emitter.  Cheap to construct, intended to live for the duration of
/// one turn.  All `emit_*` calls are async because the bus is async.
pub struct TurnStageEmitter {
    event_bus: RuntimeEventBus,
    session_id: SessionId,
    run_id: RunId,
    enabled: bool,
    current: Arc<Mutex<CurrentStage>>,
    turn_started_at_mono: Instant,
}

impl TurnStageEmitter {
    pub fn new(event_bus: RuntimeEventBus, session_id: SessionId, run_id: RunId) -> Self {
        let now = Instant::now();
        Self {
            event_bus,
            session_id,
            run_id,
            enabled: turn_stages_enabled(),
            current: Arc::new(Mutex::new(CurrentStage {
                stage: TurnStage::Submitted,
                stage_started_at_ms: now_unix_ms(),
                stage_started_at_mono: now,
            })),
            turn_started_at_mono: now,
        }
    }

    /// Returns a clone of the shared current-stage cell.  PR2's heartbeat task
    /// will hold this to read the latest stage without re-entering the driver.
    pub fn current_handle(&self) -> Arc<Mutex<CurrentStage>> {
        Arc::clone(&self.current)
    }

    pub fn turn_started_at(&self) -> Instant {
        self.turn_started_at_mono
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn transition(&self, next: TurnStage) {
        if !self.enabled {
            return;
        }
        let now_ms = now_unix_ms();
        let now_mono = Instant::now();
        {
            let mut current = self.current.lock().await;
            current.stage = next.clone();
            current.stage_started_at_ms = now_ms;
            current.stage_started_at_mono = now_mono;
        }
        if let Err(e) = self
            .event_bus
            .emit(RuntimeEvent::turn_stage_changed(
                self.session_id.clone(),
                self.run_id.clone(),
                next,
                now_ms,
            ))
            .await
        {
            log::warn!("[turn-stage] emit TurnStageChanged failed: {e}");
        }
    }

    pub async fn submitted(&self) {
        self.transition(TurnStage::Submitted).await;
    }

    pub async fn waiting_llm(&self, iteration: u32) {
        self.transition(TurnStage::WaitingLlm { iteration }).await;
    }

    pub async fn streaming(&self, iteration: u32) {
        self.transition(TurnStage::Streaming { iteration }).await;
    }

    pub async fn tools_started(&self, iteration: u32, running: Vec<RunningTool>) {
        self.transition(TurnStage::Tools {
            iteration,
            running,
            completed_in_batch: 0,
        })
        .await;
    }

    pub async fn waiting_permission(&self, tool_name: String, tool_call_id: String) {
        self.transition(TurnStage::WaitingPermission {
            tool_name,
            tool_call_id,
        })
        .await;
    }

    pub async fn waiting_interaction(&self, interaction_kind: String, interaction_id: String) {
        self.transition(TurnStage::WaitingInteraction {
            interaction_kind,
            interaction_id,
        })
        .await;
    }

    pub async fn compacting(&self) {
        self.transition(TurnStage::Compacting).await;
    }

    pub async fn completing(&self) {
        self.transition(TurnStage::Completing).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::event_bus::RuntimeEventBus;
    use crate::runtime::events::RuntimeEventKind;
    use crate::runtime::ids::{RunId, SessionId};

    fn fixture() -> TurnStageEmitter {
        let bus = RuntimeEventBus::new();
        TurnStageEmitter::new(bus, SessionId::new("s-1"), RunId::new("r-1"))
    }

    #[tokio::test]
    async fn disabled_emitter_is_silent() {
        // env var is not set in tests by default → disabled
        let emitter = fixture();
        emitter.submitted().await;
        emitter.waiting_llm(0).await;
        let recorded = emitter.event_bus.recorded();
        let stage_events: Vec<_> = recorded
            .iter()
            .filter(|e| matches!(e.kind, RuntimeEventKind::TurnStageChanged { .. }))
            .collect();
        assert!(stage_events.is_empty(), "expected no TurnStageChanged when flag off");
    }

    #[tokio::test]
    async fn enabled_emitter_records_transition() {
        // Scope the env var to this test only; other tests run in parallel and
        // would inherit a process-wide flag set.  We mutate via std::env::set_var
        // and a custom enabled override.
        let bus = RuntimeEventBus::new();
        let mut emitter = TurnStageEmitter::new(bus, SessionId::new("s-1"), RunId::new("r-1"));
        emitter.enabled = true;
        emitter.streaming(2).await;
        let recorded = emitter.event_bus.recorded();
        assert_eq!(recorded.len(), 1);
        match &recorded[0].kind {
            RuntimeEventKind::TurnStageChanged { stage, .. } => match stage {
                TurnStage::Streaming { iteration } => assert_eq!(*iteration, 2),
                other => panic!("expected Streaming, got {other:?}"),
            },
            other => panic!("expected TurnStageChanged, got {other:?}"),
        }
    }
}
