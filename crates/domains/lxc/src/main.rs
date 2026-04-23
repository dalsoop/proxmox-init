//! pxi-lxc -- Proxmox LXC 수명 관리.
//! pct 바이너리를 전제로 함 (Proxmox VE 호스트에서만 동작).

use clap::{Parser, Subcommand};
use pxi_core::{common, os};
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser)]
#[command(name = "pxi-lxc", about = "LXC 수명 관리 (Proxmox pct 래퍼)")]
struct Cli {
    /// list/snapshot-list/status를 JSON으로 출력 (자동화/CI 친화)
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Serialize)]
struct LxcRow {
    vmid: String,
    status: String,
    lock: String,
    name: String,
}

#[derive(Serialize)]
struct SnapshotRow {
    name: String,
    timestamp: String,
    description: String,
}

#[derive(Subcommand)]
enum Cmd {
    /// LXC 목록
    List,
    /// LXC 상태
    Status { vmid: String },
    /// LXC 생성 (VMID 규약 강제 — Vmid newtype).
    /// 리소스 기본값은 config.toml `[lxc]` 에서 로드. 생략 시 built-in default
    /// (debian-13 / local-lvm / 8G / 2core / 2048MB / vmbr1).
    Create {
        #[arg(long)]
        vmid: pxi_core::types::Vmid,
        #[arg(long)]
        hostname: String,
        /// IP (CIDR 포함 가능, 예: 10.0.50.181/16)
        #[arg(long)]
        ip: String,
        #[arg(long)]
        template: Option<String>,
        #[arg(long)]
        storage: Option<String>,
        #[arg(long)]
        disk: Option<String>,
        #[arg(long)]
        cores: Option<String>,
        #[arg(long)]
        memory: Option<String>,
        /// 게이트웨이 (기본: config.toml의 network.gateway)
        #[arg(long)]
        gateway: Option<String>,
        #[arg(long)]
        bridge: Option<String>,
    },
    /// LXC 시작
    Start { vmid: String },
    /// LXC 정지
    Stop { vmid: String },
    /// LXC 재시작
    Restart { vmid: String },
    /// LXC 재부팅 (restart 별칭)
    Reboot { vmid: String },
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
    /// LXC 스냅샷 생성
    SnapshotCreate {
        vmid: String,
        /// 스냅샷 이름
        name: String,
        /// 설명 (선택)
        #[arg(long)]
        description: Option<String>,
    },
    /// LXC 스냅샷 목록
    SnapshotList { vmid: String },
    /// LXC 스냅샷 복원
    SnapshotRestore { vmid: String, name: String },
    /// LXC 스냅샷 삭제
    SnapshotDelete { vmid: String, name: String },
    /// LXC 리소스 변경 (CPU/RAM/disk)
    Resize {
        vmid: String,
        /// CPU 코어
        #[arg(long)]
        cores: Option<String>,
        /// RAM MB
        #[arg(long)]
        memory: Option<String>,
        /// 디스크 확장 크기 (+GB, 예: +4G)
        #[arg(long)]
        disk_expand: Option<String>,
    },
    /// LXC 초기 설정 (locale + timezone + 기본 패키지)
    Init {
        vmid: String,
        /// 로케일 (기본 ko_KR.UTF-8). "none"이면 설정 스킵.
        #[arg(long, default_value = "ko_KR.UTF-8")]
        locale: String,
        /// 타임존 (기본 Asia/Seoul). "none"이면 설정 스킵.
        #[arg(long, default_value = "Asia/Seoul")]
        timezone: String,
        /// 설치할 기본 패키지 목록 (콤마 분리).
        #[arg(
            long,
            default_value = "git,curl,wget,rsync,tmux,jq,htop,tree,unzip,locales"
        )]
        packages: String,
    },
    /// LXC 설정 상세 표시
    Config { vmid: String },
    /// hostname prefix 기반 VMID+IP 연속 재배치 (dry-run 기본)
    AlignVmidIp {
        /// 매칭할 hostname prefix (예: "vhost-")
        #[arg(long)]
        hostname_prefix: String,
        /// 시작 VMID (예: 50100)
        #[arg(long)]
        start_vmid: String,
        /// 실제 적용 (없으면 dry-run)
        #[arg(long)]
        apply: bool,
    },
    /// NAS 폴더를 LXC에 마운트
    Mount {
        vmid: String,
        /// mp 인덱스 (예: 0, 1, 2)
        #[arg(long)]
        index: String,
        /// 소스 경로 (예: /mnt/nas/share)
        #[arg(long)]
        source: String,
        /// LXC 내부 마운트 경로 (예: /mnt/share)
        #[arg(long)]
        target: String,
    },
    /// LXC 마운트 해제
    Unmount {
        vmid: String,
        /// mp 인덱스
        #[arg(long)]
        index: String,
    },
    /// LXC 풀 부트스트랩 (locale, packages, shell, tmux)
    Bootstrap { vmid: String },
    /// vmbr1 게이트웨이 일괄 수정
    GatewayFix {
        /// 특정 노드만 대상 (없으면 전체)
        #[arg(long)]
        node: Option<String>,
        /// 실제 적용 (없으면 dry-run)
        #[arg(long)]
        apply: bool,
    },
    /// Traefik 라우트 vs 실행중 LXC 감사
    RouteAudit {
        /// 특정 노드만 대상
        #[arg(long)]
        node: Option<String>,
        /// 미등록 서비스 자동 등록
        #[arg(long)]
        fix: bool,
    },
    /// route-audit systemd timer 설치/제거/상태
    RouteAuditWatch {
        /// install / uninstall / status
        #[arg(long, default_value = "status")]
        action: String,
    },
    /// 관리 LXC 생성 (SSH 키 + API 토큰 + 도구 자동 설치)
    MgmtSetup {
        #[arg(long)]
        vmid: String,
        #[arg(long)]
        hostname: String,
        #[arg(long, default_value = "local-lvm")]
        storage: String,
        #[arg(long, default_value = "8")]
        disk: String,
        #[arg(long, default_value = "2")]
        cores: String,
        #[arg(long, default_value = "2048")]
        memory: String,
        #[arg(long, default_value = "false")]
        bootstrap: bool,
    },
    /// 상태 점검 (pct 존재, PVE 노드 확인)
    Doctor,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let json = cli.json;
    if !matches!(cli.cmd, Cmd::Doctor) {
        require_proxmox()?;
    }
    match cli.cmd {
        Cmd::List => list(json),
        Cmd::Status { vmid } => status(&vmid, json),
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
        } => {
            // config.toml [lxc] 는 **lazy load** — 명시 안 된 필드가 있을 때만.
            // 모든 플래그 explicit 이면 config 건드리지 않음 (codex #42 P2: 깨진 config 가
            // explicit 호출도 깨뜨리는 regression 방지).
            let need_config = template.is_none()
                || storage.is_none()
                || disk.is_none()
                || cores.is_none()
                || memory.is_none()
                || bridge.is_none();
            let cfg = if need_config {
                pxi_core::config::Config::load()?
            } else {
                pxi_core::config::Config::default()
            };
            create(
                vmid.as_str(),
                &hostname,
                &ip,
                &template.unwrap_or(cfg.lxc.template),
                &storage.unwrap_or(cfg.lxc.storage),
                &disk.unwrap_or(cfg.lxc.disk),
                &cores.unwrap_or(cfg.lxc.cores),
                &memory.unwrap_or(cfg.lxc.memory),
                gateway.as_deref(),
                &bridge.unwrap_or(cfg.lxc.bridge),
            )
        }
        Cmd::Start { vmid } => start(&vmid),
        Cmd::Stop { vmid } => stop(&vmid),
        Cmd::Restart { vmid } => restart(&vmid),
        Cmd::Reboot { vmid } => restart(&vmid),
        Cmd::Delete { vmid, force } => delete(&vmid, force),
        Cmd::Enter { vmid } => enter(&vmid),
        Cmd::Backup {
            vmid,
            storage,
            mode,
        } => backup(&vmid, &storage, &mode),
        Cmd::SnapshotCreate {
            vmid,
            name,
            description,
        } => snapshot_create(&vmid, &name, description.as_deref()),
        Cmd::SnapshotList { vmid } => snapshot_list(&vmid, json),
        Cmd::SnapshotRestore { vmid, name } => snapshot_restore(&vmid, &name),
        Cmd::SnapshotDelete { vmid, name } => snapshot_delete(&vmid, &name),
        Cmd::Resize {
            vmid,
            cores,
            memory,
            disk_expand,
        } => resize(
            &vmid,
            cores.as_deref(),
            memory.as_deref(),
            disk_expand.as_deref(),
        ),
        Cmd::Init {
            vmid,
            locale,
            timezone,
            packages,
        } => init_lxc(&vmid, &locale, &timezone, &packages),
        Cmd::Config { vmid } => config(&vmid),
        Cmd::AlignVmidIp {
            hostname_prefix,
            start_vmid,
            apply,
        } => {
            align_vmid_ip(&hostname_prefix, &start_vmid, apply);
            Ok(())
        }
        Cmd::Mount {
            vmid,
            index,
            source,
            target,
        } => mount(&vmid, &index, &source, &target),
        Cmd::Unmount { vmid, index } => unmount(&vmid, &index),
        Cmd::Bootstrap { vmid } => bootstrap(&vmid),
        Cmd::GatewayFix { node, apply } => {
            gateway_fix(node.as_deref(), !apply);
            Ok(())
        }
        Cmd::RouteAudit { node, fix } => {
            route_audit(node.as_deref(), fix);
            Ok(())
        }
        Cmd::RouteAuditWatch { action } => {
            route_audit_watch(&action);
            Ok(())
        }
        Cmd::MgmtSetup {
            vmid,
            hostname,
            storage,
            disk,
            cores,
            memory,
            bootstrap,
        } => mgmt_setup(
            &vmid, &hostname, &storage, &disk, &cores, &memory, bootstrap,
        ),
        Cmd::Doctor => {
            doctor();
            Ok(())
        }
    }
}

