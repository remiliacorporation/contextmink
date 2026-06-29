use std::cmp::min;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand, ValueEnum};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use regex::Regex;
use rusqlite::{Connection, OpenFlags, types::ValueRef};
use serde::Deserialize;
use serde_json::{Value, json};

const CONFIG_NAME: &str = ".contextmink.toml";
const RECEIPT_PREFIX: &str = "CONTEXTMINK_RECEIPT ";
const JSON_SMALL_NODE_LIMIT: usize = 80;
const JSON_SMALL_STRING_CHAR_LIMIT: usize = 4096;

const BUILTIN_EXCLUDES: &[&str] = &[
    ".git/**",
    "**/.git/**",
    "target/**",
    "**/target/**",
    "node_modules/**",
    "**/node_modules/**",
    ".venv/**",
    "**/.venv/**",
];

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    /// Exit nonzero after emitting a receipt if the command output was capped.
    #[arg(long, alias = "fail-on-truncated", global = true)]
    fail_if_truncated: bool,
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[arg(long, global = true)]
    no_config: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List candidate files with configured excludes and a display cap.
    Files {
        #[arg(default_value = ".")]
        path: Vec<PathBuf>,
        #[arg(long = "glob")]
        globs: Vec<String>,
        #[arg(long)]
        include_noisy: bool,
        #[arg(long, alias = "limit", default_value_t = 80)]
        max: usize,
        #[arg(long, default_value_t = 220)]
        max_line_chars: usize,
        #[arg(long, default_value_t = 50_000)]
        max_scan_files: usize,
    },
    /// Search text and report bounded file counts plus sample lines.
    ///
    /// Without --pattern-file, the first positional is PATTERN and the rest are
    /// paths. With --pattern-file, every positional is a path.
    Grep {
        #[arg(
            value_name = "PATTERN_OR_PATH",
            help = "PATTERN followed by optional PATHs, or only PATHs with --pattern-file"
        )]
        args: Vec<String>,
        #[arg(long = "pattern-file", value_name = "FILE")]
        pattern_file: Option<PathBuf>,
        #[arg(long)]
        literal: bool,
        #[arg(long)]
        include_noisy: bool,
        #[arg(long, default_value_t = 80)]
        max_count_files: usize,
        #[arg(long, default_value_t = 12)]
        max_files: usize,
        #[arg(long, default_value_t = 3)]
        lines_per_file: usize,
        #[arg(long, default_value_t = 36)]
        max_sample_lines: usize,
        #[arg(long, default_value_t = 220)]
        max_line_chars: usize,
        #[arg(long, default_value_t = 5000)]
        max_scan_files: usize,
        #[arg(long, default_value_t = 2_000_000)]
        max_file_bytes: u64,
    },
    /// Search for literal terms without regex or shell-fragile pattern syntax.
    #[command(name = "grep-terms")]
    GrepTerms {
        #[arg(long = "term")]
        terms: Vec<String>,
        #[arg(long = "term-file", value_name = "FILE")]
        term_files: Vec<PathBuf>,
        #[arg(long = "mode", value_enum, default_value_t = TermMode::All)]
        mode: TermMode,
        #[arg(value_name = "PATH")]
        paths: Vec<PathBuf>,
        #[arg(long = "path", value_name = "PATH")]
        path: Vec<PathBuf>,
        #[arg(long)]
        include_noisy: bool,
        #[arg(long, default_value_t = 80)]
        max_count_files: usize,
        #[arg(long, default_value_t = 12)]
        max_files: usize,
        #[arg(long, default_value_t = 3)]
        lines_per_file: usize,
        #[arg(long, default_value_t = 36)]
        max_sample_lines: usize,
        #[arg(long, default_value_t = 220)]
        max_line_chars: usize,
        #[arg(long, default_value_t = 5000)]
        max_scan_files: usize,
        #[arg(long, default_value_t = 2_000_000)]
        max_file_bytes: u64,
    },
    /// Print a bounded line or character window from one text file.
    Slice {
        file: PathBuf,
        #[arg(long)]
        range: Option<String>,
        #[arg(long, default_value_t = 1)]
        start: usize,
        #[arg(long)]
        end: Option<usize>,
        #[arg(long, default_value_t = 120)]
        lines: usize,
        #[arg(long, default_value_t = 220)]
        max_lines: usize,
        #[arg(long, default_value_t = 240)]
        max_line_chars: usize,
        #[arg(long)]
        char_start: Option<usize>,
        #[arg(long, default_value_t = 4000)]
        chars: usize,
    },
    /// Find JSON values by key, path, or summarized value predicates.
    JsonFind {
        file: PathBuf,
        #[arg(long)]
        key_contains: Vec<String>,
        #[arg(long)]
        key_regex: Option<String>,
        #[arg(long)]
        path_contains: Vec<String>,
        #[arg(long)]
        path_regex: Option<String>,
        #[arg(long)]
        value_contains: Vec<String>,
        #[arg(long, alias = "limit", default_value_t = 40)]
        max: usize,
        #[arg(long, default_value_t = 260)]
        max_value_chars: usize,
    },
    /// Project JSON root or array rows to bounded field summaries.
    #[command(name = "json-select")]
    JsonSelect {
        file: PathBuf,
        #[arg(long, value_name = "POINTER")]
        array: Option<String>,
        #[arg(long = "field", value_name = "KEY_OR_POINTER")]
        fields: Vec<String>,
        #[arg(long, alias = "limit", default_value_t = 40)]
        max: usize,
        #[arg(long, default_value_t = 260)]
        max_value_chars: usize,
    },
    /// Run a read-only SQLite query with bounded row output.
    Sqlite {
        db: PathBuf,
        #[arg(long)]
        sql: Option<String>,
        #[arg(long = "sql-file", value_name = "FILE")]
        sql_file: Option<PathBuf>,
        #[arg(long = "max-rows", alias = "limit", default_value_t = 40)]
        max_rows: usize,
        #[arg(long, default_value_t = 5000)]
        max_scan_rows: usize,
        #[arg(long, default_value_t = 260)]
        max_value_chars: usize,
    },
    /// Summarize SQLite tables, columns, indexes, and foreign keys.
    #[command(name = "sqlite-schema")]
    SqliteSchema {
        db: PathBuf,
        #[arg(long = "table", value_name = "NAME")]
        tables: Vec<String>,
        #[arg(long = "name-contains", value_name = "TEXT")]
        name_contains: Vec<String>,
        #[arg(long)]
        include_shadow: bool,
        #[arg(long)]
        include_system: bool,
        #[arg(long, default_value_t = 40)]
        max_tables: usize,
        #[arg(long, default_value_t = 160)]
        max_columns: usize,
        #[arg(long, default_value_t = 120)]
        max_indexes: usize,
        #[arg(long, default_value_t = 320)]
        max_line_chars: usize,
    },
    /// Execute argv directly and print bounded stdout/stderr summaries.
    #[command(visible_alias = "run")]
    Capture {
        #[arg(long, default_value_t = 80)]
        max_lines: usize,
        #[arg(long, default_value_t = 24_000)]
        max_bytes: usize,
        #[arg(long, default_value_t = 260)]
        max_line_chars: usize,
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        argv: Vec<String>,
    },
}

#[derive(Debug, Default, Deserialize)]
struct ContextminkConfig {
    profile: Option<String>,
    exclude_globs: Option<Vec<String>>,
}

struct ContextConfig {
    profile: Option<String>,
    excludes: GlobSet,
}

#[derive(Clone)]
enum TextMatcher {
    Literal(String),
    Regex(Regex),
    Terms { terms: Vec<String>, mode: TermMode },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum TermMode {
    All,
    Any,
}

impl TextMatcher {
    fn new(pattern: &str, literal: bool) -> Result<Self> {
        if literal {
            Ok(Self::Literal(pattern.to_owned()))
        } else {
            Ok(Self::Regex(Regex::new(pattern).with_context(|| {
                format!("invalid regex pattern: {pattern}")
            })?))
        }
    }

    fn is_match(&self, text: &str) -> bool {
        match self {
            Self::Literal(pattern) => text.contains(pattern),
            Self::Regex(pattern) => pattern.is_match(text),
            Self::Terms { terms, mode } => match mode {
                TermMode::All => terms.iter().all(|term| text.contains(term)),
                TermMode::Any => terms.iter().any(|term| text.contains(term)),
            },
        }
    }

