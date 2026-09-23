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
create a second store in a worktree. Without a project runtime, use the
executable named by your personal Papertiger skill and put the consuming
project's absolute root in each task intent. Below, `papertiger` means the
selected executable. Write through it, never raw SQLite. A refusal does not
authorize removing guards or manufacturing admission; use the corrective command
or preserve the blocker. Only for a genuinely new authority, read
[first use](../../../tools/papertiger/agent_integration.md#first-use).

Before mutating, set `PAPERTIGER_ACTOR` to a concise author label and
`PAPERTIGER_SESSION` to one unique session ID, and reuse them (`--actor` and
`--session` work when the shell drops environment). Set `PAPERTIGER_MODEL` and
`PAPERTIGER_REASONING_EFFORT` only to known values.

## Enter, work, finish

Orient to an unfamiliar authority with `status` once. For a known task, read
`show <N> --json` and resume it; `--no-history` rechecks current state.
Otherwise select work with `focus --plan <slug> --json`, and before adding,
search with `search "<terms>" --plan <slug> --compact`; follow continuations
when relevant. Another session's pickup is advisory: not a lock, and no reason
to ask permission for requested work.

```text
papertiger start <N> --why "Why this work resumes" --json
papertiger add "Outcome" --plan <slug> --start --intent "Standalone purpose" --intent-source user --why "Why now" --json
papertiger done <N> --result "Outcome and verification" --result-source agent --json
```

Read rejected history before reviving an approach. Choose `user`, `agent`, or
`external` for the actual source of intent. Write `--why` for choices a cold
reader could question. `--intent-file`, `--why-file`, and `--result-file` read
UTF-8 text when shell quoting becomes awkward.

Don't truncate JSON with head/tail; use `--compact`, `--no-history` or
`--limit`. New task numbers are in `events[].task.seq`. If output handling
fails after a successful mutation, check with `show`/`log`; never replay it.

Before completion, record any commit that represents the outcome with
`commit add <N> <full-oid>` (add `--repo <label>` only for a nested or external
repository). Keep task numbers out of shared Git/PR prose. Probes and decisions
require results; `done` refuses open obligations. Resolve them with evidence,
never route around a refusal. Run `audit` after planning changes.

Use live `--help` for operations. Read [the reference](../../../tools/papertiger/agent_integration.md)
for installation, authority changes, migration or recovery; its relevant section
covers gates, transfers, external references and optional Mise. Only `init`
initializes or migrates a database; never use it to replace missing history.
