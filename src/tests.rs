use super::*;
use clap::Parser;

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
        Command::Grep { args, pattern, .. } => {
            assert_eq!(pattern.as_deref(), Some("implementation-query"));
            assert_eq!(args, vec!["ghidramink/tools/ghidramink-core/src"]);
        }
        _ => panic!("expected grep command"),
    }
}

#[test]
fn cli_v2_rejects_removed_aliases_and_duplicate_input_forms() {
    let rejected = vec![
        vec!["contextmink", "--fail-on-truncated", "files"],
        vec!["contextmink", "--fail-on-truncate", "files"],
        vec!["contextmink", "--strict-complete", "files"],
        vec!["contextmink", "files", "--name-contains", "src"],
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
        vec![
            "contextmink",
            "grep",
            "--max-sample-lines",
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
        vec!["contextmink", "grep", "needle", "--path", "src"],
        vec!["contextmink", "grep-terms", "--term", "x", "--path", "src"],
        vec!["contextmink", "run", "--", "echo", "ok"],
    ];

    for argv in rejected {
        assert!(
            Cli::try_parse_from(&argv).is_err(),
            "removed CLI form unexpectedly parsed: {argv:?}"
        );
    }
}

#[test]
fn cli_accepts_current_forms() {
    Cli::try_parse_from([
        "contextmink",
        "grep-terms",
        "--term",
        "alpha",
        "--any",
        "--ext",
        "rs",
        "--limit",
        "2",
        "--max-matches",
        "3",
        "--max-count-files",
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
        "--limit",
        "2",
    ])
    .expect("parse current json-select form");
    Cli::try_parse_from([
        "contextmink",
        "sqlite",
        "sample.sqlite",
        "--sql",
        "SELECT 1",
        "--limit",
        "1",
    ])
    .expect("parse positional sqlite form");
    Cli::try_parse_from(["contextmink", "sqlite-schema", "sample.sqlite"])
        .expect("parse positional sqlite-schema form");
}
