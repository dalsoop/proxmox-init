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
    /// prelik 자체 제거 (반드시 docs/uninstall.md 먼저 읽을 것 — 많은 시스템 변경을 남김)
    Uninstall {
        /// 실제로 제거. 생략하면 dry-run만 실행.
        #[arg(long)]
        confirm: bool,
        /// config/recovery/audit 디렉토리까지 삭제 (~/.config/prelik, /etc/prelik, /var/lib/prelik).
        /// .env.vault 같은 암호화된 시크릿 포함 — 복구 불가.
        #[arg(long)]
        purge: bool,
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
        Cmd::Uninstall { confirm, purge } => uninstall(confirm, purge),
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

    // 동시 install 차단 — flock으로 프로세스 간 배타.
    // 같은 도메인을 병행 설치 시 한쪽이 쓴 파일을 다른 쪽이 덮어쓰는 경쟁 방지.
    let lock_path = paths::data_dir()?.join(".install.lock");
    std::fs::create_dir_all(lock_path.parent().unwrap())?;
    let _lock_file = std::fs::OpenOptions::new()
        .create(true).write(true).truncate(false)
        .open(&lock_path)?;
    use std::os::unix::io::AsRawFd;
    let fd = _lock_file.as_raw_fd();
    unsafe {
        extern "C" { fn flock(fd: i32, op: i32) -> i32; }
        // LOCK_EX | LOCK_NB = 2 | 4 — 논블로킹 배타
        if flock(fd, 2 | 4) != 0 {
            anyhow::bail!("다른 install이 진행 중입니다 ({}). 완료 후 재시도하세요.", lock_path.display());
        }
    }

    let total = domains.len();
    let mut failed = vec![];
    for (i, d) in domains.iter().enumerate() {
        println!("[{}/{total}] {d}", i + 1);
        if let Err(e) = install(d) {
            eprintln!("  ✗ {d}: {e}");
            failed.push(d.clone());
            // 각 도메인의 바이너리 다운로드는 독립적 — 한 개 실패해도 나머지 계속 시도.
            // (bootstrap 실패가 뒤 도메인 기능을 막지는 않음 — 다운로드만 하니까)
        }
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
    let tar_result = common::run(
        "tar",
        &[
            "-xzf",
            &dest_tar.display().to_string(),
            "-C",
            &dom_dir.display().to_string(),
        ],
    );
    // tarball 파일은 성공/실패 관계없이 정리
    let _ = std::fs::remove_file(&dest_tar);
    if let Err(e) = tar_result {
        // 부분 추출된 상태면 dom_dir 전체 삭제 (부분 설치 방지)
        let _ = std::fs::remove_dir_all(&dom_dir);
        return Err(anyhow::anyhow!("tar 추출 실패: {e}"));
    }

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
        Ok(p) => println!("  config_dir: {} (exists: {})", p.display(), p.exists()),
        Err(e) => println!("  config_dir: ✗ {e}"),
    }
    match paths::bin_dir() {
        Ok(p) => println!("  bin_dir:    {}", p.display()),
        Err(e) => println!("  bin_dir:    ✗ {e}"),
    }

    println!("\n[core 의존성]");
    for (name, cmd) in &[("curl", "curl"), ("tar", "tar"), ("systemctl", "systemctl")] {
        println!("  {} {name}", if common::has_cmd(cmd) { "✓" } else { "✗" });
    }
    println!(
        "  {} dotenvx {}",
        if prelik_core::dotenvx::is_installed() { "✓" } else { "✗" },
        if prelik_core::dotenvx::is_installed() { "" } else { "(prelik install bootstrap)" }
    );
    println!(
        "  {} nickel (runtime registry export)",
        if common::has_cmd("nickel") { "✓" } else { "✗" }
    );

    // 설치된 도메인 + 각자 doctor 실행
    println!("\n[설치된 도메인]");
    let bin_dir = match paths::bin_dir() {
        Ok(p) => p,
        Err(_) => return,
    };
    let domains_dir = match paths::domains_dir() {
        Ok(p) => p,
        Err(_) => return,
    };
    if !domains_dir.exists() {
        println!("  (prelik setup 필요)");
        return;
    }
    let mut any = false;
    if let Ok(entries) = std::fs::read_dir(&domains_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let bin = bin_dir.join(format!("prelik-{name}"));
            let status = if bin.exists() { "✓" } else { "✗ (바이너리 누락)" };
            println!("  {status} {name}");
            any = true;
        }
    }
    if !any {
        println!("  (설치된 도메인 없음 — prelik install <domain>)");
    }
}

