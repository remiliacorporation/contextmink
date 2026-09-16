---
name: papertiger
description: Keep project work resumable with the local tasklog. Use before editing work with independently reviewable outcomes or separate commits, when continuing an existing durable task, or when a validated blocker, decision, or deferred defect needs to survive the session. Skip transient checklists and work whose lifecycle already belongs to a domain tool or shared team system.
---

# Use Papertiger

Use the local tasklog as part of doing the work. Act on these triggers without
waiting for the user to name the tool. Ordinary planning needs only this skill
and command help; load the detailed reference for the operations below.

## Decide whether a record helps

Resume an existing task even when the next step is a small edit or a read-only
check. For new work, record independently reviewable outcomes, separate commits,
or consequential decisions, blockers, and deferred defects that need a durable
record. Investigation, editing, and testing are usually intermediate steps within one independently
reviewable outcome, which gets one task. A bounded edit or review with no durable outcome needs
no task. Size, elapsed time, and the number of agents alone are not triggers.

Record validated follow-up when it appears and continue authorized work. Keep
speculation in the investigation until evidence makes it actionable. Do not
duplicate a lifecycle already owned by a domain runtime or shared issue system.

## Enter and resume

Invoke `<project-root>/tools/papertiger/bin/papertiger[.exe]` directly in the
active host shell. Below, `papertiger` means that exact native binary. It selects
the authority through the project receipt; do not guess a database path or use
an unrelated binary on PATH.

Choose the canonical project by the initiative that owns the outcome. Keep
cross-repository work in that authority; editing another repository or loading
its skill does not create another task. From elsewhere, pass global
`--project-root <canonical-project-root>`. An explicit `--db` or `PAPERTIGER_DB`
is an exceptional operator override, not ordinary project selection.

Choose one unique `PAPERTIGER_SESSION` for this agent session and reuse it;
concurrent agents need different values even with the same actor. Use a known
unique worker identity or generate one once; never generate one per command.
If command shells do not retain environment, reuse global `--session <id>`.
This is advisory pickup context, not a lease or proof that an agent is alive.

Use `status` once to orient, then `focus --plan <slug> --json` to choose work.
Check for an existing outcome with
`search "<terms>" --plan <slug> --compact --json`.
Read `focus` directly: it already contains compact task summaries with
`readiness`, `pickup`, and `blockers`. Keep those fields together when selecting
work; projecting only task titles and status discards coordination context.
Pass `--plan` when multiple plans are active. Read the chosen task with
`show <N> --json`; follow bounded results' continuation commands when relevant.
For a current-state recheck, `show <N> --no-history --json` avoids repeated event
payloads; follow its history command when prior notes or decisions matter.
Inspect a rejected task's rationale before reviving its approach. Whole-backlog
alignment uses `plan list --json` and `list --all-plans --status unfinished --json`,
including paused plans; status/focus describe active work rather than that inventory.
Inventory rows are `tasks[].{plan,task}`; task fields are nested under `task`
(for example `tasks[].task.seq`). Use the relevant plan's `focus` to select work.
When choosing work freely, prefer your own pickup or available alternatives to
`picked_up_elsewhere`. The latter is a historical hint, never a blocker. For a
requested task or when no alternative remains, read its context and resume with
`start <N>` directly. Do not wait for release, demand takeover approval, or infer
liveness from actor names or timestamps. Dependencies, gates, and real blockers
still apply; pickup does not guarantee exclusive execution or prevent file conflicts.

## Keep the record aligned with the work

Set `PAPERTIGER_ACTOR` to a concise author label before mutations. Set
`PAPERTIGER_MODEL` and optional `PAPERTIGER_REASONING_EFFORT` from the exact model
variant and configured effort known to this execution context (for example,
`gpt-6-astra` and `high`). Resolve them once from explicit context or readily
available session metadata, then reuse until settings change; no extra model
call or repeated user question is needed. Omit unknown values and never infer
settings from a harness name, prose, or a parent's overridden configuration.
These are event provenance, not ownership or a session lease.
`in_progress` survives an interrupted session;
read its context and record pickup with `start` before continuing. Repeating
`start` in the same identified session is eventless; no release is required.

```text
papertiger add "Outcome" --plan <slug> --start --intent "Standalone purpose" --intent-source user --why "Why this work starts now" --json
papertiger start <N> --why "Why this work resumes" --json
papertiger done <N> --result "What changed and what evidence establishes it" --result-source agent --json
```

Use `user` for directly requested intent, `agent` for agent-discovered follow-up,
and `external` for externally supplied meaning. Use `--why` for decisions a
future reader could question. In scripts, obtain new selectors from the exact
mutation receipt's `events[].task.seq`; do not parse human output. Command help
documents UTF-8 `--intent-file`, `--why-file`, and `--result-file` inputs.
Retain mutation receipts and display only the acknowledgement needed: `changed`,
ordered event IDs, and optional task/plan identities. A receipt's task is a summary,
not full context. After a successful write, a local parsing/display failure calls
for read-only verification, never automatic replay of the mutation.

Record any representing commit's full object ID before task completion with
`commit add <N> <full-oid> --repo <stable-label>` (`.` for this project's root).
Task numbers stay private: never put them in commits, PRs, or release prose.
Probe and decision tasks require a result. `done` refuses open dependencies,
blockers, gates, or children; resolve them with evidence or explicit reasons.
Use `audit` at closeout when planning state changed. Markdown carries rationale
and doctrine, not a copy of live task status.

Mention planner state when it changes the user's understanding of progress,
scope, or a blocker. Routine reads and mutations need no narration or final
inventory. If a summary needs to name an unrequested planner, use "local
tasklog"; preserve `papertiger` in executable commands and exact evidence paths.

## Load detail when needed

Read [the project reference](../../../tools/papertiger/agent_integration.md)
completely before installation, migration, recovery, import/export, or changing
authority selection. For gates and evidence, replacements, plan moves, external
references, or Mise projection, read its relevant section before mutating.
Never write SQLite directly. If an authority is missing where prior work existed,
stop and report it; never initialize a replacement. Only deliberate `init`
initializes or migrates; follow an exact read-command refusal.

For several plausible candidates that need comparison under a fixed evaluator,
consider the episodic Mise driver. A known fix with a decisive regression test
usually needs ordinary engineering. Read the reference's Mise section, then
run the matching external `papertiger-mise guide`; the driver owns its operating
guide. Mise evidence never completes this task or grants integration authority.
