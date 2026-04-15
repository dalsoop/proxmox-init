use clap::{Parser, Subcommand, ValueEnum};
use prelik_core::{common, os};

#[derive(Parser)]
#[command(name = "prelik-bootstrap", about = "의존성 개별/일괄 설치·제거 (apt/rust/gh/dotenvx/nickel)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 도구 설치 (전체 또는 --only 로 선택)
    Install {
        /// 특정 도구만 (예: --only rust 또는 --only nickel,gh)
        #[arg(long, value_delimiter = ',')]
        only: Vec<Tool>,
    },
    /// 도구 제거 (--only 필수)
    Remove {
        /// 제거할 도구 (여러 개 쉼표 구분)
        #[arg(long, value_delimiter = ',', required = true)]
        only: Vec<Tool>,
    },
    /// 설치 상태 점검
    Doctor,
    /// 설치 가능한 도구 목록
    List,
    /// 각 도구가 설치하는 정확한 항목 (apt 패키지/바이너리 경로/제거 절차) 출력
    Manifest {
        /// 특정 도구만
        #[arg(long, value_delimiter = ',')]
        only: Vec<Tool>,
        /// JSON 출력
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Tool {
    /// apt 필수 패키지 (curl, git, jq, build-essential)
    Apt,
    Rust,
    Gh,
    Dotenvx,
    Nickel,
}

impl Tool {
    fn all() -> Vec<Self> {
        vec![Self::Apt, Self::Rust, Self::Gh, Self::Dotenvx, Self::Nickel]
    }
    fn name(&self) -> &'static str {
        match self {
            Self::Apt => "apt",
            Self::Rust => "rust",
            Self::Gh => "gh",
            Self::Dotenvx => "dotenvx",
            Self::Nickel => "nickel",
        }
    }
    fn check_cmd(&self) -> &'static str {
        match self {
            Self::Apt => "curl",
            Self::Rust => "cargo",
            Self::Gh => "gh",
            Self::Dotenvx => "dotenvx",
            Self::Nickel => "nickel",
        }
    }
    /// 정확히 무엇을 깔고/어디에 두는지/어떻게 지우는지 — uninstall.md §9의 단순 안내를 정확화.
    fn manifest_entry(&self) -> serde_json::Value {
        match self {
            Self::Apt => serde_json::json!({
                "tool": "apt",
                "apt_packages": ["curl", "ca-certificates", "build-essential", "git", "jq"],
                "binaries": [],
                "files": [],
                "uninstall": "sudo apt remove --purge curl ca-certificates build-essential git jq\n# 주의: build-essential은 다른 패키지가 의존할 수 있음 (apt autoremove 신중히)",
            }),
            Self::Rust => serde_json::json!({
                "tool": "rust",
                "apt_packages": [],
                "binaries": ["~/.cargo/bin/cargo", "~/.cargo/bin/rustc", "~/.cargo/bin/rustup"],
                "files": ["~/.cargo/", "~/.rustup/"],
                "uninstall": "rustup self uninstall\n# 또는: rm -rf ~/.cargo ~/.rustup\n# .bashrc/.zshrc의 'source $HOME/.cargo/env' 라인도 수동 제거",
            }),
            Self::Gh => serde_json::json!({
                "tool": "gh",
                "apt_packages": ["gh"],
                "binaries": ["/usr/bin/gh"],
                "files": ["/etc/apt/sources.list.d/github-cli.list", "/usr/share/keyrings/githubcli-archive-keyring.gpg", "~/.config/gh/"],
                "uninstall": "sudo apt remove --purge gh\nsudo rm -f /etc/apt/sources.list.d/github-cli.list /usr/share/keyrings/githubcli-archive-keyring.gpg\nrm -rf ~/.config/gh    # 인증 토큰 포함",
            }),
            Self::Dotenvx => serde_json::json!({
                "tool": "dotenvx",
                "apt_packages": [],
                "binaries": ["/usr/local/bin/dotenvx"],
                "files": [],
                "uninstall": "sudo rm -f /usr/local/bin/dotenvx\n# .env.keys / .env.vault 파일은 프로젝트별로 별도 관리 (자동 삭제 금지)",
            }),
            Self::Nickel => serde_json::json!({
                "tool": "nickel",
                "apt_packages": [],
                "binaries": ["/usr/local/bin/nickel"],
                "files": [],
                "uninstall": "sudo rm -f /usr/local/bin/nickel",
            }),
        }
    }
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Install { only } => {
            let tools = if only.is_empty() { Tool::all() } else { only };
            install_tools(&tools)
        }
        Cmd::Remove { only } => remove_tools(&only),
        Cmd::Doctor => {
            doctor();
            Ok(())
        }
        Cmd::List => {
            list();
            Ok(())
        }
        Cmd::Manifest { only, json } => {
            let tools = if only.is_empty() { Tool::all() } else { only };
            manifest(&tools, json)
        }
    }
}

