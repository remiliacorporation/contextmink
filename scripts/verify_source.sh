#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

echo "contextmink verify: formatting" >&2
cargo fmt --all -- --check

echo "contextmink verify: tests" >&2
cargo test --locked --all-targets --all-features

echo "contextmink verify: clippy" >&2
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings

# Cargo gives a package verification build the same package identity as the
# source checkout. Keep its fingerprints out of the ordinary target directory
# so later integration tests cannot reuse staged package binaries.
package_target_dir="${CONTEXTMINK_PACKAGE_TARGET_DIR:-$repo_root/target/package-check}"
echo "contextmink verify: clean package in $package_target_dir" >&2
CARGO_TARGET_DIR="$package_target_dir" cargo package --locked
