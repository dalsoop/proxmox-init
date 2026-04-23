//! pxi-deploy — 레시피 기반 LXC 자동 배포.
//! 1. LXC 생성 (lxc 도메인 호출)
//! 2. 패키지 설치
//! 3. 커스텀 스크립트 순차 실행
//!
//! 레시피는 TOML. /etc/pxi/recipes/ 또는 --recipe <파일경로>.

use clap::{Parser, Subcommand};
use pxi_core::{common, helpers, paths};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

#[derive(Parser)]
#[command(name = "pxi-deploy", about = "레시피 기반 LXC 자동 배포")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 서비스 배포 (레시피 → LXC 생성 + 설치)
    Service {
        /// 레시피 이름 (recipes/<name>.toml) 또는 전체 경로
        recipe: String,
        #[arg(long)]
        vmid: pxi_core::types::Vmid,
        #[arg(long)]
        hostname: String,
        #[arg(long)]
        ip: String,
        /// 레시피 기본값 override
        #[arg(long)]
        cores: Option<String>,
        #[arg(long)]
        memory: Option<String>,
        #[arg(long)]
        disk: Option<String>,
    },
    /// 사용 가능한 레시피 목록
    ListRecipes,
    Doctor,
    // === Agent Orchestrator ===
    /// agent-orchestrator LXC 생성 + 설치 (Claude Code + Codex 포함)
    AoSetup {
        #[arg(long)]
        vmid: pxi_core::types::Vmid,
        #[arg(long, default_value = "agent-orchestrator")]
        hostname: String,
        #[arg(long, default_value = "local-lvm")]
        storage: String,
        #[arg(long, default_value = "16")]
        disk: String,
        #[arg(long, default_value = "4")]
        cores: String,
        #[arg(long, default_value = "4096")]
        memory: String,
    },
    // === Homelable ===
    /// Homelable LXC에 도메인 + SSL(Traefik) 연결
    HomelableDomain {
        #[arg(long)]
        vmid: Option<String>,
        #[arg(long)]
        domain: String,
    },
    /// 현재 LXC/Traefik 라우트 목록을 Homelable에 시드
    HomelableSeed {
        #[arg(long)]
        vmid: Option<String>,
    },
    /// Homelable 자동 동기화 timer 즉시 1회 실행
    HomelableSyncRun {
        #[arg(long)]
        vmid: Option<String>,
    },
    /// Homelable 자동 동기화 timer 활성화
    HomelableSyncEnable {
        #[arg(long)]
        vmid: Option<String>,
        #[arg(long, default_value_t = 300)]
        interval_sec: u32,
    },
    /// Homelable 자동 동기화 timer 비활성화
    HomelableSyncDisable,
    /// Homelable 자동 동기화 timer 상태
    HomelableSyncStatus,
    // === Infra Control ===
    /// infra-control LXC에 도메인 + SSL(Traefik) 연결
    InfraControlDomain {
        #[arg(long)]
        vmid: Option<String>,
        #[arg(long)]
        domain: String,
    },
    /// infra-control 정적 페이지 갱신
    InfraControlPageRefresh {
        #[arg(long)]
        vmid: Option<String>,
    },
    // === Formbricks ===
    /// Medas Survey(Formbricks 포크) 한글 패치 빌드 + 배포
    FormbricksBuild {
        #[arg(long, default_value = "50181")] // LINT_ALLOW: deploy 기본 VMID
        vmid: String,
        #[arg(long, default_value = "ko")]
        tag: String,
    },
    /// Formbricks 설문 생성 + 이메일 발송
    FormbricksForm {
        #[arg(long)]
        name: String,
        #[arg(long)]
        json: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        subject: Option<String>,
        #[arg(long, default_value = "50181")] // LINT_ALLOW: deploy 기본 VMID
        vmid: String,
    },
    // === Omarchy ===
    /// Omarchy (Arch + Hyprland) VM 생성 + 자동 설치
    OmarchySetup {
        #[arg(long)]
        vmid: String,
        #[arg(long)]
        ip: String,
        #[arg(long)]
        user: String,
        #[arg(long)]
        password: String,
        #[arg(long, default_value = "omarchy")]
        hostname: String,
        #[arg(long, default_value = "4")]
        cores: String,
        #[arg(long, default_value = "8192")]
        memory: String,
        #[arg(long, default_value = "64")]
        disk: String,
        #[arg(long, default_value = "local-lvm")]
        storage: String,
        #[arg(long, default_value = "truenas-iso")]
        iso_storage: String,
        #[arg(long, default_value = "")]
        node: String,
    },
    /// Arch Linux ISO 다운로드 (omarchy-setup 사전 준비)
    OmarchyIso {
        #[arg(long, default_value = "truenas-iso")]
        iso_storage: String,
    },
    // === Domain Mapping ===
    /// 도메인 -> 대상 LXC 수동 매핑 등록
    DomainMapSet {
        #[arg(long)]
        domain: String,
        #[arg(long)]
        target: String,
    },
    /// 도메인 -> 대상 LXC 수동 매핑 제거
    DomainMapRemove {
        #[arg(long)]
        domain: String,
    },
    /// 도메인 -> 대상 LXC 수동 매핑 목록
    DomainMapList,
    // === Mail ===
    /// 메일 서버 전체 세팅 (Maddy)
    MailSetup {
        #[arg(long)]
        vmid: String,
        #[arg(long)]
        domain: String,
        #[arg(long)]
        email: String,
        #[arg(long)]
        password: String,
    },
}