    fn count_matches(&self, text: &str) -> usize {
        match self {
            Self::Literal(pattern) => {
                if pattern.is_empty() {
                    0
                } else {
                    text.matches(pattern).count()
                }
            }
            Self::Regex(pattern) => pattern.find_iter(text).count(),
            Self::Terms { .. } => text.lines().filter(|line| self.is_match(line)).count(),
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Literal(pattern) => format!("{pattern:?}"),
            Self::Regex(pattern) => format!("{:?}", pattern.as_str()),
            Self::Terms { terms, mode } => match mode {
                TermMode::All => format!("all_terms({})", terms.join(",")),
                TermMode::Any => format!("any_terms({})", terms.join(",")),
            },
        }
    }
}

#[derive(Debug)]
struct FileMatch {
    path: PathBuf,
    count: usize,
    samples: Vec<(usize, String)>,
}

#[derive(Debug)]
struct CollectedFiles {
    files: Vec<PathBuf>,
    total_seen: usize,
    truncated: bool,
}

#[derive(Debug)]
struct SqliteTableSummary {
    schema: String,
    name: String,
    kind: String,
    column_count_declared: i64,
    without_rowid: bool,
    strict: bool,
    columns: Vec<SqliteColumnSummary>,
    indexes: Vec<SqliteIndexSummary>,
    columns_total: usize,
    indexes_total: usize,
}

#[derive(Debug)]
struct SqliteColumnSummary {
    name: String,
    type_name: String,
    not_null: bool,
    default_value: Option<String>,
    primary_key_rank: i64,
    hidden: i64,
    foreign_key: Option<SqliteForeignKeySummary>,
}

#[derive(Clone, Debug)]
struct SqliteForeignKeySummary {
    table: String,
    column: String,
}

#[derive(Debug)]
struct SqliteIndexSummary {
    name: String,
    unique: bool,
    origin: String,
    partial: bool,
    columns: Vec<String>,
}

struct RawCapturedStream {
    bytes: Vec<u8>,
    total_bytes: usize,
    total_lines: usize,
    byte_truncated: bool,
}

struct CapturedStream {
    text: String,
    total_bytes: usize,
    captured_bytes: usize,
    total_lines: usize,
    shown_lines: usize,
    byte_truncated: bool,
    line_truncated: bool,
    char_truncated: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = load_context_config(cli.config.as_deref(), cli.no_config)?;
    match &cli.command {
        Command::Files {
            path,
            globs,
            include_noisy,
            max,
            max_line_chars,
            max_scan_files,
        } => command_files(
            &cli,
            &config,
            path,
            globs,
            *include_noisy,
            *max,
            *max_line_chars,
            *max_scan_files,
        ),
        Command::Grep {
            args,
            pattern_file,
            literal,
            include_noisy,
            max_count_files,
            max_files,
            lines_per_file,
            max_sample_lines,
            max_line_chars,
            max_scan_files,
            max_file_bytes,
        } => command_grep(
            &cli,
            &config,
            args,
            pattern_file.as_deref(),
            *literal,
            *include_noisy,
            *max_count_files,
            *max_files,
            *lines_per_file,
            *max_sample_lines,
            *max_line_chars,
            *max_scan_files,
            *max_file_bytes,
        ),
        Command::GrepTerms {
            terms,
            term_files,
            mode,
            paths,
            path,
            include_noisy,
            max_count_files,
            max_files,
            lines_per_file,
            max_sample_lines,
            max_line_chars,
            max_scan_files,
            max_file_bytes,
        } => {
            let terms = collect_terms(terms, term_files)?;
            command_grep_with_matcher(
                &cli,
                &config,
                "grep-terms",
                TextMatcher::Terms { terms, mode: *mode },
                &merged_paths(paths, path),
                *include_noisy,
                *max_count_files,
                *max_files,
                *lines_per_file,
                *max_sample_lines,
                *max_line_chars,
                *max_scan_files,
                *max_file_bytes,
            )
        }
        Command::Slice {
            file,
            range,
            start,
            end,
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
            *lines,
            *max_lines,
            *max_line_chars,
            *char_start,
            *chars,
        ),
        Command::JsonFind {
            file,
            key_contains,
            key_regex,
            path_contains,
            path_regex,
            value_contains,
            max,
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
            *max,
            *max_value_chars,
        ),
        Command::JsonSelect {
            file,
            array,
            fields,
            max,
            max_value_chars,
        } => command_json_select(
            &cli,
            &config,
            file,
            array.as_deref(),
            fields,
            *max,
            *max_value_chars,
        ),
        Command::Sqlite {
            db,
            sql,
            sql_file,
            max_rows,
            max_scan_rows,
            max_value_chars,
        } => command_sqlite(
            &cli,
            &config,
            db,
            sql.as_deref(),
            sql_file.as_deref(),
            *max_rows,
            *max_scan_rows,
            *max_value_chars,
        ),
        Command::SqliteSchema {
            db,
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
            db,
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
            argv,
        } => command_capture(&cli, &config, *max_lines, *max_bytes, *max_line_chars, argv),
    }
}

fn merged_paths(positional: &[PathBuf], named: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(positional.len() + named.len());
    paths.extend(positional.iter().cloned());
    paths.extend(named.iter().cloned());
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    paths
}

fn collect_terms(terms: &[String], term_files: &[PathBuf]) -> Result<Vec<String>> {
    let mut collected = terms.to_vec();
    for file in term_files {
        let text = fs::read_to_string(file)
            .with_context(|| format!("failed to read term file {}", file.display()))?;
        let text = strip_utf8_bom(&text);
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if !line.is_empty() {
                collected.push(line.to_owned());
            }
        }
    }
    if collected.is_empty() {
        return Err(anyhow!(
            "grep-terms requires at least one --term or --term-file entry"
        ));
    }
    Ok(collected)
}

fn command_capture(
    cli: &Cli,
    config: &ContextConfig,
    max_lines: usize,
    max_bytes: usize,
    max_line_chars: usize,
    argv: &[String],
) -> Result<()> {
    if max_lines == 0 {
        return Err(anyhow!("capture --max-lines must be greater than zero"));
    }
    if max_bytes == 0 {
        return Err(anyhow!("capture --max-bytes must be greater than zero"));
    }
    if max_line_chars == 0 {
        return Err(anyhow!(
            "capture --max-line-chars must be greater than zero"
        ));
    }
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| anyhow!("capture requires a command after --"))?;

    let started = Instant::now();
    let (mut child, effective_argv) = spawn_captured_child(program, args)?;

    let stdout_pipe = child
        .stdout
        .take()
        .context("failed to capture child stdout")?;
    let stderr_pipe = child
        .stderr
        .take()
        .context("failed to capture child stderr")?;
    let stdout_handle = thread::spawn(move || read_captured_stream(stdout_pipe, max_bytes));
    let stderr_handle = thread::spawn(move || read_captured_stream(stderr_pipe, max_bytes));
    let status = child
        .wait()
        .context("failed to wait for captured command")?;
    let stdout_raw = stdout_handle
        .join()
        .map_err(|_| anyhow!("stdout capture thread panicked"))?
        .context("failed to read captured stdout")?;
    let stderr_raw = stderr_handle
        .join()
        .map_err(|_| anyhow!("stderr capture thread panicked"))?
        .context("failed to read captured stderr")?;
    let stdout = render_captured_stream(stdout_raw, max_lines, max_line_chars);
    let stderr = render_captured_stream(stderr_raw, max_lines, max_line_chars);
    let duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let shown = stdout.shown_lines + stderr.shown_lines;
    let total = stdout.total_lines + stderr.total_lines;
    let truncated = captured_stream_truncated(&stdout) || captured_stream_truncated(&stderr);
    let cap_reason = capture_cap_reason(&stdout, &stderr);

    let mut map = base_receipt(
        "capture",
        config.profile.as_deref(),
        "lines",
        shown,
        total,
        truncated,
        cap_reason,
    );
    map.insert("argv".to_string(), json!(argv));
    map.insert("effective_argv".to_string(), json!(effective_argv));
    map.insert(
        "spawn_fallback".to_string(),
        json!(effective_argv.as_ref().map(|_| "bash")),
    );
    map.insert("exit_code".to_string(), json!(status.code()));
    map.insert("success".to_string(), json!(status.success()));
    map.insert("duration_ms".to_string(), json!(duration_ms));
    map.insert("stdout".to_string(), captured_stream_json(&stdout));
    map.insert("stderr".to_string(), captured_stream_json(&stderr));

    if cli.json {
        let mut object = map;
        object.insert("stdout_text".to_string(), json!(stdout.text));
        object.insert("stderr_text".to_string(), json!(stderr.text));
        return emit_json(Value::Object(object));
    }

    let mut out = io::stdout();
    writeln!(
        out,
        "[contextmink] capture command={} exit_code={} success={} duration_ms={}",
        clamp_text(&format!("{argv:?}"), 500),
        status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "null".to_string()),
        status.success(),
        duration_ms
    )?;
    if let Some(effective_argv) = &effective_argv {
        writeln!(
            out,
            "spawn_fallback=bash effective_command={}",
            clamp_text(&format!("{effective_argv:?}"), 500)
        )?;
    }
    writeln!(
        out,
        "stdout: shown_lines={} total_lines={} captured_bytes={} total_bytes={}",
        stdout.shown_lines, stdout.total_lines, stdout.captured_bytes, stdout.total_bytes
    )?;
    if !stdout.text.is_empty() {
        writeln!(out, "{}", stdout.text)?;
    }
    writeln!(
        out,
        "stderr: shown_lines={} total_lines={} captured_bytes={} total_bytes={}",
        stderr.shown_lines, stderr.total_lines, stderr.captured_bytes, stderr.total_bytes
    )?;
    if !stderr.text.is_empty() {
        writeln!(out, "{}", stderr.text)?;
    }
    if truncated {
        writeln!(
            out,
            "[contextmink] capped captured output; rerun the underlying command with native filters or raise caps only after confirming the output is useful."
        )?;
    }
    write_receipt_checked(cli, map)
}

fn spawn_captured_child(
    program: &str,
    args: &[String],
) -> Result<(std::process::Child, Option<Vec<String>>)> {
    let mut command = ProcessCommand::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match command.spawn() {
        Ok(child) => Ok((child, None)),
        Err(error) if cfg!(windows) && error.raw_os_error() == Some(193) => {
            let Some(bash) = std::env::var_os("CONTEXTMINK_BASH") else {
                return Err(error)
                    .with_context(|| format!("failed to spawn captured command {program:?}"));
            };
            let mut effective_argv = Vec::with_capacity(args.len() + 2);
            effective_argv.push(bash.to_string_lossy().into_owned());
            effective_argv.push(program.to_owned());
            effective_argv.extend(args.iter().cloned());

            let mut fallback = ProcessCommand::new(&bash);
            fallback
                .arg(program)
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let child = fallback.spawn().with_context(|| {
                format!(
                    "failed to spawn captured command {program:?} through CONTEXTMINK_BASH={}",
                    bash.to_string_lossy()
                )
            })?;
            Ok((child, Some(effective_argv)))
        }
        Err(error) => {
            Err(error).with_context(|| format!("failed to spawn captured command {program:?}"))
        }
    }
}

