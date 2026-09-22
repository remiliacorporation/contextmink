# Papertiger project reference

## First use

Release archives already contain `.agents/skills/papertiger`, the complete
Claude discovery copy, and `tools/papertiger/bin`. Copy these directories into
the project root; no setup command or AGENTS.md edit is required. Existing
projects continue with their selected authority and never repeat this section.

For a genuinely new project with no existing planning history, invoke the
bundled executable from that project, set `PAPERTIGER_ACTOR` and a stable
`PAPERTIGER_SESSION`, then run `init` without `--json` (initialization reports
plain text). Keep the same executable and authority selectors used for `status`;
an ordinary project overlay needs no `--db` override. Create a plan with
`plan add <slug> "Title" --intent "Purpose"` only if no suitable plan exists.
The default authority is `state/papertiger.sqlite`; an existing project receipt
retains its configured authority. `init` creates or migrates through the public
API. Never initialize a replacement when history is unexpectedly missing.
Keep the database and its sidecars out of Git using the project's ignore policy.

Archives contain no database, project receipt, root guidance, root README or
project configuration. Merging their contents preserves those files. Release
files under the two namespaced skill directories and `tools/papertiger` are
replaced; preserve deliberate customizations there before replacing them.
`setup-project` and `setup-user` remain optional managed installation paths.

Optional personal installation with `papertiger setup-user` places the native
runtime and skills under the user home and initializes a private fallback store.
It never rewrites consuming projects. Use `setup-project` when a managed
installation receipt is desired. All paths expose the same workflow through skills.

Papertiger is optional. Use it when work has independently reviewable outcomes,
separate commits, dependencies, external blockers, decisions, probes, or proof
obligations that merit durable identity and cold-resume context. This can apply
even when the operator requests all outcomes in one session.

The vendored skill contains the ordinary enter, resume, record, and completion
workflow. Agents do not need this entire reference before an ordinary task
mutation. Read it completely for installation, migration, recovery, transfer,
or changing authority selection; load the relevant section for less common
operations. An existing durable task takes precedence over the bounded-edit or
read-only skip: continue its record without creating a second task.

Use that judgment proactively. When authorized development exposes a deferred
defect, external dependency, consequential unresolved decision, proof debt, or
validated tooling friction that should survive the session, record it without
waiting for the operator to say "make a Papertiger task." Do not stop current
in-scope work merely to hand off the new task unless it blocks or changes the
authorized scope. Do not create tasks for speculative observations you have not
reproduced, intermediate steps inside one independently reviewable outcome, or status
reporting that belongs in a shared issue system.

## Authority

Choose one canonical authority by the initiative or outcome that owns the
work, not by whichever repository contains the next edited file. One outcome
that spans repositories remains in that authority. If its repository changes
are independently reviewable or separately committed, model them as separate
tasks in the same authority and associate their commits with stable external
repository labels. Loading another repository's Papertiger skill or editing
that repository does not require a duplicate local task. Use a second
authority only when the second project owns a genuinely independent durable
outcome; independent authorities cannot express replacements or dependencies
between each other.

The default authority is `state/papertiger.sqlite`. The native binary walks
upward from the current directory to find the nearest
`tools/papertiger/project-install.json`, verifies that its version matches the
running binary, verifies the host-local runtime receipt and installed binary
identity, and resolves its recorded authority against that project root.
When an intentional command runs from another repository, pass the global
`--project-root <canonical-project-root>` option. It requires a receipt at that
exact root and selects the receipt-bound authority without changing the
process working directory. `PAPERTIGER_DB` or an explicit global `--db`
deliberately overrides receipt discovery. The installed personal executable falls back to its private
store only when no project receipt is discovered. It needs no `--db` argument.
Do not use a raw database override for ordinary
project selection or split ordinary planning
across multiple authorities. Ordinary commands refuse combining the receipt
selector with a database override. `evidence verify` retains that combination
only so an explicitly selected database can resolve `file:` locators beneath a
supplied project root.

- Many agents and harnesses may use one canonical SQLite authority in its
  planning worktree. Every connection receives one fixed 500 ms SQLite lock
  grace for brief read or mutation overlap; commands are never replayed, and a
  longer lock produces an explicit retry refusal. Independent authorities are
  never merged or synchronized; Git cannot merge changed database copies.
