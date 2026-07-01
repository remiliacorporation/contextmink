use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn fixture_root(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = base.join(format!("contextmink-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root); // guardrail: allow-ignore-result cleanup is best-effort for reused test temp dirs
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join(".contextmink.toml"),
        "profile = \"test-profile\"\n",
    )
    .unwrap();
    fs::write(root.join("sample.txt"), "alpha beta\nalpha\nbeta\n").unwrap();
    fs::write(
        root.join("sidecar.json"),
        r#"{"mode":"demo","nested":{"mode":"inner"},"textures":[{"index":0,"texture_type":"diffuse","flags":1,"path":"World|A.blp"},{"index":1,"texture_type":"normal","flags":0,"path":"World|B.blp"}]}"#,
    )
    .unwrap();
    root
}

fn run_contextmink(root: &PathBuf, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_contextmink"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "contextmink failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn run_contextmink_raw(root: &PathBuf, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_contextmink"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn parse_json_output(root: &PathBuf, args: &[&str]) -> Value {
    serde_json::from_str(&run_contextmink(root, args)).unwrap()
}

fn assert_envelope(value: &Value, command: &str, unit: &str) {
    assert_eq!(value["tool"], "contextmink");
    assert_eq!(value["command"], command);
    assert_eq!(value["profile"], "test-profile");
    assert_eq!(value["unit"], unit);
    assert!(value["shown"].is_number());
    assert!(value["total"].is_number());
    assert!(value["truncated"].is_boolean());
    assert!(value["complete"].is_boolean());
    assert!(value.get("cap_reason").is_some());
}

#[test]
fn json_commands_share_receipt_envelope() {
    let root = fixture_root("json-envelope");

    let files = parse_json_output(&root, &["--json", "files", ".", "--max", "1"]);
    assert_envelope(&files, "files", "files");
    assert_eq!(files["truncated"], true);
    assert_eq!(files["complete"], false);
    assert_eq!(files["cap_reason"], "max");

    let slice = parse_json_output(&root, &["--json", "slice", "sample.txt", "--range", "1:2"]);
    assert_envelope(&slice, "slice", "lines");
    assert_eq!(slice["complete"], true);
    assert!(slice["cap_reason"].is_null());

    let json_find = parse_json_output(
        &root,
        &[
            "--json",
            "json-find",
            "sidecar.json",
            "--key-contains",
            "mode",
        ],
    );
    assert_envelope(&json_find, "json-find", "matches");
    assert_eq!(json_find["total"], 2);
}

#[test]
fn slice_accepts_named_path_alias() {
    let root = fixture_root("slice-path-alias");

    let json = parse_json_output(
        &root,
        &["--json", "slice", "--path", "sample.txt", "--range", "2:2"],
    );
    assert_envelope(&json, "slice", "lines");
    assert_eq!(json["path"], "sample.txt");
    assert_eq!(json["lines"][0]["line"], 2);
    assert_eq!(json["lines"][0]["text"], "alpha");
}

#[test]
fn json_commands_accept_named_path_alias() {
    let root = fixture_root("json-path-alias");

    let find = parse_json_output(
        &root,
        &[
            "--json",
            "json-find",
            "--path",
            "sidecar.json",
            "--key-contains",
            "mode",
        ],
    );
    assert_envelope(&find, "json-find", "matches");
    assert_eq!(find["total"], 2);

    let select = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "--path",
            "sidecar.json",
            "--field",
            "/mode",
        ],
    );
    assert_envelope(&select, "json-select", "rows");
    assert_eq!(select["rows"][0]["fields"]["/mode"], "\"demo\"");
}

