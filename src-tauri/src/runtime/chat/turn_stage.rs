//! Turn-stage emitter: single owner of the current `TurnStage` for a turn.
//!
//! Spec: `docs/superpowers/specs/2026-05-17-turn-stages.md` §3 + §8.
//!
//! Wraps a `RuntimeEventBus` + `SessionId` + `RunId` and exposes one method per
//! stage transition. The driver calls these at the 8 transition points listed
//! in spec §8. The emitter also owns the current stage in an `Arc<Mutex>` so
//! a future heartbeat task (PR2) can read it without racing the driver.
//!
//! ## Feature flag (post-PR5)
//!
//! On by default.  Env var `AIJIA_TURN_STAGES=0` (or `false`) is an escape
//! hatch for forcing the legacy behaviour without rebuilding; any other value
//! (or unset) means "on".  When disabled, every emit is a no-op — zero events
//! on the bus, zero cost.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::events::{RunningTool, RuntimeEvent, TurnStage};
use crate::runtime::ids::{RunId, SessionId};
use crate::storage::fs_atomic;

/// Cadence of `TurnHeartbeat` emissions while a turn is in progress.
/// Spec §4.1: 2s — large enough to amortize bus cost, small enough to stay
/// inside the "is it stuck?" user-perception window.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

/// Schema version for the persisted `turn_stage.json` file.  Bump when the
/// file shape changes; readers ignore files with newer versions.
pub const TURN_STAGE_PERSIST_SCHEMA: u32 = 1;

/// On-disk snapshot of an active turn's stage.  Written through every
/// transition + heartbeat tick; deleted by `mark_turn_complete()` at the
/// terminal exit of `run_chat_turn_s4`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PersistedTurnStage {
    pub schema_version: u32,
    pub conversation_id: String,
    pub run_id: String,
    pub stage: TurnStage,
    pub stage_started_at_ms: u64,
    pub turn_started_at_ms: u64,
    pub last_heartbeat_at_ms: u64,
}

/// Read the feature flag.  On by default; `AIJIA_TURN_STAGES=0` (or `false`)
/// forces it off — escape hatch for unblocking edge cases without a rebuild.
/// See module docs.
pub fn turn_stages_enabled() -> bool {
    match std::env::var("AIJIA_TURN_STAGES") {
        Ok(v) if matches!(v.as_str(), "0" | "false" | "FALSE" | "no" | "off") => false,
        _ => true,
    }
}

/// Freestanding emit helper for stage transitions fired from contexts that
/// can't easily hold a `&TurnStageEmitter` — e.g. the closure passed to
/// `prepare_messages_for_llm` that runs `compact_summary` and needs to emit
/// `Compacting` without capturing `&self` from the driver.  Respects the
/// feature flag (no-op when off).
pub async fn emit_oneshot(
    bus: &RuntimeEventBus,
    session_id: SessionId,
    run_id: RunId,
    stage: TurnStage,
) {
    if !turn_stages_enabled() {
        return;
    }
    let now_ms = now_unix_ms();
    if let Err(e) = bus
        .emit(RuntimeEvent::turn_stage_changed(
            session_id, run_id, stage, now_ms,
        ))
        .await
    {
        log::warn!("[turn-stage] emit_oneshot failed: {e}");
    }
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
    turn_started_at_ms: u64,
    /// Spec §5: when set, every transition / heartbeat is written through to
    /// this path.  `mark_turn_complete()` deletes it at terminal exit.  When
    /// None (tests, or PR1/PR2 callers that hadn't wired persistence yet) the
    /// emitter is pure in-memory.
    persist_path: Option<PathBuf>,
}

impl TurnStageEmitter {
    pub fn new(event_bus: RuntimeEventBus, session_id: SessionId, run_id: RunId) -> Self {
        let now = Instant::now();
        let now_ms = now_unix_ms();
        Self {
            event_bus,
            session_id,
            run_id,
            enabled: turn_stages_enabled(),
            current: Arc::new(Mutex::new(CurrentStage {
                stage: TurnStage::Submitted,
                stage_started_at_ms: now_ms,
                stage_started_at_mono: now,
            })),
            turn_started_at_mono: now,
            turn_started_at_ms: now_ms,
            persist_path: None,
        }
    }