// =============================================================================
// Core utilities (ported from phs infra/mod.rs)
// =============================================================================

fn cmd_output(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn ensure_lxc_exists(vmid: &str) -> String {
    let status = cmd_output("pct", &["status", vmid]);
    if status.is_empty() {
        eprintln!("[lxc] VMID {vmid} 을 찾을 수 없습니다.");
        std::process::exit(1);
    }
    status
}

fn ensure_lxc_running(vmid: &str) {
    let status = ensure_lxc_exists(vmid);
    let parsed: pxi_core::types::LxcStatus = status.parse().unwrap();
    if !parsed.is_running() {
        eprintln!("[lxc] LXC {vmid} 이 실행 중이 아닙니다 (현재: {status})");
        std::process::exit(1);
    }
}

fn get_lxc_ip(vmid: &str) -> String {
    let config = cmd_output("pct", &["config", vmid]);
    for line in config.lines() {
        if line.starts_with("net0:") {
            if let Some(ip_part) = line.split(',').find(|p| p.contains("ip=")) {
                let ip = ip_part.trim().trim_start_matches("ip=");
                return ip.to_string();
            }
        }
    }
    "?".to_string()
}

/// SSH/exec로 보내는 명령에서 인프라 파괴 패턴을 차단
fn detect_dangerous_command(cmd: &str) -> Option<&'static str> {
    let patterns = [
        ("rm -rf /etc/corosync", "corosync 디렉토리 삭제"),
        ("rm -rf /var/lib/corosync", "corosync 데이터 삭제"),
        ("rm /etc/pve/corosync.conf", "cluster 설정 삭제"),
        ("rm -f /etc/pve/corosync.conf", "cluster 설정 삭제"),
        ("killall pmxcfs", "pmxcfs 종료"),
        ("killall -9 pmxcfs", "pmxcfs 강제 종료"),
        ("pmxcfs -l", "local mode 시작"),
        ("systemctl stop pve-cluster", "pmxcfs 정지"),
        ("fusermount -u /etc/pve", "/etc/pve 강제 unmount"),
        ("rm -rf /etc/pve", "/etc/pve 통째 삭제"),
        ("rm -rf /root/.ssh", "SSH 키 통째 삭제"),
        ("rm /root/.ssh/authorized_keys", "authorized_keys 삭제"),
    ];
    for (pattern, reason) in &patterns {
        if cmd.contains(pattern) {
            return Some(reason);
        }
    }
    None
}

/// 특정 노드(또는 로컬)에서 pct exec 실행
fn lxc_exec_on(node: Option<&str>, vmid: &str, cmd: &[&str]) -> (bool, String) {
    let joined = cmd.join(" ");
    if let Some(reason) = detect_dangerous_command(&joined) {
        eprintln!("[safety] 위험 명령 차단: {reason}");
        if std::env::var("PRELIK_SAFETY_OVERRIDE").unwrap_or_default() != "1" {
            return (false, format!("[safety] blocked: {reason}"));
        }
    }

    let local_node = local_node_name();
    let is_local = node.is_none() || node == Some(&local_node);

    let output = if is_local {
        let mut args = vec!["exec", vmid, "--"];
        args.extend_from_slice(cmd);
        Command::new("pct")
            .args(&args)
            .output()
            .expect("pct exec 실패")
    } else {
        let node = node.unwrap();
        let node_ip = node_ip_from_name(node);
        let pct_cmd = format!(
            "pct exec {} -- {}",
            vmid,
            cmd.iter()
                .map(|c| shell_escape(c))
                .collect::<Vec<_>>()
                .join(" ")
        );
        Command::new("ssh")
            .args([
                "-o",
                "ConnectTimeout=10",
                "-o",
                "StrictHostKeyChecking=no",
                &format!("root@{node_ip}"),
                &pct_cmd,
            ])
            .output()
            .expect("ssh pct exec 실패")
    };
    let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let combined = if err.is_empty() {
        out
    } else {
        format!("{out}\n{err}")
    };
    (output.status.success(), combined)
}

fn lxc_exec(vmid: &str, cmd: &[&str]) -> (bool, String) {
    lxc_exec_on(None, vmid, cmd)
}

fn local_node_name() -> String {
    fs::read_to_string("/etc/hostname")
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn node_ip_from_name(node: &str) -> String {
    let output = cmd_output(
        "pvesh",
        &[
            "get",
            &format!("/nodes/{node}/network"),
            "--output-format",
            "json",
        ],
    );
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&output) {
        if let Some(ifaces) = parsed.as_array() {
            for iface in ifaces {
                if iface["iface"].as_str() == Some("vmbr0") {
                    if let Some(addr) = iface["address"].as_str() {
                        return addr.to_string();
                    }
                }
            }
        }
    }
    eprintln!("[lxc] 노드 '{node}' 의 IP를 찾을 수 없습니다.");
    std::process::exit(1);
}

