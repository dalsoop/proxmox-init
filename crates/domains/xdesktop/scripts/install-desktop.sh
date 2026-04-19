#!/usr/bin/env bash
# install-desktop.sh — X11 데스크톱 + 한글 + Helium 설치 (Debian 13 trixie)
# pxi-xdesktop 에 의해 LXC 내부에서 실행됨.
# 환경변수로 구성 가능:
#   XDESKTOP_USER (기본 xuser)
#   XPRA_PORT     (기본 14500)
#   XPRA_DISPLAY  (기본 :100)
#   HELIUM_TAG    (기본 0.11.2.1)

set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

XDESKTOP_USER="${XDESKTOP_USER:-xuser}"
XPRA_PORT="${XPRA_PORT:-14500}"
XPRA_DISPLAY="${XPRA_DISPLAY:-:100}"
HELIUM_TAG="${HELIUM_TAG:-0.11.2.1}"

step() { printf '\n\033[1;36m[%s]\033[0m %s\n' "$1" "$2"; }

step 1/9 "로케일 (ko_KR.UTF-8 + en_US.UTF-8)"
# 이전 실패 재실행 대비 — 잘못된 xpra.list 정리
rm -f /etc/apt/sources.list.d/xpra.list
apt-get update -y
apt-get install -y --no-install-recommends locales
sed -i 's/^# *\(ko_KR\.UTF-8 UTF-8\|en_US\.UTF-8 UTF-8\)/\1/' /etc/locale.gen
locale-gen
update-locale LANG=ko_KR.UTF-8

step 2/9 "Xpra 공식 repo (xpra.org trixie, DEB822 .sources 형식)"
apt-get install -y --no-install-recommends curl gnupg ca-certificates
install -d /usr/share/keyrings
if [ ! -f /usr/share/keyrings/xpra.asc ]; then
  curl -fsSL https://xpra.org/xpra.asc -o /usr/share/keyrings/xpra.asc
  chmod 0644 /usr/share/keyrings/xpra.asc
fi
# 기존 잘못된 .list 제거 (이전 실패 재실행 대비)
rm -f /etc/apt/sources.list.d/xpra.list
cat > /etc/apt/sources.list.d/xpra.sources <<'REPO'
Types: deb
URIs: https://xpra.org
Suites: trixie
Components: main
Signed-By: /usr/share/keyrings/xpra.asc
Architectures: amd64 arm64
REPO
apt-get update -y

step 3/9 "Xpra + XFCE + 기본 도구"
apt-get install -y --no-install-recommends \
  xpra xpra-x11 xpra-server xpra-html5 \
  xpra-codecs xpra-codecs-extras \
  xvfb \
  xfce4 xfce4-terminal xfce4-goodies \
  thunar mousepad ristretto \
  dbus-x11 xdg-utils polkitd pkexec \
  sudo wget less vim-tiny nano \
  xauth xinit

step 4/9 "CJK 폰트 + 이모지"
apt-get install -y --no-install-recommends \
  fonts-noto-cjk fonts-noto-color-emoji \
  fonts-noto-core fonts-noto-mono \
  fonts-nanum fonts-nanum-extra

# 커서 테마 — Xvfb 는 기본 커서가 안 보이므로 Adwaita(또는 DMZ) 명시 지정 필요
apt-get install -y --no-install-recommends dmz-cursor-theme xcursor-themes

step 5/9 "fcitx5 + 한글 입력기"
apt-get install -y --no-install-recommends \
  fcitx5 fcitx5-hangul fcitx5-configtool \
  fcitx5-frontend-gtk3 fcitx5-frontend-qt5 \
  im-config

# 시스템 환경변수 (chromium/helium이 fcitx 인식하도록)
cat > /etc/profile.d/99-fcitx5.sh <<'ENVFILE'
export GTK_IM_MODULE=fcitx
export QT_IM_MODULE=fcitx
export XMODIFIERS=@im=fcitx
export SDL_IM_MODULE=fcitx
ENVFILE
chmod 0644 /etc/profile.d/99-fcitx5.sh