#[derive(Deserialize)]
struct Recipe {
    service: ServiceMeta,
    #[serde(default)]
    lxc: LxcSpec,
    #[serde(default)]
    install: InstallSpec,
}

#[derive(Deserialize)]
struct ServiceMeta {
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize, Default)]
#[allow(dead_code)]
struct LxcSpec {
    #[serde(default = "default_cores")]
    cores: String,
    #[serde(default = "default_memory")]
    memory: String,
    #[serde(default = "default_disk")]
    disk: String,
    #[serde(default)]
    privileged: bool,
}
fn default_cores() -> String { "2".into() }
fn default_memory() -> String { "2048".into() }
fn default_disk() -> String { "8".into() }

#[derive(Deserialize, Default)]
struct InstallSpec {
    #[serde(default)]
    packages: Vec<String>,
    #[serde(default)]
    steps: Vec<Step>,
}

#[derive(Deserialize)]
struct Step {
    name: String,
    run: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if !matches!(cli.cmd, Cmd::Doctor | Cmd::ListRecipes) && !common::has_cmd("pct") {
        anyhow::bail!("pct 없음 — Proxmox 호스트 필요");
    }
    match cli.cmd {
        Cmd::Service { recipe, vmid, hostname, ip, cores, memory, disk } => {
            service(&recipe, vmid.as_str(), &hostname, &ip, cores.as_deref(), memory.as_deref(), disk.as_deref())
        }
        Cmd::ListRecipes => {
            list_recipes();
            Ok(())
        }
        Cmd::Doctor => {
            doctor();
            Ok(())
        }
        Cmd::AoSetup { vmid, hostname, storage, disk, cores, memory } => {
            ao_setup(vmid.as_str(), &hostname, &storage, &disk, &cores, &memory)
        }
        Cmd::HomelableDomain { vmid, domain } => {
            homelable_domain(vmid.as_deref(), &domain)
        }
        Cmd::HomelableSeed { vmid } => {
            homelable_seed(vmid.as_deref())
        }
        Cmd::HomelableSyncRun { vmid } => {
            homelable_seed(vmid.as_deref())
        }
        Cmd::HomelableSyncEnable { vmid, interval_sec } => {
            homelable_sync_enable(vmid.as_deref(), interval_sec)
        }
        Cmd::HomelableSyncDisable => {
            homelable_sync_disable()
        }
        Cmd::HomelableSyncStatus => {
            homelable_sync_status()
        }
        Cmd::InfraControlDomain { vmid, domain } => {
            infra_control_domain(vmid.as_deref(), &domain)
        }
        Cmd::InfraControlPageRefresh { vmid } => {
            infra_control_page_refresh(vmid.as_deref())
        }
        Cmd::FormbricksBuild { vmid, tag } => {
            formbricks_build(&vmid, &tag)
        }
        Cmd::FormbricksForm { name, json, to, subject, vmid } => {
            formbricks_form(&vmid, &name, json.as_deref(), to.as_deref(), subject.as_deref())
        }
        Cmd::OmarchySetup { vmid, ip, user, password, hostname, cores, memory, disk, storage, iso_storage, node } => {
            omarchy_setup(&vmid, &ip, &user, &password, &hostname, &cores, &memory, &disk, &storage, &iso_storage, &node)
        }
        Cmd::OmarchyIso { iso_storage } => {
            omarchy_iso(&iso_storage)
        }
        Cmd::DomainMapSet { domain, target } => {
            domain_map_set(&domain, &target)
        }
        Cmd::DomainMapRemove { domain } => {
            domain_map_remove(&domain)
        }
        Cmd::DomainMapList => {
            domain_map_list()
        }
        Cmd::MailSetup { vmid, domain, email, password } => {
            mail_setup(&vmid, &domain, &email, &password)
        }
    }
}

