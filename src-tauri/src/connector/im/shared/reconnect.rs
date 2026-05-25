//! Reconnect backoff helper — staged for use in PR5.
//!
//! PR2 only stages this helper. The inline `delay_secs` arithmetic in
//! `dingtalk_stream::run_with_retry` is NOT replaced here; that substitution
//! happens in PR5 when the IM connector trait is wired up.

use std::time::Duration;

/// Fixed-ladder reconnect backoff state for connector reconnect loops.
///
/// Walks through a fixed schedule of delays (default: `[5s, 15s, 30s, 60s]`),
/// returning each entry in order and then capping at the last entry on
/// subsequent calls. Call [`reset`] on a successful connection to restart from
/// the first step.
///
/// The schedule comes from the Phase 0 spec §3 — see
/// `docs/superpowers/specs/2026-05-18-im-connector-trait-phase0-design.md`.
#[derive(Debug, Clone)]
pub struct ReconnectBackoff {
    schedule: Vec<Duration>,
    idx: usize,
}

impl ReconnectBackoff {
    /// Construct with the spec default schedule `[5s, 15s, 30s, 60s]`.
    pub fn default_schedule() -> Self {
        Self {
            schedule: vec![
                Duration::from_secs(5),
                Duration::from_secs(15),
                Duration::from_secs(30),
                Duration::from_secs(60),
            ],
            idx: 0,
        }
    }

    /// Construct with a custom schedule. Panics if `schedule` is empty.
    pub fn with_schedule(schedule: Vec<Duration>) -> Self {
        assert!(
            !schedule.is_empty(),
            "ReconnectBackoff schedule must be non-empty"
        );
        Self { schedule, idx: 0 }
    }

    /// Return the current step's delay, then advance the index (capping at
    /// the last entry so repeated calls keep returning the maximum delay).
    pub fn next_delay(&mut self) -> Duration {
        let last = self.schedule.len() - 1;
        let delay = self.schedule[self.idx.min(last)];
        if self.idx < last {
            self.idx += 1;
        }
        delay
    }

    /// Reset the backoff to the first step (call after a successful connection).
    pub fn reset(&mut self) {
        self.idx = 0;
    }

    /// Return the current step's delay without advancing (test helper).
    #[cfg(test)]
    pub fn peek(&self) -> Duration {
        let last = self.schedule.len() - 1;
        self.schedule[self.idx.min(last)]
    }
}

impl Default for ReconnectBackoff {
    fn default() -> Self {
        Self::default_schedule()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_advances_5_15_30_60_then_caps() {
        let mut b = ReconnectBackoff::default_schedule();
        assert_eq!(b.next_delay(), Duration::from_secs(5));
        assert_eq!(b.next_delay(), Duration::from_secs(15));
        assert_eq!(b.next_delay(), Duration::from_secs(30));
        assert_eq!(b.next_delay(), Duration::from_secs(60));
        // capped at last entry
        assert_eq!(b.next_delay(), Duration::from_secs(60));
        assert_eq!(b.next_delay(), Duration::from_secs(60));
    }

    #[test]
    fn reset_returns_to_5s() {
        let mut b = ReconnectBackoff::default_schedule();
        b.next_delay(); // 5
        b.next_delay(); // 15
        b.reset();
        assert_eq!(b.peek(), Duration::from_secs(5));
    }
}
