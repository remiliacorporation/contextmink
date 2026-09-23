# Setting Up contextmink in a Repository

## Add to a project (default)

Download the archive for the machine where the agent runs, verify its checksum,
and merge its contents into the project root, including the dot-directories:

```text
.agents/skills/contextmink/SKILL.md
.claude/skills/contextmink/SKILL.md
tools/contextmink/bin/contextmink[.exe]
tools/contextmink/README.md
```

The skills and native executable are already in place. Start a fresh agent
session; no install command, PATH change, AGENTS.md edit, or integration-guide
reading is needed for ordinary work. Claude receives the same complete short
skill body generated from the canonical template. Codex, Pi, Cursor, OMP and
OpenCode can use the shared Agent Skills directory; model selection still
remains discretionary.

Only namespaced skills and `tools/contextmink` are shipped. Existing project guidance,
configuration, receipts and databases are not included or overwritten. Preserve
any customizations inside those tool-owned directories before replacing them.
README, licenses, optional operating references and the source manifest live
under `tools/contextmink`. Retrieval uses the project configuration when present and built-in defaults otherwise.

## Optional personal installation

From a verified extracted release, run:

```sh
# macOS/Linux, inside the extracted release
./tools/contextmink/bin/contextmink setup-user --dry-run
./tools/contextmink/bin/contextmink setup-user
```

```powershell
# Windows PowerShell, inside the extracted release
.\tools\contextmink\bin\contextmink.exe setup-user --dry-run
.\tools\contextmink\bin\contextmink.exe setup-user
```

This installs the retrieval skill in `~/.agents/skills/contextmink`, an identical generated skill
in `~/.claude/skills/contextmink`, and a native runtime plus detailed reference under
`~/.local/share/contextmink` (the same home-relative layout on Windows). The skill
binds the exact executable; no PATH, shell profile, AGENTS.md, CLAUDE.md, hooks,
or consuming-project files are changed. `--home <existing-directory>` selects
an explicit user home, including disposable test homes. Start a fresh agent
session and verify the skill appears. Skill descriptions support automatic
selection; they do not guarantee a model will choose the tool on every request.
The installer writes both complete skill files itself and executes the copied
runtime before reporting success; agents copy nothing.

Both skill paths share one semantic body. Codex, Pi and Cursor can discover the
shared Agent Skills location; Claude reads its generated copy. Other harnesses may need
an explicit skill-directory setting. A synced skill does not install a native
runtime in a remote/cloud environment: install there separately.

A host-local `user-install.json` records the tool version, the installed skill
and reference paths, and raw byte hashes of the installed executables.
Ownership is by path: setup writes the release's files at every owned path,
whatever they currently contain, and `uninstall-user` removes every owned path
without comparing content. The hashes exist because the installed runtime
checks them before each run: it refuses a missing receipt, a missing file, or an
executable that differs from its receipt. Run repair or upgrade from an external
release, not the installed executable. Installation preflights
all managed paths. On Windows, setup and removal also check existing executables
that need replacement or removal for write access before changing any files,
including during `--dry-run`. Close running tool processes when this check refuses.
Unchanged executables need no lock check. A process can acquire a lock after
preflight; this is not a crash-atomic multi-file transaction. An interrupted
install must be repaired before the runtime can run.

`uninstall-user --dry-run` previews removal. `uninstall-user` removes the
receipt's skill, reference and executable paths without comparing their
content, and retains the lifecycle receipt;
it never removes project installations or unrelated skills. Do not copy personal
receipts between machines or move their home: install for the new home instead.

Ordinary retrieval runs from the consuming project's cwd, with its local
configuration when present and built-in defaults otherwise. Personal setup
installs the native retrieval/capture executable. On Windows it also installs
`contextmink-bridge.exe` and a separate `contextmink-bridge` skill for running
project Bash scripts. Linux and macOS installations omit that skill. Native
commands run directly from any shell.

Use `setup-project` below only for explicit shared repository adoption, pinned
project runtimes, or repository-owned policy. Existing project receipt choices
remain intact. No Contextmink trigger text belongs in project guidance; skill descriptions route selection.


This guide is for adding `contextmink` to an existing repository.

`contextmink` is a transcript guard. Use it before broad file, text, line-slice,
JSON, read-only SQLite, or unknown-size command-output reads when the output
cardinality is unknown, when a known file must be navigated beyond one bounded
window, or when host-shell quoting would become the task. It is not a
replacement for project-native tools.

