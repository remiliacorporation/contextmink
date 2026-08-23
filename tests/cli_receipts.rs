use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

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

#[test]
fn non_receipt_commands_reject_irrelevant_global_flags() {
    let root = fixture_root("non-receipt-flags");
    let strict = run_contextmink_raw(
        &root,
        &[
            "--fail-if-truncated",
            "guard-check",
            "--command",
            "git status",
        ],
    );
    assert!(!strict.status.success());
    assert!(
        String::from_utf8_lossy(&strict.stderr)
            .contains("strictness flags apply only to commands that emit")
    );

    let hook_json = run_contextmink_raw(&root, &["--json", "hook-guard"]);
    assert!(!hook_json.status.success());
    assert!(String::from_utf8_lossy(&hook_json.stderr).contains("hook protocol"));
}

#[test]
fn guard_check_is_readable_by_default_and_structured_on_request() {
    let root = fixture_root("guard-check-rendering");
    let human = run_contextmink(&root, &["guard-check", "--command", "git status"]);
    assert!(human.starts_with("decision=allow input_kind=shell_command"));

    let json = parse_json_output(&root, &["--json", "guard-check", "--command", "git status"]);
    assert_eq!(json["decision"], "allow");
}

#[test]
fn structured_data_commands_share_strict_jsonl_and_numeric_contracts() {
    let root = fixture_root("structured-contracts");
    fs::write(
        root.join("multiline.jsonl"),
        "{\n  \"id\": 1\n}\n{\"id\":2}\n",
    )
    .unwrap();
    for args in [
        vec!["json-select", "multiline.jsonl", "--fields", "id"],
        vec!["json-find", "multiline.jsonl", "--key-contains", "id"],
    ] {
        let output = run_contextmink_raw(&root, &args);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("physical line"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fs::write(
        root.join("numbers.json"),
        r#"{"large":184467440737095516160}"#,
    )
    .unwrap();
    let exact = parse_json_output(
        &root,
        &["--json", "json-select", "numbers.json", "--fields", "large"],
    );
    assert_eq!(exact["rows"][0]["fields"]["large"], "184467440737095516160");

    fs::write(root.join("duplicate.json"), r#"{"id":1,"id":2}"#).unwrap();
    let duplicate =
        run_contextmink_raw(&root, &["json-select", "duplicate.json", "--fields", "id"]);
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("duplicate JSON object key"));
}

#[test]
fn json_materialization_and_sqlite_authorization_fail_before_unsafe_work() {
    let root = fixture_root("structured-bounds");
    fs::write(
        root.join("large.json"),
        format!(r#"{{"value":"{}"}}"#, "x".repeat(128)),
    )
    .unwrap();
    let bounded = run_contextmink_raw(
        &root,
        &[
            "json-select",
            "large.json",
            "--fields",
            "value",
            "--max-document-bytes",
            "32",
        ],
    );
    assert!(!bounded.status.success());
    assert!(String::from_utf8_lossy(&bounded.stderr).contains("--max-document-bytes 32"));

    rusqlite::Connection::open(root.join("main.sqlite")).unwrap();
    rusqlite::Connection::open(root.join("other.sqlite")).unwrap();
    let attach = run_contextmink_raw(
        &root,
        &[
            "sqlite",
            "main.sqlite",
            "--sql",
            "ATTACH DATABASE 'other.sqlite' AS other",
        ],
    );
    assert!(!attach.status.success());
    assert!(
        String::from_utf8_lossy(&attach.stderr)
            .to_ascii_lowercase()
            .contains("not authorized")
    );

    let schema = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite",
            "main.sqlite",
            "--sql",
            "PRAGMA table_info(sqlite_schema)",
        ],
    );
    assert_eq!(schema["columns"][1], "name");
}

#[test]
fn universal_lines_and_outline_matcher_disclosure_are_agent_visible() {
    let root = fixture_root("universal-lines");
    fs::write(root.join("cr.txt"), b"one\rtwo\rthree").unwrap();
    let slice = parse_json_output(&root, &["--json", "slice", "cr.txt", "--range", "1:3"]);
    assert_eq!(slice["total_lines"], 3);
    assert_eq!(slice["lines"][1]["text"], "two");

    fs::write(
        root.join("sample.c"),
        "/*\nint hidden(void);\n*/\nint visible(void);\n",
    )
    .unwrap();
    let outline = parse_json_output(&root, &["--json", "outline", "sample.c"]);
    assert_eq!(outline["matcher"], "heuristic");
    assert_eq!(outline["items"].as_array().unwrap().len(), 1);
    assert_eq!(outline["items"][0]["line"], 4);
}

fn assert_envelope(value: &Value, command: &str, unit: &str) {
    assert_eq!(value["schema"], "contextmink.receipt.v2");
    assert_eq!(value["tool"], "contextmink");
    assert_eq!(value["command"], command);
    assert_eq!(value["profile"], "test-profile");
    assert_eq!(value["result"]["unit"], unit);
    assert!(value["result"]["shown"].is_number());
    assert!(value["result"]["total"].is_number());
    assert!(value["result"]["total_is_lower_bound"].is_boolean());
    assert!(value["scope_complete"].is_boolean());
    assert!(value["output_truncated"].is_boolean());
    assert!(value["complete"].is_boolean());
    assert!(value["caps"].is_array());
}

fn result(value: &Value) -> &Value {
    &value["result"]
}

fn has_cap(value: &Value, boundary: &str, dimension: &str) -> bool {
    value["caps"].as_array().is_some_and(|caps| {
        caps.iter()
            .any(|cap| cap["boundary"] == boundary && cap["dimension"] == dimension)
    })
}

fn parse_human_receipt(output: &str) -> Value {
    let receipt = output
        .lines()
        .last()
        .expect("human output must end with a receipt")
        .strip_prefix("CONTEXTMINK_RECEIPT ")
        .expect("last line must be the receipt envelope");
    serde_json::from_str(receipt).expect("receipt must be valid JSON")
}

#[test]
fn setup_project_installs_agent_capability_without_editing_guidance() {
    let root = fixture_root("setup-project-command");
    fs::remove_file(root.join(".contextmink.toml")).unwrap();
    fs::write(root.join("AGENTS.md"), "existing guidance\n").unwrap();

    let setup = parse_json_output(&root, &["--json", "setup-project", "."]);
    assert_eq!(setup["schema"], "contextmink.project_setup.v1");
    assert_eq!(setup["dry_run"], false);
    assert_eq!(
        fs::read_to_string(root.join("AGENTS.md")).unwrap(),
        "existing guidance\n"
    );
    assert!(
        root.join("tools/contextmink/agent_integration.md")
            .is_file()
    );
    assert!(root.join(".agents/skills/contextmink/SKILL.md").is_file());
    assert!(
        root.join(".agents/skills/contextmink/agents/openai.yaml")
            .is_file()
    );
    assert!(root.join(".claude/skills/contextmink/SKILL.md").is_file());
    assert_eq!(
        fs::read(root.join(".agents/skills/contextmink/SKILL.md")).unwrap(),
        fs::read(root.join(".claude/skills/contextmink/SKILL.md")).unwrap()
    );
    assert!(root.join("scripts/contextmink").is_file());
    assert!(
        fs::read_to_string(root.join(".contextmink.toml"))
            .unwrap()
            .contains("profile = \"contextmink-setup-project-command-")
    );
    assert!(
        setup["agent_guidance_files_found"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "AGENTS.md")
    );
    assert!(
        setup["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action
                .as_str()
                .unwrap()
                .contains("repository-guidance trigger"))
    );

    let second = parse_json_output(&root, &["--json", "setup-project", "."]);
    assert!(
        second["actions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|action| action["path"] != ".contextmink.toml")
            .all(|action| { action["action"] == "unchanged" })
    );
    assert!(second["actions"].as_array().unwrap().iter().any(|action| {
        action["path"] == ".contextmink.toml" && action["action"] == "preserve_repository_owned"
    }));

    fs::write(root.join("scripts/contextmink"), "older launcher\n").unwrap();
    let upgrade_plan = parse_json_output(&root, &["--json", "setup-project", ".", "--dry-run"]);
    assert!(
        upgrade_plan["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| {
                action["path"] == "scripts/contextmink" && action["action"] == "replace"
            })
    );
    assert_eq!(
        fs::read_to_string(root.join("scripts/contextmink")).unwrap(),
        "older launcher\n"
    );
}

#[test]
fn setup_project_rejects_unrelated_global_flags() {
    let root = fixture_root("setup-project-global-flags");
    for flag in [
        "--no-config",
        "--fail-if-truncated",
        "--require-complete-scope",
    ] {
        let output = run_contextmink_raw(&root, &[flag, "setup-project", "."]);
        assert!(!output.status.success(), "{flag} must be rejected");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.contains("setup-project accepts only its own flags plus --json"),
            "unexpected stderr for {flag}: {stderr}"
        );
    }
}

#[test]
fn hook_snippet_emits_claude_command_hooks() {
    let root = fixture_root("hook-snippet");
    let binary = root.join("tools/contextmink/contextmink.exe");
    let config = root.join(".contextmink.toml");
    let snippet = parse_json_output(
        &root,
        &[
            "hook-snippet",
            "--binary",
            binary.to_str().unwrap(),
            "--guard-config",
            config.to_str().unwrap(),
        ],
    );

    let entries = snippet["hooks"]["PreToolUse"]
        .as_array()
        .expect("PreToolUse entries");
    assert_eq!(entries.len(), 2);
    let bash = entries
        .iter()
        .find(|entry| entry["matcher"] == "Bash")
        .expect("Bash matcher");
    let powershell = entries
        .iter()
        .find(|entry| entry["matcher"] == "PowerShell")
        .expect("PowerShell matcher");
    let bash_command = bash["hooks"][0]["command"].as_str().unwrap();
    assert!(bash_command.contains("hook-guard --config"));
    assert!(!bash_command.contains('\\'));
    assert!(bash["hooks"][0].get("args").is_none());
    let powershell_command = powershell["hooks"][0]["command"].as_str().unwrap();
    assert!(!powershell_command.starts_with("& "));
    assert!(powershell_command.contains("--shell powershell"));
}