- Mutate the database only through Papertiger commands and public APIs.
- Ensure the database plus `-journal`, `-wal`, and `-shm` sidecars are ignored
  before `init`. Never replace a missing authority with a fresh one when prior
  work clearly existed.
- `init` is the only command that initializes or migrates the selected live authority. Read commands never
  migrate; follow their exact corrective command deliberately.
- The current planner authority schema is v11. Before migrating an older authority,
  archive its export with the matching release and create a standalone SQLite
  recovery file with the new release's `--db <source> backup --output <new-path>`.
  Older dump files require
  their matching release, a temporary authority migration, and current-format
  re-export before import.
  Current dumps use `papertiger.dump.v9`; schema v10 adds advisory pickup
  context. Existing in-progress tasks retain unknown session identity; migration
  never guesses ownership from actors. Schema v12 requires explicit public
  mutation API entry for writes to every planner table. A raw SQLite writer
  fails with `papertiger_write_requires_executable`; use the bound executable,
  never remove its guards or manufacture the admission function. Normal opens
  refuse altered guards. This is a misuse boundary, not a sandbox against an
  unrestricted filesystem owner. API callers are trusted after mutation entry.
  Migration leaves earlier malformed rows untouched for `audit`. Mise's schema
  is independent.
- Legacy history recovery is explicit: preserve a `backup`, review
  `history inspect <event-id>`, then use `history quarantine <event-id>
  --expect-sha256 <digest> --why <reason>` only for its reported structural
  defects. The immutable original survives, and a recovery event carries every
  raw field through log/export/import. Unknown timestamps and associations stay
  unknown; recovery never infers task state. This is not routine task editing.
- `export` is transfer and recovery, not a second live authority.
  `export --output <path>` writes a canonical UTF-8 recovery file atomically
  and prints a digest/count receipt; replacing an existing file requires
  `--replace`.
- `backup --output <new-path> --json` creates a consistent standalone SQLite
  recovery file, including committed WAL data. It supports planner schemas 1–12
  without migration or new events, refuses foreign authorities and existing
  destinations or sidecars, and reports the output hash and original schema.
  Historical evidence is preserved without semantic validation; this can retain
  legacy records that a matching release's JSON import refuses. Inspect recovery
  copies with the matching release's `--db`; never use one as a second live
  authority. Restoration is a deliberate operator action.
- A task priority stored as text is invalid data, not a named priority level.
  Preserve a `backup` before recovery. An explicit
  `edit <task> --priority <integer> --why <reason>` with no other edits can replace
  it and records a `repair_priority` event with the exact original text and the
  chosen integer. No value is inferred; another unreadable task field rolls the
  operation back. Other malformed storage types require a verified authority.
  This repairs scheduling only; it does not certify the task's earlier provenance.

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
papertiger --project-root <canonical-project-root> status
papertiger focus --json
papertiger search "<terms>" --json
papertiger search "<terms>" --compact --json
papertiger show <task.seq> --json
papertiger show <task.seq> --no-history --json
papertiger plan list --json
papertiger list --all-plans --status unfinished --json
papertiger audit
papertiger evidence verify --project-root <project-root> --json
papertiger evidence verify --outcome failed --task-state open --limit 50 --json
```

If more than one plan is active, pass `--plan <slug>` to plan-scoped reads.
`status --json` distinguishes all in-progress work from its explicit parent
and leaf projections. Every bounded projection reports its scope, ordering,
eligible, returned, and omitted counts; when it is incomplete, follow its
`continuation_command` rather than treating the visible entries as exhaustive.
Use compact search for discovery and full `show` when resuming selected work.
`show --no-history --json` is a current-state recheck; follow its `history_command`
when earlier notes or decisions matter. `plan list --json` includes every plan
state and accepts `--plan <slug>` for one plan's orientation. For complete task
inventory across active and paused plans, use `list --all-plans --status unfinished
--json`. Follow `next_after_seq` with `--after-seq` and the returned `--snapshot`,
keeping filters unchanged. A history change invalidates the snapshot; restart the
read instead of combining pages from different states.

Planner read commands open the SQLite authority read-only by construction;
they never initialize, migrate, or repair it.
`evidence verify` is also read-only. Its summary always counts the complete
selected task scope; `--outcome`, `--task-state`, and `--limit` bound only the
detail projection and never narrow the exit-status claim. Exact status and
unsupported-scheme counts identify resolver gaps in the summary. The default
detail filter is `incomplete`, covering failed and unsupported bindings. Follow the
structured continuation command while `has_more=true`; a cursor is bound to
the root, filters, stored bindings, and live verification results and refuses
after drift. The verifier hashes stored `file:` locators in bounded memory from
one stable byte read beneath the project root, rejects escapes and symlinks,
and fails closed on missing, unhashed, or mismatched bytes. Unsupported locator
schemes are reported, never counted as verified. Failed bindings include exact
corrective argument vectors for their evented reopen-and-rebind workflow.

A `file:` locator plus SHA-256 is the byte receipt for retained evidence, not a
Git snapshot. For a commit-backed outcome, bind an immutable audit receipt as
evidence and record the repository's full commit object ID separately with
`commit add`; neither identity substitutes for the other.
`task.seq`, written as `N` or `#N`, is the only task identity and selector.
Prefer bare `N`: it is portable across shells, while `#N` must be quoted where
`#` begins a comment.

