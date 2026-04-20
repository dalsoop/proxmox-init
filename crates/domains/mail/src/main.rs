//! pxi-mail — 메일 스택 관리.
//! - mailpit LXC 설치 (수신 아카이브)
//! - postfix-relay 호스트 설정 (Maddy 경유 발송)

use clap::{Parser, Subcommand};
use pxi_core::common;
use std::fs;

// ---------- 기존 코드 호환 헬퍼 (pxi-core API drift 보정) ----------
// 예전 common에 있던 run_bash가 현재 API에서 제거되어 로컬 헬퍼로 제공.
#[allow(dead_code)]
fn run_bash(script: &str) -> anyhow::Result<String> {
    common::run_capture("bash", &["-lc", script])
}

#[derive(Parser)]
#[command(name = "pxi-mail", about = "Maddy + Mailpit + Postfix relay + CF Email Sending proxy")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Mailpit LXC에 설치 (수신 아카이브)
    InstallMailpit {
        #[arg(long)]
        vmid: pxi_core::types::Vmid,
    },
    /// [DEPRECATED] 호스트 Postfix → Maddy 587 SASL relay. cf-proxy-sync로 대체 권장.
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
    /// 메일 서버 초기 세팅 (Maddy LXC 설치 + DNS + NAT)
    Setup {
        #[arg(long)]
        vmid: pxi_core::types::Vmid,
        #[arg(long)]
        ip: String,
        #[arg(long)]
        domain: String,
        #[arg(long)]
        email: String,
        #[arg(long)]
        password: String,
    },
    /// Cloudflare Email Sending SMTP 프록시 설치 (LXC에 Rust 바이너리 + systemd)
    CfProxyInstall {
        /// 대상 LXC VMID (기본 50122 — Maddy LXC)
        #[arg(long, default_value = "50122")]
        vmid: String,
        /// 호스트에 빌드된 cf-mail-proxy 바이너리
        #[arg(long, default_value = "/opt/cf-mail-proxy/target/release/cf-mail-proxy")]
        binary: String,
    },
    /// CF Email Sending 이번달 발송 쿼터 조회 (무료 3,000/월)
    CfProxyQuota,
    /// 모든 running LXC의 postfix relayhost를 cf-mail-proxy(2525)로 일괄 동기화
    CfProxySync {
        /// cf-mail-proxy 호스트 IP (기본 10.0.50.122)
        #[arg(long, default_value = "10.0.50.122")]
        host: String,
        /// 포트 (기본 2525)
        #[arg(long, default_value = "2525")]
        port: String,
        /// 변경 없이 예상 동작만 출력
        #[arg(long)]
        dry_run: bool,
        /// 현재 모든 LXC relayhost 현황만 조회 (변경 없음)
        #[arg(long)]
        status: bool,
        /// 특정 LXC만 (미지정 시 모든 running LXC)
        #[arg(long)]
        vmid: Option<String>,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::InstallMailpit { vmid } => install_mailpit(vmid.as_str()),
        Cmd::PostfixRelay { maddy_ip, port } => postfix_relay(&maddy_ip, &port),
        Cmd::Status => { status(); Ok(()) }
        Cmd::Doctor => { doctor(); Ok(()) }
        Cmd::Setup { vmid, ip, domain, email, password } => mail_setup(vmid.as_str(), &ip, &domain, &email, &password),
        Cmd::CfProxyInstall { vmid, binary } => cf_proxy_install(&vmid, &binary),
        Cmd::CfProxyQuota => cf_proxy_quota(),
        Cmd::CfProxySync { host, port, dry_run, status, vmid } => cf_proxy_sync(&host, &port, dry_run, status, vmid.as_deref()),
    }
}