    /// Spec §5: enable write-through persistence to `path`.  Builder method
    /// so callers (chat_turn_driver) opt in; tests can leave it off.
    pub fn with_persist_path(mut self, path: PathBuf) -> Self {
        self.persist_path = Some(path);
        self
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

    /// Builder-style override for the feature flag.  Production code never
    /// calls this — it relies on env-var detection — but tests and
    /// PR4 settings plumbing override the value explicitly.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
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
        self.persist_snapshot(&next, now_ms, now_ms);
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

    /// Best-effort write of the current snapshot.  Failures are logged but
    /// never propagated — persistence is a recovery convenience, not a
    /// correctness invariant.
    fn persist_snapshot(
        &self,
        stage: &TurnStage,
        stage_started_at_ms: u64,
        last_heartbeat_at_ms: u64,
    ) {
        let Some(path) = self.persist_path.as_ref() else {
            return;
        };
        let snapshot = PersistedTurnStage {
            schema_version: TURN_STAGE_PERSIST_SCHEMA,
            conversation_id: self.session_id.as_str().to_string(),
            run_id: self.run_id.as_str().to_string(),
            stage: stage.clone(),
            stage_started_at_ms,
            turn_started_at_ms: self.turn_started_at_ms,
            last_heartbeat_at_ms,
        };
        match serde_json::to_vec_pretty(&snapshot) {
            Ok(bytes) => {
                if let Some(parent) = path.parent() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        log::warn!("[turn-stage] mkdir {:?} failed: {e}", parent);
                        return;
                    }
                }
                if let Err(e) = fs_atomic::write_atomic(path, &bytes) {
                    log::warn!("[turn-stage] persist write failed: {e}");
                }
            }
            Err(e) => log::warn!("[turn-stage] persist serialize failed: {e}"),
        }
    }

    /// Terminal-exit cleanup: remove the persisted snapshot so the next
    /// process startup does NOT treat this turn as crashed.  Must be called
    /// at every exit path of `run_chat_turn_s4` that represents a normal
    /// turn termination (success, expected error, cancel).  No-op when
    /// persistence wasn't enabled.
    pub fn mark_turn_complete(&self) {
        let Some(path) = self.persist_path.as_ref() else {
            return;
        };
        match std::fs::remove_file(path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("[turn-stage] persist cleanup failed: {e}"),
        }
    }

    /// Return an RAII guard that calls `mark_turn_complete()` on Drop — so
    /// every ?/panic/cancel exit from the turn driver atomically removes
    /// the on-disk crash sentinel.
    pub fn cleanup_guard(&self) -> CleanupGuard {
        CleanupGuard {
            persist_path: self.persist_path.clone(),
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

    /// Spawn the keep-alive task.  Returns a `HeartbeatGuard` that aborts the
    /// task on Drop, so the heartbeat is guaranteed to stop on every exit path
    /// from the turn driver (Ok, Err, ?, panic, cancel).  When the feature
    /// flag is off this returns a disabled guard with no spawned task.
    pub fn spawn_heartbeat(&self) -> HeartbeatGuard {
        if !self.enabled {
            return HeartbeatGuard { handle: None };
        }
        let event_bus = self.event_bus.clone();
        let session_id = self.session_id.clone();
        let run_id = self.run_id.clone();
        let current = Arc::clone(&self.current);
        let turn_started = self.turn_started_at_mono;
        let persist_path = self.persist_path.clone();
        let turn_started_at_ms = self.turn_started_at_ms;
        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(HEARTBEAT_INTERVAL);
            // Skip the immediate first tick — first heartbeat fires HEARTBEAT_INTERVAL
            // after spawn, not at t=0 (the TurnStageChanged for Submitted has
            // already given the UI its first signal).
            ticker.tick().await;
            loop {
                ticker.tick().await;
                let snapshot = current.lock().await.clone();
                let now = Instant::now();
                let stage_elapsed = now.saturating_duration_since(snapshot.stage_started_at_mono);
                let turn_elapsed = now.saturating_duration_since(turn_started);
                // Refresh persisted last_heartbeat_at_ms (best-effort).  Done
                // before emit so the file is fresh even if the bus is slow.
                if let Some(path) = persist_path.as_ref() {
                    persist_heartbeat_to(
                        path,
                        &session_id,
                        &run_id,
                        &snapshot,
                        turn_started_at_ms,
                    );
                }
                if let Err(e) = event_bus
                    .emit(RuntimeEvent::turn_heartbeat(
                        session_id.clone(),
                        run_id.clone(),
                        stage_elapsed.as_millis() as u64,
                        turn_elapsed.as_millis() as u64,
                    ))
                    .await
                {
                    log::warn!("[turn-stage] heartbeat emit failed: {e}");
                }
            }
        });
        HeartbeatGuard {
            handle: Some(handle),
        }
    }
}

