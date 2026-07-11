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
}

#[test]
fn bash_fallback_hides_response_file_and_shell_syntax_until_after_startup() {
    let args = vec![
        "--arguments".to_string(),
        "@scratch/args with spaces.json".to_string(),
        "semi;colon".to_string(),
    ];

    let encoded = bash_argv_relay_args("./scripts/tool", &args);

    assert_eq!(encoded.len(), 4);
    assert!(encoded.iter().all(|arg| !arg.contains('@')));
    assert!(encoded.iter().all(|arg| !arg.contains(' ')));
    assert!(encoded.iter().all(|arg| !arg.contains(';')));
    assert!(
        encoded
            .iter()
            .all(|arg| arg.bytes().all(|byte| byte.is_ascii_hexdigit()))
    );
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
