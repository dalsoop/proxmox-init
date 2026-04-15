# prelik-init

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
prelik available          # 가능한 것들 보기
prelik install bootstrap  # 필수 의존성
prelik install lxc traefik mail cloudflare connect ai
```

### 4. 사용

```bash
prelik run lxc create --vmid 200 --hostname myapp --ip 10.0.50.200
prelik run traefik recreate --vmid 100
prelik run cloudflare dns-add --domain example.com --type A --name myapp --content 1.2.3.4 --audience kr
prelik run mail install-mailpit --vmid 124
prelik run ai codex-plugin-install --fork
prelik doctor
```

---

## 도메인

| 도메인 | 기능 | 주요 커맨드 |
|--------|------|------------|
| **bootstrap** | apt/rust/gh/dotenvx/nickel 의존성 | `install`, `doctor` |
| **lxc** | Proxmox LXC 수명관리 | `list`, `create`, `delete`, `backup`, `enter` |
| **traefik** | 리버스 프록시 + compose 재생성 | `recreate`, `route-add`, `route-list` |
| **mail** | Maddy + Mailpit + Postfix relay | `install-mailpit`, `postfix-relay` |
| **cloudflare** | DNS (audience 기반) + Email Worker | `dns-add`, `email-worker-attach-all` |
| **connect** | .env + dotenvx 암호화 | `set`, `list`, `encrypt` |
| **ai** | Claude/Codex CLI + 플러그인 | `install`, `octopus`, `superpowers`, `codex-plugin` |

---

## 실전 예시

- [examples/formbricks.md](examples/formbricks.md) — Formbricks 설문조사 + Traefik + CF

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
