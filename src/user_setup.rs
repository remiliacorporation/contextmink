//! Personal installation owns tool files, never consuming projects or harness settings.
use std::collections::BTreeMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::user_installation::{
    Receipt, binary_path, paths, personal_root, retrieval_paths, validate_path,
};
use anyhow::{Context, Result, bail};

use crate::config::project_setup::receipt::managed_runtime_sha256 as sha256;
const TOOL: &str = "contextmink";
const SKILL: &str = include_str!("../templates/skills/contextmink/SKILL.md");
const BRIDGE_SKILL: &str = include_str!("../templates/skills/contextmink-bridge/SKILL.md");
const REFERENCE: &[u8] = include_bytes!("../templates/AGENTS.contextmink.md");

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

pub(crate) fn run(
    home: Option<&Path>,
    dry_run: bool,
    replace: bool,
    remove: bool,
) -> Result<serde_json::Value> {
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
        .map(|bytes| serde_json::from_slice(&bytes))
        .transpose()
        .context("invalid user-install.json; restore its verified backup before setup-user")?;
    let schema = format!("{TOOL}.user_install.v1");
    if let Some(receipt) = &previous {
        // The pre-bridge Windows installer owned exactly the retrieval set.
        // Accept that complete older set for upgrade/removal, never a partial
        // current receipt or additional unowned paths.
        let expected_paths = if cfg!(windows)
            && semver::Version::parse(&receipt.version)? < semver::Version::new(0, 14, 0)
        {
            retrieval_paths()
        } else {
            paths()
        };
        if receipt.schema != schema
            || receipt.home != home
            || (receipt.installed && receipt.files.len() != expected_paths.len())
            || (!receipt.installed && !receipt.files.is_empty())
            || receipt.files.keys().any(|p| !expected_paths.contains(p))
            || receipt
                .files
                .values()
                .any(|h| h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            bail!(
                "personal receipt identity or paths are invalid; restore user-install.json for this home before setup-user"
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
        let relative = format!("{}/bin/contextmink-bridge.exe", personal_root());
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
        previous
            .as_ref()
            .context("uninstall-user requires an existing receipt")?
            .files
            .keys()
            .cloned()
            .collect()
    } else {
        paths()
    };
    for relative in selected_paths {
        let path = validate_path(&home, &relative)?;
        let existing = read_optional(&path)?;
        let owned = previous.as_ref().and_then(|r| r.files.get(&relative));
        let action = match (existing.as_ref(), wanted.get(&relative)) {
            (Some(old), Some(new)) if old == new => "unchanged",
            (Some(old), _) if owned.is_some_and(|hash| *hash == sha256(old)) => {
                if remove {
                    "remove"
                } else {
                    "replace"
                }
            }
            (Some(_), Some(_)) if replace => "replace",
            (Some(_), _) => bail!(
                "{} is unowned or modified; review it and use setup-user --home {:?} --replace-managed to replace selected files, or move it aside before uninstall-user",
                path.display(),
                home
            ),
            (None, Some(_)) => "create",
            (None, None) => "absent",
        };
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
        let receipt = Receipt {
            schema,
            version: env!("CARGO_PKG_VERSION").into(),
            home: home.clone(),
            installed: !remove,
            files: wanted.iter().map(|(p, b)| (p.clone(), sha256(b))).collect(),
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
        "Personal executable: `{}`. Invoke this native binary from the consuming project's working directory. It honors local configuration when present; no project setup or config is required. Use an explicitly selected project runtime when its contract requires one.\n",
        home.join(binary_path())
            .to_string_lossy()
            .replace('\\', "/")
    )
}
