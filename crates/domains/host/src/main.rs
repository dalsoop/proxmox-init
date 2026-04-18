//! pxi-host — 호스트 시스템 관리.
//! bootstrap / status / monitor / postfix-relay / ssh-keygen / smb / gh-auth / self-update.

use clap::{Parser, Subcommand};
use pxi_core::common;
use pxi_core::helpers::read_host_env;
use pxi_core::os;

use std::fs;
use std::path::Path;

#[derive(Parser)]
#[command(name = "pxi-host", about = "호스트 시스템 관리")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Proxmox 호스트 풀 부트스트랩 (패키지, Rust, Node.js, gh, lazygit, SSH 하드닝, SMB, NTP)
    Bootstrap,
    /// 호스트 기본 상태 (Proxmox 무료 설정, 시스템, 패키지, 툴체인, 보안)
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
    /// SMB 포트 오픈 (445, 139) — Samba 설치 + 방화벽
    SmbOpen,
    /// SMB 포트 닫기
    SmbClose,
    /// gh CLI 풀스코프 인증 갱신 (repo/admin:org/admin:repo_hook 등 18개 스코프)
    GhAuth,
    /// pxi CLI 자체 업데이트 (install.pxi.com 재실행)
    SelfUpdate {
        /// 특정 버전 핀 (예: v1.8.3). 미지정 시 latest.
        #[arg(long)]
        version: Option<String>,
        /// 같은 버전이어도 강제 재설치
        #[arg(long)]
        force: bool,
    },
    /// 호스트 Postfix를 Maddy(LXC)로 릴레이 설정 (SPF/DKIM 적용)
    PostfixRelay {
        /// Maddy LXC IP (기본: 10.0.50.122)
        #[arg(long, default_value = "10.0.50.122")]
        maddy_ip: String,
        /// Maddy SMTP 포트
        #[arg(long, default_value = "587")]
        port: String,
        /// SMTP 사용자 (기본: .env의 SMTP_USER)
        #[arg(long)]
        user: Option<String>,
        /// SMTP 비밀번호 (기본: .env의 SMTP_PASSWORD)
        #[arg(long)]
        password: Option<String>,
        /// 발신자 헤더 재작성 주소 (기본: SMTP_USER와 동일)
        #[arg(long)]
        rewrite_from: Option<String>,
    },
    /// 의존 도구 설치 여부 점검
    Doctor,
}

// ---------------------------------------------------------------------------
// 상수
// ---------------------------------------------------------------------------

const SYSTEM_PACKAGES: &[&str] = &[
    "git", "curl", "wget", "build-essential", "rsync", "tmux", "jq",
    "htop", "tree", "unzip", "fail2ban", "unattended-upgrades", "apt-listchanges",
];

const PVE_ENTERPRISE_LIST: &str = "/etc/apt/sources.list.d/pve-enterprise.list";
const PVE_NO_SUB_LIST: &str = "/etc/apt/sources.list.d/pve-no-subscription.list";
const CEPH_ENTERPRISE_LIST: &str = "/etc/apt/sources.list.d/ceph.list";
const PVE_SUB_NAG_JS: &str = "/usr/share/javascript/proxmox-widget-toolkit/proxmoxlib.js";

const SMB_PORTS: &[(u16, &str)] = &[(445, "SMB direct"), (139, "NetBIOS session")];

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Bootstrap => bootstrap(),
        Cmd::Status => { status(); Ok(()) }
        Cmd::Monitor => { monitor(); Ok(()) }
        Cmd::SshKeygen { label, email } => ssh_keygen(&label, email.as_deref()),
        Cmd::SmbOpen => smb_set(true),
        Cmd::SmbClose => smb_set(false),
        Cmd::GhAuth => gh_auth(),
        Cmd::SelfUpdate { version, force } => self_update(version.as_deref(), force),
        Cmd::PostfixRelay { maddy_ip, port, user, password, rewrite_from } => {
            postfix_relay(&maddy_ip, &port, user.as_deref(), password.as_deref(), rewrite_from.as_deref())
        }
        Cmd::Doctor => { doctor(); Ok(()) }
    }
}

// ---------------------------------------------------------------------------
// bootstrap (포트 from phs host bootstrap)
// ---------------------------------------------------------------------------