#[test]
fn emitted_bash_hook_executes_end_to_end() {
    let root = fixture_root("hook-snippet-bash-exec");
    let config = root.join(".contextmink.toml");
    let snippet = parse_json_output(
        &root,
        &[
            "hook-snippet",
            "--binary",
            env!("CARGO_BIN_EXE_contextmink"),
            "--guard-config",
            config.to_str().unwrap(),
        ],
    );
    let command = snippet["hooks"]["PreToolUse"]
        .as_array()
        .expect("PreToolUse entries")
        .iter()
        .find(|entry| entry["matcher"] == "Bash")
        .expect("Bash matcher")["hooks"][0]["command"]
        .as_str()
        .expect("Bash hook command");
    #[cfg(windows)]
    let bash = PathBuf::from(r"C:\Program Files\Git\bin\bash.exe");
    #[cfg(not(windows))]
    let bash = PathBuf::from("bash");

    let mut child = Command::new(&bash)
        .arg("-c")
        .arg(command)
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("failed to start {}: {error}", bash.display()));
    child
        .stdin
        .take()
        .expect("hook stdin")
        .write_all(br#"{"tool_input":{"command":"git status --short"}}"#)
        .expect("write hook payload");
    let output = child
        .wait_with_output()
        .expect("wait for emitted Bash hook");
    assert!(
        output.status.success(),
        "emitted Bash hook failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn explicitly_missing_hook_policy_fails_closed() {
    let root = fixture_root("missing-hook-policy");
    let missing = root.join("missing.contextmink.toml");
    let output = run_contextmink_raw(
        &root,
        &[
            "hook-guard",
            "--config",
            missing.to_str().unwrap(),
            "--expected-root",
            root.to_str().unwrap(),
            "--shell",
            "posix",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("destructive-command policy could not be loaded")
    );
}

#[test]
fn malformed_discovered_hook_policy_fails_closed() {
    let root = fixture_root("malformed-discovered-hook-policy");
    fs::write(root.join(".contextmink.toml"), "profile = [\n").unwrap();

    let output = run_contextmink_raw(
        &root,
        &[
            "hook-guard",
            "--expected-root",
            root.to_str().unwrap(),
            "--shell",
            "posix",
        ],
    );

    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("destructive-command policy could not be loaded")
    );
}

#[test]
fn guard_check_explains_commands_without_spawning_them() {
    let root = fixture_root("guard-check");
    let denied = parse_json_output(
        &root,
        &[
            "--json",
            "guard-check",
            "--command",
            "git status && git clean -fdX",
        ],
    );
    assert_eq!(denied["schema"], "contextmink.guard_check.v1");
    assert_eq!(denied["decision"], "deny");
    assert_eq!(denied["executed"], false);
    assert!(denied["message"].as_str().unwrap().contains("git clean"));

    let allowed = parse_json_output(
        &root,
        &[
            "--json",
            "guard-check",
            "--command",
            "git commit -m 'fix git clean parser'",
        ],
    );
    assert_eq!(allowed["decision"], "allow");
    assert_eq!(allowed["message"], Value::Null);

    let powershell = parse_json_output(
        &root,
        &[
            "--json",
            "guard-check",
            "--command",
            r#"git commit -m "fix `git clean` parser""#,
            "--shell",
            "powershell",
        ],
    );
    assert_eq!(powershell["decision"], "allow");
    assert_eq!(powershell["shell"], "powershell");

    let wrapped = parse_json_output(
        &root,
        &["--json", "guard-check", "--command", "exec git clean -fdX"],
    );
    assert_eq!(wrapped["decision"], "deny");
}

#[test]
fn json_commands_share_receipt_envelope() {
    let root = fixture_root("json-envelope");

    let files = parse_json_output(&root, &["--json", "files", ".", "--limit", "1"]);
    assert_envelope(&files, "files", "files");
    assert_eq!(files["output_truncated"], true);
    assert_eq!(files["complete"], false);
    assert!(has_cap(&files, "output", "paths"));

    let slice = parse_json_output(&root, &["--json", "slice", "sample.txt", "--range", "1:2"]);
    assert_envelope(&slice, "slice", "lines");
    assert_eq!(slice["complete"], true);
    assert_eq!(slice["caps"], serde_json::json!([]));

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
    assert_eq!(result(&json_find)["total"], 2);
}

#[test]
fn files_filters_by_literal_path_terms() {
    let root = fixture_root("files-path-terms");
    fs::create_dir_all(root.join("render")).unwrap();
    fs::create_dir_all(root.join("ui")).unwrap();
    fs::write(root.join("render/cgx_state.rs"), "cgx\n").unwrap();
    fs::write(root.join("render/other_state.rs"), "other\n").unwrap();
    fs::write(root.join("ui/cgx_state.rs"), "ui\n").unwrap();

    let files = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            ".",
            "--path-contains",
            "render",
            "--path-contains",
            "cgx",
            "--limit",
            "10",
        ],
    );

    assert_envelope(&files, "files", "files");
    assert_eq!(result(&files)["total"], 1);
    assert_eq!(files["files"][0], "render/cgx_state.rs");
}

#[test]
fn slice_accepts_positional_file() {
    let root = fixture_root("slice-positional-file");

    let json = parse_json_output(&root, &["--json", "slice", "sample.txt", "--range", "2:2"]);
    assert_envelope(&json, "slice", "lines");
    assert_eq!(json["path"], "sample.txt");
    assert_eq!(json["lines"][0]["line"], 2);
    assert_eq!(json["lines"][0]["text"], "alpha");
}

#[test]
fn outline_maps_declarations_with_receipt_envelope() {
    let root = fixture_root("outline-envelope");
    fs::write(
        root.join("sample.rs"),
        concat!(
            "use std::io;\n",
            "\n",
            "pub struct Frame {\n",
            "    depth: usize,\n",
            "}\n",
            "\n",
            "impl Frame {\n",
            "    pub fn render(&self) {\n",
            "        let local = 1;\n",
            "    }\n",
            "\n",
            "    fn cull_hidden(&mut self) {}\n",
            "}\n",
            "\n",
            "mod tests;\n",
        ),
    )
    .unwrap();

    let json = parse_json_output(&root, &["--json", "outline", "sample.rs"]);
    assert_envelope(&json, "outline", "items");
    assert_eq!(json["language"], "rust");
    assert_eq!(json["path"], "sample.rs");
    assert_eq!(json["total_lines"], 15);
    assert_eq!(result(&json)["total"], 5);
    assert_eq!(json["declaration_lines_total"], 5);
    assert_eq!(json["complete"], true);
    assert_eq!(json["items"][0]["line"], 3);
    assert_eq!(json["items"][0]["text"], "pub struct Frame {");
    assert_eq!(json["items"][2]["text"], "    pub fn render(&self) {");

    let filtered = parse_json_output(
        &root,
        &[
            "--json",
            "outline",
            "sample.rs",
            "--contains",
            "CULL",
            "--ignore-case",
        ],
    );
    assert_eq!(result(&filtered)["total"], 1);
    assert_eq!(filtered["declaration_lines_total"], 5);
    assert_eq!(
        filtered["items"][0]["text"],
        "    fn cull_hidden(&mut self) {}"
    );

    let capped = parse_json_output(&root, &["--json", "outline", "sample.rs", "--limit", "2"]);
    assert_eq!(capped["output_truncated"], true);
    assert!(has_cap(&capped, "output", "items"));
    assert_eq!(result(&capped)["shown"], 2);
    assert_eq!(result(&capped)["total"], 5);

    let human = run_contextmink(&root, &["outline", "sample.rs", "--limit", "2"]);
    assert!(human.contains("[contextmink] outline path=sample.rs language=rust total_lines=15"));
    assert!(human.contains("3: pub struct Frame {"));
    assert!(human.contains("capped outline at 2 items"));
    assert!(human.contains("CONTEXTMINK_RECEIPT "));
}

#[test]
fn outline_resolves_php_wgsl_and_markdown_document_rules_end_to_end() {
    let root = fixture_root("outline-added-languages");
    fs::write(
        root.join("controller.phtml"),
        "#[Route('/items')] FINAL CLASS Controller {}\nPUBLIC STATIC FUNCTION Build(): void {}\n$closure = static function () {};\n",
    )
    .unwrap();
    fs::write(
        root.join("shader.wgsl"),
        "@group(0) @binding(0) var<uniform> camera: Camera;\n@vertex fn main() -> @builtin(position) vec4<f32> {\n    var local = 1;\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("guide.md"),
        "# Visible\n```sh\n# hidden\n```not-a-close\n# still hidden\n```\n   ## Also visible\n",
    )
    .unwrap();

    let php = parse_json_output(&root, &["--json", "outline", "controller.phtml"]);
    assert_envelope(&php, "outline", "items");
    assert_eq!(php["language"], "php");
    assert_eq!(php["declaration_lines_total"], 2);
    assert_eq!(php["items"][0]["line"], 1);
    assert_eq!(php["items"][1]["line"], 2);

    let wgsl = parse_json_output(&root, &["--json", "outline", "shader.wgsl"]);
    assert_envelope(&wgsl, "outline", "items");
    assert_eq!(wgsl["language"], "wgsl");
    assert_eq!(wgsl["declaration_lines_total"], 2);
    assert_eq!(wgsl["items"][0]["line"], 1);
    assert_eq!(wgsl["items"][1]["line"], 2);

    let markdown = parse_json_output(&root, &["--json", "outline", "guide.md"]);
    assert_envelope(&markdown, "outline", "items");
    assert_eq!(markdown["language"], "markdown");
    assert_eq!(markdown["declaration_lines_total"], 2);
    assert_eq!(markdown["items"][0]["line"], 1);
    assert_eq!(markdown["items"][1]["line"], 7);
}

