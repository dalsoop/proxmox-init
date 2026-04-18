//! pxi-wordpress — WordPress LXC 배포/관리.

use clap::{Parser, Subcommand};
use pxi_core::common;

#[derive(Parser)]
#[command(name = "pxi-wordpress", about = "WordPress 관리 (LXC 기반)")]
struct Cli { #[command(subcommand)] cmd: Cmd }

#[derive(Subcommand)]
enum Cmd {
    /// WordPress LXC 생성 + 설치 (Nginx + PHP-FPM + MariaDB + WP-CLI)
    Install {
        /// LXC VMID
        #[arg(long)]
        vmid: String,
        /// 호스트명
        #[arg(long)]
        hostname: String,
        /// 도메인 (Traefik 라우트 자동 등록)
        #[arg(long)]
        domain: String,
        /// WP admin 이메일
        #[arg(long, default_value = "team@dalsoop.com")]
        admin_email: String,
        /// 디스크 GB
        #[arg(long, default_value = "8")]
        disk: String,
        /// 메모리 MB
        #[arg(long, default_value = "2048")]
        memory: String,
    },
    /// WordPress 상태 (Nginx + PHP-FPM + MariaDB)
    Status {
        /// LXC VMID
        vmid: String,
    },
    /// WP-CLI 명령 실행
    Wp {
        /// LXC VMID
        vmid: String,
        /// wp-cli 인자 (예: "plugin list", "core update")
        args: Vec<String>,
    },
    /// 백업 (DB dump + wp-content tar)
    Backup {
        vmid: String,
        #[arg(long, default_value = "/mnt/truenas/shared/backups/wordpress")]
        dest: String,
    },
    /// SSL 인증서 + Traefik 라우트 등록
    Route {
        vmid: String,
        #[arg(long)]
        domain: String,
    },
    /// 플러그인 일괄 업데이트
    UpdatePlugins { vmid: String },
    /// PHP 버전 변경
    PhpVersion {
        vmid: String,
        /// 예: 8.3
        version: String,
    },
    /// LXC 삭제 (WordPress 포함)
    Destroy {
        vmid: String,
        #[arg(long)]
        force: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Install { vmid, hostname, domain, admin_email, disk, memory } => {
            println!("WordPress 설치: LXC {} ({})", vmid, hostname);
            println!("도메인: {}, 관리자: {}", domain, admin_email);

            // 1. LXC 생성
            common::run("pct", &["create", &vmid, "local:vztmpl/debian-13-standard_13.0-1_amd64.tar.zst",
                "--hostname", &hostname, "--storage", "local-lvm",
                "--rootfs", &format!("local-lvm:{}", disk),
                "--cores", "2", "--memory", &memory,
                "--net0", &format!("name=eth0,bridge=vmbr1,ip=dhcp"),
                "--start", "1"]);

            // 2. 패키지 설치
            let install_script = r#"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y nginx mariadb-server php-fpm php-mysql php-xml php-mbstring \
  php-curl php-zip php-gd php-intl php-imagick unzip curl
# MariaDB 초기 설정
mysql -e "CREATE DATABASE IF NOT EXISTS wordpress;"
mysql -e "CREATE USER IF NOT EXISTS 'wp'@'localhost' IDENTIFIED BY 'wp_$(openssl rand -hex 8)';"
mysql -e "GRANT ALL ON wordpress.* TO 'wp'@'localhost'; FLUSH PRIVILEGES;"
# WP-CLI
curl -sS -o /usr/local/bin/wp https://raw.githubusercontent.com/wp-cli/builds/gh-pages/phar/wp-cli.phar
chmod +x /usr/local/bin/wp
# WordPress 다운로드
mkdir -p /var/www/html
cd /var/www/html && wp core download --allow-root --locale=ko_KR
chown -R www-data:www-data /var/www/html
systemctl enable --now nginx php*-fpm mariadb
"#;
            common::run("pct", &["exec", &vmid, "--", "bash", "-c", install_script]);
            println!("✓ WordPress 설치 완료. 도메인 라우트: pxi-wordpress route {} --domain {}", vmid, domain);
        }
        Cmd::Status { vmid } => {
            common::run("pct", &["exec", &vmid, "--", "bash", "-c",
                "systemctl is-active nginx php*-fpm mariadb; echo ---; wp --allow-root --path=/var/www/html core version 2>/dev/null"]);
        }
        Cmd::Wp { vmid, args } => {
            let cmd = format!("cd /var/www/html && wp --allow-root {}", args.join(" "));
            common::run("pct", &["exec", &vmid, "--", "bash", "-c", &cmd]);
        }
        Cmd::Backup { vmid, dest } => {
            let script = format!(
                "mkdir -p {dest} && \
                 mysqldump -u root wordpress > {dest}/wordpress-$(date +%Y%m%d).sql && \
                 tar -czf {dest}/wp-content-$(date +%Y%m%d).tar.gz -C /var/www/html wp-content && \
                 echo '✓ backup → {dest}'",
                dest = dest
            );
            common::run("pct", &["exec", &vmid, "--", "bash", "-c", &script]);
        }
        Cmd::Route { vmid, domain } => {
            println!("Traefik 라우트 등록: {} → LXC {}", domain, vmid);
            // Get LXC IP
            common::run("bash", &["-c", &format!(
                "IP=$(pct exec {} -- hostname -I | tr -d ' ') && \
                 cat > /opt/traefik/dynamic/wp-{}.yml << EOF\n\
http:\n  routers:\n    wp-{d}:\n      rule: Host(\\`{d}\\`)\n      entryPoints: [websecure]\n      service: wp-{d}\n      tls:\n        certResolver: cloudflare\n  services:\n    wp-{d}:\n      loadBalancer:\n        servers:\n          - url: http://$IP:80\nEOF\n\
echo '✓ {d} → '$IP",
                vmid, domain, d = domain
            )]);
        }
        Cmd::UpdatePlugins { vmid } => {
            common::run("pct", &["exec", &vmid, "--", "bash", "-c",
                "cd /var/www/html && wp --allow-root plugin update --all"]);
        }
        Cmd::PhpVersion { vmid, version } => {
            let script = format!(
                "apt-get install -y php{v}-fpm php{v}-mysql php{v}-xml php{v}-mbstring php{v}-curl php{v}-zip php{v}-gd && \
                 systemctl disable --now php*-fpm; systemctl enable --now php{v}-fpm && \
                 sed -i 's|php.*-fpm.sock|php{v}-fpm.sock|' /etc/nginx/sites-available/default && \
                 nginx -t && systemctl reload nginx && echo '✓ PHP {v}'",
                v = version
            );
            common::run("pct", &["exec", &vmid, "--", "bash", "-c", &script]);
        }
        Cmd::Destroy { vmid, force } => {
            if !force {
                println!("⚠ LXC {} 삭제하려면 --force 플래그를 추가하세요", vmid);
                return Ok(());
            }
            common::run("pct", &["stop", &vmid]);
            common::run("pct", &["destroy", &vmid]);
            println!("✓ LXC {} 삭제됨", vmid);
        }
    }
    Ok(())
}
