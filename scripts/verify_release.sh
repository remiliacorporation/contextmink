#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

expected_actionlint="1.7.12"
if ! command -v actionlint >/dev/null 2>&1; then
    echo "contextmink release verification requires actionlint $expected_actionlint on PATH" >&2
    echo "install the pinned upstream release from https://github.com/rhysd/actionlint/releases/tag/v$expected_actionlint" >&2
    exit 2
fi
actual_actionlint=$(actionlint -version | awk 'NR == 1 { print $1 }')
if [[ "$actual_actionlint" != "$expected_actionlint" ]]; then
    echo "contextmink release verification requires actionlint $expected_actionlint; found $actual_actionlint" >&2
    exit 2
fi

echo "contextmink release verify: GitHub workflow schema" >&2
actionlint -color .github/workflows/*.yml

python_command=""
for candidate in python3 python; do
    if command -v "$candidate" >/dev/null 2>&1 &&
        "$candidate" -c 'import sys; sys.exit(sys.version_info.major != 3)' >/dev/null 2>&1; then
        python_command="$candidate"
        break
    fi
done
if [[ -z "$python_command" ]]; then
    echo "contextmink release verification requires Python 3; install it as python3 or python on PATH" >&2
    exit 2
fi
"$python_command" scripts/test_release_notes.py
version=$(awk -F '"' '/^version = "/ { print $2; exit }' Cargo.toml)
bash scripts/validate_release_dispatch.sh "$version" false refs/heads/onno/local-verification
"$python_command" scripts/render_release_notes.py "$version" >/dev/null
bash scripts/verify_source.sh
bash scripts/cross_check.sh "$@"
