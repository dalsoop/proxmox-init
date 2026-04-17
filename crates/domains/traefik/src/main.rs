//! prelik-traefik -- Traefik v3 관리.
//! compose + env_file 기반 (phs의 recreate 패턴 이식).
//! 클러스터 멀티노드 지원 (SSH 원격 실행).

use clap::{Parser, Subcommand};
use prelik_core::common;
use std::collections::HashSet;
use std::fs;
use std::process::Command;
use std::{thread, time::Duration};

#[derive(Parser)]
#[command(name = "prelik-traefik", about = "Traefik 리버스 프록시 관리")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Traefik 초기 설치 (LXC 생성 + 바이너리 + systemd + DNS)
    Setup {
        /// 대상 LXC VMID
        #[arg(long)]
        vmid: String,
        /// Traefik LXC IP (자동: VMID에서 유추)
        #[arg(long)]
        ip: Option<String>,
        /// 도메인 (예: 50.internal.kr)
        #[arg(long)]
        domain: String,
        /// 원격 노드 (없으면 로컬)
        #[arg(long)]
        node: Option<String>,
    },
    /// 컨테이너 재생성 (compose + CF env 자동 주입)
    Recreate {
        /// 대상 LXC VMID
        #[arg(long, default_value = "100")]
        vmid: String,
    },
    /// 라우트 목록 (전체 노드 또는 특정 노드)
    List {
        /// 특정 노드만
        #[arg(long)]
        node: Option<String>,
    },
    /// 라우트 추가
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        domain: String,
        /// 백엔드 URL (예: http://10.0.50.181:80)
        #[arg(long)]
        backend: String,
        /// 대상 노드 (없으면 로컬)
        #[arg(long)]
        node: Option<String>,
    },
    /// 라우트 제거
    Remove {
        #[arg(long)]
        name: String,
        /// 대상 노드
        #[arg(long)]
        node: Option<String>,
    },
    /// 모든 라우트 일괄 재배포
    Resync {
        /// 대상 노드
        #[arg(long)]
        node: Option<String>,
    },
    /// drift 감지 (expected vs actual)
    Drift {
        /// 대상 노드
        #[arg(long)]
        node: Option<String>,
        /// 자동 정리
        #[arg(long)]
        fix: bool,
    },
    /// SSL 인증서 검증 (curl 폴링)
    CertVerify {
        #[arg(long)]
        domain: String,
        /// 타임아웃 (초)
        #[arg(long, default_value = "120")]
        timeout_sec: u64,
    },
    /// SSL 인증서 재확인 (retry-after 안내)
    CertRecheck {
        #[arg(long)]
        domain: String,
        /// Retry-After UTC 시각
        #[arg(long)]
        retry_after_utc: String,
        /// 타임아웃 (초)
        #[arg(long, default_value = "120")]
        timeout_sec: u64,
    },
    /// Cloudflare env sync + compose recreate
    CloudflareSync {
        #[arg(long)]
        vmid: String,
    },
    /// 상태 점검
    Doctor,
}

// =============================================================================
// Route JSON type
// =============================================================================

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Route {
    name: String,
    domain: String,
    backend: String,
}

// =============================================================================
// Compose template
// =============================================================================

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

// =============================================================================
// Main dispatch
// =============================================================================

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if !matches!(cli.cmd, Cmd::Doctor) && !common::has_cmd("pct") {
        anyhow::bail!("pct 없음 -- Proxmox 호스트에서만 동작");
    }
    match cli.cmd {
        Cmd::Setup { vmid, ip, domain, node } => {
            let ip = ip.unwrap_or_else(|| vmid_to_ip(&vmid));
            setup(&vmid, &ip, &domain, node.as_deref())
        }
        Cmd::Recreate { vmid } => recreate(&vmid),
        Cmd::List { node } => { list_routes(node.as_deref()); Ok(()) }
        Cmd::Add { name, domain, backend, node } => add_route(&name, &domain, &backend, node.as_deref()),
        Cmd::Remove { name, node } => { remove_route(&name, node.as_deref()); Ok(()) }
        Cmd::Resync { node } => { resync_routes(node.as_deref()); Ok(()) }
        Cmd::Drift { node, fix } => { drift_check(node.as_deref(), fix); Ok(()) }
        Cmd::CertVerify { domain, timeout_sec } => cert_verify(&domain, timeout_sec),
        Cmd::CertRecheck { domain, retry_after_utc, timeout_sec } => {
            cert_recheck(&domain, &retry_after_utc, timeout_sec);
            Ok(())
        }
        Cmd::CloudflareSync { vmid } => cloudflare_sync(&vmid),
        Cmd::Doctor => { doctor(); Ok(()) }
    }
}

// =============================================================================
// Core utilities
// =============================================================================

