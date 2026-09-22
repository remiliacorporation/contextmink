---
name: papertiger
description: Use before implementing multiple independent changes, resuming recorded work, or preserving decisions and deferred defects. Keep outcomes resumable across sessions; skip small self-contained edits and work owned by another tracker.
---

# Papertiger

Use a durable record when it helps the work survive a handoff. One independently
reviewable outcome gets one task; investigation and verification are steps inside
it. Resume existing records even for a small next step. Record validated deferred
work as it appears, without interrupting authorized progress. Do not mirror an
existing team or domain lifecycle.

## Select the authority once

<!-- installed-command -->

Use `<project-root>/tools/papertiger/bin/papertiger[.exe]` in the native
shell. Keep work in the canonical project that owns the outcome, including
cross-repository work; from elsewhere pass `--project-root <canonical-root>`.
The runtime selects the existing authority; do not guess `--db`, use PATH, or
create a second store in a worktree. If no project runtime exists, use the explicit
executable binding in an installed personal Papertiger skill, when available.
Personal task intent identifies its consuming project.
Below, `papertiger` means this selected executable and authority.
Write through it, never raw SQLite. A refusal does not authorize removing guards
or manufacturing admission; use the corrective command or preserve the blocker.
For existing history, skip setup and the reference. Only for a genuinely new
authority, read [first use](../../../tools/papertiger/agent_integration.md#first-use).

Set `PAPERTIGER_ACTOR` to a concise author label and `PAPERTIGER_SESSION` to one
unique session ID before mutations; reuse them. Global `--actor` and `--session`
work when shells do not retain environment. Set `PAPERTIGER_MODEL` and
`PAPERTIGER_REASONING_EFFORT` only when their actual values are known.

## Enter, work, finish

Use `status` once to orient to an unfamiliar authority. For a known task,
read `show <N> --json` and resume it directly; use `--no-history` for a current-state
recheck. Otherwise select work with `focus --plan <slug> --json` and search for
an existing outcome with `search "<terms>" --plan <slug> --compact --json` before
adding. Follow bounded results' continuations when relevant. Keep focus's
readiness, pickup and real blockers together: another session's pickup is
advisory, not a lock or a reason to seek permission for requested work.

```text
papertiger start <N> --why "Why this work resumes" --json
papertiger add "Outcome" --plan <slug> --start --intent "Standalone purpose" --intent-source user --why "Why now" --json
papertiger done <N> --result "Outcome and verification" --result-source agent --json
```

Search before adding; read rejected history before reviving an approach. Choose
`user`, `agent`, or `external` for the actual source of intent. Write `--why` for
choices a cold reader could question. `--intent-file`, `--why-file`, and
`--result-file` accept UTF-8 text when shell quoting becomes awkward.

Do not pipe JSON receipts through `head` or `tail`. Read commands offer compact
or projection options; mutations have no compact flag. For shorter mutation output,
retain the full receipt and use the [receipt projection](../../../tools/papertiger/agent_integration.md#mutation-receipts).
JSON mutations acknowledge exact committed events. New selectors are in
`events[].task.seq`; retain the receipt, don't replay a successful mutation if
local display/parsing fails. Read-only `show` or `log` resolves uncertainty.

Before completion, associate any representing commit with
`commit add <N> <full-oid> --repo <stable-label>`. Omit it when no commit represents
the outcome. Keep task numbers out of shared Git/PR prose. Probes and decisions
require results; `done` refuses open obligations. Resolve them with evidence,
never route around a refusal. Run `audit` after planning changes.

Use live `--help` for operations. Read [the reference](../../../tools/papertiger/agent_integration.md)
for installation, authority changes, migration or recovery; its relevant section
covers gates, transfers, external references and optional Mise. Only `init`
initializes or migrates a database; never use it to replace missing history.