fn install_mailpit(vmid: &str) -> anyhow::Result<()> {
    if !common::command_exists("pct") {
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
    common::run_capture("pct", &["exec", vmid, "--", "cat", "/var/lib/mailpit/ingest-token"])
        .map(|t| println!("  INGEST_TOKEN: {t}")).ok();
    Ok(())
}

fn postfix_relay(maddy_ip: &str, port: &str) -> anyhow::Result<()> {
    println!("=== 호스트 Postfix → Maddy relay ===");

    // maddy_ip/port 검증 — main.cf/sasl_passwd에 format! 삽입되므로 config injection 차단.
    // IP(v4/v6)/hostname만 허용. ']', 개행, 공백 등이 있으면 main.cf에 추가 설정 주입 가능.
    if !maddy_ip.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == ':' || c == '-') {
        anyhow::bail!("maddy_ip 형식 오류: {maddy_ip:?} (IPv4/IPv6/hostname만 허용)");
    }
    if maddy_ip.is_empty() || maddy_ip.len() > 253 {
        anyhow::bail!("maddy_ip가 비어 있거나 너무 김");
    }
    if !port.chars().all(|c| c.is_ascii_digit()) || port.is_empty() {
        anyhow::bail!("port는 숫자만: {port:?}");
    }
    let port_num: u16 = port.parse()
        .map_err(|_| anyhow::anyhow!("port 범위 초과: {port}"))?;
    if port_num == 0 {
        anyhow::bail!("port 0 은 유효하지 않음");
    }

    let smtp_user = read_host_env("SMTP_USER");
    let smtp_pass = read_host_env("SMTP_PASSWORD");
    if smtp_user.is_empty() || smtp_pass.is_empty() {
        anyhow::bail!("/etc/pxi/.env 또는 /etc/proxmox-host-setup/.env 에 SMTP_USER/SMTP_PASSWORD 필요");
    }
    // SMTP user/pass에도 개행/제어문자 차단 (sasl_passwd 포맷 주입)
    if smtp_user.contains('\n') || smtp_user.contains('\r') || smtp_user.contains('\0') {
        anyhow::bail!("SMTP_USER에 개행/제어문자 포함");
    }
    if smtp_pass.contains('\n') || smtp_pass.contains('\r') || smtp_pass.contains('\0') {
        anyhow::bail!("SMTP_PASSWORD에 개행/제어문자 포함");
    }

    if fs::metadata("/etc/postfix/main.cf").is_err() {
        anyhow::bail!("Postfix 미설치. apt install postfix 먼저");
    }

    // 자동 백업 — main.cf + sasl_passwd + sender_canonical
    // (보조 맵 파일도 덮어쓰기 전에 저장해야 완전한 rollback이 가능)
    let ts_out = common::run_capture("date", &["+%Y%m%d-%H%M%S.%N"]).unwrap_or_else(|_| "backup".into());
    let ts = ts_out.trim();
    let backup_dir = format!("/etc/postfix/pxi-backup-{ts}");
    run_bash(&format!("sudo mkdir -p {backup_dir}"))?;

    struct BackupSet {
        dir: String,
        files: Vec<&'static str>,
    }
    let backup = BackupSet {
        dir: backup_dir.clone(),
        files: vec!["main.cf", "sasl_passwd", "sasl_passwd.db", "sender_canonical"],
    };
    for f in &backup.files {
        let src = format!("/etc/postfix/{f}");
        // 존재 여부를 먼저 Rust 쪽에서 판정 — 쉘 단락 평가의 true 마스킹 회피
        let exists = std::path::Path::new(&src).exists();
        if !exists {
            continue;
        }
        // 존재하면 cp는 반드시 성공해야 함. 실패는 명시적 에러.
        run_bash(&format!(
            "sudo cp -a {src} {}/{f}",
            backup.dir
        )).map_err(|e| anyhow::anyhow!(
            "백업 실패 ({f}): {e} — /etc/postfix 권한/공간 확인 필요"
        ))?;
    }
    println!("  백업: {}", backup.dir);

    // 실패 시 원본을 되돌리는 헬퍼
    let rollback = |reason: &str| -> anyhow::Result<()> {
        eprintln!("⚠ {reason} — 원본 복원 중...");
        // 새로 만든 파일이 있으면 삭제하고, 백업이 있으면 복원
        for f in &backup.files {
            let src_path = format!("{}/{f}", backup.dir);
            let dst_path = format!("/etc/postfix/{f}");
            let restore_result = run_bash(&format!(
                "if [ -e '{src_path}' ]; then                    sudo cp -a '{src_path}' '{dst_path}';                  else                    sudo rm -f '{dst_path}';                  fi"
            ));
            if let Err(e) = restore_result {
                return Err(anyhow::anyhow!(
                    "복원 실패 ({f}): {e} — 수동 복구 필요. 백업: {}",
                    backup.dir
                ));
            }
        }
        // 원본 설정으로 postfix reload — 실패하면 명시적으로 에러
        run_bash("sudo systemctl reload postfix")
            .map_err(|e| anyhow::anyhow!(
                "원본 복원 후 postfix reload 실패: {e} — 수동 확인 필요. 백업: {}",
                backup.dir
            ))?;
        Ok(())
    };

    // libsasl2-modules (SASL plugin — 누락 시 relay가 조용히 깨짐)
    if common::run("dpkg", &["-s", "libsasl2-modules"]).is_err() {
        println!("  libsasl2-modules 설치...");
        run_bash("sudo apt-get install -y libsasl2-modules")
            .map_err(|e| anyhow::anyhow!("libsasl2-modules 설치 실패 (sudo/apt 확인): {e}"))?;
    }

    // 기존 relay 라인 제거
    run_bash("sudo sed -i '/^relayhost[[:space:]]*=/d;/^smtp_sasl_/d;/^smtp_tls_security_level/d;/^sender_canonical_maps/d' /etc/postfix/main.cf")?;

    // 추가
    let append = format!("
# pxi postfix-relay
relayhost = [{maddy_ip}]:{port}
smtp_sasl_auth_enable = yes
smtp_sasl_password_maps = hash:/etc/postfix/sasl_passwd
smtp_sasl_security_options = noanonymous
smtp_tls_security_level = may
smtp_sasl_tls_security_options = noanonymous
sender_canonical_maps = regexp:/etc/postfix/sender_canonical
");
    run_bash(&format!("echo '{}' | sudo tee -a /etc/postfix/main.cf >/dev/null", append.replace('\'', "'\\''")))?;

    // SASL 패스워드가 /tmp에 순간이라도 평문 노출되지 않게 먼저 권한 0600으로 생성
    let sasl = format!("[{maddy_ip}]:{port} {smtp_user}:{smtp_pass}\n");
    let sasl_tmp = common::run_capture("mktemp", &["-t", "pxi.XXXXXXXX"])?;
    let sasl_tmp = sasl_tmp.trim().to_string();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup { fn drop(&mut self) { let _ = fs::remove_file(&self.0); } }
    let _g1 = Cleanup(std::path::PathBuf::from(&sasl_tmp));

    common::run("chmod", &["600", &sasl_tmp])?;
    fs::write(&sasl_tmp, sasl)?;
    run_bash(&format!(
        "sudo install -m 600 -o root -g root {sasl_tmp} /etc/postfix/sasl_passwd && sudo postmap /etc/postfix/sasl_passwd"
    ))?;

    let canonical = format!("/.+/    {smtp_user}\n");
    let can_tmp = common::run_capture("mktemp", &["-t", "pxi.XXXXXXXX"])?;
    let can_tmp = can_tmp.trim().to_string();
    let _g2 = Cleanup(std::path::PathBuf::from(&can_tmp));
    fs::write(&can_tmp, canonical)?;
    run_bash(&format!(
        "sudo install -m 644 -o root -g root {can_tmp} /etc/postfix/sender_canonical"
    ))?;

    // 1) postfix check — 실패 시 전체 rollback
    if let Err(e) = run_bash("sudo postfix check") {
        rollback(&format!("postfix check 실패: {e}"))?;
        anyhow::bail!("설정 검증 실패, 원본 복원 완료. 백업: {backup_dir}");
    }

    // 2) postfix reload — 설정 적용. 실패 시 rollback.
    //    flush는 별개 동작이므로 reload와 분리.
    if let Err(e) = run_bash("sudo systemctl reload postfix") {
        rollback(&format!("postfix reload 실패: {e}"))?;
        anyhow::bail!("reload 실패, 원본 복원 완료. 백업: {backup_dir}");
    }

    // 3) postfix flush — 이미 쌓인 deferred 메일 재시도. 실패해도 설정 자체는 OK.
    if let Err(e) = run_bash("sudo postfix flush") {
        eprintln!("⚠ postfix flush 실패 (설정 적용은 성공): {e}");
        eprintln!("  큐 재시도는 수동으로: sudo postfix flush");
    }

    println!("✓ Postfix → [{maddy_ip}]:{port} relay 설정 완료");
    println!("  롤백 (전체 파일 복원):");
    println!("    sudo cp -a {backup_dir}/* /etc/postfix/ 2>/dev/null");
    println!("    sudo postmap /etc/postfix/sasl_passwd 2>/dev/null");
    println!("    sudo systemctl reload postfix");
    println!("  (백업에 없던 파일은 현재 파일 그대로 유지됨 — 필요시 수동 rm)");
    Ok(())
}

fn status() {
    println!("=== 메일 스택 상태 ===");
    if let Ok(out) = common::run_capture("systemctl", &["is-active", "postfix"]) {
        println!("  postfix: {}", out.trim());
    }
    if let Ok(out) = run_bash("mailq 2>/dev/null | tail -2") {
        println!("  queue: {}", out.trim().lines().last().unwrap_or(""));
    }
}

fn doctor() {
    println!("=== pxi-mail doctor ===");
    println!("  pct:       {}", if common::command_exists("pct") { "✓" } else { "✗" });
    println!("  postfix:   {}", if common::command_exists("postfix") { "✓" } else { "✗" });
    println!("  systemctl: {}", if common::command_exists("systemctl") { "✓" } else { "✗" });
}

// ---------- mail-setup (Maddy) ----------

fn lxc_sh(vmid: &str, cmd: &str) -> String {
    let output = std::process::Command::new("pct")
        .args(["exec", vmid, "--", "bash", "-c", cmd])
        .output();
    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => String::new(),
    }
}