# im-config 선택
if command -v im-config >/dev/null; then
  im-config -n fcitx5 >/dev/null 2>&1 || true
fi

step 6/9 "데스크톱 유저 ($XDESKTOP_USER) + sudo + 자동시작"
id -u "$XDESKTOP_USER" >/dev/null 2>&1 || useradd -m -s /bin/bash -G sudo "$XDESKTOP_USER"
echo "$XDESKTOP_USER ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/$XDESKTOP_USER
chmod 0440 /etc/sudoers.d/$XDESKTOP_USER

USER_HOME="/home/$XDESKTOP_USER"
sudo -u "$XDESKTOP_USER" mkdir -p \
  "$USER_HOME/.config/autostart" \
  "$USER_HOME/Desktop"

# fcitx5 autostart
cat > "$USER_HOME/.config/autostart/fcitx5.desktop" <<'FCX'
[Desktop Entry]
Type=Application
Name=Fcitx 5
Exec=fcitx5 -d
X-GNOME-Autostart-enabled=true
NoDisplay=false
FCX

# 유저 환경변수 (XFCE 세션)
cat > "$USER_HOME/.xprofile" <<'XPROFILE'
export LANG=ko_KR.UTF-8
export LC_ALL=ko_KR.UTF-8
export GTK_IM_MODULE=fcitx
export QT_IM_MODULE=fcitx
export XMODIFIERS=@im=fcitx
export SDL_IM_MODULE=fcitx
XPROFILE

chown -R "$XDESKTOP_USER:$XDESKTOP_USER" "$USER_HOME"

step 7/9 "Helium 브라우저 (v$HELIUM_TAG)"
ARCH="$(dpkg --print-architecture)"
case "$ARCH" in
  amd64) SFX="amd64" ;;
  arm64) SFX="arm64" ;;
  *) echo "지원 안 되는 아키텍처: $ARCH"; exit 1 ;;
esac
HELIUM_URL="https://github.com/imputnet/helium-linux/releases/download/${HELIUM_TAG}/helium-bin_${HELIUM_TAG}-1_${SFX}.deb"
HELIUM_DEB=/tmp/helium.deb
echo "다운로드: $HELIUM_URL"
curl -fL --progress-bar -o "$HELIUM_DEB" "$HELIUM_URL"
apt-get install -y "$HELIUM_DEB" || { dpkg -i "$HELIUM_DEB" || true; apt-get install -f -y; }
rm -f "$HELIUM_DEB"

# LXC unprivileged 샌드박스 우회 wrapper.
# /usr/local/bin/helium 이 /usr/bin/helium 을 PATH로 shadow → 절대 경로(/opt/helium/helium) 호출로 재귀 회피.
# helium-bin .deb 은 /opt/helium/helium 실바이너리 + /usr/bin/helium 심볼릭링크 + /usr/share/applications/helium.desktop 설치.
cat > /usr/local/bin/helium <<'WRAP'
#!/bin/sh
exec /opt/helium/helium --no-sandbox --disable-dev-shm-usage "$@"
WRAP
chmod +x /usr/local/bin/helium

# 데스크톱 아이콘
cat > "$USER_HOME/Desktop/Helium.desktop" <<'DESK'
[Desktop Entry]
Type=Application
Name=Helium
Exec=/usr/local/bin/helium %U
Icon=helium
Terminal=false
Categories=Network;WebBrowser;
DESK
chmod +x "$USER_HOME/Desktop/Helium.desktop"

# Helium 자동 실행은 일부러 생략 — 사용자가 바탕화면/하단 dock 에서 클릭해 시작.
# (autostart 하면 세션마다 첫 인상으로 Helium 창이 뜨는데 원격 리프레시 시 중복 실행 위험.)
rm -f "$USER_HOME/.config/autostart/Helium.desktop" 2>/dev/null || true

