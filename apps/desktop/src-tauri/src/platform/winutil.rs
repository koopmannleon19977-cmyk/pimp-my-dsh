//! Shared Windows FFI helpers. Windows-only; kept here so the reviewed
//! `unsafe` Win32 calls live in one auditable module rather than scattered
//! across the supervisor. Every call is wrapped in an explicit `unsafe` block
//! (the crate denies `unsafe_op_in_unsafe_fn`).

#![cfg(windows)]

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::Pipes::CreatePipe;

/// Encode an `OsStr` to a NUL-terminated UTF-16 buffer.
pub fn to_wide_null(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Quote a single Windows command-line argument (empty → `""`, embedded
/// double-quotes doubled, trailing backslashes escaped before a closing quote).
pub fn quote_cmd_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }
    let needs_quotes = arg
        .chars()
        .any(|c| c == ' ' || c == '\t' || c == '"' || c == '\n' || c == '\r');
    if !needs_quotes {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// Build a double-NUL-terminated UTF-16 environment block from `name`/`value`
/// pairs. Windows environment names are case-insensitive: duplicates are
/// deduplicated keeping the last occurrence (its original case).
pub fn build_env_block(env: &[(std::ffi::OsString, std::ffi::OsString)]) -> Vec<u16> {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut entries: Vec<&(std::ffi::OsString, std::ffi::OsString)> = Vec::new();
    for pair in env.iter().rev() {
        let key = pair.0.to_string_lossy().to_uppercase();
        if seen.insert(key) {
            entries.push(pair);
        }
    }
    entries.reverse();

    let mut block: Vec<u16> = Vec::new();
    // Even an empty Unicode environment block is terminated by two NUL code
    // units. A single NUL makes CreateProcessW fail with ERROR_INVALID_PARAMETER.
    if entries.is_empty() {
        return vec![0, 0];
    }

    for (k, v) in entries {
        block.extend(k.encode_wide());
        block.push(b'=' as u16);
        block.extend(v.encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

/// Create an anonymous pipe pair. Both handles are created inheritable so the
/// explicit `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` may select the child ends.
pub fn create_pipe() -> io::Result<(HANDLE, HANDLE)> {
    let mut sa: SECURITY_ATTRIBUTES = unsafe { std::mem::zeroed() };
    sa.nLength = std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32;
    sa.bInheritHandle = 1;
    sa.lpSecurityDescriptor = std::ptr::null_mut();

    let mut read: HANDLE = std::ptr::null_mut();
    let mut write: HANDLE = std::ptr::null_mut();
    // SAFETY: read/write are valid out-params; sa is a valid non-null pointer.
    let ok = unsafe { CreatePipe(&mut read, &mut write, &sa, 0) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((read, write))
}
