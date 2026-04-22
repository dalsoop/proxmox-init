#!/bin/bash
# install.prelik.com → 이 스크립트.
# 환경변수:
#   PRELIK_BIN_DIR  설치 위치 (기본: root → /usr/local/bin, user → ~/.local/bin)
#   PRELIK_VERSION  특정 버전 핀 (예: v1.5.5). 미지정 시 latest.
#   PRELIK_FORCE    1 이면 같은 버전이어도 재설치.
set -euo pipefail
REPO="dalsoop/pxi-init"

# ---------- BIN_DIR 결정 + 검증 ----------
if [ -n "${PRELIK_BIN_DIR:-}" ]; then
    BIN_DIR="$PRELIK_BIN_DIR"
elif [ "$(id -u)" -eq 0 ]; then
    BIN_DIR="/usr/local/bin"
else
    BIN_DIR="$HOME/.local/bin"
fi
if ! mkdir -p "$BIN_DIR" 2>/dev/null; then
    echo "✗ BIN_DIR 생성 실패: $BIN_DIR — PRELIK_BIN_DIR로 쓰기 가능한 경로를 지정하세요." >&2
    exit 1
fi
if [ ! -w "$BIN_DIR" ]; then
    echo "✗ BIN_DIR 쓰기 권한 없음: $BIN_DIR" >&2
    exit 1
fi

# ---------- ARCH ----------
ARCH=$(uname -m)
case "$ARCH" in
    x86_64)        TARGET="x86_64-linux" ;;
    aarch64|arm64) TARGET="aarch64-linux" ;;
    *) echo "✗ 지원하지 않는 아키텍처: $ARCH" >&2; exit 1 ;;
esac

echo "=== prelik 설치 ==="
echo "  bin_dir : $BIN_DIR"
echo "  target  : $TARGET"

# ---------- 버전 결정 (PRELIK_VERSION 또는 latest, retry) ----------
fetch_with_retry() {
    local url=$1 out=$2 attempt
    for attempt in 1 2 3; do
        if curl -sSL --fail \
            --connect-timeout 10 --max-time 60 \
            -H 'Accept: application/vnd.github+json' \
            "$url" -o "$out" 2>/dev/null; then
            return 0
        fi
        sleep $((attempt * 2))
    done
    return 1
}

if [ -n "${PRELIK_VERSION:-}" ]; then
    VERSION="$PRELIK_VERSION"
    case "$VERSION" in v*) ;; *) VERSION="v$VERSION" ;; esac
else
    META=$(mktemp)
    trap 'rm -f "$META"' EXIT
    if ! fetch_with_retry "https://api.github.com/repos/$REPO/releases/latest" "$META"; then
        echo "✗ GitHub API 요청 실패 (rate limit / 네트워크). PRELIK_VERSION으로 명시 설치 가능." >&2
        exit 1
    fi
    VERSION=$(grep '"tag_name"' "$META" | head -1 | cut -d'"' -f4 || true)
    rm -f "$META"; trap - EXIT
    if [ -z "$VERSION" ]; then
        echo "✗ 릴리스 태그를 추출하지 못함" >&2
        exit 1
    fi
fi
echo "  버전     : $VERSION"

# ---------- 같은 버전 이미 설치되어 있으면 skip (PRELIK_FORCE=1로 무시) ----------
# cargo workspace.version이 git tag와 분리돼 있어서 prelik --version 비교는 부정확.
# 설치 시점에 태그를 마커 파일로 기록하고 다음 실행에서 비교한다.
MARKER="$BIN_DIR/.prelik.version"
if [ -x "$BIN_DIR/prelik" ] && [ -f "$MARKER" ] && [ -z "${PRELIK_FORCE:-}" ]; then
    INSTALLED=$(cat "$MARKER" 2>/dev/null || true)
    if [ "$INSTALLED" = "$VERSION" ]; then
        echo "✓ 이미 $VERSION 설치됨 (PRELIK_FORCE=1로 재설치 가능)"
        exit 0
    fi
fi

# ---------- 다운로드 → atomic install ----------
ASSET="pxi-${TARGET}.tar.gz"
URL="https://github.com/$REPO/releases/download/$VERSION/$ASSET"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
echo "  다운로드 : $URL"

