# contextmink Setup

The full setup guide is in [docs/setup.md](docs/setup.md). From the unpacked
release, the agent responsible for maintaining the target repository runs:

```bash
./contextmink setup-project /path/to/repository --dry-run
./contextmink setup-project /path/to/repository
```

The command installs platform-appropriate project-local binaries and launchers,
generates a real profile, updates `.gitignore`, and installs the thin namespaced
Contextmink skill for the selected harness paths. `--skill-target auto` detects
existing Agent Skills/Codex and Claude markers on first install, resolves an
unmarked project to `none`, and freezes that choice in
`tools/contextmink/project-install.json`; use
`--skill-target agents|claude|both|none` for explicit selection or reselection.
Deselected receipt-owned skills retire only while their hashes match; an
unreceipted file at a deselected Contextmink skill path refuses setup until it
is resolved manually.
The skill points to `tools/contextmink/agent_integration.md`. Setup
never edits repository agent guidance or harness settings. An existing
`.contextmink.toml` is validated and preserved as repository-owned
configuration; invalid configuration fails before any write. The same command
restores ignored host binaries in a fresh clone. The maintaining agent adds one
concise discovery trigger and adapts only repository-owned shell, native-tool,
nested-repository, exclusion, and destructive-path policy. Receipt-owned
upgrades need no flag; use `--replace-managed` only for a reviewed modified or
pre-receipt destination. The ignored
`tools/contextmink/bin/runtime-install.json` records exact host binary hashes.

To remove the receipt-owned integration, run `uninstall-project --dry-run` and
then `uninstall-project` from an unpacked matching or newer release outside the
target project. Repository-owned `.contextmink.toml`, `AGENTS.md`, `CLAUDE.md`,
harness settings, unrelated skills, and preexisting ignore policy are
preserved. Runtime files without matching host-receipt ownership are reported
and retained.
