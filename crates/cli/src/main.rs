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
    /// 인터랙티브 초기 세팅 (추천 — 첫 사용자용)
    Init,
    /// 비인터랙티브 경로만 생성
    Setup,
    /// 사용 가능한 도메인 목록
    Available,
    /// 현재 설치된 도메인
    List,
    /// 도메인 설치
    Install { domain: String },
    /// 도메인 제거
    Remove { domain: String },
    /// 도메인 업데이트 (install과 동일 동작 — 최신 릴리스 재다운로드)
    Update { domain: String },
    /// 전체 업그레이드 (미구현)
    Upgrade,
    /// 도메인 실행
    Run {
        domain: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// 상태 점검
    Doctor,
}

const REPO: &str = "dalsoop/prelik-init";

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Init => init(),
        Cmd::Setup => setup(),
        Cmd::Available => list_available(),
        Cmd::List => list_installed(),
        Cmd::Install { domain } => install(&domain),
        Cmd::Remove { domain } => remove(&domain),
        Cmd::Update { domain } => install(&domain),
        Cmd::Upgrade => {
            println!("(미구현) 전체 업그레이드 예정");
            Ok(())
        }
        Cmd::Run { domain, args } => run_domain(&domain, &args),
        Cmd::Doctor => {
            doctor();
            Ok(())
        }
    }
}

fn setup() -> anyhow::Result<()> {
    println!("=== prelik 초기 세팅 ===");
    let config = paths::config_dir()?;
    let data = paths::data_dir()?;
    let domains = paths::domains_dir()?;
    let bin = paths::bin_dir()?;
    std::fs::create_dir_all(&config)?;
    std::fs::create_dir_all(&data)?;
    std::fs::create_dir_all(&domains)?;
    std::fs::create_dir_all(&bin)?;
    println!("  config:  {}", config.display());
    println!("  data:    {}", data.display());
    println!("  domains: {}", domains.display());
    println!("  bin:     {}", bin.display());
    println!("\n다음 단계: prelik install bootstrap  또는  prelik init (인터랙티브)");
    Ok(())
}

fn init() -> anyhow::Result<()> {
    use std::io::{self, Write};
    println!("=== prelik 초기 세팅 (인터랙티브) ===\n");

    setup()?;

    let config = paths::config_dir()?;
    let env_path = config.join(".env");
    let cfg_path = config.join("config.toml");
    std::fs::create_dir_all(&config)?;

    let prompt = |q: &str, default: &str| -> io::Result<String> {
        print!("  {q}");
        if !default.is_empty() {
            print!(" [{default}]");
        }
        print!(": ");
        io::stdout().flush()?;
        let mut s = String::new();
        io::stdin().read_line(&mut s)?;
        let v = s.trim();
        Ok(if v.is_empty() { default.to_string() } else { v.to_string() })
    };

    println!("[1/3] Cloudflare 크리덴셜 (생략 가능)");
    let cf_email = prompt("CLOUDFLARE_EMAIL", "")?;
    let cf_key = if !cf_email.is_empty() {
        prompt("CLOUDFLARE_API_KEY (Global API Key)", "")?
    } else { String::new() };

    println!("\n[2/3] SMTP (발송 릴레이용, 생략 가능)");
    let smtp_user = prompt("SMTP_USER (예: devops@example.com)", "")?;
    let smtp_pass = if !smtp_user.is_empty() {
        prompt("SMTP_PASSWORD", "")?
    } else { String::new() };

    println!("\n[3/3] Proxmox 네트워크 (LXC 도메인 쓸 때 필요)");
    let detected_proxmox = os::is_proxmox();
    let bridge = prompt("network bridge", if detected_proxmox { "vmbr1" } else { "" })?;
    let gateway = prompt("기본 게이트웨이 (예: 10.0.50.1)", "")?;
    let subnet: u8 = prompt("subnet prefix", "16")?.parse().unwrap_or(16);

    // .env 작성
    let mut env_lines = vec![];
    if !cf_email.is_empty() {
        env_lines.push(format!("CLOUDFLARE_EMAIL={cf_email}"));
        env_lines.push(format!("CLOUDFLARE_API_KEY={cf_key}"));
    }
    if !smtp_user.is_empty() {
        env_lines.push(format!("SMTP_USER={smtp_user}"));
        env_lines.push(format!("SMTP_PASSWORD={smtp_pass}"));
    }
    if !env_lines.is_empty() {
        std::fs::write(&env_path, env_lines.join("\n") + "\n")?;
        common::run("chmod", &["600", &env_path.display().to_string()])?;
        println!("\n✓ {} 저장 (0600)", env_path.display());
    }

    // config.toml 작성
    let mut cfg = String::from("# prelik config — 자동 생성\n\n");
    if !bridge.is_empty() || !gateway.is_empty() {
        cfg.push_str("[network]\n");
        cfg.push_str(&format!("bridge = \"{bridge}\"\n"));
        cfg.push_str(&format!("gateway = \"{gateway}\"\n"));
        cfg.push_str(&format!("subnet = {subnet}\n"));
    }
    if !cfg.trim_end().ends_with("자동 생성") {
        std::fs::write(&cfg_path, cfg)?;
        println!("✓ {} 저장", cfg_path.display());
    }

    println!("\n=== 다음 단계 ===");
    println!("  prelik install bootstrap                   # 의존성");
    if detected_proxmox {
        println!("  prelik install lxc traefik mail cloudflare connect ai");
    }
    println!("  prelik doctor                              # 상태 점검");
    Ok(())
}