for attempt in 1 2 3; do
    if curl -sSL --fail \
        --connect-timeout 10 --max-time 120 \
        -o "$TMP/$ASSET" "$URL" 2>/dev/null; then
        break
    fi
    if [ "$attempt" = 3 ]; then
        echo "✗ 다운로드 실패 (3회 재시도)" >&2
        exit 1
    fi
    sleep $((attempt * 2))
done

# 무결성: 크기 + gzip 매직
SIZE=$(stat -c%s "$TMP/$ASSET" 2>/dev/null || stat -f%z "$TMP/$ASSET")
if [ -z "$SIZE" ] || [ "$SIZE" -lt 1024 ]; then
    echo "✗ 다운로드 파일이 비정상적으로 작음 (${SIZE} bytes) — MITM/손상 의심" >&2
    exit 1
fi
MAGIC=$(head -c 2 "$TMP/$ASSET" | od -An -tx1 | tr -d ' \n' || true)
if [ "$MAGIC" != "1f8b" ]; then
    echo "✗ gzip 형식 아님 (magic: $MAGIC) — HTML 에러 페이지 가능성" >&2
    exit 1
fi

# TMP에 풀고 atomic mv (실행 중 바이너리 교체 race + text file busy 회피)
if ! tar -xzf "$TMP/$ASSET" -C "$TMP"; then
    echo "✗ 압축 해제 실패" >&2
    exit 1
fi
if [ ! -f "$TMP/prelik" ]; then
    echo "✗ tarball에 prelik 바이너리가 없음" >&2
    exit 1
fi
chmod 755 "$TMP/prelik"
# mv는 같은 파일시스템이면 atomic. /tmp가 다른 fs일 수 있으므로 BIN_DIR 안에 staging.
STAGING="$BIN_DIR/.prelik.new.$$"
mv -f "$TMP/prelik" "$STAGING"
if ! mv -f "$STAGING" "$BIN_DIR/prelik"; then
    rm -f "$STAGING"
    echo "✗ 최종 설치 mv 실패" >&2
    exit 1
fi
# 설치 성공 — 마커에 태그 기록 (다음 실행에서 idempotent skip 판정)
printf '%s\n' "$VERSION" > "$MARKER" 2>/dev/null || true

# ---------- PATH 안내 ----------
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

# ---------- shell-tools (선택, Proxmox 호스트에서만) ----------
# Proxmox 호스트(=pct 존재) 에서만 pxi-laravel, pxi-seo-monitor, kubetest-run 같은
# shell 기반 확장 설치. PRELIK_SKIP_SHELL_TOOLS=1 이면 생략.
if [ -z "${PRELIK_SKIP_SHELL_TOOLS:-}" ] && command -v pct >/dev/null 2>&1; then
    SHARE_DIR="${PRELIK_SHARE_DIR:-/usr/local/share}"
    if [ ! -w "$SHARE_DIR" ] && [ "$(id -u)" -ne 0 ]; then
        SHARE_DIR="$HOME/.local/share"
    fi
    mkdir -p "$SHARE_DIR"

    ST_URL="https://github.com/$REPO/archive/refs/tags/$VERSION.tar.gz"
    ST_TMP="$TMP/src"
    mkdir -p "$ST_TMP"
    if curl -sSL --fail --connect-timeout 10 --max-time 180 "$ST_URL" \
        | tar -xz -C "$ST_TMP" --wildcards '*/shell-tools/*' 2>/dev/null; then
        ST_ROOT=$(find "$ST_TMP" -maxdepth 2 -name 'shell-tools' -type d | head -1)
        if [ -n "$ST_ROOT" ]; then
            for f in "$ST_ROOT/bin/"*; do
                [ -f "$f" ] || continue
                install -m 0755 "$f" "$BIN_DIR/$(basename "$f")"
            done
            if [ -d "$ST_ROOT/share" ]; then
                cp -r "$ST_ROOT/share/." "$SHARE_DIR/"
            fi
            tools_list="$(find "$ST_ROOT/bin" -maxdepth 1 -type f -printf '%f ' 2>/dev/null)"
            echo "  shell-tools: ${tools_list}→ $BIN_DIR/"
        fi
    else
        echo "  shell-tools: 이 태그에 shell-tools/ 없음 — skip"
    fi
fi

echo ""
echo "✓ prelik $VERSION 설치 완료"
echo "  다음 단계:"
echo "    $BIN_DIR/prelik setup"
echo "    $BIN_DIR/prelik install bootstrap"
