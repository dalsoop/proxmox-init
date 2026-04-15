//! prelik-monitor — 호스트/LXC/VM 리소스 모니터링 (read-only).

use clap::{Parser, Subcommand};
use prelik_core::common;
use serde::Serialize;
use std::fs;
use std::process::Command;

#[derive(Parser)]
#[command(name = "prelik-monitor", about = "호스트/LXC/VM 리소스 모니터링")]
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
        println!("=== prelik-monitor doctor ===");
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
        // "지원 안 됨"을 빈 결과로 위장하지 않음 — 자동화가 false negative를 내지 않도록.
        if !json { eprintln!("(pct 미설치 — Proxmox 호스트 아님)"); }
        anyhow::bail!("pct unavailable");
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

fn lxc_mem_pct(vmid: &str) -> String {
    let out = Command::new("pct").args(["exec", vmid, "--", "cat", "/proc/meminfo"]).output();
    let Ok(out) = out else { return "?".into() };
    if !out.status.success() { return "?".into(); }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut t = 0u64; let mut a = 0u64;
    for line in text.lines() {
        if line.starts_with("MemTotal:") { t = parse_kb(line); }
        else if line.starts_with("MemAvailable:") { a = parse_kb(line); }
    }
    if t == 0 { return "?".into(); }
    format!("{}%", (t - a) * 100 / t)
}

fn lxc_disk_pct(vmid: &str) -> String {
    let out = Command::new("pct").args(["exec", vmid, "--", "df", "-P", "/"]).output();
    let Ok(out) = out else { return "?".into() };
    if !out.status.success() { return "?".into(); }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines().skip(1) {
        let p: Vec<&str> = line.split_whitespace().collect();
        if p.len() >= 5 { return p[4].to_string(); }
    }
    "?".into()
}

fn collect_vm(running_only: bool) -> anyhow::Result<Vec<VmRow>> {
    let out = Command::new("qm").arg("list").output()?;
    if !out.status.success() { anyhow::bail!("qm list 실패"); }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
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
    Ok(rows)
}

fn vm(running_only: bool, json: bool) -> anyhow::Result<()> {
    if !common::has_cmd("qm") {
        if !json { eprintln!("(qm 미설치 — Proxmox 호스트 아님)"); }
        anyhow::bail!("qm unavailable");
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