# XFCE4 패널 프로필 — 기본 XFCE 설정은 pager(빈 워크스페이스 4개), 비는 systray 슬롯 등으로
# 원격 데스크톱 UX 에 너절함. 깔끔한 minimal profile 강제:
#   panel-1 (상단): applicationsmenu("프로그램") + tasklist + separator + systray(fcitx5) + clock
#   panel-2 (하단 dock): Helium / Terminal / Thunar / Mousepad 런처
# 런처는 시스템 /usr/share/applications/*.desktop 직접 참조 (XFCE 4.20+ 지원)
# pager 제거, 액션 플러그인 제거, 워크스페이스 1개.
PANEL_DIR="$USER_HOME/.config/xfce4/xfconf/xfce-perchannel-xml"
mkdir -p "$PANEL_DIR"
cat > "$PANEL_DIR/xfce4-panel.xml" <<'PANEL_XML'
<?xml version="1.1" encoding="UTF-8"?>
<channel name="xfce4-panel" version="1.0">
  <property name="configver" type="int" value="2"/>
  <property name="panels" type="array">
    <value type="int" value="1"/>
    <value type="int" value="2"/>
    <property name="dark-mode" type="bool" value="true"/>
    <property name="panel-1" type="empty">
      <property name="position" type="string" value="p=6;x=0;y=0"/>
      <property name="length" type="uint" value="100"/>
      <property name="position-locked" type="bool" value="true"/>
      <property name="icon-size" type="uint" value="16"/>
      <property name="size" type="uint" value="28"/>
      <property name="plugin-ids" type="array">
        <value type="int" value="1"/>
        <value type="int" value="2"/>
        <value type="int" value="3"/>
        <value type="int" value="6"/>
        <value type="int" value="7"/>
        <value type="int" value="8"/>
      </property>
    </property>
    <property name="panel-2" type="empty">
      <property name="autohide-behavior" type="uint" value="0"/>
      <property name="position" type="string" value="p=10;x=0;y=0"/>
      <property name="length" type="uint" value="1"/>
      <property name="position-locked" type="bool" value="true"/>
      <property name="size" type="uint" value="44"/>
      <property name="icon-size" type="uint" value="32"/>
      <property name="plugin-ids" type="array">
        <value type="int" value="13"/>
        <value type="int" value="11"/>
        <value type="int" value="12"/>
        <value type="int" value="14"/>
      </property>
    </property>
  </property>
  <property name="plugins" type="empty">
    <property name="plugin-1" type="string" value="applicationsmenu">
      <property name="button-title" type="string" value="프로그램"/>
    </property>
    <property name="plugin-2" type="string" value="tasklist">
      <property name="grouping" type="uint" value="1"/>
      <property name="show-labels" type="bool" value="true"/>
    </property>
    <property name="plugin-3" type="string" value="separator">
      <property name="expand" type="bool" value="true"/>
      <property name="style" type="uint" value="0"/>
    </property>
    <property name="plugin-6" type="string" value="systray">
      <property name="square-icons" type="bool" value="true"/>
    </property>
    <property name="plugin-7" type="string" value="separator">
      <property name="style" type="uint" value="0"/>
    </property>
    <property name="plugin-8" type="string" value="clock">
      <property name="mode" type="uint" value="2"/>
      <property name="digital-time-format" type="string" value="%H:%M"/>
      <property name="digital-date-format" type="string" value="%m/%d(%a)"/>
      <property name="tooltip-format" type="string" value="%Y년 %m월 %d일 %A"/>
    </property>
    <property name="plugin-11" type="string" value="launcher">
      <property name="items" type="array">
        <value type="string" value="xfce4-terminal.desktop"/>
      </property>
    </property>
    <property name="plugin-12" type="string" value="launcher">
      <property name="items" type="array">
        <value type="string" value="thunar.desktop"/>
      </property>
    </property>
    <property name="plugin-13" type="string" value="launcher">
      <property name="items" type="array">
        <value type="string" value="helium.desktop"/>
      </property>
    </property>
    <property name="plugin-14" type="string" value="launcher">
      <property name="items" type="array">
        <value type="string" value="org.xfce.mousepad.desktop"/>
      </property>
    </property>
  </property>
