use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Command as ProcessCommand, Stdio};
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::config::ContextConfig;
use crate::output::{
    Receipt, ReceiptCap, ReceiptResult, clamp_text, emit_json, fail_after_receipt, write_receipt,
};
use crate::process_boundary::prepare_command;
use crate::process_supervision::{configure as configure_supervised_command, supervise};

struct RawCapturedStream {
    /// Leading share of the stream's `max_bytes` budget.
    head: Vec<u8>,
    /// Trailing share of the stream's `max_bytes` budget.
    tail: Vec<u8>,
    /// Absolute byte offset where `tail` begins.
    tail_start: usize,
    total_bytes: usize,
    total_lines: usize,
}

struct CapturedStream {
    display_text: String,
    retained_text: String,
    total_bytes: usize,
    captured_bytes: usize,
    total_lines: usize,
    shown_lines: usize,
    head_lines: usize,
    tail_lines: usize,
    omitted_lines: usize,
    byte_truncated: bool,
    line_truncated: bool,
    char_truncated: bool,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_capture(
    cli: &Cli,
    config: &ContextConfig,
    max_lines: usize,
    max_bytes: usize,
    max_line_chars: usize,
    script: bool,
    expect_exit: &[String],
    receipt_out: Option<&PathBuf>,
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
    let expected_exit_codes = parse_expected_exit_codes(expect_exit)?;

    // Same blocking deny-list as contextmink-bridge: capture spawn
    // arbitrary commands and must refuse destructive argv before spawn.
    match crate::destructive_guard::evaluate_argv(
        argv,
        &config.destructive_guard,
        crate::destructive_guard::destructive_override_active(),
    ) {
        crate::destructive_guard::DenyDecision::Allow => {}
        crate::destructive_guard::DenyDecision::AllowWithOverride { message } => {
            eprintln!(
                "contextmink: WARNING: {}=1 break-glass override active (human operators only); \
                 capturing a command the destructive deny-list would block: {message}",
                crate::destructive_guard::ALLOW_DESTRUCTIVE_ENV
            );
        }
        crate::destructive_guard::DenyDecision::Deny { message } => {
            return Err(anyhow!("destructive command blocked: {message}"));
        }
    }

    let started = Instant::now();
    let target_cwd =
        std::env::current_dir().context("failed to resolve capture working directory")?;
    let prepared = prepare_command(program, args, &target_cwd, script, false)
        .map_err(|error| anyhow!(error))?;
    let execution_mode = prepared.execution_mode;
    let effective_argv = prepared.effective_argv.clone();
    let mut child = spawn_captured_child(prepared.command, program, execution_mode)?;
    let child_supervisor = supervise(&mut child)?;

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
    drop(child_supervisor);
    let stdout_raw = stdout_handle
        .join()
        .map_err(|_| anyhow!("stdout capture thread panicked"))?
        .context("failed to read captured stdout")?;
    let stderr_raw = stderr_handle
        .join()
        .map_err(|_| anyhow!("stderr capture thread panicked"))?
        .context("failed to read captured stderr")?;
    let (stdout_lines, stderr_lines) =
        capture_line_budgets(max_lines, stdout_raw.total_lines, stderr_raw.total_lines);
    let stdout = render_captured_stream(stdout_raw, stdout_lines, max_line_chars);
    let stderr = render_captured_stream(stderr_raw, stderr_lines, max_line_chars);
    assert!(
        stdout.omitted_lines == 0 || stdout.byte_truncated || stdout.line_truncated,
        "capture omitted stdout lines without a byte or line retention boundary"
    );
    assert!(
        stderr.omitted_lines == 0 || stderr.byte_truncated || stderr.line_truncated,
        "capture omitted stderr lines without a byte or line retention boundary"
    );
    let duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let shown = stdout.shown_lines + stderr.shown_lines;
    let total = stdout.total_lines + stderr.total_lines;
    let truncated = captured_stream_truncated(&stdout) || captured_stream_truncated(&stderr);
    let mut receipt = Receipt::new(
        "capture",
        config.profile.as_deref(),
        ReceiptResult::new("lines", total, false, shown),
    );
    if stdout.line_truncated || stderr.line_truncated {
        receipt.add_cap(ReceiptCap::output("lines", Some(max_lines)));
    }
    if stdout.byte_truncated || stderr.byte_truncated {
        receipt.add_cap(ReceiptCap::output("bytes_per_stream", Some(max_bytes)));
    }
    if stdout.char_truncated || stderr.char_truncated {
        receipt.add_cap(ReceiptCap::output("line_characters", Some(max_line_chars)));
    }
    receipt.insert("argv", json!(argv));
    receipt.insert("effective_argv", json!(effective_argv));
    receipt.insert("execution_mode", json!(execution_mode));
    receipt.insert("child_exit_code", json!(status.code()));
    receipt.insert("child_exit_zero", json!(status.success()));
    let exit_expected = status
        .code()
        .map(|code| expected_exit_codes.contains(&code))
        .unwrap_or(false);
    receipt.insert(
        "expected_exit_codes",
        json!(expected_exit_codes.iter().copied().collect::<Vec<_>>()),
    );
    receipt.insert("exit_expected", json!(exit_expected));
    receipt.insert("child_duration_ms", json!(duration_ms));
    receipt.insert("stdout", captured_stream_json(&stdout));
    receipt.insert("stderr", captured_stream_json(&stderr));
    // Double-encode proof only: child output may legitimately carry lossy or
    // control bytes, but a CP1252 round-trip that re-decodes as UTF-8 means
    // the child wrote UTF-8 through a CP1252 boundary (the classic
    // PowerShell 5.1 hazard). Field exists only when found.
    let mut suspects = crate::encoding::scan_encoding_suspects(&stdout.retained_text, true);
    let stderr_suspects = crate::encoding::scan_encoding_suspects(&stderr.retained_text, true);
    suspects.double_encoded += stderr_suspects.double_encoded;
    if suspects.sample.is_none() {
        suspects.sample = stderr_suspects.sample;
    }
    if !suspects.is_empty() {
        receipt.insert("encoding_suspects", suspects.receipt_value());
    }

    let mut full_receipt = receipt.clone();
    full_receipt.insert("stdout_text", json!(stdout.display_text));
    full_receipt.insert("stderr_text", json!(stderr.display_text));
    let sidecar_result =
        receipt_out.map(|path| write_capture_receipt(path, &full_receipt.clone().into_value()));
    let scope_complete = receipt.scope_complete();
    let output_truncated = receipt.output_truncated();

    if cli.json {
        emit_json(full_receipt.into_value())?;
        return finish_capture(
            cli,
            sidecar_result,
            scope_complete,
            output_truncated,
            exit_expected,
            &status,
        );
    }

    let mut out = io::stdout();
    writeln!(
        out,
        "[contextmink] capture command={} child_exit_code={} child_exit_zero={} duration_ms={}",
        clamp_text(&format!("{argv:?}"), 500),
        status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "null".to_string()),
        status.success(),
        duration_ms
    )?;
    writeln!(
        out,
        "execution_mode={execution_mode} effective_command={}",
        clamp_text(&format!("{effective_argv:?}"), 500)
    )?;
    writeln!(
        out,
        "stdout: shown_lines={} total_lines={} captured_bytes={} total_bytes={}",
        stdout.shown_lines, stdout.total_lines, stdout.captured_bytes, stdout.total_bytes
    )?;
    if !stdout.display_text.is_empty() {
        writeln!(out, "{}", stdout.display_text)?;
    }
    writeln!(
        out,
        "stderr: shown_lines={} total_lines={} captured_bytes={} total_bytes={}",
        stderr.shown_lines, stderr.total_lines, stderr.captured_bytes, stderr.total_bytes
    )?;
    if !stderr.display_text.is_empty() {
        writeln!(out, "{}", stderr.display_text)?;
    }
    if truncated {
        writeln!(
            out,
            "[contextmink] capped captured output; rerun the underlying command with native filters or raise caps only after confirming command scope."
        )?;
    }
    if !suspects.is_empty() {
        writeln!(out, "{}", suspects.human_note())?;
    }
    write_receipt(receipt)?;
    finish_capture(
        cli,
        sidecar_result,
        scope_complete,
        output_truncated,
        exit_expected,
        &status,
    )
}

