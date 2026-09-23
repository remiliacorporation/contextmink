//! Personal installation owns tool files, never consuming projects or harness settings.
use std::collections::BTreeMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::user_installation::{
    RECEIPT_SCHEMA, Receipt, binary_path, bridge_path, parse_receipt, personal_root, runtime_paths,
    text_paths, validate_path,
};
use anyhow::{Context, Result, bail};

use crate::config::project_setup::receipt::managed_runtime_sha256 as sha256;
const TOOL: &str = "contextmink";
const SKILL: &str = include_str!("../templates/skills/contextmink/SKILL.md");
const BRIDGE_SKILL: &str = include_str!("../templates/skills/contextmink-bridge/SKILL.md");
const REFERENCE: &[u8] = include_bytes!("../templates/agent_integration.md");

/// Every path a receipt owns, executables first.
fn owned_paths(receipt: &Receipt) -> Vec<String> {
    receipt
        .runtime_files
        .keys()
        .chain(&receipt.text_files)
        .cloned()
        .collect()
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

fn publish(path: &Path, content: &[u8]) -> Result<()> {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .context("personal destination needs a parent")?;
    fs::create_dir_all(parent)?;
    let staged = parent.join(format!(
        ".{}-{}-{}.tmp",
        TOOL,
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)?;
        file.write_all(content)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&staged, path).with_context(|| format!("publish {}; close running tool processes and rerun setup-user from an external release", path.display()))?;
        Ok(())
    })();
    if result.is_err() && staged.exists() {
        // Cleanup is limited to this invocation's exact staging file.
        fs::remove_file(&staged).context("remove failed personal-install staging file")?;
    }
    result
}