## Prerequisites

- For standalone use, unpack the release archive into the project root; the
  executable is `tools/contextmink/bin/contextmink[.exe]`.
- Rust 1.95 or newer and Cargo are needed only for source builds or vendored
  integrations that build the local `tools/contextmink` copy. `contextmink`
  uses Rust edition 2024.
- A POSIX-compatible shell is needed only for the optional `scripts/contextmink`
  launcher. On Windows, Git Bash works. Without Bash, call the release binary
  directly or use `cargo run --manifest-path tools/contextmink/Cargo.toml --bin contextmink -- ...`.
  Choose invocation by the active shell:

  | Active shell | Command form |
  | --- | --- |
  | Bash-hosted session (macOS, Linux, Git Bash, WSL, Claude Code) | `scripts/contextmink ...` |
  | Windows PowerShell, direct contextmink command | `& tools\contextmink\bin\contextmink.exe ...` |
  | Windows PowerShell, Bash launcher path | `& tools\contextmink\bin\contextmink-bridge.exe --script scripts/contextmink ...` |

  Do not rely on Windows to open the extensionless `scripts/contextmink` path
  directly from PowerShell. `contextmink-bridge.exe` is the preferred
  PowerShell-to-Git-Bash bridge when a Windows-native session needs the Bash
  launcher or another Bash-first repository script. Direct `contextmink.exe
  capture` recognizes files whose first line begins `#!`; pass `--script` for
  a no-shebang Bash script.

## Release Archives

Release archives are published at
<https://github.com/remiliacorporation/contextmink/releases>. Download the
archive for the host platform:

- `contextmink-<version>-windows-x86_64.zip`
- `contextmink-<version>-macos-x86_64.tar.gz`
- `contextmink-<version>-macos-arm64.tar.gz`
- `contextmink-<version>-linux-x86_64.tar.gz`

Each archive is a project overlay. Every file ships once, at the path it is
used from, so merging the archive into a project root needs no further copying:

```text
.agents/skills/contextmink/SKILL.md
.agents/skills/contextmink/agents/openai.yaml
.claude/skills/contextmink/SKILL.md
tools/contextmink/bin/contextmink(.exe)
tools/contextmink/agent_integration.md
tools/contextmink/manifest.json
tools/contextmink/README.md
tools/contextmink/CHANGELOG.md
tools/contextmink/docs/setup.md
tools/contextmink/LICENSE
tools/contextmink/LICENSE-SSL
tools/contextmink/LICENSE-VPL
```

The Windows archive also carries `tools/contextmink/bin/contextmink-bridge.exe`
and `contextmink-bridge` skills in `.agents` and `.claude` (see the bridge
section below); `manifest.json` records its path in a `bridge_binary` field.
`setup-project` and `setup-user` carry their templates inside the executable.

Verify the adjacent `.sha256` checksum after downloading an archive.

```bash
sha256sum -c contextmink-<version>-<platform>.<archive-ext>.sha256
```

On PowerShell:

```powershell
$archive = "contextmink-<version>-windows-x86_64.zip"
$expected = ((Get-Content "$archive.sha256" -Raw).Trim() -split '\s+')[0]
$actual = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLowerInvariant()
if ($actual -ne $expected) { throw "contextmink archive checksum mismatch" }
```

## Standalone Binary Install

This installs `contextmink` on `PATH` instead of vendoring it per repository:

1. Unpack the release archive.
2. Put `tools/contextmink/bin/contextmink(.exe)` on `PATH`, or run it from the
   unpacked directory.
3. Verify:

   ```bash
   contextmink files . --show-files 20
   ```

The binary can use a repository-local `.contextmink.toml`; it searches upward
from the current directory.

On Windows, direct `contextmink.exe` runs built-in commands, native executables,
shebang scripts, and explicit `capture --script` Bash scripts. Use Project
Binary Integration when the repository wants a stable local launcher and
policy-bearing tool layout.

## Project Binary Integration

This gives a target repository a local `scripts/contextmink` entrypoint without
a source build. The maintaining agent runs the setup command; setup does not
presume the repository already has contextmink-aware guidance.

### Integration decisions

Adapt the installation to the project before copying generic policy:

1. Choose the workspace root agents will operate from. Put configuration and
   always-loaded guidance there; nested repositories can inherit that contract
   when they are components rather than independent entrypoints.
2. Inspect the shells agents actually receive and select one canonical command
   form for each host. Do not infer the shell from the agent product name.
3. Inventory project-native compact, projection, query, and diagnostic commands.
   Keep those authoritative and use contextmink around unknown-size generic
   reads rather than replacing domain tooling.
4. Identify project-specific high-output trees, nested repository boundaries,
   generated outputs, and irrecoverable paths. Put scan exclusions and literal
   deletion guard fragments in `.contextmink.toml`; do not copy another
   project's policy. Decide whether ordinary broad scans should cross nested
   repositories or use explicit roots/`--skip-nested-repos`.
5. Decide how fresh clones receive the executable. Ignored release binaries are
   workstation-local and require an install step. A tracked
   `.contextmink.toml`, launcher, and integration reference are intentionally
   compatible with rerunning `setup-project` to restore missing host binaries;
   source vendoring or a reviewed multi-platform package policy is the
   hermetic alternative.
6. Choose the integration depth deliberately. `setup-project` is the
   deterministic agent integration, but skill residence is project-selected.
   Its default `--skill-target auto` detects existing shared Agent Skills and
   Codex markers (`.agents`, `.codex`, `.cursor`, or `AGENTS.md`), Pi's `.pi` directory,
   OMP's `.omp` directory, OpenCode's `.opencode` directory or `opencode.json` /
   `opencode.jsonc`, and Claude markers (`.claude` or `CLAUDE.md`) only on first
   install. Compatibility is path-based rather than a closed harness allowlist:
   any harness that consumes project `.agents/skills` uses `agents`; the named
   markers only bootstrap common consumers before `.agents` exists. Claude receives
   the same complete short skill body. `claude` requests and Claude-only
   detection resolve to `both` so shared discovery remains available. An unmarked repository selects
   `none`. The concrete result is receipt-frozen so
   later upgrades do not react to incidental harness files. Use
   `--skill-target agents|claude|both|none` for an explicit first selection or
   reselection. The short description is discoverable; the full body is loaded
   on selection. Pi requires project trust before loading project-local
   resources; save the decision or pass `--approve` for a noninteractive run.
   Setup never edits harness settings or guidance, and never installs
   general-purpose skills owned by another tool or workflow.
   It manages only `.agents/skills/contextmink` and
   `.claude/skills/contextmink`; Pi, OMP, and OpenCode markers do not create
   additional harness-native copies. Both installed bodies are
   generated from one template; maintain no independent harness-specific
   procedure.
7. Dogfood the result on real project work from the workspace root and a nested
   directory. Verify config/profile discovery, receipts, domain-tool precedence,
   launcher behavior, and any hook or bridge boundary the project enables.

### Install the project-local tools

1. Unpack the release archive next to, or outside, the target repository.

2. Preflight, then apply, from the unpacked directory:

   ```bash
   ./tools/contextmink/bin/contextmink setup-project /path/to/repository --dry-run
   ./tools/contextmink/bin/contextmink setup-project /path/to/repository
   ```

   Windows PowerShell:

   ```powershell
   & .\tools\contextmink\bin\contextmink.exe setup-project C:\path\to\repository --dry-run
   & .\tools\contextmink\bin\contextmink.exe setup-project C:\path\to\repository
   ```

   The command preflights every managed destination before the first write. It
   installs the current platform binary, the Windows bridge when applicable,
   both project launchers, a real-profile
   `.contextmink.toml`, `.gitignore` coverage for
   `/tools/contextmink/bin/`, `tools/contextmink/agent_integration.md`, and the
   Contextmink skill under the resolved discovery path or paths. It also
   writes `tools/contextmink/project-install.json`, a platform-neutral receipt
   containing the Contextmink version, the resolved skill target, and exact
   ownership of the additive `.gitignore` block or file it created. The skill
   target determines which launcher, skill, and reference paths the receipt
   owns. The ignored host-local `tools/contextmink/bin/runtime-install.json`
   (`contextmink.runtime_install.v2`) records the binary paths this checkout
   owns, without hashes, so a checkout shared between Windows and WSL keeps
   the other platform's binary. Setup reads a v1 runtime receipt once and
   rewrites it; an older or unknown one is refused until it is moved aside and
   setup-project reruns. Neither receipt claims
   `.contextmink.toml`, `AGENTS.md`, `CLAUDE.md`, harness settings, or unrelated
   skills.