fn finish_capture(
    cli: &Cli,
    sidecar_result: Option<Result<()>>,
    scope_complete: bool,
    output_truncated: bool,
    exit_expected: bool,
    status: &std::process::ExitStatus,
) -> Result<()> {
    let strict_result = fail_after_receipt(cli, scope_complete, output_truncated);
    if !exit_expected {
        if let Some(Err(error)) = sidecar_result.as_ref() {
            eprintln!("contextmink capture sidecar error: {error:#}");
        }
        if let Err(error) = strict_result.as_ref() {
            eprintln!("contextmink capture strictness error: {error:#}");
        }
        propagate_unexpected_child_exit(false, status)?;
        unreachable!("unexpected child exit propagation terminates the process");
    }
    if let Some(result) = sidecar_result {
        result?;
    }
    strict_result
}

/// The receipt carrying the child status has already been emitted. Every
/// status outside `--expect-exit` then becomes contextmink's own exit so a
/// capture cannot silently turn a failed child into a successful workflow.
fn propagate_unexpected_child_exit(
    exit_expected: bool,
    status: &std::process::ExitStatus,
) -> Result<()> {
    if exit_expected {
        return Ok(());
    }
    #[cfg(unix)]
    let code = status.code().unwrap_or_else(|| {
        use std::os::unix::process::ExitStatusExt;
        status.signal().map_or(1, |signal| 128 + signal)
    });
    #[cfg(not(unix))]
    let code = status.code().unwrap_or(1);
    io::stdout()
        .flush()
        .context("failed to flush stdout before propagating child exit")?;
    std::process::exit(code);
}

