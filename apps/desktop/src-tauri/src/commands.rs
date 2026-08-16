//! Command surface. Every command is parameterless except `set_theme` and
//! `set_fixed_port`, whose accepted values are closed enums/ranges. No command
//! accepts a URL, path, executable, argv, environment, PID, pipe name, run
//! token, or lifecycle target.
//!
//! The functions here are the plain, testable surface (also used by the Tauri
//! command handlers in `lib.rs`); they operate on a process-global supervisor
//! initialized once at startup.

use std::sync::{Arc, OnceLock};

use crate::supervisor::Supervisor;
use crate::types::{Snapshot, Theme};

static SUPERVISOR: OnceLock<Arc<Supervisor>> = OnceLock::new();

/// Initialize the global supervisor (idempotent) and return the shared handle.
pub fn init_supervisor() -> Arc<Supervisor> {
    SUPERVISOR.get_or_init(Supervisor::new).clone()
}

fn supervisor() -> Arc<Supervisor> {
    SUPERVISOR
        .get()
        .expect("supervisor not initialized")
        .clone()
}

pub fn get_snapshot() -> Snapshot {
    supervisor().snapshot()
}

pub fn start_harness() -> Result<(), String> {
    supervisor().start()
}

pub fn stop_harness() -> Result<(), String> {
    supervisor().stop()
}

pub fn run_doctor() -> Result<(), String> {
    supervisor().run_doctor()
}

/// Open the validated READY/RUNNING endpoint. The endpoint is constructed in
/// Rust from the current snapshot; no renderer argument is accepted.
pub fn open_harness() -> Result<(), String> {
    supervisor().open()
}

pub fn reveal_log_folder() -> Result<(), String> {
    supervisor().reveal_log_folder()
}

pub fn set_theme(theme: Theme) -> Result<(), String> {
    supervisor().set_theme(theme)
}

pub fn set_fixed_port(port: Option<u16>) -> Result<(), String> {
    supervisor().set_fixed_port(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_fixed_port_validates_range() {
        let _ = init_supervisor();
        assert!(set_fixed_port(Some(3080)).is_ok());
        assert!(set_fixed_port(Some(0)).is_err());
        assert!(set_fixed_port(Some(65535)).is_ok());
        assert!(set_fixed_port(None).is_ok());
    }

    #[test]
    fn set_theme_accepts_closed_enum_only() {
        // Theme is a closed Rust enum; the serde boundary rejects unknown
        // strings at deserialization (tested in types.rs). The command simply
        // stores a valid variant.
        let _ = init_supervisor();
        assert!(set_theme(Theme::Dark).is_ok());
        let snap = get_snapshot();
        assert_eq!(snap.settings.theme, Theme::Dark);
    }
}
