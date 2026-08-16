//! Private named pipe (bridge transport): current-user + SYSTEM DACL,
//! `PIPE_REJECT_REMOTE_CLIENTS`, first-instance creation.

#[cfg(windows)]
mod imp {
    use std::ffi::OsStr;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::thread;
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE,
        LocalFree, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
    };
    use windows_sys::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
        PeekNamedPipe,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, GetCurrentProcess, OpenProcessToken, WaitForSingleObject,
    };

    /// Owns the security resources backing the pipe DACL until the pipe exists.
    struct PipeSecurity {
        sa: SECURITY_ATTRIBUTES,
        psd: *mut core::ffi::c_void,
        sid_str: windows_sys::core::PWSTR,
        token: HANDLE,
    }

    impl PipeSecurity {
        /// Build a security descriptor DACL: current user + SYSTEM, deny others.
        fn new() -> io::Result<Self> {
            let mut token: HANDLE = std::ptr::null_mut();
            // SAFETY: token out-param valid; GetCurrentProcess() is a pseudo-handle.
            let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }

            // Query the token user SID (two calls: size, then data).
            let mut size = 0u32;
            // SAFETY: null buffer to query required size.
            unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut size) };
            let mut buf = vec![0u8; size as usize];
            // SAFETY: buf is size bytes; return length out-param valid.
            let ok = unsafe {
                GetTokenInformation(
                    token,
                    TokenUser,
                    buf.as_mut_ptr() as *mut core::ffi::c_void,
                    size,
                    &mut size,
                )
            };
            if ok == 0 {
                let e = io::Error::last_os_error();
                // SAFETY: token valid.
                unsafe { CloseHandle(token) };
                return Err(e);
            }
            let tu = buf.as_ptr() as *const TOKEN_USER;
            // SAFETY: tu points to a valid TOKEN_USER of `size` bytes.
            let sid = unsafe { (*tu).User.Sid };

            let mut sid_str: windows_sys::core::PWSTR = std::ptr::null_mut();
            // SAFETY: sid is a valid SID; sid_str out-param valid.
            let ok = unsafe { ConvertSidToStringSidW(sid, &mut sid_str) };
            if ok == 0 {
                let e = io::Error::last_os_error();
                // SAFETY: token valid.
                unsafe { CloseHandle(token) };
                return Err(e);
            }

            let sid_string = read_wide(sid_str);
            let sddl = format!("D:(A;;GA;;;SY)(A;;GA;;;{sid_string})");
            let sddl_wide: Vec<u16> = OsStr::new(&sddl)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let mut psd: *mut core::ffi::c_void = std::ptr::null_mut();
            // SAFETY: sddl_wide is a valid SDDL string; psd out-param valid.
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl_wide.as_ptr(),
                    1,
                    &mut psd,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                let e = io::Error::last_os_error();
                // SAFETY: token + sid_str valid.
                unsafe {
                    CloseHandle(token);
                    LocalFree(sid_str as *mut core::ffi::c_void);
                }
                return Err(e);
            }

            let mut sa: SECURITY_ATTRIBUTES = unsafe { std::mem::zeroed() };
            sa.nLength = std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32;
            sa.lpSecurityDescriptor = psd;
            sa.bInheritHandle = 0;

            Ok(PipeSecurity {
                sa,
                psd,
                sid_str,
                token,
            })
        }
    }

    impl Drop for PipeSecurity {
        fn drop(&mut self) {
            // SAFETY: all owned pointers/handles valid.
            unsafe {
                if !self.psd.is_null() {
                    LocalFree(self.psd);
                }
                if !self.sid_str.is_null() {
                    LocalFree(self.sid_str as *mut core::ffi::c_void);
                }
                if !self.token.is_null() {
                    CloseHandle(self.token);
                }
            }
        }
    }

    fn read_wide(ptr: windows_sys::core::PWSTR) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut s = String::new();
        let mut i = 0usize;
        loop {
            // SAFETY: ptr points to a NUL-terminated UTF-16 string.
            let c = unsafe { *ptr.add(i) };
            if c == 0 {
                break;
            }
            s.push(char::from_u32(c as u32).unwrap_or('\u{fffd}'));
            i += 1;
        }
        s
    }

    /// A first-instance, remote-rejecting named pipe server end.
    pub struct BridgePipe {
        handle: HANDLE,
        name: String,
    }
    unsafe impl Send for BridgePipe {}
    unsafe impl Sync for BridgePipe {}

    impl BridgePipe {
        /// Create the pipe. Fails if the name pre-exists (first-instance) or if
        /// the DACL/remote-reject setup fails.
        pub fn create(name: &str) -> io::Result<Self> {
            let sec = PipeSecurity::new()?;
            let name_wide = OsStr::new(name)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>();

            let open_mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE;
            let pipe_mode =
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;

            // SAFETY: name_wide + security attributes valid; first-instance
            // flag makes a second create with the same name fail.
            let handle = unsafe {
                CreateNamedPipeW(
                    name_wide.as_ptr(),
                    open_mode,
                    pipe_mode,
                    PIPE_UNLIMITED_INSTANCES,
                    64 * 1024,
                    64 * 1024,
                    0,
                    &sec.sa,
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            Ok(BridgePipe {
                handle,
                name: name.to_string(),
            })
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        /// Block until a client connects.
        pub fn connect(&self) -> io::Result<()> {
            // SAFETY: handle valid; null overlapped → blocking wait.
            let ok = unsafe { ConnectNamedPipe(self.handle, std::ptr::null_mut()) };
            if ok == 0 {
                let e = io::Error::last_os_error();
                if e.raw_os_error() == Some(ERROR_PIPE_CONNECTED as i32) {
                    return Ok(()); // client already connected
                }
                return Err(e);
            }
            Ok(())
        }

        /// Block until a client connects, or fail after `timeout`. A bounded
        /// connect is the outer edge of the handshake deadline: a child that
        /// never connects cannot pin the bridge reader thread forever.
        pub fn connect_timeout(&self, timeout: Duration) -> io::Result<()> {
            // Manual-reset event to signal the overlapped connect completion.
            // SAFETY: null security attributes, manual-reset, initially
            // non-signaled, no name.
            let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
            if event.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
            overlapped.hEvent = event;

            // SAFETY: valid handle + overlapped event.
            let ok = unsafe { ConnectNamedPipe(self.handle, &mut overlapped) };
            if ok != 0 {
                // Connected synchronously.
                // SAFETY: event valid.
                unsafe { CloseHandle(event) };
                return Ok(());
            }
            let e = io::Error::last_os_error();
            if e.raw_os_error() == Some(ERROR_PIPE_CONNECTED as i32) {
                // A client was already connected.
                // SAFETY: event valid.
                unsafe { CloseHandle(event) };
                return Ok(());
            }
            if e.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
                // SAFETY: event valid.
                unsafe { CloseHandle(event) };
                return Err(e);
            }

            let ms = (timeout.as_millis().min(u32::MAX as u128)) as u32;
            // SAFETY: event valid.
            let r = unsafe { WaitForSingleObject(event, ms) };
            match r {
                WAIT_OBJECT_0 => {
                    // Confirm the overlapped connect actually succeeded.
                    let mut transferred = 0u32;
                    // SAFETY: handle + overlapped valid; non-blocking final query.
                    let ok = unsafe {
                        GetOverlappedResult(self.handle, &overlapped, &mut transferred, 0)
                    };
                    // SAFETY: event valid.
                    unsafe { CloseHandle(event) };
                    if ok == 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                }
                WAIT_TIMEOUT => {
                    // SAFETY: event valid.
                    unsafe { CloseHandle(event) };
                    Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "pipe connect timed out",
                    ))
                }
                _ => {
                    // SAFETY: event valid.
                    unsafe { CloseHandle(event) };
                    Err(io::Error::last_os_error())
                }
            }
        }

        /// Read up to `buf.len()` bytes without pinning the duplex handle in a
        /// blocking read. A pending synchronous `ReadFile` serializes a
        /// concurrent shutdown `WriteFile` on the same pipe handle.
        pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
            if buf.is_empty() {
                return Ok(0);
            }
            loop {
                let mut available = 0u32;
                // SAFETY: handle is valid; only the available-byte count is requested.
                let ok = unsafe {
                    PeekNamedPipe(
                        self.handle,
                        std::ptr::null_mut(),
                        0,
                        std::ptr::null_mut(),
                        &mut available,
                        std::ptr::null_mut(),
                    )
                };
                if ok == 0 {
                    return Err(io::Error::last_os_error());
                }
                if available == 0 {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }

                let mut n = 0u32;
                let len = buf.len().min(available as usize) as u32;
                // SAFETY: handle + buf are valid, and PeekNamedPipe confirmed
                // that this read can complete without waiting for peer input.
                let ok = unsafe {
                    ReadFile(
                        self.handle,
                        buf.as_mut_ptr(),
                        len,
                        &mut n,
                        std::ptr::null_mut(),
                    )
                };
                if ok == 0 {
                    return Err(io::Error::last_os_error());
                }
                return Ok(n as usize);
            }
        }

        /// Read exactly `buf.len()` bytes.
        pub fn read_exact(&self, buf: &mut [u8]) -> io::Result<()> {
            let mut n = 0usize;
            while n < buf.len() {
                let r = self.read(&mut buf[n..])?;
                if r == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "bridge pipe closed",
                    ));
                }
                n += r;
            }
            Ok(())
        }

        /// Write all bytes.
        pub fn write_all(&self, buf: &[u8]) -> io::Result<()> {
            let mut n = 0usize;
            while n < buf.len() {
                let mut written = 0u32;
                // SAFETY: handle + buf valid.
                let ok = unsafe {
                    WriteFile(
                        self.handle,
                        buf[n..].as_ptr(),
                        (buf.len() - n) as u32,
                        &mut written,
                        std::ptr::null_mut(),
                    )
                };
                if ok == 0 {
                    return Err(io::Error::last_os_error());
                }
                n += written as usize;
            }
            Ok(())
        }

        pub fn disconnect(&self) {
            // SAFETY: handle valid; best-effort disconnect.
            unsafe { DisconnectNamedPipe(self.handle) };
        }
    }

    impl Drop for BridgePipe {
        fn drop(&mut self) {
            if self.handle != INVALID_HANDLE_VALUE && !self.handle.is_null() {
                // SAFETY: handle valid.
                unsafe { CloseHandle(self.handle) };
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::io;

    pub struct BridgePipe {
        name: String,
    }

    impl BridgePipe {
        pub fn create(_name: &str) -> io::Result<Self> {
            Err(unsupported())
        }
        pub fn name(&self) -> &str {
            &self.name
        }
        pub fn connect(&self) -> io::Result<()> {
            Err(unsupported())
        }
        pub fn connect_timeout(&self, _timeout: std::time::Duration) -> io::Result<()> {
            Err(unsupported())
        }
        pub fn read(&self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(unsupported())
        }
        pub fn read_exact(&self, _buf: &mut [u8]) -> io::Result<()> {
            Err(unsupported())
        }
        pub fn write_all(&self, _buf: &[u8]) -> io::Result<()> {
            Err(unsupported())
        }
        pub fn disconnect(&self) {}
    }

    fn unsupported() -> io::Error {
        io::Error::new(io::ErrorKind::Unsupported, "named pipe is Windows-only")
    }
}

pub use imp::*;
