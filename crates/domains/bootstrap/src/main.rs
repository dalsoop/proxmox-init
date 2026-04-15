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
    /// 전체 의존성 설치
    Install,
    /// 상태 확인
    Doctor,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Install => install(),
        Cmd::Doctor => doctor(),
    }
}

fn install() -> anyhow::Result<()> {
    println!("=== prelik bootstrap ===");
    let distro = os::Distro::detect();
    println!("  distro: {distro:?}");

    match distro {
        os::Distro::Debian | os::Distro::Ubuntu => apt_install()?,
        os::Distro::Alpine => apk_install()?,
        _ => anyhow::bail!("지원하지 않는 배포판: {distro:?}"),
    }

    install_rust()?;
    install_gh()?;
    install_dotenvx()?;

    println!("\n✓ bootstrap 완료");
    doctor()?;
    Ok(())
}

fn apt_install() -> anyhow::Result<()> {
    if !common::has_cmd("curl") {
        common::run_bash("sudo apt-get update && sudo apt-get install -y curl ca-certificates build-essential git jq")?;
    }
    Ok(())
}

fn apk_install() -> anyhow::Result<()> {
    common::run_bash("sudo apk add --no-cache curl ca-certificates build-base git jq")?;
    Ok(())
}

fn install_rust() -> anyhow::Result<()> {
    if common::has_cmd("cargo") {
        println!("  ✓ rust 이미 설치됨");
        return Ok(());
    }
    println!("  rust 설치 중...");
    common::run_bash("curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable")?;
    Ok(())
}

fn install_gh() -> anyhow::Result<()> {
    if common::has_cmd("gh") {
        println!("  ✓ gh 이미 설치됨");
        return Ok(());
    }
    println!("  gh 설치 중...");
    // Debian/Ubuntu 전용 공식 설치 경로
    common::run_bash(r#"
        curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg | sudo dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg
        echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" | sudo tee /etc/apt/sources.list.d/github-cli.list >/dev/null
        sudo apt-get update && sudo apt-get install -y gh
    "#)?;
    Ok(())
}

fn install_dotenvx() -> anyhow::Result<()> {
    if common::has_cmd("dotenvx") {
        println!("  ✓ dotenvx 이미 설치됨");
        return Ok(());
    }
    println!("  dotenvx 설치 중...");
    common::run_bash("curl -fsS https://dotenvx.sh | sudo sh")?;
    Ok(())
}

fn doctor() -> anyhow::Result<()> {
    let checks = [
        ("curl", common::has_cmd("curl")),
        ("git", common::has_cmd("git")),
        ("rust/cargo", common::has_cmd("cargo")),
        ("gh", common::has_cmd("gh")),
        ("dotenvx", common::has_cmd("dotenvx")),
        ("systemctl", common::has_cmd("systemctl")),
    ];
    println!("bootstrap doctor:");
    for (name, ok) in checks {
        println!("  {} {}", if ok { "✓" } else { "✗" }, name);
    }
    Ok(())
}