fn shell_escape(s: &str) -> String {
    if s.contains(|c: char| {
        c.is_whitespace()
            || c == '\''
            || c == '"'
            || c == '$'
            || c == '`'
            || c == '\\'
            || c == '!'
            || c == '('
            || c == ')'
    }) {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}

fn lxc_install_packages(vmid: &str, packages: &[&str], tag: &str) {
    let statuses: Vec<(&str, bool)> = packages
        .iter()
        .map(|pkg| {
            let (installed, _) = lxc_exec(vmid, &["dpkg", "-s", pkg]);
            (*pkg, installed)
        })
        .collect();

    if statuses.iter().any(|(_, installed)| !installed) {
        println!("[{tag}] apt 업데이트 중...");
        lxc_exec(
            vmid,
            &[
                "bash",
                "-c",
                "DEBIAN_FRONTEND=noninteractive apt-get update -qq",
            ],
        );
    }

    for (pkg, installed) in &statuses {
        if *installed {
            println!("[{tag}] {pkg} - 이미 설치됨");
        } else {
            println!("[{tag}] {pkg} - 설치 중...");
            let (ok, _) = lxc_exec(
                vmid,
                &[
                    "bash",
                    "-c",
                    &format!("DEBIAN_FRONTEND=noninteractive apt-get install -y -qq {pkg}"),
                ],
            );
            if ok {
                println!("[{tag}] {pkg} - 설치 완료");
            } else {
                eprintln!("[{tag}] {pkg} - 설치 실패");
            }
        }
    }
}

// =============================================================================
// Existing commands (list, status, create, start, stop, etc.)
// =============================================================================

fn snapshot_create(vmid: &str, name: &str, description: Option<&str>) -> anyhow::Result<()> {
    println!("=== LXC {vmid} 스냅샷 생성: {name} ===");
    let mut args: Vec<&str> = vec!["snapshot", vmid, name];
    if let Some(d) = description {
        args.push("--description");
        args.push(d);
    }
    common::run_str("pct", &args)?;
    println!("  스냅샷 생성 완료");
    Ok(())
}

fn parse_pct_listsnapshot(out: &str) -> anyhow::Result<Vec<SnapshotRow>> {
    let mut rows = Vec::new();
    for l in out.lines() {
        let trimmed =
            l.trim_start_matches(|c: char| c == '`' || c == '-' || c == '>' || c.is_whitespace());
        if trimmed.is_empty() {
            continue;
        }
        let toks: Vec<&str> = trimmed.split_whitespace().collect();
        if toks.is_empty() {
            continue;
        }
        let name = toks[0].to_string();
        if name == "current" {
            continue;
        }
        let date_ok = toks
            .get(1)
            .map(|t| {
                let b = t.as_bytes();
                t.len() == 10
                    && b[4] == b'-'
                    && b[7] == b'-'
                    && b[..4].iter().all(|c| c.is_ascii_digit())
                    && b[5..7].iter().all(|c| c.is_ascii_digit())
                    && b[8..10].iter().all(|c| c.is_ascii_digit())
            })
            .unwrap_or(false);
        let time_ok = toks
            .get(2)
            .map(|t| {
                let b = t.as_bytes();
                t.len() == 8
                    && b[2] == b':'
                    && b[5] == b':'
                    && b[..2].iter().all(|c| c.is_ascii_digit())
                    && b[3..5].iter().all(|c| c.is_ascii_digit())
                    && b[6..8].iter().all(|c| c.is_ascii_digit())
            })
            .unwrap_or(false);
        if !date_ok || !time_ok {
            anyhow::bail!(
                "pct listsnapshot 라인 파싱 실패 (timestamp가 YYYY-MM-DD HH:MM:SS 아님): {l:?}"
            );
        }
        let timestamp = format!("{} {}", toks[1], toks[2]);
        let description = toks.iter().skip(3).copied().collect::<Vec<_>>().join(" ");
        rows.push(SnapshotRow {
            name,
            timestamp,
            description,
        });
    }
    Ok(rows)
}

fn snapshot_list(vmid: &str, json: bool) -> anyhow::Result<()> {
    let out = common::run_str("pct", &["listsnapshot", vmid])?;
    if !json {
        println!("{out}");
        return Ok(());
    }
    let rows = parse_pct_listsnapshot(&out)?;
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}

fn snapshot_restore(vmid: &str, name: &str) -> anyhow::Result<()> {
    println!("=== LXC {vmid} 스냅샷 복원: {name} ===");
    common::run_str("pct", &["rollback", vmid, name])?;
    println!("  복원 완료 -- LXC 상태가 '{name}' 시점으로 되돌아감");
    Ok(())
}

fn snapshot_delete(vmid: &str, name: &str) -> anyhow::Result<()> {
    println!("=== LXC {vmid} 스냅샷 삭제: {name} ===");
    common::run_str("pct", &["delsnapshot", vmid, name])?;
    println!("  삭제 완료");
    Ok(())
}

fn resize(
    vmid: &str,
    cores: Option<&str>,
    memory: Option<&str>,
    disk_expand: Option<&str>,
) -> anyhow::Result<()> {
    println!("=== LXC {vmid} 리소스 변경 ===");
    if cores.is_none() && memory.is_none() && disk_expand.is_none() {
        anyhow::bail!("--cores / --memory / --disk-expand 중 최소 하나 필요");
    }

    if let Some(c) = cores {
        common::run_str("pct", &["set", vmid, "--cores", c])?;
        println!("  cores: {c}");
    }
    if let Some(m) = memory {
        common::run_str("pct", &["set", vmid, "--memory", m])?;
        println!("  memory: {m} MB");
    }
    if let Some(d) = disk_expand {
        common::run_str("pct", &["resize", vmid, "rootfs", d])?;
        println!("  disk expand: {d}");
    }
    println!("변경 사항은 재시작 후 반영될 수 있습니다 (cores/memory는 라이브 가능)");
    Ok(())
}

fn require_proxmox() -> anyhow::Result<()> {
    if !common::has_cmd("pct") {
        anyhow::bail!("pct 바이너리 없음 -- Proxmox VE 호스트에서만 동작합니다");
    }
    Ok(())
}

fn parse_pct_list(out: &str) -> anyhow::Result<Vec<LxcRow>> {
    let mut rows = Vec::new();
    for l in out.lines().skip(1) {
        if l.trim().is_empty() {
            continue;
        }
        let p: Vec<&str> = l.split_whitespace().collect();
        let row = match p.len() {
            4 => LxcRow {
                vmid: p[0].into(),
                status: p[1].into(),
                lock: if p[2] == "-" {
                    String::new()
                } else {
                    p[2].into()
                },
                name: p[3].into(),
            },
            3 => LxcRow {
                vmid: p[0].into(),
                status: p[1].into(),
                lock: String::new(),
                name: p[2].into(),
            },
            _ => anyhow::bail!("pct list 라인 파싱 실패 (컬럼 {}개): {l:?}", p.len()),
        };
        rows.push(row);
    }
    Ok(rows)
}

fn list(json: bool) -> anyhow::Result<()> {
    let out = common::run_str("pct", &["list"])?;
    if !json {
        // Enhanced format with IP like old phs
        for line in out.lines().skip(1) {
            let p: Vec<&str> = line.split_whitespace().collect();
            if p.len() >= 3 {
                let mark = if p[1] == "running" { "+" } else { "-" };
                let name = p.get(2..).map(|s| s.join(" ")).unwrap_or_default();
                let ip = get_lxc_ip(p[0]);
                println!("  {mark} {:<6} {:<20} {:<10} {ip}", p[0], name, p[1]);
            }
        }
        return Ok(());
    }
    let rows = parse_pct_list(&out)?;
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}

const STATUS_KNOWN: &[&str] = &["running", "stopped", "unknown"];

fn parse_pct_status(raw: &str) -> anyhow::Result<&str> {
    let body = raw.strip_suffix('\n').unwrap_or(raw);
    if body.contains('\n') {
        anyhow::bail!("pct status 출력이 단일 라인이 아님: {raw:?}");
    }
    let value = body
        .strip_prefix("status: ")
        .ok_or_else(|| anyhow::anyhow!("pct status 출력 형식이 'status: <value>' 아님: {raw:?}"))?;
    if !STATUS_KNOWN.contains(&value) {
        anyhow::bail!("pct status 값이 알 수 없는 형태: {value:?} (허용: {STATUS_KNOWN:?})");
    }
    Ok(value)
}

fn status(vmid: &str, json: bool) -> anyhow::Result<()> {
    if !json {
        let out = common::run_str("pct", &["status", vmid])?;
        println!("{out}");
        return Ok(());
    }
    let output = Command::new("pct").args(["status", vmid]).output()?;
    if !output.status.success() {
        anyhow::bail!(
            "pct status {vmid} 실패: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let raw = String::from_utf8(output.stdout)?;
    let value = parse_pct_status(&raw)?;
    let payload = serde_json::json!({ "vmid": vmid, "status": value });
    println!("{}", serde_json::to_string_pretty(&payload)?);
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
    // VMID↔IP 규약 검증 — 단일 진입점.
    pxi_core::convention::validate_ip(vmid, ip)?;

    println!("=== LXC 생성: {vmid} ({hostname}) ===");

    let templates = common::run_str("pveam", &["list", "local"])?;
    let full_template = templates
        .lines()
        .skip(1)
        .find(|l| l.contains(template))
        .and_then(|l| l.split_whitespace().next())
        .ok_or_else(|| {
            anyhow::anyhow!("템플릿 '{template}' 을 찾을 수 없음 (pveam list local 확인)")
        })?;

    let ip_cidr = if ip.contains('/') {
        ip.to_string()
    } else {
        let cfg = pxi_core::config::Config::load().unwrap_or_default();
        let subnet = if cfg.network.subnet > 0 {
            cfg.network.subnet
        } else {
            24
        };
        format!("{ip}/{subnet}")
    };

    let gw = if let Some(g) = gateway {
        g.to_string()
    } else {
        let cfg = pxi_core::config::Config::load().unwrap_or_default();
        if !cfg.network.gateway.is_empty() {
            cfg.network.gateway
        } else {
            let octets: Vec<&str> = ip.split('/').next().unwrap_or(ip).split('.').collect();
            if octets.len() >= 3 {
                format!("{}.{}.{}.1", octets[0], octets[1], octets[2])
            } else {
                anyhow::bail!("게이트웨이 추론 실패 -- --gateway 명시 필요");
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
            "create",
            vmid,
            full_template,
            "--hostname",
            hostname,
            "--storage",
            storage,
            "--rootfs",
            &format!("{storage}:{disk}"),
            "--cores",
            cores,
            "--memory",
            memory,
            "--net0",
            &net0,
            "--features",
            "nesting=1",
            "--unprivileged",
            "1",
            "--start",
            "1",
        ],
    )?;
    println!("  LXC {vmid} 생성 + 시작 완료");
    Ok(())
}

fn start(vmid: &str) -> anyhow::Result<()> {
    common::run_str("pct", &["start", vmid])?;
    println!("  LXC {vmid} 시작");
    Ok(())
}

fn stop(vmid: &str) -> anyhow::Result<()> {
    common::run_str("pct", &["stop", vmid])?;
    println!("  LXC {vmid} 정지");
    Ok(())
}

fn restart(vmid: &str) -> anyhow::Result<()> {
    common::run_str("pct", &["reboot", vmid])?;
    println!("  LXC {vmid} 재시작");
    Ok(())
}

fn delete(vmid: &str, force: bool) -> anyhow::Result<()> {
    let status = common::run_str("pct", &["status", vmid]).unwrap_or_default();
    let parsed: pxi_core::types::LxcStatus = status.parse().unwrap();
    if parsed.is_running() {
        common::run_str("pct", &["stop", vmid])?;
    }
    if !force {
        eprintln!("  삭제 전 백업 권장: pxi-lxc backup {vmid}\n  또는 --force 로 무시");
        anyhow::bail!("중단됨");
    }
    common::run_str("pct", &["destroy", vmid])?;
    println!("  LXC {vmid} 삭제");
    Ok(())
}

fn enter(vmid: &str) -> anyhow::Result<()> {
    let status = Command::new("pct").args(["enter", vmid]).status()?;
    std::process::exit(status.code().unwrap_or(1));
}

fn backup(vmid: &str, storage: &str, mode: &str) -> anyhow::Result<()> {
    println!("=== LXC {vmid} 백업 ({storage}, {mode}) ===");
    common::run(
        "vzdump",
        &[
            vmid,
            "--storage",
            storage,
            "--mode",
            mode,
            "--compress",
            "zstd",
        ],
    )?;
    println!("  백업 완료");
    Ok(())
}

// =============================================================================
// Config (detailed display)
// =============================================================================

fn config(vmid: &str) -> anyhow::Result<()> {
    let status_out = ensure_lxc_exists(vmid);
    let output = cmd_output("pct", &["config", vmid]);
    println!("=== LXC {vmid} 설정 ===\n");
    let parsed: pxi_core::types::LxcStatus = status_out.parse().unwrap();
    let state = if parsed.is_running() {
        "running"
    } else {
        "stopped"
    };
    println!("[상태] {state}");
    println!("[IP] {}", get_lxc_ip(vmid));
    println!();
    for line in output.lines() {
        println!("  {line}");
    }
    Ok(())
}

// =============================================================================
// Bootstrap (locale + packages + shell + tmux)
// =============================================================================

const BOOTSTRAP_PACKAGES: &[&str] = &[
    "git", "curl", "wget", "rsync", "tmux", "jq", "htop", "tree", "unzip", "locales",
];

fn bootstrap(vmid: &str) -> anyhow::Result<()> {
    println!("=== LXC {vmid} 부트스트랩 ===\n");
    ensure_lxc_running(vmid);

    // locale
    let (_, locale_out) = lxc_exec(vmid, &["bash", "-c", "locale 2>&1"]);
    if locale_out.contains("Cannot set") {
        println!("[locale] ko_KR.UTF-8 설정 중...");
        lxc_exec(
            vmid,
            &[
                "bash",
                "-c",
                "apt-get install -y -qq locales 2>/dev/null && \
             sed -i '/ko_KR.UTF-8/s/^# //' /etc/locale.gen && \
             locale-gen && echo 'LANG=ko_KR.UTF-8' > /etc/default/locale",
            ],
        );
        // fallback
        let (_, verify) = lxc_exec(vmid, &["bash", "-c", "locale 2>&1"]);
        if verify.contains("Cannot set") {
            lxc_exec(vmid, &["bash", "-c",
                "apt-get install -y -qq locales-all 2>/dev/null && echo 'LANG=ko_KR.UTF-8' > /etc/default/locale"]);
        }
        println!("[locale] 완료");
    } else {
        println!("[locale] 이미 정상 설정됨");
    }

    // timezone
    let (_, tz_out) = lxc_exec(vmid, &["cat", "/etc/timezone"]);
    if tz_out.trim() != "Asia/Seoul" {
        println!("[tz] Asia/Seoul 설정 중...");
        lxc_exec(vmid, &["bash", "-c",
            "ln -sf /usr/share/zoneinfo/Asia/Seoul /etc/localtime && echo 'Asia/Seoul' > /etc/timezone"]);
        println!("[tz] 완료");
    } else {
        println!("[tz] 이미 Asia/Seoul");
    }

    // packages
    lxc_install_packages(vmid, BOOTSTRAP_PACKAGES, "packages");

    // shell setup (bash defaults)
    println!("[shell] bash 기본 설정...");
    lxc_exec(vmid, &["bash", "-c",
        "grep -q 'HISTSIZE=10000' /root/.bashrc 2>/dev/null || echo 'export HISTSIZE=10000\nexport HISTFILESIZE=20000' >> /root/.bashrc"]);

    // tmux setup
    println!("[tmux] tmux 설정...");
    lxc_exec(vmid, &["bash", "-c",
        "test -f /root/.tmux.conf || echo 'set -g mouse on\nset -g history-limit 50000' > /root/.tmux.conf"]);

    println!("\n=== LXC {vmid} 부트스트랩 완료 ===");
    Ok(())
}

// =============================================================================
// Mount / Unmount
// =============================================================================

fn mount(vmid: &str, mp_index: &str, source: &str, target: &str) -> anyhow::Result<()> {
    println!("=== LXC {vmid} 마운트 ===");
    let config = cmd_output("pct", &["config", vmid]);
    if config.contains(&format!("mp{mp_index}:")) {
        anyhow::bail!("mp{mp_index} 이미 사용 중");
    }
    let mp_value = format!("{source},mp={target}");
    println!("[mount] mp{mp_index}: {source} -> {target}");
    common::run("pct", &["set", vmid, &format!("-mp{mp_index}"), &mp_value])?;
    println!("[mount] 완료. 재시작 필요: pxi-lxc restart {vmid}");
    Ok(())
}

fn unmount(vmid: &str, mp_index: &str) -> anyhow::Result<()> {
    println!("=== LXC {vmid} 마운트 해제 ===");
    let config = cmd_output("pct", &["config", vmid]);
    if !config
        .lines()
        .any(|l| l.starts_with(&format!("mp{mp_index}:")))
    {
        println!("[unmount] mp{mp_index} 없음, 스킵");
        return Ok(());
    }
    common::run("pct", &["set", vmid, "--delete", &format!("mp{mp_index}")])?;
    println!("[unmount] 완료. 재시작 필요: pxi-lxc restart {vmid}");
    Ok(())
}

// =============================================================================
// Align VMID+IP
// =============================================================================

#[derive(Clone, Debug)]
struct LxcAlignTarget {
    old_vmid: u32,
    new_vmid: u32,
    hostname: String,
    status: String,
    old_ip_cidr: String,
    new_ip_cidr: String,
    old_ip: String,
    new_ip: String,
    old_conf_path: PathBuf,
    new_conf_path: PathBuf,
}

fn align_vmid_ip(hostname_prefix: &str, start_vmid: &str, apply: bool) {
    println!("=== LXC VMID+IP 연속 재배치 ===\n");

    let start_vmid_num = start_vmid.parse::<u32>().unwrap_or_else(|_| {
        eprintln!("[align] start-vmid 값이 올바르지 않습니다: {start_vmid}");
        std::process::exit(1);
    });

    let list_output = cmd_output("pct", &["list"]);
    if list_output.trim().is_empty() {
        eprintln!("[align] pct list 결과가 비어 있습니다.");
        std::process::exit(1);
    }

    let existing_vmids = collect_existing_guest_vmids();
    let mut discovered = Vec::new();

    for line in list_output.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let Ok(vmid) = parts[0].parse::<u32>() else {
            continue;
        };
        let status = parts[1].to_string();
        let config = cmd_output("pct", &["config", parts[0]]);
        let hostname = extract_config_value(&config, "hostname").unwrap_or_default();
        if !hostname.starts_with(hostname_prefix) {
            continue;
        }

        let old_ip_cidr = extract_static_ip_cidr(&config).unwrap_or_else(|| {
            eprintln!(
                "[align] LXC {} ({}) 에 static IPv4 ip= 값이 없습니다.",
                vmid, hostname
            );
            std::process::exit(1);
        });

        let new_vmid = start_vmid_num + discovered.len() as u32;
        let new_ip_cidr = build_aligned_ip_cidr(&old_ip_cidr, new_vmid).unwrap_or_else(|err| {
            eprintln!("[align] LXC {} ({}) IP 계산 실패: {}", vmid, hostname, err);
            std::process::exit(1);
        });

        let old_ip = strip_cidr(&old_ip_cidr);
        let new_ip = strip_cidr(&new_ip_cidr);
        let old_conf_path = PathBuf::from(format!("/etc/pve/lxc/{vmid}.conf"));
        let new_conf_path = PathBuf::from(format!("/etc/pve/lxc/{new_vmid}.conf"));

        discovered.push(LxcAlignTarget {
            old_vmid: vmid,
            new_vmid,
            hostname,
            status,
            old_ip_cidr,
            new_ip_cidr,
            old_ip,
            new_ip,
            old_conf_path,
            new_conf_path,
        });
    }

    discovered.sort_by_key(|t| t.old_vmid);
    for (index, target) in discovered.iter_mut().enumerate() {
        target.new_vmid = start_vmid_num + index as u32;
        target.new_ip_cidr = build_aligned_ip_cidr(&target.old_ip_cidr, target.new_vmid)
            .unwrap_or_else(|err| {
                eprintln!(
                    "[align] LXC {} ({}) IP 계산 실패: {}",
                    target.old_vmid, target.hostname, err
                );
                std::process::exit(1);
            });
        target.new_ip = strip_cidr(&target.new_ip_cidr);
        target.new_conf_path = PathBuf::from(format!("/etc/pve/lxc/{}.conf", target.new_vmid));
    }

    if discovered.is_empty() {
        eprintln!(
            "[align] hostname-prefix '{}' 와 일치하는 LXC를 찾지 못했습니다.",
            hostname_prefix
        );
        std::process::exit(1);
    }

    validate_align_targets(&discovered, &existing_vmids);

    println!(
        "[align] 대상 prefix: '{}', 시작 VMID: {}, apply: {}",
        hostname_prefix,
        start_vmid_num,
        if apply { "yes" } else { "no (dry-run)" }
    );
    println!();
    for target in &discovered {
        println!(
            "  {} ({}) [{}]  {} -> {}  |  {} -> {}",
            target.old_vmid,
            target.hostname,
            target.status,
            target.old_vmid,
            target.new_vmid,
            target.old_ip_cidr,
            target.new_ip_cidr
        );
    }

    let exact_ip_map: Vec<(String, String)> = discovered
        .iter()
        .filter(|t| t.old_ip != t.new_ip)
        .map(|t| (t.old_ip.clone(), t.new_ip.clone()))
        .collect();

    if !apply {
        println!();
        println!("[align] dry-run 입니다. 실제 적용:");
        println!(
            "  pxi-lxc align-vmid-ip --hostname-prefix {} --start-vmid {} --apply",
            hostname_prefix, start_vmid_num
        );
        if !exact_ip_map.is_empty() {
            println!(
                "[align] /etc/pve/nodes/**/*.conf 내 exact IP 참조도 함께 치환됩니다 ({}개).",
                exact_ip_map.len()
            );
        }
        return;
    }

    let backup_root = build_backup_root();
    let mut conf_files = Vec::new();
    collect_conf_files(Path::new("/etc/pve/nodes"), &mut conf_files);
    for path in &conf_files {
        backup_file(path, &backup_root);
    }
    println!("\n[align] 백업 디렉토리: {}", backup_root.display());

    let previously_running: Vec<&LxcAlignTarget> = discovered
        .iter()
        .filter(|t| t.status == "running")
        .collect();

    for target in &previously_running {
        run_pct_checked(
            &["stop", &target.old_vmid.to_string()],
            &format!("LXC {} 정지", target.old_vmid),
        );
    }

    for target in &discovered {
        if !target.old_conf_path.exists() {
            eprintln!(
                "[align] 설정 파일이 없습니다: {}",
                target.old_conf_path.display()
            );
            std::process::exit(1);
        }
        fs::rename(&target.old_conf_path, &target.new_conf_path).unwrap_or_else(|err| {
            eprintln!(
                "[align] 설정 파일 rename 실패: {} -> {} ({})",
                target.old_conf_path.display(),
                target.new_conf_path.display(),
                err
            );
            std::process::exit(1);
        });
        let content = fs::read_to_string(&target.new_conf_path).unwrap_or_else(|err| {
            eprintln!(
                "[align] 설정 파일 읽기 실패: {} ({})",
                target.new_conf_path.display(),
                err
            );
            std::process::exit(1);
        });
        let updated = rewrite_net0_ip(&content, &target.new_ip_cidr);
        fs::write(&target.new_conf_path, updated).unwrap_or_else(|err| {
            eprintln!(
                "[align] 설정 파일 쓰기 실패: {} ({})",
                target.new_conf_path.display(),
                err
            );
            std::process::exit(1);
        });
    }

    let mut updated_files = 0usize;
    let mut conf_files_after = Vec::new();
    collect_conf_files(Path::new("/etc/pve/nodes"), &mut conf_files_after);
    for path in conf_files_after {
        let original = fs::read_to_string(&path).unwrap_or_default();
        let mut rewritten = original.clone();
        for (old_ip, new_ip) in &exact_ip_map {
            rewritten = exact_ip_sub(&rewritten, old_ip, new_ip);
        }
        if rewritten != original {
            fs::write(&path, rewritten).unwrap_or_else(|err| {
                eprintln!("[align] 참조 치환 쓰기 실패: {} ({})", path.display(), err);
                std::process::exit(1);
            });
            updated_files += 1;
        }
    }

    for target in previously_running {
        run_pct_checked(
            &["start", &target.new_vmid.to_string()],
            &format!("LXC {} 시작", target.new_vmid),
        );
    }

    println!();
    println!(
        "[align] 완료: {}개 LXC 재배치, 참조 치환 파일 {}개",
        discovered.len(),
        updated_files
    );
}

// --- align helpers ---

fn extract_config_value(config: &str, key: &str) -> Option<String> {
    config
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}:")))
        .map(|v| v.trim().to_string())
}

fn extract_static_ip_cidr(config: &str) -> Option<String> {
    for line in config.lines() {
        if !line.starts_with("net0:") {
            continue;
        }
        for part in line.split(',') {
            let trimmed = part.trim();
            if let Some(value) = trimmed.strip_prefix("ip=") {
                if value == "dhcp" || value == "manual" || value.is_empty() {
                    return None;
                }
                return Some(value.to_string());
            }
        }
    }
    None
}

fn build_aligned_ip_cidr(old_ip_cidr: &str, new_vmid: u32) -> Result<String, String> {
    let (old_ip, cidr) = old_ip_cidr
        .split_once('/')
        .ok_or_else(|| format!("CIDR 형식이 아닙니다: {old_ip_cidr}"))?;
    let octets: Vec<&str> = old_ip.split('.').collect();
    if octets.len() != 4 {
        return Err(format!("IPv4 형식이 아닙니다: {old_ip}"));
    }
    let new_host = new_vmid % 1000;
    if !(1..=255).contains(&new_host) {
        return Err(format!(
            "VMID {} 는 마지막 옥텟으로 안전하게 매핑할 수 없습니다.",
            new_vmid
        ));
    }
    Ok(format!(
        "{}.{}.{}.{}/{}",
        octets[0], octets[1], octets[2], new_host, cidr
    ))
}

fn strip_cidr(ip_cidr: &str) -> String {
    ip_cidr.split('/').next().unwrap_or(ip_cidr).to_string()
}

fn validate_align_targets(targets: &[LxcAlignTarget], existing_vmids: &HashSet<u32>) {
    let target_old_ids: HashSet<u32> = targets.iter().map(|t| t.old_vmid).collect();
    let mut target_new_ids = HashSet::new();
    for target in targets {
        if !target_new_ids.insert(target.new_vmid) {
            eprintln!(
                "[align] 중복 대상 VMID가 생성되었습니다: {}",
                target.new_vmid
            );
            std::process::exit(1);
        }
        if existing_vmids.contains(&target.new_vmid) && target.new_vmid != target.old_vmid {
            if target_old_ids.contains(&target.new_vmid) {
                eprintln!(
                    "[align] 대상 VMID {} 가 현재 다른 대상 LXC의 VMID와 겹칩니다.",
                    target.new_vmid
                );
            } else {
                eprintln!(
                    "[align] 대상 VMID {} 가 이미 다른 LXC에서 사용 중입니다.",
                    target.new_vmid
                );
            }
            std::process::exit(1);
        }
    }
}

fn collect_existing_guest_vmids() -> HashSet<u32> {
    let mut vmids = HashSet::new();
    for output in [cmd_output("pct", &["list"]), cmd_output("qm", &["list"])] {
        for line in output.lines().skip(1) {
            if let Some(first) = line.split_whitespace().next() {
                if let Ok(vmid) = first.parse::<u32>() {
                    vmids.insert(vmid);
                }
            }
        }
    }
    vmids
}

fn rewrite_net0_ip(config: &str, new_ip_cidr: &str) -> String {
    let mut out = Vec::new();
    for line in config.lines() {
        if let Some(body) = line.strip_prefix("net0: ") {
            let mut parts = Vec::new();
            let mut replaced = false;
            for part in body.split(',') {
                let trimmed = part.trim();
                if trimmed.starts_with("ip=") {
                    parts.push(format!("ip={new_ip_cidr}"));
                    replaced = true;
                } else {
                    parts.push(trimmed.to_string());
                }
            }
            if !replaced {
                parts.push(format!("ip={new_ip_cidr}"));
            }
            out.push(format!("net0: {}", parts.join(",")));
        } else {
            out.push(line.to_string());
        }
    }
    format!("{}\n", out.join("\n"))
}

fn exact_ip_sub(text: &str, old: &str, new: &str) -> String {
    if old.is_empty() || old == new {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    while let Some(relative) = text[cursor..].find(old) {
        let start = cursor + relative;
        let end = start + old.len();
        let left_digit = text[..start]
            .chars()
            .next_back()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false);
        let right_digit = text[end..]
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false);
        if left_digit || right_digit {
            out.push_str(&text[cursor..end]);
        } else {
            out.push_str(&text[cursor..start]);
            out.push_str(new);
        }
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    out
}

fn collect_conf_files(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_conf_files(&path, files);
        } else if path.extension().and_then(|e| e.to_str()) == Some("conf") {
            files.push(path);
        }
    }
}

