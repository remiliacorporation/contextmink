//! Blocking deny-list for destructive child argv.
//!
//! Shared by every subprocess-spawn path in this crate: the
//! `contextmink-bridge` binary (all four command forms, including `--script`
//! mode) and `contextmink capture`.
//!
//! The built-in rule blocks `git clean` because its flags are easy to
//! misunderstand and it deletes ignored files that the tool cannot enumerate
//! safely first. Repositories can optionally add protected path fragments in
//! `.contextmink.toml`; those fragments remain project-owned config, not
//! release-binary policy.
//!
//! Break-glass: `CONTEXTMINK_BRIDGE_ALLOW_DESTRUCTIVE=1` skips the deny with a
//! loud stderr warning at the call site. It exists for human operators doing
//! deliberate, understood maintenance only; agents must never set it.

use crate::config::DestructiveGuardConfig;
use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ShellDialect {
    Posix,
    Powershell,
    Cmd,
}

impl ShellDialect {
    // This module is also compiled directly into contextmink-bridge, whose
    // argv-only surface uses the parser but not the hook-facing constructors.
    #[allow(dead_code)]
    pub(crate) fn command_argv(self, command: &str) -> Vec<String> {
        match self {
            Self::Posix => vec!["sh".to_owned(), "-c".to_owned(), command.to_owned()],
            Self::Powershell => vec![
                "powershell".to_owned(),
                "-Command".to_owned(),
                command.to_owned(),
            ],
            Self::Cmd => vec!["cmd".to_owned(), "/c".to_owned(), command.to_owned()],
        }
    }

    #[allow(dead_code)]
    pub(crate) fn cli_name(self) -> &'static str {
        match self {
            Self::Posix => "posix",
            Self::Powershell => "powershell",
            Self::Cmd => "cmd",
        }
    }
}

/// Break-glass override — human operators only. `=1` runs a denied command
/// anyway; callers must print a loud stderr warning when it fires.
pub(crate) const ALLOW_DESTRUCTIVE_ENV: &str = "CONTEXTMINK_BRIDGE_ALLOW_DESTRUCTIVE";

/// Program stems whose remaining arguments are opaque script payloads that
/// must be re-scanned word by word (`bash -lc '<script>'` and friends).
const SHELL_STEMS: &[&str] = &[
    "bash",
    "sh",
    "dash",
    "zsh",
    "ksh",
    "powershell",
    "pwsh",
    "cmd",
];

const GIT_CLEAN_MESSAGE: &str = "git clean is blocked by contextmink's built-in \
     destructive-command guard. Its -e flag adds ignore patterns instead of protecting files, \
     and -x/-X delete git-ignored artifacts with no recovery path. Delete explicit paths with \
     `rm -f <path>` instead.";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DenyDecision {
    Allow,
    /// Denied argv riding through on the break-glass env var; the caller must
    /// print the message as a loud stderr warning before spawning.
    AllowWithOverride {
        message: String,
    },
    /// Denied argv; the caller must print the message and must not spawn.
    Deny {
        message: String,
    },
}

/// Combine the pure deny scan with the (caller-sampled) break-glass state.
pub(crate) fn evaluate_argv(
    argv: &[String],
    config: &DestructiveGuardConfig,
    override_active: bool,
) -> DenyDecision {
    match deny_destructive_argv(argv, config) {
        None => DenyDecision::Allow,
        Some(message) if override_active => DenyDecision::AllowWithOverride { message },
        Some(message) => DenyDecision::Deny { message },
    }
}

pub(crate) fn destructive_override_active() -> bool {
    std::env::var_os(ALLOW_DESTRUCTIVE_ENV).is_some_and(|value| value == "1")
}

/// Pure deny scan: `Some(message)` when the argv matches a deny rule.
///
/// Direct argv is already structurally tokenized, so inspect only the command
/// that will execute. Shell payloads are parsed into simple commands while
/// preserving quotes and command boundaries before the same rules are applied.
fn deny_destructive_argv(argv: &[String], config: &DestructiveGuardConfig) -> Option<String> {
    deny_command(argv, config, 0, ShellDialect::Posix)
}

