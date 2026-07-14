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
use crate::output::{base_receipt, clamp_text, emit_json, write_receipt_checked};
use crate::process_boundary::prepare_command;

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
    fail_with_child: bool,
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
    let child_supervisor = supervise_captured_child(&mut child)?;

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
    map.insert("execution_mode".to_string(), json!(execution_mode));
    map.insert("exit_code".to_string(), json!(status.code()));
    map.insert("success".to_string(), json!(status.success()));
    let exit_expected = status
        .code()
        .map(|code| expected_exit_codes.contains(&code))
        .unwrap_or(false);
    map.insert(
        "expected_exit_codes".to_string(),
        json!(expected_exit_codes.iter().copied().collect::<Vec<_>>()),
    );
    map.insert("exit_expected".to_string(), json!(exit_expected));
    map.insert("duration_ms".to_string(), json!(duration_ms));
    map.insert("stdout".to_string(), captured_stream_json(&stdout));
    map.insert("stderr".to_string(), captured_stream_json(&stderr));
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
        map.insert("encoding_suspects".to_string(), suspects.receipt_value());
    }

    let mut full_receipt = map.clone();
    full_receipt.insert("stdout_text".to_string(), json!(stdout.retained_text));
    full_receipt.insert("stderr_text".to_string(), json!(stderr.retained_text));
    if let Some(path) = receipt_out {
        write_capture_receipt(path, &Value::Object(full_receipt.clone()))?;
    }

    if cli.json {
        emit_json(Value::Object(full_receipt))?;
        exit_with_child(fail_with_child, exit_expected, &status)?;
        return Ok(());
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
    write_receipt_checked(cli, map)?;
    exit_with_child(fail_with_child, exit_expected, &status)
}

/// Opt-in child-status propagation for shell chaining. The receipt (carrying
/// `exit_code`/`success`) has already been emitted; a failed child then
/// becomes contextmink's own exit so `capture --fail-with-child -- cmd &&
/// next` gates on the child instead of always proceeding.
fn exit_with_child(
    fail_with_child: bool,
    exit_expected: bool,
    status: &std::process::ExitStatus,
) -> Result<()> {
    if !fail_with_child || exit_expected {
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
    configure_captured_command(&mut command);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    command.spawn().with_context(|| {
        format!(
            "failed to spawn captured command {requested_program:?} in {execution_mode} mode; use `capture --script -- <script> ...` for a Bash script without a shebang"
        )
    })
}

#[cfg(unix)]
fn configure_captured_command(command: &mut ProcessCommand) {
    use std::os::unix::process::CommandExt as _;

    command.process_group(0);
}

#[cfg(windows)]
fn configure_captured_command(command: &mut ProcessCommand) {
    use std::os::windows::process::CommandExt as _;
    use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

    command.creation_flags(CREATE_SUSPENDED);
}

#[cfg(windows)]
struct CapturedChildSupervisor(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for CapturedChildSupervisor {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn supervise_captured_child(child: &mut std::process::Child) -> Result<CapturedChildSupervisor> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        let code = unsafe { GetLastError() };
        let _ = child.kill();
        let _ = child.wait();
        return Err(anyhow!(
            "failed to create capture supervision job: win32 error {code}"
        ));
    }
    let mut limits = unsafe { std::mem::zeroed::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    let assigned = configured != 0
        && unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as _) } != 0;
    if !assigned {
        let code = unsafe { GetLastError() };
        unsafe {
            CloseHandle(job);
        }
        let _ = child.kill();
        let _ = child.wait();
        return Err(anyhow!(
            "failed to attach captured command to kill-on-close supervision job: win32 error {code}"
        ));
    }
    if let Err(error) = resume_captured_child(child) {
        unsafe {
            CloseHandle(job);
        }
        let _ = child.wait();
        return Err(error);
    }
    Ok(CapturedChildSupervisor(job))
}

#[cfg(windows)]
fn resume_captured_child(child: &std::process::Child) -> Result<()> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        let code = unsafe { GetLastError() };
        return Err(anyhow!(
            "failed to enumerate suspended capture threads: win32 error {code}"
        ));
    }
    let mut entry = unsafe { std::mem::zeroed::<THREADENTRY32>() };
    entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
    let mut available = unsafe { Thread32First(snapshot, &mut entry) };
    while available != 0 {
        if entry.th32OwnerProcessID == child.id() {
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if !thread.is_null() {
                let resumed = unsafe { ResumeThread(thread) };
                let code = if resumed == u32::MAX {
                    Some(unsafe { GetLastError() })
                } else {
                    None
                };
                unsafe {
                    CloseHandle(thread);
                    CloseHandle(snapshot);
                }
                if let Some(code) = code {
                    return Err(anyhow!(
                        "failed to resume capture child thread: win32 error {code}"
                    ));
                }
                return Ok(());
            }
        }
        available = unsafe { Thread32Next(snapshot, &mut entry) };
    }
    unsafe {
        CloseHandle(snapshot);
    }
    Err(anyhow!(
        "suspended capture child {} has no resumable primary thread",
        child.id()
    ))
}

