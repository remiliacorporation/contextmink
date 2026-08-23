# contextmink

[![CI](https://github.com/remiliacorporation/contextmink/actions/workflows/ci.yml/badge.svg)](https://github.com/remiliacorporation/contextmink/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/remiliacorporation/contextmink)](https://github.com/remiliacorporation/contextmink/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**Give coding agents the evidence they need without spending the conversation
on raw tool output.**

An agent exploring a real repository has an awkward choice. Broad commands can
dump thousands of paths, matches, log lines, JSON values, or database rows into
its transcript. Aggressive truncation keeps the transcript usable, but can hide
the one result that changes the answer. Once output is clipped, the agent often
cannot tell whether it saw the whole result or only the beginning.

Contextmink is a small, project-local transcript guard for that gap. Its commands
enumerate, search, read, query, and capture with explicit limits. Every result
ends with a machine-readable receipt that distinguishes:

- a complete result from a bounded subset;
- exact totals from lower bounds;
- inspected evidence from omitted display payload;
- a real no-match from "not found in the portion examined"; and
- a successful child command from output that merely looked successful.

The result is less transcript churn, fewer repeated probes, and a reviewable
record of what the agent actually saw.

## See the difference

Suppose an agent needs to find a rendering path in a repository it has not seen
before. A recursive search may return too much to retain, while a silently
clipped search can support the wrong conclusion. Contextmink keeps the search
bounded and makes the limitation part of the result:

```text
$ scripts/contextmink grep --pattern 'render_chunk' src tests --limit 8
[contextmink] grep pattern="render_chunk"
matching_files_total=11 matching_lines_total=37
...
selected receipt fields:
{"schema":"contextmink.receipt.v2",
 "result":{"shown":8,"total":11,"total_is_lower_bound":false},
 "scope_complete":true,"output_truncated":true,"complete":false}
```

The agent can safely conclude that 11 files matched, while also knowing that
only 8 file payloads were printed. If a scan budget had stopped content
inspection early, `scope_complete` would instead be false and match totals would
be marked as lower bounds. The receipt turns truncation from an invisible
failure mode into usable evidence.

## Where it earns its place

Contextmink is useful anywhere an agent must inspect more material than should
be pasted into a conversation:

| Work | What Contextmink adds |
| --- | --- |
| Entering an unfamiliar repository | Bounded directory and file maps before opening source |
| Searching a large codebase | Exact candidate counts, sampled matches, and honest scope caps |
| Reading large or generated files | Declaration outlines and targeted line/character windows |
| Inspecting JSON, JSONL, or SQLite | Shape discovery and projected rows without dumping whole datasets |
| Running noisy builds and diagnostics | Head-and-tail capture, child exit truth, and descendant cleanup |
| Working across nested repositories | Explicit disclosure of crossed Git roots or strict root isolation |
| Driving Bash-first projects from Windows agents | Lossless argv relay through a native Git Bash bridge |
| Protecting agent-operated repositories | A shared tripwire for known destructive shell commands |

It works well in small repositories too: the commands are fast enough to become
the default inspection vocabulary, so the same workflow continues to hold when
the repository, logs, or evidence store grow.

## A useful agent workflow

Contextmink is designed around progressive retrieval rather than one enormous
search:

```bash
# 1. Learn the shape without printing the tree.
scripts/contextmink dirs crates --depth 2 --limit 40

# 2. Enumerate or search a bounded candidate set.
scripts/contextmink files crates --path-contains render --ext rs --limit 20
scripts/contextmink grep --pattern 'render_chunk' crates --ext rs --limit 8

# 3. Map one relevant file, then read only the useful region.
scripts/contextmink outline crates/render/src/lib.rs --contains render -i
scripts/contextmink slice crates/render/src/lib.rs --range 120:190

# 4. Project structured evidence instead of serializing all of it.
scripts/contextmink json-select queue.jsonl --fields addr,name --limit 20
scripts/contextmink sqlite evidence.sqlite --sql-file query.sql --limit 20

# 5. Bound a command whose output cardinality is not yet known.
scripts/contextmink capture --max-lines 40 -- some-tool --diagnose
```

Humans can read the normal output. Agents and automation can add `--json` and
consume the same receipt contract directly. Project configuration supplies
default excludes and optional destructive-path tripwires, keeping routine
commands concise and repository-aware.

Contextmink complements rather than replaces domain tools. Compilers, language
servers, indexers, debuggers, and project-specific validators remain the
authority for their domains. Contextmink makes the surrounding discovery and
evidence transfer bounded, explicit, and consistent.

## Install

Download the archive for your platform from
[GitHub Releases](https://github.com/remiliacorporation/contextmink/releases),
unpack it, and put `contextmink` on `PATH` or run it in place:

```bash
contextmink files . --limit 20
```

Archives cover Windows x64, macOS Intel, macOS ARM, and Linux x64, with
SQLite bundled. The binary runs directly from PowerShell, cmd, WSL, or any
POSIX shell.

To build from source instead: `cargo build --release` (Rust 1.95 or newer,
edition 2024).

## Add to a project

Run the unpacked release binary from the agent task responsible for maintaining
the target repository:

```bash
./contextmink setup-project /path/to/repository --dry-run
./contextmink setup-project /path/to/repository
```

On Windows PowerShell, use
`& .\contextmink.exe setup-project C:\path\to\repository`. The command copies
the platform-appropriate release artifacts, installs the Bash launcher and the
PowerShell diagnostic shim, generates a real project profile, adds the binary
directory to `.gitignore`, and installs one thin Contextmink skill for open
Agent Skills-compatible harnesses and Claude Code around the canonical
`tools/contextmink/agent_integration.md` reference. It deliberately does not
edit `AGENTS.md` or `CLAUDE.md`: the maintaining agent must inspect the
repository, add one concise discovery trigger, and adapt only project-owned
shell, native-tool, nested-repository, exclusion, and destructive-path policy.

`setup-project` preflights every destination before writing. It is idempotent
for the same release and requires `--replace-managed` to update divergent
release-managed binaries, launchers, or the integration reference. An existing
`.contextmink.toml` is repository-owned: setup validates it with the real
configuration loader, reports `preserve_repository_owned`, and never compares
or replaces it from the release template. Invalid configuration fails before
any file is written. A dry run reports required release-file replacements
without granting permission to perform them.

This is also the fresh-clone repair path. Repositories normally track their
configuration, launchers, skills, and integration reference while ignoring
`tools/contextmink/bin/`; running `setup-project` from an unpacked release
preserves the tracked configuration and restores whichever host binaries are
missing.

After integration, verify from the repository root:

```bash
scripts/contextmink --json files . --limit 1
scripts/contextmink --json guard-check -- git clean
```

The first result must carry `schema: "contextmink.receipt.v2"`; the second must
report `decision: "deny"`. Shell-specific invocation and hermetic-install
choices are covered in [docs/setup.md](docs/setup.md).

## Commands

`contextmink <command> --help` is the authoritative flag reference; the list
below is the short map.

- `dirs` — directory overview with recursive file counts, `--depth` levels
  deep. Orientation before `files` or `grep`.
- `files` — list candidate files. `--glob`, `--path-contains`, and `--ext` filter;
  configured excludes apply to broad scans, while explicit paths bypass them.
  Enumeration deduplicates physical file identity (including hard links,
  symlinks, junctions, case aliases, and overlapping roots); `--limit` caps
  only retained/displayed paths.
  `--quiet` suppresses the path payload, sets `result.shown` to zero, and keeps
  exact totals and scope caps. Deliberate quiet suppression is not output
  truncation.
- `grep` — bounded match summary for a regex or `--literal` pattern. Supply
  exactly one pattern source with `--pattern PATTERN` or `--pattern-file FILE`;
  every positional argument is a search path. `--glob`/`--ext` narrow, `-i`,
  `--context N`, `--limit`, `--max-sample-lines`, `--max-matching-files`,
  `--max-content-files`, and optional deterministic `--max-content-bytes`.
  `--quiet` suppresses per-file match content and file lists, reports zero
  shown/sample rows, and emits only the receipt. Exact totals and scope caps
  remain; sample/output caps that would apply only to suppressed payload do not.
- `grep-terms` — match lines containing every `--term` value (`--any` for
  any). Token search without regex quoting; `--term-file` for phrase lists;
  same narrowing flags as `grep`, including `--quiet`.
- `outline` — declaration map of one source file, printed as `line: text`
  rows (functions, types, headings; for C/C++, also `// ==== Section ====`
  banner titles; for JSON, container-opening keys; for XML, container
  elements via a depth-tracking element-stack parse — named/id'd containers
  at any depth plus shallow unnamed sections, never self-closing leaves).
  Built-in heuristics cover common source/config formats; shebang detection
  handles extensionless scripts, including `/usr/bin/env -S`. Language-aware
  masking removes C-like comments/strings, Rust raw strings, JavaScript
  template literals, and Python triple-quoted bodies before classification;
  C/C++/C# also suppress compile-time `#if 0` sections. Receipts label built-in
  matching as `matcher: "heuristic"` and explicit prefix/regex matching
  separately.
  `--lang` overrides detection, `--prefix <text>` matches literal line
  starts, `--pattern <regex>` covers anything else, `--contains` filters
  rows.
- `slice` — bounded line window from one file: `--range START:END`,
  `--tail N`, or `--char-start OFFSET --chars COUNT` for a complete requested
  character window from a very long single-line file. Line-mode and
  character-mode flags are mutually exclusive rather than silently ignored.
  Defaults to a 120-line window with a 220-line ceiling; receipts report
  `encoding` and `total_lines`.
- `json-find` — locate JSON values by key, path, or summarized value. It uses
  the same strict JSON/JSONL input contract and materialization bound as
  `json-select`.
- `json-select` — project JSON or JSONL rows with `--fields` (bare key,
  JSON Pointer, or comma-separated list). `--where FIELD=VALUE` and
  `--where-contains FIELD=TEXT` filter rows; `--keys` reports the union of
  row keys with presence counts and value types for one-call shape
  discovery; UTF-8 `*.jsonl` streams record by record, while BOM-tagged UTF-16
  stays within the explicit materialization bound; every non-empty physical
  JSONL line is exactly one value across every command; fields null in every
  scanned row are flagged in `all_null_fields`.
  JSON object keys must be unique, integer spelling is preserved beyond
  64-bit ranges, and `--max-document-bytes` bounds materialized JSON documents
  and individual streamed records.
  Selector arguments are data and are never rewritten heuristically; use the
  canonical launcher or native bridge at an MSYS boundary.
- `sqlite` — read-only query against the positional DB file from `--sql` or `--sql-file` with row caps,
  named JSON bindings via `--json-param NAME=FILE` / `--jsonl-param
  NAME=FILE`, a registered `hexint(x)` SQL function (parses `0x...` hex
  strings to INTEGER for indexed joins against integer address columns),
  and a `--timeout-secs` watchdog (default 60). A SQLite authorizer permits
  only reads during preparation and execution, so `ATTACH`, `DETACH`, mutating
  pragmas, and future write-shaped statements are rejected independently of
  the read-only file open.
- `setup-project` — install a project-local release and print the remaining
  agent-owned configuration and guidance work. Supports `--dry-run` and
  explicit `--replace-managed` release upgrades.
- `sqlite-schema` — tables, columns, indexes, and foreign keys of the
  positional DB argument.
- `capture` — execute non-interactive argv with child stdin closed and print
  stdout/stderr within one combined line
  budget and a per-stream byte budget, with the exit status. Truncation keeps
  both head and tail, since verdicts sit at
  the end of tool output. Terminating `capture` also reaps the command and its
  ordinary descendants: Windows uses a kill-on-close Job Object, while Linux
  and macOS use a dedicated process group plus an independent parent-death
  watchdog. Direct mode recognizes files whose first line begins `#!`; use
  `capture --script -- <path> ...` for an intentional Bash script without a
  shebang. Receipts disclose the deterministic `execution_mode` and effective
  argv. Receipt argv fields use the same character bound as captured lines,
  so a hostile argument cannot turn the transcript guard into a transcript
  dump. Captured commands must not deliberately escape containment by
  daemonizing into a new session or process group.
- `hook-snippet` — print a Claude `.claude/settings.json` fragment that
  registers `hook-guard` with shell-safe command strings.
- `hook-guard` — evaluate an agent PreToolUse hook payload from stdin against
  the destructive-command guard; exits 2 to block a recognized destructive
  command.
- `guard-check --command <shell-text> [--shell posix|powershell|cmd]` (or
  `guard-check -- <argv...>`) —
  explain the guard decision without spawning the input. Default output is a
  readable decision line; add `--json` for `contextmink.guard_check.v1`.

Global flags: `--json` emits one JSON object for machine consumption;
`--fail-if-truncated` exits nonzero on capped output;
`--require-complete-scope` exits nonzero when scope caps made totals lower
bounds. Receipt strictness flags fail immediately on commands that do not emit
`contextmink.receipt.v2`; they are never silently ignored.

## Examples

```bash
scripts/contextmink dirs crates --depth 2 --limit 40
scripts/contextmink files specs --ext json --limit 20
scripts/contextmink files crates --path-contains render --path-contains tests --limit 20
scripts/contextmink files vendor --with-git-ignored --limit 20
scripts/contextmink grep --pattern render_chunk src --ext rs --context 2 --limit 8
scripts/contextmink grep --pattern 'render::chunk' src tests --limit 8
scripts/contextmink grep --pattern-file pattern.txt src tests --limit 8
scripts/contextmink grep-terms --term "--flag-like" --term panic --any src --max-sample-lines 12
scripts/contextmink outline src/renderer.rs --contains cull -i
scripts/contextmink outline notes/pseudocode.h --prefix '// PART'
scripts/contextmink outline capture_sidecar.json --limit 30
scripts/contextmink slice src/main.rs --range 120:180
scripts/contextmink slice build.log --tail 40
scripts/contextmink json-select queue.jsonl --fields addr --where-contains name=Cache --limit 10
scripts/contextmink json-select capture_sidecar.json --array entries --keys
scripts/contextmink sqlite state.sqlite --sql-file query.sql --limit 20
scripts/contextmink sqlite state.sqlite --sql-file join.sql --jsonl-param queue=queue.jsonl
# join.sql: SELECT t.name FROM json_each(:queue) q JOIN targets t ON t.addr = hexint(q.value ->> '$.addr')
scripts/contextmink sqlite-schema state.sqlite --name-contains user --max-tables 8
scripts/contextmink capture --max-lines 40 -- some-tool --compact-target query
scripts/contextmink hook-snippet
```

## Receipts

Every bounded inspection command ends with `CONTEXTMINK_RECEIPT` followed by a
`contextmink.receipt.v2` JSON object (under `--json`, that object is the
output). `scope_complete: false` means the result describes only a bounded
subset; `output_truncated: true` means emitted payload was omitted or shortened,
including per-line/per-value character clipping. Character limits include the
ellipsis itself. `complete` is true only when both conditions are clear. The
strict flags emit the receipt first, then exit nonzero.
Failures that prevent inspection from starting (invalid flags, unreadable
inputs, malformed JSON/SQL) exit nonzero with a stderr diagnostic and do not
claim a receipt. Automation must check the process status before parsing
stdout.

| field | meaning |
| --- | --- |
| `tool` | always `"contextmink"` |
| `schema` | always `"contextmink.receipt.v2"` |
| `command` | subcommand that ran |
| `profile` | active `.contextmink.toml` profile, or `null` |
| `result.unit` | what `result.shown` and `result.total` count |
| `result.shown` | result items actually emitted; zero under `--quiet` |
| `result.total` | observed result items |
| `result.total_is_lower_bound` | whether a scope cap prevents an exact total |
| `caps` | structured `{boundary, dimension, limit}` rows |
| `scope_complete` | false when any cap has `boundary: "scope"` |
| `output_truncated` | true when any cap has `boundary: "output"` |
| `complete` | `scope_complete && !output_truncated` |
| `duration_ms` | wall-clock cost of the command |

Search receipts use `result.unit: "matching_files"` and add
`matching_lines_total`, candidate/content admission telemetry, and skip
counts. Candidate enumeration always completes, so `candidate_files_total` is
exact; `--max-content-files`, `--max-content-bytes`,
`--max-matching-files`, or an oversized skipped file add a scope cap and make
the match-side totals lower bounds. `no_match_scope` says whether a no-match verdict covered the
`"complete_scope"` or a `"scanned_subset"`; `skipped_files_sample` names
files skipped as too large or binary. Capture receipts record the child's
`child_exit_code`, `child_exit_zero`, `expected_exit_codes`, and `exit_expected`
(`--expect-exit CODE[,CODE...]` changes only expectedness, not the observed
exit code or zero-code fact). After emitting the receipt, contextmink propagates every child status
not declared by `--expect-exit`; a failed child therefore cannot become a
successful outer workflow. Use `--receipt-out <file>` to write the full capture
  receipt, including the same bounded stdout/stderr text emitted in JSON mode.
  If the sidecar cannot be written, the stdout receipt is still emitted. An
  unexpected child status remains the outer exit status even when strict
  truncation also fails.

## Behavior notes

- Encoding is BOM-driven: UTF-16LE/BE files (the PowerShell `Out-File`
  default) are decoded and searched, a UTF-8 BOM is stripped before JSON
  parsing, CRLF and CR-only line endings are normalized, and files with NUL
  bytes and no UTF-16 BOM are skipped as binary. UTF-32 BOMs fail with an
  explicit conversion instruction instead of being misread as UTF-16.
- `slice`, `outline`, and retained `capture` output receipts flag
  `encoding_suspects` when the decoded text carries proof-grade mojibake (a
  character run whose CP1252 bytes re-decode as valid UTF-8 — the garble an
  em-dash becomes when UTF-8 is re-read as CP1252), U+FFFD replacement
  characters, or raw C1 controls. The field is omitted when nothing is found,
  and it never fails a command — it discloses.
- Capture retains stdout and stderr as separately bounded streams. It does not
  invent a cross-stream chronology that the operating-system pipes cannot
  prove.
- `contextmink-bridge` and `capture` refuse known destructive argv
  before spawn. The evaluator preserves shell quoting and command boundaries,
  resolves Git's actual subcommand, recursively inspects real shell payloads
  and command substitutions, and matches protected paths only against deletion
  operands. Recursive deletion of a protected tree is blocked with or without
  a force flag. The
  `CONTEXTMINK_BRIDGE_ALLOW_DESTRUCTIVE=1` override is for human maintenance
  only and prints a warning.
- The destructive guard is a careless-command tripwire, not a containment or
  authorization boundary. The built-in `git clean` rule and opaque encoded
  PowerShell denial are always active, independent of repository cwd;
  configured protected-path fragments apply only inside their owning project.
  Finite wrappers such as `env -S`, PowerShell encoded-command flags, and
  `find -delete`/`-exec` and an invoked `git -c alias.<name>=... <name>` are
  parsed. Literal POSIX assignments are propagated across command boundaries,
  so `c=clean; git $c` is denied. Computed expansion, sourced scripts,
  repository-configured Git aliases, and runtime `eval` remain outside this
  evaluator; arbitrary dynamic behavior cannot be proven from a pre-execution
  command string.
- `hook-guard` extends the same deny scan to agent-harness PreToolUse hooks:
  it reads the hook event JSON from stdin, extracts the command string at
  `--command-field DOT.PATH` (default `tool_input.command`, the Claude Code
  shape), and exits 2 with the deny message on stderr to block the tool call.
  Generate the Claude settings fragment with `contextmink hook-snippet`; it
  emits single `command` strings rather than a non-portable `args` array,
  normalizes Windows paths to forward slashes for Bash hooks, and binds the
  policy to its repository root with `--expected-root`. Each generated matcher
  also passes its shell dialect explicitly, so PowerShell backtick escapes are
  not interpreted as POSIX command substitutions. A copied or stale hook
  whose payload `cwd` belongs to another checkout allows with a diagnostic
  note instead of applying foreign config. Raw backslash
  paths such as `F:\repo\tools\contextmink.exe` are wrong inside a Bash hook:
  Bash treats the backslashes as escapes and tries to execute a collapsed path.
  A discovered or explicit policy that cannot be loaded fails closed with exit
  2. Unparseable hook-event payloads allow with a stderr note: the guard blocks recognized
  destructive commands, it does not validate harness payloads (fail-closed
  payload handling turns any schema drift into a total shell outage).
- Broad scans cross nested Git repository roots by default, including tracked
  submodules and Git-ignored sibling repositories, apply each repository's own
  ignore rules, and disclose the exact `nested_repos_entered_total` plus a
  bounded `nested_repos_entered_sample`. Pass `--skip-nested-repos` to keep a
  broad scan inside each explicit root.
  Passing a nested repository as an explicit root still scans it normally.
  Repository-discovery I/O failures are hard errors, never silent omissions.
  Repositories below an ignored plain directory beyond the bounded discovery
  depth need an explicit root.
- Outline is navigational, not a compiler-grade parser. Most languages use
  disclosed line-shape heuristics over comment/string-masked text; XML uses a
  lightweight element-stack parse. Indentation conveys nesting.

## Windows

The binary itself needs no shell. One optional native bridge serves
repositories whose scripts are Bash-first while the agent runs in PowerShell:

- `contextmink-bridge.exe` (Windows archive only) runs commands and repo bash
  scripts from PowerShell: it locates Git Bash itself (Git for Windows only;
  Cygwin/MSYS2 never substitute silently — point `CONTEXTMINK_BASH` at an
  exotic shell explicitly), spawns direct commands without MSYS argument
  rewriting, and takes argv as `--argv-b64` or `--argfile` so PowerShell 5.1
  quoting cannot corrupt arguments. In direct mode a program spelled as a
  path (`./gradlew`) resolves against `--cwd` like a POSIX exec. Files whose
  first line begins `#!` enter Git Bash deterministically;
  `--script <path>` explicitly selects a Bash script and resolves it from the
  bridge root. An optional `--` immediately after the script path is consumed
  as the conventional argument separator; double it when the script itself
  must receive a leading `--`. Bare commands in direct mode use the native Windows `PATH`;
  pass `--login` when the command is a utility supplied by Git Bash, such as
  `perl`. `--preserve-descendants` is the explicit exception for a
  successful child that intentionally launches a persistent GUI or service;
  ordinary commands remain kill-on-close supervised. Every bridge-owned Git
  Bash boundary hex-relays startup argv before decoding it and installs scoped
  MSYS conversion exclusions for the caller's slash-bearing values, so a
  quoted `"$@"` forwarded to a native child preserves leading-slash selectors,
  `@file` arguments, and JSON without caller-managed `MSYS2_ARG_CONV_EXCL` state.
  `--print-argv` shows exactly what arrived; `--print-root` shows the resolved
  bridge root.
  Destructive argv matching the safety deny-list is refused before spawn;
  `--help` prints the current deny-list and break-glass override. The bridge
  and `capture` share the same Rust process-boundary implementation; no
  parallel shell bridge is retained.

The `scripts/contextmink` launcher additionally shields slash-bearing JSON
selectors, predicates, regexes, literal terms, SQL, and shell-command values
from MSYS rewriting on Git Bash. Setup and boundary details:
[docs/setup.md](docs/setup.md).

## Configuration

`contextmink` searches upward from the current directory for
`.contextmink.toml`:

```toml
profile = "repo-name"

exclude_globs = [
  "generated/reports/**",
]

# Optional spawn safety for repository-owned critical paths:
# destructive_guard_recursive_delete_fragments = ["protected_cache"]
# destructive_guard_delete_fragments = ["critical.sqlite"]
```

Accepted keys are `profile`, `exclude_globs`,
`destructive_guard_recursive_delete_fragments`, and
`destructive_guard_delete_fragments`; unknown keys, duplicate keys, and
malformed values are hard errors. Repository exclude globs match paths relative
to the config file's directory and apply only inside that tree, so anchored
rules hold from any working directory without leaking into foreign scan roots.
Built-in build/dependency exclusions apply inside every explicit scan root.
Empty profiles and the shipped placeholder profile are hard errors. Use
`--config <file>` for an explicit policy or `--no-config` for built-in defaults
only.
Excludes quiet broad scans only: pass an explicit file or subdirectory when an
excluded tree is the target, or `--with-excluded` to lift the globs for one
command. Git ignore rules are separate; `--with-git-ignored` lifts those.
Configured destructive guard fragments are literal case-insensitive substrings
matched by `contextmink-bridge`, `capture`, and `hook-guard` before a child
process or agent shell command is allowed to run.

## Development

Do not launch contextmink's own replacement release build through a running
`contextmink-bridge`. Let active bridge commands finish, then run
`cargo build --release` from a standalone checkout, `scripts/contextmink` from
a parent repository, or `cargo build --release --manifest-path
tools/contextmink/Cargo.toml` from that parent repository. This avoids Windows
executable-lock contention without adding self-update machinery.

Native CI remains authoritative and runs formatting, tests, Clippy, and package
checks on Windows, Linux, and macOS. Run `scripts/verify_source.sh` for the same
local source gates in dedicated `target/source-check` and
`target/package-check` directories, preventing either prior local artifacts or
Cargo's staged package build from contaminating the proof. On Windows, invoke it through
`contextmink-bridge --script scripts/verify_source.sh`. Source checkouts also
provide an optional cross-link rehearsal for every non-Windows release target.
Install Zig plus `cargo-zigbuild`, then run `scripts/cross_check.sh`. Missing Rust targets fail
with an exact `rustup` command; `scripts/cross_check.sh --install-targets` is the
explicit opt-in to install them into the pinned toolchain. The rehearsal builds
the full compile surface and release binaries for Linux x64, Intel macOS, and
Apple Silicon macOS. A Windows host can report a missing Xcode SDK while still
completing Zig compilation; native GitHub macOS jobs remain the link/runtime
authority. The rehearsal denies every Rust warning except the environment-owned
`linker_messages` lint that carries those non-native SDK and Zig linker notices.
Zig is not a normal build dependency, and the repository does not retain
host-specific compiler wrappers.

Before handing a commit to the public artifact workflow, run
`scripts/verify_release.sh` (through `contextmink-bridge --script` on Windows).
It requires pinned actionlint `1.7.12`, runs the isolated native source gate,
then executes the complete Zig rehearsal. Pass `--install-targets` only when
explicitly authorizing repair of missing pinned-toolchain components.

Keep package verification in a separate Cargo target directory. `cargo package`
verifies the staged source tree under `target/package`; sharing its fingerprints
with a later checkout build can make that build reuse the staged artifact. CI
uses `CARGO_TARGET_DIR=target/package-check cargo package --locked`. Use the same
boundary for local package checks (in PowerShell, set `$env:CARGO_TARGET_DIR`
before the command), then build the checkout in the ordinary target directory.

## Scope

Add to this tool only when the failure mode is generic transcript overflow or
host-shell friction in file enumeration, text search, line slicing, JSON
inspection, read-only SQLite inspection, or bounded capture of unknown
command output. Anything needing domain knowledge, a schema, a compiler, an
indexer, a runtime, or a real parser belongs in the domain tool.

## License

MIT. See [LICENSE](LICENSE). [LICENSE-SSL](LICENSE-SSL) and
[LICENSE-VPL](LICENSE-VPL) accompany every release archive and mirror sync.