fn mail_setup(vmid: &str, ip: &str, domain: &str, email: &str, password: &str) -> anyhow::Result<()> {
    if !common::command_exists("pct") { anyhow::bail!("pct 없음 — Proxmox 호스트에서만 동작"); }

    let hostname = format!("mail.{domain}");
    println!("=== 메일 서버 전체 세팅 (Maddy) ===\n");

    // 1. LXC 확인/시작
    println!("[1/5] LXC 확인...");
    let status_out = common::run_capture("pct", &["status", vmid]).unwrap_or_default();
    let parsed: pxi_core::types::LxcStatus = status_out.parse().unwrap();
    if parsed.is_running() {
        println!("  LXC {vmid} 이미 실행 중");
    } else if !status_out.is_empty() {
        println!("  LXC {vmid} 존재 — 시작");
        let _ = std::process::Command::new("pct").args(["start", vmid]).status();
        std::thread::sleep(std::time::Duration::from_secs(3));
    } else {
        anyhow::bail!("LXC {vmid} 없음 — 먼저 생성하세요 (pxi-lxc create)");
    }

    // 2. Maddy 설치
    println!("[2/5] Maddy 설치...");
    let has_maddy = lxc_sh(vmid, "ls /usr/local/bin/maddy 2>/dev/null");
    if has_maddy.contains("maddy") {
        let ver = lxc_sh(vmid, "/usr/local/bin/maddy version 2>&1 | head -1");
        println!("  Maddy 이미 설치됨 ({ver})");
    } else {
        lxc_sh(vmid, "systemctl stop postfix 2>/dev/null; apt-get purge -y postfix 2>/dev/null");
        lxc_sh(vmid, "DEBIAN_FRONTEND=noninteractive apt-get update -qq && apt-get install -y -qq zstd curl ca-certificates");

        let maddy_url = "https://github.com/foxcpp/maddy/releases/latest/download/maddy-x86_64-linux-musl.tar.zst";
        lxc_sh(vmid, &format!("curl -sL {maddy_url} -o /tmp/maddy.tar.zst && cd /tmp && tar --zstd -xf maddy.tar.zst"));
        lxc_sh(vmid, "find /tmp -name 'maddy' -type f -executable | head -1 | xargs -I{{}} cp {{}} /usr/local/bin/maddy && chmod +x /usr/local/bin/maddy");
        lxc_sh(vmid, "useradd -r -s /usr/sbin/nologin -d /var/lib/maddy maddy 2>/dev/null; mkdir -p /etc/maddy /var/lib/maddy /run/maddy; chown maddy:maddy /var/lib/maddy /run/maddy");

        let ver = lxc_sh(vmid, "/usr/local/bin/maddy version 2>&1 | head -1");
        println!("  Maddy {ver} 설치 완료");
    }

    // 3. Maddy 설정
    println!("[3/5] Maddy 설정...");
    let maddy_conf = format!(r#"$(hostname) = {hostname}
$(primary_domain) = {domain}
$(local_domains) = $(primary_domain)
tls off

auth.pass_table local_authdb {{
    table sql_table {{
        driver sqlite3
        dsn credentials.db
        table_name passwords
    }}
}}
storage.imapsql local_mailboxes {{
    driver sqlite3
    dsn imapsql.db
}}
hostname $(hostname)

smtp tcp://0.0.0.0:25 {{
    limits {{ all rate 20 1s; all concurrency 10 }}
    dmarc yes
    check {{ require_mx_record; dkim; spf }}
    source $(local_domains) {{ reject 501 5.1.8 "Use Submission" }}
    default_source {{
        destination postmaster $(local_domains) {{ deliver_to &local_mailboxes }}
        default_destination {{ reject 550 5.1.1 "User doesn't exist" }}
    }}
}}
submission tcp://0.0.0.0:587 {{
    limits {{ all rate 50 1s }}
    auth &local_authdb
    source $(local_domains) {{
        destination postmaster $(local_domains) {{ deliver_to &local_mailboxes }}
        default_destination {{
            modify {{ dkim $(primary_domain) $(local_domains) default }}
            deliver_to &remote_queue
        }}
    }}
    default_source {{ reject 501 5.1.8 "Non-local sender domain" }}
}}
imap tcp://0.0.0.0:143 {{
    auth &local_authdb
    storage &local_mailboxes
}}
target.remote outbound_delivery {{
    limits {{ destination rate 20 1s; destination concurrency 10 }}
}}
target.queue remote_queue {{
    target &outbound_delivery
    autogenerated_msg_domain $(primary_domain)
}}
"#);

    write_to_lxc(vmid, "/etc/maddy/maddy.conf", &maddy_conf)?;
    lxc_sh(vmid, "touch /etc/maddy/aliases");
    println!("  설정 완료 (domain: {domain}, hostname: {hostname})");

    // 4. 계정 생성
    println!("[4/5] 메일 계정 생성...");
    lxc_sh(vmid, "systemctl daemon-reload && systemctl enable maddy && systemctl start maddy 2>/dev/null");
    std::thread::sleep(std::time::Duration::from_secs(2));

    let existing = lxc_sh(vmid, "/usr/local/bin/maddy creds list 2>/dev/null");
    if existing.contains(email) {
        println!("  {email} 계정 이미 존재");
    } else {
        lxc_sh(vmid, &format!("echo -e '{password}\\n{password}' | /usr/local/bin/maddy creds create {email} 2>/dev/null"));
        lxc_sh(vmid, &format!("/usr/local/bin/maddy imap-acct create {email} 2>/dev/null"));
        println!("  {email} 계정 생성 완료");
    }

    lxc_sh(vmid, "systemctl daemon-reload && systemctl restart maddy");

    // 5. NAT 포트포워딩
    println!("[5/5] NAT 포트포워딩...");
    for (port, label) in [(25, "SMTP"), (587, "Submission"), (143, "IMAP")] {
        let port_s = port.to_string();
        let check = std::process::Command::new("iptables")
            .args(["-t", "nat", "-C", "PREROUTING", "-p", "tcp", "--dport", &port_s,
                "-j", "DNAT", "--to-destination", &format!("{ip}:{port}")])
            .output().map(|o| o.status.success()).unwrap_or(false);
        if check {
            println!("  {label} (:{port}) -> {ip} 이미 존재");
        } else {
            let _ = std::process::Command::new("iptables")
                .args(["-t", "nat", "-A", "PREROUTING", "-p", "tcp", "--dport", &port_s,
                    "-j", "DNAT", "--to-destination", &format!("{ip}:{port}"),
                    "-m", "comment", "--comment", &format!("mail-{label}")])
                .output();
            println!("  {label}: :{port} -> {ip}:{port}");
        }
    }

    println!("\n=== Maddy 세팅 완료 ===");
    println!("  LXC: {vmid}, IP: {ip}");
    println!("  도메인: {hostname}, 계정: {email}");
    println!("  SMTP: {ip}:25 (수신), {ip}:587 (발신), IMAP: {ip}:143");
    println!("\n  DNS 설정 필요 (A, MX, SPF, DKIM, DMARC)");
    Ok(())
}

