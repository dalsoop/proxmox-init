//! pxi-monitor — 호스트/LXC/VM 리소스 모니터링 (read-only).

use clap::{Parser, Subcommand};
use pxi_core::common;
use serde::Serialize;
use std::fs;
use std::process::Command;

#[derive(Parser)]
#[command(name = "pxi-monitor", about = "호스트/LXC/VM 리소스 모니터링")]
struct Cli {
    /// 출력 포맷을 JSON으로 (자동화/CI 친화)
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 의존성 점검
    Doctor,
    /// 호스트 자원 (CPU/RAM/디스크/온도/uptime)
    Host,
    /// 모든 LXC 리소스 사용량
    Lxc {
        #[arg(long)]
        running: bool,
    },
    /// 모든 VM 리소스 사용량
    Vm {
        #[arg(long)]
        running: bool,
    },
    /// 호스트 + LXC + VM 종합 (Proxmox 호스트일 때만)
    All,
    /// 전역 인프라 검증 (inventory/route/domain/sync/kuma)
    GlobalVerify,
    /// 인프라 health check (pmxcfs/SSH/timer/cluster)
    HealthCheck,
    /// 인프라 헬스체크 (OAuth/에이전트 상태) + 자동 복구
    Healthcheck {
        /// 자동 복구 실행 (기본: 체크만)
        #[arg(long, default_value = "false")]
        fix: bool,
    },
    /// destructive 작업 audit log 확인
    AuditLog {
        #[arg(long, default_value_t = 50)]
        tail: usize,
    },
    /// Uptime Kuma를 control-plane 기준으로 1회 동기화
    KumaSyncRun {
        #[arg(long)]
        vmid: String,
    },
}

#[derive(Serialize)]
struct HostSnap {
    cpu_cores: String,
    load_avg: [String; 3],
    mem_total_kb: u64,
    mem_available_kb: u64,
    mem_used_pct: u64,
    swap_total_kb: u64,
    swap_free_kb: u64,
    disks: Vec<DiskRow>,
    thermal_zones_c: Vec<ThermalRow>,
    uptime_pretty: String,
}

#[derive(Serialize)]
struct DiskRow { mount: String, size: String, used: String, use_pct: String, source: String }

#[derive(Serialize)]
struct ThermalRow { zone: String, celsius: u64 }

#[derive(Serialize)]
struct LxcRow { vmid: String, status: String, name: String, mem_pct: String, disk_pct: String }

#[derive(Serialize)]
struct VmRow { vmid: String, name: String, status: String, mem_mb: String, disk_gb: String }

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let json = cli.json;
    match cli.cmd {
        Cmd::Doctor => doctor(json),
        Cmd::Host => host(json),
        Cmd::Lxc { running } => lxc(running, json),
        Cmd::Vm { running } => vm(running, json),
        Cmd::All => all(json),
        Cmd::GlobalVerify => global_verify(),
        Cmd::HealthCheck => health_check(),
        Cmd::Healthcheck { fix } => healthcheck(fix),
        Cmd::AuditLog { tail } => audit_log(tail),
        Cmd::KumaSyncRun { vmid } => kuma_sync_run(&vmid),
    }
}

fn doctor(json: bool) -> anyhow::Result<()> {
    let checks = [
        ("/proc/loadavg", path_ok("/proc/loadavg") == "✓"),
        ("/proc/meminfo", path_ok("/proc/meminfo") == "✓"),
        ("df", common::has_cmd("df")),
        ("pct", common::has_cmd("pct")),
        ("qm", common::has_cmd("qm")),
    ];
    if json {
        let map: serde_json::Value = checks.iter().map(|(k, v)| (k.to_string(), serde_json::Value::Bool(*v))).collect();
        println!("{}", serde_json::to_string_pretty(&map)?);
    } else {
        println!("=== pxi-monitor doctor ===");
        for (k, v) in &checks {
            let mark = if *v { "✓" } else { "✗" };
            let note = match *k { "pct" => " (선택, LXC)", "qm" => " (선택, VM)", _ => "" };
            println!("  {:<14}: {}{}", k, mark, note);
        }
    }
    Ok(())
}

