//! pxi-service — 파일 기반 서비스 레지스트리.
//!
//! /opt/services/{domain}/{name}/service.toml 유무로 서비스 관리.
//! Traefik 라우트 자동 생성/삭제.

use clap::{Parser, Subcommand};
use pxi_core::common;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SERVICES_DIR: &str = "/opt/services";

#[derive(Parser)]
#[command(name = "pxi-service", about = "서비스 레지스트리 (파일 기반)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 등록된 서비스 목록 (도메인별 그룹)
    List,
    /// Traefik 동기화 (service.toml → Traefik yml)
    Sync,
    /// 서비스 추가 (service.toml 생성 + Traefik 동기화)
    Add {
        /// 도메인 (예: pxi.com, 50.internal.kr)
        #[arg(long)]
        domain: String,
        /// 서비스 이름
        #[arg(long)]
        name: String,
        /// 전체 호스트명 (예: blog.pxi.com)
        #[arg(long)]
        host: String,
        /// 백엔드 IP
        #[arg(long)]
        ip: String,
        /// 백엔드 포트
        #[arg(long)]
        port: u16,
        /// LXC VMID (선택)
        #[arg(long)]
        vmid: Option<String>,
    },
    /// 서비스 제거 (폴더 삭제 + Traefik 동기화)
    Remove {
        /// 서비스 이름
        name: String,
        /// 강제 삭제
        #[arg(long)]
        force: bool,
    },
    /// Traefik과 불일치 감지
    Diff,
    /// 서비스 정보 조회
    Info {
        /// 서비스 이름
        name: String,
    },
    /// 환경 진단
    Doctor,
    /// 서비스 도메인 이동
    Move {
        /// 서비스 이름
        name: String,
        /// 새 도메인 그룹
        #[arg(long)]
        to: String,
    },
}

fn find_service(name: &str) -> Option<PathBuf> {
    let base = Path::new(SERVICES_DIR);
    if let Ok(domains) = fs::read_dir(base) {
        for domain in domains.flatten() {
            let svc = domain.path().join(name).join("service.toml");
            if svc.exists() {
                return Some(domain.path().join(name));
            }
        }
    }
    None
}

fn toml_get(path: &Path, key: &str) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with(key))
        .map(|l| {
            l.split('=')
                .nth(1)
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string()
        })
        .unwrap_or_default()
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::List => {
            common::run("service-sync", &["--list"]);
        }
        Cmd::Sync => {
            common::run("service-sync", &[]);
        }
        Cmd::Diff => {
            common::run("service-sync", &["--diff"]);
        }
        Cmd::Add {
            domain,
            name,
            host,
            ip,
            port,
            vmid,
        } => {
            let dir = format!("{}/{}/{}", SERVICES_DIR, domain, name);
            fs::create_dir_all(&dir)?;
            let vmid_line = vmid
                .as_ref()
                .map(|v| format!("vmid = \"{}\"\n", v))
                .unwrap_or_default();
            let content = format!(
                r#"[service]
name = "{name}"
domain = "{host}"
ip = "{ip}"
port = {port}
{vmid_line}
[traefik]
entrypoint = "websecure"
cert_resolver = "cloudflare"
"#,
            );
            fs::write(format!("{}/service.toml", dir), content)?;
            println!("✓ {}/{} 생성", domain, name);

            // Auto-sync Traefik
            common::run("service-sync", &[]);
        }
        Cmd::Remove { name, force } => {
            if let Some(path) = find_service(&name) {
                if !force {
                    println!(
                        "⚠ {} 삭제하려면 --force 추가 (경로: {})",
                        name,
                        path.display()
                    );
                    return Ok(());
                }
                fs::remove_dir_all(&path)?;
                println!("✓ {} 삭제됨", name);
                common::run("service-sync", &[]);
            } else {
                println!("✗ {} 서비스를 찾을 수 없음", name);
            }
        }
        Cmd::Info { name } => {
            if let Some(path) = find_service(&name) {
                let toml = path.join("service.toml");
                let domain_group = path
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                println!("서비스:  {}", name);
                println!("그룹:    {}", domain_group);
                println!("도메인:  {}", toml_get(&toml, "domain"));
                println!("IP:      {}", toml_get(&toml, "ip"));
                println!("포트:    {}", toml_get(&toml, "port"));
                let vmid = toml_get(&toml, "vmid");
                if !vmid.is_empty() {
                    println!("LXC:     {}", vmid);
                }
                println!("경로:    {}", path.display());

                // Show extra files in the service dir
                let entries: Vec<_> = fs::read_dir(&path)?
                    .flatten()
                    .filter(|e| e.file_name() != "service.toml")
                    .collect();
                if !entries.is_empty() {
                    println!("파일:");
                    for e in entries {
                        println!("  {}", e.file_name().to_string_lossy());
                    }
                }
            } else {
                println!("✗ {} 서비스를 찾을 수 없음", name);
            }
        }
        Cmd::Doctor => {
            doctor();
        }
        Cmd::Move { name, to } => {
            if let Some(old_path) = find_service(&name) {
                let new_dir = format!("{}/{}", SERVICES_DIR, to);
                fs::create_dir_all(&new_dir)?;
                let new_path = format!("{}/{}", new_dir, name);
                fs::rename(&old_path, &new_path)?;
                println!("✓ {} → {}/{}", name, to, name);
                common::run("service-sync", &[]);
            } else {
                println!("✗ {} 서비스를 찾을 수 없음", name);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// doctor
// ---------------------------------------------------------------------------

fn doctor() {
    println!("=== pxi-service doctor ===\n");

    // /opt/services/ directory exists
    let dir_ok = Path::new(SERVICES_DIR).is_dir();
    println!(
        "  {} {} 디렉토리",
        if dir_ok { "✓" } else { "✗" },
        SERVICES_DIR
    );

    // Count service.toml files
    if dir_ok {
        let mut count = 0usize;
        if let Ok(domains) = fs::read_dir(SERVICES_DIR) {
            for domain in domains.flatten() {
                if domain.path().is_dir() {
                    if let Ok(services) = fs::read_dir(domain.path()) {
                        for svc in services.flatten() {
                            if svc.path().join("service.toml").exists() {
                                count += 1;
                            }
                        }
                    }
                }
            }
        }
        println!("  ✓ service.toml 파일: {}개", count);
    }

    // service-sync binary exists in PATH
    let sync_ok = common::command_exists("service-sync");
    println!(
        "  {} service-sync 바이너리",
        if sync_ok { "✓" } else { "✗" }
    );

    // Traefik LXC reachable (50100 is the standard Traefik LXC)
    let traefik_ok = Command::new("pct")
        .args(["status", "50100"])
        .output()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).contains("running"))
        .unwrap_or(false);
    println!(
        "  {} Traefik LXC (50100)",
        if traefik_ok { "✓" } else { "✗" }
    );
}
