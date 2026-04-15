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
    /// "prelik이 설치할 수 있는 항목" 카탈로그. 호스트 현재 상태가 아니라 **정적 안내**.
    /// 실제 install_*는 idempotent (이미 있는 도구는 skip)이므로, 호스트의 어떤 항목이
    /// prelik이 깐 것인지 manifest만으로는 알 수 없다 — 'detected' 필드로 표시.
    /// 제거 절차는 실제 remove_* 로직과 정확히 일치.
    fn manifest_entry(&self) -> serde_json::Value {
        let detected = common::has_cmd(self.check_cmd());
        match self {
            Self::Apt => serde_json::json!({
                "tool": "apt",
                "static_install_packages": ["curl", "ca-certificates", "build-essential", "git", "jq"],
                "binaries": [],
                "files": [],
                "detected_on_host": detected,
                "warning": "이 패키지들이 이미 시스템에 있었다면 prelik이 설치한 것이 아닙니다 (idempotent install).",
                "uninstall_actual": "sudo apt-get remove -y build-essential\n# prelik bootstrap remove --only apt 가 실제로 하는 일과 동일.\n# curl/ca-certificates/git/jq는 시스템 핵심이라 제거하지 않음 (사용자가 명시적으로 깔았다고 확신할 때만 수동 제거)",
            }),
            Self::Rust => serde_json::json!({
                "tool": "rust",
                "static_install_packages": [],
                "binaries": ["~/.cargo/bin/cargo", "~/.cargo/bin/rustc", "~/.cargo/bin/rustup"],
                "files": ["~/.cargo/", "~/.rustup/"],
                "detected_on_host": detected,
                "warning": "Rust가 이미 있었다면 prelik이 설치한 게 아닐 수 있음. ~/.cargo/는 다른 cargo install 산출물도 포함.",
                "uninstall_actual": "rustup self uninstall\n# 또는: rm -rf ~/.cargo ~/.rustup\n# .bashrc/.zshrc의 'source $HOME/.cargo/env' 수동 제거 필요",
            }),
            Self::Gh => serde_json::json!({
                "tool": "gh",
                "static_install_packages": ["gh"],
                "binaries": ["/usr/bin/gh"],
                "files": ["/etc/apt/sources.list.d/github-cli.list", "/usr/share/keyrings/githubcli-archive-keyring.gpg", "~/.config/gh/"],
                "detected_on_host": detected,
                "warning": null,
                "uninstall_actual": "sudo apt remove --purge gh\nsudo rm -f /etc/apt/sources.list.d/github-cli.list /usr/share/keyrings/githubcli-archive-keyring.gpg\nrm -rf ~/.config/gh    # 인증 토큰 포함",
            }),
            Self::Dotenvx => serde_json::json!({
                "tool": "dotenvx",
                "static_install_packages": ["nodejs", "npm"],
                "binaries": ["/usr/local/bin/dotenvx (대안)", "npm -g @dotenvx/dotenvx (실제)"],
                "files": ["~/.npm/", "/usr/lib/node_modules/@dotenvx/dotenvx/"],
                "detected_on_host": detected,
                "warning": "prelik install이 npm 없으면 nodejs+npm을 같이 깔고 'sudo npm install -g @dotenvx/dotenvx'로 설치. nodejs/npm은 시스템 의존이라 제거 시 신중.",
                "uninstall_actual": "sudo npm uninstall -g @dotenvx/dotenvx\n# /usr/local/bin/dotenvx도 있으면: sudo rm -f /usr/local/bin/dotenvx\n# nodejs/npm은 다른 도구가 쓸 수 있으니 자동 제거 안 함\n# .env.keys / .env.vault는 프로젝트별 별도 관리 (자동 삭제 금지)",
            }),
            Self::Nickel => serde_json::json!({
                "tool": "nickel",
                "static_install_packages": [],
                "binaries": ["/usr/local/bin/nickel (바이너리 다운로드 성공 시)", "~/.cargo/bin/nickel (cargo install 폴백)"],
                "files": [],
                "detected_on_host": detected,
                "warning": "GitHub 다운로드 실패 시 'cargo install nickel-lang-cli'로 폴백됨. 두 경로 모두 확인 필요.",
                "uninstall_actual": "sudo rm -f /usr/local/bin/nickel\n# cargo install로 깔렸다면: cargo uninstall nickel-lang-cli",
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
    println!("=== bootstrap manifest ===");
    println!("⚠ 정적 카탈로그 — prelik이 깐 것인지 호스트의 다른 출처인지 구분하지 않음.");
    println!("  detected=true는 'PATH에 명령이 있다'는 뜻일 뿐 prelik 출처 보장 아님.\n");
    for (t, e) in tools.iter().zip(&entries) {
        println!("[{}] (detected={})", t.name(), e["detected_on_host"].as_bool().unwrap_or(false));
        if let Some(arr) = e["static_install_packages"].as_array() {
            if !arr.is_empty() {
                println!("  설치 가능 apt 패키지: {}",
                    arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "));
            }
        }
        if let Some(arr) = e["binaries"].as_array() {
            if !arr.is_empty() {
                println!("  바이너리 후보:        {}",
                    arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "));
            }
        }
        if let Some(arr) = e["files"].as_array() {
            if !arr.is_empty() {
                println!("  부가 파일:            {}",
                    arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "));
            }
        }
        if let Some(w) = e["warning"].as_str() {
            println!("  ⚠ {w}");
        }
        if let Some(s) = e["uninstall_actual"].as_str() {
            println!("  제거 절차 (실제 remove_* 로직과 일치):");
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
