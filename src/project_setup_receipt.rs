//! Persisted ownership for project-local Contextmink integration files.

use std::borrow::Cow;
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path};

use anyhow::{Context, Result, anyhow};
use semver::Version;
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};

use super::{ManagedFile, SetupFileOwnership, SkillTarget, normalized_path};

pub(super) const INSTALL_RECEIPT_PATH: &str = "tools/contextmink/project-install.json";
pub(super) const INSTALL_RECEIPT_SCHEMA: &str = "contextmink.project_install.v3";
/// Read once so an upgrade can rewrite it as the current schema.
const PREVIOUS_INSTALL_RECEIPT_SCHEMA: &str = "contextmink.project_install.v2";
pub(super) const RUNTIME_RECEIPT_PATH: &str = "tools/contextmink/bin/runtime-install.json";
pub(super) const RUNTIME_RECEIPT_SCHEMA: &str = "contextmink.runtime_install.v2";
/// Read once so the next setup can rewrite it as the current schema.
const PREVIOUS_RUNTIME_RECEIPT_SCHEMA: &str = "contextmink.runtime_install.v1";
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

/// Host-local runtime ownership: the binary paths this checkout installed.
/// Ownership is by path; nothing checks a project binary's content before it
/// runs, so the receipt records no content identity. Listing paths lets a
/// checkout shared between hosts keep the other platform's binary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimeInstallReceipt {
    pub(super) schema: String,
    pub(super) contextmink_version: String,
    pub(super) managed_paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviousRuntimeInstallReceipt {
    #[serde(rename = "schema")]
    _schema: IgnoredAny,
    contextmink_version: String,
    managed_files: Vec<PreviousManagedFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviousManagedFile {
    path: String,
    #[serde(rename = "sha256")]
    _sha256: IgnoredAny,
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

pub(super) fn build_runtime_receipt(
    managed: &[ManagedFile],
    retained: Vec<String>,
) -> RuntimeInstallReceipt {
    let mut managed_paths = managed
        .iter()
        .filter(|file| file.ownership == SetupFileOwnership::ReleaseManagedRuntime)
        .map(|file| normalized_path(&file.relative_path))
        .collect::<Vec<_>>();
    managed_paths.extend(retained);
    managed_paths.sort();
    RuntimeInstallReceipt {
        schema: RUNTIME_RECEIPT_SCHEMA.to_owned(),
        contextmink_version: env!("CARGO_PKG_VERSION").to_owned(),
        managed_paths,
    }
}

pub(super) fn receipt_bytes(receipt: &InstallReceipt) -> Result<Vec<u8>> {
    let mut bytes =
        serde_json::to_vec_pretty(receipt).context("serialize project-install receipt")?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(super) fn runtime_receipt_bytes(receipt: &RuntimeInstallReceipt) -> Result<Vec<u8>> {
    let mut bytes =
        serde_json::to_vec_pretty(receipt).context("serialize runtime-install receipt")?;
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

pub(super) fn load_runtime_receipt(path: &Path) -> Result<Option<RuntimeInstallReceipt>> {
    if !path.exists() {
        return Ok(None);
    }
    if !path.is_file() {
        return Err(anyhow!(
            "runtime-install receipt is not a file: {}",
            path.display()
        ));
    }
    let bytes = fs::read(path)
        .with_context(|| format!("read runtime-install receipt {}", path.display()))?;
    let reinstall = || {
        format!(
            "parse runtime-install receipt {}; move it aside, then rerun setup-project to reinstall",
            path.display()
        )
    };
    let envelope: serde_json::Value = serde_json::from_slice(&bytes).with_context(reinstall)?;
    let schema = envelope
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let receipt = match schema {
        RUNTIME_RECEIPT_SCHEMA => serde_json::from_value(envelope).with_context(reinstall)?,
        PREVIOUS_RUNTIME_RECEIPT_SCHEMA => {
            let previous: PreviousRuntimeInstallReceipt =
                serde_json::from_value(envelope).with_context(reinstall)?;
            RuntimeInstallReceipt {
                schema: RUNTIME_RECEIPT_SCHEMA.to_owned(),
                contextmink_version: previous.contextmink_version,
                managed_paths: previous
                    .managed_files
                    .into_iter()
                    .map(|file| file.path)
                    .collect(),
            }
        }
        _ => {
            return Err(anyhow!(
                "unsupported runtime-install receipt schema {schema:?} in {}; move it aside, then rerun setup-project to reinstall",
                path.display()
            ));
        }
    };
    validate_runtime_receipt(&receipt)?;
    Ok(Some(receipt))
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

pub(super) fn validate_runtime_receipt(receipt: &RuntimeInstallReceipt) -> Result<()> {
    if receipt.schema != RUNTIME_RECEIPT_SCHEMA {
        return Err(anyhow!(
            "unsupported runtime-install receipt schema {:?}; move it aside, then rerun setup-project to reinstall",
            receipt.schema
        ));
    }
    let version = Version::parse(&receipt.contextmink_version).map_err(|_| {
        anyhow!(
            "runtime-install receipt contextmink_version must be a canonical semantic version, found {:?}",
            receipt.contextmink_version
        )
    })?;
    if version.to_string() != receipt.contextmink_version {
        return Err(anyhow!(
            "runtime-install receipt contextmink_version must be canonical: expected {version}"
        ));
    }
    let mut paths = HashSet::new();
    for path in &receipt.managed_paths {
        validate_managed_runtime_path(Path::new(path))?;
        if normalized_path(Path::new(path)) != *path {
            return Err(anyhow!(
                "runtime-install receipt managed path must be canonical: {path}"
            ));
        }
        if !paths.insert(path.as_str()) {
            return Err(anyhow!(
                "runtime-install receipt repeats managed path {path}"
            ));
        }
    }
    Ok(())
}

pub(super) fn refuse_release_downgrade(receipt: &InstallReceipt, operation: &str) -> Result<()> {
    refuse_version_downgrade(&receipt.contextmink_version, operation, "project-install")
}

pub(super) fn refuse_runtime_release_downgrade(
    receipt: &RuntimeInstallReceipt,
    operation: &str,
) -> Result<()> {
    refuse_version_downgrade(&receipt.contextmink_version, operation, "runtime-install")
}

fn refuse_version_downgrade(version: &str, operation: &str, receipt: &str) -> Result<()> {
    let installed = Version::parse(version)
        .with_context(|| format!("parse validated {receipt} receipt version"))?;
    let running = Version::parse(env!("CARGO_PKG_VERSION"))
        .context("parse running Contextmink package version")?;
    if installed > running {
        return Err(anyhow!(
            "{operation} refuses receipt version {installed} with older running Contextmink {running}; use Contextmink {installed} or newer"
        ));
    }
    Ok(())
}

pub(super) fn validate_managed_runtime_path(path: &Path) -> Result<()> {
    validate_relative_path(path, "runtime")?;
    let normalized = normalized_path(path);
    if !MANAGED_RUNTIME_PATHS.contains(&normalized.as_str()) {
        return Err(anyhow!(
            "runtime ownership receipt names unsupported managed path {normalized}; this release will not modify or remove it"
        ));
    }
    Ok(())
}

fn validate_relative_path(path: &Path, kind: &str) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(anyhow!(
            "ownership receipt {kind} path must be a normalized project-relative path: {}",
            path.display()
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
    fn runtime_receipt_rejects_unowned_paths() {
        let receipt = RuntimeInstallReceipt {
            schema: RUNTIME_RECEIPT_SCHEMA.to_owned(),
            contextmink_version: env!("CARGO_PKG_VERSION").to_owned(),
            managed_paths: vec!["tools/contextmink/bin/foreign".to_owned()],
        };
        let error = validate_runtime_receipt(&receipt).unwrap_err().to_string();
        assert!(error.contains("unsupported managed path"), "{error}");
    }

    #[test]
    fn previous_runtime_receipt_is_read_as_owned_paths() {
        let root = std::env::temp_dir().join(format!(
            "contextmink-previous-runtime-receipt-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("runtime-install.json");
        fs::write(
            &path,
            serde_json::json!({
                "schema": PREVIOUS_RUNTIME_RECEIPT_SCHEMA,
                "contextmink_version": env!("CARGO_PKG_VERSION"),
                "managed_files": [
                    {"path": "tools/contextmink/bin/contextmink.exe", "sha256": "0".repeat(64)},
                    {"path": "tools/contextmink/bin/contextmink", "sha256": "1".repeat(64)},
                ],
            })
            .to_string(),
        )
        .unwrap();
        let receipt = load_runtime_receipt(&path).unwrap().unwrap();
        assert_eq!(receipt.schema, RUNTIME_RECEIPT_SCHEMA);
        assert_eq!(
            receipt.managed_paths,
            [
                "tools/contextmink/bin/contextmink.exe",
                "tools/contextmink/bin/contextmink"
            ]
        );

        for unsupported in [
            serde_json::json!({"schema": "contextmink.runtime_install.v0"}),
            serde_json::json!({"contextmink_version": env!("CARGO_PKG_VERSION")}),
        ] {
            fs::write(&path, unsupported.to_string()).unwrap();
            let error = load_runtime_receipt(&path).unwrap_err().to_string();
            assert!(
                error.contains("move it aside, then rerun setup-project to reinstall"),
                "{error}"
            );
        }
        fs::remove_dir_all(&root).unwrap();
    }
}
