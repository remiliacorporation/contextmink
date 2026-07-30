use std::cmp::min;
use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde_json::json;

use crate::cli::Cli;
use crate::config::ContextConfig;
use crate::files::{CollectOptions, collect_files, display_path};
use crate::grep_scan::{FileScan, SampleLine, ScanLimits, scan_files_ordered};
use crate::output::{
    Receipt, ReceiptCap, ReceiptResult, TextClamp, emit_json_checked, no_match_scope,
    write_receipt_checked,
};
use crate::paths_or_current_dir;
use crate::text::{TextMatcher, collect_single_text_source, parse_line_range};

const SKIPPED_FILES_SAMPLE_LIMIT: usize = 5;
/// Nested-repo disclosure is capped so a workspace with dozens of vendored
/// repos does not flood the transcript; the total count is always exact.
const NESTED_REPOS_RECEIPT_LIMIT: usize = 12;
const NESTED_REPOS_HUMAN_LIMIT: usize = 8;

fn nested_repos_receipt_fields(receipt: &mut Receipt, nested: &[String]) {
    receipt.insert("nested_repos_entered_total", json!(nested.len()));
    receipt.insert(
        "nested_repos_entered",
        json!(
            nested
                .iter()
                .take(NESTED_REPOS_RECEIPT_LIMIT)
                .collect::<Vec<_>>()
        ),
    );
}

#[derive(Debug)]
struct FileMatch {
    path: PathBuf,
    count: usize,
    samples: Vec<SampleLine>,
    sample_matching_lines_omitted: usize,
}

#[derive(Debug)]
struct SkippedFile {
    path: PathBuf,
    reason: &'static str,
    bytes: Option<u64>,
}

pub(crate) struct GrepCaps {
    pub(crate) max_matching_files: usize,
    pub(crate) max_files: usize,
    pub(crate) lines_per_file: usize,
    pub(crate) context: usize,
    pub(crate) max_sample_lines: usize,
    pub(crate) max_line_chars: usize,
    pub(crate) max_content_files: usize,
    pub(crate) max_file_bytes: u64,
    pub(crate) max_content_bytes: Option<u64>,
}

struct ContentAdmission {
    files: Vec<PathBuf>,
    bytes_admitted: Option<u64>,
    capped: bool,
}

