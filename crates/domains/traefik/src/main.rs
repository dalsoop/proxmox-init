//! prelik-traefik — Traefik v3 관리.
//! compose + env_file 기반 (phs의 recreate 패턴 이식).

use clap::{Parser, Subcommand};
use prelik_core::common;
use std::fs;

#[derive(Parser)]
#[command(name = "prelik-traefik", about = "Traefik 리버스 프록시 관리")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 컨테이너 재생성 (compose + CF env 자동 주입)
    Recreate {
        /// 대상 LXC VMID
        #[arg(long, default_value = "100")] // LINT_ALLOW: 관례상 Traefik LXC
        vmid: String,
    },
    /// 라우트 추가
    RouteAdd {
        #[arg(long)]
        vmid: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        domain: String,
        /// 백엔드 URL (예: http://10.0.50.181:80)
        #[arg(long)]
        backend: String,
        /// CF certResolver 사용 여부 (와일드카드 cert 필요시)
        #[arg(long)]
        use_cf: bool,
    },
    /// 라우트 목록
    RouteList {
        #[arg(long)]
        vmid: String,
    },
    /// 라우트 제거
    RouteRemove {
        #[arg(long)]
        vmid: String,
        #[arg(long)]
        name: String,
    },
    Doctor,
}

const COMPOSE_YML: &str = "services:
  traefik:
    image: traefik:v3.4
    container_name: traefik
    restart: unless-stopped
    ports:
      - \"80:80\"
      - \"443:443\"
      - \"8080:8080\"
    env_file:
      - /opt/traefik/.env
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock:ro
      - /opt/traefik/traefik.yml:/traefik.yml:ro
      - /opt/traefik/dynamic:/dynamic:ro
      - /opt/traefik/acme:/acme:rw
";

fn main() -> anyhow::Result<()> {
    if !common::has_cmd("pct") {
        anyhow::bail!("pct 없음 — Proxmox 호스트에서만 동작");
    }
    match Cli::parse().cmd {
        Cmd::Recreate { vmid } => recreate(&vmid),
        Cmd::RouteAdd { vmid, name, domain, backend, use_cf } => route_add(&vmid, &name, &domain, &backend, use_cf),
        Cmd::RouteList { vmid } => route_list(&vmid),
        Cmd::RouteRemove { vmid, name } => route_remove(&vmid, &name),
        Cmd::Doctor => { doctor(); Ok(()) }
    }
}

fn recreate(vmid: &str) -> anyhow::Result<()> {
    println!("=== Traefik 재생성 (compose + env_file) ===");

    let cf_email = read_host_env("CLOUDFLARE_EMAIL");
    let cf_key = read_host_env("CLOUDFLARE_API_KEY");
    if cf_email.is_empty() || cf_key.is_empty() {
        anyhow::bail!("/etc/prelik/.env 에 CLOUDFLARE_EMAIL / CLOUDFLARE_API_KEY 필요");
    }

    let env_content = format!(
        "CLOUDFLARE_EMAIL={cf_email}\nCLOUDFLARE_API_KEY={cf_key}\nCF_API_EMAIL={cf_email}\nCF_API_KEY={cf_key}\n"
    );
    write_to_lxc(vmid, "/opt/traefik/.env", &env_content)?;
    common::run("pct", &["exec", vmid, "--", "chmod", "600", "/opt/traefik/.env"])?;
    write_to_lxc(vmid, "/opt/traefik/docker-compose.yml", COMPOSE_YML)?;

    common::run("pct", &[
        "exec", vmid, "--", "bash", "-c",
        "mkdir -p /opt/traefik/acme /opt/traefik/dynamic && touch /opt/traefik/acme/acme.json && chmod 600 /opt/traefik/acme/acme.json"
    ])?;

    let out = common::run("pct", &[
        "exec", vmid, "--", "bash", "-c",
        "docker rm -f traefik 2>/dev/null; cd /opt/traefik && docker compose up -d 2>&1"
    ])?;
    println!("{out}");
    println!("✓ Traefik 재생성 완료");
    Ok(())
}

fn route_add(vmid: &str, name: &str, domain: &str, backend: &str, use_cf: bool) -> anyhow::Result<()> {
    println!("=== 라우트 추가: {name} ({domain} → {backend}) ===");
    let tls_block = if use_cf {
        "      tls:\n        certResolver: cloudflare"
    } else {
        "      tls: {}"
    };
    let yml = format!("http:
  routers:
    {name}:
      rule: \"Host(`{domain}`)\"
      entryPoints:
        - websecure
      service: {name}
{tls_block}
    {name}-http:
      rule: \"Host(`{domain}`)\"
      entryPoints:
        - web
      service: {name}
  services:
    {name}:
      loadBalancer:
        servers:
          - url: \"{backend}\"
");
    write_to_lxc(vmid, &format!("/opt/traefik/dynamic/{name}.yml"), &yml)?;
    println!("✓ /opt/traefik/dynamic/{name}.yml 생성");
    Ok(())
}

fn route_list(vmid: &str) -> anyhow::Result<()> {
    let out = common::run("pct", &["exec", vmid, "--", "ls", "/opt/traefik/dynamic"])?;
    println!("{out}");
    Ok(())
}

fn route_remove(vmid: &str, name: &str) -> anyhow::Result<()> {
    common::run("pct", &["exec", vmid, "--", "rm", "-f", &format!("/opt/traefik/dynamic/{name}.yml")])?;
    println!("✓ 라우트 {name} 제거");
    Ok(())
}

fn read_host_env(key: &str) -> String {
    let paths = ["/etc/prelik/.env", "/etc/proxmox-host-setup/.env"];
    for p in paths {
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
    // mktemp: 소유자만 읽기 가능, race/심볼릭 링크 공격 회피
    let out = common::run("mktemp", &["-t", "prelik.XXXXXXXX"])?;
    let tmp = out.trim();
    let tmp_path = std::path::PathBuf::from(tmp);
    // 쓰기 실패해도 반드시 정리하도록 RAII 가드
    struct Cleanup<'a>(&'a std::path::Path);
    impl Drop for Cleanup<'_> { fn drop(&mut self) { let _ = fs::remove_file(self.0); } }
    let _g = Cleanup(&tmp_path);

    fs::write(&tmp_path, content)?;
    // 소유자-only 권한 고정
    common::run("chmod", &["600", tmp])?;
    common::run("pct", &["push", vmid, tmp, path])?;
    Ok(())
}

fn doctor() {
    println!("=== prelik-traefik doctor ===");
    println!("  pct:    {}", if common::has_cmd("pct") { "✓" } else { "✗" });
    let email = read_host_env("CLOUDFLARE_EMAIL");
    let key = read_host_env("CLOUDFLARE_API_KEY");
    println!("  CF_EMAIL:   {}", if !email.is_empty() { "✓" } else { "✗ (/etc/prelik/.env)" });
    println!("  CF_API_KEY: {}", if !key.is_empty() { "✓" } else { "✗" });
}