</channel>
PANEL_XML

# xfce4-session 도 워크스페이스 1개로 고정 (pager 제거와 일관)
cat > "$PANEL_DIR/xfwm4.xml" <<'WM_XML'
<?xml version="1.1" encoding="UTF-8"?>
<channel name="xfwm4" version="1.0">
  <property name="general" type="empty">
    <property name="workspace_count" type="int" value="1"/>
    <property name="theme" type="string" value="Default-xhdpi"/>
  </property>
</channel>
WM_XML

# 이전 설치의 launcher-*/ 잔재 정리 (시스템 .desktop 참조로 통일했으므로 불필요)
rm -rf "$USER_HOME/.config/xfce4/panel/launcher-"* 2>/dev/null || true

# 커서 테마 xsettings + Xresources — Xvfb 기본 커서 안 보이는 문제 해결
cat > "$PANEL_DIR/xsettings.xml" <<'XSETTINGS_XML'
<?xml version="1.1" encoding="UTF-8"?>
<channel name="xsettings" version="1.0">
  <property name="Net" type="empty">
    <property name="ThemeName" type="string" value="Adwaita-dark"/>
    <property name="IconThemeName" type="string" value="Adwaita"/>
    <property name="DoubleClickTime" type="int" value="400"/>
  </property>
  <property name="Gtk" type="empty">
    <property name="CursorThemeName" type="string" value="Adwaita"/>
    <property name="CursorThemeSize" type="int" value="24"/>
    <property name="FontName" type="string" value="Noto Sans CJK KR 10"/>
    <property name="MonospaceFontName" type="string" value="Nanum Gothic Coding 10"/>
  </property>
</channel>
XSETTINGS_XML

# 세션 시작 시 커서 즉시 적용 (xfce 가 xsettings 못 먹이는 edge case 대비)
cat > "$USER_HOME/.Xresources" <<'XRES'
Xcursor.theme: Adwaita
Xcursor.size: 24
XRES
if ! grep -q "Xcursor" "$USER_HOME/.xprofile" 2>/dev/null; then
  cat >> "$USER_HOME/.xprofile" <<'XPROF_CURSOR'
xrdb -merge "$HOME/.Xresources" 2>/dev/null
xsetroot -cursor_name left_ptr 2>/dev/null
XPROF_CURSOR
fi

chown -R "$XDESKTOP_USER:$XDESKTOP_USER" "$USER_HOME/Desktop" "$USER_HOME/.config"

step 8/9 "nginx HTTP/1.1 bridge + Xpra systemd 서비스"
# Xpra 내장 Python HTTP 서버는 HTTP/1.0 응답 + 동시 TCP burst 에 TCP RST 로 reject.
# → Safari 등 HTTP/2 multistream 클라이언트가 502 연쇄 (traefik 이 RST 받음).
# 해결: Xpra 는 127.0.0.1:$((PORT+1)) 에 바인드, 외부 :$PORT 는 nginx 가 받아 HTTP/1.1 로 변환 + keepalive pool 로 Xpra 에 중계.
apt-get install -y --no-install-recommends nginx 2>&1 | tail -1
rm -f /etc/nginx/sites-enabled/default
cat > /etc/nginx/sites-enabled/xdesktop <<NGX_EOF
upstream xpra_backend {
  server 127.0.0.1:$((XPRA_PORT + 1));
  keepalive 32;
}
server {
  listen $XPRA_PORT default_server;
  server_name _;
  location / {
    proxy_pass http://xpra_backend;
    proxy_http_version 1.1;
    proxy_set_header Connection "";
    proxy_set_header Upgrade \$http_upgrade;
    proxy_set_header Host \$host;
    proxy_set_header X-Real-IP \$remote_addr;
    proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
    proxy_read_timeout 3600s;
    proxy_send_timeout 3600s;
    proxy_buffering off;
  }
}
NGX_EOF
nginx -t
systemctl enable --now nginx
systemctl reload nginx

