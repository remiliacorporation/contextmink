use std::io::{self, Read, Write};
use std::process::{Command as ProcessCommand, Stdio};
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::config::ContextConfig;
use crate::output::{base_receipt, clamp_text, emit_json, write_receipt_checked};

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

pub(crate) fn command_capture(
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
            "[contextmink] capped captured output; rerun the underlying command with native filters or raise caps only after confirming command scope."
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