#[test]
fn capture_caps_child_stdout_and_reports_exit_status() {
    let root = fixture_root("capture-stdout");
    let bin = env!("CARGO_BIN_EXE_contextmink");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "capture",
            "--max-lines",
            "1",
            "--",
            bin,
            "--no-config",
            "slice",
            "sample.txt",
            "--range",
            "1:3",
        ],
    );
    assert_envelope(&json, "capture", "lines");
    assert_eq!(json["success"], true);
    assert_eq!(json["exit_code"], 0);
    assert_eq!(json["stdout"]["shown_lines"], 1);
    assert!(json["stdout"]["total_lines"].as_u64().unwrap() > 1);
    assert_eq!(json["truncated"], true);
    assert_eq!(json["cap_reason"], "lines");
    assert!(json["stdout_text"].as_str().unwrap().contains("alpha beta"));
}

#[test]
fn run_alias_uses_capture_receipt_shape() {
    let root = fixture_root("run-alias");
    let bin = env!("CARGO_BIN_EXE_contextmink");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "run",
            "--max-lines",
            "1",
            "--",
            bin,
            "--no-config",
            "slice",
            "sample.txt",
            "--range",
            "1:1",
        ],
    );
    assert_envelope(&json, "capture", "lines");
    assert_eq!(json["success"], true);
    assert_eq!(json["exit_code"], 0);
}

#[test]
fn fail_if_truncated_exits_nonzero_after_receipt() {
    let root = fixture_root("fail-if-truncated");

    let output = run_contextmink_raw(
        &root,
        &[
            "--fail-if-truncated",
            "files",
            ".",
            "--max",
            "1",
            "--max-scan-files",
            "20",
        ],
    );
    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stdout.contains("CONTEXTMINK_RECEIPT "));
    assert!(stdout.contains("\"truncated\":true"));
    assert!(stderr.contains("strict completion requested"));
}

#[test]
fn strict_alias_and_scan_guard_fail_after_receipt() {
    let root = fixture_root("strict-aliases");
    fs::write(root.join("extra_a.txt"), "a\n").unwrap();
    fs::write(root.join("extra_b.txt"), "b\n").unwrap();

    let strict = run_contextmink_raw(&root, &["--strict-complete", "files", ".", "--max", "1"]);
    assert!(!strict.status.success());
    let strict_stdout = String::from_utf8(strict.stdout).unwrap();
    assert!(strict_stdout.contains("CONTEXTMINK_RECEIPT "));

    let display_capped = run_contextmink_raw(
        &root,
        &[
            "--require-complete-scan",
            "files",
            ".",
            "--max",
            "1",
            "--max-scan-files",
            "20",
        ],
    );
    assert!(display_capped.status.success());
    let display_stdout = String::from_utf8(display_capped.stdout).unwrap();
    assert!(display_stdout.contains("\"cap_reason\":\"max\""));

    let scan_capped = run_contextmink_raw(
        &root,
        &[
            "--require-complete-scan",
            "files",
            ".",
            "--max",
            "10",
            "--max-scan-files",
            "1",
        ],
    );
    assert!(!scan_capped.status.success());
    let scan_stdout = String::from_utf8(scan_capped.stdout).unwrap();
    let scan_stderr = String::from_utf8(scan_capped.stderr).unwrap();
    assert!(scan_stdout.contains("\"cap_reason\":\"scan\""));
    assert!(scan_stderr.contains("--require-complete-scan"));
}

#[test]
fn capture_keeps_nonzero_child_status_in_receipt() {
    let root = fixture_root("capture-nonzero");
    let bin = env!("CARGO_BIN_EXE_contextmink");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "capture",
            "--",
            bin,
            "--no-config",
            "slice",
            "missing.txt",
            "--range",
            "1:1",
        ],
    );
    assert_envelope(&json, "capture", "lines");
    assert_eq!(json["success"], false);
    assert_ne!(json["exit_code"], 0);
    assert!(json["stderr"]["total_bytes"].as_u64().unwrap() > 0);
    assert!(
        json["stderr_text"]
            .as_str()
            .unwrap()
            .contains("missing.txt")
    );
}

