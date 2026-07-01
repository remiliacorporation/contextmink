use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

use crate::config::ContextConfig;

#[derive(Debug)]
pub(crate) struct CollectedFiles {
    pub(crate) files: Vec<PathBuf>,
    pub(crate) total_seen: usize,
    pub(crate) truncated: bool,
}

pub(crate) fn collect_files(
    paths: &[PathBuf],
    globs: &[String],
    extensions: &[String],
    config: &ContextConfig,
    with_excluded: bool,
    with_git_ignored: bool,
    max_scan_files: usize,
) -> Result<CollectedFiles> {
    let include_matcher = build_optional_globset(globs)?;
    let extension_matcher = normalize_extensions(extensions);
    let explicit_excluded_roots = explicit_excluded_roots(paths, config, with_excluded);
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut total_seen = 0usize;
    let mut truncated = false;
    for root in paths {
        if root.is_file() {
            if file_is_included(
                root,
                &include_matcher,
                &extension_matcher,
                config,
                with_excluded,
                &explicit_excluded_roots,
            ) {
                let candidate = root.to_path_buf();
                if !seen.insert(candidate.clone()) {
                    continue;
                }
                total_seen += 1;
                if files.len() < max_scan_files {
                    files.push(candidate);
                } else {
                    truncated = true;
                    break;
                }
            }
            continue;
        }
        let mut walk = WalkBuilder::new(root);
        walk.hidden(false)
            .ignore(!with_git_ignored)
            .git_ignore(!with_git_ignored)
            .git_exclude(!with_git_ignored)
            .parents(!with_git_ignored);
        if !with_excluded {
            let excludes = config.excludes.clone();
            let explicit_roots = explicit_excluded_roots.clone();
            walk.filter_entry(move |entry| {
                let normalized = normalize_path(entry.path());
                if is_under_explicit_excluded_root(&normalized, &explicit_roots) {
                    return true;
                }
                if entry.file_type().is_some_and(|kind| kind.is_dir()) {
                    let normalized = trim_normalized_path(&normalized);
                    if normalized.is_empty() || normalized == "." {
                        return true;
                    }
                    let probe = format!("{normalized}/__contextmink_probe__");
                    !excludes.is_match(&normalized) && !excludes.is_match(&probe)
                } else {
                    !excludes.is_match(&normalized)
                }
            });
        }
        for entry in walk.build() {
            let entry = entry?;
            if entry.file_type().is_some_and(|kind| kind.is_file()) {
                if !file_is_included(
                    entry.path(),
                    &include_matcher,
                    &extension_matcher,
                    config,
                    with_excluded,
                    &explicit_excluded_roots,
                ) {
                    continue;
                }
                let candidate = entry.path().to_path_buf();
                if !seen.insert(candidate.clone()) {
                    continue;
                }
                total_seen += 1;
                if files.len() < max_scan_files {
                    files.push(candidate);
                } else {
                    truncated = true;
                    break;
                }
            }
        }
        if truncated {
            break;
        }
    }
    files.sort();
    files.dedup();
    Ok(CollectedFiles {
        files,
        total_seen,
        truncated,
    })
}

fn explicit_excluded_roots(
    paths: &[PathBuf],
    config: &ContextConfig,
    with_excluded: bool,
) -> Vec<String> {
    if with_excluded {
        return Vec::new();
    }
    paths
        .iter()
        .filter_map(|path| {
            let normalized = trim_normalized_path(&normalize_path(path));
            if normalized.is_empty() || normalized == "." {
                return None;
            }
            let probe = format!("{normalized}/__contextmink_probe__");
            if config.excludes.is_match(&normalized) || config.excludes.is_match(&probe) {
                Some(normalized)
            } else {
                None
            }
        })
        .collect()
}

fn file_is_included(
    path: &Path,
    include_matcher: &Option<GlobSet>,
    extension_matcher: &[String],
    config: &ContextConfig,
    with_excluded: bool,
    explicit_excluded_roots: &[String],
) -> bool {
    let normalized = normalize_path(path);
    if !with_excluded
        && config.excludes.is_match(&normalized)
        && !is_under_explicit_excluded_root(&normalized, explicit_excluded_roots)
    {
        return false;
    }
    if let Some(include_matcher) = include_matcher {
        let basename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !include_matcher.is_match(&normalized) && !include_matcher.is_match(basename) {
            return false;
        }
    }
    if !extension_matcher.is_empty() {
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            return false;
        };
        if !extension_matcher
            .iter()
            .any(|expected| extension.eq_ignore_ascii_case(expected))
        {
            return false;
        }
    }
    true
}

fn is_under_explicit_excluded_root(path: &str, explicit_excluded_roots: &[String]) -> bool {
    let normalized = trim_normalized_path(path);
    explicit_excluded_roots
        .iter()
        .any(|root| normalized == *root || normalized.starts_with(&format!("{root}/")))
}

fn trim_normalized_path(path: &str) -> String {
    path.trim_start_matches("./")
        .trim_end_matches('/')
        .to_string()
}

fn build_optional_globset(globs: &[String]) -> Result<Option<GlobSet>> {
    if globs.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in globs {
        builder.add(Glob::new(pattern).with_context(|| format!("invalid include glob {pattern}"))?);
    }
    Ok(Some(
        builder
            .build()
            .context("failed to build include glob set")?,
    ))
}

fn normalize_extensions(extensions: &[String]) -> Vec<String> {
    extensions
        .iter()
        .filter_map(|extension| {
            let extension = extension.trim().trim_start_matches('.');
            if extension.is_empty() {
                None
            } else {
                Some(extension.to_ascii_lowercase())
            }
        })
        .collect()
}

pub(crate) fn read_text_file(path: &Path, max_file_bytes: u64) -> Result<Option<String>> {
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to stat {}", path.display()))?;
    if metadata.len() > max_file_bytes {
        return Ok(None);
    }
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.contains(&0) {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

fn normalize_path(path: &Path) -> String {
    let path = path.strip_prefix(".").unwrap_or(path);
    path.to_string_lossy().replace('\\', "/")
}

pub(crate) fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
