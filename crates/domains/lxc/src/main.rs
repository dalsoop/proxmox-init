//! prelik-lxc — Proxmox LXC 수명 관리.
//! pct 바이너리를 전제로 함 (Proxmox VE 호스트에서만 동작).

use clap::{Parser, Subcommand};
use prelik_core::{common, os};

#[derive(Parser)]
#[command(name = "prelik-lxc", about = "LXC 수명 관리 (Proxmox pct 래퍼)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// LXC 목록
    List,
    /// LXC 상태
    Status { vmid: String },
    /// LXC 생성
    Create {
        #[arg(long)]
        vmid: String,
        #[arg(long)]
        hostname: String,
        /// IP (CIDR 포함 가능, 예: 10.0.50.181/16)
        #[arg(long)]
        ip: String,
        #[arg(long, default_value = "debian-13")]
        template: String,
        #[arg(long, default_value = "local-lvm")]
        storage: String,
        #[arg(long, default_value = "8")]
        disk: String,
        #[arg(long, default_value = "2")]
        cores: String,
        #[arg(long, default_value = "2048")]
        memory: String,
        /// 게이트웨이 (기본: config.toml의 network.gateway)
        #[arg(long)]
        gateway: Option<String>,
        #[arg(long, default_value = "vmbr1")]
        bridge: String,
    },
    /// LXC 시작
    Start { vmid: String },
    /// LXC 정지
    Stop { vmid: String },
    /// LXC 재시작
    Restart { vmid: String },
    /// LXC 삭제
    Delete {
        vmid: String,
        /// 백업 없이 강제 삭제
        #[arg(long)]
        force: bool,
    },
    /// LXC 셸 진입
    Enter { vmid: String },
    /// LXC 백업 (vzdump)
    Backup {
        vmid: String,
        #[arg(long, default_value = "local")]
        storage: String,
        #[arg(long, default_value = "snapshot")]
        mode: String,
    },
    /// 상태 점검 (pct 존재, PVE 노드 확인)
    Doctor,
}

fn main() -> anyhow::Result<()> {
    require_proxmox()?;
    match Cli::parse().cmd {
        Cmd::List => list(),
        Cmd::Status { vmid } => status(&vmid),
        Cmd::Create {
            vmid,
            hostname,
            ip,
            template,
            storage,
            disk,
            cores,
            memory,
            gateway,
            bridge,
        } => create(&vmid, &hostname, &ip, &template, &storage, &disk, &cores, &memory, gateway.as_deref(), &bridge),
        Cmd::Start { vmid } => start(&vmid),
        Cmd::Stop { vmid } => stop(&vmid),
        Cmd::Restart { vmid } => restart(&vmid),
        Cmd::Delete { vmid, force } => delete(&vmid, force),
        Cmd::Enter { vmid } => enter(&vmid),
        Cmd::Backup { vmid, storage, mode } => backup(&vmid, &storage, &mode),
        Cmd::Doctor => {
            doctor();
            Ok(())
        }
    }
}

fn require_proxmox() -> anyhow::Result<()> {
    if !common::has_cmd("pct") {
        anyhow::bail!("pct 바이너리 없음 — Proxmox VE 호스트에서만 동작합니다");
    }
    Ok(())
}

fn list() -> anyhow::Result<()> {
    let out = common::run("pct", &["list"])?;
    println!("{out}");
    Ok(())
}