fn read_captured_stream<R: Read>(mut reader: R, max_bytes: usize) -> io::Result<RawCapturedStream> {
    let mut bytes = Vec::with_capacity(max_bytes.min(8192));
    let mut total_bytes = 0usize;
    let mut newline_count = 0usize;
    let mut saw_any = false;
    let mut last_was_newline = false;
    let mut buffer = [0u8; 8192];

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        saw_any = true;
        total_bytes += read;
        for byte in &buffer[..read] {
            if *byte == b'\n' {
                newline_count += 1;
                last_was_newline = true;
            } else {
                last_was_newline = false;
            }
        }
        let remaining = max_bytes.saturating_sub(bytes.len());
        if remaining > 0 {
            bytes.extend_from_slice(&buffer[..read.min(remaining)]);
        }
    }

    let total_lines = newline_count + usize::from(saw_any && !last_was_newline);
    let byte_truncated = total_bytes > bytes.len();
    Ok(RawCapturedStream {
        bytes,
        total_bytes,
        total_lines,
        byte_truncated,
    })
}

fn render_captured_stream(
    raw: RawCapturedStream,
    max_lines: usize,
    max_line_chars: usize,
) -> CapturedStream {
    let decoded = String::from_utf8_lossy(&raw.bytes);
    let mut lines = Vec::new();
    let mut char_truncated = false;
    for line in decoded.lines().take(max_lines) {
        let line = line.trim_end_matches('\r');
        if line.chars().count() > max_line_chars {
            char_truncated = true;
        }
        lines.push(clamp_text(line, max_line_chars));
    }
    let shown_lines = lines.len();
    CapturedStream {
        text: lines.join("\n"),
        total_bytes: raw.total_bytes,
        captured_bytes: raw.bytes.len(),
        total_lines: raw.total_lines,
        shown_lines,
        byte_truncated: raw.byte_truncated,
        line_truncated: raw.total_lines > shown_lines,
        char_truncated,
    }
}

fn captured_stream_truncated(stream: &CapturedStream) -> bool {
    stream.byte_truncated || stream.line_truncated || stream.char_truncated
}

fn capture_cap_reason(stdout: &CapturedStream, stderr: &CapturedStream) -> Option<&'static str> {
    if stdout.byte_truncated || stderr.byte_truncated {
        Some("bytes")
    } else if stdout.line_truncated || stderr.line_truncated {
        Some("lines")
    } else if stdout.char_truncated || stderr.char_truncated {
        Some("chars")
    } else {
        None
    }
}

fn captured_stream_json(stream: &CapturedStream) -> Value {
    json!({
        "shown_lines": stream.shown_lines,
        "total_lines": stream.total_lines,
        "captured_bytes": stream.captured_bytes,
        "total_bytes": stream.total_bytes,
        "truncated": captured_stream_truncated(stream),
        "byte_truncated": stream.byte_truncated,
        "line_truncated": stream.line_truncated,
        "char_truncated": stream.char_truncated,
    })
}

fn collect_single_text_source(
    label: &str,
    inline: Option<&str>,
    file: Option<&Path>,
    trim_terminal_newlines: bool,
) -> Result<String> {
    match (inline, file) {
        (Some(_), Some(_)) => Err(anyhow!(
            "{label} accepts either an inline value or a file, not both"
        )),
        (Some(value), None) => Ok(value.to_owned()),
        (None, Some(path)) => {
            let mut text = if path == Path::new("-") {
                let mut text = String::new();
                io::stdin()
                    .read_to_string(&mut text)
                    .with_context(|| format!("failed to read {label} from stdin"))?;
                text
            } else {
                fs::read_to_string(path)
                    .with_context(|| format!("failed to read {label} file {}", path.display()))?
            };
            if trim_terminal_newlines {
                trim_trailing_line_endings(&mut text);
            }
            Ok(strip_utf8_bom(&text).to_owned())
        }
        (None, None) => Err(anyhow!("{label} requires an inline value or file")),
    }
}

fn strip_utf8_bom(value: &str) -> &str {
    value.strip_prefix('\u{feff}').unwrap_or(value)
}

fn trim_trailing_line_endings(value: &mut String) {
    while value.ends_with('\n') || value.ends_with('\r') {
        value.pop();
    }
}

fn parse_line_range(range: &str) -> Result<(usize, Option<usize>)> {
    let (start, end) = range
        .split_once(':')
        .ok_or_else(|| anyhow!("slice --range must use START:END line numbers"))?;
    if start.is_empty() || end.is_empty() {
        return Err(anyhow!("slice --range requires both START and END"));
    }
    let start = start
        .parse::<usize>()
        .with_context(|| format!("invalid slice --range start: {start}"))?;
    let end = end
        .parse::<usize>()
        .with_context(|| format!("invalid slice --range end: {end}"))?;
    if start == 0 || end == 0 {
        return Err(anyhow!("slice --range is 1-based; line 0 is invalid"));
    }
    if end < start {
        return Err(anyhow!(
            "slice --range end must be greater than or equal to start"
        ));
    }
    Ok((start, Some(end)))
}

#[allow(clippy::too_many_arguments)]
fn command_files(
    cli: &Cli,
    config: &ContextConfig,
    paths: &[PathBuf],
    globs: &[String],
    include_noisy: bool,
    max: usize,
    max_line_chars: usize,
    max_scan_files: usize,
) -> Result<()> {
    if max_scan_files == 0 {
        return Err(anyhow!("files --max-scan-files must be greater than zero"));
    }
    let collected = collect_files(paths, globs, config, include_noisy, max_scan_files)?;
    let files = collected.files;
    let shown = min(files.len(), max);
    let truncated = collected.truncated || shown < files.len();
    let cap_reason = if collected.truncated {
        Some("scan")
    } else if shown < files.len() {
        Some("max")
    } else {
        None
    };
    if cli.json {
        let mut map = base_receipt(
            "files",
            config.profile.as_deref(),
            "files",
            shown,
            collected.total_seen,
            truncated,
            cap_reason,
        );
        map.insert("candidate_files_scanned".to_string(), json!(files.len()));
        map.insert(
            "candidate_files_total_is_lower_bound".to_string(),
            json!(collected.truncated),
        );
        map.insert(
            "files".to_string(),
            json!(
                files
                    .iter()
                    .take(shown)
                    .map(|path| display_path(path))
                    .collect::<Vec<_>>()
            ),
        );
        emit_json_checked(cli, Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        for path in files.iter().take(shown) {
            writeln!(
                stdout,
                "{}",
                clamp_text(&display_path(path), max_line_chars)
            )?;
        }
        if collected.truncated {
            writeln!(
                stdout,
                "[contextmink] capped file scan at {max_scan_files} files; narrow the path or glob before treating this as complete."
            )?;
        }
        let mut map = base_receipt(
            "files",
            config.profile.as_deref(),
            "files",
            shown,
            collected.total_seen,
            truncated,
            cap_reason,
        );
        map.insert("candidate_files_scanned".to_string(), json!(files.len()));
        map.insert(
            "candidate_files_total_is_lower_bound".to_string(),
            json!(collected.truncated),
        );
        write_receipt_checked(cli, map)
    }
}

#[allow(clippy::too_many_arguments)]
fn command_grep(
    cli: &Cli,
    config: &ContextConfig,
    args: &[String],
    pattern_file: Option<&Path>,
    literal: bool,
    include_noisy: bool,
    max_count_files: usize,
    max_files: usize,
    lines_per_file: usize,
    max_sample_lines: usize,
    max_line_chars: usize,
    max_scan_files: usize,
    max_file_bytes: u64,
) -> Result<()> {
    let (pattern, effective_paths) = if pattern_file.is_some() {
        (None, string_args_to_paths(args))
    } else {
        let Some((pattern, paths)) = args.split_first() else {
            return Err(anyhow!("grep requires PATTERN or --pattern-file <file>"));
        };
        (Some(pattern.as_str()), string_args_to_paths(paths))
    };
    let pattern = collect_single_text_source("grep pattern", pattern, pattern_file, true)?;
    let matcher = TextMatcher::new(&pattern, literal)?;
    command_grep_with_matcher(
        cli,
        config,
        "grep",
        matcher,
        &effective_paths,
        include_noisy,
        max_count_files,
        max_files,
        lines_per_file,
        max_sample_lines,
        max_line_chars,
        max_scan_files,
        max_file_bytes,
    )
}

fn string_args_to_paths(args: &[String]) -> Vec<PathBuf> {
    if args.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.iter().map(PathBuf::from).collect()
    }
}

