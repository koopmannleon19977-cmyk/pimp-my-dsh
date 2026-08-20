//! Large Windows production-decision gate for opt-in read-side confinement.
//!
//! Requires `scripts/stage-runtime.ps1` and an rc.7 managed `web` profile.
//! Run explicitly:
//!   cargo test --test full_run_confinement_contract_test -- --ignored --nocapture

#![cfg(windows)]

use std::ffi::OsString;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use pimp_dsh_desktop::compatibility::{PackagedProvider, Provider};
use pimp_dsh_desktop::confinement::Confinement;
use pimp_dsh_desktop::job::{Child, ChildGuard, FileHandle, Job};
use pimp_dsh_desktop::pipe::BridgePipe;
use pimp_dsh_desktop::protocol::{Frame, decode, encode_shutdown};

struct Scope {
    confinement: Confinement,
}

impl Scope {
    fn new() -> Self {
        Self {
            confinement: Confinement::create().expect("create AppContainer profile"),
        }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let _ = self.confinement.cleanup();
    }
}

fn host_web_profile() -> PathBuf {
    if let Some(home) = std::env::var_os("DSH_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join("profiles").join("web");
    }
    PathBuf::from(std::env::var_os("USERPROFILE").expect("USERPROFILE"))
        .join(".dsh")
        .join("profiles")
        .join("web")
}

fn drain(pipe: &Option<FileHandle>) -> String {
    let mut bytes = Vec::new();
    if let Some(pipe) = pipe {
        let mut buf = [0u8; 4096];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(count) => bytes.extend_from_slice(&buf[..count]),
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn terminate_with_output(job: &Job, child: &Child) -> String {
    let _ = job.terminate();
    let _ = child.process.wait_timeout(Duration::from_secs(3));
    format!(
        "stdout: {}; stderr: {}",
        drain(&child.stdout),
        drain(&child.stderr)
    )
}

fn read_framed(pipe: Arc<BridgePipe>, timeout: Duration) -> Result<Vec<u8>, String> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = (|| -> std::io::Result<Vec<u8>> {
            let mut prefix = [0u8; 4];
            pipe.read_exact(&mut prefix)?;
            let len = u32::from_le_bytes(prefix) as usize;
            if len > 64 * 1024 {
                return Err(std::io::Error::other("oversize bridge frame"));
            }
            let mut bytes = prefix.to_vec();
            bytes.resize(4 + len, 0);
            pipe.read_exact(&mut bytes[4..])?;
            Ok(bytes)
        })();
        let _ = tx.send(result.map_err(|error| error.to_string()));
    });
    rx.recv_timeout(timeout)
        .map_err(|_| "bridge frame deadline exceeded".to_string())?
}

