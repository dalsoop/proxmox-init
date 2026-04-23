# proxmox-init (pxi)

[![Version](https://img.shields.io/github/v/release/dalsoop/proxmox-init)](https://github.com/dalsoop/proxmox-init/releases)
[![Domains](https://img.shields.io/badge/domains-31-blueviolet)]()
[![Commands](https://img.shields.io/badge/commands-309+-blue)]()
[![License](https://img.shields.io/badge/license-MIT-lightgrey)]()

> Proxmox/LXC/Debian 서버용 **도메인 기반 설치형 CLI**.
> 30개 도메인이 독립 바이너리로 배포됩니다. 필요한 것만 설치.
> Nickel SSOT + 3-tier runtime (fs → embedded → hard-fail) 로 drift 차단.

```bash
curl -fsSL https://install.prelik.com | bash
pxi init
```

## 사용법

```bash
pxi install elk telegram wordpress    # 도메인 설치
pxi run elk status                    # 도메인 실행
pxi run telegram send --bot ops --chat 123 --text "배포 완료"
pxi list                              # 설치된 도메인
pxi available                         # 사용 가능한 도메인 (SSOT 기반)
pxi doctor                            # 기본 상태 점검
pxi validate                          # SSOT ↔ 설치 바이너리 drift 점검
```

## 도메인 (31개)

| 도메인 | 설명 |
|---|---|
| `account` | 리눅스 계정 + Proxmox RBAC (roles, proxmox-silo) |
| `ai` | Claude/Codex CLI, mount/perm-max, OpenClaw, ComfyUI |
| `backup` | vzdump 기반 LXC/VM 백업 + 스케줄 |
| `bootstrap` | apt/rust/gh/dotenvx 의존성 설치 |
| `chrome-browser-dev` | Chromium/Helium 브라우저 빌드 LXC + 캐시/로그/스냅샷 관리 |
| `cloudflare` | DNS / Email Routing / SSL / Pages |
| `code-server` | code-server (VS Code 웹) LXC |
| `comfyui` | ComfyUI LXC 설치 (GPU 패스스루) |
| `connect` | 외부 서비스 연결 (.env + dotenvx) |
| `deploy` | 레시피 기반 LXC 배포 (Homelable, Formbricks 등) |
| `elk` | ELK 스택 (Elasticsearch + Kibana + Logstash) |
| `host` | 호스트 bootstrap, monitor, postfix-relay |
| `infisical` | Infisical 시크릿 플랫폼 |
| `iso` | Proxmox ISO 스토리지 관리 |
| `license` | Keygen CE 라이선스 관리 |
| `lxc` | LXC lifecycle + bootstrap + route-audit |
| `mail` | Maddy + Postfix relay |
| `ministack` | LocalStack AWS 에뮬레이터 |
| `monitor` | 리소스 모니터링 + health-check |
| `nas` | NAS 마운트 + Synology/TrueNAS API |
| `net` | 네트워크 진단 (audit, fix, ingress) |
| `node` | Proxmox 클러스터 노드 관리 |
| `recovery` | LXC config 스냅샷/복원 |
| `service` | 파일 기반 서비스 레지스트리 |
| `telegram` | 텔레그램 봇 (send, webhook, generate) |
| `traefik` | Traefik 리버스 프록시 관리 |
| `vaultwarden` | Vaultwarden 패스워드 매니저 LXC |
| `vm` | Proxmox VM lifecycle |
| `wordpress` | WordPress LXC 배포 |
| `workspace` | tmux + shell + nvim |
| `xdesktop` | Xpra HTML5 원격 데스크톱 (한글 + Helium) |

## 프리셋

```bash
pxi install --preset web      # bootstrap, lxc, traefik, cloudflare
pxi install --preset mail     # bootstrap, lxc, mail, cloudflare, connect
pxi install --preset dev      # bootstrap, ai, connect
```

## 서비스 레지스트리

```bash
pxi run service list                    # 도메인별 서비스 목록
pxi run service add --domain prelik.com --name blog --host blog.prelik.com --ip 10.0.50.200 --port 80
pxi run service sync                    # Traefik 자동 동기화
```

## Chromium/Helium 브라우저 개발

`chrome-browser-dev` 도메인은 대형 Chromium 빌드에서 의존성/캐시가 깨지지 않도록
LXC, 캐시, 빌드 로그, TOML 워크플로우 카드, 빌드 프로파일을 한 곳에서 관리합니다.

```bash
pxi chrome-browser-dev setup --vmid 50220 --hostname chromium-browser-dev --ip 10.0.50.220/16
pxi chrome-browser-dev init-workspace --vmid 50220
pxi chrome-browser-dev workflow-new --vmid 50220 --id tabs-refactor --goal "탭 동작 수정" \
  --patch helium/core/tab-cycling-mru.patch \
  --path chrome/browser/ui/browser_command_controller.cc \
  --verify "chromium-browser-dev build"
pxi chrome-browser-dev workflow-check --vmid 50220 --id tabs-refactor
pxi chrome-browser-dev gitlab-setup --vmid 50220 --host gitlab.internal.kr
pxi chrome-browser-dev x11-setup --vmid 50220
pxi chrome-browser-dev x11-simulate --vmid 50220
pxi chrome-browser-dev profile-show --vmid 50220
pxi chrome-browser-dev check --vmid 50220
pxi chrome-browser-dev build-status --vmid 50220
pxi chrome-browser-dev version --vmid 50220
pxi chrome-browser-dev paths --vmid 50220
pxi chrome-browser-dev run --vmid 50220 --url about:blank --timeout-sec 10
pxi chrome-browser-dev driver-start --vmid 50220 --port 9515
pxi chrome-browser-dev card-assumptions --vmid 50220
pxi chrome-browser-dev card-check --vmid 50220
```

## SSOT (Nickel)

29개 도메인 메타데이터는 `ncl/` 하위 Nickel 파일이 정본. 파일시스템 `locale.json` (
`/var/lib/pxi/locale.json` 또는 `~/.local/share/pxi/locale.json`) 과 바이너리에 embed 된
런타임 fallback 을 통해 nickel CLI 없는 환경에서도 SSOT 일관성 유지.

```
ncl/contracts/domain.ncl         # Domain record contract (NameStr / Product / Layer / Platform enum)
ncl/domains.ncl                  # 30개 domain.ncl import 인덱스 + requires 교차검증 + format_version
crates/domains/<name>/domain.ncl # 각 도메인 메타 (name/description/tags/requires?/provides)
```

### 로드 우선순위 (Registry::load)

1. 파일시스템 `locale.json` — install-local.sh / release tarball 이 배치, 수정 가능
2. 바이너리 embedded — build.rs 가 nickel export 로 구워넣은 immutable SSOT
3. hard-fail — 명확한 복구 안내

tier 1 이 있지만 format_version 미일치 면 tier 2 로 silent downgrade **하지 않음** (drift 방지).

## 새 도메인 추가

```bash
scripts/new-domain.sh <name> <product> <layer> <platform> "<설명>"

# 예시
scripts/new-domain.sh vaultwarden service remote proxmox \
  "Vaultwarden 패스워드 매니저 LXC"
```

4개 파일 원자적 생성:
- `crates/domains/<name>/Cargo.toml` (workspace inherit)
- `crates/domains/<name>/src/main.rs` (clap skeleton)
- `crates/domains/<name>/domain.ncl` (Domain contract 적용)
- `ncl/domains.ncl` (알파벳 순 import 삽입)

생성 직후 `nickel eval` + `cargo check` 자동 실행.

## 로컬 개발 흐름

```bash
# 소스 수정 후 로컬 설치 (nickel 필요)
scripts/install-local.sh              # pxi 메타만
scripts/install-local.sh --all        # 30개 도메인 전부
scripts/install-local.sh lxc traefik  # 특정 도메인만

# drift 점검
pxi validate                          # SSOT ↔ 바이너리 대조
```

## 이름 변경

```bash
pxi rebrand newname --apply    # 바이너리 + 경로 일괄 변경
./scripts/rebrand.sh pxi newname && cargo build --release  # 소스 전체
```

## 라이선스

MIT