#[test]
fn files_scan_cap_sets_complete_false() {
    let root = fixture_root("files-scan-cap");
    fs::write(root.join("extra_a.txt"), "a\n").unwrap();
    fs::write(root.join("extra_b.txt"), "b\n").unwrap();

    let files = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            ".",
            "--max",
            "10",
            "--max-scan-files",
            "2",
        ],
    );
    assert_envelope(&files, "files", "files");
    assert_eq!(files["shown"], 2);
    assert_eq!(files["truncated"], true);
    assert_eq!(files["complete"], false);
    assert_eq!(files["cap_reason"], "scan");
    assert_eq!(files["candidate_files_scanned"], 2);
    assert_eq!(files["candidate_files_total_is_lower_bound"], true);
    assert_eq!(files["total"], 3);
}

#[test]
fn files_glob_matches_basename_inside_explicit_roots() {
    let root = fixture_root("files-basename-glob");
    fs::create_dir_all(root.join("queue")).unwrap();
    fs::write(root.join("queue").join("work.jsonl"), "{}\n").unwrap();
    fs::write(root.join("queue").join("notes.txt"), "skip\n").unwrap();

    let files = parse_json_output(
        &root,
        &[
            "--json", "files", "queue", "--glob", "*.jsonl", "--limit", "5",
        ],
    );

    assert_envelope(&files, "files", "files");
    assert_eq!(files["shown"], 1);
    assert_eq!(files["total"], 1);
    assert_eq!(files["files"][0], "queue/work.jsonl");
}

#[test]
fn files_ext_filters_without_shell_glob_patterns() {
    let root = fixture_root("files-ext-filter");
    fs::create_dir_all(root.join("queue")).unwrap();
    fs::write(root.join("queue").join("work.JSON"), "{}\n").unwrap();
    fs::write(root.join("queue").join("work.jsonl"), "{}\n").unwrap();
    fs::write(root.join("queue").join("notes.txt"), "skip\n").unwrap();

    let files = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            "queue",
            "--ext",
            ".json",
            "--extension",
            "jsonl",
            "--limit",
            "5",
        ],
    );

    assert_envelope(&files, "files", "files");
    assert_eq!(files["shown"], 2);
    assert_eq!(files["total"], 2);
    assert_eq!(files["files"][0], "queue/work.JSON");
    assert_eq!(files["files"][1], "queue/work.jsonl");
}

#[test]
fn files_accepts_named_path_without_default_root() {
    let root = fixture_root("files-path-alias");
    fs::create_dir_all(root.join("queue")).unwrap();
    fs::write(root.join("queue").join("work.jsonl"), "{}\n").unwrap();

    let files = parse_json_output(&root, &["--json", "files", "--path", "queue"]);

    assert_envelope(&files, "files", "files");
    assert_eq!(files["total"], 1);
    assert_eq!(files["files"][0], "queue/work.jsonl");
}

#[test]
fn help_names_excluded_file_bypass_positively() {
    let root = fixture_root("help-exclude-globs");

    let help = run_contextmink(&root, &["files", "--help"]);
    assert!(help.contains("--with-excluded"));
    assert!(!help.contains("--ignore-exclude-globs"));
    assert!(!help.contains("--include-noisy"));
}

