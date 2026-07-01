use std::cmp::min;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::config::ContextConfig;
use crate::files::{collect_files, display_path, read_text_file};
use crate::merged_paths;
use crate::output::{
    base_receipt, clamp_text, emit_grep_receipt, emit_json_checked, no_match_scope,
    write_receipt_checked,
};
use crate::text::{TextMatcher, collect_single_text_source, parse_line_range};

#[derive(Debug)]
struct FileMatch {
    path: PathBuf,
    count: usize,
    samples: Vec<(usize, String)>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_files(
    cli: &Cli,
    config: &ContextConfig,
    paths: &[PathBuf],
    globs: &[String],
    extensions: &[String],
    with_excluded: bool,
    with_git_ignored: bool,
    max: usize,
    max_line_chars: usize,
    max_scan_files: usize,
) -> Result<()> {
    if max_scan_files == 0 {
        return Err(anyhow!("files --max-scan-files must be greater than zero"));
    }
    let collected = collect_files(
        paths,
        globs,
        extensions,
        config,
        with_excluded,
        with_git_ignored,
        max_scan_files,
    )?;
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
pub(crate) fn command_grep(
    cli: &Cli,
    config: &ContextConfig,
    args: &[String],
    named_paths: &[PathBuf],
    pattern_file: Option<&Path>,
    literal: bool,
    with_excluded: bool,
    with_git_ignored: bool,
    max_count_files: usize,
    max_files: usize,
    lines_per_file: usize,
    max_sample_lines: usize,
    max_line_chars: usize,
    max_scan_files: usize,
    max_file_bytes: u64,
) -> Result<()> {
    let (pattern, effective_paths) = if pattern_file.is_some() {
        (None, merged_paths(&string_args_to_paths(args), named_paths))
    } else {
        let Some((pattern, paths)) = args.split_first() else {
            return Err(anyhow!("grep requires PATTERN or --pattern-file <file>"));
        };
        (
            Some(pattern.as_str()),
            merged_paths(&string_args_to_paths(paths), named_paths),
        )
    };
    let pattern = collect_single_text_source("grep pattern", pattern, pattern_file, true)?;
    let matcher = TextMatcher::new(&pattern, literal)?;
    command_grep_with_matcher(
        cli,
        config,
        "grep",
        matcher,
        &effective_paths,
        with_excluded,
        with_git_ignored,
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
    args.iter().map(PathBuf::from).collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_grep_with_matcher(
    cli: &Cli,
    config: &ContextConfig,
    command_name: &str,
    matcher: TextMatcher,
    paths: &[PathBuf],
    with_excluded: bool,
    with_git_ignored: bool,
    max_count_files: usize,
    max_files: usize,
    lines_per_file: usize,
    max_sample_lines: usize,
    max_line_chars: usize,
    max_scan_files: usize,
    max_file_bytes: u64,
) -> Result<()> {
    if max_count_files == 0 {
        return Err(anyhow!(
            "{command_name} --max-count-files must be greater than zero"
        ));
    }
    if max_scan_files == 0 {
        return Err(anyhow!(
            "{command_name} --max-scan-files must be greater than zero"
        ));
    }
    let collected = collect_files(
        paths,
        &[],
        &[],
        config,
        with_excluded,
        with_git_ignored,
        max_scan_files,
    )?;
    let scan_truncated = collected.truncated;
    let total_candidate_files = collected.total_seen;
    let candidate_files = collected.files;
    let candidate_files_scanned = candidate_files.len();
    let mut content_files_scanned = 0usize;
    let mut matched_files_total_is_lower_bound = false;
    let mut matches = Vec::new();
    let mut total_matches = 0usize;
    let mut skipped_large_or_binary = 0usize;
    for (file_index, file) in candidate_files.into_iter().enumerate() {
        content_files_scanned += 1;
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
        if matches.len() >= max_count_files && file_index + 1 < candidate_files_scanned {
            matched_files_total_is_lower_bound = true;
            break;
        }
    }
    let count_shown = min(matches.len(), max_count_files);
    let files_shown = min(count_shown, max_files);
    let total_matches_is_lower_bound = matched_files_total_is_lower_bound;
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
        } else if matched_files_total_is_lower_bound {
            Some("matched_files")
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
        map.insert(
            "matched_files_total_is_lower_bound".to_string(),
            json!(matched_files_total_is_lower_bound),
        );
        map.insert("total_matches".to_string(), json!(total_matches));
        map.insert(
            "total_matches_is_lower_bound".to_string(),
            json!(total_matches_is_lower_bound),
        );
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
            "content_files_scanned".to_string(),
            json!(content_files_scanned),
        );
        map.insert(
            "candidate_files_total_is_lower_bound".to_string(),
            json!(scan_truncated),
        );
        map.insert(
            "skipped_large_or_binary".to_string(),
            json!(skipped_large_or_binary),
        );
        map.insert(
            "no_match_scope".to_string(),
            json!(no_match_scope(matches.is_empty(), scan_truncated)),
        );
        map.insert("files".to_string(), json!(files_json));
        emit_json_checked(cli, Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        writeln!(stdout, "[contextmink] grep pattern={}", matcher.label())?;
        writeln!(
            stdout,
            "matched_files_total={} matched_files_counted={} matched_files_total_is_lower_bound={} total_matches={} total_matches_is_lower_bound={}",
            matches.len(),
            count_shown,
            matched_files_total_is_lower_bound,
            total_matches,
            total_matches_is_lower_bound
        )?;
        writeln!(
            stdout,
            "candidate_files_total={} candidate_files_scanned={} content_files_scanned={} skipped_large_or_binary={}",
            total_candidate_files,
            candidate_files_scanned,
            content_files_scanned,
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
                cli,
                command_name,
                config,
                0,
                0,
                total_matches,
                0,
                total_candidate_files,
                candidate_files_scanned,
                content_files_scanned,
                skipped_large_or_binary,
                scan_truncated,
                matched_files_total_is_lower_bound,
                total_matches_is_lower_bound,
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
        } else if matched_files_total_is_lower_bound {
            Some("matched_files")
        } else if files_shown < matches.len() {
            Some("files")
        } else if sample_capped {
            Some("samples")
        } else {
            None
        };
        if matches!(
            cap_reason,
            Some("scan") | Some("matched_files") | Some("files")
        ) {
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
            content_files_scanned,
            skipped_large_or_binary,
            cap_reason.is_some(),
            matched_files_total_is_lower_bound,
            total_matches_is_lower_bound,
            cap_reason,
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_slice(
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
