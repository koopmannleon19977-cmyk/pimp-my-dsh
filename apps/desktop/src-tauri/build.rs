use std::path::PathBuf;

use sha2::{Digest, Sha256};

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

fn main() {
    // Independently pin the staged compatibility manifest so the packaged
    // provider can authenticate the external `manifest.json` BEFORE trusting
    // its self-described payload/node hashes. Staging (`stage-runtime.ps1`)
    // must run first; a missing manifest leaves the digest empty and the
    // packaged provider fails closed rather than trusting an unpinned manifest.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("runtime")
        .join("manifest.json");
    println!("cargo:rerun-if-changed=runtime/manifest.json");

    let digest = match std::fs::read(&manifest) {
        Ok(bytes) => hex_lower(&Sha256::digest(bytes)),
        Err(e) => {
            println!(
                "cargo:warning=runtime/manifest.json not found ({e}); the packaged provider will fail closed until scripts/stage-runtime.ps1 runs"
            );
            String::new()
        }
    };
    println!("cargo:rustc-env=EXPECTED_MANIFEST_SHA256={digest}");

    tauri_build::build()
}
