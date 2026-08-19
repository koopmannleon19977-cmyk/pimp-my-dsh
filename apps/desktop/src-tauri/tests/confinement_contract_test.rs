//! Focused Windows proof for the read-side confinement prototype (ADR-0004).
//!
//! The child and allowed fixture live in a fresh profile-owned AppContainer
//! root. A caller-readable file outside that root must be denied. Both normal
//! and failed-startup paths remove the private profile root.

#![cfg(windows)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use pimp_dsh_desktop::compatibility::LaunchSpec;
use pimp_dsh_desktop::confinement::Confinement;
use pimp_dsh_desktop::job::{FileHandle, Job};

struct UserProfileDir(PathBuf);

impl UserProfileDir {
    fn new(prefix: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let user_profile = std::env::var_os("USERPROFILE").expect("USERPROFILE is set on Windows");
        let path = PathBuf::from(user_profile).join(format!(
            ".{prefix}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("create user-profile fixture directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for UserProfileDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn confined_spec(child_exe: &Path, cwd: &Path, target: &Path) -> LaunchSpec {
    LaunchSpec {
        node_exe: child_exe.to_path_buf(),
        cli_entry: child_exe.to_path_buf(),
        cwd: cwd.to_path_buf(),
        // AppContainer profile initialization needs the caller's normal
        // environment; this mirrors the production LaunchSpec provider.
        env: std::env::vars_os().collect(),
        args: vec![OsString::from(target)],
    }
}

fn drain(pipe: &Option<FileHandle>) -> String {
    let mut bytes = Vec::new();
    if let Some(pipe) = pipe {
        let mut buf = [0u8; 256];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(count) => bytes.extend_from_slice(&buf[..count]),
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Bounded: a failed or stuck child is terminated through its Job before pipes
/// are read. This prevents a broken profile boundary from hanging the test.
fn run_confined(
    job: &Job,
    child_exe: &Path,
    cwd: &Path,
    confinement: &Confinement,
    target: &Path,
) -> (Option<u32>, String) {
    let child = job
        .create_suspended_with(&confined_spec(child_exe, cwd, target), Some(confinement))
        .expect("spawn confined child");
    job.assign(&child).expect("assign before resume");
    job.resume(&child).expect("resume confined child");

    let code = child
        .process
        .wait_timeout(Duration::from_secs(5))
        .expect("wait for confined child");
    let code = match code {
        Some(code) => Some(code),
        None => {
            let _ = job.terminate();
            child
                .process
                .wait_timeout(Duration::from_secs(2))
                .expect("wait after job terminate")
        }
    };
    let output = format!("{}{}", drain(&child.stdout), drain(&child.stderr));
    (code, output)
}

#[test]
fn confinement_read_matrix_and_normal_cleanup() {
    let outside = UserProfileDir::new("pimp-confinement-outside");
    let forbidden = outside.path().join("caller-readable.txt");
    std::fs::write(&forbidden, "FORBIDDEN-PAYLOAD").unwrap();

    let mut confinement = Confinement::create().expect("create AppContainer profile");
    let private_root = confinement.private_root().to_path_buf();
    let staged = private_root.join("probe");
    std::fs::create_dir_all(&staged).unwrap();
    let allowed = staged.join("allowed.txt");
    let child_exe = staged.join("fixture-confined-child.exe");
    std::fs::write(&allowed, "ALLOWED-PAYLOAD").unwrap();
    std::fs::copy(
        std::path::PathBuf::from(env!("CARGO_BIN_EXE_fixture-confined-child")),
        &child_exe,
    )
    .unwrap();

    let job = Job::new().expect("Job::new");
    let (code, output) = run_confined(&job, &child_exe, &staged, &confinement, &allowed);
    assert_eq!(code, Some(0), "allowed read failed: {output}");
    assert!(output.contains("READ_OK:ALLOWED-PAYLOAD"), "got: {output}");

    let (code, output) = run_confined(&job, &child_exe, &staged, &confinement, &forbidden);
    assert_eq!(code, Some(1), "external caller file was readable: {output}");
    assert!(output.contains("READ_FAIL"), "got: {output}");

    confinement
        .cleanup()
        .expect("remove private AppContainer profile");
    assert!(
        !private_root.exists(),
        "private profile root remains after cleanup"
    );
}

#[test]
fn failed_startup_removes_private_profile() {
    let mut confinement = Confinement::create().expect("create AppContainer profile");
    let private_root = confinement.private_root().to_path_buf();
    let missing_child = private_root.join("missing-child.exe");
    let job = Job::new().expect("Job::new");
    let spec = confined_spec(&missing_child, &private_root, &missing_child);

    assert!(
        job.create_suspended_with(&spec, Some(&confinement))
            .is_err(),
        "missing staged payload must fail before a child resumes"
    );
    confinement
        .cleanup()
        .expect("remove private profile after failed startup");
    assert!(
        !private_root.exists(),
        "private profile remains after failed startup"
    );
}
