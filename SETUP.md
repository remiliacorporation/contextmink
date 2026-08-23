# contextmink Setup

The full setup guide is in [docs/setup.md](docs/setup.md). From the unpacked
release, the agent responsible for maintaining the target repository runs:

```bash
./contextmink setup-project /path/to/repository --dry-run
./contextmink setup-project /path/to/repository
```

The command installs platform-appropriate project-local binaries and launchers,
generates a real profile, updates `.gitignore`, installs the thin namespaced
Contextmink skill for open Agent Skills-compatible harnesses and Claude Code,
and writes `tools/contextmink/project-install.json` for hash-bound upgrade and
retirement. The skill points to `tools/contextmink/agent_integration.md`. Setup
never edits repository agent guidance or harness settings. An existing
`.contextmink.toml` is validated and preserved as repository-owned
configuration; invalid configuration fails before any write. The same command
restores ignored host binaries in a fresh clone. The maintaining agent adds one
concise discovery trigger and adapts only repository-owned shell, native-tool,
nested-repository, exclusion, and destructive-path policy. Receipt-owned
upgrades need no flag; use `--replace-managed` only for a reviewed modified or
pre-receipt destination.

To remove the receipt-owned integration, run `uninstall-project --dry-run` and
then `uninstall-project` from an unpacked matching or newer release outside the
target project. Repository-owned `.contextmink.toml`, `AGENTS.md`, `CLAUDE.md`,
harness settings, unrelated skills, and preexisting ignore policy are
preserved.
