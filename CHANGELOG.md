# Changelog

Semantic Versioning (https://semver.org/)

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
- **install flock 배타 락**: `/var/lib/prelik/.install.lock` 기반 논블로킹
  flock(LOCK_EX|LOCK_NB). 같은 도메인을 병행 설치해서 바이너리가 덮어써지는
  race condition 차단.
- **tar 실패 시 부분 설치 정리**: 압축 해제 도중 실패하면 domain 디렉토리
  전체 삭제. 잔재 없음.

### Removed
- `prelik-manager` 바이너리 (공식 리스트에서 제거)

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
- **P2 ai hook legacy marker 인식**: v0.7.4에서 등록된 `prelik-adv-review-`
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
- **ai hook marker 강화**: `prelik-adv-review-` → `__PRELIK_AI_ADV_REVIEW_HOOK__`
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
- `mail postfix-relay`: SASL 패스워드를 `/tmp/prelik-sasl_passwd` 평문으로
  잠시 노출하던 경로 제거. mktemp + chmod 600 + `install -m 600 -o root` 패턴.
- `mail postfix-relay`: 실행 전 `/etc/postfix/main.cf.prelik-<timestamp>`
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
- `prelik-ai` 도메인 (Claude/Codex + octopus/superpowers/codex-plugin)
- `install.prelik.com` Cloudflare Worker 리다이렉트
- 공개 설치 URL: `curl install.prelik.com | bash`

## [0.3.0] - 2026-04-15

### Added
- `prelik-traefik`, `prelik-mail`, `prelik-cloudflare`, `prelik-connect`
- Nickel 스키마 전환 (ncl/domains.ncl)
- build.rs로 도메인 자동 감지

## [0.2.0] - 2026-04-15

### Added
- `prelik-lxc` 도메인 (Proxmox pct 래퍼)
- Nickel runtime export 통합
- CI: Docker 바이너리 빌드 (x86_64 + aarch64)

## [0.1.0] - 2026-04-15

### Added
- 초기 스캐폴딩 (workspace + core + cli)
- `bootstrap`, `connect`, `manager` 도메인 (일부 placeholder)
- `install.sh` + GitHub Release 자동 빌드