3. Read every printed `next_actions` entry. Inspect and edit
   `.contextmink.toml` for this repository's generated/high-output trees and
   literal deletion tripwires. The config becomes repository-owned at creation:
   later setup runs validate and report `preserve_repository_owned` without
   comparing it to the release template or replacing it. Invalid configuration
   fails before any setup write.

4. Start a fresh agent session and verify that Contextmink appears in its skills.
   Skills-capable harnesses need no AGENTS.md/CLAUDE.md edit. Project guidance is
   an optional fallback for harnesses without skills, or explicit local policy.
   A binary-only `none` selection does not provide skill discovery.

5. Verify from the target repository root and a representative nested working
   directory:

   ```bash
   scripts/contextmink --json files . --show-files 1
   scripts/contextmink --json guard-check -- git clean
   ```

   The first command must report `schema: "contextmink.receipt.v2"` and the
   intended profile. The second must report `decision: "deny"`.

6. For a fresh clone, rerun an unpacked release with `--dry-run`, inspect the
   plan, then apply it. Existing tracked configuration is preserved while
   missing ignored binaries are created.

7. To upgrade, rerun the newer release with `--dry-run`. A valid existing
   configuration must be reported as `preserve_repository_owned`.
   `auto` preserves the receipt's resolved skill target. Pass an explicit
   `--skill-target` to reselect; the deselected skill files are removed. An
   unreceipted file at a deselected Contextmink skill path reports
   `unowned_refusal` and blocks every setup write until the operator moves or
   removes it deliberately. Launchers, skills, the integration reference, and
   the host binary are written as the release ships them and reported as
   `replace` when their content differs. Setup never replaces
   `.contextmink.toml`. Keep project-specific guidance in repository-owned
   files, not in the release-managed skill or reference.

8. Optional: generate a Claude hook fragment from the installed binary:

   ```bash
   scripts/contextmink guard-hook-snippet
   ```

   The generated fragment registers `guard-hook` for `Bash` and `PowerShell`
   PreToolUse hooks. It uses single `command` strings, not a separate `args`
   field, and emits shell-safe absolute paths for each matcher. Machine-specific
   output belongs in `.claude/settings.local.json`; commit it to shared
   `.claude/settings.json` only when the generated command paths are stable for
   every supported clone.

### Remove the project-local integration

Run removal from an unpacked matching or newer release outside the target
project. The project-local binary cannot remove itself consistently on Windows,
so `uninstall-project` refuses that invocation on every platform.

```bash
./tools/contextmink/bin/contextmink uninstall-project /path/to/repository --dry-run
./tools/contextmink/bin/contextmink uninstall-project /path/to/repository
```

Windows PowerShell:

```powershell
& .\tools\contextmink\bin\contextmink.exe uninstall-project C:\path\to\repository --dry-run
& .\tools\contextmink\bin\contextmink.exe uninstall-project C:\path\to\repository
```

The command requires `tools/contextmink/project-install.json`. It removes the
skills, launchers, and integration reference that the receipt's skill target
implies, every host binary path that
`tools/contextmink/bin/runtime-install.json` owns (without comparing content),
and an exact receipt-owned Contextmink `.gitignore` block, and prunes only
empty Contextmink-owned directories. It refuses a symlink or non-file at an
owned path before any deletion and reports unreceipted runtime files as
`preserve_unowned`. It
preserves
`.contextmink.toml`, `AGENTS.md`, `CLAUDE.md`, harness settings, and unrelated
skills; review those repository-owned files and remove any obsolete discovery
trigger or policy deliberately.

Maintaining-agent prompt:

```text
Set up contextmink in <target-repo> from the unpacked release at <path>. Inspect
the intended workspace root, existing agent-guidance hierarchy, active shells,
project-native bounded/query commands, nested repositories, high-output trees,
and irrecoverable paths. Run `contextmink setup-project <target-repo>
--dry-run`, review its complete action plan, then apply it. Verify the installed skill is discoverable in a fresh agent session. Do not
rewrite project guidance as part of installation. Configure repository-specific excludes and guard
fragments. State whether broad scans may cross nested repositories or should
use exact roots/`--skip-nested-repos`. Verify the v2 receipt, nested-repository
disclosure/skip behavior, and unconditional git-clean denial from the workspace
root and a nested directory. Do not call the integration complete until the
installed launcher, repository-owned profile, guidance, fresh-clone binary
repair, and active-shell invocation all work end to end.
```