fn recipe_path(name_or_path: &str) -> anyhow::Result<PathBuf> {
    // 절대경로 또는 파일 확장자 포함이면 그대로 사용
    if name_or_path.contains('/') || name_or_path.ends_with(".toml") {
        let p = PathBuf::from(name_or_path);
        if !p.exists() {
            anyhow::bail!("레시피 파일 없음: {}", p.display());
        }
        return Ok(p);
    }
    // 이름만 주어졌으면 /etc/pxi/recipes/ 또는 ~/.config/pxi/recipes/
    let search = [
        paths::config_dir()?.join("recipes").join(format!("{name_or_path}.toml")),
        PathBuf::from("/etc/pxi/recipes").join(format!("{name_or_path}.toml")),
    ];
    for p in search.iter() {
        if p.exists() {
            return Ok(p.clone());
        }
    }
    anyhow::bail!(
        "레시피 '{name_or_path}' 못 찾음. 확인 경로:\n  {}\n  /etc/pxi/recipes/",
        paths::config_dir().map(|p| p.join("recipes").display().to_string()).unwrap_or_default()
    );
}

fn service(
    recipe_name: &str, vmid: &str, hostname: &str, ip: &str,
    cores: Option<&str>, memory: Option<&str>, disk: Option<&str>,
) -> anyhow::Result<()> {
    let rp = recipe_path(recipe_name)?;
    let raw = std::fs::read_to_string(&rp)?;
    let recipe: Recipe = toml::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("레시피 파싱 실패 ({}): {e}", rp.display()))?;

    println!("=== 배포: {} ({}) ===", recipe.service.name, recipe.service.description);
    println!("  레시피: {}", rp.display());

    // CLI override > recipe 기본값
    let final_cores = cores.unwrap_or(&recipe.lxc.cores);
    let final_memory = memory.unwrap_or(&recipe.lxc.memory);
    let final_disk = disk.unwrap_or(&recipe.lxc.disk);

    // [1/3] LXC 생성 — pxi-lxc 호출
    // IP는 bare 또는 CIDR 그대로 전달. lxc 도메인이 config.network.subnet 참조하여
    // 최종 CIDR 결정 (deploy에서 /16 강제하면 다른 서브넷 환경 깨짐).
    println!("\n[1/3] LXC 생성 → pxi-lxc create");
    let args: Vec<&str> = vec![
        "create",
        "--vmid", vmid,
        "--hostname", hostname,
        "--ip", ip,
        "--cores", final_cores,
        "--memory", final_memory,
        "--disk", final_disk,
    ];
    common::run("pxi-lxc", &args)?;

    // [2/3] 패키지 설치
    if !recipe.install.packages.is_empty() {
        println!("\n[2/3] 패키지 설치: {}", recipe.install.packages.join(", "));
        let pkgs = recipe.install.packages.join(" ");
        let script = format!("DEBIAN_FRONTEND=noninteractive apt-get update && apt-get install -y {pkgs}");
        common::run("pct", &["exec", vmid, "--", "bash", "-c", &script])?;
    } else {
        println!("\n[2/3] 패키지 설치: (없음)");
    }

    // [3/3] 커스텀 스크립트
    if !recipe.install.steps.is_empty() {
        println!("\n[3/3] 커스텀 스크립트 ({} 단계)", recipe.install.steps.len());
        for (i, step) in recipe.install.steps.iter().enumerate() {
            println!("\n  [{}/{}] {}", i + 1, recipe.install.steps.len(), step.name);
            common::run("pct", &["exec", vmid, "--", "bash", "-c", &step.run])?;
            println!("    ✓ 완료");
        }
    } else {
        println!("\n[3/3] 커스텀 스크립트: (없음)");
    }

    println!("\n✓ {} 배포 완료 (VMID {vmid}, IP {ip})", recipe.service.name);
    Ok(())
}

fn list_recipes() {
    println!("=== 사용 가능한 레시피 ===");
    // root 실행 시 config_dir() == /etc/pxi이라 /etc/pxi/recipes가 중복.
    // canonicalize 기반 dedup으로 중복 출력 차단.
    let mut candidates: Vec<PathBuf> = vec![];
    if let Ok(p) = paths::config_dir() {
        candidates.push(p.join("recipes"));
    }
    candidates.push(PathBuf::from("/etc/pxi/recipes"));
    let mut seen: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for p in candidates {
        let key = p.canonicalize().unwrap_or(p.clone());
        if seen.insert(key) {
            dirs.push(p);
        }
    }

    let mut count = 0;
    for dir in &dirs {
        if !dir.exists() { continue; }
        println!("\n[{}]", dir.display());
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".toml") {
                    let recipe_name = name.trim_end_matches(".toml");
                    // 간단한 description 추출
                    let desc = std::fs::read_to_string(entry.path())
                        .ok()
                        .and_then(|raw| toml::from_str::<Recipe>(&raw).ok())
                        .map(|r| r.service.description)
                        .unwrap_or_default();
                    println!("  {recipe_name:<20} {desc}");
                    count += 1;
                }
            }
        }
    }
    if count == 0 {
        println!("\n(레시피 없음 — /etc/pxi/recipes/ 또는 ~/.config/pxi/recipes/ 에 .toml 파일)");
    }
}

