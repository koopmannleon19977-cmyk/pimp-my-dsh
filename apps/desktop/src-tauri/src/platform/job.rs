//! Windows Job Object wrapper: unnamed, kill-on-close, assign-before-resume.

#[cfg(windows)]
mod imp {
    use std::io;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{
        CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Security::SECURITY_CAPABILITIES;
    use windows_sys::Win32::Storage::FileSystem::ReadFile;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
        QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
        DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess,
        InitializeProcThreadAttributeList, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
        PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, PROCESS_INFORMATION, ResumeThread,
        STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW, TerminateProcess,
        UpdateProcThreadAttribute, WaitForSingleObject,
    };

    use super::super::confinement::Confinement;

    use crate::compatibility::LaunchSpec;

    /// A retained, non-inheritable process handle (the authority, never a PID).
    pub struct ChildProcessHandle(HANDLE);
    unsafe impl Send for ChildProcessHandle {}

    impl ChildProcessHandle {
        /// Wait up to `timeout` for the process to exit; `Some(code)` on exit.
        pub fn wait_timeout(&self, timeout: Duration) -> io::Result<Option<u32>> {
            let ms = (timeout.as_millis().min(u32::MAX as u128)) as u32;
            // SAFETY: self.0 is a valid, non-null process handle.
            let r = unsafe { WaitForSingleObject(self.0, ms) };
            match r {
                WAIT_OBJECT_0 => {
                    let mut code = 0u32;
                    // SAFETY: self.0 is a valid process handle.
                    let ok = unsafe { GetExitCodeProcess(self.0, &mut code) };
                    if ok == 0 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(Some(code))
                }
                WAIT_TIMEOUT => Ok(None),
                _ => Err(io::Error::last_os_error()),
            }
        }
    }

    impl Drop for ChildProcessHandle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: self.0 is a valid handle we own.
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    /// Primary-thread handle (for `ResumeThread`).
    pub struct ThreadHandle(HANDLE);
    unsafe impl Send for ThreadHandle {}

    impl Drop for ThreadHandle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: self.0 is a valid handle we own.
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    /// A pipe read/write end (drained by the supervisor).
    pub struct FileHandle(HANDLE);
    unsafe impl Send for FileHandle {}

    impl FileHandle {
        /// Read up to `buf.len()` bytes (blocking). `Ok(0)` = EOF.
        pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
            let mut n = 0u32;
            // SAFETY: self.0 is a valid read handle; buf is valid.
            let ok = unsafe {
                ReadFile(
                    self.0,
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                    &mut n,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(n as usize)
        }
    }

    impl Drop for FileHandle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: self.0 is a valid handle we own.
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    /// A spawned, suspended child plus its stdio pipe ends.
    pub struct Child {
        pub process: ChildProcessHandle,
        pub thread: ThreadHandle,
        pub stdin: Option<FileHandle>,
        pub stdout: Option<FileHandle>,
        pub stderr: Option<FileHandle>,
    }

    impl Child {
        /// Wait for the root process to exit; `true` if it exited within
        /// `timeout`.
        pub fn wait(&self, timeout: Duration) -> io::Result<bool> {
            Ok(self.process.wait_timeout(timeout)?.is_some())
        }

        /// Directly terminate the root process. Unlike [`Job::terminate`], this
        /// reaches a process that has not yet been assigned to any Job (e.g. an
        /// assignment failure while the primary thread is still suspended).
        pub fn terminate(&self) -> io::Result<()> {
            // SAFETY: self.process.0 is a valid process handle.
            let ok = unsafe { TerminateProcess(self.process.0, 1) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        /// Terminate the root and wait (bounded) for it to exit. Used by the
        /// pre-assignment RAII guard, where the Job cannot reach the child yet.
        pub fn kill_and_wait(&self, timeout: Duration) {
            let _ = self.terminate();
            let _ = self.wait(timeout);
        }
    }

    /// RAII guard over a suspended-but-not-yet-assigned child. Dropping it
    /// before [`ChildGuard::into_inner`] terminates and waits the
    /// still-unassigned process directly — `TerminateJobObject` cannot reach a
    /// process that has not been assigned to the Job.
    pub struct ChildGuard {
        child: Option<Child>,
    }

    impl ChildGuard {
        pub fn new(child: Child) -> Self {
            ChildGuard { child: Some(child) }
        }

        /// Borrow the guarded child (for `Job::assign` / `Job::resume`).
        pub fn child(&self) -> &Child {
            self.child.as_ref().expect("guard already consumed")
        }

        /// Hand ownership to the caller after a successful assign + resume.
        pub fn into_inner(mut self) -> Child {
            self.child.take().expect("guard already consumed")
        }
    }

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if let Some(child) = self.child.take() {
                child.kill_and_wait(Duration::from_secs(2));
            }
        }
    }