#[test]
fn payload_character_caps_are_shared_by_json_text_and_strict_mode() {
    let root = fixture_root("payload-character-caps");
    fs::write(
        root.join("long.rs"),
        "pub fn declaration_name_that_exceeds_the_budget() {}\n",
    )
    .unwrap();
    let long_file = root.join("filename_that_exceeds_the_budget.rs");
    fs::write(&long_file, "needle payload that exceeds the budget\n").unwrap();
    let long_dir = root.join("directory_name_that_exceeds_the_budget");
    fs::create_dir_all(&long_dir).unwrap();
    fs::write(long_dir.join("item.rs"), "fn item() {}\n").unwrap();

    let outline = parse_json_output(
        &root,
        &["--json", "outline", "long.rs", "--max-line-chars", "12"],
    );
    assert!(
        outline["items"][0]["text"]
            .as_str()
            .unwrap()
            .ends_with("...")
    );
    assert_eq!(
        outline["items"][0]["text"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        12
    );
    assert!(has_cap(&outline, "output", "line_characters"));

    let slice = parse_json_output(
        &root,
        &[
            "--json",
            "slice",
            "sample.txt",
            "--range",
            "1:1",
            "--max-line-chars",
            "5",
        ],
    );
    assert_eq!(slice["lines"][0]["text"], "al...");
    assert!(has_cap(&slice, "output", "line_characters"));

    let files = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            long_file.file_name().unwrap().to_str().unwrap(),
            "--max-line-chars",
            "8",
        ],
    );
    assert_eq!(files["files"][0].as_str().unwrap().chars().count(), 8);
    assert!(has_cap(&files, "output", "line_characters"));

    let dirs = parse_json_output(
        &root,
        &[
            "--json",
            "dirs",
            long_dir.file_name().unwrap().to_str().unwrap(),
            "--depth",
            "1",
            "--max-line-chars",
            "12",
        ],
    );
    assert!(
        dirs["dirs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["display"].as_str().unwrap().chars().count() <= 12)
    );
    assert!(has_cap(&dirs, "output", "line_characters"));
    let dirs_text = run_contextmink(
        &root,
        &[
            "dirs",
            long_dir.file_name().unwrap().to_str().unwrap(),
            "--depth",
            "1",
            "--max-line-chars",
            "12",
        ],
    );
    let dirs_receipt = parse_human_receipt(&dirs_text);
    assert!(has_cap(&dirs_receipt, "output", "line_characters"));
    assert!(
        dirs_text
            .lines()
            .filter(|line| {
                !line.starts_with("[contextmink]") && !line.starts_with("CONTEXTMINK_RECEIPT")
            })
            .all(|line| line.chars().count() <= 12),
        "output: {dirs_text}"
    );

    let grep = parse_json_output(
        &root,
        &[
            "--json",
            "grep",
            "needle",
            long_file.file_name().unwrap().to_str().unwrap(),
            "--max-line-chars",
            "10",
        ],
    );
    assert_eq!(
        grep["matching_files"][0]["samples"][0]["text"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        10
    );
    assert!(has_cap(&grep, "output", "line_characters"));

    let strict = run_contextmink_raw(
        &root,
        &[
            "--json",
            "--fail-if-truncated",
            "slice",
            "sample.txt",
            "--range",
            "1:1",
            "--max-line-chars",
            "5",
        ],
    );
    assert!(!strict.status.success());
    let strict_receipt: Value = serde_json::from_slice(&strict.stdout).unwrap();
    assert!(has_cap(&strict_receipt, "output", "line_characters"));
}

#[test]
fn grep_reports_actual_per_file_sample_match_omissions() {
    let root = fixture_root("grep-per-file-sample-omissions");
    fs::write(
        root.join("many.txt"),
        "needle one\nneedle two\nneedle three\n",
    )
    .unwrap();

    let omitted = parse_json_output(
        &root,
        &[
            "--json",
            "grep",
            "needle",
            "many.txt",
            "--lines-per-file",
            "1",
            "--max-sample-lines",
            "10",
        ],
    );
    assert_eq!(omitted["sample_matching_lines_omitted"], 2);
    assert_eq!(
        omitted["matching_files"][0]["sample_matching_lines_omitted"],
        2
    );
    assert!(has_cap(
        &omitted,
        "output",
        "sample_matching_lines_per_file"
    ));

    let retained_as_context = parse_json_output(
        &root,
        &[
            "--json",
            "grep",
            "needle",
            "many.txt",
            "--lines-per-file",
            "1",
            "--context",
            "2",
            "--max-sample-lines",
            "10",
        ],
    );
    assert_eq!(retained_as_context["sample_matching_lines_omitted"], 0);
    assert!(!has_cap(
        &retained_as_context,
        "output",
        "sample_matching_lines_per_file"
    ));
}

#[test]
fn outline_fails_fast_without_language_heuristic() {
    let root = fixture_root("outline-unknown");

    let output = run_contextmink_raw(&root, &["outline", "sample.txt"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--lang"), "stderr: {stderr}");

    let custom = parse_json_output(
        &root,
        &["--json", "outline", "sample.txt", "--pattern", "^alpha"],
    );
    assert_eq!(custom["language"], "pattern");
    assert_eq!(result(&custom)["total"], 2);

    let prefixed = parse_json_output(
        &root,
        &["--json", "outline", "sample.txt", "--prefix", "alpha"],
    );
    assert_eq!(prefixed["language"], "prefix");
    assert_eq!(result(&prefixed)["total"], 2);
    assert_eq!(prefixed["items"][0]["line"], 1);
}

#[test]
fn json_commands_accept_positional_file() {
    let root = fixture_root("json-positional-file");

    let find = parse_json_output(
        &root,
        &[
            "--json",
            "json-find",
            "sidecar.json",
            "--key-contains",
            "mode",
        ],
    );
    assert_envelope(&find, "json-find", "matches");
    assert_eq!(result(&find)["total"], 2);

    let select = parse_json_output(
        &root,
        &["--json", "json-select", "sidecar.json", "--fields", "/mode"],
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
    assert_eq!(json["child_exit_zero"], true);
    assert_eq!(json["child_exit_code"], 0);
    assert_eq!(json["execution_mode"], "native");
    assert!(json["effective_argv"].is_array());
    assert_eq!(json["stdout"]["shown_lines"], 1);
    assert!(json["stdout"]["total_lines"].as_u64().unwrap() > 1);
    assert!(json["stdout"]["omitted_lines"].as_u64().unwrap() > 0);
    assert_eq!(json["output_truncated"], true);
    assert!(has_cap(&json, "output", "lines"));
    // With a one-line budget the tail (verdict end of the output) wins.
    assert!(
        json["stdout_text"]
            .as_str()
            .unwrap()
            .contains("CONTEXTMINK_RECEIPT")
    );
}

#[test]
fn capture_keeps_head_and_tail_when_line_capped() {
    let root = fixture_root("capture-head-tail");
    let bin = env!("CARGO_BIN_EXE_contextmink");

    // slice 1:3 of sample.txt emits 3 content lines plus a receipt line.
    let json = parse_json_output(
        &root,
        &[
            "--json",
            "capture",
            "--max-lines",
            "2",
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
    let text = json["stdout_text"].as_str().unwrap();
    assert!(text.contains("alpha beta"), "head kept: {text}");
    assert!(text.contains("CONTEXTMINK_RECEIPT"), "tail kept: {text}");
    assert!(
        text.contains("[contextmink] ... omitted 2 line(s) ..."),
        "JSON carries the same bounded head/tail payload as text mode: {text}"
    );
    assert!(!text.contains("gamma delta"), "middle omitted: {text}");
    assert_eq!(json["stdout"]["head_lines"], 1);
    assert_eq!(json["stdout"]["tail_lines"], 1);
    assert_eq!(json["stdout"]["omitted_lines"], 2);
}

#[test]
fn capture_contiguous_byte_segments_preserve_every_line() {
    let root = fixture_root("capture-contiguous-byte-segments");
    let bin = env!("CARGO_BIN_EXE_contextmink");
    let source = (0..400)
        .map(|index| format!("LINE{index:04} {}\n", "x".repeat(31)))
        .collect::<String>();
    fs::write(root.join("many-lines.txt"), source).unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "capture",
            "--max-lines",
            "2000",
            "--max-line-chars",
            "2000",
            "--",
            bin,
            "--no-config",
            "slice",
            "many-lines.txt",
            "--range",
            "1:400",
            "--max-lines",
            "1000",
            "--max-line-chars",
            "1000",
        ],
    );

    assert_eq!(json["complete"], true);
    assert_eq!(json["output_truncated"], false);
    assert_eq!(json["caps"], serde_json::json!([]));
    assert_eq!(json["stdout"]["omitted_lines"], 0);
    assert_eq!(json["stdout"]["shown_lines"], json["stdout"]["total_lines"]);
    let text = json["stdout_text"].as_str().unwrap();
    assert!(text.contains("LINE0000"));
    assert!(text.contains("LINE0399"));
    assert!(!text.contains("[contextmink] ... omitted"));
}

#[test]
fn capture_json_applies_the_line_character_cap_to_payload_text() {
    let root = fixture_root("capture-line-characters");
    let bin = env!("CARGO_BIN_EXE_contextmink");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "capture",
            "--max-lines",
            "10",
            "--max-line-chars",
            "5",
            "--",
            bin,
            "--no-config",
            "slice",
            "sample.txt",
            "--range",
            "1:1",
        ],
    );
    let text = json["stdout_text"].as_str().unwrap();
    assert!(text.contains("1:..."), "clamped payload: {text}");
    assert!(!text.contains("alpha beta"), "unbounded payload: {text}");
    assert_eq!(json["stdout"]["char_truncated"], true);
    assert_eq!(json["output_truncated"], true);
    assert!(has_cap(&json, "output", "line_characters"));
}

#[test]
fn capture_json_applies_the_character_cap_to_omission_markers() {
    let root = fixture_root("capture-marker-line-characters");
    let bin = env!("CARGO_BIN_EXE_contextmink");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "capture",
            "--max-lines",
            "1",
            "--max-line-chars",
            "10",
            "--",
            bin,
            "--help",
        ],
    );
    let text = json["stdout_text"].as_str().unwrap();
    assert!(
        text.lines().all(|line| line.chars().count() <= 10),
        "{text}"
    );
    assert_eq!(json["stdout"]["line_truncated"], true);
    assert_eq!(json["stdout"]["char_truncated"], true);
    assert!(has_cap(&json, "output", "lines"));
    assert!(has_cap(&json, "output", "line_characters"));
}

#[test]
fn capture_byte_retention_does_not_claim_a_nonbinding_line_cap() {
    let root = fixture_root("capture-byte-only-cap");
    let bin = env!("CARGO_BIN_EXE_contextmink");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "capture",
            "--max-lines",
            "100",
            "--max-bytes",
            "100",
            "--max-line-chars",
            "1000",
            "--",
            bin,
            "--help",
        ],
    );
    assert_eq!(json["stdout"]["byte_truncated"], true);
    assert_eq!(json["stdout"]["line_truncated"], false);
    assert!(has_cap(&json, "output", "bytes_per_stream"));
    assert!(!has_cap(&json, "output", "lines"));
}

#[test]
fn capture_blocks_destructive_argv_before_spawn() {
    let root = fixture_root("capture-deny-destructive");

    let output = run_contextmink_raw(
        &root,
        &["capture", "--", "git", "clean", "-fdX", "-e", "keep.sqlite"],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("destructive command blocked"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("git clean"), "stderr: {stderr}");
}

#[test]
fn capture_blocks_configured_protected_fragment_before_spawn() {
    let root = fixture_root("capture-deny-configured-fragment");
    fs::write(
        root.join(".contextmink.toml"),
        "profile = \"test-profile\"\ndestructive_guard_recursive_delete_fragments = [\"protected_cache\"]\n",
    )
    .unwrap();

    let output = run_contextmink_raw(&root, &["capture", "--", "rm", "-rf", "protected_cache"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("destructive command blocked"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("protected_cache"), "stderr: {stderr}");
}

#[test]
fn capture_preserves_dash_prefixed_protected_target_after_option_terminator() {
    let root = fixture_root("capture-deny-dash-prefixed-protected-fragment");
    fs::write(
        root.join(".contextmink.toml"),
        "profile = \"test-profile\"\ndestructive_guard_recursive_delete_fragments = [\"-protected_cache\"]\n",
    )
    .unwrap();
    let protected = root.join("-protected_cache");
    fs::create_dir(&protected).unwrap();
    let survivor = protected.join("survivor.txt");
    fs::write(&survivor, "must survive\n").unwrap();

    let output = run_contextmink_raw(
        &root,
        &["capture", "--", "rm", "-rf", "--", "-protected_cache"],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("destructive command blocked"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("-protected_cache"), "stderr: {stderr}");
    assert!(
        survivor.is_file(),
        "capture spawned the denied deletion command"
    );
}

#[test]
fn capture_uses_capture_receipt_shape() {
    let root = fixture_root("capture-receipt-shape");
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
            "1:1",
        ],
    );
    assert_envelope(&json, "capture", "lines");
    assert_eq!(json["child_exit_zero"], true);
    assert_eq!(json["child_exit_code"], 0);
    assert!(json.get("exit_code").is_none());
    assert!(json.get("success").is_none());
}

#[test]
fn capture_script_runs_no_shebang_bash_script_through_shared_boundary() {
    let root = fixture_root("capture-explicit-script");
    fs::write(root.join("probe_script"), "printf '%s\\n' \"$1\"\n").unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "capture",
            "--script",
            "--",
            "probe_script",
            "two words",
        ],
    );
    assert_envelope(&json, "capture", "lines");
    assert_eq!(json["child_exit_zero"], true);
    assert_eq!(json["execution_mode"], "bash_script");
    assert!(json["stdout_text"].as_str().unwrap().contains("two words"));
    assert_eq!(json["effective_argv"].as_array().unwrap().len(), 3);
}

#[test]
fn fail_if_truncated_exits_nonzero_after_receipt() {
    let root = fixture_root("fail-if-truncated");

    let output = run_contextmink_raw(
        &root,
        &["--fail-if-truncated", "files", ".", "--limit", "1"],
    );
    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stdout.contains("CONTEXTMINK_RECEIPT "));
    assert!(stdout.contains("\"output_truncated\":true"));
    assert!(stderr.contains("strict completion requested"));
}

#[test]
fn strict_flags_and_scan_guard_fail_after_receipt() {
    let root = fixture_root("strict-flags");
    fs::write(root.join("extra_a.txt"), "a\n").unwrap();
    fs::write(root.join("extra_b.txt"), "b\n").unwrap();

    let strict = run_contextmink_raw(
        &root,
        &["--fail-if-truncated", "files", ".", "--limit", "1"],
    );
    assert!(!strict.status.success());
    let strict_stdout = String::from_utf8(strict.stdout).unwrap();
    assert!(strict_stdout.contains("CONTEXTMINK_RECEIPT "));

    let display_capped = run_contextmink_raw(
        &root,
        &["--require-complete-scope", "files", ".", "--limit", "1"],
    );
    assert!(display_capped.status.success());
    let display_stdout = String::from_utf8(display_capped.stdout).unwrap();
    assert!(display_stdout.contains("\"scope_complete\":true"));
    assert!(display_stdout.contains("\"output_truncated\":true"));

    let scan_capped = run_contextmink_raw(
        &root,
        &[
            "--require-complete-scope",
            "grep",
            "not-present",
            ".",
            "--max-content-files",
            "1",
        ],
    );
    assert!(!scan_capped.status.success());
    let scan_stdout = String::from_utf8(scan_capped.stdout).unwrap();
    let scan_stderr = String::from_utf8(scan_capped.stderr).unwrap();
    assert!(scan_stdout.contains("\"boundary\":\"scope\""));
    assert!(scan_stderr.contains("--require-complete-scope"));
}