#[cfg(unix)]
struct CapturedChildSupervisor {
    release_fd: std::os::fd::RawFd,
    watchdog_pid: libc::pid_t,
}

#[cfg(unix)]
impl Drop for CapturedChildSupervisor {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.release_fd);
            while libc::waitpid(self.watchdog_pid, std::ptr::null_mut(), 0) == -1 {
                if std::io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                    break;
                }
            }
        }
    }
}

#[cfg(unix)]
fn supervise_captured_child(child: &mut std::process::Child) -> Result<CapturedChildSupervisor> {
    let process_group = child.id() as libc::pid_t;
    let mut release_pipe = [0; 2];
    if unsafe { libc::pipe(release_pipe.as_mut_ptr()) } != 0 {
        let error = std::io::Error::last_os_error();
        terminate_captured_process_group(child);
        return Err(error).context("failed to create capture supervision pipe");
    }
    for fd in release_pipe {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1
        {
            let error = std::io::Error::last_os_error();
            unsafe {
                libc::close(release_pipe[0]);
                libc::close(release_pipe[1]);
            }
            terminate_captured_process_group(child);
            return Err(error).context("failed to configure capture supervision pipe");
        }
    }

    let watchdog_pid = unsafe { libc::fork() };
    if watchdog_pid == -1 {
        let error = std::io::Error::last_os_error();
        unsafe {
            libc::close(release_pipe[0]);
            libc::close(release_pipe[1]);
        }
        terminate_captured_process_group(child);
        return Err(error).context("failed to fork capture supervision watchdog");
    }
    if watchdog_pid == 0 {
        unsafe {
            libc::close(release_pipe[1]);
            let mut byte = 0_u8;
            loop {
                let read = libc::read(release_pipe[0], (&raw mut byte).cast(), 1);
                if read == 0 {
                    break;
                }
                // Keep the post-fork watchdog restricted to async-signal-safe
                // libc calls. The blocking pipe is valid for its lifetime, so
                // a negative read is transient and can be retried.
            }
            libc::close(release_pipe[0]);
            libc::kill(-process_group, libc::SIGKILL);
            libc::_exit(0);
        }
    }

    unsafe {
        libc::close(release_pipe[0]);
    }
    Ok(CapturedChildSupervisor {
        release_fd: release_pipe[1],
        watchdog_pid,
    })
}

#[cfg(unix)]
fn terminate_captured_process_group(child: &mut std::process::Child) {
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.wait();
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
    let (head_lines, head_partial_last) = decode_lines(&raw.head);
    let mut head_lines = head_lines;
    let mut tail_lines = Vec::new();
    if !raw.tail.is_empty() {
        let (lines, _) = decode_lines(&raw.tail);
        if head_partial_last && !tail_contiguous && head_lines.is_empty() && !raw.head.is_empty() {
            let head_fragment = String::from_utf8_lossy(&raw.head).to_string();
            head_lines.push(head_fragment);
        }
        tail_lines = lines;
    }

    let (display_text, head_shown, tail_shown, omitted_lines) = if tail_lines.is_empty() {
        if head_lines.len() <= max_lines {
            let shown = head_lines.len();
            let rendered = head_lines
                .iter()
                .map(|line| clamp_state.clamp(line, max_line_chars))
                .collect::<Vec<_>>()
                .join("\n");
            (rendered, shown, 0usize, 0usize)
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
                parts.push(format!("[contextmink] ... omitted {omitted} line(s) ..."));
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
            parts.push(format!("[contextmink] ... omitted {omitted} line(s) ..."));
        } else if !tail_contiguous && omitted_bytes > 0 {
            parts.push(format!(
                "[contextmink] ... omitted {omitted_bytes} byte(s) ..."
            ));
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
        line_truncated: omitted_lines > 0,
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
        "head_lines": stream.head_lines,
        "tail_lines": stream.tail_lines,
        "omitted_lines": stream.omitted_lines,
        "total_lines": stream.total_lines,
        "captured_bytes": stream.captured_bytes,
        "total_bytes": stream.total_bytes,
        "truncated": captured_stream_truncated(stream),
        "byte_truncated": stream.byte_truncated,
        "line_truncated": stream.line_truncated,
        "char_truncated": stream.char_truncated,
    })
}

#[cfg(test)]
mod tests;
