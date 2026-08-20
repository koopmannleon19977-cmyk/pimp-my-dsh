//! Large Windows production gate for authenticated read-side confinement.
//!
//! Requires `scripts/stage-runtime.ps1` and an rc.7 managed `web` profile.
//! Run explicitly:
//!   cargo test --test full_run_confinement_contract_test -- --ignored --nocapture

#![cfg(windows)]

use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use pimp_dsh_desktop::compatibility::{PackagedProvider, Provider};
use pimp_dsh_desktop::confinement::Confinement;
use pimp_dsh_desktop::job::{Child, ChildGuard, FileHandle, Job};
use pimp_dsh_desktop::pipe::{AppContainerPipeFactory, BridgePipe};
use pimp_dsh_desktop::protocol::{Frame, HostFrameEncoder, decode};
use pimp_dsh_desktop::web_proxy::{BoundWebProxy, ControlChannel};

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

fn http_request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to host web proxy");
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("bound proxy response wait");
    stream
        .write_all(request.as_bytes())
        .expect("write proxy request");
    let mut response = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => response.extend_from_slice(&chunk[..count]),
            Err(error)
                if !response.is_empty()
                    && matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
            {
                break;
            }
            Err(error) => panic!("read proxy response: {error}"),
        }
    }
    String::from_utf8_lossy(&response).into_owned()
}