/// Standalone heartbeat-writer used by the spawned task (which can't hold
/// `&self`).  Identical schema to `TurnStageEmitter::persist_snapshot`.
fn persist_heartbeat_to(
    path: &std::path::Path,
    session_id: &SessionId,
    run_id: &RunId,
    snapshot: &CurrentStage,
    turn_started_at_ms: u64,
) {
    let payload = PersistedTurnStage {
        schema_version: TURN_STAGE_PERSIST_SCHEMA,
        conversation_id: session_id.as_str().to_string(),
        run_id: run_id.as_str().to_string(),
        stage: snapshot.stage.clone(),
        stage_started_at_ms: snapshot.stage_started_at_ms,
        turn_started_at_ms,
        last_heartbeat_at_ms: now_unix_ms(),
    };
    let bytes = match serde_json::to_vec_pretty(&payload) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("[turn-stage] heartbeat persist serialize failed: {e}");
            return;
        }
    };
    if let Err(e) = fs_atomic::write_atomic(path, &bytes) {
        log::warn!("[turn-stage] heartbeat persist write failed: {e}");
    }
}

/// RAII handle that deletes the persisted `turn_stage.json` on Drop.
/// Independent of the emitter so the emitter itself can be safely cloned /
/// passed around without triggering cleanup at every drop.  Spec §5.
pub struct CleanupGuard {
    persist_path: Option<PathBuf>,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let Some(path) = self.persist_path.as_ref() else {
            return;
        };
        match std::fs::remove_file(path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("[turn-stage] CleanupGuard drop: remove failed: {e}"),
        }
    }
}

/// RAII handle for the heartbeat task.  Aborts on Drop — guarantees the
/// background task cannot outlive a single turn.  Holding the guard for the
/// entire turn lifetime (and dropping it at function exit) is the contract.
pub struct HeartbeatGuard {
    handle: Option<JoinHandle<()>>,
}

impl HeartbeatGuard {
    /// Stop the heartbeat explicitly.  Idempotent.  Mostly useful in tests
    /// that want to assert "no events after this point".
    pub fn abort(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }

    #[cfg(test)]
    pub fn is_active(&self) -> bool {
        self.handle.is_some()
    }
}

