//! Child-bridge v1 wire protocol: length-prefixed UTF-8 JSON frames.
//!
//! Framing: 4-byte little-endian `u32` length prefix + UTF-8 JSON body.
//! Body is capped at [`MAX_FRAME_BYTES`] (64 KiB). The child authenticates with
//! a 64-character lowercase-hex token ([`TOKEN_CHARS`]) and a run id; the
//! sequence must strictly increase, starting from 1 for the initial `hello`.
//!
//! [`decode`] authenticates token/run/version/sequence BEFORE type-specific
//! parsing, so forged, replayed, oversized, wrong-token, or wrong-host frames
//! fail closed and never transition state.

use thiserror::Error;

use crate::compatibility::{DISTRIBUTION_VERSION, DSH_VERSION};
use crate::types::HealthCheck;

/// Maximum JSON body size (64 KiB), excluding the 4-byte length prefix.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

/// Token length: 64 lowercase hex characters.
pub const TOKEN_CHARS: usize = 64;

/// Stateful encoder for the independently sequenced Rust→child channel.
#[derive(Clone, Debug)]
pub struct HostFrameEncoder {
    run_id: String,
    token: String,
    next_sequence: u64,
}

impl HostFrameEncoder {
    pub fn new(run_id: impl Into<String>, token: impl Into<String>) -> Result<Self, ProtocolError> {
        let run_id = run_id.into();
        let token = token.into();
        if run_id.is_empty() {
            return Err(ProtocolError::BadRun);
        }
        if !is_lower_hex(&token, TOKEN_CHARS) {
            return Err(ProtocolError::BadToken);
        }
        Ok(Self {
            run_id,
            token,
            next_sequence: 1,
        })
    }

    pub fn encode_shutdown(&mut self) -> Result<Vec<u8>, ProtocolError> {
        self.encode("shutdown", None)
    }

    pub fn encode_web_accept(
        &mut self,
        pipe_name: &str,
        connection_token: &str,
    ) -> Result<Vec<u8>, ProtocolError> {
        const PIPE_PREFIX: &str = r"\\.\pipe\pimp-dsh-web-";
        let suffix = pipe_name.strip_prefix(PIPE_PREFIX);
        if !suffix.is_some_and(|value| is_lower_hex(value, 32))
            || !is_lower_hex(connection_token, TOKEN_CHARS)
        {
            return Err(ProtocolError::BadField);
        }
        self.encode("web-accept", Some((pipe_name, connection_token)))
    }

    fn encode(&mut self, kind: &str, web: Option<(&str, &str)>) -> Result<Vec<u8>, ProtocolError> {
        let sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(ProtocolError::BadSequence)?;
        let mut body = serde_json::json!({
            "protocolVersion": 1,
            "type": kind,
            "runId": self.run_id,
            "token": self.token,
            "sequence": sequence,
        });
        if let Some((pipe_name, connection_token)) = web {
            let object = body.as_object_mut().expect("host frame is an object");
            object.insert("pipeName".into(), pipe_name.into());
            object.insert("connectionToken".into(), connection_token.into());
        }
        let body = serde_json::to_vec(&body).expect("host frame serializes");
        Ok(frame_bytes(&body))
    }
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Child→Rust frame types (authenticated, then parsed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Hello {
        run_id: String,
        token: String,
        sequence: u64,
    },
    Ready {
        run_id: String,
        token: String,
        sequence: u64,
        profile: String,
        host: String,
        port: u16,
        url: String,
        distribution_version: String,
        dsh_version: String,
    },
    Health {
        run_id: String,
        token: String,
        sequence: u64,
        checks: Vec<HealthCheck>,
    },
    Stopping {
        run_id: String,
        token: String,
        sequence: u64,
    },
    Stopped {
        run_id: String,
        token: String,
        sequence: u64,
    },
    Error {
        run_id: String,
        token: String,
        sequence: u64,
        message: String,
    },
}

impl Frame {
    /// The `type` discriminator of this frame (for logging / diagnostics only).
    pub fn kind(&self) -> &'static str {
        match self {
            Frame::Hello { .. } => "hello",
            Frame::Ready { .. } => "ready",
            Frame::Health { .. } => "health",
            Frame::Stopping { .. } => "stopping",
            Frame::Stopped { .. } => "stopped",
            Frame::Error { .. } => "error",
        }
    }
}

