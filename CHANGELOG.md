# Changelog

Semantic Versioning (https://semver.org/)

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
