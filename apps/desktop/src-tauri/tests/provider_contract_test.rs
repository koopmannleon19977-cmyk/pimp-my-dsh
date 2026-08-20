//! Contract tests for the development provider launch spec.
//!
//! The development provider exists only under `debug_assertions` and resolves the backend-owned
//! absolute `node.exe`, the CLI entry, the working directory, and an explicit environment with the
//! fixed argv required by local://desktop-contracts.md §Provider launch contract.

#![cfg(debug_assertions)]

use pimp_dsh_desktop::compatibility::{DevProvider, Provider};
#[test]
fn dev_provider_resolves_absolute_backend_owned_launch_spec() {
    let spec = DevProvider
        .resolve()
        .expect("dev provider must resolve in a dev workspace build");

    assert!(
        spec.node_exe.is_absolute(),
        "node.exe must be absolute, got {:?}",
        spec.node_exe
    );
    assert!(!spec.node_exe.as_os_str().is_empty());
    assert!(
        spec.cli_entry.is_absolute(),
        "CLI entry must be absolute, got {:?}",
        spec.cli_entry
    );
    assert_eq!(
        spec.cli_entry.file_name().and_then(|f| f.to_str()),
        Some("cli.js"),
        "the CLI entry must be the validated dist/cli.js"
    );
    assert!(spec.cwd.is_absolute(), "cwd must be absolute");
}

#[test]
fn dev_provider_uses_the_fixed_argv_contract() {
    let spec = DevProvider.resolve().expect("dev provider must resolve");

    let argv: Vec<String> = spec
        .args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();

    assert_eq!(
        argv,
        [
            "run",
            "--profile",
            "web",
            "--",
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--no-open",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<String>>(),
        "args must be `run --profile web -- --host 127.0.0.1 --port 0 --no-open` (CLI entry is a separate field, no shell)"
    );
}

#[test]
fn dev_provider_environment_carries_no_public_secret_names() {
    let spec = DevProvider.resolve().expect("dev provider must resolve");

    for (key, _value) in &spec.env {
        let name = key.to_string_lossy();
        assert!(
            !name.to_ascii_uppercase().starts_with("PIMP_DSH_"),
            "must not pass the public secret name {name:?} to the child"
        );
    }
}

#[test]
fn web_argv_matches_the_provider_contract_shape() {
    let argv = pimp_dsh_desktop::compatibility::web_argv();
    let joined: Vec<String> = argv
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        joined,
        [
            "run",
            "--profile",
            "web",
            "--",
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--no-open"
        ]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<String>>()
    );
}
