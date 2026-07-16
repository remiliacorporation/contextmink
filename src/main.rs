mod capture;
mod cli;
mod commands;
mod config;
// Guard enforcement and guard-check share one parser so diagnostics cannot
// disagree with bridge/capture policy.
// #[path]-loaded so child-module resolution matches the contextmink-bridge
// target, which shares this file via #[path] from src/bin/.
#[path = "destructive_guard.rs"]
mod destructive_guard;
mod encoding;
mod files;
mod grep_scan;
mod hook_guard;
mod hook_snippet;
mod json_tools;
mod outline;
mod output;
mod process_boundary;
mod sqlite;
mod text;

use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use clap::Parser;

use capture::command_capture;
use cli::{Cli, Command};
use commands::{
    GrepCaps, command_dirs, command_files, command_grep, command_grep_with_matcher, command_slice,
};
use config::load_context_config;
use config::project_setup::{SetupActionKind, SetupProjectRequest, setup_project};
use destructive_guard::{DenyDecision, ShellDialect, evaluate_argv};
use files::display_path;
use hook_guard::command_hook_guard;
use hook_snippet::command_hook_snippet;
use json_tools::{command_json_find, command_json_select};
use outline::command_outline;
use sqlite::{command_sqlite, command_sqlite_schema};
use text::{TermMode, TextMatcher, collect_terms};

