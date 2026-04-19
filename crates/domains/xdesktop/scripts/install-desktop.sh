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

# 자동 실행 (선택사항 — 세션 열면 helium 바로 뜸)
cp "$USER_HOME/Desktop/Helium.desktop" "$USER_HOME/.config/autostart/Helium.desktop"

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
