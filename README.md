# contextmink

`contextmink` is a transcript guard for coding agents. It gives agents bounded
ways to list files, search text, read line windows, inspect JSON, and run
bounded read-only SQLite queries without dumping large unknown outputs into the
conversation.

It is deliberately generic. Project-specific parsing, validation, indexing,
diagnostics, and synchronization should stay in project-native tools.

## Commands

- `files`: list candidate files with hard caps and configured excludes.
- `grep`: count matches first, then print a bounded file/sample summary. Use
  `--pattern-file <file>` when regex punctuation would be fragile through a
  host shell bridge.
- `grep-terms`: match lines containing all `--term` values by default, or any
  term with `--mode any`. Use `--term-file <file>` for phrase lists when shell
  quoting or regex punctuation would make inline arguments fragile.
- `slice`: print bounded line windows, or character windows for very long
  single-line files and pasted attachments.
- `json-find`: query JSON by key, path, or summarized value without opening the
  whole document.
- `json-select`: project a JSON document or array to bounded row summaries using
  JSON Pointer and field selectors. The launcher preserves slash-leading JSON
  Pointer selector arguments on Git Bash/Windows so they are not rewritten as
  host paths before reaching the native binary.
- `sqlite`: run a read-only query from `--sql` or `--sql-file <file>` with row
  caps and receipt metadata.
- `sqlite-schema`: summarize SQLite tables, columns, indexes, and foreign keys
  from SQLite metadata without hand-written PRAGMA queries.

Use `--json` when another script or tool should consume the result directly.

## Quick Start

```bash
cargo test
cargo build --release
target/release/contextmink files . --max 20
```

`contextmink` uses Rust edition 2024 and requires a recent stable Rust
toolchain. To add it to another repository, follow [SETUP.md](SETUP.md).

Release builds include bundled SQLite support for agent portability.

## Examples

```bash
scripts/contextmink files . --max 20
scripts/contextmink files . --max 20 --max-scan-files 5000
scripts/contextmink grep --pattern-file pattern.txt src tests --max-files 8
scripts/contextmink grep-terms --term "TODO" --term "panic" --mode any src
scripts/contextmink slice src/main.rs --range 120:180
scripts/contextmink json-find report.json --key-contains error --max 10
scripts/contextmink json-select report.json --array /rows --field id --field /status
scripts/contextmink sqlite state.sqlite --sql-file query.sql --max-rows 20
scripts/contextmink sqlite-schema state.sqlite --name-contains user --max-tables 8
```

## Receipts

Every human-readable command ends with `CONTEXTMINK_RECEIPT ` followed by JSON.
If a receipt has `"truncated": true` or `"complete": false`, the output is
incomplete evidence. Narrow the path, glob, pattern, or slice and run again.

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
Narrow the path/glob/query before treating the result as complete evidence.

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

Keep repository policy in `.contextmink.toml` and agent guidance, not in the
binary.

## Scope

Add to this tool only when the failure mode is generic transcript overflow or
host-shell friction from file enumeration, text search, line slicing, JSON
inspection, or read-only SQLite inspection/schema summarization. If behavior
needs domain knowledge, a schema beyond the data being selected, a compiler, an
indexer, a runtime, or a specialized parser, extend that domain tool instead.

## License

MIT. See [LICENSE](LICENSE).
