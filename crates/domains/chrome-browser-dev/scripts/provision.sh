#!/usr/bin/env bash
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

apt-get update
apt-get -y upgrade

apt-get install -y apt-transport-https ca-certificates curl gnupg lsb-release file
curl -fsSL https://deb.nodesource.com/setup_22.x | bash -

apt-get update
apt-get install -y \
  bison build-essential ccache clang cmake debhelper desktop-file-utils fd-find flex gdb git git-lfs gnupg2 gperf \
  gsettings-desktop-schemas-dev htop imagemagick jq less libasound2-dev libavcodec-dev libavformat-dev libavutil-dev \
  libcap-dev libcups2-dev libcurl4-openssl-dev libdrm-dev libegl1-mesa-dev libelf-dev libevent-dev libexif-dev \
  libflac-dev libgbm-dev libgcrypt20-dev libgl1-mesa-dev libgles2-mesa-dev libglew-dev libglib2.0-dev libglu1-mesa-dev \
  libgtk-3-dev libhunspell-dev libjpeg-dev libjs-jquery-flot libjsoncpp-dev libkrb5-dev liblcms2-dev libminizip-dev \
  libmodpbase64-dev libnspr4-dev libnss3-dev libopenjp2-7-dev libopus-dev libpam0g-dev libpci-dev libpipewire-0.3-dev \
  libpng-dev libpulse-dev libre2-dev libsnappy-dev libspeechd-dev libudev-dev libusb-1.0-0-dev libva-dev libvpx-dev \
  libwebp-dev libx11-xcb-dev libxcb-dri3-dev libxshmfence-dev libxslt1-dev libxss-dev libxt-dev libxtst-dev lld lsof \
  mesa-common-dev ninja-build nodejs pkg-config procps python-is-python3 python3-httplib2 python3-jinja2 python3-pillow \
  python3-pip python3-pyparsing python3-requests python3-setuptools python3-six python3-xcbgen qtbase5-dev quilt ripgrep rsync \
  sudo tmux uuid-dev valgrind vim wdiff x11-apps xauth xcb-proto xfonts-base xvfb xz-utils yasm zip unzip zstd

if ! id builder >/dev/null 2>&1; then
  useradd -m -s /bin/bash builder
fi
usermod -aG sudo builder
install -d -o builder -g builder /home/builder/workspace /home/builder/.cache/ccache /home/builder/.cache/sccache

cat >/etc/sudoers.d/90-builder-nopasswd <<'EOF'
builder ALL=(ALL) NOPASSWD:ALL
EOF
chmod 0440 /etc/sudoers.d/90-builder-nopasswd

if ! command -v sccache >/dev/null 2>&1; then
  tmpdir="$(mktemp -d)"
  arch="$(uname -m)"
  curl --fail -L "https://github.com/mozilla/sccache/releases/download/v0.10.0/sccache-v0.10.0-${arch}-unknown-linux-musl.tar.gz" -o "$tmpdir/sccache.tar.gz"
  tar --strip-components=1 -xzf "$tmpdir/sccache.tar.gz" -C /usr/local/bin --wildcards '*/sccache'
  rm -rf "$tmpdir"
fi

cat >/etc/profile.d/chromium-build.sh <<'EOF'
export CHROMIUM_BUILDER_JOBS="${CHROMIUM_BUILDER_JOBS:-48}"
export CCACHE_DIR="${CCACHE_DIR:-$HOME/.cache/ccache}"
export CCACHE_MAXSIZE="${CCACHE_MAXSIZE:-80G}"
export CCACHE_BASEDIR="${CCACHE_BASEDIR:-$HOME/workspace}"
export CCACHE_SLOPPINESS="${CCACHE_SLOPPINESS:-include_file_mtime}"
export SCCACHE_DIR="${SCCACHE_DIR:-$HOME/.cache/sccache}"
export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-120G}"
export SCCACHE_IGNORE_SERVER_IO_ERROR=1
export NINJA_STATUS="[%r active/%f finished/%t total] "
EOF

sudo -u builder bash -lc 'ccache --set-config=max_size=80G || true'

mkdir -p /opt/chromium-browser-dev/bin
install -m 0755 /tmp/chromium-browser-dev /opt/chromium-browser-dev/bin/chromium-browser-dev
ln -sf /opt/chromium-browser-dev/bin/chromium-browser-dev /usr/local/bin/chromium-browser-dev

cat >/etc/motd <<'EOF'
Chromium Browser Dev LXC

Use the builder account:
  su - builder

Main command:
  chromium-browser-dev --help

Workspace:
  /home/builder/workspace
EOF
