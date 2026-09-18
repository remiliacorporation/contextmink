# contextmink Setup

## Add to a project (default)

Download the archive for the machine where the agent runs, verify its checksum,
and merge its contents into the project root, including the dot-directories:

```text
.agents/skills/contextmink/SKILL.md
.claude/skills/contextmink/SKILL.md
tools/contextmink/bin/contextmink[.exe]
tools/contextmink/README.md
```

The skills and native executable are already in place. Start a fresh agent
session; no install command, PATH change, AGENTS.md edit, or integration-guide
reading is needed for ordinary work. Claude receives the same complete short
skill body generated from the canonical template. Codex, Pi, Cursor, OMP and
OpenCode can use the shared Agent Skills directory; model selection still
remains discretionary.

Only namespaced skills and `tools/contextmink` are shipped. Existing project guidance,
configuration, receipts and databases are not included or overwritten. Preserve
any customizations inside those tool-owned directories before replacing them.
README, licenses, optional operating references and the source manifest live
under `tools/contextmink`. Retrieval uses the project configuration when present and built-in defaults otherwise.

## Optional personal installation

From a verified extracted release, run:

```sh
# macOS/Linux, inside the extracted release
./tools/contextmink/bin/contextmink setup-user --dry-run
./tools/contextmink/bin/contextmink setup-user
```

```powershell
# Windows PowerShell, inside the extracted release
.\tools\contextmink\bin\contextmink.exe setup-user --dry-run
.\tools\contextmink\bin\contextmink.exe setup-user
```

This installs the retrieval skill in `~/.agents/skills/contextmink`, an identical generated skill
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
installs the native retrieval/capture executable. Windows setup also installs
`contextmink-bridge.exe` and its separate skill for project Bash scripts.
Linux and macOS installations omit the bridge skill.

Use `setup-project` below only for explicit shared repository adoption, pinned
project runtimes, or repository-owned policy. Existing project receipt choices
remain intact. Project guidance triggers are optional for skills-capable agents.


The full setup guide is in [docs/setup.md](docs/setup.md). From the unpacked
release, the agent responsible for maintaining the target repository runs:

```bash
./tools/contextmink/bin/contextmink setup-project /path/to/repository --dry-run
./tools/contextmink/bin/contextmink setup-project /path/to/repository
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
