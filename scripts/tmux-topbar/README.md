# tmux-topbar

`tmux-sessionbar` + `tmux-windowbar` 위에 얹는 우측 상단 레이아웃 커스터마이즈.

## 무엇을 하나

기본 상태:
```
Users   👤 root  🖥 pve                                        ⊞  ⤢  ↻
Sessions ...
Windows  ...
Panes    ... | -
Apps     🔐 spf  📊 htop  ⚙ tmux-config
```

설치 후:
```
Users   👤 root  🖥 pve              ⊞  ⤢  ↻      | -      ⚙
Sessions ...
Windows  ...
Panes    ...
Apps     🔐 spf  📊 htop
```

- Panes 줄 끝의 `| -` (split-h / split-v) 버튼을 Users 줄 우측 끝으로 이식
- Users 줄 가장 오른쪽에 큰 톱니바퀴(⚙) 추가 → 클릭 시 `tmux-config` 새 윈도우
- Apps 줄에서 `tmux-config` 항목 제거 (톱니바퀴로 대체)

## 동작

1. `tmux-sessionbar apply` 가 `/root/.tmux.conf` 를 regen 할 때마다 systemd `path`
   유닛이 감지해 `ensure-panes-layout` 트리거
2. `ensure-panes-layout` 은:
   - `/root/.tmux.conf` 끝부분에 `patch-split-buttons.sh` 호출 줄 재주입
   - tmux 훅(`session-*`, `client-session-changed`, `window-linked/unlinked`) 을
     `render-status left + patch` 콤보로 오버라이드 → 이벤트 발생 시 자동 재이식
   - status bar 클릭 바인딩을 `tmux-statusbar-click` 디스패처로 교체
3. `patch-split-buttons.sh` 은 순수 멱등: format[3]에서 split 추출 → format[0]에
   spacer + split + spacer + ⚙ 로 이식. `windowbar apply` 호출 안 함 (훅 부작용 방지)

## 설치

```sh
sudo /opt/proxmox-init/scripts/tmux-topbar/install.sh
```

## 제거

```sh
systemctl disable --now tmux-panes-layout.path
rm -f /etc/systemd/system/tmux-panes-layout.{path,service}
rm -f /usr/local/bin/{ensure-panes-layout,tmux-statusbar-click}
rm -f /root/.config/tmux-windowbar/bin/patch-split-buttons.sh
systemctl daemon-reload
tmux-sessionbar apply  # 기본 레이아웃으로 복구
```

`tmux-config` 앱을 다시 활성화하려면 `/root/.config/tmux-windowbar/config.toml`
의 `[[apps]]` 에 다음을 추가:

```toml
[[apps]]
emoji = "⚙️"
command = "tmux-config"
fg = "#282c34"
bg = "#56b6c2"
mode = "pane"
```

## 간격 조정

`bin/patch-split-buttons.sh` 의 `SPACER_SPLIT`, `SPACER_GEAR` 의 공백 문자 수를
바꾸면 ⊞⤢↻ ↔ split ↔ ⚙ 사이 간격을 늘리거나 줄일 수 있다.
