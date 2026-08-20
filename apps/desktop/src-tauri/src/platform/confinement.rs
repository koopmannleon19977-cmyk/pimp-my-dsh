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
    use std::ffi::{OsStr, OsString};
    use std::io;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt, symlink_dir};
    use std::path::{Path, PathBuf};
    use std::ptr;

    use crate::compatibility::{LaunchSpec, expected_manifest_digest, verified_runtime_root};

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::Isolation::{
        CreateAppContainerProfile, DeleteAppContainerProfile, GetAppContainerFolderPath,
    };
    use windows_sys::Win32::Security::{FreeSid, PSID, SECURITY_CAPABILITIES};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
    };

    /// A fresh, profile-backed AppContainer identity for one confined run.
    pub struct Confinement {
        profile_name: String,
        private_root: PathBuf,
        profile_deleted: bool,
        app_sid: PSID,
        caps: SECURITY_CAPABILITIES,
    }

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

        /// Stage a full verified packaged runtime below this profile's private
        /// root and return a remapped [`LaunchSpec`]: node_exe, CLI entry, and
        /// cwd now point into `<private_root>/runtime`, while env + argv are
        /// preserved from the source.
        ///
        /// The source must already be a verified packaged runtime — it is
        /// re-authenticated here through the same shared
        /// [`verified_runtime_root`] helper the packaged provider uses, so a
        /// DevProvider/workspace or otherwise-unverified spec is rejected
        /// before any byte is copied. The recursive copy rejects reparse
        /// points (symlinks/junctions/custom reparse tags) and copies bytes
        /// (never hardlinks), so source links are broken in the private copy.
        /// The written copy is then re-verified through that same helper.
        pub fn stage_runtime(&self, source: &LaunchSpec) -> io::Result<LaunchSpec> {
            // Authenticate the source root first: a workspace/DevProvider spec
            // carries no packaged manifest and must be refused, never copied.
            let source_root = std::fs::canonicalize(&source.cwd).map_err(|e| {
                io::Error::other(format!(
                    "staging source {} is not resolvable: {e}",
                    source.cwd.display()
                ))
            })?;
            verified_runtime_root(&source_root, expected_manifest_digest()).map_err(|e| {
                io::Error::other(format!("staging source is not a packaged runtime: {e}"))
            })?;

            let dest_root = self.private_root.join("runtime");
            if dest_root.exists() {
                return Err(io::Error::other(format!(
                    "refusing to overwrite an existing staged runtime {}",
                    dest_root.display()
                )));
            }
            if let Err(error) = copy_runtime_tree(&source_root, &dest_root) {
                let cleanup = std::fs::remove_dir_all(&dest_root);
                return match cleanup {
                    Ok(()) => Err(error),
                    Err(cleanup_error) => Err(io::Error::other(format!(
                        "{error}; removing partial staged runtime also failed: {cleanup_error}"
                    ))),
                };
            }

            // Re-verify the private copy through the identical authenticator
            // (manifest digest + payload/node hashes), then remap to it while
            // keeping the source's environment and argv.
            let mut staged = match verified_runtime_root(&dest_root, expected_manifest_digest()) {
                Ok(staged) => staged,
                Err(error) => {
                    let cleanup = std::fs::remove_dir_all(&dest_root);
                    return match cleanup {
                        Ok(()) => Err(io::Error::other(format!(
                            "staged copy failed verification: {error}"
                        ))),
                        Err(cleanup_error) => Err(io::Error::other(format!(
                            "staged copy failed verification: {error}; removing it also failed: {cleanup_error}"
                        ))),
                    };
                }
            };
            staged.env = source.env.clone();
            staged
                .env
                .retain(|(name, _)| !name.to_string_lossy().eq_ignore_ascii_case("NODE_OPTIONS"));
            // Node 24 normally realpaths the main module through the volume
            // root (`C:\`), which a capability-free AppContainer cannot lstat.
            // Preserve the already-verified staged paths instead; no module is
            // resolved outside the authenticated private runtime tree.
            staged.env.push((
                OsString::from("NODE_OPTIONS"),
                OsString::from("--preserve-symlinks --preserve-symlinks-main"),
            ));
            staged.args = source.args.clone();
            Ok(staged)
        }

        /// Pointer valid for this instance's lifetime, for
        /// PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES.
        pub fn security_capabilities_ptr(&self) -> *const SECURITY_CAPABILITIES {
            &self.caps
        }

        pub(crate) fn app_container_sid(&self) -> PSID {
            self.app_sid
        }

        /// Physicalize the already-managed `web` profile under the private
        /// AppContainer root and relocate its sole distribution link to the
        /// verified staged runtime. No host profile path is granted to the
        /// child.
        pub fn stage_web_profile(
            &self,
            source_profile: &Path,
            staged_runtime: &LaunchSpec,
        ) -> io::Result<LaunchSpec> {
            let private_root = self.private_root.clone();
            let canonical_private_root = std::fs::canonicalize(&private_root)?;
            let runtime_root = std::fs::canonicalize(&staged_runtime.cwd)?;
            let launch_runtime = staged_runtime.cwd.clone();
            if !runtime_root.starts_with(&canonical_private_root) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "staged runtime is outside this AppContainer profile",
                ));
            }
            let source_profile = std::fs::canonicalize(source_profile)?;
            let marker_path = source_profile.join(".pimp-my-dsh.json");
            let marker: serde_json::Value = serde_json::from_slice(&std::fs::read(&marker_path)?)
                .map_err(|error| {
                io::Error::other(format!("invalid managed profile marker: {error}"))
            })?;
            if marker["schemaVersion"] != 1
                || marker["bundleVersion"] != "0.1.0"
                || marker["upstreamVersion"] != "0.1.0-rc.7"
                || marker["profile"] != "web"
            {
                return Err(io::Error::other(
                    "managed web profile marker does not match this distribution",
                ));
            }
            if std::fs::read(source_profile.join("cordis.patch.yml"))?
                != std::fs::read(runtime_root.join("profiles").join("web.patch.yml"))?
            {
                return Err(io::Error::other(
                    "managed web profile patch does not match the staged runtime",
                ));
            }

            let dsh_home = private_root.join("dsh-home");
            let dest_profile = dsh_home.join("profiles").join("web");
            let workspace = private_root.join("workspace");
            let temp = private_root.join("temp");
            let roaming = dsh_home.join("AppData").join("Roaming");
            let local = dsh_home.join("AppData").join("Local");
            let virtual_temp = local
                .join("Packages")
                .join(&self.profile_name)
                .join("AC")
                .join("Temp");
            for directory in [&workspace, &temp, &roaming, &local, &virtual_temp] {
                std::fs::create_dir_all(directory)?;
            }
            std::fs::write(dsh_home.join(".credentials.yaml"), b"")?;
            if dest_profile.exists() {
                return Err(io::Error::other(
                    "refusing to overwrite an existing private web profile",
                ));
            }
            copy_profile_tree(&source_profile, &dest_profile)?;
            stage_profile_module_fallback(&runtime_root, &dsh_home)?;

            let expected_link = format!(
                "link:{}",
                launch_runtime.to_string_lossy().replace('\\', "/")
            );
            rewrite_profile_package(&dest_profile.join("package.json"), &expected_link)?;
            rewrite_profile_lock(&dest_profile.join("pnpm-lock.yaml"), &expected_link)?;
            let linked_bundle = dest_profile.join("node_modules").join("pimp-my-dsh");
            symlink_dir(&launch_runtime, &linked_bundle).map_err(|error| {
                io::Error::other(format!("create private runtime link: {error}"))
            })?;

            if std::fs::canonicalize(&linked_bundle)? != runtime_root {
                return Err(io::Error::other(
                    "private profile bundle link does not target the staged runtime",
                ));
            }
            let package: serde_json::Value =
                serde_json::from_slice(&std::fs::read(dest_profile.join("package.json"))?)
                    .map_err(|error| {
                        io::Error::other(format!("invalid staged profile package: {error}"))
                    })?;
            if package["dependencies"]["pimp-my-dsh"] != expected_link {
                return Err(io::Error::other(
                    "private profile manifest was not relocated to the staged runtime",
                ));
            }
            let lock = std::fs::read_to_string(dest_profile.join("pnpm-lock.yaml"))?;
            if lock.contains(&source_profile.to_string_lossy().to_string())
                || !lock.contains(&format!("specifier: {expected_link}"))
                || !lock.contains("version: link:../../../runtime")
            {
                return Err(io::Error::other(
                    "private profile lockfile still references a host path",
                ));
            }

            let mut launch = staged_runtime.clone();
            launch.cwd = workspace;
            for (name, value) in [
                ("DSH_HOME", dsh_home.as_os_str()),
                ("HOME", dsh_home.as_os_str()),
                ("USERPROFILE", dsh_home.as_os_str()),
                ("APPDATA", roaming.as_os_str()),
                ("LOCALAPPDATA", local.as_os_str()),
                ("TEMP", temp.as_os_str()),
                ("TMP", temp.as_os_str()),
                ("DSH_PIMP_CONFINED_ROOT", self.private_root.as_os_str()),
            ] {
                upsert_env(&mut launch.env, name, value);
            }
            Ok(launch)
        }
        /// Remove distribution-owned children, unregister the profile so
        /// Windows can remove protected INet*/Temp directories, then remove
        /// any residue and free the SID. Every step is attempted.
        pub fn cleanup(&mut self) -> io::Result<()> {
            let mut first_error = None;
            if self.private_root.exists()
                && let Err(error) = remove_owned_children(&self.private_root)
            {
                first_error = Some(io::Error::new(
                    error.kind(),
                    format!("remove distribution-owned profile content: {error}"),
                ));
            }
            if !self.profile_deleted {
                let profile_wide = wide(&self.profile_name);
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                let last_hr = loop {
                    // SAFETY: profile_name identifies this unique per-run profile.
                    let hr = unsafe { DeleteAppContainerProfile(profile_wide.as_ptr()) };
                    if hr == 0 {
                        self.profile_deleted = true;
                        break 0;
                    }
                    if std::time::Instant::now() >= deadline {
                        break hr;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                };
                if !self.profile_deleted && first_error.is_none() {
                    first_error = Some(hresult_error("DeleteAppContainerProfile", last_hr));
                }
            }
            if self.profile_deleted {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while self.private_root.exists() && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                if self.private_root.exists() {
                    if first_error.is_none() {
                        first_error = Some(io::Error::new(
                            io::ErrorKind::TimedOut,
                            format!(
                                "deleted AppContainer profile root still exists: {}",
                                self.private_root.display()
                            ),
                        ));
                    }
                } else {
                    // Windows removed the protected tree; earlier best-effort
                    // child-removal errors are superseded.
                    first_error = None;
                }
            } else if self.private_root.exists()
                && let Err(error) = remove_private_tree(&self.private_root)
                && first_error.is_none()
            {
                first_error = Some(error);
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

    fn remove_owned_children(root: &Path) -> io::Result<()> {
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if matches!(
                name.as_str(),
                "inetcache" | "inethistory" | "inetcookies" | "temp"
            ) {
                continue;
            }
            let path = entry.path();
            remove_private_tree(&path).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("remove owned child {}: {error}", path.display()),
                )
            })?;
        }
        Ok(())
    }

    fn remove_private_tree(path: &Path) -> io::Result<()> {
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return match std::fs::remove_dir(path) {
                Ok(()) => Ok(()),
                Err(_) => std::fs::remove_file(path),
            };
        }
        if metadata.is_dir() {
            for entry in std::fs::read_dir(path)? {
                remove_private_tree(&entry?.path())?;
            }
            let mut permissions = metadata.permissions();
            if permissions.readonly() {
                permissions.set_readonly(false);
                std::fs::set_permissions(path, permissions)?;
            }
            std::fs::remove_dir(path)
        } else {
            let mut permissions = metadata.permissions();
            if permissions.readonly() {
                permissions.set_readonly(false);
                std::fs::set_permissions(path, permissions)?;
            }
            std::fs::remove_file(path)
        }
    }

    fn rewrite_profile_package(path: &Path, expected_link: &str) -> io::Result<()> {
        let input = std::fs::read_to_string(path)?;
        let mut found = false;
        let mut output = String::with_capacity(input.len() + expected_link.len());
        for line in input.split_inclusive('\n') {
            if line.trim_start().starts_with("\"pimp-my-dsh\": \"link:") {
                let body = line.trim_end_matches(['\r', '\n']);
                let indent = &body[..body.len() - body.trim_start().len()];
                let ending = if line.ends_with("\r\n") {
                    "\r\n"
                } else if line.ends_with('\n') {
                    "\n"
                } else {
                    ""
                };
                output.push_str(&format!(
                    "{indent}\"pimp-my-dsh\": \"{expected_link}\",{ending}"
                ));
                found = true;
            } else {
                output.push_str(line);
            }
        }
        if !found {
            return Err(io::Error::other(
                "managed profile package link entry was not found",
            ));
        }
        std::fs::write(path, output)
    }

    fn rewrite_profile_lock(path: &Path, expected_link: &str) -> io::Result<()> {
        let input = std::fs::read_to_string(path)?;
        let mut in_bundle = false;
        let mut specifier_found = false;
        let mut version_found = false;
        let mut output = String::with_capacity(input.len() + expected_link.len());
        for line in input.split_inclusive('\n') {
            let body = line.trim_end_matches(['\r', '\n']);
            let trimmed = body.trim_start();
            if trimmed == "pimp-my-dsh:" {
                in_bundle = true;
                output.push_str(line);
                continue;
            }
            if in_bundle
                && (trimmed.starts_with("specifier: link:")
                    || trimmed.starts_with("version: link:"))
            {
                let indent = &body[..body.len() - trimmed.len()];
                let ending = if line.ends_with("\r\n") {
                    "\r\n"
                } else if line.ends_with('\n') {
                    "\n"
                } else {
                    ""
                };
                if trimmed.starts_with("specifier:") {
                    output.push_str(&format!("{indent}specifier: {expected_link}{ending}"));
                    specifier_found = true;
                } else {
                    output.push_str(&format!("{indent}version: link:../../../runtime{ending}"));
                    version_found = true;
                    in_bundle = false;
                }
                continue;
            }
            output.push_str(line);
        }
        if !specifier_found || !version_found {
            return Err(io::Error::other(
                "managed profile lock link entries were not found",
            ));
        }
        std::fs::write(path, output)
    }

    fn upsert_env(env: &mut Vec<(OsString, OsString)>, name: &str, value: &OsStr) {
        env.retain(|(existing, _)| !existing.to_string_lossy().eq_ignore_ascii_case(name));
        env.push((OsString::from(name), value.to_os_string()));
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }

    /// Recursively byte-copy a packaged runtime tree using only directory
    /// creation + byte copies (no shell, no link/rename aliasing, no layout
    /// rewrite). Directories are recreated; every file's bytes are copied so a
    /// hard-linked or otherwise-shared source file becomes an independent
    /// private file. Any reparse point (symlink, junction, or custom reparse
    /// tag) anywhere in the tree is rejected — never traversed or replicated.
    fn copy_runtime_tree(source: &Path, dest: &Path) -> io::Result<()> {
        copy_tree(source, source, dest, None)
    }

    fn copy_profile_tree(source: &Path, dest: &Path) -> io::Result<()> {
        let skipped = PathBuf::from("node_modules").join("pimp-my-dsh");
        copy_tree(source, source, dest, Some(&skipped))
    }

    fn stage_profile_module_fallback(runtime_root: &Path, dsh_home: &Path) -> io::Result<()> {
        let source = runtime_root.join("node_modules");
        let destination = dsh_home.join("profiles").join("node_modules");
        std::fs::create_dir_all(&destination)?;

        for entry in std::fs::read_dir(&source)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_text = name.to_string_lossy();
            if name_text.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if name_text.starts_with('@') {
                let scope_destination = destination.join(&name);
                std::fs::create_dir_all(&scope_destination)?;
                for package in std::fs::read_dir(&path)? {
                    let package = package?;
                    link_runtime_package(
                        &package.path(),
                        &scope_destination.join(package.file_name()),
                    )?;
                }
            } else {
                link_runtime_package(&path, &destination.join(&name))?;
            }
        }
        Ok(())
    }

    fn link_runtime_package(target: &Path, link: &Path) -> io::Result<()> {
        if !target.join("package.json").is_file() {
            return Ok(());
        }
        symlink_dir(target, link).map_err(|error| {
            io::Error::other(format!("create private module fallback: {error}"))
        })?;
        if std::fs::canonicalize(link)? != std::fs::canonicalize(target)? {
            return Err(io::Error::other(
                "private module fallback does not target the staged runtime",
            ));
        }
        Ok(())
    }

    fn copy_tree(
        root: &Path,
        source: &Path,
        dest: &Path,
        skipped_reparse: Option<&Path>,
    ) -> io::Result<()> {
        std::fs::create_dir_all(dest)
            .map_err(|e| io::Error::other(format!("create staged dir {}: {e}", dest.display())))?;
        let entries = std::fs::read_dir(source).map_err(|e| {
            io::Error::other(format!("read staged source {}: {e}", source.display()))
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| io::Error::other(format!("read_dir entry: {e}")))?;
            let source_path = entry.path();
            let relative = source_path.strip_prefix(root).map_err(|e| {
                io::Error::other(format!("strip_prefix {}: {e}", source_path.display()))
            })?;
            let metadata = std::fs::symlink_metadata(&source_path)
                .map_err(|e| io::Error::other(format!("inspect {}: {e}", source_path.display())))?;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                if skipped_reparse == Some(relative) {
                    continue;
                }
                return Err(io::Error::other(format!(
                    "refusing to stage a reparse point: {}",
                    source_path.display()
                )));
            }
            let dest_path = dest.join(entry.file_name());
            if metadata.is_dir() {
                copy_tree(root, &source_path, &dest_path, skipped_reparse)?;
            } else if metadata.is_file() {
                let mut source_file = std::fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                    .open(&source_path)
                    .map_err(|e| {
                        io::Error::other(format!("open {}: {e}", source_path.display()))
                    })?;
                let opened = source_file.metadata().map_err(|e| {
                    io::Error::other(format!("inspect opened {}: {e}", source_path.display()))
                })?;
                if opened.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    return Err(io::Error::other(format!(
                        "refusing to stage a raced reparse point: {}",
                        source_path.display()
                    )));
                }
                let mut dest_file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&dest_path)
                    .map_err(|e| {
                        io::Error::other(format!("create {}: {e}", dest_path.display()))
                    })?;
                std::io::copy(&mut source_file, &mut dest_file).map_err(|e| {
                    io::Error::other(format!(
                        "copy {} -> {}: {e}",
                        source_path.display(),
                        dest_path.display()
                    ))
                })?;
            } else {
                return Err(io::Error::other(format!(
                    "refusing to stage an unsupported filesystem entry: {}",
                    source_path.display()
                )));
            }
        }
        Ok(())
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
