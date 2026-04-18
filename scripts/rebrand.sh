#!/usr/bin/env bash
#
# rebrand.sh — 프로젝트 전체 이름 변경. 이 스크립트 하나로 끝.
#
# Usage:
#   ./scripts/rebrand.sh prelik pxi        # prelik → pxi
#   ./scripts/rebrand.sh pxi foobar        # pxi → foobar
#
set -euo pipefail

OLD="${1:?Usage: $0 <old-name> <new-name>}"
NEW="${2:?Usage: $0 <old-name> <new-name>}"

if [ "$OLD" = "$NEW" ]; then echo "same name, nothing to do"; exit 0; fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "=== Rebrand: $OLD → $NEW ==="

# 1. Cargo.toml — package names, bin names, deps
echo "[1/6] Cargo.toml files..."
find . -name 'Cargo.toml' -not -path '*/target/*' | while read f; do
  sed -i "s/${OLD}-/${NEW}-/g; s/\"${OLD}\"/\"${NEW}\"/g" "$f"
done

# 2. Rust source — string literals, const, paths
echo "[2/6] Rust source (.rs)..."
find . -name '*.rs' -not -path '*/target/*' | while read f; do
  sed -i "s/${OLD}-/${NEW}-/g; s/${OLD}_/${NEW}_/g; s/\"${OLD}\"/\"${NEW}\"/g" "$f"
  sed -i "s|/etc/${OLD}|/etc/${NEW}|g; s|/var/lib/${OLD}|/var/lib/${NEW}|g" "$f"
  sed -i "s|${OLD}\.com|${NEW}.com|g" "$f" 2>/dev/null || true
done

# 3. Scripts, configs, docs
echo "[3/6] Scripts + docs..."
find . \( -name '*.sh' -o -name '*.md' -o -name '*.toml' -o -name '*.yml' -o -name '*.yaml' -o -name '*.json' \) \
  -not -path '*/target/*' -not -name 'rebrand.sh' | while read f; do
  sed -i "s/${OLD}-/${NEW}-/g; s/${OLD}_/${NEW}_/g; s/\"${OLD}\"/\"${NEW}\"/g" "$f"
  sed -i "s|/etc/${OLD}|/etc/${NEW}|g; s|/var/lib/${OLD}|/var/lib/${NEW}|g" "$f"
done

# 4. System paths on disk (if running on the actual host)
echo "[4/6] System paths..."
if [ -d "/etc/${OLD}" ] && [ ! -d "/etc/${NEW}" ]; then
  mv "/etc/${OLD}" "/etc/${NEW}"
  ln -sf "/etc/${NEW}" "/etc/${OLD}"  # compat symlink
  echo "  /etc/${OLD} → /etc/${NEW}"
fi
if [ -d "/var/lib/${OLD}" ] && [ ! -d "/var/lib/${NEW}" ]; then
  mv "/var/lib/${OLD}" "/var/lib/${NEW}"
  ln -sf "/var/lib/${NEW}" "/var/lib/${OLD}"
  echo "  /var/lib/${OLD} → /var/lib/${NEW}"
fi

# 5. Symlinks in /usr/local/bin
echo "[5/6] Binary symlinks..."
for bin in /usr/local/bin/${OLD}-*; do
  [ -f "$bin" ] || continue
  old_name=$(basename "$bin")
  new_name="${old_name/${OLD}/${NEW}}"
  if [ -f "$bin" ] && [ ! -L "$bin" ]; then
    mv "$bin" "/usr/local/bin/$new_name"
  fi
  # Update phs-style symlinks too
  old_phs="/usr/local/bin/phs-${old_name#${OLD}-}"
  [ -L "$old_phs" ] && rm -f "$old_phs"
done
# Main binary
[ -f "/usr/local/bin/${OLD}" ] && [ ! -L "/usr/local/bin/${OLD}" ] && \
  mv "/usr/local/bin/${OLD}" "/usr/local/bin/${NEW}"

# 6. Verify
echo "[6/6] Verify..."
echo "  Cargo workspace:"
grep -c "${NEW}-" Cargo.toml || echo "  (check manually)"
echo "  Remaining old refs:"
COUNT=$(grep -r "${OLD}" --include='*.rs' --include='*.toml' -l . 2>/dev/null | grep -v target | grep -v rebrand.sh | wc -l)
echo "  $COUNT files still reference '$OLD'"

echo
echo "=== Done. Run: cargo build --release ==="
