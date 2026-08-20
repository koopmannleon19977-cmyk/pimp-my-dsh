//! Windows named-pipe contract tests: first-instance creation and name round-tripping.
//!
//! The DACL restriction (current user + SYSTEM, PIPE_REJECT_REMOTE_CLIENTS) is set inside
//! `BridgePipe::create`; first-instance creation is the runtime-observable half of the IPC-01
//! rejection matrix that can be exercised without a second machine or account.

#![cfg(windows)]

use std::io::{Read, Write};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use pimp_dsh_desktop::pipe::BridgePipe;

#[test]
fn create_succeeds_and_reports_the_name() {
    let name = format!(r"\\.\pipe\pimp-dsh-test-pipe-basic-{}", std::process::id());
    let pipe = BridgePipe::create(&name).expect("create pipe");
    assert_eq!(pipe.name(), name);
}

#[test]
fn create_fails_on_a_pre_existing_name() {
    let name = format!(
        r"\\.\pipe\pimp-dsh-test-pipe-first-instance-{}",
        std::process::id()
    );
    let _first = BridgePipe::create(&name).expect("first create");
    assert!(
        BridgePipe::create(&name).is_err(),
        "a second create of the same name must fail (first-instance creation)"
    );
}

#[test]
fn distinct_names_do_not_collide() {
    let pid = std::process::id();
    let _a =
        BridgePipe::create(&format!(r"\\.\pipe\pimp-dsh-test-pipe-a-{pid}")).expect("create a");
    let _b =
        BridgePipe::create(&format!(r"\\.\pipe\pimp-dsh-test-pipe-b-{pid}")).expect("create b");
}

#[test]
fn connect_timeout_is_bounded_without_a_client() {
    let name = format!(
        r"\\.\pipe\pimp-dsh-test-pipe-timeout-{}",
        std::process::id()
    );
    let pipe = BridgePipe::create(&name).expect("create pipe");
    let started = Instant::now();
    let error = pipe
        .connect_timeout(Duration::from_millis(100))
        .expect_err("no client must time out");
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "overlapped connect timeout must not block the caller"
    );
}

#[test]
fn pending_read_does_not_delay_duplex_shutdown_write() {
    let name = format!(r"\\.\pipe\pimp-dsh-test-pipe-duplex-{}", std::process::id());
    let pipe = Arc::new(BridgePipe::create(&name).expect("create pipe"));
    let client_name = name.clone();
    let client = thread::spawn(move || {
        let mut client = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(client_name)
            .expect("connect client");
        thread::sleep(Duration::from_secs(1));
        client.write_all(b"ping").expect("send inbound frame");
        let mut shutdown = [0u8; 8];
        client
            .read_exact(&mut shutdown)
            .expect("read shutdown frame");
        assert_eq!(&shutdown, b"shutdown");
    });

    pipe.connect_timeout(Duration::from_secs(2))
        .expect("connect server");
    let (reading_tx, reading_rx) = mpsc::channel();
    let read_pipe = Arc::clone(&pipe);
    let reader = thread::spawn(move || {
        reading_tx.send(()).expect("signal reader");
        let mut inbound = [0u8; 4];
        read_pipe
            .read_exact(&mut inbound)
            .expect("read inbound frame");
        assert_eq!(&inbound, b"ping");
    });
    reading_rx.recv().expect("reader started");
    thread::sleep(Duration::from_millis(50));

    let started = Instant::now();
    pipe.write_all(b"shutdown").expect("write shutdown frame");
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "a pending read must not serialize the duplex shutdown write"
    );

    reader.join().expect("reader thread");
    client.join().expect("client thread");
}
