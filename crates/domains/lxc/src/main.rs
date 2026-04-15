//! prelik-lxc — Proxmox LXC 수명 관리.
//! pct 바이너리를 전제로 함 (Proxmox VE 호스트에서만 동작).

use clap::{Parser, Subcommand};
use prelik_core::{common, os};
use serde::Serialize;

#[derive(Parser)]
#[command(name = "prelik-lxc", about = "LXC 수명 관리 (Proxmox pct 래퍼)")]
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
    SnapshotRestore {
        vmid: String,
        name: String,
    },
    /// LXC 스냅샷 삭제
    SnapshotDelete {
        vmid: String,
        name: String,
    },
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
        #[arg(long, default_value = "git,curl,wget,rsync,tmux,jq,htop,tree,unzip,locales")]
        packages: String,
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
        } => create(&vmid, &hostname, &ip, &template, &storage, &disk, &cores, &memory, gateway.as_deref(), &bridge),
        Cmd::Start { vmid } => start(&vmid),
        Cmd::Stop { vmid } => stop(&vmid),
        Cmd::Restart { vmid } => restart(&vmid),
        Cmd::Delete { vmid, force } => delete(&vmid, force),
        Cmd::Enter { vmid } => enter(&vmid),
        Cmd::Backup { vmid, storage, mode } => backup(&vmid, &storage, &mode),
        Cmd::SnapshotCreate { vmid, name, description } => snapshot_create(&vmid, &name, description.as_deref()),
        Cmd::SnapshotList { vmid } => snapshot_list(&vmid, json),
        Cmd::SnapshotRestore { vmid, name } => snapshot_restore(&vmid, &name),
        Cmd::SnapshotDelete { vmid, name } => snapshot_delete(&vmid, &name),
        Cmd::Resize { vmid, cores, memory, disk_expand } => resize(&vmid, cores.as_deref(), memory.as_deref(), disk_expand.as_deref()),
        Cmd::Init { vmid, locale, timezone, packages } => init_lxc(&vmid, &locale, &timezone, &packages),
        Cmd::Doctor => {
            doctor();
            Ok(())
        }
    }
}

fn snapshot_create(vmid: &str, name: &str, description: Option<&str>) -> anyhow::Result<()> {
    println!("=== LXC {vmid} 스냅샷 생성: {name} ===");
    let mut args: Vec<&str> = vec!["snapshot", vmid, name];
    if let Some(d) = description {
        args.push("--description");
        args.push(d);
    }
    common::run("pct", &args)?;
    println!("✓ 스냅샷 생성 완료");
    Ok(())
}

// 실제 형식: `-> name [YYYY-MM-DD HH:MM:SS] description...
// current는 timestamp 없이 "You are here!"만 옴 → skip.
fn parse_pct_listsnapshot(out: &str) -> anyhow::Result<Vec<SnapshotRow>> {
    let mut rows = Vec::new();
    for l in out.lines() {
        let trimmed = l.trim_start_matches(|c: char| {
            c == '`' || c == '-' || c == '>' || c.is_whitespace()
        });
        if trimmed.is_empty() { continue; }
        let toks: Vec<&str> = trimmed.split_whitespace().collect();
        if toks.is_empty() { continue; }
        let name = toks[0].to_string();
        if name == "current" { continue; }
        let date_ok = toks.get(1).map(|t| {
            let b = t.as_bytes();
            t.len() == 10 && b[4] == b'-' && b[7] == b'-'
                && b[..4].iter().all(|c| c.is_ascii_digit())
                && b[5..7].iter().all(|c| c.is_ascii_digit())
                && b[8..10].iter().all(|c| c.is_ascii_digit())
        }).unwrap_or(false);
        let time_ok = toks.get(2).map(|t| {
            let b = t.as_bytes();
            t.len() == 8 && b[2] == b':' && b[5] == b':'
                && b[..2].iter().all(|c| c.is_ascii_digit())
                && b[3..5].iter().all(|c| c.is_ascii_digit())
                && b[6..8].iter().all(|c| c.is_ascii_digit())
        }).unwrap_or(false);
        if !date_ok || !time_ok {
            anyhow::bail!(
                "pct listsnapshot 라인 파싱 실패 (timestamp가 YYYY-MM-DD HH:MM:SS 아님): {l:?}"
            );
        }
        let timestamp = format!("{} {}", toks[1], toks[2]);
        let description = toks.iter().skip(3).copied().collect::<Vec<_>>().join(" ");
        rows.push(SnapshotRow { name, timestamp, description });
    }
    Ok(rows)
}

