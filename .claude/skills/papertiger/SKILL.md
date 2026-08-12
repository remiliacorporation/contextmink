---
name: papertiger
description: Use the project-local Papertiger CLI to organize or resume engineering work whose decisions, dependencies, blockers, evidence, or deferred follow-up must survive a session. Use proactively for multi-session research or implementation and validated follow-up; skip same-session checklists and shared team status.
---

# Use Papertiger

Use Papertiger as the project's ordinary local planner. Do not announce a
named "Papertiger discipline" or create ceremony around it; mention planner
state only when it helps the work or the user.

Read `../../../tools/papertiger/agent_integration.md` completely before first
use in a project; it is the concise command and authority reference.

Use the project-local launcher for the active shell (`scripts/papertiger` in
Bash, `scripts\papertiger.cmd` in Command Prompt, or
`.\scripts\papertiger.cmd` in PowerShell). From a nested directory, address the
repository's launcher through a valid relative or absolute path; once invoked,
it derives the root from its own location. Treat `task.seq` as a private
selector: never put a Papertiger task number in a shared commit, pull request,
changelog, or release note. Record an optional full commit object ID inside
Papertiger when that local reverse lookup will help future archaeology.

When implementation reveals durable follow-up or validated tooling friction,
record it without waiting for the user to say "make a task". Continue the
authorized in-scope work unless the finding blocks it or changes its scope.
