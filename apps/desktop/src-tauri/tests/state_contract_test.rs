//! Contract tests for the supervisor lifecycle state machine.
//!
//! These drive the state machine purely through its public transition surface and assert the
//! observable invariants from local://desktop-contracts.md §State machine: revision increments
//! exactly once per transition, stable kebab-case reasons, idempotency, rejection rules, and a
//! single mutex that coalesces concurrent gestures.

use std::thread;

use pimp_dsh_desktop::state::{Reason, State, Supervisor};
type StateTransition = fn(&Supervisor) -> Result<(), String>;

fn kebab_case(value: &str) -> bool {
    !value.is_empty()
        && value.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Drive the full start path: Stopped → Preflighting → Starting → Ready → Running.
fn bring_to_running(s: &Supervisor) {
    s.start().expect("start");
    s.preflight_complete().expect("preflight_complete");
    s.readiness_received().expect("readiness_received");
    s.mark_running().expect("mark_running");
}

#[test]
fn initial_state_is_stopped_with_revision_zero() {
    let s = Supervisor::new();
    assert_eq!(s.state(), State::Stopped);
    assert_eq!(s.revision(), 0);
    assert_eq!(s.snapshot().revision, 0);
    assert_eq!(s.snapshot().state, State::Stopped);
}

#[test]
fn snapshot_never_carries_a_token() {
    let s = Supervisor::new();
    let value = serde_json::to_value(s.snapshot()).expect("snapshot serializes");
    let obj = value.as_object().expect("snapshot is an object");
    assert_eq!(
        obj["protocolVersion"], 1,
        "protocolVersion must be pinned to 1"
    );
    assert!(
        !obj.contains_key("token"),
        "snapshot must never carry a token"
    );
    assert!(
        !obj.contains_key("runToken"),
        "snapshot must never carry a run token"
    );
    assert!(
        !obj.contains_key("secret"),
        "snapshot must never carry secret material"
    );
}

#[test]
fn start_transitions_stopped_to_preflighting_and_bumps_revision_once() {
    let s = Supervisor::new();
    s.start().expect("start from stopped");
    assert_eq!(s.state(), State::Preflighting);
    assert_eq!(s.revision(), 1);
    assert_eq!(s.snapshot().revision, 1);
}

#[test]
fn start_is_idempotent_across_transitional_and_ready_states() {
    let s = Supervisor::new();
    s.start().expect("first start");
    let rev = s.revision();
    s.start().expect("idempotent start in preflighting");
    assert_eq!(s.state(), State::Preflighting);
    assert_eq!(s.revision(), rev, "idempotent start must not bump revision");

    bring_to_running(&s);
    let rev = s.revision();
    s.start().expect("idempotent start in running");
    assert_eq!(s.state(), State::Running);
    assert_eq!(s.revision(), rev);
}

#[test]
fn full_start_sequence_reaches_running_in_order() {
    let s = Supervisor::new();
    s.start().unwrap();
    assert_eq!(s.state(), State::Preflighting);
    s.preflight_complete().unwrap();
    assert_eq!(s.state(), State::Starting);
    s.readiness_received().unwrap();
    assert_eq!(s.state(), State::Ready);
    s.mark_running().unwrap();
    assert_eq!(s.state(), State::Running);
    assert_eq!(s.revision(), 4);
}

#[test]
fn stop_is_idempotent_in_stopped_terminal_states() {
    let s = Supervisor::new();
    s.stop().expect("stop from stopped is idempotent");
    assert_eq!(s.state(), State::Stopped);
    assert_eq!(s.revision(), 0, "idempotent stop must not bump revision");

    bring_to_running(&s);
    s.stop().unwrap();
    s.child_exited().unwrap();
    assert_eq!(s.state(), State::StoppedGraceful);
    let rev = s.revision();
    s.stop().expect("idempotent stop in stopped-graceful");
    assert_eq!(s.revision(), rev);
}

#[test]
fn cooperative_stop_reaches_graceful_via_child_exit() {
    let s = Supervisor::new();
    bring_to_running(&s);
    s.stop().expect("stop from running");
    assert_eq!(s.state(), State::Stopping);
    s.child_exited().expect("child exit during stopping");
    assert_eq!(s.state(), State::StoppedGraceful);
}

#[test]
fn grace_deadline_escalates_to_forced() {
    let s = Supervisor::new();
    bring_to_running(&s);
    s.stop().unwrap();
    s.grace_deadline().expect("grace deadline during stopping");
    assert_eq!(s.state(), State::StoppedForced);
}

#[test]
fn start_is_rejected_during_stopping() {
    let s = Supervisor::new();
    bring_to_running(&s);
    s.stop().unwrap();
    assert_eq!(s.state(), State::Stopping);
    assert!(s.start().is_err(), "start during stopping must be rejected");
    assert_eq!(s.state(), State::Stopping);
    assert_eq!(s.revision(), 5, "rejected start must not bump revision");
}

#[test]
fn stop_is_rejected_during_transitional_states() {
    let s = Supervisor::new();
    s.start().unwrap();
    assert!(
        s.stop().is_err(),
        "stop during preflighting must be rejected"
    );
    assert_eq!(s.state(), State::Preflighting);

    s.preflight_complete().unwrap();
    assert!(s.stop().is_err(), "stop during starting must be rejected");
    assert_eq!(s.state(), State::Starting);
}

#[test]
fn child_exit_from_running_is_crash() {
    let s = Supervisor::new();
    bring_to_running(&s);
    s.child_exited().expect("child exit from running");
    assert_eq!(s.state(), State::Crashed);
}

#[test]
fn start_failure_from_preflight_is_failed_start() {
    let s = Supervisor::new();
    s.start().unwrap();
    s.start_failed().expect("start failure during preflighting");
    assert_eq!(s.state(), State::FailedStart);
}

#[test]
fn every_transition_increments_revision_exactly_once() {
    let s = Supervisor::new();
    let steps: [StateTransition; 6] = [
        Supervisor::start,
        Supervisor::preflight_complete,
        Supervisor::readiness_received,
        Supervisor::mark_running,
        Supervisor::stop,
        Supervisor::child_exited,
    ];
    let mut expected = 0;
    for step in steps {
        step(&s).expect("transition step");
        expected += 1;
        assert_eq!(
            s.revision(),
            expected,
            "revision must increment exactly once"
        );
    }
    assert_eq!(expected, 6);
}

#[test]
fn reasons_are_stable_and_kebab_case() {
    let a = Supervisor::new();
    a.start().unwrap();
    let b = Supervisor::new();
    b.start().unwrap();

    let reason_a = reason_text(&a.snapshot().reason);
    let reason_b = reason_text(&b.snapshot().reason);
    assert!(
        kebab_case(&reason_a),
        "reason must be kebab-case, got {reason_a:?}"
    );
    assert!(
        kebab_case(&reason_b),
        "reason must be kebab-case, got {reason_b:?}"
    );
    assert_eq!(
        reason_a, reason_b,
        "the same transition must record a stable reason"
    );
}

fn reason_text(reason: &Reason) -> String {
    match serde_json::to_value(reason).expect("reason must serialize") {
        serde_json::Value::String(s) => s,
        other => panic!("reason must serialize to a JSON string, got {other:?}"),
    }
}

#[test]
fn concurrent_starts_coalesce_to_exactly_one_transition() {
    let sup = Supervisor::new();
    let mut handles = Vec::new();
    for _ in 0..16 {
        let s = sup.clone();
        handles.push(thread::spawn(move || s.start()));
    }
    for handle in handles {
        let _ = handle.join().expect("worker thread");
    }
    assert_eq!(sup.state(), State::Preflighting);
    assert_eq!(
        sup.revision(),
        1,
        "16 concurrent starts must coalesce to one transition"
    );
}

#[test]
fn concurrent_start_and_stop_never_corrupt_state() {
    // A mixed burst must land in exactly one of the legal states with a monotonic revision.
    let sup = Supervisor::new();
    let mut handles = Vec::new();
    for i in 0..32 {
        let s = sup.clone();
        handles.push(thread::spawn(move || {
            if i % 2 == 0 {
                let _ = s.start();
            } else {
                let _ = s.stop();
            }
        }));
    }
    for handle in handles {
        handle.join().expect("worker thread");
    }
    let revision = sup.revision();
    // start (1) and stop (no-op from stopped) race: at most one real transition.
    assert!(revision <= 1, "mixed burst produced revision {revision}");
    assert!(
        sup.state() == State::Stopped || sup.state() == State::Preflighting,
        "unexpected state {:?}",
        sup.state()
    );
}
