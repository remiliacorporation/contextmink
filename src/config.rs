use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

const CONFIG_NAME: &str = ".contextmink.toml";
const BUILTIN_EXCLUDES: &[&str] = &[
    ".git",
    "**/.git",
    ".git/**",
    "**/.git/**",
    "target/**",
    "**/target/**",
    "node_modules/**",
    "**/node_modules/**",
    ".venv/**",
    "**/.venv/**",
];

const PLACEHOLDER_PROFILE: &str = "replace-with-workspace-name";
const ACCEPTED_CONFIG_KEYS: &str = "profile, exclude_globs, \
destructive_guard_recursive_delete_fragments, destructive_guard_delete_fragments";

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ContextminkConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exclude_globs: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    destructive_guard_recursive_delete_fragments: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    destructive_guard_delete_fragments: Option<Vec<String>>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct DestructiveGuardConfig {
    pub(crate) recursive_delete_fragments: Vec<String>,
    pub(crate) delete_fragments: Vec<String>,
}

#[allow(dead_code)]
pub(crate) struct ContextConfig {
    pub(crate) profile: Option<String>,
    pub(crate) excludes: GlobSet,
    /// Canonical, `/`-normalized directory of the loaded config file.
    /// Exclude globs are matched against paths relative to this root, so
    /// policy holds even when scan roots are absolute or `..`-relative.
    pub(crate) policy_root: Option<String>,
    pub(crate) destructive_guard: DestructiveGuardConfig,
}

pub(crate) fn load_context_config(
    config_path: Option<&Path>,
    no_config: bool,
) -> Result<ContextConfig> {
    let mut raw = ContextminkConfig::default();
    let mut policy_root = None;
    if !no_config {
        let discovered_config = find_config_path()?;
        let selected_config = config_path.map(Path::to_path_buf).or(discovered_config);
        if let Some(path) = selected_config.as_deref() {
            let text = fs::read_to_string(path)
                .with_context(|| format!("failed to read config {}", path.display()))?;
            raw = parse_config(&text)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            policy_root = path.parent().and_then(canonical_normalized);
        }
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in BUILTIN_EXCLUDES {
        builder.add(Glob::new(pattern).with_context(|| format!("invalid builtin glob {pattern}"))?);
    }
    if let Some(excludes) = &raw.exclude_globs {
        for pattern in excludes {
            builder.add(
                Glob::new(pattern).with_context(|| format!("invalid exclude glob {pattern}"))?,
            );
        }
    }
    Ok(ContextConfig {
        profile: raw.profile,
        excludes: builder
            .build()
            .context("failed to build exclude glob set")?,
        policy_root,
        destructive_guard: DestructiveGuardConfig {
            recursive_delete_fragments: raw
                .destructive_guard_recursive_delete_fragments
                .unwrap_or_default(),
            delete_fragments: raw.destructive_guard_delete_fragments.unwrap_or_default(),
        },
    })
}

/// Canonicalize a path and render it `/`-normalized without the Windows
/// verbatim (`\\?\`) prefix. Returns `None` when the path cannot be
/// canonicalized (nonexistent roots fall back to verbatim path matching).
pub(crate) fn canonical_normalized(path: &Path) -> Option<String> {
    match fs::canonicalize(path) {
        Ok(canonical) => {
            let text = canonical.to_string_lossy().replace('\\', "/");
            Some(
                text.strip_prefix("//?/")
                    .unwrap_or(&text)
                    .trim_end_matches('/')
                    .to_owned(),
            )
        }
        Err(_) => None,
    }
}

/// Strict TOML parser for the small `.contextmink.toml` surface. Serde rejects
/// unknown and duplicate fields, while the real TOML parser preserves standard
/// escaping, multiline arrays, and comment behavior.
fn parse_config(text: &str) -> Result<ContextminkConfig> {
    let config: ContextminkConfig = toml::from_str(text).map_err(|error| {
        let detail = error.to_string();
        let classification = detail
            .split_once("unknown field `")
            .and_then(|(_, tail)| tail.split_once('`'))
            .map(|(field, _)| format!("unknown key `{field}`; "))
            .or_else(|| {
                detail
                    .contains("duplicate key")
                    .then(|| error.span())
                    .flatten()
                    .and_then(|span| config_key_at_offset(text, span.start))
                    .map(|key| format!("duplicate key `{key}`; "))
            })
            .or_else(|| detail.contains("in array").then(|| "invalid array; ".to_string()))
            .unwrap_or_default();
        anyhow!(
            "invalid contextmink TOML: {classification}{detail}; accepted top-level keys: {ACCEPTED_CONFIG_KEYS}"
        )
    })?;
    if let Some(profile) = config.profile.as_deref() {
        validate_profile(profile)?;
    }
    Ok(config)
}

fn config_key_at_offset(text: &str, offset: usize) -> Option<&str> {
    let line_start = text.get(..offset)?.rfind('\n').map_or(0, |index| index + 1);
    let line_end = text
        .get(offset..)?
        .find('\n')
        .map_or(text.len(), |index| offset + index);
    let key = text.get(line_start..line_end)?.split_once('=')?.0.trim();
    (!key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
    .then_some(key)
}

fn validate_profile(profile: &str) -> Result<()> {
    let profile = profile.trim();
    if profile.is_empty() {
        return Err(anyhow!("contextmink profile must not be empty"));
    }
    if profile.eq_ignore_ascii_case(PLACEHOLDER_PROFILE) {
        return Err(anyhow!(
            "contextmink profile is still the template placeholder {PLACEHOLDER_PROFILE:?}; replace it with the workspace name"
        ));
    }
    Ok(())
}

pub(crate) fn find_config_path() -> Result<Option<PathBuf>> {
    let current =
        std::env::current_dir().context("resolve cwd for contextmink config discovery")?;
    Ok(crate::process_boundary::find_ancestor_file(
        &current,
        CONFIG_NAME,
    ))
}

#[path = "project_setup.rs"]
#[allow(dead_code)] // The bridge shares config.rs but does not expose project setup.
pub(crate) mod project_setup;

#[cfg(test)]
#[path = "config/tests.rs"]
mod tests;
