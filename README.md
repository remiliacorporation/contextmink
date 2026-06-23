# contextmink

`contextmink` is a transcript guard for coding agents. It gives agents bounded
ways to list files, search text, read line windows, and inspect JSON without
dumping large unknown outputs into the conversation.

It is deliberately generic. Project-specific parsing, validation, indexing,
diagnostics, and synchronization should stay in project-native tools.

## Commands

- `files`: list candidate files with hard caps and configured excludes.
- `grep`: count matches first, then print a bounded file/sample summary.
- `grep-terms`: match lines containing all `--term` values by default, or any
  term with `--mode any`. Use `--term-file <file>` for phrase lists when shell
  quoting or regex punctuation would make inline arguments fragile.
- `slice`: print bounded line windows, or character windows for very long
  single-line files and pasted attachments.
- `json-find`: query JSON by key, path, or summarized value without opening the
  whole document.

Use `--json` when another script or tool should consume the result directly.

## Quick Start

```bash
cargo test
cargo build --release
target/release/contextmink files . --max 20
```

`contextmink` uses Rust edition 2024 and requires a recent stable Rust
toolchain. To add it to another repository, follow [SETUP.md](SETUP.md).

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

Add to this tool only when the failure mode is generic transcript overflow from
file enumeration, text search, line slicing, or JSON inspection. If behavior
needs domain knowledge, a schema, a compiler, an indexer, a runtime, or a
specialized parser, extend that domain tool instead.

## License

MIT. See [LICENSE](LICENSE).
