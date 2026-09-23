//! Inbound argv integrity under Git Bash / MSYS2.
//!
//! When an MSYS shell launches a native Windows executable it rewrites every
//! argument (and every `--flag=value` value) that looks like a POSIX path into
//! a Windows path under the MSYS installation root. A JSON Pointer such as
//! `/report/plan_id`, a grep pattern such as `/skills/contextmink`, or a path
//! fragment such as `/contextmink/` therefore reaches Contextmink as
//! `C:/Program Files/Git/report/plan_id`, and a search then reports a
//! complete-scope no-match for a value the caller never asked about. This
//! module detects that rewrite before any work starts so the evidence is
//! refused instead of silently wrong.
//!
//! `MSYSTEM` and the MSYS PATH entries are inherited by native shells started
//! from Git Bash (PowerShell, cmd, CI steps), where no rewriting happens. The
//! refusal therefore also requires the immediate parent process image to live
//! in the MSYS root's `usr/bin` or `bin`; when the parent cannot be determined,
//! nothing is refused.
//!
//! The native `contextmink-bridge` does not apply this check: it exists to be
//! called from native (non-MSYS) hosts, and in `--script` mode it forwards
//! values to Bash, where an MSYS-rewritten path still names the same file.

use std::ffi::{OsStr, OsString};

/// Environment inputs that decide whether MSYS may have rewritten argv.
/// Injected so the refusal is testable without a Git installation.
#[derive(Debug, Default, Clone)]
pub(crate) struct MsysEnvironment {
    pub(crate) msystem: Option<OsString>,
    pub(crate) no_pathconv: Option<OsString>,
    pub(crate) arg_conv_excl: Option<OsString>,
    pub(crate) path: Option<OsString>,
    /// Image path of the process that launched this one, when observable.
    pub(crate) parent_image: Option<String>,
}

impl MsysEnvironment {
    pub(crate) fn from_process() -> Self {
        Self {
            msystem: std::env::var_os("MSYSTEM"),
            no_pathconv: std::env::var_os("MSYS_NO_PATHCONV"),
            arg_conv_excl: std::env::var_os("MSYS2_ARG_CONV_EXCL"),
            path: std::env::var_os("PATH"),
            parent_image: parent_image(),
        }
    }

    /// Only an MSYS program rewrites argv; a native parent that merely
    /// inherited the MSYS environment does not.
    fn parent_under_root(&self, root: &str) -> bool {
        let Some(parent) = self.parent_image.as_deref() else {
            return false;
        };
        let parent = parent.replace('\\', "/").to_ascii_lowercase();
        let root = root.to_ascii_lowercase();
        ["usr/bin/", "bin/"].iter().any(|directory| {
            parent
                .strip_prefix(&format!("{root}{directory}"))
                .is_some_and(|name| !name.is_empty() && !name.contains('/'))
        })
    }

    fn conversion_possible(&self) -> bool {
        let set =
            |value: &Option<OsString>| value.as_deref().is_some_and(|value| !value.is_empty());
        set(&self.msystem)
            && !set(&self.no_pathconv)
            && self.arg_conv_excl.as_deref() != Some(OsStr::new("*"))
    }

    /// MSYS roots in forward-slash form with a trailing `/`, derived from the
    /// `<root>\usr\bin` entries MSYS places on a native child's PATH.
    fn roots(&self) -> Vec<String> {
        let Some(path) = self.path.as_deref() else {
            return Vec::new();
        };
        let mut roots = Vec::new();
        for entry in path.to_string_lossy().split(';') {
            let normalized = entry.trim().replace('\\', "/");
            let normalized = normalized.trim_end_matches('/');
            let Some(root_len) = normalized
                .len()
                .checked_sub("usr/bin".len())
                .filter(|_| normalized.to_ascii_lowercase().ends_with("/usr/bin"))
            else {
                continue;
            };
            let root = &normalized[..root_len];
            // Only a drive-qualified root is a rewrite target; a bare `/`
            // would claim every absolute argument.
            let bytes = root.as_bytes();
            if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && &bytes[1..3] == b":/" {
                let root = root.to_owned();
                if !roots
                    .iter()
                    .any(|known: &String| known.eq_ignore_ascii_case(&root))
                {
                    roots.push(root);
                }
            }
        }
        roots
    }
}

/// Return the refusal message when any argument after argv[0] begins with the
/// MSYS installation root while MSYS path conversion was active.
pub(crate) fn rewritten_argument_refusal(
    args: &[OsString],
    environment: &MsysEnvironment,
) -> Option<String> {
    if !environment.conversion_possible() {
        return None;
    }
    let roots = environment
        .roots()
        .into_iter()
        .filter(|root| environment.parent_under_root(root))
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return None;
    }
    for arg in args.iter().skip(1) {
        let arg = arg.to_string_lossy();
        let value = if arg.starts_with('-') {
            arg.split_once('=').map_or(arg.as_ref(), |(_, value)| value)
        } else {
            arg.as_ref()
        };
        for root in &roots {
            if value.len() >= root.len()
                && value.is_char_boundary(root.len())
                && value[..root.len()].eq_ignore_ascii_case(root)
            {
                let original = &value[root.len() - 1..];
                return Some(format!(
                    "argument `{arg}` starts with the MSYS installation root `{root}`: Git Bash rewrote a leading-`/` value (for example `{original}`) into a Windows path before contextmink started, so results would describe a different value. Rerun with the command prefixed by `MSYS_NO_PATHCONV=1` (for example `MSYS_NO_PATHCONV=1 contextmink ...`)"
                ));
            }
        }
    }
    None
}

/// Image of the immediate parent process. A parent PID that was reused by a
/// process started after this one is not the parent and yields `None`.
#[cfg(windows)]
fn parent_image() -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };

    fn created(handle: windows_sys::Win32::Foundation::HANDLE) -> Option<u64> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: valid process handle and writable out-parameters.
        let ok =
            unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
        (ok != 0)
            .then(|| (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
    }

    let own_pid = std::process::id();
    // SAFETY: snapshot handle is checked and closed on every path below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut parent_pid = None;
    // SAFETY: entry.dwSize is initialized as the API requires.
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while more {
        if entry.th32ProcessID == own_pid {
            parent_pid = Some(entry.th32ParentProcessID);
            break;
        }
        // SAFETY: as above.
        more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    // SAFETY: snapshot is a valid handle owned here.
    unsafe { CloseHandle(snapshot) };
    let parent_pid = parent_pid?;
    // SAFETY: OpenProcess result is checked and closed below.
    let parent = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, parent_pid) };
    if parent.is_null() {
        return None;
    }
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close.
    let own_created = created(unsafe { GetCurrentProcess() });
    let parent_created = created(parent);
    let mut buffer = vec![0u16; 32768];
    let mut size = buffer.len() as u32;
    // SAFETY: parent is a valid handle; buffer and size describe writable storage.
    let queried = unsafe { QueryFullProcessImageNameW(parent, 0, buffer.as_mut_ptr(), &mut size) };
    // SAFETY: parent is a valid handle owned here.
    unsafe { CloseHandle(parent) };
    match (own_created, parent_created) {
        (Some(own), Some(parent)) if parent <= own && queried != 0 => {
            // Root comparison is ASCII-prefix based; lossy decoding cannot
            // make a non-MSYS image look like one under the detected root.
            Some(String::from_utf16_lossy(&buffer[..size as usize]))
        }
        _ => None,
    }
}

#[cfg(not(windows))]
fn parent_image() -> Option<String> {
    // MSYS argument conversion only affects native Windows executables.
    None
}

#[cfg(test)]
mod tests;
