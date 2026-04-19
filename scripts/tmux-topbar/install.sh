#!/usr/bin/env bash
# tmux-topbar 커스터마이즈 설치 (멱등).
#   - patch-split-buttons.sh: Panes 줄의 split 버튼(| -)을 Users 줄(우측 상단)로 이식
#     + 우측 끝에 톱니바퀴(⚙) 추가
#   - tmux-statusbar-click: status bar 클릭 디스패처 (톱니바퀴 → tmux-config 새 윈도우)
#   - ensure-panes-layout: tmux-sessionbar 가 ~/.tmux.conf 를 regen 할 때 커스텀 줄
#     재주입 + 훅 재설정 + format 재이식
#   - systemd path 유닛: /root/.tmux.conf 변경 감지 → ensure-panes-layout 트리거
#
# Requires: tmux-sessionbar, tmux-windowbar (이미 설치돼 있어야 함)
set -euo pipefail

SRC="$(cd "$(dirname "$0")" && pwd)"

install -m 0755 -D "$SRC/bin/patch-split-buttons.sh" /root/.config/tmux-windowbar/bin/patch-split-buttons.sh
install -m 0755 -D "$SRC/bin/ensure-panes-layout"    /usr/local/bin/ensure-panes-layout
install -m 0755 -D "$SRC/bin/tmux-statusbar-click"   /usr/local/bin/tmux-statusbar-click

install -m 0644 -D "$SRC/systemd/tmux-panes-layout.service" /etc/systemd/system/tmux-panes-layout.service
install -m 0644 -D "$SRC/systemd/tmux-panes-layout.path"    /etc/systemd/system/tmux-panes-layout.path

systemctl daemon-reload
systemctl enable --now tmux-panes-layout.path

# 즉시 1회 적용 (tmux 서버 떠 있을 때만)
if tmux list-sessions >/dev/null 2>&1; then
    /usr/local/bin/ensure-panes-layout || true
fi

echo "tmux-topbar installed."