#[test]
fn explicit_roots_inside_configured_excludes_are_honored() {
    let root = fixture_root("explicit-excluded-root");
    fs::write(
        root.join(".contextmink.toml"),
        "profile = \"test-profile\"\nexclude_globs = [\"artifacts/**\"]\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("artifacts").join("notes")).unwrap();
    fs::write(
        root.join("artifacts").join("notes").join("finding.md"),
        "needle\n",
    )
    .unwrap();

    let broad = parse_json_output(&root, &["--json", "files", ".", "--max", "20"]);
    assert_envelope(&broad, "files", "files");
    let broad_files = broad["files"].as_array().unwrap();
    assert!(
        broad_files
            .iter()
            .all(|path| !path.as_str().unwrap().starts_with("artifacts/"))
    );

    let bypass = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            ".",
            "--with-excluded",
            "--max",
            "20",
            "--max-scan-files",
            "20",
        ],
    );
    assert_envelope(&bypass, "files", "files");
    assert!(
        bypass["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path.as_str().unwrap() == "./artifacts/notes/finding.md")
    );

    let files = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            "artifacts/notes",
            "--max",
            "20",
            "--max-scan-files",
            "20",
        ],
    );
    assert_envelope(&files, "files", "files");
    assert_eq!(files["shown"], 1);
    assert_eq!(files["total"], 1);
    assert_eq!(files["files"][0], "artifacts/notes/finding.md");

    let grep = parse_json_output(
        &root,
        &[
            "--json",
            "grep",
            "needle",
            "artifacts/notes",
            "--max-scan-files",
            "20",
        ],
    );
    assert_envelope(&grep, "grep", "files");
    assert_eq!(grep["shown"], 1);
    assert_eq!(grep["total"], 1);
    assert_eq!(grep["total_matches"], 1);
}

#[test]
fn with_git_ignored_includes_gitignored_directories_without_disabling_exclude_globs() {
    let root = fixture_root("with-git-ignored");
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join(".gitignore"), "vendor/*\n").unwrap();
    fs::create_dir_all(root.join("vendor").join("sqlite-tool").join(".git")).unwrap();
    fs::write(
        root.join("vendor").join("sqlite-tool").join("README.md"),
        "sqlite helper\n",
    )
    .unwrap();
    fs::write(
        root.join("vendor")
            .join("sqlite-tool")
            .join(".git")
            .join("config"),
        "ignored metadata\n",
    )
    .unwrap();

    let without_flag = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            ".",
            "--max",
            "20",
            "--max-scan-files",
            "20",
        ],
    );
    assert_envelope(&without_flag, "files", "files");
    let files_without_flag = without_flag["files"].as_array().unwrap();
    assert!(
        files_without_flag
            .iter()
            .all(|path| path.as_str().unwrap().trim_start_matches("./")
                != "vendor/sqlite-tool/README.md")
    );

    let with_flag = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            ".",
            "--with-git-ignored",
            "--max",
            "20",
            "--max-scan-files",
            "20",
        ],
    );
    assert_envelope(&with_flag, "files", "files");
    let files = with_flag["files"].as_array().unwrap();
    assert!(
        files
            .iter()
            .any(|path| path.as_str().unwrap().trim_start_matches("./")
                == "vendor/sqlite-tool/README.md")
    );
    assert!(
        files
            .iter()
            .all(|path| !path.as_str().unwrap().contains("/.git/"))
    );
}

#[test]
fn grep_scan_cap_marks_no_match_as_scanned_subset_only() {
    let root = fixture_root("grep-scan-cap");
    fs::write(root.join("extra_a.txt"), "alpha\n").unwrap();
    fs::write(root.join("extra_b.txt"), "alpha\n").unwrap();

    let grep = parse_json_output(
        &root,
        &[
            "--json",
            "grep",
            "not-present",
            ".",
            "--max-scan-files",
            "1",
        ],
    );
    assert_envelope(&grep, "grep", "files");
    assert_eq!(grep["shown"], 0);
    assert_eq!(grep["truncated"], true);
    assert_eq!(grep["complete"], false);
    assert_eq!(grep["cap_reason"], "scan");
    assert_eq!(grep["candidate_files_scanned"], 1);
    assert_eq!(grep["candidate_files_total_is_lower_bound"], true);
    assert!(grep["candidate_files_total"].as_u64().unwrap() >= 2);
    assert_eq!(grep["no_match_scope"], "scanned_subset");
}

