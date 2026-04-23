//! pxi-infisical — Infisical 시크릿 관리 플랫폼.

use clap::{Parser, Subcommand};
use pxi_core::common;
use std::path::Path;
use std::process::Command;

#[derive(Parser)]
#[command(name = "pxi-infisical", about = "Infisical (시크릿 관리 플랫폼)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Docker Compose로 Infisical 설치
    Install {
        #[arg(long, default_value = "8082")]
        port: u16,
    },
    /// 제거
    Uninstall {
        #[arg(long)]
        force: bool,
    },
    /// 시작
    Start,
    /// 중지
    Stop,
    /// 재시작
    Restart,
    /// 상태
    Status,
    /// 로그 확인
    Logs {
        #[arg(long)]
        follow: bool,
        #[arg(long)]
        tail: Option<String>,
    },
    /// 업데이트
    Update,
    /// 환경 진단
    Doctor,
    /// SMTP 설정 주입
    Smtp {
        #[arg(long, default_value = "10.0.50.122")] // LINT_ALLOW: CF mail proxy 기본 IP
        host: String,
        #[arg(long, default_value_t = 587)]
        port: u16,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long, default_value = "Infisical")]
        from_name: String,
        #[arg(long)]
        secure: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Status => {
            common::run(
                "docker",
                &["compose", "-f", "/opt/infisical/docker-compose.yml", "ps"],
            );
        }
        Cmd::Start => {
            common::run(
                "docker",
                &[
                    "compose",
                    "-f",
                    "/opt/infisical/docker-compose.yml",
                    "up",
                    "-d",
                ],
            );
        }
        Cmd::Stop => {
            common::run(
                "docker",
                &["compose", "-f", "/opt/infisical/docker-compose.yml", "down"],
            );
        }
        Cmd::Restart => {
            common::run(
                "docker",
                &[
                    "compose",
                    "-f",
                    "/opt/infisical/docker-compose.yml",
                    "restart",
                ],
            );
        }
        Cmd::Logs { follow, tail } => {
            let mut args = vec!["compose", "-f", "/opt/infisical/docker-compose.yml", "logs"];
            if follow {
                args.push("-f");
            }
            if let Some(t) = &tail {
                args.push("--tail");
                args.push(t);
            }
            common::run("docker", &args);
        }
        Cmd::Install { port } => infisical_install(port),
        Cmd::Uninstall { force } => infisical_uninstall(force),
        Cmd::Update => infisical_update(),
        Cmd::Doctor => {
            doctor();
        }
        Cmd::Smtp {
            host,
            port,
            user,
            password,
            from,
            from_name,
            secure,
        } => {
            infisical_smtp(
                &host,
                port,
                user.as_deref(),
                password.as_deref(),
                from.as_deref(),
                &from_name,
                secure,
            );
        }
    }
    Ok(())
}

const INSTALL_DIR: &str = "/opt/infisical";
const COMPOSE_FILE: &str = "/opt/infisical/docker-compose.yml";
const ENV_FILE: &str = "/opt/infisical/.env";
const PORT_FILE: &str = "/opt/infisical/.port";

fn saved_port() -> u16 {
    std::fs::read_to_string(PORT_FILE)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(8080)
}

fn require_installed() {
    if !Path::new(COMPOSE_FILE).exists() {
        eprintln!(
            "Infisical이 설치되어 있지 않습니다. `pxi run infisical install`을 먼저 실행하세요."
        );
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

fn gen_hex(len: usize) -> String {
    let output = Command::new("openssl")
        .args(["rand", "-hex", &len.to_string()])
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "0".repeat(len * 2),
    }
}

fn gen_base64(len: usize) -> String {
    let output = Command::new("openssl")
        .args(["rand", "-base64", &len.to_string()])
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => gen_hex(len),
    }
}

