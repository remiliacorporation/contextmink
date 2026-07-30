use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use globset::{Glob, GlobSetBuilder};

use super::{CollectOptions, collect_files, display_path};
use crate::config::{ContextConfig, DestructiveGuardConfig, canonical_normalized};

static FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("files-tests")
            .join(format!("{name}-{}-{id}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).expect("stale fixture must be removable");
        }
        fs::create_dir_all(&root).expect("fixture root must be creatable");
        Self { root }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.root.exists() {
            fs::remove_dir_all(&self.root).expect("fixture root must be removable");
        }
    }
}

fn config(root: &Path, patterns: &[&str]) -> ContextConfig {
    let mut excludes = GlobSetBuilder::new();
    for pattern in patterns {
        excludes.add(Glob::new(pattern).expect("test exclude glob must compile"));
    }
    ContextConfig {
        profile: Some("files-tests".to_owned()),
        excludes: excludes.build().expect("test exclude set must compile"),
        policy_root: canonical_normalized(root),
        destructive_guard: DestructiveGuardConfig::default(),
    }
}

fn options<'a>(globs: &'a [String]) -> CollectOptions<'a> {
    CollectOptions {
        globs,
        path_terms: &[],
        extensions: &[],
        with_excluded: false,
        with_git_ignored: false,
        skip_nested_repos: true,
        max_selected_files: usize::MAX,
    }
}

#[test]
fn include_globs_use_scan_root_relative_paths_for_absolute_roots() {
    let fixture = Fixture::new("absolute-glob");
    let source = fixture.root.join("src").join("nested");
    fs::create_dir_all(&source).expect("source tree must be creatable");
    fs::write(source.join("lib.rs"), "pub fn retained() {}\n")
        .expect("matching source must be writable");
    fs::write(fixture.root.join("root.rs"), "pub fn skipped() {}\n")
        .expect("nonmatching source must be writable");

    let globs = vec!["src/**/*.rs".to_owned()];
    let collected = collect_files(
        std::slice::from_ref(&fixture.root),
        &config(&fixture.root, &[]),
        &options(&globs),
    )
    .expect("absolute-root collection must succeed");

    assert_eq!(collected.total_seen, 1);
    assert_eq!(collected.files, vec![source.join("lib.rs")]);
}

#[test]
fn nested_repo_include_globs_remain_relative_to_the_explicit_scan_root() {
    let fixture = Fixture::new("nested-repo-glob-root");
    fs::create_dir_all(fixture.root.join(".git")).expect("outer repo marker must be creatable");
    fs::write(fixture.root.join(".gitignore"), "nested/\n")
        .expect("outer ignore file must be writable");
    let nested = fixture.root.join("nested");
    let source = nested.join("src");
    fs::create_dir_all(nested.join(".git")).expect("nested repo marker must be creatable");
    fs::create_dir_all(&source).expect("nested source tree must be creatable");
    fs::write(source.join("lib.rs"), "pub fn retained() {}\n")
        .expect("nested source must be writable");

    let globs = vec!["nested/src/**/*.rs".to_owned()];
    let mut options = options(&globs);
    options.skip_nested_repos = false;
    let collected = collect_files(
        std::slice::from_ref(&fixture.root),
        &config(&fixture.root, &[]),
        &options,
    )
    .expect("nested-repo collection must succeed");

    assert_eq!(collected.total_seen, 1);
    assert_eq!(collected.files, vec![source.join("lib.rs")]);
    assert_eq!(collected.nested_repos_entered, vec![display_path(&nested)]);
}

#[test]
fn tracked_submodule_style_repo_is_disclosed_and_skipped() {
    let fixture = Fixture::new("tracked-submodule-boundary");
    fs::create_dir_all(fixture.root.join(".git")).expect("outer repo marker must be creatable");
    let nested = fixture.root.join("nested");
    let source = nested.join("src");
    fs::create_dir_all(&source).expect("nested source tree must be creatable");
    fs::write(nested.join(".git"), "gitdir: ../.git/modules/nested\n")
        .expect("submodule git marker must be writable");
    fs::write(source.join("lib.rs"), "pub fn retained() {}\n")
        .expect("nested source must be writable");

    let mut enter_options = options(&[]);
    enter_options.skip_nested_repos = false;
    let entered = collect_files(
        std::slice::from_ref(&fixture.root),
        &config(&fixture.root, &[]),
        &enter_options,
    )
    .expect("tracked nested repo must be enterable");
    assert_eq!(entered.files, vec![source.join("lib.rs")]);
    assert_eq!(entered.total_seen, 1);
    assert_eq!(entered.nested_repos_entered, vec![display_path(&nested)]);

    let skipped = collect_files(
        std::slice::from_ref(&fixture.root),
        &config(&fixture.root, &[]),
        &options(&[]),
    )
    .expect("tracked nested repo must be skippable");
    assert_eq!(skipped.total_seen, 0);
    assert!(skipped.files.is_empty());
    assert!(skipped.nested_repos_entered.is_empty());
}

