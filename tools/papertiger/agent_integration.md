# Papertiger operating contract

Papertiger is optional. Use it for work that crosses sessions or where
dependencies, external blockers, decisions, probes, or proof obligations make
live focus and cold-resume context more useful than a short checklist.

Use that judgment proactively. When authorized development exposes a deferred
defect, external dependency, consequential unresolved decision, proof debt, or
validated tooling friction that should survive the session, record it without
waiting for the operator to say "make a Papertiger task." Do not stop current
in-scope work merely to hand off the new task unless it blocks or changes the
authorized scope. Do not create tasks for speculative observations you have not
reproduced, same-session steps, or status reporting that belongs in a shared
issue system.

## Authority

The default authority is `state/papertiger.sqlite`. Project-local launchers
default the receipt-selected authority to the canonical repository root even
when called from a nested directory. `PAPERTIGER_DB` or an explicit global
`--db` deliberately overrides that launcher default for operational use; do not
accidentally split ordinary planning across multiple authorities.

- Many agents and harnesses may use one canonical SQLite authority in its
  planning worktree. Every connection receives one fixed 500 ms SQLite lock
  grace for brief read or mutation overlap; commands are never replayed, and a
  longer lock produces an explicit retry refusal. Independent authorities are
  never merged or synchronized; Git cannot merge changed database copies.
- Mutate the database only through Papertiger commands and public APIs.
- Ensure the database plus `-journal`, `-wal`, and `-shm` sidecars are ignored
  before `init`. Never replace a missing authority with a fresh one when prior
  work clearly existed.
- `init` is the only creation and migration command. Read commands never
  migrate; follow their exact corrective command deliberately.
- The current authority schema is v6. Before migrating an older authority, use
  its matching release to archive its current export. Older dump files require
  their matching release, a temporary authority migration, and current-format
  re-export before import.
- `export` is transfer and recovery, not a second live authority.

Papertiger owns modeled plans, tasks, dependencies, blockers, gates, and event
history. Domain evidence and issue systems remain authoritative for their own
facts. Markdown carries doctrine and rationale, never duplicated live status.

## Start from live truth

From the repository root, choose the project launcher for the active shell, not
the agent harness:

```bash
scripts/papertiger status
scripts/papertiger focus --json
scripts/papertiger show <task.seq> --json
scripts/papertiger audit
```

```powershell
.\scripts\papertiger.cmd status
.\scripts\papertiger.cmd focus --json
.\scripts\papertiger.cmd show <task.seq> --json
.\scripts\papertiger.cmd audit
```

If more than one plan is active, pass `--plan <slug>` to plan-scoped reads.
`task.seq`, written as `N` or `#N`, is the only task identity and selector.
Prefer bare `N`: it is portable across shells, while `#N` must be quoted where
`#` begins a comment.

`task.seq` is private to one authority. Never write a Papertiger task number in
a shared commit, pull request, changelog, release note, or public artifact. Such
prose must stand alone. When a shared issue or artifact is relevant, record its
stable URL or evidence locator in Papertiger; local planning identity never
flows outward.

## Mutations

Set `PAPERTIGER_ACTOR` to the acting agent name before mutating. Write `--why`
for anything a future session could question, using language that stands alone
without chat context.

For multi-paragraph durable text, use the same `<field>-file <path|->` pattern:
`--intent-file`, `--why-file`, `--result-file`, or `note --text-file`. `-` reads
stdin. One command may consume stdin for only one field; inline and file forms
for the same field are mutually exclusive. Explicit empty intent remains the
way to clear optional orientation; rationale, results, and notes must be
nonblank. File and stdin text must be UTF-8; one leading UTF-8 BOM is accepted.
Windows PowerShell 5.1 uses a legacy encoding for native pipelines by default,
so send non-ASCII text through a UTF-8 file or configure `$OutputEncoding`.

```bash
scripts/papertiger start <task.seq> --why "Why execution starts now"
scripts/papertiger gate close <task.seq> <name> \
  --evidence file:path/to/receipt.json --sha256 <digest>
scripts/papertiger done <task.seq>
```

`show --json` reports event-derived lifecycle timestamps. `started_at` is set
only while the task is currently in progress, and `completed_at` only while it
is currently done; event history retains earlier transitions. Use
`list --sort activity` when recency is the useful orientation; do not interpret
timestamps as duration, productivity, or submission data.

When local archaeology benefits from a reverse link after creating a Git
snapshot, resolve the full commit object ID in the owning repository and record
it locally:

```bash
git rev-parse --verify 'HEAD^{commit}'
scripts/papertiger commit add <task.seq> <full-oid> --repo <repo-label>
scripts/papertiger commit find <full-oid>
```

This association is optional evidence for lookup. Papertiger does not invoke Git,
infer repositories, track branches, scrape commit messages, or treat a commit
as task completion. A commit may be partial or wrong; task results and gates
remain the completion authority.