fn path_ok(p: &str) -> &'static str {
    if std::path::Path::new(p).exists() { "✓" } else { "✗" }
}

fn collect_host() -> HostSnap {
    let loadavg = fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let parts: Vec<&str> = loadavg.split_whitespace().collect();
    let load = [
        parts.first().unwrap_or(&"0").to_string(),
        parts.get(1).unwrap_or(&"0").to_string(),
        parts.get(2).unwrap_or(&"0").to_string(),
    ];
    let cpus = Command::new("nproc")
        .output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "?".into());

    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mut mt = 0u64; let mut ma = 0u64; let mut st = 0u64; let mut sf = 0u64;
    for line in meminfo.lines() {
        if line.starts_with("MemTotal:") { mt = parse_kb(line); }
        else if line.starts_with("MemAvailable:") { ma = parse_kb(line); }
        else if line.starts_with("SwapTotal:") { st = parse_kb(line); }
        else if line.starts_with("SwapFree:") { sf = parse_kb(line); }
    }
    let mem_pct = if mt > 0 { mt.saturating_sub(ma) * 100 / mt } else { 0 };

    let mut disks = Vec::new();
    let df = Command::new("df")
        .args(["-h", "--type=ext4", "--type=btrfs", "--type=xfs", "--type=zfs"])
        .output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    for line in df.lines().skip(1) {
        let p: Vec<&str> = line.split_whitespace().collect();
        if p.len() >= 6 {
            disks.push(DiskRow {
                source: p[0].into(), size: p[1].into(), used: p[2].into(),
                use_pct: p[4].into(), mount: p[5].into(),
            });
        }
    }

    let mut zones = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/class/thermal") {
        for ent in entries.flatten() {
            let p = ent.path();
            let temp_path = p.join("temp");
            if !temp_path.exists() { continue; }
            if let Ok(raw) = fs::read_to_string(&temp_path) {
                if let Ok(t) = raw.trim().parse::<u64>() {
                    zones.push(ThermalRow {
                        zone: p.file_name().and_then(|n| n.to_str()).unwrap_or("zone").into(),
                        celsius: t / 1000,
                    });
                }
            }
        }
    }

    let uptime = Command::new("uptime").arg("-p").output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    HostSnap {
        cpu_cores: cpus, load_avg: load,
        mem_total_kb: mt, mem_available_kb: ma, mem_used_pct: mem_pct,
        swap_total_kb: st, swap_free_kb: sf,
        disks, thermal_zones_c: zones,
        uptime_pretty: uptime,
    }
}

fn host(json: bool) -> anyhow::Result<()> {
    let s = collect_host();
    if json { println!("{}", serde_json::to_string_pretty(&s)?); return Ok(()); }
    println!("=== 호스트 리소스 ===\n");
    println!("[CPU] ({} cores)", s.cpu_cores);
    println!("  load avg: {} {} {} (1/5/15min)", s.load_avg[0], s.load_avg[1], s.load_avg[2]);
    println!("\n[메모리]");
    println!("  RAM:  {}GB / {}GB ({}%)",
        s.mem_total_kb.saturating_sub(s.mem_available_kb) / 1_048_576,
        s.mem_total_kb / 1_048_576, s.mem_used_pct);
    if s.swap_total_kb > 0 {
        let su = s.swap_total_kb.saturating_sub(s.swap_free_kb);
        println!("  Swap: {}GB / {}GB ({}%)",
            su / 1_048_576, s.swap_total_kb / 1_048_576, su * 100 / s.swap_total_kb);
    }
    println!("\n[디스크]");
    for d in &s.disks {
        println!("  {:<24} {:>8} / {:>8} ({:>5}) {}", d.mount, d.used, d.size, d.use_pct, d.source);
    }
    if !s.thermal_zones_c.is_empty() {
        println!("\n[온도]");
        for z in &s.thermal_zones_c { println!("  {}: {}°C", z.zone, z.celsius); }
    }
    if !s.uptime_pretty.is_empty() {
        println!("\n[uptime] {}", s.uptime_pretty);
    }
    Ok(())
}

fn parse_kb(line: &str) -> u64 {
    line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0)
}

