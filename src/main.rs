mod capture;
mod cli;
mod config;
mod file_commands;
// Guard enforcement and guard-check share one parser so diagnostics cannot
// disagree with bridge/capture policy.
// #[path]-loaded so child-module resolution matches the contextmink-bridge
// target, which shares this file via #[path] from src/bin/.
#[path = "destructive_guard.rs"]
mod destructive_guard;
mod encoding;
mod files;
mod grep_scan;
mod guard_hook;
mod guard_hook_snippet;
mod json_commands;
mod json_input;
mod msys_arguments;
mod outline;
mod output;
mod process_boundary;
mod process_identity;
mod process_supervision;
mod sqlite;
mod text;
mod user_installation;
mod user_setup;

use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use capture::command_capture;
use cli::{Cli, Command, parse_cli};
use config::load_context_config;
use config::project_setup::{
    SetupActionKind, SetupProjectRequest, UninstallProjectRequest, setup_project, uninstall_project,
};
use destructive_guard::{DenyDecision, ShellDialect, evaluate_argv};
use file_commands::{
    GrepCaps, command_dirs, command_files, command_grep, command_grep_with_matcher, command_slice,
};
use files::display_path;
use guard_hook::command_guard_hook;
use guard_hook_snippet::command_guard_hook_snippet;
use json_commands::{command_json_find, command_json_select};
use outline::command_outline;
use sqlite::{command_sqlite, command_sqlite_schema};
use text::{TermMode, TextMatcher, collect_terms};

/// Exit code that hook protocols treat as "block this tool call"; any other
/// nonzero exit is a non-blocking hook error that lets the tool call run.
const GUARD_HOOK_EXIT_BLOCK: i32 = 2;

fn main() -> Result<()> {
    use std::io::Write as _;

    // Decide hook mode from raw argv before any other work, so every later
    // failure (the personal-install self-check, argv rewriting, policy
    // loading, payload reading, or a panic) blocks instead of failing open.
    let guard_hook =
        cli::selected_subcommand(&std::env::args_os().collect::<Vec<_>>()) == Some("guard-hook");
    if guard_hook {
        std::panic::set_hook(Box::new(|info| {
            let message = format!(
                "BLOCKED by contextmink guard-hook: the guard panicked before reaching a decision ({info}); this is a Contextmink defect: report it, then install a fixed release with setup-user or setup-project"
            );
            let _ = writeln!(std::io::stderr(), "{message}"); // guardrail: allow-ignore-result exit 2 must not depend on stderr
            std::process::exit(GUARD_HOOK_EXIT_BLOCK);
        }));
    }
    match run_on_application_thread() {
        Err(error) if guard_hook => {
            let message = format!(
                "BLOCKED by contextmink guard-hook: the guard cannot evaluate commands, so it blocks every tool call until this is fixed: {error:#}"
            );
            let _ = writeln!(std::io::stderr(), "{message}"); // guardrail: allow-ignore-result exit 2 must not depend on stderr
            std::process::exit(GUARD_HOOK_EXIT_BLOCK);
        }
        result => result,
    }
}

fn run_on_application_thread() -> Result<()> {
    #[cfg(windows)]
    {
        // The derived clap command graph is intentionally broad. Rust's
        // default Windows main-thread stack is small enough that debug builds
        // can exhaust it while materializing the parser before any command is
        // dispatched. Keep the command surface explicit and give only the
        // application thread the stack it demonstrably needs.
        std::thread::Builder::new()
            .name("contextmink-main".to_string())
            .stack_size(8 * 1024 * 1024)
            .spawn(run_application)
            .map_err(|error| anyhow!("failed to start contextmink application thread: {error}"))?
            .join()
            .map_err(|_| anyhow!("contextmink application thread panicked"))?
    }
    #[cfg(not(windows))]
    {
        run_application()
    }
}

