use std::borrow::Cow;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

use crate::config::{ContextConfig, canonical_normalized};

#[derive(Debug)]
pub(crate) struct CollectedFiles {
    /// Sorted candidate files, capped at `max_selected_files`.
    pub(crate) files: Vec<PathBuf>,
    /// Exact deduplicated candidate count. Enumeration always completes (the
    /// cap bounds downstream scanning, not the count), so this is never a
    /// lower bound.
    pub(crate) total_seen: usize,
    /// `files` was capped at `max_selected_files`; the surviving subset is the
    /// sorted prefix, so truncation is deterministic and independent of walk
    /// order or root spelling.
    pub(crate) selection_capped: bool,
    /// Nested Git repository roots crossed during the scan, including tracked
    /// submodules and Git-ignored sibling repositories, as display paths.
    pub(crate) nested_repos_entered: Vec<String>,
}

pub(crate) struct CollectOptions<'a> {
    pub(crate) globs: &'a [String],
    pub(crate) path_terms: &'a [String],
    pub(crate) extensions: &'a [String],
    pub(crate) with_excluded: bool,
    pub(crate) with_git_ignored: bool,
    pub(crate) skip_nested_repos: bool,
    pub(crate) max_selected_files: usize,
}

/// How many directory levels below a pruned (git-ignored) directory the
/// nested-repo supplement probes for a `.git` marker. Repos nested deeper
/// under an ignored plain directory must be passed as explicit roots.
const NESTED_REPO_PROBE_DEPTH: usize = 1;

/// Bound on nested-repo recursion (a repo inside a repo inside a repo...).
const NESTED_REPO_MAX_RECURSION: usize = 4;

/// Avoid thread startup for small trees, while moving the directory-heavy
/// supplement off one serial filesystem lane for large workspaces.
const NESTED_REPO_PARALLEL_MIN_DIRS: usize = 64;

/// Bound concurrent directory probes so local SSDs benefit without turning a
/// network-backed workspace into an unbounded metadata storm.
const NESTED_REPO_MAX_PROBE_THREADS: usize = 8;

pub(crate) fn collect_files(
    paths: &[PathBuf],
    config: &ContextConfig,
    options: &CollectOptions<'_>,
) -> Result<CollectedFiles> {
    let include_matcher = build_optional_globset(options.globs)?;
    let path_terms = normalize_path_terms(options.path_terms)?;
    let extension_matcher = normalize_extensions(options.extensions);
    let explicit_excluded_roots = explicit_excluded_roots(paths, config, options.with_excluded);
    let mut state = CollectState {
        files: Vec::new(),
        nested_repos_entered: Vec::new(),
    };
    for root in paths {
        if root.is_file() {
            let mapper = PolicyMapper::for_root(root, config);
            if file_is_included(
                root,
                &mapper,
                &include_matcher,
                &path_terms,
                &extension_matcher,
                config,
                options.with_excluded,
                &explicit_excluded_roots,
            ) {
                state.files.push(root.to_path_buf());
            }
            continue;
        }
        let mapper = PolicyMapper::for_root(root, config);
        walk_root(
            root,
            &mapper,
            config,
            options,
            &include_matcher,
            &path_terms,
            &extension_matcher,
            &explicit_excluded_roots,
            &mut state,
            0,
        )?;
    }
    // Enumeration runs to completion before the cap applies: totals stay
    // exact and a capped list is the sorted prefix, deterministic no matter
    // how walk order interleaved.
    state.files.sort();
    let mut identities = HashSet::new();
    let mut deduplicated = Vec::with_capacity(state.files.len());
    for path in state.files {
        if identities.insert(file_identity(&path)?) {
            deduplicated.push(path);
        }
    }
    state.files = deduplicated;
    state.nested_repos_entered.sort();
    state.nested_repos_entered.dedup();
    let total_seen = state.files.len();
    let selection_capped = total_seen > options.max_selected_files;
    if selection_capped {
        state.files.truncate(options.max_selected_files);
    }
    Ok(CollectedFiles {
        files: state.files,
        total_seen,
        selection_capped,
        nested_repos_entered: state.nested_repos_entered,
    })
}

/// Keeps selection paths and exclusion-policy paths in their distinct spaces.
/// Include globs use `scan_path`, which is relative to the explicit scan root.
/// Exclude globs use `policy_path`, which is relative to the config root.
#[derive(Clone)]
struct PolicyMapper {
    scan_root: PathBuf,
    root_is_file: bool,
    /// Config-root-relative prefix for this scan root; `None` when no config
    /// root applies (no config, or the root lives outside the config tree),
    /// which falls back to matching the path exactly as spelled.
    policy_prefix: Option<String>,
}

