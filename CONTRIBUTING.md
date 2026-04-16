# 기여 가이드

## 새 도메인 추가

1. `crates/domains/<name>/` 생성
   - `Cargo.toml` — `prelik-<name>` 바이너리 + `prelik-core` 의존
   - `src/main.rs` — `clap` CLI + `Doctor` 서브커맨드 (graceful, exit 0)

2. `ncl/domains.ncl` 레지스트리에 메타데이터:
```nickel
<name> = {
  name = "<name>",
  description = "...",
  tags = { product = 'infra, layer = 'remote, platform = 'debian },
  provides = ["<name> cmd1", "<name> cmd2"],
} | Domain,
```

3. `crates/core/src/registry.rs` fallback에 한 줄 추가 (nickel 없는 환경 대비)

4. `tests/smoke.sh`의 도메인 리스트에 추가 (알파벳 순)

5. `.github/workflows/release.yml`의 `for bin in ...` 목록에 `prelik-<name>` 추가

6. PR 생성

## 새 deploy 레시피 추가

1. `examples/recipes/<service>.toml` 생성:
```toml
[service]
name = "myservice"
description = "..."

[lxc]
cores = "2"
memory = "2048"
disk = "8"

[install]
packages = ["docker.io"]

[[install.steps]]
name = "설치"
run = """
set -euo pipefail
# 스크립트 내용
"""
```

2. 검증 step은 실패 시 반드시 `exit 1` (deploy가 exit code로 성공 판정)

3. upstream compose/바이너리는 **태그/SHA pin** 필수 (main 추적 금지):
   - `curl -fsSL "https://.../${PIN}/docker-compose.yml" -o docker-compose.yml.tmp`
   - `mv -f docker-compose.yml.tmp docker-compose.yml` (atomic)
   - `set -euo pipefail` 스크립트 상단 필수

4. docker.sock 마운트는 opt-in 환경변수로 (기본 off)

5. 시크릿 자동 생성 시 `.env.secrets` 분리 (1회 생성) + `.env` 매 실행 갱신

6. `examples/README.md` 표에 추가

7. shellcheck CI가 자동으로 recipe step을 추출해 검증 (SC1091/SC2086 제외)

## 회귀 테스트 추가

파싱 로직은 **순수 함수로 추출** 후 `#[cfg(test)] mod tests`:

```rust
fn parse_something(text: &str) -> anyhow::Result<Vec<Row>> { ... }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn something_basic() {
        let rows = parse_something("header\ndata line\n").unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn something_rejects_bad_input() {
        assert!(parse_something("garbage").is_err());
    }
}
```

- 정상 케이스 + fail 케이스 모두 포함
- JSON 출력 도메인은 fail-fast (bail!) 검증 테스트 필수
- CI에서 `cargo test --release` 자동 실행

## --json 출력 규칙

read-only 명령(list/status 등)에 `--json` 글로벌 플래그:
- 텍스트 모드: soft-fail (안내 + exit 0) — 사람용
- JSON 모드: fail-fast (bail! + exit 1) — 자동화. 빈 결과로 위장 금지
- `lxc_supported`/`vm_supported` 같은 플래그로 "미지원" vs "실제 0개" 구분
- 비밀(--password 등) argv는 `common::run_secret()` 사용

## 코드 스타일

- `clap` derive 매크로
- 에러: `anyhow::Result`
- 외부 명령: `prelik_core::common::run()` (비밀 포함 시 `run_secret()`)
- 명령 존재 확인: `common::has_cmd()` (외부 `which` 바이너리 의존 금지)
- 호스트 경로: `prelik_core::paths::*`
- doctor: 누락 의존성은 **보고만**, exit 0 (CI smoke 호환)

## 보안 원칙

- 크리덴셜은 `/etc/prelik/.env` 또는 인자로만
- `/tmp` 직접 안 쓰고 `mktemp` 사용 (예측 불가 경로)
- curl 다운로드: HTTPS + `--fail` + atomic mv (`.tmp` → 최종)
- 사용자 설정 파일: merge/append (절대 replace 안 함)
- fstab: `tee -a` append-only + EOF 개행 검증
- 사용자 입력 검증: locale/timezone/package/snapshot ID/node 이름 전부 charset 검증 + traversal 차단
- SSH: `-F /dev/null` + `StrictHostKeyChecking=yes` + cluster IP 직접 조회

## 릴리스

메인테이너만:
```bash
git tag v1.X.Y -m "설명"
git push origin v1.X.Y
# GitHub Actions가 자동 빌드 + 릴리스 (x86_64 + aarch64)
# install.prelik.com이 latest로 리다이렉트
```