#[test]
fn capture_propagates_unexpected_child_status_after_receipt() {
    let root = fixture_root("capture-nonzero");
    let bin = env!("CARGO_BIN_EXE_contextmink");

    let output = run_contextmink_raw(
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
    assert_eq!(output.status.code(), Some(1));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_envelope(&json, "capture", "lines");
    assert_eq!(json["child_exit_zero"], false);
    assert_ne!(json["child_exit_code"], 0);
    assert!(json["stderr"]["total_bytes"].as_u64().unwrap() > 0);
    assert!(
        json["stderr_text"]
            .as_str()
            .unwrap()
            .contains("missing.txt")
    );
}

#[test]
fn capture_child_exit_precedes_strict_truncation_status() {
    let root = fixture_root("capture-child-exit-precedence");
    let bin = env!("CARGO_BIN_EXE_contextmink");
    let output = run_contextmink_raw(
        &root,
        &[
            "--json",
            "--fail-if-truncated",
            "capture",
            "--max-lines",
            "1",
            "--",
            bin,
            "--definitely-invalid",
        ],
    );

    assert_eq!(output.status.code(), Some(2));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["child_exit_code"], 2);
    assert_eq!(json["output_truncated"], true);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("strictness error"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn capture_successful_child_keeps_outer_success() {
    let root = fixture_root("capture-success");
    let bin = env!("CARGO_BIN_EXE_contextmink");

    let output = run_contextmink_raw(
        &root,
        &[
            "--json",
            "capture",
            "--",
            bin,
            "--no-config",
            "slice",
            "sample.txt",
            "--range",
            "1:1",
        ],
    );
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["child_exit_zero"], true);
}

#[test]
fn capture_help_omits_the_removed_fail_with_child_flag() {
    let root = fixture_root("capture-removed-fail-with-child");
    let output = run_contextmink_raw(&root, &["capture", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("--fail-with-child"));
}

#[test]
fn capture_expect_exit_accepts_declared_nonzero_and_reports_child_exit_zero() {
    let root = fixture_root("capture-expect-exit");
    let bin = env!("CARGO_BIN_EXE_contextmink");

    let output = run_contextmink_raw(
        &root,
        &[
            "--json",
            "capture",
            "--expect-exit",
            "0,1",
            "--",
            bin,
            "--no-config",
            "slice",
            "missing.txt",
            "--range",
            "1:1",
        ],
    );
    assert!(
        output.status.success(),
        "declared child exit code should be accepted\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["child_exit_code"], 1);
    assert_eq!(json["child_exit_zero"], false);
    assert_eq!(json["exit_expected"], true);
    assert_eq!(json["expected_exit_codes"], serde_json::json!([0, 1]));
}

#[test]
fn capture_receipt_out_writes_full_json_receipt() {
    let root = fixture_root("capture-receipt-out");
    let bin = env!("CARGO_BIN_EXE_contextmink");
    let receipt = root.join("receipts").join("slice.json");

    let output = run_contextmink_raw(
        &root,
        &[
            "capture",
            "--receipt-out",
            receipt.to_str().unwrap(),
            "--",
            bin,
            "--no-config",
            "slice",
            "sample.txt",
            "--range",
            "1:1",
        ],
    );
    assert!(
        output.status.success(),
        "capture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_str(&fs::read_to_string(&receipt).unwrap()).unwrap();
    assert_envelope(&json, "capture", "lines");
    assert_eq!(json["exit_expected"], true);
    assert_eq!(json["child_exit_zero"], true);
    assert!(json["stdout_text"].as_str().unwrap().contains("alpha beta"));
}

#[test]
fn capture_sidecar_failure_still_emits_the_stdout_receipt() {
    let root = fixture_root("capture-sidecar-failure");
    let bin = env!("CARGO_BIN_EXE_contextmink");
    let output = run_contextmink_raw(
        &root,
        &[
            "--json",
            "capture",
            "--receipt-out",
            root.to_str().unwrap(),
            "--",
            bin,
            "--no-config",
            "slice",
            "sample.txt",
            "--range",
            "1:1",
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_envelope(&json, "capture", "lines");
    assert_eq!(json["child_exit_code"], 0);
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to write"));
}

#[test]
fn capture_receipt_out_uses_the_same_bounded_long_line_text() {
    let root = fixture_root("capture-receipt-out-long-line");
    let bin = env!("CARGO_BIN_EXE_contextmink");
    let receipt = root.join("receipts").join("long.json");
    let long_payload = "x".repeat(800);
    fs::write(root.join("long.txt"), format!("{long_payload}\n")).unwrap();

    let output = run_contextmink_raw(
        &root,
        &[
            "capture",
            "--max-line-chars",
            "80",
            "--max-bytes",
            "5000",
            "--receipt-out",
            receipt.to_str().unwrap(),
            "--",
            bin,
            "--no-config",
            "--json",
            "slice",
            "long.txt",
            "--range",
            "1:1",
            "--max-line-chars",
            "2000",
        ],
    );
    assert!(
        output.status.success(),
        "capture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_str(&fs::read_to_string(&receipt).unwrap()).unwrap();
    assert_eq!(json["stdout"]["output_truncated"], true);
    assert_eq!(json["stdout"]["char_truncated"], true);
    assert!(has_cap(&json, "output", "line_characters"));
    let stdout_text = json["stdout_text"].as_str().unwrap();
    assert!(!stdout_text.contains(&long_payload));
    assert!(stdout_text.lines().all(|line| line.chars().count() <= 80));
}

#[test]
fn files_display_cap_preserves_exact_scope() {
    let root = fixture_root("files-scan-cap");
    fs::write(root.join("extra_a.txt"), "a\n").unwrap();
    fs::write(root.join("extra_b.txt"), "b\n").unwrap();

    let files = parse_json_output(&root, &["--json", "files", ".", "--limit", "2"]);
    assert_envelope(&files, "files", "files");
    assert_eq!(result(&files)["shown"], 2);
    assert_eq!(files["output_truncated"], true);
    assert_eq!(files["scope_complete"], true);
    assert_eq!(files["complete"], false);
    assert!(has_cap(&files, "output", "paths"));
    // Enumeration completes before the cap applies: the total is exact and
    // the surviving subset is the sorted prefix.
    assert_eq!(result(&files)["total_is_lower_bound"], false);
    assert_eq!(result(&files)["total"], 5);
    assert_eq!(
        files["files"],
        serde_json::json!([".contextmink.toml", "extra_a.txt"])
    );
}

#[test]
fn files_deduplicates_overlapping_root_spellings() {
    let root = fixture_root("files-overlapping-roots");
    let absolute = root.to_string_lossy().into_owned();
    let files = parse_json_output(
        &root,
        &["--json", "files", ".", absolute.as_str(), "--limit", "10"],
    );

    assert_eq!(result(&files)["total"], 3);
    assert_eq!(result(&files)["shown"], 3);
    assert_eq!(files["output_truncated"], false);
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
    assert_eq!(result(&files)["shown"], 1);
    assert_eq!(result(&files)["total"], 1);
    assert_eq!(files["files"][0], "queue/work.jsonl");
}

#[test]
fn files_path_contains_matches_literal_decomp_ledger_name() {
    let root = fixture_root("files-term-ledger-name");
    fs::create_dir_all(
        root.join("decompilation_outputs")
            .join("ledgers")
            .join("rename"),
    )
    .unwrap();
    fs::write(
        root.join("decompilation_outputs")
            .join("ledgers")
            .join("rename")
            .join("rename_ledger_wow11655_ext_shadow_quality_description_20260306_v1.jsonl"),
        "{}\n",
    )
    .unwrap();
    fs::write(
        root.join("decompilation_outputs")
            .join("ledgers")
            .join("rename")
            .join("rename_ledger_wow12196_cshadereffect_bridge_lane_20260306_v1.jsonl"),
        "{}\n",
    )
    .unwrap();

    let files = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            "decompilation_outputs",
            "--path-contains",
            "rename_ledger_wow11655_ext_shadow_quality_description_20260306_v1.jsonl",
            "--limit",
            "20",
        ],
    );

    assert_envelope(&files, "files", "files");
    assert_eq!(result(&files)["shown"], 1);
    assert_eq!(result(&files)["total"], 1);
    assert_eq!(
        files["files"][0],
        "decompilation_outputs/ledgers/rename/rename_ledger_wow11655_ext_shadow_quality_description_20260306_v1.jsonl"
    );
}

#[test]
fn files_path_contains_does_not_match_an_ancestor_outside_the_scan_root() {
    let root = fixture_root("files-ancestor-only-term");
    let files = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            ".",
            "--path-contains",
            "files-ancestor-only-term",
        ],
    );

    assert_eq!(result(&files)["shown"], 0);
    assert_eq!(result(&files)["total"], 0);
}

#[test]
fn files_path_contains_composes_with_repeated_values_and_extension_filter() {
    let root = fixture_root("files-term-composed");
    fs::create_dir_all(root.join("queue")).unwrap();
    fs::write(root.join("queue").join("rename_alpha_target.jsonl"), "{}\n").unwrap();
    fs::write(root.join("queue").join("rename_alpha_target.txt"), "skip\n").unwrap();
    fs::write(root.join("queue").join("rename_beta_target.jsonl"), "{}\n").unwrap();

    let files = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            "queue",
            "--path-contains",
            "alpha",
            "--path-contains",
            "target",
            "--ext",
            "jsonl",
            "--limit",
            "5",
        ],
    );

    assert_envelope(&files, "files", "files");
    assert_eq!(result(&files)["shown"], 1);
    assert_eq!(result(&files)["total"], 1);
    assert_eq!(files["files"][0], "queue/rename_alpha_target.jsonl");
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
            "--json", "files", "queue", "--ext", ".json", "--ext", "jsonl", "--limit", "5",
        ],
    );

    assert_envelope(&files, "files", "files");
    assert_eq!(result(&files)["shown"], 2);
    assert_eq!(result(&files)["total"], 2);
    assert_eq!(files["files"][0], "queue/work.JSON");
    assert_eq!(files["files"][1], "queue/work.jsonl");
}

#[test]
fn files_positional_path_overrides_default_root() {
    let root = fixture_root("files-positional-path");
    fs::create_dir_all(root.join("queue")).unwrap();
    fs::write(root.join("queue").join("work.jsonl"), "{}\n").unwrap();

    let files = parse_json_output(&root, &["--json", "files", "queue"]);

    assert_envelope(&files, "files", "files");
    assert_eq!(result(&files)["total"], 1);
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

    let broad = parse_json_output(&root, &["--json", "files", ".", "--limit", "20"]);
    assert_envelope(&broad, "files", "files");
    let broad_files = broad["files"].as_array().unwrap();
    assert!(
        broad_files
            .iter()
            .all(|path| !path.as_str().unwrap().starts_with("artifacts/"))
    );

    let bypass = parse_json_output(
        &root,
        &["--json", "files", ".", "--with-excluded", "--limit", "20"],
    );
    assert_envelope(&bypass, "files", "files");
    assert!(
        bypass["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path.as_str().unwrap() == "artifacts/notes/finding.md")
    );

    let files = parse_json_output(
        &root,
        &["--json", "files", "artifacts/notes", "--limit", "20"],
    );
    assert_envelope(&files, "files", "files");
    assert_eq!(result(&files)["shown"], 1);
    assert_eq!(result(&files)["total"], 1);
    assert_eq!(files["files"][0], "artifacts/notes/finding.md");

    let grep = parse_json_output(
        &root,
        &[
            "--json",
            "grep",
            "needle",
            "artifacts/notes",
            "--max-content-files",
            "20",
        ],
    );
    assert_envelope(&grep, "grep", "matching_files");
    assert_eq!(result(&grep)["shown"], 1);
    assert_eq!(result(&grep)["total"], 1);
    assert_eq!(grep["matching_lines_total"], 1);
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

    // vendor/sqlite-tool is git-ignored but is itself a repo root: the
    // nested-repo supplement enters it and discloses the entry.
    let without_flag = parse_json_output(&root, &["--json", "files", ".", "--limit", "20"]);
    assert_envelope(&without_flag, "files", "files");
    let files_without_flag = without_flag["files"].as_array().unwrap();
    assert!(
        files_without_flag
            .iter()
            .any(|path| path.as_str().unwrap().trim_start_matches("./")
                == "vendor/sqlite-tool/README.md")
    );
    let nested = without_flag["nested_repos_entered"].as_array().unwrap();
    assert_eq!(nested.len(), 1);
    assert_eq!(
        nested[0].as_str().unwrap().trim_start_matches("./"),
        "vendor/sqlite-tool"
    );

    // --skip-nested-repos keeps the scan inside the explicit repository root.
    let skipped = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            ".",
            "--skip-nested-repos",
            "--limit",
            "20",
        ],
    );
    assert_envelope(&skipped, "files", "files");
    assert!(
        skipped["files"].as_array().unwrap().iter().all(|path| path
            .as_str()
            .unwrap()
            .trim_start_matches("./")
            != "vendor/sqlite-tool/README.md")
    );
    assert_eq!(skipped["nested_repos_entered"].as_array().unwrap().len(), 0);

    let with_flag = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            ".",
            "--with-git-ignored",
            "--limit",
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
fn tracked_submodule_style_repo_is_disclosed_and_skipped() {
    let root = fixture_root("tracked-submodule-boundary");
    fs::create_dir_all(root.join(".git")).unwrap();
    let nested = root.join("tracked-module");
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        nested.join(".git"),
        "gitdir: ../.git/modules/tracked-module\n",
    )
    .unwrap();
    fs::write(nested.join("README.md"), "tracked submodule\n").unwrap();

    let entered = parse_json_output(&root, &["--json", "files", ".", "--limit", "20"]);
    assert_envelope(&entered, "files", "files");
    assert!(
        entered["files"].as_array().unwrap().iter().any(|path| path
            .as_str()
            .unwrap()
            .trim_start_matches("./")
            == "tracked-module/README.md")
    );
    assert_eq!(entered["nested_repos_entered_total"], 1);
    assert_eq!(
        entered["nested_repos_entered"][0]
            .as_str()
            .unwrap()
            .trim_start_matches("./"),
        "tracked-module"
    );

    let skipped = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            ".",
            "--skip-nested-repos",
            "--limit",
            "20",
        ],
    );
    assert_envelope(&skipped, "files", "files");
    assert!(
        skipped["files"].as_array().unwrap().iter().all(|path| path
            .as_str()
            .unwrap()
            .trim_start_matches("./")
            != "tracked-module/README.md")
    );
    assert_eq!(skipped["nested_repos_entered_total"], 0);
}

