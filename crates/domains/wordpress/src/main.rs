//! pxi-wordpress — WordPress LXC 설치/설정/관리 도메인 바이너리

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use pxi_core::common;

#[derive(Parser)]
#[command(name = "pxi-wordpress")]
#[command(about = "WordPress LXC 설치/설정/관리")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// LXC 생성 + Apache + PHP + MariaDB + WP + WP-CLI + Traefik 라우트
    Install {
        /// LXC VMID (규약 강제 — Vmid newtype)
        #[arg(long)]
        vmid: pxi_core::types::Vmid,
        /// LXC 호스트명
        #[arg(long)]
        hostname: String,
        /// 도메인 (예: blog.example.com)
        #[arg(long)]
        domain: String,
        /// MariaDB 비밀번호 (미지정 시 자동 생성)
        #[arg(long)]
        db_password: Option<String>,
        /// 디스크 크기 (GB)
        #[arg(long, default_value = "16")]
        disk: String,
        /// CPU 코어 수
        #[arg(long, default_value = "2")]
        cores: String,
        /// 메모리 (MB)
        #[arg(long, default_value = "2048")]
        memory: String,
    },
    /// WP-CLI로 초기 설정 (사이트명/관리자/한국어)
    Setup {
        /// LXC VMID
        #[arg(long)]
        vmid: String,
        /// 사이트 URL (예: https://blog.example.com)
        #[arg(long)]
        url: String,
        /// 사이트 제목
        #[arg(long)]
        title: String,
        /// 관리자 아이디
        #[arg(long)]
        admin_user: String,
        /// 관리자 비밀번호
        #[arg(long)]
        admin_password: String,
        /// 관리자 이메일
        #[arg(long)]
        admin_email: String,
        /// 언어/로캘
        #[arg(long, default_value = "ko_KR")]
        locale: String,
    },
    /// WP-CLI pass-through (pct exec 경유)
    Cli {
        /// LXC VMID
        #[arg(long)]
        vmid: String,
        /// WP-CLI 인자 (-- 뒤에 전달)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// LXC 삭제 + Traefik 라우트 제거
    Delete {
        /// LXC VMID
        #[arg(long)]
        vmid: String,
        /// 강제 삭제 확인
        #[arg(long)]
        force: bool,
    },
    /// 의존성 상태 확인 (pct, wp 존재 여부)
    Doctor,
    /// wp-admin/wp-login/xmlrpc 를 *.internal.kr 도메인에서만 허용하도록 전체 LXC 에 중앙 lockdown 배포
    LockdownApply {
        /// 특정 VMID 만 (생략 시 자동 탐지된 전체 WP LXC)
        #[arg(long)]
        vmid: Option<String>,
        /// 자동 탐지 대신 명시적 LXC 목록 (쉼표 구분, 예: 50200,50201)
        #[arg(long)]
        vmids: Option<String>,
    },
    /// 현재 lockdown 적용 상태 (활성/비활성/싱크)
    LockdownStatus {
        #[arg(long)]
        vmid: Option<String>,
        #[arg(long)]
        vmids: Option<String>,
    },
    /// lockdown 제거 (a2disconf)
    LockdownRemove {
        #[arg(long)]
        vmid: Option<String>,
        #[arg(long)]
        vmids: Option<String>,
    },
    /// lockdown 동작 테스트 (internal 통과 + 외부 차단 + admin-ajax 예외)
    LockdownTest {
        /// 테스트할 LXC VMID (기본 : 탐지된 첫번째)
        #[arg(long)]
        vmid: Option<String>,
        /// 커스텀 Host 헤더 (미지정 시 자동 조합 테스트)
        #[arg(long)]
        host: Option<String>,
        /// 커스텀 경로 (기본 /wp-admin/)
        #[arg(long, default_value = "/wp-admin/")]
        path: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Install {
            vmid,
            hostname,
            domain,
            db_password,
            disk,
            cores,
            memory,
        } => cmd_install(
            vmid.as_str(),
            &hostname,
            &domain,
            db_password.as_deref(),
            &disk,
            &cores,
            &memory,
        ),
        Commands::Setup {
            vmid,
            url,
            title,
            admin_user,
            admin_password,
            admin_email,
            locale,
        } => cmd_setup(
            &vmid,
            &url,
            &title,
            &admin_user,
            &admin_password,
            &admin_email,
            &locale,
        ),
        Commands::Cli { vmid, args } => cmd_cli(&vmid, &args),
        Commands::Delete { vmid, force } => cmd_delete(&vmid, force),
        Commands::Doctor => cmd_doctor(),
        Commands::LockdownApply { vmid, vmids } => {
            cmd_lockdown_apply(&resolve_targets(vmid.as_deref(), vmids.as_deref())?)
        }
        Commands::LockdownStatus { vmid, vmids } => {
            cmd_lockdown_status(&resolve_targets(vmid.as_deref(), vmids.as_deref())?)
        }
        Commands::LockdownRemove { vmid, vmids } => {
            cmd_lockdown_remove(&resolve_targets(vmid.as_deref(), vmids.as_deref())?)
        }
        Commands::LockdownTest { vmid, host, path } => {
            let targets = resolve_targets(vmid.as_deref(), None)?;
            let test_vmid = targets
                .first()
                .ok_or_else(|| anyhow::anyhow!("lockdown test 대상 LXC 없음"))?;
            cmd_lockdown_test(test_vmid, host.as_deref(), &path)
        }
    }
}

