//! prelik-vm — Proxmox QEMU VM 관리 (qm 래퍼).
//! LXC와 별개. vzdump는 LXC와 공통.

use clap::{Parser, Subcommand};
use prelik_core::common;
use serde::Serialize;

#[derive(Parser)]
#[command(name = "prelik-vm", about = "Proxmox QEMU VM 관리")]
struct Cli {
    /// list/status를 JSON으로 출력 (자동화/CI 친화)
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Serialize, Debug, PartialEq)]
struct VmRow {
    vmid: String,
    name: String,
    status: String,
    mem_mb: String,
    disk_gb: String,
    pid: Option<String>,
}

// upstream qemu-server VM 상태값:
//   running, stopped, paused, suspended, prelaunch
// (LXC는 paused/suspended 안 씀. VM은 ACPI suspend 등으로 가능)
const STATUS_KNOWN: &[&str] = &["running", "stopped", "paused", "suspended", "prelaunch"];

// 순수 파서 — qm list 출력. 헤더 + 데이터 라인.
// "VMID NAME STATUS MEM(MB) BOOTDISK(GB) PID" → 5컬럼(PID 0/누락) 또는 6컬럼.
fn parse_qm_list(text: &str) -> anyhow::Result<Vec<VmRow>> {
    let mut rows = Vec::new();
    for line in text.lines().skip(1) {
        if line.trim().is_empty() { continue; }
        let p: Vec<&str> = line.split_whitespace().collect();
        let row = match p.len() {
            5 => VmRow {
                vmid: p[0].into(), name: p[1].into(), status: p[2].into(),
                mem_mb: p[3].into(), disk_gb: p[4].into(), pid: None,
            },
            6 => VmRow {
                vmid: p[0].into(), name: p[1].into(), status: p[2].into(),
                mem_mb: p[3].into(), disk_gb: p[4].into(),
                pid: if p[5] == "0" { None } else { Some(p[5].into()) },
            },
            _ => anyhow::bail!("qm list 라인 파싱 실패 (컬럼 {}개): {line:?}", p.len()),
        };
        // status JSON 경로와 동일한 whitelist 계약 — drift 거부.
        if !STATUS_KNOWN.contains(&row.status.as_str()) {
            anyhow::bail!(
                "qm list 행의 status가 알 수 없는 형태: {:?} (허용: {STATUS_KNOWN:?})",
                row.status
            );
        }
        rows.push(row);
    }
    Ok(rows)
}