#[test]
fn nested_repo_supplement_is_deterministic_when_probe_is_parallel() {
    let root = fixture_root("parallel-nested-repo-probe");
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join(".gitignore"), "group/\nignored/*\n").unwrap();
    for index in 0..96 {
        let dir = root.join("src").join(format!("dir-{index:03}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("item.txt"), "candidate\n").unwrap();
    }
    for repo in [root.join("group/a-repo"), root.join("ignored/z-repo")] {
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(repo.join("README.md"), "nested repository\n").unwrap();
    }

    let expected = ["group/a-repo", "ignored/z-repo"];
    for _ in 0..4 {
        let receipt = parse_json_output(&root, &["--json", "files", ".", "--limit", "200"]);
        let nested = receipt["nested_repos_entered"]
            .as_array()
            .unwrap()
            .iter()
            .map(|path| path.as_str().unwrap().trim_start_matches("./"))
            .collect::<Vec<_>>();
        assert_eq!(nested, expected);
    }
}

#[test]
fn grep_content_file_cap_marks_no_match_as_scanned_subset_only() {
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
            "--max-content-files",
            "1",
        ],
    );
    assert_envelope(&grep, "grep", "matching_files");
    assert_eq!(result(&grep)["shown"], 0);
    assert_eq!(grep["scope_complete"], false);
    assert_eq!(grep["complete"], false);
    assert!(has_cap(&grep, "scope", "content_files"));
    assert_eq!(grep["candidate_files_selected"], 1);
    // The candidate total is exact even though content scanning was capped.
    assert_eq!(result(&grep)["total_is_lower_bound"], true);
    assert_eq!(grep["candidate_files_total"], 5);
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
    assert_envelope(&json, "grep-terms", "matching_files");
    assert_eq!(json["matching_lines_total"], 1);

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
    assert_envelope(&receipt, "grep-terms", "matching_files");
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
    assert_envelope(&default_all, "grep-terms", "matching_files");
    assert_eq!(default_all["pattern"], "all_terms(alpha,beta)");
    assert_eq!(default_all["matching_lines_total"], 1);

    let any = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--any",
            "--term",
            "alpha",
            "--term",
            "beta",
            "sample.txt",
        ],
    );
    assert_envelope(&any, "grep-terms", "matching_files");
    assert_eq!(any["pattern"], "any_terms(alpha,beta)");
    assert_eq!(any["matching_lines_total"], 3);

    let term_file = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--any",
            "--term-file",
            "phrases.txt",
            "sample.txt",
        ],
    );
    assert_envelope(&term_file, "grep-terms", "matching_files");
    assert_eq!(term_file["pattern"], "any_terms(alpha beta,missing phrase)");
    assert_eq!(term_file["matching_lines_total"], 1);
}

#[test]
fn grep_terms_accepts_canonical_limits() {
    let root = fixture_root("grep-terms-canonical-limits");
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
            "--max-sample-lines",
            "1",
            "sample.txt",
        ],
    );
    assert_envelope(&json, "grep-terms", "matching_files");
    assert_eq!(result(&json)["shown"], 1);
    assert_eq!(json["sample_lines_shown"], 1);
    assert!(has_cap(&json, "output", "sample_lines"));
    assert_eq!(
        json["matching_files"][0]["samples"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let flag_like = parse_json_output(
        &root,
        &["--json", "grep-terms", "--term", "--flag-like", "flags.txt"],
    );
    assert_envelope(&flag_like, "grep-terms", "matching_files");
    assert_eq!(flag_like["matching_lines_total"], 1);

    let help = run_contextmink(&root, &["grep-terms", "--help"]);
    assert!(help.contains("--max-matching-files"));
    assert!(help.contains("--limit"));
    assert!(help.contains("--max-sample-lines"));
    assert!(help.contains("--any"));
    assert!(!help.contains("--max-lines"));
    assert!(!help.contains("--mode"));
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
            "--max-matching-files",
            "2",
            "--limit",
            "2",
            "matches",
        ],
    );
    assert_envelope(&json, "grep-terms", "matching_files");
    assert_eq!(result(&json)["shown"], 2);
    assert_eq!(result(&json)["total"], 2);
    assert_eq!(result(&json)["total_is_lower_bound"], true);
    assert_eq!(json["matching_lines_total"], 2);
    assert_eq!(json["matching_lines_total_is_lower_bound"], true);
    assert_eq!(json["candidate_files_selected"], 5);
    assert_eq!(json["content_files_scanned"], 2);
    assert!(has_cap(&json, "scope", "matching_files"));
    assert_eq!(json["scope_complete"], false);

    let guarded = run_contextmink_raw(
        &root,
        &[
            "--require-complete-scope",
            "grep-terms",
            "--term",
            "needle",
            "--max-matching-files",
            "2",
            "matches",
        ],
    );
    assert!(!guarded.status.success());
    let guarded_stdout = String::from_utf8(guarded.stdout).unwrap();
    let guarded_stderr = String::from_utf8(guarded.stderr).unwrap();
    assert!(guarded_stdout.contains("\"dimension\":\"matching_files\""));
    assert!(guarded_stderr.contains("--require-complete-scope"));
}

#[test]
fn grep_marks_match_totals_lower_bound_when_content_file_scope_is_capped() {
    let root = fixture_root("grep-scan-cap-match-bounds");
    let matches = root.join("matches");
    fs::create_dir_all(&matches).unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(matches.join(name), "needle\n").unwrap();
    }

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--term",
            "needle",
            "--max-content-files",
            "1",
            "--max-matching-files",
            "10",
            "--limit",
            "10",
            "matches",
        ],
    );
    assert_envelope(&json, "grep-terms", "matching_files");
    assert_eq!(json["candidate_files_total"], 3);
    assert_eq!(json["candidate_files_selected"], 1);
    assert_eq!(json["content_files_scanned"], 1);
    assert_eq!(result(&json)["total"], 1);
    assert_eq!(result(&json)["total_is_lower_bound"], true);
    assert_eq!(json["matching_lines_total"], 1);
    assert_eq!(json["matching_lines_total_is_lower_bound"], true);
    assert!(has_cap(&json, "scope", "content_files"));
    assert_eq!(json["scope_complete"], false);
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
    assert_envelope(&json, "grep", "matching_files");
    assert_eq!(result(&json)["shown"], 1);
    assert_eq!(json["matching_files"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["matching_files"][0]["samples"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(json["sample_lines_shown"], 1);
    assert!(has_cap(&json, "output", "sample_lines"));
    assert_eq!(json["output_truncated"], true);
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
    assert_envelope(&json, "grep", "matching_files");
    assert_eq!(json["pattern"], "\"alpha|beta\"");
    assert_eq!(json["matching_lines_total"], 3);
}

#[test]
fn grep_accepts_positional_search_paths() {
    let root = fixture_root("grep-positional-path");

    let json = parse_json_output(&root, &["--json", "grep", "alpha", "sample.txt"]);
    assert_envelope(&json, "grep", "matching_files");
    assert_eq!(result(&json)["shown"], 1);
    assert_eq!(json["matching_lines_total"], 2);
    assert_eq!(json["matching_files"][0]["path"], "sample.txt");
}

#[test]
fn grep_pattern_flag_treats_all_positionals_as_paths() {
    let root = fixture_root("grep-pattern-flag");
    fs::write(root.join("alpha"), "needle\n").unwrap();
    fs::write(root.join("beta.txt"), "needle\n").unwrap();

    let json = parse_json_output(
        &root,
        &["--json", "grep", "--pattern", "needle", "alpha", "beta.txt"],
    );

    assert_envelope(&json, "grep", "matching_files");
    assert_eq!(result(&json)["shown"], 2);
    assert_eq!(json["matching_lines_total"], 2);
    assert_eq!(json["matching_files"][0]["path"], "alpha");
    assert_eq!(json["matching_files"][1]["path"], "beta.txt");
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
            "--fields",
            "index",
            "--fields",
            "path",
        ],
    );
    assert_envelope(&json, "json-select", "rows");
    assert_eq!(result(&json)["total"], 2);
    assert_eq!(json["rows"][0]["fields"]["index"], "0");
    assert_eq!(json["rows"][0]["fields"]["path"], "\"World|A.blp\"");
}

#[test]
fn json_select_accepts_comma_separated_fields() {
    let root = fixture_root("json-select-fields-list");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "sidecar.json",
            "--array",
            "/textures",
            "--fields",
            "index,path",
        ],
    );
    assert_envelope(&json, "json-select", "rows");
    assert_eq!(json["fields"][0], "index");
    assert_eq!(json["fields"][1], "path");
    assert_eq!(json["rows"][0]["fields"]["index"], "0");
    assert_eq!(json["rows"][0]["fields"]["path"], "\"World|A.blp\"");
}

#[test]
fn json_select_preserves_literal_slash_field_identity() {
    let root = fixture_root("json-select-literal-slash-field");
    fs::write(
        root.join("literal.json"),
        r#"{"tools/Git/hooks":5,"hooks":"decoy"}"#,
    )
    .unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "literal.json",
            "--fields",
            "tools/Git/hooks",
        ],
    );

    assert_eq!(json["fields"][0], "tools/Git/hooks");
    assert_eq!(json["rows"][0]["fields"]["tools/Git/hooks"], "5");
}

#[test]
fn json_select_shape_mismatch_is_a_null_non_match() {
    let root = fixture_root("json-select-heterogeneous-shape");
    fs::write(
        root.join("heterogeneous.json"),
        r#"[{"v":{"x":1}},{"v":[9]}]"#,
    )
    .unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "heterogeneous.json",
            "--array",
            "$",
            "--fields",
            "/v/x",
        ],
    );

    assert_eq!(json["rows"][0]["fields"]["/v/x"], "1");
    assert_eq!(json["rows"][1]["fields"]["/v/x"], "null");
}

#[test]
fn json_select_projects_jsonl_rows_with_limit() {
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
            "--fields",
            "addr",
            "--fields",
            "flags",
            "--limit",
            "1",
        ],
    );

    assert_envelope(&json, "json-select", "rows");
    assert_eq!(json["input_format"], "jsonl");
    assert_eq!(result(&json)["shown"], 1);
    assert_eq!(result(&json)["total"], 2);
    assert_eq!(json["output_truncated"], true);
    assert_eq!(json["rows"][0]["fields"]["addr"], "\"0x408690\"");
    assert_eq!(
        json["rows"][0]["fields"]["flags"],
        "[\"custom_register_args\"]"
    );
}

#[test]
fn canonical_limits_cap_outputs() {
    let root = fixture_root("canonical-limits");
    fs::write(root.join("extra.txt"), "alpha\n").unwrap();

    let files = parse_json_output(&root, &["--json", "files", ".", "--limit", "1"]);
    assert_envelope(&files, "files", "files");
    assert_eq!(result(&files)["shown"], 1);
    assert_eq!(files["output_truncated"], true);

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
    assert_eq!(result(&json_find)["shown"], 1);
    assert_eq!(result(&json_find)["total"], 2);
    assert_eq!(json_find["output_truncated"], true);

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
    assert_eq!(result(&sqlite)["shown"], 1);
    assert_eq!(result(&sqlite)["total"], 2);
    assert_eq!(sqlite["output_truncated"], true);
    assert_eq!(result(&sqlite)["total_is_lower_bound"], false);

    let sqlite_scan = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite",
            "limit.sqlite",
            "--sql",
            "SELECT * FROM rows ORDER BY id",
            "--limit",
            "1",
            "--max-rows-scanned",
            "1",
        ],
    );
    assert_envelope(&sqlite_scan, "sqlite", "rows");
    assert_eq!(result(&sqlite_scan)["total_is_lower_bound"], true);
    assert!(has_cap(&sqlite_scan, "scope", "rows_processed"));

    let guarded_scan = run_contextmink_raw(
        &root,
        &[
            "--require-complete-scope",
            "sqlite",
            "limit.sqlite",
            "--sql",
            "SELECT * FROM rows ORDER BY id",
            "--limit",
            "1",
            "--max-rows-scanned",
            "1",
        ],
    );
    assert!(!guarded_scan.status.success());
    assert!(
        String::from_utf8(guarded_scan.stderr)
            .unwrap()
            .contains("--require-complete-scope")
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
            "sample.sqlite",
            "--sql-file",
            "query.sql",
            "--limit",
            "1",
        ],
    );
    assert_envelope(&json, "sqlite", "rows");
    assert_eq!(result(&json)["shown"], 1);
    assert_eq!(result(&json)["total"], 2);
    assert!(has_cap(&json, "output", "rows"));
    assert_eq!(json["rows"][0]["fields"]["joined"], "\"alpha:beta\"");
}

