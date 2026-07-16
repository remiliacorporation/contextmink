use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Serialize;

use super::{ContextminkConfig, canonical_normalized, validate_profile};

const BASH_LAUNCHER: &[u8] = include_bytes!("../templates/scripts/contextmink");
const CMD_DIAGNOSTIC: &[u8] = include_bytes!("../templates/scripts/contextmink.cmd");
const AGENT_INTEGRATION: &[u8] = include_bytes!("../templates/AGENTS.contextmink.md");
const GITIGNORE_COMMENT: &str = "# contextmink project-local release binaries";
const GITIGNORE_ENTRY: &str = "/tools/contextmink/bin/";

#[derive(Debug)]
pub(crate) struct SetupProjectRequest<'a> {
    pub(crate) project_root: &'a Path,
    /// Defaults to the running contextmink executable. Tests and embedding
    /// callers can supply an extracted release binary explicitly.
    pub(crate) source_binary: Option<&'a Path>,
    pub(crate) dry_run: bool,
    pub(crate) replace_managed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SetupActionKind {
    Create,
    Replace,
    Unchanged,
    MakeExecutable,
    UpdateGitignore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SetupAction {
    pub(crate) path: PathBuf,
    pub(crate) action: SetupActionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SetupProjectResult {
    pub(crate) schema: &'static str,
    pub(crate) project_root: String,
    pub(crate) profile: String,
    pub(crate) dry_run: bool,
    pub(crate) actions: Vec<SetupAction>,
    pub(crate) agent_guidance_files_found: Vec<PathBuf>,
    pub(crate) next_actions: Vec<String>,
}

struct ManagedFile {
    relative_path: PathBuf,
    content: Vec<u8>,
    executable: bool,
    release_managed: bool,
}

/// Install a project-local release without overwriting managed files that have
/// diverged. Every destination is preflighted before the first mutation, so a
/// refusal cannot leave a partially installed project.
pub(crate) fn setup_project(request: SetupProjectRequest<'_>) -> Result<SetupProjectResult> {
    let root = fs::canonicalize(request.project_root).with_context(|| {
        format!(
            "resolve setup-project root {}",
            request.project_root.display()
        )
    })?;
    if !root.is_dir() {
        return Err(anyhow!(
            "setup-project root {} is not an existing directory",
            root.display()
        ));
    }
    let profile = project_profile(&root)?;
    let source_binary = match request.source_binary {
        Some(path) => path.to_path_buf(),
        None => std::env::current_exe().context("resolve the running contextmink executable")?,
    };
    let source_name = source_binary
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            anyhow!(
                "source binary has no UTF-8 file name: {}",
                source_binary.display()
            )
        })?;
    let suffix = if source_name.eq_ignore_ascii_case("contextmink.exe") {
        ".exe"
    } else if source_name == "contextmink" {
        ""
    } else {
        return Err(anyhow!(
            "setup-project source binary must be named contextmink or contextmink.exe, found {source_name:?}"
        ));
    };
    let binary = fs::read(&source_binary)
        .with_context(|| format!("read source binary {}", source_binary.display()))?;
    let config = generated_config(&profile)?;
    let mut managed = vec![
        ManagedFile {
            relative_path: PathBuf::from(format!("tools/contextmink/bin/contextmink{suffix}")),
            content: binary,
            executable: true,
            release_managed: true,
        },
        ManagedFile {
            relative_path: PathBuf::from("scripts/contextmink"),
            content: BASH_LAUNCHER.to_vec(),
            executable: true,
            release_managed: true,
        },
        ManagedFile {
            relative_path: PathBuf::from(".contextmink.toml"),
            content: config.into_bytes(),
            executable: false,
            release_managed: false,
        },
        ManagedFile {
            relative_path: PathBuf::from("tools/contextmink/agent_integration.md"),
            content: AGENT_INTEGRATION.to_vec(),
            executable: false,
            release_managed: true,
        },
    ];
    if suffix == ".exe" {
        let bridge_name = "contextmink-bridge.exe";
        let source_bridge = source_binary.with_file_name(bridge_name);
        let bridge = fs::read(&source_bridge)
            .with_context(|| format!("read sibling bridge {}", source_bridge.display()))?;
        managed.push(ManagedFile {
            relative_path: PathBuf::from(format!("tools/contextmink/bin/{bridge_name}")),
            content: bridge,
            executable: true,
            release_managed: true,
        });
        managed.push(ManagedFile {
            relative_path: PathBuf::from("scripts/contextmink.cmd"),
            content: CMD_DIAGNOSTIC.to_vec(),
            executable: false,
            release_managed: true,
        });
    }

    let mut actions = Vec::with_capacity(managed.len() + 1);
    for file in &managed {
        let destination = root.join(&file.relative_path);
        let action = preflight_managed_file(
            &destination,
            &file.content,
            file.executable,
            file.release_managed && request.replace_managed,
        )?;
        actions.push(SetupAction {
            path: file.relative_path.clone(),
            action,
        });
    }
    let gitignore_path = root.join(".gitignore");
    let existing_gitignore = if gitignore_path.exists() {
        Some(
            fs::read_to_string(&gitignore_path)
                .with_context(|| format!("read {}", gitignore_path.display()))?,
        )
    } else {
        None
    };
    let updated_gitignore = gitignore_content(existing_gitignore.as_deref());
    let gitignore_action = if existing_gitignore.as_deref() == Some(updated_gitignore.as_str()) {
        SetupActionKind::Unchanged
    } else if existing_gitignore.is_some() {
        SetupActionKind::UpdateGitignore
    } else {
        SetupActionKind::Create
    };
    actions.push(SetupAction {
        path: PathBuf::from(".gitignore"),
        action: gitignore_action,
    });

    if !request.dry_run {
        for (file, action) in managed.iter().zip(&actions) {
            let destination = root.join(&file.relative_path);
            match action.action {
                SetupActionKind::Create => write_new_file(&destination, &file.content)?,
                SetupActionKind::Replace => fs::write(&destination, &file.content)
                    .with_context(|| format!("replace managed file {}", destination.display()))?,
                SetupActionKind::MakeExecutable => {}
                SetupActionKind::Unchanged => {}
                SetupActionKind::UpdateGitignore => {
                    unreachable!("managed files never use the gitignore action")
                }
            }
            if file.executable {
                ensure_executable(&destination)?;
            }
        }
        let gitignore_action = &actions
            .last()
            .expect("gitignore action must be present")
            .action;
        if !matches!(gitignore_action, SetupActionKind::Unchanged) {
            if let Some(parent) = gitignore_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create directory {}", parent.display()))?;
            }
            fs::write(&gitignore_path, updated_gitignore)
                .with_context(|| format!("write {}", gitignore_path.display()))?;
        }
    }

    let agent_guidance_files_found = ["AGENTS.md", "CLAUDE.md"]
        .into_iter()
        .map(PathBuf::from)
        .filter(|path| root.join(path).is_file())
        .collect();
    let project_root = canonical_normalized(&root)
        .expect("setup-project root was canonicalized successfully before rendering");
    Ok(SetupProjectResult {
        schema: "contextmink.project_setup.v1",
        project_root,
        profile,
        dry_run: request.dry_run,
        actions,
        agent_guidance_files_found,
        next_actions: vec![
            "Review .contextmink.toml and add only project-specific generated or high-output exclude globs."
                .to_owned(),
            "Add repository-owned destructive-guard fragments only for critical paths that require a deletion tripwire."
                .to_owned(),
            "Read tools/contextmink/agent_integration.md and adapt its operational contract into the repository's agent guidance; setup-project never edits AGENTS.md or CLAUDE.md."
                .to_owned(),
            "From Git Bash, run scripts/contextmink --json files . --limit 1 and inspect the contextmink.receipt.v2 envelope."
                .to_owned(),
            "Run scripts/contextmink --json guard-check -- git clean and confirm the decision is deny."
                .to_owned(),
        ],
    })
}

