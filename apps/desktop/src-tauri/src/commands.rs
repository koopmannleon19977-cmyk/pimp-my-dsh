//! Command surface. Every command is parameterless except `set_theme`,
//! `set_fixed_port`, `set_autostart`, and `set_notifications_enabled`, whose
//! accepted values are closed enums/ranges/bools. No command accepts a URL,
//! path, executable, argv, environment, PID, pipe name, run token, or
//! lifecycle target.
//!
//! The functions here are the plain, testable surface (also used by the Tauri
//! command handlers in `lib.rs`); they operate on a process-global supervisor
//! initialized once at startup.

use std::sync::{Arc, OnceLock};

use crate::supervisor::Supervisor;
use crate::types::{RestartPolicy, Snapshot, Theme};

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

/// Return the validated READY/RUNNING endpoint (fail closed otherwise). The
/// endpoint is constructed in Rust from the current snapshot; no renderer
/// argument is accepted. Opening the window is the Tauri layer's concern.
pub fn validated_endpoint() -> Result<String, String> {
    supervisor().validated_endpoint()
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

pub fn set_restart_policy(policy: RestartPolicy) -> Result<(), String> {
    supervisor().set_restart_policy(policy)
}

pub fn set_notifications_enabled(enabled: bool) -> Result<(), String> {
    supervisor().set_notifications_enabled(enabled)
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

    #[test]
    fn set_restart_policy_stores_value() {
        let _ = init_supervisor();
        assert!(set_restart_policy(RestartPolicy::Always).is_ok());
        let snap = get_snapshot();
        assert_eq!(snap.settings.restart_policy, RestartPolicy::Always);
    }

    #[test]
    fn set_notifications_enabled_stores_value() {
        let _ = init_supervisor();
        assert!(set_notifications_enabled(true).is_ok());
        let snap = get_snapshot();
        assert_eq!(snap.settings.notifications_enabled, true);
    }

    #[test]
    fn validated_endpoint_fails_closed_when_not_running() {
        let _ = init_supervisor();
        // A fresh supervisor is Stopped: opening must fail closed.
        assert!(validated_endpoint().is_err());
    }
}
