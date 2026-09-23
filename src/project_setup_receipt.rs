//! Persisted ownership for project-local Contextmink integration files.

use std::borrow::Cow;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use semver::Version;
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};

use super::SkillTarget;

pub(super) const INSTALL_RECEIPT_PATH: &str = "tools/contextmink/project-install.json";
pub(super) const INSTALL_RECEIPT_SCHEMA: &str = "contextmink.project_install.v3";
/// Read once so an upgrade can rewrite it as the current schema.
const PREVIOUS_INSTALL_RECEIPT_SCHEMA: &str = "contextmink.project_install.v2";
/// The host-local runtime receipt Contextmink 0.15.0 wrote. Binaries are now
/// owned by their fixed paths, so setup and uninstall remove this file once.
pub(super) const PREVIOUS_RUNTIME_RECEIPT_PATH: &str = "tools/contextmink/bin/runtime-install.json";
const PREVIOUS_RUNTIME_RECEIPT_SCHEMA: &str = "contextmink.runtime_install.v1";
/// Every host binary path a project installation owns. Setup writes the ones
/// this host runs and never touches the others, so a checkout shared between
/// Windows and WSL keeps the other platform's binary; uninstall removes every
/// one that exists. Binaries are verified at download, never before a run.
pub(super) const MANAGED_RUNTIME_PATHS: &[&str] = &[
    "tools/contextmink/bin/contextmink",
    "tools/contextmink/bin/contextmink.exe",
    "tools/contextmink/bin/contextmink-bridge.exe",
];

/// Project ownership: the release version, the frozen skill target, and the
/// `.gitignore` surfaces setup created. The owned text paths follow from the
/// skill target; they carry no content identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InstallReceipt {
    pub(super) schema: String,
    pub(super) contextmink_version: String,
    pub(super) skill_target: SkillTarget,
    pub(super) managed_gitignore_block: bool,
    pub(super) managed_gitignore_file: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviousInstallReceipt {
    contextmink_version: String,
    skill_target: SkillTarget,
    #[serde(rename = "schema")]
    _schema: IgnoredAny,
    #[serde(rename = "managed_files")]
    _managed_files: IgnoredAny,
    managed_gitignore_block: bool,
    managed_gitignore_file: bool,
}

pub(super) fn build_install_receipt(
    skill_target: SkillTarget,
    managed_gitignore_block: bool,
    managed_gitignore_file: bool,
) -> InstallReceipt {
    InstallReceipt {
        schema: INSTALL_RECEIPT_SCHEMA.to_owned(),
        contextmink_version: env!("CARGO_PKG_VERSION").to_owned(),
        skill_target,
        managed_gitignore_block,
        managed_gitignore_file,
    }
}

pub(super) fn receipt_bytes(receipt: &InstallReceipt) -> Result<Vec<u8>> {
    let mut bytes =
        serde_json::to_vec_pretty(receipt).context("serialize project-install receipt")?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(super) fn load_install_receipt(path: &Path) -> Result<Option<InstallReceipt>> {
    if !path.exists() {
        return Ok(None);
    }
    if !path.is_file() {
        return Err(anyhow!(
            "project-install receipt is not a file: {}",
            path.display()
        ));
    }
    let bytes = fs::read(path)
        .with_context(|| format!("read project-install receipt {}", path.display()))?;
    let envelope: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse project-install receipt {}", path.display()))?;
    let schema = envelope
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            anyhow!(
                "project-install receipt {} has no string schema; restore a valid receipt or move it aside deliberately",
                path.display()
            )
        })?;
    let receipt = match schema {
        INSTALL_RECEIPT_SCHEMA => serde_json::from_value(envelope).with_context(|| {
            format!(
                "parse {}; restore a valid {} receipt or move it aside deliberately",
                path.display(),
                INSTALL_RECEIPT_SCHEMA
            )
        })?,
        PREVIOUS_INSTALL_RECEIPT_SCHEMA => {
            let previous: PreviousInstallReceipt =
                serde_json::from_value(envelope).with_context(|| {
                    format!(
                        "parse {}; restore a valid {} receipt or move it aside deliberately",
                        path.display(),
                        PREVIOUS_INSTALL_RECEIPT_SCHEMA
                    )
                })?;
            InstallReceipt {
                schema: INSTALL_RECEIPT_SCHEMA.to_owned(),
                contextmink_version: previous.contextmink_version,
                skill_target: previous.skill_target,
                managed_gitignore_block: previous.managed_gitignore_block,
                managed_gitignore_file: previous.managed_gitignore_file,
            }
        }
        _ => {
            return Err(anyhow!(
                "unsupported project-install receipt schema {schema:?} in {}; move it aside deliberately, then rerun setup-project to reinstall",
                path.display()
            ));
        }
    };
    validate_install_receipt(&receipt)?;
    Ok(Some(receipt))
}

/// True when `path` holds the previous release's runtime receipt, which the
/// caller removes. Any other file there is refused rather than deleted.
pub(super) fn previous_runtime_receipt_exists(path: &Path, operation: &str) -> Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error).with_context(|| format!("inspect {}", path.display()));
        }
    };
    let unrecognized = || {
        format!(
            "{operation} does not recognize {} as a Contextmink 0.15 runtime receipt; move it aside, then rerun {operation}",
            path.display()
        )
    };
    if !metadata.is_file() {
        return Err(anyhow!(unrecognized()));
    }
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let envelope: serde_json::Value = serde_json::from_slice(&bytes).with_context(unrecognized)?;
    if envelope.get("schema").and_then(serde_json::Value::as_str)
        != Some(PREVIOUS_RUNTIME_RECEIPT_SCHEMA)
    {
        return Err(anyhow!(unrecognized()));
    }
    Ok(true)
}

