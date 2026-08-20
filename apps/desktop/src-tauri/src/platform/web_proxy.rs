//! Host-owned loopback proxy for the confined web server.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::platform::pipe::BridgePipe;
use crate::protocol::HostFrameEncoder;

const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_ACTIVE_CONNECTIONS: usize = 16;
const COOKIE_NAME: &str = "pimp_dsh_session";
const PIPE_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const PIPE_PROOF_TIMEOUT: Duration = Duration::from_secs(2);

/// The single authenticated, strictly sequenced Rust→child writer.
pub struct ControlChannel {
    pipe: Arc<BridgePipe>,
    encoder: Mutex<HostFrameEncoder>,
}

impl ControlChannel {
    pub fn new(pipe: Arc<BridgePipe>, encoder: HostFrameEncoder) -> Self {
        Self {
            pipe,
            encoder: Mutex::new(encoder),
        }
    }

    pub fn send_web_accept(&self, pipe_name: &str, connection_token: &str) -> io::Result<()> {
        let mut encoder = self
            .encoder
            .lock()
            .map_err(|_| io::Error::other("control sequence lock poisoned"))?;
        let frame = encoder
            .encode_web_accept(pipe_name, connection_token)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        self.pipe.write_all(&frame)
    }

    pub fn send_shutdown(&self) -> io::Result<()> {
        let mut encoder = self
            .encoder
            .lock()
            .map_err(|_| io::Error::other("control sequence lock poisoned"))?;
        let frame = encoder
            .encode_shutdown()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        self.pipe.write_all(&frame)
    }
}

/// Listener bound before the confined child is spawned.
pub struct BoundWebProxy {
    listener: TcpListener,
    port: u16,
    bootstrap_path: String,
    cookie: String,
}

impl BoundWebProxy {
    /// Bind the host proxy to `127.0.0.1`; port zero requests an ephemeral port.
    pub fn bind(port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        Ok(Self {
            listener,
            port,
            bootstrap_path: format!("/_pimp/bootstrap/{}", random_hex(32)?),
            cookie: random_hex(32)?,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn bootstrap_url(&self) -> String {
        format!("http://127.0.0.1:{}{}", self.port, self.bootstrap_path)
    }

    /// Start accepting after the child control pipe is connected.
    pub fn start<F>(
        self,
        control: Arc<ControlChannel>,
        pipe_factory: F,
    ) -> io::Result<RunningWebProxy>
    where
        F: Fn(&str) -> io::Result<BridgePipe> + Send + Sync + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let workers = Arc::new(Mutex::new(Vec::<JoinHandle<()>>::new()));
        let active = Arc::new(Mutex::new(HashMap::new()));
        let fault = Arc::new(Mutex::new(None));
        let next_id = Arc::new(AtomicU64::new(1));
        let factory = Arc::new(pipe_factory);
        let thread_stop = Arc::clone(&stop);
        let thread_workers = Arc::clone(&workers);
        let thread_active = Arc::clone(&active);
        let bootstrap_path = self.bootstrap_path;
        let thread_fault = Arc::clone(&fault);
        let cookie = self.cookie;
        let listener = self.listener;

        let accept_thread =
            thread::Builder::new()
                .name("pimp-web-proxy".into())
                .spawn(move || {
                    while !thread_stop.load(Ordering::Acquire) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                let mut stream = stream;
                                let id = next_id.fetch_add(1, Ordering::Relaxed);
                                let stop = Arc::clone(&thread_stop);
                                let active = Arc::clone(&thread_active);
                                let control = Arc::clone(&control);
                                let factory = Arc::clone(&factory);
                                let fault = Arc::clone(&thread_fault);
                                let path = bootstrap_path.clone();
                                let cookie = cookie.clone();
                                let stop_stream = match stream.try_clone() {
                                    Ok(value) => value,
                                    Err(_) => continue,
                                };
                                let admitted = if let Ok(mut entries) = active.lock() {
                                    if entries.len() >= MAX_ACTIVE_CONNECTIONS {
                                        false
                                    } else {
                                        entries.insert(
                                            id,
                                            ActiveConnection {
                                                tcp: stop_stream,
                                                pipe: None,
                                            },
                                        );
                                        true
                                    }
                                } else {
                                    continue;
                                };
                                if !admitted {
                                    let _ = stream.set_nonblocking(false);
                                    let _ = write_empty_response(
                                        &mut stream,
                                        "503 Service Unavailable",
                                        &[],
                                    );
                                    continue;
                                }
                                let worker = thread::spawn(move || {
                                    if let Err(error) = handle_connection(
                                        stream, id, &path, &cookie, control, factory, &stop,
                                        &active,
                                    ) && !connection_closed(&error)
                                        && !connection_rejected(&error)
                                        && let Ok(mut current) = fault.lock()
                                        && current.is_none()
                                    {
                                        *current = Some(error.to_string());
                                    }
                                    if let Ok(mut entries) = active.lock() {
                                        entries.remove(&id);
                                    }
                                });
                                if let Ok(mut handles) = thread_workers.lock() {
                                    let mut index = 0;
                                    while index < handles.len() {
                                        if handles[index].is_finished() {
                                            let _ = handles.swap_remove(index).join();
                                        } else {
                                            index += 1;
                                        }
                                    }
                                    handles.push(worker);
                                }
                            }
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                                thread::sleep(Duration::from_millis(10));
                            }
                            Err(_) => break,
                        }
                    }
                })?;

        Ok(RunningWebProxy {
            port: self.port,
            stop,
            active,
            fault,
            accept_thread: Some(accept_thread),
            workers,
        })
    }
}

