use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn fixture_root(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = base.join(format!("contextmink-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join(".contextmink.toml"),
        "profile = \"test-profile\"\n",
    )
    .unwrap();
    fs::write(root.join("sample.txt"), "alpha beta\nalpha\nbeta\n").unwrap();
    fs::write(
        root.join("sidecar.json"),
        r#"{"mode":"demo","nested":{"mode":"inner"}}"#,
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
