//! Windows lifecycle contract tests: the unnamed kill-on-close Job Object, assign-before-resume,
//! and tree teardown without PID lookup.
//!
//! These use real Windows primitives (CreateProcessW with CREATE_SUSPENDED, AssignProcessToJobObject,
//! TerminateJobObject, KILL_ON_JOB_CLOSE) against the controlled fixture child + grandchild binaries.

#![cfg(windows)]

use std::ffi::OsString;
use std::time::Duration;

use pimp_dsh_desktop::compatibility::LaunchSpec;
use pimp_dsh_desktop::job::{ChildGuard, Job};

mod common;
use common::{fixture_child, fixture_grandchild};

/// A launch spec that spawns the fixture child which, once resumed, spawns the fixture grandchild
/// and then sleeps 30 s. Both remain alive until the Job is torn down.
fn fixture_spec() -> LaunchSpec {
    LaunchSpec {
        node_exe: fixture_child(),
        cli_entry: fixture_child(), // placeholder token; the fixture arg parser skips unknown args
        cwd: std::env::temp_dir(),
        env: Vec::new(),
        args: vec![
            OsString::from("--grandchild"),
            fixture_grandchild().into_os_string(),
            OsString::from("--ms"),
            OsString::from("30000"),
        ],
    }
}

/// A child that spawns the 30 s grandchild, sleeps only 400 ms, then exits —
/// leaving a live descendant in the Job after the root is gone.
fn short_lived_fixture_spec() -> LaunchSpec {
    LaunchSpec {
        node_exe: fixture_child(),
        cli_entry: fixture_child(),
        cwd: std::env::temp_dir(),
        env: Vec::new(),
        args: vec![
            OsString::from("--grandchild"),
            fixture_grandchild().into_os_string(),
            OsString::from("--ms"),
            OsString::from("400"),
        ],
    }
}

fn wait_for_count(job: &Job, minimum: u32, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Ok(count) = job.active_process_count() {
            if count >= minimum {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn assign_before_resume_holds_the_primary_thread() {
    let job = Job::new().expect("create job");
    let child = job
        .create_suspended(&fixture_spec())
        .expect("create suspended child");
    job.assign(&child).expect("assign before resume");

    // The primary thread is suspended, so the grandchild has not been spawned yet.
    assert_eq!(
        job.active_process_count().expect("count before resume"),
        1,
        "only the root may exist while the primary thread is suspended"
    );

    job.resume(&child).expect("resume");
    assert!(
        wait_for_count(&job, 2, Duration::from_secs(10)),
        "the grandchild must join the Job after the primary thread resumes"
    );

    job.terminate().expect("terminate");
    assert!(job.wait_empty(Duration::from_secs(10)).expect("wait empty"));
}

#[test]
fn terminate_kills_root_and_grandchild_without_pid_lookup() {
    let job = Job::new().expect("create job");
    let child = job.create_suspended(&fixture_spec()).expect("create child");
    job.assign(&child).expect("assign");
    job.resume(&child).expect("resume");
    assert!(wait_for_count(&job, 2, Duration::from_secs(10)));

    job.terminate().expect("terminate");
    assert!(
        job.wait_empty(Duration::from_secs(10)).expect("wait empty"),
        "the Job must reach active-process zero after terminate"
    );
    assert_eq!(job.active_process_count().expect("final count"), 0);
}

#[test]
fn kill_on_close_terminates_the_tree_when_the_job_handle_drops() {
    let job = Job::new().expect("create job");
    let child = job.create_suspended(&fixture_spec()).expect("create child");
    job.assign(&child).expect("assign");
    job.resume(&child).expect("resume");
    assert!(wait_for_count(&job, 2, Duration::from_secs(10)));

    // Closing the last Job handle triggers KILL_ON_JOB_CLOSE: the whole tree dies with no PID lookup.
    drop(job);
    assert!(
        child.wait(Duration::from_secs(10)).expect("wait on child"),
        "the root must terminate when the Job handle is closed"
    );
}

#[test]
fn child_guard_disarms_on_into_inner_and_kills_on_drop() {
    let job = Job::new().expect("create job");

    // into_inner() disarms the guard: the child must survive (still suspended).
    let child = job
        .create_suspended(&fixture_spec())
        .expect("create suspended child");
    let guard = ChildGuard::new(child);
    let child = guard.into_inner();
    assert!(
        !child.wait(Duration::from_millis(200)).expect("wait"),
        "a disarmed guard must not kill the child"
    );
    job.assign(&child).expect("assign");
    job.resume(&child).expect("resume");
    assert!(wait_for_count(&job, 2, Duration::from_secs(10)));
    job.terminate().expect("terminate");
    assert!(job.wait_empty(Duration::from_secs(10)).expect("wait empty"));

    // Dropping the guard (the assign/resume-failure path) runs kill_and_wait
    // directly and must not leave the unassigned child behind.
    let child = job
        .create_suspended(&fixture_spec())
        .expect("create suspended child 2");
    let guard = ChildGuard::new(child);
    drop(guard); // kill_and_wait(2 s) inside Drop
    assert_eq!(
        job.active_process_count().expect("count after guard drop"),
        0,
        "the unassigned child must never linger in the Job"
    );
}

#[test]
fn unassigned_child_survives_job_terminate_but_direct_kill_reaches_it() {
    let job = Job::new().expect("create job");
    let child = job
        .create_suspended(&fixture_spec())
        .expect("create suspended child");

    // The exact hazard behind finding 5: TerminateJobObject is a no-op for a
    // process that was never assigned to the Job.
    job.terminate().expect("terminate job");
    assert!(
        !child.wait(Duration::from_millis(200)).expect("wait"),
        "an unassigned, suspended child must survive TerminateJobObject"
    );

    // ChildGuard's Drop path (Child::kill_and_wait) reaches it directly.
    child.kill_and_wait(Duration::from_secs(2));
    assert!(
        child.wait(Duration::from_secs(2)).expect("wait after kill"),
        "the direct kill must terminate the unassigned child"
    );
}

#[test]
fn root_exit_leaves_descendants_alive_until_job_terminate() {
    let job = Job::new().expect("create job");
    let child = job
        .create_suspended(&short_lived_fixture_spec())
        .expect("create child");
    job.assign(&child).expect("assign");
    job.resume(&child).expect("resume");
    assert!(wait_for_count(&job, 2, Duration::from_secs(10)));

    // The root exits (400 ms) while the 30 s grandchild remains.
    assert!(child.wait(Duration::from_secs(10)).expect("root exits"));
    assert!(
        wait_for_count(&job, 1, Duration::from_secs(2)),
        "the grandchild must remain in the Job after the root exits"
    );

    // TerminateJobObject reaps the residual descendant — the forced path,
    // never reported graceful.
    job.terminate().expect("terminate");
    assert!(job.wait_empty(Duration::from_secs(10)).expect("wait empty"));
}
