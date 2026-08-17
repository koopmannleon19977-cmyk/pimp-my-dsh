//! The full lifecycle orchestrator: owns the state machine, the live Job/process
//! handles, the bridge, bounded logs, and the validated READY endpoint decision.
//! JavaScript is a view; every authority path is constructed here.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::compatibility::{LaunchSpec, Provider};
use crate::logging::{LogEvent, LogLevel, LogSink, LogSource, register_secret};
use crate::platform::{
    browser,
    job::{ChildGuard, Job},
    pipe::BridgePipe,
};
use crate::protocol::{Frame, construct_endpoint, decode, encode_shutdown};
use crate::state::State;
use crate::types::{CompatibilityView, DoctorResult, Settings, Snapshot, Theme};

/// Grace period beyond the upstream 5 s disposal bound.
const GRACE_TIMEOUT: Duration = Duration::from_secs(6);
/// Poll cadence for the lifecycle loop.
const TICK: Duration = Duration::from_millis(40);
/// Deadline for the child to connect and complete the Hello→Ready handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// Deadline for the CLI doctor to produce its JSON result.
const DOCTOR_TIMEOUT: Duration = Duration::from_secs(15);
/// Upper bound on captured doctor stdout/stderr before truncation.
const DOCTOR_DRAIN_CAP: usize = 64 * 1024;

fn active_provider(resource_dir: Option<PathBuf>) -> Box<dyn Provider> {
    #[cfg(debug_assertions)]
    {
        let _ = resource_dir;
        Box::new(crate::compatibility::DevProvider)
    }
    #[cfg(not(debug_assertions))]
    {
        Box::new(crate::compatibility::PackagedProvider::new(
            resource_dir.unwrap_or_default(),
        ))
    }
}

fn log_folder() -> PathBuf {
    if let Some(appdata) = std::env::var_os("LOCALAPPDATA") {
        PathBuf::from(appdata).join("pimp-my-dsh").join("logs")
    } else {
        std::env::temp_dir().join("pimp-my-dsh").join("logs")
    }
}

fn runs_file() -> PathBuf {
    if let Some(appdata) = std::env::var_os("LOCALAPPDATA") {
        PathBuf::from(appdata).join("pimp-my-dsh").join("runs.json")
    } else {
        std::env::temp_dir().join("pimp-my-dsh").join("runs.json")
    }
}

