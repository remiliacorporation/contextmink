use std::cmp::min;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand, ValueEnum};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};

const CONFIG_NAME: &str = ".contextmink.toml";
const RECEIPT_PREFIX: &str = "CONTEXTMINK_RECEIPT ";

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
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[arg(long, global = true)]
    no_config: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Files {
        #[arg(default_value = ".")]
        path: Vec<PathBuf>,
        #[arg(long = "glob")]
        globs: Vec<String>,
        #[arg(long)]
        include_noisy: bool,
        #[arg(long, default_value_t = 80)]
        max: usize,
        #[arg(long, default_value_t = 220)]
        max_line_chars: usize,
    },
    Grep {
        pattern: String,
        #[arg(default_value = ".")]
        path: Vec<PathBuf>,
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
        #[arg(long, default_value_t = 40)]
        max: usize,
        #[arg(long, default_value_t = 260)]
        max_value_chars: usize,
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
        } => command_files(
            &cli,
            &config,
            path,
            globs,
            *include_noisy,
            *max,
            *max_line_chars,
        ),
        Command::Grep {
            pattern,
            path,
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
            pattern,
            path,
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

fn command_files(
    cli: &Cli,
    config: &ContextConfig,
    paths: &[PathBuf],
    globs: &[String],
    include_noisy: bool,
    max: usize,
    max_line_chars: usize,
) -> Result<()> {
    let files = collect_files(paths, globs, config, include_noisy)?;
    let shown = min(files.len(), max);
    let truncated = shown < files.len();
    let cap_reason = if truncated { Some("max") } else { None };
    if cli.json {
        let mut map = base_receipt(
            "files",
            config.profile.as_deref(),
            "files",
            shown,
            files.len(),
            truncated,
            cap_reason,
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
        emit_json(Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        for path in files.iter().take(shown) {
            writeln!(
                stdout,
                "{}",
                clamp_text(&display_path(path), max_line_chars)
            )?;
        }
        write_receipt(base_receipt(
            "files",
            config.profile.as_deref(),
            "files",
            shown,
            files.len(),
            truncated,
            cap_reason,
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn command_grep(
    cli: &Cli,
    config: &ContextConfig,
    pattern: &str,
    paths: &[PathBuf],
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
    let matcher = TextMatcher::new(pattern, literal)?;
    command_grep_with_matcher(
        cli,
        config,
        "grep",
        matcher,
        paths,
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
    let files = collect_files(paths, &[], config, include_noisy)?;
    let scan_truncated = files.len() > max_scan_files;
    let total_candidate_files = files.len();
    let mut matches = Vec::new();
    let mut total_matches = 0usize;
    let mut skipped_large_or_binary = 0usize;
    for file in files.into_iter().take(max_scan_files) {
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
    let cap_reason = if scan_truncated {
        Some("scan")
    } else if files_shown < matches.len() {
        Some("files")
    } else {
        None
    };
    if cli.json {
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
        map.insert(
            "candidate_files_total".to_string(),
            json!(total_candidate_files),
        );
        map.insert(
            "candidate_files_scanned".to_string(),
            json!(min(total_candidate_files, max_scan_files)),
        );
        map.insert(
            "skipped_large_or_binary".to_string(),
            json!(skipped_large_or_binary),
        );
        map.insert(
            "files".to_string(),
            json!(
                matches
                    .iter()
                    .take(files_shown)
                    .map(|row| {
                        json!({
                            "path": display_path(&row.path),
                            "count": row.count,
                            "samples": row.samples.iter().map(|(line, text)| json!({
                                "line": line,
                                "text": text,
                            })).collect::<Vec<_>>(),
                        })
                    })
                    .collect::<Vec<_>>()
            ),
        );
        emit_json(Value::Object(map))
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
            total_candidate_files,
            min(total_candidate_files, max_scan_files),
            skipped_large_or_binary
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
                command_name,
                config,
                0,
                0,
                total_matches,
                0,
                total_candidate_files,
                min(total_candidate_files, max_scan_files),
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
            command_name,
            config,
            files_shown,
            matches.len(),
            total_matches,
            sample_total,
            total_candidate_files,
            min(total_candidate_files, max_scan_files),
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
            return emit_json(Value::Object(map));
        }
        let mut stdout = io::stdout();
        write!(stdout, "{}", shown_text)?;
        if !shown_text.ends_with('\n') {
            writeln!(stdout)?;
        }
        return write_receipt(base_receipt(
            "slice",
            config.profile.as_deref(),
            "chars",
            shown_text.chars().count(),
            total_chars,
            truncated,
            cap_reason,
        ));
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
        emit_json(Value::Object(map))
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
        write_receipt(base_receipt(
            "slice",
            config.profile.as_deref(),
            "lines",
            shown,
            total_lines,
            truncated,
            cap_reason,
        ))
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
    let document: Value = serde_json::from_str(&document).context("failed to parse JSON")?;
    let mut rows = Vec::new();
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
        rows.push((path.to_owned(), summary));
    });
    let shown = min(rows.len(), max);
    let truncated = shown < rows.len();
    let cap_reason = if truncated { Some("max") } else { None };
    if cli.json {
        let mut map = base_receipt(
            "json-find",
            config.profile.as_deref(),
            "matches",
            shown,
            rows.len(),
            truncated,
            cap_reason,
        );
        map.insert("path".to_string(), json!(display_path(file)));
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
        emit_json(Value::Object(map))
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
        write_receipt(base_receipt(
            "json-find",
            config.profile.as_deref(),
            "matches",
            shown,
            rows.len(),
            truncated,
            cap_reason,
        ))
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
) -> Result<Vec<PathBuf>> {
    let include_matcher = build_optional_globset(globs)?;
    let mut files = Vec::new();
    for root in paths {
        if root.is_file() {
            maybe_push_file(&mut files, root, &include_matcher, config, include_noisy);
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
                maybe_push_file(
                    &mut files,
                    entry.path(),
                    &include_matcher,
                    config,
                    include_noisy,
                );
            }
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn maybe_push_file(
    files: &mut Vec<PathBuf>,
    path: &Path,
    include_matcher: &Option<GlobSet>,
    config: &ContextConfig,
    include_noisy: bool,
) {
    let normalized = normalize_path(path);
    if !include_noisy && config.excludes.is_match(&normalized) {
        return;
    }
    if let Some(include_matcher) = include_matcher
        && !include_matcher.is_match(&normalized)
    {
        return;
    }
    files.push(path.to_path_buf());
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
        _ => clamp_text(
            &serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_owned()),
            max_chars,
        ),
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

fn write_receipt(map: serde_json::Map<String, Value>) -> Result<()> {
    let mut stdout = io::stdout();
    writeln!(stdout, "{RECEIPT_PREFIX}{}", Value::Object(map))?;
    Ok(())
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
        "skipped_large_or_binary".to_string(),
        json!(skipped_large_or_binary),
    );
    write_receipt(map)
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
