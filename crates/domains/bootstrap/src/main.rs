use clap::{Parser, Subcommand};
use prelik_core::{common, os};

#[derive(Parser)]
#[command(name = "prelik-bootstrap", about = "의존성 설치 (apt/rust/gh/dotenvx)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Install,
    Doctor,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Install => install(),
        Cmd::Doctor => {
            doctor();
            Ok(())
        }
    }
}

/// 필수 커맨드 목록. bootstrap install 이후 doctor가 모두 ✓ 여야 함.
const REQUIRED: &[(&str, &str)] = &[
    ("curl", "curl"),
    ("git", "git"),
    ("jq", "jq"),
    ("make", "build-essential"), // apt 패키지 이름
    ("cargo", "rust"),
    ("gh", "gh"),
    ("dotenvx", "dotenvx"),
    ("nickel", "nickel"),
    ("systemctl", "systemd"),
];

fn install() -> anyhow::Result<()> {
    println!("=== prelik bootstrap ===");
    let distro = os::Distro::detect();
    println!("  distro: {distro:?}");

    match distro {
        os::Distro::Debian | os::Distro::Ubuntu => apt_install()?,
        os::Distro::Alpine => {
            anyhow::bail!(
                "Alpine은 현재 미지원 (GitHub CLI/dotenvx apk 경로 미구현). \
                 지원 예정이지만 v0.1에서는 Debian/Ubuntu만 동작합니다."
            );
        }
        other => anyhow::bail!("지원하지 않는 배포판: {other:?}"),
    }

    install_rust()?;
    install_gh()?;
    install_dotenvx()?;
    install_nickel()?;

    println!("\n=== bootstrap 완료 ===");
    if !doctor() {
        anyhow::bail!("bootstrap 설치 후에도 필수 툴이 누락됐습니다 — 위 doctor 출력 확인");
    }
    Ok(())
}

fn apt_install() -> anyhow::Result<()> {
    // 각 패키지를 개별적으로 확인·설치 (curl만 있다고 전부 건너뛰면 안 됨)
    let apt_packages = ["curl", "ca-certificates", "build-essential", "git", "jq"];
    let missing: Vec<&str> = apt_packages
        .iter()
        .copied()
        .filter(|pkg| {
            // 패키지 존재 여부 확인 (dpkg -s)
            common::run("dpkg", &["-s", pkg]).is_err()
        })
        .collect();
    if !missing.is_empty() {
        let pkgs = missing.join(" ");
        println!("  apt install: {pkgs}");
        common::run_bash(&format!(
            "sudo apt-get update && sudo apt-get install -y {pkgs}"
        ))?;
    } else {
        println!("  ✓ apt 필수 패키지 이미 설치됨");
    }
    Ok(())
}

fn install_rust() -> anyhow::Result<()> {
    if common::has_cmd("cargo") {
        println!("  ✓ rust 이미 설치됨");
        return Ok(());
    }
    println!("  rust 설치 중...");
    // rustup-init 바이너리를 먼저 다운로드 + 체크섬 확인 후 실행하는 편이 안전하지만
    // 공식 채널이 HTTPS + GPG 서명된 redirect이므로 현 시점에는 공식 스크립트 허용.
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
    println!("  gh 설치 중 (Debian/Ubuntu 경로)...");
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
    // 임의 실행 방지: npm 경로로 설치. 향후 체크섬 고정 릴리스 아카이브로 전환 예정.
    if !common::has_cmd("npm") {
        println!("  dotenvx용 npm 설치...");
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
    // GitHub Release에서 musl 바이너리 직접 받기 (cargo install 대비 ~10배 빠름)
    let install_script = r#"
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
    match common::run_bash(install_script) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("  바이너리 다운로드 실패 ({e}) — cargo install로 폴백");
            common::run_bash("cargo install nickel-lang-cli --locked")?;
        }
    }
    Ok(())
}

fn doctor() -> bool {
    println!("bootstrap doctor:");
    let mut all_ok = true;
    for (cmd, pkg) in REQUIRED {
        let ok = common::has_cmd(cmd);
        println!("  {} {cmd}{}", if ok { "✓" } else { "✗" }, if ok { String::new() } else { format!("  (install: {pkg})") });
        if !ok {
            all_ok = false;
        }
    }
    all_ok
}
