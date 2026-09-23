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

The skill carries the ordinary workflow; read this reference for installation,
migration, recovery, transfer or authority changes, and record validated
deferred defects, dependencies, decisions or proof debt as they appear, never
speculative observations or intermediate steps inside one independently
reviewable outcome.

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
- The current planner authority schema is v13 and the current dump format is
  `papertiger.dump.v10`. To migrate an older authority, first create a
  standalone recovery file with the new release's
  `--db <source> backup --output <new-path>`, then run `init` with the new
  release. `import` also converts `papertiger.dump.v9`; older dumps need their
  matching release, a temporary authority migration, and a current-format
  re-export. Migration never rewrites stored events and leaves earlier
  malformed rows untouched for `audit`. Mise's schema is independent.
- Writes to every planner table require public mutation API entry. A raw
  SQLite writer fails with `papertiger_write_requires_public_api`; use the bound
  executable, never remove its guards, add triggers or manufacture the admission
  function. Guard drift (a missing or altered guard, or any trigger the
  executable did not install) blocks writes, not reads, export or backup;
  `audit` reports it. After stopping the direct access, preview with
  `repair-guards --dry-run` and repair with `repair-guards --why <reason>`,
  which reinstalls the guards, removes foreign triggers, records the observed
  SQL and cannot write planning data. An altered history view blocks reads until
  repaired. This is a misuse boundary, not a sandbox against an unrestricted
  filesystem owner. API callers are trusted after mutation entry.
- Legacy history recovery is explicit: preserve a `backup`, review
  `history inspect <event-id>`, then use `history quarantine <event-id>
  --expect-sha256 <digest> --why <reason>` only for its reported structural
  defects. The immutable original survives, and a recovery event carries every
  raw field through log/export/import. Unknown timestamps and associations stay
  unknown; recovery never infers task state. This is not routine task editing.
- `export` is transfer and recovery, not a second live authority. It carries
  plans, tasks, gates, blockers, events and Mise projections.
  `export --output <path>` writes a canonical UTF-8 recovery file atomically
  and prints a digest/count receipt; replacing an existing file requires
  `--replace`.
- `backup --output <new-path> --json` creates a consistent standalone SQLite
  recovery file, including committed WAL data. It supports planner schemas 1–13
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

Invoke the project executable `<project-root>/tools/papertiger/bin/papertiger[.exe]`
directly, not through `PATH`, a shell script, or Contextmink's process bridge;
project and authority selection belong to that binary. In the examples below,
`papertiger` means that executable.

```bash
papertiger status
papertiger --project-root <canonical-project-root> status
papertiger focus --json
papertiger search "<terms>" --json
papertiger search "<terms>" --compact
papertiger show <task.seq> --json
papertiger show <task.seq> --no-history
papertiger plan list --json
papertiger list --all-plans --status unfinished --json
papertiger audit
papertiger evidence verify --project-root <project-root> --json
papertiger evidence verify --classification failed --task-state open --limit 50 --json
```

If more than one plan is active, pass `--plan <slug>` to plan-scoped reads.
`status --json` distinguishes all in-progress work from its explicit parent
and leaf projections. Every bounded projection reports its scope, ordering,
eligible, returned, and omitted counts; when it is incomplete, follow its
`continuation_command` rather than treating the visible entries as exhaustive.
Use compact search for discovery (5 results by default, with a
`continuation_command` when more match) and full `show` when resuming selected
work. `show --no-history` is a current-state recheck; follow its
`history_command` when earlier notes or decisions matter. `plan list --json`
includes every plan state and accepts `--plan <slug>` for one plan's
orientation. For complete task inventory across active and paused plans, use
`list --all-plans --status unfinished --json` and follow each page's
`continuation_command` (its `--after-cursor` is bound to the filters and the
authority snapshot). A history change invalidates the cursor; restart the read
instead of combining pages from different states.

Planner read commands open the SQLite authority read-only by construction;
they never initialize, migrate, or repair it.
`evidence verify` is also read-only. Its summary always counts the complete
selected task scope; `--classification`, `--task-state`, and `--limit` bound only the
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
`picked_up_elsewhere`; `--include-blocked` also includes blocked proposed work. Real blockers
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

Pass `--model`/`--reasoning-effort` (or `PAPERTIGER_MODEL`/`PAPERTIGER_REASONING_EFFORT`)
only with exact known values; effort requires a model; never guess or backfill.

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

