use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

const CONFIG_NAME: &str = ".contextmink.toml";
const BUILTIN_EXCLUDES: &[&str] = &[
    ".git/**",
    "**/.git/**",
    "target/**",
    "**/target/**",
    "node_modules/**",
    "**/node_modules/**",
    ".venv/**",
    "**/.venv/**",
];

#[derive(Debug, Default, Deserialize)]
struct ContextminkConfig {
    profile: Option<String>,
    exclude_globs: Option<Vec<String>>,
}

pub(crate) struct ContextConfig {
    pub(crate) profile: Option<String>,
    pub(crate) excludes: GlobSet,
}

pub(crate) fn load_context_config(
    config_path: Option<&Path>,
    no_config: bool,
) -> Result<ContextConfig> {
    let mut raw = ContextminkConfig::default();
    if !no_config {
        let discovered_config = find_config_path();
        let selected_config = config_path.map(Path::to_path_buf).or(discovered_config);
        if let Some(path) = selected_config.as_deref() {
            let text = fs::read_to_string(path)
                .with_context(|| format!("failed to read config {}", path.display()))?;
            raw = toml::from_str(&text)
                .with_context(|| format!("failed to parse {}", path.display()))?;
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
    })
}

fn find_config_path() -> Option<PathBuf> {
    let mut current = std::env::current_dir().ok()?;
    loop {
        let candidate = current.join(CONFIG_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}
