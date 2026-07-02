# contextmink

A transcript guard for agent-driven code work. Every command lists, searches,
reads, or inspects with hard output caps and ends with a machine-readable
receipt stating whether the result was complete. Agents get bounded evidence
instead of flooded context; humans can read the same receipts to see what an
agent saw.

Project-specific parsing, validation, indexing, and diagnostics belong in
project-native tools, not here.

## Install

Download the archive for your platform from
[GitHub Releases](https://github.com/remiliacorporation/contextmink/releases),
unpack it, and put `contextmink` on `PATH` or run it in place:

```bash
contextmink files --path . --max 20
```

Archives cover Windows x64, macOS Intel, macOS ARM, and Linux x64, with
SQLite bundled. The binary runs directly from PowerShell, cmd, WSL, or any
POSIX shell.

To build from source instead: `cargo build --release` (stable Rust, edition
2024).

## Add to a project

Copy from the unpacked archive into the target repository:

1. `contextmink(.exe)` to `tools/contextmink/bin/`.
2. `templates/scripts/contextmink` to `scripts/contextmink`. The launcher
   picks or builds the right binary and smooths Git Bash argument handling on
   Windows.
3. `templates/.contextmink.toml` to `.contextmink.toml`; edit the excludes to
   your high-output trees.
4. `templates/AGENTS.contextmink.md` into `AGENTS.md` (Codex) and/or
   `templates/CLAUDE.contextmink.md` into `CLAUDE.md` (Claude). These carry
   the usage policy agents follow.
5. Verify: `scripts/contextmink files --path . --max 20`

Variants (standalone binary, vendored source, delegated setup) and the
Windows bridge are covered in [docs/setup.md](docs/setup.md).

## Commands

`contextmink <command> --help` is the authoritative flag reference; the list
below is the short map.

- `dirs` — directory overview with recursive file counts, `--depth` levels
  deep. Orientation before `files` or `grep`.
- `files` — list candidate files. `--glob` and `--ext` filter; configured
  excludes apply to broad scans, while explicit paths bypass them.
- `grep` — bounded match summary for a regex or `--literal` pattern.
  `--pattern-file` for shell-fragile regex, `--glob`/`--ext` to narrow, `-i`,
  `--context N`, `--limit`, `--max-matches`. `--quiet` suppresses per-file
  match content and file lists and emits only the receipt (totals, caps,
  truncation, scan-scope fields) — for existence/count checks that do not
  need the matching lines.
- `grep-terms` — match lines containing every `--term` value (`--or` for
  any). Token search without regex quoting; `--term-file` for phrase lists;
  same narrowing flags as `grep`, including `--quiet`.
- `outline` — declaration map of one source file, printed as `line: text`
  rows (functions, types, headings). 19 built-in languages via token
  classifiers, shebang detection for extensionless scripts. `--lang`
  overrides detection, `--prefix <text>` matches literal line starts,
  `--pattern <regex>` covers anything else, `--contains` filters rows.
- `slice` — bounded line window from one file: `--range START:END`,
  `--tail N`, or a character window for very long single-line files.
  Defaults to a 120-line window with a 220-line ceiling; receipts report
  `encoding` and `total_lines`.
- `json-find` — locate JSON values by key, path, or summarized value.
- `json-select` — project JSON or JSONL rows to selected fields via JSON
  Pointer. `--where FIELD=VALUE` and `--where-contains FIELD=TEXT` filter
  rows; `*.jsonl` streams without loading; fields null in every scanned row
  are flagged in `all_null_fields`.
- `sqlite` — read-only query from `--sql` or `--sql-file` with row caps and
  a `--timeout-secs` watchdog (default 60).
- `sqlite-schema` — tables, columns, indexes, and foreign keys of a
  database.
- `capture` (alias `run`) — execute argv and print capped stdout/stderr with
  the exit status. Truncation keeps both head and tail, since verdicts sit at
  the end of tool output.

Global flags: `--json` emits one JSON object for machine consumption;
`--fail-if-truncated` exits nonzero on capped output;
`--require-complete-scan` exits nonzero when scan caps made totals lower
bounds.

## Examples

```bash
scripts/contextmink dirs crates --depth 2 --max 40
scripts/contextmink files --path specs --ext json --max 20
scripts/contextmink files --path vendor --with-git-ignored --max 20
scripts/contextmink grep CMapChunk src --ext rs --context 2 --limit 8
scripts/contextmink grep --pattern-file pattern.txt src tests --limit 8
scripts/contextmink grep-terms --term "--flag-like" --term panic --or src --max-matches 12
scripts/contextmink outline src/renderer.rs --contains cull -i
scripts/contextmink outline notes/pseudocode.h --prefix '// PART'
scripts/contextmink slice src/main.rs --range 120:180
scripts/contextmink slice build.log --tail 40
scripts/contextmink json-select queue.jsonl --field addr --where-contains name=CMap --limit 10
scripts/contextmink sqlite --path state.sqlite --sql-file query.sql --max-rows 20
scripts/contextmink sqlite-schema --path state.sqlite --name-contains user --max-tables 8
scripts/contextmink capture --max-lines 40 -- some-tool --compact-target query
```

## Receipts

Every command ends with `CONTEXTMINK_RECEIPT` followed by JSON (under
`--json`, the receipt is the output object). `"truncated": true` or
`"complete": false` means the output was capped: narrow the query and rerun.
The strict flags emit the receipt first, then exit nonzero.

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
| `duration_ms` | wall-clock cost of the command |

Search receipts add match, scan, and skip counts. Lower-bound fields
(`candidate_files_total_is_lower_bound`, `matched_files_total_is_lower_bound`,
`total_matches_is_lower_bound`) mean a scan cap fired and totals describe only
the scanned subset; `no_match_scope` says whether a no-match verdict covered
the `"complete_scope"` or a `"scanned_subset"`; `skipped_files_sample` names
files skipped as too large or binary. Capture receipts record the child's
`exit_code` and `success` (contextmink itself exits zero when capture worked,
even if the child failed) plus per-stream head, tail, and omitted line
counts.

## Behavior notes

- Encoding is BOM-driven: UTF-16LE/BE files (the PowerShell `Out-File`
  default) are decoded and searched, a UTF-8 BOM is stripped before JSON
  parsing, and files with NUL bytes and no UTF-16 BOM are skipped as binary.
- Broad scans enter git-ignored directories that are themselves repository
  roots, apply that repository's own ignore rules, and disclose each entry in
  `nested_repos_entered`. Multi-repo workspaces would otherwise report
  complete scans that silently skipped sibling repos. `--skip-nested-repos`
  restores strict Git scope; repos nested below an ignored plain directory
  are not auto-detected and need explicit roots.
- Outline rows come from line-shape heuristics, not a parser: false
  positives are possible and indentation conveys nesting.

## Windows

The binary itself needs no shell. Two optional pieces serve repositories
whose scripts are Bash-first while the agent runs in PowerShell:

- `contextmink-bridge.exe` (Windows archive only) runs commands and repo bash
  scripts from PowerShell: it locates Git Bash itself (Git for Windows only;
  Cygwin/MSYS2 never substitute silently — point `CONTEXTMINK_BASH` at an
  exotic shell explicitly), spawns direct commands without MSYS argument
  rewriting, and takes argv as `--argv-b64` or `--argfile` so PowerShell 5.1
  quoting cannot corrupt arguments. `--print-argv` shows exactly what
  arrived; `--print-root` shows the resolved bridge root.
- `templates/scripts/codex-bash.sh` is the same bridge as a shell script, for
  repositories that do not want a second binary.

The `scripts/contextmink` launcher additionally shields slash-bearing
`--pattern`, `--prefix`, `--contains`, `--term`, and JSON Pointer values from
MSYS rewriting on Git Bash. Setup and boundary details:
[docs/setup.md](docs/setup.md).

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

These two keys are the whole surface; unknown keys, duplicate keys, and
malformed values are hard errors. Exclude globs match paths relative to the
config file's directory, so anchored rules hold from any working directory.
Excludes quiet broad scans only: pass an explicit file or subdirectory when
an excluded tree is the target, or `--with-excluded` to lift the globs for
one command. Git ignore rules are separate; `--with-git-ignored` lifts those.

## Scope

Add to this tool only when the failure mode is generic transcript overflow or
host-shell friction in file enumeration, text search, line slicing, JSON
inspection, read-only SQLite inspection, or bounded capture of unknown
command output. Anything needing domain knowledge, a schema, a compiler, an
indexer, a runtime, or a real parser belongs in the domain tool.

## License

MIT. See [LICENSE](LICENSE). [LICENSE-SSL](LICENSE-SSL) and
[LICENSE-VPL](LICENSE-VPL) accompany every release archive and mirror sync.