#[allow(clippy::too_many_arguments)]
fn command_grep_with_matcher(
    cli: &Cli,
    config: &ContextConfig,
    command_name: &str,
    matcher: TextMatcher,
    paths: &[PathBuf],
    include_noisy: bool,
    max_count_files: usize,
    max_files: usize,
    lines_per_file: usize,
    max_sample_lines: usize,
    max_line_chars: usize,
    max_scan_files: usize,
    max_file_bytes: u64,
) -> Result<()> {
    if max_scan_files == 0 {
        return Err(anyhow!(
            "{command_name} --max-scan-files must be greater than zero"
        ));
    }
    let collected = collect_files(paths, &[], config, include_noisy, max_scan_files)?;
    let scan_truncated = collected.truncated;
    let total_candidate_files = collected.total_seen;
    let candidate_files_scanned = collected.files.len();
    let mut matches = Vec::new();
    let mut total_matches = 0usize;
    let mut skipped_large_or_binary = 0usize;
    for file in collected.files {
        let Some(text) = read_text_file(&file, max_file_bytes)? else {
            skipped_large_or_binary += 1;
            continue;
        };
        let count = matcher.count_matches(&text);
        if count == 0 {
            continue;
        }
        total_matches += count;
        let mut samples = Vec::new();
        for (index, line) in text.lines().enumerate() {
            if samples.len() >= lines_per_file {
                break;
            }
            if matcher.is_match(line) {
                samples.push((index + 1, clamp_text(line.trim(), max_line_chars)));
            }
        }
        matches.push(FileMatch {
            path: file,
            count,
            samples,
        });
    }
    let count_shown = min(matches.len(), max_count_files);
    let files_shown = min(count_shown, max_files);
    if cli.json {
        let mut sample_lines_shown = 0usize;
        let mut sample_capped = false;
        let mut files_json = Vec::new();
        for row in matches.iter().take(files_shown) {
            let mut samples = Vec::new();
            for (line, text) in &row.samples {
                if sample_lines_shown >= max_sample_lines {
                    sample_capped = true;
                    break;
                }
                sample_lines_shown += 1;
                samples.push(json!({
                    "line": line,
                    "text": text,
                }));
            }
            files_json.push(json!({
                "path": display_path(&row.path),
                "count": row.count,
                "samples": samples,
            }));
        }
        let cap_reason = if scan_truncated {
            Some("scan")
        } else if files_shown < matches.len() {
            Some("files")
        } else if sample_capped {
            Some("samples")
        } else {
            None
        };
        let mut map = base_receipt(
            command_name,
            config.profile.as_deref(),
            "files",
            files_shown,
            matches.len(),
            cap_reason.is_some(),
            cap_reason,
        );
        map.insert("pattern".to_string(), json!(matcher.label()));
        map.insert("matched_files_total".to_string(), json!(matches.len()));
        map.insert("matched_files_shown".to_string(), json!(files_shown));
        map.insert("total_matches".to_string(), json!(total_matches));
        map.insert("sample_lines_shown".to_string(), json!(sample_lines_shown));
        map.insert(
            "candidate_files_total".to_string(),
            json!(total_candidate_files),
        );
        map.insert(
            "candidate_files_scanned".to_string(),
            json!(candidate_files_scanned),
        );
        map.insert(
            "candidate_files_total_is_lower_bound".to_string(),
            json!(scan_truncated),
        );
        map.insert(
            "skipped_large_or_binary".to_string(),
            json!(skipped_large_or_binary),
        );
        map.insert("files".to_string(), json!(files_json));
        emit_json_checked(cli, Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        writeln!(stdout, "[contextmink] grep pattern={}", matcher.label())?;
        writeln!(
            stdout,
            "matched_files_total={} matched_files_counted={} total_matches={}",
            matches.len(),
            count_shown,
            total_matches
        )?;
        writeln!(
            stdout,
            "candidate_files_total={} candidate_files_scanned={} skipped_large_or_binary={}",
            total_candidate_files, candidate_files_scanned, skipped_large_or_binary
        )?;
        if matches.is_empty() {
            writeln!(stdout, "no_matches")?;
            if scan_truncated {
                writeln!(
                    stdout,
                    "[contextmink] capped grep scan; no matches were found in the scanned subset."
                )?;
            }
            let cap_reason = if scan_truncated { Some("scan") } else { None };
            return emit_grep_receipt(
                cli,
                command_name,
                config,
                0,
                0,
                total_matches,
                0,
                total_candidate_files,
                candidate_files_scanned,
                skipped_large_or_binary,
                scan_truncated,
                cap_reason,
            );
        }
        writeln!(stdout, "file_counts:")?;
        for row in matches.iter().take(files_shown) {
            writeln!(
                stdout,
                "  {}:{}",
                clamp_text(&display_path(&row.path), max_line_chars),
                row.count
            )?;
        }
        let mut sample_total = 0usize;
        let mut sample_capped = false;
        if lines_per_file > 0 && files_shown > 0 {
            writeln!(stdout, "sample_lines:")?;
            'samples: for row in matches.iter().take(files_shown) {
                for (line, text) in &row.samples {
                    if sample_total >= max_sample_lines {
                        writeln!(
                            stdout,
                            "[contextmink] capped sample lines at {max_sample_lines}; narrow the query."
                        )?;
                        sample_capped = true;
                        break 'samples;
                    }
                    writeln!(
                        stdout,
                        "  {}:{}:{}",
                        clamp_text(&display_path(&row.path), max_line_chars),
                        line,
                        text
                    )?;
                    sample_total += 1;
                }
            }
        }
        let cap_reason = if scan_truncated {
            Some("scan")
        } else if files_shown < matches.len() {
            Some("files")
        } else if sample_capped {
            Some("samples")
        } else {
            None
        };
        if matches!(cap_reason, Some("scan") | Some("files")) {
            writeln!(
                stdout,
                "[contextmink] capped grep output or scan; narrow the path or pattern before treating this as complete."
            )?;
        }
        emit_grep_receipt(
            cli,
            command_name,
            config,
            files_shown,
            matches.len(),
            total_matches,
            sample_total,
            total_candidate_files,
            candidate_files_scanned,
            skipped_large_or_binary,
            cap_reason.is_some(),
            cap_reason,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn command_slice(
    cli: &Cli,
    config: &ContextConfig,
    file: &Path,
    range: Option<&str>,
    start: usize,
    end: Option<usize>,
    lines: usize,
    max_lines: usize,
    max_line_chars: usize,
    char_start: Option<usize>,
    chars: usize,
) -> Result<()> {
    let text =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    if let Some(char_start) = char_start {
        if range.is_some() {
            return Err(anyhow!(
                "slice --range cannot be combined with --char-start"
            ));
        }
        let total_chars = text.chars().count();
        let shown_text = text
            .chars()
            .skip(char_start)
            .take(chars)
            .collect::<String>();
        let shown = shown_text.chars().count();
        let truncated = char_start + shown < total_chars;
        let cap_reason = if truncated { Some("max_chars") } else { None };
        if cli.json {
            let mut map = base_receipt(
                "slice",
                config.profile.as_deref(),
                "chars",
                shown,
                total_chars,
                truncated,
                cap_reason,
            );
            map.insert("path".to_string(), json!(display_path(file)));
            map.insert("mode".to_string(), json!("chars"));
            map.insert("char_start".to_string(), json!(char_start));
            map.insert("chars_shown".to_string(), json!(shown));
            map.insert("total_chars".to_string(), json!(total_chars));
            map.insert("text".to_string(), json!(shown_text));
            return emit_json_checked(cli, Value::Object(map));
        }
        let mut stdout = io::stdout();
        write!(stdout, "{}", shown_text)?;
        if !shown_text.ends_with('\n') {
            writeln!(stdout)?;
        }
        return write_receipt_checked(
            cli,
            base_receipt(
                "slice",
                config.profile.as_deref(),
                "chars",
                shown_text.chars().count(),
                total_chars,
                truncated,
                cap_reason,
            ),
        );
    }
    let (start, end) = if let Some(range) = range {
        if start != 1 || end.is_some() {
            return Err(anyhow!(
                "slice --range cannot be combined with --start or --end"
            ));
        }
        parse_line_range(range)?
    } else {
        (start.max(1), end)
    };
    let requested_end = end.unwrap_or(start.saturating_add(lines).saturating_sub(1));
    let capped_end = min(
        requested_end,
        start.saturating_add(max_lines).saturating_sub(1),
    );
    let text_lines = text.lines().collect::<Vec<_>>();
    let total_lines = text_lines.len();
    let mut rendered = Vec::new();
    for number in start..=capped_end {
        if let Some(line) = text_lines.get(number - 1) {
            rendered.push((number, clamp_text(line, max_line_chars)));
        }
    }
    let last_available = min(requested_end, total_lines);
    let truncated = start <= total_lines && last_available > capped_end;
    let shown = if start > total_lines {
        0
    } else {
        min(capped_end, total_lines).saturating_sub(start) + 1
    };
    let displayed_end = if shown == 0 {
        start.saturating_sub(1)
    } else {
        min(capped_end, total_lines)
    };
    let cap_reason = if truncated { Some("max_lines") } else { None };
    if cli.json {
        let mut map = base_receipt(
            "slice",
            config.profile.as_deref(),
            "lines",
            shown,
            total_lines,
            truncated,
            cap_reason,
        );
        map.insert("path".to_string(), json!(display_path(file)));
        map.insert("mode".to_string(), json!("lines"));
        map.insert("start".to_string(), json!(start));
        map.insert("end".to_string(), json!(displayed_end));
        map.insert("total_lines".to_string(), json!(total_lines));
        map.insert(
            "lines".to_string(),
            json!(
                rendered
                    .iter()
                    .map(|(line, text)| json!({
                        "line": line,
                        "text": text,
                    }))
                    .collect::<Vec<_>>()
            ),
        );
        emit_json_checked(cli, Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        for (line, text) in rendered {
            writeln!(stdout, "{line}: {text}")?;
        }
        if truncated {
            writeln!(
                stdout,
                "[contextmink] capped slice at {max_lines} lines; request a narrower range."
            )?;
        }
        write_receipt_checked(
            cli,
            base_receipt(
                "slice",
                config.profile.as_deref(),
                "lines",
                shown,
                total_lines,
                truncated,
                cap_reason,
            ),
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn command_json_find(
    cli: &Cli,
    config: &ContextConfig,
    file: &Path,
    key_contains: &[String],
    key_regex: Option<&str>,
    path_contains: &[String],
    path_regex: Option<&str>,
    value_contains: &[String],
    max: usize,
    max_value_chars: usize,
) -> Result<()> {
    if key_contains.is_empty()
        && key_regex.is_none()
        && path_contains.is_empty()
        && path_regex.is_none()
        && value_contains.is_empty()
    {
        return Err(anyhow!(
            "json-find requires --key-contains, --key-regex, --path-contains, --path-regex, or --value-contains"
        ));
    }
    let key_re = key_regex
        .map(Regex::new)
        .transpose()
        .context("invalid key regex")?;
    let path_re = path_regex
        .map(Regex::new)
        .transpose()
        .context("invalid path regex")?;
    let document =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    let (document, input_format) = parse_json_or_jsonl(&document)?;
    let mut rows = Vec::new();
    let mut total_matches = 0usize;
    walk_json("$", None, &document, &mut |path, key, value| {
        if let Some(key_re) = &key_re
            && !key.is_some_and(|key| key_re.is_match(key))
        {
            return;
        }
        if !key_contains.is_empty() && !key.is_some_and(|key| contains_any(key, key_contains)) {
            return;
        }
        if let Some(path_re) = &path_re
            && !path_re.is_match(path)
        {
            return;
        }
        if !path_contains.is_empty() && !contains_any(path, path_contains) {
            return;
        }
        let summary = value_summary(value, max_value_chars);
        if !value_contains.is_empty() && !contains_any(&summary, value_contains) {
            return;
        }
        total_matches += 1;
        if rows.len() < max {
            rows.push((path.to_owned(), summary));
        }
    });
    let shown = rows.len();
    let truncated = shown < total_matches;
    let cap_reason = if truncated { Some("max") } else { None };
    if cli.json {
        let mut map = base_receipt(
            "json-find",
            config.profile.as_deref(),
            "matches",
            shown,
            total_matches,
            truncated,
            cap_reason,
        );
        map.insert("path".to_string(), json!(display_path(file)));
        map.insert("input_format".to_string(), json!(input_format));
        map.insert(
            "matches".to_string(),
            json!(
                rows.iter()
                    .take(shown)
                    .map(|(path, value)| json!({
                        "path": path,
                        "value": value,
                    }))
                    .collect::<Vec<_>>()
            ),
        );
        emit_json_checked(cli, Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        if rows.is_empty() {
            writeln!(stdout, "no_matches")?;
        }
        for (path, value) in rows.iter().take(shown) {
            writeln!(stdout, "{path} = {value}")?;
        }
        if truncated {
            writeln!(
                stdout,
                "[contextmink] capped json matches at {max}; narrow the selector."
            )?;
        }
        write_receipt_checked(
            cli,
            base_receipt(
                "json-find",
                config.profile.as_deref(),
                "matches",
                shown,
                total_matches,
                truncated,
                cap_reason,
            ),
        )
    }
}

fn parse_json_or_jsonl(text: &str) -> Result<(Value, &'static str)> {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => Ok((value, "json")),
        Err(json_error) => {
            let whole_document_error = json_error.to_string();
            let mut rows = Vec::new();
            let mut saw_line = false;
            for (index, line) in text.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                saw_line = true;
                let value: Value = serde_json::from_str(trimmed).with_context(|| {
                    format!(
                        "failed to parse JSON (whole document: {whole_document_error}); \
                             failed to parse JSONL line {}",
                        index + 1
                    )
                })?;
                rows.push(value);
            }
            if saw_line {
                Ok((Value::Array(rows), "jsonl"))
            } else {
                Err(json_error).context("failed to parse JSON")
            }
        }
    }
}

fn command_json_select(
    cli: &Cli,
    config: &ContextConfig,
    file: &Path,
    array: Option<&str>,
    fields: &[String],
    max: usize,
    max_value_chars: usize,
) -> Result<()> {
    if max == 0 {
        return Err(anyhow!("json-select --max must be greater than zero"));
    }
    let document =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    let (document, input_format) = parse_json_or_jsonl(&document)?;
    let rows: Vec<&Value> = if let Some(pointer) = array {
        let selected = json_pointer_lookup(&document, pointer)?
            .ok_or_else(|| anyhow!("json-select --array pointer did not match: {pointer}"))?;
        selected
            .as_array()
            .ok_or_else(|| {
                anyhow!("json-select --array pointer must resolve to an array: {pointer}")
            })?
            .iter()
            .collect()
    } else if input_format == "jsonl" {
        document
            .as_array()
            .expect("JSONL parser returns an array")
            .iter()
            .collect()
    } else {
        vec![&document]
    };
    let shown = min(rows.len(), max);
    let truncated = shown < rows.len();
    let cap_reason = if truncated { Some("max") } else { None };
    if cli.json {
        let mut map = base_receipt(
            "json-select",
            config.profile.as_deref(),
            "rows",
            shown,
            rows.len(),
            truncated,
            cap_reason,
        );
        map.insert("path".to_string(), json!(display_path(file)));
        map.insert("array".to_string(), json!(array));
        map.insert("input_format".to_string(), json!(input_format));
        map.insert("fields".to_string(), json!(fields));
        map.insert(
            "rows".to_string(),
            json!(
                rows.iter()
                    .take(shown)
                    .enumerate()
                    .map(|(index, row)| json_select_row(index, row, fields, max_value_chars))
                    .collect::<Result<Vec<_>>>()?
            ),
        );
        emit_json_checked(cli, Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        let source = array.unwrap_or(if input_format == "jsonl" {
            "jsonl"
        } else {
            "$"
        });
        if fields.is_empty() {
            writeln!(stdout, "[contextmink] json-select source={source}")?;
        } else {
            writeln!(
                stdout,
                "[contextmink] json-select source={source} fields={}",
                fields.join(",")
            )?;
        }
        if rows.is_empty() {
            writeln!(stdout, "no_rows")?;
        }
        for (index, row) in rows.iter().take(shown).enumerate() {
            if fields.is_empty() {
                writeln!(stdout, "{index}: {}", value_summary(row, max_value_chars))?;
                continue;
            }
            let mut parts = Vec::with_capacity(fields.len());
            for field in fields {
                let summary = json_select_field(row, field)?
                    .map(|value| value_summary(value, max_value_chars))
                    .unwrap_or_else(|| "null".to_owned());
                parts.push(format!("{field}={summary}"));
            }
            writeln!(stdout, "{index}: {}", parts.join(" "))?;
        }
        if truncated {
            writeln!(
                stdout,
                "[contextmink] capped json rows at {max}; narrow the selector."
            )?;
        }
        write_receipt_checked(
            cli,
            base_receipt(
                "json-select",
                config.profile.as_deref(),
                "rows",
                shown,
                rows.len(),
                truncated,
                cap_reason,
            ),
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn command_sqlite(
    cli: &Cli,
    config: &ContextConfig,
    db: &Path,
    sql: Option<&str>,
    sql_file: Option<&Path>,
    max_rows: usize,
    max_scan_rows: usize,
    max_value_chars: usize,
) -> Result<()> {
    if max_rows == 0 {
        return Err(anyhow!("sqlite --max-rows must be greater than zero"));
    }
    if max_scan_rows == 0 {
        return Err(anyhow!("sqlite --max-scan-rows must be greater than zero"));
    }
    if max_scan_rows < max_rows {
        return Err(anyhow!(
            "sqlite --max-scan-rows must be greater than or equal to --max-rows"
        ));
    }
    let sql = collect_single_text_source("sqlite SQL", sql, sql_file, false)?;
    if sql.trim().is_empty() {
        return Err(anyhow!("sqlite SQL must not be empty"));
    }
    let conn = open_sqlite_readonly(db)?;
    let mut stmt = conn.prepare(&sql).context("failed to prepare sqlite SQL")?;
    if stmt.parameter_count() != 0 {
        return Err(anyhow!(
            "sqlite command does not bind parameters; use a literal read-only query"
        ));
    }
    if !stmt.readonly() {
        return Err(anyhow!("sqlite command only accepts read-only statements"));
    }
    let column_count = stmt.column_count();
    let columns = stmt
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut row_iter = stmt.query([]).context("failed to run sqlite query")?;
    let mut rendered_rows = Vec::new();
    let mut json_rows = Vec::new();
    let mut total_seen = 0usize;
    let mut scan_truncated = false;
    while let Some(row) = row_iter.next().context("failed to read sqlite row")? {
        total_seen += 1;
        if total_seen <= max_rows {
            let mut rendered = Vec::with_capacity(column_count);
            let mut fields = serde_json::Map::new();
            for (index, column) in columns.iter().enumerate() {
                let summary = sqlite_value_summary(row.get_ref(index)?, max_value_chars);
                rendered.push((column.clone(), summary.clone()));
                fields.insert(column.clone(), json!(summary));
            }
            rendered_rows.push(rendered);
            json_rows.push(json!({
                "row": total_seen - 1,
                "fields": fields,
            }));
        }
        if total_seen > max_scan_rows {
            scan_truncated = true;
            break;
        }
    }
    let shown = rendered_rows.len();
    let cap_reason = if scan_truncated {
        Some("scan")
    } else if shown < total_seen {
        Some("rows")
    } else {
        None
    };
    if cli.json {
        let mut map = base_receipt(
            "sqlite",
            config.profile.as_deref(),
            "rows",
            shown,
            total_seen,
            cap_reason.is_some(),
            cap_reason,
        );
        map.insert("db".to_string(), json!(display_path(db)));
        map.insert("columns".to_string(), json!(columns));
        map.insert("rows_scanned".to_string(), json!(total_seen));
        map.insert("rows".to_string(), json!(json_rows));
        emit_json_checked(cli, Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        writeln!(
            stdout,
            "[contextmink] sqlite db={} columns={}",
            display_path(db),
            columns.join(",")
        )?;
        if rendered_rows.is_empty() {
            writeln!(stdout, "no_rows")?;
        }
        for (row_index, fields) in rendered_rows.iter().enumerate() {
            let rendered = fields
                .iter()
                .map(|(column, value)| format!("{column}={value}"))
                .collect::<Vec<_>>()
                .join(" ");
            writeln!(stdout, "{row_index}: {rendered}")?;
        }
        if scan_truncated {
            writeln!(
                stdout,
                "[contextmink] capped sqlite scan at {max_scan_rows} rows; add WHERE/LIMIT or narrow the query before treating this as complete."
            )?;
        } else if shown < total_seen {
            writeln!(
                stdout,
                "[contextmink] capped sqlite output at {max_rows} rows; increase --max-rows or narrow the query."
            )?;
        }
        let mut map = base_receipt(
            "sqlite",
            config.profile.as_deref(),
            "rows",
            shown,
            total_seen,
            cap_reason.is_some(),
            cap_reason,
        );
        map.insert("columns".to_string(), json!(columns));
        map.insert("rows_scanned".to_string(), json!(total_seen));
        write_receipt_checked(cli, map)
    }
}

#[allow(clippy::too_many_arguments)]
fn command_sqlite_schema(
    cli: &Cli,
    config: &ContextConfig,
    db: &Path,
    requested_tables: &[String],
    name_contains: &[String],
    include_shadow: bool,
    include_system: bool,
    max_tables: usize,
    max_columns: usize,
    max_indexes: usize,
    max_line_chars: usize,
) -> Result<()> {
    if max_tables == 0 {
        return Err(anyhow!(
            "sqlite-schema --max-tables must be greater than zero"
        ));
    }
    let conn = open_sqlite_readonly(db)?;
    let requested = requested_tables.iter().collect::<BTreeSet<_>>();
    let mut stmt = conn
        .prepare(
            "SELECT schema, name, type, ncol, wr, strict \
             FROM pragma_table_list \
             ORDER BY schema, name",
        )
        .context("failed to prepare sqlite schema query")?;
    let mut table_rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)? != 0,
                row.get::<_, i64>(5)? != 0,
            ))
        })
        .context("failed to query sqlite schema")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to read sqlite schema rows")?;
    table_rows.retain(|(_, name, kind, _, _, _)| {
        if !include_system && name.starts_with("sqlite_") {
            return false;
        }
        if !include_shadow && kind == "shadow" {
            return false;
        }
        if !requested.is_empty() && !requested.contains(name) {
            return false;
        }
        if !name_contains.is_empty() && !contains_any(name, name_contains) {
            return false;
        }
        true
    });
    let total_tables = table_rows.len();
    let shown_tables = min(total_tables, max_tables);
    let mut remaining_columns = max_columns;
    let mut remaining_indexes = max_indexes;
    let mut columns_total = 0usize;
    let mut columns_shown = 0usize;
    let mut indexes_total = 0usize;
    let mut indexes_shown = 0usize;
    let mut summaries = Vec::with_capacity(shown_tables);
    for (schema, name, kind, column_count_declared, without_rowid, strict) in
        table_rows.into_iter().take(shown_tables)
    {
        let all_columns = sqlite_schema_columns(&conn, &schema, &name)?;
        let all_indexes = sqlite_schema_indexes(&conn, &schema, &name)?;
        let all_columns_len = all_columns.len();
        let all_indexes_len = all_indexes.len();
        columns_total += all_columns_len;
        indexes_total += all_indexes_len;
        let columns_take = min(remaining_columns, all_columns.len());
        let indexes_take = min(remaining_indexes, all_indexes.len());
        columns_shown += columns_take;
        indexes_shown += indexes_take;
        remaining_columns = remaining_columns.saturating_sub(columns_take);
        remaining_indexes = remaining_indexes.saturating_sub(indexes_take);
        summaries.push(SqliteTableSummary {
            schema,
            name,
            kind,
            column_count_declared,
            without_rowid,
            strict,
            columns: all_columns.into_iter().take(columns_take).collect(),
            indexes: all_indexes.into_iter().take(indexes_take).collect(),
            columns_total: all_columns_len,
            indexes_total: all_indexes_len,
        });
    }
    let columns_truncated = summaries.iter().any(|table| {
        table.column_count_declared as usize > table.columns.len() && columns_shown >= max_columns
    });
    let indexes_truncated = indexes_shown < indexes_total;
    let truncated = shown_tables < total_tables || columns_truncated || indexes_truncated;
    let cap_reason = if shown_tables < total_tables {
        Some("tables")
    } else if columns_truncated {
        Some("columns")
    } else if indexes_truncated {
        Some("indexes")
    } else {
        None
    };
    if cli.json {
        let mut map = base_receipt(
            "sqlite-schema",
            config.profile.as_deref(),
            "tables",
            shown_tables,
            total_tables,
            truncated,
            cap_reason,
        );
        map.insert("db".to_string(), json!(display_path(db)));
        map.insert("columns_shown".to_string(), json!(columns_shown));
        map.insert("columns_total".to_string(), json!(columns_total));
        map.insert("indexes_shown".to_string(), json!(indexes_shown));
        map.insert("indexes_total".to_string(), json!(indexes_total));
        map.insert(
            "tables".to_string(),
            Value::Array(
                summaries
                    .iter()
                    .map(sqlite_table_summary_json)
                    .collect::<Vec<_>>(),
            ),
        );
        return emit_json_checked(cli, Value::Object(map));
    }
    let mut stdout = io::stdout();
    writeln!(
        stdout,
        "[contextmink] sqlite-schema db={}",
        display_path(db)
    )?;
    if summaries.is_empty() {
        writeln!(stdout, "no_tables")?;
    }
    for table in &summaries {
        writeln!(
            stdout,
            "{}.{} type={} ncol={} strict={} without_rowid={}",
            table.schema,
            table.name,
            table.kind,
            table.column_count_declared,
            table.strict,
            table.without_rowid
        )?;
        for column in &table.columns {
            writeln!(
                stdout,
                "  column {}",
                clamp_text(&sqlite_column_summary_human(column), max_line_chars)
            )?;
        }
        for index in &table.indexes {
            writeln!(
                stdout,
                "  index {}",
                clamp_text(&sqlite_index_summary_human(index), max_line_chars)
            )?;
        }
    }
    if truncated {
        writeln!(
            stdout,
            "[contextmink] capped sqlite schema output at tables={max_tables} columns={max_columns} indexes={max_indexes}; narrow with --table or --name-contains."
        )?;
    }
    let mut map = base_receipt(
        "sqlite-schema",
        config.profile.as_deref(),
        "tables",
        shown_tables,
        total_tables,
        truncated,
        cap_reason,
    );
    map.insert("columns_shown".to_string(), json!(columns_shown));
    map.insert("columns_total".to_string(), json!(columns_total));
    map.insert("indexes_shown".to_string(), json!(indexes_shown));
    map.insert("indexes_total".to_string(), json!(indexes_total));
    write_receipt_checked(cli, map)
}

fn sqlite_schema_columns(
    conn: &Connection,
    schema_name: &str,
    table_name: &str,
) -> Result<Vec<SqliteColumnSummary>> {
    let mut fks = HashMap::new();
    let mut fk_stmt = conn
        .prepare("SELECT \"from\", \"table\", \"to\" FROM pragma_foreign_key_list(?, ?)")
        .context("failed to prepare sqlite foreign-key query")?;
    let fk_rows = fk_stmt
        .query_map([table_name, schema_name], |row| {
            Ok((
                row.get::<_, String>(0)?,
                SqliteForeignKeySummary {
                    table: row.get::<_, String>(1)?,
                    column: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                },
            ))
        })
        .with_context(|| format!("failed to query foreign keys for {table_name}"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .with_context(|| format!("failed to read foreign keys for {table_name}"))?;
    for (column, fk) in fk_rows {
        fks.insert(column, fk);
    }

    let mut stmt = conn
        .prepare(
            "SELECT name, lower(type), \"notnull\", dflt_value, pk, hidden \
             FROM pragma_table_xinfo(?, ?) \
             ORDER BY cid",
        )
        .context("failed to prepare sqlite column query")?;
    stmt.query_map([table_name, schema_name], |row| {
        let name = row.get::<_, String>(0)?;
        Ok(SqliteColumnSummary {
            foreign_key: fks.get(&name).cloned(),
            name,
            type_name: row.get::<_, String>(1)?,
            not_null: row.get::<_, i64>(2)? != 0,
            default_value: row.get::<_, Option<String>>(3)?,
            primary_key_rank: row.get::<_, i64>(4)?,
            hidden: row.get::<_, i64>(5)?,
        })
    })
    .with_context(|| format!("failed to query columns for {table_name}"))?
    .collect::<rusqlite::Result<Vec<_>>>()
    .with_context(|| format!("failed to read columns for {table_name}"))
}

fn sqlite_schema_indexes(
    conn: &Connection,
    schema_name: &str,
    table_name: &str,
) -> Result<Vec<SqliteIndexSummary>> {
    let mut stmt = conn
        .prepare(
            "SELECT name, \"unique\", origin, partial FROM pragma_index_list(?, ?) ORDER BY seq",
        )
        .context("failed to prepare sqlite index query")?;
    let mut indexes = Vec::new();
    for row in stmt
        .query_map([table_name, schema_name], |row| {
            Ok(SqliteIndexSummary {
                name: row.get::<_, String>(0)?,
                unique: row.get::<_, i64>(1)? != 0,
                origin: row.get::<_, String>(2)?,
                partial: row.get::<_, i64>(3)? != 0,
                columns: Vec::new(),
            })
        })
        .with_context(|| format!("failed to query indexes for {table_name}"))?
    {
        let mut index = row.with_context(|| format!("failed to read index for {table_name}"))?;
        let mut col_stmt = conn
            .prepare("SELECT cid, name FROM pragma_index_xinfo(?, ?) WHERE key != 0 ORDER BY seqno")
            .with_context(|| format!("failed to prepare index-column query for {}", index.name))?;
        index.columns = col_stmt
            .query_map([index.name.as_str(), schema_name], |row| {
                let cid = row.get::<_, i64>(0)?;
                let name = row.get::<_, Option<String>>(1)?;
                Ok(name.unwrap_or_else(|| match cid {
                    -2 => "<expr>".to_owned(),
                    -1 => "<rowid>".to_owned(),
                    _ => "<unknown>".to_owned(),
                }))
            })
            .with_context(|| format!("failed to query columns for index {}", index.name))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .with_context(|| format!("failed to read columns for index {}", index.name))?;
        indexes.push(index);
    }
    Ok(indexes)
}

fn sqlite_table_summary_json(table: &SqliteTableSummary) -> Value {
    json!({
        "schema": table.schema,
        "name": table.name,
        "type": table.kind,
        "ncol": table.column_count_declared,
        "strict": table.strict,
        "without_rowid": table.without_rowid,
        "columns_total": table.columns_total,
        "indexes_total": table.indexes_total,
        "columns": table.columns.iter().map(|column| {
            json!({
                "name": column.name,
                "type": column.type_name,
                "not_null": column.not_null,
                "default": column.default_value,
                "primary_key_rank": column.primary_key_rank,
                "hidden": column.hidden,
                "foreign_key": column.foreign_key.as_ref().map(|fk| json!({
                    "table": fk.table,
                    "column": fk.column,
                })),
            })
        }).collect::<Vec<_>>(),
        "indexes": table.indexes.iter().map(|index| {
            json!({
                "name": index.name,
                "unique": index.unique,
                "origin": index.origin,
                "partial": index.partial,
                "columns": index.columns,
            })
        }).collect::<Vec<_>>(),
    })
}

fn sqlite_column_summary_human(column: &SqliteColumnSummary) -> String {
    let mut parts = vec![format!("{} {}", column.name, column.type_name)];
    if column.not_null {
        parts.push("not_null".to_owned());
    }
    if column.primary_key_rank != 0 {
        parts.push(format!("pk#{}", column.primary_key_rank));
    }
    if column.hidden != 0 {
        parts.push(format!("hidden#{}", column.hidden));
    }
    if let Some(default) = &column.default_value {
        parts.push(format!("default={default:?}"));
    }
    if let Some(fk) = &column.foreign_key {
        parts.push(format!("fk={}.{}", fk.table, fk.column));
    }
    parts.join(" ")
}

fn sqlite_index_summary_human(index: &SqliteIndexSummary) -> String {
    let mut parts = vec![format!("{}({})", index.name, index.columns.join(","))];
    if index.unique {
        parts.push("unique".to_owned());
    }
    if index.partial {
        parts.push("partial".to_owned());
    }
    parts.push(format!("origin={}", index.origin));
    parts.join(" ")
}

fn open_sqlite_readonly(db: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("failed to open sqlite DB {}", db.display()))?;
    conn.execute_batch("PRAGMA query_only = ON")
        .context("failed to enable sqlite query_only mode")?;
    Ok(conn)
}

fn json_select_row(
    index: usize,
    row: &Value,
    fields: &[String],
    max_value_chars: usize,
) -> Result<Value> {
    if fields.is_empty() {
        return Ok(json!({
            "row": index,
            "value": value_summary(row, max_value_chars),
        }));
    }
    let mut output_fields = serde_json::Map::new();
    for field in fields {
        let summary = json_select_field(row, field)?
            .map(|value| value_summary(value, max_value_chars))
            .unwrap_or_else(|| "null".to_owned());
        output_fields.insert(field.clone(), json!(summary));
    }
    Ok(json!({
        "row": index,
        "fields": output_fields,
    }))
}

fn json_select_field<'a>(row: &'a Value, selector: &str) -> Result<Option<&'a Value>> {
    if selector == "$" || selector.starts_with('/') || selector.is_empty() {
        return json_pointer_lookup(row, selector);
    }
    Ok(row.as_object().and_then(|map| map.get(selector)))
}

fn json_pointer_lookup<'a>(value: &'a Value, pointer: &str) -> Result<Option<&'a Value>> {
    if pointer.is_empty() || pointer == "$" {
        return Ok(Some(value));
    }
    if !pointer.starts_with('/') {
        return Err(anyhow!(
            "JSON pointer must be empty, $, or start with /: {pointer}"
        ));
    }
    let mut current = value;
    for raw_token in pointer[1..].split('/') {
        let token = decode_json_pointer_token(raw_token)?;
        match current {
            Value::Object(map) => {
                let Some(next) = map.get(&token) else {
                    return Ok(None);
                };
                current = next;
            }
            Value::Array(values) => {
                let index = token
                    .parse::<usize>()
                    .with_context(|| format!("invalid JSON array index in pointer: {token}"))?;
                let Some(next) = values.get(index) else {
                    return Ok(None);
                };
                current = next;
            }
            _ => return Ok(None),
        }
    }
    Ok(Some(current))
}

fn decode_json_pointer_token(token: &str) -> Result<String> {
    let mut output = String::with_capacity(token.len());
    let mut chars = token.chars();
    while let Some(ch) = chars.next() {
        if ch != '~' {
            output.push(ch);
            continue;
        }
        match chars.next() {
            Some('0') => output.push('~'),
            Some('1') => output.push('/'),
            Some(other) => {
                return Err(anyhow!(
                    "invalid JSON pointer escape: ~{other}; expected ~0 or ~1"
                ));
            }
            None => {
                return Err(anyhow!(
                    "invalid JSON pointer escape at end of token; expected ~0 or ~1"
                ));
            }
        }
    }
    Ok(output)
}

fn sqlite_value_summary(value: ValueRef<'_>, max_chars: usize) -> String {
    match value {
        ValueRef::Null => "null".to_owned(),
        ValueRef::Integer(value) => value.to_string(),
        ValueRef::Real(value) => value.to_string(),
        ValueRef::Text(value) => {
            let value = String::from_utf8_lossy(value);
            clamp_text(&format!("{value:?}"), max_chars)
        }
        ValueRef::Blob(value) => format!("<blob:{} bytes>", value.len()),
    }
}

fn load_context_config(config_path: Option<&Path>, no_config: bool) -> Result<ContextConfig> {
    let mut raw = ContextminkConfig::default();
    if !no_config {
        let discovered_config = find_config_path();
        let selected_config = config_path.map(Path::to_path_buf).or(discovered_config);
        if let Some(path) = selected_config.as_deref() {
            let text = fs::read_to_string(path)
                .with_context(|| format!("failed to read config {}", path.display()))?;
            raw = toml::from_str(&text)
                .with_context(|| format!("failed to parse {}", path.display()))?;
        }
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in BUILTIN_EXCLUDES {
        builder.add(Glob::new(pattern).with_context(|| format!("invalid builtin glob {pattern}"))?);
    }
    if let Some(excludes) = &raw.exclude_globs {
        for pattern in excludes {
            builder.add(
                Glob::new(pattern).with_context(|| format!("invalid exclude glob {pattern}"))?,
            );
        }
    }
    Ok(ContextConfig {
        profile: raw.profile,
        excludes: builder
            .build()
            .context("failed to build exclude glob set")?,
    })
}

fn find_config_path() -> Option<PathBuf> {
    let mut current = std::env::current_dir().ok()?;
    loop {
        let candidate = current.join(CONFIG_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn collect_files(
    paths: &[PathBuf],
    globs: &[String],
    config: &ContextConfig,
    include_noisy: bool,
    max_scan_files: usize,
) -> Result<CollectedFiles> {
    let include_matcher = build_optional_globset(globs)?;
    let mut files = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut total_seen = 0usize;
    let mut truncated = false;
    for root in paths {
        if root.is_file() {
            if file_is_included(root, &include_matcher, config, include_noisy) {
                let candidate = root.to_path_buf();
                if !seen.insert(candidate.clone()) {
                    continue;
                }
                total_seen += 1;
                if files.len() < max_scan_files {
                    files.push(candidate);
                } else {
                    truncated = true;
                    break;
                }
            }
            continue;
        }
        let mut walk = WalkBuilder::new(root);
        walk.hidden(false)
            .ignore(true)
            .git_ignore(true)
            .git_exclude(true)
            .parents(true);
        for entry in walk.build() {
            let entry = entry?;
            if entry.file_type().is_some_and(|kind| kind.is_file()) {
                if !file_is_included(entry.path(), &include_matcher, config, include_noisy) {
                    continue;
                }
                let candidate = entry.path().to_path_buf();
                if !seen.insert(candidate.clone()) {
                    continue;
                }
                total_seen += 1;
                if files.len() < max_scan_files {
                    files.push(candidate);
                } else {
                    truncated = true;
                    break;
                }
            }
        }
        if truncated {
            break;
        }
    }
    files.sort();
    files.dedup();
    Ok(CollectedFiles {
        files,
        total_seen,
        truncated,
    })
}

fn file_is_included(
    path: &Path,
    include_matcher: &Option<GlobSet>,
    config: &ContextConfig,
    include_noisy: bool,
) -> bool {
    let normalized = normalize_path(path);
    if !include_noisy && config.excludes.is_match(&normalized) {
        return false;
    }
    if let Some(include_matcher) = include_matcher {
        let basename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !include_matcher.is_match(&normalized) && !include_matcher.is_match(basename) {
            return false;
        }
    }
    true
}

fn build_optional_globset(globs: &[String]) -> Result<Option<GlobSet>> {
    if globs.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in globs {
        builder.add(Glob::new(pattern).with_context(|| format!("invalid include glob {pattern}"))?);
    }
    Ok(Some(
        builder
            .build()
            .context("failed to build include glob set")?,
    ))
}

fn read_text_file(path: &Path, max_file_bytes: u64) -> Result<Option<String>> {
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to stat {}", path.display()))?;
    if metadata.len() > max_file_bytes {
        return Ok(None);
    }
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.contains(&0) {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

fn walk_json<'a>(
    path: &str,
    key: Option<&'a str>,
    value: &'a Value,
    visit: &mut impl FnMut(&str, Option<&'a str>, &'a Value),
) {
    visit(path, key, value);
    match value {
        Value::Object(map) => {
            for (child_key, child) in map {
                let child_path = if is_json_identifier(child_key) {
                    format!("{path}.{child_key}")
                } else {
                    format!(
                        "{path}[{}]",
                        serde_json::to_string(child_key).unwrap_or_default()
                    )
                };
                walk_json(&child_path, Some(child_key.as_str()), child, visit);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                let child_path = format!("{path}[{index}]");
                walk_json(&child_path, None, child, visit);
            }
        }
        _ => {}
    }
}

fn contains_any(value: &str, needles: &[String]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn value_summary(value: &Value, max_chars: usize) -> String {
    match value {
        Value::String(value) => clamp_text(&format!("{value:?}"), max_chars),
        Value::Null | Value::Bool(_) | Value::Number(_) => value.to_string(),
        Value::Array(values) => {
            if is_small_json(value) {
                clamp_text(
                    &serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_owned()),
                    max_chars,
                )
            } else {
                format!("<array:{} items>", values.len())
            }
        }
        Value::Object(map) => {
            if is_small_json(value) {
                clamp_text(
                    &serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_owned()),
                    max_chars,
                )
            } else {
                let sample_keys = map.keys().take(5).cloned().collect::<Vec<_>>();
                format!(
                    "<object:{} keys sample={}>",
                    map.len(),
                    serde_json::to_string(&sample_keys).unwrap_or_else(|_| "[]".to_owned())
                )
            }
        }
    }
}

fn is_small_json(value: &Value) -> bool {
    let mut nodes = 0usize;
    let mut string_chars = 0usize;
    json_fits_budget(value, &mut nodes, &mut string_chars)
}

fn json_fits_budget(value: &Value, nodes: &mut usize, string_chars: &mut usize) -> bool {
    *nodes += 1;
    if *nodes > JSON_SMALL_NODE_LIMIT {
        return false;
    }
    match value {
        Value::String(value) => {
            *string_chars += value.chars().count();
            *string_chars <= JSON_SMALL_STRING_CHAR_LIMIT
        }
        Value::Array(values) => values
            .iter()
            .all(|value| json_fits_budget(value, nodes, string_chars)),
        Value::Object(map) => map
            .values()
            .all(|value| json_fits_budget(value, nodes, string_chars)),
        Value::Null | Value::Bool(_) | Value::Number(_) => true,
    }
}

fn is_json_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn clamp_text(value: &str, max_chars: usize) -> String {
    let mut iter = value.chars();
    let mut output = String::new();
    for _ in 0..max_chars {
        let Some(ch) = iter.next() else {
            return output;
        };
        output.push(ch);
    }
    if iter.next().is_some() {
        output.push_str("...");
    }
    output
}

fn normalize_path(path: &Path) -> String {
    let path = path.strip_prefix(".").unwrap_or(path);
    path.to_string_lossy().replace('\\', "/")
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn emit_json(value: Value) -> Result<()> {
    let mut stdout = io::stdout();
    serde_json::to_writer_pretty(&mut stdout, &value)?;
    writeln!(stdout)?;
    Ok(())
}

fn emit_json_checked(cli: &Cli, value: Value) -> Result<()> {
    let truncated = receipt_truncated_from_value(&value);
    emit_json(value)?;
    fail_if_truncated(cli, truncated)
}

fn write_receipt(map: serde_json::Map<String, Value>) -> Result<()> {
    let mut stdout = io::stdout();
    writeln!(stdout, "{RECEIPT_PREFIX}{}", Value::Object(map))?;
    Ok(())
}

fn write_receipt_checked(cli: &Cli, map: serde_json::Map<String, Value>) -> Result<()> {
    let truncated = receipt_truncated_from_map(&map);
    write_receipt(map)?;
    fail_if_truncated(cli, truncated)
}

fn receipt_truncated_from_value(value: &Value) -> bool {
    value
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn receipt_truncated_from_map(map: &serde_json::Map<String, Value>) -> bool {
    map.get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn fail_if_truncated(cli: &Cli, truncated: bool) -> Result<()> {
    if cli.fail_if_truncated && truncated {
        Err(anyhow!(
            "contextmink output was truncated (--fail-if-truncated)"
        ))
    } else {
        Ok(())
    }
}

/// Build the common receipt envelope. `shown`/`total` always carry the unit
/// named by `unit` (files, lines, chars, or matches) regardless of which cap
/// fired, so a machine consumer can rely on stable field semantics.
fn base_receipt(
    command: &str,
    profile: Option<&str>,
    unit: &str,
    shown: usize,
    total: usize,
    truncated: bool,
    cap_reason: Option<&str>,
) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    map.insert("tool".to_string(), json!("contextmink"));
    map.insert("command".to_string(), json!(command));
    map.insert("profile".to_string(), json!(profile));
    map.insert("unit".to_string(), json!(unit));
    map.insert("shown".to_string(), json!(shown));
    map.insert("total".to_string(), json!(total));
    map.insert("truncated".to_string(), json!(truncated));
    map.insert("complete".to_string(), json!(!truncated));
    map.insert("cap_reason".to_string(), json!(cap_reason));
    map
}

/// Grep receipts keep `shown`/`total` in file units in every path and report
/// match/sample/scan detail in dedicated fields, so `cap_reason` (not the unit
/// of `total`) is what tells a consumer why output stopped.
#[allow(clippy::too_many_arguments)]
fn emit_grep_receipt(
    cli: &Cli,
    command_name: &str,
    config: &ContextConfig,
    files_shown: usize,
    matched_files_total: usize,
    total_matches: usize,
    sample_lines_shown: usize,
    candidate_files_total: usize,
    candidate_files_scanned: usize,
    skipped_large_or_binary: usize,
    truncated: bool,
    cap_reason: Option<&str>,
) -> Result<()> {
    let mut map = base_receipt(
        command_name,
        config.profile.as_deref(),
        "files",
        files_shown,
        matched_files_total,
        truncated,
        cap_reason,
    );
    map.insert("total_matches".to_string(), json!(total_matches));
    map.insert("sample_lines_shown".to_string(), json!(sample_lines_shown));
    map.insert(
        "candidate_files_total".to_string(),
        json!(candidate_files_total),
    );
    map.insert(
        "candidate_files_scanned".to_string(),
        json!(candidate_files_scanned),
    );
    map.insert(
        "candidate_files_total_is_lower_bound".to_string(),
        json!(matches!(cap_reason, Some("scan"))),
    );
    map.insert(
        "skipped_large_or_binary".to_string(),
        json!(skipped_large_or_binary),
    );
    write_receipt_checked(cli, map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_text_is_character_safe() {
        assert_eq!(clamp_text("abcdef", 3), "abc...");
        assert_eq!(clamp_text("abc", 3), "abc");
        assert_eq!(clamp_text("a->b", 20), "a->b");
    }

    #[test]
    fn json_identifier_filter_matches_plain_keys_only() {
        assert!(is_json_identifier("alpha_beta1"));
        assert!(!is_json_identifier("1alpha"));
        assert!(!is_json_identifier("alpha-beta"));
    }

    #[test]
    fn value_summary_keeps_large_json_structural() {
        let large = json!({
            "items": (0..120).map(|index| json!({"index": index})).collect::<Vec<_>>(),
            "kind": "large",
        });
        let summary = value_summary(&large, 80);
        assert!(summary.starts_with("<object:2 keys sample="));
        assert!(!summary.contains("\"index\":119"));

        let small = json!({"address": "0x7FF954", "function_count": 12});
        assert_eq!(
            value_summary(&small, 200),
            "{\"address\":\"0x7FF954\",\"function_count\":12}"
        );
    }

    #[test]
    fn parse_line_range_requires_bounded_one_based_range() {
        assert_eq!(parse_line_range("10:20").unwrap(), (10, Some(20)));
        assert!(parse_line_range("10").is_err());
        assert!(parse_line_range("0:1").is_err());
        assert!(parse_line_range("20:10").is_err());
    }

    #[test]
    fn base_receipt_has_stable_envelope() {
        let map = base_receipt("grep", Some("demo"), "files", 3, 12, true, Some("files"));
        assert_eq!(map["tool"], json!("contextmink"));
        assert_eq!(map["unit"], json!("files"));
        assert_eq!(map["shown"], json!(3));
        assert_eq!(map["total"], json!(12));
        assert_eq!(map["truncated"], json!(true));
        assert_eq!(map["complete"], json!(false));
        assert_eq!(map["cap_reason"], json!("files"));

        let complete = base_receipt("files", None, "files", 5, 5, false, None);
        assert_eq!(complete["truncated"], json!(false));
        assert_eq!(complete["complete"], json!(true));
        assert_eq!(complete["cap_reason"], Value::Null);
        assert_eq!(complete["profile"], Value::Null);
    }

    #[test]
    fn merged_paths_defaults_to_workspace_root() {
        assert_eq!(merged_paths(&[], &[]), vec![PathBuf::from(".")]);
        assert_eq!(
            merged_paths(&[PathBuf::from("src")], &[PathBuf::from("tests")]),
            vec![PathBuf::from("src"), PathBuf::from("tests")]
        );
    }
}
