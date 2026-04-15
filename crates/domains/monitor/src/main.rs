//! prelik-monitor — 호스트/LXC/VM 리소스 모니터링 (read-only).

use clap::{Parser, Subcommand};
use std::fs;
use std::process::Command;

#[derive(Parser)]
#[command(name = "prelik-monitor", about = "호스트/LXC/VM 리소스 모니터링")]
struct Cli {
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
        /// 실행 중인 것만
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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Doctor => doctor(),
        Cmd::Host => host(),
        Cmd::Lxc { running } => lxc(running),
        Cmd::Vm { running } => vm(running),
        Cmd::All => {
            host()?;
            if which("pct") {
                println!();
                lxc(true)?;
            }
            if which("qm") {
                println!();
                vm(true)?;
            }
            Ok(())
        }
    }
}

fn doctor() -> anyhow::Result<()> {
    println!("=== prelik-monitor doctor ===");
    println!("  /proc/loadavg : {}", path_ok("/proc/loadavg"));
    println!("  /proc/meminfo : {}", path_ok("/proc/meminfo"));
    println!("  df            : {}", bin_ok("df"));
    println!("  pct           : {} (선택, LXC 모니터링용)", bin_ok("pct"));
    println!("  qm            : {} (선택, VM 모니터링용)", bin_ok("qm"));
    Ok(())
}

