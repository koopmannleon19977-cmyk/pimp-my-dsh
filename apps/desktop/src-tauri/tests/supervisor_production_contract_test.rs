//! Executed packaged-supervisor gate for the release-only AppContainer path.

#![cfg(all(windows, not(debug_assertions)))]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pimp_dsh_desktop::state::State;
use pimp_dsh_desktop::supervisor::Supervisor;
use pimp_dsh_desktop::types::{RestartPolicy, RunOutcome, Snapshot};

struct StopGuard(Arc<Supervisor>);

impl Drop for StopGuard {
    fn drop(&mut self) {
        let _ = self.0.stop();
    }
}

fn wait_for(supervisor: &Supervisor, expected: &[State], timeout: Duration) -> Snapshot {
    let deadline = Instant::now() + timeout;
    loop {
        let snapshot = supervisor.snapshot();
        if expected.contains(&snapshot.state) {
            return snapshot;
        }
        if matches!(
            snapshot.state,
            State::FailedStart | State::Crashed | State::Unmanaged
        ) {
            panic!(
                "supervisor entered {:?}: {:?}",
                snapshot.state, snapshot.logs
            );
        }
        assert!(
            Instant::now() < deadline,
            "supervisor state deadline: {snapshot:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect host proxy");
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("bound host response wait");
    stream
        .write_all(request.as_bytes())
        .expect("write host request");
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
            Err(error) => panic!("read host response: {error}"),
        }
    }
    String::from_utf8_lossy(&response).into_owned()
}

#[test]
#[ignore = "large local release gate; requires generated runtime and managed web profile"]
fn packaged_supervisor_serves_and_stops_the_confined_web_run() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert!(root.join("runtime").join("manifest.json").is_file());

    let supervisor = Supervisor::new();
    supervisor.set_emitter(Some(root), |_| {});
    supervisor
        .set_fixed_port(None)
        .expect("dynamic host proxy port");
    supervisor
        .set_restart_policy(RestartPolicy::Never)
        .expect("disable restart during gate");
    let _guard = StopGuard(Arc::clone(&supervisor));

    supervisor.start().expect("start packaged supervisor");
    let running = wait_for(&supervisor, &[State::Running], Duration::from_secs(300));
    let public = running.endpoint.expect("public proxy endpoint");
    assert!(!public.contains("/_pimp/bootstrap/"));
    let navigation = supervisor
        .validated_endpoint()
        .expect("private navigation endpoint");
    let bootstrap_path = navigation
        .strip_prefix(&public)
        .expect("navigation endpoint uses public proxy base");
    assert!(bootstrap_path.starts_with("/_pimp/bootstrap/"));
    let port = public
        .rsplit_once(':')
        .and_then(|(_, value)| value.parse::<u16>().ok())
        .expect("public endpoint port");

    let bootstrap = request(
        port,
        &format!(
            "GET {bootstrap_path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(bootstrap.starts_with("HTTP/1.1 303 See Other\r\n"));
    assert!(bootstrap.contains("Cache-Control: no-store\r\n"));
    assert!(bootstrap.contains("Referrer-Policy: no-referrer\r\n"));
    let cookie = bootstrap
        .lines()
        .find_map(|line| line.strip_prefix("Set-Cookie: "))
        .and_then(|value| value.split(';').next())
        .expect("private bootstrap cookie");
    let response = request(
        port,
        &format!(
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "unexpected root response {response:?}; snapshot: {:?}",
        supervisor.snapshot()
    );

    supervisor.stop().expect("request cooperative stop");
    let stopped = wait_for(
        &supervisor,
        &[State::StoppedGraceful, State::StoppedForced],
        Duration::from_secs(30),
    );
    assert_eq!(stopped.state, State::StoppedGraceful);
    assert_eq!(
        stopped.recent_runs.first().map(|run| &run.outcome),
        Some(&RunOutcome::Graceful)
    );
    assert!(supervisor.validated_endpoint().is_err());
}
