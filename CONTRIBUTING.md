# 기여 가이드

## 새 도메인 추가

**`scripts/new-domain.sh` 스캐폴더가 정본**. 4개 파일을 원자적으로 생성 + Nickel 검증 + `cargo check` 를 한 번에 돌려 SSOT 일관성을 깨지 않음.

```bash
scripts/new-domain.sh <name> <product> <layer> <platform> "<설명>"

# 예시
scripts/new-domain.sh vaultwarden service remote proxmox \
  "Vaultwarden 패스워드 매니저 LXC"
```

허용 값 (Nickel `domain_contract.ncl` 와 동일):
- `product`: `infra | desktop | ai | devops | service | monitor | tool | media | network`
- `layer`: `host | remote | tool`
- `platform`: `proxmox | linux | any`

생성되는 파일:

| 파일 | 역할 |
|---|---|
| `crates/domains/<name>/Cargo.toml` | workspace version/edition/license inherit |
| `crates/domains/<name>/src/main.rs` | clap skeleton (doctor/status) |
| `crates/domains/<name>/domain.ncl` | Domain contract 적용 인스턴스 |
| `ncl/domains.ncl` | 알파벳 순 import 라인 삽입 |

스캐폴더는 Nickel eval 과 cargo check 까지 자동 실행. 통과하면 이후 할 일:

1. `src/main.rs` TODO 구현
2. `scripts/install-local.sh <name>` 로 로컬 빌드 + locale.json 재생성
3. `pxi validate` 로 drift 없음 확인
4. 커밋 + PR

**수동 편집이 필요한 레거시 절차는 없음.** `crates/core/src/registry.rs` 에 손댈 일 없음 (Registry 가 locale.json 을 직접 읽음). `release.yml` 수정도 불필요 (`locale-export` job 이 자동). `tests/smoke.sh` 는 대표 crate 두 개만 검사 — 전수 빌드는 `cargo check --workspace` 가 담당.

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
- 외부 명령:
  - `pxi_core::common::run()` — stdout **캡처**, 반환 `Result<String>` (기본)
  - `pxi_core::common::run_passthrough()` — 진행상태 실시간 상속 (예: `pct enter`)
  - `pxi_core::common::run_secret()` — argv 에 자격증명 포함 시. 실패 메시지에 argv 노출 안 함
- 명령 존재 확인: `common::has_cmd()` (외부 `which` 바이너리 의존 금지)
- 호스트 경로: `pxi_core::paths::*`
- 도메인 메타 로딩: `pxi_core::registry::Registry::load()` (locale.json 3-tier)
- doctor: 누락 의존성은 **보고만**, exit 0 (CI smoke 호환)

## 보안 원칙

- 크리덴셜은 `/etc/pxi/.env` 또는 인자로만
- `/tmp` 직접 안 쓰고 `mktemp` 사용 (예측 불가 경로)
- curl 다운로드: HTTPS + `--fail` + atomic mv (`.tmp` → 최종)
- 사용자 설정 파일: merge/append (절대 replace 안 함)
- fstab: `tee -a` append-only + EOF 개행 검증
- 사용자 입력 검증: locale/timezone/package/snapshot ID/node 이름 전부 charset 검증 + traversal 차단
- SSH: `-F /dev/null` + `StrictHostKeyChecking=yes` + cluster IP 직접 조회

## 릴리스

메인테이너만:
```bash
# 1) Cargo.toml workspace.package.version을 새 값으로 bump (예: 1.11.1 → 1.11.2)
#    비-git 빌드(source archive/vendored)의 --version fallback 값.
# 2) git commit + tag + push
git add Cargo.toml
git commit -m "release: v1.X.Y"
git tag v1.X.Y -m "설명"
git push origin main v1.X.Y
# GitHub Actions가 자동 빌드 + 릴리스 (x86_64 + aarch64)
# install.pxi.com이 latest로 리다이렉트
```
