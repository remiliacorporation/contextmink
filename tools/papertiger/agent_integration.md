# Papertiger project reference

Papertiger is optional. Use it when work has independently reviewable outcomes,
separate commits, dependencies, external blockers, decisions, probes, or proof
obligations that merit durable identity and cold-resume context. This can apply
even when the operator requests all outcomes in one session.

Use that judgment proactively. When authorized development exposes a deferred
defect, external dependency, consequential unresolved decision, proof debt, or
validated tooling friction that should survive the session, record it without
waiting for the operator to say "make a Papertiger task." Do not stop current
in-scope work merely to hand off the new task unless it blocks or changes the
authorized scope. Do not create tasks for speculative observations you have not
reproduced, disposable same-session steps inside one durable outcome, or status
reporting that belongs in a shared issue system.

## Authority

The default authority is `state/papertiger.sqlite`. The native binary walks
upward from the current directory to find the nearest
`tools/papertiger/project-install.json`, verifies that its version matches the
running binary, and resolves its recorded authority against that project root.
`PAPERTIGER_DB` or an explicit global `--db` deliberately overrides receipt
discovery for operational use; do not accidentally split ordinary planning
across multiple authorities.

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
- The current authority schema is v7. Before migrating an older authority, use
  its matching release to archive its current export. Older dump files require
  their matching release, a temporary authority migration, and current-format
  re-export before import.
- `export` is transfer and recovery, not a second live authority.
  `export --output <path>` writes a canonical UTF-8 recovery file atomically
  and prints a digest/count receipt; replacing an existing file requires
  `--replace`.

Papertiger owns modeled plans, tasks, dependencies, blockers, gates, and event
history. Domain evidence and issue systems remain authoritative for their own
facts. Markdown carries doctrine and rationale, never duplicated live status.

## Start from live truth

Invoke the release-managed native binary directly. In the examples below,
`papertiger` means the resolved binary at
`<project-root>/tools/papertiger/bin/papertiger[.exe]`; an installation on
`PATH` may use the same binary name. Do not route it through a shell script or
Contextmink's process bridge. Project and authority selection belong to the
Papertiger binary.

```bash
papertiger status
papertiger focus --json
papertiger search "<terms>" --json
papertiger show <task.seq> --json
papertiger audit
```

If more than one plan is active, pass `--plan <slug>` to plan-scoped reads.
`status --json` distinguishes all in-progress work from its explicit parent
and leaf projections. Every bounded projection reports its scope, ordering,
eligible, returned, and omitted counts; when it is incomplete, follow its
`continuation_command` rather than treating the visible entries as exhaustive.
Planner read commands open the SQLite authority read-only by construction;
they never initialize, migrate, or repair it.
`task.seq`, written as `N` or `#N`, is the only task identity and selector.
Prefer bare `N`: it is portable across shells, while `#N` must be quoted where
`#` begins a comment.

`task.seq` is private to one authority. Never write a Papertiger task number in
a shared commit, pull request, changelog, release note, or public artifact. Such
prose must stand alone. When a shared issue or artifact is relevant, record its
stable URL or evidence locator in Papertiger; local planning identity never
flows outward.

## Mutations

Set `PAPERTIGER_ACTOR` to a concise human-readable author label before
mutating. It records who wrote each event; it is historical provenance, never
an assignee, claim, lease, session handle, or liveness signal. Write `--why`
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
papertiger add "Durable outcome" --start \
  --intent "Standalone purpose" --intent-source user \
  --why "Why this outcome starts now"
papertiger start <task.seq> --why "Why execution starts now"
papertiger gate close <task.seq> <name> \
  --evidence file:path/to/receipt.json --sha256 <digest>