`task.seq` is private to one authority. Never write a Papertiger task number in
a shared commit, pull request, changelog, release note, or public artifact. Such
prose must stand alone. When a shared issue or artifact is relevant, record its
stable URL or evidence locator in Papertiger; local planning identity never
flows outward.

## Concurrent pickup without reservations

Set `PAPERTIGER_SESSION` once to a unique identity for this agent session, or
pass global `--session <id>`; reuse it across commands. Give concurrent agents
different identities even when they use the same model or actor. Do not create
an identity per CLI invocation. Identity accepts 1–128 ASCII letters, digits,
`.`, `_`, `:`, and `-`, starting with a letter or digit. Omission is supported for
human or unidentified callers and records a null session, never a guessed actor.

`focus` prefers `mine`, then unattributed `in_progress`, then `ready`, then
`picked_up_elsewhere`; `--all` also includes blocked proposed work. Real blockers
remain in `blockers` independently of pickup. Work picked up elsewhere remains
visible and eligible: neither its presence nor its age proves another agent is
still working. When choosing freely, prefer other available work; an explicit
user request to resume a task takes precedence. With no alternative, inspect
its stored context and use `start <N>` to continue it directly.

`start` records pickup and can resume an already-in-progress task. An old pickup
never requires release, takeover permission, a reason, a heartbeat, expiry, or a
harness integration. A changed pickup records the previous and new identities;
repeating `start` from the same identified session is eventless (`changed=false`).
Initial `start` and `add --start` bind pickup in their status event and transaction.
Terminal transitions clear current pickup while preserving its history. Notes,
reads, and edits do not refresh pickup or pretend to be a heartbeat.

`task.pickup` is null or `{session, at}` in full task context. `focus` v7
returns compact task summaries with `pickup`, `readiness`, and `blockers` as
sibling fields on each entry; `status` likewise keeps pickup beside its summary.
Read `focus` directly without an additional projection. It omits task intent,
result, history, and plan narrative; use `show <N> --json` for the chosen task.
Its plan contains only `slug` and `status`. If scripting a smaller selection
view, retain readiness, pickup, and blockers together with task identity.
Whole-authority inventory uses `tasks[].{plan,task}`, so identifiers and titles
are `tasks[].task.seq` and `tasks[].task.title`, not fields directly on each row.
`at` is the last pickup time, not last-seen or last-active time. These are advisory
coordination hints, not exclusive claims, authorization, liveness detection, or
file locks. Concurrent explicit starts can both succeed; SQLite serializes their
history, not their subsequent execution. Domain-owned work continues to use its
domain's own coordination mechanism.

## Mutations

Set `PAPERTIGER_ACTOR` to a concise human-readable author label before
mutating. It records who wrote each event; it is historical provenance, never
an assignee, claim, lease, session handle, or liveness signal. Write `--why`
for anything a future session could question, using language that stands alone
without chat context.

When the event author's model is known, pass `--model <model-id>` or set
`PAPERTIGER_MODEL`. This is explicit caller-reported attribution, separate from
the recorder actor and `user|agent|external` meaning source. Use the author's
known model identifier, not a guessed identity based on a harness name or
writing style. Omit unknown attribution; never backfill historical events.
`activity.created_event.model` identifies the recorded creation author;
`activity.completed_event.model` identifies the current completion author.
For other dispositions use `activity.status_event.model`. Neither timestamps
nor model labels prove ownership, productivity, or reviewer quality.