// ---------------------------------------------------------------------------
// doctor
// ---------------------------------------------------------------------------

fn cmd_doctor() -> Result<()> {
    println!("=== pxi-wordpress doctor ===\n");
    println!("  WordPress는 여러 LXC를 관리하므로 개별 점검에는 --vmid가 필요합니다.");
    println!("  전체 점검 체크리스트:\n");

    // pct
    println!(
        "  {} pct (Proxmox LXC 관리)",
        if common::command_exists("pct") {
            "✓"
        } else {
            "✗"
        }
    );

    // pxi-traefik
    println!(
        "  {} pxi-traefik (라우트 관리)",
        if common::command_exists("pxi-traefik") {
            "✓"
        } else {
            "✗"
        }
    );

    // pveam
    println!(
        "  {} pveam (LXC 템플릿)",
        if common::command_exists("pveam") {
            "✓"
        } else {
            "✗"
        }
    );

    // List known WP LXCs (pct list, filter wp-* hostnames)
    println!();
    if let Ok(output) = common::run_capture("pct", &["list"]) {
        let wp_lxcs: Vec<&str> = output
            .lines()
            .filter(|l| l.contains("wp-") || l.contains("wordpress"))
            .collect();
        if wp_lxcs.is_empty() {
            println!("  (WordPress LXC를 찾을 수 없음)");
        } else {
            println!("  알려진 WordPress LXC:");
            for line in &wp_lxcs {
                println!("    {}", line.trim());
            }
        }
    } else {
        println!("  ✗ pct list 실행 실패");
    }

    println!("\n  개별 LXC 점검: pxi-wordpress cli --vmid <VMID> -- core version");
    Ok(())
}

// ---------------------------------------------------------------------------
// install
// ---------------------------------------------------------------------------