fn cmd_output(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn local_node_name() -> String {
    fs::read_to_string("/etc/hostname")
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn node_ip_from_name(node: &str) -> String {
    let output = cmd_output(
        "pvesh",
        &["get", &format!("/nodes/{node}/network"), "--output-format", "json"],
    );
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&output) {
        if let Some(ifaces) = parsed.as_array() {
            for iface in ifaces {
                if iface["iface"].as_str() == Some("vmbr0") {
                    if let Some(addr) = iface["address"].as_str() {
                        return addr.to_string();
                    }
                }
            }
        }
    }
    eprintln!("[traefik] 노드 '{node}' 의 IP를 찾을 수 없습니다.");
    std::process::exit(1);
}

fn lxc_exec_on(node: Option<&str>, vmid: &str, cmd: &[&str]) -> (bool, String) {
    let local = local_node_name();
    let is_local = node.is_none() || node == Some(&local);

    let output = if is_local {
        let mut args = vec!["exec", vmid, "--"];
        args.extend_from_slice(cmd);
        Command::new("pct").args(&args).output().expect("pct exec 실패")
    } else {
        let node = node.unwrap();
        let node_ip = node_ip_from_name(node);
        let pct_cmd = format!("pct exec {} -- {}", vmid, cmd.join(" "));
        Command::new("ssh")
            .args(["-o", "ConnectTimeout=10", "-o", "StrictHostKeyChecking=no", &format!("root@{node_ip}"), &pct_cmd])
            .output()
            .expect("ssh pct exec 실패")
    };
    let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let combined = if err.is_empty() { out } else { format!("{out}\n{err}") };
    (output.status.success(), combined)
}

fn lxc_exec_sh_on(node: Option<&str>, vmid: &str, cmd: &str) -> String {
    let (_, output) = lxc_exec_on(node, vmid, &["bash", "-lc", cmd]);
    output
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
    write_to_lxc_on(None, vmid, path, content)
}

fn write_to_lxc_on(node: Option<&str>, vmid: &str, path: &str, content: &str) -> anyhow::Result<()> {
    let local = local_node_name();
    let is_local = node.is_none() || node == Some(&local);

    let out = common::run("mktemp", &["-t", "prelik.XXXXXXXX"])?;
    let tmp = out.trim();
    let tmp_path = std::path::PathBuf::from(tmp);
    struct Cleanup<'a>(&'a std::path::Path);
    impl Drop for Cleanup<'_> { fn drop(&mut self) { let _ = fs::remove_file(self.0); } }
    let _g = Cleanup(&tmp_path);

    fs::write(&tmp_path, content)?;
    common::run("chmod", &["600", tmp])?;

    if is_local {
        common::run("pct", &["push", vmid, tmp, path])?;
    } else {
        let n = node.unwrap();
        let node_ip = node_ip_from_name(n);
        let remote_tmp = format!("/tmp/prelik-traefik-remote-{}", path.replace('/', "_"));
        let _ = Command::new("scp")
            .args(["-o", "ConnectTimeout=10", "-o", "StrictHostKeyChecking=no", tmp, &format!("root@{node_ip}:{remote_tmp}")])
            .status();
        let _ = Command::new("ssh")
            .args(["-o", "ConnectTimeout=10", "-o", "StrictHostKeyChecking=no", &format!("root@{node_ip}"), &format!("pct push {vmid} {remote_tmp} {path} && rm -f {remote_tmp}")])
            .status();
    }
    Ok(())
}

fn vmid_to_ip(vmid: &str) -> String {
    let n: u32 = vmid.parse().unwrap_or(100);
    let prefix = n / 1000;
    let suffix = n % 1000;
    format!("10.0.{prefix}.{suffix}")
}

fn resolve_node(node: Option<&str>) -> String {
    node.map(|n| n.to_string()).unwrap_or_else(|| local_node_name())
}

// =============================================================================
// Route file management
// =============================================================================

fn routes_file_path(node: &str) -> std::path::PathBuf {
    let local = local_node_name();
    let filename = if node == local {
        "traefik-routes.json".to_string()
    } else {
        format!("traefik-routes-{node}.json")
    };
    // Check /var/lib/prelik first, then /etc/prelik
    for dir in ["/var/lib/prelik", "/etc/prelik"] {
        let p = std::path::PathBuf::from(dir).join(&filename);
        if p.exists() { return p; }
    }
    // Default to /var/lib/prelik
    let dir = std::path::PathBuf::from("/var/lib/prelik");
    let _ = fs::create_dir_all(&dir);
    dir.join(filename)
}

fn load_routes_for_node(node: &str) -> Vec<Route> {
    let path = routes_file_path(node);
    if path.exists() {
        if let Ok(data) = fs::read_to_string(&path) {
            return serde_json::from_str(&data).unwrap_or_default();
        }
    }
    vec![]
}