For multi-paragraph text, use the same `<field>-file <path|->` pattern:
`--intent-file`, `--why-file`, `--result-file`, or `note --text-file`. `-` reads
stdin. One command may consume stdin for only one field; inline and file forms
for the same field are mutually exclusive. Explicit empty intent remains the
way to clear optional orientation; rationale, results, and notes must be
nonblank. File and stdin text must be UTF-8; one leading UTF-8 BOM is accepted.
Windows PowerShell 5.1 uses a legacy encoding for native pipelines by default,
so send non-ASCII text through a UTF-8 file or configure `$OutputEncoding`.

```bash
papertiger add "Outcome" --start \
  --intent "Standalone purpose" --intent-source user \
  --why "Why this outcome starts now"
papertiger start <task.seq> --why "Why execution starts now"
papertiger gate resolve <task.seq> <name> \
  --evidence file:path/to/receipt.json --sha256 <digest>
papertiger done <task.seq>
```

Replacing intent that already has a source requires either a replacement
`--intent-source` or `--clear-intent-source`; unchanged text keeps its stored
source. Use `edit <task.seq> --clear-intent-source --why <reason>` to correct a
mistaken attribution without erasing the intent or its revision history.

`show --json` reports event-derived activity. `started_event` exists only while
the task is currently in progress, and `completed_event` only while it is
currently done. Their actor fields identify the transition author, not who
should work next. `last_event` records the latest task, dependency, or gate
event. Use `list --sort activity` when recency is useful; do not interpret
event times as duration, productivity, or submission data.

Task context v8 is the single work-record surface: full selected-task details,
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
instead of silently reading the wrong timeline. Events keep the kind they were
recorded with: gate resolutions recorded before schema v13 read as `closed`.

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
papertiger commit add <task.seq> <full-oid>
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
open dependencies, blockers, gates, or children; resolve or waive them with
evidence and reasons rather than routing around the refusal. Check
`list --status rejected` before reviving an old approach; `reopen` can revisit a
rejected task.

Duplication is a replacement relationship, not another status: for overlapping
work with one canonical task in the same plan, use
`retire <duplicate> --into <canonical> --why <reason>` and state the overlap.
`show` stays on the retired task and renders the replacement. A task with
inbound replacements can only be retired into another live canonical task.
Use `reject` for an approach that should not be pursued; it accepts no
replacement.

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

Blockers record the external `--condition` preventing progress
(`blocker add <N> <name> --condition <text>`) and clear with
`blocker resolve` or `blocker waive`, the same verbs gates use.

## Project-local installation

`setup-project` owns only these files and never edits `AGENTS.md`, `CLAUDE.md`,
harness configuration, hooks or global state, invokes Git, or initializes or
migrates an authority:

- `tools/papertiger/bin/papertiger[.exe]` and its host-local
  `.runtime-install.json` receipt (both ignored)
- `tools/papertiger/agent_integration.md`
- `tools/papertiger/project-install.json` (tracked version, authority path, and
  managed-text hashes)
- selected skill envelopes: `.agents/skills/papertiger/SKILL.md` and/or
  `.claude/skills/papertiger/SKILL.md`
- additive Papertiger entries in `.gitignore`

Upgrade by running the newly verified release binary, never the project-local
one: preview with `setup-project <root> --dry-run --json`, then apply the
command it reports. Receipt-matching upgrades and missing-file repair are
automatic; later upgrades preserve the receipt's authority path and skill
targets. `--replace-managed` is only for a reviewed recovery of a modified
managed path; modified retired files and downgrades always refuse. On a first
install, pass `--authority-path <project-relative path>` when the project does
not use `state/papertiger.sqlite`, and `--skill-target agents|claude|both|none`
to override detection from existing harness markers. `.gitignore` cannot untrack
a path; if the host binary or authority is tracked, review it and use
`git rm --cached -- <path>`. Start a fresh harness session after a skill changes.

`uninstall-project` is the inverse: run it from an external binary matching the
receipt version, preview with `--dry-run`, and review all paths. It removes only
receipt-owned content that still matches, refuses modified content, and leaves
planner and Mise authorities, sidecars, Mise objects, repository guidance and
the `.gitignore` policy in place; data disposal is a separate decision.

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
derive with `papertiger-mise projection export`, then attach with
`papertiger mise record`; the projection cannot resolve a task or gate.
Projections recorded before 0.18 keep their original
`papertiger.mise-planner-projection.v1` id and stay readable and exportable;
new recordings require `papertiger.mise_planner_projection.v2`.
