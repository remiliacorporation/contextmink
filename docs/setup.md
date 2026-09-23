# Building, vendoring, and releasing from source

This document covers work in a Contextmink source checkout. Installing and
using a release, including `setup-project`, `setup-user`, and the guard hook,
is described in [README.md](../README.md).

## Build from source

The toolchain is pinned by `rust-toolchain.toml`; the MSRV is Rust 1.95
(edition 2024). SQLite is bundled, so no system SQLite is needed.

```bash
cargo build --release          # target/release/contextmink[.exe]
cargo install --path .         # or install it on PATH
```

On Windows, do not launch a replacement build through a running
`contextmink-bridge`. Let active bridge commands finish, then run
`cargo build --release` from a standalone checkout, `scripts/contextmink` from
a parent repository, or `cargo build --release --manifest-path
tools/contextmink/Cargo.toml` from that parent repository.

## Source vendored integration

Use this pattern only when a repository should carry and build its own copy of
the crate instead of a release overlay:

1. Copy this repository's Rust crate into the target repository at
   `tools/contextmink/`.

2. Copy `tools/contextmink/templates/scripts/contextmink` to
   `scripts/contextmink` and keep it executable (`chmod +x scripts/contextmink`).
   From a vendored crate the launcher builds and runs
   `tools/contextmink/target/release/contextmink[.exe]`. It passes that tool's
   target directory explicitly to Cargo, including when the crate belongs to a
   parent workspace or the shell sets `CARGO_TARGET_DIR`; the workspace still
   owns its lockfile and profiles. Build output goes to stderr so stdout stays
   parseable.

3. Copy `tools/contextmink/templates/.contextmink.toml` to `.contextmink.toml`
   and replace its placeholder profile. Add only repository-specific high-output
   paths and deletion tripwires (see Configuration in the README).

4. Select `agents`, `claude`, `both`, or `none` for the target repository with
   the same marker rules as `setup-project`, then freeze the concrete choice in
   the repository's vendor lock or manifest rather than redetecting it on every
   refresh. For `agents`, copy the retrieval skill
   `tools/contextmink/templates/skills/contextmink/SKILL.md` to
   `.agents/skills/contextmink/SKILL.md`, with `agents/openai.yaml` under that
   skill. For `claude` or `both`, also copy the same template to
   `.claude/skills/contextmink/SKILL.md`. Both paths carry the same body. These
   copies are owned by the target repository rather than a receipt: refresh
   them from the vendored templates, and remove a deselected copy when the
   selection changes.

5. Copy `tools/contextmink/templates/agent_integration.md` to
   `tools/contextmink/agent_integration.md`, the reference the skill links to.
   Do not add Contextmink trigger text to `AGENTS.md`, `CLAUDE.md`, or
   equivalent files; the skill description routes selection. Record only
   repository-owned decisions there, such as domain-tool precedence,
   nested-repository policy, or protected paths.

6. Verify from the target repository root. The first run may build the binary.

   ```bash
   scripts/contextmink files . --show-files 20
   scripts/contextmink --json guard-check -- git clean
   ```

For a guard hook in a vendored layout, pass explicit paths to the snippet
generator:

```bash
scripts/contextmink guard-hook-snippet \
  --binary F:/repo/tools/contextmink/target/release/contextmink.exe \
  --guard-config F:/repo/.contextmink.toml
```

### Maintaining a vendored copy

Compare or sync only the generic surface:

```text
tools/contextmink/src/
tools/contextmink/tests/
tools/contextmink/examples/
tools/contextmink/Cargo.toml
tools/contextmink/Cargo.lock
tools/contextmink/rust-toolchain.toml
tools/contextmink/README.md
tools/contextmink/CHANGELOG.md
tools/contextmink/docs/
tools/contextmink/scripts/
tools/contextmink/templates/
tools/contextmink/.github/
tools/contextmink/.gitattributes
tools/contextmink/.gitignore
tools/contextmink/LICENSE
tools/contextmink/LICENSE-SSL
tools/contextmink/LICENSE-VPL
scripts/contextmink
```

Do not sync a target repository's `.contextmink.toml`; that file is local
policy. Vendor only a verified, committed source revision.

## Verification

Native CI remains authoritative and runs formatting, tests, Clippy, package,
and Rust 1.95 MSRV checks on Windows, Linux, and macOS. Run
`scripts/verify_source.sh` for the same local source gates in dedicated
`target/source-check`, `target/package-check`, and `target/msrv-check`
directories, preventing prior local artifacts or Cargo's staged package build
from contaminating the proof. On Windows, invoke it through
`contextmink-bridge --script scripts/verify_source.sh`.

Keep package verification in a separate Cargo target directory. `cargo package`
verifies the staged source tree under `target/package`; sharing its fingerprints
with a later checkout build can make that build reuse the staged artifact. CI
uses `CARGO_TARGET_DIR=target/package-check cargo package --locked`. Use the
same boundary for local package checks (in PowerShell, set
`$env:CARGO_TARGET_DIR` before the command), then build the checkout in the
ordinary target directory.

`scripts/cross_check.sh` is an optional cross-link rehearsal for every
non-Windows release target. Install Zig plus `cargo-zigbuild` first. Missing
Rust targets fail with an exact `rustup` command; `--install-targets` is the
explicit opt-in to install them into the pinned toolchain. The rehearsal builds
the full compile surface and release binaries for Linux x64, Intel macOS, and
Apple Silicon macOS. A Windows host can report a missing Xcode SDK while still
completing Zig compilation; native GitHub macOS jobs remain the link/runtime
authority. The rehearsal denies crate warnings, accepts only the exact
environment-owned Apple SDK probe and its summaries, and fails on any other
warning. Zig is not a normal build dependency, and the repository does not
retain host-specific compiler wrappers or logs.

Before handing a commit to the release workflow, run
`scripts/verify_release.sh` (through `contextmink-bridge --script` on Windows).
It requires pinned actionlint `1.7.12`, validates release notes and dispatch
inputs, runs the isolated native source gate, then executes the complete Zig
rehearsal. Pass `--install-targets` only when explicitly authorizing repair of
missing pinned-toolchain components.

## Release packaging

Packaging and extracted-install checks use the development-only Rust example
`release_tools`, not an installed command or a Python runtime:

```sh
cargo run --locked --example release_tools -- notes <version>
cargo run --locked --example release_tools -- package-project <stage> <archive>
cargo run --locked --example release_tools -- verify-project <extracted-overlay>
cargo run --locked --example release_tools -- verify-user <extracted-binary>
```

`package-project` accepts a stage holding only the binaries, `manifest.json`,
`README.md`, `CHANGELOG.md`, and the licenses; it generates the skills and the
integration reference at their installed paths, records each binary's SHA-256
in the manifest, and archives the overlay with the host's `tar` (Windows'
built-in BSD tar for ZIP archives). Changelogs use user-visible categories and
upgrade guidance; wrapped Markdown prose and fenced examples are accepted by
the notes renderer.

The GitHub Release Artifacts workflow defaults to building without
publication. Set `artifact_version` to the crate version with a dated changelog
section. Source, MSRV, and native platform jobs run concurrently; publication
requires all of them to pass and an explicit `create_release=true` dispatch from
`master`. Each build retains rendered notes, four native archives, and adjacent
SHA-256 files; that checksum is how users verify a download. The archive
manifest identifies the source commit used for verification.
