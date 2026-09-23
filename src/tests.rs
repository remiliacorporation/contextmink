use super::*;
use clap::{CommandFactory, Parser};

#[test]
fn cli_guidance_subcommand_names_match_clap() {
    let command = Cli::command();
    let names = command
        .get_subcommands()
        .map(|command| command.get_name())
        .collect::<Vec<_>>();
    assert_eq!(names, cli::SUBCOMMAND_NAMES);
}

#[test]
fn paths_default_to_workspace_root() {
    assert_eq!(paths_or_current_dir(&[]), vec![PathBuf::from(".")]);
    assert_eq!(
        paths_or_current_dir(&[PathBuf::from("src"), PathBuf::from("tests")]),
        vec![PathBuf::from("src"), PathBuf::from("tests")]
    );
}

#[test]
fn grep_accepts_named_pattern_and_positional_paths() {
    let cli = Cli::try_parse_from([
        "contextmink",
        "grep",
        "--pattern",
        "implementation-query",
        "ghidramink/tools/ghidramink-core/src",
    ])
    .expect("parse grep --pattern");

    match cli.command {
        Command::Grep { paths, pattern, .. } => {
            assert_eq!(pattern.as_deref(), Some("implementation-query"));
            assert_eq!(
                paths,
                vec![PathBuf::from("ghidramink/tools/ghidramink-core/src")]
            );
        }
        _ => panic!("expected grep command"),
    }
}

#[test]
fn cli_rejects_noncanonical_forms_and_duplicate_inputs() {
    let noncanonical = vec![
        vec!["contextmink", "--fail-on-truncated", "files"],
        vec!["contextmink", "--fail-on-truncate", "files"],
        vec!["contextmink", "--strict-complete", "files"],
        vec!["contextmink", "--require-complete-scan", "files"],
        vec!["contextmink", "files", "--name-contains", "src"],
        vec!["contextmink", "files", "--term", "src"],
        vec!["contextmink", "files", "--extension", "rs"],
        vec!["contextmink", "files", "--max", "1"],
        vec!["contextmink", "dirs", "--max", "1"],
        vec!["contextmink", "grep", "--extension", "rs", "needle", "."],
        vec![
            "contextmink",
            "grep",
            "--max-matched-files",
            "1",
            "needle",
            ".",
        ],
        vec!["contextmink", "grep", "--max-files", "1", "needle", "."],
        vec!["contextmink", "files", "--max-scan-files", "1"],
        vec!["contextmink", "dirs", "--max-scan-files", "1"],
        vec!["contextmink", "grep", "--max-matches", "1", "needle", "."],
        vec![
            "contextmink",
            "grep",
            "--max-count-files",
            "1",
            "needle",
            ".",
        ],
        vec![
            "contextmink",
            "grep",
            "--max-scan-files",
            "1",
            "needle",
            ".",
        ],
        vec!["contextmink", "grep", "--max-lines", "1", "needle", "."],
        vec![
            "contextmink",
            "grep-terms",
            "--mode",
            "any",
            "--term",
            "x",
            ".",
        ],
        vec!["contextmink", "grep-terms", "--or", "--term", "x", "."],
        vec!["contextmink", "grep-terms", "--all", "--term", "x", "."],
        vec!["contextmink", "grep-terms", "--and", "--term", "x", "."],
        vec![
            "contextmink",
            "grep-terms",
            "--extension",
            "rs",
            "--term",
            "x",
            ".",
        ],
        vec!["contextmink", "outline", "--path", "sample.rs"],
        vec!["contextmink", "outline", "sample.rs", "--max-items", "1"],
        vec!["contextmink", "slice", "--path", "sample.txt"],
        vec!["contextmink", "json-find", "--path", "sample.json"],
        vec!["contextmink", "json-find", "sample.json", "--max", "1"],
        vec!["contextmink", "json-select", "--path", "sample.json"],
        vec![
            "contextmink",
            "json-select",
            "sample.json",
            "--field",
            "name",
        ],
        vec!["contextmink", "json-select", "sample.json", "--max", "1"],
        vec![
            "contextmink",
            "sqlite",
            "--db",
            "sample.sqlite",
            "--sql",
            "SELECT 1",
        ],
        vec![
            "contextmink",
            "sqlite",
            "--path",
            "sample.sqlite",
            "--sql",
            "SELECT 1",
        ],
        vec![
            "contextmink",
            "sqlite",
            "sample.sqlite",
            "--sql",
            "SELECT 1",
            "--max-rows",
            "1",
        ],
        vec!["contextmink", "sqlite-schema", "--db", "sample.sqlite"],
        vec!["contextmink", "sqlite-schema", "--path", "sample.sqlite"],
        vec!["contextmink", "files", "--path", "src"],
        vec!["contextmink", "dirs", "--path", "src"],
        vec!["contextmink", "grep", "needle", "."],
        vec![
            "contextmink",
            "grep",
            "--pattern",
            "needle",
            "--path",
            "src",
        ],
        vec!["contextmink", "grep-terms", "--term", "x", "--path", "src"],
        vec!["contextmink", "slice", "sample.txt", "--start-line", "2"],
        vec!["contextmink", "slice", "sample.txt", "--end-line", "3"],
        vec!["contextmink", "slice", "sample.txt", "--end", "3"],
        vec![
            "contextmink",
            "slice",
            "sample.txt",
            "--range",
            "1:2",
            "--tail",
            "1",
        ],
        vec![
            "contextmink",
            "sqlite",
            "sample.sqlite",
            "--sql",
            "SELECT 1",
            "--max-scan-rows",
            "1",
        ],
        vec!["contextmink", "run", "--", "echo", "ok"],
    ];

    for argv in noncanonical {
        assert!(
            Cli::try_parse_from(&argv).is_err(),
            "noncanonical CLI form unexpectedly parsed: {argv:?}"
        );
    }
}

