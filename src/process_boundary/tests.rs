use std::fs;

use super::*;

fn temp_tree(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "contextmink-process-boundary-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root); // guardrail: allow-ignore-result best-effort reused test fixture cleanup
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn direct_mode_classifies_shebang_before_spawn() {
    let root = temp_tree("shebang");
    fs::write(root.join("probe"), "#!/usr/bin/env bash\nexit 0\n").unwrap();
    let prepared = prepare_command("./probe", &[], &root, false, false).unwrap();
    assert_eq!(
        prepared.execution_mode,
        if cfg!(windows) {
            "shebang_script"
        } else {
            "native_shebang"
        }
    );
}

#[test]
fn non_shebang_text_stays_native_unless_script_is_explicit() {
    let root = temp_tree("explicit");
    fs::write(root.join("probe"), "exit 0\n").unwrap();
    let direct = prepare_command("./probe", &[], &root, false, false).unwrap();
    assert_eq!(direct.execution_mode, "native");

    let explicit = prepare_command("./probe", &[], &root, true, false).unwrap();
    assert_eq!(explicit.execution_mode, "bash_script");
}

#[test]
fn explicit_script_requires_a_real_file() {
    let root = temp_tree("missing");
    let error = prepare_command("./missing", &[], &root, true, false)
        .err()
        .expect("missing explicit script must fail");
    assert!(error.contains("script not found"));
}

#[test]
fn bash_relay_never_places_raw_hostile_argv_on_the_startup_command_line() {
    let root = temp_tree("relay");
    fs::write(root.join("probe"), "exit 0\n").unwrap();
    let args = vec![
        "@scratch/args with spaces.json".to_string(),
        "semi;colon".to_string(),
        "héλ".to_string(),
        String::new(),
    ];
    let prepared = prepare_command("./probe", &args, &root, true, false).unwrap();
    let startup_args = prepared
        .command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(startup_args.iter().all(|arg| !arg.contains("@scratch")));
    assert!(startup_args.iter().all(|arg| !arg.contains("semi;colon")));
    assert!(startup_args.iter().all(|arg| !arg.contains("héλ")));
}

#[test]
fn bash_relay_preserves_zero_empty_and_literal_arguments() {
    let root = temp_tree("relay-argument-cardinality");
    // A command with no arguments must also pass the non-script relay branch.
    let bash = if cfg!(target_os = "macos") {
        PathBuf::from("/bin/bash")
    } else {
        locate_bash().unwrap()
    };
    let mut no_args = Command::new(&bash);
    append_bash_argv_relay(&mut no_args, &[bash.to_string_lossy().into_owned()], false);
    let output = no_args.current_dir(&root).output().unwrap();
    assert!(
        output.status.success(),
        "zero-argument command stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let script = root.join("arguments.sh");
    fs::write(
        &script,
        "printf '%s\\n' \"$#\"\nfor arg; do printf '<%s>\\n' \"$arg\"; done\nexit 7\n",
    )
    .unwrap();
    for args in [
        vec![],
        vec![String::new()],
        vec![String::new(), "two words".into(), "@args;$()héλ".into()],
    ] {
        for explicit_script in [true, false] {
            // Exercise both relay branches; command mode starts Bash as its child.
            let mut argv = if explicit_script {
                vec![script.to_string_lossy().into_owned()]
            } else {
                vec![
                    locate_bash().unwrap().to_string_lossy().into_owned(),
                    script.to_string_lossy().into_owned(),
                ]
            };
            argv.extend(args.iter().cloned());
            // Keep coverage on the system shell even if Homebrew Bash is on PATH.
            let bash = if cfg!(target_os = "macos") {
                PathBuf::from("/bin/bash")
            } else {
                locate_bash().unwrap()
            };
            let mut command = Command::new(bash);
            append_bash_argv_relay(&mut command, &argv, explicit_script);
            let output = command.current_dir(&root).output().unwrap();
            assert_eq!(
                output.status.code(),
                Some(7),
                "relay stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let mut expected = format!("{}\n", args.len());
            for arg in &args {
                expected.push_str(&format!("<{arg}>\n"));
            }
            assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
        }
    }
}

#[test]
fn caller_project_policy_wins_for_a_global_binary() {
    let install = temp_tree("global-install");
    let project = temp_tree("global-project");
    fs::write(project.join(".contextmink.toml"), "profile = \"test\"\n").unwrap();
    let nested = project.join("nested/work");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(resolve_project_root(&install, &nested), project);
}

#[test]
fn caller_repository_wins_over_install_tree_policy() {
    let install = temp_tree("configured-install");
    fs::write(install.join(".contextmink.toml"), "profile = \"install\"\n").unwrap();
    let project = temp_tree("unconfigured-project");
    fs::create_dir_all(project.join(".git")).unwrap();
    let nested = project.join("nested/work");
    fs::create_dir_all(&nested).unwrap();

    assert_eq!(resolve_project_root(&install, &nested), project);
}