papertiger done <task.seq>
```

`add --start` creates the task and enters `in_progress` in one transaction. It
requires a standalone rationale and rolls back the task and both lifecycle
events if readiness validation fails. `--intent-source`, `--result-source`, and
`note --source` accept `user`, `agent`, or `external`; they describe who
supplied stored meaning, independently of the recording `PAPERTIGER_ACTOR`.
Omitting a source preserves explicit unspecified or legacy state.
Use `edit <task.seq> --clear-intent-source --why <reason>` to correct a
mistaken attribution without erasing the intent or its revision history.

`show --json` reports event-derived activity. `started_event` exists only while
the task is currently in progress, and `completed_event` only while it is
currently done. Their actor fields identify the transition author, not who
should work next. `last_event` records the latest task, dependency, or gate
event. Use `list --sort activity` when recency is useful; do not interpret
event times as duration, productivity, or submission data.

`log --json` returns full event identity and an `event-v1` cursor bound to the
exact history prefix. Use `--after-cursor` for new events and
`--before-cursor` for older pages. A cursor from divergent history refuses
instead of silently reading the wrong timeline.

New task `edit` events carry a
`papertiger.task_definition_revision.v1` payload with canonical before/after
values for every changed field. Their public
`task_definition_revision_state` is `complete`. Historical edit events that
predate snapshots remain readable as `legacy_without_snapshots`; never infer
their missing prior values. Pure no-op edits refuse rather than minting a
misleading revision.

`search` analyzes literal words across title, intent, result, tags, and event
rationale, requires every term somewhere in the task record, and ranks exact
phrases plus high-value fields deterministically. It searches done, retired,
and rejected history by default. Use `--plan`, `--status`, or `--limit` to
narrow it; there is no query language, external index, or semantic inference.

When local archaeology benefits from a reverse link after creating a Git
snapshot, resolve the full commit object ID in the owning repository and record
it locally:

```bash
git rev-parse --verify 'HEAD^{commit}'
papertiger commit add <task.seq> <full-oid> --repo <repo-label>
papertiger commit find <full-oid>
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

When measured overlap or duplication has one canonical task in the same plan,
use `retire <old> --into <canonical> --why ...`. `show` remains on the retired
task and renders the replacement; it never redirects silently. Rejection stays
separate and accepts no replacement. A task with inbound replacements can only
be retired into another live canonical task; rejection or bare retirement
refuses rather than leaving a replacement chain that ends in dead work.

Do not create Papertiger tasks for disposable same-session checklist steps
inside one durable outcome. Create separate tasks when outcomes are independently
reviewable, separately committed, or have distinct decisions or proof—even if
one session is expected to finish them. `in_progress` means work began and
remains unfinished; it deliberately survives a dead or replaced session and
needs no reassignment. A fresh agent reads the task and continues it directly.
Add a task note only when handoff context beyond the stored intent, result,
gates, and history is genuinely useful.

## Repository guidance discovery trip-wire

After reviewing this contract, keep one concise trigger in the repository-owned
agent guidance; a bare link or generic "planning" label is too easy for some
harnesses to ignore. Use this wording or an equivalent with the same boundaries:

> Before editing multi-outcome or separate-commit work, or work matching an
> existing durable task, load the project Papertiger skill. Skip one bounded
> edit, read-only review, disposable same-session steps, and domain-owned or
> shared-team lifecycle.

`setup-project` never edits `AGENTS.md`, `CLAUDE.md`, or another repository-owned
context file. The project owner must review and place this trip-wire; the managed
skill remains the canonical command and authority contract.

## Project-local installation

`setup-project` owns only these managed files:

- `tools/papertiger/bin/papertiger[.exe]` (host-local and ignored)
- `tools/papertiger/agent_integration.md`
- `tools/papertiger/project-install.json` (tracked version, authority path, and
  managed-text hashes; no platform-binary hash)
- `.agents/skills/papertiger/SKILL.md`
- `.claude/skills/papertiger/SKILL.md`
- additive Papertiger entries in `.gitignore`

During a pre-receipt cutover, setup recognizes the prior vendor README only as
a predecessor receipt whose recorded SHA-256 values exactly match the old
direct binary, agent contract, and Mise contract. It may then replace the
contract and remove `tools/papertiger/README.md`,
`tools/papertiger/papertiger.exe`, and `tools/papertiger/MISE.md`. A changed
bundle, unrecognized README, or full source tree refuses even with
`--replace-managed`. Later retired paths require an exact prior receipt hash.

It never edits `AGENTS.md` or `CLAUDE.md`, updates the harness, installs hooks or
an MCP server, touches global configuration, or initializes or migrates
authority. Setup never invokes Git, and `.gitignore` cannot untrack an existing
path; if the host binary or selected authority is tracked, review it and use
`git rm --cached -- <path>` to remove only its index entry while preserving the
local file. On a first cutover, pass
`--authority-path <project-relative path>` when the project does not use
`state/papertiger.sqlite`; later upgrades preserve the receipt value. For an
upgrade, run `setup-project` from the newly verified release binary; a
project-local binary cannot overwrite itself while running on Windows. Preview with `setup-project
<root> --dry-run --json`.
Receipt-matching upgrades and missing-file repair are automatic;
`--replace-managed` is only for a reviewed pre-receipt cutover or explicit
recovery of a modified current path. Modified retired files always refuse and
must be moved or deleted deliberately. An older release refuses to downgrade a
newer receipt even with `--replace-managed`; use the recorded release or a
newer verified binary.

The receipt hashes the managed text surfaces: the canonical contract and both
skill envelopes. It also owns the host-local binary but does
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
