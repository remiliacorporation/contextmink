use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::config::find_config_path;
use crate::destructive_guard::ShellDialect;
use crate::output::emit_json;

const DEFAULT_COMMAND_FIELD: &str = "tool_input.command";
const DEFAULT_MATCHERS: &[&str] = &["Bash", "PowerShell"];

pub(crate) fn command_hook_snippet(
    binary: Option<&Path>,
    guard_config: Option<&Path>,
    cli_config: Option<&Path>,
    no_config: bool,
    matchers: &[String],
    command_field: &str,
) -> Result<()> {
    let binary = resolve_binary(binary)?;
    let guard_config = resolve_guard_config(guard_config, cli_config, no_config)?;
    let expected_root = match guard_config.as_deref().and_then(Path::parent) {
        Some(root) => root.to_path_buf(),
        None => std::env::current_dir().context("failed to resolve hook policy root")?,
    };
    let selected_matchers = selected_matchers(matchers);
    emit_json(claude_hook_settings(
        &binary,
        guard_config.as_deref(),
        &expected_root,
        no_config && guard_config.is_none(),
        &selected_matchers,
        command_field,
    ))
}

fn resolve_binary(binary: Option<&Path>) -> Result<PathBuf> {
    match binary {
        Some(path) => absolutize(path),
        None => std::env::current_exe().context("failed to resolve current contextmink executable"),
    }
}

fn resolve_guard_config(
    guard_config: Option<&Path>,
    cli_config: Option<&Path>,
    no_config: bool,
) -> Result<Option<PathBuf>> {
    if let Some(path) = guard_config {
        return absolutize(path).map(Some);
    }
    if let Some(path) = cli_config {
        return absolutize(path).map(Some);
    }
    if no_config {
        return Ok(None);
    }
    find_config_path().as_deref().map(absolutize).transpose()
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .context("failed to resolve current directory")?
            .join(path))
    }
}

fn selected_matchers(matchers: &[String]) -> Vec<String> {
    if matchers.is_empty() {
        DEFAULT_MATCHERS
            .iter()
            .map(|matcher| (*matcher).to_owned())
            .collect()
    } else {
        matchers.to_owned()
    }
}

pub(crate) fn claude_hook_settings(
    binary: &Path,
    guard_config: Option<&Path>,
    expected_root: &Path,
    no_config: bool,
    matchers: &[String],
    command_field: &str,
) -> Value {
    let hooks = matchers
        .iter()
        .map(|matcher| {
            let shell = if matcher.eq_ignore_ascii_case("PowerShell") {
                ShellDialect::Powershell
            } else {
                ShellDialect::Posix
            };
            json!({
                "matcher": matcher,
                "hooks": [
                    {
                        "type": "command",
                        "command": hook_command(
                            shell,
                            binary,
                            guard_config,
                            expected_root,
                            no_config,
                            command_field,
                        )
                    }
                ]
            })
        })
        .collect::<Vec<_>>();
    json!({ "hooks": { "PreToolUse": hooks } })
}

fn hook_command(
    shell: ShellDialect,
    binary: &Path,
    guard_config: Option<&Path>,
    expected_root: &Path,
    no_config: bool,
    command_field: &str,
) -> String {
    let binary = hook_path(binary);
    let mut args = vec!["hook-guard".to_owned()];
    if let Some(config) = guard_config {
        args.push("--config".to_owned());
        args.push(hook_path(config));
    } else if no_config {
        args.push("--no-config".to_owned());
    }
    args.push("--expected-root".to_owned());
    args.push(hook_path(expected_root));
    args.push("--shell".to_owned());
    args.push(shell.cli_name().to_owned());
    if command_field != DEFAULT_COMMAND_FIELD {
        args.push("--command-field".to_owned());
        args.push(command_field.to_owned());
    }
    // Claude executes every command hook through its POSIX hook runner. `shell`
    // describes the intercepted command dialect passed to hook-guard; it never
    // changes the syntax used to launch the hook itself.
    std::iter::once(shell_word(&binary, quote_bash))
        .chain(args.iter().map(|arg| shell_word(arg, quote_bash)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn hook_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized
        .strip_prefix("//?/")
        .unwrap_or(&normalized)
        .to_owned()
}

fn shell_word(value: &str, quote: fn(&str) -> String) -> String {
    if value.chars().all(|ch| {
        ch.is_ascii_alphanumeric()
            || matches!(
                ch,
                '@' | '%' | '_' | '+' | '=' | ':' | ',' | '.' | '/' | '-'
            )
    }) {
        value.to_owned()
    } else {
        quote(value)
    }
}

fn quote_bash(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
#[path = "hook_snippet/tests.rs"]
mod tests;
