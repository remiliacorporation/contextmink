# contextmink Setup

The full setup guide is in [docs/setup.md](docs/setup.md). From the unpacked
release, the agent responsible for maintaining the target repository runs:

```bash
./contextmink setup-project /path/to/repository --dry-run
./contextmink setup-project /path/to/repository
```

The command installs platform-appropriate project-local binaries and launchers,
generates a real profile, updates `.gitignore`, and installs the Contextmink and
human-facing changelog-writing skills for open Agent Skills-compatible harnesses
and Claude Code. The thin Contextmink skill points to
`tools/contextmink/agent_integration.md`. Setup never edits repository agent
guidance. An existing `.contextmink.toml` is validated and preserved as
repository-owned configuration; invalid configuration fails before any write.
The same command restores ignored host binaries in a fresh clone. The
maintaining agent adds one concise discovery trigger and adapts only the
repository-owned shell, native-tool, nested-repository, exclusion, and
destructive-path policy. Use `--replace-managed` only for an intentional
release-artifact upgrade.