fn infisical_install(port: u16) {
    println!("=== Infisical 설치 ===\n");
    require_docker();

    if Path::new(COMPOSE_FILE).exists() {
        println!("이미 설치됨: {INSTALL_DIR}");
        println!("재설치하려면 먼저 `pxi run infisical uninstall --force`를 실행하세요.");
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

    std::fs::create_dir_all(INSTALL_DIR).unwrap_or_else(|e| {
        eprintln!("설치 디렉토리 생성 실패: {e}");
        std::process::exit(1);
    });

    // .env 생성
    let encryption_key = gen_hex(16);
    let auth_secret = gen_base64(32);
    let pg_password = gen_hex(16);

    let env_content = format!(
        r#"# Infisical configuration (auto-generated by pxi)
ENCRYPTION_KEY={encryption_key}
AUTH_SECRET={auth_secret}

# PostgreSQL
POSTGRES_USER=infisical
POSTGRES_PASSWORD={pg_password}
POSTGRES_DB=infisical
DB_CONNECTION_URI=postgres://infisical:{pg_password}@db:5432/infisical

# Redis
REDIS_URL=redis://redis:6379

# Site
SITE_URL=http://localhost:{port}
"#
    );

    std::fs::write(ENV_FILE, &env_content).unwrap_or_else(|e| {
        eprintln!(".env 작성 실패: {e}");
        std::process::exit(1);
    });
    let _ = Command::new("chmod").args(["600", ENV_FILE]).status();

    // docker-compose.yml 생성
    let compose = format!(
        r#"services:
  backend:
    container_name: infisical-backend
    restart: unless-stopped
    depends_on:
      db:
        condition: service_healthy
      redis:
        condition: service_started
    image: infisical/infisical:latest
    env_file: .env
    ports:
      - "{port}:8080"
    environment:
      - NODE_ENV=production
    networks:
      - infisical

  redis:
    image: redis:7-alpine
    container_name: infisical-redis
    restart: always
    environment:
      - ALLOW_EMPTY_PASSWORD=yes
    networks:
      - infisical
    volumes:
      - redis_data:/data

  db:
    container_name: infisical-db
    image: postgres:14-alpine
    restart: always
    env_file: .env
    volumes:
      - pg_data:/var/lib/postgresql/data
    networks:
      - infisical
    healthcheck:
      test: "pg_isready --username=infisical && psql --username=infisical --list"
      interval: 5s
      timeout: 10s
      retries: 10

volumes:
  pg_data:
    driver: local
  redis_data:
    driver: local

networks:
  infisical:
"#
    );

    std::fs::write(COMPOSE_FILE, &compose).unwrap_or_else(|e| {
        eprintln!("docker-compose.yml 작성 실패: {e}");
        std::process::exit(1);
    });
    let _ = std::fs::write(PORT_FILE, port.to_string());

    println!("  docker-compose.yml 생성: {COMPOSE_FILE}");
    println!("  .env 생성 (암호화 키 자동 생성)");
    println!("  포트: {port}");

    println!("\n이미지 pull 중...");
    common::run("docker", &["compose", "-f", COMPOSE_FILE, "pull"]).ok();

    println!("컨테이너 시작 중...");
    if common::run("docker", &["compose", "-f", COMPOSE_FILE, "up", "-d"]).is_ok() {
        println!("\n  Infisical 설치 완료!");
        println!("  대시보드: http://localhost:{port}");
    } else {
        eprintln!("컨테이너 시작 실패. `pxi run infisical logs --tail 50`으로 확인하세요.");
    }
}

fn infisical_uninstall(force: bool) {
    println!("=== Infisical 제거 ===\n");

    if !Path::new(COMPOSE_FILE).exists() {
        println!("Infisical이 설치되어 있지 않습니다.");
        return;
    }

    if !force {
        eprintln!("경고: 모든 Infisical 데이터(시크릿, DB)가 삭제됩니다.");
        eprintln!("정말 제거하려면 --force 플래그를 추가하세요:");
        eprintln!("  pxi run infisical uninstall --force");
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

    println!("  Infisical 제거 완료 ({INSTALL_DIR} 삭제됨)");
}

fn infisical_update() {
    require_installed();
    require_docker();

    println!("Infisical 업데이트...\n");

    println!("이미지 pull...");
    if common::run("docker", &["compose", "-f", COMPOSE_FILE, "pull"]).is_err() {
        eprintln!("이미지 pull 실패");
        return;
    }

    println!("컨테이너 재생성...");
    if common::run(
        "docker",
        &[
            "compose",
            "-f",
            COMPOSE_FILE,
            "up",
            "-d",
            "--force-recreate",
        ],
    )
    .is_ok()
    {
        println!("\n  업데이트 완료");
    }
}

fn infisical_smtp(
    host: &str,
    port: u16,
    user: Option<&str>,
    password: Option<&str>,
    from: Option<&str>,
    from_name: &str,
    secure: bool,
) {
    require_installed();
    require_docker();

    let user = user
        .map(|s| s.to_string())
        .or_else(|| std::env::var("SMTP_USER").ok())
        .unwrap_or_else(|| {
            eprintln!("SMTP_USER 필요: --user 지정 또는 환경변수 SMTP_USER 설정");
            std::process::exit(1);
        });
    let password = password
        .map(|s| s.to_string())
        .or_else(|| std::env::var("SMTP_PASSWORD").ok())
        .unwrap_or_else(|| {
            eprintln!("SMTP_PASSWORD 필요: --password 지정 또는 환경변수 SMTP_PASSWORD 설정");
            std::process::exit(1);
        });
    let from_addr = from.map(|s| s.to_string()).unwrap_or_else(|| user.clone());

    println!("Infisical SMTP 설정 주입...");
    println!("  host={}:{} user={} from={}", host, port, user, from_addr);

    // Read existing .env, upsert SMTP keys, write back
    let existing = std::fs::read_to_string(ENV_FILE).unwrap_or_default();
    let pairs = [
        ("SMTP_HOST", host.to_string()),
        ("SMTP_PORT", port.to_string()),
        ("SMTP_USERNAME", user),
        ("SMTP_PASSWORD", password),
        ("SMTP_FROM_ADDRESS", from_addr),
        ("SMTP_FROM_NAME", from_name.to_string()),
        ("SMTP_SECURE", secure.to_string()),
        ("SMTP_IGNORE_TLS", "false".to_string()),
    ];

    let mut lines: Vec<String> = existing.lines().map(|l| l.to_string()).collect();
    let mut seen = std::collections::HashSet::new();

    for line in lines.iter_mut() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim().to_string();
            if let Some((_, v)) = pairs.iter().find(|(k, _)| *k == key) {
                *line = format!("{}={}", key, v);
                seen.insert(key);
            }
        }
    }
    for (k, v) in &pairs {
        if !seen.contains(*k) {
            lines.push(format!("{}={}", k, v));
        }
    }

    let mut joined = lines.join("\n");
    if !joined.ends_with('\n') {
        joined.push('\n');
    }
    std::fs::write(ENV_FILE, &joined).unwrap_or_else(|e| {
        eprintln!("Error: .env 수정 실패: {e}");
        std::process::exit(1);
    });
    println!("  .env 업데이트 완료");

    println!("backend 컨테이너 재생성...");
    if common::run(
        "docker",
        &[
            "compose",
            "-f",
            COMPOSE_FILE,
            "up",
            "-d",
            "--force-recreate",
            "backend",
        ],
    )
    .is_ok()
    {
        println!("  backend 재생성 완료");
    } else {
        eprintln!("Error: backend 재생성 실패");
        std::process::exit(1);
    }
    println!(
        "\n  확인: curl -sS http://127.0.0.1:{}/api/status | grep emailConfigured",
        saved_port()
    );
}

