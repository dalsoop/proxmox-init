#!/usr/bin/env bash
# Users 줄(format[0]) 우측 상단 레이아웃 정리 — 멱등.
#   1) Panes 줄(format[3])의 split 버튼(| -)을 Users 줄 끝으로 이식
#   2) Users 줄 맨 오른쪽에 큰 톱니바퀴(⚙) 추가
# format 이 이미 정리된 상태면 no-op. render-status left 가 format 을 리셋한 직후
# 곧바로 호출되는 것을 전제로 함 (훅 콤보 + path 유닛).
set -euo pipefail

# 훅 콤보(render-status left; patch)에서 bash 는 즉시 다음 커맨드로 넘어가지만
# tmux 서버가 set-option 을 반영하는 데 미세한 지연이 있으므로 살짝 대기.
sleep 0.2

# 순수 format 이식만. windowbar apply 는 호출하지 않는다 (훅 재설정 부작용 방지).

python3 - <<'PY'
import subprocess, re, sys
def show(i):
    return subprocess.run(["tmux","show","-gv",f"status-format[{i}]"],
                          capture_output=True, text=True).stdout.rstrip("\n")
def setfmt(i, v):
    subprocess.run(["tmux","set","-g",f"status-format[{i}]", v], check=True)

f3, f0 = show(3), show(0)
if not f3 or not f0:
    sys.exit(0)

SPLIT_RE = re.compile(
    r'\s*#\[range=user\|_splith\].*?#\[norange default\]'
    r'#\[range=user\|_splitv\].*?#\[norange default\]'
)
GEAR_RE = re.compile(r'#\[range=user\|_settings\].*?#\[norange default\]')
GEAR = (
    '#[range=user|_settings]'
    '#[fg=#282c34,bg=#e06c75,bold]   \u2699   '
    '#[norange default]'
)
SPACER_SPLIT = '#[fg=#282c34,bg=#282c34]      #[default]'
SPACER_GEAR  = '#[fg=#282c34,bg=#282c34]      #[default]'

# split 출처 우선순위: format[3] → format[0] (이전에 이식됐던 값). 둘 다 없으면 생략.
m3 = SPLIT_RE.search(f3)
m0 = SPLIT_RE.search(f0)
if m3:
    splits = m3.group(0).lstrip()
elif m0:
    splits = m0.group(0).lstrip()
else:
    splits = ''

new_f3 = SPLIT_RE.sub('', f3)

# 이전 호출에서 붙인 spacer 가 누적되지 않도록 _rotate(마지막 기본 버튼) 뒤를 싹 자르고 재조립.
CUT_RE = re.compile(r'(#\[range=user\|_rotate\].*?#\[norange default\]).*$', re.DOTALL)
cm = CUT_RE.search(f0)
if cm:
    base_f0 = f0[:cm.end(1)]
else:
    base_f0 = SPLIT_RE.sub('', GEAR_RE.sub('', f0))

tail = (SPACER_SPLIT + splits) if splits else ''
tail += SPACER_GEAR + GEAR
new_f0 = base_f0 + tail

if new_f3 != f3: setfmt(3, new_f3)
if new_f0 != f0: setfmt(0, new_f0)
PY
