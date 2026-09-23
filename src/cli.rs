use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, Subcommand, error::ErrorKind};

use crate::config::project_setup::SkillTarget;
use crate::destructive_guard::ShellDialect;

pub(crate) const SUBCOMMAND_NAMES: &[&str] = &[
    "files",
    "dirs",
    "grep",
    "grep-terms",
    "slice",
    "outline",
    "json-find",
    "json-select",
    "sqlite",
    "sqlite-schema",
    "setup-user",
    "uninstall-user",
    "setup-project",
    "uninstall-project",
    "capture",
    "guard-hook",
    "guard-check",
    "guard-hook-snippet",
];

#[derive(Debug, Parser)]
#[command(author, version, about)]
pub(crate) struct Cli {
    /// Emit one JSON object instead of human-readable rows plus a receipt line.
    #[arg(long, global = true, help_heading = "Global options")]
    pub(crate) json: bool,
    /// Resolved from the subcommand's receipt options; false where none exist.
    #[arg(skip)]
    pub(crate) fail_if_truncated: bool,
    #[arg(skip)]
    pub(crate) require_complete_scope: bool,
    /// Resolved from the subcommand's configuration options.
    #[arg(skip)]
    pub(crate) config: Option<PathBuf>,
    #[arg(skip)]
    pub(crate) no_config: bool,
    #[command(subcommand)]
    pub(crate) command: Command,
}

/// Strictness for commands that emit `contextmink.receipt.v2`. Setup,
/// removal, and guard commands do not accept these flags at all.
#[derive(Debug, Clone, Default, Args)]
#[command(next_help_heading = "Receipt options")]
pub(crate) struct ReceiptOptions {
    /// Exit nonzero after emitting a receipt if the command output was capped.
    #[arg(long)]
    pub(crate) fail_if_truncated: bool,
    /// Exit nonzero after emitting a receipt if the inspected evidence scope is incomplete.
    #[arg(long)]
    pub(crate) require_complete_scope: bool,
}

/// Configuration selection for commands that read `.contextmink.toml`.
#[derive(Debug, Clone, Default, Args)]
#[command(next_help_heading = "Configuration options")]
pub(crate) struct ConfigSource {
    /// Read configuration from this TOML file instead of searching upward.
    #[arg(long, value_name = "FILE", conflicts_with = "no_config")]
    pub(crate) config: Option<PathBuf>,
    /// Ignore .contextmink.toml and use only built-in defaults.
    #[arg(long)]
    pub(crate) no_config: bool,
}

impl Cli {
    /// Project the subcommand's option groups onto the fields every command
    /// module reads, so strict-mode checks stay in one place.
    fn resolve_command_options(mut self) -> Self {
        if let Some(options) = self.command.receipt_options() {
            self.fail_if_truncated = options.fail_if_truncated;
            self.require_complete_scope = options.require_complete_scope;
        }
        if let Some(source) = self.command.config_source() {
            self.config = source.config.clone();
            self.no_config = source.no_config;
        }
        self
    }
}

impl Command {
    fn receipt_options(&self) -> Option<&ReceiptOptions> {
        match self {
            Command::Files {
                receipt_options, ..
            }
            | Command::Dirs {
                receipt_options, ..
            }
            | Command::Grep {
                receipt_options, ..
            }
            | Command::GrepTerms {
                receipt_options, ..
            }
            | Command::Slice {
                receipt_options, ..
            }
            | Command::Outline {
                receipt_options, ..
            }
            | Command::JsonFind {
                receipt_options, ..
            }
            | Command::JsonSelect {
                receipt_options, ..
            }
            | Command::Sqlite {
                receipt_options, ..
            }
            | Command::SqliteSchema {
                receipt_options, ..
            }
            | Command::Capture {
                receipt_options, ..
            } => Some(receipt_options),
            _ => None,
        }
    }

    fn config_source(&self) -> Option<&ConfigSource> {
        match self {
            Command::Files { config_source, .. }
            | Command::Dirs { config_source, .. }
            | Command::Grep { config_source, .. }
            | Command::GrepTerms { config_source, .. }
            | Command::Slice { config_source, .. }
            | Command::Outline { config_source, .. }
            | Command::JsonFind { config_source, .. }
            | Command::JsonSelect { config_source, .. }
            | Command::Sqlite { config_source, .. }
            | Command::SqliteSchema { config_source, .. }
            | Command::Capture { config_source, .. }
            | Command::GuardHook { config_source, .. }
            | Command::GuardCheck { config_source, .. }
            | Command::GuardHookSnippet { config_source, .. } => Some(config_source),
            _ => None,
        }
    }
}

pub(crate) fn parse_cli(args: &[OsString]) -> Cli {
    match Cli::try_parse_from(args) {
        Ok(cli) => cli.resolve_command_options(),
        Err(error) => {
            if error.kind() == ErrorKind::InvalidSubcommand
                && let Some(guidance) = renamed_command_guidance(args)
            {
                Cli::command()
                    .error(ErrorKind::InvalidSubcommand, guidance)
                    .exit();
            }
            if matches!(
                error.kind(),
                ErrorKind::UnknownArgument
                    | ErrorKind::MissingRequiredArgument
                    | ErrorKind::ArgumentConflict
            ) && let Some(guidance) = noncanonical_form_guidance(args)
            {
                Cli::command()
                    .error(ErrorKind::InvalidValue, guidance)
                    .exit();
            }
            error.exit()
        }
    }
}

