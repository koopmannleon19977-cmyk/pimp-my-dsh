//! Contract tests for the compatibility manifest v1 and deterministic payload tree hash.
//!
//! Reimplements the staging payload-hash algorithm independently so any drift between the staging
//! script and the packaged verifier is caught: walk the payload tree excluding `manifest.json`;
//! per file, record = 32 raw SHA-256 bytes || relpath UTF-8 bytes ('/' separators) || 0x00;
//! sort records by relpath bytes ascending; payloadSha256 = hex(SHA256(concat(records))).

use std::path::{Path, PathBuf};

use pimp_dsh_desktop::manifest::CompatManifest;
use sha2::{Digest, Sha256};

mod common;
use common::TempDir;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, PathBuf)>) {
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out);
        } else {
            let rel = path.strip_prefix(root).expect("strip root").to_path_buf();
            out.push((path, rel));
        }
    }
}

fn payload_sha256(dir: &Path) -> String {
    let mut files: Vec<(PathBuf, PathBuf)> = Vec::new();
    collect_files(dir, dir, &mut files);
    files.retain(|(_, rel)| rel != Path::new("manifest.json"));

    let mut records: Vec<(String, Vec<u8>)> = Vec::new();
    for (abs, rel) in files {
        let rel_norm = rel.to_string_lossy().replace('\\', "/");
        let mut record = Sha256::digest(std::fs::read(&abs).expect("read file")).to_vec();
        record.extend_from_slice(rel_norm.as_bytes());
        record.push(0u8);
        records.push((rel_norm, record));
    }
    records.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));

    let mut concat = Vec::new();
    for (_, record) in records {
        concat.extend_from_slice(&record);
    }
    hex(&Sha256::digest(&concat))
}

/// Build a minimal contract-shaped payload tree and return the matching manifest JSON.
fn build_tree(dir: &Path) -> String {
    std::fs::create_dir_all(dir.join("node")).unwrap();
    std::fs::create_dir_all(dir.join("cli")).unwrap();
    let node_bytes = b"fake-node-exe-bytes-for-hashing";
    std::fs::write(dir.join("node/node.exe"), node_bytes).unwrap();
    std::fs::write(dir.join("cli/cli.js"), b"fake cli entry\n").unwrap();
    std::fs::write(dir.join("package.json"), b"{}\n").unwrap();

    serde_json::json!({
        "schemaVersion": 1,
        "protocolVersion": 1,
        "controllerVersion": "0.1.0",
        "node": { "version": "24.19.0", "sha256": sha256_hex(node_bytes) },
        "pnpmVersion": "11.7.0",
        "distributionVersion": "0.1.0",
        "dshVersion": "0.1.0-rc.6",
        "target": "x86_64-pc-windows-msvc",
        "payloadSha256": payload_sha256(dir),
    })
    .to_string()
}

/// True when a manifest string is rejected by parse OR verify (robust to the parse/verify split).
fn rejected(json: &str, payload_dir: &Path) -> bool {
    match CompatManifest::parse(json) {
        Err(_) => true,
        Ok(manifest) => manifest.verify(payload_dir).is_err(),
    }
}

#[test]
fn valid_manifest_parses_and_verifies() {
    let dir = TempDir::new("pimp-dsh-manifest");
    let json = build_tree(dir.path());
    let manifest = CompatManifest::parse(&json).expect("valid manifest must parse");
    assert!(
        manifest.verify(dir.path()).is_ok(),
        "matching payload must verify"
    );
}

#[test]
fn parse_rejects_an_unknown_field() {
    let dir = TempDir::new("pimp-dsh-manifest");
    let json = build_tree(dir.path());
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("extra".into(), serde_json::json!(true));
    assert!(
        CompatManifest::parse(&value.to_string()).is_err(),
        "additionalProperties:false must reject unknown fields"
    );
}

#[test]
fn parse_rejects_a_missing_required_field() {
    let dir = TempDir::new("pimp-dsh-manifest");
    let json = build_tree(dir.path());
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value.as_object_mut().unwrap().remove("payloadSha256");
    assert!(
        CompatManifest::parse(&value.to_string()).is_err(),
        "a missing required field must be rejected"
    );
}

#[test]
fn verify_rejects_a_tampered_payload_file() {
    let dir = TempDir::new("pimp-dsh-manifest");
    let json = build_tree(dir.path());
    let manifest = CompatManifest::parse(&json).unwrap();
    std::fs::write(dir.path().join("cli/cli.js"), b"tampered").unwrap();
    assert!(
        manifest.verify(dir.path()).is_err(),
        "tampered payload must fail verification"
    );
}

#[test]
fn verify_rejects_wrong_node_sha256() {
    let dir = TempDir::new("pimp-dsh-manifest");
    let json = build_tree(dir.path());
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value["node"]["sha256"] = serde_json::json!("00".repeat(32));
    let manifest = CompatManifest::parse(&value.to_string()).unwrap();
    assert!(
        manifest.verify(dir.path()).is_err(),
        "wrong node.exe hash must fail verification"
    );
}

#[test]
fn verify_rejects_wrong_payload_sha256() {
    let dir = TempDir::new("pimp-dsh-manifest");
    let json = build_tree(dir.path());
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value["payloadSha256"] = serde_json::json!("00".repeat(32));
    let manifest = CompatManifest::parse(&value.to_string()).unwrap();
    assert!(
        manifest.verify(dir.path()).is_err(),
        "wrong payload tree hash must fail verification"
    );
}

#[test]
fn rejects_a_mismatched_version_string() {
    let dir = TempDir::new("pimp-dsh-manifest");
    let json = build_tree(dir.path());
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value["dshVersion"] = serde_json::json!("0.1.0-rc.5");
    assert!(
        rejected(&value.to_string(), dir.path()),
        "mismatched dshVersion must be rejected"
    );
}

#[test]
fn rejects_a_mismatched_target() {
    let dir = TempDir::new("pimp-dsh-manifest");
    let json = build_tree(dir.path());
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value["target"] = serde_json::json!("x86_64-unknown-linux-gnu");
    assert!(
        rejected(&value.to_string(), dir.path()),
        "mismatched target must be rejected"
    );
}