fn admit_content_files(
    files: Vec<PathBuf>,
    max_content_bytes: Option<u64>,
) -> Result<ContentAdmission> {
    let Some(max_content_bytes) = max_content_bytes else {
        return Ok(ContentAdmission {
            files,
            bytes_admitted: None,
            capped: false,
        });
    };
    if max_content_bytes == 0 {
        return Err(anyhow!(
            "grep --max-content-bytes must be greater than zero"
        ));
    }

    let mut admitted = Vec::new();
    let mut bytes_admitted = 0u64;
    let mut capped = false;
    for path in files {
        let bytes = std::fs::metadata(&path)
            .with_context(|| format!("failed to inspect {}", path.display()))?
            .len();
        let Some(next_total) = bytes_admitted.checked_add(bytes) else {
            return Err(anyhow!("aggregate content byte count overflowed u64"));
        };
        if next_total > max_content_bytes {
            capped = true;
            break;
        }
        bytes_admitted = next_total;
        admitted.push(path);
    }
    Ok(ContentAdmission {
        files: admitted,
        bytes_admitted: Some(bytes_admitted),
        capped,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_files(
    cli: &Cli,
    config: &ContextConfig,
    paths: &[PathBuf],
    globs: &[String],
    path_terms: &[String],
    extensions: &[String],
    with_excluded: bool,
    with_git_ignored: bool,
    skip_nested_repos: bool,
    quiet: bool,
    max: usize,
    max_line_chars: usize,
    max_selected_files: usize,
) -> Result<()> {
    if max_selected_files == 0 {
        return Err(anyhow!("files --limit must be greater than zero"));
    }
    let collected = collect_files(
        paths,
        config,
        &CollectOptions {
            globs,
            path_terms,
            extensions,
            with_excluded,
            with_git_ignored,
            skip_nested_repos,
            max_selected_files,
        },
    )?;
    let files = collected.files;
    let shown = if quiet { 0 } else { min(files.len(), max) };
    let mut text_clamp = TextClamp::new(max_line_chars);
    let rendered_files = files
        .iter()
        .take(shown)
        .map(|path| text_clamp.clamp(&display_path(path)))
        .collect::<Vec<_>>();
    let mut receipt = Receipt::new(
        "files",
        config.profile.as_deref(),
        ReceiptResult::new("files", collected.total_seen, false, shown),
    );
    // Enumeration still visits the complete scope when retention reaches
    // `max_selected_files`, so the result total remains exact. The retained output
    // is capped, not the scope that was counted.
    if collected.selection_capped && !quiet {
        receipt.add_cap(ReceiptCap::output(
            "candidate_files",
            Some(max_selected_files),
        ));
    }
    if !quiet && shown < files.len() {
        receipt.add_cap(ReceiptCap::output("files", Some(max)));
    }
    text_clamp.add_receipt_cap(&mut receipt);
    receipt.insert("candidate_files_selected", json!(files.len()));
    nested_repos_receipt_fields(&mut receipt, &collected.nested_repos_entered);
    if quiet {
        receipt.insert("quiet", json!(true));
    }
    if cli.json {
        if !quiet {
            receipt.insert("files", json!(rendered_files));
        }
        emit_json_checked(cli, receipt)
    } else {
        let mut stdout = io::stdout();
        if !quiet {
            for path in rendered_files {
                writeln!(stdout, "{path}")?;
            }
        }
        write_nested_repos_note(&mut stdout, &collected.nested_repos_entered)?;
        if collected.selection_capped && !quiet {
            writeln!(
                stdout,
                "[contextmink] capped displayed file paths at {max_selected_files}; narrow the path or filters to inspect the omitted paths."
            )?;
        }
        write_receipt_checked(cli, receipt)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_dirs(
    cli: &Cli,
    config: &ContextConfig,
    paths: &[PathBuf],
    depth: usize,
    with_excluded: bool,
    with_git_ignored: bool,
    skip_nested_repos: bool,
    max: usize,
    max_line_chars: usize,
    max_files_counted: usize,
) -> Result<()> {
    if depth == 0 {
        return Err(anyhow!("dirs --depth must be greater than zero"));
    }
    if max_files_counted == 0 {
        return Err(anyhow!(
            "dirs --max-files-counted must be greater than zero"
        ));
    }
    let collected = collect_files(
        paths,
        config,
        &CollectOptions {
            globs: &[],
            path_terms: &[],
            extensions: &[],
            with_excluded,
            with_git_ignored,
            skip_nested_repos,
            max_selected_files: max_files_counted,
        },
    )?;
    let root_prefixes: Vec<String> = paths
        .iter()
        .map(|root| {
            display_path(root)
                .trim_start_matches("./")
                .trim_end_matches('/')
                .to_owned()
        })
        .filter(|root| !root.is_empty() && root != ".")
        .collect();
    // Recursive file counts per directory, keyed by display path, bounded to
    // `depth` levels below the matched root (or below the scan origin when
    // the root is the current directory).
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for file in &collected.files {
        let display = display_path(file);
        let display = display.trim_start_matches("./");
        let root_prefix = root_prefixes
            .iter()
            .filter(|root| display == **root || display.starts_with(&format!("{root}/")))
            .max_by_key(|root| root.len());
        let (base, relative) = match root_prefix {
            Some(root) => (
                root.as_str(),
                display
                    .strip_prefix(root.as_str())
                    .unwrap_or("")
                    .trim_start_matches('/'),
            ),
            None => ("", display),
        };
        let components: Vec<&str> = relative.split('/').collect();
        // The last component is the file name; ancestors are directories.
        let dir_components = components.len().saturating_sub(1);
        for level in 0..=min(dir_components, depth) {
            let mut key = base.to_owned();
            if level > 0 {
                if !key.is_empty() {
                    key.push('/');
                }
                key.push_str(&components[..level].join("/"));
            }
            let key = if key.is_empty() { ".".to_owned() } else { key };
            *counts.entry(key).or_insert(0) += 1;
        }
    }
    let total_dirs = counts.len();
    let shown = min(total_dirs, max);
    let mut text_clamp = TextClamp::new(max_line_chars);
    let rendered_dirs = counts
        .iter()
        .take(shown)
        .map(|(dir, files)| {
            let path = text_clamp.clamp(dir);
            let display = text_clamp.clamp(&format!("{dir} files={files}"));
            (path, display, *files)
        })
        .collect::<Vec<_>>();
    let mut receipt = Receipt::new(
        "dirs",
        config.profile.as_deref(),
        ReceiptResult::new("dirs", total_dirs, collected.selection_capped, shown),
    );
    if collected.selection_capped {
        receipt.add_cap(ReceiptCap::scope(
            "candidate_files",
            Some(max_files_counted),
        ));
    }
    if shown < total_dirs {
        receipt.add_cap(ReceiptCap::output("dirs", Some(max)));
    }
    text_clamp.add_receipt_cap(&mut receipt);
    receipt.insert("depth", json!(depth));
    receipt.insert("files_counted", json!(collected.files.len()));
    nested_repos_receipt_fields(&mut receipt, &collected.nested_repos_entered);
    let capped = !receipt.complete();
    if cli.json {
        receipt.insert(
            "dirs",
            json!(
                rendered_dirs
                    .iter()
                    .map(|(path, display, files)| {
                        json!({"path": path, "display": display, "files": files})
                    })
                    .collect::<Vec<_>>()
            ),
        );
        emit_json_checked(cli, receipt)
    } else {
        let mut stdout = io::stdout();
        writeln!(stdout, "[contextmink] dirs depth={depth}")?;
        if counts.is_empty() {
            writeln!(stdout, "no_files")?;
        }
        for (_, display, _) in rendered_dirs {
            writeln!(stdout, "{display}")?;
        }
        write_nested_repos_note(&mut stdout, &collected.nested_repos_entered)?;
        if capped {
            writeln!(
                stdout,
                "[contextmink] capped dirs output; narrow the path or lower --depth before treating this as complete."
            )?;
        }
        write_receipt_checked(cli, receipt)
    }
}

fn write_nested_repos_note(stdout: &mut impl Write, nested: &[String]) -> Result<()> {
    if nested.is_empty() {
        return Ok(());
    }
    let mut listed = nested
        .iter()
        .take(NESTED_REPOS_HUMAN_LIMIT)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if nested.len() > NESTED_REPOS_HUMAN_LIMIT {
        listed.push_str(&format!(
            ", ... and {} more",
            nested.len() - NESTED_REPOS_HUMAN_LIMIT
        ));
    }
    writeln!(
        stdout,
        "[contextmink] entered {} nested Git repo(s): {listed}",
        nested.len()
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_grep(
    cli: &Cli,
    config: &ContextConfig,
    args: &[String],
    pattern_arg: Option<&str>,
    pattern_file: Option<&Path>,
    literal: bool,
    ignore_case: bool,
    globs: &[String],
    extensions: &[String],
    with_excluded: bool,
    with_git_ignored: bool,
    skip_nested_repos: bool,
    quiet: bool,
    caps: &GrepCaps,
) -> Result<()> {
    let (pattern, effective_paths) = if pattern_arg.is_some() || pattern_file.is_some() {
        (None, paths_or_current_dir(&string_args_to_paths(args)))
    } else {
        let Some((pattern, paths)) = args.split_first() else {
            return Err(anyhow!(
                "grep requires PATTERN, --pattern <pattern>, or --pattern-file <file>"
            ));
        };
        (
            Some(pattern.as_str()),
            paths_or_current_dir(&string_args_to_paths(paths)),
        )
    };
    let pattern =
        collect_single_text_source("grep pattern", pattern.or(pattern_arg), pattern_file, true)?;
    let matcher = TextMatcher::new(&pattern, literal, ignore_case)?;
    command_grep_with_matcher(
        cli,
        config,
        "grep",
        matcher,
        &effective_paths,
        globs,
        extensions,
        with_excluded,
        with_git_ignored,
        skip_nested_repos,
        quiet,
        caps,
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
    globs: &[String],
    extensions: &[String],
    with_excluded: bool,
    with_git_ignored: bool,
    skip_nested_repos: bool,
    quiet: bool,
    caps: &GrepCaps,
) -> Result<()> {
    if caps.max_matching_files == 0 {
        return Err(anyhow!(
            "{command_name} --max-matching-files must be greater than zero"
        ));
    }
    if caps.max_content_files == 0 {
        return Err(anyhow!(
            "{command_name} --max-content-files must be greater than zero"
        ));
    }
    if caps.max_content_bytes == Some(0) {
        return Err(anyhow!(
            "{command_name} --max-content-bytes must be greater than zero"
        ));
    }
    let collected = collect_files(
        paths,
        config,
        &CollectOptions {
            globs,
            path_terms: &[],
            extensions,
            with_excluded,
            with_git_ignored,
            skip_nested_repos,
            max_selected_files: caps.max_content_files,
        },
    )?;
    let candidate_file_scope_capped = collected.selection_capped;
    let total_candidate_files = collected.total_seen;
    let candidate_files_selected = collected.files.len();
    let nested_repos_entered = collected.nested_repos_entered;
    let content_admission = admit_content_files(collected.files, caps.max_content_bytes)?;
    let content_bytes_capped = content_admission.capped;
    let content_bytes_admitted = content_admission.bytes_admitted;
    let content_files_admitted = content_admission.files.len();

    let limits = ScanLimits {
        lines_per_file: caps.lines_per_file,
        context: caps.context,
        max_line_chars: caps.max_line_chars,
        max_file_bytes: caps.max_file_bytes,
    };
    let mut matched_so_far = 0usize;
    let (scans, content_files_scanned) =
        scan_files_ordered(content_admission.files, &matcher, &limits, |scan| {
            if matches!(scan, FileScan::Matched { .. }) {
                matched_so_far += 1;
            }
            matched_so_far >= caps.max_matching_files
        })?;
    let mut matches: Vec<FileMatch> = Vec::new();
    let mut matching_lines_total = 0usize;
    let mut skipped: Vec<SkippedFile> = Vec::new();
    let mut skipped_large = 0usize;
    for scan in scans {
        match scan {
            FileScan::Matched {
                path,
                matching_lines,
                samples,
                sample_matching_lines_omitted,
            } => {
                matching_lines_total += matching_lines;
                matches.push(FileMatch {
                    path,
                    count: matching_lines,
                    samples,
                    sample_matching_lines_omitted,
                });
            }
            FileScan::SkippedLarge { path, bytes } => {
                skipped_large += 1;
                skipped.push(SkippedFile {
                    path,
                    reason: "large",
                    bytes: Some(bytes),
                });
            }
            FileScan::SkippedBinary { path } => {
                skipped.push(SkippedFile {
                    path,
                    reason: "binary",
                    bytes: None,
                });
            }
            FileScan::NoMatch => {}
        }
    }
    let content_match_capped =
        matches.len() >= caps.max_matching_files && content_files_scanned < content_files_admitted;
    let matching_files_total_is_lower_bound = candidate_file_scope_capped
        || content_match_capped
        || content_bytes_capped
        || skipped_large > 0;
    let matching_lines_total_is_lower_bound = matching_files_total_is_lower_bound;
    let files_shown = if quiet {
        0
    } else {
        min(matches.len(), caps.max_files)
    };
    let sample_lines_available = matches
        .iter()
        .take(files_shown)
        .fold(0usize, |total, row| total.saturating_add(row.samples.len()));
    let sample_lines_shown = min(sample_lines_available, caps.max_sample_lines);
    let sample_lines_capped = sample_lines_shown < sample_lines_available;
    let sample_matching_lines_omitted = matches
        .iter()
        .take(files_shown)
        .map(|row| row.sample_matching_lines_omitted)
        .sum::<usize>();
    let mut emitted_samples_remaining = sample_lines_shown;
    let mut emitted_sample_text_truncated = false;
    for row in matches.iter().take(files_shown) {
        for sample in &row.samples {
            if emitted_samples_remaining == 0 {
                break;
            }
            emitted_samples_remaining -= 1;
            emitted_sample_text_truncated |= sample.text_truncated;
        }
    }
    let mut text_clamp = TextClamp::new(caps.max_line_chars);
    let rendered_pattern = text_clamp.clamp(&matcher.label());
    let rendered_paths = matches
        .iter()
        .take(files_shown)
        .map(|row| text_clamp.clamp(&display_path(&row.path)))
        .collect::<Vec<_>>();

    let mut receipt = Receipt::new(
        command_name,
        config.profile.as_deref(),
        ReceiptResult::new(
            "matching_files",
            matches.len(),
            matching_files_total_is_lower_bound,
            files_shown,
        ),
    );
    if candidate_file_scope_capped {
        receipt.add_cap(ReceiptCap::scope(
            "candidate_files",
            Some(caps.max_content_files),
        ));
    }
    if content_match_capped {
        receipt.add_cap(ReceiptCap::scope(
            "matching_files",
            Some(caps.max_matching_files),
        ));
    }
    if content_bytes_capped {
        receipt.add_cap(ReceiptCap::scope_bytes(
            "content_bytes",
            caps.max_content_bytes
                .expect("content byte cap exists when admission is capped"),
        ));
    }
    if skipped_large > 0 {
        receipt.add_cap(ReceiptCap::scope_bytes("file_bytes", caps.max_file_bytes));
    }
    if !quiet && files_shown < matches.len() {
        receipt.add_cap(ReceiptCap::output("matching_files", Some(caps.max_files)));
    }
    if !quiet && sample_lines_capped {
        receipt.add_cap(ReceiptCap::output(
            "sample_lines",
            Some(caps.max_sample_lines),
        ));
    }
    if !quiet && sample_matching_lines_omitted > 0 {
        receipt.add_cap(ReceiptCap::output(
            "sample_matching_lines_per_file",
            Some(caps.lines_per_file),
        ));
    }
    text_clamp.record_truncated(emitted_sample_text_truncated);
    receipt.insert("pattern", json!(rendered_pattern));
    receipt.insert("matching_lines_total", json!(matching_lines_total));
    receipt.insert(
        "matching_lines_total_is_lower_bound",
        json!(matching_lines_total_is_lower_bound),
    );
    receipt.insert("sample_lines_shown", json!(sample_lines_shown));
    receipt.insert(
        "sample_matching_lines_omitted",
        json!(sample_matching_lines_omitted),
    );
    receipt.insert("candidate_files_total", json!(total_candidate_files));
    receipt.insert("candidate_files_selected", json!(candidate_files_selected));
    receipt.insert("content_files_admitted", json!(content_files_admitted));
    receipt.insert("content_files_scanned", json!(content_files_scanned));
    if let Some(content_bytes_admitted) = content_bytes_admitted {
        receipt.insert("content_bytes_admitted", json!(content_bytes_admitted));
    }
    insert_skip_fields(&mut receipt, &skipped, &mut text_clamp);
    nested_repos_receipt_fields(&mut receipt, &nested_repos_entered);
    receipt.insert(
        "no_match_scope",
        json!(no_match_scope(matches.is_empty(), receipt.scope_complete())),
    );
    if quiet {
        receipt.insert("quiet", json!(true));
    }
    if cli.json {
        let mut files_json = Vec::new();
        let mut samples_remaining = sample_lines_shown;
        for (row, path) in matches.iter().take(files_shown).zip(rendered_paths.iter()) {
            let mut samples = Vec::new();
            for sample in &row.samples {
                if samples_remaining == 0 {
                    break;
                }
                samples_remaining -= 1;
                samples.push(json!({
                    "line": sample.line,
                    "text": sample.text,
                    "is_match": sample.is_match,
                }));
            }
            files_json.push(json!({
                "path": path,
                "matching_lines": row.count,
                "sample_matching_lines_omitted": row.sample_matching_lines_omitted,
                "samples": samples,
            }));
        }
        if !quiet {
            receipt.insert("matching_files", json!(files_json));
        }
        text_clamp.add_receipt_cap(&mut receipt);
        emit_json_checked(cli, receipt)
    } else {
        let mut stdout = io::stdout();
        writeln!(stdout, "[contextmink] grep pattern={rendered_pattern}")?;
        writeln!(
            stdout,
            "matching_files_total={} matching_files_total_is_lower_bound={} matching_lines_total={} matching_lines_total_is_lower_bound={}",
            matches.len(),
            matching_files_total_is_lower_bound,
            matching_lines_total,
            matching_lines_total_is_lower_bound
        )?;
        writeln!(
            stdout,
            "candidate_files_total={} candidate_files_selected={} content_files_scanned={} skipped_large_or_binary={}",
            total_candidate_files,
            candidate_files_selected,
            content_files_scanned,
            skipped.len()
        )?;
        write_nested_repos_note(&mut stdout, &nested_repos_entered)?;
        write_skipped_note(&mut stdout, &skipped, &mut text_clamp)?;
        text_clamp.add_receipt_cap(&mut receipt);
        let scope_complete = receipt.scope_complete();
        let output_truncated = receipt.output_truncated();
        if matches.is_empty() {
            writeln!(stdout, "no_matches")?;
            if !scope_complete {
                writeln!(
                    stdout,
                    "[contextmink] scan was capped or skipped large files; no matches were found in the scanned subset only."
                )?;
            }
        } else if !quiet {
            writeln!(stdout, "file_counts:")?;
            for (row, path) in matches.iter().take(files_shown).zip(rendered_paths.iter()) {
                writeln!(stdout, "  {path}:{}", row.count)?;
            }
        }
        if caps.lines_per_file > 0 && files_shown > 0 {
            if !quiet {
                writeln!(stdout, "sample_lines:")?;
            }
            let mut samples_remaining = sample_lines_shown;
            'samples: for (row, path) in matches.iter().take(files_shown).zip(rendered_paths.iter())
            {
                for sample in &row.samples {
                    if samples_remaining == 0 {
                        if !quiet {
                            writeln!(
                                stdout,
                                "[contextmink] capped sample lines at {}; narrow the query.",
                                caps.max_sample_lines
                            )?;
                        }
                        break 'samples;
                    }
                    if !quiet {
                        let separator = if sample.is_match { ':' } else { '-' };
                        writeln!(
                            stdout,
                            "  {}:{}{}{}",
                            path, sample.line, separator, sample.text
                        )?;
                    }
                    samples_remaining -= 1;
                }
            }
        }
        if !scope_complete || output_truncated {
            writeln!(
                stdout,
                "[contextmink] capped grep output or scan; narrow the path or pattern before treating this as complete."
            )?;
        }
        write_receipt_checked(cli, receipt)
    }
}

fn insert_skip_fields(receipt: &mut Receipt, skipped: &[SkippedFile], text_clamp: &mut TextClamp) {
    receipt.insert("skipped_large_or_binary", json!(skipped.len()));
    // Split counts: only skipped *large* files (unexamined text) mark match
    // totals as lower bounds; binary skips are out-of-domain for text search.
    let large = skipped.iter().filter(|skip| skip.reason == "large").count();
    receipt.insert("skipped_large", json!(large));
    receipt.insert("skipped_binary", json!(skipped.len() - large));
    receipt.insert(
        "skipped_files_sample",
        json!(
            skipped
                .iter()
                .take(SKIPPED_FILES_SAMPLE_LIMIT)
                .map(|skip| {
                    json!({
                        "path": text_clamp.clamp(&display_path(&skip.path)),
                        "reason": skip.reason,
                        "bytes": skip.bytes,
                    })
                })
                .collect::<Vec<_>>()
        ),
    );
}

fn write_skipped_note(
    stdout: &mut impl Write,
    skipped: &[SkippedFile],
    text_clamp: &mut TextClamp,
) -> Result<()> {
    if skipped.is_empty() {
        return Ok(());
    }
    let sample = skipped
        .iter()
        .take(SKIPPED_FILES_SAMPLE_LIMIT)
        .map(|skip| format!("{} ({})", display_path(&skip.path), skip.reason))
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(
        stdout,
        "[contextmink] skipped {} file(s) without scanning: {}",
        skipped.len(),
        text_clamp.clamp(&sample)
    )?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SliceWindowRequest {
    Inclusive { start: usize, end: usize },
    Tail { lines: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SliceWindowPlan {
    start: usize,
    end: usize,
    output_truncated: bool,
}

fn plan_slice_window(
    total_lines: usize,
    request: SliceWindowRequest,
    max_lines: usize,
) -> Result<SliceWindowPlan> {
    if max_lines == 0 {
        return Err(anyhow!("slice --max-lines must be greater than zero"));
    }
    match request {
        SliceWindowRequest::Inclusive { start, end } => {
            if start == 0 || end == 0 {
                return Err(anyhow!("slice line numbers are 1-based; line 0 is invalid"));
            }
            if end < start {
                return Err(anyhow!("slice end must be greater than or equal to start"));
            }
            let capped_end = min(end, start.saturating_add(max_lines).saturating_sub(1));
            let available_end = min(end, total_lines);
            let rendered_end = if start > total_lines {
                start.saturating_sub(1)
            } else {
                min(capped_end, total_lines)
            };
            Ok(SliceWindowPlan {
                start,
                end: rendered_end,
                output_truncated: available_end > capped_end,
            })
        }
        SliceWindowRequest::Tail { lines } => {
            if lines == 0 {
                return Err(anyhow!("slice --tail must be greater than zero"));
            }
            let requested_lines = min(lines, total_lines);
            let rendered_lines = min(requested_lines, max_lines);
            let start = if rendered_lines == 0 {
                1
            } else {
                total_lines - rendered_lines + 1
            };
            Ok(SliceWindowPlan {
                start,
                end: total_lines,
                output_truncated: rendered_lines < requested_lines,
            })
        }
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
    tail: Option<usize>,
    lines: usize,
    max_lines: usize,
    max_line_chars: usize,
    char_start: Option<usize>,
    chars: usize,
) -> Result<()> {
    if let Some(char_start) = char_start {
        if range.is_some() || tail.is_some() {
            return Err(anyhow!(
                "slice --char-start cannot be combined with --range or --tail"
            ));
        }
        return command_slice_chars(cli, config, file, char_start, chars);
    }
    let request = if let Some(tail) = tail {
        if range.is_some() || start != 1 || end.is_some() {
            return Err(anyhow!(
                "slice --tail cannot be combined with --range, --start, or --end"
            ));
        }
        SliceWindowRequest::Tail { lines: tail }
    } else {
        let (start, end) = if let Some(range) = range {
            if start != 1 || end.is_some() {
                return Err(anyhow!(
                    "slice --range cannot be combined with --start or --end"
                ));
            }
            parse_line_range(range)?
        } else {
            (start, end)
        };
        if lines == 0 {
            return Err(anyhow!("slice --lines must be greater than zero"));
        }
        let requested_end = match end {
            Some(end) => end,
            None => start
                .checked_add(lines - 1)
                .ok_or_else(|| anyhow!("slice line window exceeds supported line numbers"))?,
        };
        SliceWindowRequest::Inclusive {
            start,
            end: requested_end,
        }
    };
    // Validate zero and inverted windows before paying for a whole-file pass.
    plan_slice_window(0, request, max_lines)?;
    let retained_lines = match request {
        SliceWindowRequest::Inclusive { start, end } => {
            let capped_end = min(end, start.saturating_add(max_lines).saturating_sub(1));
            capped_end.saturating_sub(start).saturating_add(1)
        }
        SliceWindowRequest::Tail { lines } => min(lines, max_lines),
    };
    let mut rendered = VecDeque::with_capacity(retained_lines);
    let mut total_lines = 0usize;
    let mut suspects = crate::encoding::EncodingSuspects::default();
    let mut text_clamp = TextClamp::new(max_line_chars);
    let visited = crate::encoding::visit_file_lines(file, u64::MAX, |line_number, line| {
        total_lines = line_number;
        suspects.merge(crate::encoding::scan_encoding_suspects_from_line(
            line,
            false,
            line_number,
        ));
        match request {
            SliceWindowRequest::Inclusive { start, end } => {
                let capped_end = min(end, start.saturating_add(max_lines).saturating_sub(1));
                if (start..=capped_end).contains(&line_number) {
                    rendered.push_back((line_number, text_clamp.clamp(line)));
                }
            }
            SliceWindowRequest::Tail { .. } => {
                if retained_lines > 0 {
                    if rendered.len() == retained_lines {
                        rendered.pop_front();
                    }
                    rendered.push_back((line_number, text_clamp.clamp(line)));
                }
            }
        }
    })
    .with_context(|| format!("failed to read {}", file.display()))?;
    let encoding = match visited {
        crate::encoding::VisitedFileText::Text { encoding } => encoding,
        crate::encoding::VisitedFileText::SkippedBinary => {
            return Err(anyhow!(
                "{} contains NUL bytes and does not decode as text",
                file.display()
            ));
        }
        crate::encoding::VisitedFileText::SkippedLarge { .. } => {
            unreachable!("slice line visitor uses an unbounded byte limit")
        }
    };
    let plan = plan_slice_window(total_lines, request, max_lines)?;
    let shown = rendered.len();
    let displayed_end = rendered
        .back()
        .map(|(number, _)| *number)
        .unwrap_or_else(|| plan.start.saturating_sub(1));
    let mut receipt = Receipt::new(
        "slice",
        config.profile.as_deref(),
        ReceiptResult::new("lines", total_lines, false, shown),
    );
    if plan.output_truncated {
        receipt.add_cap(ReceiptCap::output("lines", Some(max_lines)));
    }
    text_clamp.add_receipt_cap(&mut receipt);
    receipt.insert("path", json!(display_path(file)));
    receipt.insert("mode", json!("lines"));
    receipt.insert("encoding", json!(encoding));
    receipt.insert("start", json!(plan.start));
    receipt.insert("end", json!(displayed_end));
    receipt.insert("total_lines", json!(total_lines));
    // The line visitor inspects the complete decoded scope; the field only
    // exists when something was found, so clean files cost nothing.
    if !suspects.is_empty() {
        receipt.insert("encoding_suspects", suspects.receipt_value());
    }
    if cli.json {
        receipt.insert(
            "lines",
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
        emit_json_checked(cli, receipt)
    } else {
        let mut stdout = io::stdout();
        for (line, text) in rendered {
            writeln!(stdout, "{line}: {text}")?;
        }
        if plan.output_truncated {
            writeln!(
                stdout,
                "[contextmink] capped slice at {max_lines} lines; request a narrower range."
            )?;
        }
        if !suspects.is_empty() {
            writeln!(stdout, "{}", suspects.human_note())?;
        }
        write_receipt_checked(cli, receipt)
    }
}

fn command_slice_chars(
    cli: &Cli,
    config: &ContextConfig,
    file: &Path,
    char_start: usize,
    chars: usize,
) -> Result<()> {
    if chars == 0 {
        return Err(anyhow!("slice --chars must be greater than zero"));
    }
    let (text, encoding) = crate::encoding::read_required_text(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let total_chars = text.chars().count();
    let shown_text = text
        .chars()
        .skip(char_start)
        .take(chars)
        .collect::<String>();
    let shown = shown_text.chars().count();
    let mut receipt = Receipt::new(
        "slice",
        config.profile.as_deref(),
        ReceiptResult::new("chars", total_chars, false, shown),
    );
    receipt.insert("path", json!(display_path(file)));
    receipt.insert("mode", json!("chars"));
    receipt.insert("encoding", json!(encoding));
    receipt.insert("char_start", json!(char_start));
    receipt.insert("total_chars", json!(total_chars));
    if cli.json {
        receipt.insert("text", json!(shown_text));
        return emit_json_checked(cli, receipt);
    }
    let mut stdout = io::stdout();
    write!(stdout, "{}", shown_text)?;
    if !shown_text.ends_with('\n') {
        writeln!(stdout)?;
    }
    write_receipt_checked(cli, receipt)
}

#[cfg(test)]
mod tests;