pub(crate) fn noncanonical_form_guidance(args: &[OsString]) -> Option<String> {
    let flag_present = |value: &str| {
        args.iter().any(|arg| {
            let arg = arg.to_string_lossy();
            arg == value || arg.starts_with(&format!("{value}="))
        })
    };
    let command = selected_subcommand(args)?;
    if let Some(guidance) = misplaced_option_guidance(args, command) {
        return Some(guidance);
    }

    if command == "json-select" && flag_present("--array") {
        return Some(
            "json-select uses `--at <KEY_OR_POINTER>` for arrays, objects, and scalars; replace `--array` with `--at`"
                .to_owned(),
        );
    }
    if command == "grep" && flag_present("--path") {
        return Some(
            "grep paths are positional; use `contextmink grep --pattern <PATTERN> <PATH>...`"
                .to_owned(),
        );
    }
    if command == "grep" && !flag_present("--pattern") && !flag_present("--pattern-file") {
        return Some(
            "grep requires an explicit pattern; use `contextmink grep --pattern <PATTERN> <PATH>...` or `--pattern-file <FILE> <PATH>...`"
                .to_owned(),
        );
    }
    if command == "slice"
        && ["--start", "--end", "--lines", "--start-line", "--end-line"]
            .into_iter()
            .any(flag_present)
    {
        return Some(
            "slice selects a window with `--range START:END` or `--tail N`; without either it reads from line 1 up to `--line-ceiling` lines"
                .to_owned(),
        );
    }
    RENAMED_FLAGS
        .iter()
        .find(|(commands, old, _)| commands.contains(&command) && flag_present(old))
        .map(|(_, _, guidance)| (*guidance).to_owned())
}

/// Configuration and receipt flags belong to the subcommands that use them.
/// Name the placement fix, or say the option does not apply at all.
fn misplaced_option_guidance(args: &[OsString], command: &str) -> Option<String> {
    const SUBCOMMAND_OPTIONS: &[&str] = &[
        "--config",
        "--no-config",
        "--fail-if-truncated",
        "--require-complete-scope",
    ];
    let command_index = args.iter().position(|arg| arg.to_str() == Some(command))?;
    let option_name = |arg: &OsString| {
        let arg = arg.to_string_lossy();
        let name = arg.split_once('=').map_or(arg.as_ref(), |(name, _)| name);
        SUBCOMMAND_OPTIONS
            .iter()
            .copied()
            .find(|option| *option == name)
    };
    let command_help = Cli::command();
    let accepts = |option: &str| {
        command_help
            .find_subcommand(command)
            .is_some_and(|subcommand| {
                subcommand
                    .get_arguments()
                    .any(|arg| arg.get_long() == option.strip_prefix("--"))
            })
    };
    for (index, arg) in args.iter().enumerate().skip(1) {
        let Some(option) = option_name(arg) else {
            continue;
        };
        if !accepts(option) {
            let reason = if matches!(option, "--config" | "--no-config") {
                "it does not read .contextmink.toml"
            } else {
                "it does not emit a contextmink receipt"
            };
            return Some(format!(
                "{command} does not accept {option}: {reason}; remove {option}"
            ));
        }
        if index < command_index {
            return Some(format!(
                "{option} is a {command} option; place it after the subcommand: `contextmink {command} {option} ...`"
            ));
        }
    }
    None
}

/// Guard commands are noun-first; removed spellings name their replacement.
/// A stale `hook-guard` registration therefore exits 2 (blocking) with the
/// fix on stderr instead of silently disabling the guard.
pub(crate) fn renamed_command_guidance(args: &[OsString]) -> Option<&'static str> {
    args.iter()
        .skip(1)
        .find(|arg| !arg.to_string_lossy().starts_with('-'))
        .and_then(|arg| match arg.to_str()? {
            "hook-guard" => Some(
                "`hook-guard` was renamed `guard-hook`, and its `--command-field` now takes a JSON Pointer (`/tool_input/command`); update the hook command, for example by regenerating it with `contextmink guard-hook-snippet`",
            ),
            "hook-snippet" => Some(
                "`hook-snippet` was renamed `guard-hook-snippet`; its output registers `guard-hook`",
            ),
            _ => None,
        })
}

/// Replacement guidance for one removed flag spelling of `command`.
pub(crate) fn renamed_flag_guidance(command: &str, flag: &str) -> Option<&'static str> {
    let flag = flag.split_once('=').map_or(flag, |(name, _)| name);
    RENAMED_FLAGS
        .iter()
        .find(|(commands, old, _)| commands.contains(&command) && *old == flag)
        .map(|(_, _, guidance)| *guidance)
}

