#!/usr/bin/env bash
# Windows 크로스컴파일 환경 프로비전 (LXC 내부에서 root 실행)
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

echo "=== [1/4] 시스템 패키지 설치 ==="
apt-get update -qq
apt-get install -y \
  clang lld llvm \
  ninja-build \
  python3 python3-pip python3-requests \
  git curl ca-certificates \
  dosfstools mtools util-linux \
  2>/dev/null | tail -5

echo "=== [2/4] depot_tools 설치 ==="
if [ ! -d /home/builder/depot_tools/.git ]; then
  runuser -l builder -c 'git clone --depth 1 \
    https://chromium.googlesource.com/chromium/tools/depot_tools.git \
    /home/builder/depot_tools 2>&1'
else
  echo "  depot_tools 이미 존재 — skip"
fi

echo "=== [3/4] msvc-wine 설치 ==="
if [ ! -d /home/builder/msvc-wine/.git ]; then
  runuser -l builder -c 'git clone --depth 1 \
    https://github.com/mstorsjo/msvc-wine \
    /home/builder/msvc-wine 2>&1'
else
  echo "  msvc-wine 이미 존재 — skip"
fi

echo "=== [4/4] 환경 파일 생성 ==="
# TOOLCHAIN_HASH / SDK_VERSION 은 build/vs_toolchain.py 에서 추출
TOOLCHAIN_HASH=$(grep '^TOOLCHAIN_HASH' \
  /home/builder/workspace/helium-linux/build/src/build/vs_toolchain.py \
  2>/dev/null | cut -d"'" -f2 || echo "e66617bc68")
SDK_VERSION=$(grep '^SDK_VERSION' \
  /home/builder/workspace/helium-linux/build/src/build/vs_toolchain.py \
  2>/dev/null | cut -d"'" -f2 || echo "10.0.26100.0")

cat > /home/builder/.win-cross-env <<EOF
export PATH="/home/builder/depot_tools:\$PATH"
export DEPOT_TOOLS_WIN_TOOLCHAIN=0
export WIN_SDK_DIR=/home/builder/win-sdk
export TOOLCHAIN_HASH=${TOOLCHAIN_HASH}
export SDK_VERSION=${SDK_VERSION}
# Chromium이 로컬 toolchain을 찾을 수 있도록 설정
export DEPOT_TOOLS_WIN_TOOLCHAIN_BASE_URL=file:///home/builder/win-sdk/chromium-pkg/
export GYP_MSVS_HASH_${TOOLCHAIN_HASH}=${TOOLCHAIN_HASH}
EOF
chown builder:builder /home/builder/.win-cross-env

echo ""
echo "✓ Windows 크로스컴파일 프로비전 완료"
echo "  TOOLCHAIN_HASH : ${TOOLCHAIN_HASH}"
echo "  SDK_VERSION    : ${SDK_VERSION}"
echo ""
echo "다음 단계:"
echo "  pxi run chrome-browser-dev win-cross-sdk --vmid <vmid>   # SDK 다운로드 (~30분)"
echo "  pxi run chrome-browser-dev win-cross-build --vmid <vmid> # 빌드 시작"