const MAX_NESTED_SHELL_DEPTH: usize = 16;

fn deny_command(
    tokens: &[String],
    config: &DestructiveGuardConfig,
    depth: usize,
    dialect: ShellDialect,
) -> Option<String> {
    if depth > MAX_NESTED_SHELL_DEPTH {
        return Some("destructive-command inspection exceeded the nested shell limit".to_owned());
    }
    let program_index = command_program_index(tokens, dialect)?;
    let stem = stem_lower(&tokens[program_index]);
    let args = &tokens[program_index + 1..];

    if stem == "git"
        && let Some((subcommand_index, subcommand)) = git_subcommand(args)
    {
        if subcommand.eq_ignore_ascii_case("clean") {
            return Some(GIT_CLEAN_MESSAGE.to_owned());
        }
        if subcommand.eq_ignore_ascii_case("rm") {
            let git_rm_args = &args[subcommand_index + 1..];
            let targets = path_operands("git-rm", git_rm_args);
            if git_rm_args
                .iter()
                .any(|token| rm_flag(token, &['r', 'R'], "--recursive"))
                && let Some(fragment) = any_fragment(&targets, &config.recursive_delete_fragments)
            {
                return Some(protected_recursive_delete_message(fragment));
            }
            if let Some(fragment) = any_fragment(&targets, &config.delete_fragments) {
                return Some(protected_delete_message(fragment));
            }
        }
    }

    if SHELL_STEMS.contains(&stem.as_str())
        && let Some(payload) = shell_payload(&stem, args)
        && let Some(message) = deny_shell_payload(&payload, config, depth + 1, shell_dialect(&stem))
    {
        return Some(message);
    }
    if stem == "eval"
        && !args.is_empty()
        && let Some(message) =
            deny_shell_payload(&args.join(" "), config, depth + 1, ShellDialect::Posix)
    {
        return Some(message);
    }

    let recursive = match stem.as_str() {
        "rm" => args
            .iter()
            .any(|token| rm_flag(token, &['r', 'R'], "--recursive")),
        "remove-item" | "ri" => args.iter().any(|token| powershell_recurse_flag(token)),
        // `del`/`erase` are both a cmd builtin (`/s` recurses) and
        // PowerShell aliases of Remove-Item (`-Recurse`).
        "del" | "erase" => args
            .iter()
            .any(|token| powershell_recurse_flag(token) || token.eq_ignore_ascii_case("/s")),
        "rmdir" | "rd" => args.iter().any(|token| token.eq_ignore_ascii_case("/s")),
        _ => false,
    };
    let is_delete = matches!(
        stem.as_str(),
        "rm" | "del" | "erase" | "unlink" | "remove-item" | "ri" | "rmdir" | "rd"
    );
    if !is_delete {
        return None;
    }
    let targets = path_operands(&stem, args);
    if recursive && let Some(fragment) = any_fragment(&targets, &config.recursive_delete_fragments)
    {
        return Some(protected_recursive_delete_message(fragment));
    }
    if let Some(fragment) = any_fragment(&targets, &config.delete_fragments) {
        return Some(protected_delete_message(fragment));
    }
    None
}

fn deny_shell_payload(
    payload: &str,
    config: &DestructiveGuardConfig,
    depth: usize,
    dialect: ShellDialect,
) -> Option<String> {
    let parsed = parse_shell_payload(payload, dialect);
    for command in parsed.commands {
        let expanded_commands = match expand_literal_braces(&command) {
            Ok(expanded_commands) => expanded_commands,
            Err(()) => {
                return Some(
                    "destructive-command inspection exceeded the literal brace expansion limit"
                        .to_owned(),
                );
            }
        };
        for expanded in expanded_commands {
            if let Some(message) = deny_command(&expanded, config, depth, dialect) {
                return Some(message);
            }
        }
    }
    for substitution in parsed.substitutions {
        if let Some(message) = deny_shell_payload(&substitution, config, depth + 1, dialect) {
            return Some(message);
        }
    }
    None
}

