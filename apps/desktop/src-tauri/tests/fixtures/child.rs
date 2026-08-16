//! Controlled Windows fixture child for supervisor integration tests.
//!
//! This binary is spawned suspended by `job::Job::create_suspended` and assigned to a Job before
//! resume. It deliberately stays alive long enough for the test to observe Job membership, and can
//! spawn a grandchild that inherits Job membership (default CreateProcess behavior from a Job member).
//!
//! Modes (argv):
//!   --grandchild <exe>   spawn this grandchild (stdio nulled) before sleeping
//!   --ms <n>             sleep for n milliseconds (default 30000)

use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[allow(clippy::zombie_processes)] // Intentionally leaves the grandchild alive for Job cleanup tests.
fn main() {
    let mut grandchild: Option<String> = None;
    let mut millis: u64 = 30_000;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--grandchild" => {
                grandchild = Some(
                    args.get(i + 1)
                        .cloned()
                        .expect("--grandchild requires an executable path"),
                );
                i += 2;
            }
            "--ms" => {
                millis = args
                    .get(i + 1)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(30_000);
                i += 2;
            }
            _ => i += 1,
        }
    }

    if let Some(exe) = grandchild {
        let _grandchild = Command::new(&exe)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn grandchild");
    }

    thread::sleep(Duration::from_millis(millis));
}
