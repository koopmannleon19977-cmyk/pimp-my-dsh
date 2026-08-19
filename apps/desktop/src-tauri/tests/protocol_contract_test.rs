//! Contract tests for the length-prefixed child bridge protocol.
//!
//! Constructs raw wire frames (4-byte little-endian length prefix + UTF-8 JSON body) and drives
//! `protocol::decode` through the full failure matrix from local://desktop-contracts.md §Child bridge:
//! size limits, UTF-8/JSON validity, additionalProperties:false, version/token/run/sequence
//! authentication, replay rejection, and ready host/port/url validation.

use pimp_dsh_desktop::protocol::{
    Frame, MAX_FRAME_BYTES, ProtocolError, TOKEN_CHARS, decode, encode_shutdown,
};

const RUN: &str = "run-123";
const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// Wrap a UTF-8 JSON body in the 4-byte little-endian length prefix.
fn frame(body: &str) -> Vec<u8> {
    let body = body.as_bytes();
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    out
}

/// Wrap a raw (possibly non-UTF-8) body in the length prefix.
fn raw_frame(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    out
}

fn hello(sequence: u64, run: &str, token: &str) -> String {
    format!(
        r#"{{"protocolVersion":1,"type":"hello","runId":"{run}","token":"{token}","sequence":{sequence}}}"#
    )
}

fn ready(sequence: u64, host: &str, port: u16, url: &str) -> String {
    format!(
        r#"{{"protocolVersion":1,"type":"ready","runId":"{RUN}","token":"{TOKEN}","sequence":{sequence},"profile":"web","host":"{host}","port":{port},"url":"{url}","distributionVersion":"0.1.0","dshVersion":"0.1.0-rc.7"}}"#
    )
}

#[test]
fn constants_match_the_wire_contract() {
    assert_eq!(MAX_FRAME_BYTES, 64 * 1024);
    assert_eq!(TOKEN_CHARS, 64);
}

#[test]
fn accepts_hello_then_ready_in_strict_sequence() {
    let mut next_seq = 1u64;
    match decode(&frame(&hello(1, RUN, TOKEN)), RUN, TOKEN, &mut next_seq) {
        Ok(Frame::Hello {
            run_id,
            token,
            sequence,
        }) => {
            assert_eq!(run_id, RUN);
            assert_eq!(token, TOKEN);
            assert_eq!(sequence, 1);
        }
        other => panic!("expected Hello frame, got {other:?}"),
    }
    assert_eq!(next_seq, 2);

    let url = "http://127.0.0.1:49152";
    match decode(
        &frame(&ready(2, "127.0.0.1", 49152, url)),
        RUN,
        TOKEN,
        &mut next_seq,
    ) {
        Ok(Frame::Ready {
            host,
            port,
            url,
            distribution_version,
            dsh_version,
            profile,
            ..
        }) => {
            assert_eq!(host, "127.0.0.1");
            assert_eq!(port, 49152);
            assert_eq!(url, "http://127.0.0.1:49152");
            assert_eq!(distribution_version, "0.1.0");
            assert_eq!(dsh_version, "0.1.0-rc.7");
            assert_eq!(profile, "web");
        }
        other => panic!("expected Ready frame, got {other:?}"),
    }
    assert_eq!(next_seq, 3);
}

#[test]
fn rejects_oversized_length_prefix() {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&(MAX_FRAME_BYTES as u32 + 1).to_le_bytes());
    bytes.extend_from_slice(&[b'{'; 4]);
    let mut next_seq = 1u64;
    assert!(matches!(
        decode(&bytes, RUN, TOKEN, &mut next_seq),
        Err(ProtocolError::Oversize)
    ));
}

#[test]
fn rejects_truncated_frame() {
    // Declares a 32-byte body but only provides 3 bytes.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&32u32.to_le_bytes());
    bytes.extend_from_slice(b"{\"a");
    let mut next_seq = 1u64;
    assert!(matches!(
        decode(&bytes, RUN, TOKEN, &mut next_seq),
        Err(ProtocolError::BadLength)
    ));
}

#[test]
fn rejects_non_utf8_body() {
    let body = [0xff, 0xfe, 0xfd, 0xfc];
    let mut next_seq = 1u64;
    assert!(matches!(
        decode(&raw_frame(&body), RUN, TOKEN, &mut next_seq),
        Err(ProtocolError::BadUtf8)
    ));
}

#[test]
fn rejects_malformed_json() {
    let mut next_seq = 1u64;
    assert!(matches!(
        decode(&frame("not json"), RUN, TOKEN, &mut next_seq),
        Err(ProtocolError::BadJson)
    ));
}

