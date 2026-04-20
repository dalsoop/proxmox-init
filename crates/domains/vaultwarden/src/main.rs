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
    /// /etc/systemd/system/vaultwarden.service 를 표준 템플릿으로 install.
    /// 바이너리(/opt/vaultwarden/bin/vaultwarden)·유저·data 디렉토리는 전제.
    InstallSystemd { #[arg(long, default_value_t = DEFAULT_VMID)] vmid: u32 },
    /// vaultwarden-backup.service + .timer (daily) + /opt/vaultwarden/backup.sh 설치.
    InstallBackupTimer { #[arg(long, default_value_t = DEFAULT_VMID)] vmid: u32 },
    /// 기본 /opt/vaultwarden/.env 를 생성 (DOMAIN, ROCKET 세팅, mailgun-shim SMTP).
    /// ADMIN_TOKEN 은 `--admin-token` 또는 control-plane/.env 의 VAULTWARDEN_ADMIN_TOKEN.
    InstallEnv {
        #[arg(long, default_value_t = DEFAULT_VMID)] vmid: u32,
        #[arg(long, default_value = DEFAULT_DOMAIN)] url: String,
        #[arg(long)] admin_token: Option<String>,
    },
    /// LXC 내부에서 Vaultwarden 소스 빌드 → /opt/vaultwarden/bin/vaultwarden 배치.
    /// rustup + apt deps + git clone + cargo build --release 로 수분 소요.
    /// 이미 있으면 --force 없이 skip.
    InstallBinary {
        #[arg(long, default_value_t = DEFAULT_VMID)] vmid: u32,
        /// vaultwarden git tag 또는 branch (기본: latest 릴리스 태그)
        #[arg(long)] version: Option<String>,
        /// cargo features (sqlite / mysql / postgresql). 기본 sqlite
        #[arg(long, default_value = "sqlite")] features: String,
        /// 이미 있어도 재빌드
        #[arg(long)] force: bool,
    },
    /// bw_web_builds 의 pre-built web-vault tarball 을 /opt/vaultwarden/web-vault 로 배치.
    InstallWebVault {
        #[arg(long, default_value_t = DEFAULT_VMID)] vmid: u32,
        /// git tag (예: v2026.2.0). 기본: latest
        #[arg(long)] version: Option<String>,
        #[arg(long)] force: bool,
    },
    /// 신규 LXC 에 원샷으로: install-env + install-systemd + install-backup-timer
    /// + install-web-vault + install-binary (필요 시) → start.
    /// 이미 있는 단계는 각자 멱등 처리.
    Bootstrap {
        #[arg(long, default_value_t = DEFAULT_VMID)] vmid: u32,
        #[arg(long, default_value = DEFAULT_DOMAIN)] url: String,
        #[arg(long)] admin_token: Option<String>,
        #[arg(long)] version: Option<String>,
        #[arg(long)] web_vault_version: Option<String>,
        /// 바이너리 빌드 step 건너뜀 (이미 /opt/vaultwarden/bin/vaultwarden 있을 때)
        #[arg(long)] skip_binary: bool,
    },
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
        Cmd::InstallSystemd { vmid } => install_systemd(vmid),
        Cmd::InstallBackupTimer { vmid } => install_backup_timer(vmid),
        Cmd::InstallEnv { vmid, url, admin_token } => install_env(vmid, &url, admin_token.as_deref()),
        Cmd::InstallBinary { vmid, version, features, force } =>
            install_binary(vmid, version.as_deref(), &features, force),
        Cmd::InstallWebVault { vmid, version, force } =>
            install_web_vault(vmid, version.as_deref(), force),
        Cmd::Bootstrap { vmid, url, admin_token, version, web_vault_version, skip_binary } =>
            bootstrap(vmid, &url, admin_token.as_deref(), version.as_deref(),
                      web_vault_version.as_deref(), skip_binary),
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

// ── install 템플릿 ──

const VAULTWARDEN_UNIT: &str = include_str!("../templates/vaultwarden.service");
const BACKUP_SCRIPT: &str    = include_str!("../templates/backup.sh");
const BACKUP_UNIT: &str      = include_str!("../templates/vaultwarden-backup.service");
const BACKUP_TIMER: &str     = include_str!("../templates/vaultwarden-backup.timer");

fn pct_write(vmid: u32, path: &str, content: &str) -> Result<()> {
    let tmp = format!("/tmp/pxi-vw-push-{}", std::process::id());
    std::fs::write(&tmp, content)?;
    let id = vmid.to_string();
    let out = Command::new("pct").args(["push", &id, &tmp, path]).output()?;
    let _ = std::fs::remove_file(&tmp);
    if !out.status.success() {
        return Err(anyhow!("pct push {path}: {}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(())
}

fn install_systemd(vmid: u32) -> Result<()> {
    println!("=== vaultwarden.service 설치 (LXC {vmid}) ===");
    pct_write(vmid, "/etc/systemd/system/vaultwarden.service", VAULTWARDEN_UNIT)?;
    // user + data dir 멱등 보장
    pct(vmid, &["sh", "-c",
        "id -u vaultwarden >/dev/null 2>&1 || useradd --system --no-create-home --shell /bin/false vaultwarden; \
         mkdir -p /opt/vaultwarden/data /opt/vaultwarden/bin; \
         chown -R vaultwarden:vaultwarden /opt/vaultwarden/data"
    ])?;
    pct(vmid, &["systemctl", "daemon-reload"])?;
    pct(vmid, &["systemctl", "enable", "vaultwarden"])?;
    println!("  ok — 바이너리(/opt/vaultwarden/bin/vaultwarden) 배포 후 `systemctl start vaultwarden`");
    Ok(())
}

fn install_backup_timer(vmid: u32) -> Result<()> {
    println!("=== vaultwarden-backup timer 설치 (LXC {vmid}) ===");
    pct_write(vmid, "/opt/vaultwarden/backup.sh", BACKUP_SCRIPT)?;
    pct(vmid, &["chmod", "+x", "/opt/vaultwarden/backup.sh"])?;
    pct(vmid, &["chown", "vaultwarden:vaultwarden", "/opt/vaultwarden/backup.sh"])?;
    pct_write(vmid, "/etc/systemd/system/vaultwarden-backup.service", BACKUP_UNIT)?;
    pct_write(vmid, "/etc/systemd/system/vaultwarden-backup.timer", BACKUP_TIMER)?;
    pct(vmid, &["systemctl", "daemon-reload"])?;
    pct(vmid, &["systemctl", "enable", "--now", "vaultwarden-backup.timer"])?;
    let status = pct(vmid, &["systemctl", "is-active", "vaultwarden-backup.timer"]).unwrap_or_default();
    println!("  timer: {}", status.trim());
    Ok(())
}

fn install_env(vmid: u32, url: &str, admin_token: Option<&str>) -> Result<()> {
    // ADMIN_TOKEN 조회: 인자 → control-plane/.env
    let token = match admin_token {
        Some(t) => t.to_string(),
        None => std::fs::read_to_string("/root/control-plane/.env")
            .ok()
            .and_then(|s| s.lines()
                .map(|l| l.trim_start_matches('#').trim())
                .find_map(|l| l.strip_prefix("VAULTWARDEN_ADMIN_TOKEN=").map(|v| v.trim().to_string()))
                .filter(|v| !v.is_empty()))
            .ok_or_else(|| anyhow!(
                "ADMIN_TOKEN 없음. --admin-token 명시 또는 control-plane/.env 에 VAULTWARDEN_ADMIN_TOKEN= 추가"))?,
    };

    let env_content = format!(
        "# Generated by `pxi run vaultwarden install-env`\n\
         ADMIN_TOKEN={token}\n\
         ROCKET_ADDRESS=0.0.0.0\n\
         # ROCKET_TLS 는 Traefik 경유라 off. 다시 쓰려면 주석 해제.\n\
         DATA_FOLDER=/opt/vaultwarden/data\n\
         DATABASE_MAX_CONNS=10\n\
         WEB_VAULT_FOLDER=/opt/vaultwarden/web-vault\n\
         WEB_VAULT_ENABLED=true\n\
         # 도메인 — invite/verify 메일 링크에 사용 (config.json 이 있으면 그쪽 우선)\n\
         DOMAIN={url}\n\
         # SMTP — mailgun-smtp-proxy shim 경유 (CF same-zone 수신자도 배달)\n\
         SMTP_HOST=10.0.50.122\n\
         SMTP_PORT=2526\n\
         SMTP_SECURITY=off\n\
         SMTP_FROM=devops@ranode.net\n\
         SMTP_FROM_NAME=Vaultwarden\n"
    );
    pct_write(vmid, "/opt/vaultwarden/.env", &env_content)?;
    pct(vmid, &["chown", "vaultwarden:vaultwarden", "/opt/vaultwarden/.env"])?;
    pct(vmid, &["chmod", "640", "/opt/vaultwarden/.env"])?;
    println!("  ok — /opt/vaultwarden/.env (DOMAIN={url}, SMTP mailgun-shim)");
    Ok(())
}

// ── install-binary / install-web-vault / bootstrap ──

const VW_UPSTREAM: &str = "https://github.com/dani-garcia/vaultwarden.git";
const WEB_VAULT_RELEASE_BASE: &str =
    "https://github.com/dani-garcia/bw_web_builds/releases/download";

fn install_binary(
    vmid: u32, version: Option<&str>, features: &str, force: bool,
) -> Result<()> {
    // 이미 있고 force 아니면 skip
    let exists = pct(vmid, &["sh", "-c", "test -x /opt/vaultwarden/bin/vaultwarden && echo yes || echo no"])
        .unwrap_or_default();
    if exists.trim() == "yes" && !force {
        println!("  /opt/vaultwarden/bin/vaultwarden 이미 존재 — skip (--force 로 재빌드)");
        return Ok(());
    }

    // 태그 결정: 지정 or latest
    let tag = match version {
        Some(v) => v.to_string(),
        None => resolve_latest("dani-garcia/vaultwarden")?,
    };
    println!("=== vaultwarden {tag} 빌드 (LXC {vmid}, features={features}) ===");
    println!("  rustup + apt deps + git clone + cargo build (수분 소요)");

    // apt deps + rustup + build 를 한 스크립트에
    let script = format!(
        r#"set -e
apt-get update -qq
apt-get install -y --no-install-recommends \
    git curl ca-certificates build-essential pkg-config \
    libssl-dev libsqlite3-dev libpq-dev libmariadb-dev-compat >/dev/null

# rustup (per-run, minimal)
if ! command -v cargo >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable >/dev/null
fi
. "$HOME/.cargo/env"

SRC=/opt/vaultwarden/src
mkdir -p /opt/vaultwarden/bin /opt/vaultwarden/data
if [ ! -d "$SRC/.git" ]; then
    git clone --depth 1 --branch {tag} {upstream} "$SRC"
else
    git -C "$SRC" fetch --depth 1 origin {tag} && git -C "$SRC" checkout {tag}
fi

cd "$SRC"
cargo build --release --features {features} --no-default-features
install -m 0755 target/release/vaultwarden /opt/vaultwarden/bin/vaultwarden
chown -R vaultwarden:vaultwarden /opt/vaultwarden/bin /opt/vaultwarden/data
echo "binary_sha=$(sha256sum /opt/vaultwarden/bin/vaultwarden | awk '{{print $1}}')"
"#,
        tag = tag, upstream = VW_UPSTREAM, features = features
    );
    let out = pct(vmid, &["sh", "-c", &script])?;
    print!("{out}");
    Ok(())
}

fn install_web_vault(vmid: u32, version: Option<&str>, force: bool) -> Result<()> {
    let exists = pct(vmid, &["sh", "-c",
        "test -f /opt/vaultwarden/web-vault/index.html && echo yes || echo no"]
    ).unwrap_or_default();
    if exists.trim() == "yes" && !force {
        println!("  /opt/vaultwarden/web-vault 이미 존재 — skip (--force 로 재설치)");
        return Ok(());
    }
    let tag = match version {
        Some(v) => v.to_string(),
        None => resolve_latest("dani-garcia/bw_web_builds")?,
    };
    println!("=== web-vault {tag} 배치 (LXC {vmid}) ===");
    let url = format!("{WEB_VAULT_RELEASE_BASE}/{tag}/bw_web_{tag}.tar.gz");
    let script = format!(
        r#"set -e
apt-get install -y --no-install-recommends curl ca-certificates tar >/dev/null
mkdir -p /opt/vaultwarden
rm -rf /opt/vaultwarden/web-vault.new
mkdir -p /opt/vaultwarden/web-vault.new
curl -fsSL "{url}" | tar -xz -C /opt/vaultwarden/web-vault.new --strip-components=1
[ -f /opt/vaultwarden/web-vault.new/index.html ] || {{ echo "web-vault tarball missing index.html"; exit 1; }}
rm -rf /opt/vaultwarden/web-vault
mv /opt/vaultwarden/web-vault.new /opt/vaultwarden/web-vault
chown -R vaultwarden:vaultwarden /opt/vaultwarden/web-vault
echo ok
"#, url = url);
    let out = pct(vmid, &["sh", "-c", &script])?;
    print!("{out}");
    Ok(())
}

fn bootstrap(
    vmid: u32, url: &str, admin_token: Option<&str>,
    vw_version: Option<&str>, web_vault_version: Option<&str>,
    skip_binary: bool,
) -> Result<()> {
    println!("=== Vaultwarden bootstrap (LXC {vmid}) ===");
    install_env(vmid, url, admin_token)?;
    install_systemd(vmid)?;
    install_backup_timer(vmid)?;
    install_web_vault(vmid, web_vault_version, false)?;
    if !skip_binary {
        install_binary(vmid, vw_version, "sqlite", false)?;
    } else {
        println!("  --skip-binary 지정, 바이너리 설치 skip");
    }
    // start
    pct(vmid, &["systemctl", "restart", "vaultwarden"])?;
    let st = pct(vmid, &["systemctl", "is-active", "vaultwarden"]).unwrap_or_default();
    println!("  service: {}", st.trim());
    Ok(())
}

/// GitHub API 로 owner/repo 의 latest release tag 조회.
fn resolve_latest(repo: &str) -> Result<String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let out = Command::new("curl")
        .args(["-sSL", "-H", "Accept: application/vnd.github+json", &url])
        .output()?;
    if !out.status.success() {
        return Err(anyhow!("latest release 조회 실패: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let body = String::from_utf8_lossy(&out.stdout);
    let start = body.find("\"tag_name\":\"")
        .ok_or_else(|| anyhow!("tag_name 파싱 실패: {}", &body.chars().take(160).collect::<String>()))?;
    let rest = &body[start + 12..];
    let end = rest.find('"').ok_or_else(|| anyhow!("tag_name 끝 따옴표 찾기 실패"))?;
    Ok(rest[..end].to_string())
}
