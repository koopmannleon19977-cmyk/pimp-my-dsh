//! Opt-in Windows AppContainer read-side confinement prototype.
//!
//! Every instance creates a fresh AppContainer profile with a private staging
//! root. The confined executable and its fixture/runtime payload must be
//! staged below that root. No user-profile, workspace, `%TEMP%`, or drive-root
//! DACL is modified. The prototype does not add access to external caller
//! paths; it does not claim that an AppContainer cannot read all world-readable
//! host locations. Cleanup removes the profile and its private root.
//!
//! This is deliberately not the desktop supervisor's default launcher. See
//! `docs/adr/0004-windows-read-side-confinement.md` for the production matrix.

#[cfg(windows)]
mod imp {
    use std::ffi::OsString;
    use std::io;
    use std::os::windows::ffi::OsStringExt;
    use std::path::{Path, PathBuf};
    use std::ptr;

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::Isolation::{
        CreateAppContainerProfile, DeleteAppContainerProfile, GetAppContainerFolderPath,
    };
    use windows_sys::Win32::Security::{FreeSid, PSID, SECURITY_CAPABILITIES};

    /// A fresh, profile-backed AppContainer identity for one confined run.
    pub struct Confinement {
        profile_name: String,
        private_root: PathBuf,
        profile_deleted: bool,
        app_sid: PSID,
        caps: SECURITY_CAPABILITIES,
    }

    // The Job borrows caps only during CreateProcessW; this owns the SID.
    unsafe impl Send for Confinement {}
    unsafe impl Sync for Confinement {}

    impl Confinement {
        /// Create an unprivileged AppContainer profile without any capability
        /// SIDs. Its private root is the only allowed staging location.
        pub fn create() -> io::Result<Self> {
            let profile_name = app_container_name()?;
            let profile_wide = wide(&profile_name);
            let mut app_sid: PSID = ptr::null_mut();
            // SAFETY: strings are NUL-terminated; no network capabilities are
            // requested; app_sid receives an owned SID on success.
            let hr = unsafe {
                CreateAppContainerProfile(
                    profile_wide.as_ptr(),
                    profile_wide.as_ptr(),
                    profile_wide.as_ptr(),
                    ptr::null(),
                    0,
                    &mut app_sid,
                )
            };
            if hr != 0 || app_sid.is_null() {
                return Err(hresult_error("CreateAppContainerProfile", hr));
            }

            let private_root = match app_container_folder(app_sid) {
                Ok(path) => path,
                Err(error) => {
                    // SAFETY: profile was just created and this scope owns sid.
                    unsafe {
                        DeleteAppContainerProfile(profile_wide.as_ptr());
                        FreeSid(app_sid);
                    }
                    return Err(error);
                }
            };
            if let Err(error) = std::fs::create_dir_all(&private_root) {
                // SAFETY: profile was just created and this scope owns sid.
                unsafe {
                    DeleteAppContainerProfile(profile_wide.as_ptr());
                    FreeSid(app_sid);
                }
                return Err(error);
            }

            Ok(Self {
                profile_name,
                private_root,
                profile_deleted: false,
                app_sid,
                caps: SECURITY_CAPABILITIES {
                    AppContainerSid: app_sid,
                    Capabilities: ptr::null_mut(),
                    CapabilityCount: 0,
                    Reserved: 0,
                },
            })
        }

        /// The profile-owned root. Stage the confined executable and every
        /// readable fixture/runtime file here; external paths are never added.
        pub fn private_root(&self) -> &Path {
            &self.private_root
        }

        /// Pointer valid for this instance's lifetime, for
        /// PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES.
        pub fn security_capabilities_ptr(&self) -> *const SECURITY_CAPABILITIES {
            &self.caps
        }

        /// Attempt every cleanup step: remove the private root, unregister the
        /// profile, and free its SID. The first failure is returned only after
        /// profile deletion and SID release have also been attempted.
        pub fn cleanup(&mut self) -> io::Result<()> {
            let mut first_error = None;
            if self.private_root.exists()
                && let Err(error) = std::fs::remove_dir_all(&self.private_root)
            {
                first_error = Some(error);
            }
            if !self.profile_deleted {
                let profile_wide = wide(&self.profile_name);
                // SAFETY: profile_name identifies this unique per-run profile.
                let hr = unsafe { DeleteAppContainerProfile(profile_wide.as_ptr()) };
                if hr == 0 {
                    self.profile_deleted = true;
                } else if first_error.is_none() {
                    first_error = Some(hresult_error("DeleteAppContainerProfile", hr));
                }
            }
            self.free_sid();
            match first_error {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }

        fn free_sid(&mut self) {
            if !self.app_sid.is_null() {
                // SAFETY: this instance owns the SID from profile creation.
                unsafe { FreeSid(self.app_sid) };
                self.app_sid = ptr::null_mut();
                self.caps.AppContainerSid = ptr::null_mut();
            }
        }
    }

    impl Drop for Confinement {
        fn drop(&mut self) {
            // Drop cannot report cleanup failure; normal/failure paths call
            // cleanup() explicitly when an operational result is required.
            let _ = self.cleanup();
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }

    fn app_container_name() -> io::Result<String> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes)
            .map_err(|error| io::Error::other(format!("random AppContainer name: {error}")))?;
        Ok(format!(
            "pimp-dsh-{}",
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ))
    }

    fn app_container_folder(app_sid: PSID) -> io::Result<PathBuf> {
        let mut sid_string = ptr::null_mut();
        // SAFETY: app_sid is valid; output is LocalFree-allocated.
        if unsafe { ConvertSidToStringSidW(app_sid, &mut sid_string) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut folder = ptr::null_mut();
        // SAFETY: sid_string is NUL-terminated; folder is LocalFree-allocated.
        let hr = unsafe { GetAppContainerFolderPath(sid_string, &mut folder) };
        // SAFETY: ConvertSidToStringSidW allocated sid_string.
        unsafe { LocalFree(sid_string as *mut core::ffi::c_void) };
        if hr != 0 || folder.is_null() {
            if !folder.is_null() {
                // SAFETY: GetAppContainerFolderPath allocated folder.
                unsafe { LocalFree(folder as *mut core::ffi::c_void) };
            }
            return Err(hresult_error("GetAppContainerFolderPath", hr));
        }

        let len = (0..32_768)
            .find(|&index| unsafe { *folder.add(index) == 0 })
            .ok_or_else(|| io::Error::other("unterminated AppContainer folder path"))?;
        // SAFETY: folder has len UTF-16 code units followed by NUL.
        let path = PathBuf::from(OsString::from_wide(unsafe {
            std::slice::from_raw_parts(folder, len)
        }));
        // SAFETY: GetAppContainerFolderPath allocated folder.
        unsafe { LocalFree(folder as *mut core::ffi::c_void) };
        Ok(path)
    }

    fn hresult_error(operation: &str, hr: i32) -> io::Error {
        io::Error::other(format!("{operation} failed (hr=0x{hr:08X})"))
    }
}

#[cfg(not(windows))]
mod imp {
    use std::io;
    use std::path::Path;

    /// Portable stub for pure contract compilation.
    pub struct Confinement;
    impl Confinement {
        pub fn create() -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "AppContainer confinement is Windows-only",
            ))
        }
        pub fn private_root(&self) -> &Path {
            Path::new("")
        }
        pub fn security_capabilities_ptr(&self) -> *const core::ffi::c_void {
            std::ptr::null()
        }
        pub fn cleanup(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::Unsupported, "Windows-only"))
        }
    }
}

pub use imp::*;