// "status: <value>\n" 단일 라인 raw 검증. lxc와 동일 패턴, whitelist만 다름.
fn parse_qm_status(raw: &str) -> anyhow::Result<&str> {
    let body = raw.strip_suffix('\n').unwrap_or(raw);
    if body.contains('\n') {
        anyhow::bail!("qm status 출력이 단일 라인이 아님: {raw:?}");
    }
    let value = body.strip_prefix("status: ")
        .ok_or_else(|| anyhow::anyhow!("qm status 출력 형식이 'status: <value>' 아님: {raw:?}"))?;
    if !STATUS_KNOWN.contains(&value) {
        anyhow::bail!("qm status 값이 알 수 없는 형태: {value:?} (허용: {STATUS_KNOWN:?})");
    }
    Ok(value)
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
    /// VM 초기 설정 (SSH 키 + qemu-guest-agent + timezone)
    Setup {
        vmid: String,
        #[arg(long)]
        ip: String,
        #[arg(long, default_value = "root")]
        user: String,
        #[arg(long)]
        password: Option<String>,
    },
    /// VM 한국어 환경 (로케일 + 폰트 + ibus-hangul)
    Korean {
        vmid: String,
        #[arg(long)]
        ip: String,
        #[arg(long, default_value = "root")]
        user: String,
    },
    /// VM 클립보드 연동 (spice-vdagent + xclip)
    Clipboard {
        vmid: String,
        #[arg(long)]
        ip: String,
        #[arg(long, default_value = "root")]
        user: String,
        /// SPICE 모드 전환
        #[arg(long)]
        spice: bool,
    },
    /// VM 콘솔에 키 입력 전송 (qm sendkey)
    Type {
        vmid: String,
        /// 전송할 텍스트
        #[arg(long)]
        text: String,
        /// 마지막에 Enter 전송
        #[arg(long)]
        enter: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let json = cli.json;
    if !matches!(cli.cmd, Cmd::Doctor) && !common::has_cmd("qm") {
        anyhow::bail!("qm 없음 — Proxmox 호스트에서만 동작");
    }
    match cli.cmd {
        Cmd::List => list(json),
        Cmd::Status { vmid } => status(&vmid, json),
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
        Cmd::Setup { vmid, ip, user, password } => vm_setup(&vmid, &ip, &user, password.as_deref()),
        Cmd::Korean { vmid, ip, user } => vm_korean(&vmid, &ip, &user),
        Cmd::Clipboard { vmid, ip, user, spice } => vm_clipboard(&vmid, &ip, &user, spice),
        Cmd::Type { vmid, text, enter } => { vm_type(&vmid, &text, enter); Ok(()) }
    }
}

fn list(json: bool) -> anyhow::Result<()> {
    let out = common::run("qm", &["list"])?;
    if !json {
        println!("{out}");
        return Ok(());
    }
    let rows = parse_qm_list(&out)?;
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}

fn status(vmid: &str, json: bool) -> anyhow::Result<()> {
    if !json {
        let out = common::run("qm", &["status", vmid])?;
        println!("{out}");
        return Ok(());
    }
    // raw stdout — common::run의 trim 회피.
    let output = std::process::Command::new("qm").args(["status", vmid]).output()?;
    if !output.status.success() {
        anyhow::bail!("qm status {vmid} 실패: {}", String::from_utf8_lossy(&output.stderr));
    }
    let raw = String::from_utf8(output.stdout)?;
    let value = parse_qm_status(&raw)?;
    let payload = serde_json::json!({ "vmid": vmid, "status": value });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

fn doctor() {
    println!("=== prelik-vm doctor ===");
    for (name, cmd) in &[("qm", "qm"), ("vzdump", "vzdump"), ("pvesh", "pvesh")] {
        println!("  {} {name}", if common::has_cmd(cmd) { "✓" } else { "✗" });
    }
}

// ---------- VM SSH helpers ----------

use std::process::Command;

fn cmd_output(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd).args(args).output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn vm_exec(ip: &str, user: &str, cmd: &[&str]) -> (bool, String) {
    let target = format!("{user}@{ip}");
    let shell_cmd = cmd.join(" ");
    let output = Command::new("ssh")
        .args(["-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=5", "-o", "BatchMode=yes", &target, &shell_cmd])
        .output();
    match output {
        Ok(o) => {
            let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
            let combined = if err.is_empty() { out } else { format!("{out}\n{err}") };
            (o.status.success(), combined)
        }
        Err(e) => (false, format!("ssh 실행 실패: {e}")),
    }
}

fn vm_ssh_test(ip: &str, user: &str) -> bool {
    let (ok, _) = vm_exec(ip, user, &["echo", "ok"]);
    ok
}

fn ensure_vm_running(vmid: &str) -> anyhow::Result<()> {
    let status = cmd_output("qm", &["status", vmid]);
    if status.is_empty() { anyhow::bail!("VMID {vmid}를 찾을 수 없습니다"); }
    if !status.contains("running") { anyhow::bail!("VM {vmid}이 실행 중이 아닙니다 (현재: {status})"); }
    Ok(())
}

// ---------- vm-setup ----------

fn vm_setup(vmid: &str, ip: &str, user: &str, password: Option<&str>) -> anyhow::Result<()> {
    println!("=== VM {vmid} 초기 설정 ({ip}, {user}) ===\n");
    ensure_vm_running(vmid)?;

    // SSH 키 확인
    let pubkey_path = "/root/.ssh/id_ed25519.pub";
    let pubkey_rsa = "/root/.ssh/id_rsa.pub";
    let pubkey = if std::path::Path::new(pubkey_path).exists() {
        pubkey_path
    } else if std::path::Path::new(pubkey_rsa).exists() {
        pubkey_rsa
    } else {
        println!("[vm-setup] SSH 키가 없습니다. 생성 중...");
        let output = Command::new("ssh-keygen").args(["-t", "ed25519", "-f", "/root/.ssh/id_ed25519", "-N", ""]).output()?;
        if !output.status.success() { anyhow::bail!("SSH 키 생성 실패"); }
        println!("[vm-setup] SSH 키 생성 완료");
        pubkey_path
    };

    if vm_ssh_test(ip, user) {
        println!("[vm-setup] SSH 키 접속 가능 ✓");
    } else {
        if let Some(pw) = password {
            println!("[vm-setup] sshpass로 키 배포 중...");
            let status = Command::new("sshpass")
                .args(["-p", pw, "ssh-copy-id", "-o", "StrictHostKeyChecking=no", "-i", pubkey, &format!("{user}@{ip}")])
                .status()?;
            if !status.success() { anyhow::bail!("SSH 키 배포 실패"); }
        } else {
            println!("[vm-setup] ssh-copy-id 실행 (비밀번호 입력 필요)");
            let status = Command::new("ssh-copy-id")
                .args(["-o", "StrictHostKeyChecking=no", "-i", pubkey, &format!("{user}@{ip}")])
                .status()?;
            if !status.success() { anyhow::bail!("ssh-copy-id 실패"); }
        }
        if !vm_ssh_test(ip, user) { anyhow::bail!("SSH 키 접속 여전히 실패"); }
        println!("[vm-setup] SSH 키 접속 확인 ✓");
    }

    // QEMU guest agent
    println!("\n[vm-setup] QEMU guest agent 설치 중...");
    let (installed, _) = vm_exec(ip, user, &["dpkg", "-s", "qemu-guest-agent"]);
    if installed {
        println!("[vm-setup] qemu-guest-agent 이미 설치됨");
    } else {
        let (ok, out) = vm_exec(ip, user, &["bash", "-c",
            "sudo DEBIAN_FRONTEND=noninteractive apt-get update -qq && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq qemu-guest-agent"]);
        if ok { println!("[vm-setup] qemu-guest-agent 설치 완료"); }
        else { anyhow::bail!("qemu-guest-agent 설치 실패: {out}"); }
    }
    let _ = vm_exec(ip, user, &["sudo", "systemctl", "enable", "qemu-guest-agent"]);
    let _ = vm_exec(ip, user, &["sudo", "systemctl", "start", "qemu-guest-agent"]);

    // Proxmox agent 옵션
    let config = cmd_output("qm", &["config", vmid]);
    if !config.contains("agent:") || config.contains("agent: 0") {
        let _ = Command::new("qm").args(["set", vmid, "--agent", "enabled=1"]).output();
    }

    // guest agent ping
    let ga = Command::new("qm").args(["guest", "cmd", vmid, "ping"]).output();
    match ga {
        Ok(o) if o.status.success() => println!("[vm-setup] guest agent 응답 ✓"),
        _ => println!("[vm-setup] guest agent 아직 응답 없음 (재부팅 후 확인)"),
    }

    println!("\n=== VM {vmid} 설정 완료 ===");
    println!("  SSH: ssh {user}@{ip}");
    Ok(())
}

// ---------- vm-korean ----------

fn vm_korean(vmid: &str, ip: &str, user: &str) -> anyhow::Result<()> {
    println!("=== VM {vmid} 한국어 환경 설정 ({ip}) ===\n");
    ensure_vm_running(vmid)?;
    if !vm_ssh_test(ip, user) { anyhow::bail!("SSH 접속 실패: {user}@{ip}"); }

    let sudo = if user == "root" { "" } else { "sudo " };

    // 로케일
    println!("[vm-korean] 로케일 설정 중...");
    let (has_locale, _) = vm_exec(ip, user, &["bash", "-c", "locale -a 2>/dev/null | grep -q ko_KR.utf8"]);
    if !has_locale {
        let _ = vm_exec(ip, user, &[&format!("{sudo}sed -i 's/# ko_KR.UTF-8/ko_KR.UTF-8/' /etc/locale.gen && {sudo}locale-gen")]);
    }
    let (lang_ok, _) = vm_exec(ip, user, &["bash", "-c", "grep -q 'LANG=.*ko_KR' /etc/default/locale 2>/dev/null"]);
    if !lang_ok {
        let _ = vm_exec(ip, user, &[&format!("{sudo}bash -c \"echo 'LANG=ko_KR.UTF-8' > /etc/default/locale\"")]);
    }
    println!("[vm-korean] 로케일 설정 완료");

    // 폰트
    println!("[vm-korean] 한국어 폰트 설치 중...");
    let font_pkgs = ["fonts-nanum", "fonts-nanum-coding", "fonts-nanum-extra", "fonts-noto-cjk"];
    let mut need: Vec<&str> = Vec::new();
    for pkg in &font_pkgs {
        let (ok, _) = vm_exec(ip, user, &["dpkg", "-s", pkg]);
        if !ok { need.push(pkg); }
    }
    if !need.is_empty() {
        let pkg_list = need.join(" ");
        let _ = vm_exec(ip, user, &[&format!("{sudo}DEBIAN_FRONTEND=noninteractive apt-get update -qq && {sudo}DEBIAN_FRONTEND=noninteractive apt-get install -y -qq {pkg_list}")]);
    }
    println!("[vm-korean] 폰트 설치 완료");

    // ibus-hangul
    println!("[vm-korean] 한글 입력기 설치 중...");
    let ibus_pkgs = ["ibus", "ibus-hangul"];
    let mut need_ibus: Vec<&str> = Vec::new();
    for pkg in &ibus_pkgs {
        let (ok, _) = vm_exec(ip, user, &["dpkg", "-s", pkg]);
        if !ok { need_ibus.push(pkg); }
    }
    if !need_ibus.is_empty() {
        let pkg_list = need_ibus.join(" ");
        let _ = vm_exec(ip, user, &[&format!("{sudo}DEBIAN_FRONTEND=noninteractive apt-get install -y -qq {pkg_list}")]);
    }
    println!("[vm-korean] 입력기 설치 완료");

    // ibus 환경변수
    let (has_ibus_env, _) = vm_exec(ip, user, &["bash", "-c", "grep -q GTK_IM_MODULE=ibus /etc/environment 2>/dev/null"]);
    if !has_ibus_env {
        let ibus_env = "GTK_IM_MODULE=ibus\nQT_IM_MODULE=ibus\nXMODIFIERS=@im=ibus";
        let _ = vm_exec(ip, user, &[&format!("{sudo}bash -c \"cat >> /etc/environment << 'IEOF'\n{ibus_env}\nIEOF\"")]);
    }

    // timezone
    let (tz_ok, _) = vm_exec(ip, user, &["bash", "-c", "timedatectl show --value -p Timezone 2>/dev/null | grep -q Asia/Seoul"]);
    if !tz_ok {
        let _ = vm_exec(ip, user, &[&format!("{sudo}timedatectl set-timezone Asia/Seoul")]);
    }

    println!("\n=== VM {vmid} 한국어 설정 완료 ===");
    println!("  VM 재부팅 또는 로그아웃/로그인 후 적용");
    println!("  한/영 전환: Shift+Space 또는 한/영 키");
    Ok(())
}

// ---------- vm-clipboard ----------

fn vm_clipboard(vmid: &str, ip: &str, user: &str, spice: bool) -> anyhow::Result<()> {
    println!("=== VM {vmid} 클립보드 연동 설정 ({ip}) ===\n");
    ensure_vm_running(vmid)?;
    if !vm_ssh_test(ip, user) { anyhow::bail!("SSH 접속 실패: {user}@{ip}"); }

    let sudo = if user == "root" { "" } else { "sudo " };

    let packages = ["spice-vdagent", "xclip", "xsel"];
    let mut need: Vec<&str> = Vec::new();
    for pkg in &packages {
        let (ok, _) = vm_exec(ip, user, &["dpkg", "-s", pkg]);
        if !ok { need.push(pkg); }
    }
    if !need.is_empty() {
        let pkg_list = need.join(" ");
        let (ok, out) = vm_exec(ip, user, &[&format!(
            "{sudo}DEBIAN_FRONTEND=noninteractive apt-get update -qq && {sudo}DEBIAN_FRONTEND=noninteractive apt-get install -y -qq {pkg_list}")]);
        if !ok { eprintln!("[vm-clipboard] 패키지 설치 실패: {out}"); }
    }

    let _ = vm_exec(ip, user, &["bash", "-c", &format!(
        "{sudo}systemctl enable spice-vdagentd 2>/dev/null; {sudo}systemctl start spice-vdagentd 2>/dev/null")]);

    if spice {
        println!("[vm-clipboard] SPICE 모드 전환 중...");
        let config = cmd_output("qm", &["config", vmid]);
        if !config.lines().any(|l| l.starts_with("vga:") && l.contains("qxl")) {
            let _ = Command::new("qm").args(["set", vmid, "--vga", "qxl"]).output();
            println!("[vm-clipboard] VGA -> qxl 변경 (재부팅 필요)");
        }
        if !config.contains("spice") {
            let _ = Command::new("qm").args(["set", vmid, "--spice_enhancements", "foldersharing=1,videostreaming=all"]).output();
        }
    }

    // 결과 확인
    let (vda_ok, _) = vm_exec(ip, user, &["systemctl", "is-active", "spice-vdagentd"]);
    let (xclip_ok, _) = vm_exec(ip, user, &["which", "xclip"]);
    println!("  spice-vdagentd: {}", if vda_ok { "active ✓" } else { "inactive (재부팅 후)" });
    println!("  xclip:          {}", if xclip_ok { "✓" } else { "✗" });

    println!("\n=== VM {vmid} 클립보드 설정 완료 ===");
    Ok(())
}

// ---------- vm-type ----------

fn char_to_keyname(c: char) -> Option<String> {
    match c {
        'a'..='z' => Some(c.to_string()),
        'A'..='Z' => Some(format!("shift-{}", c.to_ascii_lowercase())),
        '0'..='9' => Some(c.to_string()),
        ' ' => Some("spc".into()), '-' => Some("minus".into()), '_' => Some("shift-minus".into()),
        '=' => Some("equal".into()), '+' => Some("shift-equal".into()),
        '.' => Some("dot".into()), ',' => Some("comma".into()),
        '/' => Some("slash".into()), '\\' => Some("backslash".into()),
        '\'' => Some("apostrophe".into()), '"' => Some("shift-apostrophe".into()),
        ';' => Some("semicolon".into()), ':' => Some("shift-semicolon".into()),
        '[' => Some("bracket_left".into()), ']' => Some("bracket_right".into()),
        '`' => Some("grave_accent".into()), '~' => Some("shift-grave_accent".into()),
        '!' => Some("shift-1".into()), '@' => Some("shift-2".into()),
        '#' => Some("shift-3".into()), '$' => Some("shift-4".into()),
        '%' => Some("shift-5".into()), '^' => Some("shift-6".into()),
        '&' => Some("shift-7".into()), '*' => Some("shift-8".into()),
        '(' => Some("shift-9".into()), ')' => Some("shift-0".into()),
        '\t' => Some("tab".into()), '\n' => Some("ret".into()),
        _ => None,
    }
}

fn vm_type(vmid: &str, text: &str, enter: bool) {
    println!("[vm-type] VM {vmid}에 입력 전송: {text}");
    for c in text.chars() {
        if let Some(key) = char_to_keyname(c) {
            let _ = Command::new("qm").args(["sendkey", vmid, &key]).output();
            std::thread::sleep(std::time::Duration::from_millis(30));
        } else {
            eprintln!("[vm-type] 변환 불가 문자 스킵: '{c}'");
        }
    }
    if enter {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let _ = Command::new("qm").args(["sendkey", vmid, "ret"]).output();
    }
    println!("[vm-type] 전송 완료");
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- parse_qm_list -----

    #[test]
    fn list_running_with_pid() {
        let text = "VMID NAME STATUS MEM(MB) BOOTDISK(GB) PID\n\
                    100 web running 2048 32 1234\n";
        let rows = parse_qm_list(text).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], VmRow {
            vmid: "100".into(), name: "web".into(), status: "running".into(),
            mem_mb: "2048".into(), disk_gb: "32".into(), pid: Some("1234".into()),
        });
    }

    #[test]
    fn list_stopped_pid_zero_normalized_to_none() {
        let text = "VMID NAME STATUS MEM DISK PID\n\
                    101 db stopped 4096 64 0\n";
        let rows = parse_qm_list(text).unwrap();
        assert_eq!(rows[0].pid, None);
    }

    #[test]
    fn list_5_columns_no_pid() {
        // qm이 PID 컬럼 자체를 생략하는 경우 (오래된 출력)
        let text = "VMID NAME STATUS MEM DISK\n\
                    102 stopped-vm stopped 1024 8\n";
        let rows = parse_qm_list(text).unwrap();
        assert_eq!(rows[0].pid, None);
        assert_eq!(rows[0].name, "stopped-vm");
    }

    #[test]
    fn list_skips_empty_lines() {
        let text = "VMID NAME STATUS MEM DISK PID\n\
                    100 a running 2048 32 1\n\
                    \n\
                    101 b stopped 4096 64 0\n";
        assert_eq!(parse_qm_list(text).unwrap().len(), 2);
    }

    #[test]
    fn list_fails_on_too_few_columns() {
        let text = "VMID NAME STATUS MEM\n\
                    100 a running 2048\n";
        assert!(parse_qm_list(text).is_err());
    }

    #[test]
    fn list_fails_on_too_many_columns() {
        let text = "VMID NAME STATUS MEM DISK PID EXTRA\n\
                    100 a running 2048 32 1234 x\n";
        assert!(parse_qm_list(text).is_err());
    }

    #[test]
    fn list_only_header_returns_empty() {
        assert!(parse_qm_list("VMID NAME STATUS MEM DISK PID").unwrap().is_empty());
    }

    #[test]
    fn list_fails_on_unknown_status() {
        // status whitelist 위반 — qm이 'unknown'을 emit하면 (LXC 전용 fallback)
        // VM 도메인에선 거부해야 함. status JSON 경로와 동일 계약.
        let text = "VMID NAME STATUS MEM DISK PID\n\
                    100 web unknown 2048 32 0\n";
        assert!(parse_qm_list(text).is_err());
    }

    #[test]
    fn list_fails_on_drifted_status() {
        let text = "VMID NAME STATUS MEM DISK PID\n\
                    100 web RUNNING 2048 32 0\n"; // 대문자
        assert!(parse_qm_list(text).is_err());
    }

    // ----- parse_qm_status -----

    #[test]
    fn status_running() {
        assert_eq!(parse_qm_status("status: running\n").unwrap(), "running");
    }

    #[test]
    fn status_paused() {
        assert_eq!(parse_qm_status("status: paused\n").unwrap(), "paused");
    }

    #[test]
    fn status_suspended() {
        assert_eq!(parse_qm_status("status: suspended\n").unwrap(), "suspended");
    }

    #[test]
    fn status_prelaunch() {
        assert_eq!(parse_qm_status("status: prelaunch\n").unwrap(), "prelaunch");
    }

    #[test]
    fn status_stopped_no_trailing_newline() {
        assert_eq!(parse_qm_status("status: stopped").unwrap(), "stopped");
    }

    #[test]
    fn status_rejects_extra_lines() {
        assert!(parse_qm_status("status: running\nwarning: x\n").is_err());
    }

    #[test]
    fn status_rejects_missing_prefix() {
        assert!(parse_qm_status("state: running\n").is_err());
        assert!(parse_qm_status(" status: running\n").is_err());
    }

    #[test]
    fn status_rejects_value_drift() {
        assert!(parse_qm_status("status: \n").is_err());
        assert!(parse_qm_status("status:  running\n").is_err());
        assert!(parse_qm_status("status: running \n").is_err());
        assert!(parse_qm_status("status: unknown\n").is_err()); // qm은 unknown 안 emit (LXC 전용)
    }
}
