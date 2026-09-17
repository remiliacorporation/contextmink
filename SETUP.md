# contextmink Setup

## Personal installation (default)

From a verified extracted release, run:

```sh
# macOS/Linux, inside the extracted release
./contextmink setup-user --dry-run
./contextmink setup-user
```

```powershell
# Windows PowerShell, inside the extracted release
.\contextmink.exe setup-user --dry-run
.\contextmink.exe setup-user
```

This installs one canonical skill in `~/.agents/skills/contextmink`, an identical generated skill
in `~/.claude/skills/contextmink`, and a native runtime plus detailed reference under
`~/.local/share/contextmink` (the same home-relative layout on Windows). The skill
binds the exact executable; no PATH, shell profile, AGENTS.md, CLAUDE.md, hooks,
or consuming-project files are changed. `--home <existing-directory>` selects
an explicit user home, including disposable test homes. Start a fresh agent
session and verify the skill appears. Skill descriptions support automatic
selection; they do not guarantee a model will choose the tool on every request.
The installer writes both complete skill files itself and executes the copied
runtime before reporting success. No agent-side copying or routing setup remains.

Both skill paths share one semantic body. Codex, Pi and Cursor can discover the
shared Agent Skills location; Claude reads its generated copy. Other harnesses may need
an explicit skill-directory setting. A synced skill does not install a native
runtime in a remote/cloud environment: install there separately.

A host-local `user-install.json` binds installed files to raw byte hashes and
the tool version. Owned upgrades need no replacement flag. Conflicting unowned or modified
files refuse; review them before using `--replace-managed`. The installed
runtime refuses a missing or divergent receipt/file set. Run repair or upgrade
from an external release, not the installed executable. Installation preflights
all managed paths, but does not promise a crash-atomic multi-file transaction;
an interrupted install must be repaired before the runtime can run.

`uninstall-user --dry-run` previews removal. `uninstall-user` removes only
receipt-owned matching runtime/skill files and retains the lifecycle receipt;
it never removes project installations or unrelated skills. Do not copy personal
receipts between machines or move their home: install for the new home instead.

Ordinary retrieval runs from the consuming project's cwd, with its local
configuration when present and built-in defaults otherwise. Personal setup
installs the native retrieval/capture executable; the optional Windows Bash
bridge remains available in the release for intentional shell integration.

Use `setup-project` below only for explicit shared repository adoption, pinned
project runtimes, or repository-owned policy. Existing project receipt choices
remain intact. Project guidance triggers are optional for skills-capable agents.


The full setup guide is in [docs/setup.md](docs/setup.md). From the unpacked
release, the agent responsible for maintaining the target repository runs:

```bash
./contextmink setup-project /path/to/repository --dry-run
./contextmink setup-project /path/to/repository
```

The command installs platform-appropriate project-local binaries and launchers,
generates a real profile, updates `.gitignore`, and installs the short namespaced
Contextmink skill for the selected harness paths. `--skill-target auto` detects
existing Agent Skills, Codex, Pi, OMP, OpenCode, and Claude markers on first
install, resolves an unmarked project to `none`, and freezes that choice in
`tools/contextmink/project-install.json`; use
`--skill-target agents|claude|both|none` for explicit selection or reselection.
Deselected receipt-owned skills retire only while their hashes match; an
unreceipted file at a deselected Contextmink skill path refuses setup until it
is resolved manually.
The skill points to `tools/contextmink/agent_integration.md`. Setup
never edits repository agent guidance or harness settings. An existing
`.contextmink.toml` is validated and preserved as repository-owned
configuration; invalid configuration fails before any write. The same command
restores ignored host binaries in a fresh clone. No project guidance trigger is required in skills-capable harnesses. Adapt
repository-owned policy only when the project needs custom behavior. Receipt-owned
upgrades need no flag; use `--replace-managed` only for a reviewed modified or
pre-receipt destination. The ignored
`tools/contextmink/bin/runtime-install.json` records exact host binary hashes.

To remove the receipt-owned integration, run `uninstall-project --dry-run` and
then `uninstall-project` from an unpacked matching or newer release outside the
target project. Repository-owned `.contextmink.toml`, `AGENTS.md`, `CLAUDE.md`,
harness settings, unrelated skills, and preexisting ignore policy are
preserved. Runtime files without matching host-receipt ownership are reported
and retained.
