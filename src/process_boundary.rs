//! Shared process-boundary policy for the native bridge and `capture`.
//!
//! Script classification happens before spawn. A failed native spawn is not a
//! script detector: direct mode only enters Git Bash for a real
//! shebang file, while `--script` is the explicit Bash-script contract.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

const MSYS2_ARG_CONV_EXCL_ENV: &str = "MSYS2_ARG_CONV_EXCL";

// Raw argv must not appear on the Windows command line used to start Git Bash.
// MSYS expands `@file` response arguments before Bash starts, which corrupts
// structured argv intended for the child. Decode hex only after Bash is live.
const BASH_ARGV_RELAY: &str = r#"set -euo pipefail
decode_hex() {
    local hex=$1 out= byte
    while [[ -n $hex ]]; do
        printf -v byte '%b' "\\x${hex:0:2}"
        out+=$byte
        hex=${hex:2}
    done
    REPLY=$out
}
decode_hex "$1"
program=$REPLY
shift
args=()
for encoded in "$@"; do
    decode_hex "$encoded"
    args+=("$REPLY")
done
exec "$program" "${args[@]}""#;

pub(crate) struct PreparedCommand {
    pub(crate) command: Command,
    /// Human-readable logical argv after interpreter selection. The encoded
    /// relay payload is intentionally not exposed as the effective command.
    pub(crate) effective_argv: Vec<String>,
    pub(crate) execution_mode: &'static str,
}

pub(crate) fn prepare_command(
    program: &str,
    args: &[String],
    target_cwd: &Path,
    explicit_script: bool,
    login: bool,
) -> Result<PreparedCommand, String> {
    let resolved = if explicit_script {
        let path = Path::new(program);
        if path.is_absolute() || path.has_root() {
            program.to_owned()
        } else {
            target_cwd.join(path).to_string_lossy().replace('\\', "/")
        }
    } else {
        resolve_program(program, target_cwd)
    };
    let resolved_path = Path::new(&resolved);
    if explicit_script && !resolved_path.is_file() {
        return Err(format!("script not found: {}", resolved_path.display()));
    }

    let shebang = if resolved_path.is_file() {
        file_has_shebang(resolved_path)?
    } else {
        false
    };
    let bash_boundary = explicit_script || login || (cfg!(windows) && shebang);

    let mut logical_argv = Vec::with_capacity(args.len() + 1);
    logical_argv.push(resolved.clone());
    logical_argv.extend(args.iter().cloned());

    if bash_boundary {
        let bash = locate_bash()?;
        let mut command = Command::new(&bash);
        if cfg!(windows) {
            let exclusions = msys2_arg_conversion_exclusions(&logical_argv);
            if exclusions.is_empty() {
                command.env_remove(MSYS2_ARG_CONV_EXCL_ENV);
            } else {
                command.env(MSYS2_ARG_CONV_EXCL_ENV, exclusions);
            }
        }
        if login {
            command.arg("--login");
        }
        append_bash_argv_relay(&mut command, &logical_argv);

        let mut effective_argv = Vec::with_capacity(logical_argv.len() + 1);
        effective_argv.push(bash.to_string_lossy().into_owned());
        effective_argv.extend(logical_argv);
        let execution_mode = if explicit_script {
            "bash_script"
        } else if login {
            "bash_login"
        } else {
            "shebang_script"
        };
        return Ok(PreparedCommand {
            command,
            effective_argv,
            execution_mode,
        });
    }

    let mut command = Command::new(&resolved);
    command.args(args);
    Ok(PreparedCommand {
        command,
        effective_argv: logical_argv,
        execution_mode: if shebang { "native_shebang" } else { "native" },
    })
}

fn file_has_shebang(path: &Path) -> Result<bool, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
    let mut prefix = [0_u8; 2];
    let bytes = file
        .read(&mut prefix)
        .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
    Ok(bytes == prefix.len() && prefix == *b"#!")
}