struct ActiveConnection {
    tcp: TcpStream,
    pipe: Option<Arc<BridgePipe>>,
}

/// Running proxy. Stop and join it before deleting confinement resources.
pub struct RunningWebProxy {
    port: u16,
    stop: Arc<AtomicBool>,
    active: Arc<Mutex<HashMap<u64, ActiveConnection>>>,
    fault: Arc<Mutex<Option<String>>>,
    accept_thread: Option<JoinHandle<()>>,
    workers: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl RunningWebProxy {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn fault(&self) -> Option<String> {
        self.fault.lock().ok().and_then(|value| value.clone())
    }

    pub fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.accept_thread.take() {
            let _ = handle.join();
        }
        if let Ok(entries) = self.active.lock() {
            for connection in entries.values() {
                let _ = connection.tcp.shutdown(Shutdown::Both);
                if let Some(pipe) = &connection.pipe {
                    pipe.cancel_io();
                }
            }
        }
        if let Ok(mut workers) = self.workers.lock() {
            for worker in workers.drain(..) {
                let _ = worker.join();
            }
        }
    }
}

impl Drop for RunningWebProxy {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

fn handle_connection<F>(
    mut tcp: TcpStream,
    id: u64,
    bootstrap_path: &str,
    cookie: &str,
    control: Arc<ControlChannel>,
    pipe_factory: Arc<F>,
    stop: &AtomicBool,
    active: &Mutex<HashMap<u64, ActiveConnection>>,
) -> io::Result<()>
where
    F: Fn(&str) -> io::Result<BridgePipe> + Send + Sync + 'static,
{
    tcp.set_nonblocking(false)?;
    tcp.set_read_timeout(Some(Duration::from_secs(2)))?;
    let request = match read_header(&mut tcp) {
        Ok(value) => value,
        Err(error) => {
            let status = if error.kind() == io::ErrorKind::InvalidData {
                "431 Request Header Fields Too Large"
            } else {
                "400 Bad Request"
            };
            write_empty_response(&mut tcp, status, &[])?;
            return Ok(());
        }
    };

    if request_target(&request) == Some(bootstrap_path) {
        let set_cookie =
            format!("Set-Cookie: {COOKIE_NAME}={cookie}; Path=/; HttpOnly; SameSite=Strict\r\n");
        write_empty_response(
            &mut tcp,
            "303 See Other",
            &[
                &set_cookie,
                "Location: /\r\n",
                "Referrer-Policy: no-referrer\r\n",
                "Cache-Control: no-store\r\n",
            ],
        )?;
        return Ok(());
    }
    if !has_cookie(&request, cookie) {
        write_empty_response(&mut tcp, "403 Forbidden", &[])?;
        return Ok(());
    }
    if stop.load(Ordering::Acquire) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "web proxy is stopping",
        ));
    }

    tcp.set_read_timeout(None)?;
    let pipe_name = format!(r"\\.\pipe\pimp-dsh-web-{}", random_hex(16)?);
    let connection_token = random_hex(32)?;
    let pipe = Arc::new(pipe_factory(&pipe_name)?);
    if let Ok(mut entries) = active.lock() {
        if let Some(connection) = entries.get_mut(&id) {
            connection.pipe = Some(Arc::clone(&pipe));
        }
    }
    control
        .send_web_accept(&pipe_name, &connection_token)
        .map_err(|error| io::Error::new(error.kind(), format!("send web-accept: {error}")))?;

    let started = Instant::now();
    loop {
        match pipe.connect_timeout(Duration::from_millis(250)) {
            Ok(()) => break,
            Err(error)
                if error.kind() == io::ErrorKind::TimedOut
                    && !stop.load(Ordering::Acquire)
                    && started.elapsed() < PIPE_CONNECT_TIMEOUT => {}
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("connect data pipe: {error}"),
                ));
            }
        }
    }
    let mut proof = [0u8; 64];
    pipe.read_exact_timeout(&mut proof, PIPE_PROOF_TIMEOUT)
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("read child proof: {error}"),
            )
        })?;
    if !constant_time_eq(&proof, connection_token.as_bytes()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "child data-pipe proof rejected",
        ));
    }
    pipe.write_all_blocking(&request)
        .map_err(|error| io::Error::new(error.kind(), format!("write initial request: {error}")))?;
    copy_duplex(tcp, pipe)
        .map_err(|error| io::Error::new(error.kind(), format!("copy tunnel bytes: {error}")))
}