impl PolicyMapper {
    fn for_root(root: &Path, config: &ContextConfig) -> Self {
        let policy_prefix = match (&config.policy_root, canonical_normalized(root)) {
            (Some(policy_root), Some(canonical_root)) => {
                if canonical_root == *policy_root {
                    Some(String::new())
                } else {
                    canonical_root
                        .strip_prefix(&format!("{policy_root}/"))
                        .map(str::to_owned)
                }
            }
            _ => None,
        };
        Self {
            scan_root: root.to_path_buf(),
            root_is_file: root.is_file(),
            policy_prefix,
        }
    }

    fn scan_path(&self, path: &Path) -> String {
        let relative = path.strip_prefix(&self.scan_root).unwrap_or(path);
        let selected = if relative.as_os_str().is_empty() && self.root_is_file {
            path.file_name().map_or(path, Path::new)
        } else {
            relative
        };
        trim_normalized_path(&normalize_path(selected)).to_owned()
    }

    fn policy_path<'a>(&self, path: &Path, scan_path: &'a str) -> Cow<'a, str> {
        let Some(prefix) = &self.policy_prefix else {
            return Cow::Owned(trim_normalized_path(&normalize_path(path)).to_owned());
        };
        if self.root_is_file || scan_path.is_empty() {
            Cow::Owned(prefix.clone())
        } else if prefix.is_empty() {
            Cow::Borrowed(scan_path)
        } else {
            Cow::Owned(format!("{prefix}/{scan_path}"))
        }
    }
}

struct CollectState {
    files: Vec<PathBuf>,
    nested_repos_entered: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
fn walk_root(
    root: &Path,
    mapper: &PolicyMapper,
    config: &ContextConfig,
    options: &CollectOptions<'_>,
    include_matcher: &Option<GlobSet>,
    path_terms: &[String],
    extension_matcher: &[String],
    explicit_excluded_roots: &[String],
    state: &mut CollectState,
    nesting: usize,
) -> Result<()> {
    let probe_ignored_repo_roots = !options.with_git_ignored
        && !options.skip_nested_repos
        && nesting < NESTED_REPO_MAX_RECURSION;
    let mut walk = WalkBuilder::new(root);
    walk.hidden(false)
        .ignore(!options.with_git_ignored)
        .git_ignore(!options.with_git_ignored)
        .git_exclude(!options.with_git_ignored)
        .parents(!options.with_git_ignored);
    if !options.with_excluded {
        let excludes = config.excludes.clone();
        let explicit_roots = explicit_excluded_roots.to_vec();
        let filter_mapper = mapper.clone();
        walk.filter_entry(move |entry| {
            let scan_path = filter_mapper.scan_path(entry.path());
            let policy_path = filter_mapper.policy_path(entry.path(), &scan_path);
            !is_excluded_by_policy(
                &policy_path,
                entry.file_type().is_some_and(|kind| kind.is_dir()),
                &excludes,
                &explicit_roots,
            )
        });
    }
    // Parallel enumeration: entries arrive in nondeterministic order and are
    // merged into deterministic sorted-and-capped output by `collect_files`.
    // The walk always completes, so candidate totals are exact.
    struct WalkBatch {
        files: Vec<PathBuf>,
        visited_dirs: Option<HashSet<PathBuf>>,
        nested_repos_entered: Vec<PathBuf>,
        error: Option<ignore::Error>,
    }

    struct WorkerBatch<'a> {
        batches: &'a std::sync::Mutex<Vec<WalkBatch>>,
        batch: Option<WalkBatch>,
    }