fn save_routes_for_node(node: &str, routes: &[Route]) {
    let path = routes_file_path(node);
    if let Some(parent) = path.parent() { let _ = fs::create_dir_all(parent); }
    let data = serde_json::to_string_pretty(routes).unwrap_or_default();
    let _ = fs::write(path, data);
}

fn list_route_nodes() -> Vec<String> {
    let mut nodes = vec![];
    for dir in ["/var/lib/prelik", "/etc/prelik"] {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(node) = name.strip_prefix("traefik-routes-").and_then(|s| s.strip_suffix(".json")) {
                    nodes.push(node.to_string());
                }
            }
        }
    }
    nodes.sort();
    nodes.dedup();
    nodes
}

// =============================================================================
// Find Traefik LXC
// =============================================================================

fn find_traefik_vmid_on(node: Option<&str>) -> String {
    let local = local_node_name();
    let is_local = node.is_none() || node == Some(&local);

    if is_local {
        let output = cmd_output("pct", &["list"]);
        for line in output.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && parts[1] == "running" && parts.last().map_or(false, |n| *n == "traefik") {
                return parts[0].to_string();
            }
        }
    } else {
        let n = node.unwrap();
        let output = cmd_output("pvesh", &["get", &format!("/nodes/{n}/lxc"), "--output-format", "json"]);
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&output) {
            if let Some(lxcs) = parsed.as_array() {
                for lxc in lxcs {
                    let name = lxc["name"].as_str().unwrap_or("");
                    let status = lxc["status"].as_str().unwrap_or("");
                    if name == "traefik" && status == "running" {
                        if let Some(vmid) = lxc["vmid"].as_u64() {
                            return vmid.to_string();
                        }
                    }
                }
            }
        }
    }
    String::new()
}

// =============================================================================
// Setup (initial Traefik installation)
// =============================================================================

fn setup(vmid: &str, ip: &str, domain: &str, node: Option<&str>) -> anyhow::Result<()> {
    let target_node = node.unwrap_or("(local)");
    println!("=== Traefik 전체 세팅 (node: {target_node}) ===\n");

    let local = local_node_name();
    if let Some(n) = node {
        if n != local {
            validate_node(n)?;
            println!("[traefik] 노드 '{n}' 검증 완료");
        }
    }
    let is_remote = node.is_some() && node != Some(&local);

    // 1. LXC 생성
    println!("[1/4] LXC 생성...");
    let status = if is_remote {
        cmd_output("pvesh", &["get", &format!("/nodes/{}/lxc/{vmid}/status/current", node.unwrap()), "--output-format", "json"])
    } else {
        cmd_output("pct", &["status", vmid])
    };
    if status.contains("running") {
        println!("  LXC {vmid} 이미 실행 중 -- 건너뜀");
    } else if !status.is_empty() && !status.contains("does not exist") && !status.contains("no such") {
        println!("  LXC {vmid} 존재하지만 중지 상태 -- 시작");
        if is_remote {
            let _ = Command::new("pvesh")
                .args(["create", &format!("/nodes/{}/lxc/{vmid}/status/start", node.unwrap())])
                .status();
        } else {
            let _ = Command::new("pct").args(["start", vmid]).status();
        }
    } else {
        let template = find_template("debian-13");
        let password = std::env::var("LXC_ROOT_PASSWORD").unwrap_or_else(|_| "changeme".to_string());
        let prefix = ip.rsplitn(2, '.').last().expect("IP 형식 오류");
        let gw = format!("{prefix}.1");

        if is_remote {
            let n = node.unwrap();
            let node_ip = node_ip_from_name(n);
            let pct_cmd = format!(
                "pct create {vmid} local:vztmpl/{template} --hostname traefik --memory 1024 --cores 2 --rootfs local-lvm:8 --net0 'name=eth0,bridge=vmbr1,ip={ip}/16,gw={gw}' --password '{password}' --unprivileged 1 --features nesting=1 --start 1"
            );
            let result = Command::new("ssh")
                .args(["-o", "ConnectTimeout=30", "-o", "StrictHostKeyChecking=no", &format!("root@{node_ip}"), &pct_cmd])
                .status();
            if result.is_err() || !result.unwrap().success() {
                anyhow::bail!("원격 LXC 생성 실패 (node: {n})");
            }
        } else {
            let result = Command::new("pct")
                .args([
                    "create", vmid, &format!("local:vztmpl/{template}"),
                    "--hostname", "traefik", "--memory", "1024", "--cores", "2",
                    "--rootfs", "local-lvm:8",
                    "--net0", &format!("name=eth0,bridge=vmbr1,ip={ip}/16,gw={gw}"),
                    "--password", &password,
                    "--unprivileged", "1", "--features", "nesting=1", "--start", "1",
                ])
                .status();
            if result.is_err() || !result.unwrap().success() {
                anyhow::bail!("LXC 생성 실패");
            }
        }
        thread::sleep(Duration::from_secs(3));
        println!("  LXC {vmid} 생성 완료");
    }

    // 2. Traefik 설치
    println!("[2/4] Traefik 설치...");
    let has_traefik = lxc_exec_sh_on(node, vmid, "ls /usr/local/bin/traefik 2>/dev/null");
    if has_traefik.contains("traefik") {
        println!("  Traefik 이미 설치됨");
    } else if is_remote {
        install_traefik_binary_remote(node.unwrap(), vmid);
    } else {
        lxc_exec_sh_on(node, vmid,
            "apt-get update -qq && apt-get install -y -qq curl ca-certificates && \
             curl -sL https://github.com/traefik/traefik/releases/download/v3.4.0/traefik_v3.4.0_linux_amd64.tar.gz | tar xz -C /usr/local/bin/ traefik");
        let version = lxc_exec_sh_on(node, vmid, "/usr/local/bin/traefik version 2>&1 | head -1");
        println!("  {version}");
    }

    // Traefik config directories
    lxc_exec_sh_on(node, vmid, "mkdir -p /etc/traefik/conf.d /opt/traefik/dynamic");

    let static_config = "entryPoints:\n  web:\n    address: \":80\"\n  websecure:\n    address: \":443\"\n\nproviders:\n  file:\n    directory: /opt/traefik/dynamic\n    watch: true\n\napi:\n  dashboard: true\n  insecure: true\n";
    write_to_lxc_on(node, vmid, "/etc/traefik/traefik.yml", static_config)?;

    let service_unit = "[Unit]\nDescription=Traefik Reverse Proxy\nAfter=network.target\n\n[Service]\nExecStart=/usr/local/bin/traefik --configfile=/etc/traefik/traefik.yml\nRestart=on-failure\nRestartSec=5\n\n[Install]\nWantedBy=multi-user.target\n";
    write_to_lxc_on(node, vmid, "/etc/systemd/system/traefik.service", service_unit)?;
    lxc_exec_sh_on(node, vmid, "systemctl daemon-reload && systemctl enable traefik && systemctl start traefik");
    println!("  Traefik 서비스 시작 완료");

    // 3. DNS
    println!("[3/4] DNS 레코드...");
    println!("  수동 등록: prelik-dns add --domain {domain} --type A --name <subdomain> --content {ip}");

    // 4. SSL
    println!("[4/4] SSL 인증서...");
    println!("  수동 발급: prelik-ssl issue --domain {domain}");

    println!("\n=== Traefik 세팅 완료 ===");
    println!("  LXC: {vmid}");
    println!("  IP: {ip}");
    println!("  도메인: {domain}");
    println!("  대시보드: http://{ip}:8080/dashboard/");
    Ok(())
}

