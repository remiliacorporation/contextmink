use super::*;

#[test]
fn byte_truncated_single_line_keeps_head_and_tail_fragments_visible() {
    let raw = RawCapturedStream {
        head: br#"{"rows":["#.to_vec(),
        tail: br#""tail"]}"#.to_vec(),
        tail_start: 128,
        total_bytes: 136,
        total_lines: 1,
    };

    let rendered = render_captured_stream(raw, 8, 120);

    assert!(rendered.byte_truncated);
    assert!(!rendered.display_text.is_empty());
    assert!(rendered.display_text.contains(r#"{"rows":["#));
    assert!(rendered.display_text.contains("[contextmink] ... omitted"));
    assert!(rendered.display_text.contains(r#""tail"]}"#));
    assert_eq!(rendered.shown_lines, 1);
    assert_eq!(rendered.total_lines, 1);
}

#[test]
fn byte_budget_is_the_total_retained_per_stream() {
    let raw = read_captured_stream(std::io::Cursor::new(vec![b'x'; 100]), 10).unwrap();

    assert_eq!(raw.total_bytes, 100);
    assert!(raw.head.len() + raw.tail.len() <= 10);
    let rendered = render_captured_stream(raw, 8, 120);
    assert_eq!(rendered.captured_bytes, 10);
    assert_eq!(rendered.shown_lines, 1);
    assert_eq!(rendered.total_lines, 1);
}

#[test]
fn line_budget_is_shared_across_stdout_and_stderr() {
    assert_eq!(capture_line_budgets(5, 10, 10), (2, 3));
    assert_eq!(capture_line_budgets(5, 1, 10), (1, 4));
    assert_eq!(capture_line_budgets(5, 10, 0), (5, 0));
    assert_eq!(capture_line_budgets(1, 10, 10), (0, 1));
}

#[cfg(windows)]
#[test]
fn capture_supervision_job_terminates_child_when_dropped() {
    let mut child = ProcessCommand::new("cmd.exe")
        .args(["/c", "ping", "-n", "30", "127.0.0.1", ">NUL"])
        .spawn()
        .expect("spawn supervised fixture");
    let supervisor = supervise_captured_child(&mut child).expect("supervise fixture child");
    drop(supervisor);
    for _ in 0..20 {
        if child.try_wait().expect("poll supervised child").is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    panic!("supervised child survived closing the capture job");
}

#[cfg(windows)]
#[test]
fn capture_supervision_job_contains_descendants_before_resume() {
    use std::io::BufRead as _;

    let script = "$p=Start-Process -FilePath powershell.exe -ArgumentList '-NoProfile','-Command','Start-Sleep 30' -PassThru; [Console]::Out.WriteLine($p.Id); [Console]::Out.Flush(); Wait-Process -Id $p.Id";
    let args = ["-NoProfile".into(), "-Command".into(), script.into()];
    let prepared = prepare_command(
        "powershell.exe",
        &args,
        &std::env::current_dir().unwrap(),
        false,
        false,
    )
    .expect("prepare suspended supervision fixture");
    let mut child = spawn_captured_child(prepared.command, "powershell.exe", "native")
        .expect("spawn suspended supervision fixture");
    let supervisor = supervise_captured_child(&mut child).expect("assign and resume fixture");
    let mut descendant_pid = String::new();
    std::io::BufReader::new(child.stdout.take().expect("fixture stdout"))
        .read_line(&mut descendant_pid)
        .expect("read descendant pid");
    let descendant_pid = descendant_pid
        .trim()
        .parse::<u32>()
        .expect("parse descendant pid");
    assert!(windows_process_is_running(descendant_pid));

    drop(supervisor);
    for _ in 0..40 {
        let root_exited = child.try_wait().expect("poll supervised child").is_some();
        if root_exited && !windows_process_is_running(descendant_pid) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    panic!("capture descendant survived closing the assigned-before-resume job");
}

#[cfg(windows)]
fn windows_process_is_running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};

    const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;
    let process = unsafe { OpenProcess(SYNCHRONIZE_ACCESS, 0, pid) };
    if process.is_null() {
        return false;
    }
    let wait = unsafe { WaitForSingleObject(process, 0) };
    unsafe {
        CloseHandle(process);
    }
    wait == 258
}

#[cfg(unix)]
#[test]
fn capture_supervision_watchdog_terminates_process_group_when_dropped() {
    use std::io::BufRead as _;

    let args = ["-c".into(), "sleep 30 & echo $!; wait".into()];
    let prepared = prepare_command("sh", &args, &std::env::current_dir().unwrap(), false, false)
        .expect("prepare supervised fixture");
    let mut child =
        spawn_captured_child(prepared.command, "sh", "native").expect("spawn supervised fixture");
    let supervisor = supervise_captured_child(&mut child).expect("supervise fixture child");
    let mut descendant_pid = String::new();
    std::io::BufReader::new(child.stdout.take().expect("fixture stdout"))
        .read_line(&mut descendant_pid)
        .expect("read descendant pid");
    let descendant_pid = descendant_pid
        .trim()
        .parse::<libc::pid_t>()
        .expect("parse descendant pid");
    drop(supervisor);
    for _ in 0..40 {
        let root_exited = child.try_wait().expect("poll supervised child").is_some();
        let descendant_exited = unsafe { libc::kill(descendant_pid, 0) } == -1;
        if root_exited && descendant_exited {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    unsafe {
        libc::kill(descendant_pid, libc::SIGKILL);
    }
    panic!("supervised Unix process group survived closing the watchdog pipe");
}
