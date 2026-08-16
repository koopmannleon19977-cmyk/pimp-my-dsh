//! Contract tests for renderer authority rejection: the only parameterized commands accept a
//! closed theme enum and a validated fixed port. Arbitrary strings (URLs, paths, executables) are
//! rejected at the serde boundary before any side effect.

use pimp_dsh_desktop::commands::{init_supervisor, set_fixed_port, set_theme};
use pimp_dsh_desktop::types::Theme;
use serde_json::Value;

#[test]
fn theme_is_a_closed_enum() {
    for (raw, expected) in [("system", "system"), ("light", "light"), ("dark", "dark")] {
        let theme: Theme = serde_json::from_str(&format!("\"{raw}\"")).expect("valid theme");
        assert_eq!(
            serde_json::to_value(theme).expect("serialize"),
            Value::String(expected.into())
        );
    }
}

#[test]
fn theme_rejects_arbitrary_authority_strings() {
    for bad in [
        "http://evil.example",
        "file:///C:/Windows/System32/cmd.exe",
        "cmd.exe",
        "run",
        "start_harness",
        "127.0.0.1:8080",
        "\\\\server\\share",
        "SYSTEM",
    ] {
        let json = format!("\"{bad}\"");
        assert!(
            serde_json::from_str::<Theme>(&json).is_err(),
            "theme {bad:?} must be rejected as a non-theme authority string"
        );
    }
}

#[test]
fn set_theme_accepts_only_the_closed_enum() {
    init_supervisor();
    assert!(set_theme(Theme::System).is_ok());
    assert!(set_theme(Theme::Light).is_ok());
    assert!(set_theme(Theme::Dark).is_ok());
}

#[test]
fn set_fixed_port_accepts_the_closed_range_and_null() {
    init_supervisor();
    assert!(set_fixed_port(None).is_ok(), "null clears the fixed port");
    assert!(set_fixed_port(Some(1)).is_ok());
    assert!(set_fixed_port(Some(8080)).is_ok());
    assert!(set_fixed_port(Some(65_535)).is_ok());
}

#[test]
fn set_fixed_port_rejects_zero() {
    init_supervisor();
    assert!(
        set_fixed_port(Some(0)).is_err(),
        "port 0 is reserved for the OS-assigned dynamic path"
    );
}