fn parse_expected_exit_codes(raw: &[String]) -> Result<BTreeSet<i32>> {
    if raw.is_empty() {
        return Ok(BTreeSet::from([0]));
    }
    let mut codes = BTreeSet::new();
    for value in raw {
        for part in value.split(',') {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                return Err(anyhow!("capture --expect-exit contains an empty exit code"));
            }
            let code = trimmed
                .parse::<i32>()
                .with_context(|| format!("invalid capture --expect-exit code {trimmed:?}"))?;
            codes.insert(code);
        }
    }
    if codes.is_empty() {
        Err(anyhow!("capture --expect-exit requires at least one code"))
    } else {
        Ok(codes)
    }
}

fn write_capture_receipt(path: &PathBuf, receipt: &Value) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut bytes = serde_json::to_vec_pretty(receipt)?;
    bytes.push(b'\n');
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

fn spawn_captured_child(
    mut command: ProcessCommand,
    requested_program: &str,
    execution_mode: &str,
) -> Result<std::process::Child> {
    configure_supervised_command(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.spawn().with_context(|| {
        format!(
            "failed to spawn captured command {requested_program:?} in {execution_mode} mode; use `capture --script -- <script> ...` for a Bash script without a shebang"
        )
    })
}

/// Split a total `max_bytes` budget between the beginning and end of the
/// stream. Tool output puts its verdict at the end (test summaries, compiler
/// error totals), so keeping only the head would drop exactly the part an
/// agent needs most.
fn read_captured_stream<R: Read>(mut reader: R, max_bytes: usize) -> io::Result<RawCapturedStream> {
    let head_budget = max_bytes.div_ceil(2);
    let tail_budget = max_bytes / 2;
    let mut head = Vec::with_capacity(head_budget.min(8192));
    let mut tail: Vec<u8> = Vec::new();
    let mut tail_start = 0usize;
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
        for byte in &buffer[..read] {
            if *byte == b'\n' {
                newline_count += 1;
                last_was_newline = true;
            } else {
                last_was_newline = false;
            }
        }
        let head_remaining = head_budget.saturating_sub(head.len());
        if head_remaining > 0 {
            head.extend_from_slice(&buffer[..read.min(head_remaining)]);
        }
        if read > head_remaining {
            let overflow = &buffer[head_remaining..read];
            let overflow_start = total_bytes + head_remaining;
            if tail.is_empty() {
                tail_start = overflow_start;
            }
            if tail_budget > 0 {
                tail.extend_from_slice(overflow);
                if tail.len() > tail_budget {
                    let drop = tail.len() - tail_budget;
                    tail.drain(..drop);
                    tail_start += drop;
                }
            }
        }
        total_bytes += read;
    }

    let total_lines = newline_count + usize::from(saw_any && !last_was_newline);
    Ok(RawCapturedStream {
        head,
        tail,
        tail_start,
        total_bytes,
        total_lines,
    })
}

