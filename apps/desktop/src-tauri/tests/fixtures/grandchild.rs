//! Controlled Windows fixture grandchild: a second process in the tree that only sleeps.
//!
//! The supervisor integration tests spawn this via the fixture child (`--grandchild`), verify it
//! inherits Job membership, and assert it is torn down by `TerminateJobObject` / kill-on-close
//! without any PID lookup.

use std::thread;
use std::time::Duration;

fn main() {
    thread::sleep(Duration::from_millis(30_000));
}
