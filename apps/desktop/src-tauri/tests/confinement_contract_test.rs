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

#[test]
fn cleanup_clears_readonly_staged_files() {
    let mut confinement = Confinement::create().expect("create AppContainer profile");
    let private_root = confinement.private_root().to_path_buf();
    let staged = private_root.join("readonly.bin");
    std::fs::write(&staged, b"readonly").unwrap();
    let mut permissions = std::fs::metadata(&staged).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&staged, permissions).unwrap();

    confinement
        .cleanup()
        .expect("cleanup clears read-only attributes");
    assert!(!private_root.exists());
}

/// Copy the confined-child fixture into a fresh `probe` dir under the private
/// root. Returns `(staged_dir, child_exe)`. The fixture exe and every readable
/// payload must live under this root; external paths are never added.
fn stage_confined_child(private_root: &Path) -> (PathBuf, PathBuf) {
    let staged = private_root.join("probe");
    std::fs::create_dir_all(&staged).unwrap();
    let child_exe = staged.join("fixture-confined-child.exe");
    std::fs::copy(
        std::path::PathBuf::from(env!("CARGO_BIN_EXE_fixture-confined-child")),
        &child_exe,
    )
    .unwrap();
    (staged, child_exe)
}

/// Poll until the Job reports at least `minimum` active processes or the
/// timeout elapses. Bounded without any PID lookup.
fn wait_for_count(job: &Job, minimum: u32, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Ok(count) = job.active_process_count() {
            if count >= minimum {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    false
}

/// Poll until the Job reports exactly `count` active processes or the timeout
/// elapses. Bounded without any PID lookup.
fn wait_for_exact_count(job: &Job, count: u32, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Ok(active) = job.active_process_count() {
            if active == count {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    false
}

/// A confined tree spec: the fixture child spawns the fixture grandchild from
/// the same private root and then sleeps; both inherit Job membership.
fn tree_spec(child_exe: &Path, staged: &Path, grandchild_exe: &Path) -> LaunchSpec {
    LaunchSpec {
        node_exe: child_exe.to_path_buf(),
        cli_entry: child_exe.to_path_buf(),
        cwd: staged.to_path_buf(),
        env: std::env::vars_os().collect(),
        args: vec![
            OsString::from("--grandchild"),
            grandchild_exe.as_os_str().to_os_string(),
            OsString::from("--ms"),
            OsString::from("30000"),
        ],
    }
}

/// Resolve a host `node.exe` from PATH (the caller's node, never a confined
/// runtime). `Err` names the precise reason node is unavailable.
fn host_node() -> std::io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("node.exe");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(std::io::Error::other(
        "node.exe not found on PATH; the junction row is unavailable on this host",
    ))
}

/// Create a directory junction host-side through Node's
/// `fs.symlinkSync(target, link, 'junction')`. `Err` names the precise host
/// refusal when node is missing or the filesystem rejects junction creation.
fn host_junction(link: &Path, target: &Path) -> std::io::Result<()> {
    let node = host_node()?;
    // With `node -e`, process.argv is [node, arg1, arg2, …]: the first
    // post-script argument is the symlink target, the second the link path.
    let script = "require('fs').symlinkSync(process.argv[1], process.argv[2], 'junction')";
    let output = std::process::Command::new(&node)
        .arg("-e")
        .arg(script)
        .arg(target.as_os_str())
        .arg(link.as_os_str())
        .output()
        .map_err(|e| std::io::Error::other(format!("node junction launch failed: {e}")))?;
    if output.status.success() {
        return Ok(());
    }
    Err(std::io::Error::other(format!(
        "host node refused junction creation; precise reason: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

#[test]
fn appcontainer_identity_is_zero_capability() {
    let mut confinement = Confinement::create().expect("create AppContainer profile");

    // The capabilities struct is only borrowed during CreateProcessW; copy it
    // to assert the zero-capability, AppContainer-only identity.
    let caps = unsafe { *confinement.security_capabilities_ptr() };
    assert!(
        !caps.AppContainerSid.is_null(),
        "an AppContainer identity must carry a valid application SID"
    );
    assert!(
        caps.Capabilities.is_null(),
        "no capability SIDs may be attached to the prototype profile"
    );
    assert_eq!(caps.CapabilityCount, 0, "capability count must be zero");
    assert_eq!(
        caps.Reserved, 0,
        "SECURITY_CAPABILITIES.Reserved must be zero"
    );

    confinement
        .cleanup()
        .expect("remove private AppContainer profile after caps row");
}

#[test]
fn hardlink_alias_into_root_is_denied() {
    let outside = UserProfileDir::new("pimp-confinement-hardlink-outside");
    let forbidden = outside.path().join("caller-readable.txt");
    std::fs::write(&forbidden, "FORBIDDEN-PAYLOAD").unwrap();

    let mut confinement = Confinement::create().expect("create AppContainer profile");
    let private_root = confinement.private_root().to_path_buf();
    let (staged, child_exe) = stage_confined_child(&private_root);

    // Alias the caller's external file inside the private root with
    // std::fs::hard_link (CreateHardLinkW; no capability SID or privilege is
    // involved). A hardlink only adds a directory entry: the inode's DACL is
    // still the external caller file, so the confined read must be denied.
    let alias = staged.join("caller-readable-alias.txt");
    std::fs::hard_link(&forbidden, &alias)
        .expect("same-volume hard link must succeed (CreateHardLinkW)");

    let job = Job::new().expect("Job::new");
    let (code, output) = run_confined(&job, &child_exe, &staged, &confinement, &alias);
    assert_eq!(
        code,
        Some(1),
        "hardlink alias was readable by the confined child: {output}"
    );
    assert!(output.contains("READ_FAIL"), "got: {output}");

    confinement
        .cleanup()
        .expect("remove private AppContainer profile after hardlink row");
    assert!(
        !private_root.exists(),
        "hardlink row left the private profile behind"
    );
}

#[test]
fn junction_alias_is_denied_or_reports_unavailable() {
    let outside = UserProfileDir::new("pimp-confinement-junction-outside");
    let external = outside.path().join("ext");
    std::fs::create_dir_all(&external).unwrap();
    std::fs::write(external.join("caller-readable.txt"), "FORBIDDEN-PAYLOAD").unwrap();

    let mut confinement = Confinement::create().expect("create AppContainer profile");
    let private_root = confinement.private_root().to_path_buf();
    let (staged, child_exe) = stage_confined_child(&private_root);

    // A junction alias planted inside the private root, created host-side
    // through Node's fs.symlinkSync (type 'junction'). The confined child
    // reads through it to an external user-profile directory.
    let junction = staged.join("escape-junction");
    match host_junction(&junction, &external) {
        Ok(()) => {
            let job = Job::new().expect("Job::new");
            let through = junction.join("caller-readable.txt");
            let (code, output) = run_confined(&job, &child_exe, &staged, &confinement, &through);
            assert_eq!(
                code,
                Some(1),
                "junction alias was readable by the confined child: {output}"
            );
            assert!(output.contains("READ_FAIL"), "got: {output}");
        }
        Err(reason) => {
            // Host policy could not create the junction (node missing or a
            // filesystem/privilege restriction). Report the precise reason
            // and return only this row; it is unavailable, not a fault.
            println!("confinement junction row unavailable on this host: {reason}");
            return;
        }
    }

    confinement
        .cleanup()
        .expect("remove private AppContainer profile after junction row");
    assert!(
        !private_root.exists(),
        "junction row left the private profile behind"
    );
}

#[test]
fn confined_tree_root_crash_containment_and_cleanup() {
    let mut confinement = Confinement::create().expect("create AppContainer profile");
    let private_root = confinement.private_root().to_path_buf();
    let staged = private_root.join("probe");
    std::fs::create_dir_all(&staged).unwrap();
    let child_exe = staged.join("fixture-child.exe");
    std::fs::copy(
        std::path::PathBuf::from(env!("CARGO_BIN_EXE_fixture-child")),
        &child_exe,
    )
    .unwrap();
    let grandchild_exe = staged.join("fixture-grandchild.exe");
    std::fs::copy(
        std::path::PathBuf::from(env!("CARGO_BIN_EXE_fixture-grandchild")),
        &grandchild_exe,
    )
    .unwrap();

    // Long-lived confined tree: assign before resume, one process while the
    // primary thread is suspended, two after the fixture spawns its grandchild.
    let job = Job::new().expect("Job::new");
    let spec = tree_spec(&child_exe, &staged, &grandchild_exe);
    let tree = job
        .create_suspended_with(&spec, Some(&confinement))
        .expect("spawn confined tree");
    job.assign(&tree).expect("assign before resume");
    assert_eq!(
        job.active_process_count().expect("count suspended"),
        1,
        "only the root may exist while the primary thread is suspended"
    );
    job.resume(&tree).expect("resume");
    assert!(
        wait_for_count(&job, 2, Duration::from_secs(10)),
        "the confined grandchild did not join the Job"
    );

    // Root crash: TerminateProcess takes the root down with a nonzero exit
    // while its descendant stays alive in the Job.
    tree.terminate().expect("crash the confined root");
    assert_eq!(
        tree.process
            .wait_timeout(Duration::from_secs(10))
            .expect("wait for crashed root")
            .expect("the crashed root must exit"),
        1,
        "the crashed root must surface a nonzero exit code"
    );
    assert!(
        wait_for_exact_count(&job, 1, Duration::from_secs(2)),
        "the descendant must remain alone in the Job after the root crash"
    );

    // TerminateJobObject reaps the residual descendant; wait_empty reaches zero.
    job.terminate().expect("terminate after root crash");
    assert!(
        job.wait_empty(Duration::from_secs(10))
            .expect("wait empty after crash")
    );
    assert_eq!(
        job.active_process_count().expect("final count"),
        0,
        "the confined tree must be fully reaped"
    );

    confinement
        .cleanup()
        .expect("remove private AppContainer profile after tree row");
    assert!(
        !private_root.exists(),
        "the confined tree row left the private profile behind"
    );
}
