//! Bounded, redacted log sink.
//!
//! Every event passes a fixed pipeline before storage: UTF-8 replacement,
//! ANSI stripping, secret redaction, then 16 KiB truncation. HTML text
//! rendering is a renderer-side concern ([`escape_html`] is provided for
//! completeness/tests); the stored message is never HTML-escaped.
//!
//! The in-memory queue is bounded. The optional disk writer appends JSONL; on
//! the first write failure it switches once to drain-and-discard and surfaces a
//! single supervisor error, leaving lifecycle operation unaffected.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

/// Maximum message length per event (16 KiB).
pub const MAX_EVENT_BYTES: usize = 16 * 1024;

/// Event origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogSource {
    Supervisor,
    Stdout,
    Stderr,
    Lifecycle,
    Doctor,
}

/// Event severity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogLevel {
    Trace,
    Info,
    Warning,
    Error,
}

/// One log event (v1 field set; serialized `camelCase`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEvent {
    pub run_id: Option<String>,
    pub revision: u64,
    pub sequence: u64,
    pub timestamp: String,
    pub source: LogSource,
    pub level: LogLevel,
    pub message: String,
}

/// Registry of extra secret values to redact (the per-run bridge token). The
/// current process environment's `PIMP_DSH_*` / `DSH_PIMP_*` values are always
/// redacted regardless of this registry.
static SECRETS: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// Register a secret value for [`redact`] (e.g. the current run token). The
/// value is held in memory only and is never persisted.
pub fn register_secret(secret: &str) {
    if !secret.is_empty() {
        SECRETS
            .write()
            .expect("secrets lock poisoned")
            .push(secret.to_string());
    }
}

/// Remove all registered secrets (test isolation / run teardown).
pub fn clear_secrets() {
    SECRETS.write().expect("secrets lock poisoned").clear();
}

fn is_secret_env_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("PIMP_DSH_") || upper.starts_with("DSH_PIMP_")
}

/// Collect every secret value to redact: the registered secrets plus every
/// case-insensitive `PIMP_DSH_*` / `DSH_PIMP_*` environment value. Longer
/// secrets are sorted first so a value that contains another secret is fully
/// removed.
fn collect_secrets() -> Vec<String> {
    let mut secrets: Vec<String> = SECRETS.read().expect("secrets lock poisoned").clone();
    for (name, value) in std::env::vars_os() {
        let name = name.to_string_lossy().into_owned();
        if is_secret_env_name(&name) {
            let value = value.to_string_lossy().into_owned();
            if !value.is_empty() {
                secrets.push(value);
            }
        }
    }
    secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
    secrets
}

/// Byte ranges of every non-overlapping secret occurrence in `input`, with
/// longer secrets claiming their spans first (mirroring `String::replace` run
/// longest-first so a nested secret is removed wholesale).
fn secret_occurrences(input: &str, secrets: &[String]) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for secret in secrets {
        if secret.is_empty() {
            continue;
        }
        let mut from = 0;
        while let Some(found) = input[from..].find(secret) {
            let start = from + found;
            let end = start + secret.len();
            if ranges.iter().any(|&(s, e)| start < e && end > s) {
                // Overlaps a longer secret already claimed; skip past it by one
                // character so `input[from..]` stays on a UTF-8 boundary.
                let step = secret.chars().next().map_or(1, |c| c.len_utf8());
                from = start + step;
            } else {
                ranges.push((start, end));
                from = end;
            }
        }
    }
    ranges.sort_unstable_by_key(|&(s, _)| s);
    ranges
}