fn render_captured_stream(
    raw: RawCapturedStream,
    max_lines: usize,
    max_line_chars: usize,
) -> CapturedStream {
    let captured_bytes = raw.head.len() + raw.tail.len();
    let byte_truncated = raw.total_bytes > captured_bytes;
    let retained_text = retained_stream_text(&raw);
    if max_lines == 0 {
        return CapturedStream {
            display_text: String::new(),
            retained_text,
            total_bytes: raw.total_bytes,
            captured_bytes,
            total_lines: raw.total_lines,
            shown_lines: 0,
            head_lines: 0,
            tail_lines: 0,
            omitted_lines: raw.total_lines,
            byte_truncated,
            line_truncated: raw.total_lines > 0,
            char_truncated: false,
        };
    }
    // Bytes between the head and the retained tail were dropped whenever the
    // tail does not start exactly where the head ended.
    let tail_contiguous = raw.tail.is_empty() || raw.tail_start == raw.head.len();

    let mut clamp_state = ClampState::default();
    let (head_lines, tail_lines) = if tail_contiguous {
        (decode_lines(retained_text.as_bytes()).0, Vec::new())
    } else {
        let (mut head_lines, head_partial_last) = decode_lines(&raw.head);
        let tail_lines = decode_lines(&raw.tail).0;
        if head_partial_last && head_lines.is_empty() && !raw.head.is_empty() {
            head_lines.push(String::from_utf8_lossy(&raw.head).to_string());
        }
        (head_lines, tail_lines)
    };
    // A byte-retention gap can omit source lines without the line budget
    // binding; only retained display candidates participate in this cap.
    let retained_line_count = head_lines.len() + tail_lines.len();
    let line_truncated = retained_line_count > max_lines;

    let (display_text, head_shown, tail_shown, omitted_lines) = if tail_lines.is_empty() {
        if head_lines.len() <= max_lines {
            let shown = head_lines.len();
            let omitted = if byte_truncated {
                raw.total_lines.saturating_sub(shown)
            } else {
                0
            };
            let mut parts = head_lines
                .iter()
                .map(|line| clamp_state.clamp(line, max_line_chars))
                .collect::<Vec<_>>();
            if omitted > 0 {
                parts.push(clamp_state.clamp(
                    &format!("[contextmink] ... omitted {omitted} line(s) ..."),
                    max_line_chars,
                ));
            }
            (parts.join("\n"), shown, 0usize, omitted)
        } else {
            // Everything fits in the head buffer but exceeds the line budget:
            // split the budget so the end of the output (summaries, error
            // totals) stays visible.
            let head_budget = max_lines / 2;
            let tail_shown = max_lines - head_budget;
            let omitted = head_lines.len() - max_lines;
            let mut parts = Vec::new();
            parts.extend(
                head_lines
                    .iter()
                    .take(head_budget)
                    .map(|line| clamp_state.clamp(line, max_line_chars)),
            );
            if omitted > 0 {
                let marker = format!("[contextmink] ... omitted {omitted} line(s) ...");
                parts.push(clamp_state.clamp(&marker, max_line_chars));
            }
            parts.extend(
                head_lines
                    .iter()
                    .skip(head_lines.len() - tail_shown)
                    .map(|line| clamp_state.clamp(line, max_line_chars)),
            );
            (parts.join("\n"), head_budget, tail_shown, omitted)
        }
    } else {
        let head_budget = max_lines / 2;
        let head_shown = head_lines.len().min(head_budget);
        let tail_budget = max_lines.saturating_sub(head_shown).max(1);
        let tail_shown = tail_lines.len().min(tail_budget);
        let omitted = raw
            .total_lines
            .saturating_sub(head_shown)
            .saturating_sub(tail_shown);
        let omitted_bytes = raw.tail_start.saturating_sub(raw.head.len());
        let mut parts = Vec::new();
        parts.extend(
            head_lines
                .iter()
                .take(head_shown)
                .map(|line| clamp_state.clamp(line, max_line_chars)),
        );
        if omitted > 0 {
            let marker = format!("[contextmink] ... omitted {omitted} line(s) ...");
            parts.push(clamp_state.clamp(&marker, max_line_chars));
        } else if !tail_contiguous && omitted_bytes > 0 {
            let marker = format!("[contextmink] ... omitted {omitted_bytes} byte(s) ...");
            parts.push(clamp_state.clamp(&marker, max_line_chars));
        }
        parts.extend(
            tail_lines
                .iter()
                .skip(tail_lines.len() - tail_shown)
                .map(|line| clamp_state.clamp(line, max_line_chars)),
        );
        (parts.join("\n"), head_shown, tail_shown, omitted)
    };

    let shown_lines = (head_shown + tail_shown).min(raw.total_lines);
    CapturedStream {
        display_text,
        retained_text,
        total_bytes: raw.total_bytes,
        captured_bytes,
        total_lines: raw.total_lines,
        shown_lines,
        head_lines: head_shown,
        tail_lines: tail_shown,
        omitted_lines,
        byte_truncated,
        line_truncated,
        char_truncated: clamp_state.truncated,
    }
}

