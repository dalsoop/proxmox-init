//! prelik-mail — 메일 스택 관리.
//! - mailpit LXC 설치 (수신 아카이브)
//! - postfix-relay 호스트 설정 (Maddy 경유 발송)

use clap::{Parser, Subcommand};
use prelik_core::common;
use std::fs;

#[derive(Parser)]
#[command(name = "prelik-mail", about = "Maddy + Mailpit + Postfix relay")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Mailpit LXC에 설치 (수신 아카이브)
    InstallMailpit {
        #[arg(long)]
        vmid: String,
    },
    /// 호스트 Postfix를 Maddy로 relay (SPF/DKIM 차단 회피)
    PostfixRelay {
        /// Maddy LXC IP (기본 10.0.50.122)
        #[arg(long, default_value = "10.0.50.122")] // LINT_ALLOW: 관례상 Maddy IP
        maddy_ip: String,
        #[arg(long, default_value = "587")]
        port: String,
    },
    /// 메일 스택 상태 점검
    Status,
    Doctor,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::InstallMailpit { vmid } => install_mailpit(&vmid),
        Cmd::PostfixRelay { maddy_ip, port } => postfix_relay(&maddy_ip, &port),
        Cmd::Status => { status(); Ok(()) }
        Cmd::Doctor => { doctor(); Ok(()) }
    }
}

fn install_mailpit(vmid: &str) -> anyhow::Result<()> {
    if !common::has_cmd("pct") {
        anyhow::bail!("pct 없음 — Proxmox 호스트에서만 동작");
    }
    println!("=== Mailpit 설치 (LXC {vmid}) ===");

    common::run("pct", &["exec", vmid, "--", "bash", "-c",
        "apt-get update && apt-get install -y curl ca-certificates socat python3-flask"
    ])?;

    // 바이너리
    common::run("pct", &["exec", vmid, "--", "bash", "-c",
        "curl -sL https://github.com/axllent/mailpit/releases/latest/download/mailpit-linux-amd64.tar.gz | tar -xz -C /usr/local/bin mailpit && chmod +x /usr/local/bin/mailpit"
    ])?;

    // 유저 + 데이터 + 토큰
    common::run("pct", &["exec", vmid, "--", "bash", "-c",
        "useradd --system --no-create-home --shell /bin/false mailpit 2>/dev/null || true; mkdir -p /var/lib/mailpit; chown -R mailpit:mailpit /var/lib/mailpit; openssl rand -hex 32 > /var/lib/mailpit/ingest-token; chmod 600 /var/lib/mailpit/ingest-token; chown mailpit:mailpit /var/lib/mailpit/ingest-token"
    ])?;

    // systemd unit
    let unit = "[Unit]
Description=Mailpit mail archive
After=network-online.target
[Service]
Type=simple
User=mailpit
Group=mailpit
Environment=MP_DATABASE=/var/lib/mailpit/mailpit.db
Environment=MP_MAX_MESSAGES=0
Environment=MP_SMTP_BIND_ADDR=0.0.0.0:1025
Environment=MP_UI_BIND_ADDR=0.0.0.0:8025
Environment=MP_SMTP_AUTH_ACCEPT_ANY=true
Environment=MP_SMTP_AUTH_ALLOW_INSECURE=true
ExecStart=/usr/local/bin/mailpit
Restart=always
RestartSec=5
[Install]
WantedBy=multi-user.target
";
    write_to_lxc(vmid, "/etc/systemd/system/mailpit.service", unit)?;
    common::run("pct", &["exec", vmid, "--", "bash", "-c",
        "systemctl daemon-reload && systemctl enable --now mailpit"
    ])?;

    println!("✓ Mailpit 설치 완료");
    common::run("pct", &["exec", vmid, "--", "cat", "/var/lib/mailpit/ingest-token"])
        .map(|t| println!("  INGEST_TOKEN: {t}")).ok();
    Ok(())
}