fn bootstrap() -> anyhow::Result<()> {
    println!("=== HOST 부트스트랩 ===\n");

    setup_apt_sources()?;
    disable_ceph_enterprise()?;
    remove_subscription_nag()?;
    apt_update()?;
    ensure_packages(SYSTEM_PACKAGES)?;
    setup_timezone()?;
    setup_ntp()?;
    setup_unattended_upgrades()?;
    install_rust()?;
    install_nodejs()?;
    install_gh()?;
    install_github_release("lazygit", "jesseduffield/lazygit")?;
    install_github_release("lazydocker", "jesseduffield/lazydocker")?;
    harden_ssh()?;

    // SMB 포트 오픈
    smb_set(true)?;

    println!("\n=== 부트스트랩 완료 ===");
    Ok(())
}

fn setup_apt_sources() -> anyhow::Result<()> {
    if Path::new(PVE_ENTERPRISE_LIST).exists() {
        let content = fs::read_to_string(PVE_ENTERPRISE_LIST).unwrap_or_default();
        if !content.lines().all(|l| l.trim().starts_with('#') || l.trim().is_empty()) {
            let commented: String = content.lines().map(|l| {
                if l.trim().is_empty() || l.trim().starts_with('#') { l.to_string() }
                else { format!("# {l}") }
            }).collect::<Vec<_>>().join("\n");
            fs::write(PVE_ENTERPRISE_LIST, &commented)?;
            println!("[apt] enterprise repo 비활성화 완료");
        } else {
            println!("[apt] enterprise repo 이미 비활성화됨");
        }
    }

    if !Path::new(PVE_NO_SUB_LIST).exists() {
        let codename = common::run_bash("grep VERSION_CODENAME /etc/os-release | cut -d= -f2")
            .unwrap_or_else(|_| "bookworm".to_string());
        let codename = codename.trim();
        let content = format!("deb http://download.proxmox.com/debian/pve {codename} pve-no-subscription\n");
        fs::write(PVE_NO_SUB_LIST, &content)?;
        println!("[apt] no-subscription repo 추가 완료");
    } else {
        println!("[apt] no-subscription repo 이미 존재");
    }
    Ok(())
}

fn disable_ceph_enterprise() -> anyhow::Result<()> {
    if !Path::new(CEPH_ENTERPRISE_LIST).exists() {
        println!("[apt] ceph enterprise repo 없음, 스킵");
        return Ok(());
    }
    let content = fs::read_to_string(CEPH_ENTERPRISE_LIST).unwrap_or_default();
    if content.lines().all(|l| l.trim().starts_with('#') || l.trim().is_empty()) {
        println!("[apt] ceph enterprise repo 이미 비활성화됨");
        return Ok(());
    }
    let commented: String = content.lines().map(|l| {
        if l.trim().is_empty() || l.trim().starts_with('#') { l.to_string() }
        else { format!("# {l}") }
    }).collect::<Vec<_>>().join("\n");
    fs::write(CEPH_ENTERPRISE_LIST, &commented)?;
    println!("[apt] ceph enterprise repo 비활성화 완료");
    Ok(())
}

fn remove_subscription_nag() -> anyhow::Result<()> {
    if !Path::new(PVE_SUB_NAG_JS).exists() {
        println!("[pve] proxmoxlib.js 없음, 스킵");
        return Ok(());
    }
    let content = fs::read_to_string(PVE_SUB_NAG_JS).unwrap_or_default();
    if content.contains("// PATCHED: subscription nag removed") {
        println!("[pve] 구독 팝업 이미 제거됨");
        return Ok(());
    }

    let needle = "res.data.status.toLowerCase() !== 'active'\n                    ) {\n                        Ext.Msg.show({\n                            title: gettext('No valid subscription'),";
    if !content.contains(needle) {
        println!("[pve] 구독 팝업 패치 대상을 찾지 못함 (PVE 버전 변경?)");
        return Ok(());
    }

    let old_block = "if (\n                        res === null ||\n                        res === undefined ||\n                        !res ||\n                        res.data.status.toLowerCase() !== 'active'\n                    ) {\n                        Ext.Msg.show({\n                            title: gettext('No valid subscription'),\n                            icon: Ext.Msg.WARNING,\n                            message: Proxmox.Utils.getNoSubKeyHtml(res.data.url),\n                            buttons: Ext.Msg.OK,\n                            callback: function (btn) {\n                                if (btn !== 'ok') {\n                                    return;\n                                }\n                                orig_cmd();\n                            },\n                        });\n                    } else {\n                        orig_cmd();\n                    }";
    let new_block = "// PATCHED: subscription nag removed\n                    orig_cmd();";
    let patched = content.replacen(old_block, new_block, 1);
    if patched == content {
        println!("[pve] 구독 팝업 패치 블록 매칭 실패");
        return Ok(());
    }

    fs::write(PVE_SUB_NAG_JS, &patched)?;
    if common::run("systemctl", &["restart", "pveproxy"]).is_ok() {
        println!("[pve] 구독 팝업 제거 완료 (pveproxy 재시작됨)");
    } else {
        println!("[pve] 구독 팝업 패치 완료 (pveproxy 재시작 실패, 수동 필요)");
    }
    Ok(())
}

