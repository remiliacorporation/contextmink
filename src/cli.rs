use std::ffi::OsString;
use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand, error::ErrorKind};

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
    "setup-project",
    "uninstall-project",
    "capture",
    "hook-guard",
    "guard-check",
    "hook-snippet",
];

#[derive(Debug, Parser)]
#[command(author, version, about)]
pub(crate) struct Cli {
    /// Emit one JSON object instead of human-readable rows plus a receipt line.
    #[arg(long, global = true)]
    pub(crate) json: bool,
    /// Exit nonzero after emitting a receipt if the command output was capped.
    #[arg(long, global = true)]
    pub(crate) fail_if_truncated: bool,
    /// Exit nonzero after emitting a receipt if the inspected evidence scope is incomplete.
    #[arg(long, global = true)]
    pub(crate) require_complete_scope: bool,
    /// Read configuration from this TOML file instead of searching upward.
    #[arg(long, global = true)]
    pub(crate) config: Option<PathBuf>,
    /// Ignore .contextmink.toml and use only built-in defaults.
    #[arg(long, global = true)]
    pub(crate) no_config: bool,
    #[command(subcommand)]
    pub(crate) command: Command,
}

pub(crate) fn parse_cli() -> Cli {
    let args = std::env::args_os().collect::<Vec<_>>();
    match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                ErrorKind::UnknownArgument
                    | ErrorKind::MissingRequiredArgument
                    | ErrorKind::ArgumentConflict
            ) && let Some(guidance) = noncanonical_form_guidance(&args)
            {
                Cli::command()
                    .error(ErrorKind::InvalidValue, guidance)
                    .exit();
            }
            error.exit()
        }
    }
}

pub(crate) fn noncanonical_form_guidance(args: &[OsString]) -> Option<&'static str> {
    let flag_present = |value: &str| {
        args.iter().any(|arg| {
            let arg = arg.to_string_lossy();
            arg == value || arg.starts_with(&format!("{value}="))
        })
    };
    let command = selected_subcommand(args)?;

    if command == "grep" && flag_present("--path") {
        return Some(
            "grep paths are positional; use `contextmink grep --pattern <PATTERN> <PATH>...`",
        );
    }
    if command == "grep" && !flag_present("--pattern") && !flag_present("--pattern-file") {
        return Some(
            "grep requires an explicit pattern; use `contextmink grep --pattern <PATTERN> <PATH>...` or `--pattern-file <FILE> <PATH>...`",
        );
    }
    if command == "slice" && flag_present("--start-line") {
        return Some("slice uses `--start <LINE>`; replace `--start-line` with `--start`");
    }
    if command == "slice" && flag_present("--end-line") {
        return Some("slice uses `--end <LINE>`; replace `--end-line` with `--end`");
    }
    if command == "outline" && flag_present("--max-items") {
        return Some("outline uses `--limit <COUNT>`; replace `--max-items` with `--limit`");
    }
    None
}

