#!/usr/bin/env bash
# 새 도메인 스캐폴더 — pxi 도메인 추가 시 필요한 모든 파일을 원자적으로 생성.
# SSOT 규약을 깨지 않도록 한 번에 전부 만들기:
#   - crates/domains/<name>/Cargo.toml   (workspace inherit)
#   - crates/domains/<name>/src/main.rs  (clap skeleton + doctor)
#   - crates/domains/<name>/domain.ncl   (Domain contract 적용)
#   - ncl/domains.ncl                    (import 라인 추가)
#
# 사용법:
#   scripts/new-domain.sh <name> <product> <layer> <platform> "<설명>"
#
#   product:  infra|desktop|ai|devops|service|monitor|tool|media|network
#   layer:    host|remote|tool
#   platform: proxmox|linux|any
#
# 예시:
#   scripts/new-domain.sh vaultwarden service remote proxmox "Vaultwarden 패스워드 매니저 LXC"

set -euo pipefail

NAME="${1:-}"
PRODUCT="${2:-}"
LAYER="${3:-}"
PLATFORM="${4:-}"
DESC="${5:-}"

print_usage() {
  sed -n '2,20p' "$0"
}

if [ -z "$NAME" ] || [ -z "$PRODUCT" ] || [ -z "$LAYER" ] || [ -z "$PLATFORM" ] || [ -z "$DESC" ]; then
  print_usage
  exit 1
fi

# name 포맷 검사 (domain_contract.ncl NameStr 와 동일 규약)
if ! [[ "$NAME" =~ ^[a-z][a-z0-9-]*$ ]]; then
  echo "✗ name 은 소문자+하이픈만 허용: $NAME" >&2
  exit 1
fi

# enum 검사
case "$PRODUCT" in
  infra|desktop|ai|devops|service|monitor|tool|media|network) ;;
  *) echo "✗ product 허용값: infra|desktop|ai|devops|service|monitor|tool|media|network (받음: $PRODUCT)" >&2; exit 1 ;;
esac
case "$LAYER" in
  host|remote|tool) ;;
  *) echo "✗ layer 허용값: host|remote|tool (받음: $LAYER)" >&2; exit 1 ;;
esac
case "$PLATFORM" in
  proxmox|linux|any) ;;
  *) echo "✗ platform 허용값: proxmox|linux|any (받음: $PLATFORM)" >&2; exit 1 ;;
esac

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIR="$ROOT/crates/domains/$NAME"

if [ -d "$DIR" ]; then
  echo "✗ 이미 존재: $DIR" >&2
  exit 1
fi

# ncl/domains.ncl 에 이미 있는지 미리 확인 (디렉토리는 없지만 import 만 있는 케이스)
if grep -q "^  $NAME\s*=\s*import " "$ROOT/ncl/domains.ncl" 2>/dev/null; then
  echo "✗ ncl/domains.ncl 에 이미 '$NAME' import 가 있음 — 수동 정리 필요" >&2
  exit 1
fi

mkdir -p "$DIR/src"

# ── 1) Cargo.toml (workspace inherit) ──
cat > "$DIR/Cargo.toml" <<EOF
[package]
name = "pxi-$NAME"
version.workspace = true
edition.workspace = true
license.workspace = true

[[bin]]
name = "pxi-$NAME"
path = "src/main.rs"

[dependencies]
clap = { workspace = true }
anyhow = { workspace = true }
pxi-core = { workspace = true }
EOF

# ── 2) src/main.rs (clap skeleton) ──
cat > "$DIR/src/main.rs" <<EOF
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "pxi-$NAME",
    version,
    about = "$DESC",
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 상태 점검
    Doctor,
    /// 상태 조회
    Status,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Doctor => doctor(),
        Cmd::Status => status(),
    }
}

fn doctor() -> anyhow::Result<()> {
    println!("✓ pxi-$NAME doctor — TODO");
    Ok(())
}

