use std::fs;
use std::path::PathBuf;

use super::process_boundary::{
    msys2_arg_conversion_exclusions, resolve_program, resolve_project_root, windows_bash_candidates,
};
use super::{
    DUMP_WARN_LINES, assemble_argv, decode_base64, reader_exceeds_line_limit, sed_window_span,
};

fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut token = String::new();
    for chunk in bytes.chunks(3) {
        let mut buffer = 0u32;
        for (index, byte) in chunk.iter().enumerate() {
            buffer |= u32::from(*byte) << (16 - 8 * index);
        }
        for position in 0..=chunk.len() {
            let shift = 18 - 6 * position;
            token.push(ALPHABET[((buffer >> shift) & 0x3f) as usize] as char);
        }
    }
    token
}

fn assemble_argv_b64(argv: &[&str]) -> Result<Vec<String>, String> {
    let token = encode_base64(argv.join("\0").as_bytes());
    assemble_argv("--argv-b64", vec![token], std::path::Path::new("."))
        .map(|(_, argv)| argv)
        .map_err(|error| error.message)
}

fn temp_tree(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("bridge-root-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root); // guardrail: allow-ignore-result cleanup is best-effort for reused test temp dirs
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn root_resolution_prefers_policy_root_over_nested_vendored_git() {
    // Workspace layout: <ws>/.contextmink.toml with a vendored contextmink
    // checkout (its own .git) at <ws>/tools/contextmink and the bridge binary
    // under its target/release. Relative paths must anchor to <ws>.
    let workspace = temp_tree("policy");
    fs::write(workspace.join(".contextmink.toml"), "profile = \"t\"\n").unwrap();
    let exe_dir = workspace.join("tools/contextmink/target/release");
    fs::create_dir_all(&exe_dir).unwrap();
    fs::create_dir_all(workspace.join("tools/contextmink/.git")).unwrap();
    assert_eq!(resolve_project_root(&exe_dir, &workspace), workspace);

    // Standalone clone: no policy file anywhere, nearest .git wins.
    let clone = temp_tree("standalone");
    fs::create_dir_all(clone.join(".git")).unwrap();
    let exe_dir = clone.join("target/release");
    fs::create_dir_all(&exe_dir).unwrap();
    assert_eq!(resolve_project_root(&exe_dir, &clone), clone);

    // A globally installed bridge must discover policy from the caller's
    // repository subtree before considering its own install location.
    let global = temp_tree("global");
    let exe_dir = global.join("bin");
    fs::create_dir_all(&exe_dir).unwrap();
    let project = temp_tree("global-project");
    fs::write(project.join(".contextmink.toml"), "profile = \"g\"\n").unwrap();
    let nested = project.join("crates/one");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(resolve_project_root(&exe_dir, &nested), project);
}

#[test]
fn base64_decodes_standard_urlsafe_and_padded_forms() {
    assert_eq!(decode_base64("aGVsbG8=").unwrap(), b"hello");
    assert_eq!(decode_base64("aGVsbG8").unwrap(), b"hello");
    assert_eq!(decode_base64("aGVs\nbG8=").unwrap(), b"hello");
    // URL-safe '-'/'_' map onto the standard '+'/'/' values.
    assert_eq!(
        decode_base64("-_-_").unwrap(),
        decode_base64("+/+/").unwrap()
    );
    assert_eq!(decode_base64("").unwrap(), b"");
    assert!(decode_base64("a!b").unwrap_err().contains("0x21"));

    let argv = "printf\0%s\0he said \"hi\"\0^// PART";
    let token = encode_base64(argv.as_bytes());
    assert_eq!(decode_base64(&token).unwrap(), argv.as_bytes());
}

#[test]
fn argv_b64_preserves_every_argument_including_trailing_empty() {
    // `$argv -join [char]0` never emits a trailing NUL, so a trailing empty
    // entry is a genuine argument and must survive the round-trip.
    assert_eq!(
        assemble_argv_b64(&["prog", "keep", ""]).unwrap(),
        vec!["prog".to_owned(), "keep".to_owned(), String::new()]
    );
    assert_eq!(
        assemble_argv_b64(&["prog", "", "mid-empty"]).unwrap(),
        vec!["prog".to_owned(), String::new(), "mid-empty".to_owned()]
    );

    // Degenerate payloads (empty, single NUL) still decode to no arguments.
    assert!(assemble_argv_b64(&[]).unwrap_err().contains("no arguments"));
    assert!(
        assemble_argv_b64(&["", ""])
            .unwrap_err()
            .contains("no arguments")
    );
}

#[test]
fn windows_bash_candidates_are_git_for_windows_only() {
    // Cygwin/MSYS2 bash have different path and locking semantics and must
    // never silently substitute for Git Bash (CONTEXTMINK_BASH is the
    // explicit override for exotic hosts).
    for candidate in windows_bash_candidates() {
        let lower = candidate.to_string_lossy().to_ascii_lowercase();
        assert!(lower.contains(r"git\bin\bash.exe"), "candidate: {lower}");
        assert!(!lower.contains("cygwin"), "candidate: {lower}");
        assert!(!lower.contains("msys"), "candidate: {lower}");
    }
}

#[test]
fn git_bash_boundaries_exclude_only_caller_slash_arguments() {
    let argv = vec![
        "/lua_5_1_1/_union_41".to_owned(),
        "@C:/requests/type-graph.json".to_owned(),
        "{\"type\":\"/lua_5_1_1/_union_41\"}".to_owned(),
        "plain".to_owned(),
    ];
    assert_eq!(
        msys2_arg_conversion_exclusions(&argv),
        "/lua_5_1_1/_union_41;@C:/requests/type-graph.json;{\"type\":\"/lua_5_1_1/_union_41\"}"
    );
}

#[test]
fn pathlike_programs_resolve_against_cwd_and_bare_names_keep_path_lookup() {
    let cwd = std::path::Path::new("/ws/sub");
    // POSIX exec semantics: a separator makes the name a path relative to
    // the child's working directory.
    assert_eq!(resolve_program("./gradlew", cwd), "/ws/sub/gradlew");
    assert_eq!(resolve_program("bin/tool", cwd), "/ws/sub/bin/tool");
    // Bare names go through PATH lookup untouched.
    assert_eq!(resolve_program("git", cwd), "git");
    // Absolute and rooted spellings are never re-anchored.
    let absolute = if cfg!(windows) {
        r"C:\ws\tool.exe"
    } else {
        "/ws/tool"
    };
    assert_eq!(resolve_program(absolute, cwd), absolute);
    assert_eq!(resolve_program("/rooted/tool", cwd), "/rooted/tool");
    #[cfg(windows)]
    assert_eq!(resolve_program(r".\gradlew", cwd), "/ws/sub/gradlew");
}

#[test]
fn dump_warning_line_probe_is_bounded_and_handles_trailing_newlines() {
    let exactly_at_limit = "line\n".repeat(DUMP_WARN_LINES);
    assert!(!reader_exceeds_line_limit(
        std::io::Cursor::new(exactly_at_limit),
        DUMP_WARN_LINES
    ));

    let unterminated_line_after_limit = format!("{}tail", "line\n".repeat(DUMP_WARN_LINES));
    assert!(reader_exceeds_line_limit(
        std::io::Cursor::new(unterminated_line_after_limit),
        DUMP_WARN_LINES
    ));

    let over_limit = "line\n".repeat(DUMP_WARN_LINES + 1);
    assert!(reader_exceeds_line_limit(
        std::io::Cursor::new(over_limit),
        DUMP_WARN_LINES
    ));
}

/// Direct mode classifies a shebang file before spawn and enters Git Bash
/// without depending on a failed native CreateProcess call.
#[cfg(windows)]
#[test]
fn direct_mode_runs_relative_extensionless_script_under_cwd() {
    let root = temp_tree("cwd-script");
    fs::write(root.join("probe"), "#!/bin/sh\nexit 42\n").unwrap();
    let code = super::run(vec![
        "--cwd".to_owned(),
        root.to_string_lossy().into_owned(),
        "--".to_owned(),
        "./probe".to_owned(),
    ])
    .unwrap();
    assert_eq!(code, 42);
}

#[cfg(windows)]
#[test]
fn direct_mode_refuses_non_native_text_without_shebang() {
    let root = temp_tree("cwd-non-script");
    fs::write(root.join("probe"), "exit 0\n").unwrap();
    let error = super::run(vec![
        "--cwd".to_owned(),
        root.to_string_lossy().into_owned(),
        "--".to_owned(),
        "./probe".to_owned(),
    ])
    .unwrap_err();
    assert_eq!(error.code, super::EXIT_SPAWN_FAILED);
    assert!(error.message.contains("use --script"), "{}", error.message);
}

/// The deny-list must refuse `git clean` before spawn. `--cwd` points at a
/// fresh temp dir (not a git repo), so even a guard regression could not
/// delete anything if the command actually ran.
#[test]
fn run_blocks_git_clean_argv_before_spawn() {
    let root = temp_tree("deny-direct");
    let error = super::run(vec![
        "--cwd".to_owned(),
        root.to_string_lossy().into_owned(),
        "--".to_owned(),
        "git".to_owned(),
        "clean".to_owned(),
        "-fdX".to_owned(),
        "-e".to_owned(),
        "keep.sqlite".to_owned(),
    ])
    .unwrap_err();
    assert_eq!(error.code, super::EXIT_USAGE);
    assert!(
        error.message.contains("destructive command blocked"),
        "message: {}",
        error.message
    );
    assert!(
        error.message.contains("git clean"),
        "message: {}",
        error.message
    );
}

/// The deny-list covers every command form, not just `--`: an argfile
/// carrying the same argv is refused identically.
#[test]
fn run_blocks_destructive_argv_from_argfile_form() {
    let root = temp_tree("deny-argfile");
    let argfile = root.join("args.txt");
    fs::write(
        &argfile,
        "bash\n-lc\ncd generated_output && git clean -fdX\n",
    )
    .unwrap();
    let error = super::run(vec![
        "--cwd".to_owned(),
        root.to_string_lossy().into_owned(),
        "--argfile".to_owned(),
        argfile.to_string_lossy().into_owned(),
    ])
    .unwrap_err();
    assert_eq!(error.code, super::EXIT_USAGE);
    assert!(
        error.message.contains("git clean"),
        "message: {}",
        error.message
    );
}

/// Script arguments are data owned by the script, not independent commands.
/// Treating words inside them as executable commands makes test probes and
/// commit-message helpers impossible to invoke safely.
#[test]
fn run_allows_command_words_as_script_arguments() {
    let root = temp_tree("deny-script");
    let script = root.join("probe.sh");
    fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    let exit_code = super::run(vec![
        "--cwd".to_owned(),
        root.to_string_lossy().into_owned(),
        "--script".to_owned(),
        script.to_string_lossy().into_owned(),
        "git".to_owned(),
        "clean".to_owned(),
        "-fdX".to_owned(),
    ])
    .unwrap();
    assert_eq!(exit_code, 0);
}

#[test]
fn run_keeps_protected_path_words_local_to_the_script_command() {
    let root = temp_tree("deny-config-script");
    fs::write(
        root.join(".contextmink.toml"),
        "profile = \"test\"\ndestructive_guard_recursive_delete_fragments = [\"protected_cache\"]\n",
    )
    .unwrap();
    let script = root.join("probe.sh");
    fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    let exit_code = super::run_with_root(
        vec![
            "--cwd".to_owned(),
            root.to_string_lossy().into_owned(),
            "--script".to_owned(),
            script.to_string_lossy().into_owned(),
            "rm".to_owned(),
            "-rf".to_owned(),
            "protected_cache".to_owned(),
        ],
        root,
    )
    .unwrap();
    assert_eq!(exit_code, 0);
}

#[test]
fn sed_window_spans_parse_print_ranges_only() {
    assert_eq!(sed_window_span("1,460p"), Some(460));
    assert_eq!(sed_window_span("-n930,1260p"), Some(331));
    assert_eq!(sed_window_span("5,5p"), Some(1));
    assert_eq!(sed_window_span("s/a/b/"), None);
    assert_eq!(sed_window_span("1,460d"), None);
    assert_eq!(sed_window_span("460p"), None);
}