fn run_application() -> Result<()> {
    user_installation::check_installation()?;
    output::mark_command_start();
    let args = std::env::args_os().collect::<Vec<_>>();
    if let Some(refusal) = msys_arguments::rewritten_argument_refusal(
        &args,
        &msys_arguments::MsysEnvironment::from_process(),
    ) {
        // Under guard-hook, main turns this into a blocking exit.
        return Err(anyhow!(refusal));
    }
    let cli = parse_cli(&args);
    validate_global_flags(&cli)?;
    match &cli.command {
        Command::SetupUser { home, dry_run } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&user_setup::run(home.as_deref(), *dry_run, false)?)?
            );
            return Ok(());
        }
        Command::UninstallUser { home, dry_run } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&user_setup::run(home.as_deref(), *dry_run, true)?)?
            );
            return Ok(());
        }
        Command::SetupProject {
            project_root,
            dry_run,
            skill_target,
        } => {
            let result = setup_project(SetupProjectRequest {
                project_root,
                source_binary: None,
                dry_run: *dry_run,
                skill_target: *skill_target,
            })?;
            let mut stdout = io::stdout();
            if cli.json {
                serde_json::to_writer(&mut stdout, &result)?;
                writeln!(stdout)?;
            } else {
                writeln!(
                    stdout,
                    "[contextmink] setup-project root={} profile={} dry_run={} ready={} requested_skill_target={} resolved_skill_target={}",
                    result.project_root,
                    result.profile,
                    result.dry_run,
                    result.ready,
                    result.requested_skill_target.as_str(),
                    result.resolved_skill_target.as_str()
                )?;
                write_setup_actions(&mut stdout, &result.actions)?;
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
        Command::UninstallProject {
            project_root,
            dry_run,
        } => {
            let result = uninstall_project(UninstallProjectRequest {
                project_root,
                running_binary: None,
                dry_run: *dry_run,
            })?;
            let mut stdout = io::stdout();
            if cli.json {
                serde_json::to_writer(&mut stdout, &result)?;
                writeln!(stdout)?;
            } else {
                writeln!(
                    stdout,
                    "[contextmink] uninstall-project root={} dry_run={} ready={}",
                    result.project_root, result.dry_run, result.ready
                )?;
                write_setup_actions(&mut stdout, &result.actions)?;
                if !result.preserved_repository_owned.is_empty() {
                    writeln!(
                        stdout,
                        "preserved_repository_owned={}",
                        result
                            .preserved_repository_owned
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
        _ => {}
    }
    let config = match load_context_config(cli.config.as_deref(), cli.no_config) {
        Ok(config) => config,
        Err(error) if matches!(cli.command, Command::GuardHook { .. }) => {
            return Err(error.context("destructive-command policy could not be loaded"));
        }
        Err(error) => return Err(error),
    };
    match &cli.command {
        Command::SetupProject { .. }
        | Command::UninstallProject { .. }
        | Command::SetupUser { .. }
        | Command::UninstallUser { .. } => {
            unreachable!("project lifecycle commands return before config loading")
        }
        Command::Files {
            paths,
            globs,
            path_terms,
            extensions,
            with_excluded,
            with_git_ignored,
            skip_nested_repos,
            quiet,
            show_files,
            show_line_chars,
            ..
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
            *show_files,
            *show_line_chars,
            (*show_files).max(1),
        ),
        Command::Dirs {
            paths,
            depth,
            with_excluded,
            with_git_ignored,
            skip_nested_repos,
            show_dirs,
            show_line_chars,
            max_files_counted,
            ..
        } => command_dirs(
            &cli,
            &config,
            &paths_or_current_dir(paths),
            *depth,
            *with_excluded,
            *with_git_ignored,
            *skip_nested_repos,
            *show_dirs,
            *show_line_chars,
            *max_files_counted,
        ),
        Command::Grep {
            paths,
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
            show_files,
            show_lines_per_file,
            context,
            show_lines,
            show_line_chars,
            max_content_files,
            max_file_bytes,
            max_content_bytes,
            ..
        } => command_grep(
            &cli,
            &config,
            paths,
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
                max_files: *show_files,
                lines_per_file: *show_lines_per_file,
                context: *context,
                max_sample_lines: *show_lines,
                max_line_chars: *show_line_chars,
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
            show_files,
            show_lines_per_file,
            context,
            show_lines,
            show_line_chars,
            max_content_files,
            max_file_bytes,
            max_content_bytes,
            ..
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
                    max_files: *show_files,
                    lines_per_file: *show_lines_per_file,
                    context: *context,
                    max_sample_lines: *show_lines,
                    max_line_chars: *show_line_chars,
                    max_content_files: *max_content_files,
                    max_file_bytes: *max_file_bytes,
                    max_content_bytes: *max_content_bytes,
                },
            )
        }
        Command::Slice {
            file,
            range,
            tail,
            line_ceiling,
            show_line_chars,
            char_start,
            chars,
            ..
        } => command_slice(
            &cli,
            &config,
            file,
            range.as_deref(),
            *tail,
            *line_ceiling,
            *show_line_chars,
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
            show_items,
            show_line_chars,
            ..
        } => command_outline(
            &cli,
            &config,
            file,
            lang.as_deref(),
            prefix.as_deref(),
            pattern.as_deref(),
            contains,
            *ignore_case,
            *show_items,
            *show_line_chars,
        ),
        Command::JsonFind {
            file,
            key_contains,
            key_regex,
            pointer_contains,
            pointer_regex,
            value_contains,
            show_matches,
            show_value_chars,
            max_document_bytes,
            ..
        } => command_json_find(
            &cli,
            &config,
            file,
            key_contains,
            key_regex.as_deref(),
            pointer_contains,
            pointer_regex.as_deref(),
            value_contains,
            *show_matches,
            *show_value_chars,
            *max_document_bytes,
        ),
        Command::JsonSelect {
            file,
            entries,
            at,
            fields,
            keys,
            where_exact,
            where_contains,
            show_rows,
            show_value_chars,
            max_document_bytes,
            ..
        } => command_json_select(
            &cli,
            &config,
            file,
            at.as_deref(),
            *entries,
            fields,
            where_exact,
            where_contains,
            *keys,
            *show_rows,
            *show_value_chars,
            *max_document_bytes,
        ),
        Command::Sqlite {
            path,
            sql,
            sql_file,
            json_params,
            jsonl_params,
            max_param_bytes,
            show_rows,
            max_rows_scanned,
            timeout_secs,
            show_value_chars,
            ..
        } => command_sqlite(
            &cli,
            &config,
            path,
            sql.as_deref(),
            sql_file.as_deref(),
            json_params,
            jsonl_params,
            *max_param_bytes,
            *show_rows,
            *max_rows_scanned,
            *timeout_secs,
            *show_value_chars,
        ),
        Command::SqliteSchema {
            path,
            tables,
            name_contains,
            with_shadow_tables,
            with_system_tables,
            show_tables,
            show_columns,
            show_indexes,
            show_line_chars,
            ..
        } => command_sqlite_schema(
            &cli,
            &config,
            path,
            tables,
            name_contains,
            *with_shadow_tables,
            *with_system_tables,
            *show_tables,
            *show_columns,
            *show_indexes,
            *show_line_chars,
        ),
        Command::Capture {
            show_lines,
            show_bytes_per_stream,
            show_line_chars,
            script,
            expect_exit,
            receipt_out,
            argv,
            ..
        } => command_capture(
            &cli,
            &config,
            *show_lines,
            *show_bytes_per_stream,
            *show_line_chars,
            *script,
            expect_exit,
            receipt_out.as_ref(),
            argv,
        ),
        Command::GuardHook {
            command_field,
            expected_root,
            shell,
            ..
        } => command_guard_hook(
            &config.destructive_guard,
            command_field,
            expected_root.as_deref(),
            *shell,
        ),
        Command::GuardCheck {
            command,
            shell,
            argv,
            ..
        } => {
            // argv is trailing and accepts hyphen values; a flag-like first
            // item is a misplaced or unsupported option, never a program.
            if let Some(first) = argv.first().filter(|first| first.starts_with('-')) {
                return Err(anyhow!(
                    "guard-check received `{first}` as the first argv item; it is not a guard-check option (see `contextmink guard-check --help`). Place guard-check options before `--` and the argv after it"
                ));
            }
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
            let report = serde_json::json!({
                "schema": "contextmink.guard_check.v1",
                "input_kind": input_kind,
                "shell": command.as_ref().map(|_| shell.unwrap_or(ShellDialect::Posix).cli_name()),
                "decision": outcome,
                "message": message,
                "executed": false,
                "policy_scope": "contextmink_only",
                "policy_root": config.policy_root,
                "configured_rules": {
                    "recursive_delete_fragments": config.destructive_guard.recursive_delete_fragments,
                    "delete_fragments": config.destructive_guard.delete_fragments,
                },
                "override_applied": false,
                "scope_note": "Evaluates Contextmink rules only. Host approval policies are not evaluated; allow is not authorization to execute.",
            });
            if cli.json {
                output::emit_json(report)
            } else {
                let mut stdout = io::stdout();
                writeln!(
                    stdout,
                    "decision={outcome} input_kind={input_kind} shell={} executed=false",
                    command
                        .as_ref()
                        .map_or("argv", |_| shell.unwrap_or(ShellDialect::Posix).cli_name())
                )?;
                if let Some(message) = report["message"].as_str() {
                    writeln!(stdout, "{message}")?;
                }
                writeln!(stdout, "{}", report["scope_note"].as_str().unwrap())?;
                Ok(())
            }
        }
        Command::GuardHookSnippet {
            binary,
            guard_config,
            matchers,
            command_field,
            ..
        } => command_guard_hook_snippet(
            binary.as_deref(),
            guard_config.as_deref(),
            cli.config.as_deref(),
            cli.no_config,
            matchers,
            command_field,
        ),
    }
}

fn validate_global_flags(cli: &Cli) -> Result<()> {
    if cli.json && matches!(&cli.command, Command::GuardHook { .. }) {
        return Err(anyhow!(
            "guard-hook uses the agent hook protocol; --json does not apply"
        ));
    }
    Ok(())
}

fn write_setup_actions(
    stdout: &mut impl Write,
    actions: &[config::project_setup::SetupAction],
) -> Result<()> {
    for action in actions {
        let verb = match action.action {
            SetupActionKind::Create => "create",
            SetupActionKind::Replace => "replace",
            SetupActionKind::Unchanged => "unchanged",
            SetupActionKind::PreserveRepositoryOwned => "preserve_repository_owned",
            SetupActionKind::MakeExecutable => "make_executable",
            SetupActionKind::UpdateGitignore => "update_gitignore",
            SetupActionKind::RemoveManaged => "remove_managed",
            SetupActionKind::RemoveRetired => "remove_retired",
            SetupActionKind::UnownedRefusal => "unowned_refusal",
            SetupActionKind::ModifiedRefusal => "modified_refusal",
        };
        writeln!(stdout, "{verb}\t{}", display_path(&action.path))?;
    }
    Ok(())
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