fn list_available() -> anyhow::Result<()> {
    let reg = prelik_core::registry::Registry::load()?;
    println!("사용 가능한 도메인:");
    for d in reg.available() {
        println!("  {:<12} {}", d.name, d.description);
    }
    let planned = reg.planned();
    if !planned.is_empty() {
        println!("\n예정(아직 미구현):");
        for d in planned {
            println!("  {:<12} {}", d.name, d.description);
        }
    }
    Ok(())
}

fn list_installed() -> anyhow::Result<()> {
    let dir = paths::domains_dir()?;
    if !dir.exists() {
        println!("(설치된 도메인 없음)");
        return Ok(());
    }
    let mut count = 0;
    println!("설치된 도메인:");
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        println!("  {}", entry.file_name().to_string_lossy());
        count += 1;
    }
    if count == 0 {
        println!("  (없음)");
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

    let dom_dir = paths::domains_dir()?.join(domain);
    std::fs::create_dir_all(&dom_dir)?;
    common::run(
        "tar",
        &[
            "-xzf",
            &dest_tar.display().to_string(),
            "-C",
            &dom_dir.display().to_string(),
        ],
    )?;
    // tarball 파일 정리
    let _ = std::fs::remove_file(&dest_tar);

    // 기대 바이너리 검증
    let bin_name = format!("prelik-{domain}");
    let bin_src = dom_dir.join(&bin_name);
    if !bin_src.exists() {
        // 실패 시 받은 디렉토리 정리 (부분 설치 금지)
        let _ = std::fs::remove_dir_all(&dom_dir);
        anyhow::bail!(
            "압축 해제 후 기대한 바이너리가 없음: {} \
             — tarball 레이아웃이 잘못되었을 수 있음",
            bin_src.display()
        );
    }

    let bin_dst = paths::bin_dir()?.join(&bin_name);
    std::fs::copy(&bin_src, &bin_dst)
        .map_err(|e| anyhow::anyhow!("바이너리 복사 실패 ({}): {}", bin_dst.display(), e))?;
    common::run("chmod", &["+x", &bin_dst.display().to_string()])?;
    println!("✓ {domain} 설치 완료 → {}", bin_dst.display());
    Ok(())
}

fn remove(domain: &str) -> anyhow::Result<()> {
    let dom_dir = paths::domains_dir()?.join(domain);
    let bin_dst = paths::bin_dir()?.join(format!("prelik-{domain}"));

    let mut removed_any = false;
    if dom_dir.exists() {
        std::fs::remove_dir_all(&dom_dir)?;
        removed_any = true;
    }
    if bin_dst.exists() {
        std::fs::remove_file(&bin_dst)?;
        removed_any = true;
    }
    if !removed_any {
        println!("(이미 제거됨 또는 설치된 적 없음: {domain})");
    } else {
        println!("✓ {domain} 제거 완료");
    }
    Ok(())
}

fn run_domain(domain: &str, args: &[String]) -> anyhow::Result<()> {
    let bin = paths::bin_dir()?.join(format!("prelik-{domain}"));
    if !bin.exists() {
        anyhow::bail!(
            "도메인 바이너리 없음: {} (prelik install {})",
            bin.display(),
            domain
        );
    }
    let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let status = std::process::Command::new(&bin).args(&args_str).status()?;
    std::process::exit(status.code().unwrap_or(1));
}

fn doctor() {
    println!("=== prelik doctor ===");
    println!("  OS: {:?}", os::Distro::detect());
    println!("  Proxmox: {}", os::is_proxmox());
    println!("  root: {}", paths::is_root());

    match paths::config_dir() {
        Ok(p) => println!("  config_dir: {}", p.display()),
        Err(e) => println!("  config_dir: ✗ {e}"),
    }
    match paths::bin_dir() {
        Ok(p) => println!("  bin_dir:    {}", p.display()),
        Err(e) => println!("  bin_dir:    ✗ {e}"),
    }

    println!(
        "  dotenvx:    {}",
        if prelik_core::dotenvx::is_installed() {
            "✓"
        } else {
            "✗ (prelik install bootstrap)"
        }
    );
    println!(
        "  curl:       {}",
        if common::has_cmd("curl") { "✓" } else { "✗" }
    );
    println!(
        "  systemctl:  {}",
        if common::has_cmd("systemctl") {
            "✓"
        } else {
            "✗"
        }
    );
}

fn detect_target() -> anyhow::Result<String> {
    let arch = common::run("uname", &["-m"])?;
    match arch.as_str() {
        "x86_64" => Ok("x86_64-linux".into()),
        "aarch64" | "arm64" => Ok("aarch64-linux".into()),
        other => anyhow::bail!("지원하지 않는 아키텍처: {other}"),
    }
}
