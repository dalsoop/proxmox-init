//! pxi-vaultwarden — 이미 배포된 Vaultwarden(LXC 50118) reconcile 도구.
//!
//! 설치 자체 (LXC 생성 + 바이너리 빌드) 는 과거 수동/phs 로 이뤄져
//! 여기선 "이미 있는 인스턴스의 설정을 올바른 상태로 맞춘다" 를 주 책임으로 둔다.

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use pxi_core::common;
use std::process::Command;

const DEFAULT_VMID: u32 = 50118;
const DEFAULT_DOMAIN: &str = "https://vaultwarden.50.internal.kr";
const MAILGUN_SHIM_HOST: &str = "10.0.50.122";
const MAILGUN_SHIM_PORT: u16 = 2526;
const MAILGUN_FROM: &str = "devops@ranode.net";

#[derive(Parser)]
#[command(name = "pxi-vaultwarden", about = "Vaultwarden (self-hosted Bitwarden) reconcile")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// systemctl status vaultwarden
    Status { #[arg(long, default_value_t = DEFAULT_VMID)] vmid: u32 },
    /// journalctl 로그
    Logs {
        #[arg(long, default_value_t = DEFAULT_VMID)] vmid: u32,
        #[arg(long)] follow: bool,
        #[arg(long, default_value_t = 50)] tail: u32,
    },
    /// Vaultwarden 재시작
    Restart { #[arg(long, default_value_t = DEFAULT_VMID)] vmid: u32 },
    /// 설정 점검 (DOMAIN / SMTP / TLS / backup)
    Doctor { #[arg(long, default_value_t = DEFAULT_VMID)] vmid: u32 },
    /// invite/verify 메일 링크 생성용 DOMAIN 설정 (config.json 반영)
    DomainSet {
        #[arg(long, default_value_t = DEFAULT_VMID)] vmid: u32,
        #[arg(long, default_value = DEFAULT_DOMAIN)] url: String,
    },
    /// SMTP 를 mailgun-smtp-proxy(2526) 경유로 설정 — 같은 CF zone 수신 가능
    SmtpMailgun {
        #[arg(long, default_value_t = DEFAULT_VMID)] vmid: u32,
        #[arg(long, default_value = MAILGUN_SHIM_HOST)] host: String,
        #[arg(long, default_value_t = MAILGUN_SHIM_PORT)] port: u16,
        #[arg(long, default_value = MAILGUN_FROM)] from: String,
    },
    /// Bitwarden CLI 설치 — 2026.x 는 Vaultwarden 1.34 와 호환 이슈라
    /// 기본값으로 2024.7.2 를 깐다. `--rbw` 면 rust 구현체.
    BwInstall {
        #[arg(long)] rbw: bool,
        #[arg(long, default_value = "2024.7.2")] version: String,
    },
    /// 일일 sqlite 백업 systemd timer 상태 확인
    Backup { #[arg(long, default_value_t = DEFAULT_VMID)] vmid: u32 },
}

fn pct(vmid: u32, args: &[&str]) -> Result<String> {
    let id = vmid.to_string();
    let mut full: Vec<&str> = vec!["exec", &id, "--"];
    full.extend_from_slice(args);
    let out = Command::new("pct").args(&full).output()
        .with_context(|| format!("pct exec {vmid} failed"))?;
    if !out.status.success() {
        return Err(anyhow!("pct {:?} -> {}", args, String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Status { vmid } => {
            let _ = common::run("pct", &["exec", &vmid.to_string(), "--", "systemctl", "status", "vaultwarden", "--no-pager"]);
            Ok(())
        }
        Cmd::Logs { vmid, follow, tail } => {
            let mut args: Vec<String> = vec![
                "exec".into(), vmid.to_string(), "--".into(),
                "journalctl".into(), "-u".into(), "vaultwarden".into(),
                "-n".into(), tail.to_string(), "--no-pager".into(),
            ];
            if follow { args.push("-f".into()); }
            let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let _ = common::run("pct", &refs);
            Ok(())
        }
        Cmd::Restart { vmid } => {
            let _ = common::run("pct", &["exec", &vmid.to_string(), "--", "systemctl", "restart", "vaultwarden"]);
            Ok(())
        }
        Cmd::Doctor { vmid } => doctor(vmid),
        Cmd::DomainSet { vmid, url } => domain_set(vmid, &url),
        Cmd::SmtpMailgun { vmid, host, port, from } => smtp_mailgun(vmid, &host, port, &from),
        Cmd::BwInstall { rbw, version } => bw_install(rbw, &version),
        Cmd::Backup { vmid } => backup_status(vmid),
    }
}

fn doctor(vmid: u32) -> Result<()> {
    println!("=== Vaultwarden doctor (LXC {vmid}) ===");
    // 서비스
    let active = pct(vmid, &["systemctl", "is-active", "vaultwarden"]).unwrap_or_default();
    println!("  service: {}", active.trim());

    // config.json 핵심 값
    let domain = pct(vmid, &["sh", "-c", "grep -E '\"domain\"' /opt/vaultwarden/data/config.json || true"])
        .unwrap_or_default();
    println!("  domain:  {}", domain.trim());

    let smtp = pct(vmid, &["sh", "-c",
        r#"grep -E '"smtp_(host|port|from)"' /opt/vaultwarden/data/config.json | tr -d ' ,' | tr '\n' ' '"#
    ]).unwrap_or_default();
    println!("  smtp:    {}", smtp.trim());

    // .env 의 ROCKET_TLS 상태
    let tls = pct(vmid, &["sh", "-c",
        r#"grep -E '^ROCKET_TLS|^#\s*ROCKET_TLS' /opt/vaultwarden/.env || echo 'not set'"#
    ]).unwrap_or_default();
    println!("  tls:     {} (Traefik 경유면 off 가 정상)", tls.trim());

    // backup timer
    let timer = pct(vmid, &["sh", "-c",
        r#"systemctl is-active vaultwarden-backup.timer 2>/dev/null || echo missing"#
    ]).unwrap_or_default();
    println!("  backup:  {}", timer.trim());

    Ok(())
}

fn domain_set(vmid: u32, url: &str) -> Result<()> {
    println!("setting domain → {url}");
    let script = format!(
        r#"python3 - <<'PY'
import json
p = "/opt/vaultwarden/data/config.json"
c = json.load(open(p))
c["domain"] = "{url}"
json.dump(c, open(p, "w"), indent=2)
PY
chown vaultwarden:vaultwarden /opt/vaultwarden/data/config.json
systemctl restart vaultwarden
echo ok
"#);
    let out = pct(vmid, &["sh", "-c", &script])?;
    println!("{}", out.trim());
    Ok(())
}

fn smtp_mailgun(vmid: u32, host: &str, port: u16, from: &str) -> Result<()> {
    println!("setting smtp → {host}:{port} (mailgun-smtp-proxy) from={from}");
    let script = format!(
        r#"python3 - <<'PY'
import json
p = "/opt/vaultwarden/data/config.json"
c = json.load(open(p))
c["smtp_host"] = "{host}"
c["smtp_port"] = {port}
c["smtp_security"] = "off"
c["smtp_from"] = "{from}"
c["smtp_from_name"] = "Vaultwarden"
c["smtp_username"] = None
c["smtp_password"] = None
c["smtp_auth_mechanism"] = None
c["smtp_accept_invalid_certs"] = True
c["smtp_accept_invalid_hostnames"] = True
json.dump(c, open(p, "w"), indent=2)
PY
# .env 에 auth 변수가 남아 있으면 warning 뜨므로 주석 처리
sed -i 's|^SMTP_USERNAME=|# SMTP_USERNAME=|' /opt/vaultwarden/.env
sed -i 's|^SMTP_PASSWORD=|# SMTP_PASSWORD=|' /opt/vaultwarden/.env
sed -i 's|^SMTP_AUTH_MECHANISM=|# SMTP_AUTH_MECHANISM=|' /opt/vaultwarden/.env
chown vaultwarden:vaultwarden /opt/vaultwarden/data/config.json
systemctl restart vaultwarden
echo ok
"#);
    let out = pct(vmid, &["sh", "-c", &script])?;
    println!("{}", out.trim());
    Ok(())
}

fn bw_install(rbw: bool, version: &str) -> Result<()> {
    if rbw {
        println!("installing rbw (Rust bitwarden CLI) via cargo");
        let _ = common::run("cargo", &["install", "rbw", "--locked"]);
    } else {
        println!("installing @bitwarden/cli@{version} (최신 2026.x 은 Vaultwarden 1.34 와 호환 이슈)");
        let pkg = format!("@bitwarden/cli@{version}");
        let _ = common::run("npm", &["install", "-g", &pkg]);
    }
    Ok(())
}

fn backup_status(vmid: u32) -> Result<()> {
    println!("=== backup ===");
    let timer = pct(vmid, &["systemctl", "list-timers", "vaultwarden-backup.timer", "--all", "--no-pager"])
        .unwrap_or_default();
    println!("{}", timer.trim());
    let files = pct(vmid, &["sh", "-c",
        "ls -lah /opt/vaultwarden/data/db.sqlite3.backup.* 2>/dev/null | tail -3 || echo '(no backup files yet)'"
    ]).unwrap_or_default();
    println!("--- recent backup files ---\n{}", files.trim());
    Ok(())
}