/// Removed flag spellings and the refusal that names their replacement.
/// Display caps are `--show-*`; `--max-*` names only scope or admission caps.
const RENAMED_FLAGS: &[(&[&str], &str, &str)] = &[
    (
        &["files", "grep", "grep-terms"],
        "--limit",
        "the displayed-file cap is `--show-files`; replace `--limit`",
    ),
    (
        &["dirs"],
        "--limit",
        "the displayed-directory cap is `--show-dirs`; replace `--limit`",
    ),
    (
        &["outline"],
        "--limit",
        "the displayed-row cap is `--show-items`; replace `--limit`",
    ),
    (
        &["outline"],
        "--max-items",
        "the displayed-row cap is `--show-items`; replace `--max-items`",
    ),
    (
        &["json-find"],
        "--limit",
        "the displayed-match cap is `--show-matches`; replace `--limit`",
    ),
    (
        &["json-select", "sqlite"],
        "--limit",
        "the displayed-row cap is `--show-rows`; replace `--limit`",
    ),
    (
        &["grep", "grep-terms"],
        "--lines-per-file",
        "the per-file sample cap is `--show-lines-per-file`; replace `--lines-per-file`",
    ),
    (
        &["grep", "grep-terms"],
        "--max-sample-lines",
        "the total sample-line cap is `--show-lines`; replace `--max-sample-lines`",
    ),
    (
        &["capture"],
        "--max-lines",
        "the displayed-line cap is `--show-lines`; replace `--max-lines`",
    ),
    (
        &["capture"],
        "--max-bytes",
        "the per-stream retention cap is `--show-bytes-per-stream`; replace `--max-bytes`",
    ),
    (
        &["slice"],
        "--max-lines",
        "the slice line ceiling is `--line-ceiling`; replace `--max-lines`",
    ),
    (
        &[
            "files",
            "dirs",
            "grep",
            "grep-terms",
            "slice",
            "outline",
            "sqlite-schema",
            "capture",
        ],
        "--max-line-chars",
        "the per-line character cap is `--show-line-chars`; replace `--max-line-chars`",
    ),
    (
        &["json-find", "json-select", "sqlite"],
        "--max-value-chars",
        "the per-value character cap is `--show-value-chars`; replace `--max-value-chars`",
    ),
    (
        &["sqlite-schema"],
        "--max-tables",
        "the displayed-table cap is `--show-tables`; replace `--max-tables`",
    ),
    (
        &["sqlite-schema"],
        "--max-columns",
        "the displayed-column cap is `--show-columns`; replace `--max-columns`",
    ),
    (
        &["sqlite-schema"],
        "--max-indexes",
        "the displayed-index cap is `--show-indexes`; replace `--max-indexes`",
    ),
    (
        &["sqlite-schema"],
        "--include-shadow",
        "shadow tables are included with `--with-shadow-tables`; replace `--include-shadow`",
    ),
    (
        &["sqlite-schema"],
        "--include-system",
        "system tables are included with `--with-system-tables`; replace `--include-system`",
    ),
    (
        &["json-find"],
        "--path-contains",
        "JSON Pointer filters are `--pointer-contains`; replace `--path-contains`",
    ),
    (
        &["json-find"],
        "--path-regex",
        "JSON Pointer filters are `--pointer-regex`; replace `--path-regex`",
    ),
];