The repository label defaults to `.` for the project root selected by this
authority. Pass `--repo` only for a nested or external repository, using the
same stable label for add, remove, and find.

Probe and decision tasks require `--result` or `--result-file`. `done` refuses
open dependencies, blockers, gates, or children; close or waive them with
evidence and reasons rather than routing around the refusal. Check
`list --status rejected` before reviving an old approach.

When measured overlap or duplication has one canonical owner in the same plan,
use `retire <old> --into <canonical> --why ...`. `show` remains on the retired
task and renders the replacement; it never redirects silently. Rejection stays
separate and accepts no replacement. A task with inbound replacements can only
be retired into another live canonical task; rejection or bare retirement
refuses rather than leaving a replacement chain that ends in dead work.

Do not create Papertiger tasks for same-session checklist steps or leave tasks
`in_progress` when no agent owns them.

## Project-local installation

`setup-project` owns only these managed files:

- `tools/papertiger/bin/papertiger[.exe]` (host-local and ignored)
- `tools/papertiger/agent_integration.md`
- `tools/papertiger/project-install.json` (tracked version, authority path, and
  managed-text hashes; no platform-binary hash)
- `scripts/papertiger`
- `scripts/papertiger.cmd`
- `.agents/skills/papertiger/SKILL.md`
- `.claude/skills/papertiger/SKILL.md`
- additive Papertiger entries in `.gitignore`

During a reviewed pre-receipt cutover, setup may also remove a recognized
obsolete `tools/papertiger/README.md`; it never creates that path. Later retired
paths require an exact prior receipt hash.

Commit `scripts/papertiger` with executable mode. On a host filesystem that
does not preserve that bit, run `git update-index --chmod=+x
scripts/papertiger` before committing it.

It never edits `AGENTS.md` or `CLAUDE.md`, updates the harness, installs hooks or
an MCP server, touches global configuration, or initializes or migrates
authority. Setup never invokes Git, and `.gitignore` cannot untrack an existing
path; if the host binary or selected authority is tracked, review it and use
`git rm --cached -- <path>` to remove only its index entry while preserving the
local file. On a first cutover, pass
`--authority-path <project-relative path>` when the project does not use
`state/papertiger.sqlite`; later upgrades preserve the receipt value. For an
upgrade, run `setup-project` from the newly verified release binary; the
existing project launcher cannot update itself. Preview with `setup-project
<root> --dry-run --json`.
Receipt-matching upgrades and missing-file repair are automatic;
`--replace-managed` is only for a reviewed pre-receipt cutover or explicit
recovery of a modified current path. Modified retired files always refuse and
must be moved or deleted deliberately. An older release refuses to downgrade a
newer receipt even with `--replace-managed`; use the recorded release or a
newer verified binary.

The receipt hashes the managed text surfaces: both launchers, the canonical
contract, and both skill envelopes. It also owns the host-local binary but does
not put platform-specific binary bytes in that text hash list; each applied
setup upgrades the binary to the exact bytes of the running release. Modified
receipt-hashed text refuses unless the operator explicitly reviews replacement.

The two skill paths are byte-identical thin discovery envelopes around this
canonical contract. `.agents/skills` serves open Agent Skills-compatible
harnesses including Codex, Pi, and OpenCode; `.claude/skills` serves Claude
Code. Pi loads project skills only after project trust; for a noninteractive
run, save that trust or pass `--approve`, otherwise project resources are
ignored. Hermes requires an explicit `skills.external_dirs` entry for the
project's `.agents/skills` directory. Filesystem permissions are its protection
boundary: Hermes skill management may change or delete writable external
skills, which a later receipt-checked setup will report as divergence. A
same-named local Hermes skill takes precedence. Harnesses without Agent Skills
should load a concise pointer from their project guidance. After setup changes
a skill, start a fresh harness session if the active one does not rescan project
skills. Do not fork the semantic body per harness.

## Mise is an episodic external driver

`papertiger-mise` is included in every Papertiger release but is not vendored
by `setup-project`. When a bounded RSI campaign is warranted, invoke the stable
peer binary from the release against the consumer:

```bash
papertiger-mise --project-root <repository> status --json
papertiger-mise --project-root <repository> init
```

The consumer owns `state/papertiger-mise.sqlite`,
`state/papertiger-mise-objects/`, and campaign workspaces. The release binary
is part of the frozen outer judge and must not change during the campaign.
Read `MISE.md` from the same release before campaign admission.

Mise nominations are evidence, never planning completion, integration,
promotion, or deployment authority. Historical and domain-shadow evidence is
permanently decision-ineligible. Projection back into planning is two-key:
derive with `papertiger-mise projection inspect`, then attach with
`papertiger mise project`; the projection cannot close a task or gate.
