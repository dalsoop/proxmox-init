#!/usr/bin/env bash
# 로컬에서 빌드한 pxi 를 ~/.local/bin (user) 또는 /usr/local/bin (root) 로 설치.
# GitHub release 경유 없이 개발 중인 변경을 바로 쓰고 싶을 때.
#
# 핵심 게이트:
#   1) ncl/domains.ncl 스키마 검증 (nickel export) → 실패 시 빌드 중단.
#      mac-app-init 의 install-local.sh:15 와 동일 패턴. Rust 코드와 Nickel
#      SSOT 가 일치하지 않은 상태로 설치되는 사고 방지.
#   2) 검증 통과 시 locale.json 을 data_dir 에 기록 (런타임 SSOT consumer 용 — PR 5).
#   3) cargo build 후 바이너리 복사.
#
# 사용법:
#   scripts/install-local.sh              # pxi 메타 바이너리만
#   scripts/install-local.sh --all        # 29개 도메인 바이너리까지 전부
#   scripts/install-local.sh <domain>     # 특정 도메인만 (예: lxc, traefik)
#
# 환경:
#   PXI_SKIP_NCL=1   ncl 게이트 건너뛰기 (위험 — nickel 미설치 dev 머신 긴급용)

set -euo pipefail

# --help 는 ncl 게이트 돌리기 전에 처리 (빠른 사용법 확인용).
case "${1:-}" in
  -h|--help)
    sed -n '2,22p' "$0"
    exit 0
    ;;
esac

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# bin_dir 결정: root → /usr/local/bin, user → ~/.local/bin
if [ "$(id -u)" -eq 0 ]; then
  BIN_DIR="${PXI_BIN_DIR:-/usr/local/bin}"
  LOCALE_DIR="${PXI_DATA_DIR:-/var/lib/pxi}"
else
  BIN_DIR="${PXI_BIN_DIR:-$HOME/.local/bin}"
  LOCALE_DIR="${PXI_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/pxi}"
fi
mkdir -p "$BIN_DIR" "$LOCALE_DIR"

# ── 게이트 1: ncl 스키마 검증 + locale.json 렌더 ──
if [ "${PXI_SKIP_NCL:-}" != "1" ]; then
  if ! command -v nickel >/dev/null 2>&1; then
    echo "✗ nickel 미설치 — 'pxi run bootstrap nickel' 또는 cargo install nickel-lang-cli 로 설치" >&2
    echo "  긴급 우회: PXI_SKIP_NCL=1 $0 $*" >&2
    exit 1
  fi
  echo "▶ ncl/domains.ncl 스키마 검증 + locale.json 렌더..."
  if ! nickel export --format json "$ROOT/ncl/domains.ncl" \
        > "$LOCALE_DIR/locale.json.tmp" 2>&1; then
    echo "✗ ncl/domains.ncl 스키마 위반 — 빌드 중단" >&2
    echo "  'nickel export ncl/domains.ncl' 로 에러 확인" >&2
    rm -f "$LOCALE_DIR/locale.json.tmp"
    exit 1
  fi
  mv -f "$LOCALE_DIR/locale.json.tmp" "$LOCALE_DIR/locale.json"
  echo "  ✓ $LOCALE_DIR/locale.json"
else
  echo "⚠ PXI_SKIP_NCL=1 — ncl 게이트 건너뜀 (drift 위험)"
fi

# ── 게이트 2: cargo build + 설치 ──
build_and_copy() {
  local crate="$1"      # Cargo.toml package 이름 (예: pxi, pxi-lxc)
  local bin_name="$2"   # 설치할 바이너리 이름 (대개 crate 와 동일)
  echo "▶ build $crate"
  (cd "$ROOT" && cargo build -p "$crate" --release --quiet)
  local src="$ROOT/target/release/$bin_name"
  if [ ! -f "$src" ]; then
    echo "  ✗ 빌드 산출물 없음: $src" >&2
    return 1
  fi
  install -m 0755 "$src" "$BIN_DIR/$bin_name"
  echo "  ✓ $BIN_DIR/$bin_name"
}

case "${1:-}" in
  --all)
    build_and_copy "pxi" "pxi"
    for d in "$ROOT"/crates/domains/*/; do
      name="$(basename "$d")"
      build_and_copy "pxi-$name" "pxi-$name" || true
    done
    ;;
  "")
    build_and_copy "pxi" "pxi"
    ;;
  *)
    for name in "$@"; do
      case "$name" in
        pxi) build_and_copy "pxi" "pxi" ;;
        *)   build_and_copy "pxi-$name" "pxi-$name" ;;
      esac
    done
    ;;
esac

echo ""
echo "✓ install-local 완료"
