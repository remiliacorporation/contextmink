# Contextmink — Agent Instructions

Small Rust CLI plus a Windows process bridge for bounded, machine-readable
repository inspection and command capture. This checkout is the standalone
Contextmink source authority; downstream workspaces consume reviewed committed
snapshots or release-managed binaries.

## How To Use This File

This file is the always-loaded repository contract. Load operational detail
only when needed:

- **Product and command behavior**: `README.md` and `contextmink <command> --help`.
- **Project integration and release-managed files**: `docs/setup.md`.
- **Human changelogs and release notes**: `.agents/skills/changelog-writing/SKILL.md`
  is the repository-local reviewed copy of Papertiger's canonical skill; it is
  not a Contextmink setup-managed capability.
- **Capability triggers**: the dedicated section below; each trigger names its
  own operational authority.

Do not grow this file with flag inventories, receipt schemas, or setup recipes.
Those belong to the owning help, documentation, or skill. Keep `AGENTS.md` and
`CLAUDE.md` byte-identical and each under 32 KiB.

## Hard Rules

- Never run `git clean` in this repository. `state/` and release binaries are
  ignored; sweeping ignored paths can destroy the Papertiger authority and
  local dogfood artifacts. Delete only exact reviewed paths.
- Mutate `state/papertiger.sqlite` only through the receipt-selected Papertiger
  binary. If it is missing where prior work clearly existed, stop instead of
  initializing replacement state. `papertiger init` is the only migration path.
- Set `PAPERTIGER_ACTOR` to a concise provenance label before planner mutations.
  Task numbers are authority-local and never belong in commits, changelogs,
  release notes, or pull requests.
- Preserve unrelated worktree changes. Contextmink is the source authority;
  vendor only a verified committed source revision into another repository.
- Fail closed. A missing input, invalid policy, incomplete evidence scope, child
  failure, or unsupported command shape must remain explicit; never turn it
  into an optimistic default or silent skip.

## Product Contract

- Contextmink bounds transcript payload without hiding evidence limits. Keep
  inspected scope, displayed output, exact totals, lower bounds, caps, and child
  exit truth independently represented.
- Contextmink complements project-native tools. Compilers, language servers,
  domain query tools, debuggers, and repository diagnostics retain authority for
  their domains; Contextmink handles uncertain-cardinality generic retrieval and
  capture around them.
- Public commands, flags, receipt fields, and diagnostics use one concrete name
  per concept. Renames are complete cutovers without compatibility aliases.
- Every refusal names the violated boundary and a safe corrective action when
  one exists. Destructive guard and process-supervision refusal paths are
  product behavior and require tests.
- Setup is project-generic and harness-generic. Release-managed capability files
  may be installed consistently, while `.contextmink.toml`, always-loaded
  guidance, shell choice, nested-repository policy, domain-tool precedence, and
  destructive path fragments remain repository-owned adaptation decisions.
- Keep ordinary use low-ceremony: use Contextmink before a read may be broad or
  noisy; use direct commands when output is already known to be small and
  structurally bounded.

## Source and Template Synchronization

- `templates/AGENTS.contextmink.md` and
  `templates/CLAUDE.contextmink.md` are equivalent integration references.
- `scripts/contextmink` and `templates/scripts/contextmink` stay byte-identical.
- Give each setup-managed skill one canonical template and keep installed
  harness copies byte-identical. Keep the Contextmink skill a thin discovery
  envelope around the canonical integration reference.
- When setup-managed surfaces change, update setup preflight, idempotence,
  replacement, release-package, and extracted-install tests in the same change.
- The Windows bridge and `capture` share process-boundary and destructive-guard
  behavior. Update both consumers or prove why a boundary applies to only one.

## Coding Contract

- Use the toolchain pinned by `rust-toolchain.toml` and preserve the declared
  MSRV in release verification.
- Warnings are defects. Keep errors contextual and deterministic; avoid ignored
  results except explicitly justified best-effort cleanup.
- Prefer cohesive owned modules over monolith growth, but do not split code only
  to move lines. Hoist shared invariants rather than duplicating helpers.
- Tests assert public semantics and failure boundaries, not implementation
  history. A product claim in docs requires executable proof.
- Use Contextmink itself for broad repository reconnaissance and for real
  downstream dogfood after changing retrieval, setup, bridge, capture, guard,
  or receipt behavior.

## Capability Triggers

Before broad or potentially high-output file, text, structured-data, or
command-output reads, load the project Contextmink skill. Skip known-small
direct reads and project-native compact or domain-query commands.

Before the first edit or commit on multi-outcome or separate-commit work, or
work matching an existing durable task, read
`.agents/skills/papertiger/SKILL.md` for shared agent harnesses or
`.claude/skills/papertiger/SKILL.md` for Claude Code completely and follow it.
Skip one bounded edit, read-only review, intermediate steps inside one
independently reviewable outcome, and domain-owned or shared-team lifecycle.

## Verification

Run the source gate before claiming completion. It isolates source verification
and `cargo package` into separate target directories:

```text
scripts/verify_source.sh
# Windows native harness:
target/release/contextmink-bridge.exe --script scripts/verify_source.sh
tools/papertiger/bin/papertiger[.exe] audit
```

For cross-platform or release work, also run `scripts/cross_check.sh` and the
release workflow/static checks documented in `docs/setup.md`. Before artifact
handoff, use `scripts/verify_release.sh` (through `contextmink-bridge --script`
on Windows) as the combined pinned actionlint, source, and cross-target gate.