fn project_profile(root: &Path) -> Result<String> {
    let profile = root
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "cannot derive a profile from project root {}",
                root.display()
            )
        })?
        .to_owned();
    validate_profile(&profile)?;
    Ok(profile)
}

fn generated_config(profile: &str) -> Result<String> {
    let config = ContextminkConfig {
        profile: Some(profile.to_owned()),
        exclude_globs: Some(Vec::new()),
        destructive_guard_recursive_delete_fragments: None,
        destructive_guard_delete_fragments: None,
    };
    toml::to_string_pretty(&config).context("serialize generated .contextmink.toml")
}

fn preflight_managed_file(
    destination: &Path,
    expected: &[u8],
    executable: bool,
    replace_divergent: bool,
) -> Result<SetupActionKind> {
    if !destination.exists() {
        return Ok(SetupActionKind::Create);
    }
    if !destination.is_file() {
        return Err(anyhow!(
            "setup-project destination is not a file: {}",
            destination.display()
        ));
    }
    let existing = fs::read(destination)
        .with_context(|| format!("read existing managed file {}", destination.display()))?;
    if existing != expected {
        if replace_divergent {
            return Ok(SetupActionKind::Replace);
        }
        return Err(anyhow!(
            "setup-project refuses to overwrite divergent file {}; --replace-managed applies only to release artifacts and never replaces .contextmink.toml, which must be reviewed or removed explicitly",
            destination.display()
        ));
    }
    if executable && executable_bit_missing(destination)? {
        Ok(SetupActionKind::MakeExecutable)
    } else {
        Ok(SetupActionKind::Unchanged)
    }
}