// ---------- cf-proxy-install ----------

fn cf_proxy_install(vmid: &str, binary: &str) -> anyhow::Result<()> {
    if !common::command_exists("pct") {
        anyhow::bail!("pct 없음 — Proxmox 호스트에서만 동작");
    }
    if !std::path::Path::new(binary).exists() {
        anyhow::bail!("바이너리 없음: {binary} — 먼저 빌드하세요 (github.com/dalsoop/cf-mail-proxy)");
    }

    let cf_account = read_host_env("CF_ACCOUNT_ID");
    let cf_email = read_host_env("CLOUDFLARE_EMAIL");
    let cf_key = read_host_env("CLOUDFLARE_API_KEY");
    if cf_account.is_empty() || cf_email.is_empty() || cf_key.is_empty() {
        anyhow::bail!("/etc/pxi/.env 에 CF_ACCOUNT_ID / CLOUDFLARE_EMAIL / CLOUDFLARE_API_KEY 필요");
    }
    let default_from = {
        let v = read_host_env("DEFAULT_FROM");
        if v.is_empty() { "devops@prelik.com".to_string() } else { v }
    };
    let allowed = {
        let v = read_host_env("CF_ALLOWED_DOMAINS");
        if v.is_empty() { "prelik.com,ranode.net,internal.kr".to_string() } else { v }
    };

    // 1) 바이너리 push
    println!("=== cf-mail-proxy 설치 (LXC {vmid}) ===");
    println!("[1/4] 바이너리 배포: {binary} → /usr/local/bin/cf-mail-proxy");
    common::run("pct", &["push", vmid, binary, "/usr/local/bin/cf-mail-proxy"])?;
    common::run("pct", &["exec", vmid, "--", "chmod", "+x", "/usr/local/bin/cf-mail-proxy"])?;

    // 2) env 파일 (크리덴셜 + 정책)
    println!("[2/4] /etc/cf-mail-proxy.env 작성");
    let env_content = format!(
        "CF_ACCOUNT_ID={cf_account}\nCLOUDFLARE_EMAIL={cf_email}\nCLOUDFLARE_API_KEY={cf_key}\nDEFAULT_FROM={default_from}\nALLOWED_DOMAINS={allowed}\n"
    );
    write_to_lxc(vmid, "/etc/cf-mail-proxy.env", &env_content)?;
    common::run("pct", &["exec", vmid, "--", "chmod", "600", "/etc/cf-mail-proxy.env"])?;

    // 3) systemd unit
    println!("[3/4] systemd unit 작성");
    let unit = "[Unit]
Description=Cloudflare Email Sending SMTP proxy
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
EnvironmentFile=/etc/cf-mail-proxy.env
Environment=PROXY_HOST=0.0.0.0
Environment=PROXY_PORT=2525
Environment=RUST_LOG=info
ExecStart=/usr/local/bin/cf-mail-proxy
Restart=always
RestartSec=3
User=nobody
Group=nogroup
AmbientCapabilities=CAP_NET_BIND_SERVICE

[Install]
WantedBy=multi-user.target
";
    write_to_lxc(vmid, "/etc/systemd/system/cf-mail-proxy.service", unit)?;

    // 4) 기동
    println!("[4/4] systemd daemon-reload + 기동");
    common::run("pct", &["exec", vmid, "--", "systemctl", "daemon-reload"])?;
    common::run("pct", &["exec", vmid, "--", "systemctl", "enable", "--now", "cf-mail-proxy"])?;
    std::thread::sleep(std::time::Duration::from_secs(2));
    let state = common::run_capture("pct", &["exec", vmid, "--", "systemctl", "is-active", "cf-mail-proxy"])
        .unwrap_or_else(|_| "unknown".into());
    println!("✓ cf-mail-proxy 설치 완료 (상태: {})", state.trim());
    println!("  다음: pxi run mail cf-proxy-sync  (전 LXC postfix를 2525로 통일)");
    Ok(())
}