    /// Unnamed kill-on-close Job Object.
    pub struct Job {
        handle: HANDLE,
    }
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        /// Create an unnamed Job and set `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
        pub fn new() -> io::Result<Self> {
            // SAFETY: NULL name (unnamed), NULL security attributes → default,
            // non-inheritable handle.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: handle + info are valid; size matches the structure.
            let ok = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                // SAFETY: handle valid; best-effort cleanup before failing.
                unsafe { CloseHandle(handle) };
                return Err(io::Error::last_os_error());
            }
            Ok(Job { handle })
        }

        /// Spawn `node.exe` with `CREATE_NO_WINDOW | CREATE_SUSPENDED`, an
        /// explicit environment, a fixed argv, and an explicit inherited-handle
        /// allowlist (stdio only). The primary thread stays suspended.
        pub fn create_suspended(&self, app: &LaunchSpec) -> io::Result<Child> {
            self.create_suspended_with(app, None)
        }

        /// Like [`Job::create_suspended`] but additionally attaches
        /// `confinement` (an AppContainer identity) via
        /// `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`, so the spawned
        /// process runs under the AppContainer's restricted, read-side ACL.
        pub fn create_suspended_with(
            &self,
            app: &LaunchSpec,
            confinement: Option<&Confinement>,
        ) -> io::Result<Child> {
            // 1. stdio pipes (all inheritable; the handle list selects the child ends).
            let (stdin_read, stdin_write) = super::super::winutil::create_pipe()?;
            let (stdout_read, stdout_write) = super::super::winutil::create_pipe()?;
            let (stderr_read, stderr_write) = super::super::winutil::create_pipe()?;

            // Parent's own ends must not be inherited by any future inheritable spawn.
            // SAFETY: valid handles.
            unsafe {
                SetHandleInformation(stdin_write, HANDLE_FLAG_INHERIT, 0);
                SetHandleInformation(stdout_read, HANDLE_FLAG_INHERIT, 0);
                SetHandleInformation(stderr_read, HANDLE_FLAG_INHERIT, 0);
            }

            // 2. STARTUPINFOEX with an explicit handle list (plus optional
            //    AppContainer security capabilities).
            let mut si: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
            si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            si.StartupInfo.hStdInput = stdin_read;
            si.StartupInfo.hStdOutput = stdout_write;
            si.StartupInfo.hStdError = stderr_write;

            let handle_list = [stdin_read, stdout_write, stderr_write];
            let attr_count = if confinement.is_some() { 2 } else { 1 };

            let mut attr_size: usize = 0;
            // SAFETY: first call with null list returns the required size.
            unsafe {
                InitializeProcThreadAttributeList(
                    std::ptr::null_mut(),
                    attr_count,
                    0,
                    &mut attr_size,
                );
            }
            let mut attr_buf: Vec<u8> = vec![0; attr_size];
            si.lpAttributeList = attr_buf.as_mut_ptr() as *mut core::ffi::c_void;
            // SAFETY: attr list buffer is `attr_size` bytes; `attr_count` attributes.
            let ok = unsafe {
                InitializeProcThreadAttributeList(si.lpAttributeList, attr_count, 0, &mut attr_size)
            };
            if ok == 0 {
                let e = io::Error::last_os_error();
                close_many(&[
                    stdin_read,
                    stdin_write,
                    stdout_read,
                    stdout_write,
                    stderr_read,
                    stderr_write,
                ]);
                return Err(e);
            }
            // SAFETY: attribute list valid; handle list is a contiguous HANDLE array.
            let ok = unsafe {
                UpdateProcThreadAttribute(
                    si.lpAttributeList,
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    handle_list.as_ptr() as *const core::ffi::c_void,
                    handle_list.len() * std::mem::size_of::<HANDLE>(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            };
            if ok == 0 {
                let e = io::Error::last_os_error();
                // SAFETY: attribute list valid.
                unsafe { DeleteProcThreadAttributeList(si.lpAttributeList) };
                close_many(&[
                    stdin_read,
                    stdin_write,
                    stdout_read,
                    stdout_write,
                    stderr_read,
                    stderr_write,
                ]);
                return Err(e);
            }

            // 2b. Attach the AppContainer security capabilities alongside the
            // handle list when confinement is requested.
            if let Some(conf) = confinement {
                // SAFETY: attribute list valid; pointer + size describe the
                // SECURITY_CAPABILITIES owned by `conf`, which outlives this
                // call (CreateProcessW copies the struct during the call).
                let ok = unsafe {
                    UpdateProcThreadAttribute(
                        si.lpAttributeList,
                        0,
                        PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
                        conf.security_capabilities_ptr() as *const core::ffi::c_void,
                        std::mem::size_of::<SECURITY_CAPABILITIES>(),
                        std::ptr::null_mut(),
                        std::ptr::null(),
                    )
                };
                if ok == 0 {
                    let e = io::Error::last_os_error();
                    // SAFETY: attribute list valid.
                    unsafe { DeleteProcThreadAttributeList(si.lpAttributeList) };
                    close_many(&[
                        stdin_read,
                        stdin_write,
                        stdout_read,
                        stdout_write,
                        stderr_read,
                        stderr_write,
                    ]);
                    return Err(e);
                }
            }

            // 3. Environment block + command line.
            let env_block = super::super::winutil::build_env_block(&app.env);
            let cmdline = build_command_line(app);
            let cmdline_wide = super::super::winutil::to_wide_null(std::ffi::OsStr::new(&cmdline));
            let mut cmdline_mut = cmdline_wide;
            let node_wide = super::super::winutil::to_wide_null(app.node_exe.as_os_str());
            let cwd_wide = super::super::winutil::to_wide_null(app.cwd.as_os_str());

            let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
            let flags = CREATE_NO_WINDOW
                | CREATE_SUSPENDED
                | CREATE_UNICODE_ENVIRONMENT
                | EXTENDED_STARTUPINFO_PRESENT;

            // SAFETY: all pointers valid; lpApplicationName is the absolute
            // node.exe; bInheritHandles=TRUE with an explicit handle list.
            let ok = unsafe {
                CreateProcessW(
                    node_wide.as_ptr(),
                    cmdline_mut.as_mut_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    1,
                    flags,
                    env_block.as_ptr() as *const core::ffi::c_void,
                    cwd_wide.as_ptr(),
                    &si.StartupInfo as *const STARTUPINFOW,
                    &mut pi,
                )
            };
            // SAFETY: attribute list valid; release it.
            unsafe { DeleteProcThreadAttributeList(si.lpAttributeList) };
            // Close the child-side pipe ends in the parent.
            close_many(&[stdin_read, stdout_write, stderr_write]);

            if ok == 0 {
                let e = io::Error::last_os_error();
                if !pi.hProcess.is_null() {
                    // SAFETY: valid handle.
                    unsafe { CloseHandle(pi.hProcess) };
                }
                if !pi.hThread.is_null() {
                    // SAFETY: valid handle.
                    unsafe { CloseHandle(pi.hThread) };
                }
                close_many(&[stdin_write, stdout_read, stderr_read]);
                return Err(e);
            }

            Ok(Child {
                process: ChildProcessHandle(pi.hProcess),
                thread: ThreadHandle(pi.hThread),
                stdin: Some(FileHandle(stdin_write)),
                stdout: Some(FileHandle(stdout_read)),
                stderr: Some(FileHandle(stderr_read)),
            })
        }

        /// Assign the suspended child to this Job (before resume).
        pub fn assign(&self, child: &Child) -> io::Result<()> {
            // SAFETY: handle + process handle valid.
            let ok = unsafe { AssignProcessToJobObject(self.handle, child.process.0) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        /// Resume the child's primary thread.
        pub fn resume(&self, child: &Child) -> io::Result<()> {
            // SAFETY: thread handle valid. ResumeThread returns the previous
            // suspend count, or u32::MAX on failure.
            let r = unsafe { ResumeThread(child.thread.0) };
            if r == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        /// Number of processes currently associated with the Job.
        pub fn active_process_count(&self) -> io::Result<u32> {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
            // SAFETY: handle + info valid.
            let ok = unsafe {
                QueryInformationJobObject(
                    self.handle,
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(info.ActiveProcesses)
        }

        /// Force-terminate every process in the Job.
        pub fn terminate(&self) -> io::Result<()> {
            // SAFETY: handle valid.
            let ok = unsafe { TerminateJobObject(self.handle, 1) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        /// Poll until the Job has zero active processes or the timeout elapses.
        pub fn wait_empty(&self, timeout: Duration) -> io::Result<bool> {
            let deadline = Instant::now() + timeout;
            loop {
                if self.active_process_count()? == 0 {
                    return Ok(true);
                }
                if Instant::now() >= deadline {
                    return Ok(false);
                }
                std::thread::sleep(Duration::from_millis(40));
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                // SAFETY: closing the last handle triggers kill-on-close.
                unsafe { CloseHandle(self.handle) };
            }
        }
    }

    fn build_command_line(app: &LaunchSpec) -> String {
        let mut cmd = super::super::winutil::quote_cmd_arg(&app.node_exe.to_string_lossy());
        for part in
            std::iter::once(app.cli_entry.as_os_str()).chain(app.args.iter().map(|a| a.as_os_str()))
        {
            cmd.push(' ');
            cmd.push_str(&super::super::winutil::quote_cmd_arg(
                &part.to_string_lossy(),
            ));
        }
        cmd
    }

    fn close_many(handles: &[HANDLE]) {
        for &h in handles {
            if !h.is_null() {
                // SAFETY: each handle valid and owned by the caller.
                unsafe { CloseHandle(h) };
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::io;
    use std::time::Duration;

    use super::confinement::Confinement;
    use crate::compatibility::LaunchSpec;

    /// Portable stub (non-Windows: pure-contract compile-test only).
    pub struct ChildProcessHandle;
    impl ChildProcessHandle {
        pub fn wait_timeout(&self, _timeout: Duration) -> io::Result<Option<u32>> {
            Err(unsupported())
        }
    }

    pub struct ThreadHandle;
    pub struct FileHandle;
    impl FileHandle {
        pub fn read(&self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(unsupported())
        }
    }

    pub struct Child {
        pub process: ChildProcessHandle,
        pub thread: ThreadHandle,
        pub stdin: Option<FileHandle>,
        pub stdout: Option<FileHandle>,
        pub stderr: Option<FileHandle>,
    }

    impl Child {
        pub fn wait(&self, _timeout: Duration) -> io::Result<bool> {
            Err(unsupported())
        }
        pub fn terminate(&self) -> io::Result<()> {
            Err(unsupported())
        }
        pub fn kill_and_wait(&self, _timeout: Duration) {
            let _ = self.terminate();
        }
    }

    pub struct ChildGuard {
        child: Option<Child>,
    }
    impl ChildGuard {
        pub fn new(child: Child) -> Self {
            ChildGuard { child: Some(child) }
        }
        pub fn child(&self) -> &Child {
            self.child.as_ref().expect("guard already consumed")
        }
        pub fn into_inner(mut self) -> Child {
            self.child.take().expect("guard already consumed")
        }
    }
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if let Some(child) = self.child.take() {
                child.kill_and_wait(Duration::from_secs(2));
            }
        }
    }

    pub struct Job;
    impl Job {
        pub fn new() -> io::Result<Self> {
            Err(unsupported())
        }
        pub fn create_suspended(&self, _app: &LaunchSpec) -> io::Result<Child> {
            Err(unsupported())
        }
        pub fn create_suspended_with(
            &self,
            _app: &LaunchSpec,
            _confinement: Option<&Confinement>,
        ) -> io::Result<Child> {
            Err(unsupported())
        }
        pub fn assign(&self, _child: &Child) -> io::Result<()> {
            Err(unsupported())
        }
        pub fn resume(&self, _child: &Child) -> io::Result<()> {
            Err(unsupported())
        }
        pub fn active_process_count(&self) -> io::Result<u32> {
            Err(unsupported())
        }
        pub fn terminate(&self) -> io::Result<()> {
            Err(unsupported())
        }
        pub fn wait_empty(&self, _timeout: Duration) -> io::Result<bool> {
            Err(unsupported())
        }
    }

    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows Job Object is Windows-only",
        )
    }
}

pub use imp::*;
