---
name: papertiger
description: Load this skill before editing whenever a request contains two or more independently reviewable outcomes, requires separate commits, matches an existing durable task, or reveals validated durable follow-up. Use the project-local Papertiger CLI to record or resume that work. Skip one bounded file edit, read-only review, intermediate steps within one independently reviewable outcome, and domain-owned or shared team lifecycle.
---

# Use Papertiger

Use Papertiger as the project's ordinary local planner. Do not announce a
named "Papertiger discipline" or create ceremony around it; mention planner
state only when it helps the work or the user.

Resolve the release-managed native binary at
`<project-root>/tools/papertiger/bin/papertiger[.exe]` for the current host and
invoke that binary directly for every command. Do not route Papertiger through
a shell script, another shell, or a process bridge. The binary walks upward
from the current directory to find `tools/papertiger/project-install.json`,
verifies its release version, and binds its recorded authority. An explicit
`--db` or `PAPERTIGER_DB` is an exceptional operator override, not ordinary
project selection.

Select the authority by the initiative or outcome that owns the work, not by
the repository containing the next edited file. One cross-repository outcome
stays in one canonical authority; separately reviewable repository changes can
be separate tasks there. Loading another repository's skill or editing its
files never requires a duplicate task in that repository's unsynchronized
authority. From outside the canonical project, pass global
`--project-root <canonical-project-root>` so the exact receipt selects the
authority; use stable external `--repo` labels for commit associations.

Start with `status`, then use `focus --plan <slug> --json` when more than one
plan is active. Use `show <task.seq> --json`, `search "<terms>" --json`, and
`audit` as needed. These reads never initialize, migrate, or replace a missing
or older planning authority and open SQLite read-only by construction; follow
an exact refusal deliberately. Report the planner evidence or changed
conclusion, not skill discovery, contract loading, binary-path resolution, or
other routine read mechanics. Reserve "campaign" for Papertiger Mise; the
ordinary planner uses a planning authority.

When the request does not name Papertiger, call it the local tasklog in the
final summary. Keep `papertiger` in executable corrective commands, evidence
paths, and authority facts where replacing it would reduce precision.

Before the first mutation, initialization, migration, recovery, commit
association, or Mise use in a project, read
`../../../tools/papertiger/agent_integration.md` completely and follow its
authority contract. Treat `task.seq` as a private selector: never put a
Papertiger task number in a shared commit, pull request, changelog, or release
note.

For a durable outcome requested directly by the user, pass
`--intent-source user`; use `agent` for validated follow-up first identified by
the agent and `external` only for meaning supplied by an external source. When
a local commit represents a task outcome, resolve its full object ID and run
`papertiger commit add <task.seq> <full-oid> --repo .` before task completion.
Omit the association only when no commit represents that outcome.

When implementation reveals durable follow-up or validated tooling friction,
record it without waiting for the user to say "make a task". Continue the
authorized in-scope work unless the finding blocks it or changes its scope.
