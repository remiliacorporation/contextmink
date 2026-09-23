use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use super::{ContextminkConfig, canonical_normalized, load_context_config, validate_profile};

#[path = "project_setup_receipt.rs"]
pub(crate) mod receipt;

use receipt::{
    INSTALL_RECEIPT_PATH, MANAGED_RUNTIME_PATHS, PREVIOUS_RUNTIME_RECEIPT_PATH,
    build_install_receipt, load_install_receipt, previous_runtime_receipt_exists, receipt_bytes,
    refuse_release_downgrade, same_managed_text,
};

const BASH_LAUNCHER: &[u8] = include_bytes!("../templates/scripts/contextmink");
const CONTEXTMINK_INTEGRATION: &[u8] = include_bytes!("../templates/agent_integration.md");
const CONTEXTMINK_SKILL: &[u8] = include_bytes!("../templates/skills/contextmink/SKILL.md");
const BRIDGE_SKILL: &[u8] = include_bytes!("../templates/skills/contextmink-bridge/SKILL.md");
const CONTEXTMINK_OPENAI_METADATA: &[u8] =
    include_bytes!("../templates/skills/contextmink/agents/openai.yaml");
const GITIGNORE_COMMENT: &str = "# contextmink project-local release binaries";
const GITIGNORE_ENTRY: &str = "/tools/contextmink/bin/";
const AGENTS_SKILL_PATHS: &[&str] = &[
    ".agents/skills/contextmink/SKILL.md",
    ".agents/skills/contextmink/agents/openai.yaml",
    ".agents/skills/contextmink-bridge/SKILL.md",
];
const CLAUDE_SKILL_PATHS: &[&str] = &[
    ".claude/skills/contextmink/SKILL.md",
    ".claude/skills/contextmink-bridge/SKILL.md",
];
const SHARED_TEXT_PATHS: &[&str] = &[
    "scripts/contextmink",
    "tools/contextmink/agent_integration.md",
];
// Compatibility is anchored in the shared `.agents/skills` contract. The
// bootstrap catalog only selects that shared residence for common compatible
// harnesses before `.agents` exists; it never creates harness-native copies.
const SHARED_AGENT_SKILLS_DIRECTORIES: &[&str] = &[".agents"];
const COMMON_AGENT_SKILLS_BOOTSTRAP_DIRECTORIES: &[&str] =
    &[".codex", ".cursor", ".pi", ".omp", ".opencode"];
const COMMON_AGENT_SKILLS_BOOTSTRAP_FILES: &[&str] =
    &["AGENTS.md", "opencode.json", "opencode.jsonc"];
const CLAUDE_HARNESS_DIRECTORIES: &[&str] = &[".claude"];
const CLAUDE_HARNESS_FILES: &[&str] = &["CLAUDE.md"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SkillTarget {
    Auto,
    Agents,
    Claude,
    Both,
    None,
}

impl SkillTarget {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Agents => "agents",
            Self::Claude => "claude",
            Self::Both => "both",
            Self::None => "none",
        }
    }

    fn installs_agents(self) -> bool {
        matches!(self, Self::Agents | Self::Both)
    }

    fn installs_claude(self) -> bool {
        matches!(self, Self::Claude | Self::Both)
    }

    /// Release-managed text paths a receipt with this target owns. The bridge
    /// skill paths are included on every platform: they are Contextmink's
    /// namespace even where the current host installs no bridge.
    fn owned_text_paths(self) -> Vec<&'static str> {
        let mut paths = SHARED_TEXT_PATHS.to_vec();
        if self.installs_agents() {
            paths.extend(AGENTS_SKILL_PATHS);
        }
        if self.installs_claude() {
            paths.extend(CLAUDE_SKILL_PATHS);
        }
        paths
    }
}

#[derive(Debug)]
pub(crate) struct SetupProjectRequest<'a> {
    pub(crate) project_root: &'a Path,
    /// Defaults to the running contextmink executable. Tests and embedding
    /// callers can supply an extracted release binary explicitly.
    pub(crate) source_binary: Option<&'a Path>,
    pub(crate) dry_run: bool,
    pub(crate) skill_target: SkillTarget,
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
    UnownedRefusal,
    ModifiedRefusal,
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
    pub(crate) ready: bool,
    pub(crate) requested_skill_target: SkillTarget,
    pub(crate) resolved_skill_target: SkillTarget,
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

fn contextmink_skill_files(target: SkillTarget, windows: bool) -> Vec<ManagedFile> {
    let mut files = Vec::new();
    if target.installs_agents() {
        files.extend([
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
        ]);
    }
    if target.installs_claude() {
        files.push(ManagedFile {
            relative_path: PathBuf::from(".claude/skills/contextmink/SKILL.md"),
            content: CONTEXTMINK_SKILL.to_vec(),
            executable: false,
            ownership: SetupFileOwnership::ReleaseManagedText,
        });
    }
    if windows {
        for (directory, selected) in [
            (".agents", target.installs_agents()),
            (".claude", target.installs_claude()),
        ] {
            if selected {
                files.push(ManagedFile {
                    relative_path: PathBuf::from(format!(
                        "{directory}/skills/contextmink-bridge/SKILL.md"
                    )),
                    content: BRIDGE_SKILL.to_vec(),
                    executable: false,
                    ownership: SetupFileOwnership::ReleaseManagedText,
                });
            }
        }
    }
    files
}