fn postfix_relay(maddy_ip: &str, port: &str) -> anyhow::Result<()> {
    println!("=== 호스트 Postfix → Maddy relay ===");

    let smtp_user = read_host_env("SMTP_USER");
    let smtp_pass = read_host_env("SMTP_PASSWORD");
    if smtp_user.is_empty() || smtp_pass.is_empty() {
        anyhow::bail!("/etc/prelik/.env 또는 /etc/proxmox-host-setup/.env 에 SMTP_USER/SMTP_PASSWORD 필요");
    }

    if !fs::metadata("/etc/postfix/main.cf").is_ok() {
        anyhow::bail!("Postfix 미설치. apt install postfix 먼저");
    }

    // main.cf 자동 백업 (동일 초 내 재실행에도 안전하게 nanosecond 포함)
    let ts = common::run("date", &["+%Y%m%d-%H%M%S.%N"]).unwrap_or_else(|_| "backup".into());
    let backup = format!("/etc/postfix/main.cf.prelik-{}", ts.trim());
    common::run_bash(&format!("sudo cp /etc/postfix/main.cf {backup}"))?;
    println!("  백업: {backup}");

    // libsasl2-modules (SASL plugin — 누락 시 relay가 조용히 깨짐)
    if common::run("dpkg", &["-s", "libsasl2-modules"]).is_err() {
        println!("  libsasl2-modules 설치...");
        common::run_bash("sudo apt-get install -y libsasl2-modules")
            .map_err(|e| anyhow::anyhow!("libsasl2-modules 설치 실패 (sudo/apt 확인): {e}"))?;
    }

    // 기존 relay 라인 제거
    common::run_bash("sudo sed -i '/^relayhost[[:space:]]*=/d;/^smtp_sasl_/d;/^smtp_tls_security_level/d;/^sender_canonical_maps/d' /etc/postfix/main.cf")?;

    // 추가
    let append = format!("
# prelik postfix-relay
relayhost = [{maddy_ip}]:{port}
smtp_sasl_auth_enable = yes
smtp_sasl_password_maps = hash:/etc/postfix/sasl_passwd
smtp_sasl_security_options = noanonymous
smtp_tls_security_level = may
smtp_sasl_tls_security_options = noanonymous
sender_canonical_maps = regexp:/etc/postfix/sender_canonical
");
    common::run_bash(&format!("echo '{}' | sudo tee -a /etc/postfix/main.cf >/dev/null", append.replace('\'', "'\\''")))?;

    // SASL 패스워드가 /tmp에 순간이라도 평문 노출되지 않게 먼저 권한 0600으로 생성
    let sasl = format!("[{maddy_ip}]:{port} {smtp_user}:{smtp_pass}\n");
    let sasl_tmp = common::run("mktemp", &["-t", "prelik.XXXXXXXX"])?;
    let sasl_tmp = sasl_tmp.trim().to_string();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup { fn drop(&mut self) { let _ = fs::remove_file(&self.0); } }
    let _g1 = Cleanup(std::path::PathBuf::from(&sasl_tmp));

    common::run("chmod", &["600", &sasl_tmp])?;
    fs::write(&sasl_tmp, sasl)?;
    common::run_bash(&format!(
        "sudo install -m 600 -o root -g root {sasl_tmp} /etc/postfix/sasl_passwd && sudo postmap /etc/postfix/sasl_passwd"
    ))?;

    let canonical = format!("/.+/    {smtp_user}\n");
    let can_tmp = common::run("mktemp", &["-t", "prelik.XXXXXXXX"])?;
    let can_tmp = can_tmp.trim().to_string();
    let _g2 = Cleanup(std::path::PathBuf::from(&can_tmp));
    fs::write(&can_tmp, canonical)?;
    common::run_bash(&format!(
        "sudo install -m 644 -o root -g root {can_tmp} /etc/postfix/sender_canonical"
    ))?;

    // 설정 적용 전 한번 더 검증
    if let Err(e) = common::run_bash("sudo postfix check") {
        eprintln!("⚠ postfix check 실패 — main.cf 롤백 중: {e}");
        common::run_bash(&format!("sudo cp {backup} /etc/postfix/main.cf"))?;
        anyhow::bail!("postfix 설정 검증 실패, main.cf 롤백 완료. 백업: {backup}");
    }

    if let Err(e) = common::run_bash("sudo systemctl reload postfix && sudo postfix flush") {
        eprintln!("⚠ postfix reload 실패 — main.cf 롤백 중: {e}");
        common::run_bash(&format!("sudo cp {backup} /etc/postfix/main.cf"))?;
        common::run_bash("sudo systemctl reload postfix").ok();
        anyhow::bail!("postfix reload 실패, main.cf 롤백 완료. 백업: {backup}");
    }
    println!("✓ Postfix → [{maddy_ip}]:{port} relay 설정 완료");
    println!("  롤백이 필요하면: sudo cp {backup} /etc/postfix/main.cf && sudo systemctl reload postfix");
    Ok(())
}

fn status() {
    println!("=== 메일 스택 상태 ===");
    if let Ok(out) = common::run("systemctl", &["is-active", "postfix"]) {
        println!("  postfix: {}", out.trim());
    }
    if let Ok(out) = common::run_bash("mailq 2>/dev/null | tail -2") {
        println!("  queue: {}", out.trim().lines().last().unwrap_or(""));
    }
}

fn doctor() {
    println!("=== prelik-mail doctor ===");
    println!("  pct:       {}", if common::has_cmd("pct") { "✓" } else { "✗" });
    println!("  postfix:   {}", if common::has_cmd("postfix") { "✓" } else { "✗" });
    println!("  systemctl: {}", if common::has_cmd("systemctl") { "✓" } else { "✗" });
}

fn read_host_env(key: &str) -> String {
    for p in ["/etc/prelik/.env", "/etc/proxmox-host-setup/.env"] {
        if let Ok(raw) = fs::read_to_string(p) {
            for line in raw.lines() {
                if let Some(v) = line.strip_prefix(&format!("{key}=")) {
                    return v.trim().trim_matches('"').to_string();
                }
            }
        }
    }
    String::new()
}

fn write_to_lxc(vmid: &str, path: &str, content: &str) -> anyhow::Result<()> {
    let out = common::run("mktemp", &["-t", "prelik.XXXXXXXX"])?;
    let tmp = out.trim().to_string();
    let tmp_path = std::path::PathBuf::from(&tmp);
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup { fn drop(&mut self) { let _ = fs::remove_file(&self.0); } }
    let _g = Cleanup(tmp_path.clone());
    fs::write(&tmp_path, content)?;
    common::run("chmod", &["600", &tmp])?;
    common::run("pct", &["push", vmid, &tmp, path])?;
    Ok(())
}