fn backup_file(path: &Path, backup_root: &Path) {
    let Ok(relative) = path.strip_prefix("/") else {
        return;
    };
    let dest = backup_root.join(relative);
    if let Some(parent) = dest.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::copy(path, dest);
}

fn build_backup_root() -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = PathBuf::from(format!("/root/pxi-lxc-align-backup-{ts}"));
    let _ = fs::create_dir_all(&path);
    path
}

fn run_pct_checked(args: &[&str], label: &str) {
    let output = Command::new("pct")
        .args(args)
        .output()
        .unwrap_or_else(|err| {
            eprintln!("[align] pct 실행 실패 ({label}): {err}");
            std::process::exit(1);
        });
    if output.status.success() {
        println!("[align] {} 완료", label);
    } else {
        eprintln!(
            "[align] {} 실패: {}",
            label,
            String::from_utf8_lossy(&output.stderr).trim()
        );
        std::process::exit(1);
    }
}

// =============================================================================
// Gateway Fix
// =============================================================================

fn gateway_fix(node: Option<&str>, dry_run: bool) {
    let nodes: Vec<String> = if let Some(n) = node {
        vec![n.to_string()]
    } else {
        let output = cmd_output("pvesh", &["get", "/nodes", "--output-format", "json"]);
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&output) {
            parsed.as_array().map_or(vec![], |arr| {
                arr.iter()
                    .filter_map(|n| n["node"].as_str().map(|s| s.to_string()))
                    .collect()
            })
        } else {
            eprintln!("[gateway-fix] 노드 목록을 가져올 수 없습니다.");
            return;
        }
    };

    let mut total_fixed = 0u32;
    let mut total_skipped = 0u32;

    for node_name in &nodes {
        let correct_gw = get_node_vmbr1_ip(node_name);
        if correct_gw.is_empty() {
            println!("[gateway-fix] {node_name}: vmbr1 IP를 찾을 수 없음 -- 건너뜀");
            continue;
        }

        let conf_dir = format!("/etc/pve/nodes/{node_name}/lxc");
        let entries = match fs::read_dir(&conf_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let mut node_fixes = vec![];
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("conf") {
                continue;
            }
            let vmid = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            for line in content.lines() {
                if !line.starts_with("net") || !line.contains("gw=") {
                    continue;
                }
                if let Some(gw) = line.split(',').find_map(|p| p.strip_prefix("gw=")) {
                    if !gw.is_empty() && gw != correct_gw {
                        node_fixes.push((
                            vmid.clone(),
                            path.clone(),
                            gw.to_string(),
                            line.to_string(),
                        ));
                    } else {
                        total_skipped += 1;
                    }
                }
            }
        }

        if node_fixes.is_empty() {
            println!("[gateway-fix] {node_name}: 수정 필요 없음 (GW={correct_gw})");
            continue;
        }
        println!(
            "[gateway-fix] {node_name}: GW={correct_gw}, {}개 LXC 수정 필요",
            node_fixes.len()
        );

        for (vmid, path, old_gw, old_line) in &node_fixes {
            let new_line = old_line.replace(&format!("gw={old_gw}"), &format!("gw={correct_gw}"));
            if dry_run {
                println!("  [dry-run] {vmid}: gw={old_gw} -> gw={correct_gw}");
            } else {
                let content = fs::read_to_string(path).unwrap_or_default();
                let new_content = content.replace(old_line.as_str(), &new_line);
                if let Err(e) = fs::write(path, &new_content) {
                    eprintln!("  {vmid}: 설정 파일 쓰기 실패: {e}");
                    continue;
                }
                // runtime apply for running LXCs
                let status = cmd_output(
                    "pvesh",
                    &[
                        "get",
                        &format!("/nodes/{node_name}/lxc/{vmid}/status/current"),
                        "--output-format",
                        "json",
                    ],
                );
                if status.contains("\"status\":\"running\"") {
                    let node_ip = node_ip_from_name(node_name);
                    let _ = Command::new("ssh")
                        .args(["-o", "ConnectTimeout=5", "-o", "StrictHostKeyChecking=no",
                            &format!("root@{node_ip}"),
                            &format!("pct exec {vmid} -- bash -c 'ip route replace default via {correct_gw}'")])
                        .status();
                    println!("  {vmid}: gw={old_gw} -> gw={correct_gw} (config + runtime)");
                } else {
                    println!("  {vmid}: gw={old_gw} -> gw={correct_gw} (config only)");
                }
                total_fixed += 1;
            }
        }
    }

    if dry_run {
        println!("\n[gateway-fix] dry-run 완료. 실제 적용: --apply 추가");
    } else {
        println!("\n[gateway-fix] 완료: {total_fixed}개 수정, {total_skipped}개 정상");
    }
}

