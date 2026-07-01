# contextmink Setup

The full setup guide is in [docs/setup.md](docs/setup.md). Start there for the
normal no-build path: download a release archive, copy `contextmink(.exe)` to
`tools/contextmink/bin/`, add the `scripts/contextmink` launcher, configure
`.contextmink.toml`, and merge the Codex/Claude instruction snippet into the
target repository. Source vendoring is optional and only needed when the target
repository should build contextmink itself.