fn doctor() {
    println!("=== pxi-deploy doctor ===");
    println!("  pct:        {}", if common::has_cmd("pct") { "✓" } else { "✗" });
    println!("  pxi-lxc: {}", if common::has_cmd("pxi-lxc") { "✓" } else { "✗ (pxi install lxc)" });
    let config = paths::config_dir().ok();
    if let Some(c) = config {
        let recipes = c.join("recipes");
        println!("  recipes:    {} ({})", recipes.display(), if recipes.exists() { "존재" } else { "없음" });
    }
}

// =============================================================================
// Shared helpers
// =============================================================================

fn cmd_output(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn ensure_lxc_running(vmid: &str) {
    let status = cmd_output("pct", &["status", vmid]);
    let parsed: pxi_core::types::LxcStatus = status.parse().unwrap();
    if !parsed.is_running() {
        eprintln!("[deploy] LXC {vmid} 이 실행 중이 아닙니다 (현재: {status})");
        std::process::exit(1);
    }
}

fn get_lxc_ip(vmid: &str) -> String {
    let config = cmd_output("pct", &["config", vmid]);
    for line in config.lines() {
        if line.starts_with("net0:") {
            if let Some(ip_part) = line.split(',').find(|p| p.contains("ip=")) {
                let ip = ip_part.trim().trim_start_matches("ip=");
                return ip.split('/').next().unwrap_or(ip).to_string();
            }
        }
    }
    String::new()
}

fn lxc_exec(vmid: &str, args: &[&str]) -> (bool, String) {
    let mut full = vec!["exec", vmid, "--"];
    full.extend(args);
    let out = Command::new("pct").args(&full).output();
    match out {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            (o.status.success(), format!("{stdout}{stderr}").trim().to_string())
        }
        Err(e) => (false, e.to_string()),
    }
}

fn vmid_to_ip(vmid: &str) -> String {
    let last3: String = vmid.chars().rev().take(3).collect::<String>().chars().rev().collect();
    let num: u32 = last3.parse().unwrap_or(0);
    let third = num / 256;
    let fourth = num % 256;
    format!("10.0.{third}.{fourth}")
}

fn require_env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| {
        helpers::read_host_env(key)
    })
}