fn get_node_vmbr1_ip(node: &str) -> String {
    let output = cmd_output(
        "pvesh",
        &[
            "get",
            &format!("/nodes/{node}/network"),
            "--output-format",
            "json",
        ],
    );
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&output) {
        if let Some(ifaces) = parsed.as_array() {
            for iface in ifaces {
                if iface["iface"].as_str() == Some("vmbr1") {
                    if let Some(addr) = iface["address"].as_str() {
                        return addr.to_string();
                    }
                }
            }
        }
    }
    String::new()
}

// =============================================================================
// Route Audit
// =============================================================================

const WEB_PORTS: &[u16] = &[
    80, 443, 8080, 8443, 3000, 3100, 4173, 4200, 5000, 8000, 8065, 8088, 8188, 8189, 8384, 8888,
    9000, 9090, 18789, 2283,
];

const INFRA_NAMES: &[&str] = &[
    "traefik",
    "host-ops",
    "proxmox-host-ops",
    "dalcenter",
    "dalcenter-rs",
    "maddy",
    "mailpit",
    "android-dev",
    "vhost-mysql-dev",
    "vhost-php-dev",
    "vhost-gitlab-sub-traefik",
    "agent-orchestrator",
];

fn route_audit(node: Option<&str>, fix: bool) {
    println!("=== Traefik 라우트 감사 ===\n");

    // Load route backends from Traefik dynamic dirs
    let local = local_node_name();
    let mut all_backends: HashSet<String> = HashSet::new();
    all_backends.extend(load_route_backends_from_dir(&local));

    // Target nodes
    let nodes: Vec<String> = if let Some(n) = node {
        vec![n.to_string()]
    } else {
        let output = cmd_output("pvesh", &["get", "/nodes", "--output-format", "json"]);
        serde_json::from_str::<serde_json::Value>(&output)
            .ok()
            .and_then(|v| {
                v.as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|n| n["node"].as_str().map(|s| s.to_string()))
                        .collect()
                })
            })
            .unwrap_or_default()
    };

    let mut registered = 0u32;
    let mut unregistered = Vec::new();
    let mut infra_skipped = 0u32;

    for node_name in &nodes {
        let output = cmd_output(
            "pvesh",
            &[
                "get",
                &format!("/nodes/{node_name}/lxc"),
                "--output-format",
                "json",
            ],
        );
        let lxcs: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap_or_default();

        for lxc in &lxcs {
            let status = lxc["status"].as_str().unwrap_or("");
            if status != "running" {
                continue;
            }
            let vmid = lxc["vmid"].as_u64().unwrap_or(0);
            let name = lxc["name"].as_str().unwrap_or("");

            if INFRA_NAMES.iter().any(|&infra| name == infra) {
                infra_skipped += 1;
                continue;
            }

            let conf_path = format!("/etc/pve/nodes/{node_name}/lxc/{vmid}.conf");
            let ip = fs::read_to_string(&conf_path)
                .ok()
                .and_then(|content| {
                    content
                        .lines()
                        .find(|l| l.starts_with("net0:"))
                        .and_then(|line| {
                            line.split(',')
                                .find_map(|p| p.strip_prefix("ip="))
                                .map(|ip| ip.split('/').next().unwrap_or(ip).to_string())
                        })
                })
                .unwrap_or_default();

            if ip.is_empty() {
                continue;
            }

            let has_route = all_backends.iter().any(|b| b.contains(&ip));
            if has_route {
                registered += 1;
            } else {
                unregistered.push((node_name.clone(), vmid, name.to_string(), ip));
            }
        }
    }

    if unregistered.is_empty() {
        println!("  모든 실행 중 LXC가 Traefik에 등록되어 있습니다.");
        println!("\n  등록: {registered} | 미등록: 0 | 인프라(스킵): {infra_skipped}");
        return;
    }

    println!("  미등록 서비스 ({}):", unregistered.len());
    println!("  {:<12} {:<8} {:<25} {}", "NODE", "VMID", "NAME", "IP");
    println!("  {}", "-".repeat(65));
    for (node_name, vmid, name, ip) in &unregistered {
        println!("  {:<12} {:<8} {:<25} {}", node_name, vmid, name, ip);
    }
    println!(
        "\n  등록: {registered} | 미등록: {} | 인프라(스킵): {infra_skipped}",
        unregistered.len()
    );

    if !fix {
        println!("\n  자동 등록: pxi-lxc route-audit --fix");
        return;
    }

    println!("\n[route-audit] 웹 포트 탐지 + 자동 등록 시작...\n");
    let mut fixed = 0u32;
    for (node_name, vmid, name, ip) in &unregistered {
        let detected_port = detect_web_port(node_name, &vmid.to_string(), ip);
        if let Some(port) = detected_port {
            let prefix = extract_prefix_from_ip(ip);
            let domain = format!("{name}.{prefix}.internal.kr");
            let backend = if port == 443 {
                format!("https://{ip}:{port}")
            } else {
                format!("http://{ip}:{port}")
            };
            println!("  + {name} -> {domain} -> {backend}");
            // Write route yml directly to Traefik LXC
            let traefik_vmid = find_traefik_vmid();
            if !traefik_vmid.is_empty() {
                let yml = build_route_yml(name, &domain, &backend);
                write_to_lxc(
                    &traefik_vmid,
                    &format!("/opt/traefik/dynamic/{name}.yml"),
                    &yml,
                );
            }
            fixed += 1;
        } else {
            println!("  - {name} ({ip}): 웹 포트 미감지 -- 수동 등록 필요");
        }
    }
    println!("\n[route-audit] {fixed}개 자동 등록 완료");
}