fn copy_duplex(mut tcp: TcpStream, pipe: Arc<BridgePipe>) -> io::Result<()> {
    let mut pipe_to_tcp = tcp.try_clone()?;
    let read_pipe = Arc::clone(&pipe);
    let reverse = thread::spawn(move || {
        let result = (|| {
            let mut buf = [0u8; 64 * 1024];
            loop {
                let count = match read_pipe.read_blocking(&mut buf) {
                    Ok(value) => value,
                    Err(error) if connection_closed(&error) => return Ok(()),
                    Err(error) => return Err(error),
                };
                if count == 0 {
                    return Ok::<(), io::Error>(());
                }
                if let Err(error) = pipe_to_tcp.write_all(&buf[..count]) {
                    if connection_closed(&error) {
                        return Ok(());
                    }
                    return Err(error);
                }
            }
        })();
        let _ = pipe_to_tcp.shutdown(Shutdown::Both);
        result
    });

    let mut buf = [0u8; 64 * 1024];
    let forward = loop {
        match tcp.read(&mut buf) {
            Ok(0) => break Ok(()),
            Ok(count) => {
                if let Err(error) = pipe.write_all_blocking(&buf[..count]) {
                    break if connection_closed(&error) {
                        Ok(())
                    } else {
                        Err(error)
                    };
                }
            }
            Err(error) if connection_closed(&error) => break Ok(()),
            Err(error) => break Err(error),
        }
    };
    pipe.cancel_io();
    let _ = tcp.shutdown(Shutdown::Both);
    let reverse = reverse
        .join()
        .map_err(|_| io::Error::other("pipe copy worker panicked"))?;
    forward.and(reverse)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

fn connection_rejected(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::PermissionDenied
}

fn connection_closed(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::NotConnected
            | io::ErrorKind::UnexpectedEof
    ) || matches!(error.raw_os_error(), Some(109 | 232 | 995))
}

fn read_header(stream: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = vec![0u8; MAX_HEADER_BYTES];
    let mut length = 0usize;
    loop {
        let count = stream.read(&mut bytes[length..])?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "HTTP header closed",
            ));
        }
        length += count;
        if bytes[..length]
            .windows(4)
            .any(|window| window == b"\r\n\r\n")
        {
            bytes.truncate(length);
            return Ok(bytes);
        }
        if length == bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP header too large",
            ));
        }
    }
}

fn request_target(request: &[u8]) -> Option<&str> {
    let line_end = request.windows(2).position(|window| window == b"\r\n")?;
    let line = std::str::from_utf8(&request[..line_end]).ok()?;
    let mut parts = line.split(' ');
    if parts.next()? != "GET" {
        return None;
    }
    let target = parts.next()?;
    (parts.next()?.starts_with("HTTP/1.") && parts.next().is_none()).then_some(target)
}

fn has_cookie(request: &[u8], expected: &str) -> bool {
    let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    request[..end]
        .split(|byte| *byte == b'\n')
        .filter_map(|line| std::str::from_utf8(line.strip_suffix(b"\r").unwrap_or(line)).ok())
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("cookie"))
        .flat_map(|(_, value)| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .any(|(name, value)| name == COOKIE_NAME && value == expected)
}

fn write_empty_response(stream: &mut TcpStream, status: &str, headers: &[&str]) -> io::Result<()> {
    write!(stream, "HTTP/1.1 {status}\r\n")?;
    for header in headers {
        stream.write_all(header.as_bytes())?;
    }
    stream.write_all(b"Content-Length: 0\r\nConnection: close\r\n\r\n")
}