pub(crate) fn run(home: Option<&Path>, dry_run: bool, remove: bool) -> Result<serde_json::Value> {
    let home = match home {
        Some(path) => path.to_path_buf(),
        None => std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .context("home is unavailable; pass setup-user --home <existing-directory>")?,
    };
    let home = PathBuf::from(
        crate::config::canonical_normalized(&home)
            .context("home must exist; pass --home <existing-directory>")?,
    );
    if !home.is_dir() {
        bail!("home is not a directory; pass --home <existing-directory>");
    }
    let receipt_path = validate_path(&home, &format!("{}/user-install.json", personal_root()))?;
    let previous: Option<Receipt> = read_optional(&receipt_path)?
        .map(|bytes| parse_receipt(&bytes))
        .transpose()?;
    if let Some(receipt) = &previous {
        if receipt.home != home
            || (receipt.installed && !receipt.owns_current_paths())
            || (!receipt.installed && !owned_paths(receipt).is_empty())
            || receipt
                .runtime_files
                .values()
                .any(|h| h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            bail!(
                "personal receipt identity or paths are invalid; restore user-install.json for this home, or move it aside and rerun setup-user to reinstall"
            );
        }
        if semver::Version::parse(&receipt.version)?
            > semver::Version::parse(env!("CARGO_PKG_VERSION"))?
        {
            bail!(
                "personal installation is newer; run setup-user with version {} or newer",
                receipt.version
            );
        }
    } else if remove {
        bail!(
            "no personal receipt at {}; nothing is owned for uninstall-user",
            receipt_path.display()
        );
    }
    let source = std::env::current_exe()?;
    let destination = home.join(binary_path());
    if destination.exists() && fs::canonicalize(&destination)? == fs::canonicalize(&source)? {
        bail!(
            "run setup-user/uninstall-user from an external release binary, outside {}",
            destination.display()
        );
    }
    if !SKILL.contains("<!-- installed-command -->") {
        bail!("release skill has no command binding; use a complete verified release");
    }
    let binding = installed_binding(&home);
    let reference = home.join(format!("{}/agent_integration.md", personal_root()));
    let skill = SKILL
        .replace("\r\n", "\n")
        .replace("<!-- installed-command -->", &binding)
        .replace(
            "../../../tools/contextmink/agent_integration.md",
            &reference.to_string_lossy().replace('\\', "/"),
        );
    let mut wanted = if remove {
        BTreeMap::new()
    } else {
        BTreeMap::from([
            (binary_path(), fs::read(&source)?),
            (
                format!("{}/agent_integration.md", personal_root()),
                REFERENCE.to_vec(),
            ),
            (
                format!(".agents/skills/{TOOL}/SKILL.md"),
                skill.as_bytes().to_vec(),
            ),
            (
                format!(".claude/skills/{TOOL}/SKILL.md"),
                skill.as_bytes().to_vec(),
            ),
        ])
    };
    if cfg!(windows) && !remove {
        let bridge = source.with_file_name("contextmink-bridge.exe");
        let bytes = fs::read(&bridge).with_context(|| format!(
            "missing sibling bridge {}; run setup-user from a complete extracted Windows release",
            bridge.display()
        ))?;
        let probe = std::process::Command::new(&bridge)
            .arg("--version")
            .output()
            .with_context(|| format!("cannot execute sibling bridge {}; run setup-user from a complete verified Windows release", bridge.display()))?;
        let expected = format!("contextmink-bridge {}", env!("CARGO_PKG_VERSION"));
        if !probe.status.success() || String::from_utf8_lossy(&probe.stdout).trim() != expected {
            bail!(
                "sibling bridge does not match {expected}; run setup-user from a complete verified Windows release"
            );
        }
        let relative = bridge_path();
        let binding = format!(
            "Personal bridge: `{}`. Use an explicitly selected project bridge when the project pins its own runtime.",
            home.join(&relative).to_string_lossy().replace('\\', "/")
        );
        if !BRIDGE_SKILL.contains("<!-- installed-command -->") {
            bail!(
                "release bridge skill has no command binding; use a complete verified Windows release"
            );
        }
        let skill = BRIDGE_SKILL
            .replace("\r\n", "\n")
            .replace("<!-- installed-command -->", &binding);
        wanted.insert(relative, bytes);
        for directory in [".agents", ".claude"] {
            wanted.insert(
                format!("{directory}/skills/contextmink-bridge/SKILL.md"),
                skill.as_bytes().to_vec(),
            );
        }
    }
    let mut actions = Vec::new();
    let selected_paths = if remove {
        owned_paths(
            previous
                .as_ref()
                .context("uninstall-user requires an existing receipt")?,
        )
    } else {
        runtime_paths().into_iter().chain(text_paths()).collect()
    };
    for relative in selected_paths {
        let path = validate_path(&home, &relative)?;
        let existing = read_optional(&path)?;
        let owned_hash = previous
            .as_ref()
            .and_then(|r| r.runtime_files.get(&relative));
        let action = match (existing.as_ref(), wanted.get(&relative)) {
            (Some(old), Some(new)) if old == new => "unchanged",
            (Some(_), Some(_)) => "replace",
            (None, Some(_)) => "create",
            (None, None) => "absent",
            // Removal: text is owned by path; an executable only while its
            // bytes still match the receipt.
            (Some(old), None) => {
                if runtime_paths().contains(&relative)
                    && !owned_hash.is_some_and(|hash| *hash == sha256(old))
                {
                    bail!(
                        "personal runtime {} differs from its receipt; restore it or move it aside, then rerun uninstall-user",
                        path.display()
                    );
                }
                "remove"
            }
        };
        #[cfg(windows)]
        if matches!(action, "replace" | "remove")
            && path.extension().is_some_and(|extension| extension == "exe")
        {
            // A mapped executable can be readable while Windows refuses its
            // replacement. Probe without truncating before publishing any file.
            // This catches existing locks, not locks acquired after preflight.
            fs::OpenOptions::new().write(true).open(&path).with_context(|| {
                format!(
                    "cannot open personal runtime {} for {action}; close running tool processes and rerun {}-user from an external release; if it still refuses, check the file's write permissions",
                    path.display(),
                    if remove { "uninstall" } else { "setup" }
                )
            })?;
        }
        actions.push(serde_json::json!({"path":path,"action":action}));
    }
    if !dry_run {
        for action in &actions {
            let path = PathBuf::from(
                action["path"]
                    .as_str()
                    .context("personal path is not UTF-8")?,
            );
            match action["action"].as_str() {
                Some("create" | "replace") => {
                    let relative = path
                        .strip_prefix(&home)?
                        .to_string_lossy()
                        .replace('\\', "/");
                    publish(&path, &wanted[&relative])?;
                }
                Some("remove") => fs::remove_file(&path)?,
                _ => {}
            }
        }
        if !remove {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))?;
            }
        }
        let runtime = runtime_paths();
        let receipt = Receipt {
            schema: RECEIPT_SCHEMA.to_owned(),
            version: env!("CARGO_PKG_VERSION").into(),
            home: home.clone(),
            installed: !remove,
            runtime_files: wanted
                .iter()
                .filter(|(p, _)| runtime.contains(p))
                .map(|(p, b)| (p.clone(), sha256(b)))
                .collect(),
            text_files: wanted
                .keys()
                .filter(|p| !runtime.contains(p))
                .cloned()
                .collect(),
        };
        let bytes = serde_json::to_vec_pretty(&receipt)?;
        if read_optional(&receipt_path)?.as_deref() != Some(bytes.as_slice()) {
            publish(&receipt_path, &bytes)?;
        }
        if !remove {
            let probe = std::process::Command::new(&destination).arg("--version").output()
                .with_context(|| format!("installed executable {} could not run; check execute permissions and run setup-user again", destination.display()))?;
            let expected = format!("{TOOL} {}", env!("CARGO_PKG_VERSION"));
            if !probe.status.success() || String::from_utf8_lossy(&probe.stdout).trim() != expected
            {
                bail!(
                    "installed runtime verification failed: {}; repair with setup-user from a verified external release",
                    String::from_utf8_lossy(&probe.stderr)
                );
            }
        }
    }
    Ok(
        serde_json::json!({"schema":format!("{TOOL}.user_setup.v1"),"home":home,"dry_run":dry_run,"operation":if remove {"remove"} else {"install"},"actions":actions,
        "next_actions": ["After installation, start a fresh agent session to refresh skill discovery. No agent-side copying or AGENTS.md edits are needed. Selection remains model-dependent.", "Personal data and the lifecycle receipt survive uninstall-user. Project installations and project guidance are untouched."]}),
    )
}

fn installed_binding(home: &Path) -> String {
    format!(
        "Personal executable: `{}`. Prefer a project-pinned runtime when one exists.\n",
        home.join(binary_path())
            .to_string_lossy()
            .replace('\\', "/")
    )
}