Before the first mutation in an execution context, identify the exact model
variant and configured reasoning effort from explicit context or readily
available session metadata. Supply `--model gpt-6-astra --reasoning-effort high`,
for example, or set `PAPERTIGER_MODEL` and `PAPERTIGER_REASONING_EFFORT` in the
command environment. Reuse known values until the execution configuration
changes; do not make an extra model call or ask the user for every mutation.
The reasoning effort is a separate caller-reported identifier, requires a model,
and is exposed as `reasoning_effort` alongside `model` on events and activity.
Do not substitute a family name when the exact variant is available, infer
effort from prose, or let a child inherit a parent's identity after an override.
When exact settings are unavailable, record only what is known and omit effort.
Papertiger does not inspect private harness logs or contact a model provider.
Existing history remains unchanged; absent effort reads as null, and no database
migration is required.

### Mutation receipts

For scripted mutation chains, pass `--json`. `papertiger.mutation.v1` contains
`changed` and the exact emitted `events`; each entry includes the event and
its task/plan snapshots captured inside the committed transaction. Obtain
created task selectors from `events[].task.seq`. Failed mutations produce no
success receipt, and idempotent operations may emit an empty event list with
`changed=false`. Never recover a task number by parsing human prose. `init`
retains its separate human-readable migration output.

For a concise acknowledgement, retain the receipt locally and display a projection
of `changed` and each ordered event's `event_id`, `kind`, `task`, and `plan`.
The optional `task` is a **summary** (`seq`, `title`, `status`, `kind`, `priority`),
not a full task: it has no `intent` or `result`. Taskless events are legitimate.
For example, after capturing successful JSON stdout in `$receipt` in PowerShell:

```powershell
$receipt | Select-Object schema, changed, @{Name='events'; Expression={
    @($_.events | ForEach-Object {
        [pscustomobject]@{
            event_id = $_.event.event_id
            kind = $_.event.kind
            task = $_.task
            plan = $_.plan
        }
    })
}} | ConvertTo-Json -Depth 8
```

Check the native command's exit code before parsing. If local parsing or display
fails after a successful write, inspect the retained receipt and use read-only
`show`/`log` to verify the outcome; do not replay the mutation. Empty events and
`changed=false` are distinct from a failed command with no success receipt.

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
For a durable outcome requested directly by the user, pass
`--intent-source user`; use `agent` for validated follow-up first identified by
the agent and `external` only for meaning supplied by an external source.
Omitting a genuinely unknown source stores no source on new text. Replacing
intent that already has a source requires either a replacement
`--intent-source` or `--clear-intent-source`; unchanged text keeps its stored
source.
Use `edit <task.seq> --clear-intent-source --why <reason>` to correct a
mistaken attribution without erasing the intent or its revision history.

`show --json` reports event-derived activity. `started_event` exists only while
the task is currently in progress, and `completed_event` only while it is
currently done. Their actor fields identify the transition author, not who
should work next. `last_event` records the latest task, dependency, or gate
event. Use `list --sort activity` when recency is useful; do not interpret
event times as duration, productivity, or submission data.

Task context v7 is the single work-record surface: full selected-task details,
compact related-task summaries, and twelve recent events with a history cursor
when older events remain. Use `show <related-seq> --json` for that task's full
context. `schema` emits the bundled JSON Schema for context, status, task list,
event log, dump, and mutation receipts without opening an authority. Every
field is provider-local; stored prose and locators require editorial review
before external publication. There is no automatic publication renderer or
provider-discovery protocol.

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

When a local Git commit represents a task outcome, resolve the full object ID
in the owning repository and record it inward before task completion:

```bash
git rev-parse --verify 'HEAD^{commit}'
papertiger commit add <task.seq> <full-oid> --repo <repo-label>
papertiger commit find <full-oid>
```

Omit this association only when no commit represents the task outcome. The
association remains lookup evidence, not completion authority. Papertiger does
not invoke Git, infer repositories, track branches, scrape commit messages, or
treat a commit as task completion. A commit may be partial or wrong; task
results and gates remain the completion authority.

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