// ---------------------------------------------------------------------------
// doctor
// ---------------------------------------------------------------------------

fn doctor() {
    println!("=== pxi-infisical doctor ===\n");

    // Docker installed
    let docker_ok = Command::new("docker")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    println!("  {} docker", if docker_ok { "✓" } else { "✗" });

    // docker-compose.yml exists
    let compose_ok = Path::new(COMPOSE_FILE).exists();
    println!("  {} {}", if compose_ok { "✓" } else { "✗" }, COMPOSE_FILE);

    // Container running
    if compose_ok && docker_ok {
        let ps_ok = Command::new("docker")
            .args([
                "compose",
                "-f",
                COMPOSE_FILE,
                "ps",
                "--status",
                "running",
                "-q",
            ])
            .output()
            .map(|o| o.status.success() && !o.stdout.is_empty())
            .unwrap_or(false);
        println!("  {} 컨테이너 실행 중", if ps_ok { "✓" } else { "✗" });
    } else {
        println!("  ✗ 컨테이너 확인 불가 (docker/compose 없음)");
    }

    // Port reachable
    let port = saved_port();
    let port_ok = Command::new("curl")
        .args([
            "-sf",
            "--max-time",
            "5",
            &format!("http://localhost:{}", port),
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    println!(
        "  {} localhost:{} 응답",
        if port_ok { "✓" } else { "✗" },
        port
    );
}
