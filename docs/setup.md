# Setting Up contextmink In A Repository

This guide is for adding `contextmink` to an existing repository.

`contextmink` is a transcript guard. Use it before broad file, text, line-slice,
JSON, read-only SQLite, or unknown-size command-output reads when the output
cardinality is unknown or host-shell quoting would become the task. It is not a
replacement for project-native tools.

## Prerequisites

- For standalone use, download the release archive for your platform and put
  `contextmink` on `PATH`, or run it from the unpacked directory.
- Rust and Cargo are needed only for source builds or vendored integrations that
  build the local `tools/contextmink` copy. `contextmink` uses Rust edition
  2024.
- A POSIX-compatible shell is needed only for the optional `scripts/contextmink`
  launcher. On Windows, Git Bash works. Without Bash, call the release binary
  directly or use `cargo run --manifest-path tools/contextmink/Cargo.toml -- ...`.

## Standalone Binary Install

Use this when the user wants `contextmink` on PATH instead of vendored in each
repository:

1. Download the release archive for the host platform:

   - `contextmink-<version>-windows-x86_64.zip`
   - `contextmink-<version>-macos-x86_64.tar.gz`
   - `contextmink-<version>-macos-arm64.tar.gz`
   - `contextmink-<version>-linux-x86_64.tar.gz`

2. Verify the adjacent `.sha256` checksum if the archive was downloaded outside
   GitHub's release UI.

3. Unpack the archive and run:

   ```bash
   contextmink files --path . --max 20
   ```

The binary can still use a repository-local `.contextmink.toml`; it searches
upward from the current directory.

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

   The launcher uses `tools/contextmink/target/release/contextmink(.exe)` when
   it builds from source. If the repository should avoid requiring Rust, copy a
   release binary to `tools/contextmink/bin/contextmink(.exe)` instead.

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
   scripts/contextmink files --path . --max 20
   scripts/contextmink grep contextmink --path . --limit 5
   ```

   The first source-backed run may build the release binary. Build output is
   sent to stderr so stdout remains parseable. Release builds include bundled
   SQLite support so read-only DB inspection works without a system SQLite
   install.

## Source Install

Use this for local development or when a release archive is not available for
the host:

```bash
cargo install --path .
contextmink files --path . --max 20
```

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

- For extension filtering, prefer `files --ext json` / `--extension jsonl`
  over wildcard globs when commands cross Windows-to-Bash boundaries. Wildcards
  can be expanded before contextmink receives them.
- Prefer `grep --pattern-file <file>` when regex punctuation is shell-fragile.
- For `grep` and `grep-terms`, use `--limit` to cap printed matching files,
  `--max-matches` / `--max-lines` to cap printed sample match lines, and
  `--max-count-files` only when it is acceptable for match totals to become
  lower bounds after enough matching files are found.
- Pass an explicit file or subdirectory for artifact lookups inside configured
  high-output trees. Use `--with-excluded` only when the whole command should
  include files matched by contextmink's built-in and configured exclude globs.
  It does not disable Git ignore rules.
- Use `--with-git-ignored` only when intentionally inspecting a Git-ignored
  vendor, cache, or artifact tree. Contextmink exclude globs still apply unless
  `--with-excluded` is also set.
- Prefer `grep-terms --term-file <file>` when phrases contain shell-fragile
  punctuation or spaces.
- Prefer `slice --range START:END` before opening large files.
- Prefer `json-find` over opening whole JSON reports, and `json-select` for
  bounded row/field projection from JSON arrays or JSONL row files.
- On Git Bash/Windows, use the `scripts/contextmink` launcher for
  `json-select`; it preserves slash-leading JSON Pointer selectors while still
  leaving normal file path handling to the shell/runtime boundary.
- Prefer `sqlite-schema --path <db>` before ad hoc SQLite queries against
  unfamiliar databases.
- Prefer `sqlite --path <db> --sql-file <file>` for read-only SQL containing
  shell-fragile operators or quotes.
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
