//! Receipt and integrity boundary shared by both personally installed executables.
use crate::config::project_setup::receipt::managed_runtime_sha256 as sha256;
use anyhow::{Context, Result, bail};
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
const TOOL: &str = "contextmink";
pub(crate) const RECEIPT_SCHEMA: &str = "contextmink.user_install.v2";
/// Read once so an upgrade or removal can rewrite it as the current schema.
const PREVIOUS_RECEIPT_SCHEMA: &str = "contextmink.user_install.v1";

/// Personal ownership: executables are bound to their raw-byte SHA-256, while
/// release-managed skill and reference text is owned by path alone.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    pub(crate) schema: String,
    pub(crate) version: String,
    pub(crate) home: PathBuf,
    pub(crate) installed: bool,
    pub(crate) runtime_files: BTreeMap<String, String>,
    pub(crate) text_files: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviousReceipt {
    #[serde(rename = "schema")]
    _schema: IgnoredAny,
    version: String,
    home: PathBuf,
    installed: bool,
    files: BTreeMap<String, String>,
}

impl Receipt {
    /// True when the owned paths are exactly this platform's installation set.
    pub(crate) fn owns_current_paths(&self) -> bool {
        let text = self.text_files.iter().cloned().collect::<BTreeSet<_>>();
        text.len() == self.text_files.len()
            && text == text_paths().into_iter().collect()
            && self.runtime_files.keys().cloned().collect::<BTreeSet<_>>()
                == runtime_paths().into_iter().collect()
    }
}

/// Parse `user-install.json`, reading the previous schema as the current one.
pub(crate) fn parse_receipt(bytes: &[u8]) -> Result<Receipt> {
    let envelope: serde_json::Value = serde_json::from_slice(bytes).context(
        "invalid user-install.json; restore its verified backup or move it aside, then rerun setup-user from a verified release",
    )?;
    let schema = envelope
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    match schema.as_str() {
        RECEIPT_SCHEMA => serde_json::from_value(envelope).context(
            "invalid user-install.json; restore its verified backup or move it aside, then rerun setup-user from a verified release",
        ),
        PREVIOUS_RECEIPT_SCHEMA => {
            let previous: PreviousReceipt = serde_json::from_value(envelope).context(
                "invalid user-install.json; restore its verified backup or move it aside, then rerun setup-user from a verified release",
            )?;
            let runtime = runtime_paths();
            let (runtime_files, text_files): (BTreeMap<_, _>, BTreeMap<_, _>) = previous
                .files
                .into_iter()
                .partition(|(path, _)| runtime.contains(path));
            Ok(Receipt {
                schema: RECEIPT_SCHEMA.to_owned(),
                version: previous.version,
                home: previous.home,
                installed: previous.installed,
                runtime_files,
                text_files: text_files.into_keys().collect(),
            })
        }
        _ => bail!(
            "unsupported personal receipt schema {schema:?}; move user-install.json aside, then rerun setup-user from a verified release to reinstall"
        ),
    }
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

pub(crate) fn bridge_path() -> String {
    format!("{}/bin/contextmink-bridge.exe", personal_root())
}

/// Installed executables, which the receipt binds by content hash.
pub(crate) fn runtime_paths() -> Vec<String> {
    let mut paths = vec![binary_path()];
    if cfg!(windows) {
        paths.push(bridge_path());
    }
    paths
}

/// Release-managed skill and reference text, which the receipt owns by path.
pub(crate) fn text_paths() -> Vec<String> {
    let mut paths = vec![
        format!("{}/agent_integration.md", personal_root()),
        format!(".agents/skills/{TOOL}/SKILL.md"),
        format!(".claude/skills/{TOOL}/SKILL.md"),
    ];
    if cfg!(windows) {
        paths.extend([
            ".agents/skills/contextmink-bridge/SKILL.md".into(),
            ".claude/skills/contextmink-bridge/SKILL.md".into(),
        ]);
    }
    paths
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
    let receipt = parse_receipt(&bytes)?;
    let expected_root = receipt.home.join(personal_root());
    if !receipt.installed
        || receipt.schema != RECEIPT_SCHEMA
        || receipt.version != env!("CARGO_PKG_VERSION")
        || fs::canonicalize(expected_root)? != fs::canonicalize(root)?
        || !receipt.owns_current_paths()
    {
        bail!(
            "personal installation identity differs; run {TOOL} setup-user from a verified external release"
        );
    }
    for (relative, hash) in &receipt.runtime_files {
        let file = validate_path(&receipt.home, relative)?;
        if sha256(
            &fs::read(&file)
                .with_context(|| format!("missing {}; repair with setup-user", file.display()))?,
        ) != *hash
        {
            bail!(
                "personal runtime {} differs from its receipt; repair with setup-user from a verified external release",
                file.display()
            );
        }
    }
    for relative in &receipt.text_files {
        let file = validate_path(&receipt.home, relative)?;
        if !file.is_file() {
            bail!(
                "missing {}; repair with setup-user from a verified external release",
                file.display()
            );
        }
    }
    Ok(())
}
