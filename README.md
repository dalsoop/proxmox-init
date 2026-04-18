# pxi-init

[![Version](https://img.shields.io/badge/version-1.9.11-blue)](https://github.com/dalsoop/pxi-init/releases/tag/v1.9.11)
[![Codex Reviews](https://img.shields.io/badge/codex--reviewed-71x-green)]()
[![Domains](https://img.shields.io/badge/domains-21-blueviolet)]()
[![Tests](https://img.shields.io/badge/unit--tests-84-orange)]()
[![License](https://img.shields.io/badge/license-MIT-lightgrey)]()

> Proxmox/LXC/Debian 서버용 **도메인 기반 설치형 CLI**.
> 하나의 거대 바이너리가 아닌, 18개 도메인이 독립 바이너리로 배포됩니다.

```bash
curl -fsSL https://install.prelik.com | bash
prelik init
```

## Demo

```
$ prelik run monitor host
=== 호스트 리소스 ===

[CPU] (64 cores)
  load avg: 11.13 11.36 10.34 (1/5/15min)

[메모리]
  RAM:  134GB / 503GB (26%)
  Swap: 4GB / 7GB (61%)

[디스크]
  /                        79G / 94G (89%) /dev/mapper/pve-root

[uptime] up 3 weeks, 1 day

$ prelik run node --json list
[
  {"node": "pve",           "status": "online", "cpus": 64, "mem_total_gb": 503, "uptime_days": 22},
  {"node": "ranode-3960x",  "status": "online", "cpus": 48, "mem_total_gb": 62,  "uptime_days": 31}
]

$ prelik run deploy service nginx --vmid 200 --hostname myapp --ip 10.0.50.200
[1/3] LXC 생성 → pxi-lxc create
[2/3] 패키지 설치: nginx, curl
[3/3] 커스텀 스크립트 (3 단계)
✓ nginx 배포 완료 (VMID 200, IP 10.0.50.200)

$ curl http://10.0.50.200/
<h1>Deployed via pxi-deploy</h1>
```

---

## 왜?

기존 "인프라 관리 CLI"들은 모든 기능을 한 바이너리에 우겨넣어:
- 바이너리 크기 / 빌드 시간 증가
- 안 쓰는 기능까지 같이 설치
- 한 도메인의 버그가 전체에 영향

**pxi-init은 도메인별 독립 바이너리**:
- 필요한 것만 설치/제거/업데이트
- 도메인마다 자체 `doctor` (의존성 점검)
- Nickel (`ncl/domains.ncl`)이 SSOT — CLI는 자동 감지

---

## 빠른 시작

### 1. 설치
```bash
curl -fsSL https://install.prelik.com | bash
```

### 2. 초기 세팅
```bash
prelik init        # 인터랙티브 — CF/SMTP/Network 입력
prelik available   # 가능한 도메인 18개
prelik doctor      # 환경 점검
```

### 3. 프리셋 또는 개별 설치
```bash
prelik install --preset web          # bootstrap + lxc + traefik + cloudflare
prelik install --preset mail         # bootstrap + lxc + mail + cloudflare + connect
prelik install bootstrap lxc traefik # 공백으로 여러 개
```

프리셋: `web` / `mail` / `dev` / `minimal`

### 4. 사용
```bash
prelik run lxc create --vmid 200 --hostname myapp --ip 10.0.50.200
prelik run iso download debian-13.iso --url https://... --storage local
prelik run cloudflare dns-add --domain example.com --type A --name myapp --content 1.2.3.4 --audience kr
prelik run monitor all
prelik run deploy service --recipe nginx.toml
```

---

## 도메인 카탈로그 (18개)

### 🏗 platform — 기본 인프라
| 도메인 | 기능 | 주요 커맨드 |
|--------|------|------------|
| **bootstrap** | 의존성 (apt/rust/gh/dotenvx/nickel) | `install [--only X]`, `remove`, `list`, `doctor` |
| **host** | 호스트 시스템 관리 | `status`, `monitor`, `ssh-keygen`, `smb-open/close` |
| **account** | 리눅스 계정 관리 | `create`, `remove`, `list`, `ssh-key-add` |
| **nas** | SMB/NFS 마운트 (cifs-credentials 분리) | `mount`, `unmount`, `list` |
| **workspace** | tmux + shell alias | `tmux-setup`, `shell-setup`, `status` |

### 🖧 proxmox — 가상화 인프라
| 도메인 | 기능 | 주요 커맨드 |
|--------|------|------------|
| **lxc** | LXC 수명관리 (pct 래퍼) | `create`, `delete`, `enter`, `snapshot-*`, `resize` |
| **vm** | QEMU VM 수명관리 (qm 래퍼) | `start/stop/reboot`, `backup`, `resize` |
| **backup** | vzdump 기반 백업 + 스케줄 | `now`, `list`, `schedule-add`, `restore` |
| **iso** | ISO 스토리지 + 파일 관리 | `list`, `storage-add-nfs/cifs`, `download`, `remove` |
| **deploy** | TOML 레시피 → LXC 자동 배포 | `service`, `list-recipes` |
| **monitor** | 호스트/LXC/VM 리소스 (read-only) | `host`, `lxc`, `vm`, `all` |

### 🌐 network — 네트워크/외부 연동
| 도메인 | 기능 | 주요 커맨드 |
|--------|------|------------|
| **traefik** | Traefik 리버스 프록시 | `recreate`, `route-add`, `route-list` |
| **cloudflare** | DNS CRUD + Email Worker + Pages + SSL | `dns-add/list/update/delete`, `pages-deploy`, `ssl-issue` |
| **mail** | Maddy + Mailpit + Postfix relay | `install-mailpit`, `postfix-relay` |
| **connect** | .env + dotenvx 암호화 | `set`, `list`, `encrypt` |
| **telegram** | 봇 관리 + 발송 | `register`, `send`, `verify`, `remove` |

### 🤖 ai — AI/특수 워크로드
| 도메인 | 기능 | 주요 커맨드 |
|--------|------|------------|
| **ai** | Claude/Codex CLI + 플러그인 | `install`, `octopus-install`, `superpowers-install`, `codex-plugin-install` |
| **comfyui** | GPU LXC + ComfyUI 자동 설치 | `install`, `gpu-passthrough`, `status` |

각 도메인 사용법:
```bash
prelik run <domain> --help
prelik run <domain> doctor       # 의존성 점검 (모두 graceful exit 0)
```

---

## 안정성

- **26차 Codex 어드버서리얼 리뷰** 통과 (P0/P1 35건+ 수정)
- 전 도메인 `doctor` graceful (CI smoke 호환)
- `common::run_secret()` — `--password` 등 비밀 argv를 anyhow 체인에서 마스킹
- `common::has_cmd()` — 외부 `which` 바이너리 의존 없음
- SMB credentials는 `/etc/cifs-credentials/` 분리 (0600 root:root)
- fstab은 `tee -a` append-only + EOF 개행 검증
- postfix relay는 `/etc/postfix/pxi-backup-<ns-ts>/` 백업 + 자동 롤백

---

## 실전 예시

- [examples/formbricks.md](examples/formbricks.md) — Formbricks + Traefik + CF
- [examples/recipes/nginx.toml](examples/recipes/nginx.toml) — deploy 레시피

## 제거

```bash
prelik uninstall              # dry-run (어떤 파일이 지워지는지 미리 확인)
prelik uninstall --confirm    # 바이너리만 제거
prelik uninstall --confirm --purge   # config/recovery/audit까지 삭제
```

⚠️ **prelik이 만든 LXC/VM, NAS 마운트, postfix relay, Cloudflare DNS 등은 자동으로 안 지웁니다.**
완전 청소 절차: [docs/uninstall.md](docs/uninstall.md)

## phs와 비교

pxi-init은 dalsoop의 내부 도구 phs를 일반화·도려낸 OSS 서브셋입니다.
[docs/phs-vs-prelik.md](docs/phs-vs-prelik.md)

---

## 설계 원칙

| 원칙 | 구체 |
|------|------|
| **도메인 = 독립 바이너리** | 자기 책임 완결, 최소 의존성 |
| **Nickel SSOT** | `ncl/domains.ncl`이 레지스트리, 런타임 export |
| **Doctor 일관성** | 누락 의존성은 보고만, 종료 코드 0 (CI 친화) |
| **Secret 마스킹** | 비밀 argv는 `run_secret`으로 anyhow에 미노출 |
| **Install 채널** | `install.prelik.com` → GitHub Release 리다이렉트 (CF Worker) |

---

## 구조

```
pxi-init/
├── crates/
│   ├── core/         # 공통: paths, config, common(run/run_secret/has_cmd), registry
│   ├── cli/          # `prelik` 진입점 (도메인 자동 감지)
│   └── domains/      # 독립 바이너리 × 18
├── ncl/              # Nickel 레지스트리 (SSOT)
├── workers/          # Cloudflare Worker (install.prelik.com)
├── examples/         # 실전 사용 예시 + recipes
├── tests/smoke.sh    # 전 도메인 --help/doctor smoke
└── install.sh
```

---

## 릴리스 (v1.x)

| 버전 | 주요 변경 |
|------|----------|
| [v1.5.2](https://github.com/dalsoop/pxi-init/releases/tag/v1.5.2) | 안정성 스윕 — `run_secret`/`has_cmd` 통합 |
| [v1.5.1](https://github.com/dalsoop/pxi-init/releases/tag/v1.5.1) | monitor/iso 외부 `which` 바이너리 의존 제거 |
| [v1.5.0](https://github.com/dalsoop/pxi-init/releases/tag/v1.5.0) | **monitor** 도메인 (호스트/LXC/VM read-only) |
| [v1.4.1](https://github.com/dalsoop/pxi-init/releases/tag/v1.4.1) | iso SMB 비밀번호 로그 노출 차단 |
| [v1.4.0](https://github.com/dalsoop/pxi-init/releases/tag/v1.4.0) | **iso** 도메인 (Proxmox ISO 스토리지/파일) |
| [v1.3.1](https://github.com/dalsoop/pxi-init/releases/tag/v1.3.1) | deploy의 IP /16 강제 회귀 수정 |
| [v1.3.0](https://github.com/dalsoop/pxi-init/releases/tag/v1.3.0) | **deploy** 도메인 (TOML 레시피) |
| [v1.2.0](https://github.com/dalsoop/pxi-init/releases/tag/v1.2.0) | **backup** + lxc snapshot/resize |
| [v1.1.0](https://github.com/dalsoop/pxi-init/releases/tag/v1.1.0) | **vm** 도메인 |
| [v1.0.0](https://github.com/dalsoop/pxi-init/releases/tag/v1.0.0) | Phase 2 안정화 + install.prelik.com |

전체 변경 이력: [CHANGELOG.md](CHANGELOG.md)

---

## 기여

1. 새 도메인은 `crates/domains/<name>/` 추가 + `Cargo.toml` 작성
2. `ncl/domains.ncl` 레지스트리에 메타데이터
3. `crates/core/src/registry.rs` fallback에 한 줄
4. `tests/smoke.sh` + `.github/workflows/release.yml`에 도메인명 추가
5. CLI는 자동 감지 — 수정 불필요

[CONTRIBUTING.md](CONTRIBUTING.md)

---

## 라이선스

MIT

## 관련 프로젝트

- [mac-app-init](https://github.com/dalsoop/mac-app-init) — macOS용 자매 프로젝트