fn random_hex(bytes: usize) -> io::Result<String> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut random = vec![0u8; bytes];
    getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
    let mut value = String::with_capacity(bytes * 2);
    for byte in random {
        value.push(HEX[(byte >> 4) as usize] as char);
        value.push(HEX[(byte & 0xf) as usize] as char);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn bootstrap_and_cookie_authentication_are_exact() {
        let bootstrap = b"GET /_pimp/bootstrap/abc HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert_eq!(request_target(bootstrap), Some("/_pimp/bootstrap/abc"));
        assert!(!has_cookie(bootstrap, "secret"));
        assert!(has_cookie(
            b"GET / HTTP/1.1\r\nCookie: other=x; pimp_dsh_session=secret\r\n\r\n",
            "secret"
        ));
        assert!(!has_cookie(
            b"GET / HTTP/1.1\r\nCookie: pimp_dsh_session=wrong\r\n\r\n",
            "secret"
        ));
    }

    #[test]
    fn header_reader_preserves_body_bytes_already_received() {
        let expected = b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\nbody";
        let mut input = Cursor::new(expected);
        assert_eq!(read_header(&mut input).unwrap(), expected);
    }

    #[test]
    fn normal_connection_close_errors_are_not_proxy_faults() {
        for code in [109, 232, 995] {
            assert!(connection_closed(&io::Error::from_raw_os_error(code)));
        }
        assert!(!connection_closed(&io::Error::from(
            io::ErrorKind::WouldBlock
        )));
        assert!(connection_rejected(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
        assert!(!connection_rejected(&io::Error::from(
            io::ErrorKind::TimedOut
        )));
        assert!(constant_time_eq(&[0xaa; 64], &[0xaa; 64]));
        assert!(!constant_time_eq(&[0xaa; 64], &[0xab; 64]));
        assert!(!constant_time_eq(&[0xaa; 63], &[0xaa; 64]));
    }

    #[cfg(windows)]
    #[test]
    fn bootstrap_rejection_and_teardown_contract() {
        fn request(port: u16, request: &str) -> String {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.write_all(request.as_bytes()).unwrap();
            stream.shutdown(Shutdown::Write).unwrap();
            let mut response = String::new();
            if let Err(error) = stream.read_to_string(&mut response) {
                assert!(connection_closed(&error), "read proxy response: {error}");
            }
            response
        }

        let control_name = format!(
            r"\\.\pipe\pimp-dsh-web-control-test-{}-{}",
            std::process::id(),
            random_hex(8).unwrap()
        );
        let control_pipe = Arc::new(BridgePipe::create(&control_name).unwrap());
        let token = random_hex(32).unwrap();
        let control = Arc::new(ControlChannel::new(
            control_pipe,
            HostFrameEncoder::new("web-proxy-test", token).unwrap(),
        ));
        let bound = BoundWebProxy::bind(0).unwrap();
        let port = bound.port();
        let bootstrap_path = bound.bootstrap_path.clone();
        let mut running = bound.start(control, BridgePipe::create).unwrap();

        let response = request(
            port,
            &format!("GET {bootstrap_path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"),
        );
        assert!(response.starts_with("HTTP/1.1 303 See Other\r\n"));
        assert!(response.contains("Set-Cookie: pimp_dsh_session="));
        assert!(response.contains("; Path=/; HttpOnly; SameSite=Strict\r\n"));
        assert!(!response.contains("Domain="));
        assert!(response.contains("Referrer-Policy: no-referrer\r\n"));
        assert!(response.contains("Cache-Control: no-store\r\n"));
        assert!(
            request(port, "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                .starts_with("HTTP/1.1 403 Forbidden\r\n")
        );
        assert!(
            request(
                port,
                "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: pimp_dsh_session=wrong\r\n\r\n"
            )
            .starts_with("HTTP/1.1 403 Forbidden\r\n")
        );

        let reset = TcpStream::connect(("127.0.0.1", port)).unwrap();
        reset.shutdown(Shutdown::Both).unwrap();
        drop(reset);
        thread::sleep(Duration::from_millis(50));
        assert_eq!(running.fault(), None);

        let held = (0..MAX_ACTIVE_CONNECTIONS)
            .map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap())
            .collect::<Vec<_>>();
        thread::sleep(Duration::from_millis(100));
        assert!(
            request(port, "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                .starts_with("HTTP/1.1 503 Service Unavailable\r\n"),
            "connections beyond the authentication bound must be rejected"
        );
        drop(held);
        thread::sleep(Duration::from_millis(100));

        for _ in 0..8 {
            let _ = request(port, "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        }
        thread::sleep(Duration::from_millis(50));
        let _ = request(port, "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        thread::sleep(Duration::from_millis(50));
        let worker_count = running
            .workers
            .lock()
            .map(|handles| handles.len())
            .unwrap_or(usize::MAX);
        assert!(
            worker_count <= 2,
            "finished connection worker handles must be reaped during the run"
        );

        let started = Instant::now();
        running.stop_and_join();
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