/// Best-effort load: a missing or malformed history file starts empty.
fn load_runs() -> Vec<crate::types::RunRecord> {
    match std::fs::read_to_string(runs_file()) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save_runs(runs: &[crate::types::RunRecord]) -> Result<(), String> {
    let path = runs_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string(runs).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

fn random_hex(bytes: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut buf = vec![0u8; bytes];
    if getrandom::fill(&mut buf).is_err() {
        panic!("OS randomness unavailable");
    }
    let mut s = String::with_capacity(bytes * 2);
    for &b in &buf {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

fn random_token() -> String {
    random_hex(32) // 64 lowercase hex chars
}

fn random_run_id() -> String {
    random_hex(16) // 32 lowercase hex chars (random but not secret)
}

enum BridgeEvent {
    Ready { endpoint: String, port: u16 },
    Health(Vec<crate::types::HealthCheck>),
    ChildStopping,
    ChildStopped,
    ChildError(String),
    ProtocolFailure(String),
    Closed,
}

struct Resources {
    run_id: Option<String>,
    endpoint: Option<String>,
    uptime_start: Option<Instant>,
    started_at: Option<String>,
    busy: bool,
    /// Per-run cancellation token. Fresh on each real start; cleared only by
    /// the run that owns it (never a newer run's token).
    cancel: Option<Arc<AtomicBool>>,
    health: Vec<crate::types::HealthCheck>,
    recent_runs: Vec<crate::types::RunRecord>,
    doctor: Option<DoctorResult>,
    settings: Settings,
    compatibility: CompatibilityView,
    logs: LogSink,
}

type SnapshotEmitter = dyn Fn(&Snapshot) + Send + Sync + 'static;

pub struct Supervisor {
    state: crate::state::Supervisor,
    resources: Arc<Mutex<Resources>>,
    /// Singleflight gate so concurrent `run_doctor` calls never spawn a second
    /// doctor process.
    doctor_lock: Arc<Mutex<()>>,
    emitter: Arc<Mutex<Option<Arc<SnapshotEmitter>>>>,
    resource_dir: Arc<Mutex<Option<PathBuf>>>,
}

impl Supervisor {
    pub fn new() -> Arc<Self> {
        Arc::new(Supervisor {
            state: crate::state::Supervisor::new(),
            resources: Arc::new(Mutex::new(Resources {
                run_id: None,
                endpoint: None,
                uptime_start: None,
                started_at: None,
                busy: false,
                cancel: None,
                health: Vec::new(),
                recent_runs: load_runs(),
                doctor: None,
                settings: Settings::default(),
                compatibility: CompatibilityView::default(),
                logs: LogSink::new(2000, Some(log_folder())),
            })),
            doctor_lock: Arc::new(Mutex::new(())),
            emitter: Arc::new(Mutex::new(None)),
            resource_dir: Arc::new(Mutex::new(None)),
        })
    }

    pub fn set_emitter(
        &self,
        resource_dir: Option<PathBuf>,
        emit: impl Fn(&Snapshot) + Send + Sync + 'static,
    ) {
        *self.resource_dir.lock().expect("resource dir lock") = resource_dir;
        *self.emitter.lock().expect("emitter lock") = Some(Arc::new(emit));
    }

    fn emit(&self) {
        let snap = self.snapshot();
        let emit = self.emitter.lock().expect("emitter lock").clone();
        if let Some(emit) = emit {
            emit(&snap);
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut snap = self.state.snapshot();
        let res = self.resources.lock().expect("resources lock");
        snap.run_id = res.run_id.clone();
        snap.endpoint = res.endpoint.clone();
        snap.uptime_ms = res.uptime_start.map(|t| t.elapsed().as_millis() as u64);
        snap.busy = res.busy;
        snap.health = res.health.clone();
        snap.recent_runs = res.recent_runs.clone();
        snap.doctor = res.doctor.clone();
        snap.logs = res.logs.snapshot();
        snap.settings = res.settings.clone();
        snap.compatibility = res.compatibility.clone();
        snap.logging_fault = res.logs.fault().map(|s| s.to_string());
        snap
    }

    fn log(&self, source: LogSource, level: LogLevel, run_id: Option<String>, message: String) {
        let revision = self.state.revision();
        let mut res = self.resources.lock().expect("resources lock");
        res.logs.push(LogEvent {
            run_id,
            revision,
            sequence: 0, // LogSink assigns
            timestamp: String::new(),
            source,
            level,
            message,
        });
    }

    /// Start: idempotent; spawns the lifecycle thread only on the real
    /// `Stopped* → Preflighting` transition (atomically decided by the state
    /// machine, so two concurrent starts cannot double-spawn).
    pub fn start(self: &Arc<Self>) -> Result<(), String> {
        if !self.state.start_changed()? {
            return Ok(()); // idempotent; already preflighting/starting/ready/running
        }
        // Install a fresh per-run cancellation token BEFORE the lifecycle thread
        // can observe it. No stale `stop_flag` ever leaks into a new run.
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut res = self.resources.lock().expect("resources lock");
            res.cancel = Some(cancel.clone());
        }
        self.log(
            LogSource::Lifecycle,
            LogLevel::Info,
            None,
            "start requested".to_string(),
        );
        self.emit();
        let this = self.clone();
        std::thread::spawn(move || this.run_lifecycle(cancel));
        Ok(())
    }

    /// Stop: cooperative first, forced on the grace deadline. Gated on the
    /// atomic `stop_changed` so only the current run's token is signalled.
    pub fn stop(self: &Arc<Self>) -> Result<(), String> {
        let changed = self.state.stop_changed()?;
        if changed {
            let res = self.resources.lock().expect("resources lock");
            if let Some(cancel) = &res.cancel {
                cancel.store(true, Ordering::SeqCst);
            }
        }
        self.log(
            LogSource::Lifecycle,
            LogLevel::Info,
            None,
            "stop requested".to_string(),
        );
        self.emit();
        Ok(())
    }

    /// Return the validated READY/RUNNING endpoint, or fail closed. Opening
    /// (embedded window vs. anything else) is the Tauri layer's concern; the
    /// lifecycle core only decides what endpoint may be opened.
    pub fn validated_endpoint(&self) -> Result<String, String> {
        let endpoint = {
            let res = self.resources.lock().expect("resources lock");
            if !crate::types::state_allows_open(self.state.state()) {
                return Err("open requires ready/running".to_string());
            }
            res.endpoint.clone()
        };
        endpoint.ok_or_else(|| "no validated endpoint".to_string())
    }

    /// Reveal the on-disk log folder.
    pub fn reveal_log_folder(&self) -> Result<(), String> {
        let dir = log_folder();
        std::fs::create_dir_all(&dir).map_err(|e| format!("create log dir: {e}"))?;
        browser::open_url(&dir.to_string_lossy()).map_err(|e| format!("reveal failed: {e}"))
    }

    /// Run the CLI doctor and store the structured result.
    pub fn run_doctor(self: &Arc<Self>) -> Result<(), String> {
        let result = self.execute_doctor();
        {
            let mut res = self.resources.lock().expect("resources lock");
            res.doctor = Some(result.clone());
        }
        self.log(
            LogSource::Doctor,
            if result.ok {
                LogLevel::Info
            } else {
                LogLevel::Error
            },
            None,
            format!("doctor {}", if result.ok { "ok" } else { "failed" }),
        );
        self.emit();
        Ok(())
    }

    fn execute_doctor(&self) -> DoctorResult {
        // Singleflight: a second concurrent doctor request fails fast instead of
        // spawning a second node process.
        let _guard = match self.doctor_lock.try_lock() {
            Ok(g) => g,
            Err(_) => {
                return DoctorResult {
                    ok: false,
                    error: Some("doctor already running".to_string()),
                    ..Default::default()
                };
            }
        };

        let resource_dir = self.resource_dir.lock().expect("resource dir lock").clone();
        let mut spec = match active_provider(resource_dir).resolve() {
            Ok(s) => s,
            Err(e) => {
                return DoctorResult {
                    ok: false,
                    error: Some(e),
                    ..Default::default()
                };
            }
        };
        // Fixed, verified argv: `node.exe <cli_entry> doctor --json` (no shell).
        spec.args = vec![
            std::ffi::OsString::from("doctor"),
            std::ffi::OsString::from("--json"),
        ];

        let job = match Job::new() {
            Ok(j) => j,
            Err(e) => {
                return DoctorResult {
                    ok: false,
                    error: Some(format!("doctor job create failed: {e}")),
                    ..Default::default()
                };
            }
        };
        let guard = ChildGuard::new(match job.create_suspended(&spec) {
            Ok(c) => c,
            Err(e) => {
                return DoctorResult {
                    ok: false,
                    error: Some(format!("doctor spawn failed: {e}")),
                    ..Default::default()
                };
            }
        });
        if let Err(e) = job.assign(guard.child()) {
            // guard drops → terminate+wait the still-unassigned child
            return DoctorResult {
                ok: false,
                error: Some(format!("doctor assign failed: {e}")),
                ..Default::default()
            };
        }
        if let Err(e) = job.resume(guard.child()) {
            return DoctorResult {
                ok: false,
                error: Some(format!("doctor resume failed: {e}")),
                ..Default::default()
            };
        }
        let mut child = guard.into_inner();

        // Bounded, redacted drains (still drain past the cap so the child never
        // blocks on a full pipe).
        let stdout_drain = spawn_bounded_drain(
            child.stdout.take().expect("doctor stdout"),
            DOCTOR_DRAIN_CAP,
        );
        let stderr_drain = spawn_bounded_drain(
            child.stderr.take().expect("doctor stderr"),
            DOCTOR_DRAIN_CAP,
        );
        drop(child.stdin);

        // Deadline-bounded wait for the root process.
        let deadline = Instant::now() + DOCTOR_TIMEOUT;
        let mut exit_code = None;
        loop {
            match child.process.wait_timeout(TICK) {
                Ok(Some(code)) => {
                    exit_code = Some(code);
                    break;
                }
                Ok(None) if Instant::now() >= deadline => break, // timed out
                Ok(None) => {}
                Err(_) => break, // wait failure → treat as timeout
            }
        }

        let timed_out = exit_code.is_none();
        if timed_out {
            let _ = job.terminate();
            let _ = job.wait_empty(Duration::from_secs(2));
        }

        let stdout = stdout_drain.join().unwrap_or_default();
        let stderr = stderr_drain.join().unwrap_or_default();

        if timed_out {
            return DoctorResult {
                ok: false,
                error: Some("doctor timed out".to_string()),
                ..Default::default()
            };
        }
        let code = exit_code.expect("exit code present");

        let stderr_text = crate::logging::redact(&String::from_utf8_lossy(&stderr));
        if code != 0 {
            return DoctorResult {
                ok: false,
                error: Some(format!("doctor exited {code}: {}", stderr_text.trim())),
                ..Default::default()
            };
        }

        let stdout_text = crate::logging::redact(&String::from_utf8_lossy(&stdout));
        match serde_json::from_str::<serde_json::Value>(stdout_text.trim()) {
            Ok(v) => DoctorResult {
                ok: true,
                error: None,
                node: v["node"].as_str().map(String::from),
                platform: v["platform"].as_str().map(String::from),
                architecture: v["architecture"].as_str().map(String::from),
                dsh_available: v["dshAvailable"].as_bool(),
                dsh_error: v["dshError"].as_str().map(String::from),
                profile_ready: v["profileReady"].as_bool(),
                api_key_configured: v["apiKeyConfigured"].as_bool(),
                base_url_configured: v["baseUrlConfigured"].as_bool(),
                model_configured: v["modelConfigured"].as_bool(),
                lsp_enabled: v["lspEnabled"].as_bool(),
                telemetry_enabled: v["telemetryEnabled"].as_bool(),
            },
            Err(e) => DoctorResult {
                ok: false,
                error: Some(format!("parse doctor output: {e}")),
                ..Default::default()
            },
        }
    }

    pub fn set_theme(self: &Arc<Self>, theme: Theme) -> Result<(), String> {
        {
            let mut res = self.resources.lock().expect("resources lock");
            res.settings.theme = theme;
        }
        self.emit();
        Ok(())
    }

    pub fn set_fixed_port(self: &Arc<Self>, port: Option<u16>) -> Result<(), String> {
        match port {
            Some(p) if !(1..=65535).contains(&p) => {
                return Err(format!("fixed port {p} out of range 1..=65535"));
            }
            _ => {}
        }
        {
            let mut res = self.resources.lock().expect("resources lock");
            res.settings.fixed_port = port;
        }
        self.emit();
        Ok(())
    }

    fn set_compat(&self, verified: bool) {
        let mut res = self.resources.lock().expect("resources lock");
        res.compatibility.verified = verified;
    }

    fn finish_run(&self, cancel: &Arc<AtomicBool>) {
        let mut res = self.resources.lock().expect("resources lock");
        res.busy = false;
        res.uptime_start = None;
        res.started_at = None;
        res.endpoint = None;
        res.health = Vec::new();
        res.run_id = None;
        // Clear only this run's token; never clobber a newer run's token if a
        // start raced the terminal transition.
        if let Some(current) = &res.cancel {
            if Arc::ptr_eq(current, cancel) {
                res.cancel = None;
            }
        }
    }

    /// Record a completed run (newest-first, capped at 10). No-op when the run
    /// never committed an identity/start time.
    fn record_run_end(&self, outcome: crate::types::RunOutcome, reason: String) {
        let ended_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let mut res = self.resources.lock().expect("resources lock");
        let (run_id, started_at) = match (&res.run_id, &res.started_at) {
            (Some(id), Some(ts)) => (id.clone(), ts.clone()),
            _ => return,
        };
        res.recent_runs.insert(0, crate::types::RunRecord {
            run_id,
            started_at,
            ended_at,
            outcome,
            reason,
        });
        res.recent_runs.truncate(10);
        // Best-effort persist; the in-memory list is authoritative for the UI.
        let _ = save_runs(&res.recent_runs);
    }

    /// Terminate the Job, wait for it to empty, then record the terminal state.
    /// A run force-killed during a cooperative stop is always `stopped-forced`
    /// (never `stopped-graceful`); an early child exit is a failed start; an
    /// otherwise-running child that dies is a crash.
    fn reap_job(&self, job: &Job) {
        let _ = job.terminate();
        let _ = job.wait_empty(Duration::from_secs(2));
        if self.state.state() == State::Stopping {
            let _ = self.state.grace_deadline();
        } else {
            let _ = self.state.child_exited();
        }
    }

    /// The full start→run→stop lifecycle, driven on a dedicated thread.
    /// `cancel` is the per-run cancellation token installed by `start()`.
    fn run_lifecycle(self: Arc<Self>, cancel: Arc<AtomicBool>) {
        // ---- preflight (resolve provider) ----
        let resource_dir = self.resource_dir.lock().expect("resource dir lock").clone();
        let spec: LaunchSpec = match active_provider(resource_dir).resolve() {
            Ok(s) => s,
            Err(e) => {
                self.log(
                    LogSource::Lifecycle,
                    LogLevel::Error,
                    None,
                    format!("preflight failed: {e}"),
                );
                self.set_compat(false);
                let _ = self.state.start_failed();
                self.emit();
                return;
            }
        };
        self.set_compat(true);
        if self.state.preflight_complete().is_err() {
            return;
        }

        // ---- run identity + bridge secret (memory-only) ----
        let run_id = random_run_id();
        let token = random_token();
        let pipe_name = format!(r"\\.\pipe\pimp-dsh-{}", random_hex(16));
        register_secret(&token);

        let mut spec = spec;
        let fixed_port = {
            let res = self.resources.lock().expect("resources lock");
            res.settings.fixed_port
        };
        spec.set_port(fixed_port);
        spec.env.push((
            std::ffi::OsString::from("DSH_PIMP_SUPERVISOR_PIPE"),
            std::ffi::OsString::from(&pipe_name),
        ));
        spec.env.push((
            std::ffi::OsString::from("DSH_PIMP_SUPERVISOR_TOKEN"),
            std::ffi::OsString::from(&token),
        ));
        spec.env.push((
            std::ffi::OsString::from("DSH_PIMP_SUPERVISOR_RUN_ID"),
            std::ffi::OsString::from(&run_id),
        ));

        // ---- bridge pipe (before spawn) ----
        let pipe = match BridgePipe::create(&pipe_name) {
            Ok(p) => Arc::new(p),
            Err(e) => {
                self.log(
                    LogSource::Lifecycle,
                    LogLevel::Error,
                    Some(run_id.clone()),
                    format!("pipe create failed: {e}"),
                );
                let _ = self.state.start_failed();
                self.emit();
                return;
            }
        };

        // ---- Job + suspended spawn + assign-before-resume ----
        let job = match Job::new() {
            Ok(j) => j,
            Err(e) => {
                self.log(
                    LogSource::Lifecycle,
                    LogLevel::Error,
                    Some(run_id.clone()),
                    format!("job create failed: {e}"),
                );
                let _ = self.state.start_failed();
                self.emit();
                return;
            }
        };
        // RAII guard: if assign/resume fails, the still-suspended child is
        // terminated + waited directly (TerminateJobObject cannot reach an
        // unassigned process). into_inner() disarms only after both succeed.
        let guard = ChildGuard::new(match job.create_suspended(&spec) {
            Ok(c) => c,
            Err(e) => {
                self.log(
                    LogSource::Lifecycle,
                    LogLevel::Error,
                    Some(run_id.clone()),
                    format!("spawn failed: {e}"),
                );
                let _ = self.state.start_failed();
                self.emit();
                return;
            }
        });
        if let Err(e) = job.assign(guard.child()) {
            self.log(
                LogSource::Lifecycle,
                LogLevel::Error,
                Some(run_id.clone()),
                format!("job assign failed: {e}"),
            );
            let _ = self.state.start_failed();
            self.emit();
            return; // guard drops → kills the unassigned child
        }
        if let Err(e) = job.resume(guard.child()) {
            self.log(
                LogSource::Lifecycle,
                LogLevel::Error,
                Some(run_id.clone()),
                format!("resume failed: {e}"),
            );
            let _ = self.state.start_failed();
            self.emit();
            return; // guard drops → kills the child
        }
        let child = guard.into_inner();

        // ---- record live identity + start worker threads ----
        {
            let mut res = self.resources.lock().expect("resources lock");
            res.run_id = Some(run_id.clone());
            res.uptime_start = Some(Instant::now());
            res.started_at =
                Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
            res.busy = true;
            res.endpoint = None;
            res.health = Vec::new();
        }

        let (tx, rx) = mpsc::channel::<BridgeEvent>();
        spawn_bridge_reader(
            pipe.clone(),
            tx.clone(),
            run_id.clone(),
            token,
            HANDSHAKE_TIMEOUT,
        );
        let mut drains: Vec<std::thread::JoinHandle<()>> = Vec::new();
        if let Some(stdout) = child.stdout {
            drains.push(spawn_drain(
                stdout,
                LogSource::Stdout,
                self.resources.clone(),
                run_id.clone(),
                self.state.clone(),
            ));
        }
        if let Some(stderr) = child.stderr {
            drains.push(spawn_drain(
                stderr,
                LogSource::Stderr,
                self.resources.clone(),
                run_id.clone(),
                self.state.clone(),
            ));
        }
        // Close the child's stdin write end so reads return EOF.
        drop(child.stdin);

        // ---- readiness / stop / crash loop ----
        let mut stop_started = false;
        let mut stop_started_at: Option<Instant> = None;
        let mut root_exited = false;
        let mut ready_received = false;
        let handshake_deadline = Instant::now() + HANDSHAKE_TIMEOUT;

        loop {
            // Cooperative stop requested?
            if cancel.load(Ordering::SeqCst) && !stop_started {
                stop_started = true;
                stop_started_at = Some(Instant::now());
                let _ = self.state.stop();
                self.log(
                    LogSource::Lifecycle,
                    LogLevel::Info,
                    Some(run_id.clone()),
                    "sending cooperative shutdown".to_string(),
                );
                // Rust→child shutdown frame (sequence 1).
                let _ = pipe.write_all(&encode_shutdown(1));
            }

            // Root process exited?
            match child.process.wait_timeout(TICK) {
                Ok(Some(code)) => {
                    root_exited = true;
                    if !stop_started {
                        self.log(
                            LogSource::Lifecycle,
                            LogLevel::Error,
                            Some(run_id.clone()),
                            format!("child exited unexpectedly (code {code})"),
                        );
                        self.reap_job(&job);
                        for d in drains.drain(..) {
                            let _ = d.join();
                        }
                        self.record_run_end(
                            crate::types::RunOutcome::Crashed,
                            format!("child exited unexpectedly (code {code})"),
                        );
                        self.finish_run(&cancel);
                        self.emit();
                        return;
                    }
                    self.log(
                        LogSource::Lifecycle,
                        LogLevel::Info,
                        Some(run_id.clone()),
                        format!("child exited (code {code})"),
                    );
                }
                Ok(None) => {}
                Err(e) => {
                    self.log(
                        LogSource::Lifecycle,
                        LogLevel::Error,
                        Some(run_id.clone()),
                        format!("wait failed: {e}"),
                    );
                }
            }

            // Drain bridge events.
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    BridgeEvent::Ready { endpoint, port } => {
                        // Fixed-port mismatch is fail-closed: the child must
                        // report the port it was told to bind.
                        if let Some(fp) = fixed_port {
                            if fp != port {
                                self.log(
                                    LogSource::Lifecycle,
                                    LogLevel::Error,
                                    Some(run_id.clone()),
                                    format!("ready reported port {port}, expected fixed {fp}"),
                                );
                                self.reap_job(&job);
                                for d in drains.drain(..) {
                                    let _ = d.join();
                                }
                                self.finish_run(&cancel);
                                self.emit();
                                return;
                            }
                        }
                        if ready_received {
                            // Duplicate Ready is a fatal protocol violation.
                            self.log(
                                LogSource::Lifecycle,
                                LogLevel::Error,
                                Some(run_id.clone()),
                                "duplicate ready frame".to_string(),
                            );
                            self.reap_job(&job);
                            for d in drains.drain(..) {
                                let _ = d.join();
                            }
                            self.finish_run(&cancel);
                            self.emit();
                            return;
                        }
                        ready_received = true;
                        // Commit the endpoint only AFTER the state machine
                        // accepts readiness (Starting → Ready).
                        if self.state.readiness_received().is_err() {
                            self.log(
                                LogSource::Lifecycle,
                                LogLevel::Error,
                                Some(run_id.clone()),
                                "readiness_received out of state".to_string(),
                            );
                            self.reap_job(&job);
                            for d in drains.drain(..) {
                                let _ = d.join();
                            }
                            self.finish_run(&cancel);
                            self.emit();
                            return;
                        }
                        {
                            let mut res = self.resources.lock().expect("resources lock");
                            res.endpoint = Some(endpoint.clone());
                            res.busy = false;
                        }
                        let _ = self.state.mark_running();
                        self.log(
                            LogSource::Lifecycle,
                            LogLevel::Info,
                            Some(run_id.clone()),
                            format!("ready at {endpoint}"),
                        );
                        self.emit();
                    }
                    BridgeEvent::Health(checks) => {
                        {
                            let mut res = self.resources.lock().expect("resources lock");
                            res.health = checks;
                        }
                        self.emit();
                    }
                    BridgeEvent::ChildStopping => {
                        self.log(
                            LogSource::Lifecycle,
                            LogLevel::Info,
                            Some(run_id.clone()),
                            "child stopping".to_string(),
                        );
                    }
                    BridgeEvent::ChildStopped => {
                        self.log(
                            LogSource::Lifecycle,
                            LogLevel::Info,
                            Some(run_id.clone()),
                            "child stopped".to_string(),
                        );
                    }
                    BridgeEvent::ChildError(msg) => {
                        self.log(
                            LogSource::Lifecycle,
                            LogLevel::Error,
                            Some(run_id.clone()),
                            msg,
                        );
                    }
                    BridgeEvent::ProtocolFailure(msg) => {
                        self.record_run_end(crate::types::RunOutcome::Crashed, msg.clone());
                        self.log(
                            LogSource::Lifecycle,
                            LogLevel::Error,
                            Some(run_id.clone()),
                            msg,
                        );
                        self.reap_job(&job);
                        for d in drains.drain(..) {
                            let _ = d.join();
                        }
                        self.finish_run(&cancel);
                        self.emit();
                        return;
                    }
                    BridgeEvent::Closed => {
                        // Fatal EOF — except during a cooperative stop, where
                        // the child closes its pipe as part of graceful
                        // shutdown.
                        if !stop_started {
                            self.log(
                                LogSource::Lifecycle,
                                LogLevel::Error,
                                Some(run_id.clone()),
                                "bridge closed unexpectedly".to_string(),
                            );
                            self.reap_job(&job);
                            for d in drains.drain(..) {
                                let _ = d.join();
                            }
                        self.record_run_end(
                            crate::types::RunOutcome::Crashed,
                            "bridge closed unexpectedly".to_string(),
                        );
                            self.finish_run(&cancel);
                            self.emit();
                            return;
                        }
                    }
                }
            }

            // Handshake deadline: the child never completed Hello→Ready.
            // Terminate the Job to unblock the bridge reader thread.
            if !ready_received && Instant::now() >= handshake_deadline {
                self.log(
                    LogSource::Lifecycle,
                    LogLevel::Warning,
                    Some(run_id.clone()),
                    "handshake deadline exceeded; terminating start".to_string(),
                );
                let _ = job.terminate();
                let _ = job.wait_empty(Duration::from_secs(2));
                for d in drains.drain(..) {
                    let _ = d.join();
                }
                let _ = self.state.start_failed();
                self.record_run_end(
                    crate::types::RunOutcome::FailedStart,
                    "handshake deadline exceeded".to_string(),
                );
                self.finish_run(&cancel);
                self.emit();
                return;
            }

            // Stopping classification: graceful (whole Job emptied on its own)
            // vs forced (grace deadline elapsed with processes still alive).
            if stop_started && self.state.state() == State::Stopping {
                let t0 = stop_started_at.expect("stop started time");
                if t0.elapsed() >= GRACE_TIMEOUT {
                    self.log(
                        LogSource::Lifecycle,
                        LogLevel::Warning,
                        Some(run_id.clone()),
                        "grace deadline exceeded; forcing termination".to_string(),
                    );
                    self.reap_job(&job);
                    for d in drains.drain(..) {
                        let _ = d.join();
                    }
                    self.record_run_end(
                        crate::types::RunOutcome::Forced,
                        "grace deadline exceeded".to_string(),
                    );
                    self.finish_run(&cancel);
                    self.emit();
                    return;
                }
                // Graceful only after the root exited AND every descendant in
                // the Job emptied on its own — never after a force-kill.
                if root_exited {
                    match job.active_process_count() {
                        Ok(0) => {
                            self.log(
                                LogSource::Lifecycle,
                                LogLevel::Info,
                                Some(run_id.clone()),
                                "child exited gracefully".to_string(),
                            );
                            let _ = self.state.child_exited(); // → stopped-graceful
                            for d in drains.drain(..) {
                                let _ = d.join();
                            }
                            self.record_run_end(
                                crate::types::RunOutcome::Graceful,
                                "child exited gracefully".to_string(),
                            );
                            self.finish_run(&cancel);
                            self.emit();
                            return;
                        }
                        Ok(_) => {} // descendants still draining
                        Err(e) => self.log(
                            LogSource::Lifecycle,
                            LogLevel::Error,
                            Some(run_id.clone()),
                            format!("job count failed: {e}"),
                        ),
                    }
                }
            }
        }
    }
}

fn spawn_bridge_reader(
    pipe: Arc<BridgePipe>,
    tx: mpsc::Sender<BridgeEvent>,
    run_id: String,
    token: String,
    handshake_timeout: Duration,
) {
    std::thread::spawn(move || {
        // Bounded connect is the outer edge of the handshake deadline: a child
        // that never connects cannot pin this reader thread forever.
        if let Err(e) = pipe.connect_timeout(handshake_timeout) {
            let _ = tx.send(BridgeEvent::ProtocolFailure(format!("connect: {e}")));
            return;
        }
        let mut next_seq: u64 = 1;
        let mut ready_seen = false;
        loop {
            let mut prefix = [0u8; 4];
            if pipe.read_exact(&mut prefix).is_err() {
                let _ = tx.send(BridgeEvent::Closed);
                return;
            }
            let body_len = u32::from_le_bytes(prefix) as usize;
            if body_len > crate::protocol::MAX_FRAME_BYTES {
                let _ = tx.send(BridgeEvent::ProtocolFailure("oversized frame".to_string()));
                return;
            }
            let mut body = vec![0u8; body_len];
            if pipe.read_exact(&mut body).is_err() {
                let _ = tx.send(BridgeEvent::Closed);
                return;
            }
            let mut framed = Vec::with_capacity(4 + body_len);
            framed.extend_from_slice(&prefix);
            framed.extend_from_slice(&body);
            match decode(&framed, &run_id, &token, &mut next_seq) {
                Ok(Frame::Hello { .. }) => {}
                Ok(Frame::Ready { host, port, .. }) => {
                    if ready_seen {
                        // A second Ready is a protocol violation, fail closed.
                        let _ = tx.send(BridgeEvent::ProtocolFailure(
                            "duplicate ready frame".to_string(),
                        ));
                        return;
                    }
                    ready_seen = true;
                    let endpoint = construct_endpoint(&host, port);
                    let _ = tx.send(BridgeEvent::Ready { endpoint, port });
                }
                Ok(Frame::Health { checks, .. }) => {
                    let _ = tx.send(BridgeEvent::Health(checks));
                }
                Ok(Frame::Stopping { .. }) => {
                    let _ = tx.send(BridgeEvent::ChildStopping);
                }
                Ok(Frame::Stopped { .. }) => {
                    let _ = tx.send(BridgeEvent::ChildStopped);
                }
                Ok(Frame::Error { message, .. }) => {
                    let _ = tx.send(BridgeEvent::ChildError(message));
                }
                Err(e) => {
                    let _ = tx.send(BridgeEvent::ProtocolFailure(format!(
                        "protocol violation: {e}"
                    )));
                    return;
                }
            }
        }
    });
}

fn push_drain_event(
    resources: &Arc<Mutex<Resources>>,
    run_id: &str,
    state: &crate::state::Supervisor,
    source: LogSource,
    message: String,
) {
    if message.is_empty() {
        return;
    }
    let mut res = resources.lock().expect("resources lock");
    res.logs.push(LogEvent {
        run_id: Some(run_id.to_string()),
        revision: state.revision(),
        sequence: 0,
        timestamp: String::new(),
        source,
        level: LogLevel::Info,
        message,
    });
}

fn spawn_drain(
    handle: crate::platform::job::FileHandle,
    source: LogSource,
    resources: Arc<Mutex<Resources>>,
    run_id: String,
    state: crate::state::Supervisor,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        // Streaming redaction holds back a secret prefix split across chunk
        // boundaries, then flushes the redacted remainder on EOF.
        let mut redactor = crate::logging::StreamingRedactor::new();
        let mut buf = [0u8; 8192];
        loop {
            match handle.read(&mut buf) {
                Ok(0) => {
                    push_drain_event(&resources, &run_id, &state, source, redactor.flush());
                    return;
                }
                Ok(n) => {
                    let text = redactor.feed(&buf[..n]);
                    push_drain_event(&resources, &run_id, &state, source, text);
                }
                Err(_) => {
                    push_drain_event(&resources, &run_id, &state, source, redactor.flush());
                    return;
                }
            }
        }
    })
}

/// Read a child stdio handle to EOF, returning at most `cap` bytes while still
/// draining the remainder so the child never blocks on a full pipe. Used by the
/// doctor path to capture bounded, redactable output.
fn spawn_bounded_drain(
    handle: crate::platform::job::FileHandle,
    cap: usize,
) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            match handle.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let remaining = cap.saturating_sub(out.len());
                    if remaining > 0 {
                        out.extend_from_slice(&buf[..n.min(remaining)]);
                    }
                    // Continue reading (discarding) past the cap to keep the
                    // child unblocked.
                }
                Err(_) => break,
            }
        }
        out
    })
}