fn find_template(prefix: &str) -> String {
    let output = cmd_output("pveam", &["list", "local"]);
    for line in output.lines() {
        if line.contains(prefix) && line.contains(".tar") {
            if let Some(name) = line.split_whitespace().next() {
                return name.replace("local:vztmpl/", "");
            }
        }
    }
    format!("{prefix}-standard_13.1-2_amd64.tar.zst")
}

fn install_traefik_binary_remote(node: &str, vmid: &str) {
    let traefik_url = "https://github.com/traefik/traefik/releases/download/v3.4.0/traefik_v3.4.0_linux_amd64.tar.gz";
    let host_binary = "/tmp/prelik-traefik-binary";
    let node_ip = node_ip_from_name(node);

    if !std::path::Path::new(host_binary).exists() {
        println!("  호스트에서 Traefik 다운로드 중...");
        let _ = Command::new("bash")
            .args(["-c", &format!("curl -sL {traefik_url} | tar xz -C /tmp/ traefik && mv /tmp/traefik {host_binary}")])
            .status();
    }

    let remote_tmp = "/tmp/traefik-binary";
    let _ = Command::new("scp")
        .args(["-o", "ConnectTimeout=10", "-o", "StrictHostKeyChecking=no", host_binary, &format!("root@{node_ip}:{remote_tmp}")])
        .status();
    let push_ok = Command::new("ssh")
        .args(["-o", "ConnectTimeout=10", "-o", "StrictHostKeyChecking=no", &format!("root@{node_ip}"),
            &format!("pct push {vmid} {remote_tmp} /usr/local/bin/traefik --perms 755")])
        .status().map_or(false, |s| s.success());
    if !push_ok {
        let _ = Command::new("ssh")
            .args(["-o", "ConnectTimeout=10", "-o", "StrictHostKeyChecking=no", &format!("root@{node_ip}"),
                &format!("pct push {vmid} {remote_tmp} /usr/local/bin/traefik")])
            .status();
        lxc_exec_sh_on(Some(node), vmid, "chmod 755 /usr/local/bin/traefik");
    }
    let _ = Command::new("ssh")
        .args(["-o", "ConnectTimeout=10", "-o", "StrictHostKeyChecking=no", &format!("root@{node_ip}"), &format!("rm -f {remote_tmp}")])
        .status();
    let version = lxc_exec_sh_on(Some(node), vmid, "/usr/local/bin/traefik version 2>&1 | head -1");
    println!("  {version}");
}