#[test]
fn sqlite_rejects_multiple_executable_statements_without_rejecting_empty_tails() {
    let root = fixture_root("sqlite-single-statement");
    let db_path = root.join("sample.sqlite");
    rusqlite::Connection::open(&db_path).unwrap();

    let rejected = run_contextmink_raw(
        &root,
        &[
            "sqlite",
            "sample.sqlite",
            "--sql",
            "SELECT 1 AS first; SELECT 2 AS second",
        ],
    );
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8(rejected.stderr)
            .unwrap()
            .contains("exactly one executable read-only statement; found 2")
    );

    let accepted = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite",
            "sample.sqlite",
            "--sql",
            "/* leading comment */; SELECT 1 AS one; -- empty tail\n;",
        ],
    );
    assert_eq!(accepted["rows"][0]["fields"]["one"], "1");
}

#[test]
fn sqlite_binds_json_and_jsonl_file_params() {
    let root = fixture_root("sqlite-json-params");
    let db_path = root.join("sample.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE items(addr INTEGER PRIMARY KEY, name TEXT NOT NULL, size INTEGER NOT NULL);
         CREATE TABLE exclusions(item_key TEXT NOT NULL, status TEXT NOT NULL);
         INSERT INTO items(addr, name, size) VALUES (16, 'item_a', 4), (32, 'item_b', 8);
         INSERT INTO exclusions(item_key, status) VALUES ('item_a@0x00000010', 'active');",
    )
    .unwrap();
    drop(conn);
    fs::write(
        root.join("queue.jsonl"),
        concat!(
            r#"{"state":"ready","item":{"name":"item_a","address":{"addr":"0x00000010"}}}"#,
            "\n",
            r#"{"state":"ready","item":{"name":"item_b","address":{"addr":"0x00000020"}}}"#,
            "\n",
            r#"{"state":"pending","item":{"name":"item_c","address":{"addr":"0x00000030"}}}"#,
            "\n"
        ),
    )
    .unwrap();
    fs::write(root.join("filter.json"), r#"{"min_size":8}"#).unwrap();
    fs::write(
        root.join("query.sql"),
        "WITH queue AS (
             SELECT
               json_extract(value, '$.item.name') AS name,
               lower(json_extract(value, '$.item.address.addr')) AS addr_hex,
               json_extract(value, '$.item.name') || '@' || lower(json_extract(value, '$.item.address.addr')) AS item_key
             FROM json_each(:queue)
             WHERE json_extract(value, '$.state') = 'ready'
           )
           SELECT q.name, f.size
           FROM queue q
           JOIN items f ON printf('0x%08x', f.addr) = q.addr_hex
           WHERE f.size >= json_extract(:filter, '$.min_size')
             AND NOT EXISTS (
               SELECT 1 FROM exclusions c
               WHERE c.item_key = q.item_key AND c.status != 'retired'
             )
           ORDER BY f.addr",
    )
    .unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite",
            "sample.sqlite",
            "--sql-file",
            "query.sql",
            "--jsonl-param",
            "queue=queue.jsonl",
            "--json-param",
            "filter=filter.json",
        ],
    );
    assert_envelope(&json, "sqlite", "rows");
    assert_eq!(result(&json)["shown"], 1);
    assert_eq!(json["rows"][0]["fields"]["name"], "\"item_b\"");
    assert_eq!(json["rows"][0]["fields"]["size"], "8");
    assert_eq!(json["params"][0]["name"], ":filter");
    assert_eq!(json["params"][0]["format"], "json");
    assert_eq!(json["params"][1]["name"], ":queue");
    assert_eq!(json["params"][1]["format"], "jsonl");
}

#[test]
fn sqlite_file_params_fail_fast_on_unbound_sql_parameters() {
    let root = fixture_root("sqlite-json-param-unbound");
    let db_path = root.join("sample.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch("CREATE TABLE t(value INTEGER);")
        .unwrap();
    drop(conn);
    fs::write(root.join("queue.jsonl"), "{\"value\":1}\n").unwrap();

    let output = run_contextmink_raw(
        &root,
        &[
            "sqlite",
            "sample.sqlite",
            "--sql",
            "SELECT :queue, :missing",
            "--jsonl-param",
            "queue=queue.jsonl",
        ],
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("unbound sqlite parameter :missing")
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
            "schema.sqlite",
            "--table",
            "child",
        ],
    );
    assert_envelope(&json, "sqlite-schema", "tables");
    assert_eq!(result(&json)["shown"], 1);
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
    assert_eq!(capped["output_truncated"], true);
    assert!(has_cap(&capped, "output", "tables") || has_cap(&capped, "output", "columns"));
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
    assert_eq!(result(&json)["shown"], 3);
    assert_eq!(result(&json)["total"], 3);
    assert_eq!(json["end"], 3);
    assert_eq!(json["output_truncated"], false);
    assert_eq!(json["complete"], true);
    assert_eq!(json["caps"], serde_json::json!([]));
}

#[test]
fn slice_character_window_is_a_complete_requested_selection() {
    let root = fixture_root("slice-character-window");
    let json = parse_json_output(
        &root,
        &[
            "--json",
            "slice",
            "sample.txt",
            "--char-start",
            "1",
            "--chars",
            "5",
        ],
    );

    assert_eq!(json["mode"], "chars");
    assert_eq!(result(&json)["shown"], 5);
    assert_eq!(json["text"], "lpha ");
    assert_eq!(json["complete"], true);
    assert_eq!(json["output_truncated"], false);
    assert_eq!(json["caps"], serde_json::json!([]));
}

#[test]
fn slice_rejects_cross_mode_flags_instead_of_ignoring_them() {
    let root = fixture_root("slice-cross-mode-flags");
    let output = run_contextmink_raw(
        &root,
        &[
            "slice",
            "sample.txt",
            "--char-start",
            "0",
            "--chars",
            "5",
            "--start",
            "2",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));

    let chars_without_mode = run_contextmink_raw(&root, &["slice", "sample.txt", "--chars", "5"]);
    assert_eq!(chars_without_mode.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&chars_without_mode.stderr).contains("--char-start"));
}
#[test]
fn grep_filters_by_extension_and_glob() {
    let root = fixture_root("grep-ext-glob");
    fs::write(root.join("code.rs"), "needle in rust\n").unwrap();
    fs::write(root.join("notes.md"), "needle in markdown\n").unwrap();

    let by_ext = parse_json_output(&root, &["--json", "grep", "needle", ".", "--ext", "rs"]);
    assert_envelope(&by_ext, "grep", "matching_files");
    assert_eq!(result(&by_ext)["total"], 1);
    assert_eq!(by_ext["matching_files"][0]["path"], "code.rs");

    let by_glob = parse_json_output(&root, &["--json", "grep", "needle", ".", "--glob", "*.md"]);
    assert_envelope(&by_glob, "grep", "matching_files");
    assert_eq!(result(&by_glob)["total"], 1);
    assert_eq!(by_glob["matching_files"][0]["path"], "notes.md");
}

#[test]
fn grep_ignore_case_matches_and_labels() {
    let root = fixture_root("grep-ignore-case");
    fs::write(root.join("mixed.txt"), "NeEdLe here\n").unwrap();

    let sensitive = parse_json_output(&root, &["--json", "grep", "needle", "mixed.txt"]);
    assert_eq!(sensitive["matching_lines_total"], 0);

    let insensitive = parse_json_output(
        &root,
        &["--json", "grep", "-i", "--literal", "needle", "mixed.txt"],
    );
    assert_eq!(insensitive["matching_lines_total"], 1);
    assert!(
        insensitive["pattern"]
            .as_str()
            .unwrap()
            .contains("ignore_case")
    );

    let terms = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "-i",
            "--term",
            "NEEDLE",
            "mixed.txt",
        ],
    );
    assert_eq!(terms["matching_lines_total"], 1);
}

#[test]
fn grep_context_lines_render_with_dash_separator() {
    let root = fixture_root("grep-context");
    fs::write(root.join("ctx.txt"), "before\nneedle\nafter\n").unwrap();

    let json = parse_json_output(
        &root,
        &["--json", "grep", "needle", "ctx.txt", "--context", "1"],
    );
    let samples = json["matching_files"][0]["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 3);
    assert_eq!(samples[0]["is_match"], false);
    assert_eq!(samples[1]["is_match"], true);
    assert_eq!(samples[2]["is_match"], false);

    let human = run_contextmink(&root, &["grep", "needle", "ctx.txt", "--context", "1"]);
    assert!(human.contains("ctx.txt:1-before"));
    assert!(human.contains("ctx.txt:2:needle"));
    assert!(human.contains("ctx.txt:3-after"));
}

#[test]
fn grep_scans_utf16_files_and_lists_skipped_files() {
    let root = fixture_root("grep-utf16-skips");
    let mut utf16 = vec![0xFF, 0xFE];
    for unit in "needle utf16\n".encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(root.join("powershell.log"), utf16).unwrap();
    fs::write(root.join("binary.bin"), b"MZ\x00\x00needle").unwrap();

    let json = parse_json_output(&root, &["--json", "grep", "needle", "."]);
    assert_eq!(json["matching_lines_total"], 1);
    assert_eq!(json["matching_files"][0]["path"], "powershell.log");
    assert_eq!(json["skipped_large_or_binary"], 1);
    let skipped = json["skipped_files_sample"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["path"], "binary.bin");
    assert_eq!(skipped[0]["reason"], "binary");
}

#[test]
fn grep_no_match_scope_demotes_when_large_files_skipped() {
    let root = fixture_root("grep-large-skip-scope");
    fs::write(root.join("big.txt"), "x".repeat(64)).unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "grep",
            "not-present",
            "big.txt",
            "--max-file-bytes",
            "8",
        ],
    );
    assert_eq!(json["matching_lines_total"], 0);
    assert_eq!(json["no_match_scope"], "scanned_subset");
    assert_eq!(json["skipped_files_sample"][0]["reason"], "large");
}

#[test]
fn slice_tail_returns_last_lines() {
    let root = fixture_root("slice-tail");

    let json = parse_json_output(&root, &["--json", "slice", "sample.txt", "--tail", "2"]);
    assert_envelope(&json, "slice", "lines");
    assert_eq!(result(&json)["shown"], 2);
    assert_eq!(json["lines"][0]["line"], 2);
    assert_eq!(json["lines"][0]["text"], "alpha");
    assert_eq!(json["lines"][1]["line"], 3);
    assert_eq!(json["lines"][1]["text"], "beta");
    assert_eq!(json["encoding"], "utf8");
}

#[test]
fn json_select_where_filters_rows() {
    let root = fixture_root("json-select-where");
    fs::write(
        root.join("queue.jsonl"),
        "{\"addr\":\"0x1\",\"state\":\"open\"}\n{\"addr\":\"0x2\",\"state\":\"closed\"}\n{\"addr\":\"0x3\",\"state\":\"open\"}\n",
    )
    .unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "queue.jsonl",
            "--fields",
            "addr",
            "--where",
            "state=open",
        ],
    );
    assert_envelope(&json, "json-select", "rows");
    assert_eq!(result(&json)["total"], 2);
    assert_eq!(json["rows_scanned"], 3);
    assert_eq!(json["rows"][0]["fields"]["addr"], "\"0x1\"");
    assert_eq!(json["rows"][1]["fields"]["addr"], "\"0x3\"");

    let contains = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "queue.jsonl",
            "--fields",
            "addr",
            "--where-contains",
            "state=clo",
        ],
    );
    assert_eq!(result(&contains)["total"], 1);
    assert_eq!(contains["rows"][0]["fields"]["addr"], "\"0x2\"");
}

