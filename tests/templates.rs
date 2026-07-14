#[test]
fn instruction_templates_are_policy_equivalent() {
    let codex = include_str!("../templates/AGENTS.contextmink.md");
    let claude = include_str!("../templates/CLAUDE.contextmink.md");

    assert_eq!(
        codex, claude,
        "Codex and Claude contextmink guidance must stay equivalent"
    );
}

#[test]
fn setup_points_to_templates_instead_of_duplicating_policy() {
    let setup = include_str!("../docs/setup.md");

    assert!(setup.contains("templates/AGENTS.contextmink.md"));
    assert!(setup.contains("templates/CLAUDE.contextmink.md"));
    assert!(
        !setup.contains("Do not route everything through `contextmink`."),
        "setup.md should point to templates instead of duplicating snippet prose"
    );
}

#[test]
fn public_guidance_uses_current_cli_forms() {
    let surfaces = [
        ("README.md", include_str!("../README.md")),
        ("SETUP.md", include_str!("../SETUP.md")),
        ("docs/setup.md", include_str!("../docs/setup.md")),
        (
            "templates/AGENTS.contextmink.md",
            include_str!("../templates/AGENTS.contextmink.md"),
        ),
        (
            "templates/CLAUDE.contextmink.md",
            include_str!("../templates/CLAUDE.contextmink.md"),
        ),
        (
            ".github/workflows/release-artifacts.yml",
            include_str!("../.github/workflows/release-artifacts.yml"),
        ),
    ];
    let retired_examples = [
        "files --path",
        "dirs --path",
        "grep contextmink --path",
        "sqlite --path",
        "sqlite-schema --path",
        "files --max ",
    ];

    for (name, contents) in surfaces {
        for retired in retired_examples {
            assert!(
                !contents.contains(retired),
                "{name} still documents retired CLI form {retired:?}"
            );
        }
    }
}

#[test]
fn project_template_requires_explicit_policy_adaptation() {
    let config = include_str!("../templates/.contextmink.toml");
    let guidance = include_str!("../templates/AGENTS.contextmink.md");

    assert!(config.contains("profile = \"replace-with-workspace-name\""));
    assert!(config.contains("Add only project-specific high-output paths"));
    assert!(guidance.contains("intended workspace root"));
    assert!(guidance.contains("& tools\\contextmink\\bin\\contextmink.exe"));
}

#[test]
fn release_workflow_verifies_extracted_project_integration() {
    let workflow = include_str!("../.github/workflows/release-artifacts.yml");

    for required in [
        "tar -xzf",
        "Expand-Archive",
        "integration-project",
        "scripts/contextmink --json guard-check",
        "scripts/contextmink --json hook-snippet",
        "\"decision\"[[:space:]]*:[[:space:]]*\"deny\"",
        "guardSmoke.decision",
        "--print-argv --argv-b64",
        "--target \"$GITHUB_SHA\"",
    ] {
        assert!(
            workflow.contains(required),
            "release workflow is missing integration proof {required:?}"
        );
    }
}

#[test]
fn launcher_template_matches_repo_launcher() {
    let repo_launcher = include_str!("../scripts/contextmink");
    let template_launcher = include_str!("../templates/scripts/contextmink");

    assert_eq!(
        repo_launcher, template_launcher,
        "the installed launcher template must match scripts/contextmink"
    );
}

#[test]
fn launcher_finds_cargo_outside_non_login_path() {
    let launcher = include_str!("../templates/scripts/contextmink");

    assert!(launcher.contains("find_cargo()"));
    assert!(launcher.contains("\"$home_dir/.cargo/bin/cargo\""));
    assert!(launcher.contains("\"$home_dir/.cargo/bin/cargo.exe\""));
    assert!(launcher.contains("bash -lc 'command -v cargo'"));
    assert!(launcher.contains("cargo_bin=\"$(find_cargo || true)\""));
    assert!(launcher.contains("\"$cargo_bin\" build --quiet --release"));
}

#[test]
fn launcher_declares_json_pointer_filter_exclusions() {
    let launcher = include_str!("../templates/scripts/contextmink");

    assert!(launcher.contains("--array | --fields | --where | --where-contains | --path-contains"));
    assert!(launcher.contains(
        "--array=/* | --fields=/* | --where=/* | --where-contains=/* | --path-contains=/*"
    ));
}

#[cfg(windows)]
#[test]
fn launcher_preserves_json_pointer_filter_values() {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};

    fn git_bash() -> PathBuf {
        [
            PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"),
            PathBuf::from(r"C:\Program Files\Git\usr\bin\bash.exe"),
        ]
        .into_iter()
        .find(|candidate| candidate.is_file())
        .expect("launcher tests require Git Bash")
    }

    fn run(launcher: &Path, root: &Path, args: &[&str]) -> Output {
        Command::new(git_bash())
            .arg(launcher)
            .args(args)
            .current_dir(root)
            .output()
            .expect("run contextmink launcher")
    }

    fn parse_success(output: Output) -> serde_json::Value {
        assert!(
            output.status.success(),
            "launcher failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("launcher JSON receipt")
    }

    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = base.join(format!(
        "contextmink-launcher-selectors-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root); // guardrail: allow-ignore-result cleanup is best-effort for reused test temp dirs
    let scripts = root.join("scripts");
    let bin_dir = root.join("tools/contextmink/bin");
    fs::create_dir_all(&scripts).unwrap();
    fs::create_dir_all(&bin_dir).unwrap();

    let launcher = scripts.join("contextmink");
    fs::write(&launcher, include_str!("../scripts/contextmink")).unwrap();
    let copied_binary = bin_dir.join("contextmink.exe");
    fs::copy(env!("CARGO_BIN_EXE_contextmink"), &copied_binary).unwrap();
    let copied_binary = copied_binary.to_string_lossy().replace('\\', "/");
    let rows = root.join("rows.jsonl");
    fs::write(&rows, "{\"mode\":\"x\"}\n").unwrap();
    let rows = rows.to_string_lossy().replace('\\', "/");

    for predicate in [
        vec!["--where", "/mode=x"],
        vec!["--where=/mode=x"],
        vec!["--where-contains", "/mode=x"],
        vec!["--where-contains=/mode=x"],
    ] {
        let mut args = vec!["--json", "json-select", rows.as_str(), "--fields", "/mode"];
        args.extend(predicate);
        let json = parse_success(run(&launcher, &root, &args));
        assert_eq!(json["total"], 1, "JSON-pointer predicate was rewritten");
        assert_eq!(json["rows"][0]["fields"]["/mode"], "\"x\"");
    }

    for (path_filter, expected) in [
        (vec!["--path-contains", "/mode"], "/mode"),
        (vec!["--path-contains=/mode"], "--path-contains=/mode"),
    ] {
        let mut args = vec!["--json", "capture", "--", copied_binary.as_str(), "--help"];
        args.extend(path_filter);
        let json = parse_success(run(&launcher, &root, &args));
        assert!(
            json["argv"]
                .as_array()
                .unwrap()
                .iter()
                .any(|arg| arg == expected),
            "JSON path filter was rewritten: {}",
            json["argv"]
        );
    }

    fs::remove_dir_all(&root).unwrap();
}
