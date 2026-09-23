use super::*;
use clap::{CommandFactory, Parser};

#[test]
fn subcommand_names_match_clap() {
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
fn cli_rejects_incomplete_and_conflicting_forms() {
    let refused = vec![
        vec!["contextmink", "grep", "needle", "."],
        vec![
            "contextmink",
            "slice",
            "sample.txt",
            "--range",
            "1:2",
            "--tail",
            "1",
        ],
        vec!["contextmink", "run", "--", "echo", "ok"],
    ];

    for argv in refused {
        assert!(
            Cli::try_parse_from(&argv).is_err(),
            "CLI form unexpectedly parsed: {argv:?}"
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
