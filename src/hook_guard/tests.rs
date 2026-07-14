use std::path::Path;

use super::{
    HookVerdict, evaluate_hook_payload, evaluate_hook_payload_for_root,
    evaluate_hook_payload_with_shell,
};
use crate::config::DestructiveGuardConfig;
use crate::destructive_guard::ShellDialect;

const FIELD: &str = "tool_input.command";

fn protected_config() -> DestructiveGuardConfig {
    DestructiveGuardConfig {
        recursive_delete_fragments: vec![".state-cache".to_owned(), "generated-reports".to_owned()],
        delete_fragments: vec!["index.sqlite".to_owned(), "analysis.gpr".to_owned()],
    }
}

fn payload(command: &str) -> String {
    serde_json::json!({
        "tool_name": "Bash",
        "tool_input": { "command": command }
    })
    .to_string()
}

fn payload_with_cwd(command: &str, cwd: &str) -> String {
    serde_json::json!({
        "cwd": cwd,
        "tool_name": "Bash",
        "tool_input": { "command": command }
    })
    .to_string()
}

fn verdict(command: &str) -> HookVerdict {
    evaluate_hook_payload(&payload(command), FIELD, &protected_config(), false)
}

fn assert_denied(command: &str, expect_in_message: &str) {
    match verdict(command) {
        HookVerdict::Deny { message } => assert!(
            message.contains(expect_in_message),
            "deny message for {command:?} missing {expect_in_message:?}: {message}"
        ),
        other => panic!("expected deny for {command:?}, got {other:?}"),
    }
}

fn assert_allowed(command: &str) {
    match verdict(command) {
        HookVerdict::Allow => {}
        other => panic!("expected allow for {command:?}, got {other:?}"),
    }
}

#[test]
fn git_clean_spellings_are_denied() {
    assert_denied("git clean -fdX", "git clean");
    assert_denied("cd minkwrath && git clean -fd", "git clean");
    assert_denied("git -C ghidramink clean -f", "git clean");
    assert_denied("git.exe clean -n", "git clean");
    assert_denied(
        "bash -lc 'cd /f/AI/wow_modernclient && git clean -fdX'",
        "git clean",
    );
}

#[test]
fn recursive_deletion_of_protected_fragments_is_denied() {
    assert_denied("rm -r .state-cache", "protected path fragment");
    assert_denied(
        "Remove-Item -Recurse -Force generated-reports/exports",
        "protected path fragment",
    );
}

#[test]
fn direct_deletion_of_protected_files_is_denied() {
    assert_denied("rm -f .state-cache/index.sqlite", "protected path fragment");
    assert_denied("del analysis.gpr", "protected path fragment");
}

#[test]
fn benign_commands_are_allowed() {
    assert_allowed("git status --short");
    assert_allowed("git status --short && echo '(clean = history intact)'");
    assert_allowed("git status && make clean");
    assert_allowed("git commit -m 'fix git clean parser'");
    assert_allowed("python -c 'payload = \"git clean -fdX\"'");
    assert_allowed("echo .state-cache && rm -rf scratch-only");
    assert_allowed("cargo test --workspace");
    assert_allowed("rm -f scratch_probe.rs");
    // Reads and backups of protected artifacts must never be blocked.
    assert_allowed("contextmink sqlite .state-cache/index.sqlite --sql-file q.sql");
    assert_allowed("cp .state-cache/index.sqlite /e/backups/index.sqlite");
}

#[test]
fn powershell_backtick_escapes_are_data_not_command_substitutions() {
    let command = r#"git commit -m "fix `git clean` parser""#;
    let powershell = evaluate_hook_payload_with_shell(
        &payload(command),
        FIELD,
        &protected_config(),
        false,
        ShellDialect::Powershell,
    );
    assert_eq!(powershell, HookVerdict::Allow);

    let posix = evaluate_hook_payload_with_shell(
        &payload(command),
        FIELD,
        &protected_config(),
        false,
        ShellDialect::Posix,
    );
    assert!(matches!(posix, HookVerdict::Deny { .. }));

    let destructive = evaluate_hook_payload_with_shell(
        &payload("& git clean -fdX"),
        FIELD,
        &protected_config(),
        false,
        ShellDialect::Powershell,
    );
    assert!(matches!(destructive, HookVerdict::Deny { .. }));
}

#[test]
fn empty_command_is_allowed() {
    assert_allowed("");
}

#[test]
fn override_downgrades_deny_to_warning() {
    let verdict =
        evaluate_hook_payload(&payload("git clean -fdX"), FIELD, &protected_config(), true);
    match verdict {
        HookVerdict::AllowWithOverride { message } => {
            assert!(message.contains("git clean"), "message: {message}");
        }
        other => panic!("expected override allow, got {other:?}"),
    }
}

#[test]
fn unparseable_payloads_allow_with_note() {
    let config = protected_config();
    for (raw, expect) in [
        ("", "empty hook payload"),
        ("   \n", "empty hook payload"),
        ("{not json", "not valid JSON"),
        (
            r#"{"tool_name": "Bash"}"#,
            "has no `tool_input.command` field",
        ),
        (r#"{"tool_input": {"command": 42}}"#, "is not a string"),
    ] {
        match evaluate_hook_payload(raw, FIELD, &config, false) {
            HookVerdict::AllowUnparsed { note } => assert!(
                note.contains(expect),
                "note for {raw:?} missing {expect:?}: {note}"
            ),
            other => panic!("expected allow-unparsed for {raw:?}, got {other:?}"),
        }
    }
}

#[test]
fn custom_command_field_path_is_honored() {
    let raw = r#"{"cmd": "git clean -fd"}"#;
    match evaluate_hook_payload(raw, "cmd", &protected_config(), false) {
        HookVerdict::Deny { message } => assert!(message.contains("git clean")),
        other => panic!("expected deny via custom field, got {other:?}"),
    }
}

#[test]
fn expected_root_prevents_a_foreign_checkout_policy_from_applying() {
    let config = protected_config();
    let foreign = evaluate_hook_payload_for_root(
        &payload_with_cwd("git clean -fdX", "D:/work/other-repo"),
        FIELD,
        &config,
        false,
        Some(Path::new("D:/work/expected-repo")),
        ShellDialect::Posix,
    );
    match foreign {
        HookVerdict::AllowUnparsed { note } => {
            assert!(note.contains("outside policy root"), "note: {note}");
        }
        other => panic!("expected foreign policy to allow with a note, got {other:?}"),
    }

    let local = evaluate_hook_payload_for_root(
        &payload_with_cwd("git clean -fdX", "D:/work/expected-repo/subdir"),
        FIELD,
        &config,
        false,
        Some(Path::new("D:/work/expected-repo")),
        ShellDialect::Posix,
    );
    assert!(matches!(local, HookVerdict::Deny { .. }));
}