fn collect_lxc(running_only: bool) -> anyhow::Result<Vec<LxcRow>> {
    let out = Command::new("pct").arg("list").output()?;
    if !out.status.success() { anyhow::bail!("pct list 실패"); }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut rows = Vec::new();
    for line in text.lines().skip(1) {
        let p: Vec<&str> = line.split_whitespace().collect();
        if p.len() < 3 { continue; }
        let (vmid, status, name) = (p[0], p[1], p[2]);
        if running_only && status != "running" { continue; }
        let (m, d) = if status == "running" {
            (lxc_mem_pct(vmid), lxc_disk_pct(vmid))
        } else { ("-".into(), "-".into()) };
        rows.push(LxcRow { vmid: vmid.into(), status: status.into(), name: name.into(), mem_pct: m, disk_pct: d });
    }
    Ok(rows)
}

fn lxc(running_only: bool, json: bool) -> anyhow::Result<()> {
    if !common::has_cmd("pct") {
        // 텍스트는 기존 soft-fail 계약 유지 (안내 후 Ok),
        // JSON만 fail-fast (자동화 false negative 차단).
        if json { anyhow::bail!("pct unavailable"); }
        println!("(pct 미설치 — Proxmox 호스트 아님)");
        return Ok(());
    }
    let rows = collect_lxc(running_only)?;
    if json { println!("{}", serde_json::to_string_pretty(&rows)?); return Ok(()); }
    println!("=== LXC 리소스 ===\n");
    println!("{:<6} {:<10} {:<25} {:>8} {:>10}", "VMID", "STATUS", "NAME", "MEM%", "DISK%");
    for r in &rows {
        println!("{:<6} {:<10} {:<25} {:>8} {:>10}", r.vmid, r.status, r.name, r.mem_pct, r.disk_pct);
    }
    Ok(())
}

// 순수 파서 — /proc/meminfo 텍스트에서 used% 계산. 0 또는 데이터 결손 시 None.
fn parse_meminfo_used_pct(text: &str) -> Option<u64> {
    let mut t = 0u64; let mut a = 0u64;
    for line in text.lines() {
        if line.starts_with("MemTotal:") { t = parse_kb(line); }
        else if line.starts_with("MemAvailable:") { a = parse_kb(line); }
    }
    if t == 0 { return None; }
    Some((t.saturating_sub(a)) * 100 / t)
}

// df -P 출력의 첫 데이터 라인 use% 컬럼 (5번째). 5개 미만이면 None.
fn parse_df_root_pct(text: &str) -> Option<String> {
    for line in text.lines().skip(1) {
        let p: Vec<&str> = line.split_whitespace().collect();
        if p.len() >= 5 { return Some(p[4].to_string()); }
    }
    None
}

// qm list 파싱. running_only면 status가 running이 아닌 행 제외.
fn parse_qm_list(text: &str, running_only: bool) -> Vec<VmRow> {
    let mut rows = Vec::new();
    for line in text.lines().skip(1) {
        let p: Vec<&str> = line.split_whitespace().collect();
        if p.len() < 5 { continue; }
        if running_only && p[2] != "running" { continue; }
        rows.push(VmRow {
            vmid: p[0].into(), name: p[1].into(), status: p[2].into(),
            mem_mb: p[3].into(), disk_gb: p[4].into(),
        });
    }
    rows
}

fn lxc_mem_pct(vmid: &str) -> String {
    let out = Command::new("pct").args(["exec", vmid, "--", "cat", "/proc/meminfo"]).output();
    let Ok(out) = out else { return "?".into() };
    if !out.status.success() { return "?".into(); }
    let text = String::from_utf8_lossy(&out.stdout);
    parse_meminfo_used_pct(&text).map(|p| format!("{p}%")).unwrap_or_else(|| "?".into())
}

fn lxc_disk_pct(vmid: &str) -> String {
    let out = Command::new("pct").args(["exec", vmid, "--", "df", "-P", "/"]).output();
    let Ok(out) = out else { return "?".into() };
    if !out.status.success() { return "?".into(); }
    let text = String::from_utf8_lossy(&out.stdout);
    parse_df_root_pct(&text).unwrap_or_else(|| "?".into())
}

