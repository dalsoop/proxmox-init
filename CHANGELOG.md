# Changelog

Semantic Versioning (https://semver.org/)

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
