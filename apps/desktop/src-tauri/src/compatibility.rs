//! Provider launch contract.
//!
//! Both providers return only backend-owned absolute `node.exe`, CLI entry,
//! cwd, and an explicit environment. The fixed argv is
//! `CLI run --profile web -- --host 127.0.0.1 --port <0|fixed>`; Rust always
//! invokes Node with that argv via `CreateProcessW` (no shell).
//!
//! The development provider exists only in debug builds and verifies absolute
//! workspace identity plus installed versions. The packaged provider resolves
//! strictly under the canonicalized app `resource_dir/runtime`, authenticates
//! the manifest against a build-time-pinned digest, and never consults `PATH`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Exact compatibility constants (shared with the manifest + protocol modules).
pub const CONTROLLER_VERSION: &str = "0.1.0";
pub const DISTRIBUTION_VERSION: &str = "0.1.0";
pub const DSH_VERSION: &str = "0.1.0-rc.7";
pub const NODE_VERSION: &str = "24.19.0";
pub const PNPM_VERSION: &str = "11.7.0";
pub const TARGET: &str = "x86_64-pc-windows-msvc";

/// SHA-256 of the canonical staged `runtime/manifest.json`, pinned at build
/// time (`build.rs`) and independent of the manifest's own self-described
/// hashes. Empty when staging has not run; the packaged provider fails closed.
const EXPECTED_MANIFEST_SHA256: &str = match option_env!("EXPECTED_MANIFEST_SHA256") {
    Some(v) => v,
    None => "",
};

/// Backend-owned launch specification. `args` is the argv AFTER `node.exe`,
/// beginning with `run` (the `cli_entry` is the fixed first argv element).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchSpec {
    pub node_exe: PathBuf,
    pub cli_entry: PathBuf,
    pub cwd: PathBuf,
    pub env: Vec<(OsString, OsString)>,
    pub args: Vec<OsString>,
}

impl LaunchSpec {
    /// Rewrite the trailing `--port` value to a validated fixed port, or `0`
    /// (dynamic) when `None` or out of range.
    pub fn set_port(&mut self, port: Option<u16>) {
        if self.args.len() >= 2 && self.args[self.args.len() - 2] == "--port" {
            let value = match port {
                Some(p) if (1..=65535).contains(&p) => p.to_string(),
                _ => "0".to_string(),
            };
            let last = self.args.len() - 1;
            self.args[last] = OsString::from(value);
        }
    }
}

/// The fixed child argv (excluding the CLI entry): `run --profile web -- …`.
pub fn web_argv() -> Vec<OsString> {
    [
        "run",
        "--profile",
        "web",
        "--",
        "--host",
        "127.0.0.1",
        "--port",
        "0",
    ]
    .iter()
    .map(OsString::from)
    .collect()
}

pub trait Provider {
    fn resolve(&self) -> Result<LaunchSpec, String>;
}

/// Development provider: debug builds only.
#[cfg(debug_assertions)]
#[derive(Default)]
pub struct DevProvider;

#[cfg(debug_assertions)]
impl Provider for DevProvider {
    fn resolve(&self) -> Result<LaunchSpec, String> {
        let workspace = workspace_root()?;
        verify_workspace_identity(&workspace)?;
        let node_exe = resolve_node()?;
        verify_pnpm(&workspace)?;
        let cli_entry = workspace.join("dist").join("cli.js");
        if !cli_entry.is_file() {
            return Err(format!(
                "CLI entry missing: {} (run `pnpm build` first)",
                cli_entry.display()
            ));
        }
        Ok(LaunchSpec {
            node_exe,
            cli_entry,
            cwd: workspace,
            env: inherited_env(),
            args: web_argv(),
        })
    }
}

/// Packaged provider: authenticates the staged manifest against an
/// independently pinned digest, then verifies the payload hashes, and never
/// consults `PATH`. The runtime is resolved strictly under the canonicalized
/// Tauri app `resource_dir/runtime`, never the source checkout.
pub struct PackagedProvider {
    resource_dir: PathBuf,
}

impl PackagedProvider {
    pub fn new(resource_dir: PathBuf) -> Self {
        Self { resource_dir }
    }
}

