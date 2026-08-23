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

bash scripts/verify_source.sh
bash scripts/cross_check.sh "$@"
