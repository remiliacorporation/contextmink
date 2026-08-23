//! Persisted ownership for project-local Contextmink integration files.

use std::borrow::Cow;
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path};

use anyhow::{Context, Result, anyhow};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ManagedFile, SetupFileOwnership, SkillTarget, normalized_path};

pub(super) const INSTALL_RECEIPT_PATH: &str = "tools/contextmink/project-install.json";
pub(super) const INSTALL_RECEIPT_SCHEMA: &str = "contextmink.project_install.v2";
const LEGACY_INSTALL_RECEIPT_SCHEMA: &str = "contextmink.project_install.v1";
pub(super) const RUNTIME_RECEIPT_PATH: &str = "tools/contextmink/bin/runtime-install.json";
pub(super) const RUNTIME_RECEIPT_SCHEMA: &str = "contextmink.runtime_install.v1";
pub(super) const MANAGED_RUNTIME_PATHS: &[&str] = &[
    "tools/contextmink/bin/contextmink",
    "tools/contextmink/bin/contextmink.exe",
    "tools/contextmink/bin/contextmink-bridge.exe",
];

const SUPPORTED_MANAGED_TEXT_PATHS: &[&str] = &[
    "scripts/contextmink",
    "scripts/contextmink.cmd",
    "tools/contextmink/agent_integration.md",
    ".agents/skills/contextmink/SKILL.md",
    ".agents/skills/contextmink/agents/openai.yaml",
    ".claude/skills/contextmink/SKILL.md",
    // Kept in the allowlist only so a hash-bound receipt from a prerelease
    // installer can retire the general-purpose skill without guessing that an
    // unreceipted repository-local copy belongs to Contextmink.
    ".agents/skills/changelog-writing/SKILL.md",
    ".agents/skills/changelog-writing/agents/openai.yaml",
    ".claude/skills/changelog-writing/SKILL.md",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InstallReceipt {
    pub(super) schema: String,
    pub(super) contextmink_version: String,
    pub(super) skill_target: SkillTarget,
    pub(super) managed_files: Vec<ManagedFileReceipt>,
    pub(super) managed_gitignore_block: bool,
    pub(super) managed_gitignore_file: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyInstallReceipt {
    schema: String,
    contextmink_version: String,
    managed_files: Vec<ManagedFileReceipt>,
    managed_runtime_paths: Vec<String>,
    managed_gitignore_block: bool,
    managed_gitignore_file: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimeInstallReceipt {
    pub(super) schema: String,
    pub(super) contextmink_version: String,
    pub(super) managed_files: Vec<ManagedFileReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ManagedFileReceipt {
    pub(super) path: String,
    pub(super) sha256: String,
}

pub(super) fn build_install_receipt(
    managed: &[ManagedFile],
    skill_target: SkillTarget,
    managed_gitignore_block: bool,
    managed_gitignore_file: bool,
) -> InstallReceipt {
    InstallReceipt {
        schema: INSTALL_RECEIPT_SCHEMA.to_owned(),
        contextmink_version: env!("CARGO_PKG_VERSION").to_owned(),
        skill_target,
        managed_files: managed
            .iter()
            .filter(|file| file.ownership == SetupFileOwnership::ReleaseManagedText)
            .map(|file| ManagedFileReceipt {
                path: normalized_path(&file.relative_path),
                sha256: managed_text_sha256(&file.content),
            })
            .collect(),
        managed_gitignore_block,
        managed_gitignore_file,
    }
}

pub(super) fn build_runtime_receipt(
    managed: &[ManagedFile],
    retained: Vec<ManagedFileReceipt>,
) -> RuntimeInstallReceipt {
    let mut managed_files = managed
        .iter()
        .filter(|file| file.ownership == SetupFileOwnership::ReleaseManagedRuntime)
        .map(|file| ManagedFileReceipt {
            path: normalized_path(&file.relative_path),
            sha256: managed_runtime_sha256(&file.content),
        })
        .collect::<Vec<_>>();
    managed_files.extend(retained);
    managed_files.sort_by(|left, right| left.path.cmp(&right.path));
    RuntimeInstallReceipt {
        schema: RUNTIME_RECEIPT_SCHEMA.to_owned(),
        contextmink_version: env!("CARGO_PKG_VERSION").to_owned(),
        managed_files,
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
        LEGACY_INSTALL_RECEIPT_SCHEMA => {
            let legacy: LegacyInstallReceipt =
                serde_json::from_value(envelope).with_context(|| {
                    format!(
                        "parse legacy {}; restore a valid {} receipt or move it aside deliberately",
                        path.display(),
                        LEGACY_INSTALL_RECEIPT_SCHEMA
                    )
                })?;
            migrate_legacy_install_receipt(legacy)?
        }
        _ => {
            return Err(anyhow!(
                "unsupported project-install receipt schema {schema:?}; use a Contextmink release that owns it or migrate it deliberately"
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
    let receipt: RuntimeInstallReceipt = serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "parse {}; restore a valid {} receipt or move it aside deliberately",
            path.display(),
            RUNTIME_RECEIPT_SCHEMA
        )
    })?;
    validate_runtime_receipt(&receipt)?;
    Ok(Some(receipt))
}

fn migrate_legacy_install_receipt(legacy: LegacyInstallReceipt) -> Result<InstallReceipt> {
    if legacy.schema != LEGACY_INSTALL_RECEIPT_SCHEMA {
        return Err(anyhow!(
            "legacy project-install receipt schema mismatch: {:?}",
            legacy.schema
        ));
    }
    Version::parse(&legacy.contextmink_version).map_err(|_| {
        anyhow!(
            "project-install receipt contextmink_version must be a canonical semantic version, found {:?}",
            legacy.contextmink_version
        )
    })?;
    for path in &legacy.managed_runtime_paths {
        validate_managed_runtime_path(Path::new(path))?;
    }
    let agents = legacy.managed_files.iter().any(|file| {
        file.path == ".agents/skills/contextmink/SKILL.md"
            || file.path == ".agents/skills/contextmink/agents/openai.yaml"
    });
    let claude = legacy
        .managed_files
        .iter()
        .any(|file| file.path == ".claude/skills/contextmink/SKILL.md");
    let skill_target = match (agents, claude) {
        (true, true) => SkillTarget::Both,
        (true, false) => SkillTarget::Agents,
        (false, true) => SkillTarget::Claude,
        (false, false) => SkillTarget::None,
    };
    Ok(InstallReceipt {
        schema: INSTALL_RECEIPT_SCHEMA.to_owned(),
        contextmink_version: legacy.contextmink_version,
        skill_target,
        managed_files: legacy.managed_files,
        managed_gitignore_block: legacy.managed_gitignore_block,
        managed_gitignore_file: legacy.managed_gitignore_file,
    })
}

pub(super) fn validate_install_receipt(receipt: &InstallReceipt) -> Result<()> {
    if receipt.schema != INSTALL_RECEIPT_SCHEMA {
        return Err(anyhow!(
            "unsupported project-install receipt schema {:?}; use a Contextmink release that owns it or migrate it deliberately",
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

    let mut paths = HashSet::new();
    for file in &receipt.managed_files {
        validate_managed_text_path(Path::new(&file.path))?;
        if normalized_path(Path::new(&file.path)) != file.path {
            return Err(anyhow!(
                "project-install receipt managed path must be canonical: {}",
                file.path
            ));
        }
        if !paths.insert(file.path.as_str()) {
            return Err(anyhow!(
                "project-install receipt repeats managed path {}",
                file.path
            ));
        }
        validate_sha256(&file.sha256, "project-install")?;
    }

    Ok(())
}

pub(super) fn validate_runtime_receipt(receipt: &RuntimeInstallReceipt) -> Result<()> {
    if receipt.schema != RUNTIME_RECEIPT_SCHEMA {
        return Err(anyhow!(
            "unsupported runtime-install receipt schema {:?}; use a Contextmink release that owns it or move it aside deliberately",
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
    for file in &receipt.managed_files {
        validate_managed_runtime_path(Path::new(&file.path))?;
        if normalized_path(Path::new(&file.path)) != file.path {
            return Err(anyhow!(
                "runtime-install receipt managed path must be canonical: {}",
                file.path
            ));
        }
        if !paths.insert(file.path.as_str()) {
            return Err(anyhow!(
                "runtime-install receipt repeats managed path {}",
                file.path
            ));
        }
        validate_sha256(&file.sha256, "runtime-install")?;
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

pub(super) fn validate_managed_text_path(path: &Path) -> Result<()> {
    validate_relative_path(path, "managed")?;
    let normalized = normalized_path(path);
    if !SUPPORTED_MANAGED_TEXT_PATHS.contains(&normalized.as_str()) {
        return Err(anyhow!(
            "project-install receipt names unsupported managed path {normalized}; this release will not modify or remove it"
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

pub(super) fn canonical_managed_text(content: &[u8]) -> Cow<'_, [u8]> {
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

pub(super) fn managed_text_sha256(content: &[u8]) -> String {
    let digest = Sha256::digest(canonical_managed_text(content).as_ref());
    format!("{digest:x}")
}

pub(super) fn managed_runtime_sha256(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

fn validate_sha256(value: &str, receipt: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(anyhow!(
            "{receipt} managed file sha256 must be 64 lowercase hexadecimal characters, found {value:?}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_text_hash_is_line_ending_independent() {
        assert_eq!(
            managed_text_sha256(b"a\nb\n"),
            managed_text_sha256(b"a\r\nb\r\n")
        );
    }

    #[test]
    fn receipt_rejects_paths_outside_the_owned_allowlist() {
        let mut receipt = InstallReceipt {
            schema: INSTALL_RECEIPT_SCHEMA.to_owned(),
            contextmink_version: env!("CARGO_PKG_VERSION").to_owned(),
            skill_target: SkillTarget::Both,
            managed_files: vec![ManagedFileReceipt {
                path: "AGENTS.md".to_owned(),
                sha256: "0".repeat(64),
            }],
            managed_gitignore_block: false,
            managed_gitignore_file: false,
        };
        assert!(validate_install_receipt(&receipt).is_err());
        receipt.managed_files[0].path = "../outside".to_owned();
        assert!(validate_install_receipt(&receipt).is_err());
    }

    #[test]
    fn legacy_receipt_derives_the_frozen_skill_target() {
        let receipt = migrate_legacy_install_receipt(LegacyInstallReceipt {
            schema: LEGACY_INSTALL_RECEIPT_SCHEMA.to_owned(),
            contextmink_version: env!("CARGO_PKG_VERSION").to_owned(),
            managed_files: vec![
                ManagedFileReceipt {
                    path: ".agents/skills/contextmink/SKILL.md".to_owned(),
                    sha256: "0".repeat(64),
                },
                ManagedFileReceipt {
                    path: ".agents/skills/contextmink/agents/openai.yaml".to_owned(),
                    sha256: "1".repeat(64),
                },
                ManagedFileReceipt {
                    path: ".claude/skills/contextmink/SKILL.md".to_owned(),
                    sha256: "2".repeat(64),
                },
            ],
            managed_runtime_paths: MANAGED_RUNTIME_PATHS
                .iter()
                .map(|path| (*path).to_owned())
                .collect(),
            managed_gitignore_block: true,
            managed_gitignore_file: false,
        })
        .unwrap();
        assert_eq!(receipt.schema, INSTALL_RECEIPT_SCHEMA);
        assert_eq!(receipt.skill_target, SkillTarget::Both);
        validate_install_receipt(&receipt).unwrap();
    }

    #[test]
    fn runtime_receipt_rejects_unowned_paths() {
        let receipt = RuntimeInstallReceipt {
            schema: RUNTIME_RECEIPT_SCHEMA.to_owned(),
            contextmink_version: env!("CARGO_PKG_VERSION").to_owned(),
            managed_files: vec![ManagedFileReceipt {
                path: "tools/contextmink/bin/foreign".to_owned(),
                sha256: "0".repeat(64),
            }],
        };
        assert!(validate_runtime_receipt(&receipt).is_err());
    }
}
