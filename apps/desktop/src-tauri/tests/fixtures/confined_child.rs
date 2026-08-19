//! Controlled Windows fixture for the read-side confinement contract test.
//!
//! This binary is spawned suspended by `Job::create_suspended_with` under an
//! AppContainer. It tries to read a single file given as argv and reports the
//! outcome on stdout:
//!
//!   `READ_OK:<first non-empty line>`  exit 0  — the confined child could open it
//!   `READ_FAIL:<io error>`            exit 1  — the confined child was denied
//!
//! argv layout (from `job.rs::build_command_line`): [0]=exe, [1]=cli_entry,
//! [2]=first LaunchSpec arg = the file path to attempt reading.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| panic!("usage: confined_child <file-path>"));

    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let first = s.lines().next().unwrap_or_default().to_string();
            println!("READ_OK:{first}");
        }
        Err(e) => {
            eprintln!("READ_FAIL:{e}");
            std::process::exit(1);
        }
    }
}