#[test]
#[ignore = "large local production gate; requires generated runtime and managed web profile"]
fn private_real_web_run_serves_through_authenticated_host_pipe_proxy() {
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

    let bound_proxy = BoundWebProxy::bind(0).expect("bind host web proxy");
    let proxy_port = bound_proxy.port();
    let bootstrap_url = bound_proxy.bootstrap_url();
    spec.set_port(Some(proxy_port));
    let preload = spec.cli_entry.with_file_name("confined-web-transport.js");
    assert!(
        preload.is_file(),
        "confined web preload is missing from staged runtime: {}",
        preload.display()
    );
    let preload_url = tauri::Url::from_file_path(&preload).expect("preload file URL");
    let (_, node_options) = spec
        .env
        .iter_mut()
        .find(|(name, _)| name.to_string_lossy().eq_ignore_ascii_case("NODE_OPTIONS"))
        .expect("staged runtime NODE_OPTIONS");
    let mut options = node_options.to_string_lossy().into_owned();
    options.push_str(" --import=");
    options.push_str(preload_url.as_str());
    *node_options = OsString::from(options);
    let web_pipes =
        AppContainerPipeFactory::new(&scope.confinement).expect("create web pipe factory");
    let proxy_port_text = proxy_port.to_string();

    let run_id = format!("confined-full-run-{}", std::process::id());
    let token = "a".repeat(64);
    let pipe_name = format!(r"\\.\pipe\pimp-dsh-confined-full-{}", std::process::id());
    let anchor_pipe = format!(r"\\.\pipe\LOCAL\pimp-dsh-anchor-{}", "d".repeat(32));
    for (name, value) in [
        ("DSH_PIMP_SUPERVISOR_PIPE", pipe_name.as_str()),
        ("DSH_PIMP_SUPERVISOR_TOKEN", token.as_str()),
        ("DSH_PIMP_SUPERVISOR_RUN_ID", run_id.as_str()),
        ("DSH_PIMP_CONFINED_WEB", "1"),
        ("DSH_PIMP_WEB_PROXY_PORT", proxy_port_text.as_str()),
        ("DSH_PIMP_WEB_ANCHOR_PIPE", anchor_pipe.as_str()),
    ] {
        spec.env.push((OsString::from(name), OsString::from(value)));
    }

    let pipe = Arc::new(
        BridgePipe::create_for_appcontainer(&pipe_name, &scope.confinement)
            .expect("create run-scoped AppContainer pipe"),
    );
    eprintln!("full-gate: pipe created {:?}", started.elapsed());
    let control = Arc::new(ControlChannel::new(
        Arc::clone(&pipe),
        HostFrameEncoder::new(run_id.clone(), token.clone()).expect("host control encoder"),
    ));
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

    assert_eq!(port, proxy_port, "ready must advertise the host proxy port");
    let factory = web_pipes.clone();
    let mut proxy = bound_proxy
        .start(Arc::clone(&control), move |name| factory.create(name))
        .expect("start authenticated host web proxy");
    let base_url = format!("http://127.0.0.1:{proxy_port}");
    let bootstrap_path = bootstrap_url
        .strip_prefix(&base_url)
        .expect("bootstrap URL uses proxy base");
    let bootstrap_response = http_request(
        proxy_port,
        &format!(
            "GET {bootstrap_path} HTTP/1.1\r\nHost: 127.0.0.1:{proxy_port}\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(bootstrap_response.starts_with("HTTP/1.1 303 See Other\r\n"));
    assert!(bootstrap_response.contains("Referrer-Policy: no-referrer\r\n"));
    assert!(bootstrap_response.contains("Cache-Control: no-store\r\n"));
    let cookie = bootstrap_response
        .lines()
        .find_map(|line| line.strip_prefix("Set-Cookie: "))
        .and_then(|value| value.split(';').next())
        .expect("bootstrap sets the host-only session cookie");
    let response = http_request(
        proxy_port,
        &format!(
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:{proxy_port}\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n"
        ),
    );
    if !response.starts_with("HTTP/1.1 200") {
        let bridge_diagnostic = read_framed(pipe.clone(), Duration::from_secs(1))
            .map(|bytes| {
                format!(
                    "{:?}",
                    decode(&bytes, &run_id, &token, &mut expected_sequence)
                )
            })
            .unwrap_or_else(|error| error);
        std::thread::sleep(Duration::from_millis(100));
        let proxy_diagnostic = proxy
            .fault()
            .unwrap_or_else(|| "no proxy fault".to_string());
        let child_status = child
            .process
            .wait_timeout(Duration::from_millis(0))
            .map(|value| format!("{value:?}"))
            .unwrap_or_else(|error| error.to_string());
        panic!(
            "confined web root did not traverse the authenticated pipe proxy: {response:?}; proxy: {proxy_diagnostic}; bridge: {bridge_diagnostic}; child: {child_status}; {}",
            terminate_with_output(&job, &child)
        );
    }
    eprintln!(
        "full-gate: authenticated web root reached after {:?}",
        started.elapsed()
    );
    proxy.stop_and_join();

    let shutdown_sent = control.send_shutdown().is_ok();
    if shutdown_sent {
        let mut saw_stopping = false;
        let mut observed = Vec::new();
        for _ in 0..3 {
            let bytes = match read_framed(pipe.clone(), Duration::from_secs(10)) {
                Ok(bytes) => bytes,
                Err(error) => {
                    observed.push(error);
                    break;
                }
            };
            match decode(&bytes, &run_id, &token, &mut expected_sequence) {
                Ok(frame @ Frame::Stopping { .. }) => {
                    observed.push(format!("{frame:?}"));
                    saw_stopping = true;
                }
                Ok(frame @ Frame::Stopped { .. }) => {
                    observed.push(format!("{frame:?}"));
                    break;
                }
                Ok(frame) => observed.push(format!("{frame:?}")),
                Err(error) => panic!("invalid shutdown frame: {error}"),
            }
        }
        assert!(
            saw_stopping,
            "child never acknowledged stopping; observed {observed:?}"
        );
        eprintln!("full-gate: shutdown frames {observed:?}");
        assert!(
            child
                .process
                .wait_timeout(Duration::from_secs(15))
                .expect("wait for graceful root exit")
                .is_some(),
            "private root did not exit after shutdown"
        );
    } else {
        // A control-write failure is terminal: reap the complete Job; never
        // adopt or PID-kill the confined process.
        let _ = job.terminate();
        let _ = child.process.wait_timeout(Duration::from_secs(3));
    }
    assert!(job.wait_empty(Duration::from_secs(3)).unwrap());
    eprintln!("full-gate: job empty {:?}", started.elapsed());
    pipe.disconnect();
    drop(control);
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