## Optional: Claude PreToolUse Hook Guard

`guard-hook` is the same command-aware destructive-command evaluator used by
`contextmink-bridge` and `capture`, exposed as a Claude PreToolUse hook.
It preserves quoting and command boundaries, resolves Git's actual subcommand,
binds protected-path rules to deletion operands, and parses Bash and PowerShell
escaping according to the matcher that invoked it. It reads Claude's hook
payload JSON on stdin and exits 2 only for a recognized destructive command.
Any failure before or during guard evaluation also exits 2: a policy that
cannot be loaded, a failed personal-install self-check, a rewritten argument,
or an internal error. While the personal install is broken, the hook blocks
every command, harmless ones included, because it cannot vouch for any; rerun
`setup-user` from a verified release to restore it. This needs an executable
that starts: a missing or unloadable binary exits with the shell's own status,
which the harness does not treat as a block, so remove the hook registration
before `uninstall-user`. An
unparseable hook-event payload still allows with a diagnostic so harness schema
drift does not disable all shell use. The evaluator is a tripwire, not a shell
interpreter: literal POSIX assignments such as `c=clean; git $c` are resolved,
but computed expansion, sourced scripts, repository-configured Git aliases, and
runtime `eval` remain outside its threat model.

Generate the settings fragment instead of hand-writing it:

```bash
scripts/contextmink guard-hook-snippet
```

The generated hook is bound to the directory containing the selected
`.contextmink.toml` via `--expected-root`. If settings are copied from another
checkout or the repository moves, a hook payload whose `cwd` is outside that
root is allowed with a diagnostic note instead of applying the foreign
repository's policy. Regenerate the snippet in the target repository to restore
protection.

Because the generated fragment contains canonical absolute paths, store it in
`.claude/settings.local.json` for workstation-local release installations.
Shared `.claude/settings.json` is appropriate only when the selected binary and
config paths are stable across every supported checkout.

For source-vendored or custom layouts, pass explicit paths:

```bash
scripts/contextmink guard-hook-snippet \
  --binary F:/repo/tools/contextmink/target/release/contextmink.exe \
  --guard-config F:/repo/.contextmink.toml
```

On Windows, Claude `Bash` hooks are shell command strings. Do not put raw
backslash paths in that string: `F:\repo\tools\contextmink.exe` is parsed by
Bash as escape sequences and collapses before execution. The generated snippet
normalizes Windows paths to `F:/repo/...` and quotes paths with spaces. Every
matcher's hook, including `PowerShell`, is a POSIX command string because Claude
launches hooks through its POSIX hook runner; the matcher only selects the
`--shell` dialect used to parse the intercepted command. A non-default
`--command-field` pointer is emitted with a scoped `MSYS_NO_PATHCONV=1` prefix so
Git Bash does not rewrite it. Prefer the generated
single-string `command` form unless the host's hook schema has been verified to
support an `args` field.

## Optional: PowerShell -> Git Bash Bridge (Windows + Codex-style hosts)

The contextmink binary needs none of this — it runs natively from any shell.
This section applies only to repositories that keep their scripts Bash-first
while the agent runs in PowerShell. POSIX hosts need no bridge.

Windows project overlays and setup install a separate `contextmink-bridge`
skill for this workflow; personal setup also binds its absolute executable path.
Non-Windows installations omit bridge skill discovery. Existing owned personal
installations upgrade by rerunning `setup-user` from the complete new release.
The companion executable is required before Windows setup writes any files.

**Native binary (preferred on Windows).** The Windows release archive carries
`contextmink-bridge.exe`. It locates Git Bash itself (no hardcoded path on the
agent side), spawns direct commands natively with zero MSYS argument
rewriting, and accepts argv through channels PowerShell cannot mangle:

