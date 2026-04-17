//! prelik-ministack — MiniStack (로컬 AWS 에뮬레이터 = LocalStack).

use clap::{Parser, Subcommand};
use prelik_core::common;
use std::path::Path;
use std::process::Command;

#[derive(Parser)]
#[command(name = "prelik-ministack", about = "MiniStack (LocalStack AWS 에뮬레이터)")]
struct Cli { #[command(subcommand)] cmd: Cmd }

#[derive(Subcommand)]
enum Cmd {
    Install { #[arg(long, default_value = "4566")] port: u16, #[arg(long)] data_dir: Option<String> },
    Uninstall { #[arg(long)] force: bool },
    Start, Stop, Restart, Status, Reset,
    Logs { #[arg(long)] follow: bool, #[arg(long)] tail: Option<String> },
    Update,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let dc = "/opt/ministack/docker-compose.yml";
    match cli.cmd {
        Cmd::Status => { common::run("docker", &["compose", "-f", dc, "ps"]); }
        Cmd::Start => { common::run("docker", &["compose", "-f", dc, "up", "-d"]); }
        Cmd::Stop => { common::run("docker", &["compose", "-f", dc, "down"]); }
        Cmd::Restart => { common::run("docker", &["compose", "-f", dc, "restart"]); }
        Cmd::Logs { follow, tail } => {
            let mut a = vec!["compose", "-f", dc, "logs"];
            if follow { a.push("-f"); }
            if let Some(t) = &tail { a.push("--tail"); a.push(t); }
            common::run("docker", &a);
        }
        Cmd::Install { port, data_dir } => ministack_install(port, data_dir.as_deref()),
        Cmd::Uninstall { force } => ministack_uninstall(force),
        Cmd::Reset => ministack_reset(),
        Cmd::Update => ministack_update(),
    }
    Ok(())
}

const INSTALL_DIR: &str = "/opt/ministack";
const COMPOSE_FILE: &str = "/opt/ministack/docker-compose.yml";
const PORT_FILE: &str = "/opt/ministack/.port";
const DOCKER_IMAGE: &str = "ministackorg/ministack:latest";
const AWS_ENV_FILE: &str = "/etc/profile.d/ministack.sh";

fn saved_port() -> u16 {
    std::fs::read_to_string(PORT_FILE)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(4566)
}

fn require_installed() {
    if !Path::new(COMPOSE_FILE).exists() {
        eprintln!("MiniStack이 설치되어 있지 않습니다. `prelik run ministack install`을 먼저 실행하세요.");
        std::process::exit(1);
    }
}

fn require_docker() {
    let ok = Command::new("docker")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("Error: docker가 설치되어 있지 않습니다.");
        std::process::exit(1);
    }
}

fn ministack_install(port: u16, data_dir: Option<&str>) {
    println!("=== MiniStack 설치 ===\n");
    require_docker();

    if Path::new(COMPOSE_FILE).exists() {
        println!("이미 설치됨: {INSTALL_DIR}");
        println!("재설치하려면 먼저 `prelik run ministack uninstall --force`를 실행하세요.");
        return;
    }

    // 포트 충돌 확인
    let port_check = Command::new("ss")
        .args(["-tlnp"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&format!(":{port} ")))
        .unwrap_or(false);
    if port_check {
        eprintln!("Error: 포트 {port}가 이미 사용 중입니다.");
        std::process::exit(1);
    }

    let s3_data = data_dir.unwrap_or("/opt/ministack/data/s3");
    std::fs::create_dir_all(s3_data).unwrap_or_else(|e| {
        eprintln!("데이터 디렉토리 생성 실패: {e}");
        std::process::exit(1);
    });

    let compose = format!(
        r#"services:
  ministack:
    image: {DOCKER_IMAGE}
    container_name: ministack
    ports:
      - "{port}:4566"
    environment:
      - GATEWAY_PORT=4566
      - LOG_LEVEL=INFO
      - S3_PERSIST=1
      - REDIS_HOST=redis
      - REDIS_PORT=6379
      - RDS_BASE_PORT=15432
      - ELASTICACHE_BASE_PORT=16379
    volumes:
      - {s3_data}:/tmp/ministack-data/s3
      - /var/run/docker.sock:/var/run/docker.sock
    depends_on:
      redis:
        condition: service_healthy
    healthcheck:
      test: ["CMD", "python", "-c", "import urllib.request; urllib.request.urlopen('http://localhost:4566/_ministack/health')"]
      interval: 10s
      timeout: 3s
      retries: 3
      start_period: 5s
    restart: unless-stopped

  redis:
    image: redis:7-alpine
    container_name: ministack-redis
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 5s
      timeout: 3s
      retries: 5
    restart: unless-stopped
"#
    );

    std::fs::create_dir_all(INSTALL_DIR).unwrap_or_else(|e| {
        eprintln!("설치 디렉토리 생성 실패: {e}");
        std::process::exit(1);
    });
    std::fs::write(COMPOSE_FILE, &compose).unwrap_or_else(|e| {
        eprintln!("docker-compose.yml 작성 실패: {e}");
        std::process::exit(1);
    });
    let _ = std::fs::write(PORT_FILE, port.to_string());

    println!("  docker-compose.yml 생성: {COMPOSE_FILE}");
    println!("  S3 데이터 디렉토리: {s3_data}");
    println!("  포트: {port}");

    println!("\n이미지 pull 중...");
    common::run("docker", &["compose", "-f", COMPOSE_FILE, "pull"]).ok();

    // AWS CLI 확인
    if !Command::new("aws").arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
        println!("\nAWS CLI 설치 중...");
        let _ = Command::new("apt-get").args(["install", "-y", "-qq", "awscli"]).status();
    }

    // AWS 환경변수 설정
    let env_content = format!(
        r#"# MiniStack AWS endpoint (prelik ministack install 자동 생성)
export AWS_ENDPOINT_URL="http://localhost:{port}"
export AWS_ACCESS_KEY_ID="test"
export AWS_SECRET_ACCESS_KEY="test"
export AWS_DEFAULT_REGION="us-east-1"
"#
    );
    match std::fs::write(AWS_ENV_FILE, &env_content) {
        Ok(_) => println!("  AWS 환경변수 설정: {AWS_ENV_FILE}"),
        Err(e) => eprintln!("  환경변수 파일 생성 실패: {e}"),
    }

    println!("컨테이너 시작 중...");
    if common::run("docker", &["compose", "-f", COMPOSE_FILE, "up", "-d"]).is_ok() {
        println!("\n  MiniStack 설치 완료!");
        println!("  엔드포인트: http://localhost:{port}");
        println!("\n  바로 사용 가능:");
        println!("    source {AWS_ENV_FILE}");
        println!("    aws s3 mb s3://test-bucket");
    } else {
        eprintln!("컨테이너 시작 실패. `prelik run ministack logs --tail 50`으로 확인하세요.");
    }
}

fn ministack_uninstall(force: bool) {
    println!("=== MiniStack 제거 ===\n");

    if !Path::new(COMPOSE_FILE).exists() {
        println!("MiniStack이 설치되어 있지 않습니다.");
        return;
    }

    if !force {
        eprintln!("경고: 모든 MiniStack 데이터가 삭제됩니다.");
        eprintln!("정말 제거하려면 --force 플래그를 추가하세요:");
        eprintln!("  prelik run ministack uninstall --force");
        std::process::exit(1);
    }

    println!("컨테이너 중지 및 제거...");
    let _ = Command::new("docker")
        .args(["compose", "-f", COMPOSE_FILE, "down", "-v"])
        .status();

    if let Err(e) = std::fs::remove_dir_all(INSTALL_DIR) {
        eprintln!("디렉토리 삭제 실패: {e}");
        return;
    }

    let _ = std::fs::remove_file(AWS_ENV_FILE);
    println!("  MiniStack 제거 완료 ({INSTALL_DIR} + 환경변수 삭제됨)");
}

fn ministack_reset() {
    println!("MiniStack 상태 초기화...");

    let port = saved_port();
    let url = format!("http://localhost:{port}/_ministack/reset");
    let result = Command::new("curl")
        .args(["-sf", "--max-time", "5", "-X", "POST", &url])
        .output();
    match result {
        Ok(out) if out.status.success() => println!("  초기화 완료 (모든 AWS 리소스 삭제됨)"),
        _ => eprintln!("  초기화 실패 (MiniStack이 실행 중인지 확인: prelik run ministack status)"),
    }
}

fn ministack_update() {
    require_installed();
    require_docker();

    println!("MiniStack 업데이트...\n");

    // 현재 이미지 digest
    let old_digest = Command::new("docker")
        .args(["inspect", "--format", "{{.Id}}", DOCKER_IMAGE])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    println!("이미지 pull...");
    if common::run("docker", &["compose", "-f", COMPOSE_FILE, "pull"]).is_err() {
        eprintln!("이미지 pull 실패");
        return;
    }

    let new_digest = Command::new("docker")
        .args(["inspect", "--format", "{{.Id}}", DOCKER_IMAGE])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    if !old_digest.is_empty() && old_digest == new_digest {
        println!("\n  이미 최신 버전입니다.");
        return;
    }

    println!("컨테이너 재생성...");
    if common::run("docker", &["compose", "-f", COMPOSE_FILE, "up", "-d", "--force-recreate"]).is_ok() {
        println!("\n  업데이트 완료");
    }
}
