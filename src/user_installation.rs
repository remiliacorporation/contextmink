//! Receipt and integrity boundary shared by both personally installed executables.
use crate::config::project_setup::receipt::managed_runtime_sha256 as sha256;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
const TOOL: &str = "contextmink";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    pub(crate) schema: String,
    pub(crate) version: String,
    pub(crate) home: PathBuf,
    pub(crate) installed: bool,
    pub(crate) files: BTreeMap<String, String>,
}

pub(crate) fn personal_root() -> String {
    format!(".local/share/{TOOL}")
}

pub(crate) fn binary_path() -> String {
    format!(
        "{}/bin/{TOOL}{}",
        personal_root(),
        std::env::consts::EXE_SUFFIX
    )
}

pub(crate) fn paths() -> Vec<String> {
    let mut paths = retrieval_paths();
    if cfg!(windows) {
        paths.extend([
            format!("{}/bin/contextmink-bridge.exe", personal_root()),
            ".agents/skills/contextmink-bridge/SKILL.md".into(),
            ".claude/skills/contextmink-bridge/SKILL.md".into(),
        ]);
    }
    paths
}

pub(crate) fn retrieval_paths() -> Vec<String> {
    vec![
        binary_path(),
        format!("{}/agent_integration.md", personal_root()),
        format!(".agents/skills/{TOOL}/SKILL.md"),
        format!(".claude/skills/{TOOL}/SKILL.md"),
    ]
}

/// Refuse links and wrong file types at every boundary before reading or writing.
pub(crate) fn validate_path(home: &Path, relative: &str) -> Result<PathBuf> {
    let mut path = home.to_path_buf();
    let components: Vec<_> = Path::new(relative).components().collect();
    for (i, component) in components.iter().enumerate() {
        let std::path::Component::Normal(part) = component else {
            bail!("invalid personal install path {relative}; use a verified release's setup-user");
        };
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(meta) => {
                if meta.file_type().is_symlink()
                    || (i + 1 < components.len() && !meta.is_dir())
                    || (i + 1 == components.len() && !meta.is_file())
                {
                    bail!(
                        "personal install path {} is a link or wrong file type; move it aside before setup-user",
                        path.display()
                    );
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error).with_context(|| format!("inspect {}", path.display())),
        }
    }
    Ok(path)
}

/// A personal runtime refuses a torn or divergent installation before doing work.
pub(crate) fn verify_runtime() -> Result<()> {
    let exe = std::env::current_exe()?;
    let Some(root) = exe.parent().and_then(Path::parent) else {
        return Ok(());
    };
    if root.file_name().and_then(|s| s.to_str()) != Some(TOOL)
        || root
            .parent()
            .and_then(Path::file_name)
            .and_then(|s| s.to_str())
            != Some("share")
        || root
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .and_then(|s| s.to_str())
            != Some(".local")
    {
        return Ok(());
    }
    let home = root
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .context("personal runtime has no home; run setup-user from an external release")?;
    let path = validate_path(home, &format!("{}/user-install.json", personal_root()))?;
    let bytes = fs::read(&path).with_context(|| {
        format!(
            "personal receipt is missing; run {TOOL} setup-user from a verified external release"
        )
    })?;
    let receipt: Receipt = serde_json::from_slice(&bytes).context(
        "invalid personal receipt; restore it or repair with setup-user from a verified release",
    )?;
    let expected_root = receipt.home.join(personal_root());
    if !receipt.installed
        || receipt.schema != format!("{TOOL}.user_install.v1")
        || receipt.version != env!("CARGO_PKG_VERSION")
        || fs::canonicalize(expected_root)? != fs::canonicalize(root)?
        || receipt.files.len() != paths().len()
        || receipt.files.keys().any(|p| !paths().contains(p))
    {
        bail!(
            "personal installation identity differs; run {TOOL} setup-user from a verified external release"
        );
    }
    for (relative, hash) in &receipt.files {
        let file = validate_path(&receipt.home, relative)?;
        if sha256(
            &fs::read(&file)
                .with_context(|| format!("missing {}; repair with setup-user", file.display()))?,
        ) != *hash
        {
            bail!(
                "personal file {} differs from its receipt; review it, then repair with setup-user --replace-managed",
                file.display()
            );
        }
    }
    Ok(())
}