fn status(vmid: &str) -> anyhow::Result<()> {
    let out = common::run("pct", &["status", vmid])?;
    println!("{out}");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn create(
    vmid: &str,
    hostname: &str,
    ip: &str,
    template: &str,
    storage: &str,
    disk: &str,
    cores: &str,
    memory: &str,
    gateway: Option<&str>,
    bridge: &str,
) -> anyhow::Result<()> {
    println!("=== LXC 생성: {vmid} ({hostname}) ===");

    // 템플릿 찾기 (부분 문자열 매칭)
    let templates = common::run("pveam", &["list", "local"])?;
    let full_template = templates
        .lines()
        .skip(1)
        .find(|l| l.contains(template))
        .and_then(|l| l.split_whitespace().next())
        .ok_or_else(|| anyhow::anyhow!("템플릿 '{template}' 을 찾을 수 없음 (pveam list local 확인)"))?;

    // IP에 CIDR 포함 여부 확인
    let ip_cidr = if ip.contains('/') {
        ip.to_string()
    } else {
        let cfg = prelik_core::config::Config::load().unwrap_or_default();
        let subnet = if cfg.network.subnet > 0 { cfg.network.subnet } else { 24 };
        format!("{ip}/{subnet}")
    };

    // 게이트웨이: 명시적 > config.toml > IP 첫 3옥텟 + .1
    let gw = if let Some(g) = gateway {
        g.to_string()
    } else {
        let cfg = prelik_core::config::Config::load().unwrap_or_default();
        if !cfg.network.gateway.is_empty() {
            cfg.network.gateway
        } else {
            let octets: Vec<&str> = ip.split('/').next().unwrap_or(ip).split('.').collect();
            if octets.len() >= 3 {
                format!("{}.{}.{}.1", octets[0], octets[1], octets[2])
            } else {
                anyhow::bail!("게이트웨이 추론 실패 — --gateway 명시 필요");
            }
        }
    };

    let net0 = format!("name=eth0,bridge={bridge},ip={ip_cidr},gw={gw}");

    println!("  template: {full_template}");
    println!("  storage:  {storage}, disk: {disk}G");
    println!("  cpu:      {cores}코어, ram: {memory}MB");
    println!("  net0:     {net0}");

    common::run(
        "pct",
        &[
            "create", vmid, full_template,
            "--hostname", hostname,
            "--storage", storage,
            "--rootfs", &format!("{storage}:{disk}"),
            "--cores", cores,
            "--memory", memory,
            "--net0", &net0,
            "--features", "nesting=1",
            "--unprivileged", "1",
            "--start", "1",
        ],
    )?;
    println!("✓ LXC {vmid} 생성 + 시작 완료");
    Ok(())
}

fn start(vmid: &str) -> anyhow::Result<()> {
    common::run("pct", &["start", vmid])?;
    println!("✓ LXC {vmid} 시작");
    Ok(())
}

fn stop(vmid: &str) -> anyhow::Result<()> {
    common::run("pct", &["stop", vmid])?;
    println!("✓ LXC {vmid} 정지");
    Ok(())
}

fn restart(vmid: &str) -> anyhow::Result<()> {
    common::run("pct", &["reboot", vmid])?;
    println!("✓ LXC {vmid} 재시작");
    Ok(())
}

fn delete(vmid: &str, force: bool) -> anyhow::Result<()> {
    // 실행 중이면 먼저 정지
    let status = common::run("pct", &["status", vmid]).unwrap_or_default();
    if status.contains("running") {
        common::run("pct", &["stop", vmid])?;
    }
    // 백업 권장 (force 아니면 경고)
    if !force {
        eprintln!(
            "⚠ 삭제 전 백업 권장: prelik-lxc backup {vmid}\n  또는 --force 로 무시"
        );
        anyhow::bail!("중단됨");
    }
    common::run("pct", &["destroy", vmid])?;
    println!("✓ LXC {vmid} 삭제");
    Ok(())
}

fn enter(vmid: &str) -> anyhow::Result<()> {
    // pct enter는 interactive라 status()와 다름
    let status = std::process::Command::new("pct")
        .args(["enter", vmid])
        .status()?;
    std::process::exit(status.code().unwrap_or(1));
}

fn backup(vmid: &str, storage: &str, mode: &str) -> anyhow::Result<()> {
    println!("=== LXC {vmid} 백업 ({storage}, {mode}) ===");
    common::run(
        "vzdump",
        &[vmid, "--storage", storage, "--mode", mode, "--compress", "zstd"],
    )?;
    println!("✓ 백업 완료");
    Ok(())
}

fn doctor() {
    println!("=== prelik-lxc doctor ===");
    println!("  pct:       {}", if common::has_cmd("pct") { "✓" } else { "✗" });
    println!("  vzdump:    {}", if common::has_cmd("vzdump") { "✓" } else { "✗" });
    println!("  pveam:     {}", if common::has_cmd("pveam") { "✓" } else { "✗" });
    println!("  pvesh:     {}", if common::has_cmd("pvesh") { "✓" } else { "✗" });
    println!("  proxmox:   {}", if os::is_proxmox() { "✓" } else { "✗" });
}