#[test]
fn explicit_repo_root_is_not_treated_as_its_own_nested_boundary() {
    let fixture = Fixture::new("explicit-repo-root");
    let nested = fixture.root.join("nested");
    fs::create_dir_all(&nested).expect("explicit repo root must be creatable");
    fs::write(nested.join(".git"), "gitdir: ../.git/modules/nested\n")
        .expect("submodule git marker must be writable");
    fs::write(nested.join("README.md"), "explicit root\n")
        .expect("explicit-root file must be writable");

    let collected = collect_files(
        std::slice::from_ref(&nested),
        &config(&fixture.root, &[]),
        &options(&[]),
    )
    .expect("explicit nested root must remain scannable");
    assert_eq!(collected.files, vec![nested.join("README.md")]);
    assert_eq!(collected.total_seen, 1);
    assert!(collected.nested_repos_entered.is_empty());
}

#[test]
fn explicit_excluded_root_reapplies_nested_exclusions() {
    let fixture = Fixture::new("explicit-exclude-boundary");
    let artifacts = fixture.root.join("artifacts");
    fs::create_dir_all(artifacts.join("node_modules").join("dependency"))
        .expect("nested dependency tree must be creatable");
    fs::write(artifacts.join("finding.md"), "retained\n")
        .expect("explicit-root file must be writable");
    fs::write(
        artifacts
            .join("node_modules")
            .join("dependency")
            .join("generated.js"),
        "excluded\n",
    )
    .expect("nested excluded file must be writable");

    let config = config(
        &fixture.root,
        &["artifacts/**", "node_modules/**", "**/node_modules/**"],
    );
    let collected = collect_files(std::slice::from_ref(&artifacts), &config, &options(&[]))
        .expect("explicit excluded-root collection must succeed");

    assert_eq!(collected.total_seen, 1);
    assert_eq!(collected.files, vec![artifacts.join("finding.md")]);
}

#[test]
fn overlapping_roots_keep_exact_deduplicated_totals() {
    let fixture = Fixture::new("overlapping-roots");
    let source = fixture.root.join("src");
    fs::create_dir_all(&source).expect("source tree must be creatable");
    fs::write(source.join("lib.rs"), "pub fn once() {}\n").expect("source must be writable");

    let roots = vec![fixture.root.clone(), source.clone()];
    let collected = collect_files(&roots, &config(&fixture.root, &[]), &options(&[]))
        .expect("overlapping-root collection must succeed");

    assert_eq!(collected.total_seen, 1);
    assert_eq!(collected.files, vec![source.join("lib.rs")]);
}

#[test]
fn physical_file_aliases_keep_one_deterministic_path() {
    let fixture = Fixture::new("physical-aliases");
    let first = fixture.root.join("a.json");
    let alias = fixture.root.join("b.json");
    fs::write(&first, "{}\n").expect("source must be writable");
    fs::hard_link(&first, &alias).expect("hard-link fixture must be creatable");

    let collected = collect_files(
        &[alias, first.clone()],
        &config(&fixture.root, &[]),
        &options(&[]),
    )
    .expect("physical aliases must be collected");

    assert_eq!(collected.total_seen, 1);
    assert_eq!(collected.files, vec![first]);
}

#[cfg(unix)]
#[test]
fn posix_backslashes_remain_filename_characters() {
    assert_eq!(
        display_path(Path::new(r"queue/name\part.jsonl")),
        r"queue/name\part.jsonl"
    );
}

#[cfg(windows)]
#[test]
fn windows_backslashes_render_as_path_separators() {
    assert_eq!(
        display_path(Path::new(r"queue\work.jsonl")),
        "queue/work.jsonl"
    );
}