fn status() -> anyhow::Result<()> {
    println!("pxi-$NAME status — TODO");
    Ok(())
}
EOF

# ── 3) domain.ncl (Domain contract) ──
cat > "$DIR/domain.ncl" <<EOF
let { Domain } = import "../../../ncl/contracts/domain.ncl" in

{
  name = "$NAME",
  description = "$DESC",
  tags = { product = '$PRODUCT, layer = '$LAYER, platform = '$PLATFORM },
  provides = [
    "$NAME doctor",
    "$NAME status",
  ],
} | Domain
EOF

# ── 4) ncl/domains.ncl 에 import 추가 (알파벳 순 유지) ──
NCL="$ROOT/ncl/domains.ncl"
# 삽입 위치: 기존 import 라인들 사이에 알파벳 순. python 으로 정확히 처리.
python3 - "$NCL" "$NAME" <<'PYEOF'
import sys, re, pathlib

path = pathlib.Path(sys.argv[1])
name = sys.argv[2]
lines = path.read_text(encoding="utf-8").splitlines(keepends=False)

import_re = re.compile(r'^(\s+)([a-z][a-z0-9-]*)(\s+=\s+import\s+"\.\./crates/domains/)([a-z][a-z0-9-]*)/domain\.ncl",$')

idx_of = []
indent = "  "
for i, line in enumerate(lines):
    m = import_re.match(line)
    if m:
        idx_of.append((i, m.group(2)))
        indent = m.group(1)

if not idx_of:
    print("✗ 기존 import 블록을 찾지 못함 — 수동 편집 필요", file=sys.stderr)
    sys.exit(2)

# 알파벳 순 삽입 위치 계산
first_idx = idx_of[0][0]
last_idx = idx_of[-1][0]
existing_max_len = max(len(n) for _, n in idx_of)
new_pad = max(existing_max_len, len(name))

insert_pos = last_idx + 1
for i, (line_i, n) in enumerate(idx_of):
    if n > name:
        insert_pos = line_i
        break

new_line = f'{indent}{name.ljust(new_pad)} = import "../crates/domains/{name}/domain.ncl",'
lines.insert(insert_pos, new_line)

# 기존 import 라인도 padding 통일 (미관).
for i, (line_i, n) in enumerate(idx_of):
    # 재삽입 이후 인덱스 shift 반영
    cur = line_i if line_i < insert_pos else line_i + 1
    cur_line = lines[cur]
    m = import_re.match(cur_line)
    if m:
        lines[cur] = f'{m.group(1)}{n.ljust(new_pad)} = import "../crates/domains/{n}/domain.ncl",'

path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print(f"✓ ncl/domains.ncl 에 '{name}' import 추가")
PYEOF

# ── 5) Nickel 검증 (nickel CLI 있을 때만) ──
if command -v nickel >/dev/null 2>&1; then
  if nickel eval "$ROOT/ncl/domains.ncl" > /dev/null 2>&1; then
    echo "✓ nickel eval ncl/domains.ncl 통과"
  else
    echo "✗ nickel eval 실패 — 수동 확인 필요:" >&2
    nickel eval "$ROOT/ncl/domains.ncl" 2>&1 | head -20 >&2
    exit 1
  fi
fi

# ── 6) cargo check (선택) ──
if command -v cargo >/dev/null 2>&1 && [ -z "${PXI_SKIP_CARGO:-}" ]; then
  echo "▶ cargo check -p pxi-$NAME"
  if (cd "$ROOT" && cargo check -p "pxi-$NAME" 2>&1 | tail -5); then
    :
  else
    echo "⚠ cargo check 경고 — 수동 확인" >&2
  fi
fi

echo ""
echo "✓ 도메인 '$NAME' 스캐폴드 완료"
echo ""
echo "다음 단계:"
echo "  1. $DIR/src/main.rs 의 TODO 채우기"
echo "  2. cargo build --release -p pxi-$NAME"
echo "  3. scripts/install-local.sh  # locale.json 재생성 + 로컬 설치"
