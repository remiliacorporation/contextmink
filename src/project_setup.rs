use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Serialize;

use super::{ContextminkConfig, canonical_normalized, load_context_config, validate_profile};

#[path = "project_setup_receipt.rs"]
mod receipt;

use receipt::{
    INSTALL_RECEIPT_PATH, build_install_receipt, load_install_receipt, managed_text_sha256,
    receipt_bytes, refuse_release_downgrade, validate_managed_text_path,
};

const BASH_LAUNCHER: &[u8] = include_bytes!("../templates/scripts/contextmink");
const CMD_DIAGNOSTIC: &[u8] = include_bytes!("../templates/scripts/contextmink.cmd");
const CONTEXTMINK_INTEGRATION: &[u8] = include_bytes!("../templates/AGENTS.contextmink.md");
const CONTEXTMINK_SKILL: &[u8] = include_bytes!("../templates/skills/contextmink/SKILL.md");
const CONTEXTMINK_OPENAI_METADATA: &[u8] =
    include_bytes!("../templates/skills/contextmink/agents/openai.yaml");
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

#[derive(Debug)]
pub(crate) struct UninstallProjectRequest<'a> {
    pub(crate) project_root: &'a Path,
    /// Defaults to the running Contextmink executable. Tests can supply an
    /// extracted release binary to exercise the self-removal refusal.
    pub(crate) running_binary: Option<&'a Path>,
    pub(crate) dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SetupActionKind {
    Create,
    Replace,
    Unchanged,
    PreserveRepositoryOwned,
    MakeExecutable,
    UpdateGitignore,
    RemoveManaged,
    RemoveRetired,
    ModifiedRefusal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SetupAction {
    pub(crate) path: PathBuf,
    pub(crate) action: SetupActionKind,
    pub(crate) requires_replace_managed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SetupProjectResult {
    pub(crate) schema: &'static str,
    pub(crate) project_root: String,
    pub(crate) profile: String,
    pub(crate) dry_run: bool,
    pub(crate) ready: bool,
    pub(crate) actions: Vec<SetupAction>,
    pub(crate) agent_guidance_files_found: Vec<PathBuf>,
    pub(crate) next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct UninstallProjectResult {
    pub(crate) schema: &'static str,
    pub(crate) project_root: String,
    pub(crate) dry_run: bool,
    pub(crate) ready: bool,
    pub(crate) actions: Vec<SetupAction>,
    pub(crate) preserved_repository_owned: Vec<PathBuf>,
    pub(crate) next_actions: Vec<String>,
}

pub(super) struct ManagedFile {
    relative_path: PathBuf,
    content: Vec<u8>,
    executable: bool,
    ownership: SetupFileOwnership,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SetupFileOwnership {
    ReleaseManagedRuntime,
    ReleaseManagedText,
    RepositoryOwnedConfig,
}

fn contextmink_skill_files() -> [ManagedFile; 3] {
    [
        ManagedFile {
            relative_path: PathBuf::from(".agents/skills/contextmink/SKILL.md"),
            content: CONTEXTMINK_SKILL.to_vec(),
            executable: false,
            ownership: SetupFileOwnership::ReleaseManagedText,
        },
        ManagedFile {
            relative_path: PathBuf::from(".agents/skills/contextmink/agents/openai.yaml"),
            content: CONTEXTMINK_OPENAI_METADATA.to_vec(),
            executable: false,
            ownership: SetupFileOwnership::ReleaseManagedText,
        },
        ManagedFile {
            relative_path: PathBuf::from(".claude/skills/contextmink/SKILL.md"),
            content: CONTEXTMINK_SKILL.to_vec(),
            executable: false,
            ownership: SetupFileOwnership::ReleaseManagedText,
        },
    ]
}

struct PreflightFile {
    action: SetupActionKind,
    profile: Option<String>,
    requires_replace_managed: bool,
}

struct SetupPreflight<'a> {
    prior_sha256: Option<&'a str>,
    prior_receipt_loaded: bool,
    dry_run: bool,
    replace_managed: bool,
    generated_profile: &'a str,
}

/// Install a project-local release without overwriting managed files that have
/// diverged. Every destination is preflighted before the first mutation, so a
/// refusal cannot leave a partially installed project.
pub(crate) fn setup_project(request: SetupProjectRequest<'_>) -> Result<SetupProjectResult> {
    let root = resolve_project_root(request.project_root, "setup-project")?;
    let generated_profile = project_profile(&root)?;
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
    let config = generated_config(&generated_profile)?;
    let mut managed = vec![
        ManagedFile {
            relative_path: PathBuf::from(format!("tools/contextmink/bin/contextmink{suffix}")),
            content: binary,
            executable: true,
            ownership: SetupFileOwnership::ReleaseManagedRuntime,
        },
        ManagedFile {
            relative_path: PathBuf::from("scripts/contextmink"),
            content: BASH_LAUNCHER.to_vec(),
            executable: true,
            ownership: SetupFileOwnership::ReleaseManagedText,
        },
        ManagedFile {
            relative_path: PathBuf::from("scripts/contextmink.cmd"),
            content: CMD_DIAGNOSTIC.to_vec(),
            executable: false,
            ownership: SetupFileOwnership::ReleaseManagedText,
        },
        ManagedFile {
            relative_path: PathBuf::from(".contextmink.toml"),
            content: config.into_bytes(),
            executable: false,
            ownership: SetupFileOwnership::RepositoryOwnedConfig,
        },
        ManagedFile {
            relative_path: PathBuf::from("tools/contextmink/agent_integration.md"),
            content: CONTEXTMINK_INTEGRATION.to_vec(),
            executable: false,
            ownership: SetupFileOwnership::ReleaseManagedText,
        },
    ];
    managed.extend(contextmink_skill_files());
    if suffix == ".exe" {
        let bridge_name = "contextmink-bridge.exe";
        let source_bridge = source_binary.with_file_name(bridge_name);
        let bridge = fs::read(&source_bridge)
            .with_context(|| format!("read sibling bridge {}", source_bridge.display()))?;
        managed.push(ManagedFile {
            relative_path: PathBuf::from(format!("tools/contextmink/bin/{bridge_name}")),
            content: bridge,
            executable: true,
            ownership: SetupFileOwnership::ReleaseManagedRuntime,
        });
    }

    let receipt_relative = Path::new(INSTALL_RECEIPT_PATH);
    validate_destination(&root, receipt_relative)?;
    let receipt_path = root.join(receipt_relative);
    let prior_receipt = load_install_receipt(&receipt_path)?;
    if let Some(receipt) = prior_receipt.as_ref() {
        refuse_release_downgrade(receipt, "setup-project")?;
    }
    let prior_hashes = prior_receipt
        .as_ref()
        .map(|receipt| {
            receipt
                .managed_files
                .iter()
                .map(|file| (file.path.clone(), file.sha256.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    let gitignore_relative = Path::new(".gitignore");
    validate_destination(&root, gitignore_relative)?;
    let gitignore_path = root.join(gitignore_relative);
    let existing_gitignore = if gitignore_path.exists() {
        if !gitignore_path.is_file() {
            return Err(anyhow!(
                "setup-project cannot preserve non-file {}",
                gitignore_path.display()
            ));
        }
        Some(
            fs::read_to_string(&gitignore_path)
                .with_context(|| format!("read {} as UTF-8", gitignore_path.display()))?,
        )
    } else {
        None
    };
    let managed_block_count = existing_gitignore
        .as_deref()
        .map(gitignore_managed_block_count)
        .unwrap_or(0);
    if managed_block_count > 1 {
        return Err(anyhow!(
            "setup-project found multiple Contextmink-managed blocks in {}; consolidate them deliberately before retrying",
            gitignore_path.display()
        ));
    }
    let prior_owns_gitignore = prior_receipt
        .as_ref()
        .is_some_and(|receipt| receipt.managed_gitignore_block);
    let existing_has_entry = existing_gitignore
        .as_deref()
        .is_some_and(gitignore_has_entry);
    let modified_owned_gitignore =
        prior_owns_gitignore && managed_block_count == 0 && existing_has_entry;
    if modified_owned_gitignore && !request.dry_run {
        return Err(anyhow!(
            "setup-project refuses modified Contextmink-managed .gitignore block in {}; restore the managed comment and entry or remove the standalone entry deliberately, then rerun setup-project",
            gitignore_path.display()
        ));
    }
    let updated_gitignore = if modified_owned_gitignore {
        existing_gitignore.clone().unwrap_or_default()
    } else {
        gitignore_content(existing_gitignore.as_deref())
    };
    let gitignore_action = if modified_owned_gitignore {
        SetupActionKind::ModifiedRefusal
    } else if existing_gitignore.as_deref() == Some(updated_gitignore.as_str()) {
        SetupActionKind::Unchanged
    } else if existing_gitignore.is_some() {
        SetupActionKind::UpdateGitignore
    } else {
        SetupActionKind::Create
    };
    let manages_gitignore_block = prior_owns_gitignore || !existing_has_entry;
    let managed_gitignore_file = prior_receipt
        .as_ref()
        .is_some_and(|receipt| receipt.managed_gitignore_file)
        || existing_gitignore.is_none();

    let desired_receipt =
        build_install_receipt(&managed, manages_gitignore_block, managed_gitignore_file);
    let desired_receipt_bytes = receipt_bytes(&desired_receipt)?;

    let mut actions = Vec::with_capacity(managed.len() + 4);
    let mut profile = generated_profile.clone();
    for file in &managed {
        validate_destination(&root, &file.relative_path)?;
        let destination = root.join(&file.relative_path);
        let preflight = preflight_setup_file(
            &destination,
            file,
            SetupPreflight {
                prior_sha256: prior_hashes
                    .get(&normalized_path(&file.relative_path))
                    .map(String::as_str),
                prior_receipt_loaded: prior_receipt.is_some(),
                dry_run: request.dry_run,
                replace_managed: request.replace_managed,
                generated_profile: &generated_profile,
            },
        )?;
        if let Some(repository_profile) = preflight.profile {
            profile = repository_profile;
        }
        actions.push(SetupAction {
            path: file.relative_path.clone(),
            action: preflight.action,
            requires_replace_managed: preflight.requires_replace_managed,
        });
    }

    let desired_paths = desired_receipt
        .managed_files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<HashSet<_>>();
    let mut retired_paths = Vec::new();
    if let Some(prior) = &prior_receipt {
        for prior_file in &prior.managed_files {
            if desired_paths.contains(prior_file.path.as_str()) {
                continue;
            }
            let relative = PathBuf::from(&prior_file.path);
            validate_managed_text_path(&relative)?;
            validate_destination(&root, &relative)?;
            let destination = root.join(&relative);
            if !destination.exists() {
                continue;
            }
            let existing = fs::read(&destination)
                .with_context(|| format!("read retired managed file {}", destination.display()))?;
            if managed_text_sha256(&existing) == prior_file.sha256 {
                retired_paths.push(relative.clone());
                actions.push(SetupAction {
                    path: relative,
                    action: SetupActionKind::RemoveRetired,
                    requires_replace_managed: false,
                });
            } else if request.dry_run {
                actions.push(SetupAction {
                    path: relative,
                    action: SetupActionKind::ModifiedRefusal,
                    requires_replace_managed: false,
                });
            } else {
                return Err(anyhow!(
                    "setup-project refuses modified retired managed file {}; move or delete it deliberately, then rerun setup-project",
                    destination.display()
                ));
            }
        }
    }

    actions.push(SetupAction {
        path: gitignore_relative.to_path_buf(),
        action: gitignore_action,
        requires_replace_managed: false,
    });

    let receipt_action = if receipt_path.exists() {
        let existing = fs::read(&receipt_path)
            .with_context(|| format!("read project-install receipt {}", receipt_path.display()))?;
        if managed_text_sha256(&existing) == managed_text_sha256(&desired_receipt_bytes) {
            SetupActionKind::Unchanged
        } else {
            SetupActionKind::Replace
        }
    } else {
        SetupActionKind::Create
    };
    actions.push(SetupAction {
        path: receipt_relative.to_path_buf(),
        action: receipt_action,
        requires_replace_managed: false,
    });

    let ready = actions.iter().all(|action| {
        action.action != SetupActionKind::ModifiedRefusal && !action.requires_replace_managed
    });

    if !request.dry_run {
        for (file, action) in managed.iter().zip(actions.iter()) {
            let destination = root.join(&file.relative_path);
            match action.action {
                SetupActionKind::Create => write_new_file(&destination, &file.content)?,
                SetupActionKind::Replace => fs::write(&destination, &file.content)
                    .with_context(|| format!("replace managed file {}", destination.display()))?,
                SetupActionKind::PreserveRepositoryOwned => {}
                SetupActionKind::MakeExecutable => {}
                SetupActionKind::Unchanged => {}
                SetupActionKind::UpdateGitignore
                | SetupActionKind::RemoveManaged
                | SetupActionKind::RemoveRetired
                | SetupActionKind::ModifiedRefusal => {
                    unreachable!("managed files never use the gitignore action")
                }
            }
            if file.executable {
                ensure_executable(&destination)?;
            }
        }
        for relative in &retired_paths {
            fs::remove_file(root.join(relative)).with_context(|| {
                format!(
                    "remove retired managed file {}",
                    root.join(relative).display()
                )
            })?;
        }
        remove_empty_managed_directories(&root)?;
        if !matches!(gitignore_action, SetupActionKind::Unchanged) {
            if let Some(parent) = gitignore_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create directory {}", parent.display()))?;
            }
            fs::write(&gitignore_path, updated_gitignore)
                .with_context(|| format!("write {}", gitignore_path.display()))?;
        }
        match receipt_action {
            SetupActionKind::Create => write_new_file(&receipt_path, &desired_receipt_bytes)?,
            SetupActionKind::Replace => fs::write(&receipt_path, &desired_receipt_bytes)
                .with_context(|| {
                    format!("replace project-install receipt {}", receipt_path.display())
                })?,
            SetupActionKind::Unchanged => {}
            _ => unreachable!("project-install receipt uses create, replace, or unchanged"),
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
        schema: "contextmink.project_setup.v2",
        project_root,
        profile,
        dry_run: request.dry_run,
        ready,
        actions,
        agent_guidance_files_found,
        next_actions: vec![
            "Review .contextmink.toml and add only project-specific generated or high-output exclude globs."
                .to_owned(),
            "Add repository-owned destructive-guard fragments only for critical paths that require a deletion tripwire."
                .to_owned(),
            "Review the installed Contextmink skill and tools/contextmink/agent_integration.md, then add one concise repository-guidance trigger for broad or potentially high-output reads; setup-project never edits AGENTS.md or CLAUDE.md."
                .to_owned(),
            "Verify the project-local entrypoint from every supported agent shell and a representative nested working directory; require the intended profile and contextmink.receipt.v2."
                .to_owned(),
            "Inventory nested Git repositories, decide whether broad scans may cross them or require exact roots/--skip-nested-repos, and verify nested_repos_entered_total plus nested_repos_entered_sample."
                .to_owned(),
            "Run the project-local guard-check -- git clean from the repository root and confirm the decision is deny."
                .to_owned(),
            "Document the fresh-clone install step: rerunning setup-project preserves tracked configuration and restores ignored host binaries."
                .to_owned(),
            "To remove Contextmink later, run uninstall-project from an extracted release binary outside the project; it removes only receipt-owned integration files and preserves repository-owned configuration and guidance."
                .to_owned(),
        ],
    })
}

/// Remove only receipt-owned Contextmink integration surfaces. Repository-owned
/// configuration and always-loaded guidance remain explicit project decisions.
pub(crate) fn uninstall_project(
    request: UninstallProjectRequest<'_>,
) -> Result<UninstallProjectResult> {
    let root = resolve_project_root(request.project_root, "uninstall-project")?;
    let receipt_relative = Path::new(INSTALL_RECEIPT_PATH);
    validate_destination(&root, receipt_relative)?;
    let receipt_path = root.join(receipt_relative);
    let receipt = load_install_receipt(&receipt_path)?.ok_or_else(|| {
        anyhow!(
            "uninstall-project cannot prove managed-file ownership because {} is missing; restore the receipt or remove reviewed paths manually",
            receipt_path.display()
        )
    })?;
    refuse_release_downgrade(&receipt, "uninstall-project")?;

    let running_binary = match request.running_binary {
        Some(path) => fs::canonicalize(path)
            .with_context(|| format!("resolve running Contextmink binary {}", path.display()))?,
        None => fs::canonicalize(std::env::current_exe()?)
            .context("resolve running Contextmink binary")?,
    };
    for relative in &receipt.managed_runtime_paths {
        let destination = root.join(relative);
        if destination.exists()
            && fs::canonicalize(&destination)
                .with_context(|| format!("resolve managed runtime {}", destination.display()))?
                == running_binary
        {
            return Err(anyhow!(
                "uninstall-project cannot remove the running project-local binary {}; run uninstall-project from an extracted Contextmink release outside the project",
                destination.display()
            ));
        }
    }

    let mut actions = Vec::new();
    let mut removable_paths = Vec::new();
    for file in &receipt.managed_files {
        let relative = PathBuf::from(&file.path);
        validate_managed_text_path(&relative)?;
        validate_destination(&root, &relative)?;
        let destination = root.join(&relative);
        if !destination.exists() {
            actions.push(SetupAction {
                path: relative,
                action: SetupActionKind::Unchanged,
                requires_replace_managed: false,
            });
            continue;
        }
        let existing = fs::read(&destination)
            .with_context(|| format!("read managed file {}", destination.display()))?;
        if managed_text_sha256(&existing) == file.sha256 {
            removable_paths.push(relative.clone());
            actions.push(SetupAction {
                path: relative,
                action: SetupActionKind::RemoveManaged,
                requires_replace_managed: false,
            });
        } else if request.dry_run {
            actions.push(SetupAction {
                path: relative,
                action: SetupActionKind::ModifiedRefusal,
                requires_replace_managed: false,
            });
        } else {
            return Err(anyhow!(
                "uninstall-project refuses modified managed file {}; move or delete it deliberately, then rerun uninstall-project",
                destination.display()
            ));
        }
    }

    for relative in &receipt.managed_runtime_paths {
        let relative = PathBuf::from(relative);
        validate_destination(&root, &relative)?;
        let destination = root.join(&relative);
        if destination.exists() {
            if !destination.is_file() {
                return Err(anyhow!(
                    "uninstall-project managed runtime is not a file: {}",
                    destination.display()
                ));
            }
            removable_paths.push(relative.clone());
            actions.push(SetupAction {
                path: relative,
                action: SetupActionKind::RemoveManaged,
                requires_replace_managed: false,
            });
        } else {
            actions.push(SetupAction {
                path: relative,
                action: SetupActionKind::Unchanged,
                requires_replace_managed: false,
            });
        }
    }

    let gitignore_relative = Path::new(".gitignore");
    validate_destination(&root, gitignore_relative)?;
    let gitignore_path = root.join(gitignore_relative);
    let existing_gitignore = if gitignore_path.exists() {
        Some(
            fs::read_to_string(&gitignore_path)
                .with_context(|| format!("read {} as UTF-8", gitignore_path.display()))?,
        )
    } else {
        None
    };
    let updated_gitignore = if receipt.managed_gitignore_block {
        existing_gitignore
            .as_deref()
            .map(gitignore_without_managed_block)
            .transpose()?
    } else {
        existing_gitignore.clone()
    };
    let gitignore_changed = existing_gitignore != updated_gitignore;
    let remove_gitignore_file = receipt.managed_gitignore_file
        && existing_gitignore.is_some()
        && updated_gitignore.as_deref() == Some("");
    actions.push(SetupAction {
        path: gitignore_relative.to_path_buf(),
        action: if remove_gitignore_file {
            SetupActionKind::RemoveManaged
        } else if gitignore_changed {
            SetupActionKind::UpdateGitignore
        } else {
            SetupActionKind::Unchanged
        },
        requires_replace_managed: false,
    });
    actions.push(SetupAction {
        path: receipt_relative.to_path_buf(),
        action: SetupActionKind::RemoveManaged,
        requires_replace_managed: false,
    });

    let ready = actions
        .iter()
        .all(|action| action.action != SetupActionKind::ModifiedRefusal);
    if !request.dry_run {
        for relative in &removable_paths {
            fs::remove_file(root.join(relative)).with_context(|| {
                format!("remove managed file {}", root.join(relative).display())
            })?;
        }
        if remove_gitignore_file {
            fs::remove_file(&gitignore_path).with_context(|| {
                format!("remove installer-created {}", gitignore_path.display())
            })?;
        } else if gitignore_changed {
            fs::write(&gitignore_path, updated_gitignore.unwrap_or_default())
                .with_context(|| format!("update {}", gitignore_path.display()))?;
        }
        fs::remove_file(&receipt_path).with_context(|| {
            format!("remove project-install receipt {}", receipt_path.display())
        })?;
        remove_empty_managed_directories(&root)?;
    }

    let preserved_repository_owned = [".contextmink.toml", "AGENTS.md", "CLAUDE.md"]
        .into_iter()
        .map(PathBuf::from)
        .filter(|path| root.join(path).is_file())
        .collect();
    Ok(UninstallProjectResult {
        schema: "contextmink.project_uninstall.v1",
        project_root: canonical_normalized(&root)
            .expect("uninstall-project root was canonicalized before rendering"),
        dry_run: request.dry_run,
        ready,
        actions,
        preserved_repository_owned,
        next_actions: vec![
            "Review repository-owned AGENTS.md and CLAUDE.md and remove any Contextmink trigger that is no longer wanted."
                .to_owned(),
            "Keep or deliberately remove .contextmink.toml; uninstall-project preserves it because setup transfers configuration ownership to the repository."
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

fn preflight_setup_file(
    destination: &Path,
    file: &ManagedFile,
    preflight: SetupPreflight<'_>,
) -> Result<PreflightFile> {
    if !destination.exists() {
        return Ok(PreflightFile {
            action: SetupActionKind::Create,
            profile: matches!(file.ownership, SetupFileOwnership::RepositoryOwnedConfig)
                .then(|| preflight.generated_profile.to_owned()),
            requires_replace_managed: false,
        });
    }
    if !destination.is_file() {
        return Err(anyhow!(
            "setup-project destination is not a file: {}",
            destination.display()
        ));
    }
    if file.ownership == SetupFileOwnership::RepositoryOwnedConfig {
        let config = load_context_config(Some(destination), false).with_context(|| {
            format!(
                "setup-project cannot preserve invalid repository-owned configuration {}",
                destination.display()
            )
        })?;
        let profile = config.profile.ok_or_else(|| {
            anyhow!(
                "setup-project cannot preserve repository-owned configuration {} without a profile",
                destination.display()
            )
        })?;
        return Ok(PreflightFile {
            action: SetupActionKind::PreserveRepositoryOwned,
            profile: Some(profile),
            requires_replace_managed: false,
        });
    }
    let existing = fs::read(destination).with_context(|| {
        format!(
            "read existing release-managed file {}",
            destination.display()
        )
    })?;
    let content_matches = match file.ownership {
        SetupFileOwnership::ReleaseManagedText => {
            managed_text_sha256(&existing) == managed_text_sha256(&file.content)
        }
        SetupFileOwnership::ReleaseManagedRuntime => existing == file.content,
        SetupFileOwnership::RepositoryOwnedConfig => {
            unreachable!("repository-owned configuration returned before content comparison")
        }
    };
    if !content_matches {
        let receipt_owned = match file.ownership {
            SetupFileOwnership::ReleaseManagedText => preflight
                .prior_sha256
                .is_some_and(|sha256| managed_text_sha256(&existing) == sha256),
            SetupFileOwnership::ReleaseManagedRuntime => preflight.prior_receipt_loaded,
            SetupFileOwnership::RepositoryOwnedConfig => false,
        };
        if receipt_owned || preflight.replace_managed {
            return Ok(PreflightFile {
                action: SetupActionKind::Replace,
                profile: None,
                requires_replace_managed: false,
            });
        }
        if preflight.dry_run {
            return Ok(PreflightFile {
                action: SetupActionKind::Replace,
                profile: None,
                requires_replace_managed: true,
            });
        }
        return Err(anyhow!(
            "setup-project found divergent release-managed file {}; rerun with --replace-managed after reviewing the replacement",
            destination.display()
        ));
    }
    let action = if file.executable && executable_bit_missing(destination)? {
        SetupActionKind::MakeExecutable
    } else {
        SetupActionKind::Unchanged
    };
    Ok(PreflightFile {
        action,
        profile: None,
        requires_replace_managed: false,
    })
}

fn resolve_project_root(project_root: &Path, operation: &str) -> Result<PathBuf> {
    let root = fs::canonicalize(project_root)
        .with_context(|| format!("resolve {operation} root {}", project_root.display()))?;
    if !root.is_dir() {
        return Err(anyhow!(
            "{operation} root {} is not an existing directory",
            root.display()
        ));
    }
    Ok(root)
}

fn validate_destination(root: &Path, relative: &Path) -> Result<()> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(anyhow!(
            "managed destination must be a normalized project-relative path: {}",
            relative.display()
        ));
    }
    let mut current = root.to_path_buf();
    let component_count = relative.components().count();
    for (index, component) in relative.components().enumerate() {
        let Component::Normal(name) = component else {
            unreachable!("relative components were validated above")
        };
        current.push(name);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("inspect managed destination {}", current.display()));
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(anyhow!(
                "managed destination traverses symlink {}; move the link or choose a regular project tree",
                current.display()
            ));
        }
        if index + 1 < component_count && !metadata.is_dir() {
            return Err(anyhow!(
                "managed destination parent is not a directory: {}",
                current.display()
            ));
        }
    }
    Ok(())
}

pub(super) fn normalized_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
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

fn gitignore_has_entry(existing: &str) -> bool {
    existing.lines().any(|line| line.trim() == GITIGNORE_ENTRY)
}

fn gitignore_managed_block_count(existing: &str) -> usize {
    existing
        .lines()
        .collect::<Vec<_>>()
        .windows(2)
        .filter(|pair| pair[0].trim() == GITIGNORE_COMMENT && pair[1].trim() == GITIGNORE_ENTRY)
        .count()
}

fn gitignore_without_managed_block(existing: &str) -> Result<String> {
    let lines = existing.lines().collect::<Vec<_>>();
    let matches = lines
        .windows(2)
        .enumerate()
        .filter_map(|(index, pair)| {
            (pair[0].trim() == GITIGNORE_COMMENT && pair[1].trim() == GITIGNORE_ENTRY)
                .then_some(index)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Ok(existing.to_owned()),
        [start] => {
            let mut retained = lines
                .iter()
                .enumerate()
                .filter_map(|(index, line)| {
                    (index != *start && index != *start + 1).then_some(*line)
                })
                .collect::<Vec<_>>();
            while retained.last().is_some_and(|line| line.is_empty()) {
                retained.pop();
            }
            let mut output = retained.join("\n");
            if !output.is_empty() {
                output.push('\n');
            }
            Ok(output)
        }
        _ => Err(anyhow!(
            "uninstall-project found multiple Contextmink-managed blocks in .gitignore; consolidate them deliberately before retrying"
        )),
    }
}

fn remove_empty_managed_directories(root: &Path) -> Result<()> {
    for relative in [
        ".agents/skills/contextmink/agents",
        ".agents/skills/contextmink",
        ".claude/skills/contextmink",
        ".agents/skills/changelog-writing/agents",
        ".agents/skills/changelog-writing",
        ".claude/skills/changelog-writing",
        "tools/contextmink/bin",
        "tools/contextmink",
    ] {
        let path = root.join(relative);
        if !path.exists() {
            continue;
        }
        match fs::remove_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("remove empty managed directory {}", path.display()));
            }
        }
    }
    Ok(())
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
        for path in [
            ".agents/skills/contextmink/SKILL.md",
            ".agents/skills/contextmink/agents/openai.yaml",
            ".claude/skills/contextmink/SKILL.md",
        ] {
            assert!(result.actions.iter().any(|action| {
                action.path == Path::new(path) && action.action == SetupActionKind::Create
            }));
        }
        assert!(
            result
                .actions
                .iter()
                .all(|action| !action.path.to_string_lossy().contains("changelog-writing"))
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
            CONTEXTMINK_INTEGRATION
        );
        assert_eq!(
            fs::read(project.join(".agents/skills/contextmink/SKILL.md")).unwrap(),
            CONTEXTMINK_SKILL
        );
        assert_eq!(
            fs::read(project.join(".claude/skills/contextmink/SKILL.md")).unwrap(),
            CONTEXTMINK_SKILL
        );
        assert_eq!(
            fs::read(project.join(".agents/skills/contextmink/agents/openai.yaml")).unwrap(),
            CONTEXTMINK_OPENAI_METADATA
        );
        assert!(project.join(INSTALL_RECEIPT_PATH).is_file());
        assert!(
            fs::read_to_string(project.join("scripts/contextmink.cmd"))
                .unwrap()
                .contains("requires Git Bash")
        );
        if cfg!(windows) {
            assert_eq!(
                fs::read(project.join("tools/contextmink/bin/contextmink-bridge.exe")).unwrap(),
                b"contextmink-bridge-binary"
            );
        }

        let second = setup_project(request(&project, &binary, false)).unwrap();
        assert!(
            second
                .actions
                .iter()
                .filter(|action| action.path != Path::new(".contextmink.toml"))
                .all(|action| action.action == SetupActionKind::Unchanged)
        );
        assert!(second.actions.iter().any(|action| {
            action.path == Path::new(".contextmink.toml")
                && action.action == SetupActionKind::PreserveRepositoryOwned
        }));
        assert_eq!(second.profile, "demo-project");
        let gitignore = fs::read_to_string(project.join(".gitignore")).unwrap();
        assert_eq!(gitignore.matches(GITIGNORE_ENTRY).count(), 1);
        cleanup(&project);
    }

    #[test]
    fn customized_configuration_is_preserved_while_missing_binaries_are_restored() {
        let (project, binary) = fixture("preserve-config");
        setup_project(request(&project, &binary, false)).unwrap();
        let config = "profile = \"owned-profile\"\nexclude_globs = [\"generated/**\"]\n";
        fs::write(project.join(".contextmink.toml"), config).unwrap();
        let installed_binary = project.join(format!(
            "tools/contextmink/bin/contextmink{}",
            std::env::consts::EXE_SUFFIX
        ));
        fs::remove_file(&installed_binary).unwrap();

        let dry_run = setup_project(request(&project, &binary, true)).unwrap();
        assert_eq!(dry_run.profile, "owned-profile");
        assert!(dry_run.actions.iter().any(|action| {
            action.path == Path::new(".contextmink.toml")
                && action.action == SetupActionKind::PreserveRepositoryOwned
        }));
        assert!(dry_run.actions.iter().any(|action| {
            action.path
                == Path::new(&format!(
                    "tools/contextmink/bin/contextmink{}",
                    std::env::consts::EXE_SUFFIX
                ))
                && action.action == SetupActionKind::Create
        }));
        assert!(!installed_binary.exists());
        assert_eq!(
            fs::read_to_string(project.join(".contextmink.toml")).unwrap(),
            config
        );

        let applied = setup_project(request(&project, &binary, false)).unwrap();
        assert_eq!(applied.profile, "owned-profile");
        assert!(installed_binary.is_file());
        assert_eq!(
            fs::read_to_string(project.join(".contextmink.toml")).unwrap(),
            config
        );
        cleanup(&project);
    }

    #[test]
    fn divergent_managed_file_refuses_before_any_write() {
        let (project, binary) = fixture("divergence");
        setup_project(request(&project, &binary, false)).unwrap();
        fs::remove_file(project.join(".contextmink.toml")).unwrap();
        fs::write(project.join("scripts/contextmink"), b"locally changed").unwrap();

        let error = setup_project(request(&project, &binary, false)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("found divergent release-managed file")
        );
        assert!(!project.join(".contextmink.toml").exists());
        cleanup(&project);
    }

    #[test]
    fn dry_run_reports_release_replacement_without_mutation_authority() {
        let (project, binary) = fixture("dry-run-replacement");
        setup_project(request(&project, &binary, false)).unwrap();
        fs::write(project.join("scripts/contextmink"), b"older release").unwrap();
        let owned_config = "profile = \"owned\"\nexclude_globs = [\"cache/**\"]\n";
        fs::write(project.join(".contextmink.toml"), owned_config).unwrap();

        let result = setup_project(request(&project, &binary, true)).unwrap();
        assert_eq!(result.profile, "owned");
        assert!(result.actions.iter().any(|action| {
            action.path == Path::new("scripts/contextmink")
                && action.action == SetupActionKind::Replace
                && action.requires_replace_managed
        }));
        assert!(!result.ready);
        assert!(result.actions.iter().any(|action| {
            action.path == Path::new(".contextmink.toml")
                && action.action == SetupActionKind::PreserveRepositoryOwned
        }));
        assert_eq!(
            fs::read(project.join("scripts/contextmink")).unwrap(),
            b"older release"
        );
        assert_eq!(
            fs::read_to_string(project.join(".contextmink.toml")).unwrap(),
            owned_config
        );
        cleanup(&project);
    }

    #[test]
    fn explicit_replacement_updates_release_files_and_preserves_configuration() {
        let (project, binary) = fixture("replace-managed");
        setup_project(request(&project, &binary, false)).unwrap();
        fs::write(project.join("scripts/contextmink"), b"older release").unwrap();
        fs::write(
            project.join(".agents/skills/contextmink/SKILL.md"),
            b"older release",
        )
        .unwrap();
        let owned_config = "profile = \"owned\"\nexclude_globs = [\"cache/**\"]\n";
        fs::write(project.join(".contextmink.toml"), owned_config).unwrap();

        let mut replace = request(&project, &binary, false);
        replace.replace_managed = true;
        let result = setup_project(replace).unwrap();
        assert_eq!(result.profile, "owned");
        assert!(result.actions.iter().any(|action| {
            action.path == Path::new("scripts/contextmink")
                && action.action == SetupActionKind::Replace
        }));
        assert!(result.actions.iter().any(|action| {
            action.path == Path::new(".agents/skills/contextmink/SKILL.md")
                && action.action == SetupActionKind::Replace
        }));
        assert!(result.actions.iter().any(|action| {
            action.path == Path::new(".contextmink.toml")
                && action.action == SetupActionKind::PreserveRepositoryOwned
        }));
        assert_eq!(
            fs::read(project.join("scripts/contextmink")).unwrap(),
            BASH_LAUNCHER
        );
        assert_eq!(
            fs::read(project.join(".agents/skills/contextmink/SKILL.md")).unwrap(),
            CONTEXTMINK_SKILL
        );
        assert_eq!(
            fs::read_to_string(project.join(".contextmink.toml")).unwrap(),
            owned_config
        );
        cleanup(&project);
    }

    #[test]
    fn invalid_repository_configuration_refuses_before_release_replacement() {
        let (project, binary) = fixture("invalid-config");
        setup_project(request(&project, &binary, false)).unwrap();
        fs::write(project.join("scripts/contextmink"), b"older release").unwrap();
        fs::write(
            project.join(".contextmink.toml"),
            "profile = \"owned\"\nunknown_key = true\n",
        )
        .unwrap();

        let mut replace = request(&project, &binary, false);
        replace.replace_managed = true;
        let error = setup_project(replace).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cannot preserve invalid repository-owned configuration")
        );
        assert_eq!(
            fs::read(project.join("scripts/contextmink")).unwrap(),
            b"older release"
        );
        cleanup(&project);
    }

    #[test]
    fn receipt_owned_text_upgrades_without_replacement_authority() {
        let (project, binary) = fixture("receipt-upgrade");
        setup_project(request(&project, &binary, false)).unwrap();
        let prior = b"prior managed launcher\n";
        fs::write(project.join("scripts/contextmink"), prior).unwrap();
        let receipt_path = project.join(INSTALL_RECEIPT_PATH);
        let mut install_receipt = load_install_receipt(&receipt_path).unwrap().unwrap();
        install_receipt
            .managed_files
            .iter_mut()
            .find(|file| file.path == "scripts/contextmink")
            .unwrap()
            .sha256 = managed_text_sha256(prior);
        fs::write(&receipt_path, receipt_bytes(&install_receipt).unwrap()).unwrap();

        let result = setup_project(request(&project, &binary, false)).unwrap();
        assert!(result.actions.iter().any(|action| {
            action.path == Path::new("scripts/contextmink")
                && action.action == SetupActionKind::Replace
                && !action.requires_replace_managed
        }));
        assert_eq!(
            fs::read(project.join("scripts/contextmink")).unwrap(),
            BASH_LAUNCHER
        );
        cleanup(&project);
    }

    #[test]
    fn receipt_retires_only_hash_matching_obsolete_skill_files() {
        let (project, binary) = fixture("receipt-retirement");
        setup_project(request(&project, &binary, false)).unwrap();
        let retired_relative = Path::new(".agents/skills/changelog-writing/SKILL.md");
        let retired = b"formerly managed general skill\n";
        fs::create_dir_all(project.join(retired_relative).parent().unwrap()).unwrap();
        fs::write(project.join(retired_relative), retired).unwrap();
        let receipt_path = project.join(INSTALL_RECEIPT_PATH);
        let mut install_receipt = load_install_receipt(&receipt_path).unwrap().unwrap();
        install_receipt
            .managed_files
            .push(receipt::ManagedFileReceipt {
                path: normalized_path(retired_relative),
                sha256: managed_text_sha256(retired),
            });
        fs::write(&receipt_path, receipt_bytes(&install_receipt).unwrap()).unwrap();

        let result = setup_project(request(&project, &binary, false)).unwrap();
        assert!(result.actions.iter().any(|action| {
            action.path == retired_relative && action.action == SetupActionKind::RemoveRetired
        }));
        assert!(!project.join(retired_relative).exists());
        cleanup(&project);
    }

    #[test]
    fn modified_retired_skill_refuses_without_deleting_current_files() {
        let (project, binary) = fixture("modified-retirement");
        setup_project(request(&project, &binary, false)).unwrap();
        let retired_relative = Path::new(".agents/skills/changelog-writing/SKILL.md");
        fs::create_dir_all(project.join(retired_relative).parent().unwrap()).unwrap();
        fs::write(project.join(retired_relative), b"project-owned version\n").unwrap();
        let receipt_path = project.join(INSTALL_RECEIPT_PATH);
        let mut install_receipt = load_install_receipt(&receipt_path).unwrap().unwrap();
        install_receipt
            .managed_files
            .push(receipt::ManagedFileReceipt {
                path: normalized_path(retired_relative),
                sha256: managed_text_sha256(b"old managed version\n"),
            });
        fs::write(&receipt_path, receipt_bytes(&install_receipt).unwrap()).unwrap();

        let dry_run = setup_project(request(&project, &binary, true)).unwrap();
        assert!(!dry_run.ready);
        assert!(dry_run.actions.iter().any(|action| {
            action.path == retired_relative && action.action == SetupActionKind::ModifiedRefusal
        }));
        let error = setup_project(request(&project, &binary, false)).unwrap_err();
        assert!(error.to_string().contains("modified retired managed file"));
        assert_eq!(
            fs::read(project.join(retired_relative)).unwrap(),
            b"project-owned version\n"
        );
        cleanup(&project);
    }

    #[test]
    fn uninstall_removes_receipt_owned_surfaces_and_preserves_project_policy() {
        let (project, binary) = fixture("uninstall");
        fs::write(project.join("AGENTS.md"), "project guidance\n").unwrap();
        setup_project(request(&project, &binary, false)).unwrap();
        let config = fs::read(project.join(".contextmink.toml")).unwrap();

        let result = uninstall_project(UninstallProjectRequest {
            project_root: &project,
            running_binary: Some(&binary),
            dry_run: false,
        })
        .unwrap();
        assert!(result.ready);
        for relative in [
            INSTALL_RECEIPT_PATH,
            "scripts/contextmink",
            "scripts/contextmink.cmd",
            "tools/contextmink/agent_integration.md",
            ".agents/skills/contextmink/SKILL.md",
            ".agents/skills/contextmink/agents/openai.yaml",
            ".claude/skills/contextmink/SKILL.md",
        ] {
            assert!(
                !project.join(relative).exists(),
                "{relative} must be removed"
            );
        }
        assert_eq!(fs::read(project.join(".contextmink.toml")).unwrap(), config);
        assert_eq!(
            fs::read_to_string(project.join("AGENTS.md")).unwrap(),
            "project guidance\n"
        );
        assert!(!project.join(".gitignore").exists());
        cleanup(&project);
    }

    #[test]
    fn uninstall_refuses_modified_skill_before_removing_anything() {
        let (project, binary) = fixture("uninstall-modified");
        setup_project(request(&project, &binary, false)).unwrap();
        let skill = project.join(".agents/skills/contextmink/SKILL.md");
        fs::write(&skill, b"project modification\n").unwrap();

        let dry_run = uninstall_project(UninstallProjectRequest {
            project_root: &project,
            running_binary: Some(&binary),
            dry_run: true,
        })
        .unwrap();
        assert!(!dry_run.ready);
        let error = uninstall_project(UninstallProjectRequest {
            project_root: &project,
            running_binary: Some(&binary),
            dry_run: false,
        })
        .unwrap_err();
        assert!(error.to_string().contains("refuses modified managed file"));
        assert!(project.join(INSTALL_RECEIPT_PATH).is_file());
        assert!(project.join("scripts/contextmink").is_file());
        cleanup(&project);
    }

    #[test]
    fn uninstall_preserves_a_preexisting_repository_owned_gitignore_entry() {
        let (project, binary) = fixture("uninstall-owned-gitignore");
        let original = format!("{GITIGNORE_COMMENT}\n{GITIGNORE_ENTRY}\n");
        fs::write(project.join(".gitignore"), &original).unwrap();
        setup_project(request(&project, &binary, false)).unwrap();
        let install_receipt = load_install_receipt(&project.join(INSTALL_RECEIPT_PATH))
            .unwrap()
            .unwrap();
        assert!(!install_receipt.managed_gitignore_block);
        assert!(!install_receipt.managed_gitignore_file);

        uninstall_project(UninstallProjectRequest {
            project_root: &project,
            running_binary: Some(&binary),
            dry_run: false,
        })
        .unwrap();
        assert_eq!(
            fs::read_to_string(project.join(".gitignore")).unwrap(),
            original
        );
        cleanup(&project);
    }

    #[test]
    fn setup_refuses_a_modified_receipt_owned_gitignore_block() {
        let (project, binary) = fixture("modified-gitignore");
        setup_project(request(&project, &binary, false)).unwrap();
        fs::write(project.join(".gitignore"), format!("{GITIGNORE_ENTRY}\n")).unwrap();

        let dry_run = setup_project(request(&project, &binary, true)).unwrap();
        assert!(!dry_run.ready);
        assert!(dry_run.actions.iter().any(|action| {
            action.path == Path::new(".gitignore")
                && action.action == SetupActionKind::ModifiedRefusal
        }));
        let error = setup_project(request(&project, &binary, false)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("modified Contextmink-managed .gitignore block")
        );
        cleanup(&project);
    }

    #[test]
    fn uninstall_refuses_to_remove_the_running_project_binary() {
        let (project, binary) = fixture("uninstall-self");
        setup_project(request(&project, &binary, false)).unwrap();
        let installed = project.join(format!(
            "tools/contextmink/bin/contextmink{}",
            std::env::consts::EXE_SUFFIX
        ));
        let error = uninstall_project(UninstallProjectRequest {
            project_root: &project,
            running_binary: Some(&installed),
            dry_run: true,
        })
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cannot remove the running project-local binary")
        );
        cleanup(&project);
    }
}