# xpra-server 패키지가 설치하는 스톡 socket/service 비활성화 (포트 충돌 방지).
# 우리 커스텀 xpra-xdesktop.service 가 XPRA_PORT 를 직접 바인딩함.
systemctl stop xpra-server.socket 2>/dev/null || true
systemctl disable xpra-server.socket 2>/dev/null || true
systemctl stop xpra-server.service 2>/dev/null || true
systemctl disable xpra-server.service 2>/dev/null || true

# 기존 xdesktop 세션 정리 (재설치 대비)
systemctl stop xpra-xdesktop.service 2>/dev/null || true
sleep 1
# 고아 프로세스 제거 — 정확한 유저 + 명령어 매칭만
pgrep --exact-cmd -u "$XDESKTOP_USER" xpra | xargs -r kill -TERM 2>/dev/null || true
pgrep --exact-cmd -u "$XDESKTOP_USER" Xvfb | xargs -r kill -TERM 2>/dev/null || true
rm -rf "$USER_HOME/.xpra" 2>/dev/null || true

cat > /etc/systemd/system/xpra-xdesktop.service <<SVC
[Unit]
Description=Xpra desktop session for $XDESKTOP_USER
After=network.target

[Service]
Type=simple
User=$XDESKTOP_USER
Group=$XDESKTOP_USER
WorkingDirectory=$USER_HOME
Environment=HOME=$USER_HOME
Environment=LANG=ko_KR.UTF-8
Environment=LC_ALL=ko_KR.UTF-8
Environment=GTK_IM_MODULE=fcitx
Environment=QT_IM_MODULE=fcitx
Environment=XMODIFIERS=@im=fcitx
Environment=XDG_RUNTIME_DIR=/tmp/xpra-runtime-$XDESKTOP_USER
# XDG_RUNTIME_DIR — 표준 /run/user/UID 는 systemd-logind 세션 없인 생성 안 됨.
# root 권한으로 생성 후 xuser 소유로 변경.
ExecStartPre=+/bin/install -d -o $XDESKTOP_USER -g $XDESKTOP_USER -m 0700 /tmp/xpra-runtime-$XDESKTOP_USER
ExecStart=/usr/bin/xpra start-desktop $XPRA_DISPLAY \\
  --bind-tcp=127.0.0.1:$((XPRA_PORT + 1)) \\
  --html=on \\
  --start=xfce4-session \\
  --resize-display=1920x1080 \\
  --daemon=no \\
  --mdns=no \\
  --notifications=yes \\
  --bell=no \\
  --pulseaudio=no \\
  --webcam=no \\
  --printing=no \\
  --exit-with-children=no
ExecStop=/usr/bin/xpra stop $XPRA_DISPLAY
Restart=on-failure
RestartSec=5
# Xpra 내부 파일
RuntimeDirectory=xpra-xdesktop

[Install]
WantedBy=multi-user.target
SVC

systemctl daemon-reload
systemctl enable xpra-xdesktop.service
systemctl restart xpra-xdesktop.service

# Xpra HTML5 클라이언트 패치 — 2가지 이슈 해결:
#  1) start-desktop 모드에서 .windowhead (Xpra 자체 타이틀바) 가 루트창 위에 중복 렌더.
#     update_offsets() 가 header.css("height") 를 topoffset 에 더함 → canvas 클릭/드래그
#     좌표가 실제 서버 좌표보다 topoffset 만큼 **밀림**. CSS display:none 만으론 불충분
#     (jQuery .css("height") 가 "auto" 반환 → NaN). JS 패치 병행 필요.
#  2) 브라우저 기본 커서 + Xpra 서버 렌더 커서 = 더블 커서.
XPRA_CSS=/usr/share/xpra/www/css/client.css
XPRA_JS=/usr/share/xpra/www/js/Window.js

# CSS override (멱등)
if [ -f "$XPRA_CSS" ] && ! grep -q "xpra-pxi-overrides" "$XPRA_CSS"; then
  cat >> "$XPRA_CSS" <<'XPRA_OVERRIDES'