fn capture_line_budgets(
    max_lines: usize,
    stdout_total: usize,
    stderr_total: usize,
) -> (usize, usize) {
    match (stdout_total, stderr_total) {
        (0, _) => (0, max_lines.min(stderr_total)),
        (_, 0) => (max_lines.min(stdout_total), 0),
        _ if max_lines == 1 => (0, 1),
        _ => {
            let mut stdout = (max_lines / 2).min(stdout_total);
            let mut stderr = (max_lines - stdout).min(stderr_total);
            let remaining = max_lines - stdout - stderr;
            let stdout_extra = remaining.min(stdout_total - stdout);
            stdout += stdout_extra;
            stderr += (remaining - stdout_extra).min(stderr_total - stderr);
            (stdout, stderr)
        }
    }
}

fn retained_stream_text(raw: &RawCapturedStream) -> String {
    if raw.tail.is_empty() {
        return String::from_utf8_lossy(&raw.head).to_string();
    }
    if raw.tail_start == raw.head.len() {
        let mut bytes = raw.head.clone();
        bytes.extend_from_slice(&raw.tail);
        return String::from_utf8_lossy(&bytes).to_string();
    }

    let omitted_bytes = raw.tail_start.saturating_sub(raw.head.len());
    let head = String::from_utf8_lossy(&raw.head);
    let tail = String::from_utf8_lossy(&raw.tail);
    format!("{head}\n[contextmink] ... omitted {omitted_bytes} byte(s) ...\n{tail}")
}

#[derive(Default)]
struct ClampState {
    truncated: bool,
}

impl ClampState {
    fn clamp(&mut self, line: &str, max_line_chars: usize) -> String {
        if line.chars().count() > max_line_chars {
            self.truncated = true;
        }
        clamp_text(line, max_line_chars)
    }
}

/// Decode captured bytes into trimmed lines; the boolean reports whether the
/// final line lacked a terminating newline (possibly partial content).
fn decode_lines(bytes: &[u8]) -> (Vec<String>, bool) {
    let decoded = String::from_utf8_lossy(bytes);
    let partial_last = !decoded.is_empty() && !decoded.ends_with('\n');
    let lines = decoded
        .lines()
        .map(|line| line.trim_end_matches('\r').to_owned())
        .collect();
    (lines, partial_last)
}

fn captured_stream_truncated(stream: &CapturedStream) -> bool {
    stream.omitted_lines > 0
        || stream.byte_truncated
        || stream.line_truncated
        || stream.char_truncated
}

fn captured_stream_json(stream: &CapturedStream) -> Value {
    json!({
        "shown_lines": stream.shown_lines,
        "head_lines": stream.head_lines,
        "tail_lines": stream.tail_lines,
        "omitted_lines": stream.omitted_lines,
        "total_lines": stream.total_lines,
        "captured_bytes": stream.captured_bytes,
        "total_bytes": stream.total_bytes,
        "output_truncated": captured_stream_truncated(stream),
        "byte_truncated": stream.byte_truncated,
        "line_truncated": stream.line_truncated,
        "char_truncated": stream.char_truncated,
    })
}

#[cfg(test)]
mod tests;