fn apt_update() -> anyhow::Result<()> {
    println!("[apt] 패키지 목록 업데이트 중...");
    common::run_bash("DEBIAN_FRONTEND=noninteractive apt-get update -qq")?;
    println!("[apt] 업데이트 완료");
    Ok(())
}

fn ensure_packages(packages: &[&str]) -> anyhow::Result<()> {
    let missing: Vec<&&str> = packages.iter().filter(|p| !pkg_installed(p)).collect();
    if missing.is_empty() {
        println!("[apt] 모든 패키지 이미 설치됨");
        return Ok(());
    }
    let pkgs = missing.iter().map(|p| **p).collect::<Vec<_>>().join(" ");
    println!("[apt] 설치 중: {pkgs}");
    common::run_bash(&format!("DEBIAN_FRONTEND=noninteractive apt-get install -y -qq {pkgs}"))?;
    println!("[apt] 설치 완료");
    Ok(())
}

fn pkg_installed(pkg: &str) -> bool {
    common::run_bash(&format!("dpkg -s {pkg} 2>/dev/null | grep -q 'Status.*installed'")).is_ok()
}

fn setup_timezone() -> anyhow::Result<()> {
    let current = common::run("timedatectl", &["show", "--property=Timezone", "--value"]).unwrap_or_default();
    if current.trim() == "Asia/Seoul" {
        println!("[tz] timezone 이미 Asia/Seoul");
        return Ok(());
    }
    common::run("timedatectl", &["set-timezone", "Asia/Seoul"])?;
    println!("[tz] timezone Asia/Seoul 설정 완료");
    Ok(())
}

fn setup_ntp() -> anyhow::Result<()> {
    let ntp = common::run("timedatectl", &["show", "--property=NTP", "--value"]).unwrap_or_default();
    if ntp.trim() == "yes" {
        println!("[ntp] NTP 동기화 이미 활성화됨");
        return Ok(());
    }
    common::run("timedatectl", &["set-ntp", "true"])?;
    println!("[ntp] NTP 동기화 활성화 완료");
    Ok(())
}

fn setup_unattended_upgrades() -> anyhow::Result<()> {
    let conf_path = "/etc/apt/apt.conf.d/20auto-upgrades";
    if Path::new(conf_path).exists() {
        println!("[apt] unattended-upgrades 이미 설정됨");
        return Ok(());
    }
    let content = "APT::Periodic::Update-Package-Lists \"1\";\nAPT::Periodic::Unattended-Upgrade \"1\";\nAPT::Periodic::AutocleanInterval \"7\";\n";
    fs::write(conf_path, content)?;
    println!("[apt] unattended-upgrades 설정 완료");
    Ok(())
}

fn install_rust() -> anyhow::Result<()> {
    if common::has_cmd("rustc") {
        println!("[rust] 이미 설치됨");
        return Ok(());
    }
    println!("[rust] rustup 설치 중...");
    common::run_bash("curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y")?;
    println!("[rust] 설치 완료");
    Ok(())
}

fn install_nodejs() -> anyhow::Result<()> {
    if common::has_cmd("node") {
        println!("[node] 이미 설치됨");
        return Ok(());
    }
    println!("[node] Node.js 설치 중...");
    ensure_packages(&["nodejs", "npm"])?;
    Ok(())
}

fn install_gh() -> anyhow::Result<()> {
    if common::has_cmd("gh") {
        println!("[gh] 이미 설치됨");
        return Ok(());
    }
    println!("[gh] GitHub CLI 설치 중...");
    common::run_bash(
        "curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg | dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg"
    )?;
    common::run_bash(
        "echo \"deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main\" > /etc/apt/sources.list.d/github-cli.list"
    )?;
    apt_update()?;
    ensure_packages(&["gh"])?;
    Ok(())
}

