//! Revisioned supervisor state machine.
//!
//! The state machine is the single authority over lifecycle transitions. It
//! lives behind exactly one mutex; every serialized transition increments
//! `revision` exactly once and records a stable kebab-case `Reason`.
//!
//! The public [`Supervisor`] type is the DesktopTests-facing pure facade: a
//! `Clone` handle over `Arc<Mutex<Inner>>` with `&self` transition methods. The
//! full orchestrator (`crate::supervisor`) drives these same transitions from
//! its platform/bridge threads, then overlays resource-backed snapshot fields.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

pub use crate::types::Snapshot;

/// Closed lifecycle state vocabulary (serialized kebab-case).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Stopped,
    Preflighting,
    Starting,
    Ready,
    Running,
    Stopping,
    StoppedGraceful,
    StoppedForced,
    FailedStart,
    Crashed,
    Unmanaged,
    UpdatePending,
    Updating,
}

/// Stable kebab-case reason recorded for every transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    Idle,
    StartRequested,
    PreflightOk,
    Ready,
    Running,
    StopRequested,
    StoppedGraceful,
    StoppedForced,
    StartFailed,
    ChildExited,
    ChildExitedEarly,
    Unmanaged,
    UpdatePending,
    Updating,
    Reset,
}

/// The state-machine event driving one transition.
#[derive(Clone, Copy, Debug)]
enum LifecycleEvent {
    StartRequested,
    PreflightComplete,
    ReadinessReceived,
    MarkRunning,
    StopRequested,
    ChildExited,
    GraceDeadline,
    StartFailed,
}

