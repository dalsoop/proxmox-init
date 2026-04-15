//! prelik-host — 호스트 시스템 관리.
//! status / monitor / ssh-keygen / smb-open.

use clap::{Parser, Subcommand};
use prelik_core::common;

#[derive(Parser)]
#[command(name = "prelik-host", about = "호스트 시스템 관리")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 호스트 기본 상태 (OS, 커널, uptime, 디스크, 메모리)
    Status,
    /// 리소스 모니터링 (CPU/메모리/디스크/온도, 1회 스냅샷)
    Monitor,
    /// SSH 키 생성 (~/.ssh/id_ed25519_<label>)
    SshKeygen {
        /// 키 라벨 (파일명 suffix)
        #[arg(long)]
        label: String,
        /// 이메일 주석 (ssh-keygen -C)
        #[arg(long)]
        email: Option<String>,
    },
    /// SMB 포트 오픈 (445, 139) — 내부망 SMB 서버용
    SmbOpen,
    /// SMB 포트 닫기
    SmbClose,
    /// prelik CLI 자체 업데이트 (install.prelik.com 재실행)
    SelfUpdate {
        /// 특정 버전 핀 (예: v1.8.3). 미지정 시 latest.
        #[arg(long)]
        version: Option<String>,
        /// 같은 버전이어도 강제 재설치
        #[arg(long)]
        force: bool,
    },
    Doctor,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Status => {
            status();
            Ok(())
        }
        Cmd::Monitor => {
            monitor();
            Ok(())
        }
        Cmd::SshKeygen { label, email } => ssh_keygen(&label, email.as_deref()),
        Cmd::SmbOpen => smb_set(true),
        Cmd::SmbClose => smb_set(false),
        Cmd::SelfUpdate { version, force } => self_update(version.as_deref(), force),
        Cmd::Doctor => {
            doctor();
            Ok(())
        }
    }
}

fn status() {
    println!("=== prelik-host status ===");
    print_cmd("OS", "uname -sr");
    print_cmd("Hostname", "hostname");
    print_cmd("Uptime", "uptime -p");
    print_cmd("Load", "uptime | awk -F'load average:' '{print $2}'");
    if let Ok(out) = common::run_bash("df -h / | awk 'NR==2 {printf \"%s used / %s (%s)\", $3, $2, $5}'") {
        println!("  Disk /:   {out}");
    }
    if let Ok(out) = common::run_bash("free -h | awk 'NR==2 {printf \"%s used / %s\", $3, $2}'") {
        println!("  Memory:   {out}");
    }
}

fn monitor() {
    println!("=== prelik-host monitor ===");
    print_cmd("CPU", "top -bn1 | grep 'Cpu(s)' | head -1 | awk '{print $2 \"% user, \" $4 \"% system\"}'");
    print_cmd("Load", "uptime | awk -F'load average:' '{print $2}'");
    if let Ok(out) = common::run_bash("free -h | awk 'NR==2 {print $3 \" / \" $2}'") {
        println!("  Memory:   {out}");
    }
    if let Ok(out) = common::run_bash("df -h / | awk 'NR==2 {print $3 \" / \" $2 \" (\" $5 \")\"}'") {
        println!("  Disk /:   {out}");
    }
    // 온도 (가능한 경우)
    if let Ok(out) = common::run_bash(
        "which sensors >/dev/null 2>&1 && sensors 2>/dev/null | grep -E 'Package id|Core 0' | head -2 || true",
    ) {
        if !out.trim().is_empty() {
            println!("  Temp:");
            for line in out.lines() {
                println!("    {}", line.trim());
            }
        }
    }
}