#[test]
fn cli_accepts_current_forms() {
    Cli::try_parse_from(["contextmink", "grep", "--pattern", "needle", "src", "tests"])
        .expect("parse canonical grep form");
    Cli::try_parse_from([
        "contextmink",
        "grep-terms",
        "--term",
        "alpha",
        "--any",
        "--ext",
        "rs",
        "--show-files",
        "2",
        "--show-lines",
        "3",
        "--show-lines-per-file",
        "1",
        "--max-matching-files",
        "4",
        ".",
    ])
    .expect("parse current grep-terms form");
    Cli::try_parse_from([
        "contextmink",
        "json-select",
        "sample.json",
        "--fields",
        "name,address",
        "--show-rows",
        "2",
    ])
    .expect("parse current json-select form");
    Cli::try_parse_from([
        "contextmink",
        "sqlite",
        "sample.sqlite",
        "--sql",
        "SELECT 1",
        "--show-rows",
        "1",
    ])
    .expect("parse positional sqlite form");
    Cli::try_parse_from([
        "contextmink",
        "sqlite-schema",
        "sample.sqlite",
        "--with-shadow-tables",
        "--with-system-tables",
        "--show-tables",
        "2",
    ])
    .expect("parse positional sqlite-schema form");
    Cli::try_parse_from([
        "contextmink",
        "slice",
        "a.txt",
        "--range",
        "2:4",
        "--line-ceiling",
        "9",
    ])
    .expect("parse slice range form");
    Cli::try_parse_from(["contextmink", "slice", "a.txt", "--tail", "3"])
        .expect("parse slice tail");
    Cli::try_parse_from([
        "contextmink",
        "json-find",
        "a.json",
        "--pointer-contains",
        "/a",
        "--pointer-regex",
        "b$",
        "--show-matches",
        "3",
    ])
    .expect("parse json-find pointer form");
    Cli::try_parse_from([
        "contextmink",
        "capture",
        "--show-lines",
        "3",
        "--show-bytes-per-stream",
        "9",
        "--show-line-chars",
        "9",
        "--",
        "echo",
    ])
    .expect("parse capture display form");
}

#[test]
fn removed_flag_spellings_are_refused_with_their_replacement() {
    let cases: &[(&[&str], &str)] = &[
        (&["files", "--limit", "1"], "--show-files"),
        (&["grep", "--pattern", "x", "--limit", "1"], "--show-files"),
        (
            &["grep-terms", "--term", "x", "--lines-per-file", "1"],
            "--show-lines-per-file",
        ),
        (
            &["grep", "--pattern", "x", "--max-sample-lines", "1"],
            "--show-lines",
        ),
        (&["dirs", "--limit", "1"], "--show-dirs"),
        (&["outline", "a.rs", "--limit", "1"], "--show-items"),
        (&["outline", "a.rs", "--max-items", "1"], "--show-items"),
        (&["json-find", "a.json", "--limit", "1"], "--show-matches"),
        (
            &["json-find", "a.json", "--path-contains", "/a"],
            "--pointer-contains",
        ),
        (
            &["json-find", "a.json", "--path-regex", "a"],
            "--pointer-regex",
        ),
        (&["json-select", "a.json", "--limit", "1"], "--show-rows"),
        (
            &["json-select", "a.json", "--max-value-chars", "1"],
            "--show-value-chars",
        ),
        (&["sqlite", "a.db", "--limit", "1"], "--show-rows"),
        (
            &["sqlite-schema", "a.db", "--include-shadow"],
            "--with-shadow-tables",
        ),
        (
            &["sqlite-schema", "a.db", "--include-system"],
            "--with-system-tables",
        ),
        (
            &["sqlite-schema", "a.db", "--max-columns", "1"],
            "--show-columns",
        ),
        (&["slice", "a.txt", "--max-lines", "1"], "--line-ceiling"),
        (
            &["slice", "a.txt", "--max-line-chars", "1"],
            "--show-line-chars",
        ),
        (&["slice", "a.txt", "--start", "2"], "--range START:END"),
        (&["slice", "a.txt", "--lines", "2"], "--range START:END"),
    ];
    for (args, replacement) in cases {
        let argv = std::iter::once("contextmink")
            .chain(args.iter().copied())
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        assert!(Cli::try_parse_from(&argv).is_err(), "{args:?} still parses");
        let guidance = cli::noncanonical_form_guidance(&argv)
            .unwrap_or_else(|| panic!("no replacement guidance for {args:?}"));
        assert!(guidance.contains(replacement), "{args:?}: {guidance}");
    }
    // Capture's trailing argv absorbs unknown leading flags; the command
    // refuses them before spawn with the same replacement table.
    assert!(
        cli::renamed_flag_guidance("capture", "--max-lines")
            .is_some_and(|guidance| guidance.contains("--show-lines"))
    );
    assert!(
        cli::renamed_flag_guidance("capture", "--max-bytes=9")
            .is_some_and(|guidance| guidance.contains("--show-bytes-per-stream"))
    );
}
