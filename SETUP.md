# contextmink Setup

The full setup guide is in [docs/setup.md](docs/setup.md). From the unpacked
release, the agent responsible for maintaining the target repository runs:

```bash
./contextmink setup-project /path/to/repository --dry-run
./contextmink setup-project /path/to/repository
```

The command installs platform-appropriate project-local binaries and launchers,
generates a real profile, updates `.gitignore`, and writes
`tools/contextmink/agent_integration.md`. It never edits repository agent
guidance or replaces a divergent `.contextmink.toml`; the maintaining agent
must adapt the integration reference and project policy. Use
`--replace-managed` only for an intentional release-artifact upgrade.