fn detect_web_port(node: &str, vmid: &str, ip: &str) -> Option<u16> {
    let (ok, output) = lxc_exec_on(
        Some(node),
        vmid,
        &[
            "bash",
            "-lc",
            "ss -tlnp 2>/dev/null | awk 'NR>1 {print $4}' | grep -oE '[0-9]+$' | sort -un",
        ],
    );
    if ok {
        let listen_ports: Vec<u16> = output
            .lines()
            .filter_map(|l| l.trim().parse::<u16>().ok())
            .collect();
        for &web_port in WEB_PORTS {
            if listen_ports.contains(&web_port) {
                return Some(web_port);
            }
        }
        for port in &listen_ports {
            if *port >= 1024 && *port != 22 {
                return Some(*port);
            }
        }
    }
    // fallback: host-side port probe
    for &port in &WEB_PORTS[..5] {
        let check = cmd_output(
            "bash",
            &[
                "-c",
                &format!(
                    "timeout 1 bash -c 'echo > /dev/tcp/{ip}/{port}' 2>/dev/null && echo open"
                ),
            ],
        );
        if check.contains("open") {
            return Some(port);
        }
    }
    None
}

fn extract_prefix_from_ip(ip: &str) -> String {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() == 4 && parts[0] == "10" {
        return parts[2].to_string();
    }
    "50".to_string()
}

fn load_route_backends_from_dir(node: &str) -> Vec<String> {
    // Look for route JSON files in /var/lib/pxi or /etc/pxi data dirs
    let paths = [
        format!("/var/lib/pxi/traefik-routes.json"),
        format!("/var/lib/pxi/traefik-routes-{node}.json"),
        format!("/etc/pxi/traefik-routes.json"),
        format!("/etc/pxi/traefik-routes-{node}.json"),
    ];
    for p in &paths {
        if let Ok(data) = fs::read_to_string(p) {
            if let Ok(routes) = serde_json::from_str::<Vec<serde_json::Value>>(&data) {
                return routes
                    .iter()
                    .filter_map(|r| r["backend"].as_str().map(|s| s.to_string()))
                    .collect();
            }
        }
    }
    vec![]
}

fn find_traefik_vmid() -> String {
    let output = cmd_output("pct", &["list"]);
    for line in output.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3
            && parts[1] == "running"
            && parts.last().map_or(false, |n| *n == "traefik")
        {
            return parts[0].to_string();
        }
    }
    String::new()
}

fn build_route_yml(name: &str, domain: &str, backend: &str) -> String {
    format!("http:\n  routers:\n    {name}:\n      rule: \"Host(`{domain}`)\"\n      entryPoints:\n        - websecure\n      service: {name}\n      tls:\n        certResolver: cloudflare\n    {name}-http:\n      rule: \"Host(`{domain}`)\"\n      entryPoints:\n        - web\n      service: {name}\n  services:\n    {name}:\n      loadBalancer:\n        servers:\n          - url: \"{backend}\"\n")
}

fn write_to_lxc(vmid: &str, path: &str, content: &str) {
    let tmp = format!("/tmp/pxi-lxc-{}", path.replace('/', "_"));
    let _ = fs::write(&tmp, content);
    let _ = Command::new("pct")
        .args(["push", vmid, &tmp, path])
        .status();
    let _ = fs::remove_file(&tmp);
}

// =============================================================================
// Route Audit Watch (systemd timer)
// =============================================================================