    impl Drop for WorkerBatch<'_> {
        fn drop(&mut self) {
            let batch = self
                .batch
                .take()
                .expect("walk worker batch must be present until drop");
            self.batches
                .lock()
                .expect("walk batch sink poisoned")
                .push(batch);
        }
    }

    let batches = std::sync::Mutex::new(Vec::new());
    walk.build_parallel().run(|| {
        let worker_mapper = mapper.clone();
        let mut worker = WorkerBatch {
            batches: &batches,
            batch: Some(WalkBatch {
                files: Vec::new(),
                visited_dirs: probe_ignored_repo_roots.then(HashSet::new),
                nested_repos_entered: Vec::new(),
                error: None,
            }),
        };
        Box::new(move |entry| {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    let batch = worker
                        .batch
                        .as_mut()
                        .expect("walk worker batch must be present");
                    if batch.error.is_none() {
                        batch.error = Some(error);
                    }
                    return ignore::WalkState::Quit;
                }
            };
            match entry.file_type() {
                Some(kind) if kind.is_dir() => {
                    let path = entry.path().to_path_buf();
                    let batch = worker
                        .batch
                        .as_mut()
                        .expect("walk worker batch must be present");
                    if entry.depth() > 0 && path.join(".git").exists() {
                        if options.skip_nested_repos {
                            return ignore::WalkState::Skip;
                        }
                        batch.nested_repos_entered.push(path.clone());
                    }
                    if probe_ignored_repo_roots {
                        batch
                            .visited_dirs
                            .as_mut()
                            .expect("ignored-repo probing must retain visited directories")
                            .insert(path);
                    }
                }
                Some(kind)
                    if kind.is_file()
                        && file_is_included(
                            entry.path(),
                            &worker_mapper,
                            include_matcher,
                            path_terms,
                            extension_matcher,
                            config,
                            options.with_excluded,
                            explicit_excluded_roots,
                        ) =>
                {
                    worker
                        .batch
                        .as_mut()
                        .expect("walk worker batch must be present")
                        .files
                        .push(entry.into_path());
                }
                _ => {}
            }
            ignore::WalkState::Continue
        })
    });
    let batches = batches.into_inner().expect("walk batch sink poisoned");
    let mut files = Vec::new();
    let mut visited_dirs = probe_ignored_repo_roots.then(HashSet::new);
    let mut error = None;
    for mut batch in batches {
        files.append(&mut batch.files);
        state.nested_repos_entered.extend(
            batch
                .nested_repos_entered
                .into_iter()
                .map(|path| display_path(&path)),
        );
        if let (Some(all_dirs), Some(worker_dirs)) = (&mut visited_dirs, batch.visited_dirs) {
            all_dirs.extend(worker_dirs);
        }
        if error.is_none() {
            error = batch.error;
        }
    }
    if let Some(error) = error {
        return Err(error.into());
    }
    for file in files {
        state.files.push(file);
    }
    // Ignored-repository supplement: a directory pruned by Git ignore rules
    // that is itself a repository root is still part of the caller's broad
    // workspace scope. Enter it with its own ignore rules and disclose it just
    // like a tracked submodule crossed by the primary walk.
    if !probe_ignored_repo_roots {
        return Ok(());
    }
    let visited_dirs = visited_dirs.expect("ignored-repo probing must retain visited directories");
    let mut nested_roots = collect_pruned_repo_roots_for_visited_dirs(
        mapper,
        &visited_dirs,
        config,
        options.with_excluded,
        explicit_excluded_roots,
    )?;
    nested_roots.sort();
    for nested_root in nested_roots {
        state.nested_repos_entered.push(display_path(&nested_root));
        walk_root(
            &nested_root,
            mapper,
            config,
            options,
            include_matcher,
            path_terms,
            extension_matcher,
            explicit_excluded_roots,
            state,
            nesting + 1,
        )?;
    }
    Ok(())
}

fn collect_pruned_repo_roots_for_visited_dirs(
    mapper: &PolicyMapper,
    visited_dirs: &HashSet<PathBuf>,
    config: &ContextConfig,
    with_excluded: bool,
    explicit_excluded_roots: &[String],
) -> Result<Vec<PathBuf>> {
    let dirs = visited_dirs
        .iter()
        .map(PathBuf::as_path)
        .collect::<Vec<_>>();
    let available_threads = std::thread::available_parallelism().map_or(1, |threads| threads.get());
    let worker_count = available_threads
        .min(NESTED_REPO_MAX_PROBE_THREADS)
        .min(dirs.len());
    if dirs.len() < NESTED_REPO_PARALLEL_MIN_DIRS || worker_count <= 1 {
        let mut output = Vec::new();
        for dir in dirs {
            collect_pruned_repo_roots(
                dir,
                mapper,
                visited_dirs,
                config,
                with_excluded,
                explicit_excluded_roots,
                &mut output,
            )?;
        }
        return Ok(output);
    }

    let chunk_size = dirs.len().div_ceil(worker_count);
    std::thread::scope(|scope| {
        let workers = dirs
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    let mut output = Vec::new();
                    for dir in chunk {
                        collect_pruned_repo_roots(
                            dir,
                            mapper,
                            visited_dirs,
                            config,
                            with_excluded,
                            explicit_excluded_roots,
                            &mut output,
                        )?;
                    }
                    Ok::<_, anyhow::Error>(output)
                })
            })
            .collect::<Vec<_>>();
        let mut output = Vec::new();
        for worker in workers {
            output.extend(worker.join().expect("nested-repo probe worker panicked")?);
        }
        Ok(output)
    })
}