#[test]
fn json_select_reports_all_null_fields() {
    let root = fixture_root("json-select-all-null");
    fs::write(
        root.join("rows.jsonl"),
        "{\"addr\":\"0x1\"}\n{\"addr\":\"0x2\"}\n",
    )
    .unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "rows.jsonl",
            "--fields",
            "addr",
            "--fields",
            "typo_field",
        ],
    );
    let all_null = json["all_null_fields"].as_array().unwrap();
    assert_eq!(all_null.len(), 1);
    assert_eq!(all_null[0], "typo_field");

    let human = run_contextmink(
        &root,
        &["json-select", "rows.jsonl", "--fields", "typo_field"],
    );
    assert!(human.contains("warning: field(s) typo_field"));
}

#[test]
fn json_commands_tolerate_utf8_bom_documents() {
    let root = fixture_root("json-bom");
    fs::write(root.join("bom.json"), b"\xEF\xBB\xBF{\"mode\":\"demo\"}").unwrap();

    let json = parse_json_output(
        &root,
        &["--json", "json-find", "bom.json", "--key-contains", "mode"],
    );
    assert_eq!(result(&json)["total"], 1);

    let select = parse_json_output(
        &root,
        &["--json", "json-select", "bom.json", "--fields", "mode"],
    );
    assert_eq!(select["rows"][0]["fields"]["mode"], "\"demo\"");
}

#[test]
fn json_select_decodes_bom_jsonl_consistently() {
    let root = fixture_root("jsonl-bom");
    fs::write(
        root.join("utf8.jsonl"),
        b"\xEF\xBB\xBF{\"id\":1}\n{\"id\":2}\n",
    )
    .unwrap();
    let utf8 = parse_json_output(
        &root,
        &["--json", "json-select", "utf8.jsonl", "--fields", "id"],
    );
    assert_eq!(result(&utf8)["total"], 2);

    let mut utf16 = vec![0xFF, 0xFE];
    for unit in "{\"id\":1}\n{\"id\":2}\n".encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(root.join("utf16.jsonl"), utf16).unwrap();
    let utf16 = parse_json_output(
        &root,
        &["--json", "json-select", "utf16.jsonl", "--fields", "id"],
    );
    assert_eq!(result(&utf16)["total"], 2);
    assert_eq!(utf16["rows"][1]["fields"]["id"], "2");
}

#[test]
fn dirs_reports_bounded_recursive_file_counts() {
    let root = fixture_root("dirs-overview");
    fs::create_dir_all(root.join("crates").join("alpha").join("src")).unwrap();
    fs::create_dir_all(root.join("crates").join("beta")).unwrap();
    fs::write(
        root.join("crates").join("alpha").join("src").join("lib.rs"),
        "x\n",
    )
    .unwrap();
    fs::write(root.join("crates").join("alpha").join("Cargo.toml"), "x\n").unwrap();
    fs::write(root.join("crates").join("beta").join("Cargo.toml"), "x\n").unwrap();

    let json = parse_json_output(&root, &["--json", "dirs", "crates", "--depth", "1"]);
    assert_envelope(&json, "dirs", "dirs");
    let dirs = json["dirs"].as_array().unwrap();
    let find = |name: &str| {
        dirs.iter()
            .find(|dir| dir["path"] == name)
            .unwrap_or_else(|| panic!("missing dir {name} in {dirs:?}"))
    };
    assert_eq!(find("crates")["files"], 3);
    assert_eq!(find("crates/alpha")["files"], 2);
    assert_eq!(find("crates/beta")["files"], 1);

    let deeper = parse_json_output(&root, &["--json", "dirs", "crates", "--depth", "2"]);
    let dirs = deeper["dirs"].as_array().unwrap();
    assert!(dirs.iter().any(|dir| dir["path"] == "crates/alpha/src"));
}

#[test]
fn config_typos_fail_fast() {
    let root = fixture_root("config-typo");
    fs::write(
        root.join(".contextmink.toml"),
        "profile = \"x\"\nexclude_glob = [\"typo/**\"]\n",
    )
    .unwrap();

    let output = run_contextmink_raw(&root, &["files", ".", "--limit", "1"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("unknown key `exclude_glob`")
    );
}

#[test]
fn receipts_carry_duration_ms() {
    let root = fixture_root("duration-ms");

    let json = parse_json_output(&root, &["--json", "files", ".", "--limit", "1"]);
    assert!(json["duration_ms"].is_number());
}

#[test]
fn excludes_hold_for_absolute_scan_roots() {
    let root = fixture_root("absolute-root-policy");
    fs::write(
        root.join(".contextmink.toml"),
        "profile = \"test-profile\"\nexclude_globs = [\"artifacts/**\"]\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("artifacts")).unwrap();
    fs::write(root.join("artifacts").join("big.log"), "noise\n").unwrap();

    // Anchored excludes must hold even when the scan root is an absolute
    // path (or the command runs from a subdirectory), not only for
    // config-root-relative spellings.
    let absolute_root = root.to_string_lossy().replace('\\', "/");
    let files = parse_json_output(&root, &["--json", "files", &absolute_root, "--limit", "50"]);
    assert_envelope(&files, "files", "files");
    let listed = files["files"].as_array().unwrap();
    assert!(
        listed
            .iter()
            .all(|path| !path.as_str().unwrap().contains("artifacts/")),
        "absolute-root scan must honor anchored excludes: {listed:?}"
    );
    assert!(
        listed
            .iter()
            .any(|path| path.as_str().unwrap().ends_with("sample.txt"))
    );

    // An explicit absolute path INTO the excluded tree is still the target.
    let absolute_excluded = format!("{absolute_root}/artifacts");
    let explicit = parse_json_output(
        &root,
        &["--json", "files", &absolute_excluded, "--limit", "10"],
    );
    assert_eq!(result(&explicit)["total"], 1);
    assert!(
        explicit["files"][0]
            .as_str()
            .unwrap()
            .ends_with("artifacts/big.log")
    );
}

#[test]
fn bare_config_filename_keeps_excludes_for_absolute_scan_roots() {
    let root = fixture_root("bare-config-policy-root");
    fs::write(
        root.join(".contextmink.toml"),
        "profile = \"test-profile\"\nexclude_globs = [\"artifacts/**\"]\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("artifacts")).unwrap();
    fs::write(root.join("artifacts").join("secret.txt"), "excluded\n").unwrap();
    let absolute_root = root.to_string_lossy().replace('\\', "/");

    let files = parse_json_output(
        &root,
        &[
            "--json",
            "--config",
            ".contextmink.toml",
            "files",
            &absolute_root,
            "--limit",
            "50",
        ],
    );

    assert!(
        files["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|path| !path.as_str().unwrap().contains("artifacts/"))
    );
}

#[test]
fn sqlite_timeout_interrupts_runaway_queries() {
    let root = fixture_root("sqlite-timeout");
    let db_path = root.join("tiny.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch("CREATE TABLE t(id INTEGER PRIMARY KEY);")
        .unwrap();
    drop(conn);

    // A nonterminating recursive CTE must be interrupted, not hang.
    let output = run_contextmink_raw(
        &root,
        &[
            "sqlite",
            "tiny.sqlite",
            "--timeout-secs",
            "1",
            "--sql",
            "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c) SELECT count(*) FROM c",
        ],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("interrupted after --timeout-secs 1"),
        "stderr: {stderr}"
    );

    // A normal query under the same budget still succeeds.
    let ok = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite",
            "tiny.sqlite",
            "--timeout-secs",
            "1",
            "--sql",
            "SELECT 1 AS one",
        ],
    );
    assert_eq!(ok["rows"][0]["fields"]["one"], "1");
}

#[test]
fn grep_quiet_suppresses_match_content_but_keeps_receipt_fields() {
    let root = fixture_root("grep-quiet");

    // JSON: no matching-files payload. Quiet reports emitted payload truthfully
    // while retaining scan totals and scope classification.
    let loud = parse_json_output(&root, &["--json", "grep", "alpha", "sample.txt"]);
    let quiet = parse_json_output(&root, &["--json", "grep", "alpha", "sample.txt", "--quiet"]);
    assert_envelope(&quiet, "grep", "matching_files");
    assert!(quiet.get("matching_files").is_none());
    assert_eq!(quiet["quiet"], true);
    assert!(loud["matching_files"].is_array());
    assert_eq!(result(&quiet)["shown"], 0);
    assert_eq!(quiet["sample_lines_shown"], 0);
    assert_eq!(quiet["output_truncated"], false);
    assert_eq!(quiet["complete"], quiet["scope_complete"]);
    assert!(
        quiet["caps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|cap| cap["boundary"] != "output")
    );
    for field in [
        "matching_lines_total",
        "matching_lines_total_is_lower_bound",
        "candidate_files_total",
        "candidate_files_selected",
        "content_files_admitted",
        "content_files_scanned",
        "skipped_large_or_binary",
        "no_match_scope",
    ] {
        assert_eq!(quiet[field], loud[field], "field: {field}");
    }

    // Text: no file_counts/sample_lines blocks, receipt line intact.
    let human = run_contextmink(&root, &["grep", "alpha", "sample.txt", "--quiet"]);
    assert!(!human.contains("file_counts:"), "output: {human}");
    assert!(!human.contains("sample_lines:"), "output: {human}");
    let receipt = human
        .lines()
        .last()
        .unwrap()
        .strip_prefix("CONTEXTMINK_RECEIPT ")
        .unwrap();
    let receipt: Value = serde_json::from_str(receipt).unwrap();
    assert_envelope(&receipt, "grep", "matching_files");
    assert_eq!(receipt["quiet"], true);
    assert_eq!(
        receipt["matching_lines_total"],
        loud["matching_lines_total"]
    );
    assert_eq!(result(&receipt)["shown"], 0);
    assert_eq!(receipt["sample_lines_shown"], 0);
    assert!(
        receipt["caps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|cap| cap["boundary"] != "output")
    );
}

#[test]
fn grep_quiet_no_match_still_reports_scan_scope() {
    let root = fixture_root("grep-quiet-nomatch");
    fs::write(root.join("extra_a.txt"), "alpha\n").unwrap();
    fs::write(root.join("extra_b.txt"), "alpha\n").unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "grep",
            "not-present",
            ".",
            "--quiet",
            "--max-content-files",
            "1",
        ],
    );
    assert_envelope(&json, "grep", "matching_files");
    assert_eq!(json["quiet"], true);
    assert!(has_cap(&json, "scope", "content_files"));
    assert_eq!(json["no_match_scope"], "scanned_subset");
    assert_eq!(result(&json)["total_is_lower_bound"], true);

    let human = run_contextmink(&root, &["grep", "not-present", "sample.txt", "--quiet"]);
    assert!(human.contains("no_matches"), "output: {human}");
    let receipt = human
        .lines()
        .last()
        .unwrap()
        .strip_prefix("CONTEXTMINK_RECEIPT ")
        .unwrap();
    let receipt: Value = serde_json::from_str(receipt).unwrap();
    assert_eq!(receipt["quiet"], true);
    assert_eq!(receipt["no_match_scope"], "complete_scope");
}

#[test]
fn grep_terms_quiet_composes_with_any_mode() {
    let root = fixture_root("grep-terms-quiet");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--term",
            "alpha",
            "--term",
            "nowhere",
            "--any",
            "sample.txt",
            "--quiet",
        ],
    );
    assert_envelope(&json, "grep-terms", "matching_files");
    assert_eq!(json["quiet"], true);
    assert!(json.get("matching_files").is_none());
    assert_eq!(result(&json)["total"], 1);
    assert_eq!(result(&json)["shown"], 0);
    assert_eq!(json["matching_lines_total"], 2);
    assert_eq!(json["sample_lines_shown"], 0);

    let human = run_contextmink(
        &root,
        &[
            "grep-terms",
            "--term",
            "alpha",
            "--term",
            "beta",
            "sample.txt",
            "--quiet",
        ],
    );
    assert!(!human.contains("file_counts:"), "output: {human}");
    assert!(!human.contains("sample_lines:"), "output: {human}");
    let receipt = human
        .lines()
        .last()
        .unwrap()
        .strip_prefix("CONTEXTMINK_RECEIPT ")
        .unwrap();
    let receipt: Value = serde_json::from_str(receipt).unwrap();
    assert_envelope(&receipt, "grep-terms", "matching_files");
    assert_eq!(receipt["quiet"], true);
}

#[test]
fn sqlite_hexint_joins_hex_address_strings_against_integer_columns() {
    let root = fixture_root("sqlite-hexint");
    let db_path = root.join("sample.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE targets(addr INTEGER PRIMARY KEY, name TEXT NOT NULL);
         INSERT INTO targets(addr, name) VALUES (5206432, 'alpha'), (8324272, 'beta');",
    )
    .unwrap();
    drop(conn);
    fs::write(
        root.join("worklist.jsonl"),
        "{\"addr\":\"0x4f71a0\"}\n{\"addr\":\"0x7F04B0\"}\n{\"addr\":\"8324272\"}\n",
    )
    .unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite",
            "sample.sqlite",
            "--sql",
            "SELECT t.name FROM json_each(:w) q JOIN targets t ON t.addr = hexint(q.value ->> '$.addr') ORDER BY t.addr",
            "--jsonl-param",
            "w=worklist.jsonl",
        ],
    );
    assert_envelope(&json, "sqlite", "rows");
    assert_eq!(result(&json)["shown"], 3);
    assert_eq!(json["rows"][0]["fields"]["name"], "\"alpha\"");
    assert_eq!(json["rows"][2]["fields"]["name"], "\"beta\"");
    assert_eq!(json["params"][0]["values"], 3);
}