fn ssh_keygen(label: &str, email: Option<&str>) -> anyhow::Result<()> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("HOME 미설정"))?;
    let ssh_dir = home.join(".ssh");
    std::fs::create_dir_all(&ssh_dir)?;
    common::run("chmod", &["700", &ssh_dir.display().to_string()])?;

    let key_path = ssh_dir.join(format!("id_ed25519_{label}"));
    if key_path.exists() {
        anyhow::bail!(
            "이미 존재: {}. --label 변경하거나 기존 삭제.",
            key_path.display()
        );
    }

    let comment = email.unwrap_or(label);
    println!("=== SSH 키 생성: {} ===", key_path.display());
    common::run(
        "ssh-keygen",
        &[
            "-t",
            "ed25519",
            "-f",
            &key_path.display().to_string(),
            "-N",
            "",
            "-C",
            comment,
        ],
    )?;

    let pub_key = std::fs::read_to_string(format!("{}.pub", key_path.display()))?;
    println!("\n✓ 키 생성 완료");
    println!("  개인키: {} (0600)", key_path.display());
    println!("  공개키: {}.pub", key_path.display());
    println!("\n공개키:");
    println!("{}", pub_key.trim());
    Ok(())
}

fn smb_set(open: bool) -> anyhow::Result<()> {
    let action = if open { "오픈" } else { "닫기" };
    println!("=== SMB 포트 {action} (139, 445) ===");
    if !common::has_cmd("iptables") {
        anyhow::bail!("iptables 없음 — 설치 후 재시도");
    }
    for port in [139, 445] {
        let rule = format!("INPUT -p tcp --dport {port} -j ACCEPT -m comment --comment 'prelik-smb'");
        if open {
            // 존재하지 않으면 추가
            let check = format!("sudo iptables -C {rule} 2>/dev/null");
            if common::run_bash(&check).is_err() {
                common::run_bash(&format!("sudo iptables -I {rule}"))?;
                println!("  ✓ 포트 {port} 오픈");
            } else {
                println!("  ⊘ 포트 {port} 이미 열려 있음");
            }
        } else {
            // 존재하면 제거 — 반복 (중복 규칙 고려)
            let del = format!("sudo iptables -D {rule} 2>/dev/null");
            let mut removed = 0;
            while common::run_bash(&del).is_ok() {
                removed += 1;
                if removed > 10 {
                    break;
                }
            }
            if removed > 0 {
                println!("  ✓ 포트 {port} {removed}개 규칙 제거");
            } else {
                println!("  ⊘ 포트 {port} 규칙 없음");
            }
        }
    }
    // rules.v4 영구 저장 (iptables-persistent 설치돼 있으면)
    if std::path::Path::new("/etc/iptables/rules.v4").exists() {
        common::run_bash("sudo iptables-save | sudo tee /etc/iptables/rules.v4 >/dev/null")?;
        println!("  iptables rules.v4 저장");
    } else {
        eprintln!("  ⚠ iptables-persistent 없음 — 재부팅 시 규칙 사라짐");
    }
    Ok(())
}

fn print_cmd(label: &str, cmd: &str) {
    if let Ok(out) = common::run_bash(cmd) {
        println!("  {label}:   {}", out.trim());
    }
}

fn doctor() {
    println!("=== prelik-host doctor ===");
    for (name, cmd) in &[
        ("iptables", "iptables"),
        ("ssh-keygen", "ssh-keygen"),
        ("uname", "uname"),
        ("df", "df"),
        ("free", "free"),
        ("top", "top"),
        ("sensors", "sensors"),
    ] {
        let ok = common::has_cmd(cmd);
        println!("  {} {name}", if ok { "✓" } else { "✗" });
    }
}

/// prelik CLI 자체 업데이트 — install.prelik.com 재실행.
/// install.sh가 idempotent skip + atomic install + retry를 모두 처리.
fn self_update(version: Option<&str>, force: bool) -> anyhow::Result<()> {
    println!("=== prelik self-update ===");
    if !common::has_cmd("curl") {
        anyhow::bail!("curl 없음");
    }
    if !common::has_cmd("bash") {
        anyhow::bail!("bash 없음");
    }
    let mut cmd = std::process::Command::new("bash");
    cmd.arg("-c").arg("curl -fsSL https://install.prelik.com | bash");
    if let Some(v) = version {
        cmd.env("PRELIK_VERSION", v);
        println!("  PRELIK_VERSION={v}");
    }
    if force {
        cmd.env("PRELIK_FORCE", "1");
        println!("  PRELIK_FORCE=1");
    }
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("self-update 실패 (exit {})", status.code().unwrap_or(-1));
    }
    Ok(())
}