fn validate_node(node: &str) -> anyhow::Result<()> {
    let nodes_json = cmd_output("pvesh", &["get", "/nodes", "--output-format", "json"]);
    let node_exists = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&nodes_json) {
        parsed.as_array().map_or(false, |arr| arr.iter().any(|n| n["node"].as_str() == Some(node)))
    } else {
        anyhow::bail!("Proxmox API에서 노드 목록을 가져올 수 없습니다");
    };
    if !node_exists { anyhow::bail!("노드 '{node}' 가 클러스터에 존재하지 않습니다"); }

    let node_online = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&nodes_json) {
        parsed.as_array().map_or(false, |arr| {
            arr.iter().any(|n| n["node"].as_str() == Some(node) && n["status"].as_str() == Some("online"))
        })
    } else { false };
    if !node_online { anyhow::bail!("노드 '{node}' 가 오프라인입니다"); }

    let node_ip = node_ip_from_name(node);
    let ssh_check = Command::new("ssh")
        .args(["-o", "ConnectTimeout=5", "-o", "StrictHostKeyChecking=no", "-o", "BatchMode=yes", &format!("root@{node_ip}"), "echo ok"])
        .output();
    match ssh_check {
        Ok(out) if out.status.success() => {}
        _ => anyhow::bail!("노드 '{node}' ({node_ip}) 에 SSH 접근이 불가합니다"),
    }
    Ok(())
}

// =============================================================================
// Recreate (compose + env_file)
// =============================================================================

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
    println!("  Traefik 재생성 완료");
    Ok(())
}

// =============================================================================
// Route CRUD
// =============================================================================

fn add_route(name: &str, domain: &str, backend: &str, node: Option<&str>) -> anyhow::Result<()> {
    // Validation
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        anyhow::bail!("라우트 이름은 [A-Za-z0-9-]만 허용: {name:?}");
    }
    if domain.is_empty() || !domain.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        anyhow::bail!("도메인은 [A-Za-z0-9.-]만 허용: {domain:?}");
    }
    if backend.contains('\n') || backend.contains('\r') || backend.contains('"') {
        anyhow::bail!("backend URL에 개행/따옴표 포함: {backend:?}");
    }
    // Reject IP-as-domain
    if domain.chars().next().map_or(false, |c| c.is_ascii_digit())
        && domain.contains('.')
        && domain.chars().all(|c| c.is_ascii_digit() || c == '.')
    {
        anyhow::bail!("IP 주소({domain})는 도메인으로 사용할 수 없습니다. FQDN을 사용하세요.");
    }
    if !domain.contains('.') {
        anyhow::bail!("'{domain}'은 유효한 도메인이 아닙니다. FQDN을 사용하세요.");
    }

    let resolved_node = resolve_node(node);
    let mut routes = load_routes_for_node(&resolved_node);

    if routes.iter().any(|r| r.name == name) {
        println!("[traefik] 라우트 '{name}' 이미 존재합니다. 업데이트합니다.");
        routes.retain(|r| r.name != name);
    }

    routes.push(Route {
        name: name.to_string(),
        domain: domain.to_string(),
        backend: backend.to_string(),
    });
    save_routes_for_node(&resolved_node, &routes);
    println!("[traefik] 라우트 추가: {name} -> {domain} -> {backend} (node: {resolved_node})");

    // Sync to Traefik LXC
    let traefik_vmid = find_traefik_vmid_on(node);
    if !traefik_vmid.is_empty() {
        sync_routes_direct_on(node, &traefik_vmid, &routes);
    }
    Ok(())
}

fn remove_route(name: &str, node: Option<&str>) {
    let resolved_node = resolve_node(node);
    let mut routes = load_routes_for_node(&resolved_node);
    let before = routes.len();
    routes.retain(|r| r.name != name);

    if routes.len() == before {
        // Search all nodes if not found
        if node.is_none() {
            for n in list_route_nodes() {
                let mut nr = load_routes_for_node(&n);
                let nb = nr.len();
                nr.retain(|r| r.name != name);
                if nr.len() < nb {
                    save_routes_for_node(&n, &nr);
                    println!("[traefik] 라우트 제거: {name} (node: {n})");
                    let traefik_vmid = find_traefik_vmid_on(Some(&n));
                    if !traefik_vmid.is_empty() {
                        // Remove yml file from Traefik
                        let _ = lxc_exec_on(Some(&n), &traefik_vmid, &["bash", "-lc", &format!("rm -f /opt/traefik/dynamic/{name}.yml")]);
                    }
                    return;
                }
            }
        }
        eprintln!("[traefik] 라우트 '{name}' 을 찾을 수 없습니다.");
        return;
    }

    save_routes_for_node(&resolved_node, &routes);
    println!("[traefik] 라우트 제거: {name} (node: {resolved_node})");

    let traefik_vmid = find_traefik_vmid_on(node);
    if !traefik_vmid.is_empty() {
        let _ = lxc_exec_on(node, &traefik_vmid, &["bash", "-lc", &format!("rm -f /opt/traefik/dynamic/{name}.yml")]);
    }
}