fn snapshot_list(vmid: &str, json: bool) -> anyhow::Result<()> {
    let out = common::run("pct", &["listsnapshot", vmid])?;
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
    common::run("pct", &["rollback", vmid, name])?;
    println!("✓ 복원 완료 — LXC 상태가 '{name}' 시점으로 되돌아감");
    Ok(())
}

fn snapshot_delete(vmid: &str, name: &str) -> anyhow::Result<()> {
    println!("=== LXC {vmid} 스냅샷 삭제: {name} ===");
    common::run("pct", &["delsnapshot", vmid, name])?;
    println!("✓ 삭제 완료");
    Ok(())
}

fn resize(vmid: &str, cores: Option<&str>, memory: Option<&str>, disk_expand: Option<&str>) -> anyhow::Result<()> {
    println!("=== LXC {vmid} 리소스 변경 ===");
    if cores.is_none() && memory.is_none() && disk_expand.is_none() {
        anyhow::bail!("--cores / --memory / --disk-expand 중 최소 하나 필요");
    }

    if let Some(c) = cores {
        common::run("pct", &["set", vmid, "--cores", c])?;
        println!("  ✓ cores: {c}");
    }
    if let Some(m) = memory {
        common::run("pct", &["set", vmid, "--memory", m])?;
        println!("  ✓ memory: {m} MB");
    }
    if let Some(d) = disk_expand {
        // +4G 형식. rootfs 확장
        common::run("pct", &["resize", vmid, "rootfs", d])?;
        println!("  ✓ disk expand: {d}");
    }
    println!("변경 사항은 재시작 후 반영될 수 있습니다 (cores/memory는 라이브 가능)");
    Ok(())
}

fn require_proxmox() -> anyhow::Result<()> {
    if !common::has_cmd("pct") {
        anyhow::bail!("pct 바이너리 없음 — Proxmox VE 호스트에서만 동작합니다");
    }
    Ok(())
}

// 순수 파서 — 회귀 테스트 용이.
fn parse_pct_list(out: &str) -> anyhow::Result<Vec<LxcRow>> {
    let mut rows = Vec::new();
    for l in out.lines().skip(1) {
        if l.trim().is_empty() { continue; }
        let p: Vec<&str> = l.split_whitespace().collect();
        let row = match p.len() {
            4 => LxcRow {
                vmid: p[0].into(), status: p[1].into(),
                lock: if p[2] == "-" { String::new() } else { p[2].into() },
                name: p[3].into(),
            },
            3 => LxcRow {
                vmid: p[0].into(), status: p[1].into(),
                lock: String::new(), name: p[2].into(),
            },
            _ => anyhow::bail!("pct list 라인 파싱 실패 (컬럼 {}개): {l:?}", p.len()),
        };
        rows.push(row);
    }
    Ok(rows)
}

fn list(json: bool) -> anyhow::Result<()> {
    let out = common::run("pct", &["list"])?;
    if !json {
        println!("{out}");
        return Ok(());
    }
    let rows = parse_pct_list(&out)?;
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}

// upstream pve-container `pct status` 실제 출력값:
//   정상: "running", "stopped"
//   $stat->{status}가 없을 때 fallback: "unknown"
// paused/suspended는 LXC 미적용 (VM은 prelik-vm 별도).
const STATUS_KNOWN: &[&str] = &["running", "stopped", "unknown"];