/// Find git-repo roots among the unvisited (walker-pruned) children of a
/// visited directory. Pruned plain directories are probed one level deeper
/// so a repo directly under an ignored grouping directory is still found.
#[allow(clippy::too_many_arguments)]
fn collect_pruned_repo_roots(
    dir: &Path,
    mapper: &PolicyMapper,
    visited_dirs: &HashSet<PathBuf>,
    config: &ContextConfig,
    with_excluded: bool,
    explicit_excluded_roots: &[String],
    output: &mut Vec<PathBuf>,
) -> Result<()> {
    probe_children_for_repos(
        dir,
        mapper,
        visited_dirs,
        config,
        with_excluded,
        explicit_excluded_roots,
        NESTED_REPO_PROBE_DEPTH,
        output,
    )
}

#[allow(clippy::too_many_arguments)]
fn probe_children_for_repos(
    dir: &Path,
    mapper: &PolicyMapper,
    visited_dirs: &HashSet<PathBuf>,
    config: &ContextConfig,
    with_excluded: bool,
    explicit_excluded_roots: &[String],
    remaining_probe_depth: usize,
    output: &mut Vec<PathBuf>,
) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| {
        format!(
            "failed to inspect nested repositories under {}",
            dir.display()
        )
    })?;
    for entry in entries {
        let entry = entry.with_context(|| {
            format!(
                "failed to inspect a nested-repository candidate under {}",
                dir.display()
            )
        })?;
        let path = entry.path();
        if !entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", path.display()))?
            .is_dir()
        {
            continue;
        }
        if visited_dirs.contains(&path) {
            continue;
        }
        if path.file_name().is_some_and(|name| name == ".git") {
            continue;
        }
        if !with_excluded {
            let scan_path = mapper.scan_path(&path);
            let policy_path = mapper.policy_path(&path, &scan_path);
            if is_excluded_by_policy(
                &policy_path,
                true,
                &config.excludes,
                explicit_excluded_roots,
            ) {
                continue;
            }
        }
        if path.join(".git").exists() {
            output.push(path);
        } else if remaining_probe_depth > 0 {
            probe_children_for_repos(
                &path,
                mapper,
                visited_dirs,
                config,
                with_excluded,
                explicit_excluded_roots,
                remaining_probe_depth - 1,
                output,
            )?;
        }
    }
    Ok(())
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
            let mapper = PolicyMapper::for_root(path, config);
            let scan_path = mapper.scan_path(path);
            let policy_path = mapper.policy_path(path, &scan_path);
            if policy_path.is_empty() || policy_path == "." {
                return None;
            }
            if matches_exclude_policy(&policy_path, path.is_dir(), &config.excludes) {
                Some(policy_path.into_owned())
            } else {
                None
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn file_is_included(
    path: &Path,
    mapper: &PolicyMapper,
    include_matcher: &Option<GlobSet>,
    path_terms: &[String],
    extension_matcher: &[String],
    config: &ContextConfig,
    with_excluded: bool,
    explicit_excluded_roots: &[String],
) -> bool {
    let scan_path = mapper.scan_path(path);
    if !with_excluded {
        if path.file_name().is_some_and(|name| name == ".git") {
            return false;
        }
        let policy_path = mapper.policy_path(path, &scan_path);
        if is_excluded_by_policy(
            &policy_path,
            false,
            &config.excludes,
            explicit_excluded_roots,
        ) {
            return false;
        }
    }
    if let Some(include_matcher) = include_matcher {
        let basename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !include_matcher.is_match(&scan_path) && !include_matcher.is_match(basename) {
            return false;
        }
    }
    if !path_terms.is_empty() && !path_terms.iter().all(|term| scan_path.contains(term)) {
        return false;
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

fn is_excluded_by_policy(
    policy_path: &str,
    is_dir: bool,
    excludes: &GlobSet,
    explicit_excluded_roots: &[String],
) -> bool {
    if policy_path.is_empty() || policy_path == "." {
        return false;
    }
    if !matches_exclude_policy(policy_path, is_dir, excludes) {
        return false;
    }
    let Some(relative_to_explicit_root) =
        relative_to_explicit_excluded_root(policy_path, explicit_excluded_roots)
    else {
        return true;
    };
    if relative_to_explicit_root.is_empty() {
        return false;
    }
    matches_exclude_policy(relative_to_explicit_root, is_dir, excludes)
}

fn matches_exclude_policy(path: &str, is_dir: bool, excludes: &GlobSet) -> bool {
    if excludes.is_match(path) {
        return true;
    }
    is_dir && excludes.is_match(format!("{path}/__contextmink_probe__"))
}

fn relative_to_explicit_excluded_root<'a>(
    path: &'a str,
    explicit_excluded_roots: &[String],
) -> Option<&'a str> {
    let normalized = trim_normalized_path(path);
    explicit_excluded_roots
        .iter()
        .filter_map(|root| {
            if normalized == root {
                Some((root.len(), ""))
            } else {
                normalized
                    .strip_prefix(root)
                    .and_then(|relative| relative.strip_prefix('/'))
                    .map(|relative| (root.len(), relative))
            }
        })
        .max_by_key(|(root_len, _)| *root_len)
        .map(|(_, relative)| relative)
}

fn trim_normalized_path(path: &str) -> &str {
    path.trim_start_matches("./").trim_end_matches('/')
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

fn normalize_path_terms(path_terms: &[String]) -> Result<Vec<String>> {
    path_terms
        .iter()
        .map(|term| {
            let term = normalize_user_path_fragment(term.trim());
            if term.is_empty() {
                Err(anyhow!("files --path-contains entries must not be empty"))
            } else {
                Ok(term)
            }
        })
        .collect()
}

fn normalize_extensions(extensions: &[String]) -> Vec<String> {
    extensions
        .iter()
        .flat_map(|extension| extension.split(','))
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

fn normalize_path(path: &Path) -> Cow<'_, str> {
    let path = path.strip_prefix(".").unwrap_or(path);
    #[cfg(windows)]
    {
        Cow::Owned(path.to_string_lossy().replace('\\', "/"))
    }
    #[cfg(not(windows))]
    {
        path.to_string_lossy()
    }
}

fn normalize_user_path_fragment(fragment: &str) -> String {
    #[cfg(windows)]
    {
        fragment.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        fragment.to_owned()
    }
}

#[derive(Debug, Hash, PartialEq, Eq)]
enum FileIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(windows)]
    Windows { volume: u32, index: u64 },
    #[cfg(not(any(unix, windows)))]
    Canonical(PathBuf),
}