fn selected_subcommand(args: &[OsString]) -> Option<&str> {
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
        limit: usize,
        #[arg(
            long,
            default_value_t = 220,
            help = "Maximum characters per printed path"
        )]
        max_line_chars: usize,
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
        limit: usize,
        #[arg(
            long,
            default_value_t = 220,
            help = "Maximum characters per complete printed directory row"
        )]
        max_line_chars: usize,
        #[arg(
            long,
            default_value_t = 50_000,
            help = "Maximum candidate files to count into directory summaries"
        )]
        max_files_counted: usize,
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
        limit: usize,
        #[arg(
            long,
            default_value_t = 3,
            help = "Maximum sample lines per matching file"
        )]
        lines_per_file: usize,
        #[arg(
            long = "max-sample-lines",
            default_value_t = 36,
            help = "Maximum matching and context sample lines to print across all files"
        )]
        max_sample_lines: usize,
        #[arg(
            long,
            default_value_t = 220,
            help = "Maximum characters per sample line"
        )]
        max_line_chars: usize,
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
            help = "Maximum cumulative candidate bytes admitted for deterministic content inspection"
        )]
        max_content_bytes: Option<u64>,
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
        #[arg(value_name = "PATH", help = "Files or directories to search")]
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
        limit: usize,
        #[arg(
            long,
            default_value_t = 3,
            help = "Maximum sample lines per matching file"
        )]
        lines_per_file: usize,
        #[arg(
            long = "max-sample-lines",
            default_value_t = 36,
            help = "Maximum matching and context sample lines to print across all files"
        )]
        max_sample_lines: usize,
        #[arg(
            long,
            default_value_t = 220,
            help = "Maximum characters per sample line"
        )]
        max_line_chars: usize,
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
            help = "Maximum cumulative candidate bytes admitted for deterministic content inspection"
        )]
        max_content_bytes: Option<u64>,
    },
    /// Print a bounded line or character window from one text file.
    Slice {
        #[arg(value_name = "FILE", help = "Text file to slice")]
        file: PathBuf,
        #[arg(long, help = "One-based inclusive line range START:END")]
        range: Option<String>,
        #[arg(long, default_value_t = 1, help = "First one-based line to print")]
        start: usize,
        #[arg(long, help = "Last one-based line to print")]
        end: Option<usize>,
        #[arg(
            long,
            value_name = "N",
            help = "Print the last N lines instead of a start-anchored window"
        )]
        tail: Option<usize>,
        #[arg(
            long,
            default_value_t = 120,
            help = "Line count when --end/--range is omitted"
        )]
        lines: usize,
        #[arg(
            long,
            default_value_t = 220,
            help = "Maximum lines to print even if the range is larger"
        )]
        max_lines: usize,
        #[arg(
            long,
            default_value_t = 240,
            help = "Maximum characters per source-line text field"
        )]
        max_line_chars: usize,
        #[arg(
            long,
            conflicts_with_all = [
                "range",
                "start",
                "end",
                "tail",
                "lines",
                "max_lines",
                "max_line_chars"
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
        limit: usize,
        #[arg(
            long,
            default_value_t = 220,
            help = "Maximum characters per declaration text field"
        )]
        max_line_chars: usize,
    },
    /// Find JSON values by key, path, or summarized value predicates.
    JsonFind {
        #[arg(value_name = "FILE", help = "JSON or JSONL file to inspect")]
        file: PathBuf,
        #[arg(long, help = "Match object keys containing this text")]
        key_contains: Vec<String>,
        #[arg(long, help = "Match object keys with this regex")]
        key_regex: Option<String>,
        #[arg(long, help = "Match JSON paths containing this text")]
        path_contains: Vec<String>,
        #[arg(long, help = "Match JSON paths with this regex")]
        path_regex: Option<String>,
        #[arg(long, help = "Match summarized values containing this text")]
        value_contains: Vec<String>,
        #[arg(long, default_value_t = 40, help = "Maximum matches to print")]
        limit: usize,
        #[arg(
            long,
            default_value_t = 260,
            help = "Maximum characters per summarized value"
        )]
        max_value_chars: usize,
        #[arg(
            long,
            default_value_t = crate::json_input::DEFAULT_MAX_JSON_DOCUMENT_BYTES,
            help = "Maximum bytes materialized for one JSON document or retained for one JSONL record"
        )]
        max_document_bytes: u64,
    },
    /// Project JSON root or array rows to bounded field summaries.
    #[command(name = "json-select")]
    JsonSelect {
        #[arg(value_name = "FILE", help = "JSON or JSONL file to project")]
        file: PathBuf,
        #[arg(
            long,
            value_name = "KEY_OR_POINTER",
            help = "Top-level key or JSON Pointer to an array to project; omit for the root"
        )]
        array: Option<String>,
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
        #[arg(long, default_value_t = 40, help = "Maximum rows to print")]
        limit: usize,
        #[arg(
            long,
            default_value_t = 260,
            help = "Maximum characters per projected value"
        )]
        max_value_chars: usize,
        #[arg(
            long,
            default_value_t = crate::json_input::DEFAULT_MAX_JSON_DOCUMENT_BYTES,
            help = "Maximum bytes materialized for one JSON document or retained for one JSONL record"
        )]
        max_document_bytes: u64,
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
        limit: usize,
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
        max_value_chars: usize,
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
            help = "Only summarize tables whose names contain this text"
        )]
        name_contains: Vec<String>,
        #[arg(long, help = "Include SQLite shadow tables")]
        include_shadow: bool,
        #[arg(long, help = "Include SQLite system tables")]
        include_system: bool,
        #[arg(long, default_value_t = 40, help = "Maximum tables to print")]
        max_tables: usize,
        #[arg(
            long,
            default_value_t = 160,
            help = "Maximum columns to print across all tables"
        )]
        max_columns: usize,
        #[arg(
            long,
            default_value_t = 120,
            help = "Maximum indexes to print across all tables"
        )]
        max_indexes: usize,
        #[arg(
            long,
            default_value_t = 320,
            help = "Maximum characters per printed schema line"
        )]
        max_line_chars: usize,
    },
    /// Install a project-local release and report the agent-owned integration work.
    #[command(
        after_help = "Writes a platform-neutral ownership receipt and never edits repository guidance or harness settings. Only --json applies globally; receipt strictness and configuration-selection flags do not apply."
    )]
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
    },
    /// Remove receipt-owned project integration without touching repository-owned policy.
    #[command(
        after_help = "Run this command from an extracted Contextmink release outside the project. Only --json applies globally; receipt strictness and configuration-selection flags do not apply."
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
        max_lines: usize,
        #[arg(
            long,
            default_value_t = 24_000,
            help = "Maximum bytes to retain per stream"
        )]
        max_bytes: usize,
        #[arg(
            long,
            default_value_t = 260,
            help = "Maximum characters per printed output line"
        )]
        max_line_chars: usize,
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
    },
    /// Evaluate an agent `PreToolUse` hook payload (JSON on stdin) against the
    /// destructive-command guard; exit 2 blocks the tool call.
    HookGuard {
        #[arg(
            long = "command-field",
            default_value = "tool_input.command",
            value_name = "DOT.PATH",
            help = "Dot-separated JSON object path of the command string in the hook payload"
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
    },
    /// Print a Claude settings fragment that installs hook-guard safely.
    HookSnippet {
        #[arg(
            long,
            value_name = "FILE",
            help = "contextmink executable path to register; defaults to the current executable"
        )]
        binary: Option<PathBuf>,
        #[arg(
            long = "guard-config",
            value_name = "FILE",
            help = "Config path passed to hook-guard; defaults to --config or discovered .contextmink.toml"
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
            default_value = "tool_input.command",
            value_name = "DOT.PATH",
            help = "Dot-separated JSON object path of the command string in the hook payload"
        )]
        command_field: String,
    },
}