impl Provider for PackagedProvider {
    fn resolve(&self) -> Result<LaunchSpec, String> {
        resolve_packaged(&self.resource_dir, EXPECTED_MANIFEST_SHA256)
    }
}

/// Packaged resolution core, testable with a controlled manifest digest.
fn resolve_packaged(
    resource_dir: &Path,
    expected_manifest_sha256: &str,
) -> Result<LaunchSpec, String> {
    let runtime = packaged_runtime_dir(resource_dir)?;
    let manifest_path = runtime.join("manifest.json");
    let json = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("manifest missing at {}: {e}", manifest_path.display()))?;
    // Authenticate the manifest BEFORE trusting its self-described hashes.
    crate::manifest::verify_manifest_digest(&json, expected_manifest_sha256)?;
    let manifest = crate::manifest::CompatManifest::parse(&json)?;
    manifest.verify(&runtime)?;

    // `std::fs::canonicalize` returns a `\\?\` verbatim path on Windows.
    // Node 24 treats such a script argv as `C:` and fails in runMain, so retain
    // the canonical target for verification but pass its equivalent DOS/UNC
    // spelling to Node.
    let launch_runtime = node_compatible_path(&runtime)?;
    let node_exe = launch_runtime.join("node").join("node.exe");
    let cli_entry = launch_runtime.join("cli").join("cli.js");
    if !node_exe.is_file() {
        return Err(format!("node.exe missing at {}", node_exe.display()));
    }
    if !cli_entry.is_file() {
        return Err(format!("cli.js missing at {}", cli_entry.display()));
    }
    Ok(LaunchSpec {
        node_exe,
        cli_entry,
        cwd: launch_runtime,
        env: inherited_env(),
        args: web_argv(),
    })
}

/// The packaged payload dir: the canonicalized `resource_dir/runtime`. The
/// resource directory is the Tauri app resource dir, so the installed path is
/// independent of the source checkout.
fn packaged_runtime_dir(resource_dir: &Path) -> Result<PathBuf, String> {
    let joined = resource_dir.join("runtime");
    std::fs::canonicalize(&joined)
        .map_err(|e| format!("packaged runtime missing at {}: {e}", joined.display()))
}

#[cfg(windows)]
fn node_compatible_path(path: &Path) -> Result<PathBuf, String> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    const VERBATIM: &[u16] = &[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    const VERBATIM_UNC: &[u16] = &[
        b'\\' as u16,
        b'\\' as u16,
        b'?' as u16,
        b'\\' as u16,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        b'\\' as u16,
    ];

    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let normalized = if wide.starts_with(VERBATIM_UNC) {
        let mut normalized = Vec::with_capacity(wide.len() - VERBATIM_UNC.len() + 2);
        normalized.extend_from_slice(&VERBATIM[..2]);
        normalized.extend_from_slice(&wide[VERBATIM_UNC.len()..]);
        normalized
    } else if wide.starts_with(VERBATIM) {
        let drive = wide.get(4).copied().unwrap_or_default();
        if wide.len() < 7
            || !((b'A' as u16..=b'Z' as u16).contains(&drive)
                || (b'a' as u16..=b'z' as u16).contains(&drive))
            || wide[5] != b':' as u16
            || wide[6] != b'\\' as u16
        {
            return Err(format!(
                "unsupported canonical runtime path: {}",
                path.display()
            ));
        }
        wide[VERBATIM.len()..].to_vec()
    } else {
        return Ok(path.to_path_buf());
    };

    Ok(PathBuf::from(OsString::from_wide(&normalized)))
}

#[cfg(not(windows))]
fn node_compatible_path(path: &Path) -> Result<PathBuf, String> {
    Ok(path.to_path_buf())
}

/// The repo root, derived from the compile-time manifest dir
/// (`apps/desktop/src-tauri` → repo root). Debug builds only.
#[cfg(debug_assertions)]
fn workspace_root() -> Result<PathBuf, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Ok(manifest.join("..").join("..").join(".."))
}

#[cfg(debug_assertions)]
fn verify_workspace_identity(workspace: &std::path::Path) -> Result<(), String> {
    let pkg = workspace.join("package.json");
    let content = std::fs::read_to_string(&pkg).map_err(|e| format!("read package.json: {e}"))?;
    let v: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("parse package.json: {e}"))?;
    if v["name"] != "pimp-my-dsh" {
        return Err("workspace is not pimp-my-dsh".to_string());
    }
    Ok(())
}

