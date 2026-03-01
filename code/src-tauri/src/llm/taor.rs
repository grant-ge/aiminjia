//! TAOR (Think → Act → Observe → Repeat) phase tracking for the agent loop.
//!
//! Emits `agent:phase` events to the frontend so the UI can display
//! real-time status ("正在思考..." / "正在执行..." / "正在整理...").
//!
//! Controlled by `enable_taor_tracking` in AppSettings.
//! When disabled, all methods are no-ops with zero overhead.

use serde::Serialize;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

/// The three phases of a single TAOR iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentPhase {
    Think,
    Act,
    Observe,
}

/// Event payload sent to the frontend via `agent:phase`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseEvent {
    pub conversation_id: String,
    pub iteration: usize,
    pub phase: AgentPhase,
    pub prev_phase_duration_ms: u64,
    pub tool_names: Vec<String>,
    pub max_iterations: usize,
}

/// Lightweight wrapper that emits TAOR phase transitions.
///
/// When `enabled` is false, every method returns immediately —
/// no `Instant::now()`, no serialization, no event emission.
pub struct PhaseTracker {
    enabled: bool,
    conversation_id: String,
    app: AppHandle,
    phase_start: Option<Instant>,
    iteration: usize,
    max_iterations: usize,
}

impl PhaseTracker {
    /// Create a new tracker. If `enabled` is false, all methods are no-ops.
    pub fn new(
        enabled: bool,
        conversation_id: String,
        app: AppHandle,
        max_iterations: usize,
    ) -> Self {
        Self {
            enabled,
            conversation_id,
            app,
            phase_start: None,
            iteration: 0,
            max_iterations,
        }
    }

    /// Advance to the next iteration (called at the top of `for iteration in ..`).
    pub fn next_iteration(&mut self, iteration: usize) {
        if !self.enabled {
            return;
        }
        self.iteration = iteration;
    }

    /// Enter the Think phase (LLM streaming).
    pub fn think(&mut self) {
        if !self.enabled {
            return;
        }
        self.emit(AgentPhase::Think, Vec::new());
    }

    /// Enter the Act phase (tool execution).
    pub fn act(&mut self, tool_names: Vec<String>) {
        if !self.enabled {
            return;
        }
        self.emit(AgentPhase::Act, tool_names);
    }

    /// Enter the Observe phase (processing tool results).
    pub fn observe(&mut self) {
        if !self.enabled {
            return;
        }
        self.emit(AgentPhase::Observe, Vec::new());
    }

    /// Emit the phase event and reset the phase timer.
    fn emit(&mut self, phase: AgentPhase, tool_names: Vec<String>) {
        let prev_duration_ms = self
            .phase_start
            .map(|s| s.elapsed().as_millis() as u64)
            .unwrap_or(0);

        let event = PhaseEvent {
            conversation_id: self.conversation_id.clone(),
            iteration: self.iteration,
            phase,
            prev_phase_duration_ms: prev_duration_ms,
            tool_names,
            max_iterations: self.max_iterations,
        };

        let _ = self.app.emit("agent:phase", &event);

        self.phase_start = Some(Instant::now());
    }
}
