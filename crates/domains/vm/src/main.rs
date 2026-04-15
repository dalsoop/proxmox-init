//! prelik-vm — Proxmox QEMU VM 관리 (qm 래퍼).
//! LXC와 별개. vzdump는 LXC와 공통.

use clap::{Parser, Subcommand};
use prelik_core::common;

#[derive(Parser)]
#[command(name = "prelik-vm", about = "Proxmox QEMU VM 관리")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// VM 목록
    List,
    /// VM 상태
    Status { vmid: String },
    /// VM 시작
    Start { vmid: String },
    /// VM 정지 (graceful shutdown, 타임아웃 시 강제)
    Stop { vmid: String },
    /// VM 재시작 (reset)
    Reboot { vmid: String },
    /// VM 삭제 (purge — 디스크까지)
    Delete {
        vmid: String,
        #[arg(long)]
        force: bool,
    },
    /// VM 백업
    Backup {
        vmid: String,
        #[arg(long, default_value = "local")]
        storage: String,
        #[arg(long, default_value = "snapshot")]
        mode: String,
    },
    /// VM 리소스 변경
    Resize {
        vmid: String,
        #[arg(long)]
        cores: Option<String>,
        #[arg(long)]
        memory: Option<String>,
    },
    /// 콘솔 접속 (qm terminal)
    Console { vmid: String },
    Doctor,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if !matches!(cli.cmd, Cmd::Doctor) && !common::has_cmd("qm") {
        anyhow::bail!("qm 없음 — Proxmox 호스트에서만 동작");
    }
    match cli.cmd {
        Cmd::List => list(),
        Cmd::Status { vmid } => status(&vmid),
        Cmd::Start { vmid } => {
            common::run("qm", &["start", &vmid])?;
            println!("✓ VM {vmid} 시작");
            Ok(())
        }
        Cmd::Stop { vmid } => {
            common::run("qm", &["shutdown", &vmid, "--timeout", "60", "--forceStop", "1"])?;
            println!("✓ VM {vmid} 정지");
            Ok(())
        }
        Cmd::Reboot { vmid } => {
            common::run("qm", &["reboot", &vmid])?;
            println!("✓ VM {vmid} 재시작");
            Ok(())
        }
        Cmd::Delete { vmid, force } => {
            if !force {
                anyhow::bail!("삭제는 --force 필요 (복구 불가)");
            }
            let status = common::run("qm", &["status", &vmid]).unwrap_or_default();
            if status.contains("running") {
                common::run("qm", &["stop", &vmid])?;
            }
            common::run("qm", &["destroy", &vmid, "--purge", "1"])?;
            println!("✓ VM {vmid} 삭제");
            Ok(())
        }
        Cmd::Backup { vmid, storage, mode } => {
            println!("=== VM {vmid} 백업 ===");
            common::run("vzdump", &[&vmid, "--storage", &storage, "--mode", &mode, "--compress", "zstd"])?;
            println!("✓ 백업 완료");
            Ok(())
        }
        Cmd::Resize { vmid, cores, memory } => {
            if cores.is_none() && memory.is_none() {
                anyhow::bail!("--cores 또는 --memory 최소 하나");
            }
            if let Some(c) = cores {
                common::run("qm", &["set", &vmid, "--cores", &c])?;
                println!("  ✓ cores: {c}");
            }
            if let Some(m) = memory {
                common::run("qm", &["set", &vmid, "--memory", &m])?;
                println!("  ✓ memory: {m} MB");
            }
            Ok(())
        }
        Cmd::Console { vmid } => {
            let status = std::process::Command::new("qm")
                .args(["terminal", &vmid])
                .status()?;
            std::process::exit(status.code().unwrap_or(1));
        }
        Cmd::Doctor => {
            doctor();
            Ok(())
        }
    }
}

fn list() -> anyhow::Result<()> {
    let out = common::run("qm", &["list"])?;
    println!("{out}");
    Ok(())
}

fn status(vmid: &str) -> anyhow::Result<()> {
    let out = common::run("qm", &["status", vmid])?;
    println!("{out}");
    Ok(())
}

fn doctor() {
    println!("=== prelik-vm doctor ===");
    for (name, cmd) in &[("qm", "qm"), ("vzdump", "vzdump"), ("pvesh", "pvesh")] {
        println!("  {} {name}", if common::has_cmd(cmd) { "✓" } else { "✗" });
    }
}
