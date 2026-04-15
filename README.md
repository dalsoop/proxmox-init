# prelik-init

[![Version](https://img.shields.io/badge/version-1.0.0-blue)](https://github.com/dalsoop/prelik-init/releases/tag/v1.0.0)
[![Codex Reviews](https://img.shields.io/badge/codex--reviewed-14x-green)]()
[![License](https://img.shields.io/badge/license-MIT-lightgrey)]()

> Proxmox/LXC/Debian 서버용 **도메인 기반 설치형 CLI**.
> 하나의 바이너리가 아닌, 각 도메인이 독립 바이너리로 배포됩니다.

```bash
curl -fsSL https://install.prelik.com | bash
prelik init
```

---

## 왜?

기존 "인프라 관리 CLI"들은 하나의 거대 바이너리에 모든 기능을 우겨넣어:
- 바이너리 크기 증가
- 빌드 시간 지연
- 쓰지도 않는 기능 같이 설치됨
- 버그 하나가 전체에 영향

**prelik-init은 도메인별 독립 바이너리**로 설계되었습니다:
- `prelik-lxc`: LXC 수명관리
- `prelik-traefik`: 리버스 프록시
- `prelik-mail`: 메일 스택
- `prelik-cloudflare`: CF DNS/Worker
- `prelik-connect`: 시크릿 관리
- `prelik-ai`: Claude/Codex 플러그인

필요한 것만 설치, 업데이트, 제거할 수 있습니다.

---

## 빠른 시작

### 1. 설치

```bash
curl -fsSL https://install.prelik.com | bash
```

### 2. 초기 세팅

```bash
prelik init   # 인터랙티브 — CF/SMTP/Network 입력
```

### 3. 도메인 설치

```bash
prelik available                     # 가능한 것들 보기
prelik install --preset web          # 프리셋으로 한 번에 (bootstrap + lxc + traefik + cloudflare)
prelik install --preset mail         # 메일 스택
prelik install bootstrap lxc traefik # 공백으로 여러 개
```

프리셋:
- `web` — 웹 호스팅 기본 (bootstrap, lxc, traefik, cloudflare)
- `mail` — 메일 스택 (bootstrap, lxc, mail, cloudflare, connect)
- `dev` — 개발 도구 (bootstrap, ai, connect)
- `minimal` — 필수 최소 (bootstrap)

### 4. 사용

```bash
prelik run lxc create --vmid 200 --hostname myapp --ip 10.0.50.200
prelik run traefik recreate --vmid 100
prelik run cloudflare dns-add --domain example.com --type A --name myapp --content 1.2.3.4 --audience kr
prelik run mail install-mailpit --vmid 124
prelik run ai codex-plugin-install --fork
prelik doctor
```

### 도구 단위 설치/제거 (bootstrap)

```bash
prelik run bootstrap list              # 각 도구 상태
prelik run bootstrap install           # 전부 설치
prelik run bootstrap install --only nickel,rust    # 선택 설치
prelik run bootstrap remove  --only nickel         # 선택 제거
```

---

## 도메인

| 도메인 | 기능 | 주요 커맨드 |
|--------|------|------------|
| **account** | 리눅스 계정 관리 | `create`, `remove`, `ssh-key-add` |
| **ai** | Claude/Codex CLI + 플러그인 | `install`, `octopus`, `superpowers`, `codex-plugin` |
| **bootstrap** | 의존성 (apt/rust/gh/dotenvx/nickel) | `install --only`, `remove`, `doctor` |
| **cloudflare** | DNS CRUD + Email Worker | `dns-add/list/update/delete`, `email-worker-attach-all --dry-run` |
| **comfyui** | GPU LXC + ComfyUI 설치 | `install`, `gpu-passthrough`, `status` |
| **connect** | .env + dotenvx 암호화 | `set`, `list`, `encrypt` |
| **host** | 호스트 시스템 관리 | `status`, `monitor`, `ssh-keygen`, `smb-open/close` |
| **lxc** | Proxmox LXC 수명관리 | `list`, `create`, `delete`, `backup`, `enter` |
| **mail** | Maddy + Mailpit + Postfix relay | `install-mailpit`, `postfix-relay` |
| **nas** | SMB/NFS 마운트 | `mount --protocol smb|nfs`, `unmount`, `list` |
| **telegram** | 봇 관리 + 발송 | `register`, `send`, `verify` |
| **traefik** | 리버스 프록시 | `recreate`, `route-add`, `route-list` |
| **workspace** | tmux + shell alias | `tmux-setup`, `shell-setup` |

---

## 실전 예시

- [examples/formbricks.md](examples/formbricks.md) — Formbricks 설문조사 + Traefik + CF

## phs (내부 도구)와 비교

prelik-init은 dalsoop의 내부 도구 phs의 ~25%를 추출한 서브셋입니다.
정확한 동작 차이와 누락 기능: [docs/phs-vs-prelik.md](docs/phs-vs-prelik.md)

---

## 설계 원칙

| 원칙 | 구체 |
|------|------|
| **도메인 = 독립 바이너리** | 각 도메인이 자기 책임 완결, 의존성 최소화 |
| **Nickel SSOT** | `ncl/domains.ncl` 가 레지스트리, 런타임에 export |
| **Install 채널** | `install.prelik.com` → GitHub Release 리다이렉트 |
| **권한 모델** | user 기본, root일 때만 `/etc/prelik` 사용 |
| **Secret** | dotenvx `.env.vault` (키 파일 분리) |

---

## 구조

```
prelik-init/
├── crates/
│   ├── core/         # 공통: os, paths, dotenvx, github, systemd, registry
│   ├── cli/          # `prelik` 진입점
│   └── domains/      # 독립 바이너리 × 8
├── ncl/              # Nickel 레지스트리 (SSOT)
├── workers/          # Cloudflare Worker (install.prelik.com)
├── examples/         # 실전 사용 예시
└── install.sh
```

---

## 릴리스

| 버전 | 주요 변경 |
|------|----------|
| [v0.5.0](https://github.com/dalsoop/prelik-init/releases/tag/v0.5.0) | Codex 보안 리뷰 6개 이슈 수정 |
| [v0.4.0](https://github.com/dalsoop/prelik-init/releases/tag/v0.4.0) | ai 도메인 + install.prelik.com |
| [v0.3.0](https://github.com/dalsoop/prelik-init/releases/tag/v0.3.0) | traefik, mail, cloudflare, connect |
| [v0.2.0](https://github.com/dalsoop/prelik-init/releases/tag/v0.2.0) | lxc + Nickel runtime |
| [v0.1.0](https://github.com/dalsoop/prelik-init/releases/tag/v0.1.0) | 초기 스캐폴딩 |

---

## 기여

1. 새 도메인은 `crates/domains/<name>/` 디렉토리로 추가
2. `domain.ncl` 마커 파일 필수 (build.rs가 감지)
3. `ncl/domains.ncl` 레지스트리에 메타데이터 추가
4. `crates/cli/src/main.rs`는 수정 불필요 — 도메인 자동 감지

[CONTRIBUTING.md](CONTRIBUTING.md) 참조.

---

## 라이선스

MIT License

## 관련 프로젝트

- [mac-app-init](https://github.com/dalsoop/mac-app-init) — macOS용 자매 프로젝트
