#!/usr/bin/env bash
set -euo pipefail

if ! command -v zig >/dev/null 2>&1; then
    echo "contextmink cross-check requires Zig on PATH" >&2
    exit 2
fi
if ! command -v cargo-zigbuild >/dev/null 2>&1; then
    echo "contextmink cross-check requires cargo-zigbuild (cargo install cargo-zigbuild)" >&2
    exit 2
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

targets=(
    x86_64-unknown-linux-gnu
    x86_64-apple-darwin
)
for target in "${targets[@]}"; do
    echo "contextmink cross-check: $target" >&2
    cargo zigbuild --locked --all-targets --target "$target"
done