#[test]
fn grep_terms_reports_public_command_name() {
    let root = fixture_root("grep-terms-command");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--term",
            "alpha",
            "--term",
            "beta",
            "sample.txt",
        ],
    );
    assert_envelope(&json, "grep-terms", "files");
    assert_eq!(json["total_matches"], 1);

    let human = run_contextmink(
        &root,
        &["grep-terms", "--term", "alpha", "--term", "beta", "."],
    );
    let receipt = human
        .lines()
        .last()
        .unwrap()
        .strip_prefix("CONTEXTMINK_RECEIPT ")
        .unwrap();
    let receipt: Value = serde_json::from_str(receipt).unwrap();
    assert_envelope(&receipt, "grep-terms", "files");
}

#[test]
fn grep_terms_supports_any_mode_and_term_files() {
    let root = fixture_root("grep-terms-any");
    fs::write(root.join("phrases.txt"), "alpha beta\nmissing phrase\n").unwrap();

    let default_all = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--term",
            "alpha",
            "--term",
            "beta",
            "sample.txt",
        ],
    );
    assert_envelope(&default_all, "grep-terms", "files");
    assert_eq!(default_all["pattern"], "all_terms(alpha,beta)");
    assert_eq!(default_all["total_matches"], 1);

    let any = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--mode",
            "any",
            "--term",
            "alpha",
            "--term",
            "beta",
            "sample.txt",
        ],
    );
    assert_envelope(&any, "grep-terms", "files");
    assert_eq!(any["pattern"], "any_terms(alpha,beta)");
    assert_eq!(any["total_matches"], 3);

    let or_alias = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--or",
            "--term",
            "alpha",
            "--term",
            "beta",
            "sample.txt",
        ],
    );
    assert_envelope(&or_alias, "grep-terms", "files");
    assert_eq!(or_alias["pattern"], "any_terms(alpha,beta)");
    assert_eq!(or_alias["total_matches"], 3);

    let term_file = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--mode",
            "any",
            "--term-file",
            "phrases.txt",
            "sample.txt",
        ],
    );
    assert_envelope(&term_file, "grep-terms", "files");
    assert_eq!(term_file["pattern"], "any_terms(alpha beta,missing phrase)");
    assert_eq!(term_file["total_matches"], 1);
}

#[test]
fn grep_terms_accepts_agent_friendly_limit_aliases() {
    let root = fixture_root("grep-terms-aliases");
    fs::write(root.join("flags.txt"), "--flag-like value\n").unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--term",
            "alpha",
            "--limit",
            "1",
            "--max-matches",
            "1",
            "sample.txt",
        ],
    );
    assert_envelope(&json, "grep-terms", "files");
    assert_eq!(json["shown"], 1);
    assert_eq!(json["sample_lines_shown"], 1);
    assert_eq!(json["cap_reason"], "samples");
    assert_eq!(json["files"][0]["samples"].as_array().unwrap().len(), 1);

    let flag_like = parse_json_output(
        &root,
        &["--json", "grep-terms", "--term", "--flag-like", "flags.txt"],
    );
    assert_envelope(&flag_like, "grep-terms", "files");
    assert_eq!(flag_like["total_matches"], 1);

    let help = run_contextmink(&root, &["grep-terms", "--help"]);
    assert!(help.contains("--max-matched-files"));
    assert!(help.contains("--limit"));
    assert!(help.contains("--max-matches"));
    assert!(help.contains("--max-lines"));
}