fn append_bash_argv_relay(command: &mut Command, argv: &[String]) {
    command
        .arg("-c")
        .arg(BASH_ARGV_RELAY)
        .arg("contextmink-process-boundary");
    command.args(argv.iter().map(|arg| hex_encode_arg(arg)));
}

fn hex_encode_arg(arg: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(arg.len() * 2);
    for byte in arg.as_bytes() {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

pub(crate) fn msys2_arg_conversion_exclusions(argv: &[String]) -> String {
    let mut exclusions = std::collections::BTreeSet::new();
    for arg in argv {
        if !arg.contains(['/', '\\']) {
            continue;
        }
        let prefix = arg.split(';').next().unwrap_or_default();
        if !prefix.is_empty() && prefix != "*" {
            exclusions.insert(prefix);
        }
    }
    exclusions.into_iter().collect::<Vec<_>>().join(";")
}

/// A program spelled as a path resolves against the child's working directory,
/// matching POSIX exec semantics. Bare names retain PATH lookup.
pub(crate) fn resolve_program(program: &str, target_cwd: &Path) -> String {
    let path = Path::new(program);
    let is_pathlike = program.chars().any(std::path::is_separator);
    if !is_pathlike || path.is_absolute() || path.has_root() {
        return program.to_owned();
    }
    let mut resolved = target_cwd.to_path_buf();
    for component in path.components() {
        if component != std::path::Component::CurDir {
            resolved.push(component.as_os_str());
        }
    }
    resolved.to_string_lossy().replace('\\', "/")
}

pub(crate) fn find_ancestor_file(start: &Path, name: &str) -> Option<PathBuf> {
    let mut cursor = Some(start);
    while let Some(dir) = cursor {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        cursor = dir.parent();
    }
    None
}

#[allow(dead_code)] // used by the separately compiled native bridge target
fn find_ancestor_entry(start: &Path, name: &str) -> Option<PathBuf> {
    let mut cursor = Some(start);
    while let Some(dir) = cursor {
        if dir.join(name).exists() {
            return Some(dir.to_path_buf());
        }
        cursor = dir.parent();
    }
    None
}

/// Resolve the project served by a bridge installed either inside that project
/// or globally. Caller location wins, then executable location; policy roots
/// win over repository markers in both cases.
#[allow(dead_code)] // used by the separately compiled native bridge target
pub(crate) fn resolve_project_root(exe_dir: &Path, cwd: &Path) -> PathBuf {
    if let Some(root) = std::env::var_os("CONTEXTMINK_BRIDGE_ROOT") {
        return PathBuf::from(root);
    }
    find_ancestor_file(cwd, ".contextmink.toml")
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .or_else(|| {
            find_ancestor_file(exe_dir, ".contextmink.toml")
                .and_then(|path| path.parent().map(Path::to_path_buf))
        })
        .or_else(|| find_ancestor_entry(cwd, ".git"))
        .or_else(|| find_ancestor_entry(exe_dir, ".git"))
        .unwrap_or_else(|| cwd.to_path_buf())
}

pub(crate) fn locate_bash() -> Result<PathBuf, String> {
    if let Some(bash) = std::env::var_os("CONTEXTMINK_BASH") {
        let bash = PathBuf::from(bash);
        if bash.is_file() || !cfg!(windows) {
            return Ok(bash);
        }
        return Err(format!(
            "CONTEXTMINK_BASH does not name a file: {}",
            bash.display()
        ));
    }
    if cfg!(windows) {
        windows_bash_candidates()
            .into_iter()
            .find(|candidate| candidate.is_file())
            .ok_or_else(|| {
                "unable to locate Git Bash (set CONTEXTMINK_BASH to an explicit Bash executable)"
                    .to_string()
            })
    } else {
        Ok(PathBuf::from("bash"))
    }
}

pub(crate) fn windows_bash_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(program_files).join(r"Git\bin\bash.exe"));
    }
    candidates.push(PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"));
    candidates.push(PathBuf::from(r"C:\Program Files (x86)\Git\bin\bash.exe"));
    candidates
}

#[cfg(test)]
#[path = "process_boundary/tests.rs"]
mod tests;
