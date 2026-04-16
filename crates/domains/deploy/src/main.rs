//! prelik-deploy — 레시피 기반 LXC 자동 배포.
//! 1. LXC 생성 (lxc 도메인 호출)
//! 2. 패키지 설치
//! 3. 커스텀 스크립트 순차 실행
//!
//! 레시피는 TOML. /etc/prelik/recipes/ 또는 --recipe <파일경로>.

use clap::{Parser, Subcommand};
use prelik_core::{common, paths};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "prelik-deploy", about = "레시피 기반 LXC 자동 배포")]
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
        vmid: String,
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
            service(&recipe, &vmid, &hostname, &ip, cores.as_deref(), memory.as_deref(), disk.as_deref())
        }
        Cmd::ListRecipes => {
            list_recipes();
            Ok(())
        }
        Cmd::Doctor => {
            doctor();
            Ok(())
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
    // 이름만 주어졌으면 /etc/prelik/recipes/ 또는 ~/.config/prelik/recipes/
    let search = [
        paths::config_dir()?.join("recipes").join(format!("{name_or_path}.toml")),
        PathBuf::from("/etc/prelik/recipes").join(format!("{name_or_path}.toml")),
    ];
    for p in search.iter() {
        if p.exists() {
            return Ok(p.clone());
        }
    }
    anyhow::bail!(
        "레시피 '{name_or_path}' 못 찾음. 확인 경로:\n  {}\n  /etc/prelik/recipes/",
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

    // [1/3] LXC 생성 — prelik-lxc 호출
    // IP는 bare 또는 CIDR 그대로 전달. lxc 도메인이 config.network.subnet 참조하여
    // 최종 CIDR 결정 (deploy에서 /16 강제하면 다른 서브넷 환경 깨짐).
    println!("\n[1/3] LXC 생성 → prelik-lxc create");
    let args: Vec<&str> = vec![
        "create",
        "--vmid", vmid,
        "--hostname", hostname,
        "--ip", ip,
        "--cores", final_cores,
        "--memory", final_memory,
        "--disk", final_disk,
    ];
    common::run("prelik-lxc", &args)?;

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
    // root 실행 시 config_dir() == /etc/prelik이라 /etc/prelik/recipes가 중복.
    // canonicalize 기반 dedup으로 중복 출력 차단.
    let mut candidates: Vec<PathBuf> = vec![];
    if let Ok(p) = paths::config_dir() {
        candidates.push(p.join("recipes"));
    }
    candidates.push(PathBuf::from("/etc/prelik/recipes"));
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
        println!("\n(레시피 없음 — /etc/prelik/recipes/ 또는 ~/.config/prelik/recipes/ 에 .toml 파일)");
    }
}

fn doctor() {
    println!("=== prelik-deploy doctor ===");
    println!("  pct:        {}", if common::has_cmd("pct") { "✓" } else { "✗" });
    println!("  prelik-lxc: {}", if common::has_cmd("prelik-lxc") { "✓" } else { "✗ (prelik install lxc)" });
    let config = paths::config_dir().ok();
    if let Some(c) = config {
        let recipes = c.join("recipes");
        println!("  recipes:    {} ({})", recipes.display(), if recipes.exists() { "존재" } else { "없음" });
    }
}