#[cfg(debug_assertions)]
fn resolve_node() -> Result<PathBuf, String> {
    let exe = if cfg!(windows) { "node.exe" } else { "node" };
    let mut found: Option<PathBuf> = None;
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(exe);
            if candidate.is_file() {
                found = Some(candidate);
                break;
            }
        }
    }
    let node = found.ok_or_else(|| format!("{exe} not found on PATH"))?;
    let output = std::process::Command::new(&node)
        .arg("--version")
        .output()
        .map_err(|e| format!("node --version failed: {e}"))?;
    if !output.status.success() {
        return Err("node --version failed".to_string());
    }
    let ver = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let expected = format!("v{NODE_VERSION}");
    if ver != expected {
        return Err(format!("node version {ver} != {expected}"));
    }
    Ok(node)
}

#[cfg(debug_assertions)]
fn verify_pnpm(workspace: &std::path::Path) -> Result<(), String> {
    let pkg = workspace
        .join("node_modules")
        .join("pnpm")
        .join("package.json");
    let content = std::fs::read_to_string(&pkg).map_err(|e| format!("pnpm not installed ({e})"))?;
    let v: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("parse pnpm package.json: {e}"))?;
    let ver = v["version"].as_str().unwrap_or_default();
    if ver != PNPM_VERSION {
        return Err(format!("pnpm version {ver} != {PNPM_VERSION}"));
    }
    Ok(())
}

