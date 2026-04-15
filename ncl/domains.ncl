# 도메인 레지스트리 — 이후 각 도메인 설치 메타데이터로 사용
{
  bootstrap = {
    description = "apt/rust/gh/dotenvx 의존성 설치",
    platforms = ["debian", "ubuntu", "alpine"],
  },
  connect = {
    description = "외부 서비스 연결 관리 (.env + dotenvx)",
    platforms = ["any"],
  },
  lxc = {
    description = "LXC 수명 관리 (Proxmox pct 래퍼)",
    platforms = ["proxmox"],
  },
  traefik = {
    description = "Traefik 리버스 프록시",
    platforms = ["any"],
  },
  mail = {
    description = "Maddy + Mailpit + Postfix relay 번들",
    platforms = ["any"],
  },
  cloudflare = {
    description = "CF DNS/Email Routing/Worker",
    platforms = ["any"],
  },
  ai = {
    description = "Claude/Codex + 플러그인",
    platforms = ["any"],
  },
}
