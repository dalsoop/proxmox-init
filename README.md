# prelik-init

Proxmox/LXC/Debian 서버용 도메인 기반 설치형 CLI.
mac-app-init의 Linux 서버판. Rust workspace + Nickel 스키마.

## 빠른 설치

```bash
curl -fsSL https://install.prelik.com | bash
prelik setup
prelik install bootstrap
```

## 사용

```bash
prelik available           # 설치 가능 도메인
prelik install <domain>    # GitHub Release에서 바이너리 다운
prelik list                # 설치된 도메인
prelik run <domain> <...>  # 도메인 커맨드 실행
prelik doctor              # 상태 점검
```

## 도메인 (v0.4.0)

| 도메인 | 역할 | 주요 커맨드 |
|--------|------|------------|
| **bootstrap** | 의존성 설치 | install, doctor |
| **lxc** | Proxmox LXC 수명관리 | list, create, delete, enter, backup |
| **traefik** | 리버스 프록시 | recreate, route-add/list/remove |
| **mail** | Maddy + Mailpit + Postfix relay | install-mailpit, postfix-relay |
| **cloudflare** | CF DNS/Email Worker | dns-add, email-worker-attach-all |
| **connect** | .env + dotenvx | set, remove, list, encrypt |
| **ai** | Claude/Codex + 플러그인 | install, octopus/superpowers/codex-plugin |
| **manager** | (내장) | prelik CLI 자체 |

## 구조

```
prelik-init/
├── crates/
│   ├── core/         # 공통 (os, paths, dotenvx, github, systemd, registry)
│   ├── cli/          # `prelik` 진입점
│   └── domains/      # 독립 바이너리 × 8
│       ├── bootstrap/
│       ├── connect/
│       ├── lxc/
│       ├── traefik/
│       ├── mail/
│       ├── cloudflare/
│       ├── ai/
│       └── manager/
├── ncl/              # Nickel 스키마 + 도메인 레지스트리 (SSOT)
├── workers/          # Cloudflare Worker (install.prelik.com)
├── install.sh
└── .github/workflows/release.yml
```

## 권한 모델

- user 실행 기본, sudo 에스컬레이션은 필요 시
- `is_root()` 감지로 `/etc/prelik` vs `~/.config/prelik` 자동 분기

## Secret 저장

- `.env` / `.env.vault` / `.env.keys` (dotenvx)
- `.env.keys`는 절대 git 배제

## 예시

```bash
# 한 방 세팅 (호스트)
prelik install bootstrap
prelik install lxc traefik mail cloudflare connect ai

# LXC 생성
prelik run lxc create --vmid 50200 --hostname myapp --ip 10.0.50.200/16

# Traefik 라우트
prelik run traefik route-add --vmid 100 --name myapp \
  --domain myapp.example.com --backend http://10.0.50.200:80 --use-cf

# CF DNS (audience 기반 proxied 자동)
prelik run cloudflare dns-add --domain example.com --type A \
  --name myapp --content 203.0.113.1 --audience kr

# 메일 발송 경로 (호스트 Postfix → Maddy)
prelik run mail postfix-relay --maddy-ip 10.0.50.122

# 메일 수신 아카이브 (Mailpit)
prelik run mail install-mailpit --vmid 124

# AI 플러그인
prelik run ai octopus-install
prelik run ai codex-plugin-install --fork
prelik run ai adversarial-review-hook
```

## 릴리스

- [v0.4.0](https://github.com/dalsoop/prelik-init/releases/tag/v0.4.0) — 8개 도메인 완성
- [v0.3.0](https://github.com/dalsoop/prelik-init/releases/tag/v0.3.0) — traefik, mail, cloudflare, connect
- [v0.2.0](https://github.com/dalsoop/prelik-init/releases/tag/v0.2.0) — lxc + nickel runtime
- [v0.1.0](https://github.com/dalsoop/prelik-init/releases/tag/v0.1.0) — 초기 스캐폴딩

## 라이선스

MIT