fn shell_dialect(stem: &str) -> ShellDialect {
    match stem {
        "powershell" | "pwsh" => ShellDialect::Powershell,
        "cmd" => ShellDialect::Cmd,
        _ => ShellDialect::Posix,
    }
}

fn command_program_index(tokens: &[String], dialect: ShellDialect) -> Option<usize> {
    let mut index = 0usize;
    while index < tokens.len() && shell_assignment(&tokens[index]) {
        index += 1;
    }
    // Every transparent wrapper below advances `index`, so the token count is
    // the natural bound. A fixed wrapper-depth ceiling fails open for a valid
    // command such as `env env ... git clean` once the ceiling is exceeded.
    while index < tokens.len() {
        let stem = stem_lower(tokens.get(index)?);
        match stem.as_str() {
            // Shell control-flow keywords are transparent in command position:
            // `then git clean -fdx` executes git clean, so the keyword must not
            // become the command stem and hide the program behind it.
            "then" | "else" | "elif" | "elseif" | "do" | "!" | "while" | "until" => index += 1,
            "if" => {
                index += 1;
                if dialect == ShellDialect::Cmd {
                    index = skip_cmd_if_condition(tokens, index);
                }
            }
            "call" if dialect == ShellDialect::Cmd => index += 1,
            // POSIX `for VAR in LIST` words are never executed (the body is a
            // separate `do ...` command); cmd `for ... do CMD` keeps the body
            // in the same command, so resume scanning after its `do` token.
            "for" | "select" => {
                match tokens[index..]
                    .iter()
                    .position(|token| token.eq_ignore_ascii_case("do"))
                {
                    Some(offset) => index += offset + 1,
                    None => return None,
                }
            }
            "env" => {
                index += 1;
                while index < tokens.len() {
                    let token = &tokens[index];
                    if token == "--" {
                        index += 1;
                        break;
                    }
                    if shell_assignment(token) {
                        index += 1;
                    } else if matches!(token.as_str(), "-u" | "--unset" | "-C" | "--chdir") {
                        index += 2;
                    } else if token.starts_with('-') {
                        index += 1;
                    } else {
                        break;
                    }
                }
            }
            "command" => {
                index += 1;
                while let Some(token) = tokens.get(index) {
                    if token == "--" {
                        index += 1;
                        break;
                    }
                    if !token.starts_with('-') || token == "-" {
                        break;
                    }
                    if matches!(token.as_str(), "-v" | "-V") {
                        return None;
                    }
                    index += 1;
                }
            }
            "sudo" => {
                index += 1;
                while index < tokens.len() {
                    let token = &tokens[index];
                    if token == "--" {
                        index += 1;
                        break;
                    }
                    if matches!(
                        token.as_str(),
                        "-u" | "--user"
                            | "-g"
                            | "--group"
                            | "-h"
                            | "--host"
                            | "-p"
                            | "--prompt"
                            | "-C"
                            | "--close-from"
                            | "-R"
                            | "--chroot"
                            | "-D"
                            | "--chdir"
                    ) {
                        index += 2;
                    } else if token.starts_with('-') {
                        index += 1;
                    } else {
                        break;
                    }
                }
            }
            "nice" => {
                index += 1;
                if tokens.get(index).is_some_and(|token| token == "-n") {
                    index += 2;
                } else if tokens
                    .get(index)
                    .is_some_and(|token| token.starts_with('-'))
                {
                    index += 1;
                }
            }
            "nohup" => index += 1,
            "exec" => {
                index += 1;
                while index < tokens.len() {
                    let token = &tokens[index];
                    if token == "--" {
                        index += 1;
                        break;
                    }
                    if matches!(token.as_str(), "-a" | "--argv0") {
                        index += 2;
                    } else if matches!(token.as_str(), "-c" | "-l" | "-cl" | "-lc") {
                        index += 1;
                    } else if token.starts_with('-') {
                        return None;
                    } else {
                        break;
                    }
                }
            }
            "time" => {
                index += 1;
                while index < tokens.len() {
                    let token = &tokens[index];
                    if token == "--" {
                        index += 1;
                        break;
                    }
                    if matches!(token.as_str(), "--help" | "--version") {
                        return None;
                    }
                    if matches!(token.as_str(), "-f" | "--format" | "-o" | "--output") {
                        index += 2;
                    } else if matches!(
                        token.as_str(),
                        "-p" | "-a" | "--append" | "-v" | "--verbose" | "-q" | "--quiet"
                    ) || token.starts_with("--format=")
                        || token.starts_with("--output=")
                    {
                        index += 1;
                    } else if token.starts_with('-') {
                        return None;
                    } else {
                        break;
                    }
                }
            }
            "timeout" => {
                index += 1;
                while index < tokens.len() {
                    let token = &tokens[index];
                    if token == "--" {
                        index += 1;
                        break;
                    }
                    if matches!(token.as_str(), "--help" | "--version") {
                        return None;
                    }
                    if matches!(token.as_str(), "-k" | "--kill-after" | "-s" | "--signal") {
                        index += 2;
                    } else if matches!(
                        token.as_str(),
                        "--preserve-status" | "--foreground" | "-v" | "--verbose"
                    ) || token.starts_with("--kill-after=")
                        || token.starts_with("--signal=")
                        || token.starts_with("-k") && token.len() > 2
                        || token.starts_with("-s") && token.len() > 2
                    {
                        index += 1;
                    } else if token.starts_with('-') {
                        return None;
                    } else {
                        break;
                    }
                }
                if index < tokens.len() {
                    index += 1;
                }
            }
            "stdbuf" => {
                index += 1;
                while index < tokens.len() {
                    let token = &tokens[index];
                    if token == "--" {
                        index += 1;
                        break;
                    }
                    if matches!(token.as_str(), "--help" | "--version") {
                        return None;
                    }
                    if matches!(
                        token.as_str(),
                        "-i" | "-o" | "-e" | "--input" | "--output" | "--error"
                    ) {
                        index += 2;
                    } else if token.starts_with("-i")
                        || token.starts_with("-o")
                        || token.starts_with("-e")
                        || token.starts_with("--input=")
                        || token.starts_with("--output=")
                        || token.starts_with("--error=")
                    {
                        index += 1;
                    } else if token.starts_with('-') {
                        return None;
                    } else {
                        break;
                    }
                }
            }
            "setsid" => {
                index += 1;
                while let Some(token) = tokens.get(index) {
                    if token == "--" {
                        index += 1;
                        break;
                    }
                    if matches!(token.as_str(), "--help" | "--version") {
                        return None;
                    }
                    if matches!(
                        token.as_str(),
                        "-c" | "--ctty" | "-f" | "--fork" | "-w" | "--wait"
                    ) {
                        index += 1;
                    } else if token.starts_with('-') {
                        return None;
                    } else {
                        break;
                    }
                }
            }
            "chronic" => index += 1,
            "doas" => {
                index += 1;
                while index < tokens.len() {
                    let token = &tokens[index];
                    if token == "--" {
                        index += 1;
                        break;
                    }
                    if matches!(token.as_str(), "-a" | "-C" | "-u") {
                        index += 2;
                    } else if matches!(token.as_str(), "-L" | "-n" | "-s") {
                        index += 1;
                    } else if token.starts_with('-') {
                        return None;
                    } else {
                        break;
                    }
                }
            }
            "xargs" => {
                index += 1;
                while index < tokens.len() {
                    let token = &tokens[index];
                    if token == "--" {
                        index += 1;
                        break;
                    }
                    if matches!(
                        token.as_str(),
                        "-a" | "--arg-file"
                            | "-d"
                            | "--delimiter"
                            | "-E"
                            | "-I"
                            | "-L"
                            | "-n"
                            | "-P"
                            | "-s"
                    ) {
                        index += 2;
                    } else if token.starts_with('-') {
                        index += 1;
                    } else {
                        break;
                    }
                }
            }
            _ => return Some(index),
        }
    }
    None
}

