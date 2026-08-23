#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/cross_check.sh [--install-targets]

Cross-compile every non-Windows GitHub release target with cargo-zigbuild.
Missing rustup targets fail with an exact repair command unless the explicit
--install-targets opt-in is supplied.
EOF
}

install_targets=false
while (($#)); do
    case "$1" in
        --install-targets) install_targets=true ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            echo "unknown cross-check argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
    shift
done

if ! command -v zig >/dev/null 2>&1; then
    echo "contextmink cross-check requires Zig on PATH" >&2
    exit 2
fi
if ! command -v cargo-zigbuild >/dev/null 2>&1; then
    echo "contextmink cross-check requires cargo-zigbuild (cargo install cargo-zigbuild)" >&2
    exit 2
fi
if ! command -v rustup >/dev/null 2>&1; then
    echo "contextmink cross-check requires rustup to verify target components" >&2
    exit 2
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

targets=(
    x86_64-unknown-linux-gnu
    x86_64-apple-darwin
    aarch64-apple-darwin
)

active_toolchain=$(rustup show active-toolchain | awk 'NR == 1 { print $1 }')
mapfile -t installed_targets < <(rustup target list --installed --toolchain "$active_toolchain")
missing_targets=()
for target in "${targets[@]}"; do
    installed=false
    for candidate in "${installed_targets[@]}"; do
        if [[ "$candidate" == "$target" ]]; then
            installed=true
            break
        fi
    done
    if [[ "$installed" == false ]]; then
        missing_targets+=("$target")
    fi
done

if ((${#missing_targets[@]})); then
    if [[ "$install_targets" == true ]]; then
        rustup target add --toolchain "$active_toolchain" "${missing_targets[@]}"
    else
        printf 'contextmink cross-check is missing rustup target(s): %s\n' \
            "${missing_targets[*]}" >&2
        printf 'repair explicitly with: rustup target add --toolchain %s' \
            "$active_toolchain" >&2
        printf ' %s' "${missing_targets[@]}" >&2
        printf '\nor rerun scripts/cross_check.sh --install-targets\n' >&2
        exit 2
    fi
fi

echo "contextmink cross-check toolchain: $active_toolchain; zig $(zig version)" >&2
# Rust's linker_messages lint reports non-native SDK discovery and Zig linker
# compatibility diagnostics on this rehearsal host. Native release jobs retain
# link/runtime authority; deny every other Rust warning while silencing only
# that environment-owned lint.
cross_rustflags="${RUSTFLAGS:-} -Dwarnings -Alinker-messages"
for target in "${targets[@]}"; do
    echo "contextmink cross-check compile surface: $target" >&2
    RUSTFLAGS="$cross_rustflags" cargo zigbuild --locked --all-targets --target "$target"
    echo "contextmink cross-check release binary: $target" >&2
    RUSTFLAGS="$cross_rustflags" cargo zigbuild --locked --release --bins --target "$target"
done