#[test]
fn rejects_additional_properties() {
    let body = format!(
        r#"{{"protocolVersion":1,"type":"hello","runId":"{RUN}","token":"{TOKEN}","sequence":1,"extra":true}}"#
    );
    let mut next_seq = 1u64;
    assert!(matches!(
        decode(&frame(&body), RUN, TOKEN, &mut next_seq),
        Err(ProtocolError::ExtraField)
    ));
}

#[test]
fn rejects_wrong_protocol_version() {
    let body = format!(
        r#"{{"protocolVersion":2,"type":"hello","runId":"{RUN}","token":"{TOKEN}","sequence":1}}"#
    );
    let mut next_seq = 1u64;
    assert!(matches!(
        decode(&frame(&body), RUN, TOKEN, &mut next_seq),
        Err(ProtocolError::BadVersion)
    ));
}

#[test]
fn rejects_wrong_run_id() {
    let mut next_seq = 1u64;
    let body = hello(1, "other-run", TOKEN);
    assert!(matches!(
        decode(&frame(&body), RUN, TOKEN, &mut next_seq),
        Err(ProtocolError::BadRun)
    ));
}

#[test]
fn rejects_wrong_token() {
    let mut next_seq = 1u64;
    let body = hello(
        1,
        RUN,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    assert!(matches!(
        decode(&frame(&body), RUN, TOKEN, &mut next_seq),
        Err(ProtocolError::BadToken)
    ));
}

#[test]
fn rejects_sequence_ahead_of_expected() {
    // next_seq starts at 1; a first frame with sequence 2 jumps ahead.
    let mut next_seq = 1u64;
    let body = hello(2, RUN, TOKEN);
    assert!(matches!(
        decode(&frame(&body), RUN, TOKEN, &mut next_seq),
        Err(ProtocolError::BadSequence)
    ));
}

#[test]
fn rejects_replayed_sequence() {
    let mut next_seq = 1u64;
    assert!(decode(&frame(&hello(1, RUN, TOKEN)), RUN, TOKEN, &mut next_seq).is_ok());
    assert_eq!(next_seq, 2);
    // Replay the already-accepted sequence.
    assert!(matches!(
        decode(&frame(&hello(1, RUN, TOKEN)), RUN, TOKEN, &mut next_seq),
        Err(ProtocolError::Replay)
    ));
    assert_eq!(
        next_seq, 2,
        "a rejected frame must not advance the sequence"
    );
}

#[test]
fn requires_hello_as_first_frame() {
    let mut next_seq = 1u64;
    let body = ready(1, "127.0.0.1", 49152, "http://127.0.0.1:49152");
    assert!(matches!(
        decode(&frame(&body), RUN, TOKEN, &mut next_seq),
        Err(ProtocolError::BadSequence)
    ));
}

#[test]
fn rejects_non_loopback_ready_host() {
    let mut next_seq = 1u64;
    assert!(decode(&frame(&hello(1, RUN, TOKEN)), RUN, TOKEN, &mut next_seq).is_ok());
    let body = ready(2, "0.0.0.0", 49152, "http://0.0.0.0:49152");
    assert!(matches!(
        decode(&frame(&body), RUN, TOKEN, &mut next_seq),
        Err(ProtocolError::BadField)
    ));
}

#[test]
fn rejects_ready_url_mismatch() {
    let mut next_seq = 1u64;
    assert!(decode(&frame(&hello(1, RUN, TOKEN)), RUN, TOKEN, &mut next_seq).is_ok());
    // port says 49152 but url points elsewhere.
    let body = ready(2, "127.0.0.1", 49152, "http://127.0.0.1:1");
    assert!(matches!(
        decode(&frame(&body), RUN, TOKEN, &mut next_seq),
        Err(ProtocolError::BadField)
    ));
}

#[test]
fn rejects_zero_port_ready() {
    let mut next_seq = 1u64;
    assert!(decode(&frame(&hello(1, RUN, TOKEN)), RUN, TOKEN, &mut next_seq).is_ok());
    let body = ready(2, "127.0.0.1", 0, "http://127.0.0.1:0");
    assert!(matches!(
        decode(&frame(&body), RUN, TOKEN, &mut next_seq),
        Err(ProtocolError::BadField)
    ));
}

#[test]
fn encode_shutdown_round_trips_the_frame() {
    let bytes = encode_shutdown(7);
    assert!(bytes.len() >= 4);
    let declared = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    assert_eq!(declared, bytes.len() - 4);
    let body: serde_json::Value = serde_json::from_slice(&bytes[4..]).expect("valid shutdown JSON");
    assert_eq!(body["protocolVersion"], 1);
    assert_eq!(body["type"], "shutdown");
    assert_eq!(body["sequence"], 7);
}