// ---------- cf-proxy-sync ----------

fn cf_proxy_sync(host: &str, port: &str, dry_run: bool, status_only: bool, target_vmid: Option<&str>) -> anyhow::Result<()> {
    if !common::command_exists("pct") {
        anyhow::bail!("pct 없음 — Proxmox 호스트에서만 동작");
    }
    // IP/port 검증 (postfix main.cf 주입 방지)
    if !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == ':' || c == '-') {
        anyhow::bail!("host 형식 오류: {host:?}");
    }
    if !port.chars().all(|c| c.is_ascii_digit()) || port.is_empty() {
        anyhow::bail!("port는 숫자만: {port:?}");
    }

    let relay = format!("[{host}]:{port}");
    let vmids: Vec<String> = match target_vmid {
        Some(v) => v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        None => {
            let out = common::run_capture("pct", &["list"])?;
            out.lines().skip(1)
                .filter_map(|l| {
                    let cols: Vec<&str> = l.split_whitespace().collect();
                    if cols.len() >= 2 && cols[1] == "running" {
                        Some(cols[0].to_string())
                    } else { None }
                })
                .collect()
        }
    };

    if status_only {
        println!("=== LXC postfix relayhost 현황 (기대값: {relay}) ===");
        println!("  {:<8}  {:<22}  RELAYHOST", "VMID", "HOST");
        for vmid in &vmids {
            let hostname = pct_hostname(vmid);
            let current = common::run_capture("pct", &["exec", vmid, "--", "postconf", "-h", "relayhost"])
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "(no postfix)".into());
            println!("  {:<8}  {:<22}  {}", vmid, hostname, if current.is_empty() { "(empty)" } else { &current });
        }
        return Ok(());
    }

    println!("=== cf-proxy-sync → {relay} {}===", if dry_run { "(dry-run) " } else { "" });
    let proxy_ip = host.to_string();
    for vmid in &vmids {
        let hostname = pct_hostname(vmid);
        // 프록시가 돌고 있는 LXC는 건너뜀
        let my_ip = common::run_capture("pct", &["exec", vmid, "--", "hostname", "-I"])
            .unwrap_or_default();
        if my_ip.split_whitespace().next().unwrap_or("") == proxy_ip {
            println!("  ◎ LXC {vmid} ({hostname}) — 프록시 자체, 건너뜀");
            continue;
        }
        // postfix 없으면 건너뜀
        if common::run_capture("pct", &["exec", vmid, "--", "which", "postfix"]).is_err() {
            println!("  — LXC {vmid} ({hostname}) — postfix 없음");
            continue;
        }
        let current = common::run_capture("pct", &["exec", vmid, "--", "postconf", "-h", "relayhost"])
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if current == relay {
            println!("  ✓ LXC {vmid} ({hostname}) — 이미 동기화됨");
            continue;
        }
        println!("  → LXC {vmid} ({hostname}) — {:?} → {}", current, relay);
        if dry_run { continue; }
        let script = format!(
            "postconf -e 'relayhost = {relay}' && postconf -e 'smtp_sasl_auth_enable = no' && \
             {{ [ -f /etc/postfix/sasl_passwd ] && mv /etc/postfix/sasl_passwd /etc/postfix/sasl_passwd.bak-$(date +%Y%m%d) 2>/dev/null || true ; }} && \
             {{ systemctl reload postfix 2>/dev/null || systemctl restart postfix 2>/dev/null || true ; }}"
        );
        let _ = common::run("pct", &["exec", vmid, "--", "bash", "-lc", &script]);
    }
    println!("완료.");
    Ok(())
}

