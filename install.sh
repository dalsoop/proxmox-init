#!/bin/bash
set -euo pipefail
REPO="dalsoop/prelik-init"

# BIN_DIR 결정:
# 1) 유저가 PRELIK_BIN_DIR 지정 → 그대로 사용 (없으면 생성 시도)
# 2) root → /usr/local/bin
# 3) 나머지 → ~/.local/bin
if [ -n "${PRELIK_BIN_DIR:-}" ]; then
    BIN_DIR="$PRELIK_BIN_DIR"
elif [ "$(id -u)" -eq 0 ]; then
    BIN_DIR="/usr/local/bin"
else
    BIN_DIR="$HOME/.local/bin"
fi

# mkdir → 부모 디렉토리 쓰기 가능 여부 확인으로 override 무시 방지
if ! mkdir -p "$BIN_DIR" 2>/dev/null; then
    echo "✗ BIN_DIR 생성 실패: $BIN_DIR"
    echo "  PRELIK_BIN_DIR 환경변수로 쓰기 가능한 경로를 지정하세요."
    exit 1
fi
if [ ! -w "$BIN_DIR" ]; then
    echo "✗ BIN_DIR 쓰기 권한 없음: $BIN_DIR"
    echo "  sudo 또는 다른 PRELIK_BIN_DIR 사용."
    exit 1
fi

ARCH=$(uname -m)
case "$ARCH" in
    x86_64) TARGET="x86_64-linux" ;;
    aarch64|arm64) TARGET="aarch64-linux" ;;
    *) echo "✗ 지원하지 않는 아키텍처: $ARCH"; exit 1 ;;
esac

echo "=== prelik 설치 ==="
echo "  bin_dir: $BIN_DIR"
echo "  target:  $TARGET"

LATEST=$(curl -sSL --fail \
    -H 'Accept: application/vnd.github+json' \
    "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
    | grep '"tag_name"' | head -1 | cut -d'"' -f4 || true)
if [ -z "$LATEST" ]; then
    echo "✗ 릴리스를 찾을 수 없음. 먼저 'cargo install --path crates/cli' 로 로컬 설치하거나 태그를 푸시하세요."
    exit 1
fi

ASSET="prelik-${TARGET}.tar.gz"
URL="https://github.com/$REPO/releases/download/$LATEST/$ASSET"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

echo "  버전:    $LATEST"
echo "  다운로드: $URL"
if ! curl -sSL --fail -o "$TMP/$ASSET" "$URL"; then
    echo "✗ 다운로드 실패"
    exit 1
fi

# 최소 무결성 검증 (TLS + 파일 크기 + 압축 해제 성공)
SIZE=$(stat -c%s "$TMP/$ASSET" 2>/dev/null || stat -f%z "$TMP/$ASSET")
if [ -z "$SIZE" ] || [ "$SIZE" -lt 1024 ]; then
    echo "✗ 다운로드 파일이 비정상적으로 작음 (${SIZE} bytes) — MITM 또는 손상 의심"
    exit 1
fi

# file 타입 확인 (gzip이어야 함)
if ! file "$TMP/$ASSET" | grep -q "gzip compressed"; then
    echo "✗ 다운로드 파일이 gzip 형식이 아님 — HTML 에러 페이지 가능성"
    exit 1
fi

if ! tar -xzf "$TMP/$ASSET" -C "$BIN_DIR"; then
    echo "✗ 압축 해제 실패"
    exit 1
fi
chmod +x "$BIN_DIR/prelik"

# PATH 안내
if ! echo "$PATH" | tr ':' '\n' | grep -qxF "$BIN_DIR"; then
    echo ""
    echo "  ⚠ PATH에 $BIN_DIR 없음. shell rc에 추가 권장:"
    if [ -f "$HOME/.zshrc" ]; then
        echo "    echo 'export PATH=\"\$PATH:$BIN_DIR\"' >> ~/.zshrc"
    elif [ -f "$HOME/.bashrc" ]; then
        echo "    echo 'export PATH=\"\$PATH:$BIN_DIR\"' >> ~/.bashrc"
    else
        echo "    export PATH=\"\$PATH:$BIN_DIR\""
    fi
fi

echo ""
echo "✓ prelik $LATEST 설치 완료"
echo "  다음 단계:"
echo "    $BIN_DIR/prelik setup"
echo "    $BIN_DIR/prelik install bootstrap"
