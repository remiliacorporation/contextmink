# contextmink

`contextmink` is a transcript guard for command-line code work. It provides
bounded ways to list files, search text, read line windows, inspect JSON, run
read-only SQLite queries, and capture unknown-size command output without
dumping large outputs into the conversation.

It is deliberately generic. Project-specific parsing, validation, indexing,
diagnostics, and synchronization should stay in project-native tools.

## Commands

- `files`: list candidate files with hard caps and configured excludes. Include
  globs match either the displayed path or the basename, so `--glob '*.jsonl'`
  works inside an explicit queue directory. Configured excludes apply to broad
  scans, but an explicit path inside an excluded tree is treated as the target
  and searched without `--with-excluded`. Use `--with-git-ignored` only when
  intentionally inspecting files hidden by Git or `.ignore` rules.
- `grep`: count matches first, then print a bounded file/sample summary. Use
  `--pattern-file <file>` when regex punctuation would be fragile through a
  host shell bridge.
- `grep-terms`: match lines containing all `--term` values by default, or any
  term with `--mode any` / `--any` / `--or`. Use `--term-file <file>` for
  phrase lists when shell quoting or regex punctuation would make inline
  arguments fragile.
- `slice`: print bounded line windows, or character windows for very long
  single-line files and pasted attachments.
- `json-find`: query JSON by key, path, or summarized value without opening the
  whole document.
- `json-select`: project a JSON document or array to bounded row summaries using
  JSON Pointer and field selectors. JSONL files are treated as row streams when
  the file is not one complete JSON document. The launcher preserves
  slash-leading JSON Pointer selector arguments on Git Bash/Windows so they are
  not rewritten as host paths before reaching the native binary.
- `sqlite`: run a read-only query from `--sql` or `--sql-file <file>` with row
  caps and receipt metadata. The DB path may be positional, `--db <file>`, or
  `--path <file>`.
- `sqlite-schema`: summarize SQLite tables, columns, indexes, and foreign keys
  from SQLite metadata without hand-written PRAGMA queries. The DB path may be
  positional, `--db <file>`, or `--path <file>`.
- `capture` (`run` alias): execute argv directly and print capped stdout/stderr summaries
  with exit status. Use it only when a command's output cardinality is unknown
  and the command lacks a better native filter or projection.

Use `--json` when another script or tool should consume the result directly.
Use `--fail-if-truncated` (aliases: `--fail-on-truncate`,
`--strict-complete`) when a capped result should stop automation after the
receipt is emitted. Use `--require-complete-scan` when display caps may be fine
but scan-capped lower-bound totals should fail.

## Quick Start

```bash
cargo test
cargo build --release
target/release/contextmink files --path . --max 20
```

`contextmink` uses Rust edition 2024 and requires a recent stable Rust
toolchain. To add it to another repository, follow [SETUP.md](SETUP.md).

Release builds include bundled SQLite support for portability.

## Examples

```bash
scripts/contextmink files --path . --max 20
scripts/contextmink files --path . --max 20 --max-scan-files 5000
scripts/contextmink files vendor --with-git-ignored --max 20
scripts/contextmink grep --pattern-file pattern.txt src tests --max-files 8
scripts/contextmink grep-terms --term "TODO" --term "panic" --or src
scripts/contextmink slice src/main.rs --range 120:180
scripts/contextmink json-find report.json --key-contains error --max 10
scripts/contextmink json-select report.json --array /rows --field id --field /status
scripts/contextmink json-select queue.jsonl --field addr --field flags --limit 10
scripts/contextmink sqlite state.sqlite --sql-file query.sql --max-rows 20
scripts/contextmink sqlite-schema --path state.sqlite --name-contains user --max-tables 8
scripts/contextmink capture --max-lines 40 -- some-tool --compact-target query
scripts/contextmink --fail-if-truncated run --max-lines 40 -- some-tool --compact-target query
```

## Receipts

Every human-readable command ends with `CONTEXTMINK_RECEIPT ` followed by JSON.
If a receipt has `"truncated": true` or `"complete": false`, the output is
capped. Narrow the path, glob, pattern, or slice and run again.
With strict completion flags, contextmink still emits the receipt and then exits
nonzero when the requested completeness condition fails.

Stable receipt fields:

| field | meaning |
| --- | --- |
| `tool` | always `"contextmink"` |
| `command` | subcommand that ran |
| `profile` | active `.contextmink.toml` profile, or `null` |
| `unit` | what `shown` and `total` count |
| `shown` | items printed, in `unit` |
| `total` | items available, in `unit` |
| `truncated` | whether output was capped |
| `complete` | `!truncated` |
| `cap_reason` | why output stopped, or `null` |

For `grep` and `grep-terms`, `shown` and `total` are file counts. Match,
sample, scan, and skip counts are reported in dedicated fields.
When `cap_reason` is `"scan"` or `candidate_files_total_is_lower_bound` is
true, candidate totals and no-match results only describe the scanned subset.
Narrow the path/glob/query before treating the result as complete.
Grep receipts also include `no_match_scope` (`"complete_scope"` or
`"scanned_subset"`) when no files match.

For `capture`, `shown` and `total` are stdout plus stderr line counts. The
receipt records the child command's `exit_code` and `success`; `contextmink`
itself exits successfully when capture succeeds, even if the child command
failed. `capture` is not a shell, sandbox, retry layer, or read-only guard. On
Windows through the Bash launcher, extensionless shell scripts that fail direct
spawn with "not a Win32 application" are retried through the current Bash
interpreter as argv, not as a shell string; receipts include `spawn_fallback`
and `effective_argv` when that happens.

## Configuration

`contextmink` searches upward from the current directory for
`.contextmink.toml`:

```toml
profile = "repo-name"

exclude_globs = [
  "target/**",
  "**/target/**",
  "node_modules/**",
  "**/node_modules/**",
]
```

Keep repository policy in `.contextmink.toml` and repository instructions, not in the
binary. Exclude generated or high-output trees from broad scans, then pass an
explicit subdirectory or file when that tree is the target.
`--with-excluded` includes files matched by contextmink's built-in and
configured exclude globs for the whole command. It does not disable Git ignore
rules; pass an explicit path when an ignored artifact tree is the target.

## Scope

Add to this tool only when the failure mode is generic transcript overflow or
host-shell friction from file enumeration, text search, line slicing, JSON
inspection, read-only SQLite inspection/schema summarization, or bounded capture
of otherwise unknown command output. If behavior needs domain knowledge, a
schema beyond the data being selected, a compiler, an indexer, a runtime, or a
specialized parser, extend that domain tool instead.

## License

MIT. See [LICENSE](LICENSE).