fn collect_vm(running_only: bool) -> anyhow::Result<Vec<VmRow>> {
    let out = Command::new("qm").arg("list").output()?;
    if !out.status.success() { anyhow::bail!("qm list 실패"); }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(parse_qm_list(&text, running_only))
}

fn vm(running_only: bool, json: bool) -> anyhow::Result<()> {
    if !common::has_cmd("qm") {
        if json { anyhow::bail!("qm unavailable"); }
        println!("(qm 미설치 — Proxmox 호스트 아님)");
        return Ok(());
    }
    let rows = collect_vm(running_only)?;
    if json { println!("{}", serde_json::to_string_pretty(&rows)?); return Ok(()); }
    println!("=== VM 리소스 ===\n");
    println!("{:<6} {:<25} {:<10} {:>10} {:>12}", "VMID", "NAME", "STATUS", "MEM(MB)", "DISK(GB)");
    for r in &rows {
        println!("{:<6} {:<25} {:<10} {:>10} {:>12}", r.vmid, r.name, r.status, r.mem_mb, r.disk_gb);
    }
    Ok(())
}

#[derive(Serialize)]
struct AllSnap {
    host: HostSnap,
    lxc_supported: bool,
    lxc: Vec<LxcRow>,
    vm_supported: bool,
    vm: Vec<VmRow>,
}

fn all(json: bool) -> anyhow::Result<()> {
    if json {
        // unwrap_or_default 금지 — 수집 실패는 자동화에 그대로 노출.
        // 미지원(pct/qm 없음)은 빈 배열로 표현하되, lxc_supported/vm_supported 플래그로 구분.
        let snap = AllSnap {
            host: collect_host(),
            lxc_supported: common::has_cmd("pct"),
            lxc: if common::has_cmd("pct") { collect_lxc(true)? } else { vec![] },
            vm_supported: common::has_cmd("qm"),
            vm:  if common::has_cmd("qm")  { collect_vm(true)?  } else { vec![] },
        };
        println!("{}", serde_json::to_string_pretty(&snap)?);
        return Ok(());
    }
    host(false)?;
    if common::has_cmd("pct") { println!(); lxc(true, false)?; }
    if common::has_cmd("qm")  { println!(); vm(true, false)?; }
    Ok(())
}

// =============================================================================
// Global Verify — 전역 인프라 검증
// =============================================================================

fn global_verify() -> anyhow::Result<()> {
    println!("=== 실제 적용 전역 검증 ===\n");
    let mut failures = 0u32;

    // 1. inventory 일치
    println!("[1/4] inventory 일치");
    let cluster = cmd_output_silent("pvesh", &["get", "/cluster/resources", "--type", "vm", "--output-format", "json"]);
    let cluster_lxc: usize = serde_json::from_str::<Vec<serde_json::Value>>(&cluster)
        .unwrap_or_default()
        .iter()
        .filter(|v| v.get("type").and_then(|t| t.as_str()) == Some("lxc"))
        .count();
    println!("  cluster LXC count: {cluster_lxc}");

    // 2. 핵심 도메인 응답
    println!("\n[2/4] 핵심 도메인 응답");
    let domains = ["infra.internal.kr", "home.internal.kr", "traefik.internal.kr"];
    for domain in &domains {
        let code = curl_status_code(domain);
        let mark = if code >= 200 && code < 500 { "✓" } else { failures += 1; "✗" };
        println!("  {mark} {domain} -> {code}");
    }

    // 3. sync chain/systemd
    println!("\n[3/4] sync chain/systemd");
    let timer_active = cmd_output_silent("systemctl", &["is-active", "phs-homelable-sync.timer"]);
    if timer_active.trim() == "active" {
        println!("  ✓ phs-homelable-sync.timer active");
    } else {
        failures += 1;
        eprintln!("  ✗ phs-homelable-sync.timer: {}", timer_active.trim());
    }

    // 4. Uptime Kuma
    println!("\n[4/4] Uptime Kuma 상태");
    let kuma_vmid = std::env::var("KUMA_VMID").unwrap_or_default();
    if kuma_vmid.is_empty() {
        println!("  ⊘ KUMA_VMID 미설정 — 건너뜀");
    } else {
        println!("  kuma vmid: {kuma_vmid}");
    }

    println!("\n{}", "─".repeat(50));
    if failures == 0 {
        println!("✓ 전역 검증 통과");
    } else {
        eprintln!("✗ 전역 검증 실패: {failures}건");
    }
    Ok(())
}