fn manifest(tools: &[Tool], json: bool) -> anyhow::Result<()> {
    let entries: Vec<serde_json::Value> = tools.iter().map(|t| t.manifest_entry()).collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }
    println!("=== bootstrap manifest ===\n");
    for (t, e) in tools.iter().zip(&entries) {
        println!("[{}]", t.name());
        if let Some(arr) = e["apt_packages"].as_array() {
            if !arr.is_empty() {
                println!("  apt 패키지: {}",
                    arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "));
            }
        }
        if let Some(arr) = e["binaries"].as_array() {
            if !arr.is_empty() {
                println!("  바이너리:   {}",
                    arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "));
            }
        }
        if let Some(arr) = e["files"].as_array() {
            if !arr.is_empty() {
                println!("  파일:       {}",
                    arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "));
            }
        }
        if let Some(s) = e["uninstall"].as_str() {
            println!("  제거 절차:");
            for line in s.lines() {
                println!("    {line}");
            }
        }
        println!();
    }
    Ok(())
}

fn install_tools(tools: &[Tool]) -> anyhow::Result<()> {
    println!("=== prelik bootstrap install ===");
    println!("  distro: {:?}", os::Distro::detect());
    println!("  대상: {}", tools.iter().map(|t| t.name()).collect::<Vec<_>>().join(", "));

    match os::Distro::detect() {
        os::Distro::Debian | os::Distro::Ubuntu => {}
        os::Distro::Alpine => anyhow::bail!("Alpine 미지원 (gh/dotenvx apk 경로 미구현)"),
        other => anyhow::bail!("지원하지 않는 배포판: {other:?}"),
    }

    for t in tools {
        match t {
            Tool::Apt => install_apt()?,
            Tool::Rust => install_rust()?,
            Tool::Gh => install_gh()?,
            Tool::Dotenvx => install_dotenvx()?,
            Tool::Nickel => install_nickel()?,
        }
    }

    println!("\n=== bootstrap install 완료 ===");
    doctor();
    Ok(())
}

fn remove_tools(tools: &[Tool]) -> anyhow::Result<()> {
    println!("=== prelik bootstrap remove ===");
    println!("  대상: {}", tools.iter().map(|t| t.name()).collect::<Vec<_>>().join(", "));

    for t in tools {
        match t {
            Tool::Apt => remove_apt()?,
            Tool::Rust => remove_rust()?,
            Tool::Gh => remove_gh()?,
            Tool::Dotenvx => remove_dotenvx()?,
            Tool::Nickel => remove_nickel()?,
        }
    }

    println!("\n=== bootstrap remove 완료 ===");
    Ok(())
}

fn list() {
    println!("설치 가능한 도구:");
    for t in Tool::all() {
        let status = if common::has_cmd(t.check_cmd()) { "✓" } else { "✗" };
        println!("  {status} {}", t.name());
    }
    println!("\n사용: prelik run bootstrap install --only rust,nickel");
    println!("      prelik run bootstrap remove  --only nickel");
}

// ============ install ============

fn install_apt() -> anyhow::Result<()> {
    let pkgs = ["curl", "ca-certificates", "build-essential", "git", "jq"];
    let missing: Vec<&str> = pkgs.iter().copied()
        .filter(|pkg| common::run("dpkg", &["-s", pkg]).is_err())
        .collect();
    if missing.is_empty() {
        println!("  ✓ apt 필수 패키지 이미 설치됨");
        return Ok(());
    }
    println!("  apt install: {}", missing.join(" "));
    common::run_bash(&format!("sudo apt-get update && sudo apt-get install -y {}", missing.join(" ")))?;
    Ok(())
}

fn install_rust() -> anyhow::Result<()> {
    if common::has_cmd("cargo") {
        println!("  ✓ rust 이미 설치됨");
        return Ok(());
    }
    println!("  rust 설치 중...");
    common::run_bash(
        "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
         sh -s -- -y --default-toolchain stable --profile minimal",
    )?;
    Ok(())
}

fn install_gh() -> anyhow::Result<()> {
    if common::has_cmd("gh") {
        println!("  ✓ gh 이미 설치됨");
        return Ok(());
    }
    println!("  gh 설치 중 (Debian/Ubuntu)...");
    common::run_bash(r#"
        set -e
        curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
          | sudo dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg
        echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
          | sudo tee /etc/apt/sources.list.d/github-cli.list >/dev/null
        sudo apt-get update && sudo apt-get install -y gh
    "#)?;
    Ok(())
}

fn install_dotenvx() -> anyhow::Result<()> {
    if common::has_cmd("dotenvx") {
        println!("  ✓ dotenvx 이미 설치됨");
        return Ok(());
    }
    if !common::has_cmd("npm") {
        println!("  npm 설치 (dotenvx 의존)...");
        common::run_bash("sudo apt-get install -y nodejs npm")?;
    }
    println!("  dotenvx 설치 중 (npm -g)...");
    common::run_bash("sudo npm install -g @dotenvx/dotenvx@latest")?;
    Ok(())
}

fn install_nickel() -> anyhow::Result<()> {
    if common::has_cmd("nickel") {
        println!("  ✓ nickel 이미 설치됨");
        return Ok(());
    }
    println!("  nickel 설치 중 (바이너리 다운로드)...");
    let script = r#"
        set -e
        ARCH=$(uname -m)
        case "$ARCH" in
          x86_64) SUFFIX="x86_64-linux" ;;
          aarch64|arm64) SUFFIX="arm64-linux" ;;
          *) echo "지원 아키텍처 아님: $ARCH"; exit 1 ;;
        esac
        URL="https://github.com/tweag/nickel/releases/latest/download/nickel-${SUFFIX}"
        TMP=$(mktemp)
        trap 'rm -f "$TMP"' EXIT
        curl -fsSL --retry 3 -o "$TMP" "$URL"
        sudo install -m 755 "$TMP" /usr/local/bin/nickel
    "#;
    match common::run_bash(script) {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("  바이너리 다운로드 실패 ({e}) — cargo install로 폴백");
            common::run_bash("cargo install nickel-lang-cli --locked")?;
            Ok(())
        }
    }
}