#[test]
fn grep_stops_content_scan_at_matching_file_cap() {
    let root = fixture_root("grep-count-cap");
    let matches = root.join("matches");
    fs::create_dir_all(&matches).unwrap();
    for name in ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"] {
        fs::write(matches.join(name), "needle\n").unwrap();
    }

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--term",
            "needle",
            "--max-count-files",
            "2",
            "--max-files",
            "2",
            "matches",
        ],
    );
    assert_envelope(&json, "grep-terms", "files");
    assert_eq!(json["shown"], 2);
    assert_eq!(json["matched_files_total"], 2);
    assert_eq!(json["matched_files_total_is_lower_bound"], true);
    assert_eq!(json["total_matches"], 2);
    assert_eq!(json["total_matches_is_lower_bound"], true);
    assert_eq!(json["candidate_files_scanned"], 5);
    assert_eq!(json["content_files_scanned"], 2);
    assert_eq!(json["cap_reason"], "matched_files");
    assert_eq!(json["truncated"], true);

    let guarded = run_contextmink_raw(
        &root,
        &[
            "--require-complete-scan",
            "grep-terms",
            "--term",
            "needle",
            "--max-count-files",
            "2",
            "matches",
        ],
    );
    assert!(!guarded.status.success());
    let guarded_stdout = String::from_utf8(guarded.stdout).unwrap();
    let guarded_stderr = String::from_utf8(guarded.stderr).unwrap();
    assert!(guarded_stdout.contains("\"cap_reason\":\"matched_files\""));
    assert!(guarded_stderr.contains("--require-complete-scan"));
}

#[test]
fn grep_json_honors_global_sample_cap() {
    let root = fixture_root("grep-json-sample-cap");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "grep",
            "alpha",
            "sample.txt",
            "--lines-per-file",
            "3",
            "--max-sample-lines",
            "1",
        ],
    );
    assert_envelope(&json, "grep", "files");
    assert_eq!(json["shown"], 1);
    assert_eq!(json["files"].as_array().unwrap().len(), 1);
    assert_eq!(json["files"][0]["samples"].as_array().unwrap().len(), 1);
    assert_eq!(json["sample_lines_shown"], 1);
    assert_eq!(json["cap_reason"], "samples");
    assert_eq!(json["truncated"], true);
}

#[test]
fn grep_supports_pattern_files_for_shell_fragile_regex() {
    let root = fixture_root("grep-pattern-file");
    fs::write(root.join("pattern.txt"), "\u{feff}alpha|beta\n").unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "grep",
            "--pattern-file",
            "pattern.txt",
            "sample.txt",
        ],
    );
    assert_envelope(&json, "grep", "files");
    assert_eq!(json["pattern"], "\"alpha|beta\"");
    assert_eq!(json["total_matches"], 4);
}

#[test]
fn grep_accepts_named_search_paths() {
    let root = fixture_root("grep-path-alias");

    let json = parse_json_output(&root, &["--json", "grep", "alpha", "--path", "sample.txt"]);
    assert_envelope(&json, "grep", "files");
    assert_eq!(json["shown"], 1);
    assert_eq!(json["total_matches"], 2);
    assert_eq!(json["files"][0]["path"], "sample.txt");
}

#[test]
fn json_select_projects_array_fields_without_jq_filters() {
    let root = fixture_root("json-select");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "sidecar.json",
            "--array",
            "/textures",
            "--field",
            "index",
            "--field",
            "path",
        ],
    );
    assert_envelope(&json, "json-select", "rows");
    assert_eq!(json["total"], 2);
    assert_eq!(json["rows"][0]["fields"]["index"], "0");
    assert_eq!(json["rows"][0]["fields"]["path"], "\"World|A.blp\"");
}

