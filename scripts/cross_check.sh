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
# Rust's linker_messages lint reports Zig linker compatibility diagnostics on
# this rehearsal host. Deny every crate warning while silencing that lint; the
# Apple SDK probe occurs before crate lint levels apply and is classified below.
cross_rustflags="${RUSTFLAGS:-} -Dwarnings -Alinker-messages"

run_cross_build() {
    local warning_log="$repo_root/target/cross-check-warnings.log"
    local cargo_status
    local sdk_probe_warnings=0
    local summary_warnings=0
    local -a unexpected_warnings=()

    mkdir -p "$repo_root/target"
    set +e
    RUSTFLAGS="$cross_rustflags" cargo zigbuild "$@" 2>&1 | tee "$warning_log" >&2
    cargo_status=${PIPESTATUS[0]}
    set -e
    if ((cargo_status != 0)); then
        rm -f -- "$warning_log"
        return "$cargo_status"
    fi

    while IFS= read -r warning; do
        if [[ "$warning" == 'warning: invoking `"xcrun" "--sdk" "macosx" "--show-sdk-path"` to find MacOSX.sdk failed: program not found' ]]; then
            ((sdk_probe_warnings += 1))
        elif [[ "$warning" == 'warning: `contextmink` '* && "$warning" == *' generated 1 warning'* ]]; then
            ((summary_warnings += 1))
        else
            unexpected_warnings+=("$warning")
        fi
    done < <(grep '^warning:' "$warning_log" || true)
    rm -f -- "$warning_log"

    if ((summary_warnings > 0 && sdk_probe_warnings == 0)); then
        unexpected_warnings+=("contextmink warning summaries appeared without the accepted Apple SDK probe")
    fi
    if ((${#unexpected_warnings[@]})); then
        echo "contextmink cross-check found unexpected warning(s):" >&2
        printf '  %s\n' "${unexpected_warnings[@]}" >&2
        return 1
    fi
    if ((sdk_probe_warnings > 0)); then
        echo "contextmink cross-check accepted the non-native Apple SDK probe; native macOS CI retains link/runtime authority" >&2
    fi
}

for target in "${targets[@]}"; do
    echo "contextmink cross-check compile surface: $target" >&2
    run_cross_build --locked --all-targets --target "$target"
    echo "contextmink cross-check release binary: $target" >&2
    run_cross_build --locked --release --bins --target "$target"
done
