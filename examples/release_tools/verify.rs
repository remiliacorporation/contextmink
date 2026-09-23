use super::support::{cleanup_temp, copy_tree, json, path_text, run, snapshot, temp_root};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::fs;
use std::path::{Component, Path};

fn initial_project(root: &Path) -> Result<()> {
    fs::create_dir_all(root)?;
    for name in ["AGENTS.md", "CLAUDE.md", "README.md", ".gitignore"] {
        fs::write(root.join(name), format!("Existing project-owned {name}\n"))?;
    }
    Ok(())
}

pub fn project(bundle: &Path) -> Result<()> {
    let bundle = fs::canonicalize(bundle)?;
    let owned = bundle.join("tools/contextmink");
    let manifest = json(&owned.join("manifest.json"))?;
    ensure!(
        manifest["schema"] == "contextmink.release_manifest.v3"
            && manifest["layout"] == "project-overlay",
        "expected project-overlay release manifest"
    );
    let hashes = manifest["binary_sha256"]
        .as_object()
        .context("manifest requires binary_sha256")?;
    ensure!(
        !hashes.is_empty(),
        "manifest must bind its executable hashes"
    );
    for (name, hash) in hashes {
        ensure!(
            Path::new(name)
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
            "manifest has unsafe binary path {name}"
        );
        ensure!(
            crate::digest::sha256(&fs::read(owned.join(name))?)
                == hash.as_str().context("binary hash must be text")?,
            "binary hash differs for {name}"
        );
    }
    for path in snapshot(&bundle)?.keys() {
        ensure!(
            matches!(path.components().next(), Some(Component::Normal(n)) if n == ".agents" || n == ".claude" || n == "tools"),
            "unexpected overlay path {}",
            path.display()
        );
        ensure!(
            !matches!(
                path.file_name().and_then(|s| s.to_str()),
                Some("project-install.json" | "runtime-install.json")
            ) && path.extension().is_none_or(|e| e != "sqlite"),
            "overlay includes private receipt or database {}",
            path.display()
        );
    }
    let skill = fs::read_to_string(bundle.join(".agents/skills/contextmink/SKILL.md"))?;
    ensure!(
        skill == fs::read_to_string(bundle.join(".claude/skills/contextmink/SKILL.md"))?
            && !skill.contains("<!-- installed-command -->"),
        "generated retrieval skills differ or retain a placeholder"
    );
    let windows = manifest.get("bridge_binary").is_some();
    for directory in [".agents", ".claude"] {
        ensure!(
            bundle
                .join(directory)
                .join("skills/contextmink-bridge/SKILL.md")
                .exists()
                == windows,
            "bridge skill does not match payload platform"
        );
    }
    if windows {
        ensure!(
            fs::read(bundle.join(".agents/skills/contextmink-bridge/SKILL.md"))?
                == fs::read(bundle.join(".claude/skills/contextmink-bridge/SKILL.md"))?,
            "bridge skills differ"
        );
    }
    let root = temp_root("project overlay")?;
    let project = root.join("established project");
    initial_project(&project)?;
    let baseline = snapshot(&project)?;
    copy_tree(&bundle, &project)?;
    let nested = project.join("nested/work");
    fs::create_dir_all(&nested)?;
    let binary = project
        .join("tools/contextmink/bin")
        .join(format!("contextmink{}", std::env::consts::EXE_SUFFIX));
    ensure!(
        run(&binary, &nested, &["--version"])?.trim()
            == format!(
                "contextmink {}",
                manifest["version"]
                    .as_str()
                    .context("manifest version missing")?
            ),
        "packaged version differs"
    );
    fs::write(project.join("sample.json"), "{\"count\":17}")?;
    let output: Value = serde_json::from_str(&run(
        &binary,
        &nested,
        &[
            "--json",
            "json-select",
            "../../sample.json",
            "--fields",
            "count",
        ],
    )?)?;
    ensure!(
        output["scope_complete"] == true,
        "packaged query is incomplete"
    );
    let after = snapshot(&project)?;
    for (path, hash) in baseline {
        ensure!(
            after.get(&path) == Some(&hash),
            "overlay changed project-owned {}",
            path.display()
        );
    }
    cleanup_temp(&root)?;
    println!("contextmink: direct project overlay smoke passed");
    Ok(())
}

