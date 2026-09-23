# contextmink

**Give coding agents the evidence they need without spending the conversation
on raw tool output.**

An agent exploring a real repository has an awkward choice. Broad commands can
dump thousands of paths, matches, log lines, JSON values, or database rows into
its transcript. Aggressive truncation keeps the transcript usable, but can hide
the one result that changes the answer. Once output is clipped, the agent often
cannot tell whether it saw the whole result or only the beginning.

Contextmink is a small transcript guard for that gap. Its commands
enumerate, search, read, query, and capture with explicit limits. Every result
ends with a machine-readable receipt that distinguishes:

- a complete result from a bounded subset;
- exact totals from lower bounds;
- inspected evidence from omitted display payload;
- a real no-match from "not found in the portion examined"; and
- a successful child command from output that merely looked successful.

The result is less transcript churn, fewer repeated probes, and a reviewable
record of what the agent actually saw.

## Install

### Download and verify

Download the archive for the machine where the agent runs from
[GitHub Releases](https://github.com/remiliacorporation/contextmink/releases):

- `contextmink-<version>-windows-x86_64.zip`
- `contextmink-<version>-macos-x86_64.tar.gz`
- `contextmink-<version>-macos-arm64.tar.gz`
- `contextmink-<version>-linux-x86_64.tar.gz`

Verify it against the adjacent `.sha256` file. This is the integrity check:
Contextmink does not re-hash its binaries when they run.

```bash
sha256sum -c contextmink-<version>-<platform>.<archive-ext>.sha256
```

```powershell
$archive = "contextmink-<version>-windows-x86_64.zip"
$expected = ((Get-Content "$archive.sha256" -Raw).Trim() -split '\s+')[0]
$actual = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLowerInvariant()
if ($actual -ne $expected) { throw "contextmink archive checksum mismatch" }
```

SQLite is bundled; the binary needs no runtime and runs from PowerShell, cmd,
WSL, or any POSIX shell.

### Unpack into the project root

Each archive is a project overlay: every file ships once, at the path it is
used from. Unpack it into the project root, including the dot-directories, and
it is ready to use:

```text
.agents/skills/contextmink/SKILL.md
.agents/skills/contextmink/agents/openai.yaml
.claude/skills/contextmink/SKILL.md
tools/contextmink/bin/contextmink[.exe]
tools/contextmink/agent_integration.md
tools/contextmink/manifest.json
tools/contextmink/README.md
tools/contextmink/CHANGELOG.md
tools/contextmink/LICENSE, LICENSE-SSL, LICENSE-VPL
```

The Windows archive adds `tools/contextmink/bin/contextmink-bridge.exe` and a
`contextmink-bridge` skill in `.agents` and `.claude` (see
[Windows bridge](#windows-bridge)). `manifest.json` records the release version,
source commit, and binary hashes.

Start a fresh agent session. No install command, PATH change, or `AGENTS.md`
edit is needed: the skill tells the agent where the executable is. Claude reads
`.claude/skills`; Codex, Pi, Cursor, OMP, OpenCode and other Agent Skills
consumers read `.agents/skills`. Both copies have the same body. Skill
descriptions support automatic selection; they do not guarantee a model picks
the tool on every request.

The archive never contains project guidance, configuration, receipts, or
databases, so unpacking it overwrites nothing the project owns. Retrieval uses
the project's `.contextmink.toml` when present and built-in defaults otherwise.
To upgrade, unpack the newer archive over the old one.

The rest of this section is optional.

### Managed project installation

`setup-project` turns the overlay into a repository-owned integration. Use it
when the repository wants a Bash launcher, a real configuration profile,
a pinned skill selection, and an uninstall path. Run it from an unpacked release:

```bash
./tools/contextmink/bin/contextmink setup-project /path/to/repository --dry-run
./tools/contextmink/bin/contextmink setup-project /path/to/repository
```

```powershell
& .\tools\contextmink\bin\contextmink.exe setup-project C:\path\to\repository --dry-run
& .\tools\contextmink\bin\contextmink.exe setup-project C:\path\to\repository
```

It preflights every destination before the first write, then installs:

- this host's binaries in `tools/contextmink/bin/` (plus the bridge on Windows)
  and a `.gitignore` entry for that directory;
- the `scripts/contextmink` Bash launcher and
  `tools/contextmink/agent_integration.md`;
- the Contextmink skill at the selected discovery paths;
- `.contextmink.toml` with a real profile named after the repository, if none
  exists; and
- `tools/contextmink/project-install.json` (`contextmink.project_install.v3`),
  recording the release version, the resolved skill target, and the
  `.gitignore` block or file setup created.

Skills, the launcher, and the integration reference are release files: setup
writes them as the release ships them and reports `replace` when a local copy
differs. Keep project-specific guidance in `AGENTS.md`, `CLAUDE.md`, or
`.contextmink.toml`. An existing `.contextmink.toml` is repository-owned: setup
validates it, reports `preserve_repository_owned`, and never replaces it;
invalid configuration fails before any write. Setup never edits `AGENTS.md`,
`CLAUDE.md`, harness settings, or hooks. The `--dry-run` report
(`contextmink.project_setup.v3`) lists every create, replace, and removal and
its `ready` verdict; read its `next_actions`.

**Skill target.** `--skill-target auto` (the default) resolves once and the
receipt freezes the result. Detection is
path-based rather than a closed harness allowlist: `.agents`, `.codex`, `.cursor`, `.pi`, `.omp`, `.opencode`,
`opencode.json`, `opencode.jsonc`, or `AGENTS.md` select `agents`; `.claude` or
`CLAUDE.md` select `both`, so shared discovery stays available; an unmarked
project selects `none`. Pass `--skill-target agents|claude|both|none` to choose
or reselect; deselected skill files are removed. An unreceipted file at a
deselected Contextmink skill path makes the plan unready until you move it.
Setup manages only `.agents/skills/contextmink` and `.claude/skills/contextmink`
(plus the bridge skills on Windows); harness markers never create extra `.pi`,
`.omp`, or `.opencode` copies. Both installed bodies are
generated from one template. Pi requires project trust before loading project-local resources:
save the decision, or pass `--approve` for a noninteractive run.

**Adapting to the repository.** Add only this repository's generated or
high-output trees to `exclude_globs`, and literal deletion fragments for
irrecoverable paths (see [Configuration](#configuration)). Decide whether broad
scans may cross nested repositories or should use exact roots or
`--skip-nested-repos`. Keep project-native compilers, query tools, and
diagnostics authoritative. Then verify from the repository root and a nested
directory:

```bash
scripts/contextmink --json files . --show-files 1        # schema contextmink.receipt.v2, intended profile
scripts/contextmink --json guard-check -- git clean      # decision "deny"
```

**Fresh clones and upgrades.** Repositories normally track the configuration,
launcher, skills, integration reference, and install receipt, and ignore
`tools/contextmink/bin/`. Rerunning `setup-project` from an unpacked release
preserves the tracked configuration and restores missing host binaries. For an
upgrade, rerun the newer release with `--dry-run` first; `auto` keeps the
receipt's skill target. Do not hand-edit the receipt.

**Binaries are owned by path.** The owned binary paths are fixed:
`tools/contextmink/bin/contextmink`, `contextmink.exe`, and
`contextmink-bridge.exe`. Setup writes the ones this host runs and never
touches the others, so a checkout shared between Windows and WSL keeps both
platforms' binaries: run `setup-project` from each host.

**Removal.** Run `uninstall-project` from an unpacked matching or newer release
outside the project; the project-local binary cannot remove itself, and the
command refuses to try.

```bash
./tools/contextmink/bin/contextmink uninstall-project /path/to/repository --dry-run
./tools/contextmink/bin/contextmink uninstall-project /path/to/repository
```

It requires `project-install.json`. It removes the launcher, skills, and
integration reference the receipt's skill target implies, every owned binary
path that exists (the other platform's included), and the `.gitignore` block
setup created, without comparing content, then prunes empty Contextmink
directories. It refuses a symlink or non-file at an owned path before deleting
anything. `.contextmink.toml`, `AGENTS.md`, `CLAUDE.md`, harness settings, and
unrelated skills stay; remove any obsolete project policy deliberately.

### Personal installation

`setup-user` installs Contextmink for every project on the machine instead of
one. Run it from an unpacked release:

```sh
./tools/contextmink/bin/contextmink setup-user --dry-run
./tools/contextmink/bin/contextmink setup-user
```

```powershell
.\tools\contextmink\bin\contextmink.exe setup-user --dry-run
.\tools\contextmink\bin\contextmink.exe setup-user
```

It writes the skill to `~/.agents/skills/contextmink` and
`~/.claude/skills/contextmink`, bound to the exact installed executable, and the
executable, integration reference and `user-install.json` receipt under
`~/.local/share/contextmink` (the same home-relative layout on Windows). On
Windows it also installs `contextmink-bridge.exe` and its skill. It changes no
PATH, shell profile, guidance, hooks, or project files, and runs the installed
executable before reporting success. `--home <existing-directory>` selects
another home, such as a disposable test home.

The receipt (`contextmink.user_install.v2`) records the release version, the
home, and the owned paths; it records no hashes. Each run of the personal
executable or bridge checks that the receipt exists and names this release and
this home, and refuses to run otherwise; rerun `setup-user` from a verified
release to repair. Setup writes every owned path and `uninstall-user` removes
every owned path, whatever they contain. `uninstall-user` keeps the receipt and
never touches project installations or unrelated skills.

Run setup, repair, and removal from an unpacked release, not the installed
executable. Setup preflights every path; on Windows it also checks that
executables it must replace or remove are not locked, including during
`--dry-run`, so close running Contextmink processes when it refuses. A process
can still take a lock after preflight: setup is not a crash-atomic multi-file
transaction: after an interruption the runtime may refuse to start until
setup is rerun.
Do not copy the receipt to another machine or home; install there instead. A
synced skill does not install a runtime in a remote or cloud environment.

### Guard hook

`guard-hook` applies Contextmink's destructive-command tripwire to an agent's
shell calls as a Claude PreToolUse hook. Generate the settings fragment rather
than writing it by hand, and merge it yourself:

```bash
scripts/contextmink guard-hook-snippet
```

The fragment registers `guard-hook` for the `Bash` and `PowerShell` matchers
with shell-safe single `command` strings and absolute paths, bound to the
repository root that owns the selected `.contextmink.toml`. Put it in
`.claude/settings.local.json`; use the shared `.claude/settings.json` only when
the binary and config paths are the same in every supported checkout. For
custom layouts pass `--binary` and `--guard-config`; see
`contextmink guard-hook-snippet --help`. A hook whose payload `cwd` is outside
the bound root allows with a diagnostic instead of applying another
repository's policy; regenerate the fragment after moving a repository.

The hook exits 2 to block a recognized destructive command. Any failure before
or during evaluation also exits 2 and blocks every command, harmless ones
included: a policy that cannot load, a failed personal-install check (for
example a missing receipt or one from another release), a rewritten argument,
or an internal error. The stderr message names the repair. Blocking requires an
executable that starts: a missing binary exits with the shell's own status,
which Claude treats as a non-blocking error, so remove the hook registration
before `uninstall-user`. An unparseable hook payload allows with a note, so
harness schema drift cannot disable all shell use.

On Windows, raw backslash paths such as `F:\repo\tools\contextmink.exe` break
inside a Bash hook command: Bash reads the backslashes as escapes. The
generated fragment uses forward slashes and quotes paths with spaces. Every
matcher's command is a POSIX string because Claude runs hooks through its POSIX
hook runner; the matcher only selects the `--shell` dialect used to parse the
intercepted command.

### Running from each shell

| Active shell | Command form |
| --- | --- |
| Bash (macOS, Linux, Git Bash, WSL) with `setup-project` | `scripts/contextmink ...` |
| Any shell, native executable | `tools/contextmink/bin/contextmink ...` |
| PowerShell, native executable | `& tools\contextmink\bin\contextmink.exe ...` |
| PowerShell, Bash launcher or script | `& tools\contextmink\bin\contextmink-bridge.exe --script scripts/contextmink ...` |

PowerShell cannot open the extensionless `scripts/contextmink` directly. From
Git Bash, prefix `MSYS_NO_PATHCONV=1` when calling the native executable with
an argument that starts with `/`; see [Windows bridge](#windows-bridge).

### From source

`cargo install --path .` (Rust 1.95 or newer, edition 2024) builds and installs
the binary. Building, vendoring the crate into another repository, and the
release gates are covered in
[docs/setup.md](https://github.com/remiliacorporation/contextmink/blob/master/docs/setup.md)
in the source repository.

## See the difference

Suppose an agent needs to find a rendering path in a repository it has not seen
before. A recursive search may return too much to retain, while a silently
clipped search can support the wrong conclusion. Contextmink keeps the search
bounded and makes the limitation part of the result:

```text
$ scripts/contextmink grep --pattern 'render_chunk' src tests --show-files 8
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
scripts/contextmink dirs crates --depth 2 --show-dirs 40

# 2. Enumerate or search a bounded candidate set.
scripts/contextmink files crates --path-contains render --ext rs --show-files 20
scripts/contextmink grep --pattern 'render_chunk' crates --ext rs --show-files 8

# 3. Map one relevant file, then read only the useful region.
scripts/contextmink outline crates/render/src/lib.rs --contains render -i
scripts/contextmink slice crates/render/src/lib.rs --range 120:190

# 4. Project structured evidence instead of serializing all of it.
scripts/contextmink json-select queue.jsonl --fields addr,name --show-rows 20
scripts/contextmink sqlite evidence.sqlite --sql-file query.sql --show-rows 20

# 5. Bound a command whose output cardinality is not yet known.
scripts/contextmink capture --show-lines 40 -- some-tool --diagnose
```

Humans can read the normal output. Agents and automation can add `--json` and
consume the same receipt contract directly. Project configuration supplies
default excludes and optional destructive-path tripwires, keeping routine
commands concise and repository-aware.

Contextmink complements rather than replaces domain tools. Compilers, language
servers, indexers, debuggers, and project-specific validators remain the
authority for their domains. Contextmink makes the surrounding discovery and
evidence transfer bounded, explicit, and consistent.

Use the smallest scope that answers the question. For known root metadata,
read its exact path or enumerate only root files in the host shell; a filename
filter on a recursive `files` scan does not prune directory traversal. `dirs
--depth` bounds displayed levels only. Prefer an outline, targeted grep, or a
character window over repeated wide slices. Budget the **combined** output of
parallel calls; per-command caps cannot prevent an outer tool from clipping the
batch. Direct known-small reads remain appropriate and need no extra receipt.

## Commands

`contextmink <command> --help` is the authoritative flag reference; the list
below is the short map.

- `dirs` — directory overview with recursive file counts, `--depth` levels
  deep, including admitted empty directories. Overlapping roots retain ancestor
  counts; each physical file counts once per directory even when it has aliases
  in different subtrees. `--max-files-counted` limits counting, not directory
  enumeration: directory totals stay exact, while capped file counts use
  `files>=N` and `file_counts_are_lower_bounds: true` with incomplete scope.
  Directory inputs are required; use `files` to inspect an explicit file.
- `files` — list candidate files. `--glob`, `--path-contains`, and `--ext` filter;
  configured excludes apply to broad scans, while explicit paths bypass them.
  Enumeration deduplicates physical file identity (including hard links,
  symlinks, junctions, case aliases, and overlapping roots); `--show-files`
  caps only retained/displayed paths.
  `--quiet` suppresses the path payload, sets `result.shown` to zero, and keeps
  exact totals and scope caps. Deliberate quiet suppression is not output
  truncation.
- `grep` — bounded match summary for a regex or `--literal` pattern. Supply
  exactly one pattern source with `--pattern PATTERN` or `--pattern-file FILE`;
  every positional argument is a search path. `--glob`/`--ext` narrow, `-i`,
  `--context N`, display caps `--show-files`, `--show-lines-per-file`,
  `--show-lines`, and `--show-line-chars`, and scope caps
  `--max-matching-files`, `--max-content-files`, `--max-file-bytes`, and
  optional deterministic `--max-content-bytes`.
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
  Without `--range` or `--tail` it reads from line 1; `--line-ceiling`
  (default 220) bounds every line window, and a longer window reports the
  omitted remainder as `remaining_range`. Receipts report `encoding` and
  `total_lines`.
- `json-find` — locate JSON values by key (`--key-contains`, `--key-regex`),
  JSON Pointer (`--pointer-contains`, `--pointer-regex`), or summarized value
  (`--value-contains`). Repeated `--*-contains` filters must all hold, as in
  every Contextmink command; use the matching `--*-regex` for alternatives. It uses
  the same strict JSON/JSONL input contract and materialization bound as
  `json-select`. Match paths and path filters use [JSON Pointer](https://www.rfc-editor.org/rfc/rfc6901.html):
  `/items/0/name`, with `~0` for a tilde and `~1` for a slash in a key.
  The empty pointer addresses the document root. JSONL is a logical array of
  non-empty records, so `/12/result` addresses the thirteenth record's result.
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
  `--at KEY_OR_POINTER` selects any value: objects and scalars yield one row,
  while arrays yield their elements as rows. Pass a `json-find` match path
  directly to `--at`, including pointers containing commas or whitespace;
  `--at /result --keys` discovers a nested object's shape. This replaces the
  former `--array` flag and the receipt's `array` field; receipts use `at`.
  A missing `--at` target refuses. For UTF-8 JSONL, selection stays streaming
  and validates later records too. Selector syntax is validated before any
  inspection, including on empty input or when a preceding token is missing.
  `--at /instructions --entries --fields address,disassembly` projects an
  object of keyed records without enumerating opaque keys first: each JSON row
  adds the exact `key`, a reusable escaped `pointer`, `value_type`, and separate
  `missing_fields`/`null_fields`, in lexical key order. For JSONL, select a
  record explicitly, for example `--at /1/instructions`.
- `sqlite` — read-only query against the positional DB file from `--sql` or `--sql-file` with row caps,
  named JSON bindings via `--json-param NAME=FILE` / `--jsonl-param
  NAME=FILE`, a registered `hexint(x)` SQL function (parses `0x...` hex
  strings to INTEGER for indexed joins against integer address columns),
  and a `--timeout-secs` watchdog (default 60). A SQLite authorizer permits
  only reads during preparation and execution, so `ATTACH`, `DETACH`, mutating
  pragmas, and future write-shaped statements are rejected independently of
  the read-only file open.
- `sqlite-schema` — tables, columns, indexes, and foreign keys of the
  positional DB argument. `--with-shadow-tables` and `--with-system-tables`
  include virtual-table shadow tables and `sqlite_*` tables.
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
- `setup-project` / `uninstall-project` — managed project installation and
  removal; see [Managed project installation](#managed-project-installation).
- `setup-user` / `uninstall-user` — personal installation and removal; see
  [Personal installation](#personal-installation).
- `guard-hook-snippet` — print a Claude settings JSON fragment that registers
  `guard-hook`; see [Guard hook](#guard-hook). It only prints.
- `guard-hook` — evaluate an agent PreToolUse hook payload from stdin against
  the destructive-command guard; exits 2 to block.
- `guard-check --command <shell-text> [--shell posix|powershell|cmd]` (or
  `guard-check -- <argv...>`) —
  explain the guard decision without spawning the input. Default output is a
  readable decision line; add `--json` for `contextmink.guard_check.v1`.
  Reports identify the Contextmink-only policy scope, configuration root and
  active protected fragments. They do not evaluate host approval policies or
  authorize execution, and do not apply the destructive override environment
  variable. Direct shell calls outside Contextmink are not governed by this
  diagnostic.
  Git `rm --cached` preserves working files (but changes the index), and Git
  `rm -n`/`--dry-run` does not remove files; these modes are exempt from file
  deletion checks when their options are unambiguous. Options after `--` are
  filenames. The built-in prohibition on `git clean` remains unchanged.

The only global flag is `--json`, which emits one JSON object for machine
consumption. Receipt options follow the subcommand on every command that emits
`contextmink.receipt.v2`: `--fail-if-truncated` exits nonzero on capped output,
and `--require-complete-scope` exits nonzero when scope caps made totals lower
bounds. Configuration options `--config FILE` and `--no-config` follow the
subcommand on every command that reads `.contextmink.toml`. Setup and removal
commands accept neither group; a misplaced or inapplicable option is refused
with the fix rather than ignored.

## Examples

```bash
scripts/contextmink dirs crates --depth 2 --show-dirs 40
scripts/contextmink files specs --ext json --show-files 20
scripts/contextmink files crates --path-contains render --path-contains tests --show-files 20
scripts/contextmink files vendor --with-git-ignored --show-files 20
scripts/contextmink grep --pattern render_chunk src --ext rs --context 2 --show-files 8
scripts/contextmink grep --pattern 'render::chunk' src tests --show-files 8
scripts/contextmink grep --pattern-file pattern.txt src tests --show-files 8
scripts/contextmink grep-terms --term "--flag-like" --term panic --any src --show-lines 12
scripts/contextmink outline src/renderer.rs --contains cull -i
scripts/contextmink outline notes/pseudocode.h --prefix '// PART'
scripts/contextmink outline capture_sidecar.json --show-items 30
scripts/contextmink slice src/main.rs --range 120:180
scripts/contextmink slice build.log --tail 40
scripts/contextmink json-select queue.jsonl --fields addr --where-contains name=Cache --show-rows 10
scripts/contextmink json-select capture_sidecar.json --at entries --keys
scripts/contextmink sqlite state.sqlite --sql-file query.sql --show-rows 20
scripts/contextmink sqlite state.sqlite --sql-file join.sql --jsonl-param queue=queue.jsonl
# join.sql: SELECT t.name FROM json_each(:queue) q JOIN targets t ON t.addr = hexint(q.value ->> '$.addr')
scripts/contextmink sqlite-schema state.sqlite --name-contains user --show-tables 8
scripts/contextmink capture --show-lines 40 -- some-tool --compact-target query
scripts/contextmink guard-hook-snippet
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
counts. Every receipt with an output cap carries `output_cap_arguments`, naming
only the exhausted display controls: for example, `--show-lines-per-file`
rather than `--show-files` when matches within a displayed file were omitted.
Display caps are spelled `--show-*` (slice's window bound is `--line-ceiling`);
`--max-*` flags bound inspected scope or admitted input. Narrow the query before raising
the corresponding control. Line slices report `remaining_range` when the
requested window exceeds the line cap; pass it to `slice FILE --range ...` to
continue. It does not recover character-clipped text or promise a file snapshot.
Candidate enumeration always completes, so `candidate_files_total` is
exact; `--max-content-files`, `--max-content-bytes`,
`--max-matching-files`, or an oversized skipped file add a scope cap and make
the match-side totals lower bounds. `no_match_scope` says whether a no-match verdict covered the
`"complete_scope"` or a `"scanned_subset"`; `skipped_files_sample` names
files skipped as too large or binary.

Capture receipts record the child's `child_exit_code`, `child_exit_zero`,
`expected_exit_codes`, and `exit_expected` (`--expect-exit CODE[,CODE...]`
changes only expectedness, not the observed exit code or zero-code fact). After
emitting the receipt, contextmink propagates every child status not declared by
`--expect-exit`; a failed child therefore cannot become a successful outer
workflow. Use `--receipt-out <file>` to write the full capture receipt,
including the same bounded stdout/stderr text emitted in JSON mode. If the
sidecar cannot be written, the stdout receipt is still emitted. An unexpected
child status remains the outer exit status even when strict truncation also
fails.

Capture receipts also report `executable.path`, `executable.source` and
`executable.error`. On Windows, the process handle identifies the spawned image
(including an interpreter when one was selected); it does not identify a later
program launched by that interpreter. Elsewhere the receipt explicitly reports
unobserved identity. A shell may select a different alias or shim: use an
explicit native executable path when exact selection matters. Legitimate empty
successful output remains successful.

For costly or state-changing producers, arrange output retention **before** the
run: redirect stdout/stderr to explicit producer-owned files, retain the native
exit code, wait for completion, then inspect those files with `slice --tail`,
character windows or `json-select`. `capture --receipt-out` stores the receipt
and bounded displayed text, not omitted original output. A display cap is not a
reason to repeat execution.

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
- `contextmink-bridge` and `capture` refuse known destructive argv before
  spawn, and `guard-hook` blocks the same commands in an agent shell. The evaluator preserves shell quoting and command
  boundaries, resolves Git's actual subcommand, recursively inspects real shell
  payloads and command substitutions, and matches protected paths only against
  deletion operands. Recursive deletion of a protected tree is blocked with or
  without a force flag. The `CONTEXTMINK_BRIDGE_ALLOW_DESTRUCTIVE=1` override
  is for human maintenance only and prints a warning.
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
- `guard-hook` reads the hook event JSON from stdin and extracts the command
  string at the JSON Pointer `--command-field POINTER` (default
  `/tool_input/command`, the Claude Code shape). Failure handling is described
  under [Guard hook](#guard-hook).
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

## Windows bridge

The native binary needs no shell. `contextmink-bridge.exe` (Windows archive
only) serves repositories whose scripts are Bash-first while the agent runs in
PowerShell; POSIX hosts need no bridge.

```powershell
# Direct command; slash-bearing arguments arrive verbatim:
& tools\contextmink\bin\contextmink-bridge.exe -- <program> <args...>
# Repository Bash script, Git Bash discovered automatically; the separator is optional:
& tools\contextmink\bin\contextmink-bridge.exe --script scripts/some_tool.sh -- <args...>
# Lossless single-token argv channel (immune to PowerShell 5.1 quote loss):
$argv = @('grep', '-n', 'he said "hi"', 'notes.md')
$b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes(($argv -join [char]0)))
& tools\contextmink\bin\contextmink-bridge.exe --argv-b64 $b64
```

- It locates Git Bash itself (Git for Windows only; Cygwin/MSYS2 never
  substitute silently — point `CONTEXTMINK_BASH` at another shell explicitly),
  spawns direct commands without MSYS argument rewriting, and takes argv as
  `--argv-b64` or `--argfile` (UTF-8, one argument per line) so PowerShell 5.1
  quoting cannot corrupt arguments. `--print-argv` shows exactly what arrived;
  `--print-root` shows the resolved bridge root.
- Relative paths resolve from `CONTEXTMINK_BRIDGE_ROOT`; otherwise caller-side
  `.contextmink.toml`/`.git` discovery wins before executable-side discovery,
  so personal and project-local bridges both stay anchored to the project they
  serve. In direct mode a program spelled as a path (`./gradlew`) resolves
  against `--cwd` like a POSIX exec, and bare names use the native Windows
  `PATH`; pass `--login` for a utility supplied by Git Bash, such as `perl`.
- Files whose first line begins `#!` enter Git Bash deterministically;
  `--script <path>` explicitly selects a Bash script and resolves it from the
  bridge root. An optional `--` immediately after the script path is consumed
  as the conventional argument separator; double it when the script itself
  must receive a leading `--`.
- Every bridge-owned Git Bash boundary hex-relays startup argv before decoding
  it and installs scoped MSYS conversion exclusions for the caller's
  slash-bearing values, so a quoted `"$@"` forwarded to a native child
  preserves leading-slash selectors, `@file` arguments, and JSON. Do not set
  `MSYS2_ARG_CONV_EXCL` yourself.
- `--preserve-descendants` is the explicit exception for a successful child
  that intentionally launches a persistent GUI or service; ordinary commands
  remain kill-on-close supervised.
- Destructive argv matching the deny-list is refused before spawn;
  `--help` prints the current deny-list and break-glass override. The bridge
  and `capture` share one process-boundary implementation.

The `scripts/contextmink` launcher shields slash-bearing JSON selectors,
predicates, regexes, literal terms, SQL, and shell-command values from MSYS
rewriting on Git Bash. Windows-to-Bash boundaries can also expand wildcard
globs, so prefer `--ext` over `--glob '*.<ext>'` there. When the native
`contextmink` executable is invoked directly from an MSYS shell (`MSYSTEM` set,
`MSYS_NO_PATHCONV` unset, and the parent process image inside the MSYS root's
`usr/bin` or `bin`) and any argument or `--flag=value` value begins with the
MSYS installation root derived from its `<root>\usr\bin` PATH entry, such as
`C:/Program Files/Git/`, it refuses before doing any work and names
`MSYS_NO_PATHCONV=1` as the fix: a rewritten pointer, pattern, or path fragment
would otherwise yield a confident answer about a different value. The bridge
does not apply this check; it serves native callers, and Bash treats a
rewritten path as the same file.

Do not launch a replacement Contextmink build through a running
`contextmink-bridge`: let active bridge commands finish first, which avoids
Windows executable-lock contention.

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
Built-in exclusions for common build and dependency trees (such as `.git`,
`target`, `node_modules`, and `.venv`) apply inside every explicit scan root,
so add only repository-specific high-output paths.
Empty profiles and the template placeholder profile
(`replace-with-workspace-name`) are hard errors. Use
`<command> --config <file>` for an explicit policy or `<command> --no-config`
for built-in defaults only.
Excludes quiet broad scans only: pass an explicit file or subdirectory when an
excluded tree is the target, or `--with-excluded` to lift the globs for one
command. Git ignore rules are separate; `--with-git-ignored` lifts those.
Configured destructive guard fragments are literal case-insensitive substrings
matched by `contextmink-bridge`, `capture`, and `guard-hook` before a child
process or agent shell command is allowed to run.

## Scope

Add to this tool only when the failure mode is generic transcript overflow or
host-shell friction in file enumeration, text search, line slicing, JSON
inspection, read-only SQLite inspection, or bounded capture of unknown
command output. Anything needing domain knowledge, a schema, a compiler, an
indexer, a runtime, or a real parser belongs in the domain tool.

## License

MIT. See [LICENSE](LICENSE). [LICENSE-SSL](LICENSE-SSL) and
[LICENSE-VPL](LICENSE-VPL) accompany every release archive and mirror sync.