Duplication is represented by that replacement relationship, not another
status. Use `retire <duplicate> --into <canonical> --why <reason>` and state
the overlap. Use rejection for an approach that should not be pursued.

Rehome work with `move-plan <N>... --plan <destination> --why <reason>`.
Supply every task connected by parent, dependency, or replacement links;
a partial selection refuses with all missing selectors. The destination must
be active. The operation preserves task state and all obligations, and records
before/after plan revisions without rewriting old events. Scoped exports carry
current member tasks, their full histories, and required former-plan definitions.

Use `reference add <N> <locator> --kind <pull_request|issue|review|adr|input|other>`
for inward artifact associations. Locators must be scheme-qualified with no
literal whitespace; percent-encode spaces. Optional `--sha256` and `--note`
retain a claimed byte identity and context. `reference list <N>` and
`reference find <locator>` retrieve them; `reference remove <N> <locator>
--kind <kind> --why <reason>` preserves the removed value in history before
rebinding. References may annotate terminal tasks, never fetch external data,
import status, or satisfy proof. Reference hashes are not part of gate/blocker
`evidence verify`; verify producer inputs independently. The existing validated
Mise projection remains the typed producer boundary.

Do not create Papertiger tasks for intermediate steps inside one independently
reviewable outcome. Create separate tasks when outcomes are independently
reviewable, separately committed, or have distinct decisions or proof—even if
one session is expected to finish them. `in_progress` means work began and
remains unfinished; it deliberately survives a dead or replaced session and
needs no reassignment. A fresh agent reads the task and records its pickup with `start` before continuing.
Add a task note only when handoff context beyond the stored intent, result,
gates, and history is genuinely useful.

Repository boundaries do not change that rule. Keep those separate outcomes
in the initiative's canonical authority unless another project truly owns an
independent lifecycle. Do not mirror one task into every repository it touches.

## Optional repository guidance discovery trigger

Skills-capable harnesses discover the installed skill without a project guidance
edit. For a harness without skill discovery, or explicit project policy, an owner
may add the following optional trigger:

> Before the first edit or commit on multi-outcome or separate-commit work, or
> work matching an existing durable task, read
> `<selected-skill-path>/papertiger/SKILL.md` completely and follow it. Skip one
> new bounded edits or read-only reviews without a durable outcome, intermediate
> steps, and domain-owned or shared-team lifecycle. Resume existing durable work
> even for a small step.

Replace `<selected-skill-path>` with `.agents/skills` or `.claude/skills` only
when that path was deliberately selected. A repository using another harness
can point directly to `tools/papertiger/agent_integration.md` instead.

After installation, inspect the repository-owned discovery surface from the
project root or any nested directory:

```bash
tools/papertiger/bin/papertiger inspect-project-guidance --json
papertiger --project-root /path/to/project inspect-project-guidance --json
```

This command validates the project receipt and host runtime but never opens the
planning authority. It reads only regular, non-symlink repository-root
`AGENTS.md` and `CLAUDE.md`, with a fixed 64 KiB cap per file. The deterministic
result distinguishes an exact selected-skill trigger, a generic
Papertiger-skill trigger, `CLAUDE.md` indirection to `AGENTS.md`, an integration
pointer, a bare mention, stale positive shell-launcher wording, absence, and
bounded refusal. It reports exact byte identity when both files were read.
These are lexical observations, not proof that a harness discovers or follows
the guidance; nested guidance and imported semantics remain outside the result.

When a request does not name Papertiger, describe its use in the final summary
as the local tasklog. Keep `papertiger` in executable corrective commands,
evidence paths, and authority facts where replacing it would reduce precision.

`setup-project` never edits `AGENTS.md`, `CLAUDE.md`, or another repository-owned
context file. The managed skill supplies the ordinary workflow; owners may opt
into this additional trigger, but installation and ordinary use do not require it.

## Project-local installation

`setup-project` owns only these managed files:

- `tools/papertiger/bin/papertiger[.exe]` (host-local and ignored)
- `tools/papertiger/bin/papertiger[.exe].runtime-install.json` (host-local and
  ignored exact path, byte count, and SHA-256)
- `tools/papertiger/agent_integration.md`
- `tools/papertiger/project-install.json` (tracked version, authority path, and
  managed-text hashes; no platform-binary hash)
