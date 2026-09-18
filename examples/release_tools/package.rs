use super::support::{copy_tree, json, snapshot};
use anyhow::{Context, Result, ensure};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Component, Path};
use std::process::Command;

pub fn run(stage: &Path, archive: &Path) -> Result<()> {
    let stage = fs::canonicalize(stage)?;
    ensure!(
        !archive.exists(),
        "archive already exists; choose a fresh output path: {}",
        archive.display()
    );
    let archive = std::path::absolute(archive)?;
    ensure!(
        !archive.starts_with(&stage),
        "archive must be outside the stage directory"
    );
    let mut manifest = json(&stage.join("manifest.json"))?;
    ensure!(
        manifest["name"] == "contextmink",
        "manifest name must be contextmink"
    );
    ensure!(
        !stage.join("tools").exists(),
        "stage must be a fresh native release; use a new staging directory"
    );
    let binary = manifest["binary"]
        .as_str()
        .context("manifest requires binary")?
        .to_owned();
    ensure!(
        binary == "contextmink" || binary == "contextmink.exe",
        "manifest binary must name contextmink or contextmink.exe"
    );
    let windows = binary.ends_with(".exe");
    ensure!(
        if windows {
            manifest["bridge_binary"] == "contextmink-bridge.exe"
        } else {
            manifest.get("bridge_binary").is_none()
        },
        "Windows payload requires its companion bridge; non-Windows payload must omit it"
    );
    let mut names = vec![binary];
    if windows {
        names.push("contextmink-bridge.exe".into());
    }
    let mut hashes = Map::new();
    for name in &names {
        hashes.insert(
            format!("bin/{name}"),
            Value::String(crate::digest::sha256(
                &fs::read(stage.join(name))
                    .with_context(|| format!("missing staged binary {name}"))?,
            )),
        );
    }
    snapshot(&stage)?; // Reject links before moving any staged files.
    let children: Vec<_> = fs::read_dir(&stage)?.collect::<std::io::Result<_>>()?;
    let owned = stage.join("tools/contextmink");
    fs::create_dir_all(owned.join("bin"))?;
    for child in children {
        let name = child.file_name();
        let target = if names.iter().any(|n| name == n.as_str()) {
            owned.join("bin").join(name)
        } else {
            owned.join(name)
        };
        fs::rename(child.path(), target)?;
    }
    manifest["schema"] = "contextmink.release-manifest.v2".into();
    manifest["layout"] = "project-overlay".into();
    manifest["binary_sha256"] = hashes.into();
    manifest["binary"] = format!("bin/{}", names[0]).into();
    if windows {
        manifest["bridge_binary"] = "bin/contextmink-bridge.exe".into();
    }
    fs::write(
        owned.join("manifest.json"),
        format!("{}\n", serde_json::to_string_pretty(&manifest)?),
    )?;
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    let skills = if windows {
        vec!["contextmink", "contextmink-bridge"]
    } else {
        vec!["contextmink"]
    };
    for name in skills {
        let template = source.join("templates/skills").join(name);
        for directory in [".agents", ".claude"] {
            let target = stage.join(directory).join("skills").join(name);
            if directory == ".agents" {
                copy_tree(&template, &target)?;
            } else {
                fs::create_dir_all(&target)?;
            }
            let body = fs::read_to_string(template.join("SKILL.md"))?
                .replace("\r\n", "\n")
                .replace("<!-- installed-command -->\n", "");
            fs::write(target.join("SKILL.md"), body)?;
        }
    }
    fs::copy(
        source.join("templates/AGENTS.contextmink.md"),
        owned.join("agent_integration.md"),
    )?;
    for path in snapshot(&stage)?.keys() {
        ensure!(
            matches!(path.components().next(), Some(Component::Normal(n)) if n == ".agents" || n == ".claude" || n == "tools"),
            "unexpected overlay path {}",
            path.display()
        );
    }
    let mut command;
    if windows {
        ensure!(
            cfg!(windows),
            "build the Windows zip on its native Windows release runner"
        );
        let system = std::env::var_os("SystemRoot")
            .context("SystemRoot is missing; run on a native Windows host")?;
        command = Command::new(Path::new(&system).join("System32/tar.exe"));
        command.args(["-a", "-cf"]);
    } else {
        command = Command::new("tar");
        command.arg("-czf");
    }
    let status = command
        .arg(&archive)
        .args([".agents", ".claude", "tools"])
        .current_dir(&stage)
        .status()?;
    ensure!(
        status.success(),
        "archive creation failed; inspect {}, then stage a fresh release",
        archive.display()
    );
    println!("contextmink: packaged skills and tools at project-relative paths");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::{cleanup_temp, temp_root};

    #[test]
    fn packages_host_skills_and_hashes_without_private_state() {
        let root = temp_root("package test").unwrap();
        let stage = root.join("stage");
        fs::create_dir(&stage).unwrap();
        let binary = format!("contextmink{}", std::env::consts::EXE_SUFFIX);
        fs::write(stage.join(&binary), b"fixture runtime").unwrap();
        let mut manifest =
            serde_json::json!({"name":"contextmink", "version":"0.14.0", "binary":binary});
        if cfg!(windows) {
            manifest["bridge_binary"] = "contextmink-bridge.exe".into();
            fs::write(stage.join("contextmink-bridge.exe"), b"fixture bridge").unwrap();
        }
        fs::write(
            stage.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let archive = root.join(if cfg!(windows) {
            "release.zip"
        } else {
            "release.tar.gz"
        });
        run(&stage, &archive).unwrap();
        assert!(archive.metadata().unwrap().len() > 0);
        let output = json(&stage.join("tools/contextmink/manifest.json")).unwrap();
        assert_eq!(
            output["binary_sha256"][format!("bin/{binary}")],
            crate::digest::sha256(b"fixture runtime")
        );
        for directory in [".agents", ".claude"] {
            assert!(
                stage
                    .join(directory)
                    .join("skills/contextmink/SKILL.md")
                    .exists()
            );
            assert_eq!(
                stage
                    .join(directory)
                    .join("skills/contextmink-bridge/SKILL.md")
                    .exists(),
                cfg!(windows)
            );
        }
        assert!(
            run(&stage, &archive).is_err(),
            "must not overwrite an existing archive"
        );
        cleanup_temp(&root).unwrap();
    }

    #[test]
    fn missing_companion_refuses_before_staging_mutations() {
        let root = temp_root("package missing bridge").unwrap();
        let stage = root.join("stage");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("contextmink.exe"), b"fixture runtime").unwrap();
        fs::write(stage.join("manifest.json"), r#"{"name":"contextmink","binary":"contextmink.exe","bridge_binary":"contextmink-bridge.exe"}"#).unwrap();
        let before = snapshot(&stage).unwrap();
        assert!(run(&stage, &root.join("release.zip")).is_err());
        assert_eq!(snapshot(&stage).unwrap(), before);
        cleanup_temp(&root).unwrap();
    }
}
