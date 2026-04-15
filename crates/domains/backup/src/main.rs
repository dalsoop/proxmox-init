//! prelik-backup — Proxmox vzdump 관리.
//! 즉시 백업 + cron 스케줄 + 목록 + 복원.

use clap::{Parser, Subcommand};
use prelik_core::common;

#[derive(Parser)]
#[command(name = "prelik-backup", about = "LXC/VM 백업 (vzdump)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 즉시 백업
    Now {
        vmid: String,
        #[arg(long, default_value = "local")]
        storage: String,
        /// snapshot | suspend | stop
        #[arg(long, default_value = "snapshot")]
        mode: String,
    },
    /// 백업 파일 목록 (dump 디렉토리)
    List {
        #[arg(long)]
        vmid: Option<String>,
        #[arg(long, default_value = "/var/lib/vz/dump")]
        dir: String,
    },
    /// 스케줄 추가 (Proxmox /etc/pve/jobs.cfg)
    ScheduleAdd {
        /// cron schedule (예: "03:00")
        #[arg(long)]
        schedule: String,
        /// 대상 VMID (쉼표 구분, 생략 시 all)
        #[arg(long)]
        vmid: Option<String>,
        #[arg(long, default_value = "local")]
        storage: String,
        /// 유지할 백업 수
        #[arg(long, default_value = "7")]
        keep: String,
    },
    /// 스케줄 목록
    ScheduleList,
    /// 스케줄 제거 (job-id로)
    ScheduleRemove {
        id: String,
    },
    /// 복원
    Restore {
        /// 백업 파일 경로 (.tar.zst)
        #[arg(long)]
        file: String,
        /// 복원할 VMID
        #[arg(long)]
        vmid: String,
        #[arg(long, default_value = "local-lvm")]
        storage: String,
    },
    Doctor,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if !matches!(cli.cmd, Cmd::Doctor) && !common::has_cmd("vzdump") {
        anyhow::bail!("vzdump 없음 — Proxmox 호스트 필요");
    }
    match cli.cmd {
        Cmd::Now { vmid, storage, mode } => now(&vmid, &storage, &mode),
        Cmd::List { vmid, dir } => list(vmid.as_deref(), &dir),
        Cmd::ScheduleAdd { schedule, vmid, storage, keep } => {
            schedule_add(&schedule, vmid.as_deref(), &storage, &keep)
        }
        Cmd::ScheduleList => schedule_list(),
        Cmd::ScheduleRemove { id } => schedule_remove(&id),
        Cmd::Restore { file, vmid, storage } => restore(&file, &vmid, &storage),
        Cmd::Doctor => {
            doctor();
            Ok(())
        }
    }
}

fn now(vmid: &str, storage: &str, mode: &str) -> anyhow::Result<()> {
    println!("=== 즉시 백업: VMID {vmid} → {storage} ({mode}) ===");
    common::run("vzdump", &[
        vmid, "--storage", storage, "--mode", mode, "--compress", "zstd",
    ])?;
    println!("✓ 백업 완료");
    Ok(())
}

fn list(vmid: Option<&str>, dir: &str) -> anyhow::Result<()> {
    println!("=== 백업 파일 목록 ({dir}) ===");
    let pattern = match vmid {
        Some(v) => format!("vzdump-*-{v}-*.tar.zst vzdump-*-{v}-*.tar.gz"),
        None => "vzdump-*.tar.zst vzdump-*.tar.gz".to_string(),
    };
    // ls로 파일 목록 + 크기
    let cmd = format!(
        "ls -lah {dir}/{pattern} 2>/dev/null | awk '{{print $NF, \"(\", $5, \")\"}}'"
    );
    match common::run_bash(&cmd) {
        Ok(out) => {
            if out.trim().is_empty() {
                println!("  (백업 파일 없음)");
            } else {
                for line in out.lines() {
                    println!("  {line}");
                }
            }
        }
        Err(_) => println!("  (목록 조회 실패)"),
    }
    Ok(())
}

fn schedule_add(schedule: &str, vmid: Option<&str>, storage: &str, keep: &str) -> anyhow::Result<()> {
    println!("=== 백업 스케줄 추가 ===");
    // Proxmox backup job API: pvesh create /cluster/backup
    let target = vmid.unwrap_or("all");
    let vmid_arg: Vec<String> = match vmid {
        Some(v) => vec!["--vmid".to_string(), v.to_string()],
        None => vec!["--all".to_string(), "1".to_string()],
    };

    let mut args: Vec<String> = vec![
        "create".into(), "/cluster/backup".into(),
        "--schedule".into(), schedule.to_string(),
        "--storage".into(), storage.to_string(),
        "--mode".into(), "snapshot".into(),
        "--compress".into(), "zstd".into(),
        "--prune-backups".into(), format!("keep-last={keep}"),
        "--enabled".into(), "1".into(),
    ];
    args.extend(vmid_arg);

    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    common::run("pvesh", &args_ref)?;
    println!("✓ 스케줄 등록: {schedule}, 대상: {target}, 보관: {keep}개");
    Ok(())
}

fn schedule_list() -> anyhow::Result<()> {
    println!("=== 백업 스케줄 목록 ===");
    let out = common::run("pvesh", &["get", "/cluster/backup", "--output-format", "yaml"])?;
    println!("{out}");
    Ok(())
}

fn schedule_remove(id: &str) -> anyhow::Result<()> {
    println!("=== 스케줄 제거: {id} ===");
    common::run("pvesh", &["delete", &format!("/cluster/backup/{id}")])?;
    println!("✓ 제거 완료");
    Ok(())
}

fn restore(file: &str, vmid: &str, storage: &str) -> anyhow::Result<()> {
    println!("=== 복원: {file} → VMID {vmid} (storage: {storage}) ===");
    if !std::path::Path::new(file).exists() {
        anyhow::bail!("백업 파일 없음: {file}");
    }
    // 기존 VMID 있으면 에러
    if common::run("pct", &["status", vmid]).is_ok() {
        anyhow::bail!("VMID {vmid} 이미 존재. 먼저 삭제 또는 다른 VMID 사용.");
    }
    // LXC 복원 (file이 lxc 백업인지 qemu인지 파일명으로 판단)
    if file.contains("vzdump-lxc") {
        common::run("pct", &["restore", vmid, file, "--storage", storage])?;
    } else if file.contains("vzdump-qemu") {
        common::run("qmrestore", &[file, vmid, "--storage", storage])?;
    } else {
        anyhow::bail!("알 수 없는 백업 형식: {file} (vzdump-lxc-* 또는 vzdump-qemu-* 필요)");
    }
    println!("✓ 복원 완료");
    Ok(())
}

fn doctor() {
    println!("=== prelik-backup doctor ===");
    for (name, cmd) in &[
        ("vzdump", "vzdump"),
        ("pct (LXC 복원)", "pct"),
        ("qmrestore (VM 복원)", "qmrestore"),
        ("pvesh (스케줄)", "pvesh"),
    ] {
        println!("  {} {name}", if common::has_cmd(cmd) { "✓" } else { "✗" });
    }
}