fn list_routes(node: Option<&str>) {
    if let Some(n) = node {
        let routes = load_routes_for_node(n);
        print_routes_table(n, &routes);
    } else {
        let local = local_node_name();
        let local_routes = load_routes_for_node(&local);
        let mut found = false;
        if !local_routes.is_empty() {
            print_routes_table(&local, &local_routes);
            found = true;
        }
        for n in list_route_nodes() {
            if n != local {
                let routes = load_routes_for_node(&n);
                if !routes.is_empty() {
                    print_routes_table(&n, &routes);
                    found = true;
                }
            }
        }
        if !found {
            println!("[traefik] 등록된 라우트가 없습니다.");
        }
    }
}

fn print_routes_table(node: &str, routes: &[Route]) {
    println!("=== Traefik 라우트 [{node}] ===\n");
    println!("  {:<20} {:<35} {}", "NAME", "DOMAIN", "BACKEND");
    println!("  {}", "-".repeat(75));
    for r in routes {
        println!("  {:<20} {:<35} {}", r.name, r.domain, r.backend);
    }
    println!("\n  총 {}개 라우트\n", routes.len());
}

fn resync_routes(node: Option<&str>) {
    let resolved_node = resolve_node(node);
    println!("=== Traefik 라우트 일괄 재배포 (node: {resolved_node}) ===\n");

    let routes = load_routes_for_node(&resolved_node);
    if routes.is_empty() {
        println!("  등록된 라우트 없음");
        return;
    }

    println!("  {}개 라우트 재배포 중...", routes.len());
    for r in &routes {
        println!("    - {} -> {}", r.name, r.domain);
    }

    let traefik_vmid = find_traefik_vmid_on(node);
    if traefik_vmid.is_empty() {
        eprintln!("\n  Traefik LXC를 찾을 수 없음 (node: {resolved_node})");
        std::process::exit(1);
    }

    sync_routes_direct_on(node, &traefik_vmid, &routes);
    println!("\n  {}개 라우트 재배포 완료 (HTTP+HTTPS)", routes.len());
}

fn drift_check(node: Option<&str>, fix: bool) {
    let resolved_node = resolve_node(node);
    println!("=== Traefik drift check (node: {resolved_node}) ===\n");

    let traefik_vmid = find_traefik_vmid_on(node);
    if traefik_vmid.is_empty() {
        eprintln!("  Traefik LXC를 찾을 수 없음");
        std::process::exit(1);
    }

    // 1. Traefik dynamic yml 목록
    let (_, ls_out) = lxc_exec_on(node, &traefik_vmid, &["bash", "-lc",
        "ls /opt/traefik/dynamic/*.yml 2>/dev/null | xargs -n1 basename | sed 's/.yml$//'"]);
    let traefik_routes: HashSet<String> = ls_out.lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // 2. 정본 라우트 목록
    let phs_routes: HashSet<String> = load_routes_for_node(&resolved_node)
        .iter().map(|r| r.name.clone()).collect();

    // 3. drift 계산
    let unknown: Vec<String> = traefik_routes.difference(&phs_routes).cloned().collect();
    let missing: Vec<String> = phs_routes.difference(&traefik_routes).cloned().collect();

    if unknown.is_empty() && missing.is_empty() {
        println!("  drift 없음 -- Traefik {}개 / 정본 {}개 일치", traefik_routes.len(), phs_routes.len());
        return;
    }

    if !unknown.is_empty() {
        println!("  Traefik에만 있음 (정본 미등록 외부 yml):");
        for name in &unknown { println!("       - {name}"); }
    }
    if !missing.is_empty() {
        println!("  정본에만 있음 (Traefik에 미배포):");
        for name in &missing { println!("       - {name}"); }
    }

    if !fix {
        println!("\n  자동 정리: prelik-traefik drift --fix");
        return;
    }

    println!("\n[fix] 외부 yml {}개 제거 중...", unknown.len());
    for name in &unknown {
        let (ok, _) = lxc_exec_on(node, &traefik_vmid, &["bash", "-lc", &format!("rm -f /opt/traefik/dynamic/{name}.yml && echo OK")]);
        if ok { println!("       - {name}.yml 제거"); }
    }

    if !missing.is_empty() {
        println!("\n[fix] 정본 라우트 {}개 재배포 중...", missing.len());
        let routes = load_routes_for_node(&resolved_node);
        sync_routes_direct_on(node, &traefik_vmid, &routes);
    }

    println!("\n  drift 정리 완료");
}

