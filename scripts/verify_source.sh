#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

source_target_dir="${CONTEXTMINK_SOURCE_TARGET_DIR:-$repo_root/target/source-check}"

echo "contextmink verify: formatting" >&2
cargo fmt --all -- --check

echo "contextmink verify: tests in $source_target_dir" >&2
CARGO_TARGET_DIR="$source_target_dir" cargo test --locked --all-targets --all-features

echo "contextmink verify: clippy in $source_target_dir" >&2
CARGO_TARGET_DIR="$source_target_dir" \
    cargo clippy --locked --workspace --all-targets --all-features -- -D warnings

# Cargo gives a package verification build the same package identity as the
# source checkout. Keep its fingerprints out of the ordinary target directory
# so later integration tests cannot reuse staged package binaries.
package_target_dir="${CONTEXTMINK_PACKAGE_TARGET_DIR:-$repo_root/target/package-check}"
echo "contextmink verify: clean package in $package_target_dir" >&2
CARGO_TARGET_DIR="$package_target_dir" cargo package --locked

msrv_toolchain="1.95.0"
msrv_target_dir="${CONTEXTMINK_MSRV_TARGET_DIR:-$repo_root/target/msrv-check}"
if ! rustup toolchain list | grep -Eq "^${msrv_toolchain}(-|$)"; then
    echo "contextmink verify requires Rust $msrv_toolchain; install it with: rustup toolchain install $msrv_toolchain --profile minimal" >&2
    exit 2
fi
echo "contextmink verify: MSRV $msrv_toolchain in $msrv_target_dir" >&2
CARGO_TARGET_DIR="$msrv_target_dir" cargo +"$msrv_toolchain" check --locked