/// Skip a cmd `if` condition so the guarded command becomes the scanned stem:
/// `if [/i] [not] (exist|defined|errorlevel|cmdextversion) X CMD` and the
/// three-token (`%a% == %b%`) or fused (`"%a%"=="%b%"`) comparison forms.
fn skip_cmd_if_condition(tokens: &[String], mut index: usize) -> usize {
    while tokens
        .get(index)
        .is_some_and(|token| token.eq_ignore_ascii_case("/i") || token.eq_ignore_ascii_case("not"))
    {
        index += 1;
    }
    let Some(token) = tokens.get(index) else {
        return index;
    };
    let unary = ["exist", "defined", "errorlevel", "cmdextversion"];
    let comparators = ["==", "equ", "neq", "lss", "leq", "gtr", "geq"];
    if unary.iter().any(|kind| token.eq_ignore_ascii_case(kind)) {
        index + 2
    } else if tokens.get(index + 1).is_some_and(|operator| {
        comparators
            .iter()
            .any(|kind| operator.eq_ignore_ascii_case(kind))
    }) {
        index + 3
    } else if token.contains("==") {
        index + 1
    } else {
        index
    }
}

fn shell_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name.chars().enumerate().all(|(index, ch)| {
            ch == '_' || ch.is_ascii_alphanumeric() && (index > 0 || !ch.is_ascii_digit())
        })
}

