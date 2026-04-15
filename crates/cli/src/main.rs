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
    /// 도메인 설치 (여러 개 공백 구분 가능, --preset으로 번들)
    Install {
        /// 도메인 이름 (공백 구분, 여러 개)
        domains: Vec<String>,
        /// 프리셋 이름 (web, mail, dev, minimal)
        #[arg(long)]
        preset: Option<String>,
    },
    /// 도메인 제거 (여러 개 가능)
    Remove {
        domains: Vec<String>,
    },
    /// 도메인 업데이트 (여러 개 가능)
    Update {
        domains: Vec<String>,
    },
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
        Cmd::Install { domains, preset } => install_many(domains, preset.as_deref()),
        Cmd::Remove { domains } => remove_many(&domains),
        Cmd::Update { domains } => install_many(domains, None),
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
    println!("\n프리셋 (--preset 으로 한 번에 설치):");
    println!("  web         웹 호스팅 (bootstrap, lxc, traefik, cloudflare)");
    println!("  mail        메일 스택 (bootstrap, lxc, mail, cloudflare, connect)");
    println!("  dev         개발 도구 (bootstrap, ai, connect)");
    println!("  minimal     필수 최소 (bootstrap)");
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

/// 프리셋 이름 → 도메인 리스트
fn resolve_preset(name: &str) -> Option<Vec<String>> {
    match name {
        "web" => Some(vec!["bootstrap", "lxc", "traefik", "cloudflare"]),
        "mail" => Some(vec!["bootstrap", "lxc", "mail", "cloudflare", "connect"]),
        "minimal" => Some(vec!["bootstrap"]),
        "dev" => Some(vec!["bootstrap", "ai", "connect"]),
        _ => None,
    }.map(|v| v.into_iter().map(String::from).collect())
}

fn install_many(mut domains: Vec<String>, preset: Option<&str>) -> anyhow::Result<()> {
    if let Some(p) = preset {
        let expanded = resolve_preset(p)
            .ok_or_else(|| anyhow::anyhow!("알 수 없는 프리셋: {p} (web/mail/dev/minimal)"))?;
        println!("=== 프리셋 '{p}' 설치: {} ===\n", expanded.join(", "));
        // preset 먼저, 그 뒤 명시적 domains (중복 제거)
        let mut all = expanded;
        for d in &domains {
            if !all.contains(d) {
                all.push(d.clone());
            }
        }
        domains = all;
    }
    if domains.is_empty() {
        anyhow::bail!("설치할 도메인 없음. 예: prelik install bootstrap lxc --preset mail");
    }

    let total = domains.len();
    let mut failed = vec![];
    let mut aborted_remaining: Vec<String> = vec![];
    let mut iter = domains.iter().enumerate().peekable();
    while let Some((i, d)) = iter.next() {
        println!("[{}/{total}] {d}", i + 1);
        if let Err(e) = install(d) {
            eprintln!("  ✗ {d}: {e}");
            failed.push(d.clone());
            // bootstrap이 첫 실패면 뒤 도메인은 의미 없음
            if d == "bootstrap" {
                eprintln!("\n⚠ bootstrap 실패 — 남은 도메인 설치 중단");
                while let Some((_, rest)) = iter.next() {
                    aborted_remaining.push(rest.clone());
                }
                break;
            }
        }
    }

    if !aborted_remaining.is_empty() {
        eprintln!("  중단된 대기 도메인: {}", aborted_remaining.join(", "));
    }
    if !failed.is_empty() {
        anyhow::bail!("{}개 도메인 설치 실패: {}", failed.len(), failed.join(", "));
    }
    Ok(())
}

fn remove_many(domains: &[String]) -> anyhow::Result<()> {
    if domains.is_empty() {
        anyhow::bail!("제거할 도메인 이름 필요");
    }
    for d in domains {
        if let Err(e) = remove(d) {
            eprintln!("  ✗ {d}: {e}");
        }
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
