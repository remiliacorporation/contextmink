---
name: changelog-writing
description: Write or review source-faithful, human-facing changelogs and release notes. Use when adding or revising a CHANGELOG entry, preparing a release description, converting an implementation diff into upgrade guidance, or adversarially reviewing release prose. Do not use for commit messages, internal task logs, session handoffs, or reference documentation.
---

# Changelog Writing

Write a durable release record that helps a human understand what changed,
decide whether to upgrade, and act safely. Preserve the repository's existing
format unless that format prevents those outcomes.

## Establish the release boundary

1. Read the current changelog and its last released entry.
2. Identify the exact previous-release and candidate revisions. Compare those
   revisions directly; do not infer the release from task titles or memory.
3. Inspect the public documentation, tests, and interfaces touched by that
   diff. Use internal plans and commit subjects only as navigation.
4. Separate candidate changes from unreleased follow-up. If the boundary is
   uncertain, resolve it before writing.

Treat current behavior and verified compatibility as authority. Never turn an
intention, issue claim, or test plan into a shipped capability.

## Select human-relevant changes

Include a change when it affects at least one of these:

- a capability, command, output, or workflow a user can observe;
- upgrade compatibility, migration, configuration, or required operator action;
- data integrity, recovery, security, privacy, or refusal behavior;
- a deprecation, removal, changed default, or changed support boundary;
- documentation or installation behavior that changes successful product use.

Usually exclude:

- internal refactors, file moves, helper names, and test-only changes;
- task identifiers, session chronology, authorship, and review process;
- commit-by-commit transcription;
- test counts, hashes, and evidence paths that users do not need to operate or
  verify the release;
- promotional claims, subjective adjectives, and benefits not established by
  the implementation.

Do not omit a compatibility or migration fact merely to keep the entry short.
Compress supporting process before compressing an operational boundary.

## Write for the release reader

Use the existing version and date convention. Add only non-empty semantic
sections such as `Added`, `Changed`, `Fixed`, `Deprecated`, `Removed`, and
`Security`.

For each bullet:

1. Lead with the observable outcome, not the implementation work.
2. Name exact public commands, fields, schemas, files, or states in code style.
3. State affected users or required action when it is not obvious.
4. State compatibility and failure behavior at the point where it matters.
5. Keep one coherent change per bullet; merge several implementation steps
   when they produce one user-visible result.

Prefer concrete verbs and plain language. Avoid "we", release ceremony,
superlatives, vague improvement claims, and repeated "now" phrasing. A short
lead paragraph is useful only when several bullets share one release-level
outcome.

Examples:

- Weak: "Refactored evidence handling and added extensive tests."
- Strong: "`evidence verify` refuses missing or changed `file:` bindings and
  reports the exact reopen-and-rebind command."
- Weak: "Improved intuitive adoption of the planner."
- Strong: "Repository guidance names the multi-outcome cases that trigger the
  Papertiger skill and the bounded cases that skip it."
- Weak: "Made imports safer."
- Strong: "Import validates the complete dump before replacing an authority
  and refuses references to records outside that dump."

## Adversarial review

Before accepting the entry, test it from a release reader's perspective:

- **Source trace:** Can every claim be traced to the candidate diff and current
  behavior?
- **Upgrade decision:** Can a reader identify the material reasons to upgrade?
- **Compatibility:** Are migrations, removals, changed defaults, and required
  actions explicit?
- **Naming:** Are public identifiers exact and internal labels absent?
- **Category:** Does each bullet describe what changed rather than how the team
  built or verified it?
- **Deletion:** Can any sentence be removed without losing user knowledge or
  required action? If yes, remove it.
- **Blind read:** Does the entry stand alone without task history, commit logs,
  or release-session context?
- **Overcompression:** Did concise wording erase a refusal, data-safety, or
  support boundary?

If an existing entry already passes these checks, report `No material change`
instead of rewriting it for stylistic novelty.

## Verify the finished artifact

Update comparison links when the repository maintains them. Run its changelog
or release-note renderer, if present, and inspect the rendered section for
semantic line boundaries, empty headings, duplicate claims, and leakage from
other versions. Re-read the final entry against the exact release diff.
