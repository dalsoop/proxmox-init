# 기여 가이드

## 새 도메인 추가

1. `crates/domains/<name>/` 생성
2. 필수 파일:
   - `Cargo.toml` — `prelik-<name>` 바이너리 + `prelik-core` 의존
   - `src/main.rs` — `clap` CLI
   - `domain.ncl` — 마커 파일 (build.rs가 감지)

3. `ncl/domains.ncl` 레지스트리에 메타데이터 추가:
```
<name> = {
  name = "<name>",
  description = "...",
  enabled = true,
  tags = { product = 'infra, layer = 'remote, platform = 'debian },
} | Domain,
```

4. `.github/workflows/release.yml`의 `for bin in ...` 목록에 `prelik-<name>` 추가

5. PR 생성

## 코드 스타일

- `clap` derive 매크로 사용
- 에러는 `anyhow::Result`
- 외부 명령 실행은 `prelik_core::common::run()` 사용
- 호스트 경로는 `prelik_core::paths::*` 사용

## 보안 원칙

- 크리덴셜은 `/etc/prelik/.env` 또는 인자로만 받기
- `/tmp` 직접 쓰지 말고 `mktemp` 사용
- 외부 curl 스크립트는 체크섬 검증 (또는 최소 HTTPS + --fail)
- 사용자 설정 파일 덮어쓸 때는 merge/append, 절대 replace 안 함

## 릴리스

메인테이너만:
```bash
git tag v0.X.0 -m "설명"
git push origin v0.X.0
# GitHub Actions가 자동 빌드 + 릴리스
```
