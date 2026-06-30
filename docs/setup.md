# Setting Up contextmink In A Repository

This guide is for adding `contextmink` to an existing repository.

`contextmink` is a transcript guard. Use it before broad file, text, line-slice,
JSON, read-only SQLite, or unknown-size command-output reads when the output
cardinality is unknown or host-shell quoting would become the task. It is not a
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

   Keep only repo-local high-output paths. Good candidates include generated build
   directories, vendored dependencies, caches, exported reports, large binary
   asset trees, and tool output directories. These excludes keep broad scans
   quiet; callers can still pass an explicit file or subdirectory inside an
   excluded tree when that tree is the target.

4. Add the instruction snippet for the tool surface the target repository uses:

   - Codex: copy `templates/AGENTS.contextmink.md` into the repository's
     `AGENTS.md` or equivalent Codex guidance file.
   - Claude: copy `templates/CLAUDE.contextmink.md` into the repository's
     `CLAUDE.md` or equivalent Claude guidance file.

   The two snippets are intentionally equivalent in policy. Keep any
   repository-specific shell or path guidance in the target repository, not in
   these templates.

5. Verify the integration from the target repository root:

   ```bash
   scripts/contextmink files . --max 20
   scripts/contextmink grep contextmink . --max-files 5
   ```

   The first run may build the release binary. Build output is sent to stderr
   so stdout remains parseable. Release builds include bundled SQLite support
   so read-only DB inspection works without a system SQLite install.

## Standalone Install

Use this when the user wants `contextmink` on PATH instead of vendored in each
repository:

```bash
cargo install --path .
contextmink files . --max 20
```

The binary can still use a repository-local `.contextmink.toml`; it
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

The binary already excludes common high-output paths such as `.git`, `target`,
`node_modules`, and `.venv`. Include them in repo configs only if doing so makes
the local policy clearer for future maintainers.

## Instruction Rule

Do not copy prose from this setup guide by hand. Use the maintained templates:

- `templates/AGENTS.contextmink.md` for Codex-facing guidance.
- `templates/CLAUDE.contextmink.md` for Claude-facing guidance.

The template files are intentionally kept equivalent by tests so behavior does
not drift between instruction surfaces.

Do not create a separate contextmink skill or slash command by default.
The bounded-output rule should be visible before broad reads start; an on-demand
skill is easier to miss and adds another instruction surface to keep in
sync. Use the short Codex/Claude snippets above unless a host tool requires a
different integration mechanism.

## Operational Notes

- Prefer `grep --pattern-file <file>` when regex punctuation is shell-fragile.
- Pass an explicit file or subdirectory for artifact lookups inside configured
  high-output trees. Use `--with-excluded` only when the whole command should
  include files matched by contextmink's built-in and configured exclude globs.
  It does not disable Git ignore rules.
- Prefer `grep-terms --term-file <file>` when phrases contain shell-fragile
  punctuation or spaces.
- Prefer `slice --range START:END` before opening large files.
- Prefer `json-find` over opening whole JSON reports, and `json-select` for
  bounded row/field projection from JSON arrays or JSONL row files.
- On Git Bash/Windows, use the `scripts/contextmink` launcher for
  `json-select`; it preserves slash-leading JSON Pointer selectors while still
  leaving normal file path handling to the shell/runtime boundary.
- Prefer `sqlite-schema <db>` before ad hoc SQLite queries against unfamiliar
  databases.
- Prefer `sqlite --sql-file <file>` for read-only SQL containing shell-fragile
  operators or quotes.
- Prefer a domain command's native compact/projection/limit flags first. Use
  `capture -- <command> ...` only when output size is uncertain and no better
  native bound exists; read the child `exit_code`/`success` fields in the
  receipt. On Windows, the Bash launcher lets `capture` retry extensionless
  shell scripts through the current Bash interpreter without shell-string
  parsing.
- Treat capped receipts as `complete: false`. When `cap_reason` is `scan` or
  `candidate_files_total_is_lower_bound` is true, totals and no-match results
  only describe the scanned subset; narrow and rerun.
- Keep repository-specific policy in `.contextmink.toml` and repository
  instructions, not in `contextmink` source code.

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