#[test]
fn json_select_tolerates_msys_converted_json_pointers() {
    let root = fixture_root("json-select-msys-pointers");

    let output = Command::new(env!("CARGO_BIN_EXE_contextmink"))
        .current_dir(&root)
        .env("MSYSTEM", "MINGW64")
        .env("EXEPATH", r"C:\Program Files\Git\bin")
        .args([
            "--json",
            "json-select",
            "sidecar.json",
            "--array",
            "C:/Program Files/Git/textures",
            "--field",
            "C:/Program Files/Git/path",
            "--limit",
            "1",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "contextmink failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_envelope(&json, "json-select", "rows");
    assert_eq!(json["array"], "/textures");
    assert_eq!(json["fields"][0], "/path");
    assert_eq!(json["rows"][0]["fields"]["/path"], "\"World|A.blp\"");
}

#[test]
fn json_select_projects_jsonl_rows_and_limit_alias() {
    let root = fixture_root("json-select-jsonl");
    fs::write(
        root.join("queue.jsonl"),
        "{\"addr\":\"0x408690\",\"flags\":[\"custom_register_args\"]}\n{\"addr\":\"0x409880\",\"flags\":[\"fpu_or_reg_dropped\"]}\n",
    )
    .unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "queue.jsonl",
            "--field",
            "addr",
            "--field",
            "flags",
            "--limit",
            "1",
        ],
    );

    assert_envelope(&json, "json-select", "rows");
    assert_eq!(json["input_format"], "jsonl");
    assert_eq!(json["shown"], 1);
    assert_eq!(json["total"], 2);
    assert_eq!(json["truncated"], true);
    assert_eq!(json["rows"][0]["fields"]["addr"], "\"0x408690\"");
    assert_eq!(
        json["rows"][0]["fields"]["flags"],
        "[\"custom_register_args\"]"
    );
}

#[test]
fn limit_aliases_match_canonical_caps() {
    let root = fixture_root("limit-aliases");
    fs::write(root.join("extra.txt"), "alpha\n").unwrap();

    let files = parse_json_output(&root, &["--json", "files", ".", "--limit", "1"]);
    assert_envelope(&files, "files", "files");
    assert_eq!(files["shown"], 1);
    assert_eq!(files["truncated"], true);

    let json_find = parse_json_output(
        &root,
        &[
            "--json",
            "json-find",
            "sidecar.json",
            "--key-contains",
            "mode",
            "--limit",
            "1",
        ],
    );
    assert_envelope(&json_find, "json-find", "matches");
    assert_eq!(json_find["shown"], 1);
    assert_eq!(json_find["total"], 2);
    assert_eq!(json_find["truncated"], true);

    let db_path = root.join("limit.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE rows(id INTEGER PRIMARY KEY, label TEXT);
         INSERT INTO rows(label) VALUES ('a'), ('b');",
    )
    .unwrap();
    drop(conn);
    let sqlite = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite",
            "limit.sqlite",
            "--sql",
            "SELECT * FROM rows ORDER BY id",
            "--limit",
            "1",
        ],
    );
    assert_envelope(&sqlite, "sqlite", "rows");
    assert_eq!(sqlite["shown"], 1);
    assert_eq!(sqlite["total"], 2);
    assert_eq!(sqlite["truncated"], true);
    assert_eq!(sqlite["rows_total_is_lower_bound"], false);

    let sqlite_scan = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite",
            "limit.sqlite",
            "--sql",
            "SELECT * FROM rows ORDER BY id",
            "--max-rows",
            "1",
            "--max-scan-rows",
            "1",
        ],
    );
    assert_envelope(&sqlite_scan, "sqlite", "rows");
    assert_eq!(sqlite_scan["rows_total_is_lower_bound"], true);
    assert_eq!(sqlite_scan["cap_reason"], "scan");

    let guarded_scan = run_contextmink_raw(
        &root,
        &[
            "--require-complete-scan",
            "sqlite",
            "limit.sqlite",
            "--sql",
            "SELECT * FROM rows ORDER BY id",
            "--max-rows",
            "1",
            "--max-scan-rows",
            "1",
        ],
    );
    assert!(!guarded_scan.status.success());
    assert!(
        String::from_utf8(guarded_scan.stderr)
            .unwrap()
            .contains("--require-complete-scan")
    );
}