```powershell
# Direct command; slash-bearing args arrive verbatim:
& tools\contextmink\bin\contextmink-bridge.exe -- <program> <args...>
# Repository bash script, Git Bash discovered automatically; the separator is optional:
& tools\contextmink\bin\contextmink-bridge.exe --script scripts/some_tool.sh -- <args...>
# Lossless single-token argv channel (immune to PowerShell 5.1 quote loss):
$argv = @('grep', '-n', 'he said "hi"', 'notes.md')
$b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes(($argv -join [char]0)))
& tools\contextmink\bin\contextmink-bridge.exe --argv-b64 $b64
```

`--print-argv` shows exactly what survived the PowerShell boundary;
`--argfile <file>` (UTF-8, one argument per line) is the file-based alternative;
`--cwd` and `--login` share the same process-boundary policy. Relative paths
resolve from `CONTEXTMINK_BRIDGE_ROOT`; otherwise caller-side
`.contextmink.toml`/`.git` discovery wins before executable-side discovery.
This supports both globally installed and project-local bridges while keeping
a vendored contextmink checkout anchored to the project it serves. In direct
mode a path-like program (`./gradlew`, `bin/tool`) resolves against `--cwd`,
matching POSIX exec semantics. A file whose first line begins `#!` enters Git
Bash deterministically; a Bash script without a shebang requires
explicit `--script`, whose path resolves from the bridge root instead of
`--cwd`. One optional `--` immediately after the script path is consumed as an
argument separator; use two when the script itself must receive one. Bare names
in direct mode use the native Windows `PATH`; use
`--login` for utilities supplied by Git Bash (for example,
`contextmink-bridge.exe --login -- perl --version`) instead of hardcoding a
Git installation path. Destructive argv matching the safety
deny-list is refused before spawn; `contextmink-bridge --help` prints the
current deny-list and break-glass override.

Both explicit `--script` and deterministic shebang execution hex-encode argv
across Git Bash startup, decode it with a constant builtin-only relay, and install scoped
MSYS conversion exclusions for the caller's slash-bearing values. A repository
script that forwards with quoted `"$@"` therefore passes `/type/path`,
`@request.json`, and inline JSON to native Windows children byte-for-byte while
its own generated POSIX paths still receive normal MSYS conversion. Do not set
`MSYS2_ARG_CONV_EXCL` at the caller; the native bridge owns that boundary
setting. (`--print-argv` exits before spawning and diagnoses only the
PowerShell-to-bridge boundary; the installed regression probe also covers the
Bash-to-native-child hop.)

## Source Vendored Integration

Use this pattern only when the target repository should carry and build its own
copy of the Rust crate:

1. Copy this repository's Rust crate into the target repository at
   `tools/contextmink/`.

2. Copy `tools/contextmink/templates/scripts/contextmink` to
   `scripts/contextmink`.

   Preserve the executable bit on Unix-like systems:

   ```bash
   chmod +x scripts/contextmink
   ```

   The launcher uses `tools/contextmink/target/release/contextmink(.exe)` when
   it builds from source. It passes that tool's target directory explicitly to
   Cargo, including when the crate belongs to a parent workspace or the shell
   sets `CARGO_TARGET_DIR`. The workspace still owns its lockfile and profiles.
   For release binary installs, use Project Binary
   Integration instead.

3. Copy `tools/contextmink/templates/.contextmink.toml` to
   `.contextmink.toml`, then edit it.

   Keep only repo-local high-output paths. Good candidates include generated build
   directories, vendored dependencies, caches, exported reports, large binary
   asset trees, and tool output directories. These excludes keep broad scans
   quiet; callers can still pass an explicit file or subdirectory inside an
   excluded tree when that tree is the target.

4. Select `agents`, `claude`, `both`, or `none` for the target repository; do
   not project both harness paths merely because both templates exist. For an
   auto-like first selection, use the same existing-marker rules as
   `setup-project`, then freeze the concrete choice in the repository's vendor
   lock or manifest rather than redetecting it on every refresh. For `agents`,
   copy `tools/contextmink/templates/skills/contextmink/SKILL.md` to
   `.agents/skills/contextmink/SKILL.md` and copy `agents/openai.yaml` under that
   skill. For `claude` or `both`, also copy the same complete template to
   `.claude/skills/contextmink/SKILL.md`. Both paths must contain the same body.
   In a source-vendored integration these
   copies are owned by the target repository rather than a binary-install
   receipt: refresh them from the vendored templates, and remove a deselected
   copy when the selection changes.

