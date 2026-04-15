# 도메인 메타데이터 스키마 + 레지스트리 (Single Source of Truth)

let Domain = {
  name | String,
  description | String,
  enabled | Bool | default = true,
  tags | {
    product | [| 'init, 'infra, 'auto, 'mail, 'ai |],
    layer | [| 'platform, 'remote, 'local, 'tool |],
    platform | [| 'debian, 'ubuntu, 'proxmox, 'any |] | default = 'debian,
  },
  requires | Array String | default = [],
  provides | Array String | default = [],
}
in

{
  domains = {
    bootstrap = {
      name = "bootstrap",
      description = "apt/rust/gh/dotenvx 의존성 설치",
      tags = { product = 'init, layer = 'platform, platform = 'debian },
      provides = ["bootstrap install", "bootstrap doctor"],
    } | Domain,

    connect = {
      name = "connect",
      description = "외부 서비스 연결 관리 (.env + dotenvx)",
      tags = { product = 'init, layer = 'tool, platform = 'any },
      requires = ["bootstrap"],
      provides = ["connect add", "connect remove", "connect list"],
    } | Domain,

    manager = {
      name = "manager",
      description = "도메인 설치/업데이트 매니저 (prelik CLI 내부)",
      tags = { product = 'init, layer = 'platform, platform = 'any },
      provides = ["install", "remove", "update", "list"],
    } | Domain,

    lxc = {
      name = "lxc",
      description = "LXC 수명 관리 (Proxmox pct 래퍼)",
      enabled = true,
      tags = { product = 'infra, layer = 'remote, platform = 'proxmox },
      provides = ["lxc create", "lxc delete", "lxc backup"],
    } | Domain,

    traefik = {
      name = "traefik",
      description = "Traefik 리버스 프록시 + compose 기반 재생성",
      enabled = true,
      tags = { product = 'infra, layer = 'remote, platform = 'debian },
      requires = ["lxc"],
      provides = ["traefik install", "traefik route-add", "traefik recreate"],
    } | Domain,

    mail = {
      name = "mail",
      description = "Maddy + Mailpit + Postfix relay 번들",
      enabled = true,
      tags = { product = 'mail, layer = 'remote, platform = 'debian },
      requires = ["lxc"],
      provides = ["mail install", "mail status"],
    } | Domain,

    cloudflare = {
      name = "cloudflare",
      description = "CF DNS / Email Routing / Worker",
      enabled = true,
      tags = { product = 'infra, layer = 'tool, platform = 'any },
      provides = ["cf dns-add", "cf email-worker-attach"],
    } | Domain,

    ai = {
      name = "ai",
      description = "Claude / Codex / 플러그인 (octopus, superpowers, codex-plugin)",
      enabled = true,
      tags = { product = 'ai, layer = 'tool, platform = 'any },
      provides = ["ai install", "ai octopus-install", "ai codex-plugin-install"],
    } | Domain,
  },
}