#[test]
fn sqlite_reads_query_from_file_and_caps_rows() {
    let root = fixture_root("sqlite-query-file");
    let db_path = root.join("sample.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE pairs(id INTEGER PRIMARY KEY, left_value TEXT, right_value TEXT);
         INSERT INTO pairs(left_value, right_value) VALUES ('alpha', 'beta'), ('gamma', 'delta');",
    )
    .unwrap();
    drop(conn);
    fs::write(
        root.join("query.sql"),
        "\u{feff}SELECT id, left_value || ':' || right_value AS joined FROM pairs ORDER BY id\n",
    )
    .unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite",
            "--path",
            "sample.sqlite",
            "--sql-file",
            "query.sql",
            "--max-rows",
            "1",
        ],
    );
    assert_envelope(&json, "sqlite", "rows");
    assert_eq!(json["shown"], 1);
    assert_eq!(json["total"], 2);
    assert_eq!(json["cap_reason"], "rows");
    assert_eq!(json["rows"][0]["fields"]["joined"], "\"alpha:beta\"");

    let duplicate_db = run_contextmink_raw(
        &root,
        &[
            "--json",
            "sqlite",
            "sample.sqlite",
            "--db",
            "sample.sqlite",
            "--sql",
            "SELECT 1",
        ],
    );
    assert!(!duplicate_db.status.success());
    assert!(
        String::from_utf8(duplicate_db.stderr)
            .unwrap()
            .contains("either positional <DB> or --db/--path <DB>, not both")
    );
}

#[test]
fn sqlite_schema_reports_tables_columns_foreign_keys_and_indexes() {
    let root = fixture_root("sqlite-schema");
    let db_path = root.join("schema.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE parent(rowid INTEGER PRIMARY KEY, label TEXT NOT NULL UNIQUE) STRICT;
         CREATE TABLE child(rowid INTEGER PRIMARY KEY, parent_id INTEGER NOT NULL REFERENCES parent(rowid), note TEXT) STRICT;
         CREATE INDEX child_parent_id_idx ON child(parent_id);
         CREATE INDEX child_note_expr_idx ON child(coalesce(note, ''));",
    )
    .unwrap();
    drop(conn);

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite-schema",
            "--path",
            "schema.sqlite",
            "--table",
            "child",
        ],
    );
    assert_envelope(&json, "sqlite-schema", "tables");
    assert_eq!(json["shown"], 1);
    assert_eq!(json["tables"][0]["name"], "child");
    assert_eq!(json["tables"][0]["strict"], true);
    assert_eq!(json["tables"][0]["columns_total"], 3);
    assert_eq!(json["tables"][0]["columns"][1]["name"], "parent_id");
    assert_eq!(
        json["tables"][0]["columns"][1]["foreign_key"]["table"],
        "parent"
    );
    let indexes = json["tables"][0]["indexes"].as_array().unwrap();
    let parent_index = indexes
        .iter()
        .find(|index| index["name"] == "child_parent_id_idx")
        .unwrap();
    assert_eq!(parent_index["columns"][0], "parent_id");
    let expr_index = indexes
        .iter()
        .find(|index| index["name"] == "child_note_expr_idx")
        .unwrap();
    assert_eq!(expr_index["columns"][0], "<expr>");

    let capped = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite-schema",
            "schema.sqlite",
            "--max-tables",
            "1",
            "--max-columns",
            "1",
        ],
    );
    assert_eq!(capped["truncated"], true);
    assert!(matches!(
        capped["cap_reason"].as_str(),
        Some("tables") | Some("columns")
    ));
}

#[test]
fn slice_past_eof_is_complete_when_every_available_line_is_shown() {
    let root = fixture_root("slice-past-eof");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "slice",
            "sample.txt",
            "--start",
            "1",
            "--end",
            "260",
        ],
    );
    assert_envelope(&json, "slice", "lines");
    assert_eq!(json["shown"], 3);
    assert_eq!(json["total"], 3);
    assert_eq!(json["end"], 3);
    assert_eq!(json["truncated"], false);
    assert_eq!(json["complete"], true);
    assert!(json["cap_reason"].is_null());
}
