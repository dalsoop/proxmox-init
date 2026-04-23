#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

FILES="$(git diff --cached --name-only --diff-filter=ACMR -- '*.py' || true)"

if [ -n "$FILES" ]; then
  echo "python-mix-guard: failed" >&2
  echo "  - Python 파일 추가/수정 금지. Rust 또는 shell hook로 옮겨야 함." >&2
  while IFS= read -r f; do
    [ -z "$f" ] && continue
    echo "  - $f" >&2
  done <<< "$FILES"
  exit 1
fi

echo "python-mix-guard: ok"