fn route_audit_watch(action: &str) {
    let service_unit = "/etc/systemd/system/pxi-route-audit.service";
    let timer_unit = "/etc/systemd/system/pxi-route-audit.timer";

    match action {
        "install" => {
            let service = "[Unit]\n\
Description=pxi Traefik route audit + auto-register\n\
After=pve-cluster.service\n\
\n\
[Service]\n\
Type=oneshot\n\
ExecStart=/usr/local/bin/pxi-lxc route-audit --fix\n\
StandardOutput=journal\n\
StandardError=journal\n";

            let timer = "[Unit]\n\
Description=pxi Traefik route audit timer\n\
Requires=pxi-route-audit.service\n\
\n\
[Timer]\n\
OnBootSec=30s\n\
OnUnitActiveSec=1min\n\
AccuracySec=10s\n\
Unit=pxi-route-audit.service\n\
\n\
[Install]\n\
WantedBy=timers.target\n";

            fs::write(service_unit, service).expect("service unit 쓰기 실패");
            fs::write(timer_unit, timer).expect("timer unit 쓰기 실패");

            for cmd in [
                vec!["daemon-reload"],
                vec!["enable", "pxi-route-audit.timer"],
                vec!["start", "pxi-route-audit.timer"],
            ] {
                let _ = Command::new("systemctl").args(&cmd).status();
            }

            println!("[route-audit-watch] systemd timer 설치 완료");
            println!("  주기: 부팅 후 30초, 이후 1분마다");
            println!("  로그: journalctl -u pxi-route-audit.service -f");
        }
        "uninstall" => {
            let _ = Command::new("systemctl")
                .args(["stop", "pxi-route-audit.timer"])
                .status();
            let _ = Command::new("systemctl")
                .args(["disable", "pxi-route-audit.timer"])
                .status();
            let _ = fs::remove_file(service_unit);
            let _ = fs::remove_file(timer_unit);
            let _ = Command::new("systemctl").args(["daemon-reload"]).status();
            println!("[route-audit-watch] systemd timer 제거 완료");
        }
        "status" => {
            let timer_status = cmd_output("systemctl", &["is-active", "pxi-route-audit.timer"]);
            let timer_enabled = cmd_output("systemctl", &["is-enabled", "pxi-route-audit.timer"]);
            println!("[route-audit-watch] timer status:");
            println!("  active:  {}", timer_status);
            println!("  enabled: {}", timer_enabled);

            let next_run = cmd_output(
                "systemctl",
                &[
                    "show",
                    "pxi-route-audit.timer",
                    "--property=NextElapseUSecRealtime",
                    "--value",
                ],
            );
            if !next_run.is_empty() {
                println!("  next run: {}", next_run);
            }

            println!("\n[route-audit-watch] recent logs:");
            let logs = cmd_output(
                "journalctl",
                &["-u", "pxi-route-audit.service", "-n", "10", "--no-pager"],
            );
            println!("{}", logs);
        }
        _ => {
            eprintln!("[route-audit-watch] action: install / uninstall / status 중 선택");
        }
    }
}

// =============================================================================
// Init (locale + timezone + packages)
// =============================================================================

fn init_lxc(
    vmid: &str,
    locale_v: &str,
    timezone_v: &str,
    packages_csv: &str,
) -> anyhow::Result<()> {
    println!("=== LXC {vmid} 초기화 ===");
    require_proxmox()?;
    let status_out = common::run_str("pct", &["status", vmid])?;
    let parsed: pxi_core::types::LxcStatus = status_out.parse().unwrap();
    if !parsed.is_running() {
        println!("[init] LXC 시작 중 (pct start {vmid})");
        common::run_str("pct", &["start", vmid])?;
    }

    if locale_v != "none" {
        setup_locale(vmid, locale_v)?;
    }
    if timezone_v != "none" {
        setup_timezone(vmid, timezone_v)?;
    }
    let pkgs: Vec<&str> = packages_csv
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if !pkgs.is_empty() {
        install_base_packages(vmid, &pkgs)?;
    }
    println!("  LXC {vmid} 초기화 완료");
    Ok(())
}

fn pct_exec(vmid: &str, script: &str) -> anyhow::Result<String> {
    common::run_str("pct", &["exec", vmid, "--", "bash", "-c", script])
}

fn setup_locale(vmid: &str, locale_v: &str) -> anyhow::Result<()> {
    if !locale_v
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        anyhow::bail!("locale 값이 비정상: {locale_v:?}");
    }
    let current = pct_exec(vmid, "locale 2>&1 || true").unwrap_or_default();
    if !current.contains("Cannot set") && current.contains(locale_v) {
        println!("[locale] 이미 {locale_v} 설정됨");
        return Ok(());
    }
    println!("[locale] {locale_v} 설정 중...");
    let script = format!(
        "apt-get install -y -qq locales 2>/dev/null && \
         sed -i '/{locale_v}/s/^# //' /etc/locale.gen && locale-gen && \
         echo 'LANG={locale_v}' > /etc/default/locale"
    );
    pct_exec(vmid, &script)?;
    let verify = pct_exec(vmid, "locale 2>&1 || true").unwrap_or_default();
    if verify.contains("Cannot set") {
        println!("[locale] locale-gen 실패 -> locales-all 재시도");
        pct_exec(vmid, &format!(
            "apt-get install -y -qq locales-all 2>/dev/null && echo 'LANG={locale_v}' > /etc/default/locale"
        ))?;
    }
    println!("[locale] {locale_v} 완료");
    Ok(())
}

fn setup_timezone(vmid: &str, tz: &str) -> anyhow::Result<()> {
    if !tz
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '_' || c == '-' || c == '+')
    {
        anyhow::bail!("timezone 값이 비정상: {tz:?}");
    }
    if tz.contains("..") || tz.starts_with('/') {
        anyhow::bail!("timezone 값에 '..' 또는 선행 '/' 금지: {tz:?}");
    }
    let check = pct_exec(
        vmid,
        &format!("test -f /usr/share/zoneinfo/{tz} && echo ok"),
    )
    .unwrap_or_default();
    if !check.contains("ok") {
        anyhow::bail!("LXC {vmid} 안에 /usr/share/zoneinfo/{tz} 파일 없음");
    }
    let current = pct_exec(vmid, "cat /etc/timezone 2>/dev/null || true").unwrap_or_default();
    if current.trim() == tz {
        println!("[tz] 이미 {tz}");
        return Ok(());
    }
    println!("[tz] {tz} 설정 중...");
    pct_exec(
        vmid,
        &format!("ln -sf /usr/share/zoneinfo/{tz} /etc/localtime && echo '{tz}' > /etc/timezone"),
    )?;
    println!("[tz] {tz} 완료");
    Ok(())
}

fn install_base_packages(vmid: &str, pkgs: &[&str]) -> anyhow::Result<()> {
    for p in pkgs {
        if p.starts_with('-') {
            anyhow::bail!("패키지 이름이 '-'로 시작할 수 없음: {p:?}");
        }
        if !p
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '+')
        {
            anyhow::bail!("패키지 이름이 비정상: {p:?}");
        }
    }
    let check = pkgs.iter().map(|p| {
        format!("printf '%s\\t' '{p}'; dpkg-query -W -f='${{Status}}\\n' '{p}' 2>/dev/null || echo 'missing'")
    }).collect::<Vec<_>>().join("; ");
    let out = pct_exec(vmid, &check)?;
    let mut missing: Vec<&str> = Vec::new();
    for p in pkgs {
        let matched = out.lines().find_map(|line| {
            let (name, status) = line.split_once('\t')?;
            if name == *p {
                Some(status)
            } else {
                None
            }
        });
        match matched {
            Some(status) => {
                if !status.contains("install ok installed") {
                    missing.push(p);
                }
            }
            None => anyhow::bail!("dpkg-query probe 출력에 '{p}' 라인 없음"),
        }
    }
    if missing.is_empty() {
        println!("[packages] 이미 모두 설치됨 ({} 개)", pkgs.len());
        return Ok(());
    }
    let joined = missing.join(" ");
    println!("[packages] 누락 설치: {joined}");
    pct_exec(
        vmid,
        &format!(
            "DEBIAN_FRONTEND=noninteractive apt-get update -qq && \
         DEBIAN_FRONTEND=noninteractive apt-get install -y -qq -- {joined}"
        ),
    )?;
    println!("[packages] {} 개 설치 완료", missing.len());
    Ok(())
}

// =============================================================================
// Doctor
// =============================================================================

fn doctor() {
    println!("=== pxi-lxc doctor ===");
    println!(
        "  pct:       {}",
        if common::has_cmd("pct") {
            "ok"
        } else {
            "missing"
        }
    );
    println!(
        "  vzdump:    {}",
        if common::has_cmd("vzdump") {
            "ok"
        } else {
            "missing"
        }
    );
    println!(
        "  pveam:     {}",
        if common::has_cmd("pveam") {
            "ok"
        } else {
            "missing"
        }
    );
    println!(
        "  pvesh:     {}",
        if common::has_cmd("pvesh") {
            "ok"
        } else {
            "missing"
        }
    );
    println!(
        "  proxmox:   {}",
        if os::is_proxmox() { "ok" } else { "no" }
    );
}

// =============================================================================
// Management LXC setup
// =============================================================================