fn main() -> Result<()> {
    output::mark_command_start();
    let cli = Cli::parse();
    if let Command::SetupProject {
        project_root,
        dry_run,
        replace_managed,
    } = &cli.command
    {
        if cli.config.is_some()
            || cli.no_config
            || cli.fail_if_truncated
            || cli.require_complete_scope
        {
            return Err(anyhow!(
                "setup-project accepts only its own flags plus --json; receipt strictness and configuration-selection flags do not apply to installation"
            ));
        }
        let result = setup_project(SetupProjectRequest {
            project_root,
            source_binary: None,
            dry_run: *dry_run,
            replace_managed: *replace_managed,
        })?;
        let mut stdout = io::stdout();
        if cli.json {
            serde_json::to_writer(&mut stdout, &result)?;
            writeln!(stdout)?;
        } else {
            writeln!(
                stdout,
                "[contextmink] setup-project root={} profile={} dry_run={}",
                result.project_root, result.profile, result.dry_run
            )?;
            for action in &result.actions {
                let verb = match action.action {
                    SetupActionKind::Create => "create",
                    SetupActionKind::Replace => "replace",
                    SetupActionKind::Unchanged => "unchanged",
                    SetupActionKind::MakeExecutable => "make_executable",
                    SetupActionKind::UpdateGitignore => "update_gitignore",
                };
                writeln!(stdout, "{verb}\t{}", display_path(&action.path))?;
            }
            if !result.agent_guidance_files_found.is_empty() {
                writeln!(
                    stdout,
                    "agent_guidance_files_found={}",
                    result
                        .agent_guidance_files_found
                        .iter()
                        .map(|path| display_path(path))
                        .collect::<Vec<_>>()
                        .join(",")
                )?;
            }
            writeln!(stdout, "next_actions:")?;
            for action in &result.next_actions {
                writeln!(stdout, "- {action}")?;
            }
        }
        return Ok(());
    }
    let config = match load_context_config(cli.config.as_deref(), cli.no_config) {
        Ok(config) => config,
        Err(error) if matches!(cli.command, Command::HookGuard { .. }) && cli.config.is_some() => {
            eprintln!(
                "contextmink hook-guard: explicitly configured policy could not be loaded: {error:#}"
            );
            std::process::exit(2);
        }
        Err(error) => return Err(error),
    };
    match &cli.command {
        Command::SetupProject { .. } => unreachable!("setup-project returns before config loading"),
        Command::Files {
            paths,
            globs,
            path_terms,
            extensions,
            with_excluded,
            with_git_ignored,
            skip_nested_repos,
            quiet,
            limit,
            max_line_chars,
        } => command_files(
            &cli,
            &config,
            &paths_or_current_dir(paths),
            globs,
            path_terms,
            extensions,
            *with_excluded,
            *with_git_ignored,
            *skip_nested_repos,
            *quiet,
            *limit,
            *max_line_chars,
            (*limit).max(1),
        ),
        Command::Dirs {
            paths,
            depth,
            with_excluded,
            with_git_ignored,
            skip_nested_repos,
            limit,
            max_line_chars,
            max_files_counted,
        } => command_dirs(
            &cli,
            &config,
            &paths_or_current_dir(paths),
            *depth,
            *with_excluded,
            *with_git_ignored,
            *skip_nested_repos,
            *limit,
            *max_line_chars,
            *max_files_counted,
        ),
        Command::Grep {
            args,
            pattern,
            pattern_file,
            literal,
            ignore_case,
            globs,
            extensions,
            with_excluded,
            with_git_ignored,
            skip_nested_repos,
            quiet,
            max_matching_files,
            limit,
            lines_per_file,
            context,
            max_sample_lines,
            max_line_chars,
            max_content_files,
            max_file_bytes,
            max_content_bytes,
        } => command_grep(
            &cli,
            &config,
            args,
            pattern.as_deref(),
            pattern_file.as_deref(),
            *literal,
            *ignore_case,
            globs,
            extensions,
            *with_excluded,
            *with_git_ignored,
            *skip_nested_repos,
            *quiet,
            &GrepCaps {
                max_matching_files: *max_matching_files,
                max_files: *limit,
                lines_per_file: *lines_per_file,
                context: *context,
                max_sample_lines: *max_sample_lines,
                max_line_chars: *max_line_chars,
                max_content_files: *max_content_files,
                max_file_bytes: *max_file_bytes,
                max_content_bytes: *max_content_bytes,
            },
        ),
        Command::GrepTerms {
            terms,
            term_files,
            any,
            ignore_case,
            globs,
            extensions,
            paths,
            with_excluded,
            with_git_ignored,
            skip_nested_repos,
            quiet,
            max_matching_files,
            limit,
            lines_per_file,
            context,
            max_sample_lines,
            max_line_chars,
            max_content_files,
            max_file_bytes,
            max_content_bytes,
        } => {
            let terms = collect_terms(terms, term_files)?;
            let mode = if *any { TermMode::Any } else { TermMode::All };
            command_grep_with_matcher(
                &cli,
                &config,
                "grep-terms",
                TextMatcher::terms(terms, mode, *ignore_case),
                &paths_or_current_dir(paths),
                globs,
                extensions,
                *with_excluded,
                *with_git_ignored,
                *skip_nested_repos,
                *quiet,
                &GrepCaps {
                    max_matching_files: *max_matching_files,
                    max_files: *limit,
                    lines_per_file: *lines_per_file,
                    context: *context,
                    max_sample_lines: *max_sample_lines,
                    max_line_chars: *max_line_chars,
                    max_content_files: *max_content_files,
                    max_file_bytes: *max_file_bytes,
                    max_content_bytes: *max_content_bytes,
                },
            )
        }
        Command::Slice {
            file,
            range,
            start,
            end,
            tail,
            lines,
            max_lines,
            max_line_chars,
            char_start,
            chars,
        } => command_slice(
            &cli,
            &config,
            file,
            range.as_deref(),
            *start,
            *end,
            *tail,
            *lines,
            *max_lines,
            *max_line_chars,
            *char_start,
            *chars,
        ),
        Command::Outline {
            file,
            lang,
            prefix,
            pattern,
            contains,
            ignore_case,
            limit,
            max_line_chars,
        } => command_outline(
            &cli,
            &config,
            file,
            lang.as_deref(),
            prefix.as_deref(),
            pattern.as_deref(),
            contains,
            *ignore_case,
            *limit,
            *max_line_chars,
        ),
        Command::JsonFind {
            file,
            key_contains,
            key_regex,
            path_contains,
            path_regex,
            value_contains,
            limit,
            max_value_chars,
        } => command_json_find(
            &cli,
            &config,
            file,
            key_contains,
            key_regex.as_deref(),
            path_contains,
            path_regex.as_deref(),
            value_contains,
            *limit,
            *max_value_chars,
        ),
        Command::JsonSelect {
            file,
            array,
            fields,
            keys,
            where_exact,
            where_contains,
            limit,
            max_value_chars,
        } => command_json_select(
            &cli,
            &config,
            file,
            array.as_deref(),
            fields,
            where_exact,
            where_contains,
            *keys,
            *limit,
            *max_value_chars,
        ),
        Command::Sqlite {
            path,
            sql,
            sql_file,
            json_params,
            jsonl_params,
            max_param_bytes,
            limit,
            max_rows_scanned,
            timeout_secs,
            max_value_chars,
        } => command_sqlite(
            &cli,
            &config,
            path,
            sql.as_deref(),
            sql_file.as_deref(),
            json_params,
            jsonl_params,
            *max_param_bytes,
            *limit,
            *max_rows_scanned,
            *timeout_secs,
            *max_value_chars,
        ),
        Command::SqliteSchema {
            path,
            tables,
            name_contains,
            include_shadow,
            include_system,
            max_tables,
            max_columns,
            max_indexes,
            max_line_chars,
        } => command_sqlite_schema(
            &cli,
            &config,
            path,
            tables,
            name_contains,
            *include_shadow,
            *include_system,
            *max_tables,
            *max_columns,
            *max_indexes,
            *max_line_chars,
        ),
        Command::Capture {
            max_lines,
            max_bytes,
            max_line_chars,
            script,
            expect_exit,
            receipt_out,
            argv,
        } => command_capture(
            &cli,
            &config,
            *max_lines,
            *max_bytes,
            *max_line_chars,
            *script,
            expect_exit,
            receipt_out.as_ref(),
            argv,
        ),
        Command::HookGuard {
            command_field,
            expected_root,
            shell,
        } => command_hook_guard(
            &config.destructive_guard,
            command_field,
            expected_root.as_deref(),
            *shell,
        ),
        Command::GuardCheck {
            command,
            shell,
            argv,
        } => {
            let (input_kind, evaluated_argv) = match (command.as_deref(), argv.is_empty()) {
                (Some(command), true) => (
                    "shell_command",
                    shell.unwrap_or(ShellDialect::Posix).command_argv(command),
                ),
                (None, false) => ("argv", argv.clone()),
                (Some(_), false) => {
                    return Err(anyhow!(
                        "guard-check accepts either --command or argv, not both"
                    ));
                }
                (None, true) => {
                    return Err(anyhow!("guard-check requires --command or argv"));
                }
            };
            let decision = evaluate_argv(&evaluated_argv, &config.destructive_guard, false);
            let (outcome, message) = match decision {
                DenyDecision::Allow => ("allow", None),
                DenyDecision::AllowWithOverride { message } => {
                    ("allow_with_override", Some(message))
                }
                DenyDecision::Deny { message } => ("deny", Some(message)),
            };
            output::emit_json(serde_json::json!({
                "schema": "contextmink.guard_check.v1",
                "input_kind": input_kind,
                "shell": command.as_ref().map(|_| shell.unwrap_or(ShellDialect::Posix).cli_name()),
                "decision": outcome,
                "message": message,
                "executed": false,
            }))
        }
        Command::HookSnippet {
            binary,
            guard_config,
            matchers,
            command_field,
        } => command_hook_snippet(
            binary.as_deref(),
            guard_config.as_deref(),
            cli.config.as_deref(),
            cli.no_config,
            matchers,
            command_field,
        ),
    }
}

pub(crate) fn paths_or_current_dir(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths = paths.to_vec();
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    paths
}

#[cfg(test)]
mod tests;