fn detect_target() -> anyhow::Result<String> {
    let arch = common::run("uname", &["-m"])?;
    match arch.as_str() {
        "x86_64" => Ok("x86_64-linux".into()),
        "aarch64" | "arm64" => Ok("aarch64-linux".into()),
        other => anyhow::bail!("지원하지 않는 아키텍처: {other}"),
    }
}

// ========== uninstall ==========

fn uninstall(confirm: bool, purge: bool) -> anyhow::Result<()> {
    println!("=== prelik uninstall ===");
    if !confirm {
        println!("(dry-run — 실제 삭제하려면 --confirm)\n");
    }

    // 제거 대상 수집 (실제 파일 시스템 점검 후만 보고).
    let mut bin_dirs: Vec<PathBuf> = vec![PathBuf::from("/usr/local/bin")];
    if let Ok(home) = std::env::var("HOME") {
        bin_dirs.push(PathBuf::from(home).join(".local/bin"));
    }
    let mut bin_targets: Vec<PathBuf> = Vec::new();
    for dir in &bin_dirs {
        // prelik 본체 + .prelik.version 마커
        let main_bin = dir.join("prelik");
        if main_bin.exists() { bin_targets.push(main_bin); }
        let marker = dir.join(".prelik.version");
        if marker.exists() { bin_targets.push(marker); }
        // prelik-* 도메인 바이너리
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                if let Some(name) = e.file_name().to_str() {
                    if name.starts_with("prelik-") {
                        bin_targets.push(e.path());
                    }
                }
            }
        }
    }
    bin_targets.sort();
    bin_targets.dedup();

    let mut purge_dirs: Vec<PathBuf> = Vec::new();
    if purge {
        // 도메인별 sub-binary cache + 사용자/시스템 config + recovery snapshots.
        // root에선 paths::config_dir() == /etc/prelik이라 중복될 수 있어 canonical 후 dedup.
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(d) = paths::domains_dir() { candidates.push(d); }
        if let Ok(d) = paths::config_dir()  { candidates.push(d); }
        candidates.push(PathBuf::from("/etc/prelik"));
        candidates.push(PathBuf::from("/var/lib/prelik"));
        let mut seen: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
        for p in candidates {
            if !p.exists() { continue; }
            // canonicalize로 동일 디렉토리 변형 (경로 표기/symlink) 통합. 실패 시 원본 사용.
            let key = p.canonicalize().unwrap_or(p.clone());
            if seen.insert(key) {
                purge_dirs.push(p);
            }
        }
    }

    println!("[삭제 대상] 바이너리 ({}개):", bin_targets.len());
    for p in &bin_targets { println!("  - {}", p.display()); }
    if purge {
        println!("\n[--purge] 디렉토리 ({}개):", purge_dirs.len());
        for p in &purge_dirs { println!("  - {}", p.display()); }
    } else {
        println!("\n[참고] config/recovery 디렉토리는 보존됩니다 (--purge로 함께 삭제 가능).");
    }

    println!("\n[건드리지 않는 것]");
    println!("  - LXC/VM 자체 (pct/qm 리소스 — 데이터 유실 방지)");
    println!("  - /etc/fstab의 nas 마운트 항목, /etc/cifs-credentials/*");
    println!("  - postfix relay 백업 (/etc/postfix/prelik-backup-*/), sasl_passwd, sender_canonical");
    println!("  - traefik 컨테이너 / 라우트 / TLS 인증서");
    println!("  - cloudflare DNS 레코드 / Worker / Pages");
    println!("  - dotenvx로 암호화된 .env.vault (purge에도 별도 위치면 보존)");
    println!("  - systemd timers/services (cluster-files-sync.timer 등)");
    println!("\n수동 정리 절차: docs/uninstall.md 참조.\n");

    if !confirm {
        println!("실제 삭제하려면: prelik uninstall --confirm{}",
            if purge { " --purge" } else { "" });
        return Ok(());
    }

    let mut failed = 0u32;
    for p in &bin_targets {
        match std::fs::remove_file(p) {
            Ok(_) => println!("  ✓ {}", p.display()),
            Err(e) => { eprintln!("  ✗ {} ({e})", p.display()); failed += 1; }
        }
    }
    if purge {
        for p in &purge_dirs {
            match std::fs::remove_dir_all(p) {
                Ok(_) => println!("  ✓ {} (purged)", p.display()),
                Err(e) => { eprintln!("  ✗ {} ({e})", p.display()); failed += 1; }
            }
        }
    }

    if failed > 0 {
        anyhow::bail!("{failed}개 항목 제거 실패. 권한(sudo) 또는 잠금 상태 확인.");
    }
    println!("\n✓ 완료. 외부 시스템(LXC/postfix/CF 등) 정리는 docs/uninstall.md 참조.");
    Ok(())
}