fn git_subcommand(args: &[String]) -> Option<(usize, &str)> {
    let mut index = 0usize;
    while index < args.len() {
        let token = &args[index];
        if token == "--" {
            index += 1;
            break;
        }
        if !token.starts_with('-') || token == "-" {
            break;
        }
        if git_global_option_takes_value(token) {
            index += 2;
        } else {
            index += 1;
        }
    }
    args.get(index)
        .map(|subcommand| (index, subcommand.as_str()))
}

fn git_global_option_takes_value(token: &str) -> bool {
    matches!(
        token,
        "-C" | "-c"
            | "--exec-path"
            | "--git-dir"
            | "--work-tree"
            | "--namespace"
            | "--super-prefix"
            | "--config-env"
    )
}

fn shell_payload(stem: &str, args: &[String]) -> Option<String> {
    match stem {
        "bash" | "sh" | "dash" | "zsh" | "ksh" => {
            args.iter().enumerate().find_map(|(index, token)| {
                (token.starts_with('-') && !token.starts_with("--") && token[1..].contains('c'))
                    .then(|| args.get(index + 1).cloned())
                    .flatten()
            })
        }
        "powershell" | "pwsh" => args.iter().enumerate().find_map(|(index, token)| {
            let option = token.strip_prefix('-')?.to_ascii_lowercase();
            (!option.is_empty() && "command".starts_with(&option))
                .then(|| args.get(index + 1..).map(|tail| tail.join(" ")))
                .flatten()
        }),
        "cmd" => args.iter().enumerate().find_map(|(index, token)| {
            token
                .eq_ignore_ascii_case("/c")
                .then(|| args.get(index + 1..).map(|tail| tail.join(" ")))
                .flatten()
        }),
        _ => None,
    }
}