/// Identify the underlying file rather than its spelling. This collapses
/// overlapping roots, symlinks/junctions, hard links, case aliases, and drive
/// aliases without retaining one open handle per candidate.
fn file_identity(path: &Path) -> Result<FileIdentity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let metadata =
            fs::metadata(path).with_context(|| format!("failed to identify {}", path.display()))?;
        Ok(FileIdentity::Unix {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_READ_ATTRIBUTES,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
            OPEN_EXISTING,
        };

        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("failed to open {} for identity", path.display()));
        }
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        let succeeded = unsafe { GetFileInformationByHandle(handle, &mut information) };
        let information_error = (succeeded == 0).then(std::io::Error::last_os_error);
        let close_succeeded = unsafe { CloseHandle(handle) };
        if close_succeeded == 0 {
            return Err(std::io::Error::last_os_error()).with_context(|| {
                format!("failed to close identity handle for {}", path.display())
            });
        }
        if let Some(error) = information_error {
            return Err(error).with_context(|| format!("failed to identify {}", path.display()));
        }
        Ok(FileIdentity::Windows {
            volume: information.dwVolumeSerialNumber,
            index: (u64::from(information.nFileIndexHigh) << 32)
                | u64::from(information.nFileIndexLow),
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let canonical = fs::canonicalize(path)
            .with_context(|| format!("failed to canonicalize {}", path.display()))?;
        Ok(FileIdentity::Canonical(canonical))
    }
}

pub(crate) fn display_path(path: &Path) -> String {
    normalize_path(path).into_owned()
}

#[cfg(test)]
#[path = "files/tests.rs"]
mod tests;