// ============ remove ============

fn remove_apt() -> anyhow::Result<()> {
    // build-essential만 제거 (다른 패키지는 시스템 의존성 있을 수 있음 — 안전 선택)
    println!("  apt: build-essential만 제거 (나머지는 시스템 의존성이라 유지)");
    common::run_bash("sudo apt-get remove -y build-essential 2>&1 | tail -1").ok();
    Ok(())
}

fn remove_rust() -> anyhow::Result<()> {
    if !common::has_cmd("cargo") {
        println!("  ⊘ rust 이미 없음");
        return Ok(());
    }
    if common::has_cmd("rustup") {
        println!("  rust 제거 중 (rustup self uninstall)...");
        common::run_bash("rustup self uninstall -y")?;
    } else {
        println!("  ⚠ rustup 없음 — 수동으로 ~/.cargo, ~/.rustup 삭제 필요");
    }
    Ok(())
}

fn remove_gh() -> anyhow::Result<()> {
    println!("  gh 제거 중...");
    common::run_bash("sudo apt-get remove -y gh 2>&1 | tail -1").ok();
    common::run_bash("sudo rm -f /etc/apt/sources.list.d/github-cli.list /usr/share/keyrings/githubcli-archive-keyring.gpg").ok();
    Ok(())
}

fn remove_dotenvx() -> anyhow::Result<()> {
    if !common::has_cmd("dotenvx") {
        println!("  ⊘ dotenvx 이미 없음");
        return Ok(());
    }
    println!("  dotenvx 제거 중 (npm -g)...");
    common::run_bash("sudo npm uninstall -g @dotenvx/dotenvx")?;
    Ok(())
}

fn remove_nickel() -> anyhow::Result<()> {
    if !common::has_cmd("nickel") {
        println!("  ⊘ nickel 이미 없음");
        return Ok(());
    }
    let path = common::run("which", &["nickel"]).unwrap_or_default();
    println!("  nickel 제거 중 ({})...", path);
    if path.starts_with("/usr/local/bin") {
        common::run_bash(&format!("sudo rm -f {}", path))?;
    } else if path.contains(".cargo/bin") {
        common::run_bash("cargo uninstall nickel-lang-cli 2>/dev/null || rm -f ~/.cargo/bin/nickel").ok();
    } else {
        eprintln!("  ⚠ 예상치 못한 경로 ({path}) — 수동 제거 필요");
    }
    Ok(())
}

fn doctor() {
    println!("bootstrap doctor:");
    for t in Tool::all() {
        let ok = common::has_cmd(t.check_cmd());
        println!("  {} {}", if ok { "✓" } else { "✗" }, t.name());
    }
}