fn cmd_install(
    vmid: &str,
    hostname: &str,
    domain: &str,
    db_password: Option<&str>,
    disk: &str,
    cores: &str,
    memory: &str,
) -> Result<()> {
    println!("=== WordPress 설치: VMID {vmid}, 도메인 {domain} ===\n");

    // DB 비밀번호 (미지정 시 자동 생성)
    let db_pass = match db_password {
        Some(p) => p.to_string(),
        None => common::run_capture("openssl", &["rand", "-hex", "16"])?,
    };

    // [1/5] LXC 생성
    println!("[1/5] LXC 생성 (VMID={vmid}, hostname={hostname})...");
    let storage = "local-lvm";
    let template = get_debian_template()?;
    common::run(
        "pct",
        &[
            "create",
            vmid,
            &template,
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
            &format!("name=eth0,bridge=vmbr0,ip=dhcp"),
            "--unprivileged",
            "1",
            "--features",
            "nesting=1",
            "--start",
            "1",
        ],
    )?;

    // 부팅 대기
    std::thread::sleep(std::time::Duration::from_secs(5));
    common::ensure_lxc_running(vmid)?;

    // [2/5] Apache + PHP + MariaDB + WordPress
    println!("[2/5] WordPress 스택 설치...");
    let install_script = format!(
        r#"
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

apt-get update -qq
apt-get install -y -qq \
  apache2 mariadb-server \
  php php-mysql php-curl php-gd php-mbstring php-xml php-zip \
  libapache2-mod-php curl wget unzip

systemctl enable --now mariadb
mysql -e "CREATE DATABASE IF NOT EXISTS wordpress;"
mysql -e "CREATE USER IF NOT EXISTS 'wp_user'@'localhost' IDENTIFIED BY '{db_pass}';"
mysql -e "GRANT ALL PRIVILEGES ON wordpress.* TO 'wp_user'@'localhost';"
mysql -e "FLUSH PRIVILEGES;"

cd /var/www/html
rm -f index.html
if [ ! -f wp-config.php ]; then
  curl -fsSL https://wordpress.org/latest.tar.gz -o /tmp/wordpress.tar.gz
  tar xzf /tmp/wordpress.tar.gz --strip-components=1 -C /var/www/html
  rm -f /tmp/wordpress.tar.gz

  cp wp-config-sample.php wp-config.php
  sed -i "s/database_name_here/wordpress/" wp-config.php
  sed -i "s/username_here/wp_user/" wp-config.php
  sed -i "s/password_here/{db_pass}/" wp-config.php

  SALT=$(curl -fsSL https://api.wordpress.org/secret-key/1.1/salt/)
  sed -i "/put your unique phrase here/d" wp-config.php
  printf "%s\n" "$SALT" >> wp-config.php
fi

chown -R www-data:www-data /var/www/html
chmod -R 755 /var/www/html

a2enmod rewrite
systemctl enable --now apache2
systemctl restart apache2
echo "WordPress stack installed"
"#
    );
    common::pct_exec(vmid, &["bash", "-c", &install_script])?;

    // WP-CLI 설치
    println!("  wp-cli 설치...");
    ensure_wp_cli(vmid)?;

    // [3/5] Traefik 라우트
    println!("[3/5] Traefik 라우트 등록 ({domain})...");
    let route_name = format!("wp-{vmid}");
    let ip = common::pct_exec(vmid, &["bash", "-c", "hostname -I | awk '{print $1}'"])?;
    let backend = format!("http://{}:80", ip.trim());
    common::run(
        "pxi-traefik",
        &[
            "route-add",
            "--name",
            &route_name,
            "--domain",
            domain,
            "--backend",
            &backend,
        ],
    )?;

    // [4/5] .env 기록 (LXC 안)
    println!("[4/5] LXC 내부 .env 기록...");
    let env_content = format!(
        "WORDPRESS_DOMAIN={domain}\nWORDPRESS_DB_PASSWORD={db_pass}\nWORDPRESS_DB_USER=wp_user\nWORDPRESS_DB_NAME=wordpress\n"
    );
    common::pct_exec(
        vmid,
        &[
            "bash",
            "-c",
            &format!(
                "mkdir -p /etc/pxi && cat > /etc/pxi/.env << 'ENV'\n{}ENV\nchmod 600 /etc/pxi/.env",
                env_content,
            ),
        ],
    )?;

    // [5/5] 검증
    println!("[5/5] 검증...");
    let check_url = format!("https://{domain}/");
    match common::run_capture("curl", &["-fsSk", "--max-time", "10", &check_url]) {
        Ok(_) => println!("  https://{domain}/ 접속 가능"),
        Err(_) => println!("  https://{domain}/ 아직 응답 없음 — Traefik/DNS 전파 대기"),
    }

    println!("\n=== WordPress 설치 완료 ===");
    println!("  VMID:     {vmid}");
    println!("  도메인:   https://{domain}/");
    println!("  DB 비번:  {db_pass}");
    println!("  초기 설정: pxi-wordpress setup --vmid {vmid} --url https://{domain} ...");
    Ok(())
}

/// 사용 가능한 Debian 템플릿 조회
fn get_debian_template() -> Result<String> {
    let templates = common::run_capture("pveam", &["available", "--section", "system"])?;
    // debian-12 또는 debian-13 우선
    for line in templates.lines().rev() {
        if line.contains("debian-12") || line.contains("debian-13") {
            if let Some(tpl) = line.split_whitespace().nth(1) {
                // 로컬에 다운로드 확인
                let _ = common::run("pveam", &["download", "local", tpl]);
                return Ok(format!("local:vztmpl/{tpl}"));
            }
        }
    }
    bail!("Debian 템플릿을 찾을 수 없습니다. pveam update 실행 후 재시도");
}

/// WP-CLI 설치 (LXC 안)
fn ensure_wp_cli(vmid: &str) -> Result<()> {
    let check = common::pct_exec(
        vmid,
        &["bash", "-c", "test -x /usr/local/bin/wp && echo ok"],
    );
    if let Ok(out) = check {
        if out.trim() == "ok" {
            return Ok(());
        }
    }
    common::pct_exec(
        vmid,
        &[
            "bash",
            "-c",
            "curl -fsSL https://raw.githubusercontent.com/wp-cli/builds/gh-pages/phar/wp-cli.phar \
         -o /usr/local/bin/wp && chmod +x /usr/local/bin/wp",
        ],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// setup
// ---------------------------------------------------------------------------

fn cmd_setup(
    vmid: &str,
    url: &str,
    title: &str,
    admin_user: &str,
    admin_password: &str,
    admin_email: &str,
    locale: &str,
) -> Result<()> {
    println!("=== WordPress 초기 설정: VMID {vmid} ===\n");

    common::ensure_lxc_running(vmid)?;
    ensure_wp_cli(vmid)?;

    // wp core install
    let script = format!(
        r#"export PATH=/usr/local/bin:$PATH
cd /var/www/html && \
wp core install \
  --url='{url}' \
  --title='{title}' \
  --admin_user='{admin_user}' \
  --admin_password='{admin_password}' \
  --admin_email='{admin_email}' \
  --locale='{locale}' \
  --allow-root \
  --skip-email"#
    );
    common::pct_exec(vmid, &["bash", "-c", &script])?;

    // 한국어 설정
    if locale == "ko_KR" {
        println!("  한국어 팩 설치 + 활성화...");
        let ko_script = r#"export PATH=/usr/local/bin:$PATH
cd /var/www/html
wp language core install ko_KR --allow-root 2>/dev/null || true
wp site switch-language ko_KR --allow-root
wp option update timezone_string 'Asia/Seoul' --allow-root
wp option update date_format 'Y년 n월 j일' --allow-root
wp option update time_format 'H:i' --allow-root"#;
        match common::pct_exec(vmid, &["bash", "-c", ko_script]) {
            Ok(_) => println!("  한국어/KST/날짜형식 설정 완료"),
            Err(e) => eprintln!("  한국어 설정 일부 실패 (WordPress는 동작함): {e}"),
        }
    }

    println!("\nWordPress 초기 설정 완료");
    println!("  URL:      {url}");
    println!("  관리자:   {admin_user}");
    println!("  대시보드: {url}/wp-admin/");
    Ok(())
}

// ---------------------------------------------------------------------------
// cli
// ---------------------------------------------------------------------------

fn cmd_cli(vmid: &str, args: &[String]) -> Result<()> {
    common::ensure_lxc_running(vmid)?;
    ensure_wp_cli(vmid)?;

    let wp_args = args.join(" ");
    let script =
        format!("export PATH=/usr/local/bin:$PATH; cd /var/www/html && wp {wp_args} --allow-root");
    common::pct_exec_passthrough(vmid, &["bash", "-c", &script])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// delete
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// lockdown — *.internal.kr 외부 요청에 대해 wp-admin/wp-login/xmlrpc 차단
// ---------------------------------------------------------------------------

/// Apache conf (크레이트 assets 에서 compile-time 임베딩)
const LOCKDOWN_CONF: &str = include_str!("../assets/wp-admin-lockdown.conf");
const LOCKDOWN_CONF_NAME: &str = "wp-admin-lockdown";
const LOCKDOWN_REMOTE_PATH: &str = "/etc/apache2/conf-available/wp-admin-lockdown.conf";
const LOCKDOWN_ENABLED_PATH: &str = "/etc/apache2/conf-enabled/wp-admin-lockdown.conf";

/// 대상 LXC 결정 : --vmid > --vmids > 자동 탐지
fn resolve_targets(single: Option<&str>, csv: Option<&str>) -> Result<Vec<String>> {
    if let Some(v) = single {
        return Ok(vec![v.to_string()]);
    }
    if let Some(list) = csv {
        return Ok(list
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect());
    }
    // 자동 탐지 : Apache 설치 + WP 파일 존재한 LXC
    let listing = common::run_capture("pct", &["list"]).unwrap_or_default();
    let mut candidates = Vec::new();
    for line in listing.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let vmid = parts[0];
        let status = parts[1];
        if status != "running" {
            continue;
        }
        // Apache + WordPress 감지
        let check = common::pct_exec(
            vmid,
            &[
                "bash",
                "-c",
                "test -d /etc/apache2/conf-available && \
             (test -f /var/www/html/wp-config.php || ls /var/www/*/wp-config.php >/dev/null 2>&1) \
             && echo yes",
            ],
        );
        if let Ok(out) = check {
            if out.trim() == "yes" {
                candidates.push(vmid.to_string());
            }
        }
    }
    if candidates.is_empty() {
        bail!("Apache + WordPress LXC 를 자동 탐지하지 못함. --vmid 또는 --vmids 로 지정");
    }
    Ok(candidates)
}

fn lxc_has_apache(vmid: &str) -> bool {
    matches!(
        common::pct_exec(vmid, &["test", "-d", "/etc/apache2/conf-available/"]),
        Ok(_)
    )
}

fn cmd_lockdown_apply(targets: &[String]) -> Result<()> {
    if targets.is_empty() {
        bail!("적용 대상 없음");
    }
    println!("=== wp-admin lockdown 적용: {} 개 LXC ===\n", targets.len());
    let mut failed = Vec::new();
    for vmid in targets {
        if !lxc_has_apache(vmid) {
            println!("[{vmid}] Apache 없음 — 건너뜀");
            continue;
        }
        // 임시 파일로 push (pct push 는 파일 경로만 받음)
        let tmp = format!("/tmp/wp-admin-lockdown-{vmid}.conf");
        std::fs::write(&tmp, LOCKDOWN_CONF)?;
        if let Err(e) = common::run("pct", &["push", vmid, &tmp, LOCKDOWN_REMOTE_PATH]) {
            eprintln!("[{vmid}] ✗ push 실패: {e}");
            failed.push(vmid.clone());
            continue;
        }
        let _ = std::fs::remove_file(&tmp);
        // a2enconf
        if let Err(e) = common::pct_exec(vmid, &["a2enconf", LOCKDOWN_CONF_NAME]) {
            eprintln!("[{vmid}] ✗ a2enconf 실패: {e}");
            failed.push(vmid.clone());
            continue;
        }
        // configtest
        let test_out = common::pct_exec(vmid, &["apachectl", "configtest"]).unwrap_or_default();
        if !test_out.contains("Syntax OK")
            && !common::pct_exec(
                vmid,
                &[
                    "bash",
                    "-c",
                    "apachectl configtest 2>&1 | grep -q 'Syntax OK' && echo ok",
                ],
            )
            .map(|s| s.trim() == "ok")
            .unwrap_or(false)
        {
            eprintln!("[{vmid}] ✗ configtest 실패 — 롤백");
            let _ = common::pct_exec(vmid, &["a2disconf", LOCKDOWN_CONF_NAME]);
            failed.push(vmid.clone());
            continue;
        }
        // reload
        if let Err(e) = common::pct_exec(vmid, &["systemctl", "reload", "apache2"]) {
            eprintln!("[{vmid}] ✗ reload 실패: {e}");
            failed.push(vmid.clone());
            continue;
        }
        println!("[{vmid}] ✓ 적용 완료");
    }
    println!();
    if failed.is_empty() {
        println!("전체 {} 개 LXC 에 lockdown 적용 성공", targets.len());
        Ok(())
    } else {
        bail!("적용 실패 LXC: {}", failed.join(", "));
    }
}

fn cmd_lockdown_status(targets: &[String]) -> Result<()> {
    println!("=== wp-admin lockdown 상태 ===\n");
    for vmid in targets {
        if !lxc_has_apache(vmid) {
            println!("[{vmid}] Apache 없음");
            continue;
        }
        let enabled = common::pct_exec(vmid, &["test", "-L", LOCKDOWN_ENABLED_PATH]).is_ok();
        if !enabled {
            println!("[{vmid}] ✗ 비활성화");
            continue;
        }
        // content 직접 비교
        let remote = common::pct_exec(vmid, &["cat", LOCKDOWN_REMOTE_PATH]).unwrap_or_default();
        if remote.trim_end() == LOCKDOWN_CONF.trim_end() {
            println!("[{vmid}] ✓ 활성화 (싱크 OK)");
        } else {
            println!("[{vmid}] ⚠ 활성화 (싱크 안맞음 — apply 재실행 필요)");
        }
    }
    Ok(())
}

fn cmd_lockdown_remove(targets: &[String]) -> Result<()> {
    println!("=== wp-admin lockdown 제거 ===\n");
    for vmid in targets {
        if !lxc_has_apache(vmid) {
            println!("[{vmid}] Apache 없음 — 건너뜀");
            continue;
        }
        let enabled = common::pct_exec(vmid, &["test", "-L", LOCKDOWN_ENABLED_PATH]).is_ok();
        if !enabled {
            println!("[{vmid}] 이미 비활성화");
            continue;
        }
        common::pct_exec(vmid, &["a2disconf", LOCKDOWN_CONF_NAME])?;
        common::pct_exec(vmid, &["systemctl", "reload", "apache2"])?;
        println!("[{vmid}] ✓ 비활성화 완료");
    }
    Ok(())
}

fn cmd_lockdown_test(vmid: &str, custom_host: Option<&str>, path: &str) -> Result<()> {
    // LXC IP
    let ip = common::pct_exec(vmid, &["bash", "-c", "hostname -I | awk '{print $1}'"])?;
    let ip = ip.trim();
    if ip.is_empty() {
        bail!("LXC {vmid} IP 조회 실패");
    }
    println!("=== lockdown 테스트: LXC {vmid} ({ip}) ===\n");

    let run_curl = |host: &str, p: &str| -> Result<u16> {
        let out = common::run_capture(
            "curl",
            &[
                "-sk",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                "-H",
                &format!("Host: {host}"),
                &format!("http://{ip}{p}"),
            ],
        )?;
        out.trim()
            .parse::<u16>()
            .map_err(|e| anyhow::anyhow!("status parse: {e}"))
    };

    // 커스텀 호스트 지정 시 한 번만
    if let Some(host) = custom_host {
        let code = run_curl(host, path)?;
        println!("  [{code}] {host}{path}");
        return Ok(());
    }

    // 자동 : internal vhost 하나 뽑기
    let internal_host = common::pct_exec(
        vmid,
        &[
            "bash",
            "-c",
            "grep -h ServerName /etc/apache2/sites-enabled/*.conf 2>/dev/null | \
         awk '{print $2}' | grep internal.kr | head -1",
        ],
    )
    .unwrap_or_default()
    .trim()
    .to_string();

    let mut failed = false;
    let expect = |desc: &str, host: &str, p: &str, want_block: bool| -> Result<bool> {
        let code = run_curl(host, p)?;
        let blocked = code == 403;
        let ok = blocked == want_block;
        let mark = if ok { "✓" } else { "✗" };
        println!("  {mark} [{code}] {desc:30} {host}{p}");
        Ok(ok)
    };

    if !internal_host.is_empty() {
        if !expect(
            "internal → wp-admin (통과)",
            &internal_host,
            "/wp-admin/",
            false,
        )? {
            failed = true;
        }
        if !expect(
            "internal → admin-ajax (통과)",
            &internal_host,
            "/wp-admin/admin-ajax.php",
            false,
        )? {
            failed = true;
        }
    } else {
        println!("  (internal.kr vhost 없음 — internal 테스트 스킵)");
    }

    if !expect(
        "외부 → wp-admin (차단)",
        "evil.example.com",
        "/wp-admin/",
        true,
    )? {
        failed = true;
    }
    if !expect(
        "외부 → wp-login (차단)",
        "evil.example.com",
        "/wp-login.php",
        true,
    )? {
        failed = true;
    }
    if !expect(
        "외부 → xmlrpc (차단)",
        "evil.example.com",
        "/xmlrpc.php",
        true,
    )? {
        failed = true;
    }
    if !expect(
        "외부 → admin-ajax (통과)",
        "evil.example.com",
        "/wp-admin/admin-ajax.php",
        false,
    )? {
        failed = true;
    }

    println!();
    if failed {
        bail!("일부 테스트 실패");
    }
    println!("모든 테스트 통과");
    Ok(())
}

// ---------------------------------------------------------------------------
// delete
// ---------------------------------------------------------------------------

fn cmd_delete(vmid: &str, force: bool) -> Result<()> {
    if !force {
        bail!("삭제는 --force 필요 (WordPress 데이터/DB 모두 사라짐)");
    }
    println!("=== WordPress 삭제: VMID {vmid} ===");

    // [1/2] Traefik 라우트 제거
    let route_name = format!("wp-{vmid}");
    println!("[1/2] Traefik 라우트 제거 ({route_name})...");
    match common::run("pxi-traefik", &["route-remove", "--name", &route_name]) {
        Ok(_) => println!("  Traefik 라우트 제거 완료"),
        Err(e) => eprintln!("  Traefik 라우트 제거 실패 (계속 진행): {e}"),
    }

    // [2/2] LXC 삭제
    println!("[2/2] LXC 삭제...");
    // 실행 중이면 먼저 정지
    let _ = common::run("pct", &["stop", vmid]);
    common::run("pct", &["destroy", vmid])?;

    println!("WordPress VMID {vmid} 삭제 완료");
    Ok(())
}