/* xpra-pxi-overrides (pxi-xdesktop)
 * start-desktop 루트창의 Xpra 자체 타이틀바 숨김 (JS 패치와 병행 — 아래 Window.js 참조). */
.windowhead { display: none !important; height: 0 !important; min-height: 0 !important; }
.window { border: none !important; border-radius: 0 !important; }

/* 더블 커서 방지 — Xpra 창 내부에선 브라우저 커서 숨김 (서버 Adwaita 커서만 표시) */
div.window, div.window canvas { cursor: none !important; }
XPRA_OVERRIDES
fi

# Window.js 패치 — server_is_desktop|shadow 인 창에선 add_headerbar() 를 건너뛰고
# decorated=false 로 고정. update_offsets() 가 topoffset 에 헤더 높이 더하는 경로 차단.
if [ -f "$XPRA_JS" ] && ! grep -q "server_is_desktop||this.client.server_is_shadow?(this.decorated" "$XPRA_JS"; then
  python3 - "$XPRA_JS" <<'PY'
import sys, pathlib
p = pathlib.Path(sys.argv[1])
s = p.read_text()
orig = "this.configure_border_class(),this.add_headerbar(),this.make_draggable()"
patched = "this.configure_border_class(),(this.client.server_is_desktop||this.client.server_is_shadow?(this.decorated=!1,this.decorations=!1):this.add_headerbar()),this.make_draggable()"
if orig in s:
    # 백업
    if not (p.parent / "Window.js.orig").exists():
        (p.parent / "Window.js.orig").write_text(s)
    p.write_text(s.replace(orig, patched, 1))
    print("Window.js patched")
else:
    print("Window.js 원본 시그니처 불일치 — 이미 패치됐거나 xpra-html5 버전 다름 (수동 확인 필요)")
PY
fi

# Xpra HTML5 기본 index.html → 자동접속 리다이렉터.
# 원본 폼에 비어있는 password 필드가 있어 UX가 혼란 → autoconnect=true 로 바로 연결.
# 서버 쪽 auth 는 이미 비활성 (--tcp-auth 없음) 이라 실제 인증은 필요 없음.
if [ -f /usr/share/xpra/www/index.html ] && [ ! -f /usr/share/xpra/www/index.html.orig ]; then
  cp /usr/share/xpra/www/index.html /usr/share/xpra/www/index.html.orig
fi
cat > /usr/share/xpra/www/index.html <<'HTML_INDEX'
<!DOCTYPE html>
<html lang="ko">
<head>
  <meta charset="utf-8">
  <title>xdesktop</title>
  <meta http-equiv="refresh" content="0; url=/connect.html?autoconnect=true&password=&username=&server=">
  <script>location.replace("/connect.html?autoconnect=true&password=&username=&server=" + encodeURIComponent(location.host));</script>
</head>
<body>
  <p>연결 중…</p>
</body>
</html>
HTML_INDEX

# 기동 대기
sleep 3
systemctl --no-pager --lines=8 status xpra-xdesktop.service || true

step 9/9 "완료"
cat <<DONE

======================================================================
 xdesktop 설치 완료

  유저:       $XDESKTOP_USER  (NOPASSWD sudo)
  Display:    $XPRA_DISPLAY
  HTML5:      http://<LXC_IP>:$XPRA_PORT/
  로케일:     ko_KR.UTF-8
  입력기:     fcitx5 + fcitx5-hangul (기본 한영: Ctrl+Space)
  앱:         Helium, XFCE4, 터미널, Thunar 파일매니저
  폰트:       Noto CJK, Nanum, Nanum Coding

  설치 버전:
    xpra:     $(dpkg-query -W -f='\${Version}' xpra 2>/dev/null)
    helium:   $HELIUM_TAG
    fcitx5:   $(dpkg-query -W -f='\${Version}' fcitx5 2>/dev/null)
======================================================================
DONE