/// 라우트 yml을 Traefik LXC에 직접 배포 (HTTP+HTTPS 양쪽)
fn sync_routes_direct_on(node: Option<&str>, vmid: &str, routes: &[Route]) {
    for route in routes {
        let yml = format!(
            r#"http:
  routers:
    {name}:
      rule: "Host(`{domain}`)"
      entryPoints:
        - websecure
      service: {name}
      tls:
        certResolver: cloudflare
    {name}-http:
      rule: "Host(`{domain}`)"
      entryPoints:
        - web
      service: {name}
  services:
    {name}:
      loadBalancer:
        servers:
          - url: "{backend}"
"#,
            name = route.name,
            domain = route.domain,
            backend = route.backend,
        );
        let path = format!("/opt/traefik/dynamic/{}.yml", route.name);
        let _ = write_to_lxc_on(node, vmid, &path, &yml);
    }
    let node_label = node.unwrap_or("local");
    println!("[traefik] {} 라우트 Traefik LXC({}, node:{}) 에 배포 (HTTP+HTTPS)", routes.len(), vmid, node_label);
}

// =============================================================================
// Cert verify / recheck
// =============================================================================

fn cert_verify(domain: &str, timeout_sec: u64) -> anyhow::Result<()> {
    println!("=== Traefik Cert Verify ===\n");
    println!("[traefik] domain: {domain}");

    let mut last_err = String::new();
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < timeout_sec {
        let output = Command::new("curl")
            .args(["-I", "--max-time", "10", &format!("https://{domain}")])
            .output()?;

        if output.status.success() {
            println!("[traefik] 인증서 검증 성공");
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().take(10) { println!("{line}"); }
            return Ok(());
        }

        last_err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        thread::sleep(Duration::from_secs(5));
    }

    anyhow::bail!("인증서 검증 실패: {last_err}");
}

fn cert_recheck(domain: &str, retry_after_utc: &str, timeout_sec: u64) {
    println!("=== Traefik Cert Recheck ===\n");
    println!("[traefik] domain: {domain}");
    println!("[traefik] retry-after-utc: {retry_after_utc}");
    println!("[traefik] 지정 시각 이후 재실행하세요:");
    let local_traefik = find_traefik_vmid_on(None);
    let hint = if local_traefik.is_empty() { "100".to_string() } else { local_traefik };
    println!("  prelik-traefik cloudflare-sync --vmid {hint}");
    println!("  prelik-traefik cert-verify --domain {domain} --timeout-sec {timeout_sec}");
}

// =============================================================================
// Cloudflare Sync
// =============================================================================

fn cloudflare_sync(vmid: &str) -> anyhow::Result<()> {
    println!("=== Traefik Cloudflare Env Sync ===\n");

    // Check running
    let status = cmd_output("pct", &["status", vmid]);
    if !status.contains("running") {
        anyhow::bail!("LXC {vmid} 이 실행 중이 아닙니다 (현재: {status})");
    }

    // Read CF credentials from env vars or host .env
    let cf_key = std::env::var("CLOUDFLARE_API_KEY").ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| read_host_env("CLOUDFLARE_API_KEY"));
    let cf_email = std::env::var("CLOUDFLARE_EMAIL").ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| read_host_env("CLOUDFLARE_EMAIL"));

    if cf_key.is_empty() || cf_email.is_empty() {
        anyhow::bail!("CLOUDFLARE_API_KEY / CLOUDFLARE_EMAIL 미설정 (환경변수 또는 /etc/prelik/.env)");
    }

    let env_content = format!(
        "CF_API_EMAIL={cf_email}\nCF_API_KEY={cf_key}\nCLOUDFLARE_EMAIL={cf_email}\nCLOUDFLARE_API_KEY={cf_key}\n"
    );
    write_to_lxc(vmid, "/opt/traefik/.env", &env_content)?;

    common::run("pct", &["exec", vmid, "--", "bash", "-c",
        "mkdir -p /opt/traefik/acme && touch /opt/traefik/acme/acme.json && chmod 600 /opt/traefik/.env /opt/traefik/acme/acme.json"
    ])?;

    // Compose recreate
    let out = common::run("pct", &["exec", vmid, "--", "bash", "-c",
        r#"set -e
if docker compose version >/dev/null 2>&1; then
  docker rm -f traefik >/dev/null 2>&1 || true
  cd /opt/traefik && docker compose up -d --force-recreate traefik
elif command -v docker-compose >/dev/null 2>&1; then
  docker rm -f traefik >/dev/null 2>&1 || true
  cd /opt/traefik && docker-compose up -d --force-recreate traefik
else
  IMAGE=$(awk '/image:/ {print $2; exit}' /opt/traefik/docker-compose.yml 2>/dev/null || true)
  [ -n "$IMAGE" ] || IMAGE=traefik:v3.0
  docker rm -f traefik >/dev/null 2>&1 || true
  docker run -d --name traefik --restart unless-stopped \
    -p 80:80 -p 443:443 -p 8080:8080 \
    --env-file /opt/traefik/.env \
    -v /var/run/docker.sock:/var/run/docker.sock:ro \
    -v /opt/traefik/traefik.yml:/traefik.yml:ro \
    -v /opt/traefik/dynamic:/dynamic:ro \
    -v /opt/traefik/acme:/acme \
    "$IMAGE" traefik
fi"#
    ])?;
    if !out.trim().is_empty() { println!("{}", out.trim()); }
    println!("[traefik] Cloudflare env 반영 완료");
    Ok(())
}

