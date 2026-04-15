use clap::{Parser, Subcommand};
use prelik_core::{common, github, os, paths};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "prelik", version, about = "Proxmox/LXC 도메인 기반 설치형 CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 초기 세팅 (경로 생성, dotenvx 설치, 자동 업데이트 등록)
    Setup,
    /// 사용 가능한 도메인 목록
    Available,
    /// 현재 설치된 도메인
    List,
    /// 도메인 설치
    Install { domain: String },
    /// 도메인 제거
    Remove { domain: String },
    /// 도메인 업데이트
    Update { domain: String },
    /// 전체 업그레이드 (prelik 자체 + 모든 도메인)
    Upgrade,
    /// 도메인 실행 (`prelik run lxc list`)
    Run {
        domain: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// 상태 점검 (경로, 권한, 의존성)
    Doctor,
}

const REPO: &str = "dalsoop/prelik-init";

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Setup => setup(),
        Cmd::Available => list_available(),
        Cmd::List => list_installed(),
        Cmd::Install { domain } => install(&domain),
        Cmd::Remove { domain } => remove(&domain),
        Cmd::Update { domain } => install(&domain),
        Cmd::Upgrade => upgrade(),
        Cmd::Run { domain, args } => run_domain(&domain, &args),
        Cmd::Doctor => doctor(),
    }
}

fn setup() -> anyhow::Result<()> {
    println!("=== prelik 초기 세팅 ===");
    std::fs::create_dir_all(paths::config_dir())?;
    std::fs::create_dir_all(paths::data_dir())?;
    std::fs::create_dir_all(paths::domains_dir())?;
    std::fs::create_dir_all(paths::bin_dir())?;
    println!("  config:  {}", paths::config_dir().display());
    println!("  data:    {}", paths::data_dir().display());
    println!("  domains: {}", paths::domains_dir().display());
    println!("  bin:     {}", paths::bin_dir().display());
    println!("\n다음 단계: prelik install bootstrap");
    Ok(())
}

fn list_available() -> anyhow::Result<()> {
    // Nickel 레지스트리를 나중에 추가. 지금은 하드코드.
    let domains = [
        ("bootstrap", "apt/rust/gh/dotenvx 의존성 설치"),
        ("connect", "외부 서비스 연결 관리 (.env + dotenvx)"),
        ("lxc", "LXC 수명 관리 (Proxmox pct 래퍼)"),
        ("traefik", "Traefik 리버스 프록시"),
        ("mail", "Maddy + Mailpit + Postfix relay 번들"),
        ("cloudflare", "CF DNS/Email Routing/Worker"),
        ("ai", "Claude/Codex + 플러그인"),
    ];
    println!("사용 가능한 도메인:");
    for (name, desc) in &domains {
        println!("  {name:<12} {desc}");
    }
    Ok(())
}

fn list_installed() -> anyhow::Result<()> {
    let dir = paths::domains_dir();
    if !dir.exists() {
        println!("(설치된 도메인 없음)");
        return Ok(());
    }
    println!("설치된 도메인:");
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        println!("  {}", entry.file_name().to_string_lossy());
    }
    Ok(())
}

fn install(domain: &str) -> anyhow::Result<()> {
    println!("=== {domain} 설치 ===");
    let arch = detect_target()?;
    let tag = github::latest_tag(REPO)?;
    let asset = format!("prelik-{domain}-{arch}.tar.gz");
    let dest_tar = PathBuf::from("/tmp").join(&asset);
    println!("  버전: {tag}");
    println!("  에셋: {asset}");

    github::download_asset(REPO, &tag, &asset, &dest_tar)?;

    let dom_dir = paths::domains_dir().join(domain);
    std::fs::create_dir_all(&dom_dir)?;
    common::run(
        "tar",
        &["-xzf", &dest_tar.display().to_string(), "-C", &dom_dir.display().to_string()],
    )?;

    // 바이너리를 PATH에
    let bin_src = dom_dir.join(format!("prelik-{domain}"));
    let bin_dst = paths::bin_dir().join(format!("prelik-{domain}"));
    if bin_src.exists() {
        std::fs::copy(&bin_src, &bin_dst)?;
        common::run("chmod", &["+x", &bin_dst.display().to_string()])?;
    }
    println!("✓ {domain} 설치 완료");
    Ok(())
}

fn remove(domain: &str) -> anyhow::Result<()> {
    let dom_dir = paths::domains_dir().join(domain);
    let bin_dst = paths::bin_dir().join(format!("prelik-{domain}"));
    if dom_dir.exists() {
        std::fs::remove_dir_all(&dom_dir)?;
    }
    if bin_dst.exists() {
        std::fs::remove_file(&bin_dst)?;
    }
    println!("✓ {domain} 제거 완료");
    Ok(())
}

fn upgrade() -> anyhow::Result<()> {
    println!("(미구현) 전체 업그레이드 예정");
    Ok(())
}

fn run_domain(domain: &str, args: &[String]) -> anyhow::Result<()> {
    let bin = paths::bin_dir().join(format!("prelik-{domain}"));
    if !bin.exists() {
        anyhow::bail!("도메인 바이너리 없음: {} (prelik install {})", bin.display(), domain);
    }
    let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let status = std::process::Command::new(&bin).args(&args_str).status()?;
    std::process::exit(status.code().unwrap_or(1));
}

fn doctor() -> anyhow::Result<()> {
    println!("=== prelik doctor ===");
    println!("  OS: {:?}", os::Distro::detect());
    println!("  Proxmox: {}", os::is_proxmox());
    println!("  root: {}", paths::is_root());
    println!("  config_dir: {}", paths::config_dir().display());
    println!("  bin_dir:    {}", paths::bin_dir().display());
    println!("  dotenvx:    {}", if prelik_core::dotenvx::is_installed() { "✓" } else { "✗ (prelik install bootstrap)" });
    println!("  curl:       {}", if common::has_cmd("curl") { "✓" } else { "✗" });
    println!("  systemctl:  {}", if common::has_cmd("systemctl") { "✓" } else { "✗" });
    Ok(())
}

fn detect_target() -> anyhow::Result<String> {
    let arch = common::run("uname", &["-m"])?;
    match arch.as_str() {
        "x86_64" => Ok("x86_64-linux".into()),
        "aarch64" | "arm64" => Ok("aarch64-linux".into()),
        other => anyhow::bail!("지원하지 않는 아키텍처: {other}"),
    }
}
