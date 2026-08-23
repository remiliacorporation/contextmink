//! Persisted ownership for project-local Contextmink integration files.

use std::borrow::Cow;
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path};

use anyhow::{Context, Result, anyhow};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ManagedFile, SetupFileOwnership, normalized_path};

pub(super) const INSTALL_RECEIPT_PATH: &str = "tools/contextmink/project-install.json";
pub(super) const INSTALL_RECEIPT_SCHEMA: &str = "contextmink.project_install.v1";
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
    pub(super) managed_files: Vec<ManagedFileReceipt>,
    pub(super) managed_runtime_paths: Vec<String>,
    pub(super) managed_gitignore_block: bool,
    pub(super) managed_gitignore_file: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ManagedFileReceipt {
    pub(super) path: String,
    pub(super) sha256: String,
}

pub(super) fn build_install_receipt(
    managed: &[ManagedFile],
    managed_gitignore_block: bool,
    managed_gitignore_file: bool,
) -> InstallReceipt {
    InstallReceipt {
        schema: INSTALL_RECEIPT_SCHEMA.to_owned(),
        contextmink_version: env!("CARGO_PKG_VERSION").to_owned(),
        managed_files: managed
            .iter()
            .filter(|file| file.ownership == SetupFileOwnership::ReleaseManagedText)
            .map(|file| ManagedFileReceipt {
                path: normalized_path(&file.relative_path),
                sha256: managed_text_sha256(&file.content),
            })
            .collect(),
        managed_runtime_paths: MANAGED_RUNTIME_PATHS
            .iter()
            .map(|path| (*path).to_owned())
            .collect(),
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
    let receipt: InstallReceipt = serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "parse {}; restore a valid {} receipt or move it aside deliberately",
            path.display(),
            INSTALL_RECEIPT_SCHEMA
        )
    })?;
    validate_install_receipt(&receipt)?;
    Ok(Some(receipt))
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
        validate_sha256(&file.sha256)?;
    }

    let mut runtime_paths = HashSet::new();
    for path in &receipt.managed_runtime_paths {
        validate_managed_runtime_path(Path::new(path))?;
        if normalized_path(Path::new(path)) != *path {
            return Err(anyhow!(
                "project-install receipt runtime path must be canonical: {path}"
            ));
        }
        if !runtime_paths.insert(path.as_str()) {
            return Err(anyhow!(
                "project-install receipt repeats runtime path {path}"
            ));
        }
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
            "project-install receipt names unsupported runtime path {normalized}; this release will not modify or remove it"
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
            "project-install receipt {kind} path must be a normalized project-relative path: {}",
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

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(anyhow!(
            "project-install managed file sha256 must be 64 lowercase hexadecimal characters, found {value:?}"
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
            managed_files: vec![ManagedFileReceipt {
                path: "AGENTS.md".to_owned(),
                sha256: "0".repeat(64),
            }],
            managed_runtime_paths: MANAGED_RUNTIME_PATHS
                .iter()
                .map(|path| (*path).to_owned())
                .collect(),
            managed_gitignore_block: false,
            managed_gitignore_file: false,
        };
        assert!(validate_install_receipt(&receipt).is_err());
        receipt.managed_files[0].path = "../outside".to_owned();
        assert!(validate_install_receipt(&receipt).is_err());
    }
}