// raw stdout만 받아 엄격 검증 — 순수 함수.
fn parse_pct_status(raw: &str) -> anyhow::Result<&str> {
    let body = raw.strip_suffix('\n').unwrap_or(raw);
    if body.contains('\n') {
        anyhow::bail!("pct status 출력이 단일 라인이 아님: {raw:?}");
    }
    let value = body.strip_prefix("status: ")
        .ok_or_else(|| anyhow::anyhow!("pct status 출력 형식이 'status: <value>' 아님: {raw:?}"))?;
    if !STATUS_KNOWN.contains(&value) {
        anyhow::bail!("pct status 값이 알 수 없는 형태: {value:?} (허용: {STATUS_KNOWN:?})");
    }
    Ok(value)
}

fn status(vmid: &str, json: bool) -> anyhow::Result<()> {
    if !json {
        let out = common::run("pct", &["status", vmid])?;
        println!("{out}");
        return Ok(());
    }
    let output = std::process::Command::new("pct").args(["status", vmid]).output()?;
    if !output.status.success() {
        anyhow::bail!("pct status {vmid} 실패: {}", String::from_utf8_lossy(&output.stderr));
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
        assert!(parse_pct_status("status:  running\n").is_err()); // 2칸 공백
        assert!(parse_pct_status("status: running \n").is_err()); // 트레일링 공백
        assert!(parse_pct_status("status: paused\n").is_err());   // VM 전용 값
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
        assert_eq!(rows[0].name, "myhost");
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
        assert_eq!(rows[0].description, "no-description");
    }

    #[test]
    fn snapshot_list_multi_word_description() {
        let out = "`-> snap1 2026-04-15 09:04:11 hello world\n";
        let rows = parse_pct_listsnapshot(out).unwrap();
        assert_eq!(rows[0].description, "hello world");
    }

    #[test]
    fn snapshot_list_rejects_bad_date() {
        let out = "`-> snap1 2026-04 09:04:11 desc\n";
        assert!(parse_pct_listsnapshot(out).is_err());
    }

    #[test]
    fn snapshot_list_rejects_bad_time() {
        let out = "`-> snap1 2026-04-15 BAD desc\n";
        assert!(parse_pct_listsnapshot(out).is_err());
    }

    #[test]
    fn snapshot_list_rejects_missing_time() {
        let out = "`-> snap1 2026-04-15\n";
        assert!(parse_pct_listsnapshot(out).is_err());
    }

    #[test]
    fn snapshot_list_rejects_non_digit_date() {
        let out = "`-> snap1 20XX-04-15 09:04:11 desc\n";
        assert!(parse_pct_listsnapshot(out).is_err());
    }
}

// ========== init ==========

fn init_lxc(vmid: &str, locale_v: &str, timezone_v: &str, packages_csv: &str) -> anyhow::Result<()> {
    println!("=== LXC {vmid} 초기화 ===");
    require_proxmox()?;
    // running 상태 확인
    let status_out = common::run("pct", &["status", vmid])?;
    if !status_out.contains("running") {
        println!("[init] LXC 시작 중 (pct start {vmid})");
        common::run("pct", &["start", vmid])?;
    }

    if locale_v != "none" {
        setup_locale(vmid, locale_v)?;
    }
    if timezone_v != "none" {
        setup_timezone(vmid, timezone_v)?;
    }
    let pkgs: Vec<&str> = packages_csv.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if !pkgs.is_empty() {
        install_base_packages(vmid, &pkgs)?;
    }
    println!("✓ LXC {vmid} 초기화 완료");
    Ok(())
}

fn pct_exec(vmid: &str, script: &str) -> anyhow::Result<String> {
    common::run("pct", &["exec", vmid, "--", "bash", "-c", script])
}

fn setup_locale(vmid: &str, locale_v: &str) -> anyhow::Result<()> {
    // locale_v 검증 — shell injection + locale.gen 비정상 엔트리 방지
    if !locale_v.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-') {
        anyhow::bail!("locale 값이 비정상: {locale_v:?} (예: ko_KR.UTF-8)");
    }
    // 이미 설정돼 있으면 skip
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
    // 검증 + fallback (locales-all)
    let verify = pct_exec(vmid, "locale 2>&1 || true").unwrap_or_default();
    if verify.contains("Cannot set") {
        println!("[locale] locale-gen 실패 → locales-all 재시도");
        pct_exec(vmid, &format!(
            "apt-get install -y -qq locales-all 2>/dev/null && \
             echo 'LANG={locale_v}' > /etc/default/locale"
        ))?;
    }
    println!("[locale] ✓ {locale_v}");
    Ok(())
}