/// Protocol violation, each mapped to a fail-closed outcome.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ProtocolError {
    #[error("frame exceeds 64 KiB")]
    Oversize,
    #[error("frame body is not valid UTF-8")]
    BadUtf8,
    #[error("malformed length prefix")]
    BadLength,
    #[error("frame body is not valid JSON")]
    BadJson,
    #[error("unexpected extra JSON field")]
    ExtraField,
    #[error("unsupported protocol version")]
    BadVersion,
    #[error("run id mismatch")]
    BadRun,
    #[error("token mismatch")]
    BadToken,
    #[error("sequence out of order")]
    BadSequence,
    #[error("replayed sequence")]
    Replay,
    #[error("malformed type-specific field")]
    BadField,
}

/// Decode and authenticate one framed message.
///
/// `bytes` is the full framed message (4-byte length prefix + body).
/// `next_seq` is the expected next sequence (starts at 1); it is advanced only
/// when a frame is fully authenticated and accepted.
pub fn decode(
    bytes: &[u8],
    expected_run: &str,
    expected_token: &str,
    next_seq: &mut u64,
) -> Result<Frame, ProtocolError> {
    if bytes.len() < 4 {
        return Err(ProtocolError::BadLength);
    }
    let mut prefix = [0u8; 4];
    prefix.copy_from_slice(&bytes[..4]);
    let body_len = u32::from_le_bytes(prefix) as usize;
    if body_len > MAX_FRAME_BYTES {
        return Err(ProtocolError::Oversize);
    }
    if bytes.len() < 4 + body_len {
        return Err(ProtocolError::BadLength);
    }
    let body = &bytes[4..4 + body_len];
    let text = std::str::from_utf8(body).map_err(|_| ProtocolError::BadUtf8)?;
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| ProtocolError::BadJson)?;
    let obj = value.as_object().ok_or(ProtocolError::BadJson)?;

    // Version gate first.
    match obj.get("protocolVersion").and_then(|v| v.as_u64()) {
        Some(1) => {}
        _ => return Err(ProtocolError::BadVersion),
    }

    let ty = obj
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or(ProtocolError::BadJson)?;

    // Strict field set (`additionalProperties:false`).
    let allowed: &[&str] = match ty {
        "hello" => &["protocolVersion", "type", "runId", "token", "sequence"],
        "ready" => &[
            "protocolVersion",
            "type",
            "runId",
            "token",
            "sequence",
            "profile",
            "host",
            "port",
            "url",
            "distributionVersion",
            "dshVersion",
        ],
        "health" => &[
            "protocolVersion",
            "type",
            "runId",
            "token",
            "sequence",
            "checks",
        ],
        "stopping" => &["protocolVersion", "type", "runId", "token", "sequence"],
        "stopped" => &["protocolVersion", "type", "runId", "token", "sequence"],
        "error" => &[
            "protocolVersion",
            "type",
            "runId",
            "token",
            "sequence",
            "message",
        ],
        _ => return Err(ProtocolError::BadJson), // unknown type
    };
    for key in obj.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(ProtocolError::ExtraField);
        }
    }

    let run_id = obj
        .get("runId")
        .and_then(|v| v.as_str())
        .ok_or(ProtocolError::BadJson)?;
    let token = obj
        .get("token")
        .and_then(|v| v.as_str())
        .ok_or(ProtocolError::BadJson)?;
    let sequence = obj
        .get("sequence")
        .and_then(|v| v.as_u64())
        .ok_or(ProtocolError::BadJson)?;

    // Authentication before type-specific parsing.
    if run_id != expected_run {
        return Err(ProtocolError::BadRun);
    }
    if token != expected_token {
        return Err(ProtocolError::BadToken);
    }
    if sequence < *next_seq {
        return Err(ProtocolError::Replay);
    }
    if sequence > *next_seq {
        return Err(ProtocolError::BadSequence);
    }
    if *next_seq == 1 && ty != "hello" {
        // First frame must be the authenticated hello.
        return Err(ProtocolError::BadSequence);
    }

    let frame = match ty {
        "hello" => Frame::Hello {
            run_id: run_id.to_string(),
            token: token.to_string(),
            sequence,
        },
        "health" => {
            let checks: Vec<HealthCheck> = obj
                .get("checks")
                .ok_or(ProtocolError::BadField)
                .and_then(|v| {
                    serde_json::from_value(v.clone()).map_err(|_| ProtocolError::BadField)
                })?;
            Frame::Health {
                run_id: run_id.to_string(),
                token: token.to_string(),
                sequence,
                checks,
            }
        }
        "stopping" => Frame::Stopping {
            run_id: run_id.to_string(),
            token: token.to_string(),
            sequence,
        },
        "stopped" => Frame::Stopped {
            run_id: run_id.to_string(),
            token: token.to_string(),
            sequence,
        },
        "error" => {
            let message = obj
                .get("message")
                .and_then(|v| v.as_str())
                .ok_or(ProtocolError::BadField)?;
            Frame::Error {
                run_id: run_id.to_string(),
                token: token.to_string(),
                sequence,
                message: message.to_string(),
            }
        }
        "ready" => {
            let profile = obj
                .get("profile")
                .and_then(|v| v.as_str())
                .ok_or(ProtocolError::BadField)?;
            let host = obj
                .get("host")
                .and_then(|v| v.as_str())
                .ok_or(ProtocolError::BadField)?;
            let port = obj
                .get("port")
                .and_then(|v| v.as_u64())
                .ok_or(ProtocolError::BadField)?;
            let url = obj
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or(ProtocolError::BadField)?;
            let distribution_version = obj
                .get("distributionVersion")
                .and_then(|v| v.as_str())
                .ok_or(ProtocolError::BadField)?;
            let dsh_version = obj
                .get("dshVersion")
                .and_then(|v| v.as_str())
                .ok_or(ProtocolError::BadField)?;

            // The supplied URL never becomes authority: it must exactly equal
            // the endpoint Rust constructs from host+port.
            if profile != "web" {
                return Err(ProtocolError::BadField);
            }
            if host != "127.0.0.1" {
                return Err(ProtocolError::BadField);
            }
            if !(1..=65535).contains(&port) {
                return Err(ProtocolError::BadField);
            }
            let expected_url = format!("http://127.0.0.1:{port}");
            if url != expected_url {
                return Err(ProtocolError::BadField);
            }
            if distribution_version != DISTRIBUTION_VERSION {
                return Err(ProtocolError::BadField);
            }
            if dsh_version != DSH_VERSION {
                return Err(ProtocolError::BadField);
            }

            Frame::Ready {
                run_id: run_id.to_string(),
                token: token.to_string(),
                sequence,
                profile: profile.to_string(),
                host: host.to_string(),
                port: port as u16,
                url: url.to_string(),
                distribution_version: distribution_version.to_string(),
                dsh_version: dsh_version.to_string(),
            }
        }
        _ => unreachable!("unknown type already rejected"),
    };

    *next_seq += 1;
    Ok(frame)
}