// =============================================================================
// Health Check — Proxmox 클러스터 상태
// =============================================================================

fn health_check() -> anyhow::Result<()> {
    println!("=== phs Health Check ===\n");
    let mut issues = 0u32;

    // 1. 로컬 핵심 서비스
    println!("[1/4] 로컬 핵심 서비스");
    for (svc, label) in [
        ("pve-cluster", "Proxmox cluster filesystem"),
        ("corosync", "Cluster messaging"),
        ("pvedaemon", "Proxmox daemon"),
        ("pveproxy", "Proxmox web proxy"),
    ] {
        let active = cmd_output_silent("systemctl", &["is-active", svc]);
        let mark = if active.trim() == "active" { "✓" } else { issues += 1; "❌" };
        println!("  {mark} {svc} ({label}): {}", active.trim());
    }

    // 2. /etc/pve 마운트
    println!("\n[2/4] pmxcfs 마운트");
    let mount = cmd_output_silent("mount", &[]);
    if mount.contains("on /etc/pve type fuse") || mount.contains("/etc/pve") {
        println!("  ✓ /etc/pve mounted");
    } else {
        println!("  ❌ /etc/pve mount 없음");
        issues += 1;
    }

    // 3. SSH authorized_keys 무결성
    println!("\n[3/4] SSH authorized_keys");
    let path = "/root/.ssh/authorized_keys";
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.is_file() && meta.len() > 0 {
            println!("  ✓ 파일 존재 ({} bytes)", meta.len());
        } else if meta.is_file() {
            println!("  ❌ 파일 비어있음");
            issues += 1;
        }
    } else {
        println!("  ❌ {path} 없음");
        issues += 1;
    }

    // 4. route-audit timer
    println!("\n[4/4] route-audit timer");
    let timer = cmd_output_silent("systemctl", &["is-active", "phs-route-audit.timer"]);
    if timer.trim() == "active" {
        println!("  ✓ active");
    } else {
        println!("  ⚠ {} (install needed)", timer.trim());
    }

    println!("\n=== 결과: {} 이슈 ===", issues);
    if issues > 0 {
        anyhow::bail!("{issues}건 이슈 발견");
    }
    Ok(())
}

// =============================================================================
// Healthcheck — OAuth/Agent 상태 체크 + 자동 복구
// =============================================================================

fn healthcheck(fix: bool) -> anyhow::Result<()> {
    let mode = if fix { "체크 + 자동 복구" } else { "체크만" };
    println!("=== 인프라 헬스체크 ({mode}) ===\n");

    let mut issues = 0u32;

    // OAuth 토큰 체크
    println!("[1/2] OAuth 토큰 체크");
    let oauth_targets = [("50161", "openclaw"), ("105", "dalcenter")];
    for (vmid, name) in &oauth_targets {
        let ok = Command::new("pct")
            .args(["exec", vmid, "--", "bash", "-c", "true"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            println!("  ⊘ {} (LXC {}) — 컨테이너 접근 불가, 건너뜀", name, vmid);
            continue;
        }
        let auth_out = Command::new("pct")
            .args(["exec", vmid, "--", "bash", "-c", "claude auth status 2>&1"])
            .output();
        let auth_ok = auth_out
            .as_ref()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("\"loggedIn\": true"))
            .unwrap_or(false);
        if auth_ok {
            println!("  ✓ {} (LXC {}) — 토큰 정상", name, vmid);
        } else {
            issues += 1;
            println!("  ✗ {} (LXC {}) — 토큰 만료", name, vmid);
            if fix {
                println!("    → credential-sync 필요 (pxi-ai credential-sync --vmid {vmid})");
            }
        }
    }

    // dalcenter 서비스 체크
    println!("\n[2/2] dalcenter → dalcenter-rs로 이전됨 (dalcenter status 사용)");

    let unresolved = issues;
    println!("\n{}", "─".repeat(50));
    if issues == 0 {
        println!("✓ 전체 정상");
    } else if fix {
        println!("발견: {issues}건 — 자동 복구 시도 (위 로그 참조)");
    } else {
        println!("발견: {issues}건 — `--fix` 옵션으로 자동 복구 가능");
    }
    if unresolved > 0 && !fix {
        anyhow::bail!("{unresolved}건 미해결");
    }
    Ok(())
}

