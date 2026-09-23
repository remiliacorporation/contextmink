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
}

impl MsysEnvironment {
    pub(crate) fn from_process() -> Self {
        Self {
            msystem: std::env::var_os("MSYSTEM"),
            no_pathconv: std::env::var_os("MSYS_NO_PATHCONV"),
            arg_conv_excl: std::env::var_os("MSYS2_ARG_CONV_EXCL"),
            path: std::env::var_os("PATH"),
        }
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
    let roots = environment.roots();
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
                    "argument `{arg}` starts with the MSYS installation root `{root}`: Git Bash rewrote a leading-`/` value (for example `{original}`) into a Windows path before contextmink started, so results would describe a different value. Rerun with the command prefixed by `MSYS_NO_PATHCONV=1` (for example `MSYS_NO_PATHCONV=1 contextmink ...`), or run it from PowerShell"
                ));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