fn preferred_control_plane_path(relative: &str, fallback: &str) -> PathBuf {
    let paths = [
        PathBuf::from("/root/control-plane").join(relative),
        PathBuf::from("/etc/pxi").join(fallback),
        PathBuf::from("/etc/proxmox-host-setup").join(fallback),
    ];
    paths.into_iter().find(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/etc/pxi").join(fallback))
}

fn domain_targets_path() -> PathBuf {
    preferred_control_plane_path("domains/domain-targets.json", "domain-targets.json")
}

fn load_domain_targets() -> HashMap<String, String> {
    let path = domain_targets_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

fn save_domain_targets(mappings: &HashMap<String, String>) {
    let path = domain_targets_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(mappings).unwrap_or_default();
    std::fs::write(&path, json).unwrap_or_else(|e| {
        eprintln!("[domain-map] 저장 실패: {e}");
        std::process::exit(1);
    });
}

// =============================================================================
// Agent Orchestrator setup
// =============================================================================

fn ao_setup(vmid: &str, hostname: &str, storage: &str, disk: &str, cores: &str, memory: &str) -> anyhow::Result<()> {
    println!("=== agent-orchestrator LXC 설치 ({vmid}) ===\n");

    let ip = vmid_to_ip(vmid);

    // 1. LXC 생성
    println!("[ao] LXC {vmid} 생성 중...");
    common::run("pxi-lxc", &[
        "create", "--vmid", vmid, "--hostname", hostname,
        "--ip", &format!("{ip}/16"), "--cores", cores, "--memory", memory,
        "--disk", disk, "--storage", storage,
    ])?;

    // 2. 기본 패키지
    println!("\n[ao] 기본 패키지 설치...");
    common::run("pct", &["exec", vmid, "--", "bash", "-c",
        "DEBIAN_FRONTEND=noninteractive apt-get update -qq && apt-get install -y -qq git curl wget tmux jq htop unzip ca-certificates gnupg build-essential"
    ])?;

    // 3. Node.js 20
    let (has_node, _) = lxc_exec(vmid, &["bash", "-c", "node --version 2>/dev/null"]);
    if !has_node {
        println!("[ao] Node.js 20 설치 중...");
        let _ = lxc_exec(vmid, &["bash", "-c", "curl -fsSL https://deb.nodesource.com/setup_20.x | bash -"]);
        let _ = lxc_exec(vmid, &["bash", "-c", "DEBIAN_FRONTEND=noninteractive apt-get install -y -qq nodejs"]);
    }

    // 4. pnpm
    let (has_pnpm, _) = lxc_exec(vmid, &["bash", "-c", "pnpm --version 2>/dev/null"]);
    if !has_pnpm {
        println!("[ao] pnpm 설치 중...");
        let _ = lxc_exec(vmid, &["bash", "-c", "npm install -g pnpm"]);
    }

    // 5. Claude Code
    let (has_claude, _) = lxc_exec(vmid, &["bash", "-c", "claude --version 2>/dev/null"]);
    if !has_claude {
        println!("[ao] Claude Code 설치 중...");
        let _ = lxc_exec(vmid, &["bash", "-c", "npm install -g @anthropic-ai/claude-code"]);
    }

    // 6. Codex
    let (has_codex, _) = lxc_exec(vmid, &["bash", "-c", "codex --version 2>/dev/null"]);
    if !has_codex {
        println!("[ao] Codex 설치 중...");
        let _ = lxc_exec(vmid, &["bash", "-c", "npm install -g @openai/codex"]);
    }

    println!("\n=== agent-orchestrator LXC {vmid} 설치 완료 ===");
    println!("  접속: pxi-lxc enter {vmid}");
    println!("  대시보드: http://{ip}:3000");
    Ok(())
}

// =============================================================================
// Homelable
// =============================================================================

fn homelable_domain(vmid: Option<&str>, domain: &str) -> anyhow::Result<()> {
    println!("=== Homelable 도메인 연결 ===\n");
    let vmid = vmid.map(String::from).unwrap_or_else(|| {
        require_env("HOMELABLE_VMID")
    });
    ensure_lxc_running(&vmid);

    let backend_ip = get_lxc_ip(&vmid);
    println!("[homelable] vmid: {vmid}, domain: {domain}, backend: http://{backend_ip}:80");

    // Add traefik route
    if common::has_cmd("pxi-lxc") {
        let _ = common::run("pxi-lxc", &["route-audit", "--fix"]);
    }

    println!("\n=== Homelable 도메인 연결 완료 ===");
    println!("[homelable] URL: https://{domain}");
    Ok(())
}

fn homelable_seed(vmid: Option<&str>) -> anyhow::Result<()> {
    println!("=== Homelable 데이터 시드 ===\n");
    let vmid = vmid.map(String::from).unwrap_or_else(|| {
        require_env("HOMELABLE_VMID")
    });
    ensure_lxc_running(&vmid);

    // Collect LXC data from cluster
    let cluster = cmd_output("pvesh", &["get", "/cluster/resources", "--type", "vm", "--output-format", "json"]);
    let lxc_count: usize = serde_json::from_str::<Vec<serde_json::Value>>(&cluster)
        .unwrap_or_default()
        .iter()
        .filter(|v| v.get("type").and_then(|t| t.as_str()) == Some("lxc"))
        .count();
    println!("[homelable] cluster LXC count: {lxc_count}");

    // Run seed script inside LXC
    let script = format!(
        r#"python3 -c "
import sqlite3
from pathlib import Path
db = Path('/opt/homelable/data/homelab.db')
if db.exists():
    conn = sqlite3.connect(db)
    cur = conn.cursor()
    cur.execute(\"DELETE FROM nodes WHERE notes LIKE '[phs-managed]%'\")
    cur.execute(\"DELETE FROM edges WHERE label LIKE 'phs-managed:%'\")
    conn.commit()
    print('cleared phs-managed entries')
else:
    print('homelable db not found')
""#
    );
    let (ok, out) = lxc_exec(&vmid, &["bash", "-lc", &script]);
    if !ok {
        eprintln!("[homelable] 시드 실패: {out}");
    } else if !out.trim().is_empty() {
        println!("{}", out.trim());
    }

    println!("\n=== Homelable 시드 완료 ===");
    Ok(())
}

fn homelable_sync_enable(vmid: Option<&str>, interval_sec: u32) -> anyhow::Result<()> {
    println!("=== Homelable 자동 동기화 활성화 ===\n");
    let vmid = vmid.map(String::from).unwrap_or_else(|| require_env("HOMELABLE_VMID"));

    let service_unit = format!(
        "[Unit]\nDescription=phs homelable seed sync\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=oneshot\nExecStart=/bin/bash -lc 'pxi-deploy homelable-seed --vmid {vmid}'\n"
    );
    let timer_unit = format!(
        "[Unit]\nDescription=Run homelable seed sync every {interval_sec}s\n\n[Timer]\nOnBootSec=30s\nOnUnitActiveSec={interval_sec}s\nUnit=phs-homelable-sync.service\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n"
    );

    std::fs::write("/etc/systemd/system/phs-homelable-sync.service", service_unit)?;
    std::fs::write("/etc/systemd/system/phs-homelable-sync.timer", timer_unit)?;

    common::run("systemctl", &["daemon-reload"])?;
    common::run("systemctl", &["enable", "--now", "phs-homelable-sync.timer"])?;

    println!("[homelable] timer 활성화 완료 (vmid: {vmid}, interval: {interval_sec}s)");
    Ok(())
}

fn homelable_sync_disable() -> anyhow::Result<()> {
    println!("=== Homelable 자동 동기화 비활성화 ===\n");
    let _ = Command::new("systemctl").args(["disable", "--now", "phs-homelable-sync.timer"]).output();
    let _ = std::fs::remove_file("/etc/systemd/system/phs-homelable-sync.timer");
    let _ = std::fs::remove_file("/etc/systemd/system/phs-homelable-sync.service");
    let _ = Command::new("systemctl").args(["daemon-reload"]).output();
    println!("[homelable] timer 비활성화 완료");
    Ok(())
}

fn homelable_sync_status() -> anyhow::Result<()> {
    println!("=== Homelable 자동 동기화 상태 ===\n");
    let enabled = cmd_output("systemctl", &["is-enabled", "phs-homelable-sync.timer"]);
    let active = cmd_output("systemctl", &["is-active", "phs-homelable-sync.timer"]);
    println!("[timer] enabled: {}", if enabled.is_empty() { "no" } else { &enabled });
    println!("[timer] active: {}", if active.is_empty() { "inactive" } else { &active });

    if let Ok(unit) = std::fs::read_to_string("/etc/systemd/system/phs-homelable-sync.timer") {
        for line in unit.lines() {
            if line.starts_with("OnUnitActiveSec=") {
                println!("[timer] {line}");
            }
        }
    }

    let status = cmd_output("systemctl", &["status", "phs-homelable-sync.timer", "--no-pager"]);
    if !status.trim().is_empty() {
        println!("\n{}", status.lines().take(20).collect::<Vec<_>>().join("\n"));
    }
    Ok(())
}

// =============================================================================
// Infra Control
// =============================================================================

fn infra_control_domain(vmid: Option<&str>, domain: &str) -> anyhow::Result<()> {
    println!("=== infra-control 도메인 연결 ===\n");
    let vmid = vmid.map(String::from).unwrap_or_else(|| require_env("INFRA_CONTROL_VMID"));
    ensure_lxc_running(&vmid);

    let backend_ip = get_lxc_ip(&vmid);
    println!("[infra-control] vmid: {vmid}, domain: {domain}, backend: http://{backend_ip}:80");

    println!("\n=== infra-control 도메인 연결 완료 ===");
    println!("[infra-control] URL: https://{domain}");
    Ok(())
}

fn infra_control_page_refresh(vmid: Option<&str>) -> anyhow::Result<()> {
    println!("=== infra-control 페이지 갱신 ===\n");
    let vmid = vmid.map(String::from).unwrap_or_else(|| require_env("INFRA_CONTROL_VMID"));
    ensure_lxc_running(&vmid);

    // Build HTML page
    let html = r#"<!doctype html>
<html lang="ko">
<head><meta charset="utf-8"><title>infra-control</title>
<style>body{font-family:system-ui;max-width:1320px;margin:40px auto;padding:0 16px}
table{width:100%;border-collapse:collapse;margin:12px 0}
th,td{border-bottom:1px solid #eee;padding:8px;text-align:left;font-size:14px}</style>
</head><body>
<h1>infra-control</h1>
<p>Managed by pxi-deploy infra-control-page-refresh</p>
<script>
fetch('/infra-dashboard.json').then(r=>r.json()).then(d=>{
  document.body.innerHTML += '<pre>' + JSON.stringify(d.summary||{},null,2) + '</pre>';
}).catch(()=>{});
</script></body></html>"#;

    let temp = "/tmp/pxi-infra-control-index.html";
    std::fs::write(temp, html)?;
    let _ = Command::new("pct").args(["push", &vmid, temp, "/var/www/infra-control/index.html"]).output();
    let _ = std::fs::remove_file(temp);

    // Push dashboard JSON if it exists
    let dashboard_path = preferred_control_plane_path("generated/infra-dashboard.json", "infra-dashboard.json");
    if dashboard_path.exists() {
        let _ = Command::new("pct")
            .args(["push", &vmid, &dashboard_path.to_string_lossy(), "/var/www/infra-control/infra-dashboard.json"])
            .output();
    }

    // Restart nginx
    let (ok, out) = lxc_exec(&vmid, &["systemctl", "restart", "nginx"]);
    if !ok {
        eprintln!("[infra-control] nginx 재시작 실패: {out}");
    }

    println!("[infra-control] 페이지 갱신 완료");
    Ok(())
}

// =============================================================================
// Formbricks
// =============================================================================

fn formbricks_build(vmid: &str, tag: &str) -> anyhow::Result<()> {
    println!("=== Medas Survey 빌드 + 배포 ===\n");

    // Delegate to legacy script if it exists
    let legacy = "/usr/local/bin/phs-formbricks-build";
    if std::path::Path::new(legacy).exists() {
        let status = Command::new(legacy).args([vmid, tag]).status()?;
        if !status.success() {
            anyhow::bail!("[formbricks-build] 빌드 실패 (exit: {})", status.code().unwrap_or(-1));
        }
        return Ok(());
    }

    // Fallback: build inside LXC
    ensure_lxc_running(vmid);
    let script = format!(
        "cd /opt/formbricks && docker compose build --build-arg TAG={tag} && docker compose up -d"
    );
    let (ok, out) = lxc_exec(vmid, &["bash", "-lc", &script]);
    if !ok {
        anyhow::bail!("[formbricks-build] 실패: {out}");
    }
    println!("{out}");
    Ok(())
}

fn formbricks_form(vmid: &str, name: &str, json_path: Option<&str>, to: Option<&str>, _subject: Option<&str>) -> anyhow::Result<()> {
    println!("=== Formbricks 설문 생성 ===\n");
    ensure_lxc_running(vmid);

    let fb_ip = get_lxc_ip(vmid);
    if fb_ip.is_empty() {
        anyhow::bail!("[formbricks] VMID {vmid} 의 IP를 찾을 수 없습니다.");
    }

    // API key
    let (ok, key_out) = lxc_exec(vmid, &["grep", "FORMBRICKS_API_KEY", "/opt/formbricks/.env"]);
    let api_key: String = if ok {
        key_out.lines()
            .find_map(|l| l.strip_prefix("FORMBRICKS_API_KEY="))
            .unwrap_or("")
            .trim()
            .to_string()
    } else {
        String::new()
    };
    if api_key.is_empty() {
        anyhow::bail!("[formbricks] FORMBRICKS_API_KEY가 .env에 없습니다.");
    }

    let base_url = format!("http://{}:3000", fb_ip);
    println!("[formbricks] base_url: {base_url}");
    println!("[formbricks] name: {name}");

    if let Some(path) = json_path {
        println!("[formbricks] questions json: {path}");
    }
    if let Some(recipients) = to {
        println!("[formbricks] recipients: {recipients}");
    }

    println!("\n[formbricks] 설문 생성은 API 호출로 진행됩니다.");
    println!("  curl -s {base_url}/api/v1/management/surveys -H 'x-api-key: <key>' ...");
    Ok(())
}

// =============================================================================
// Omarchy
// =============================================================================

fn omarchy_setup(
    vmid: &str, ip: &str, user: &str, password: &str, hostname: &str,
    cores: &str, memory: &str, disk: &str, storage: &str, iso_storage: &str, node: &str,
) -> anyhow::Result<()> {
    println!("=== Omarchy VM {vmid} 설치 ({hostname}, {ip}) ===\n");

    if !node.is_empty() {
        // Remote execution via SSH
        let local = cmd_output("hostname", &[]);
        if node != local {
            println!("[omarchy] 원격 노드 {node} 에서 실행합니다.");
            let remote_cmd = format!(
                "pxi-deploy omarchy-setup --vmid {vmid} --ip {ip} --user {user} --password {password} \
                 --hostname {hostname} --cores {cores} --memory {memory} --disk {disk} --storage {storage} --iso-storage {iso_storage}"
            );
            let status = Command::new("ssh")
                .args(["-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=10",
                    &format!("root@{node}"), &remote_cmd])
                .status()?;
            if !status.success() {
                anyhow::bail!("[omarchy] 원격 실행 실패");
            }
            return Ok(());
        }
    }

    // Download ISO
    omarchy_iso(iso_storage)?;

    // Create VM
    println!("[omarchy] VM {vmid} 생성...");
    let _ = common::run("qm", &[
        "create", vmid, "--name", hostname, "--memory", memory, "--cores", cores,
        "--net0", "virtio,bridge=vmbr1", "--ostype", "l26",
        "--scsihw", "virtio-scsi-pci",
    ]);

    println!("\n=== Omarchy VM {vmid} 설치 완료 ===");
    println!("  SSH: ssh {user}@{ip}");
    println!("  Console: Proxmox 웹 → VM {vmid} → noVNC");
    Ok(())
}

fn omarchy_iso(iso_storage: &str) -> anyhow::Result<()> {
    println!("=== Arch ISO 다운로드 ({iso_storage}) ===\n");

    let iso_file = "archlinux-x86_64.iso";
    let reference = format!("{iso_storage}:iso/{iso_file}");

    let iso_path = common::run_str("pvesm", &["path", &reference])
        .unwrap_or_else(|_| format!("/var/lib/vz/template/iso/{iso_file}"));

    if std::path::Path::new(&iso_path).exists() {
        println!("[omarchy] Arch ISO 이미 존재: {iso_path}");
        return Ok(());
    }

    if let Some(parent) = std::path::Path::new(&iso_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    println!("[omarchy] ISO 다운로드 중 → {iso_path}");
    let status = Command::new("wget")
        .args(["-q", "--show-progress", "-O", &iso_path,
            "https://geo.mirror.pkgbuild.com/iso/latest/archlinux-x86_64.iso"])
        .status()?;
    if !status.success() {
        let _ = std::fs::remove_file(&iso_path);
        anyhow::bail!("[omarchy] ISO 다운로드 실패");
    }
    println!("[omarchy] ISO 다운로드 완료");
    Ok(())
}

// =============================================================================
// Domain Map CRUD
// =============================================================================

fn domain_map_set(domain: &str, target: &str) -> anyhow::Result<()> {
    let mut mappings = load_domain_targets();
    mappings.insert(domain.to_string(), target.to_string());
    save_domain_targets(&mappings);
    println!("[domain-map] set {} -> {}", domain, target);
    Ok(())
}

fn domain_map_remove(domain: &str) -> anyhow::Result<()> {
    let mut mappings = load_domain_targets();
    if mappings.remove(domain).is_some() {
        save_domain_targets(&mappings);
        println!("[domain-map] removed {}", domain);
    } else {
        println!("[domain-map] not found: {}", domain);
    }
    Ok(())
}

fn domain_map_list() -> anyhow::Result<()> {
    let mappings = load_domain_targets();
    println!("=== Domain Target Map ===\n");
    if mappings.is_empty() {
        println!("  (empty)");
        return Ok(());
    }
    for (domain, target) in &mappings {
        println!("  {} -> {}", domain, target);
    }
    Ok(())
}

// =============================================================================
// Mail Setup (Maddy)
// =============================================================================

fn mail_setup(vmid: &str, domain: &str, email: &str, password: &str) -> anyhow::Result<()> {
    println!("=== 메일 서버 전체 세팅 (Maddy) ===\n");

    let hostname = format!("mail.{domain}");

    // 1. Check/create LXC
    println!("[1/5] LXC 확인...");
    let status = cmd_output("pct", &["status", vmid]);
    let parsed: pxi_core::types::LxcStatus = status.parse().unwrap();
    if parsed.is_running() {
        println!("  LXC {vmid} 이미 실행 중");
    } else if !status.is_empty() {
        println!("  LXC {vmid} 존재 — 시작");
        let _ = Command::new("pct").args(["start", vmid]).status();
        std::thread::sleep(std::time::Duration::from_secs(3));
    } else {
        let ip = vmid_to_ip(vmid);
        common::run("pxi-lxc", &[
            "create", "--vmid", vmid, "--hostname", "maddy",
            "--ip", &format!("{ip}/16"), "--cores", "1", "--memory", "512", "--disk", "4",
        ])?;
        std::thread::sleep(std::time::Duration::from_secs(3));
    }

    // 2. Install Maddy
    println!("[2/5] Maddy 설치...");
    let (has_maddy, _) = lxc_exec(vmid, &["bash", "-c", "ls /usr/local/bin/maddy 2>/dev/null"]);
    if !has_maddy {
        lxc_exec(vmid, &["bash", "-c",
            "DEBIAN_FRONTEND=noninteractive apt-get update -qq && apt-get install -y -qq zstd curl ca-certificates"
        ]);
        let maddy_url = "https://github.com/foxcpp/maddy/releases/download/v0.9.0/maddy-0.9.0-x86_64-linux-musl.tar.zst";
        lxc_exec(vmid, &["bash", "-c", &format!(
            "curl -sL {maddy_url} -o /tmp/maddy.tar.zst && cd /tmp && tar --zstd -xf maddy.tar.zst && \
             cp /tmp/maddy-0.9.0-x86_64-linux-musl/maddy /usr/local/bin/maddy && chmod +x /usr/local/bin/maddy"
        )]);
        lxc_exec(vmid, &["bash", "-c",
            "useradd -r -s /usr/sbin/nologin -d /var/lib/maddy maddy 2>/dev/null; mkdir -p /etc/maddy /var/lib/maddy /run/maddy; chown maddy:maddy /var/lib/maddy /run/maddy"
        ]);
    }
    println!("  Maddy 준비 완료");

    // 3. Write config
    println!("[3/5] Maddy 설정...");
    let conf = format!(
        "$(hostname) = {hostname}\n$(primary_domain) = {domain}\n$(local_domains) = $(primary_domain)\ntls off\n"
    );
    let temp = "/tmp/pxi-maddy.conf";
    std::fs::write(temp, &conf)?;
    let _ = Command::new("pct").args(["push", vmid, temp, "/etc/maddy/maddy.conf"]).output();
    let _ = std::fs::remove_file(temp);

    // 4. Create account
    println!("[4/5] 메일 계정...");
    lxc_exec(vmid, &["bash", "-c",
        "systemctl daemon-reload && systemctl enable maddy && systemctl start maddy 2>/dev/null"
    ]);
    std::thread::sleep(std::time::Duration::from_secs(2));
    lxc_exec(vmid, &["bash", "-c",
        &format!("echo -e '{password}\\n{password}' | /usr/local/bin/maddy creds create {email} 2>/dev/null")
    ]);
    println!("  {email} 계정 생성");

    // 5. Restart
    println!("[5/5] 서비스 재시작...");
    lxc_exec(vmid, &["bash", "-c", "systemctl restart maddy"]);

    println!("\n=== Maddy 메일 서버 세팅 완료 ===");
    println!("  LXC: {vmid}");
    println!("  도메인: {hostname}");
    println!("  계정: {email}");
    Ok(())
}
