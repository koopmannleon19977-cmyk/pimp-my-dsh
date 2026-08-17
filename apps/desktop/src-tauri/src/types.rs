//! Renderer-facing data-transfer types.
//!
//! Every struct in this module is `serde`-serialized to the renderer with
//! `camelCase` field names and closed enums, matching the v1 snapshot contract
//! in `docs/desktop-launcher-research.md` / `local://desktop-contracts.md`.
//! No secret value (token, `PIMP_DSH_*` / `DSH_PIMP_*` environment values) is
//! ever represented here.

use serde::{Deserialize, Serialize};

use crate::logging::LogEvent;
use crate::state::{Reason, State};

/// The renderer theme preference. A closed enum; unknown values are rejected
/// at the command boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestartPolicy {
    #[default]
    Never,
    Always,
}

/// User-adjustable supervisor settings. `fixed_port` is the opt-in compatibility
/// port (1..=65535); `None` means dynamic (`--port 0`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub theme: Theme,
    pub fixed_port: Option<u16>,
    pub restart_policy: RestartPolicy,
    pub notifications_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::System,
            fixed_port: None,
            restart_policy: RestartPolicy::Never,
            notifications_enabled: false,
        }
    }
}

/// A single health facet. `id` is a stable kebab-case identifier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    pub id: String,
    #[serde(rename = "status")]
    pub status: HealthStatus,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HealthStatus {
    Ok,
    Warning,
    Error,
}

/// Structured result of `run_doctor`. `ok` is false when the doctor invocation
/// itself failed; the remaining fields are `None` when unset/unknown.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorResult {
    pub ok: bool,
    pub error: Option<String>,
    pub node: Option<String>,
    pub platform: Option<String>,
    pub architecture: Option<String>,
    pub dsh_available: Option<bool>,
    pub dsh_error: Option<String>,
    pub profile_ready: Option<bool>,
    pub api_key_configured: Option<bool>,
    pub base_url_configured: Option<bool>,
    pub model_configured: Option<bool>,
    pub lsp_enabled: Option<bool>,
    pub telemetry_enabled: Option<bool>,
}

/// Compatibility surface shown to the renderer. `verified` is true when the
/// active provider's preflight (packaged manifest + payload hash, or development
/// workspace identity + installed versions) has succeeded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityView {
    pub controller_version: String,
    pub distribution_version: String,
    pub dsh_version: String,
    pub node_version: String,
    pub pnpm_version: String,
    pub target: String,
    pub verified: bool,
}

impl Default for CompatibilityView {
    fn default() -> Self {
        CompatibilityView {
            controller_version: crate::compatibility::CONTROLLER_VERSION.to_string(),
            distribution_version: crate::compatibility::DISTRIBUTION_VERSION.to_string(),
            dsh_version: crate::compatibility::DSH_VERSION.to_string(),
            node_version: crate::compatibility::NODE_VERSION.to_string(),
            pnpm_version: crate::compatibility::PNPM_VERSION.to_string(),
            target: crate::compatibility::TARGET.to_string(),
            verified: false,
        }
    }
}

/// Terminal outcome of a supervised run, recorded into recent history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunOutcome {
    Graceful,
    Forced,
    Crashed,
    FailedStart,
}

/// One completed run, newest-first in [`Snapshot::recent_runs`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub run_id: String,
    pub started_at: String,
    pub ended_at: String,
    pub outcome: RunOutcome,
    pub reason: String,
}

/// The complete v1 supervisor snapshot. Rust emits this whole value (never a
/// delta) on the `supervisor://snapshot` event after every revision change.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub protocol_version: u32,
    pub revision: u64,
    pub state: State,
    pub reason: Reason,
    pub run_id: Option<String>,
    pub endpoint: Option<String>,
    pub profile: String,
    pub uptime_ms: Option<u64>,
    pub last_transition_at: String,
    pub busy: bool,
    pub health: Vec<HealthCheck>,
    pub recent_runs: Vec<RunRecord>,
    pub doctor: Option<DoctorResult>,
    pub logs: Vec<LogEvent>,
    pub settings: Settings,
    pub compatibility: CompatibilityView,
    pub logging_fault: Option<String>,
}

impl Snapshot {
    /// A minimal, contract-valid snapshot at the given revision/state/reason.
    /// Used by the pure state facade; the full supervisor overrides the
    /// resource-backed fields.
    pub fn minimal(
        revision: u64,
        state: State,
        reason: Reason,
        last_transition_at: String,
    ) -> Self {
        Snapshot {
            protocol_version: 1,
            revision,
            state,
            reason,
            run_id: None,
            endpoint: None,
            profile: "web".to_string(),
            uptime_ms: None,
            last_transition_at,
            busy: false,
            health: Vec::new(),
            recent_runs: Vec::new(),
            doctor: None,
            logs: Vec::new(),
            settings: Settings::default(),
            compatibility: CompatibilityView::default(),
            logging_fault: None,
        }
    }
}

/// `true` when the state admits an "Open Web UI" primary action, i.e. the
/// harness is ready/running with a validated endpoint.
pub fn state_allows_open(state: State) -> bool {
    matches!(state, State::Ready | State::Running)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_serializes_kebab_case() {
        let json = serde_json::to_string(&Theme::Light).unwrap();
        assert_eq!(json, "\"light\"");
        let parsed: Theme = serde_json::from_str("\"dark\"").unwrap();
        assert_eq!(parsed, Theme::Dark);
        assert!(serde_json::from_str::<Theme>("\"neon\"").is_err());
    }

    #[test]
    fn settings_serialize_camel_case() {
        let s = Settings {
            theme: Theme::Dark,
            fixed_port: Some(3080),
            restart_policy: RestartPolicy::Never,
            notifications_enabled: false,
        };
        let v: serde_json::Value = serde_json::to_value(&s).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["fixedPort"], 3080);
        assert_eq!(v["notificationsEnabled"], false);
    }

    #[test]
    fn snapshot_fields_match_v1_contract() {
        let snap = Snapshot::minimal(0, State::Stopped, Reason::Idle, "t".to_string());
        let v: serde_json::Value = serde_json::to_value(&snap).unwrap();
        assert_eq!(v["protocolVersion"], 1);
        assert_eq!(v["revision"], 0);
        assert_eq!(v["state"], "stopped");
        assert_eq!(v["profile"], "web");
        assert!(v.get("token").is_none(), "token must never be present");
    }

    #[test]
    fn open_only_in_ready_or_running() {
        assert!(state_allows_open(State::Ready));
        assert!(state_allows_open(State::Running));
        assert!(!state_allows_open(State::Stopped));
        assert!(!state_allows_open(State::Stopping));
    }
}
