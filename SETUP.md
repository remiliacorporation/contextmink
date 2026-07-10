# contextmink Setup

The full setup guide is in [docs/setup.md](docs/setup.md). For a release
install, download an archive and copy `contextmink(.exe)` to
`tools/contextmink/bin/`; on Windows also copy `contextmink-bridge.exe` there
because the PowerShell launcher path requires both executables. Add
`/tools/contextmink/bin/contextmink*` to the target repository's `.gitignore`
unless a reviewed hermetic install intentionally tracks host-specific binaries.
Then add the `scripts/contextmink` launcher, configure `.contextmink.toml`, and
merge the Codex/Claude instruction snippet into the target repository. Source
vendoring is optional and only needed when the target repository should build
contextmink itself.