impl Drop for HeartbeatGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::event_bus::RuntimeEventBus;
    use crate::runtime::events::RuntimeEventKind;
    use crate::runtime::ids::{RunId, SessionId};

    /// Post-PR5 the flag defaults ON; tests that want hermetic enabled/disabled
    /// state should use `with_enabled(...)` explicitly so they aren't sensitive
    /// to the ambient env var.
    fn disabled_fixture() -> TurnStageEmitter {
        let bus = RuntimeEventBus::new();
        TurnStageEmitter::new(bus, SessionId::new("s-1"), RunId::new("r-1"))
            .with_enabled(false)
    }

    #[tokio::test]
    async fn disabled_emitter_is_silent() {
        let emitter = disabled_fixture();
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

    #[tokio::test]
    async fn heartbeat_task_disabled_when_flag_off() {
        let emitter = disabled_fixture();
        let guard = emitter.spawn_heartbeat();
        assert!(!guard.is_active(), "guard should be inert when flag is off");
        // Run longer than one heartbeat interval to be sure nothing was emitted.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let recorded = emitter.event_bus.recorded();
        assert!(recorded.is_empty(), "no events expected with flag off");
    }

    /// Wait until at least `expected` heartbeats are recorded, polling every
    /// 5ms.  Caps at `max_wait_ms` to avoid wedging the test suite.  Uses
    /// real time so we don't have to fight tokio's paused-time scheduler in
    /// concert with `Mutex::lock().await` inside the spawned task.
    async fn wait_for_heartbeats(
        emitter: &TurnStageEmitter,
        expected: usize,
        max_wait_ms: u64,
    ) -> usize {
        let started = Instant::now();
        loop {
            let count = emitter
                .event_bus
                .recorded()
                .iter()
                .filter(|e| matches!(e.kind, RuntimeEventKind::TurnHeartbeat { .. }))
                .count();
            if count >= expected {
                return count;
            }
            if started.elapsed().as_millis() as u64 > max_wait_ms {
                return count;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn heartbeat_task_emits_at_fixed_cadence() {
        // Use a short interval for the test by reaching directly into the
        // spawn helper logic — but since HEARTBEAT_INTERVAL is a module
        // const, instead we accept ~2 real-time seconds of wait and budget
        // 5s max.  Test stays under typical CI budget.
        let bus = RuntimeEventBus::new();
        let mut emitter = TurnStageEmitter::new(bus, SessionId::new("s-1"), RunId::new("r-1"));
        emitter.enabled = true;
        let _guard = emitter.spawn_heartbeat();
        // Skip-first means first heartbeat lands at t≈2s.  Wait up to 5s for
        // at least one heartbeat.
        let got = wait_for_heartbeats(&emitter, 1, 5_000).await;
        assert!(got >= 1, "expected ≥1 heartbeat within 5s, got {got}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn heartbeat_guard_aborts_task_on_drop() {
        let bus = RuntimeEventBus::new();
        let mut emitter = TurnStageEmitter::new(bus, SessionId::new("s-1"), RunId::new("r-1"));
        emitter.enabled = true;
        {
            let _guard = emitter.spawn_heartbeat();
            // Let the task spin up + reach its first await.
            tokio::time::sleep(Duration::from_millis(50)).await;
        } // guard dropped here → task aborted
        let before = emitter.event_bus.recorded().len();
        // Wait longer than HEARTBEAT_INTERVAL; nothing new should land.
        tokio::time::sleep(HEARTBEAT_INTERVAL + Duration::from_millis(200)).await;
        let after = emitter.event_bus.recorded().len();
        assert_eq!(before, after, "heartbeat task must stop after guard drop");
    }

    // ── PR4 persistence + recovery ────────────────────────────────────────

    #[tokio::test]
    async fn transition_writes_turn_stage_json_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("turn_stages").join("conv-1.json");
        let bus = RuntimeEventBus::new();
        let emitter =
            TurnStageEmitter::new(bus, SessionId::new("conv-1"), RunId::new("run-1"))
                .with_enabled(true)
                .with_persist_path(path.clone());

        emitter.submitted().await;
        emitter
            .tools_started(0, vec![running_helper("Bash", "tc-1")])
            .await;

        let raw = std::fs::read(&path).expect("turn_stage.json should exist after transitions");
        let parsed: PersistedTurnStage = serde_json::from_slice(&raw).expect("parse");
        assert_eq!(parsed.schema_version, TURN_STAGE_PERSIST_SCHEMA);
        assert_eq!(parsed.conversation_id, "conv-1");
        assert_eq!(parsed.run_id, "run-1");
        match parsed.stage {
            TurnStage::Tools { iteration, running, .. } => {
                assert_eq!(iteration, 0);
                assert_eq!(running[0].tool_name, "Bash");
            }
            other => panic!("expected Tools, got {other:?}"),
        }
        assert!(parsed.stage_started_at_ms >= parsed.turn_started_at_ms);
    }

    #[tokio::test]
    async fn cleanup_guard_removes_turn_stage_json_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("turn_stages").join("conv-x.json");
        let bus = RuntimeEventBus::new();
        let emitter =
            TurnStageEmitter::new(bus, SessionId::new("conv-x"), RunId::new("run-x"))
                .with_enabled(true)
                .with_persist_path(path.clone());
        emitter.submitted().await;
        assert!(path.exists(), "transition should produce file");
        {
            let _guard = emitter.cleanup_guard();
        } // guard drops → file removed
        assert!(!path.exists(), "CleanupGuard drop should remove file");
    }

    fn running_helper(name: &str, id: &str) -> RunningTool {
        RunningTool {
            tool_name: name.to_string(),
            tool_call_id: id.to_string(),
            started_at_ms: 0,
        }
    }
}
