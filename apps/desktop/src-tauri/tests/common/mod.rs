//! Shared helpers for desktop integration tests.
//!
//! Cargo does not treat `tests/common/mod.rs` as a test target (only top-level `tests/*.rs` files
//! are auto-discovered), so this module is free to hold helpers shared across the contract tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A uniquely-named temporary directory that removes itself (best effort) on drop.
#[allow(dead_code)]
pub struct TempDir(PathBuf);

#[allow(dead_code)]
impl TempDir {
    pub fn new(prefix: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = format!(
            "{prefix}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).expect("create temp dir");
        TempDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Compiled path to the controlled fixture child binary (see `tests/fixtures/child.rs`).
#[allow(dead_code)]
pub fn fixture_child() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fixture-child"))
}

/// Compiled path to the controlled fixture grandchild binary (see `tests/fixtures/grandchild.rs`).
#[allow(dead_code)]
pub fn fixture_grandchild() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fixture-grandchild"))
}