fn inherited_env() -> Vec<(OsString, OsString)> {
    std::env::vars_os().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_argv_is_fixed_and_loopback() {
        let argv = web_argv();
        assert_eq!(argv[0], "run");
        assert_eq!(argv[1], "--profile");
        assert_eq!(argv[2], "web");
        assert_eq!(argv[3], "--");
        assert_eq!(argv[4], "--host");
        assert_eq!(argv[5], "127.0.0.1");
        assert_eq!(argv[6], "--port");
        assert_eq!(argv[7], "0");
        // No shell tokens, no remote host, no PATH fallback marker.
        assert!(
            !argv
                .iter()
                .any(|a| a == "cmd" || a == "/c" || a == "0.0.0.0")
        );
    }

    #[test]
    fn set_port_rewrites_dynamic_and_fixed() {
        let mut spec = LaunchSpec {
            node_exe: PathBuf::from("node.exe"),
            cli_entry: PathBuf::from("cli.js"),
            cwd: PathBuf::from("."),
            env: vec![],
            args: web_argv(),
        };
        spec.set_port(Some(3080));
        assert_eq!(spec.args[7], "3080");
        spec.set_port(None);
        assert_eq!(spec.args[7], "0");
        spec.set_port(Some(0)); // 0 is not a valid fixed port → dynamic
        assert_eq!(spec.args[7], "0");
        spec.set_port(Some(1)); // minimum valid fixed port
        assert_eq!(spec.args[7], "1");
    }

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

    fn sha256_hex(data: &[u8]) -> String {
        hex_lower(&Sha256::digest(data))
    }

    // Reimplement the deterministic payload tree hash to build fixtures.
    fn payload_tree_hash(root: &Path) -> String {
        let mut files: Vec<String> = Vec::new();
        collect_files(root, root, &mut files);
        let mut records: Vec<(String, Vec<u8>)> = Vec::new();
        for rel in files {
            let data = std::fs::read(root.join(&rel)).unwrap();
            let mut record = Sha256::digest(data).to_vec();
            record.extend_from_slice(rel.as_bytes());
            record.push(0);
            records.push((rel, record));
        }
        records.sort_by(|a, b| a.0.cmp(&b.0));
        let mut concat = Vec::new();
        for (_, rec) in records {
            concat.extend_from_slice(&rec);
        }
        hex_lower(&Sha256::digest(concat))
    }

    fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                collect_files(root, &path, out);
            } else if rel != "manifest.json" {
                out.push(rel);
            }
        }
    }

    fn manifest_for(runtime: &Path) -> String {
        let node_bytes = std::fs::read(runtime.join("node").join("node.exe")).unwrap();
        format!(
            "{{\"schemaVersion\":1,\"protocolVersion\":1,\"controllerVersion\":\"{CONTROLLER_VERSION}\",\"node\":{{\"version\":\"{NODE_VERSION}\",\"sha256\":\"{}\"}},\"pnpmVersion\":\"{PNPM_VERSION}\",\"distributionVersion\":\"{DISTRIBUTION_VERSION}\",\"dshVersion\":\"{DSH_VERSION}\",\"target\":\"{TARGET}\",\"payloadSha256\":\"{}\"}}",
            sha256_hex(&node_bytes),
            payload_tree_hash(runtime),
        )
    }

    // Build a minimal valid packaged runtime under `resource_dir/runtime` and
    // return the authentic manifest digest.
    fn build_packaged(resource_dir: &Path) -> String {
        let runtime = resource_dir.join("runtime");
        std::fs::create_dir_all(runtime.join("node")).unwrap();
        std::fs::create_dir_all(runtime.join("cli")).unwrap();
        std::fs::write(runtime.join("node").join("node.exe"), b"fake-node-exe").unwrap();
        std::fs::write(runtime.join("cli").join("cli.js"), b"fake cli\n").unwrap();
        std::fs::write(runtime.join("package.json"), b"{}\n").unwrap();
        let manifest = manifest_for(&runtime);
        std::fs::write(runtime.join("manifest.json"), manifest.as_bytes()).unwrap();
        sha256_hex(manifest.as_bytes())
    }

    #[test]
    fn packaged_runtime_dir_is_canonicalized_and_not_source_checkout() {
        let dir = std::env::temp_dir().join(format!("pimp-dsh-resdir-test-{}", std::process::id()));
        let resource = dir.join("resources");
        let runtime = resource.join("runtime");
        std::fs::create_dir_all(runtime.join("node")).unwrap();
        std::fs::write(runtime.join("manifest.json"), b"{}").unwrap();
        let resolved = packaged_runtime_dir(&resource).unwrap();
        assert_eq!(resolved, std::fs::canonicalize(&runtime).unwrap());
        assert_ne!(
            resolved,
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime"),
            "packaged runtime must not resolve under the source checkout"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn node_compatible_path_preserves_unicode_and_removes_verbatim_prefixes() {
        assert_eq!(
            node_compatible_path(Path::new(r"\\?\C:\Users\Zoë\Pimp my DSH")).unwrap(),
            PathBuf::from(r"C:\Users\Zoë\Pimp my DSH")
        );
        assert_eq!(
            node_compatible_path(Path::new(r"\\?\UNC\server\share\Pimp my DSH")).unwrap(),
            PathBuf::from(r"\\server\share\Pimp my DSH")
        );
    }
    #[test]
    fn resolve_packaged_resolves_under_resource_dir() {
        let dir =
            std::env::temp_dir().join(format!("pimp-dsh-resolve-test-{}", std::process::id()));
        let digest = build_packaged(&dir);
        let spec = resolve_packaged(&dir, &digest).unwrap();
        let canonical_runtime = std::fs::canonicalize(dir.join("runtime")).unwrap();
        let runtime = node_compatible_path(&canonical_runtime).unwrap();
        assert_eq!(spec.cwd, runtime);
        assert_eq!(spec.node_exe, runtime.join("node").join("node.exe"));
        assert_eq!(spec.cli_entry, runtime.join("cli").join("cli.js"));
        assert_ne!(
            spec.cwd,
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime"),
            "installed path must not depend on the source checkout"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_packaged_rejects_adjacent_manifest_replacement() {
        let dir = std::env::temp_dir().join(format!("pimp-dsh-tamper-test-{}", std::process::id()));
        let digest = build_packaged(&dir);
        assert!(resolve_packaged(&dir, &digest).is_ok());
        // Adjacent replacement: modify a payload file AND regenerate a
        // self-consistent manifest (fresh hashes). The pinned digest is
        // unchanged, so resolution must fail before trusting the replacement.
        let runtime = dir.join("runtime");
        std::fs::write(runtime.join("cli").join("cli.js"), b"tampered payload\n").unwrap();
        let replaced = manifest_for(&runtime);
        std::fs::write(runtime.join("manifest.json"), replaced.as_bytes()).unwrap();
        assert!(
            resolve_packaged(&dir, &digest).is_err(),
            "a replaced manifest must not authenticate a modified payload"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
