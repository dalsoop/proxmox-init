#!/bin/bash
set -euo pipefail
REPO="dalsoop/prelik-init"
BIN_DIR="${PRELIK_BIN_DIR:-/usr/local/bin}"

if [ "$(id -u)" -ne 0 ] && [ ! -w "$BIN_DIR" ]; then
    BIN_DIR="$HOME/.local/bin"
fi

ARCH=$(uname -m)
case "$ARCH" in
    x86_64) TARGET="x86_64-linux" ;;
    aarch64|arm64) TARGET="aarch64-linux" ;;
    *) echo "지원하지 않는 아키텍처: $ARCH"; exit 1 ;;
esac

echo "=== prelik 설치 ==="
echo "  bin_dir: $BIN_DIR"
echo "  target:  $TARGET"

LATEST=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | head -1 | cut -d'"' -f4)
if [ -z "$LATEST" ]; then
    echo "✗ 릴리스 없음. 먼저 'cargo install --path crates/cli' 로 로컬 설치하세요."
    exit 1
fi

ASSET="prelik-${TARGET}.tar.gz"
mkdir -p "$BIN_DIR"
curl -sL "https://github.com/$REPO/releases/download/$LATEST/$ASSET" | tar xz -C "$BIN_DIR"
chmod +x "$BIN_DIR/prelik"

# PATH 권고
if ! echo "$PATH" | grep -q "$BIN_DIR"; then
    echo ""
    echo "  PATH에 $BIN_DIR 추가 필요:"
    echo "    echo 'export PATH=\"\$PATH:$BIN_DIR\"' >> ~/.bashrc"
fi

echo ""
echo "✓ prelik $LATEST 설치 완료"
echo "  다음 단계: prelik setup"