pub fn user(binary: &Path) -> Result<()> {
    let binary = fs::canonicalize(binary)?;
    let root = temp_root("personal smoke")?;
    let home = root.join("home");
    let project = root.join("established project");
    fs::create_dir(&home)?;
    initial_project(&project)?;
    fs::write(home.join("AGENTS.md"), "Existing personal instructions\n")?;
    let original_project = snapshot(&project)?;
    let original_home = snapshot(&home)?;
    let home_arg = path_text(&home)?;
    let setup = |command: &str, extra: &[&str]| -> Result<String> {
        let mut args = vec![command, "--home", &home_arg];
        args.extend_from_slice(extra);
        run(&binary, &project, &args)
    };
    setup("setup-user", &["--dry-run"])?;
    ensure!(
        snapshot(&home)? == original_home,
        "dry-run changed personal files"
    );
    let result: Value = serde_json::from_str(&setup("setup-user", &["--json"])?)?;
    ensure!(
        result["schema"] == "contextmink.user_setup.v1",
        "unexpected setup receipt"
    );
    let installed = snapshot(&home)?;
    setup("setup-user", &[])?;
    ensure!(
        snapshot(&home)? == installed,
        "personal setup is not idempotent"
    );
    let runtime = home
        .join(".local/share/contextmink/bin")
        .join(binary.file_name().context("binary name missing")?);
    ensure!(
        run(&runtime, &project, &["--version"])? == run(&binary, &project, &["--version"])?,
        "installed version differs"
    );
    let skill = fs::read_to_string(home.join(".agents/skills/contextmink/SKILL.md"))?;
    ensure!(
        skill.contains(&path_text(&runtime)?)
            && !skill.contains("<!-- installed-command -->")
            && skill.starts_with("---\nname:"),
        "personal skill binding missing"
    );
    ensure!(
        skill == fs::read_to_string(home.join(".claude/skills/contextmink/SKILL.md"))?,
        "personal retrieval skills differ"
    );
    let bridge = runtime.with_file_name("contextmink-bridge.exe");
    let bridge_skill = home.join(".agents/skills/contextmink-bridge/SKILL.md");
    ensure!(
        bridge.exists() == cfg!(windows) && bridge_skill.exists() == cfg!(windows),
        "personal bridge installation has wrong platform"
    );
    if cfg!(windows) {
        let skill = fs::read_to_string(&bridge_skill)?;
        ensure!(
            skill.contains(&path_text(&bridge)?)
                && skill
                    == fs::read_to_string(home.join(".claude/skills/contextmink-bridge/SKILL.md"))?,
            "personal bridge binding differs"
        );
        let script = project.join("forward.sh");
        fs::write(
            &script,
            "#!/usr/bin/env bash\nset -euo pipefail\nnative=$1\nshift\nexec \"$native\" --print-argv -- \"$@\"\n",
        )?;
        let args = [
            "/unchanged/selector",
            "{\"type\":\"/unchanged/selector\"}",
            "space quoted",
            "",
        ];
        let script_arg = path_text(&script)?;
        let bridge_arg = path_text(&bridge)?;
        let project_arg = path_text(&project)?;
        let mut command = vec!["--cwd", &project_arg, "--script", &script_arg, &bridge_arg];
        command.extend(args);
        let output = run(&bridge, &project, &command)?;
        let expected: String = args
            .iter()
            .enumerate()
            .map(|(i, a)| format!("argv[{i}]={a}\n"))
            .collect();
        ensure!(
            output.replace("\r\n", "\n") == expected,
            "installed bridge changed script arguments: {output}"
        );
        fs::remove_file(script)?;
    }
    let result: Value = serde_json::from_str(&run(
        &runtime,
        &project,
        &["--json", "files", ".", "--show-files", "10"],
    )?)?;
    ensure!(
        result["scope_complete"] == true,
        "installed retrieval is incomplete"
    );
    setup("uninstall-user", &["--dry-run"])?;
    ensure!(
        snapshot(&home)? == installed,
        "uninstall dry-run changed files"
    );
    setup("uninstall-user", &[])?;
    for path in [
        &runtime,
        &bridge,
        &bridge_skill,
        &home.join(".agents/skills/contextmink/SKILL.md"),
        &home.join(".claude/skills/contextmink/SKILL.md"),
        &home.join(".claude/skills/contextmink-bridge/SKILL.md"),
    ] {
        ensure!(!path.exists(), "uninstall retained {}", path.display());
    }
    setup("setup-user", &[])?;
    ensure!(
        snapshot(&project)? == original_project,
        "personal setup changed consuming project"
    );
    ensure!(
        fs::read_to_string(home.join("AGENTS.md"))? == "Existing personal instructions\n",
        "personal instructions changed"
    );
    cleanup_temp(&root)?;
    println!("contextmink: personal extracted-install smoke passed");
    Ok(())
}
