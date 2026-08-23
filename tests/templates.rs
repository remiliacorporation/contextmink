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
fn agent_skill_templates_are_thin_and_harness_equivalent() {
    let template = include_str!("../templates/skills/contextmink/SKILL.md");
    let normalized_template = template.replace("\r\n", "\n");
    assert!(template.contains("output cardinality is unknown"));
    assert!(template.contains("Skip known-small direct reads"));
    assert!(template.contains("grep-terms --term TERM"));
    assert!(template.contains("contextmink.receipt.v2"));
    assert!(
        template.lines().count() < 120,
        "skill must remain a thin envelope"
    );

    let metadata = include_str!("../templates/skills/contextmink/agents/openai.yaml");
    assert!(metadata.contains("$contextmink"));
    assert!(metadata.contains("Bound broad agent reads"));

    // Source checkouts install these convenience copies at the manifest root,
    // while packaged and vendored tool subtrees intentionally contain only the
    // reusable templates. Validate installed copies when present without making
    // an unrelated consumer project recreate Contextmink's repository layout.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in [
        ".agents/skills/contextmink/SKILL.md",
        ".claude/skills/contextmink/SKILL.md",
    ] {
        let installed = root.join(relative);
        if installed.is_file() {
            assert_eq!(
                std::fs::read_to_string(installed)
                    .unwrap()
                    .replace("\r\n", "\n"),
                normalized_template
            );
        }
    }
    let installed_metadata = root.join(".agents/skills/contextmink/agents/openai.yaml");
    if installed_metadata.is_file() {
        assert_eq!(
            std::fs::read_to_string(installed_metadata)
                .unwrap()
                .replace("\r\n", "\n"),
            metadata.replace("\r\n", "\n")
        );
    }
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
        "files --path ",
        "files --path`",
        "dirs --path",
        "grep contextmink --path",
        "sqlite --path",
        "sqlite-schema --path",
        "files --max ",
        "files --term",
        "--require-complete-scan",
        "--max-scan-files",
        "--max-count-files",
        "--max-matches",
        "--max-scan-rows",
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
    assert!(guidance.contains("When the target file is unknown"));
    assert!(guidance.contains("repeated `--path-contains` values"));
    assert!(guidance.contains("`--max-document-bytes`"));
    assert!(guidance.contains("do not invent stdout/stderr chronology"));
    assert!(guidance.contains("including tracked submodules and Git-ignored"));
    assert!(guidance.contains("`--skip-nested-repos`"));
    assert!(guidance.contains("explicit root"));
}

#[test]
fn setup_guidance_preserves_repository_owned_configuration() {
    for (name, contents) in [
        ("README.md", include_str!("../README.md")),
        ("SETUP.md", include_str!("../SETUP.md")),
        ("docs/setup.md", include_str!("../docs/setup.md")),
    ] {
        assert!(
            contents.contains("repository-owned"),
            "{name} must identify configuration ownership"
        );
        assert!(
            contents.contains("preserve"),
            "{name} must explain configuration preservation"
        );
        assert!(
            contents.contains("missing") || contents.contains("fresh clone"),
            "{name} must explain fresh-clone binary repair"
        );
    }
}

#[test]
fn source_checkout_dogfoods_repository_owned_guard_policy_when_present() {
    let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".contextmink.toml");
    if !config_path.is_file() {
        return;
    }

    let config = std::fs::read_to_string(config_path).unwrap();
    assert!(config.contains("profile = \"contextmink\""));
    assert!(config.contains("exclude_globs = [\"state/**\"]"));
    assert!(config.contains("destructive_guard_recursive_delete_fragments = [\"state\"]"));
    assert!(config.contains("papertiger.sqlite"));
    assert!(config.contains("papertiger-mise.sqlite"));
}

#[test]
fn release_workflow_verifies_extracted_project_integration() {
    let workflow = include_str!("../.github/workflows/release-artifacts.yml");

    for required in [
        "tar -xzf",
        "Expand-Archive",
        "verify-source",
        "refs/heads/master",
        "cargo test --locked --all-targets --all-features",
        "cargo clippy --locked --workspace --all-targets --all-features -- -D warnings",
        "CARGO_TARGET_DIR: target/package-check",
        "cargo +1.95.0 check --locked",
        "needs: [verify-source, build]",
        "integration-project",
        "--json setup-project",
        "contextmink.project_setup.v1",
        "preserve_repository_owned",
        "setup-repair-smoke",
        "contextmink.receipt.v2",
        "agent_integration.md",
        ".agents/skills/contextmink/SKILL.md",
        ".claude/skills/contextmink/SKILL.md",
        "scripts/contextmink --json guard-check",
        "scripts/contextmink --json hook-snippet",
        "child_exit_code",
        "child_exit_zero",
        "exit_expected",
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
    assert!(!workflow.contains("\"success\"[[:space:]]*:[[:space:]]*true"));
    assert!(!workflow.contains("$captureSmoke.exit_code"));
    assert!(!workflow.contains("$captureSmoke.success"));
}

#[test]
fn cross_check_rehearses_every_non_windows_release_target() {
    let workflow = include_str!("../.github/workflows/release-artifacts.yml");
    let cross_check = include_str!("../scripts/cross_check.sh");

    for target in [
        "x86_64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ] {
        assert!(workflow.contains(target));
        assert!(cross_check.contains(target));
    }
    assert!(cross_check.contains("--install-targets"));
    assert!(cross_check.contains("rustup target add --toolchain"));
    assert!(cross_check.contains("--release --bins --target"));
}

#[test]
fn source_verification_isolates_package_fingerprints() {
    let verify = include_str!("../scripts/verify_source.sh");

    assert!(verify.contains("cargo fmt --all -- --check"));
    assert!(verify.contains("CONTEXTMINK_SOURCE_TARGET_DIR"));
    assert!(verify.contains(
        "CARGO_TARGET_DIR=\"$source_target_dir\" cargo test --locked --all-targets --all-features"
    ));
    assert!(
        verify.contains(
            "cargo clippy --locked --workspace --all-targets --all-features -- -D warnings"
        )
    );
    assert!(verify.contains("CONTEXTMINK_PACKAGE_TARGET_DIR"));
    assert!(verify.contains("CARGO_TARGET_DIR=\"$package_target_dir\" cargo package --locked"));
}

#[test]
fn release_verification_composes_pinned_independent_gates() {
    let verify = include_str!("../scripts/verify_release.sh");

    assert!(verify.contains("expected_actionlint=\"1.7.12\""));
    assert!(verify.contains("actionlint -color .github/workflows/*.yml"));
    assert!(verify.contains("bash scripts/verify_source.sh"));
    assert!(verify.contains("bash scripts/cross_check.sh \"$@\""));
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

    assert!(launcher.contains("--array | --fields)"));
    assert!(launcher.contains("--where | --where-contains | --key-contains"));
    assert!(launcher.contains("--where=*/* | --where-contains=*/* | --key-contains=*/*"));
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
        args.extend(predicate.iter().copied());
        let json = parse_success(run(&launcher, &root, &args));
        assert_eq!(
            json["result"]["total"], 1,
            "JSON-pointer predicate was rewritten: {predicate:?}"
        );
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