5. Copy `tools/contextmink/templates/agent_integration.md` to
   `tools/contextmink/agent_integration.md`, the integration reference the
   skill links to. Do not add Contextmink trigger text to `AGENTS.md`,
   `CLAUDE.md`, or equivalent files; the skill description routes selection.
   Record only genuinely repository-owned decisions there, such as
   domain-tool precedence, nested-repository policy, or protected paths, and
   preserve existing shell, path, and output rules.

6. Verify the integration from the target repository root:

   ```bash
   scripts/contextmink files . --show-files 20
   scripts/contextmink grep --pattern contextmink . --show-files 5
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
contextmink files . --show-files 20
```

## Config Template

`setup-project` creates this file with a real profile. Add only
repository-specific policy, for example:

```toml
profile = "repo-name"

exclude_globs = [
  "generated/reports/**",
]

# Optional spawn safety for repository-owned critical paths:
# destructive_guard_recursive_delete_fragments = ["protected_cache"]
# destructive_guard_delete_fragments = ["critical.sqlite"]
```

The binary already excludes common high-output paths such as `.git`, `target`,
`node_modules`, and `.venv`. Empty profiles and the template placeholder
(`replace-with-workspace-name`) are hard errors.

## Instruction Rule

Use the installed Contextmink skill as the discoverable operational envelope
and `tools/contextmink/agent_integration.md` as its detailed integration
reference. Both harness families read the same reference, sourced from the
single `templates/agent_integration.md`.

The reference states invocation for every shell: the native executable runs
directly from any shell (PowerShell uses `&`), `scripts/contextmink` exists
only in `setup-project` or source-vendored installs, and Git Bash callers
prefix `MSYS_NO_PATHCONV=1` when an argument starts with `/`.

Setup installs the canonical Contextmink skill under `.agents/skills`
for compatible Agent Skills consumers. Claude Code discovers the same complete
short body under `.claude/skills`, generated from the same source template. Codex-facing `agents/openai.yaml` metadata exists only under
the shared Agent Skills directory. Do not fork the
Contextmink semantic body by harness. The skill description supplies the
selection boundary; project guidance carries no Contextmink trigger.
Ordinary retrieval uses the skill and command help; the detailed integration
reference is loaded for setup, policy changes, or unfamiliar receipt semantics.

## Operational Notes

Usage policy lives in the skill and `tools/contextmink/agent_integration.md`; flag
details live in `contextmink <command> --help`. This section covers only host
mechanics those do not:

- Windows-to-Bash boundaries can expand wildcard globs before contextmink
  receives them; that is why the templates steer toward `--ext` over
  `--glob '*.<ext>'` there.
- The `scripts/contextmink` launcher shields slash-leading JSON Pointer
  selectors and slash-bearing `--pattern` / `--prefix` / `--contains` /
  `files --path-contains` / `grep-terms --term` values from MSYS path rewriting
  on Git Bash, while leaving normal file paths to the shell.
- Bridge and `capture` share deterministic script classification. Files whose
  first line begins `#!` enter the Bash boundary before spawn; use
  `capture --script -- <script> ...` for a no-shebang Bash script. Receipts
  disclose `execution_mode` and the always-present `effective_argv`.
- Keep ordinary repository-specific scan policy and protected deletion
  fragments in `.contextmink.toml` and repository instructions.

## Maintenance

For release-binary integration, keep
`tools/contextmink/project-install.json` tracked beside the managed launchers,
skills, and integration reference. Do not hand-edit it. Run the newer
release's `setup-project --dry-run` to inspect an upgrade or reselection.

For a vendored copy, compare or sync only the generic surface:

```text
tools/contextmink/src/
tools/contextmink/tests/
tools/contextmink/examples/
tools/contextmink/Cargo.toml
tools/contextmink/Cargo.lock
tools/contextmink/rust-toolchain.toml
tools/contextmink/README.md
tools/contextmink/CHANGELOG.md
tools/contextmink/docs/
tools/contextmink/scripts/
tools/contextmink/templates/
tools/contextmink/.github/
tools/contextmink/.gitattributes
tools/contextmink/.gitignore
tools/contextmink/LICENSE
tools/contextmink/LICENSE-SSL
tools/contextmink/LICENSE-VPL
scripts/contextmink
```

Do not sync a target repository's `.contextmink.toml`; that file is local
policy.