fn path_ok(p: &str) -> &'static str {
    if std::path::Path::new(p).exists() { "✓" } else { "✗" }
}
fn bin_ok(b: &str) -> &'static str {
    if which(b) { "✓" } else { "✗" }
}
fn which(b: &str) -> bool {
    Command::new("which")
        .arg(b)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn host() -> anyhow::Result<()> {
    println!("=== 호스트 리소스 ===\n");

    // CPU / load
    let loadavg = fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let parts: Vec<&str> = loadavg.split_whitespace().collect();
    let cpus = Command::new("nproc")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "?".into());
    println!("[CPU] ({cpus} cores)");
    if parts.len() >= 3 {
        println!("  load avg: {} {} {} (1/5/15min)", parts[0], parts[1], parts[2]);
    }

    // 메모리
    println!("\n[메모리]");
    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mut mem_total = 0u64;
    let mut mem_avail = 0u64;
    let mut swap_total = 0u64;
    let mut swap_free = 0u64;
    for line in meminfo.lines() {
        if line.starts_with("MemTotal:") { mem_total = parse_kb(line); }
        else if line.starts_with("MemAvailable:") { mem_avail = parse_kb(line); }
        else if line.starts_with("SwapTotal:") { swap_total = parse_kb(line); }
        else if line.starts_with("SwapFree:") { swap_free = parse_kb(line); }
    }
    let mem_used = mem_total.saturating_sub(mem_avail);
    let mem_pct = if mem_total > 0 { mem_used * 100 / mem_total } else { 0 };
    println!(
        "  RAM:  {}GB / {}GB ({}%)",
        mem_used / 1_048_576,
        mem_total / 1_048_576,
        mem_pct
    );
    if swap_total > 0 {
        let swap_used = swap_total.saturating_sub(swap_free);
        println!(
            "  Swap: {}GB / {}GB ({}%)",
            swap_used / 1_048_576,
            swap_total / 1_048_576,
            swap_used * 100 / swap_total
        );
    }

    // 디스크
    println!("\n[디스크]");
    let df = Command::new("df")
        .args(["-h", "--type=ext4", "--type=btrfs", "--type=xfs", "--type=zfs"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    for line in df.lines().skip(1) {
        let p: Vec<&str> = line.split_whitespace().collect();
        if p.len() >= 6 {
            println!("  {:<24} {:>8} / {:>8} ({:>5}) {}", p[5], p[2], p[1], p[4], p[0]);
        }
    }

    // 온도
    if let Ok(temps) = fs::read_dir("/sys/class/thermal") {
        let mut shown = false;
        for ent in temps.flatten() {
            let p = ent.path();
            let temp_path = p.join("temp");
            if !temp_path.exists() { continue; }
            if let Ok(raw) = fs::read_to_string(&temp_path) {
                if let Ok(t) = raw.trim().parse::<u64>() {
                    if !shown { println!("\n[온도]"); shown = true; }
                    let zone = p.file_name().and_then(|n| n.to_str()).unwrap_or("zone");
                    println!("  {zone}: {}°C", t / 1000);
                }
            }
        }
    }

    // uptime
    if let Ok(out) = Command::new("uptime").arg("-p").output() {
        let up = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !up.is_empty() {
            println!("\n[uptime] {up}");
        }
    }

    Ok(())
}

fn parse_kb(line: &str) -> u64 {
    line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0)
}

fn lxc(running_only: bool) -> anyhow::Result<()> {
    if !which("pct") {
        println!("(pct 미설치 — Proxmox 호스트 아님)");
        return Ok(());
    }
    println!("=== LXC 리소스 ===\n");
    let out = Command::new("pct").arg("list").output()?;
    if !out.status.success() {
        anyhow::bail!("pct list 실패");
    }
    let text = String::from_utf8_lossy(&out.stdout);
    println!("{:<6} {:<10} {:<25} {:>8} {:>10}", "VMID", "STATUS", "NAME", "MEM%", "DISK%");
    for line in text.lines().skip(1) {
        let p: Vec<&str> = line.split_whitespace().collect();
        if p.len() < 3 { continue; }
        let vmid = p[0];
        let status = p[1];
        let name = p[2];
        if running_only && status != "running" { continue; }
        let (mem_pct, disk_pct) = if status == "running" {
            (lxc_mem_pct(vmid), lxc_disk_pct(vmid))
        } else {
            ("-".into(), "-".into())
        };
        println!("{:<6} {:<10} {:<25} {:>8} {:>10}", vmid, status, name, mem_pct, disk_pct);
    }
    Ok(())
}

fn lxc_mem_pct(vmid: &str) -> String {
    // pct exec <id> -- cat /proc/meminfo
    let out = Command::new("pct")
        .args(["exec", vmid, "--", "cat", "/proc/meminfo"])
        .output();
    let Ok(out) = out else { return "?".into() };
    if !out.status.success() { return "?".into(); }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut total = 0u64;
    let mut avail = 0u64;
    for line in text.lines() {
        if line.starts_with("MemTotal:") { total = parse_kb(line); }
        else if line.starts_with("MemAvailable:") { avail = parse_kb(line); }
    }
    if total == 0 { return "?".into(); }
    format!("{}%", (total - avail) * 100 / total)
}

fn lxc_disk_pct(vmid: &str) -> String {
    let out = Command::new("pct")
        .args(["exec", vmid, "--", "df", "-P", "/"])
        .output();
    let Ok(out) = out else { return "?".into() };
    if !out.status.success() { return "?".into(); }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines().skip(1) {
        let p: Vec<&str> = line.split_whitespace().collect();
        if p.len() >= 5 { return p[4].to_string(); }
    }
    "?".into()
}

fn vm(running_only: bool) -> anyhow::Result<()> {
    if !which("qm") {
        println!("(qm 미설치 — Proxmox 호스트 아님)");
        return Ok(());
    }
    println!("=== VM 리소스 ===\n");
    let out = Command::new("qm").arg("list").output()?;
    if !out.status.success() {
        anyhow::bail!("qm list 실패");
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // qm list 헤더: VMID NAME STATUS MEM(MB) BOOTDISK(GB) PID
    println!("{:<6} {:<25} {:<10} {:>10} {:>12}", "VMID", "NAME", "STATUS", "MEM(MB)", "DISK(GB)");
    for line in text.lines().skip(1) {
        let p: Vec<&str> = line.split_whitespace().collect();
        if p.len() < 5 { continue; }
        let vmid = p[0];
        let name = p[1];
        let status = p[2];
        let mem = p[3];
        let disk = p[4];
        if running_only && status != "running" { continue; }
        println!("{:<6} {:<25} {:<10} {:>10} {:>12}", vmid, name, status, mem, disk);
    }
    Ok(())
}