#[test]
#[ignore = "large local production-decision gate; requires generated runtime and managed web profile"]
fn private_real_web_run_reaches_ready_but_zero_capability_loopback_is_blocked() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("runtime")
        .join("manifest.json");
    let profile = host_web_profile();
    assert!(
        manifest.is_file(),
        "generated runtime is required; run scripts/stage-runtime.ps1"
    );
    assert!(
        profile.join(".pimp-my-dsh.json").is_file(),
        "managed rc.7 web profile is required"
    );
    let started = Instant::now();
    eprintln!("full-gate: resolve source");

    let provider = PackagedProvider::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    let source = provider.resolve().expect("resolve verified runtime");
    eprintln!("full-gate: create profile {:?}", started.elapsed());
    let mut scope = Scope::new();
    eprintln!("full-gate: stage runtime {:?}", started.elapsed());
    let staged_runtime = scope
        .confinement
        .stage_runtime(&source)
        .expect("stage verified runtime");
    eprintln!("full-gate: stage profile {:?}", started.elapsed());
    let mut spec = scope
        .confinement
        .stage_web_profile(&profile, &staged_runtime)
        .expect("physicalize managed web profile");
    eprintln!("full-gate: profile staged {:?}", started.elapsed());

    let run_id = format!("confined-full-run-{}", std::process::id());
    let token = "a".repeat(64);
    let pipe_name = format!(r"\\.\pipe\pimp-dsh-confined-full-{}", std::process::id());
    for (name, value) in [
        ("DSH_PIMP_SUPERVISOR_PIPE", pipe_name.as_str()),
        ("DSH_PIMP_SUPERVISOR_TOKEN", token.as_str()),
        ("DSH_PIMP_SUPERVISOR_RUN_ID", run_id.as_str()),
    ] {
        spec.env.push((OsString::from(name), OsString::from(value)));
    }

    let pipe = Arc::new(
        BridgePipe::create_for_appcontainer(&pipe_name, &scope.confinement)
            .expect("create run-scoped AppContainer pipe"),
    );
    eprintln!("full-gate: pipe created {:?}", started.elapsed());
    let job = Job::new().expect("Job::new");
    let guard = ChildGuard::new(
        job.create_suspended_with(&spec, Some(&scope.confinement))
            .expect("spawn private real web run"),
    );
    job.assign(guard.child()).expect("assign before resume");
    assert_eq!(job.active_process_count().unwrap(), 1);
    job.resume(guard.child()).expect("resume private run");
    let child = guard.into_inner();
    eprintln!("full-gate: child resumed {:?}", started.elapsed());

    if let Err(error) = pipe.connect_timeout(Duration::from_secs(30)) {
        panic!(
            "AppContainer could not connect to the authenticated pipe: {error}; {}",
            terminate_with_output(&job, &child)
        );
    }
    eprintln!("full-gate: pipe connected {:?}", started.elapsed());
    let mut expected_sequence = 1u64;
    let hello_bytes = read_framed(pipe.clone(), Duration::from_secs(15)).unwrap_or_else(|error| {
        panic!(
            "missing authenticated hello: {error}; {}",
            terminate_with_output(&job, &child)
        )
    });
    let hello = decode(&hello_bytes, &run_id, &token, &mut expected_sequence)
        .expect("decode authenticated hello");
    assert!(matches!(hello, Frame::Hello { .. }));
    eprintln!("full-gate: hello {:?}", started.elapsed());

    let ready_bytes = read_framed(pipe.clone(), Duration::from_secs(60)).unwrap_or_else(|error| {
        panic!(
            "missing authenticated ready: {error}; {}",
            terminate_with_output(&job, &child)
        )
    });
    let ready = decode(&ready_bytes, &run_id, &token, &mut expected_sequence)
        .expect("decode authenticated ready");
    let (port, url) = match ready {
        Frame::Ready {
            profile,
            host,
            port,
            url,
            distribution_version,
            dsh_version,
            ..
        } => {
            assert_eq!(profile, "web");
            assert_eq!(host, "127.0.0.1");
            assert_eq!(distribution_version, "0.1.0");
            assert_eq!(dsh_version, "0.1.0-rc.7");
            (port, url)
        }
        other => panic!("expected ready, got {}", other.kind()),
    };
    assert_eq!(url, format!("http://127.0.0.1:{port}"));
    eprintln!("full-gate: ready {:?}", started.elapsed());

    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let loopback_error = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .expect_err(
            "a capability-free AppContainer unexpectedly accepted a loopback connection; revisit the production decision",
        );
    assert_eq!(
        loopback_error.raw_os_error(),
        Some(10061),
        "the recorded production blocker must be WSAECONNREFUSED, not a later HTTP failure: {loopback_error}"
    );
    eprintln!(
        "full-gate: production blocker confirmed after {:?}: loopback connect to {} failed with {}",
        started.elapsed(),
        address,
        loopback_error
    );

    if pipe.write_all(&encode_shutdown(1)).is_ok() {
        let mut saw_stopping = false;
        for _ in 0..3 {
            let bytes = match read_framed(pipe.clone(), Duration::from_secs(10)) {
                Ok(bytes) => bytes,
                Err(_) => break,
            };
            match decode(&bytes, &run_id, &token, &mut expected_sequence) {
                Ok(Frame::Stopping { .. }) => saw_stopping = true,
                Ok(Frame::Stopped { .. }) => break,
                Ok(_) => {}
                Err(error) => panic!("invalid shutdown frame: {error}"),
            }
        }
        assert!(saw_stopping, "child never acknowledged stopping");
        assert!(
            child
                .process
                .wait_timeout(Duration::from_secs(8))
                .expect("wait for graceful root exit")
                .is_some(),
            "private root did not exit after shutdown"
        );
    } else {
        // The zero-capability web run may close itself immediately after its
        // unreachable Ready endpoint. Reap the complete Job; never adopt/PID-kill.
        let _ = job.terminate();
        let _ = child.process.wait_timeout(Duration::from_secs(3));
    }
    assert!(job.wait_empty(Duration::from_secs(3)).unwrap());
    eprintln!("full-gate: job empty {:?}", started.elapsed());
    pipe.disconnect();
    drop(pipe);
    drop(child);
    drop(job);
    drop(spec);
    drop(staged_runtime);
    scope
        .confinement
        .cleanup()
        .expect("remove private full-run profile");
    eprintln!("full-gate: cleanup complete {:?}", started.elapsed());
}
