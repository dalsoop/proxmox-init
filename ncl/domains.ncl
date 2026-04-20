# pxi 도메인 SSOT 인덱스 — 실제 도메인 정의는 crates/domains/*/domain.ncl.
# 각 도메인 파일은 ncl/contracts/domain.ncl 의 Domain contract 를 준수하므로,
# 이 파일을 eval 하는 것만으로 29개 전부 검증됨.
#
# 추가로 이 파일은 top-level 에서:
#   - requires 에 적힌 도메인이 실제로 존재하는 이름인지 확인 (codex #46 P1)
# 검증을 수행한다. 누락되면 nickel eval 이 명시적으로 실패 → CI 게이트.

let domains_map = {
  account     = import "../crates/domains/account/domain.ncl",
  ai          = import "../crates/domains/ai/domain.ncl",
  backup      = import "../crates/domains/backup/domain.ncl",
  bootstrap   = import "../crates/domains/bootstrap/domain.ncl",
  cloudflare  = import "../crates/domains/cloudflare/domain.ncl",
  code-server = import "../crates/domains/code-server/domain.ncl",
  comfyui     = import "../crates/domains/comfyui/domain.ncl",
  connect     = import "../crates/domains/connect/domain.ncl",
  deploy      = import "../crates/domains/deploy/domain.ncl",
  elk         = import "../crates/domains/elk/domain.ncl",
  host        = import "../crates/domains/host/domain.ncl",
  infisical   = import "../crates/domains/infisical/domain.ncl",
  iso         = import "../crates/domains/iso/domain.ncl",
  license     = import "../crates/domains/license/domain.ncl",
  lxc         = import "../crates/domains/lxc/domain.ncl",
  mail        = import "../crates/domains/mail/domain.ncl",
  ministack   = import "../crates/domains/ministack/domain.ncl",
  monitor     = import "../crates/domains/monitor/domain.ncl",
  nas         = import "../crates/domains/nas/domain.ncl",
  net         = import "../crates/domains/net/domain.ncl",
  node        = import "../crates/domains/node/domain.ncl",
  recovery    = import "../crates/domains/recovery/domain.ncl",
  service     = import "../crates/domains/service/domain.ncl",
  telegram    = import "../crates/domains/telegram/domain.ncl",
  traefik     = import "../crates/domains/traefik/domain.ncl",
  vaultwarden = import "../crates/domains/vaultwarden/domain.ncl",
  vm          = import "../crates/domains/vm/domain.ncl",
  wordpress   = import "../crates/domains/wordpress/domain.ncl",
  workspace   = import "../crates/domains/workspace/domain.ncl",
  xdesktop    = import "../crates/domains/xdesktop/domain.ncl",
}
in

let all_names = std.record.fields domains_map in

# 모든 도메인의 requires 를 flat 하게 모아 존재하지 않는 이름 추출.
let all_requires = std.array.flat_map (fun d =>
  let r = domains_map."%{d}" in
  if std.record.has_field "requires" r then r.requires else []
) all_names in

let unknown = std.array.filter (fun r =>
  !(std.array.any ((==) r) all_names)
) all_requires in

if std.array.length unknown > 0 then
  std.fail_with "requires references unknown domain(s): %{std.string.join ", " unknown}"
else
  # codex #47 P2: locale.json 포맷 버전 명시 — 향후 스키마 변경 시 Registry::load 가
  # graceful migration 판단 가능. runtime consumer 가 모르는 버전이면 hard-fail.
  {
    format_version = 1,
    domains = domains_map,
  }