fn gitignore_content(existing: Option<&str>) -> String {
    let existing = existing.unwrap_or("");
    if existing.lines().any(|line| line.trim() == GITIGNORE_ENTRY) {
        return existing.to_owned();
    }
    let mut output = existing.to_owned();
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    if !output.is_empty() && !output.ends_with("\n\n") {
        output.push('\n');
    }
    output.push_str(GITIGNORE_COMMENT);
    output.push('\n');
    output.push_str(GITIGNORE_ENTRY);
    output.push('\n');
    output
}

fn write_new_file(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("managed destination has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("create directory {}", parent.display()))?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create managed file {}", path.display()))?;
    output
        .write_all(content)
        .with_context(|| format!("write managed file {}", path.display()))
}

#[cfg(unix)]
fn executable_bit_missing(path: &Path) -> Result<bool> {
    use std::os::unix::fs::PermissionsExt;

    let mode = fs::metadata(path)
        .with_context(|| format!("read permissions for {}", path.display()))?
        .permissions()
        .mode();
    Ok(mode & 0o111 == 0)
}

#[cfg(not(unix))]
fn executable_bit_missing(_path: &Path) -> Result<bool> {
    Ok(false)
}

#[cfg(unix)]
fn ensure_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let metadata =
        fs::metadata(path).with_context(|| format!("read permissions for {}", path.display()))?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("set executable permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn ensure_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT_TEST_ROOT: AtomicUsize = AtomicUsize::new(0);

    fn test_root(name: &str) -> PathBuf {
        let serial = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/project-setup-tests")
            .join(format!("{name}-{}-{serial}", std::process::id()))
    }

    fn fixture(name: &str) -> (PathBuf, PathBuf) {
        let root = test_root(name);
        fs::create_dir_all(&root).unwrap();
        let release = root.join("release");
        let project = root.join("demo-project");
        fs::create_dir_all(&release).unwrap();
        fs::create_dir_all(&project).unwrap();
        let suffix = std::env::consts::EXE_SUFFIX;
        let binary = release.join(format!("contextmink{suffix}"));
        fs::write(&binary, b"contextmink-binary").unwrap();
        fs::write(
            release.join(format!("contextmink-bridge{suffix}")),
            b"contextmink-bridge-binary",
        )
        .unwrap();
        (project, binary)
    }

    fn request<'a>(project: &'a Path, binary: &'a Path, dry_run: bool) -> SetupProjectRequest<'a> {
        SetupProjectRequest {
            project_root: project,
            source_binary: Some(binary),
            dry_run,
            replace_managed: false,
        }
    }

    fn cleanup(project: &Path) {
        fs::remove_dir_all(
            project
                .parent()
                .expect("fixture project must have a parent"),
        )
        .unwrap();
    }

    #[test]
    fn dry_run_reports_without_writing() {
        let (project, binary) = fixture("dry-run");
        let result = setup_project(request(&project, &binary, true)).unwrap();
        assert!(result.dry_run);
        assert!(
            result
                .actions
                .iter()
                .all(|action| action.action == SetupActionKind::Create)
        );
        assert!(!project.join(".contextmink.toml").exists());
        assert!(!project.join("AGENTS.md").exists());
        cleanup(&project);
    }

    #[test]
    fn install_is_idempotent_and_never_writes_agent_guidance() {
        let (project, binary) = fixture("idempotent");
        fs::write(project.join(".gitignore"), "target/\n").unwrap();
        let first = setup_project(request(&project, &binary, false)).unwrap();
        assert_eq!(first.profile, "demo-project");
        assert!(
            first
                .next_actions
                .iter()
                .any(|action| action.contains("AGENTS.md"))
        );
        let config = fs::read_to_string(project.join(".contextmink.toml")).unwrap();
        assert!(config.contains("profile = \"demo-project\""));
        assert!(!config.contains("replace-with-workspace-name"));
        let gitignore = fs::read_to_string(project.join(".gitignore")).unwrap();
        assert_eq!(gitignore.matches(GITIGNORE_ENTRY).count(), 1);
        assert!(!project.join("AGENTS.md").exists());
        assert!(!project.join("CLAUDE.md").exists());
        assert_eq!(
            fs::read(project.join(format!(
                "tools/contextmink/bin/contextmink{}",
                std::env::consts::EXE_SUFFIX
            )))
            .unwrap(),
            b"contextmink-binary"
        );
        assert_eq!(
            fs::read(project.join("scripts/contextmink")).unwrap(),
            BASH_LAUNCHER
        );
        assert_eq!(
            fs::read(project.join("tools/contextmink/agent_integration.md")).unwrap(),
            AGENT_INTEGRATION
        );
        if cfg!(windows) {
            assert_eq!(
                fs::read(project.join("tools/contextmink/bin/contextmink-bridge.exe")).unwrap(),
                b"contextmink-bridge-binary"
            );
            assert!(
                fs::read_to_string(project.join("scripts/contextmink.cmd"))
                    .unwrap()
                    .contains("requires Git Bash")
            );
        } else {
            assert!(!project.join("scripts/contextmink.cmd").exists());
        }

        let second = setup_project(request(&project, &binary, false)).unwrap();
        assert!(
            second
                .actions
                .iter()
                .all(|action| action.action == SetupActionKind::Unchanged)
        );
        let gitignore = fs::read_to_string(project.join(".gitignore")).unwrap();
        assert_eq!(gitignore.matches(GITIGNORE_ENTRY).count(), 1);
        cleanup(&project);
    }

    #[test]
    fn divergent_managed_file_refuses_before_any_write() {
        let (project, binary) = fixture("divergence");
        setup_project(request(&project, &binary, false)).unwrap();
        fs::remove_file(project.join(".contextmink.toml")).unwrap();
        fs::write(project.join("scripts/contextmink"), b"locally changed").unwrap();

        let error = setup_project(request(&project, &binary, false)).unwrap_err();
        assert!(error.to_string().contains("refuses to overwrite divergent"));
        assert!(!project.join(".contextmink.toml").exists());
        cleanup(&project);
    }

    #[test]
    fn explicit_replacement_updates_release_files_but_never_configuration() {
        let (project, binary) = fixture("replace-managed");
        setup_project(request(&project, &binary, false)).unwrap();
        fs::write(project.join("scripts/contextmink"), b"older release").unwrap();

        let mut replace = request(&project, &binary, false);
        replace.replace_managed = true;
        let result = setup_project(replace).unwrap();
        assert!(result.actions.iter().any(|action| {
            action.path == Path::new("scripts/contextmink")
                && action.action == SetupActionKind::Replace
        }));
        assert_eq!(
            fs::read(project.join("scripts/contextmink")).unwrap(),
            BASH_LAUNCHER
        );

        fs::write(project.join(".contextmink.toml"), "profile = \"owned\"\n").unwrap();
        let mut replace = request(&project, &binary, false);
        replace.replace_managed = true;
        let error = setup_project(replace).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("never replaces .contextmink.toml")
        );
        cleanup(&project);
    }
}