fn setup_timezone(vmid: &str, tz: &str) -> anyhow::Result<()> {
    // tz 검증 — zoneinfo 경로 조립 시 traversal 차단
    if !tz.chars().all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '_' || c == '-' || c == '+') {
        anyhow::bail!("timezone 값이 비정상: {tz:?} (예: Asia/Seoul)");
    }
    if tz.contains("..") || tz.starts_with('/') {
        anyhow::bail!("timezone 값에 '..' 또는 선행 '/' 금지: {tz:?}");
    }
    // 실제 zoneinfo 파일 존재 검증 — 'Asia/Seou' 같은 오타 거부.
    // 이거 빠지면 broken symlink로 /etc/timezone만 쓰고 skip 영구화.
    let check = pct_exec(vmid, &format!("test -f /usr/share/zoneinfo/{tz} && echo ok"))
        .unwrap_or_default();
    if !check.contains("ok") {
        anyhow::bail!("LXC {vmid} 안에 /usr/share/zoneinfo/{tz} 파일 없음 — 유효한 tz인지 확인");
    }
    let current = pct_exec(vmid, "cat /etc/timezone 2>/dev/null || true").unwrap_or_default();
    if current.trim() == tz {
        println!("[tz] 이미 {tz}");
        return Ok(());
    }
    println!("[tz] {tz} 설정 중...");
    let script = format!(
        "ln -sf /usr/share/zoneinfo/{tz} /etc/localtime && echo '{tz}' > /etc/timezone"
    );
    pct_exec(vmid, &script)?;
    println!("[tz] ✓ {tz}");
    Ok(())
}

fn install_base_packages(vmid: &str, pkgs: &[&str]) -> anyhow::Result<()> {
    for p in pkgs {
        // 선행 '-' 차단 — apt option injection (--allow-downgrades 등) 방지
        if p.starts_with('-') {
            anyhow::bail!("패키지 이름이 '-'로 시작할 수 없음: {p:?}");
        }
        if !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '+') {
            anyhow::bail!("패키지 이름이 비정상: {p:?}");
        }
    }
    // idempotency: 이미 설치된 것 제외. dpkg-query로 누락만 계산.
    // `dpkg-query -W -f='${Status}' <pkg>`가 "install ok installed"면 있음.
    let check = pkgs.iter().map(|p| {
        // 패키지명은 이미 검증됨
        format!("printf '%s\\t' '{p}'; dpkg-query -W -f='${{Status}}\\n' '{p}' 2>/dev/null || echo 'missing'")
    }).collect::<Vec<_>>().join("; ");
    let out = pct_exec(vmid, &check).unwrap_or_default();
    let mut missing: Vec<&str> = Vec::new();
    for line in out.lines() {
        let (name, status) = match line.split_once('\t') {
            Some(v) => v,
            None => continue,
        };
        if !status.contains("install ok installed") {
            if let Some(&p) = pkgs.iter().find(|p| **p == name) {
                missing.push(p);
            }
        }
    }
    if missing.is_empty() {
        println!("[packages] ✓ 이미 모두 설치됨 ({} 개)", pkgs.len());
        return Ok(());
    }
    let joined = missing.join(" ");
    println!("[packages] 누락 설치: {joined}");
    // '--'로 옵션 종료 강제 — 패키지명이 '-'로 시작하면 앞선 검증이 거부했지만
    // 이중 방어.
    pct_exec(vmid, &format!(
        "DEBIAN_FRONTEND=noninteractive apt-get update -qq && \
         DEBIAN_FRONTEND=noninteractive apt-get install -y -qq -- {joined}"
    ))?;
    println!("[packages] ✓ {} 개 설치", missing.len());
    Ok(())
}