fn install_github_release(name: &str, repo: &str) -> anyhow::Result<()> {
    if common::has_cmd(name) {
        println!("[{name}] 이미 설치됨");
        return Ok(());
    }
    println!("[{name}] 설치 중...");

    let arch_raw = common::run("uname", &["-m"])?;
    let arch = match arch_raw.trim() {
        "x86_64" => "x86_64",
        "aarch64" | "arm64" => "arm64",
        other => anyhow::bail!("[{name}] 지원하지 않는 아키텍처: {other}"),
    };

    let tag = pxi_core::github::latest_tag(repo)?;
    let version = tag.trim_start_matches('v');
    let tarball = format!("{name}_{version}_Linux_{arch}.tar.gz");
    let url = format!("https://github.com/{repo}/releases/download/{tag}/{tarball}");
    common::run_bash(&format!(
        "curl -sL '{url}' -o /tmp/{tarball} && tar -xzf /tmp/{tarball} -C /tmp {name} && mv /tmp/{name} /usr/local/bin/{name} && chmod +x /usr/local/bin/{name} && rm -f /tmp/{tarball}"
    ))?;
    println!("[{name}] 설치 완료 ({tag})");
    Ok(())
}

fn harden_ssh() -> anyhow::Result<()> {
    println!("[ssh] SSH 하드닝 적용 중...");
    let config_path = "/etc/ssh/sshd_config";
    let mut config = fs::read_to_string(config_path)?;
    let mut changed = false;

    let rules = [
        ("PermitEmptyPasswords", "no"),
        ("PasswordAuthentication", "no"),
        ("PermitRootLogin", "prohibit-password"),
    ];

    for (key, value) in rules {
        let target = format!("{key} {value}");
        let already_set = config.lines().any(|l| {
            let t = l.trim();
            !t.starts_with('#') && t.starts_with(key) && t.contains(value)
        });
        if already_set {
            println!("[ssh] {target} - 이미 설정됨");
            continue;
        }

        let lines: Vec<String> = config.lines().map(|l| l.to_string()).collect();
        let active = lines.iter().position(|l| {
            let t = l.trim();
            !t.starts_with('#') && t.starts_with(key)
        });
        let commented = lines.iter().position(|l| {
            let t = l.trim();
            t.starts_with('#') && t.contains(key)
        });

        if let Some(idx) = active {
            let mut new_lines = lines.clone();
            new_lines[idx] = target.clone();
            config = new_lines.join("\n");
            if !config.ends_with('\n') { config.push('\n'); }
        } else if let Some(idx) = commented {
            let mut new_lines = lines.clone();
            new_lines.insert(idx + 1, target.clone());
            config = new_lines.join("\n");
            if !config.ends_with('\n') { config.push('\n'); }
        } else {
            config.push_str(&format!("{target}\n"));
        }

        println!("[ssh] {target} - 적용 완료");
        changed = true;
    }

    if changed {
        fs::write(config_path, &config)?;
        if common::run("sshd", &["-t"]).is_ok() {
            common::run("systemctl", &["reload", "sshd"])?;
            println!("[ssh] sshd reload 완료");
        } else {
            eprintln!("[ssh] sshd 설정 검증 실패! 수동 확인 필요");
        }
    } else {
        println!("[ssh] 변경 사항 없음");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// status (포트 from phs host status)
// ---------------------------------------------------------------------------

fn status() {
    println!("=== HOST 상태 ===\n");

    println!("[Proxmox 무료 설정]");
    let enterprise_disabled = if Path::new(PVE_ENTERPRISE_LIST).exists() {
        fs::read_to_string(PVE_ENTERPRISE_LIST).unwrap_or_default()
            .lines().all(|l| l.trim().starts_with('#') || l.trim().is_empty())
    } else { true };
    let ceph_disabled = if Path::new(CEPH_ENTERPRISE_LIST).exists() {
        fs::read_to_string(CEPH_ENTERPRISE_LIST).unwrap_or_default()
            .lines().all(|l| l.trim().starts_with('#') || l.trim().is_empty())
    } else { true };
    let no_sub_ok = Path::new(PVE_NO_SUB_LIST).exists();
    let nag_removed = if Path::new(PVE_SUB_NAG_JS).exists() {
        fs::read_to_string(PVE_SUB_NAG_JS).unwrap_or_default()
            .contains("// PATCHED: subscription nag removed")
    } else { false };
    println!("  enterprise repo 비활성화: {}", check(enterprise_disabled));
    println!("  ceph enterprise 비활성화: {}", check(ceph_disabled));
    println!("  no-subscription repo: {}", check(no_sub_ok));
    println!("  구독 팝업 제거: {}", check(nag_removed));

    println!("\n[시스템]");
    print_kv("timezone", "timedatectl show --property=Timezone --value");
    let ntp = common::run_bash("timedatectl show --property=NTP --value").unwrap_or_default();
    println!("  NTP 동기화: {}", check(ntp.trim() == "yes"));
    let ua_ok = Path::new("/etc/apt/apt.conf.d/20auto-upgrades").exists();
    println!("  unattended-upgrades: {}", check(ua_ok));

    println!("\n[시스템 패키지]");
    for pkg in SYSTEM_PACKAGES {
        println!("  {} {pkg}", check(pkg_installed(pkg)));
    }

    println!("\n[툴체인]");
    for (name, cmd, args) in [
        ("Rust", "rustc", "--version"),
        ("Cargo", "cargo", "--version"),
        ("Node.js", "node", "--version"),
        ("npm", "npm", "--version"),
        ("gh", "gh", "--version"),
        ("lazygit", "lazygit", "--version"),
        ("lazydocker", "lazydocker", "--version"),
    ] {
        match common::run(cmd, &[args]) {
            Ok(v) => println!("  + {name} ({v})"),
            Err(_) => println!("  - {name}"),
        }
    }

    println!("\n[보안]");
    let f2b = common::run("systemctl", &["is-active", "fail2ban"])
        .map(|o| o.trim() == "active").unwrap_or(false);
    println!("  fail2ban: {}", if f2b { "+ active" } else { "- inactive" });
    let sshd = fs::read_to_string("/etc/ssh/sshd_config").unwrap_or_default();
    for (label, key, expected) in [
        ("PermitEmptyPasswords no", "PermitEmptyPasswords", "no"),
        ("PasswordAuthentication no", "PasswordAuthentication", "no"),
        ("PermitRootLogin prohibit-password", "PermitRootLogin", "prohibit-password"),
    ] {
        let ok = sshd.lines().any(|l| {
            let t = l.trim();
            !t.starts_with('#') && t.starts_with(key) && t.contains(expected)
        });
        println!("  SSH {label}: {}", check(ok));
    }
}

// ---------------------------------------------------------------------------
// monitor (포트 from phs host monitor)
// ---------------------------------------------------------------------------

fn monitor() {
    println!("=== 시스템 모니터링 ===\n");

    let loadavg = fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let parts: Vec<&str> = loadavg.split_whitespace().collect();
    let cpus = common::run("nproc", &[]).unwrap_or_else(|_| "?".to_string());
    println!("[CPU] ({cpus} cores)");
    if parts.len() >= 3 {
        println!("  load avg: {} {} {} (1/5/15min)", parts[0], parts[1], parts[2]);
    }

    println!("\n[메모리]");
    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mut mem_total: u64 = 0;
    let mut mem_avail: u64 = 0;
    let mut swap_total: u64 = 0;
    let mut swap_free: u64 = 0;
    for line in meminfo.lines() {
        if line.starts_with("MemTotal:") { mem_total = parse_kb(line); }
        else if line.starts_with("MemAvailable:") { mem_avail = parse_kb(line); }
        else if line.starts_with("SwapTotal:") { swap_total = parse_kb(line); }
        else if line.starts_with("SwapFree:") { swap_free = parse_kb(line); }
    }
    let mem_used = mem_total.saturating_sub(mem_avail);
    let mem_pct = if mem_total > 0 { mem_used * 100 / mem_total } else { 0 };
    println!("  RAM:  {}GB / {}GB ({}%)", mem_used / 1_048_576, mem_total / 1_048_576, mem_pct);
    if swap_total > 0 {
        let swap_used = swap_total.saturating_sub(swap_free);
        println!("  Swap: {}GB / {}GB ({}%)", swap_used / 1_048_576, swap_total / 1_048_576, swap_used * 100 / swap_total);
    }

    println!("\n[디스크]");
    if let Ok(df) = common::run("df", &["-h", "--type=ext4", "--type=btrfs", "--type=xfs", "--type=zfs"]) {
        for line in df.lines().skip(1) {
            let p: Vec<&str> = line.split_whitespace().collect();
            if p.len() >= 6 {
                println!("  {:<20} {:<8} {:<8} {:<6} {}", p[5], p[2], p[1], p[4], p[0]);
            }
        }
    }

    if let Ok(temps) = common::run_bash("cat /sys/class/thermal/thermal_zone*/temp 2>/dev/null") {
        if !temps.trim().is_empty() {
            println!("\n[온도]");
            for (i, temp) in temps.lines().enumerate() {
                if let Ok(t) = temp.trim().parse::<u64>() {
                    println!("  zone{i}: {}C", t / 1000);
                }
            }
        }
    }

    let uptime = common::run("uptime", &["-p"]).unwrap_or_default();
    let procs = fs::read_dir("/proc")
        .map(|d| d.filter(|e| {
            e.as_ref().ok().and_then(|e| e.file_name().to_str().map(|s| s.chars().all(|c| c.is_ascii_digit()))).unwrap_or(false)
        }).count()).unwrap_or(0);
    println!("\n[시스템]");
    println!("  업타임: {uptime}");
    println!("  프로세스: {procs}개");
}

fn parse_kb(line: &str) -> u64 {
    line.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// ssh_keygen
// ---------------------------------------------------------------------------

fn ssh_keygen(label: &str, email: Option<&str>) -> anyhow::Result<()> {
    if label.is_empty() || label.len() > 64 {
        anyhow::bail!("label 길이 1~64자 필요");
    }
    if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
        anyhow::bail!("label은 [A-Za-z0-9._-]만 허용: {label:?}");
    }
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("HOME 미설정"))?;
    let ssh_dir = home.join(".ssh");
    fs::create_dir_all(&ssh_dir)?;
    common::run("chmod", &["700", &ssh_dir.display().to_string()])?;

    let key_path = ssh_dir.join(format!("id_ed25519_{label}"));
    if key_path.exists() {
        anyhow::bail!("이미 존재: {}. --label 변경하거나 기존 삭제.", key_path.display());
    }

    let comment = email.unwrap_or(label);
    println!("=== SSH 키 생성: {} ===", key_path.display());
    common::run("ssh-keygen", &[
        "-t", "ed25519", "-f", &key_path.display().to_string(), "-N", "", "-C", comment,
    ])?;

    // authorized_keys에 등록
    let auth_keys_path = ssh_dir.join("authorized_keys");
    let pub_key = fs::read_to_string(format!("{}.pub", key_path.display()))?;
    let mut auth = fs::OpenOptions::new().create(true).append(true).open(&auth_keys_path)?;
    use std::io::Write;
    writeln!(auth, "{}", pub_key.trim())?;
    println!("  공개키 등록: {}", auth_keys_path.display());

    println!("\n+ 키 생성 완료");
    println!("  개인키: {} (0600)", key_path.display());
    println!("  공개키: {}.pub", key_path.display());
    println!("\n공개키:");
    println!("{}", pub_key.trim());
    Ok(())
}

// ---------------------------------------------------------------------------
// smb_set (포트 from phs host smb-open + iptables)
// ---------------------------------------------------------------------------

fn smb_set(open: bool) -> anyhow::Result<()> {
    let action = if open { "오픈" } else { "닫기" };
    println!("=== SMB 포트 {action} (139, 445) ===");
    if !common::has_cmd("iptables") {
        anyhow::bail!("iptables 없음 -- 설치 후 재시도");
    }

    // Samba 설치 (open 시)
    if open {
        if !pkg_installed("samba") {
            ensure_packages(&["samba"])?;
        }
        // smbd 활성화
        let enabled = common::run("systemctl", &["is-enabled", "smbd"]).unwrap_or_default();
        if enabled.trim() != "enabled" {
            common::run("systemctl", &["enable", "--now", "smbd"])?;
            println!("  + smbd 서비스 활성화 완료");
        }
    }

    for (port, desc) in SMB_PORTS {
        let rule = format!("INPUT -p tcp --dport {port} -j ACCEPT -m comment --comment 'pxi-smb {desc}'");
        if open {
            let check = format!("iptables -C {rule} 2>/dev/null");
            if common::run_bash(&check).is_err() {
                common::run_bash(&format!("iptables -I {rule}"))?;
                println!("  + 포트 {port} ({desc}) 오픈");
            } else {
                println!("  = 포트 {port} ({desc}) 이미 열려 있음");
            }
        } else {
            let del = format!("iptables -D {rule} 2>/dev/null");
            let mut removed = 0;
            while common::run_bash(&del).is_ok() {
                removed += 1;
                if removed > 10 { break; }
            }
            if removed > 0 {
                println!("  + 포트 {port} {removed}개 규칙 제거");
            } else {
                println!("  = 포트 {port} 규칙 없음");
            }
        }
    }

    // Proxmox 방화벽 처리
    if os::is_proxmox() {
        let pve_fw_active = common::run_bash("pve-firewall status 2>/dev/null")
            .map(|o| o.contains("running")).unwrap_or(false);
        if pve_fw_active {
            open_pve_firewall(open)?;
        }
    }

    // iptables-persistent 저장
    if Path::new("/etc/iptables/rules.v4").exists() {
        common::run_bash("iptables-save > /etc/iptables/rules.v4")?;
        println!("  iptables rules.v4 저장");
    } else if open {
        // 설치 시도
        if common::run_bash("DEBIAN_FRONTEND=noninteractive apt-get install -y -qq iptables-persistent").is_ok() {
            let _ = common::run("netfilter-persistent", &["save"]);
            println!("  iptables-persistent 설치 + 규칙 저장 완료");
        } else {
            eprintln!("  ! iptables-persistent 미설치 -- 재부팅 시 규칙 사라짐");
        }
    }
    Ok(())
}

fn open_pve_firewall(open: bool) -> anyhow::Result<()> {
    let nodename = common::run("hostname", &[]).unwrap_or_else(|_| "pve".to_string());
    let nodename = nodename.trim();
    let fw_path = format!("/etc/pve/nodes/{nodename}/host.fw");

    let mut content = if Path::new(&fw_path).exists() {
        fs::read_to_string(&fw_path).unwrap_or_default()
    } else {
        String::new()
    };

    if !content.contains("[RULES]") {
        content.push_str("\n[RULES]\n");
    }

    let mut changed = false;
    for (port, desc) in SMB_PORTS {
        let rule = format!("IN ACCEPT -p tcp -dport {port} # {desc}");
        if open {
            if !content.contains(&format!("-dport {port}")) {
                let rules_pos = content.find("[RULES]").unwrap() + "[RULES]".len();
                content.insert_str(rules_pos, &format!("\n{rule}"));
                println!("  + pve-firewall: 포트 {port} ({desc}) 규칙 추가");
                changed = true;
            }
        }
        // close는 iptables로 충분, pve 규칙은 수동 관리
    }

    if changed {
        fs::write(&fw_path, &content)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// gh_auth
// ---------------------------------------------------------------------------

fn gh_auth() -> anyhow::Result<()> {
    const SCOPES: &[&str] = &[
        "repo", "admin:org", "admin:public_key", "admin:repo_hook",
        "admin:org_hook", "gist", "notifications", "user", "delete_repo",
        "write:packages", "read:packages", "admin:gpg_key", "codespace",
        "project", "admin:ssh_signing_key", "audit_log", "copilot", "workflow",
    ];
    println!("=== gh CLI 풀스코프 인증 ===");
    if !common::has_cmd("gh") {
        anyhow::bail!("gh CLI 없음 -- pxi run host bootstrap 먼저");
    }
    let scopes = SCOPES.join(",");
    println!("요청 스코프: {scopes}\n");
    let status = std::process::Command::new("gh")
        .args(["auth", "refresh", "-h", "github.com", "-s", &scopes])
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()?;
    if !status.success() {
        anyhow::bail!("gh auth refresh 실패 (exit {})", status.code().unwrap_or(-1));
    }
    println!("\n+ 인증 완료");
    Ok(())
}

// ---------------------------------------------------------------------------
// self_update
// ---------------------------------------------------------------------------

fn self_update(version: Option<&str>, force: bool) -> anyhow::Result<()> {
    println!("=== pxi self-update ===");
    if !common::has_cmd("curl") { anyhow::bail!("curl 없음"); }
    if !common::has_cmd("bash") { anyhow::bail!("bash 없음"); }
    let mut cmd = std::process::Command::new("bash");
    cmd.arg("-c").arg("set -eo pipefail; curl -fsSL https://install.pxi.com | bash");
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

// ---------------------------------------------------------------------------
// postfix_relay (포트 from phs host postfix-relay)
// ---------------------------------------------------------------------------

fn postfix_relay(
    maddy_ip: &str,
    port: &str,
    user: Option<&str>,
    password: Option<&str>,
    rewrite_from: Option<&str>,
) -> anyhow::Result<()> {
    println!("=== 호스트 Postfix -> Maddy relay 설정 ===\n");

    let smtp_user = user.map(String::from).unwrap_or_else(|| read_host_env("SMTP_USER"));
    let smtp_pass = password.map(String::from).unwrap_or_else(|| read_host_env("SMTP_PASSWORD"));
    if smtp_user.is_empty() || smtp_pass.is_empty() {
        anyhow::bail!("SMTP_USER/SMTP_PASSWORD 미설정. --user / --password 또는 .env 필요");
    }
    let from_addr = rewrite_from.map(String::from).unwrap_or_else(|| smtp_user.clone());

    println!("[postfix] Maddy: {maddy_ip}:{port}");
    println!("[postfix] 인증: {smtp_user}");
    println!("[postfix] From 재작성: -> {from_addr}");

    // 1. Postfix 설치 확인
    if !Path::new("/etc/postfix/main.cf").exists() {
        anyhow::bail!("Postfix가 설치되어 있지 않습니다. apt install postfix 먼저 실행");
    }

    // 2. SASL 모듈 설치
    if !pkg_installed("libsasl2-modules") {
        println!("[postfix] libsasl2-modules 설치 중...");
        ensure_packages(&["libsasl2-modules"])?;
    }

    // 3. 기존 relayhost 라인 제거 (중복 방지)
    for pattern in ["relayhost", "smtp_sasl_", "smtp_tls_security_level", "sender_canonical_maps"] {
        let _ = common::run_bash(&format!("sed -i '/^{pattern}/d' /etc/postfix/main.cf"));
    }

    // 4. main.cf에 relay 블록 추가
    let main_cf_append = format!(
        "\n# pxi host postfix-relay: Maddy SASL relay\nrelayhost = [{maddy_ip}]:{port}\nsmtp_sasl_auth_enable = yes\nsmtp_sasl_password_maps = hash:/etc/postfix/sasl_passwd\nsmtp_sasl_security_options = noanonymous\nsmtp_tls_security_level = may\nsmtp_sasl_tls_security_options = noanonymous\nsender_canonical_maps = regexp:/etc/postfix/sender_canonical\n"
    );
    let main_cf = fs::read_to_string("/etc/postfix/main.cf").unwrap_or_default();
    fs::write("/etc/postfix/main.cf", main_cf + &main_cf_append)?;
    println!("[postfix] /etc/postfix/main.cf 업데이트");

    // 5. SASL 패스워드 파일
    let sasl_content = format!("[{maddy_ip}]:{port} {smtp_user}:{smtp_pass}\n");
    fs::write("/etc/postfix/sasl_passwd", sasl_content)?;
    common::run("chmod", &["600", "/etc/postfix/sasl_passwd"])?;
    common::run("postmap", &["/etc/postfix/sasl_passwd"])?;
    println!("[postfix] /etc/postfix/sasl_passwd 생성 (0600)");

    // 6. sender_canonical (발신자 재작성)
    let canonical = format!("/.+/    {from_addr}\n");
    fs::write("/etc/postfix/sender_canonical", canonical)?;
    println!("[postfix] /etc/postfix/sender_canonical 생성");

    // 7. reload + flush
    common::run("systemctl", &["reload", "postfix"])?;
    let _ = common::run("postfix", &["flush"]);
    println!("[postfix] reload + flush 완료");

    // 8. 검증
    if let Ok(queue) = common::run("mailq", &[]) {
        println!("\n=== 큐 상태 ===");
        for line in queue.lines().take(10) {
            println!("  {line}");
        }
    }

    println!("\n[postfix] 설정 완료. 테스트:");
    println!("  echo 'test' | mail -s 'postfix-relay-test' your@email.com");
    Ok(())
}

// ---------------------------------------------------------------------------
// doctor
// ---------------------------------------------------------------------------

fn doctor() {
    println!("=== pxi-host doctor ===");
    for (name, cmd) in &[
        ("iptables", "iptables"),
        ("ssh-keygen", "ssh-keygen"),
        ("uname", "uname"),
        ("df", "df"),
        ("free", "free"),
        ("top", "top"),
        ("sensors", "sensors"),
        ("postfix", "postfix"),
        ("postmap", "postmap"),
        ("timedatectl", "timedatectl"),
        ("sshd", "sshd"),
    ] {
        let ok = common::has_cmd(cmd);
        println!("  {} {name}", if ok { "+" } else { "-" });
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn check(ok: bool) -> &'static str {
    if ok { "+" } else { "-" }
}

fn print_kv(label: &str, cmd: &str) {
    if let Ok(out) = common::run_bash(cmd) {
        let val = out.trim();
        if val.is_empty() {
            println!("  {label}: -");
        } else {
            println!("  {label}: + {val}");
        }
    }
}
