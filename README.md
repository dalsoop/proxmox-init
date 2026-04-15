# prelik-init

Proxmox/LXC 도메인 기반 설치형 CLI. mac-app-init의 Linux 서버판.

## 설치

```bash
curl -fsSL https://raw.githubusercontent.com/dalsoop/prelik-init/main/install.sh | bash
prelik setup
prelik install bootstrap
```

## 사용

```bash
prelik available            # 설치 가능 도메인
prelik install <domain>     # GitHub Release에서 도메인 바이너리 다운
prelik list                 # 설치된 도메인
prelik run <domain> <...>   # 도메인 실행
prelik doctor               # 상태 점검
```

## 도메인

| 도메인 | 설명 | 상태 |
|--------|------|------|
| bootstrap | apt/rust/gh/dotenvx | v0.1 |
| connect | .env + dotenvx 연결 관리 | placeholder |
| manager | 도메인 매니저 | placeholder |
| lxc | Proxmox pct 래퍼 | 예정 |
| traefik | 리버스 프록시 | 예정 |
| mail | maddy + mailpit | 예정 |
| cloudflare | CF DNS/Worker | 예정 |
| ai | Claude/Codex 플러그인 | 예정 |

## 구조

```
prelik-init/
├── crates/
│   ├── core/          # 공통 (os, paths, dotenvx, github, systemd, config)
│   ├── cli/           # `prelik` 진입점
│   └── domains/       # 도메인별 독립 바이너리
│       ├── bootstrap/
│       ├── connect/
│       └── manager/
├── ncl/               # Nickel 도메인 레지스트리
├── install.sh
└── .github/workflows/
```

## 권한 모델

- user 실행 기본, 시스템 경로 쓸 때만 sudo 에스컬레이션
- `is_root()` → `/etc/prelik` vs `~/.config/prelik` 자동 결정

## Secret

- `.env` / `.env.vault` / `.env.keys` — dotenvx 규약
- `.env.keys`는 절대 git에 안 올라감