- zero or more selected skill envelopes:
  `.agents/skills/papertiger/SKILL.md` and
  `.claude/skills/papertiger/SKILL.md`
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

On a first install, the default `auto` selection follows existing harness
markers. `.agents`, `.codex`, `.cursor`, `.pi`, `.omp`, `.opencode`, `AGENTS.md`, or an
OpenCode `opencode.json` / `opencode.jsonc` file selects the shared `agents`
residence; `.claude` or `CLAUDE.md` selects `claude`; both marker families
select `both`; and an unmarked repository selects `none`. These markers only
bootstrap common consumers before `.agents` exists. Explicit `agents`,
`claude`, `both`, or `none` avoids detection; explicit `auto` reruns it. An
upgrade with no `--skill-target` preserves the receipt's selected targets.
Changing targets removes a deselected envelope only when its prior receipt hash
still matches; local edits refuse retirement.

The tracked receipt hashes the managed text surfaces: the canonical contract
and the selected skill envelopes. It does not put platform-specific binary
bytes in that clone-portable hash list. The separate ignored runtime receipt is
written atomically after all other setup verification and records the exact
installed binary path, byte count, and SHA-256. Dry-run JSON exposes the same
`runtime_install` identity. The `papertiger.project_setup.v5` result also embeds
the bounded `project_guidance` observation used by
`inspect-project-guidance`, without managing those repository files. Ordinary
receipt discovery refuses a missing,
malformed, or mismatched host receipt and directs the operator to run
`setup-project` from a trusted external release. This is local identity, not a
claim that independently linked Windows or other platform builds reproduce the
same bytes. Modified receipt-hashed text still refuses unless the operator
explicitly reviews replacement.
During an upgrade, a current tracked receipt plus a valid prior runtime receipt
that exactly matches the installed native binary proves ownership when the
host receipt must change across release-version or contract changes. A
non-identical existing host receipt whose ownership is malformed, mismatched,
legacy, or otherwise unproved is reported in dry-run and requires a reviewed
`--replace-managed`; a missing host receipt can be recreated without claiming
an existing file.

The skill under `.agents/skills` contains the ordinary workflow. `.claude/skills`
receives the identical complete body, generated from the same source template.
Claude selection resolves to both paths; owned legacy routers upgrade in place.
The detailed reference remains conditional. `.agents/skills` serves compatible
harnesses including Codex, Pi, OMP, and OpenCode. Auto detection never creates
`.codex`, `.pi`, `.omp`, or
`.opencode` skill copies. Pi loads project skills only after project trust; for
a noninteractive run, save that trust or pass `--approve`, otherwise project
resources are ignored. Hermes requires an explicit `skills.external_dirs`
entry for the project's `.agents/skills` directory. Filesystem permissions are
its protection boundary: Hermes skill management may change or delete writable
external skills, which a later receipt-checked setup will report as
divergence. A same-named local Hermes skill takes precedence. Harnesses without
Agent Skills should load a concise pointer from their project guidance. After
setup changes a skill, start a fresh harness session if the active one does not
rescan project skills. Do not fork the semantic body per harness.

`uninstall-project` is the inverse receipt-owned lifecycle. Run it from an
external binary matching the receipt version, preview it first, and review all
paths. It removes only matching receipt-owned text, a native binary whose bytes
equal that external release, its exact runtime receipt, and finally the tracked
receipt. It refuses modified content and project-local self-deletion. Planner
and Mise authorities, SQLite sidecars, Mise objects, repository guidance,
unrelated skills, and the entire `.gitignore` policy remain in place; data
disposal is a separate decision.

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

`papertiger-mise guide` supplies the matching driver's concise operating path;
`guide --reference` emits its full bundled `MISE.md`. Start from `status`, then
`campaign inspect <id>` to discover retained candidate, trial, cohort, and
reservation identities without querying SQLite. These are discovery records;
reopen the specific CAS evidence before using a result.

Mise nominations are evidence, never planning completion, integration,
promotion, or deployment authority. Historical and domain-shadow evidence is
permanently decision-ineligible. Projection back into planning is two-key:
derive with `papertiger-mise projection inspect`, then attach with
`papertiger mise project`; the projection cannot close a task or gate.