struct Inner {
    revision: u64,
    state: State,
    reason: Reason,
    last_transition_at: String,
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Pure, mutex-guarded lifecycle state. `Clone` shares the single mutex.
#[derive(Clone)]
pub struct Supervisor {
    inner: Arc<Mutex<Inner>>,
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl Supervisor {
    /// Create a fresh supervisor in `Stopped` at revision 0.
    pub fn new() -> Self {
        Supervisor {
            inner: Arc::new(Mutex::new(Inner {
                revision: 0,
                state: State::Stopped,
                reason: Reason::Idle,
                last_transition_at: now_rfc3339(),
            })),
        }
    }

    /// Deep copy of the current state as a minimal, contract-valid snapshot.
    pub fn snapshot(&self) -> Snapshot {
        let g = self.inner.lock().expect("state mutex poisoned");
        Snapshot::minimal(g.revision, g.state, g.reason, g.last_transition_at.clone())
    }

    pub fn revision(&self) -> u64 {
        self.inner.lock().expect("state mutex poisoned").revision
    }

    pub fn state(&self) -> State {
        self.inner.lock().expect("state mutex poisoned").state
    }

    pub fn reason(&self) -> Reason {
        self.inner.lock().expect("state mutex poisoned").reason
    }

    pub fn last_transition_at(&self) -> String {
        self.inner
            .lock()
            .expect("state mutex poisoned")
            .last_transition_at
            .clone()
    }

    /// Start: idempotent in preflighting/starting/ready/running; rejected while
    /// stopping or updating. Safe stopped-terminal states begin preflight.
    pub fn start(&self) -> Result<(), String> {
        self.transition(LifecycleEvent::StartRequested).map(|_| ())
    }

    /// Start, reporting whether the `Stopped* → Preflighting` transition
    /// actually happened (`Ok(true)`), atomically with the transition itself so
    /// callers can gate side effects without a TOCTOU race.
    pub fn start_changed(&self) -> Result<bool, String> {
        self.transition(LifecycleEvent::StartRequested)
    }

    pub fn preflight_complete(&self) -> Result<(), String> {
        self.transition(LifecycleEvent::PreflightComplete)
            .map(|_| ())
    }

    pub fn readiness_received(&self) -> Result<(), String> {
        self.transition(LifecycleEvent::ReadinessReceived)
            .map(|_| ())
    }

    pub fn mark_running(&self) -> Result<(), String> {
        self.transition(LifecycleEvent::MarkRunning).map(|_| ())
    }

    /// Stop: idempotent in stopping/stopped-terminal states; from ready/running
    /// begins stopping; from a failed/crashed/unmanaged state acknowledges and
    /// returns to stopped; rejected during preflighting/starting (a mid-start
    /// settles on its own via start failure or the handshake deadline).
    pub fn stop(&self) -> Result<(), String> {
        self.transition(LifecycleEvent::StopRequested).map(|_| ())
    }

    /// Stop, reporting whether the transition actually happened (`Ok(true)`).
    pub fn stop_changed(&self) -> Result<bool, String> {
        self.transition(LifecycleEvent::StopRequested)
    }

    pub fn child_exited(&self) -> Result<(), String> {
        self.transition(LifecycleEvent::ChildExited).map(|_| ())
    }

    pub fn grace_deadline(&self) -> Result<(), String> {
        self.transition(LifecycleEvent::GraceDeadline).map(|_| ())
    }

    pub fn start_failed(&self) -> Result<(), String> {
        self.transition(LifecycleEvent::StartFailed).map(|_| ())
    }

    /// The single serialized transition under the lifecycle mutex.
    /// `Ok(true)` = a real transition (revision bumped); `Ok(false)` = an
    /// idempotent no-op; `Err` = rejected transition (no change).
    fn transition(&self, event: LifecycleEvent) -> Result<bool, String> {
        let mut g = self.inner.lock().expect("state mutex poisoned");
        // Determine outcome under the single lifecycle mutex.
        let outcome: Result<Option<(State, Reason)>, String> = match (g.state, event) {
            // ---- start ----
            (
                State::Stopped
                | State::StoppedGraceful
                | State::StoppedForced
                | State::FailedStart
                | State::Crashed
                | State::Unmanaged,
                LifecycleEvent::StartRequested,
            ) => Ok(Some((State::Preflighting, Reason::StartRequested))),
            (
                State::Preflighting | State::Starting | State::Ready | State::Running,
                LifecycleEvent::StartRequested,
            ) => Ok(None), // idempotent, no revision bump
            (State::Stopping, LifecycleEvent::StartRequested) => {
                Err("start rejected while stopping".to_string())
            }
            (State::UpdatePending | State::Updating, LifecycleEvent::StartRequested) => {
                Err("start rejected while updating".to_string())
            }

            // ---- preflight complete ----
            (State::Preflighting, LifecycleEvent::PreflightComplete) => {
                Ok(Some((State::Starting, Reason::PreflightOk)))
            }
            (_, LifecycleEvent::PreflightComplete) => {
                Err("preflight_complete out of state".to_string())
            }

            // ---- readiness ----
            (State::Starting, LifecycleEvent::ReadinessReceived) => {
                Ok(Some((State::Ready, Reason::Ready)))
            }
            (_, LifecycleEvent::ReadinessReceived) => {
                Err("readiness_received out of state".to_string())
            }

            // ---- running ----
            (State::Ready, LifecycleEvent::MarkRunning) => {
                Ok(Some((State::Running, Reason::Running)))
            }
            (_, LifecycleEvent::MarkRunning) => Err("mark_running out of state".to_string()),

            // ---- stop ----
            (State::Ready | State::Running, LifecycleEvent::StopRequested) => {
                Ok(Some((State::Stopping, Reason::StopRequested)))
            }
            (State::Preflighting | State::Starting, LifecycleEvent::StopRequested) => {
                // A mid-start cannot be cancelled from the stop command: the
                // lifecycle thread owns the start and settles on its own (start
                // failure or the handshake deadline). This avoids a stuck
                // `Stopping` state with no lifecycle thread to empty the Job.
                Err("stop rejected while starting".to_string())
            }
            (
                State::Stopping | State::Stopped | State::StoppedGraceful | State::StoppedForced,
                LifecycleEvent::StopRequested,
            ) => Ok(None), // idempotent
            (
                State::FailedStart | State::Crashed | State::Unmanaged,
                LifecycleEvent::StopRequested,
            ) => Ok(Some((State::Stopped, Reason::StopRequested))),
            (State::UpdatePending | State::Updating, LifecycleEvent::StopRequested) => {
                Err("stop rejected while updating".to_string())
            }

            // ---- child exit ----
            (State::Ready | State::Running, LifecycleEvent::ChildExited) => {
                Ok(Some((State::Crashed, Reason::ChildExited)))
            }
            (State::Preflighting | State::Starting, LifecycleEvent::ChildExited) => {
                Ok(Some((State::FailedStart, Reason::ChildExitedEarly)))
            }
            (State::Stopping, LifecycleEvent::ChildExited) => {
                // Cooperative exit after the whole Job has emptied.
                Ok(Some((State::StoppedGraceful, Reason::StoppedGraceful)))
            }
            (
                State::Stopped
                | State::StoppedGraceful
                | State::StoppedForced
                | State::FailedStart
                | State::Crashed
                | State::Unmanaged,
                LifecycleEvent::ChildExited,
            ) => Ok(None),
            (State::UpdatePending | State::Updating, LifecycleEvent::ChildExited) => {
                Err("child_exited out of state".to_string())
            }

            // ---- grace deadline (forced) ----
            (State::Stopping, LifecycleEvent::GraceDeadline) => {
                Ok(Some((State::StoppedForced, Reason::StoppedForced)))
            }
            (_, LifecycleEvent::GraceDeadline) => Err("grace_deadline out of state".to_string()),

            // ---- start failure ----
            (State::Preflighting | State::Starting, LifecycleEvent::StartFailed) => {
                Ok(Some((State::FailedStart, Reason::StartFailed)))
            }
            (_, LifecycleEvent::StartFailed) => Err("start_failed out of state".to_string()),
        };

        match outcome? {
            Some((next, reason)) => {
                g.revision += 1; // exactly once per serialized transition
                g.state = next;
                g.reason = reason;
                g.last_transition_at = now_rfc3339();
                Ok(true)
            }
            None => Ok(false), // idempotent no-op: revision unchanged
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_and_reason_serialize_kebab_case() {
        assert_eq!(
            serde_json::to_string(&State::StoppedGraceful).unwrap(),
            "\"stopped-graceful\""
        );
        assert_eq!(
            serde_json::to_string(&State::FailedStart).unwrap(),
            "\"failed-start\""
        );
        assert_eq!(
            serde_json::to_string(&Reason::StoppedForced).unwrap(),
            "\"stopped-forced\""
        );
        // Every reason is stable kebab-case [a-z][a-z-]*.
        for r in [
            Reason::Idle,
            Reason::StartRequested,
            Reason::PreflightOk,
            Reason::Ready,
            Reason::Running,
            Reason::StopRequested,
            Reason::StoppedGraceful,
            Reason::StoppedForced,
            Reason::StartFailed,
            Reason::ChildExited,
            Reason::ChildExitedEarly,
            Reason::Unmanaged,
            Reason::UpdatePending,
            Reason::Updating,
            Reason::Reset,
        ] {
            let s = serde_json::to_string(&r).unwrap();
            let s = s.trim_matches('"');
            assert!(
                s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'-' || b.is_ascii_digit()),
                "reason {s:?} not kebab-case"
            );
        }
    }

    #[test]
    fn initial_state_is_stopped_revision_zero() {
        let s = Supervisor::new();
        assert_eq!(s.state(), State::Stopped);
        assert_eq!(s.revision(), 0);
        assert_eq!(s.reason(), Reason::Idle);
    }

    #[test]
    fn start_increments_revision_once() {
        let s = Supervisor::new();
        s.start().unwrap();
        assert_eq!(s.state(), State::Preflighting);
        assert_eq!(s.revision(), 1);
        assert_eq!(s.reason(), Reason::StartRequested);
    }

    #[test]
    fn start_is_idempotent_during_transitional_states() {
        let s = Supervisor::new();
        s.start().unwrap(); // 0 -> preflighting (1)
        s.start().unwrap(); // idempotent
        assert_eq!(s.revision(), 1);
        s.preflight_complete().unwrap(); // -> starting (2)
        s.start().unwrap(); // idempotent
        assert_eq!(s.revision(), 2);
        assert_eq!(s.state(), State::Starting);
    }

    #[test]
    fn start_rejected_while_stopping() {
        let s = Supervisor::new();
        s.start().unwrap();
        s.preflight_complete().unwrap();
        s.readiness_received().unwrap();
        s.mark_running().unwrap();
        s.stop().unwrap(); // -> stopping
        let err = s.start().unwrap_err();
        assert!(err.contains("stopping"));
        assert_eq!(s.state(), State::Stopping);
    }

    #[test]
    fn full_graceful_lifecycle() {
        let s = Supervisor::new();
        s.start().unwrap(); // 1 preflighting
        s.preflight_complete().unwrap(); // 2 starting
        s.readiness_received().unwrap(); // 3 ready
        s.mark_running().unwrap(); // 4 running
        s.stop().unwrap(); // 5 stopping
        s.child_exited().unwrap(); // 6 stopped-graceful
        assert_eq!(s.state(), State::StoppedGraceful);
        assert_eq!(s.reason(), Reason::StoppedGraceful);
        assert_eq!(s.revision(), 6);
    }

    #[test]
    fn forced_stop_is_never_graceful() {
        let s = Supervisor::new();
        s.start().unwrap();
        s.preflight_complete().unwrap();
        s.readiness_received().unwrap();
        s.mark_running().unwrap();
        s.stop().unwrap();
        s.grace_deadline().unwrap(); // forced
        assert_eq!(s.state(), State::StoppedForced);
        assert_eq!(s.reason(), Reason::StoppedForced);
        assert_ne!(s.state(), State::StoppedGraceful);
    }

    #[test]
    fn child_exit_crashes_running() {
        let s = Supervisor::new();
        s.start().unwrap();
        s.preflight_complete().unwrap();
        s.readiness_received().unwrap();
        s.mark_running().unwrap();
        s.child_exited().unwrap();
        assert_eq!(s.state(), State::Crashed);
        assert_eq!(s.reason(), Reason::ChildExited);
    }

    #[test]
    fn child_exit_early_is_failed_start() {
        let s = Supervisor::new();
        s.start().unwrap();
        s.preflight_complete().unwrap();
        s.child_exited().unwrap(); // starting -> failed-start (early)
        assert_eq!(s.state(), State::FailedStart);
        assert_eq!(s.reason(), Reason::ChildExitedEarly);
    }

    #[test]
    fn start_failed_transitions() {
        let s = Supervisor::new();
        s.start().unwrap();
        s.start_failed().unwrap();
        assert_eq!(s.state(), State::FailedStart);
        assert_eq!(s.reason(), Reason::StartFailed);
    }

    #[test]
    fn stop_is_idempotent_in_stopped_terminal() {
        let s = Supervisor::new();
        s.start().unwrap();
        s.preflight_complete().unwrap();
        s.readiness_received().unwrap();
        s.mark_running().unwrap();
        s.stop().unwrap();
        s.grace_deadline().unwrap();
        let rev = s.revision();
        s.stop().unwrap(); // idempotent
        assert_eq!(s.revision(), rev);
        assert_eq!(s.state(), State::StoppedForced);
    }

    #[test]
    fn concurrent_transitions_never_double_increment() {
        use std::sync::Barrier;
        let s = Supervisor::new();
        let barrier = Arc::new(Barrier::new(4));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let s = s.clone();
            let b = barrier.clone();
            handles.push(std::thread::spawn(move || {
                b.wait();
                for _ in 0..100 {
                    // Start is idempotent after the first; the rest are no-ops.
                    let _ = s.start();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(
            s.revision(),
            1,
            "concurrent start must bump revision exactly once"
        );
        assert_eq!(s.state(), State::Preflighting);
    }

    #[test]
    fn restart_from_crashed_is_allowed() {
        let s = Supervisor::new();
        s.start().unwrap();
        s.preflight_complete().unwrap();
        s.readiness_received().unwrap();
        s.mark_running().unwrap();
        s.child_exited().unwrap();
        assert_eq!(s.state(), State::Crashed);
        s.start().unwrap(); // crashed -> preflighting
        assert_eq!(s.state(), State::Preflighting);
    }
}