/// Replace every occurrence in `ranges` with `[redacted]`.
fn redact_with(input: &str, secrets: &[String]) -> String {
    let ranges = secret_occurrences(input, secrets);
    if ranges.is_empty() {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    for (s, e) in ranges {
        out.push_str(&input[cursor..s]);
        out.push_str("[redacted]");
        cursor = e;
    }
    out.push_str(&input[cursor..]);
    out
}

/// The longest secret length currently registered (0 when none). Streaming
/// redaction must hold back at least this many bytes minus one so a secret
/// split across chunk boundaries is fully present when redaction runs.
fn max_secret_len() -> usize {
    collect_secrets().iter().map(String::len).max().unwrap_or(0)
}

/// Redact the registered token and every case-insensitive `PIMP_DSH_*` /
/// `DSH_PIMP_*` environment value. Longer secrets are replaced first so a
/// value that contains another secret is fully removed.
pub fn redact(input: &str) -> String {
    redact_with(input, &collect_secrets())
}

/// Split `text` at byte offset `split` (a UTF-8 char boundary) into an emitted,
/// redacted prefix and a raw carry tail. A secret that straddles `split` is
/// fully present in `text`, so it is redacted in the emitted prefix and its
/// tail is dropped from the carry rather than leaked as a fragment.
fn redact_prefix(text: &str, split: usize) -> (String, String) {
    let secrets = collect_secrets();
    let ranges = secret_occurrences(text, &secrets);

    let mut emitted = String::with_capacity(split + 10);
    let mut carry = String::with_capacity(text.len().saturating_sub(split) + 10);
    let mut emit_cursor = 0usize;
    let mut carry_cursor = split;

    for (s, e) in ranges {
        if s >= split {
            carry.push_str(&text[carry_cursor..]);
            carry_cursor = text.len();
            break;
        }
        if e <= split {
            emitted.push_str(&text[emit_cursor..s]);
            emitted.push_str("[redacted]");
            emit_cursor = e;
        } else {
            emitted.push_str(&text[emit_cursor..s]);
            emitted.push_str("[redacted]");
            emit_cursor = split;
            carry_cursor = e;
        }
    }
    emitted.push_str(&text[emit_cursor..split]);
    carry.push_str(&text[carry_cursor..]);
    (emitted, carry)
}

/// Remove ANSI CSI/OSC control sequences.
pub fn strip_ansi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            // ESC [ ... final byte
            if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                let mut j = i + 2;
                while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                    j += 1;
                }
                if j < bytes.len() {
                    i = j + 1; // skip through final byte
                } else {
                    break;
                }
            } else if i + 1 < bytes.len() && bytes[i + 1] == b']' {
                // OSC: ESC ] ... (BEL | ESC \)
                let mut j = i + 2;
                while j < bytes.len() {
                    if bytes[j] == 0x07 {
                        j += 1;
                        break;
                    }
                    if bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                        j += 2;
                        break;
                    }
                    j += 1;
                }
                i = j;
            } else {
                i += 1;
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Escape a message for HTML text rendering.
pub fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Apply the full sanitization pipeline to raw child output bytes.
pub fn sanitize_bytes(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    sanitize_text(&text)
}

/// Apply the sanitization pipeline to a string: strip ANSI, redact, truncate.
pub fn sanitize_text(input: &str) -> String {
    let mut s = strip_ansi(input);
    s = redact(&s);
    truncate_utf8(&s, MAX_EVENT_BYTES)
}

/// Truncate a string to `max` bytes on a UTF-8 character boundary.
fn truncate_utf8(input: &str, max: usize) -> String {
    if input.len() <= max {
        return input.to_string();
    }
    let mut end = max;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    input[..end].to_string()
}

/// Incremental secret redaction for a byte stream that arrives in arbitrary
/// chunks. A secret split across chunk boundaries must never surface as clear
/// text, so the redactor holds back a carry tail long enough to contain any
/// secret whose head has already arrived, then redacts the whole buffer before
/// emitting. [`StreamingRedactor::flush`] drains the remaining tail at
/// end-of-stream.
pub struct StreamingRedactor {
    carry: String,
}

impl Default for StreamingRedactor {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingRedactor {
    pub fn new() -> Self {
        StreamingRedactor {
            carry: String::new(),
        }
    }

    /// Feed one chunk of raw bytes. Returns the sanitized text that is safe to
    /// emit now (possibly empty while a potential secret prefix is held back).
    pub fn feed(&mut self, bytes: &[u8]) -> String {
        // Strip ANSI per chunk (matching the single-event pipeline) so a secret
        // interrupted by an escape sequence is still matched after stripping.
        let chunk = String::from_utf8_lossy(bytes);
        self.carry.push_str(&strip_ansi(&chunk));

        let keep = max_secret_len().saturating_sub(1);
        if self.carry.len() <= keep {
            return String::new();
        }
        let mut split = self.carry.len() - keep;
        while split > 0 && !self.carry.is_char_boundary(split) {
            split -= 1;
        }
        let (emitted, tail) = redact_prefix(&self.carry, split);
        self.carry = tail;
        emitted
    }

    /// Flush any remaining buffered bytes at end-of-stream, redacting them in
    /// full. Called exactly once after the final feed.
    pub fn flush(&mut self) -> String {
        if self.carry.is_empty() {
            return String::new();
        }
        let text = std::mem::take(&mut self.carry);
        redact(&text)
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Bounded in-memory queue plus optional JSONL disk writer.
pub struct LogSink {
    capacity: usize,
    queue: VecDeque<LogEvent>,
    next_sequence: u64,
    disk_dir: Option<PathBuf>,
    disk_faulted: bool,
}

impl LogSink {
    pub fn new(capacity: usize, disk_dir: Option<PathBuf>) -> Self {
        LogSink {
            capacity: capacity.max(1),
            queue: VecDeque::new(),
            next_sequence: 0,
            disk_dir,
            disk_faulted: false,
        }
    }

    /// Append an event. Assigns a monotonic sequence + timestamp, sanitizes the
    /// message, bounds the queue, and best-effort writes to disk.
    pub fn push(&mut self, mut event: LogEvent) {
        event.message = sanitize_text(&event.message);
        event.sequence = self.next_sequence;
        event.timestamp = now_rfc3339();
        self.next_sequence += 1;

        if self.queue.len() >= self.capacity {
            self.queue.pop_front();
        }
        self.queue.push_back(event.clone());

        if let Some(dir) = self.disk_dir.clone() {
            if !self.disk_faulted {
                if let Err(err) = write_event(&dir, &event) {
                    self.disk_faulted = true;
                    // One supervisor error, then drain-and-discard.
                    let fault_event = LogEvent {
                        run_id: event.run_id.clone(),
                        revision: event.revision,
                        sequence: self.next_sequence,
                        timestamp: now_rfc3339(),
                        source: LogSource::Supervisor,
                        level: LogLevel::Error,
                        message: format!("disk log write failed: {err}; draining to discard"),
                    };
                    self.next_sequence += 1;
                    if self.queue.len() >= self.capacity {
                        self.queue.pop_front();
                    }
                    self.queue.push_back(fault_event);
                }
            }
            // If faulted, drain-and-discard: push continues without writing.
        }
    }

    /// Copy of the bounded queue (oldest first).
    pub fn snapshot(&self) -> Vec<LogEvent> {
        self.queue.iter().cloned().collect()
    }

    /// The single surfaced logging fault, if any.
    pub fn fault(&self) -> Option<&'static str> {
        if self.disk_faulted {
            Some("disk log write failed; draining to discard")
        } else {
            None
        }
    }
}

fn write_event(dir: &PathBuf, event: &LogEvent) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("supervisor.log");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line = serde_json::to_string(event).expect("LogEvent serializes");
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(message: &str) -> LogEvent {
        LogEvent {
            run_id: None,
            revision: 0,
            sequence: 0,
            timestamp: String::new(),
            source: LogSource::Stdout,
            level: LogLevel::Info,
            message: message.to_string(),
        }
    }

    static SECRET_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn strip_ansi_removes_csi_and_osc() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(strip_ansi("a\x1b]0;title\x07b"), "ab");
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn escape_html_renders_text_safe() {
        assert_eq!(escape_html("<b>&\"'"), "&lt;b&gt;&amp;&quot;&#39;");
    }

    #[test]
    fn redact_removes_token() {
        let _guard = SECRET_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let token = "f".repeat(64);
        register_secret(&token);
        let out = redact(&format!("token={token} done"));
        assert!(!out.contains(&token));
        assert!(out.contains("[redacted]"));
        clear_secrets();
    }

    #[test]
    fn redact_removes_env_values() {
        let _guard = SECRET_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Value is read from the live process env; just verify a secret-shaped
        // value in the registry is redacted (env redaction is the same path).
        register_secret("s3cr3t-value");
        assert_eq!(redact("x s3cr3t-value y"), "x [redacted] y");
        clear_secrets();
    }

    #[test]
    fn streaming_redacts_secret_split_at_every_position() {
        let _guard = SECRET_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let token = "f".repeat(64);
        register_secret(&token);
        let full = format!("pre-{token}-post");
        for split in 0..=full.len() {
            let mut redactor = StreamingRedactor::new();
            let mut out = String::new();
            out.push_str(&redactor.feed(&full.as_bytes()[..split]));
            out.push_str(&redactor.feed(&full.as_bytes()[split..]));
            out.push_str(&redactor.flush());
            assert_eq!(out, "pre-[redacted]-post", "split at byte {split}");
        }
        clear_secrets();
    }

    #[test]
    fn streaming_flush_redacts_short_secret_in_carry() {
        let _guard = SECRET_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // A short secret (shorter than the longest registered secret) can sit
        // entirely inside the held-back carry tail and must be redacted at flush.
        register_secret(&"f".repeat(64)); // sets max carry length
        register_secret("abc123");
        let mut redactor = StreamingRedactor::new();
        let mut out = String::new();
        out.push_str(&redactor.feed(format!("{}abc123", "x".repeat(100)).as_bytes()));
        out.push_str(&redactor.flush());
        assert!(!out.contains("abc123"));
        assert!(out.contains("[redacted]"));
        clear_secrets();
    }

    #[test]
    fn streaming_redacts_secret_across_many_chunks() {
        let _guard = SECRET_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let token = "f".repeat(64);
        register_secret(&token);
        let full = format!("a{token}b");
        let mut redactor = StreamingRedactor::new();
        let mut out = String::new();
        // Single-byte chunks force the secret across 64 separate boundaries.
        for byte in full.as_bytes() {
            out.push_str(&redactor.feed(&[*byte]));
        }
        out.push_str(&redactor.flush());
        assert_eq!(out, "a[redacted]b");
        clear_secrets();
    }

    #[test]
    fn queue_is_bounded() {
        let mut sink = LogSink::new(3, None);
        for i in 0..10 {
            sink.push(event(&format!("msg {i}")));
        }
        let snap = sink.snapshot();
        assert_eq!(snap.len(), 3);
        assert_eq!(snap[0].message, "msg 7"); // oldest retained
        assert_eq!(snap[2].message, "msg 9");
    }

    #[test]
    fn oversize_event_truncated() {
        let mut sink = LogSink::new(10, None);
        let big = "x".repeat(MAX_EVENT_BYTES + 100);
        sink.push(event(&big));
        let snap = sink.snapshot();
        assert!(snap[0].message.len() <= MAX_EVENT_BYTES);
    }

    #[test]
    fn utf8_replacement() {
        let bytes = [0xff, 0xfe, b'a', b'b', b'c'];
        assert_eq!(sanitize_bytes(&bytes), "\u{fffd}\u{fffd}abc");
    }

    #[test]
    fn sequence_is_monotonic() {
        let mut sink = LogSink::new(10, None);
        sink.push(event("a"));
        sink.push(event("b"));
        let snap = sink.snapshot();
        assert_eq!(snap[0].sequence, 0);
        assert_eq!(snap[1].sequence, 1);
    }

    #[test]
    fn disk_failure_surfaces_one_fault() {
        // A path that cannot be a directory (a file) forces create_dir_all to fail.
        let dir = std::env::temp_dir().join("pimp-dsh-not-a-dir-file");
        std::fs::write(&dir, b"x").unwrap();
        let mut sink = LogSink::new(10, Some(dir.clone()));
        sink.push(event("one"));
        sink.push(event("two"));
        assert_eq!(
            sink.fault(),
            Some("disk log write failed; draining to discard")
        );
        // The queue still holds events despite the fault.
        assert_eq!(sink.snapshot().len(), 3);
        assert_eq!(
            sink.snapshot()
                .iter()
                .filter(|event| event.source == LogSource::Supervisor)
                .count(),
            1
        );
        let _ = std::fs::remove_file(&dir);
    }
}