fn path_operands<'a>(stem: &str, args: &'a [String]) -> Vec<&'a str> {
    let mut targets = Vec::new();
    let mut options_done = false;
    let mut skip_value = false;
    for token in args {
        if skip_value {
            skip_value = false;
            continue;
        }
        if !options_done && token == "--" {
            options_done = true;
            continue;
        }
        if !options_done && token.starts_with('-') {
            if matches!(stem, "remove-item" | "ri" | "del" | "erase")
                && let Some(path) = powershell_attached_path_operand(token)
            {
                targets.push(path);
                continue;
            }
            if matches!(
                token.to_ascii_lowercase().as_str(),
                "-exclude" | "-filter" | "-include" | "--pathspec-from-file"
            ) {
                skip_value = true;
            }
            continue;
        }
        if !options_done
            && matches!(stem, "del" | "erase" | "rmdir" | "rd")
            && token.starts_with('/')
        {
            continue;
        }
        targets.push(token.as_str());
    }
    targets
}

fn powershell_attached_path_operand(token: &str) -> Option<&str> {
    let (parameter, value) = token.split_once(':')?;
    let parameter = parameter.strip_prefix('-')?;
    if value.is_empty()
        || !matches!(
            parameter.to_ascii_lowercase().as_str(),
            "path" | "literalpath"
        )
    {
        return None;
    }
    Some(value)
}

fn protected_recursive_delete_message(fragment: &str) -> String {
    format!(
        "recursive deletion references configured protected path fragment {fragment:?}; remove or \
         change the fragment in .contextmink.toml only for deliberate human maintenance"
    )
}

fn protected_delete_message(fragment: &str) -> String {
    format!(
        "deletion references configured protected path fragment {fragment:?}; remove or change \
         the fragment in .contextmink.toml only for deliberate human maintenance"
    )
}

/// Lowercased program stem: `git`, `git.exe`, `/usr/bin/git`, and
/// `C:\...\git.EXE` all reduce to `git` on every host OS.
fn stem_lower(token: &str) -> String {
    // cmd.exe accepts `@` as an echo-control prefix on executable commands,
    // including after control-flow keywords and `call`.
    let leaf = token
        .trim_start_matches('@')
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(token);
    let stem = leaf.rsplit_once('.').map_or(leaf, |(stem, _)| stem);
    stem.to_ascii_lowercase()
}

fn any_fragment<'a>(tokens: &[&str], fragments: &'a [String]) -> Option<&'a str> {
    tokens.iter().find_map(|token| {
        let lower = token.to_ascii_lowercase();
        fragments.iter().find_map(|fragment| {
            let fragment = fragment.trim();
            (!fragment.is_empty() && lower.contains(&fragment.to_ascii_lowercase()))
                .then_some(fragment)
        })
    })
}

/// `-rf`, `-fr`, `-r`, `-Rf`, or the long spelling: short-flag clusters carry
/// the letter anywhere in the cluster.
fn rm_flag(token: &str, letters: &[char], long: &str) -> bool {
    if token == long {
        return true;
    }
    token.len() > 1
        && token.starts_with('-')
        && !token.starts_with("--")
        && token[1..].chars().any(|ch| letters.contains(&ch))
}

/// PowerShell accepts any unambiguous parameter prefix, so `-r`, `-rec`, and
/// `-Recurse` all mean `-Recurse` on Remove-Item.
fn powershell_recurse_flag(token: &str) -> bool {
    let Some(rest) = token.strip_prefix('-') else {
        return false;
    };
    let (name, value) = rest
        .split_once(':')
        .map_or((rest, None), |(name, value)| (name, Some(value)));
    let enabled = value.is_none_or(|value| {
        !matches!(
            value.to_ascii_lowercase().as_str(),
            "false" | "$false" | "0"
        )
    });
    enabled && !name.is_empty() && "recurse".starts_with(&name.to_ascii_lowercase())
}

// Explicit paths: this module is #[path]-included by both the contextmink
// and contextmink-bridge targets, which makes it mod-rs for child
// resolution — a bare `mod shell_parse;` would look for src/shell_parse.rs.
#[path = "destructive_guard/shell_parse.rs"]
mod shell_parse;
use shell_parse::{expand_literal_braces, parse_shell_payload};

#[cfg(test)]
#[path = "destructive_guard/tests.rs"]
mod tests;
