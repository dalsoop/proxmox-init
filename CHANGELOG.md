# proxmox-init (pxi) Changelog

Semantic Versioning (https://semver.org/)

## scripts/tmux-topbar — 2026-04-20

신규 보조 스크립트 번들 (PR #27). pxi 도메인이나 버전 bump는 아니며,
`scripts/tmux-topbar/install.sh` 로 호스트에 직접 설치.

- tmux-sessionbar/windowbar 위에 얹는 우측 상단 레이아웃 커스터마이즈
- Panes 줄 split 버튼(`| -`)을 Users 줄 우측 끝으로 이식
- Apps 줄에서 설정 TUI 항목 제거, 톱니바퀴(⚙)를 Users 줄 맨 오른쪽에 배치
  (클릭 → `tmux-topbar` 새 윈도우; 구 `tmux-config` 는 호환 심링크로 유지)
- systemd path 유닛 + tmux 훅 오버라이드로 `tmux-sessionbar apply` /
  세션·윈도우 이벤트 발생 시 자동 재이식
- 멱등 install.sh, 자세한 동작/제거 가이드는 `scripts/tmux-topbar/README.md`

## [1.9.x] — 2026-04-16 (uninstall + 시스템 정리)

### v1.9.6 — NFS fstab replace TOCTOU 차단
- `sudo mktemp /etc/...` + `sudo tee` + chown/chmod로 /tmp 심볼릭 링크 race 방지 (codex 65차 P1)

### v1.9.5 — NAS 가이드 regex/순서, NFS 섹션 추가
- 정규식을 옵션 순서 무관 (`credentials=/etc/cifs-credentials/` 단독)으로 수정 (codex 64차 P1)
- §3.1 CIFS / §3.2 NFS 분리 (NFS-only 설치도 cleanup 가능)

### v1.9.4 — NAS 제거 §3 재구성
- "캡처 → 편집 → 삭제" 안전 순서를 본문 단계에 직접 반영 (codex 63차 P1)
- `_netdev,nofail` + `cifs-credentials/` 조합 필터로 prelik 시그니처 한정
- `sed -i.bak` + `mount -a` 검증

### v1.9.3 — cifs 가이드 fstab 역추적
- `safe_name` 비단사 회피 — fstab의 `credentials=` 경로 직접 grep (codex 62차 P1)

### v1.9.2 — cifs safe_name 정확화
- 비영숫자 `_` 치환 규칙 명시 + dot/IP 두 예시

### v1.9.1 — uninstall purge dedup
- `BTreeSet` + `canonicalize`로 `/etc/pxi` 중복 제거 (codex 60차 P1)
- cifs 무차별 삭제 권장 → 선별 삭제 가이드

### v1.9.0 — `prelik uninstall` + `docs/uninstall.md`
- `prelik uninstall [--confirm] [--purge]` — dry-run 기본
  - `/usr/local/bin` + `~/.local/bin` 양쪽 스캔
  - `pxi-*` 도메인 바이너리 + `.prelik.version` 마커 수집
  - `--purge`: `~/.config/prelik`, `/etc/pxi`, `/var/lib/pxi`까지
  - LXC/VM/fstab/postfix/CF는 의도적으로 안 건드림 (데이터 유실 방지)
- `docs/uninstall.md` — 10 섹션 가이드:
  1. uninstall이 실제 하는 일 + 안 건드리는 항목
  2. LXC/VM 정리 (vzdump 백업 → stop → delete --force)
  3. NAS fstab + cifs-credentials (CIFS/NFS 분리)
  4. Postfix relay 백업 디렉토리 복원
  5. Traefik LXC + acme.json
  6. Cloudflare DNS/Worker/Pages 수동 정리
  7. dotenvx `.env.vault` + `.env.keys` 보존
  8. systemd timers/services 비활성화
  9. bootstrap 의존성 (manifest로 정확 안내 — v1.9.7)
  10. 완료 검증 + 복구 절차

---

## [1.8.x] — 2026-04-15~16 (phs migration)

### v1.8.16 — lxc init dpkg-query probe fail-open 차단
- `pct_exec().unwrap_or_default()` → `?` 전파 (codex 58차 P1)
- 각 패키지가 probe 출력에 없으면 fail-fast

### v1.8.15 — lxc init 안전성/idempotency
- `--packages` `-` 시작 거부 + `apt-get install -- <pkg>...` 옵션 종료 마커 (P0)
- `--timezone` 실제 zoneinfo 파일 존재 검증 (P1)
- `dpkg-query`로 누락 패키지만 설치 (idempotent, P1)

### v1.8.14 — `pxi-lxc init` 서브커맨드 [phs batch 6]
- phs/infra/lxc_init.rs (96줄) → lxc 도메인 흡수
- `--locale` (기본 ko_KR.UTF-8), `--timezone` (기본 Asia/Seoul), `--packages`
- 모든 입력 검증: shell injection / path traversal / apt injection 차단

### v1.8.10~v1.8.13 — infisical/ministack 레시피 정합성
- v1.8.13: infisical `.env` override 매 실행 갱신, 다운로드 atomic mv (codex 55차 P1)
- v1.8.12: 매 실행 재다운로드 + image latest 추적 disclaimer (codex 54차 P1)
- v1.8.11: commit SHA/tag pin + docker.sock opt-in (codex 53차 P1)
- v1.8.10: POSTGRES/REDIS/SITE_URL upstream 정합 (codex 52차 P0×2 + P1)

### v1.8.9 — infisical + ministack 레시피 [phs batch 5]
- phs/infisical (850줄) + phs/ministack (471줄) → deploy 레시피로 변환

### v1.8.6~v1.8.8 — recovery 도메인 [phs batch 4]
- v1.8.8: validate_node `..`/`/` 차단, `O_CREAT|O_EXCL` atomic ID, 부분 실패 fail-fast (codex 50차 3 P1)
- v1.8.7: validate_id, validate_config_filename, 빈 스냅샷 거부 (codex 49차 3 P1)
- v1.8.6: `pxi-recovery` 신규 (snapshot/restore/audit) + `pxi-host gh-auth`

### v1.8.4~v1.8.5 — host self-update + 3 deploy 레시피 [phs batch 3]
- v1.8.5: formbricks compose URL 정확화, self-update `set -eo pipefail`, 검증 timeout exit 1 (codex 47차 3 P1)
- v1.8.4: pxi-host self-update, uptime-kuma/formbricks/matterbridge 레시피

### v1.8.0~v1.8.3 — net + node 도메인 [phs batch 1-2]
- v1.8.3: node exec를 cluster canonical IP 직접 조회 + `-F /dev/null` + ProxyCommand=none (codex 45차 P1)
- v1.8.2: node exec 클러스터 멤버십 검증 + StrictHostKey=yes (codex 44차 P1)
- v1.8.1: `pxi-node` (list/info/exec) + 8 회귀 테스트
- v1.8.0: `pxi-net` (interfaces/routes/bridges/dns/ping) + 6 회귀 테스트

---

## [1.7.x] — 2026-04-15 (vm/iso --json + 회귀 테스트)

### v1.7.2 — vm list status whitelist 계약 일치
- `parse_qm_list`도 `STATUS_KNOWN` 검증 — `status` 두 surface 일관성 (codex 41차 P1)

### v1.7.1 — vm list/status `--json` + 17 회귀 테스트
- `parse_qm_list` (5/6 컬럼, PID=0→null) + `parse_qm_status` (whitelist)
- VM 전용 상태값: running/stopped/paused/suspended/prelaunch

### v1.7.0 — iso list `--json` + 10 회귀 테스트
- `parse_pvesm_status` + `parse_pvesm_list` 추출
- `ListSnap { storages, files }` 통합 스냅샷

---

## [1.6.x] — 2026-04-15 (lxc --json + 회귀 테스트 인프라)

### v1.6.7~v1.6.8 — 회귀 테스트 인프라
- v1.6.8: monitor 파서 추출 (parse_meminfo/df/qm) + 13 테스트
- v1.6.7: lxc 파서 추출 (parse_pct_list/status/listsnapshot) + 18 테스트, CI에 `cargo test` 통합

### v1.6.0~v1.6.6 — lxc list/status/snapshot-list `--json` (7라운드 codex)
- v1.6.6: status whitelist를 upstream pve-container와 정확 일치 (running/stopped/unknown)
- v1.6.5: status 값 whitelist 검증
- v1.6.4: status 경로 raw stdout 직접 사용 (common::run trim 회피)
- v1.6.3: status 단일 라인 + `strip_prefix("status:")` 엄격
- v1.6.2: snapshot 시간 토큰 ASCII digit 검증, status 키 == "status" 검증
- v1.6.1: snapshot 파서 재작성 (parent → timestamp), JSON fail-fast 강화
- v1.6.0: `--json` 글로벌 플래그 추가 (list/status/snapshot-list)

---

## [1.5.x] — 2026-04-14~15 (monitor/iso/install.sh)

### v1.5.6 — install.sh 견고화
- `PRELIK_VERSION` / `PRELIK_FORCE` 환경변수
- `BIN_DIR/.prelik.version` 마커로 idempotent skip
- API/다운로드 retry 3회 + curl timeout
- atomic install (BIN_DIR 안 staging mv)

### v1.5.5 — monitor 텍스트 모드 soft-fail 복구 (회귀)
- v1.5.4가 텍스트 모드까지 fail-fast로 만든 것을 분리 (codex 28차 P1)

### v1.5.4 — monitor JSON fail-fast
- `--json lxc/vm`은 pct/qm 누락 시 EXIT 1 (자동화 false negative 차단)
- `AllSnap`에 `lxc_supported`/`vm_supported` 플래그

### v1.5.3 — monitor `--json` 출력
- doctor/host/lxc/vm/all 모두 JSON 지원

### v1.5.2 — 안정성 스윕
- `common::run_secret()` 추가 — 비밀 argv 노출 방지 표준
- monitor/iso의 자체 `which()` 헬퍼 → `common::has_cmd` 통합
- doctor 일관성 18/18 (CI smoke 안전)

### v1.5.1 — monitor/iso 외부 `which` 바이너리 의존 제거
- workspace의 `which` crate (PATH 직접 탐색) 사용

### v1.5.0 — `pxi-monitor` (호스트/LXC/VM read-only)
- 18번째 도메인. host (CPU/RAM/disk/온도/uptime), lxc (mem%/disk%), vm, all

---

## [1.4.x] — 2026-04-14 (iso 도메인)

### v1.4.1 — iso SMB 비밀번호 로그 노출 차단
- `storage_add_cifs`를 직접 `Command::status()`로 호출 (실패 시 argv 비공개) — common::run_secret 도입의 모태

### v1.4.0 — `pxi-iso` (Proxmox ISO 스토리지 + 파일)
- `list`, `storage-add-nfs/cifs`, `download`, `remove` (pvesm 래퍼)

---

## [1.3.x] — 2026-04-14 (deploy 도메인)

### v1.3.1 — deploy IP /16 강제 회귀 수정
- bare IP를 pxi-lxc에 그대로 전달 (`config.network.subnet` 존중)

### v1.3.0 — `pxi-deploy` (TOML 레시피)
- 레시피 기반 LXC 자동 배포 (`service`, `list-recipes`)
- examples/recipes/nginx.toml

---

## [1.0.0~1.2.x] — 2026-04-14 (Phase 1-2 안정화)

### v1.2.0 — backup 도메인 + cloudflare pages-deploy
(원본 항목 유지, 아래 §[1.2.0] 참조)

### v1.1.0 — `pxi-vm` (Proxmox QEMU)
- list/status/start/stop/reboot/delete/backup/resize/console

### v1.0.0 — Phase 2 안정화 + install.prelik.com
- 14 도메인 안정화, GitHub Actions release CI, install 채널 가동

---

## [1.2.0] - 2026-04-15

### Added
- **backup 도메인 신규** (15번째 도메인)
  - `now`: 즉시 vzdump (--storage --mode)
  - `list`: 백업 파일 목록 (vmid 필터)
  - `schedule-add`: pvesh로 Proxmox backup job 등록 (schedule/keep/prune)
  - `schedule-list/remove`: backup job 관리
  - `restore`: pct restore 또는 qmrestore 자동 분기
- **cloudflare pages-deploy**: wrangler pages deploy 래퍼
  - `--project X --directory dist`

### 총 15 도메인
ai, account, backup, bootstrap, cloudflare, comfyui, connect, host, lxc,
mail, nas, telegram, traefik, vm, workspace

## [1.1.0] - 2026-04-15

### Added
- **lxc snapshot**: create/list/restore/delete
  - `prelik run lxc snapshot-create X name --description "msg"`
  - rollback 전에 현재 상태 저장, 테스트 후 되돌리기
- **lxc resize**: CPU/RAM/disk 동적 변경
  - `--cores N --memory MB --disk-expand +4G`
- **cloudflare ssl**: Let's Encrypt DNS-01 발급/갱신 (acme.sh 래퍼)
  - `prelik run cloudflare ssl-issue --domain X [--wildcard]`
  - `prelik run cloudflare ssl-renew --domain X`
- **vm 도메인 신규**: Proxmox QEMU VM 관리 (qm 래퍼)
  - list/status/start/stop/reboot/delete/backup/resize/console

### 총 14 도메인
ai, account, bootstrap, cloudflare, comfyui, connect, host, lxc, mail, nas,
telegram, traefik, vm, workspace

## [1.0.0] - 2026-04-15

### 🎉 첫 공식 릴리스

13개 도메인, 14회 Codex 어드버서리얼 리뷰 통과, 26개 이슈 반영 후 v1.0.0 승격.

### 설치

```bash
curl -fsSL https://install.prelik.com | bash
prelik init
```

### 도메인 전체 (13)

- **ai** — Claude/Codex CLI + 플러그인 (octopus, superpowers, codex-plugin)
- **account** — 리눅스 계정 관리 (create/remove/ssh-key-add)
- **bootstrap** — 의존성 (apt/rust/gh/dotenvx/nickel), 도구 단위 install/remove
- **cloudflare** — DNS CRUD + Email Worker (audience 기반 proxied 자동)
- **comfyui** — GPU LXC + ComfyUI 설치
- **connect** — .env + dotenvx 암호화
- **host** — 호스트 시스템 관리 (status/monitor/ssh-keygen/smb)
- **lxc** — Proxmox LXC 수명관리
- **mail** — Maddy + Mailpit + Postfix relay
- **nas** — SMB/NFS 마운트 (credentials 파일 분리)
- **telegram** — 봇 등록 + 메시지 발송 (범용)
- **traefik** — Traefik 리버스 프록시 + compose 재생성
- **workspace** — tmux + shell alias

### 보안 강화

- mktemp + chmod 600 + Drop 가드 (traefik/mail/nas/ai)
- SMB credentials 파일 분리 (ps/cmdline/fstab 평문 차단)
- postfix rollback (전체 파일 백업 + tee -a append)
- install flock (동시 설치 race 차단)
- ai hook marker 기반 filter (기존 Stop 훅 보존)
- visudo -cf 검증 후 sudoers 설치
- fstab append EOF 개행 체크
- CF API 에러 구분 (401/403/429 별도 처리)

### 품질

- 14회 Codex 어드버서리얼 리뷰 통과 (P1 26건 수정)
- CI smoke test 자동화 (모든 바이너리 --help + doctor)
- 다중 도메인 + 프리셋 설치 (web/mail/dev/minimal)
- Nickel SSOT 런타임 export
- 도구 단위 선택 install/remove
- CRUD 사이클 검증 (lxc, cloudflare dns, account)

### 문서

- README (빠른시작, 도메인 표, 설계 원칙)
- CONTRIBUTING.md
- CHANGELOG.md
- docs/phs-vs-prelik.md (phs 내부 도구와 솔직한 비교)
- examples/formbricks.md

## [0.13.2] - 2026-04-15

### Added (Phase 2 완료 — 3/3)
- **pxi-comfyui**: ComfyUI LXC 설치 관리 (GPU 패스스루 + systemd)
  - gpu-passthrough: /etc/pve/lxc/<vmid>.conf에 NVIDIA device 줄 추가 (멱등)
  - install: git clone + venv + requirements + systemd unit
  - status: systemctl + 포트 확인

### Phase 2 전체 완료
- [x] account (v0.13.0) — 범용 리눅스 계정 관리
- [x] telegram (v0.13.1) — 봇 등록/발송 (범용화)
- [x] comfyui (v0.13.2) — GPU LXC + ComfyUI 설치

### 총 도메인 13개
ai, account, bootstrap, cloudflare, comfyui, connect, host, lxc, mail, nas,
telegram, traefik, workspace

## [0.12.1] - 2026-04-15

### Fixed (Codex 10차 리뷰 — P1 3건)
- **nas mount argv 방식 전환**: 모든 인자를 `Command::args()`로 직접 전달.
  bash interpolation 제거. 공백/특수문자 포함한 경로/비밀번호도 안전.
- **SMB 비밀번호 ps/cmdline 노출 차단**: `/etc/cifs-credentials/<host>_<share>`
  (0600, root:root) credentials 파일로 이동. `mount -o credentials=<file>`.
  비밀번호가 프로세스 리스트에서 더 이상 보이지 않음.
- **/etc/fstab SMB 비밀번호 평문 차단**: fstab에는 credentials 파일 경로만
  적힘. 로컬 사용자의 fstab 열람을 통한 NAS 자격증명 유출 방지.

### Changed
- `mount`, `umount`, `mkdir`, `install` 전부 argv 호출로 통일
- `secure_tempfile()` + `TempGuard` Drop으로 RAII 정리 (nas 내부)

## [0.12.0] - 2026-04-15

### Added (Phase 1 완료 — 3/3)
- **pxi-workspace**: tmux 기본 설정 + shell alias (~/.bashrc.d/prelik.sh)
  - tmux-setup: 기본 .tmux.conf (vim 키바인딩, mouse, base-index 1)
  - shell-setup: 편의 alias (ll, g, gs, bat/eza 자동 대체)
  - ~/.bashrc에 `.bashrc.d/*.sh` source 자동 등록
- Phase 1 전체 이식 완료:
  - [x] host (v0.10.0)
  - [x] nas (v0.11.0)
  - [x] workspace (v0.12.0)

### 이식률 재평가
- 총 10개 도메인: ai, bootstrap, cloudflare, connect, host, lxc, mail, nas,
  traefik, workspace
- phs의 "범용 공유 가치" 영역 거의 완성.
- 남은 것은 dalsoop 환경에 깊이 박힌 것 (telegram 봇, ComfyUI, OpenClaw 등)
  또는 별도 일반화 작업 필요한 것 (account/RBAC, cluster-files, homelable).

## [0.9.0] - 2026-04-15

### Added
- **cloudflare dns list/update/delete**: phs의 CRUD를 prelik에 이식. 기존
  add만 있던 것에서 완전한 CRUD로 확장.
  - `dns-list`: phs와 거의 동일 출력 포맷
  - `dns-update`: type+name으로 기존 레코드 찾아 교체
  - `dns-delete`: 동일 방식으로 삭제
- **docs/phs-vs-prelik.md**: phs ↔ prelik 솔직한 비교표.
  - 도메인별 이식 커맨드 매핑
  - prelik이 우수한 점 8개 / 못 하는 점 7개 명시
  - 로드맵 (remote node, bootstrap, SSL, NAS 등)
- README에서 비교 문서 링크

### Documented
- phs 대비 prelik 이식률: 약 **23%**
- 미이식 도메인: host, nas, telegram, workspace, account

## [0.8.2] - 2026-04-15

### Added
- **tests/smoke.sh**: 모든 바이너리의 `--help` + `doctor`를 한 방에 검증.
  CI `Build` 단계 뒤에 자동 실행 (x86_64 only).
- **CI 트리거 확장**: main push + PR에서도 빌드+smoke test 실행.
  태그 푸시 전에 회귀 조기 발견.
- **prelik doctor 강화**:
  - `config_dir` 존재 여부 표시
  - 의존성을 "core 의존성" 섹션으로 분리 (curl/tar/systemctl/dotenvx/nickel)
  - 설치된 도메인 목록 + 바이너리 존재 체크 (누락 감지)

## [0.8.1] - 2026-04-15

### Fixed / Stability
- **`manager` placeholder 완전 제거**: 실행해도 "TODO: 구현 예정"만 출력해서
  공유 시 혼란 유발. crate, domain.ncl, registry fallback, release.yml 전부 정리.
- **install flock 배타 락**: `/var/lib/pxi/.install.lock` 기반 논블로킹
  flock(LOCK_EX|LOCK_NB). 같은 도메인을 병행 설치해서 바이너리가 덮어써지는
  race condition 차단.
- **tar 실패 시 부분 설치 정리**: 압축 해제 도중 실패하면 domain 디렉토리
  전체 삭제. 잔재 없음.

### Removed
- `pxi-manager` 바이너리 (공식 리스트에서 제거)

## [0.8.0] - 2026-04-15

### Added
- **도구 단위 개별 install/remove**: `prelik run bootstrap install --only rust,nickel`
  처럼 원하는 도구만 설치하거나 제거 가능. 기존 "전체 한꺼번에"가 단일
  선택지였던 문제 해결.
- `prelik run bootstrap list` — 각 도구의 현재 설치 상태 확인
- `Tool` enum (apt/rust/gh/dotenvx/nickel) + 각자 install/remove 함수

### Changed
- `bootstrap install` (인자 없음) = 기본적으로 **5개 전부** 설치 (기존 동일 동작)
- `bootstrap install --only X,Y` = X, Y만 설치
- `bootstrap remove --only X,Y` = X, Y만 제거
- `bootstrap doctor` 출력 포맷 단순화

### Honesty
- phs 대비 prelik 포팅 비율 ~23%. 아직 phs의 nas/workspace/telegram/account/
  host/config/homelable/comfyui/dalcenter 등 미이식. "공유 가능"이지 "phs 대체"는 아님.

## [0.7.8] - 2026-04-15

### Fixed (Codex 7차 — v0.7.7 회귀 수정)
- **P1 Tweag nickel arm64 자산명**: `nickel-aarch64-linux` → `nickel-arm64-linux`.
  Tweag 실제 릴리스 이름에 맞춤. arm64 호스트 bootstrap 불가 문제 해결.
- **P1 install.sh의 file 커맨드 의존 제거**: debian slim 등 최소 이미지에
  `file`이 없음. `od -tx1`로 gzip 매직 바이트(1f8b) 직접 검사로 대체.

## [0.7.7] - 2026-04-15

### Fixed / Enhanced (Codex 6차 구조 리뷰 반영)
- **Registry silent fallback 개선**: nickel export 실패 시 stderr 경고 출력
  (`ncl/domains.ncl` 편집 실수 감지 가능). 기능은 기존대로 fallback 유지.
- **bootstrap nickel 설치 속도**: cargo install nickel-lang-cli (수 분) 대신
  GitHub Release 바이너리 직접 설치 (수 초). 실패 시 cargo 폴백.
- **install.sh 최소 무결성 검증**: 파일 크기 >= 1024 + gzip 타입 확인.
  MITM 또는 HTML 에러 페이지 방지. 체크섬 검증은 향후 릴리스 프로세스에서.

### Internal
- `crates/core/src/helpers.rs` 추가: `read_host_env`, `write_to_lxc`,
  `secure_tempfile`, `FileCleanup` 공통 헬퍼. 기존 도메인 리팩터는 별도 작업.

## [0.7.6] - 2026-04-15

### Fixed (Codex 5차 — v0.7.5 회귀 수정)
- **P1 install_many short-circuit 철회**: bootstrap 실패가 뒤 도메인의
  바이너리 다운로드를 막을 이유 없음. 각 도메인은 독립적으로 GitHub
  Release에서 내려받으므로 실패 누적만 하고 계속 진행.
- **P2 ai hook legacy marker 인식**: v0.7.4에서 등록된 `pxi-adv-review-`
  marker 훅이 업그레이드 후 사라지지 않던 문제. retain 필터가 현재 +
  legacy marker 둘 다 제거하도록 보강.

## [0.7.5] - 2026-04-15

### Fixed (Codex 4차 리뷰)
- **P1 postfix 백업 실패 마스킹**: `[ -e X ] && cp || true` 패턴이 cp 실패도
  true로 삼켰음. 존재 판정을 Rust로 옮기고 cp 실패는 명시적 에러.
- **P2 롤백 안내 불완전**: 출력 메시지가 `main.cf`만 복원하라고 안내했으나
  sasl_passwd/sender_canonical도 덮어쓰므로 `backup_dir/*` 전체 복원 + postmap
  재실행 명령으로 수정.

### Enhanced
- **install_many 단락 평가**: `bootstrap` 첫 도메인이 실패하면 의존성 없는
  뒤 도메인은 의미 없으므로 중단하고 남은 목록을 에러 메시지에 표시.
- **ai hook marker 강화**: `pxi-adv-review-` → `__PRELIK_AI_ADV_REVIEW_HOOK__`
  로 변경. 사용자 자체 훅 커맨드에 오판 가능성 차단.

## [0.7.4] - 2026-04-15

### Fixed (Codex 3차 리뷰)
- **P1 rollback 복원 실패 은닉**: main.cf 복원 후 systemctl reload를 `.ok()`로
  버려서 복구 실패해도 "완료"라 표시하던 문제. 이제 reload 실패는 bail!().
- **P1 보조 맵 파일 미복원**: rollback이 main.cf만 되돌려 `sasl_passwd` /
  `sender_canonical`은 새 값으로 남던 문제. 모든 관련 파일을 디렉토리에
  백업하고 실패 시 전체 복원.
- **P2 `postfix flush` 오분류**: `reload && flush`를 AND로 묶어 flush만
  실패해도 rollback 유발. reload(설정 적용)와 flush(큐 재시도)를 분리.
  flush 실패는 경고만 출력.

## [0.7.3] - 2026-04-15

### Fixed (Codex 재리뷰)
- **P1 mail postfix-relay 롤백**: main.cf rewrite 후 postfix check / reload
  실패 시 자동으로 백업 복원. outbound mail 단절 방지.
- **P2 cf email-worker-attach-all --dry-run**: GET catch-all 실패를
  "(없음)"으로 은닉하지 않고 명시적으로 실패 리스트에 추가. 403/429가
  미리보기를 거짓으로 만들던 문제 해결.
- **P3 postfix 백업 nanosecond**: 동일 초 내 재실행 시 이전 백업이
  덮어써지는 것 방지. `%Y%m%d-%H%M%S.%N` 포맷.

## [0.7.2] - 2026-04-15

### Security / Safety
- `mail postfix-relay`: SASL 패스워드를 `/tmp/pxi-sasl_passwd` 평문으로
  잠시 노출하던 경로 제거. mktemp + chmod 600 + `install -m 600 -o root` 패턴.
- `mail postfix-relay`: 실행 전 `/etc/postfix/main.cf.pxi-<timestamp>`
  자동 백업 (실수 복구용).
- `cloudflare email-worker-attach-all --dry-run`: 실제 변경 없이 대상 목록 +
  현재 catch-all action 표시. 무심코 모든 도메인 포워딩 덮어쓰는 사고 방지.

## [0.7.1] - 2026-04-15

### Removed
- `full` 프리셋 제거 — 무차별 전체 설치는 의도치 않은 사이드이펙트 위험.
  필요한 도메인은 이름 명시 또는 용도별 프리셋(web/mail/dev/minimal) 사용.

## [0.7.0] - 2026-04-15

### Added
- 다중 도메인 설치: `prelik install bootstrap lxc traefik` (공백 구분)
- 프리셋: `prelik install --preset web/mail/full/dev/minimal`
- `ncl/presets.ncl` 레지스트리
- `prelik available`이 프리셋 목록도 표시
- `prelik remove/update`도 다중 도메인 지원

### Changed
- install 실패 시 개별 실패 기록 + 전체 결과에 fail count 반영

## [0.6.0] - 2026-04-15

### Added
- `prelik init` 인터랙티브 초기 세팅 커맨드
- `examples/formbricks.md` 실전 사용 예시
- 공유용 README (데모, 도메인 표, 설계 원칙)
- CONTRIBUTING.md
- CHANGELOG.md

## [0.5.0] - 2026-04-15

### Fixed (Codex 어드버서리얼 리뷰)
- Traefik write_to_lxc가 predictable /tmp 경로 → mktemp + Drop 가드
- ai install의 sudo가 pct exec에서 실패 → vmid 있으면 sudo 생략
- CF email routing 에러가 "비활성화"로 은닉 → 에러/비활성화 구분
- CF worker-attach-all 실패 시에도 Ok(()) 반환 → bail!()
- adversarial-review-hook이 기존 Stop 훅 덮어씀 → marker 기반 filter
- mail libsasl2-modules 설치 실패 무시 → 강제 bail!()

## [0.4.0] - 2026-04-15

### Added
- `pxi-ai` 도메인 (Claude/Codex + octopus/superpowers/codex-plugin)
- `install.prelik.com` Cloudflare Worker 리다이렉트
- 공개 설치 URL: `curl install.prelik.com | bash`

## [0.3.0] - 2026-04-15

### Added
- `pxi-traefik`, `pxi-mail`, `pxi-cloudflare`, `pxi-connect`
- Nickel 스키마 전환 (ncl/domains.ncl)
- build.rs로 도메인 자동 감지

## [0.2.0] - 2026-04-15

### Added
- `pxi-lxc` 도메인 (Proxmox pct 래퍼)
- Nickel runtime export 통합
- CI: Docker 바이너리 빌드 (x86_64 + aarch64)

## [0.1.0] - 2026-04-15

### Added
- 초기 스캐폴딩 (workspace + core + cli)
- `bootstrap`, `connect`, `manager` 도메인 (일부 placeholder)
- `install.sh` + GitHub Release 자동 빌드
