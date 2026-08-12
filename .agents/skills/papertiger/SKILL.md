---
name: papertiger
description: Use the project-local Papertiger planner to start, organize, orient to, resume, or close engineering work that should survive the current session, including research campaigns, deferred defects, external blockers, consequential decisions or probes, dependencies, proof obligations, and validated tooling friction. Do not use it for a same-session checklist or shared team status tracking.
---

# Use Papertiger

Read `../../../tools/papertiger/agent_integration.md` completely and follow its
live-authority, judgment, mutation, and closeout contract.

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