fn mgmt_setup(
    vmid: &str,
    hostname: &str,
    storage: &str,
    disk: &str,
    cores: &str,
    _memory: &str,
    bootstrap: bool,
) -> anyhow::Result<()> {
    println!("=== 관리 LXC 생성: {vmid} ({hostname}) ===\n");

    let ip = vmid_to_ip(vmid);

    // 1. Create LXC
    println!("[mgmt] 1/5 LXC 생성...");
    create(
        vmid,
        hostname,
        &ip,
        "debian-13",
        storage,
        disk,
        cores,
        "2048",
        None,
        "vmbr1",
    )?;

    if bootstrap {
        println!("\n{}\n", "─".repeat(50));
        self::bootstrap(vmid)?;
        println!("\n{}\n", "─".repeat(50));
    }

    // 2. Remove root password
    println!("[mgmt] 2/5 root 비밀번호 제거...");
    lxc_exec(vmid, &["passwd", "-d", "root"]);

    // 3. Install management packages
    println!("\n[mgmt] 3/5 관리 패키지 설치...");
    lxc_exec(vmid, &["bash", "-c",
        "DEBIAN_FRONTEND=noninteractive apt-get update -qq && apt-get install -y -qq openssh-client jq"
    ]);

    // 4. SSH key setup (LXC -> host)
    println!("\n[mgmt] 4/5 SSH 키 설정...");
    let host_ip = cmd_output("bash", &["-c", "hostname -I | awk '{print $1}'"]);
    let host_ip = if host_ip.is_empty() {
        "127.0.0.1".to_string()
    } else {
        host_ip
    };

    let (has_key, _) = lxc_exec(vmid, &["test", "-f", "/root/.ssh/id_ed25519"]);
    if !has_key {
        lxc_exec(vmid, &["mkdir", "-p", "/root/.ssh"]);
        lxc_exec(
            vmid,
            &[
                "ssh-keygen",
                "-t",
                "ed25519",
                "-f",
                "/root/.ssh/id_ed25519",
                "-N",
                "",
                "-C",
                &format!("mgmt-lxc-{vmid}"),
            ],
        );
    }

    // Read pubkey and add to host authorized_keys
    let (pub_ok, pubkey) = lxc_exec(vmid, &["cat", "/root/.ssh/id_ed25519.pub"]);
    if pub_ok && !pubkey.is_empty() {
        let auth_keys_path = "/root/.ssh/authorized_keys";
        let existing = fs::read_to_string(auth_keys_path).unwrap_or_default();
        if !existing.contains(pubkey.trim()) {
            fs::create_dir_all("/root/.ssh").ok();
            let mut content = existing;
            if !content.ends_with('\n') && !content.is_empty() {
                content.push('\n');
            }
            content.push_str(pubkey.trim());
            content.push('\n');
            fs::write(auth_keys_path, &content)?;
            println!("[mgmt] 호스트 authorized_keys에 등록 완료");
        }
    }

    let ssh_config = format!(
        "Host host\n  HostName {host_ip}\n  User root\n  StrictHostKeyChecking no\n  UserKnownHostsFile /dev/null\n"
    );
    lxc_exec(
        vmid,
        &[
            "bash",
            "-c",
            &format!("cat > /root/.ssh/config << 'SSHEOF'\n{ssh_config}SSHEOF"),
        ],
    );
    lxc_exec(vmid, &["chmod", "600", "/root/.ssh/config"]);

    // 5. Proxmox API token
    println!("\n[mgmt] 5/5 Proxmox API 토큰 발급...");
    let token_id = format!("mgmt-{hostname}");
    // Remove existing token if present
    let _ = Command::new("pveum")
        .args(["user", "token", "remove", "root@pam", &token_id])
        .output();

    let token_out = Command::new("pveum")
        .args([
            "user",
            "token",
            "add",
            "root@pam",
            &token_id,
            "--privsep",
            "0",
            "--output-format",
            "json",
        ])
        .output()?;

    if token_out.status.success() {
        let stdout = String::from_utf8_lossy(&token_out.stdout);
        if let Some(val) = stdout
            .lines()
            .find(|l| l.contains("value"))
            .and_then(|l| l.split('"').nth(3))
        {
            let api_env = format!(
                "PVE_API_URL=https://{host_ip}:8006/api2/json\nPVE_API_TOKEN=root@pam!{token_id}={val}\n"
            );
            lxc_exec(
                vmid,
                &[
                    "bash",
                    "-c",
                    &format!("cat > /etc/proxmox-api.env << 'ENVEOF'\n{api_env}ENVEOF"),
                ],
            );
            lxc_exec(vmid, &["chmod", "600", "/etc/proxmox-api.env"]);
            println!("[mgmt] API 토큰 발급 완료");
        }
    }

    println!("\n=== 관리 LXC {vmid} ({hostname}) 설정 완료 ===");
    println!("  접속: pxi-lxc enter {vmid}");
    println!("  SSH:  ssh root@{ip}");
    Ok(())
}

/// VMID에서 IP를 유추 (10.0.50.{vmid 뒤 숫자}/16)
fn vmid_to_ip(vmid: &str) -> String {
    let last3: String = vmid
        .chars()
        .rev()
        .take(3)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let num: u32 = last3.parse().unwrap_or(0);
    let third = num / 256;
    let fourth = num % 256;
    format!("10.0.{third}.{fourth}/16")
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ----- parse_pct_status -----

    #[test]
    fn status_running() {
        assert_eq!(parse_pct_status("status: running\n").unwrap(), "running");
    }

    #[test]
    fn status_stopped_no_trailing_newline() {
        assert_eq!(parse_pct_status("status: stopped").unwrap(), "stopped");
    }

    #[test]
    fn status_unknown_fallback() {
        assert_eq!(parse_pct_status("status: unknown\n").unwrap(), "unknown");
    }

    #[test]
    fn status_rejects_extra_lines() {
        assert!(parse_pct_status("status: running\nwarning: drift\n").is_err());
    }

    #[test]
    fn status_rejects_missing_prefix() {
        assert!(parse_pct_status("state: running\n").is_err());
        assert!(parse_pct_status(" status: running\n").is_err());
    }

    #[test]
    fn status_rejects_value_drift() {
        assert!(parse_pct_status("status: \n").is_err());
        assert!(parse_pct_status("status:  running\n").is_err());
        assert!(parse_pct_status("status: running \n").is_err());
        assert!(parse_pct_status("status: paused\n").is_err());
    }

    // ----- parse_pct_list -----

    #[test]
    fn list_4_columns_with_lock() {
        let out = "VMID       Status     Lock         Name\n\
                   100        running    backup       myhost\n";
        let rows = parse_pct_list(out).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].vmid, "100");
        assert_eq!(rows[0].lock, "backup");
        assert_eq!(rows[0].name, "myhost");
    }

    #[test]
    fn list_4_columns_dash_lock_empties_lock() {
        let out = "VMID       Status     Lock         Name\n\
                   100        running    -            myhost\n";
        let rows = parse_pct_list(out).unwrap();
        assert_eq!(rows[0].lock, "");
    }

    #[test]
    fn list_3_columns_no_lock() {
        let out = "VMID       Status     Name\n\
                   100        stopped    myhost\n";
        let rows = parse_pct_list(out).unwrap();
        assert_eq!(rows[0].vmid, "100");
        assert_eq!(rows[0].lock, "");
    }

    #[test]
    fn list_skips_empty_lines() {
        let out = "VMID       Status     Lock         Name\n\
                   100        running    -            a\n\
                   \n\
                   101        stopped    -            b\n";
        let rows = parse_pct_list(out).unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn list_fails_on_unknown_columns() {
        let out = "VMID       Status     Lock         Name      Extra\n\
                   100        running    -            a         x\n";
        assert!(parse_pct_list(out).is_err());
    }

    // ----- parse_pct_listsnapshot -----

    #[test]
    fn snapshot_list_skips_current() {
        let out = "`-> current                                            You are here!\n";
        let rows = parse_pct_listsnapshot(out).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn snapshot_list_real_format() {
        let out = "`-> snap1   2026-04-15 09:04:11     no-description\n\
                   `-> current                                You are here!\n";
        let rows = parse_pct_listsnapshot(out).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "snap1");
        assert_eq!(rows[0].timestamp, "2026-04-15 09:04:11");
    }

    #[test]
    fn snapshot_list_multi_word_description() {
        let out = "`-> snap1 2026-04-15 09:04:11 hello world\n";
        let rows = parse_pct_listsnapshot(out).unwrap();
        assert_eq!(rows[0].description, "hello world");
    }

    #[test]
    fn snapshot_list_rejects_bad_date() {
        assert!(parse_pct_listsnapshot("`-> snap1 2026-04 09:04:11 desc\n").is_err());
    }

    #[test]
    fn snapshot_list_rejects_bad_time() {
        assert!(parse_pct_listsnapshot("`-> snap1 2026-04-15 BAD desc\n").is_err());
    }

    // ----- align helpers -----

    #[test]
    fn aligned_ip_keeps_network_and_cidr() {
        let updated = build_aligned_ip_cidr("10.0.50.113/16", 50054).unwrap();
        assert_eq!(updated, "10.0.50.54/16");
    }

    #[test]
    fn exact_ip_sub_does_not_touch_longer_addresses() {
        let input = "A=10.0.50.113 B=10.0.50.1134 C=210.0.50.113";
        let output = exact_ip_sub(input, "10.0.50.113", "10.0.50.54");
        assert_eq!(output, "A=10.0.50.54 B=10.0.50.1134 C=210.0.50.113");
    }

    #[test]
    fn rewrite_net0_ip_replaces_existing_ip() {
        let config = "hostname: vhost-test\nnet0: name=eth0,bridge=vmbr1,gw=10.0.50.1,ip=10.0.50.113/16,type=veth\n";
        let output = rewrite_net0_ip(config, "10.0.50.54/16");
        assert!(output.contains("ip=10.0.50.54/16"));
        assert!(!output.contains("ip=10.0.50.113/16"));
    }

    // ----- safety -----

    #[test]
    fn test_dangerous_command_detection() {
        assert!(detect_dangerous_command("rm -rf /etc/corosync/*").is_some());
        assert!(detect_dangerous_command("killall pmxcfs").is_some());
        assert!(detect_dangerous_command("echo ok").is_none());
        assert!(detect_dangerous_command("pct list").is_none());
    }
}