// =============================================================================
// Doctor
// =============================================================================

fn doctor() {
    println!("=== prelik-traefik doctor ===");
    println!("  pct:        {}", if common::has_cmd("pct") { "ok" } else { "missing" });
    let email = read_host_env("CLOUDFLARE_EMAIL");
    let key = read_host_env("CLOUDFLARE_API_KEY");
    println!("  CF_EMAIL:   {}", if !email.is_empty() { "ok" } else { "missing (/etc/prelik/.env)" });
    println!("  CF_API_KEY: {}", if !key.is_empty() { "ok" } else { "missing" });
    let local_traefik = find_traefik_vmid_on(None);
    println!("  traefik:    {}", if !local_traefik.is_empty() { format!("ok (VMID {})", local_traefik) } else { "not found".to_string() });
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vmid_to_ip() {
        assert_eq!(vmid_to_ip("50100"), "10.0.50.100");
        assert_eq!(vmid_to_ip("60105"), "10.0.60.105");
        assert_eq!(vmid_to_ip("100"), "10.0.0.100");
    }

    #[test]
    fn test_route_yml_has_both_entrypoints() {
        let route = Route {
            name: "comfyui".into(),
            domain: "comfyui.60.internal.kr".into(),
            backend: "http://10.0.60.105:8188".into(),
        };
        let yml = format!(
            r#"http:
  routers:
    {name}:
      rule: "Host(`{domain}`)"
      entryPoints:
        - websecure
      service: {name}
      tls:
        certResolver: cloudflare
    {name}-http:
      rule: "Host(`{domain}`)"
      entryPoints:
        - web
      service: {name}
  services:
    {name}:
      loadBalancer:
        servers:
          - url: "{backend}"
"#,
            name = route.name, domain = route.domain, backend = route.backend,
        );
        assert!(yml.contains("- websecure"), "HTTPS entryPoint 필수");
        assert!(yml.contains("certResolver: cloudflare"), "TLS resolver 필수");
        assert!(yml.contains("- web"), "HTTP entryPoint 필수");
        assert!(yml.contains("comfyui-http:"), "HTTP 라우터 이름 필수");
    }

    #[test]
    fn test_route_name_validation() {
        assert!("my-route".chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        assert!(!"my route".chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        assert!(!"my;route".chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    #[test]
    fn test_domain_validation() {
        // IP addresses rejected
        let domain = "10.0.60.105";
        let is_ip = domain.chars().next().map_or(false, |c| c.is_ascii_digit())
            && domain.contains('.')
            && domain.chars().all(|c| c.is_ascii_digit() || c == '.');
        assert!(is_ip);

        // Valid domains pass
        let domain = "comfyui.60.internal.kr";
        let is_ip2 = domain.chars().next().map_or(false, |c| c.is_ascii_digit())
            && domain.contains('.')
            && domain.chars().all(|c| c.is_ascii_digit() || c == '.');
        assert!(!is_ip2);
    }

    #[test]
    fn test_route_roundtrip() {
        let routes = vec![
            Route { name: "gitlab".into(), domain: "gitlab.50.internal.kr".into(), backend: "http://10.0.50.101:443".into() },
            Route { name: "comfyui".into(), domain: "comfyui.60.internal.kr".into(), backend: "http://10.0.60.105:8188".into() },
        ];
        let json = serde_json::to_string(&routes).unwrap();
        let parsed: Vec<Route> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "gitlab");
        assert_eq!(parsed[1].backend, "http://10.0.60.105:8188");
    }

    #[test]
    fn test_resolve_node_with_explicit() {
        assert_eq!(resolve_node(Some("ranode-3960x")), "ranode-3960x");
    }
}