pub(super) fn validate_install_receipt(receipt: &InstallReceipt) -> Result<()> {
    if receipt.schema != INSTALL_RECEIPT_SCHEMA {
        return Err(anyhow!(
            "unsupported project-install receipt schema {:?}; move it aside deliberately, then rerun setup-project to reinstall",
            receipt.schema
        ));
    }
    let version = Version::parse(&receipt.contextmink_version).map_err(|_| {
        anyhow!(
            "project-install receipt contextmink_version must be a canonical semantic version, found {:?}",
            receipt.contextmink_version
        )
    })?;
    if version.to_string() != receipt.contextmink_version {
        return Err(anyhow!(
            "project-install receipt contextmink_version must be canonical: expected {version}"
        ));
    }
    if receipt.skill_target == SkillTarget::Auto {
        return Err(anyhow!(
            "project-install receipt skill_target must be resolved, not auto"
        ));
    }

    Ok(())
}

pub(super) fn refuse_release_downgrade(receipt: &InstallReceipt, operation: &str) -> Result<()> {
    let installed = Version::parse(&receipt.contextmink_version)
        .context("parse validated project-install receipt version")?;
    let running = Version::parse(env!("CARGO_PKG_VERSION"))
        .context("parse running Contextmink package version")?;
    if installed > running {
        return Err(anyhow!(
            "{operation} refuses receipt version {installed} with older running Contextmink {running}; use Contextmink {installed} or newer"
        ));
    }
    Ok(())
}

fn canonical_managed_text(content: &[u8]) -> Cow<'_, [u8]> {
    if !content.windows(2).any(|pair| pair == b"\r\n") {
        return Cow::Borrowed(content);
    }
    let mut canonical = Vec::with_capacity(content.len());
    let mut index = 0;
    while index < content.len() {
        if content.get(index..index + 2) == Some(b"\r\n") {
            canonical.push(b'\n');
            index += 2;
        } else {
            canonical.push(content[index]);
            index += 1;
        }
    }
    Cow::Owned(canonical)
}

/// Release-managed text matches when it differs only in CRLF versus LF line
/// endings, so checkouts with converted line endings stay unchanged.
pub(super) fn same_managed_text(left: &[u8], right: &[u8]) -> bool {
    canonical_managed_text(left) == canonical_managed_text(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_text_comparison_is_line_ending_independent() {
        assert!(same_managed_text(b"a\nb\n", b"a\r\nb\r\n"));
        assert!(!same_managed_text(b"a\nb\n", b"a\nc\n"));
    }

    #[test]
    fn receipt_requires_a_resolved_skill_target() {
        let mut receipt = build_install_receipt(SkillTarget::Both, false, false);
        validate_install_receipt(&receipt).unwrap();
        receipt.skill_target = SkillTarget::Auto;
        assert!(validate_install_receipt(&receipt).is_err());
    }

    #[test]
    fn previous_receipt_is_read_as_the_current_schema() {
        let root = std::env::temp_dir().join(format!(
            "contextmink-previous-receipt-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("project-install.json");
        fs::write(
            &path,
            serde_json::json!({
                "schema": PREVIOUS_INSTALL_RECEIPT_SCHEMA,
                "contextmink_version": env!("CARGO_PKG_VERSION"),
                "skill_target": "agents",
                "managed_files": [{"path": "scripts/contextmink", "sha256": "0".repeat(64)}],
                "managed_gitignore_block": true,
                "managed_gitignore_file": false,
            })
            .to_string(),
        )
        .unwrap();
        let receipt = load_install_receipt(&path).unwrap().unwrap();
        assert_eq!(receipt.schema, INSTALL_RECEIPT_SCHEMA);
        assert_eq!(receipt.skill_target, SkillTarget::Agents);
        assert!(receipt.managed_gitignore_block);

        fs::write(
            &path,
            serde_json::json!({"schema": "contextmink.project_install.v1"}).to_string(),
        )
        .unwrap();
        let error = load_install_receipt(&path).unwrap_err().to_string();
        assert!(
            error.contains("rerun setup-project to reinstall"),
            "{error}"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn previous_runtime_receipt_is_recognized_and_other_files_are_refused() {
        let root = std::env::temp_dir().join(format!(
            "contextmink-previous-runtime-receipt-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("runtime-install.json");
        assert!(!previous_runtime_receipt_exists(&path, "setup-project").unwrap());
        fs::write(
            &path,
            serde_json::json!({
                "schema": PREVIOUS_RUNTIME_RECEIPT_SCHEMA,
                "contextmink_version": "0.15.0",
                "managed_files": [
                    {"path": "tools/contextmink/bin/contextmink", "sha256": "1".repeat(64)},
                ],
            })
            .to_string(),
        )
        .unwrap();
        assert!(previous_runtime_receipt_exists(&path, "setup-project").unwrap());

        for unrecognized in [
            serde_json::json!({"schema": "contextmink.runtime_install.v2"}).to_string(),
            serde_json::json!({"contextmink_version": "0.15.0"}).to_string(),
            "not json".to_owned(),
        ] {
            fs::write(&path, unrecognized).unwrap();
            let error = previous_runtime_receipt_exists(&path, "setup-project")
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("move it aside, then rerun setup-project"),
                "{error}"
            );
        }
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let error = previous_runtime_receipt_exists(&path, "uninstall-project")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("move it aside, then rerun uninstall-project"),
            "{error}"
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
