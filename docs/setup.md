# Setting Up contextmink In A Repository

This guide is written for coding agents and maintainers adding `contextmink` to
an existing repository.

`contextmink` is a transcript guard. Use it before broad file, text, line-slice,
or JSON reconnaissance when the output cardinality is unknown. It is not a
replacement for project-native tools.

## Prerequisites

- Recent stable Rust toolchain with Cargo. `contextmink` uses Rust edition
  2024.
- A POSIX-compatible shell for the `scripts/contextmink` launcher. On Windows,
  Git Bash works. Without Bash, call `cargo run --manifest-path
  tools/contextmink/Cargo.toml -- ...` or the release binary directly.

## Vendored Integration

Use this pattern when the target repository should carry its own copy of the
tool:

1. Copy this repository's Rust crate into the target repository at
   `tools/contextmink/`.

2. Copy `templates/scripts/contextmink` to `scripts/contextmink`.

   Preserve the executable bit on Unix-like systems:

   ```bash
   chmod +x scripts/contextmink
   ```

3. Copy `templates/.contextmink.toml` to `.contextmink.toml`, then edit it.

   Keep only repo-local noisy paths. Good candidates include generated build
   directories, vendored dependencies, caches, exported reports, large binary
   asset trees, and tool output directories.

4. Add the agent instruction snippet from `templates/AGENTS.contextmink.md`.

   Put it in the repository's `AGENTS.md`, `CLAUDE.md`, or equivalent agent
   guidance file.

5. Verify the integration from the target repository root:

   ```bash
   scripts/contextmink files . --max 20
   scripts/contextmink grep contextmink . --max-files 5
   ```

   The first run may build the release binary. Build output is sent to stderr
   so stdout remains parseable.

## Standalone Install

Use this when the user wants `contextmink` on PATH instead of vendored in each
repository:

```bash
cargo install --path .
contextmink files . --max 20
```

Agents can still use a repository-local `.contextmink.toml`; the binary
searches upward from the current directory.

## Config Template

Start from:

```toml
profile = "repo-name"

exclude_globs = [
  "target/**",
  "**/target/**",
  "node_modules/**",
  "**/node_modules/**",
  ".venv/**",
  "**/.venv/**",
]
```

The binary already excludes common noisy paths such as `.git`, `target`,
`node_modules`, and `.venv`. Include them in repo configs only if doing so makes
the local policy clearer for future maintainers.

## Agent Rule

Use this rule in the target repository:

```md
Use `scripts/contextmink` for broad or uncertain file/text/JSON reconnaissance before opening raw files or command output in the transcript. Start with `files` or `grep`, use `grep-terms` for shell-fragile multi-token searches, use `slice` for exact line windows, and use `json-find` for sidecars, reports, manifests, logs, or other structured command output. Treat a `CONTEXTMINK_RECEIPT` with `"truncated": true` as incomplete evidence and narrow the query.

`contextmink` is only a generic transcript guard. Prefer project-native tools for domain-specific parsing, validation, indexing, diagnostics, or synchronization.
```

## Operational Notes

- Prefer `grep-terms` when a phrase contains shell-fragile punctuation or
  spaces.
- Prefer `slice --range START:END` before opening large files.
- Prefer `json-find` over opening whole JSON reports.
- Treat capped receipts as incomplete. Narrow and rerun.
- Keep repository-specific policy in `.contextmink.toml` and agent guidance,
  not in `contextmink` source code.

## Maintenance

For a vendored copy, compare or sync only the generic surface:

```text
tools/contextmink/src/
tools/contextmink/tests/
tools/contextmink/Cargo.toml
tools/contextmink/Cargo.lock
tools/contextmink/README.md
tools/contextmink/SETUP.md
tools/contextmink/docs/
tools/contextmink/scripts/
tools/contextmink/templates/
tools/contextmink/.github/
tools/contextmink/.gitignore
tools/contextmink/LICENSE
```

Do not sync a target repository's `.contextmink.toml`; that file is local
policy.
