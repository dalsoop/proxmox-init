#!/bin/bash
# prelik-init smoke test — 모든 바이너리의 --help와 doctor가 동작하는지
set -euo pipefail
BIN_DIR="${1:-target/release}"
FAIL=0

check() {
    local name=$1
    local cmd=$2
    if bash -c "$cmd" >/dev/null 2>&1; then
        echo "  ✓ $name"
    else
        echo "  ✗ $name — cmd: $cmd"
        FAIL=$((FAIL+1))
    fi
}

echo "=== prelik CLI ==="
check "prelik --version" "$BIN_DIR/prelik --version"
check "prelik --help" "$BIN_DIR/prelik --help"
check "prelik available" "$BIN_DIR/prelik available"
check "prelik doctor" "$BIN_DIR/prelik doctor"

for dom in account ai backup bootstrap cloudflare comfyui connect deploy host iso lxc mail monitor nas net telegram traefik vm workspace; do
    echo ""
    echo "=== prelik-$dom ==="
    check "$dom --help" "$BIN_DIR/prelik-$dom --help"
    check "$dom doctor" "$BIN_DIR/prelik-$dom doctor"
done

echo ""
if [ $FAIL -eq 0 ]; then
    echo "✓ 모든 smoke test 통과"
    exit 0
else
    echo "✗ $FAIL 건 실패"
    exit 1
fi
