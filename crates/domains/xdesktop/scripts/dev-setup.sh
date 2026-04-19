#!/usr/bin/env bash
# dev-setup.sh — xdesktop LXC 안에 GitHub 연동 + 개발 도구 세팅
# pxi run xdesktop dev 가 호출. 이미 설치된 게 있으면 skip (멱등).
#
# 환경변수:
#   XDESKTOP_USER     (기본 xuser)
#   GITHUB_USER       (선택 — 설정 시 SSH key clone 대상 추정)
#   REPOS             (선택, 쉼표구분, "dalsoop/proxmox-init,imputnet/helium-linux" 같이)
#   INSTALL_VSCODIUM  (기본 1 — 0 으로 주면 skip)

set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

XDESKTOP_USER="${XDESKTOP_USER:-xuser}"
GITHUB_USER="${GITHUB_USER:-}"
REPOS="${REPOS:-}"
INSTALL_VSCODIUM="${INSTALL_VSCODIUM:-1}"
USER_HOME="/home/$XDESKTOP_USER"

step() { printf '\n\033[1;36m[%s]\033[0m %s\n' "$1" "$2"; }
as_user() { sudo -u "$XDESKTOP_USER" -H bash -c "$*"; }

step 1/6 "apt 개발 기본 패키지"
apt-get update -qq
apt-get install -y --no-install-recommends \
  git git-lfs gh \
  build-essential pkg-config libssl-dev \
  python3 python3-pip python3-venv python3-dev pipx \
  ripgrep fd-find jq yq \
  tmux htop ncdu \
  ca-certificates gnupg curl wget \
  make cmake \
  2>&1 | tail -3

# 별칭 (fd-find → fd, batcat → bat 류)
command -v fd >/dev/null 2>&1 || ln -sf "$(command -v fdfind)" /usr/local/bin/fd 2>/dev/null || true

step 2/6 "Node.js LTS (NodeSource repo)"
if ! command -v node >/dev/null 2>&1; then
  curl -fsSL https://deb.nodesource.com/setup_22.x | bash -
  apt-get install -y --no-install-recommends nodejs 2>&1 | tail -2
  npm install -g pnpm yarn 2>&1 | tail -2
else
  echo "  node 이미 설치됨: $(node --version)"
fi

step 3/6 "Rust (rustup, $XDESKTOP_USER 유저 홈)"
if [ ! -x "$USER_HOME/.cargo/bin/rustc" ]; then
  as_user "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile default"
else
  echo "  rust 이미 설치됨: $(as_user '$HOME/.cargo/bin/rustc --version')"
fi

# PATH 에 cargo bin 추가 (bashrc / zshenv)
for rc in "$USER_HOME/.bashrc" "$USER_HOME/.profile"; do
  if [ -f "$rc" ] && ! grep -q 'cargo/env' "$rc"; then
    echo '. "$HOME/.cargo/env"' >> "$rc"
  fi
done

step 4/6 "VSCodium (옵션)"
if [ "$INSTALL_VSCODIUM" = "1" ]; then
  if ! command -v codium >/dev/null 2>&1; then
    install -d /usr/share/keyrings
    curl -fsSL https://gitlab.com/paulcarroty/vscodium-deb-rpm-repo/raw/master/pub.gpg \
      | gpg --dearmor -o /usr/share/keyrings/vscodium-archive-keyring.gpg
    echo 'deb [ signed-by=/usr/share/keyrings/vscodium-archive-keyring.gpg ] https://download.vscodium.com/debs vscodium main' \
      > /etc/apt/sources.list.d/vscodium.list
    apt-get update -qq
    apt-get install -y --no-install-recommends codium 2>&1 | tail -2
  else
    echo "  codium 이미 설치됨"
  fi
  # 바탕화면 아이콘
  as_user "mkdir -p '$USER_HOME/Desktop'"
  cat > "$USER_HOME/Desktop/VSCodium.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=VSCodium
Comment=Code editor
Exec=codium %U
Icon=codium
Terminal=false
Categories=Development;
DESK
  chmod +x "$USER_HOME/Desktop/VSCodium.desktop"
fi

step 5/6 "GitHub SSH key ($XDESKTOP_USER)"
SSH_DIR="$USER_HOME/.ssh"
as_user "mkdir -p '$SSH_DIR' && chmod 700 '$SSH_DIR'"
if [ ! -f "$SSH_DIR/id_ed25519" ]; then
  as_user "ssh-keygen -t ed25519 -N '' -f '$SSH_DIR/id_ed25519' -C '$XDESKTOP_USER@xdesktop'"
fi
# github.com known_hosts 선등록 (처음 git clone 인터랙티브 yes/no 회피)
if ! grep -q "github.com" "$SSH_DIR/known_hosts" 2>/dev/null; then
  ssh-keyscan -t ed25519,rsa github.com 2>/dev/null >> "$SSH_DIR/known_hosts" || true
fi
chown -R "$XDESKTOP_USER:$XDESKTOP_USER" "$SSH_DIR"

echo "  -- PUBLIC KEY (이걸 https://github.com/settings/keys 에 추가) --"
cat "$SSH_DIR/id_ed25519.pub"
echo "  -------------------------------------------------------------------"

# GitHub CLI 로그인 안내 (토큰 자동 아님, 수동 단계)
if [ -n "$GITHUB_USER" ]; then
  echo "  다음 단계 (LXC 내부 터미널에서 xuser 로):"
  echo "    gh auth login   # 또는  gh auth login --with-token < token.txt"
fi

step 6/6 "$([ -n "$REPOS" ] && echo "지정 레포 clone" || echo "src 디렉토리 준비")"
as_user "mkdir -p '$USER_HOME/src'"
if [ -n "$REPOS" ]; then
  IFS=',' read -ra REPO_ARR <<< "$REPOS"
  for r in "${REPO_ARR[@]}"; do
    r="$(echo "$r" | xargs)"  # trim
    [ -z "$r" ] && continue
    name="$(basename "$r")"
    if [ -d "$USER_HOME/src/$name" ]; then
      echo "  이미 있음: $USER_HOME/src/$name (skip)"
    else
      echo "  clone: $r"
      # SSH 방식 먼저, 실패시 HTTPS fallback
      as_user "cd '$USER_HOME/src' && (git clone git@github.com:${r}.git 2>/dev/null || git clone https://github.com/${r}.git)" || echo "  !! clone 실패: $r"
    fi
  done
else
  echo "  (레포 미지정 — 나중에 'git clone git@github.com:USER/REPO.git' 로 수동 clone)"
fi

RUSTC_VER="$("$USER_HOME/.cargo/bin/rustc" --version 2>/dev/null || echo '(설치 직후 — 새 쉘에서 확인)')"
CODIUM_VER="$(codium --version 2>/dev/null | head -1 || echo '-')"

cat <<DONE

======================================================================
 xdesktop dev 환경 준비 완료
   홈:        $USER_HOME
   git:       $(git --version 2>/dev/null)
   gh:        $(gh --version 2>/dev/null | head -1)
   node:      $(node --version 2>/dev/null)  (pnpm $(pnpm --version 2>/dev/null))
   rust:      $RUSTC_VER
   docker:    $(docker --version 2>/dev/null)
$([ "$INSTALL_VSCODIUM" = "1" ] && echo "   editor:    $CODIUM_VER")

   SSH key:   $SSH_DIR/id_ed25519 (pub 위에 출력됨)
   src:       $USER_HOME/src
======================================================================
DONE