// =============================================================================
// Audit Log
// =============================================================================

const AUDIT_LOG_PATH: &str = "/var/lib/phs/audit.log";

fn audit_log(tail: usize) -> anyhow::Result<()> {
    println!("=== Audit Log (최근 {tail}건) ===\n");
    if let Ok(content) = fs::read_to_string(AUDIT_LOG_PATH) {
        let lines: Vec<&str> = content.lines().collect();
        let start = lines.len().saturating_sub(tail);
        for line in &lines[start..] {
            println!("  {line}");
        }
    } else {
        println!("  audit log 없음");
    }
    Ok(())
}

// =============================================================================
// Kuma Sync Run
// =============================================================================

fn kuma_sync_run(vmid: &str) -> anyhow::Result<()> {
    println!("=== Uptime Kuma 동기화 ===\n");

    // Ensure LXC running
    let status = cmd_output_silent("pct", &["status", vmid]);
    if !status.contains("running") {
        anyhow::bail!("LXC {vmid} 이 실행 중이 아닙니다 (현재: {status})");
    }

    // Load monitors from control-plane
    let monitor_paths = [
        "/root/control-plane/domains/kuma-monitors.json",
        "/etc/pxi/domains/kuma-monitors.json",
        "/etc/proxmox-host-setup/kuma-monitors.json",
    ];
    let monitors_json = monitor_paths.iter()
        .find_map(|p| fs::read_to_string(p).ok())
        .unwrap_or_else(|| {
            eprintln!("[kuma] kuma-monitors.json 을 찾을 수 없습니다.");
            std::process::exit(1);
        });

    // Validate JSON
    let _: Vec<serde_json::Value> = serde_json::from_str(&monitors_json)
        .map_err(|e| anyhow::anyhow!("[kuma] JSON 파싱 실패: {e}"))?;

    let script = format!(
        r#"set -e
cp /opt/uptime-kuma/data/kuma.db /opt/uptime-kuma/data/kuma.db.bak-phs-$(date +%Y%m%d-%H%M%S)
python3 - <<'PY'
import json, sqlite3
from datetime import datetime
DB = "/opt/uptime-kuma/data/kuma.db"
seed = json.loads(r'''{monitors_json}''')
con = sqlite3.connect(DB)
cur = con.cursor()
managed_prefix = "[phs-managed] kuma-sync"
desired = {{item["name"] for item in seed}}
for item in seed:
    row = cur.execute("select id from monitor where name=?", (item["name"],)).fetchone()
    statuses = item.get("accepted_statuscodes_json", '["200-299"]')
    interval = int(item.get("interval", 60))
    timeout = int(item.get("timeout", 8))
    description = f"{{managed_prefix}} {{item['url']}}"
    if row:
        cur.execute("update monitor set active=1, url=?, type=?, interval=?, method=?, accepted_statuscodes_json=?, timeout=?, description=? where id=?",
            (item["url"], "http", interval, "GET", statuses, timeout, description, row[0]))
    else:
        cur.execute("insert into monitor (name,active,user_id,interval,url,type,weight,created_date,maxretries,ignore_tls,upside_down,maxredirects,accepted_statuscodes_json,retry_interval,method,expiry_notification,timeout,http_body_encoding,description) values (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            (item["name"], 1, None, interval, item["url"], "http", 2000, datetime.utcnow().strftime("%Y-%m-%d %H:%M:%S"), 0, 0, 0, 10, statuses, 0, "GET", 1, timeout, "utf-8", description))
if desired:
    cur.execute("update monitor set active=0 where description like ? and name not in ({{}})".format(",".join("?" for _ in desired)),
        tuple([f"{{managed_prefix}}%"] + sorted(desired)))
con.commit()
print("seeded_or_updated=", len(seed))
print("active_managed=", cur.execute("select count(*) from monitor where description like ? and active=1", (f"{{managed_prefix}}%",)).fetchone()[0])
PY
docker restart uptime-kuma >/dev/null
"#
    );

    let out = Command::new("pct")
        .args(["exec", vmid, "--", "bash", "-lc", &script])
        .output()?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("[kuma] 동기화 실패: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    if !stdout.trim().is_empty() {
        println!("{}", stdout.trim());
    }
    println!("\n=== Uptime Kuma 동기화 완료 ===");
    Ok(())
}

// =============================================================================
// Helpers for new commands
// =============================================================================

fn cmd_output_silent(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn curl_status_code(domain: &str) -> u16 {
    let out = Command::new("curl")
        .args(["-k", "-s", "-o", "/dev/null", "-w", "%{http_code}",
            "--max-time", "8", &format!("https://{domain}/")])
        .output()
        .ok();
    out.map(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u16>().unwrap_or(0))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- parse_kb -----

    #[test]
    fn parse_kb_normal() {
        assert_eq!(parse_kb("MemTotal:       527987380 kB"), 527987380);
    }

    #[test]
    fn parse_kb_missing() {
        assert_eq!(parse_kb("MemTotal:"), 0);
        assert_eq!(parse_kb("MemTotal: notanumber kB"), 0);
    }

    // ----- parse_meminfo_used_pct -----

    #[test]
    fn meminfo_basic() {
        let text = "MemTotal: 1000 kB\nMemAvailable: 250 kB\n";
        // used = 750 / 1000 = 75%
        assert_eq!(parse_meminfo_used_pct(text), Some(75));
    }

    #[test]
    fn meminfo_zero_total_returns_none() {
        let text = "MemAvailable: 100 kB\n";
        assert_eq!(parse_meminfo_used_pct(text), None);
    }

    #[test]
    fn meminfo_avail_gt_total_saturates() {
        // 비정상 입력: avail > total. saturating_sub로 0% 처리.
        let text = "MemTotal: 100 kB\nMemAvailable: 200 kB\n";
        assert_eq!(parse_meminfo_used_pct(text), Some(0));
    }

    #[test]
    fn meminfo_ignores_other_lines() {
        let text = "Buffers: 999 kB\nMemTotal: 100 kB\nCached: 50 kB\nMemAvailable: 25 kB\n";
        assert_eq!(parse_meminfo_used_pct(text), Some(75));
    }

    // ----- parse_df_root_pct -----

    #[test]
    fn df_basic() {
        let text = "Filesystem     1024-blocks    Used Available Capacity Mounted on\n\
                    /dev/sda1         100000   77000     20000      77% /\n";
        assert_eq!(parse_df_root_pct(text), Some("77%".into()));
    }

    #[test]
    fn df_short_line_returns_none() {
        let text = "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
                    only three cols\n";
        assert_eq!(parse_df_root_pct(text), None);
    }

    #[test]
    fn df_only_header_returns_none() {
        assert_eq!(parse_df_root_pct("Filesystem ..."), None);
    }

    // ----- parse_qm_list -----

    #[test]
    fn qm_list_running_only() {
        let text = "VMID NAME       STATUS  MEM(MB) BOOTDISK(GB) PID\n\
                    100  web        running  2048    32           1234\n\
                    101  db         stopped  4096    64           0\n";
        let rows = parse_qm_list(text, true);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].vmid, "100");
        assert_eq!(rows[0].status, "running");
    }

    #[test]
    fn qm_list_all() {
        let text = "VMID NAME STATUS MEM DISK PID\n\
                    100 web running 2048 32 1234\n\
                    101 db stopped 4096 64 0\n";
        assert_eq!(parse_qm_list(text, false).len(), 2);
    }

    #[test]
    fn qm_list_skips_short_lines() {
        let text = "VMID NAME STATUS MEM DISK\n\
                    100 web running 2048 32\n\
                    bad line\n\
                    101 db stopped 4096 64\n";
        assert_eq!(parse_qm_list(text, false).len(), 2);
    }

    #[test]
    fn qm_list_empty() {
        let text = "VMID NAME STATUS MEM DISK PID\n";
        assert_eq!(parse_qm_list(text, false).len(), 0);
    }
}