#[test]
fn sqlite_hexint_fails_fast_on_unparseable_text() {
    let root = fixture_root("sqlite-hexint-invalid");
    let db_path = root.join("sample.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch("CREATE TABLE t(x INTEGER);").unwrap();
    drop(conn);

    let output = run_contextmink_raw(
        &root,
        &[
            "sqlite",
            "sample.sqlite",
            "--sql",
            "SELECT hexint('bad_004f71a0')",
        ],
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("hexint: cannot parse")
    );
}

#[test]
fn sqlite_jsonl_param_rejects_single_top_level_array() {
    let root = fixture_root("sqlite-jsonl-array-guard");
    let db_path = root.join("sample.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch("CREATE TABLE t(x INTEGER);").unwrap();
    drop(conn);
    fs::write(root.join("rows.json"), "[{\"a\":1},{\"a\":2}]").unwrap();

    let output = run_contextmink_raw(
        &root,
        &[
            "sqlite",
            "sample.sqlite",
            "--sql",
            "SELECT count(*) FROM json_each(:w)",
            "--jsonl-param",
            "w=rows.json",
        ],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("single top-level JSON array"));
    assert!(stderr.contains("--json-param"));
}

#[test]
fn sqlite_json_param_teaches_jsonl_misuse() {
    let root = fixture_root("sqlite-json-param-misuse");
    let db_path = root.join("sample.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch("CREATE TABLE t(x INTEGER);").unwrap();
    drop(conn);
    fs::write(root.join("rows.jsonl"), "{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n").unwrap();

    let output = run_contextmink_raw(
        &root,
        &[
            "sqlite",
            "sample.sqlite",
            "--sql",
            "SELECT count(*) FROM json_each(:w)",
            "--json-param",
            "w=rows.jsonl",
        ],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("parses as 3 JSONL values"));
    assert!(stderr.contains("--jsonl-param"));
}

#[test]
fn files_ext_accepts_comma_separated_lists() {
    let root = fixture_root("files-ext-csv");
    fs::write(root.join("a.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("b.toml"), "[package]\n").unwrap();
    fs::write(root.join("c.md"), "# doc\n").unwrap();

    let json = parse_json_output(&root, &["--json", "files", ".", "--ext", "rs,md"]);
    assert_envelope(&json, "files", "files");
    assert_eq!(result(&json)["total"], 2);
    assert_eq!(json["output_truncated"], false);
}

#[test]
fn files_quiet_suppresses_list_but_keeps_receipt() {
    let root = fixture_root("files-quiet");
    fs::write(root.join("a.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("b.rs"), "fn other() {}\n").unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json", "files", ".", "--ext", "rs", "--quiet", "--limit", "1",
        ],
    );
    assert_envelope(&json, "files", "files");
    assert_eq!(json["quiet"], true);
    assert_eq!(result(&json)["total"], 2);
    assert_eq!(result(&json)["shown"], 0);
    assert_eq!(json["output_truncated"], false);
    assert_eq!(json["scope_complete"], true);
    assert_eq!(json["complete"], true);
    assert_eq!(json["candidate_files_selected"], 1);
    assert!(json["caps"].as_array().unwrap().is_empty());
    assert!(json.get("files").is_none());

    let text = run_contextmink(&root, &["files", ".", "--ext", "rs", "--quiet"]);
    assert!(!text.contains("a.rs"));
    assert!(text.contains("CONTEXTMINK_RECEIPT"));
}

#[test]
fn json_select_array_accepts_bare_top_level_key() {
    let root = fixture_root("json-select-bare-array");
    fs::write(
        root.join("doc.json"),
        "{\"entries\":[{\"id\":\"a\"},{\"id\":\"b\"}]}",
    )
    .unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "doc.json",
            "--array",
            "entries",
            "--fields",
            "id",
        ],
    );
    assert_envelope(&json, "json-select", "rows");
    assert_eq!(result(&json)["shown"], 2);
    assert_eq!(json["rows"][0]["fields"]["id"], "\"a\"");
}

#[test]
fn json_select_keys_reports_row_shape() {
    let root = fixture_root("json-select-keys");
    fs::write(
        root.join("rows.jsonl"),
        concat!(
            "{\"id\":\"a\",\"expect\":{\"n\":1},\"size\":4}\n",
            "{\"id\":\"b\",\"expect\":{\"n\":2}}\n",
            "{\"id\":\"c\",\"size\":null}\n",
        ),
    )
    .unwrap();

    let json = parse_json_output(&root, &["--json", "json-select", "rows.jsonl", "--keys"]);
    assert_envelope(&json, "json-select", "keys");
    assert_eq!(json["keys_mode"], true);
    assert_eq!(json["rows_scanned"], 3);
    assert_eq!(result(&json)["total"], 3);
    let keys = json["keys"].as_array().unwrap();
    assert_eq!(keys[0]["key"], "expect");
    assert_eq!(keys[0]["present"], 2);
    assert_eq!(keys[0]["types"][0], "object");
    assert_eq!(keys[2]["key"], "size");
    assert_eq!(keys[2]["present"], 2);
    assert_eq!(keys[2]["non_null"], 1);

    let output = run_contextmink_raw(
        &root,
        &["json-select", "rows.jsonl", "--keys", "--fields", "id"],
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("cannot be combined")
    );

    let filtered = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "rows.jsonl",
            "--keys",
            "--where",
            "missing=open",
        ],
    );
    assert_eq!(filtered["rows_scanned"], 3);
    assert_eq!(filtered["rows_matching"], 0);
    assert_eq!(filtered["all_null_fields"][0], "missing");

    let human = run_contextmink(
        &root,
        &[
            "json-select",
            "rows.jsonl",
            "--keys",
            "--where",
            "missing=open",
        ],
    );
    assert!(human.contains("warning: field(s) missing"));
}

#[test]
fn sqlite_schema_elides_table_detail_atomically() {
    let root = fixture_root("sqlite-schema-atomic");
    let db_path = root.join("sample.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE wide(a INTEGER, b INTEGER, c INTEGER, d INTEGER, e INTEGER);
         CREATE INDEX idx_wide_a ON wide(a);
         CREATE TABLE narrow(x INTEGER);",
    )
    .unwrap();
    drop(conn);

    // Budget of 3 columns: `wide` (5 columns) must elide whole, not show a
    // 3-column prefix with its index attached; `narrow` still fits.
    let json = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite-schema",
            "sample.sqlite",
            "--max-columns",
            "3",
        ],
    );
    assert_envelope(&json, "sqlite-schema", "tables");
    assert_eq!(json["tables_detail_elided"], 1);
    let tables = json["tables"].as_array().unwrap();
    let wide = tables.iter().find(|t| t["name"] == "wide").unwrap();
    assert_eq!(wide["detail_elided"], true);
    assert_eq!(wide["columns"].as_array().unwrap().len(), 0);
    assert_eq!(wide["indexes"].as_array().unwrap().len(), 0);
    assert_eq!(wide["columns_total"], 5);
    let narrow = tables.iter().find(|t| t["name"] == "narrow").unwrap();
    assert_eq!(narrow["detail_elided"], false);
    assert_eq!(narrow["columns"].as_array().unwrap().len(), 1);
    assert_eq!(json["output_truncated"], true);
}

#[test]
fn grep_receipts_split_skipped_large_and_binary() {
    let root = fixture_root("grep-skip-split");
    fs::write(root.join("binary.bin"), [0u8, 159, 146, 150, 0, 1]).unwrap();
    fs::write(root.join("plain.txt"), "needle\n").unwrap();

    let json = parse_json_output(&root, &["--json", "grep", "needle", ".", "--quiet"]);
    assert_eq!(json["skipped_binary"], 1);
    assert_eq!(json["skipped_large"], 0);
    assert_eq!(json["skipped_large_or_binary"], 1);
    assert_eq!(result(&json)["total_is_lower_bound"], false);
}

/// Re-encode UTF-8 text as if its bytes were read back through WHATWG
/// windows-1252 — the exact PowerShell/CP1252 double encode. Generated here
/// so no literal mojibake sits in the test source (which the mojibake gate
/// would otherwise flag).
fn cp1252_double_encode(text: &str) -> String {
    const SPECIALS: &[(u8, char)] = &[
        (0x80, '\u{20AC}'),
        (0x82, '\u{201A}'),
        (0x83, '\u{0192}'),
        (0x84, '\u{201E}'),
        (0x85, '\u{2026}'),
        (0x86, '\u{2020}'),
        (0x87, '\u{2021}'),
        (0x88, '\u{02C6}'),
        (0x89, '\u{2030}'),
        (0x8A, '\u{0160}'),
        (0x8B, '\u{2039}'),
        (0x8C, '\u{0152}'),
        (0x8E, '\u{017D}'),
        (0x91, '\u{2018}'),
        (0x92, '\u{2019}'),
        (0x93, '\u{201C}'),
        (0x94, '\u{201D}'),
        (0x95, '\u{2022}'),
        (0x96, '\u{2013}'),
        (0x97, '\u{2014}'),
        (0x98, '\u{02DC}'),
        (0x99, '\u{2122}'),
        (0x9A, '\u{0161}'),
        (0x9B, '\u{203A}'),
        (0x9C, '\u{0153}'),
        (0x9E, '\u{017E}'),
        (0x9F, '\u{0178}'),
    ];
    text.bytes()
        .map(|b| {
            if let Some((_, ch)) = SPECIALS.iter().find(|(byte, _)| *byte == b) {
                *ch
            } else {
                b as char
            }
        })
        .collect()
}

#[test]
fn slice_and_outline_flag_encoding_suspects_only_when_found() {
    let root = fixture_root("encoding-suspects");
    // UTF-8 text that was already double-encoded once through CP1252: the
    // em-dash is a 3-byte run and the é is a 2-byte Latin-1 run (2 total).
    let dash = cp1252_double_encode("—");
    let eacute = cp1252_double_encode("é");
    fs::write(
        root.join("mojibake.md"),
        format!("# Title\n\nDash {dash} and eacute {eacute} survive a CP1252 boundary.\n"),
    )
    .unwrap();
    fs::write(root.join("clean.md"), "# Title\n\nDash — and eacute é.\n").unwrap();

    let json = parse_json_output(&root, &["--json", "slice", "mojibake.md", "--range", "1:5"]);
    let suspects = &json["encoding_suspects"];
    assert_eq!(suspects["double_encoded"], 2);
    assert_eq!(suspects["replacement_chars"], 0);
    let sample = suspects["sample"].as_str().unwrap();
    assert!(sample.contains("line 3"), "{sample}");
    assert!(sample.contains('—'), "repair shown: {sample}");

    // Clean files carry no field at all — the common case costs nothing.
    let json = parse_json_output(&root, &["--json", "slice", "clean.md", "--range", "1:5"]);
    assert!(json.get("encoding_suspects").is_none());
    let json = parse_json_output(&root, &["--json", "outline", "clean.md"]);
    assert!(json.get("encoding_suspects").is_none());

    let json = parse_json_output(&root, &["--json", "outline", "mojibake.md"]);
    assert_eq!(json["encoding_suspects"]["double_encoded"], 2);

    // Human mode prints a single note line plus the receipt.
    let text = run_contextmink(&root, &["slice", "mojibake.md", "--range", "1:5"]);
    assert!(
        text.contains("encoding suspects: 2 double-encoded"),
        "{text}"
    );
}