// ---------- cf-proxy-quota ----------

fn cf_proxy_quota() -> anyhow::Result<()> {
    // CF Email Sending public beta는 아직 metrics/stats API를 공개하지 않음 (2026-04 기준).
    // 대안: cf-mail-proxy systemd 저널에서 `delivered=` 라인을 카운트. LXC 50122에서 실행.
    let vmid = std::env::var("CF_MAIL_PROXY_VMID").unwrap_or_else(|_| "50122".into());

    // 이번달 시작시각 (UTC)
    let since = common::run_capture("date", &["-u", "-d", "-30 days", "+%Y-%m-%d"])
        .unwrap_or_default();
    let since = since.trim();

    println!("=== CF Email Sending 이번달 발송 카운트 (LXC {vmid}, since {since}) ===");

    // 프록시 로그에서 delivered 파싱
    let logs = common::run_capture(
        "pct",
        &[
            "exec", &vmid, "--", "bash", "-lc",
            &format!("journalctl -u cf-mail-proxy --since '{since}' --no-pager 2>/dev/null | grep -c 'delivered=\\[' || echo 0"),
        ],
    )
    .unwrap_or_default();

    let delivered: u64 = logs.trim().parse().unwrap_or(0);
    let pct_used = (delivered as f64 / 3000.0 * 100.0) as u64;

    println!("  발송 완료: {delivered} / 3,000 ({pct_used}%)");

    // 소스별 분해 (호스트, 주요 LXC)
    let by_source = common::run_capture(
        "pct",
        &[
            "exec", &vmid, "--", "bash", "-lc",
            &format!(
                "journalctl -u cf-mail-proxy --since '{since}' --no-pager 2>/dev/null | \
                 grep -oE 'session start.*peer=\\S+' | awk '{{print $NF}}' | sort | uniq -c | sort -rn | head -10"
            ),
        ],
    )
    .unwrap_or_default();
    if !by_source.trim().is_empty() {
        println!("\n피어별 세션 수 (Top 10):");
        for line in by_source.lines() {
            println!("  {line}");
        }
    }

    if delivered > 2700 {
        eprintln!("\n⚠ 90% 초과 — 이번달 CF 쿼터 소진 임박. 초과 시 $0.35/1k 과금.");
    } else if delivered > 2400 {
        eprintln!("\n주의: 80% 도달.");
    }

    println!("\n참고: CF Email Sending beta는 공식 stats API 미제공 — 프록시 저널 기반 근사치.");
    Ok(())
}

fn pct_hostname(vmid: &str) -> String {
    let conf = common::run_capture("pct", &["config", vmid]).unwrap_or_default();
    for line in conf.lines() {
        if let Some(rest) = line.strip_prefix("hostname: ") {
            return rest.trim().to_string();
        }
    }
    "?".into()
}

fn read_host_env(key: &str) -> String {
    for p in ["/etc/pxi/.env", "/etc/proxmox-host-setup/.env"] {
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
    let out = common::run_capture("mktemp", &["-t", "pxi.XXXXXXXX"])?;
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