pub(crate) fn selected_subcommand(args: &[OsString]) -> Option<&str> {
    let mut index = 1;
    while index < args.len() {
        let arg = args[index].to_str()?;
        match arg {
            "--config" => index += 2,
            "--json" | "--fail-if-truncated" | "--require-complete-scope" | "--no-config" => {
                index += 1;
            }
            value if value.starts_with("--config=") => index += 1,
            value if SUBCOMMAND_NAMES.contains(&value) => return Some(value),
            _ => return None,
        }
    }
    None
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// List candidate files with configured excludes and a display cap.
    Files {
        #[arg(value_name = "PATH", help = "Files or directories to enumerate")]
        paths: Vec<PathBuf>,
        #[arg(
            long = "glob",
            help = "Only include paths matching this glob or basename"
        )]
        globs: Vec<String>,
        #[arg(
            long = "path-contains",
            value_name = "TEXT",
            help = "Only include paths containing this literal text; repeat for all required terms"
        )]
        path_terms: Vec<String>,
        #[arg(
            long = "ext",
            value_name = "EXT",
            help = "Only include files with this extension (comma-separated list ok); leading dot is optional"
        )]
        extensions: Vec<String>,
        #[arg(
            long = "with-excluded",
            help = "Include files matched by contextmink exclude globs. Does not disable Git ignore rules; explicit paths inside excluded trees do not need this."
        )]
        with_excluded: bool,
        #[arg(
            long = "with-git-ignored",
            help = "Include files hidden by Git/.ignore rules. Contextmink exclude globs still apply unless --with-excluded is also set."
        )]
        with_git_ignored: bool,
        #[arg(
            long = "skip-nested-repos",
            help = "Do not cross nested Git repository roots, including tracked submodules and Git-ignored sibling repositories, during broad scans"
        )]
        skip_nested_repos: bool,
        #[arg(
            long,
            help = "Suppress the file list; emit only the receipt (totals, caps, truncation, scan-scope fields)"
        )]
        quiet: bool,
        #[arg(long, default_value_t = 80, help = "Maximum paths to print")]
        show_files: usize,
        #[arg(
            long,
            default_value_t = 220,
            help = "Maximum characters per printed path"
        )]
        show_line_chars: usize,
        #[command(flatten)]
        receipt_options: ReceiptOptions,
        #[command(flatten)]
        config_source: ConfigSource,
    },
    /// Summarize directories with bounded recursive file counts.
    Dirs {
        #[arg(value_name = "PATH", help = "Directories to summarize")]
        paths: Vec<PathBuf>,
        #[arg(
            long,
            default_value_t = 2,
            help = "Directory levels below each root to report"
        )]
        depth: usize,
        #[arg(
            long = "with-excluded",
            help = "Include files matched by contextmink exclude globs. Does not disable Git ignore rules; explicit paths inside excluded trees do not need this."
        )]
        with_excluded: bool,
        #[arg(
            long = "with-git-ignored",
            help = "Include files hidden by Git/.ignore rules. Contextmink exclude globs still apply unless --with-excluded is also set."
        )]
        with_git_ignored: bool,
        #[arg(
            long = "skip-nested-repos",
            help = "Do not cross nested Git repository roots, including tracked submodules and Git-ignored sibling repositories, during broad scans"
        )]
        skip_nested_repos: bool,
        #[arg(long, default_value_t = 60, help = "Maximum directories to print")]
        show_dirs: usize,
        #[arg(
            long,
            default_value_t = 220,
            help = "Maximum characters per complete printed directory row"
        )]
        show_line_chars: usize,
        #[arg(
            long,
            default_value_t = 50_000,
            help = "Maximum candidate files to count into directory summaries"
        )]
        max_files_counted: usize,
        #[command(flatten)]
        receipt_options: ReceiptOptions,
        #[command(flatten)]
        config_source: ConfigSource,
    },
    /// Search text and report bounded file counts plus sample lines.
    ///
    /// Supply the pattern with --pattern or --pattern-file. Every positional is
    /// a search path; the current directory is used when none is supplied.
    Grep {
        #[arg(
            value_name = "PATH",
            help = "Files or directories to search; defaults to the current directory"
        )]
        paths: Vec<PathBuf>,
        #[arg(
            long = "pattern",
            value_name = "PATTERN",
            required_unless_present = "pattern_file",
            conflicts_with = "pattern_file",
            help = "Regex or literal pattern to search for"
        )]
        pattern: Option<String>,
        #[arg(
            long = "pattern-file",
            value_name = "FILE",
            required_unless_present = "pattern",
            conflicts_with = "pattern",
            help = "Read the regex or literal pattern from a UTF-8 file"
        )]
        pattern_file: Option<PathBuf>,
        #[arg(long, help = "Treat the pattern as literal text instead of regex")]
        literal: bool,
        #[arg(long = "ignore-case", short = 'i', help = "Match case-insensitively")]
        ignore_case: bool,
        #[arg(
            long = "glob",
            help = "Only search files matching this glob or basename"
        )]
        globs: Vec<String>,
        #[arg(
            long = "ext",
            value_name = "EXT",
            help = "Only search files with this extension (comma-separated list ok); leading dot is optional"
        )]
        extensions: Vec<String>,
        #[arg(
            long,
            short = 'C',
            default_value_t = 0,
            help = "Context lines to print around each sample match line"
        )]
        context: usize,
        #[arg(
            long = "with-excluded",
            help = "Include files matched by contextmink exclude globs. Does not disable Git ignore rules; explicit paths inside excluded trees do not need this."
        )]
        with_excluded: bool,
        #[arg(
            long = "with-git-ignored",
            help = "Include files hidden by Git/.ignore rules. Contextmink exclude globs still apply unless --with-excluded is also set."
        )]
        with_git_ignored: bool,
        #[arg(
            long = "skip-nested-repos",
            help = "Do not cross nested Git repository roots, including tracked submodules and Git-ignored sibling repositories, during broad scans"
        )]
        skip_nested_repos: bool,
        #[arg(
            long,
            help = "Suppress per-file match content and file lists; emit only the receipt (totals, caps, truncation, scan-scope fields)"
        )]
        quiet: bool,
        #[arg(
            long,
            default_value_t = 80,
            help = "Maximum matching files to count before stopping content inspection"
        )]
        max_matching_files: usize,
        #[arg(long, default_value_t = 12, help = "Maximum matching files to print")]
        show_files: usize,
        #[arg(
            long,
            default_value_t = 3,
            help = "Maximum sample lines per matching file"
        )]
        show_lines_per_file: usize,
        #[arg(
            long = "show-lines",
            default_value_t = 36,
            help = "Maximum matching and context sample lines to print across all files"
        )]
        show_lines: usize,
        #[arg(
            long,
            default_value_t = 220,
            help = "Maximum characters per sample line"
        )]
        show_line_chars: usize,
        #[arg(
            long,
            default_value_t = 20_000,
            help = "Maximum candidate files whose content may be inspected"
        )]
        max_content_files: usize,
        #[arg(
            long,
            default_value_t = 2_000_000,
            help = "Skip files larger than this byte count"
        )]
        max_file_bytes: u64,
        #[arg(
            long,
            value_name = "BYTES",
            help = "Maximum cumulative candidate bytes admitted for deterministic content inspection [default: no byte cap]"
        )]
        max_content_bytes: Option<u64>,
        #[command(flatten)]
        receipt_options: ReceiptOptions,
        #[command(flatten)]
        config_source: ConfigSource,
    },
    /// Search for literal terms without regex or shell-fragile pattern syntax.
    #[command(name = "grep-terms")]
    GrepTerms {
        #[arg(
            long = "term",
            allow_hyphen_values = true,
            value_name = "TERM",
            help = "Literal term to search for"
        )]
        terms: Vec<String>,
        #[arg(
            long = "term-file",
            value_name = "FILE",
            help = "Read literal terms from a UTF-8 file, one per line"
        )]
        term_files: Vec<PathBuf>,
        #[arg(
            long,
            help = "Match a line containing any term instead of requiring all terms"
        )]
        any: bool,
        #[arg(long = "ignore-case", short = 'i', help = "Match case-insensitively")]
        ignore_case: bool,
        #[arg(
            long = "glob",
            help = "Only search files matching this glob or basename"
        )]
        globs: Vec<String>,
        #[arg(
            long = "ext",
            value_name = "EXT",
            help = "Only search files with this extension (comma-separated list ok); leading dot is optional"
        )]
        extensions: Vec<String>,
        #[arg(
            long,
            short = 'C',
            default_value_t = 0,
            help = "Context lines to print around each sample match line"
        )]
        context: usize,
        #[arg(
            value_name = "PATH",
            help = "Files or directories to search; defaults to the current directory"
        )]
        paths: Vec<PathBuf>,
        #[arg(
            long = "with-excluded",
            help = "Include files matched by contextmink exclude globs. Does not disable Git ignore rules; explicit paths inside excluded trees do not need this."
        )]
        with_excluded: bool,
        #[arg(
            long = "with-git-ignored",
            help = "Include files hidden by Git/.ignore rules. Contextmink exclude globs still apply unless --with-excluded is also set."
        )]
        with_git_ignored: bool,
        #[arg(
            long = "skip-nested-repos",
            help = "Do not cross nested Git repository roots, including tracked submodules and Git-ignored sibling repositories, during broad scans"
        )]
        skip_nested_repos: bool,
        #[arg(
            long,
            help = "Suppress per-file match content and file lists; emit only the receipt (totals, caps, truncation, scan-scope fields)"
        )]
        quiet: bool,
        #[arg(
            long,
            default_value_t = 80,
            help = "Maximum matching files to count before stopping content inspection"
        )]
        max_matching_files: usize,
        #[arg(long, default_value_t = 12, help = "Maximum matching files to print")]
        show_files: usize,
        #[arg(
            long,
            default_value_t = 3,
            help = "Maximum sample lines per matching file"
        )]
        show_lines_per_file: usize,
        #[arg(
            long = "show-lines",
            default_value_t = 36,
            help = "Maximum matching and context sample lines to print across all files"
        )]
        show_lines: usize,
        #[arg(
            long,
            default_value_t = 220,
            help = "Maximum characters per sample line"
        )]
        show_line_chars: usize,
        #[arg(
            long,
            default_value_t = 20_000,
            help = "Maximum candidate files whose content may be inspected"
        )]
        max_content_files: usize,
        #[arg(
            long,
            default_value_t = 2_000_000,
            help = "Skip files larger than this byte count"
        )]
        max_file_bytes: u64,
        #[arg(
            long,
            value_name = "BYTES",
            help = "Maximum cumulative candidate bytes admitted for deterministic content inspection [default: no byte cap]"
        )]
        max_content_bytes: Option<u64>,
        #[command(flatten)]
        receipt_options: ReceiptOptions,
        #[command(flatten)]
        config_source: ConfigSource,
    },
    /// Print a bounded line or character window from one text file.
    Slice {
        #[arg(value_name = "FILE", help = "Text file to slice")]
        file: PathBuf,
        #[arg(
            long,
            value_name = "START:END",
            conflicts_with = "tail",
            help = "One-based inclusive line window; without --range or --tail, slice reads from line 1 up to --line-ceiling lines"
        )]
        range: Option<String>,
        #[arg(
            long,
            value_name = "N",
            help = "Print the last N lines instead of a start-anchored window"
        )]
        tail: Option<usize>,
        #[arg(
            long,
            value_name = "LINES",
            default_value_t = 220,
            help = "Most lines printed for any window; a larger window is capped and the receipt names remaining_range"
        )]
        line_ceiling: usize,
        #[arg(
            long,
            default_value_t = 240,
            help = "Maximum characters per source-line text field"
        )]
        show_line_chars: usize,
        #[arg(
            long,
            conflicts_with_all = [
                "range",
                "tail",
                "line_ceiling",
                "show_line_chars"
            ],
            help = "Zero-based character offset for character-window mode"
        )]
        char_start: Option<usize>,
        #[arg(
            long,
            default_value_t = 4000,
            requires = "char_start",
            help = "Character count for character-window mode"
        )]
        chars: usize,
        #[command(flatten)]
        receipt_options: ReceiptOptions,
        #[command(flatten)]
        config_source: ConfigSource,
    },
    /// Map declaration-shaped lines in one source file for orientation.
    ///
    /// An outline is structured grep over per-language declaration heuristics,
    /// not a parser: use it to locate the right region of a large file, then
    /// read that region with `slice`.
    Outline {
        #[arg(value_name = "FILE", help = "Source file to outline")]
        file: PathBuf,
        #[arg(
            long,
            value_name = "LANG",
            conflicts_with_all = ["prefix", "pattern"],
            help = "Language heuristic to use instead of the file extension"
        )]
        lang: Option<String>,
        #[arg(
            long,
            value_name = "TEXT",
            conflicts_with = "pattern",
            help = "Outline lines that start with this literal text after indentation, instead of a language heuristic"
        )]
        prefix: Option<String>,
        #[arg(
            long,
            value_name = "REGEX",
            help = "Custom declaration-line regex instead of a built-in language heuristic"
        )]
        pattern: Option<String>,
        #[arg(
            long = "contains",
            value_name = "TEXT",
            help = "Only keep outline rows containing this text; repeatable, all must hold"
        )]
        contains: Vec<String>,
        #[arg(
            long = "ignore-case",
            short = 'i',
            help = "Match --contains case-insensitively"
        )]
        ignore_case: bool,
        #[arg(long, default_value_t = 120, help = "Maximum outline rows to print")]
        show_items: usize,
        #[arg(
            long,
            default_value_t = 220,
            help = "Maximum characters per declaration text field"
        )]
        show_line_chars: usize,
        #[command(flatten)]
        receipt_options: ReceiptOptions,
        #[command(flatten)]
        config_source: ConfigSource,
    },
    /// Find JSON values by key, path, or summarized value predicates.
    JsonFind {
        #[arg(value_name = "FILE", help = "JSON or JSONL file to inspect")]
        file: PathBuf,
        #[arg(
            long,
            value_name = "TEXT",
            help = "Match object keys containing this text; repeatable, all must hold (use --key-regex for alternatives)"
        )]
        key_contains: Vec<String>,
        #[arg(long, value_name = "REGEX", help = "Match object keys with this regex")]
        key_regex: Option<String>,
        #[arg(
            long,
            value_name = "TEXT",
            help = "Match JSON Pointers containing this text (for example /items/0/name); repeatable, all must hold (use --pointer-regex for alternatives)"
        )]
        pointer_contains: Vec<String>,
        #[arg(
            long,
            value_name = "REGEX",
            help = "Match JSON Pointers with this regex"
        )]
        pointer_regex: Option<String>,
        #[arg(
            long,
            value_name = "TEXT",
            help = "Match summarized values containing this text; repeatable, all must hold"
        )]
        value_contains: Vec<String>,
        #[arg(long, default_value_t = 40, help = "Maximum matches to print")]
        show_matches: usize,
        #[arg(
            long,
            default_value_t = 260,
            help = "Maximum characters per summarized value"
        )]
        show_value_chars: usize,
        #[arg(
            long,
            default_value_t = crate::json_input::DEFAULT_MAX_JSON_DOCUMENT_BYTES,
            help = "Maximum bytes materialized for one JSON document or retained for one JSONL record"
        )]
        max_document_bytes: u64,
        #[command(flatten)]
        receipt_options: ReceiptOptions,
        #[command(flatten)]
        config_source: ConfigSource,
    },
    /// Select JSON values or array rows and print bounded field summaries.
    #[command(name = "json-select")]
    JsonSelect {
        #[arg(value_name = "FILE", help = "JSON or JSONL file to project")]
        file: PathBuf,
        #[arg(
            long,
            help = "Project each entry of the selected object; retain exact source key and JSON Pointer. JSONL requires --at /RECORD/object."
        )]
        entries: bool,
        #[arg(
            long,
            value_name = "KEY_OR_POINTER",
            help = "Select any value by key or JSON Pointer; arrays yield rows, objects/scalars yield one row. JSONL pointers start with a zero-based record index (for example /12/result)."
        )]
        at: Option<String>,
        #[arg(
            long = "fields",
            value_name = "KEY_OR_POINTERS",
            help = "Comma-separated field keys or JSON Pointers to include in each row"
        )]
        fields: Vec<String>,
        #[arg(
            long,
            help = "Report the union of top-level row keys (presence counts and value types) instead of projecting rows; discovers an unknown row shape in one call"
        )]
        keys: bool,
        #[arg(
            long = "where",
            value_name = "FIELD=VALUE",
            help = "Only keep rows whose field equals VALUE exactly; repeatable, all must hold"
        )]
        where_exact: Vec<String>,
        #[arg(
            long = "where-contains",
            value_name = "FIELD=TEXT",
            help = "Only keep rows whose field value contains TEXT; repeatable, all must hold"
        )]
        where_contains: Vec<String>,
        #[arg(
            long,
            default_value_t = 40,
            help = "Maximum rows (or --keys entries) to print"
        )]
        show_rows: usize,
        #[arg(
            long,
            default_value_t = 260,
            help = "Maximum characters per projected value"
        )]
        show_value_chars: usize,
        #[arg(
            long,
            default_value_t = crate::json_input::DEFAULT_MAX_JSON_DOCUMENT_BYTES,
            help = "Maximum bytes materialized for one JSON document or retained for one JSONL record"
        )]
        max_document_bytes: u64,
        #[command(flatten)]
        receipt_options: ReceiptOptions,
        #[command(flatten)]
        config_source: ConfigSource,
    },
    /// Run a read-only `SQLite` query with bounded row output.
    Sqlite {
        #[arg(value_name = "DB", help = "SQLite database file")]
        path: PathBuf,
        #[arg(long, help = "Read-only SQL query to run")]
        sql: Option<String>,
        #[arg(
            long = "sql-file",
            value_name = "FILE",
            help = "Read the SQL query from a UTF-8 file; use '-' to read SQL from stdin"
        )]
        sql_file: Option<PathBuf>,
        #[arg(
            long = "json-param",
            value_name = "NAME=FILE",
            help = "Bind a JSON file as SQL parameter :NAME; repeatable"
        )]
        json_params: Vec<String>,
        #[arg(
            long = "jsonl-param",
            value_name = "NAME=FILE",
            help = "Read a JSONL file as a JSON array and bind it as SQL parameter :NAME; repeatable"
        )]
        jsonl_params: Vec<String>,
        #[arg(
            long,
            default_value_t = 8_388_608u64,
            help = "Maximum bytes per --json-param/--jsonl-param file"
        )]
        max_param_bytes: u64,
        #[arg(long, default_value_t = 40, help = "Maximum rows to print")]
        show_rows: usize,
        #[arg(
            long,
            default_value_t = 5000,
            help = "Maximum rows to scan before treating totals as lower bounds"
        )]
        max_rows_scanned: usize,
        #[arg(
            long = "timeout-secs",
            default_value_t = 60,
            help = "Interrupt the query after this many seconds; 0 disables"
        )]
        timeout_secs: u64,
        #[arg(
            long,
            default_value_t = 260,
            help = "Maximum characters per cell value"
        )]
        show_value_chars: usize,
        #[command(flatten)]
        receipt_options: ReceiptOptions,
        #[command(flatten)]
        config_source: ConfigSource,
    },
    /// Summarize `SQLite` tables, columns, indexes, and foreign keys.
    #[command(name = "sqlite-schema")]
    SqliteSchema {
        #[arg(value_name = "DB", help = "SQLite database file")]
        path: PathBuf,
        #[arg(
            long = "table",
            value_name = "NAME",
            help = "Only summarize this table; repeatable"
        )]
        tables: Vec<String>,
        #[arg(
            long = "name-contains",
            value_name = "TEXT",
            help = "Only summarize tables whose names contain this text; repeatable, all must hold"
        )]
        name_contains: Vec<String>,
        #[arg(long, help = "Include virtual-table shadow tables")]
        with_shadow_tables: bool,
        #[arg(long, help = "Include sqlite_* system tables")]
        with_system_tables: bool,
        #[arg(long, default_value_t = 40, help = "Maximum tables to print")]
        show_tables: usize,
        #[arg(
            long,
            default_value_t = 160,
            help = "Maximum columns to print across all tables"
        )]
        show_columns: usize,
        #[arg(
            long,
            default_value_t = 120,
            help = "Maximum indexes to print across all tables"
        )]
        show_indexes: usize,
        #[arg(
            long,
            default_value_t = 320,
            help = "Maximum characters per printed schema line"
        )]
        show_line_chars: usize,
        #[command(flatten)]
        receipt_options: ReceiptOptions,
        #[command(flatten)]
        config_source: ConfigSource,
    },
    /// Install personal skills and a native runtime without changing consuming projects.
    #[command(
        after_help = "Writes a host-local ownership receipt and never edits repository guidance or harness settings. Always prints a JSON report."
    )]
    SetupUser {
        #[arg(
            long,
            value_name = "DIR",
            help = "Existing home directory to install into [default: USERPROFILE on Windows, HOME elsewhere]"
        )]
        home: Option<PathBuf>,
        #[arg(
            long,
            help = "Preflight and report every action without writing any file"
        )]
        dry_run: bool,
        #[arg(
            long,
            help = "Replace reviewed unowned or modified personal destinations; receipt-owned upgrades need no flag"
        )]
        replace_managed: bool,
    },
    /// Remove receipt-owned personal skills and runtime; leaves projects untouched.
    #[command(after_help = "Removes only receipt-owned files. Always prints a JSON report.")]
    UninstallUser {
        #[arg(
            long,
            value_name = "DIR",
            help = "Home directory whose personal receipt should be removed [default: USERPROFILE on Windows, HOME elsewhere]"
        )]
        home: Option<PathBuf>,
        #[arg(
            long,
            help = "Preflight and report every removal or refusal without writing any file"
        )]
        dry_run: bool,
    },
    /// Install optional project-owned integration for shared repository adoption
    SetupProject {
        #[arg(
            value_name = "PROJECT_ROOT",
            default_value = ".",
            help = "Existing repository root to equip with project-local contextmink entrypoints"
        )]
        project_root: PathBuf,
        #[arg(
            long,
            help = "Preflight and report every action without writing any file"
        )]
        dry_run: bool,
        #[arg(
            long,
            help = "Replace a reviewed modified or pre-receipt managed destination; receipt-owned upgrades need no flag and .contextmink.toml is always preserved"
        )]
        replace_managed: bool,
        #[arg(
            long,
            value_enum,
            default_value = "auto",
            help = "Select Contextmink skill discovery: auto preserves a receipt choice or detects existing harness markers once; explicit values reselect safely"
        )]
        skill_target: SkillTarget,
    },
    /// Remove receipt-owned project integration without touching repository-owned policy.
    #[command(
        after_help = "Run this command from an extracted Contextmink release outside the project."
    )]
    UninstallProject {
        #[arg(
            value_name = "PROJECT_ROOT",
            default_value = ".",
            help = "Existing repository root whose receipt-owned Contextmink integration should be removed"
        )]
        project_root: PathBuf,
        #[arg(
            long,
            help = "Preflight and report every removal or refusal without writing any file"
        )]
        dry_run: bool,
    },
    /// Execute argv non-interactively and print bounded stdout/stderr summaries.
    Capture {
        #[arg(
            long,
            default_value_t = 80,
            help = "Maximum stdout plus stderr lines to print"
        )]
        show_lines: usize,
        #[arg(
            long,
            default_value_t = 24_000,
            help = "Maximum bytes retained per stream (split between its head and tail)"
        )]
        show_bytes_per_stream: usize,
        #[arg(
            long,
            default_value_t = 260,
            help = "Maximum characters per printed output line"
        )]
        show_line_chars: usize,
        #[arg(
            long,
            help = "Execute the first argv item as an explicit Bash script; direct mode also recognizes shebang files by their leading #! line"
        )]
        script: bool,
        #[arg(
            long = "expect-exit",
            value_name = "CODE[,CODE...]",
            help = "Treat these child exit code(s) as expected; every other child status is propagated after the receipt is emitted. Repeatable and comma-separated values are accepted"
        )]
        expect_exit: Vec<String>,
        #[arg(
            long = "receipt-out",
            value_name = "FILE",
            help = "Write the full capture receipt JSON to this file after the child exits"
        )]
        receipt_out: Option<PathBuf>,
        #[arg(
            required = true,
            trailing_var_arg = true,
            allow_hyphen_values = true,
            help = "Non-interactive command argv to execute directly (child stdin is closed)"
        )]
        argv: Vec<String>,
        #[command(flatten)]
        receipt_options: ReceiptOptions,
        #[command(flatten)]
        config_source: ConfigSource,
    },
    /// Evaluate an agent `PreToolUse` hook payload (JSON on stdin) against the
    /// destructive-command guard; exit 2 blocks the tool call.
    GuardHook {
        #[arg(
            long = "command-field",
            default_value = crate::guard_hook::DEFAULT_COMMAND_FIELD,
            value_name = "POINTER",
            value_parser = crate::guard_hook::parse_command_field,
            help = "JSON Pointer to the command string in the hook payload (for example /tool_input/command)"
        )]
        command_field: String,
        #[arg(
            long = "expected-root",
            value_name = "DIR",
            help = "Apply this hook policy only when the hook payload cwd is inside DIR"
        )]
        expected_root: Option<PathBuf>,
        #[arg(
            long,
            value_enum,
            default_value_t = ShellDialect::Posix,
            help = "Shell dialect used by the intercepted command string"
        )]
        shell: ShellDialect,
        #[command(flatten)]
        config_source: ConfigSource,
    },
    /// Explain whether a direct argv or shell command would be allowed by the
    /// destructive-command guard. This command never spawns the input.
    GuardCheck {
        #[arg(
            long,
            value_name = "SHELL_COMMAND",
            conflicts_with = "argv",
            help = "Shell command text to parse and evaluate without executing"
        )]
        command: Option<String>,
        #[arg(
            long,
            value_enum,
            conflicts_with = "argv",
            help = "Shell dialect for --command (default: posix)"
        )]
        shell: Option<ShellDialect>,
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            help = "Already-tokenized argv to evaluate without executing"
        )]
        argv: Vec<String>,
        #[command(flatten)]
        config_source: ConfigSource,
    },
    /// Print a Claude settings JSON fragment that registers guard-hook.
    ///
    /// Only prints the fragment; merging it into a settings file is the
    /// caller's reviewed edit. Nothing is installed or modified.
    GuardHookSnippet {
        #[arg(
            long,
            value_name = "FILE",
            help = "contextmink executable path to register; defaults to the current executable"
        )]
        binary: Option<PathBuf>,
        #[arg(
            long = "guard-config",
            value_name = "FILE",
            help = "Config path passed to guard-hook; defaults to --config or discovered .contextmink.toml"
        )]
        guard_config: Option<PathBuf>,
        #[arg(
            long = "matcher",
            value_name = "TOOL",
            help = "Claude tool matcher to include; repeatable. Defaults to Bash and PowerShell."
        )]
        matchers: Vec<String>,
        #[arg(
            long = "command-field",
            default_value = crate::guard_hook::DEFAULT_COMMAND_FIELD,
            value_name = "POINTER",
            value_parser = crate::guard_hook::parse_command_field,
            help = "JSON Pointer to the command string in the hook payload (for example /tool_input/command)"
        )]
        command_field: String,
        #[command(flatten)]
        config_source: ConfigSource,
    },
}
