# Authority binding remediation — 2026-08-24

This record replaces three legacy Papertiger gate locators from Contextmink's
original standalone-repository cutover. The underlying outcome remains valid;
the old locators did not satisfy Papertiger 0.9.0's project-local file evidence
contract. The checks below were rerun before rebinding the gates to this file.

## Standalone repository continuity

- `F:\AI\contextmink` remains the standalone Git authority with origin
  `https://github.com/remiliacorporation/contextmink.git`.
- The originally associated standalone commit
  `174980effe67416725010b08877900b0deb869b3` resolves to a commit object.
- The release candidate product commit is
  `f21d65ab8a154b0a6a4fbace94c36bce73824f8a`.
- The Papertiger authority is schema v8, `papertiger audit` reports no findings,
  and the Papertiger 0.9.0 integration dry-run reports `operation: unchanged`.
- `AGENTS.md` and `CLAUDE.md` have identical SHA-256 hashes. The only untracked
  source-repository path is the operator-owned `.claude/settings.json`; no
  release-managed file is modified.

## Retained vendor snapshot

The downstream workspace receipt at
`F:\AI\wow_modernclient\tools\contextmink\vendor_source.json` names standalone
commit `0044934b5d69bf31e1a71baad61607853362a9f0`. That object resolves in the
standalone repository.

To avoid comparing the downstream snapshot with a moving branch, the recorded
commit was checked out into a detached local clone whose origin was set to the
canonical repository URL. From `F:\AI\wow_modernclient`, this command passed:

```text
scripts/sync_contextmink.sh --check F:/AI/contextmink-vendor-verify-20260824
Contextmink vendor matches committed source 0044934b5d69bf31e1a71baad61607853362a9f0
```

The unrelated untracked downstream file `docs/x64dbg_mcp_server_review.md` was
not modified.

## Vendored runtime smoke

The exact retained vendor snapshot passed these current checks from
`F:\AI\wow_modernclient`:

- `cargo build --locked --release --manifest-path tools/contextmink/Cargo.toml`
- the workspace `scripts/contextmink` launcher returned a
  `contextmink.receipt.v2` for a bounded read under
  `ghidramink/tools/ghidramink-core/src`, with `scope_complete: true`; output was
  intentionally capped at one path and truthfully reported `output_truncated:
  true`
- `scripts/ghidramink-indexer --help` executed through the vendored
  `contextmink-bridge.exe` and returned the complete command surface
- the retained binaries reported `contextmink 0.9.0-rc.1` and
  `contextmink-bridge 0.9.0-rc.1`, matching the recorded pre-release snapshot

These checks prove the original relocation outcome without claiming that the
retained downstream snapshot is the current standalone release candidate.

## Current release-candidate verification

Separately, the standalone
`f21d65ab8a154b0a6a4fbace94c36bce73824f8a` candidate passed the release verifier:
404 tests, formatting, Clippy with warnings denied, clean 85-file source
packaging and extracted rebuild, Rust 1.95 MSRV checking, Linux Zig builds, and
classified macOS cross-target probes. Native macOS CI remains the link/runtime
authority because the Windows host has no Apple SDK.

All three remediated gates bind to the SHA-256 of this tracked file. That makes
authority-wide verification deterministic under one project root while
preserving the original commit associations as historical repository identity.