/// Rust→child frames must be encoded through [`HostFrameEncoder`], which owns
/// the authenticated channel's sequence.

/// Prefix an already-serialized JSON body with its little-endian length.
pub fn frame_bytes(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    out
}

/// Construct an endpoint from a validated host+port. Always loopback.
pub fn construct_endpoint(host: &str, port: u16) -> String {
    format!("http://{host}:{port}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::HealthStatus;

    const RUN: &str = "run-123";
    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn frame(body: &str) -> Vec<u8> {
        frame_bytes(body.as_bytes())
    }

    fn hello(sequence: u64) -> String {
        format!(
            "{{\"protocolVersion\":1,\"type\":\"hello\",\"runId\":\"{RUN}\",\"token\":\"{TOKEN}\",\"sequence\":{sequence}}}"
        )
    }

    fn ready(sequence: u64, port: u16) -> String {
        format!(
            "{{\"protocolVersion\":1,\"type\":\"ready\",\"runId\":\"{RUN}\",\"token\":\"{TOKEN}\",\"sequence\":{sequence},\"profile\":\"web\",\"host\":\"127.0.0.1\",\"port\":{port},\"url\":\"http://127.0.0.1:{port}\",\"distributionVersion\":\"0.1.0\",\"dshVersion\":\"0.1.0-rc.7\"}}"
        )
    }

    fn health(sequence: u64, checks: &str) -> String {
        format!(
            "{{\"protocolVersion\":1,\"type\":\"health\",\"runId\":\"{RUN}\",\"token\":\"{TOKEN}\",\"sequence\":{sequence},\"checks\":{checks}}}"
        )
    }

    #[test]
    fn health_carries_checks() {
        let mut seq = 1;
        decode(&frame(&hello(1)), RUN, TOKEN, &mut seq).unwrap();
        let h = decode(
            &frame(&health(
                2,
                r#"[{"id":"web-server","status":"ok","message":"listening on 127.0.0.1:4567"}]"#,
            )),
            RUN,
            TOKEN,
            &mut seq,
        )
        .unwrap();
        match h {
            Frame::Health { checks, .. } => {
                assert_eq!(checks.len(), 1);
                assert_eq!(checks[0].id, "web-server");
                assert_eq!(checks[0].status, HealthStatus::Ok);
                assert_eq!(checks[0].message, "listening on 127.0.0.1:4567");
            }
            _ => panic!("expected health"),
        }
    }

    #[test]
    fn health_missing_checks_rejected() {
        let mut seq = 1;
        decode(&frame(&hello(1)), RUN, TOKEN, &mut seq).unwrap();
        let bad = format!(
            "{{\"protocolVersion\":1,\"type\":\"health\",\"runId\":\"{RUN}\",\"token\":\"{TOKEN}\",\"sequence\":2}}"
        );
        assert_eq!(
            decode(&frame(&bad), RUN, TOKEN, &mut seq),
            Err(ProtocolError::BadField)
        );
    }

    #[test]
    fn hello_then_ready_accepted() {
        let mut seq = 1;
        let f = decode(&frame(&hello(1)), RUN, TOKEN, &mut seq).unwrap();
        assert!(matches!(f, Frame::Hello { .. }));
        assert_eq!(seq, 2);
        let r = decode(&frame(&ready(2, 4567)), RUN, TOKEN, &mut seq).unwrap();
        match r {
            Frame::Ready {
                profile,
                host,
                port,
                url,
                ..
            } => {
                assert_eq!(profile, "web");
                assert_eq!(host, "127.0.0.1");
                assert_eq!(port, 4567);
                assert_eq!(url, "http://127.0.0.1:4567");
            }
            _ => panic!("expected ready"),
        }
        assert_eq!(seq, 3);
    }

    #[test]
    fn wrong_token_rejected() {
        let mut seq = 1;
        let bad = hello(1).replace(TOKEN, &"f".repeat(64));
        assert_eq!(
            decode(&frame(&bad), RUN, TOKEN, &mut seq),
            Err(ProtocolError::BadToken)
        );
        assert_eq!(seq, 1); // not advanced
    }

    #[test]
    fn wrong_run_rejected() {
        let mut seq = 1;
        let bad = hello(1).replace(RUN, "other-run");
        assert_eq!(
            decode(&frame(&bad), RUN, TOKEN, &mut seq),
            Err(ProtocolError::BadRun)
        );
    }

    #[test]
    fn replayed_sequence_rejected() {
        let mut seq = 1;
        decode(&frame(&hello(1)), RUN, TOKEN, &mut seq).unwrap();
        assert_eq!(seq, 2);
        // Replay the accepted hello.
        assert_eq!(
            decode(&frame(&hello(1)), RUN, TOKEN, &mut seq),
            Err(ProtocolError::Replay)
        );
    }

    #[test]
    fn gap_sequence_rejected() {
        let mut seq = 1;
        assert_eq!(
            decode(&frame(&hello(5)), RUN, TOKEN, &mut seq),
            Err(ProtocolError::BadSequence)
        );
    }

    #[test]
    fn first_frame_must_be_hello() {
        let mut seq = 1;
        assert_eq!(
            decode(&frame(&ready(1, 3000)), RUN, TOKEN, &mut seq),
            Err(ProtocolError::BadSequence)
        );
    }

    #[test]
    fn extra_field_rejected() {
        let mut seq = 1;
        let mut v: serde_json::Value = serde_json::from_str(&hello(1)).unwrap();
        v["extra"] = serde_json::json!(true);
        let bytes = frame(&v.to_string());
        assert_eq!(
            decode(&bytes, RUN, TOKEN, &mut seq),
            Err(ProtocolError::ExtraField)
        );
    }

    #[test]
    fn bad_version_rejected() {
        let mut seq = 1;
        let mut v: serde_json::Value = serde_json::from_str(&hello(1)).unwrap();
        v["protocolVersion"] = serde_json::json!(2);
        assert_eq!(
            decode(&frame(&v.to_string()), RUN, TOKEN, &mut seq),
            Err(ProtocolError::BadVersion)
        );
    }

    #[test]
    fn non_loopback_host_rejected() {
        let mut seq = 1;
        decode(&frame(&hello(1)), RUN, TOKEN, &mut seq).unwrap();
        let bad = ready(2, 4567).replace("127.0.0.1", "0.0.0.0");
        assert_eq!(
            decode(&frame(&bad), RUN, TOKEN, &mut seq),
            Err(ProtocolError::BadField)
        );
    }

    #[test]
    fn mismatched_url_rejected() {
        let mut seq = 1;
        decode(&frame(&hello(1)), RUN, TOKEN, &mut seq).unwrap();
        let bad = ready(2, 4567).replace("http://127.0.0.1:4567", "http://evil");
        assert_eq!(
            decode(&frame(&bad), RUN, TOKEN, &mut seq),
            Err(ProtocolError::BadField)
        );
    }

    #[test]
    fn oversize_frame_rejected() {
        let mut seq = 1;
        let mut bytes = vec![0u8; 4];
        bytes[0] = 0x01;
        bytes[2] = 0x01; // little-endian 65537 > MAX_FRAME_BYTES
        bytes.extend_from_slice(&[0u8; 65537]);
        assert_eq!(
            decode(&bytes, RUN, TOKEN, &mut seq),
            Err(ProtocolError::Oversize)
        );
    }

    #[test]
    fn bad_utf8_rejected() {
        let mut seq = 1;
        let mut bytes = vec![0u8; 4];
        bytes[0] = 4;
        bytes.extend_from_slice(&[0xff, 0xfe, 0xfd, 0xfc]);
        assert_eq!(
            decode(&bytes, RUN, TOKEN, &mut seq),
            Err(ProtocolError::BadUtf8)
        );
    }

    #[test]
    fn host_frames_are_authenticated_and_strictly_sequenced() {
        let mut encoder = HostFrameEncoder::new(RUN, TOKEN).unwrap();
        let shutdown = encoder.encode_shutdown().unwrap();
        let web = encoder
            .encode_web_accept(
                r"\\.\pipe\pimp-dsh-web-0123456789abcdef0123456789abcdef",
                TOKEN,
            )
            .unwrap();
        let shutdown: serde_json::Value = serde_json::from_slice(&shutdown[4..]).unwrap();
        let web: serde_json::Value = serde_json::from_slice(&web[4..]).unwrap();
        assert_eq!(shutdown["sequence"], 1);
        assert_eq!(web["sequence"], 2);
        assert_eq!(shutdown["runId"], RUN);
        assert_eq!(shutdown["token"], TOKEN);
        assert_eq!(
            web["pipeName"],
            r"\\.\pipe\pimp-dsh-web-0123456789abcdef0123456789abcdef"
        );
        assert_eq!(web["connectionToken"], TOKEN);
    }
    #[test]
    fn construct_endpoint_is_loopback() {
        assert_eq!(
            construct_endpoint("127.0.0.1", 8080),
            "http://127.0.0.1:8080"
        );
    }
}
