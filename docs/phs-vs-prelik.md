# phs ↔ prelik-init 비교 (v1.9.11 기준)

prelik-init은 dalsoop의 내부 도구 **phs (proxmox-host-setup)**에서 공유 가치
있는 부분을 **도메인별 독립 바이너리**로 재설계한 프로젝트입니다.

---

## 핵심 차이

| 항목 | phs | prelik-init |
|------|-----|-------------|
| 아키텍처 | 단일 바이너리 (18MB+) | 도메인별 독립 바이너리 (각 2~4MB) |
| 설치 | `cargo build` from source | `curl install.prelik.com \| bash` + `prelik install <domain>` |
| 설정 | `.env` + control-plane JSON | `~/.config/prelik/config.toml` + Nickel SSOT |
| 범위 | dalsoop 운영 100% | 범용 공유 가능 서브셋 |
| 제거 | 없음 | `prelik uninstall [--purge]` + 10 섹션 가이드 |
| 리뷰 | 없음 | 71차 Codex 어드버서리얼 리뷰 |
| 테스트 | 0 | 84 unit tests + shellcheck CI |

---

## 이식 현황 (21 도메인 + 8 레시피)

### ✅ 완전 이식 (독립 바이너리)

| prelik 도메인 | phs 원본 | 이식 상태 |
|---|---|---|
| **bootstrap** | phs host bootstrap | 완전 + **manifest** 신규 |
| **host** | phs host status/monitor/ssh/smb | 완전 + **self-update** + **gh-auth** 신규 |
| **lxc** | phs infra lxc-* | 완전 + **snapshot-\***, **resize**, **init**, **--json** 신규 |
| **vm** | phs infra vm-* | 완전 + **--json** 신규 |
| **backup** | phs infra backup-* | 완전 |
| **nas** | phs nas mount/unmount/list | 완전 (cifs-credentials 분리 + fstab append 안전) |
| **account** | phs accounts | 완전 |
| **telegram** | phs telegram | 완전 |
| **workspace** | phs workspace | 완전 |
| **traefik** | phs infra traefik-* | 핵심 (recreate/route-add/remove/list) |
| **cloudflare** | phs cloudflare dns/email/ssl/pages | DNS CRUD + Email Worker + SSL issue + Pages deploy |
| **mail** | phs infra mail/host postfix | Mailpit + postfix-relay (자동 백업/롤백) |
| **ai** | phs ai install/octopus/superpowers/codex | 플러그인 설치 중심 |
| **comfyui** | phs ai comfyui-* | GPU LXC + ComfyUI |
| **connect** | phs config (.env) | dotenvx 암호화 관리 |
| **deploy** | phs infra deploy | TOML 레시피 기반 LXC 배포 |
| **iso** | phs infra iso-storage | NFS/CIFS ISO 스토리지 + 파일 + **--json** 신규 |
| **monitor** | phs host monitor | 호스트/LXC/VM + **--json** 신규 |
| **net** | phs infra net | 인터페이스/라우트/브리지/DNS/ping + **--json** 신규 |
| **node** | phs infra node | list/info/exec (canonical IP 직접 조회) |
| **recovery** | phs infra recovery | snapshot/restore/audit (O_EXCL atomic, path traversal 차단) |

### ✅ 레시피로 이식 (deploy 도메인이 처리)

| 레시피 | phs 원본 | 비고 |
|---|---|---|
| nginx.toml | - | 신규 |
| postgres.toml | - | 신규 (DB_PASSWORD 환경변수) |
| redis.toml | - | 신규 (REDIS_PASSWORD) |
| uptime-kuma.toml | phs infra kuma | Docker + :3001 |
| formbricks.toml | phs infra formbricks | Docker Compose + secret 자동 생성 |
| matterbridge.toml | phs infra matterbridge | systemd + 바이너리 |
| infisical.toml | phs infisical | Docker Compose + SHA pin |
| ministack.toml | phs ministack | Docker + tag pin + docker.sock opt-in |

### ⚠️ dalsoop 특화로 제외

| phs 모듈 | 이유 |
|---|---|
| agent_orchestrator | dal AI 에이전트 오케스트레이션 |
| dalcenter_provision | dalcenter 전용 프로비저닝 |
| infra_control | dalsoop LXC 50101 전용 |
| openclaw-* (12개) | OpenClaw 게이트웨이 (dalsoop 인프라) |
| cluster-files | sshfs 클러스터 파일 집계 (Synology/vmbr1 의존) |
| homelable (1162줄) | Traefik + DNS + LXC 자동 연결 (control-plane JSON 의존) |
| ingress | PUBLIC_IP / ROUTER_FORWARD_TARGET 환경 의존 |
| mgmt_setup | 관리 LXC SSH+API 설정 |
| exec_proxy | HTTP whitelist exec (보안 민감) |
| global_verify | phs inventory/route/sync/kuma 검증 |
| rbac (836줄) | pveum + roles.toml (규모 크고 Proxmox API deep) |
| omarchy | Arch Linux 자동 설치 |

---

## prelik이 phs보다 우수한 점

1. **도메인별 독립 설치** — 필요한 것만, 영향 범위 최소화
2. **71차 Codex 어드버서리얼 리뷰** — 50건+ P0/P1 수정
3. **84 unit tests + shellcheck CI** — phs는 테스트 0
4. **완전한 uninstall 경로** — `prelik uninstall` + 10 섹션 가이드 + bootstrap manifest
5. **--json 출력** — monitor/lxc/vm/iso/net/node 6개 도메인 자동화 친화
6. **recovery** — O_CREAT|O_EXCL atomic ID + path traversal 차단 + audit log
7. **NAS 안전성** — cifs-credentials 분리(0600) + fstab append-only + EOF 개행 검증
8. **postfix relay 완전 롤백** — main.cf + sasl_passwd + sender_canonical 일괄
9. **node exec 권한 경계** — cluster canonical IP 직접 조회 + `-F /dev/null` + ProxyCommand=none
10. **install.sh** — idempotent skip + PRELIK_VERSION pin + atomic install + retry

## prelik이 못 하는 점 (phs 전용)

1. **OpenClaw 게이트웨이 운영** — 12개 커맨드
2. **cluster-files (sshfs 집계)** — NAS/sshfs mount 포인트 통합
3. **homelable 도메인 자동 연결** — Traefik + CF DNS + LXC IP 일괄
4. **RBAC/roles.toml** — pveum 역할 기반 접근 제어
5. **global-verify** — 인벤토리/라우트/동기화 체인 검증
6. **exec-proxy** — whitelist 기반 HTTP 원격 실행

---

## 결론

**prelik-init은 phs의 범용 가치 있는 기능을 모두 이식하고, 안전성/테스트/제거 경로에서 크게 개선한 OSS 프로젝트**입니다. dalsoop 특화 운영 기능(OpenClaw, homelable, rbac 등)만 제외하고 나머지는 전부 이식 완료.