struct PreflightFile {
    action: SetupActionKind,
    profile: Option<String>,
}

/// Install a project-local release. Release-managed files are written as the
/// release ships them; repository-owned configuration is validated and kept.
/// Every destination is preflighted before the first mutation, so a refusal
/// cannot leave a partially installed project.
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
    let receipt_relative = Path::new(INSTALL_RECEIPT_PATH);
    validate_destination(&root, receipt_relative)?;
    let receipt_path = root.join(receipt_relative);
    let prior_receipt = load_install_receipt(&receipt_path)?;
    if let Some(receipt) = prior_receipt.as_ref() {
        refuse_release_downgrade(receipt, "setup-project")?;
    }
    let resolved_skill_target = resolve_skill_target(
        &root,
        request.skill_target,
        prior_receipt.as_ref().map(|receipt| receipt.skill_target),
    )?;
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
    managed.extend(contextmink_skill_files(
        resolved_skill_target,
        suffix == ".exe",
    ));
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

    let prior_text_paths = prior_receipt
        .as_ref()
        .map(|receipt| receipt.skill_target.owned_text_paths())
        .unwrap_or_default();
    let previous_runtime_receipt = Path::new(PREVIOUS_RUNTIME_RECEIPT_PATH);
    validate_destination(&root, previous_runtime_receipt)?;
    let remove_previous_runtime_receipt =
        previous_runtime_receipt_exists(&root.join(previous_runtime_receipt), "setup-project")?;
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

    let desired_receipt = build_install_receipt(
        resolved_skill_target,
        manages_gitignore_block,
        managed_gitignore_file,
    );
    let desired_receipt_bytes = receipt_bytes(&desired_receipt)?;

    let mut actions = Vec::with_capacity(managed.len() + 4);
    let mut profile = generated_profile.clone();
    for file in &managed {
        validate_destination(&root, &file.relative_path)?;
        let destination = root.join(&file.relative_path);
        let preflight = preflight_setup_file(&destination, file, &generated_profile)?;
        if let Some(repository_profile) = preflight.profile {
            profile = repository_profile;
        }
        actions.push(SetupAction {
            path: file.relative_path.clone(),
            action: preflight.action,
        });
    }

    let desired_paths = managed
        .iter()
        .filter(|file| file.ownership == SetupFileOwnership::ReleaseManagedText)
        .map(|file| normalized_path(&file.relative_path))
        .collect::<HashSet<_>>();
    let mut retired_paths = Vec::new();
    for path in &prior_text_paths {
        if desired_paths.contains(*path) {
            continue;
        }
        let relative = PathBuf::from(path);
        validate_destination(&root, &relative)?;
        let destination = root.join(&relative);
        if !destination.exists() {
            continue;
        }
        if !destination.is_file() {
            return Err(anyhow!(
                "setup-project cannot remove non-file managed path {}; move it aside deliberately, then rerun setup-project",
                destination.display()
            ));
        }
        retired_paths.push(relative.clone());
        actions.push(SetupAction {
            path: relative,
            action: SetupActionKind::RemoveRetired,
        });
    }
    // Binaries are owned by fixed path, so the previous release's runtime
    // receipt no longer records anything.
    if remove_previous_runtime_receipt {
        retired_paths.push(previous_runtime_receipt.to_path_buf());
        actions.push(SetupAction {
            path: previous_runtime_receipt.to_path_buf(),
            action: SetupActionKind::RemoveRetired,
        });
    }
    for path in AGENTS_SKILL_PATHS.iter().chain(CLAUDE_SKILL_PATHS) {
        if desired_paths.contains(*path) || prior_text_paths.contains(path) {
            continue;
        }
        let relative = PathBuf::from(path);
        validate_destination(&root, &relative)?;
        if root.join(&relative).exists() {
            if request.dry_run {
                actions.push(SetupAction {
                    path: relative,
                    action: SetupActionKind::UnownedRefusal,
                });
            } else {
                return Err(anyhow!(
                    "setup-project refuses unreceipted skill at deselected path {}; move or delete it deliberately, then rerun setup-project",
                    root.join(&relative).display()
                ));
            }
        }
    }

    actions.push(SetupAction {
        path: gitignore_relative.to_path_buf(),
        action: gitignore_action,
    });

    let receipt_action = if receipt_path.exists() {
        let existing = fs::read(&receipt_path)
            .with_context(|| format!("read project-install receipt {}", receipt_path.display()))?;
        if same_managed_text(&existing, &desired_receipt_bytes) {
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
    });

    let ready = actions.iter().all(|action| {
        !matches!(
            action.action,
            SetupActionKind::UnownedRefusal | SetupActionKind::ModifiedRefusal
        )
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
                | SetupActionKind::UnownedRefusal
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
    let skill_next_action = if resolved_skill_target == SkillTarget::None {
        "No Contextmink skill was selected. Use setup-user for personal discovery or rerun setup-project with --skill-target agents for a project skill. Neither requires edits to AGENTS.md or CLAUDE.md."
    } else {
        "Start a fresh agent session and verify the selected Contextmink skill is listed. No repository-guidance trigger is required for skills-capable harnesses; setup-project never edits AGENTS.md or CLAUDE.md."
    };
    let mut next_actions = vec![
        "Review .contextmink.toml and add only project-specific generated or high-output exclude globs."
            .to_owned(),
        "Add repository-owned destructive-guard fragments only for critical paths that require a deletion tripwire."
            .to_owned(),
        skill_next_action.to_owned(),
        "Verify the project-local entrypoint from every supported agent shell and a representative nested working directory; require the intended profile and contextmink.receipt.v2."
            .to_owned(),
        "Inventory nested Git repositories, decide whether broad scans may cross them or require exact roots/--skip-nested-repos, and verify nested_repos_entered_total plus nested_repos_entered_sample."
            .to_owned(),
        "Run the project-local guard-check -- git clean from the repository root and confirm the decision is deny."
            .to_owned(),
        "Document the fresh-clone install step: rerunning setup-project preserves tracked configuration and restores ignored host binaries."
            .to_owned(),
        "To remove Contextmink later, run uninstall-project from an extracted release binary outside the project; it removes Contextmink-owned integration files (including every host binary under tools/contextmink/bin) and preserves repository-owned configuration and guidance."
            .to_owned(),
    ];
    if actions
        .iter()
        .any(|action| action.action == SetupActionKind::UnownedRefusal)
    {
        next_actions.push(
            "Move or delete each unreceipted Contextmink skill at a deselected path, or select the matching skill target, then rerun setup-project."
                .to_owned(),
        );
    }
    Ok(SetupProjectResult {
        schema: "contextmink.project_setup.v3",
        project_root,
        profile,
        dry_run: request.dry_run,
        ready,
        requested_skill_target: request.skill_target,
        resolved_skill_target,
        actions,
        agent_guidance_files_found,
        next_actions,
    })
}

/// Remove Contextmink-owned integration surfaces: the receipt-selected skills and
/// reference plus every host binary at the fixed tools/contextmink/bin paths. Repository-owned
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

    let previous_runtime_receipt = Path::new(PREVIOUS_RUNTIME_RECEIPT_PATH);
    validate_destination(&root, previous_runtime_receipt)?;
    let remove_previous_runtime_receipt =
        previous_runtime_receipt_exists(&root.join(previous_runtime_receipt), "uninstall-project")?;

    let running_binary = match request.running_binary {
        Some(path) => fs::canonicalize(path)
            .with_context(|| format!("resolve running Contextmink binary {}", path.display()))?,
        None => fs::canonicalize(std::env::current_exe()?)
            .context("resolve running Contextmink binary")?,
    };
    let mut actions = Vec::new();
    let mut removable_paths = Vec::new();
    for path in receipt.skill_target.owned_text_paths() {
        let relative = PathBuf::from(path);
        validate_destination(&root, &relative)?;
        let destination = root.join(&relative);
        if !destination.exists() {
            continue;
        }
        if !destination.is_file() {
            return Err(anyhow!(
                "uninstall-project cannot remove non-file managed path {}; move it aside deliberately, then rerun uninstall-project",
                destination.display()
            ));
        }
        removable_paths.push(relative.clone());
        actions.push(SetupAction {
            path: relative,
            action: SetupActionKind::RemoveManaged,
        });
    }

    // Binaries are owned by fixed path: remove whatever regular file is at
    // each one, including the other platform's binary in a shared checkout.
    for owned in MANAGED_RUNTIME_PATHS {
        let relative = PathBuf::from(owned);
        validate_destination(&root, &relative)?;
        let destination = root.join(&relative);
        if !destination.exists() {
            continue;
        }
        if !destination.is_file() {
            return Err(anyhow!(
                "uninstall-project cannot remove non-file managed runtime {}; move it aside deliberately, then rerun uninstall-project",
                destination.display()
            ));
        }
        if fs::canonicalize(&destination)
            .with_context(|| format!("resolve managed runtime {}", destination.display()))?
            == running_binary
        {
            return Err(anyhow!(
                "uninstall-project cannot remove the running project-local binary {}; run uninstall-project from an extracted Contextmink release outside the project",
                destination.display()
            ));
        }
        removable_paths.push(relative.clone());
        actions.push(SetupAction {
            path: relative,
            action: SetupActionKind::RemoveManaged,
        });
    }
    if remove_previous_runtime_receipt {
        removable_paths.push(previous_runtime_receipt.to_path_buf());
        actions.push(SetupAction {
            path: previous_runtime_receipt.to_path_buf(),
            action: SetupActionKind::RemoveManaged,
        });
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
    });
    actions.push(SetupAction {
        path: receipt_relative.to_path_buf(),
        action: SetupActionKind::RemoveManaged,
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
    let next_actions = vec![
        "Review repository-owned AGENTS.md and CLAUDE.md and remove any Contextmink trigger that is no longer wanted."
            .to_owned(),
        "Keep or deliberately remove .contextmink.toml; uninstall-project preserves it because setup transfers configuration ownership to the repository."
            .to_owned(),
    ];
    Ok(UninstallProjectResult {
        schema: "contextmink.project_uninstall.v2",
        project_root: canonical_normalized(&root)
            .expect("uninstall-project root was canonicalized before rendering"),
        dry_run: request.dry_run,
        ready,
        actions,
        preserved_repository_owned,
        next_actions,
    })
}

fn resolve_skill_target(
    root: &Path,
    requested: SkillTarget,
    installed: Option<SkillTarget>,
) -> Result<SkillTarget> {
    if requested != SkillTarget::Auto {
        return Ok(canonical_skill_target(requested));
    }
    if let Some(installed) = installed {
        return Ok(canonical_skill_target(installed));
    }
    let agents = harness_directory_exists(root, SHARED_AGENT_SKILLS_DIRECTORIES)?
        || harness_directory_exists(root, COMMON_AGENT_SKILLS_BOOTSTRAP_DIRECTORIES)?
        || harness_file_exists(root, COMMON_AGENT_SKILLS_BOOTSTRAP_FILES)?;
    let claude = harness_directory_exists(root, CLAUDE_HARNESS_DIRECTORIES)?
        || harness_file_exists(root, CLAUDE_HARNESS_FILES)?;
    Ok(match (agents, claude) {
        (true, true) => SkillTarget::Both,
        (true, false) => SkillTarget::Agents,
        (false, true) => SkillTarget::Both,
        (false, false) => SkillTarget::None,
    })
}

fn canonical_skill_target(target: SkillTarget) -> SkillTarget {
    if target == SkillTarget::Claude {
        SkillTarget::Both
    } else {
        target
    }
}

fn harness_directory_exists(root: &Path, markers: &[&str]) -> Result<bool> {
    harness_marker_exists(root, markers, |metadata| metadata.is_dir())
}

fn harness_file_exists(root: &Path, markers: &[&str]) -> Result<bool> {
    harness_marker_exists(root, markers, |metadata| metadata.is_file())
}

fn harness_marker_exists(
    root: &Path,
    markers: &[&str],
    expected_type: fn(&fs::Metadata) -> bool,
) -> Result<bool> {
    for marker in markers {
        let path = root.join(marker);
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("inspect harness marker {marker}"));
            }
        };
        if expected_type(&metadata) {
            return Ok(true);
        }
    }
    Ok(false)
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
    generated_profile: &str,
) -> Result<PreflightFile> {
    if !destination.exists() {
        return Ok(PreflightFile {
            action: SetupActionKind::Create,
            profile: matches!(file.ownership, SetupFileOwnership::RepositoryOwnedConfig)
                .then(|| generated_profile.to_owned()),
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
        });
    }
    let existing = fs::read(destination).with_context(|| {
        format!(
            "read existing release-managed file {}",
            destination.display()
        )
    })?;
    let content_matches = match file.ownership {
        SetupFileOwnership::ReleaseManagedText => same_managed_text(&existing, &file.content),
        SetupFileOwnership::ReleaseManagedRuntime => existing == file.content,
        SetupFileOwnership::RepositoryOwnedConfig => {
            unreachable!("repository-owned configuration returned before content comparison")
        }
    };
    let action = if !content_matches {
        SetupActionKind::Replace
    } else if file.executable && executable_bit_missing(destination)? {
        SetupActionKind::MakeExecutable
    } else {
        SetupActionKind::Unchanged
    };
    Ok(PreflightFile {
        action,
        profile: None,
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
        ".agents/skills/contextmink-bridge",
        ".claude/skills/contextmink-bridge",
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
    #[test]
    fn bridge_skill_selection_tracks_payload_platform_and_harness() {
        for target in [
            SkillTarget::Agents,
            SkillTarget::Claude,
            SkillTarget::Both,
            SkillTarget::None,
        ] {
            for windows in [false, true] {
                let paths: Vec<_> = contextmink_skill_files(target, windows)
                    .into_iter()
                    .map(|f| f.relative_path)
                    .collect();
                assert_eq!(
                    paths.contains(&PathBuf::from(".agents/skills/contextmink-bridge/SKILL.md")),
                    windows && target.installs_agents()
                );
                assert_eq!(
                    paths.contains(&PathBuf::from(".claude/skills/contextmink-bridge/SKILL.md")),
                    windows && target.installs_claude()
                );
            }
        }
    }

    #[test]
    fn windows_bridge_skill_install_and_deselection() {
        let (project, binary) = fixture("windows-bridge-skill");
        let windows_binary = binary.with_file_name("contextmink.exe");
        fs::write(&windows_binary, b"windows runtime").unwrap();
        fs::write(
            windows_binary.with_file_name("contextmink-bridge.exe"),
            b"bridge runtime",
        )
        .unwrap();
        let mut install = request(&project, &windows_binary, false);
        install.skill_target = SkillTarget::Both;
        setup_project(install).unwrap();
        let skill = project.join(".agents/skills/contextmink-bridge/SKILL.md");
        assert_eq!(fs::read(&skill).unwrap(), BRIDGE_SKILL);
        fs::write(&skill, b"locally edited bridge guidance").unwrap();
        let mut deselect = request(&project, &windows_binary, false);
        deselect.skill_target = SkillTarget::None;
        setup_project(deselect).unwrap();
        assert!(!skill.exists());
        assert!(
            !project
                .join(".claude/skills/contextmink-bridge/SKILL.md")
                .exists()
        );
        cleanup(&project);
    }

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
            skill_target: SkillTarget::Both,
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
        assert!(!project.join(".contextmink.toml").exists());
        assert!(!project.join("AGENTS.md").exists());
        cleanup(&project);
    }

    #[test]
    fn auto_selects_only_detected_harnesses_and_none_when_unmarked() {
        for (name, markers, expected, agents, claude) in [
            ("auto-none", &[][..], SkillTarget::None, false, false),
            (
                "auto-agents-guidance",
                &["AGENTS.md"][..],
                SkillTarget::Agents,
                true,
                false,
            ),
            (
                "auto-agent-skills-directory",
                &[".agents/"][..],
                SkillTarget::Agents,
                true,
                false,
            ),
            (
                "auto-codex",
                &[".codex/"][..],
                SkillTarget::Agents,
                true,
                false,
            ),
            (
                "auto-cursor",
                &[".cursor/"][..],
                SkillTarget::Agents,
                true,
                false,
            ),
            ("auto-pi", &[".pi/"][..], SkillTarget::Agents, true, false),
            ("auto-omp", &[".omp/"][..], SkillTarget::Agents, true, false),
            (
                "auto-opencode-directory",
                &[".opencode/"][..],
                SkillTarget::Agents,
                true,
                false,
            ),
            (
                "auto-opencode-json",
                &["opencode.json"][..],
                SkillTarget::Agents,
                true,
                false,
            ),
            (
                "auto-opencode-jsonc",
                &["opencode.jsonc"][..],
                SkillTarget::Agents,
                true,
                false,
            ),
            (
                "auto-claude",
                &["CLAUDE.md"][..],
                SkillTarget::Both,
                true,
                true,
            ),
            (
                "auto-both",
                &["AGENTS.md", "CLAUDE.md"][..],
                SkillTarget::Both,
                true,
                true,
            ),
        ] {
            let (project, binary) = fixture(name);
            for marker in markers {
                if let Some(directory) = marker.strip_suffix('/') {
                    fs::create_dir_all(project.join(directory)).unwrap();
                } else {
                    fs::write(project.join(marker), "project guidance\n").unwrap();
                }
            }
            let mut auto = request(&project, &binary, false);
            auto.skill_target = SkillTarget::Auto;
            let result = setup_project(auto).unwrap();
            assert_eq!(result.requested_skill_target, SkillTarget::Auto);
            assert_eq!(result.resolved_skill_target, expected);
            assert_eq!(
                project
                    .join(".agents/skills/contextmink/SKILL.md")
                    .is_file(),
                agents
            );
            assert_eq!(
                project
                    .join(".claude/skills/contextmink/SKILL.md")
                    .is_file(),
                claude
            );
            for duplicate in [
                ".pi/skills/contextmink/SKILL.md",
                ".omp/skills/contextmink/SKILL.md",
                ".opencode/skills/contextmink/SKILL.md",
            ] {
                assert!(
                    !project.join(duplicate).exists(),
                    "auto detection must not create a duplicate harness-native skill at {duplicate}"
                );
            }
            let receipt = load_install_receipt(&project.join(INSTALL_RECEIPT_PATH))
                .unwrap()
                .unwrap();
            assert_eq!(receipt.skill_target, expected);
            cleanup(&project);
        }
    }

    #[test]
    fn auto_selection_requires_marker_file_types() {
        let (project, binary) = fixture("auto-marker-types");
        fs::write(project.join(".pi"), "not a harness directory\n").unwrap();
        fs::create_dir(project.join("opencode.json")).unwrap();
        fs::create_dir(project.join("CLAUDE.md")).unwrap();

        let mut auto = request(&project, &binary, false);
        auto.skill_target = SkillTarget::Auto;
        let result = setup_project(auto).unwrap();
        assert_eq!(result.resolved_skill_target, SkillTarget::None);
        assert!(!project.join(".agents/skills/contextmink/SKILL.md").exists());
        assert!(!project.join(".claude/skills/contextmink/SKILL.md").exists());
        cleanup(&project);
    }

    #[test]
    fn explicit_reselection_is_frozen_and_removes_deselected_skills() {
        let (project, binary) = fixture("skill-reselection");
        fs::write(project.join("AGENTS.md"), "agents\n").unwrap();
        fs::write(project.join("CLAUDE.md"), "claude\n").unwrap();
        setup_project(request(&project, &binary, false)).unwrap();

        let mut agents_only = request(&project, &binary, false);
        agents_only.skill_target = SkillTarget::Agents;
        let selected = setup_project(agents_only).unwrap();
        assert_eq!(selected.resolved_skill_target, SkillTarget::Agents);
        assert!(
            project
                .join(".agents/skills/contextmink/SKILL.md")
                .is_file()
        );
        assert!(!project.join(".claude/skills/contextmink/SKILL.md").exists());

        let mut auto = request(&project, &binary, false);
        auto.skill_target = SkillTarget::Auto;
        let frozen = setup_project(auto).unwrap();
        assert_eq!(frozen.resolved_skill_target, SkillTarget::Agents);
        assert!(!project.join(".claude/skills/contextmink/SKILL.md").exists());

        let agents_skill = project.join(".agents/skills/contextmink/SKILL.md");
        fs::write(&agents_skill, "local edit\n").unwrap();
        let mut no_skill = request(&project, &binary, true);
        no_skill.skill_target = SkillTarget::None;
        let dry_run = setup_project(no_skill).unwrap();
        assert!(dry_run.ready);
        assert!(dry_run.actions.iter().any(|action| {
            action.path == Path::new(".agents/skills/contextmink/SKILL.md")
                && action.action == SetupActionKind::RemoveRetired
        }));
        assert!(agents_skill.is_file());

        let mut no_skill = request(&project, &binary, false);
        no_skill.skill_target = SkillTarget::None;
        let removed = setup_project(no_skill).unwrap();
        assert_eq!(removed.resolved_skill_target, SkillTarget::None);
        assert!(!agents_skill.exists());
        cleanup(&project);
    }

    #[test]
    fn first_install_refuses_an_unselected_unreceipted_skill() {
        let (project, binary) = fixture("unowned-unselected-skill");
        let claude_skill = project.join(".claude/skills/contextmink/SKILL.md");
        fs::create_dir_all(claude_skill.parent().unwrap()).unwrap();
        fs::write(&claude_skill, "repository-owned contextmink guidance\n").unwrap();

        let mut agents_only = request(&project, &binary, true);
        agents_only.skill_target = SkillTarget::Agents;
        let result = setup_project(agents_only).unwrap();
        assert!(!result.ready);
        assert!(result.actions.iter().any(|action| {
            action.path == Path::new(".claude/skills/contextmink/SKILL.md")
                && action.action == SetupActionKind::UnownedRefusal
        }));
        let mut agents_only = request(&project, &binary, false);
        agents_only.skill_target = SkillTarget::Agents;
        assert!(
            setup_project(agents_only)
                .unwrap_err()
                .to_string()
                .contains("unreceipted skill at deselected path")
        );
        assert_eq!(
            fs::read_to_string(&claude_skill).unwrap(),
            "repository-owned contextmink guidance\n"
        );
        cleanup(&project);
    }

    #[test]
    fn previous_receipt_upgrade_rewrites_the_current_schema() {
        let (project, binary) = fixture("previous-receipt-upgrade");
        let mut agents = request(&project, &binary, false);
        agents.skill_target = SkillTarget::Agents;
        setup_project(agents).unwrap();
        let receipt_path = project.join(INSTALL_RECEIPT_PATH);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
        value["schema"] = serde_json::Value::String("contextmink.project_install.v2".into());
        value["managed_files"] = serde_json::json!([
            {"path": "scripts/contextmink", "sha256": "0".repeat(64)}
        ]);
        fs::write(&receipt_path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let mut auto = request(&project, &binary, false);
        auto.skill_target = SkillTarget::Auto;
        let result = setup_project(auto).unwrap();
        assert_eq!(result.resolved_skill_target, SkillTarget::Agents);
        let written: serde_json::Value =
            serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
        assert_eq!(written["schema"], receipt::INSTALL_RECEIPT_SCHEMA);
        assert!(written.get("managed_files").is_none());
        assert_eq!(written["skill_target"], "agents");

        value["schema"] = serde_json::Value::String("contextmink.project_install.v1".into());
        fs::write(&receipt_path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        let error = setup_project(request(&project, &binary, false)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("rerun setup-project to reinstall"),
            "{error}"
        );
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
            CONTEXTMINK_SKILL.to_vec()
        );
        assert_eq!(
            fs::read(project.join(".agents/skills/contextmink/agents/openai.yaml")).unwrap(),
            CONTEXTMINK_OPENAI_METADATA
        );
        assert!(project.join(INSTALL_RECEIPT_PATH).is_file());
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
    fn divergent_release_file_is_rewritten() {
        let (project, binary) = fixture("divergence");
        setup_project(request(&project, &binary, false)).unwrap();
        fs::remove_file(project.join(".contextmink.toml")).unwrap();
        fs::write(project.join("scripts/contextmink"), b"locally changed").unwrap();

        let result = setup_project(request(&project, &binary, false)).unwrap();
        assert!(result.ready);
        assert_eq!(
            fs::read(project.join("scripts/contextmink")).unwrap(),
            BASH_LAUNCHER
        );
        assert!(project.join(".contextmink.toml").is_file());
        cleanup(&project);
    }

    #[test]
    fn dry_run_previews_release_replacement_without_writing() {
        let (project, binary) = fixture("dry-run-replacement");
        setup_project(request(&project, &binary, false)).unwrap();
        fs::write(project.join("scripts/contextmink"), b"older release").unwrap();
        let owned_config = "profile = \"owned\"\nexclude_globs = [\"cache/**\"]\n";
        fs::write(project.join(".contextmink.toml"), owned_config).unwrap();

        let result = setup_project(request(&project, &binary, true)).unwrap();
        assert_eq!(result.profile, "owned");
        assert!(result.ready);
        assert!(result.actions.iter().any(|action| {
            action.path == Path::new("scripts/contextmink")
                && action.action == SetupActionKind::Replace
        }));
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
    fn setup_rewrites_release_files_and_preserves_configuration() {
        let (project, binary) = fixture("rewrite-release-files");
        setup_project(request(&project, &binary, false)).unwrap();
        fs::write(project.join("scripts/contextmink"), b"older release").unwrap();
        fs::write(
            project.join(".agents/skills/contextmink/SKILL.md"),
            b"older release",
        )
        .unwrap();
        let owned_config = "profile = \"owned\"\nexclude_globs = [\"cache/**\"]\n";
        fs::write(project.join(".contextmink.toml"), owned_config).unwrap();

        let result = setup_project(request(&project, &binary, false)).unwrap();
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

        let error = setup_project(request(&project, &binary, false)).unwrap_err();
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
        let installed = format!(
            "tools/contextmink/bin/contextmink{}",
            std::env::consts::EXE_SUFFIX
        );
        for relative in [
            INSTALL_RECEIPT_PATH,
            installed.as_str(),
            "scripts/contextmink",
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
    fn uninstall_removes_a_divergent_binary() {
        let (project, binary) = fixture("divergent-runtime");
        setup_project(request(&project, &binary, false)).unwrap();
        assert!(!project.join(PREVIOUS_RUNTIME_RECEIPT_PATH).exists());
        let installed_relative = format!(
            "tools/contextmink/bin/contextmink{}",
            std::env::consts::EXE_SUFFIX
        );

        let installed = project.join(&installed_relative);
        fs::write(&installed, "modified runtime\n").unwrap();
        let dry_run = uninstall_project(UninstallProjectRequest {
            project_root: &project,
            running_binary: Some(&binary),
            dry_run: true,
        })
        .unwrap();
        assert!(dry_run.ready);
        assert!(dry_run.actions.iter().any(|action| {
            action.path == Path::new(&installed_relative)
                && action.action == SetupActionKind::RemoveManaged
        }));
        assert_eq!(fs::read(&installed).unwrap(), b"modified runtime\n");

        let result = uninstall_project(UninstallProjectRequest {
            project_root: &project,
            running_binary: Some(&binary),
            dry_run: false,
        })
        .unwrap();
        assert!(result.ready);
        assert!(!installed.exists());
        assert!(!project.join(INSTALL_RECEIPT_PATH).exists());
        cleanup(&project);
    }

    #[test]
    fn uninstall_refuses_a_non_file_at_an_owned_runtime_path() {
        let (project, binary) = fixture("runtime-non-file");
        setup_project(request(&project, &binary, false)).unwrap();
        let installed = project.join(format!(
            "tools/contextmink/bin/contextmink{}",
            std::env::consts::EXE_SUFFIX
        ));
        fs::remove_file(&installed).unwrap();
        fs::create_dir(&installed).unwrap();
        for dry_run in [true, false] {
            let error = uninstall_project(UninstallProjectRequest {
                project_root: &project,
                running_binary: Some(&binary),
                dry_run,
            })
            .unwrap_err()
            .to_string();
            assert!(
                error.contains("cannot remove non-file managed runtime"),
                "{error}"
            );
        }
        assert!(installed.is_dir());
        assert!(project.join(INSTALL_RECEIPT_PATH).is_file());
        cleanup(&project);
    }

    #[test]
    fn setup_keeps_and_uninstall_removes_the_other_hosts_binary() {
        let (project, binary) = fixture("cross-host-runtime");
        let alternate = if std::env::consts::EXE_SUFFIX.is_empty() {
            "tools/contextmink/bin/contextmink.exe"
        } else {
            "tools/contextmink/bin/contextmink"
        };
        // The other host installed first; this host's setup never touches it.
        fs::create_dir_all(project.join("tools/contextmink/bin")).unwrap();
        fs::write(project.join(alternate), "alternate host binary\n").unwrap();
        let result = setup_project(request(&project, &binary, false)).unwrap();
        assert!(result.ready);
        assert!(
            !result
                .actions
                .iter()
                .any(|action| action.path == Path::new(alternate))
        );
        assert_eq!(
            fs::read(project.join(alternate)).unwrap(),
            b"alternate host binary\n"
        );

        let dry_run = uninstall_project(UninstallProjectRequest {
            project_root: &project,
            running_binary: Some(&binary),
            dry_run: true,
        })
        .unwrap();
        assert!(dry_run.actions.iter().any(|action| {
            action.path == Path::new(alternate) && action.action == SetupActionKind::RemoveManaged
        }));
        assert!(project.join(alternate).is_file());
        uninstall_project(UninstallProjectRequest {
            project_root: &project,
            running_binary: Some(&binary),
            dry_run: false,
        })
        .unwrap();
        assert!(!project.join(alternate).exists());
        assert!(!project.join("tools/contextmink").exists());
        cleanup(&project);
    }

    fn write_previous_runtime_receipt(project: &Path) -> PathBuf {
        let path = project.join(PREVIOUS_RUNTIME_RECEIPT_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            serde_json::json!({
                "schema": "contextmink.runtime_install.v1",
                "contextmink_version": "0.15.0",
                "managed_files": [
                    {"path": "tools/contextmink/bin/contextmink", "sha256": "0".repeat(64)},
                ],
            })
            .to_string(),
        )
        .unwrap();
        path
    }

    #[test]
    fn setup_and_uninstall_remove_the_previous_runtime_receipt() {
        let (project, binary) = fixture("previous-runtime-receipt");
        setup_project(request(&project, &binary, false)).unwrap();
        let previous = write_previous_runtime_receipt(&project);

        let dry_run = setup_project(request(&project, &binary, true)).unwrap();
        assert!(dry_run.actions.iter().any(|action| {
            action.path == Path::new(PREVIOUS_RUNTIME_RECEIPT_PATH)
                && action.action == SetupActionKind::RemoveRetired
        }));
        assert!(previous.is_file());
        setup_project(request(&project, &binary, false)).unwrap();
        assert!(!previous.exists());

        write_previous_runtime_receipt(&project);
        let result = uninstall_project(UninstallProjectRequest {
            project_root: &project,
            running_binary: Some(&binary),
            dry_run: false,
        })
        .unwrap();
        assert!(result.actions.iter().any(|action| {
            action.path == Path::new(PREVIOUS_RUNTIME_RECEIPT_PATH)
                && action.action == SetupActionKind::RemoveManaged
        }));
        assert!(!previous.exists());
        assert!(!project.join("tools/contextmink").exists());
        cleanup(&project);
    }

    #[test]
    fn setup_refuses_an_unrecognized_file_at_the_previous_runtime_receipt_path() {
        let (project, binary) = fixture("unrecognized-runtime-receipt");
        let path = project.join(PREVIOUS_RUNTIME_RECEIPT_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"schema":"contextmink.runtime_install.v2"}"#).unwrap();
        for dry_run in [true, false] {
            let error = setup_project(request(&project, &binary, dry_run))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("move it aside, then rerun setup-project"),
                "{error}"
            );
        }
        assert!(path.is_file());
        assert!(!project.join(INSTALL_RECEIPT_PATH).exists());
        cleanup(&project);
    }

    #[test]
    fn setup_replaces_a_divergent_runtime_at_its_install_path() {
        let (project, binary) = fixture("unreceipted-runtime");
        let installed = project.join(format!(
            "tools/contextmink/bin/contextmink{}",
            std::env::consts::EXE_SUFFIX
        ));
        fs::create_dir_all(installed.parent().unwrap()).unwrap();
        fs::write(&installed, "foreign runtime\n").unwrap();

        let dry_run = setup_project(request(&project, &binary, true)).unwrap();
        assert!(dry_run.ready);
        assert!(dry_run.actions.iter().any(|action| {
            action.path == installed.strip_prefix(&project).unwrap()
                && action.action == SetupActionKind::Replace
        }));
        assert_eq!(fs::read(&installed).unwrap(), b"foreign runtime\n");

        setup_project(request(&project, &binary, false)).unwrap();
        assert_eq!(fs::read(&installed).unwrap(), b"contextmink-binary");
        cleanup(&project);
    }

    #[test]
    fn uninstall_removes_locally_edited_release_text() {
        let (project, binary) = fixture("uninstall-edited");
        setup_project(request(&project, &binary, false)).unwrap();
        let skill = project.join(".agents/skills/contextmink/SKILL.md");
        fs::write(&skill, b"local edit\n").unwrap();

        let result = uninstall_project(UninstallProjectRequest {
            project_root: &project,
            running_binary: Some(&binary),
            dry_run: false,
        })
        .unwrap();
        assert!(result.ready);
        assert!(!skill.exists());
        assert!(!project.join(INSTALL_RECEIPT_PATH).exists());
        assert!(project.join(".contextmink.toml").is_file());
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
