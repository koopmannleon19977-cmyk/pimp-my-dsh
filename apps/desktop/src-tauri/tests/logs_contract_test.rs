//! Contract tests for the bounded log pipeline: secret redaction, ANSI stripping, 16 KiB event
//! truncation, bounded in-memory queue, and drain-to-discard on disk-write failure.
//!
//! `LogEvent` is constructed via its camelCase JSON wire form so these tests exercise the same
//! serde boundary the renderer and disk writer use.

use std::path::PathBuf;

use pimp_dsh_desktop::logging::{
    LogEvent, LogSink, clear_secrets, redact, register_secret, sanitize_text, strip_ansi,
};

mod common;
use common::TempDir;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn event(message: &str) -> LogEvent {
    let message_json = serde_json::to_string(message).expect("message serializes");
    serde_json::from_str(&format!(
        r#"{{"runId":null,"revision":1,"sequence":1,"timestamp":"2026-01-01T00:00:00Z","source":"stdout","level":"info","message":{message_json}}}"#
    ))
    .expect("valid LogEvent wire form")
}

#[test]
fn redact_removes_registered_secret_values() {
    clear_secrets();
    register_secret(TOKEN);
    let out = redact(&format!("supervisor started token={TOKEN}"));
    assert!(!out.contains(TOKEN), "registered secret must be redacted");
    clear_secrets();
}

#[test]
fn strip_ansi_removes_control_sequences() {
    let ansi = "\u{1b}[31mred\u{1b}[0m and \u{1b}[1mbold\u{1b}[22m";
    let out = strip_ansi(ansi);
    assert!(
        !out.contains("\u{1b}["),
        "ANSI CSI sequences must be stripped, got {out:?}"
    );
    assert!(out.contains("red"));
    assert!(out.contains("bold"));
}

#[test]
fn sanitize_text_truncates_to_16_kib() {
    let big = "x".repeat(32 * 1024);
    let out = sanitize_text(&big);
    assert!(
        out.len() <= 16 * 1024,
        "message must be truncated to 16 KiB"
    );
}

#[test]
fn sink_bounds_memory_queue_to_capacity() {
    let mut sink = LogSink::new(3, None);
    for i in 0..10 {
        sink.push(event(&format!("message {i}")));
    }
    assert_eq!(sink.snapshot().len(), 3, "queue must never exceed capacity");
    assert!(sink.fault().is_none());
}

#[test]
fn sink_strips_ansi_before_storing() {
    let mut sink = LogSink::new(8, None);
    sink.push(event("\u{1b}[31malert\u{1b}[0m"));
    let stored = sink.snapshot();
    assert_eq!(stored.len(), 1);
    assert!(
        !stored[0].message.contains("\u{1b}["),
        "stored message must be ANSI-free"
    );
}

#[test]
fn sink_drains_to_discard_on_disk_failure_with_single_fault() {
    let dir = TempDir::new("pimp-dsh-log");
    // A regular file as the disk target makes `create_dir_all` fail deterministically.
    let file_path: PathBuf = dir.path().join("not-a-directory");
    std::fs::write(&file_path, b"x").expect("seed file");

    let mut sink = LogSink::new(8, Some(file_path));
    sink.push(event("hello"));
    assert!(
        sink.fault().is_some(),
        "disk failure must surface exactly one visible fault"
    );

    // Lifecycle remains operable: pushes keep draining to the memory queue.
    sink.push(event("world"));
    sink.push(event("still-works"));
    let snap = sink.snapshot();
    let messages: Vec<&str> = snap.iter().map(|e| e.message.as_str()).collect();
    assert!(
        messages.contains(&"still-works"),
        "sink must keep accepting events after a disk fault"
    );
}
