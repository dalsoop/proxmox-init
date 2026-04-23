//! pxi-code-server — 기존 LXC에 code-server 설치/제거.
//! VS Code 웹 에디터 + systemd + Traefik 라우트 + git auto-sync.

use clap::{Parser, Subcommand};
use pxi_core::common;

#[derive(Parser)]
#[command(name = "pxi-code-server", about = "code-server (VS Code 웹) 설치/제거")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// code-server 설치 (LXC 내부)
    Install {
        #[arg(long)]
        vmid: String,
        /// 리스닝 포트
        #[arg(long, default_value = "8443")]
        port: String,
        /// 웹 접속 비밀번호 (미지정 시 랜덤 생성)
        #[arg(long)]
        password: Option<String>,
        /// 시작 폴더
        #[arg(long, default_value = "/root")]
        folder: String,
        /// Traefik 라우트용 도메인 (미지정 시 라우트 생략)
        #[arg(long)]
        domain: Option<String>,
        /// git auto-sync cron 설치 (5분 간격 pull+push)
        #[arg(long)]
        git_sync: bool,
    },
    /// code-server 제거 + Traefik 라우트 제거
    Remove {
        #[arg(long)]
        vmid: String,
    },
    Doctor,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if !matches!(cli.cmd, Cmd::Doctor) && !common::command_exists("pct") {
        anyhow::bail!("pct 없음 — Proxmox 호스트에서만 동작");
    }
    match cli.cmd {
        Cmd::Install {
            vmid,
            port,
            password,
            folder,
            domain,
            git_sync,
        } => install(
            &vmid,
            &port,
            password.as_deref(),
            &folder,
            domain.as_deref(),
            git_sync,
        ),
        Cmd::Remove { vmid } => remove(&vmid),
        Cmd::Doctor => {
            doctor();
            Ok(())
        }
    }
}

fn install(
    vmid: &str,
    port: &str,
    password: Option<&str>,
    folder: &str,
    domain: Option<&str>,
    git_sync: bool,
) -> anyhow::Result<()> {
    println!("=== code-server 설치: LXC {vmid} ===\n");

    common::ensure_lxc_running(vmid)?;

    // 비밀번호: 지정값 또는 랜덤 생성
    let pw = match password {
        Some(p) => p.to_string(),
        None => common::run_capture("openssl", &["rand", "-hex", "12"])
            .unwrap_or_else(|_| "changeme".to_string()),
    };

    // [1/5] code-server 설치
    println!("[1/5] code-server 설치...");
    let install_script = r#"
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
curl -fsSL https://code-server.dev/install.sh | sh -s -- --method standalone 2>&1 | tail -3
echo "done"
"#;
    common::pct_exec_passthrough(vmid, &["bash", "-c", install_script])?;

    // [2/5] config.yaml
    println!("[2/5] 설정 (포트 {port}, 폴더 {folder})...");
    let auth = if pw == "none" { "none" } else { "password" };
    let config_script = format!(
        r#"
mkdir -p ~/.config/code-server
cat > ~/.config/code-server/config.yaml << CFGEOF
bind-addr: 0.0.0.0:{port}
auth: {auth}
password: {pw}
cert: false
CFGEOF
echo "config.yaml 생성"
"#
    );
    common::pct_exec_passthrough(vmid, &["bash", "-c", &config_script])?;

    // [3/5] systemd 서비스
    println!("[3/5] systemd 서비스...");
    let svc_script = format!(
        r#"
set -euo pipefail
CS_BIN=$(find /root/.local/lib -name "code-server" -path "*/bin/*" -type f -executable 2>/dev/null | head -1)
if [ -z "$CS_BIN" ]; then
  CS_BIN=$(command -v code-server 2>/dev/null || true)
fi
if [ -z "$CS_BIN" ]; then
  echo "code-server 바이너리를 찾을 수 없음" >&2
  exit 1
fi

cat > /etc/systemd/system/code-server.service << SVCEOF
[Unit]
Description=code-server (VS Code Web)
After=network.target

[Service]
Type=simple
ExecStart=$CS_BIN --config /root/.config/code-server/config.yaml {folder}
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
SVCEOF

systemctl daemon-reload
systemctl enable --now code-server
systemctl restart code-server
sleep 2
systemctl is-active code-server
"#
    );
    let out = common::pct_exec(vmid, &["bash", "-c", &svc_script])?;
    if out.contains("active") {
        println!("  code-server 서비스 active");
    } else {
        eprintln!("  서비스 시작 확인 필요: {out}");
    }

    // [4/5] Traefik 라우트 (도메인 지정 시)
    if let Some(dom) = domain {
        println!("[4/5] Traefik 라우트 ({dom})...");
        let route_name = format!("code-{vmid}");
        let backend = format!("http://{{{{ip}}}}:{port}");
        match common::run(
            "pxi-traefik",
            &[
                "route-add",
                "--name",
                &route_name,
                "--domain",
                dom,
                "--backend",
                &backend,
                "--vmid",
                vmid,
            ],
        ) {
            Ok(_) => println!("  Traefik 라우트 추가"),
            Err(e) => eprintln!("  Traefik 라우트 실패 (수동 설정 필요): {e}"),
        }
    } else {
        println!("[4/5] 도메인 미지정 — Traefik 라우트 생략");
    }

    // [5/5] git auto-sync
    if git_sync {
        println!("[5/5] git auto-sync cron 설치...");
        let sync_script = format!(
            r#"
cat > /usr/local/bin/git-auto-sync.sh << 'SYNCEOF'
#!/bin/bash
# git auto-sync: pull + add + commit + push (5분마다 cron)
set -euo pipefail
REPO_DIR="{folder}"
cd "$REPO_DIR"
if [ ! -d .git ]; then
  exit 0
fi
git pull --rebase --autostash 2>/dev/null || true
if [ -n "$(git status --porcelain)" ]; then
  git add -A
  git commit -m "auto-sync $(date +%Y-%m-%d\ %H:%M)" --no-gpg-sign 2>/dev/null || true
  git push 2>/dev/null || true
fi
SYNCEOF
chmod +x /usr/local/bin/git-auto-sync.sh

# cron 등록 (멱등)
CRON_LINE="*/5 * * * * /usr/local/bin/git-auto-sync.sh >/dev/null 2>&1"
( crontab -l 2>/dev/null | grep -v git-auto-sync; echo "$CRON_LINE" ) | crontab -
echo "git auto-sync cron 설치 (5분 간격)"
"#
        );
        common::pct_exec_passthrough(vmid, &["bash", "-c", &sync_script])?;
    } else {
        println!("[5/5] git-sync 미지정 — 생략");
    }

    println!("\n=== code-server 설치 완료 ===");
    println!("  VMID:     {vmid}");
    println!("  포트:     {port}");
    println!("  비밀번호: {pw}");
    println!("  폴더:     {folder}");
    if let Some(dom) = domain {
        println!("  URL:      https://{dom}/");
    }
    Ok(())
}

fn remove(vmid: &str) -> anyhow::Result<()> {
    println!("=== code-server 제거: LXC {vmid} ===");

    common::ensure_lxc_running(vmid)?;

    common::pct_exec_passthrough(
        vmid,
        &[
            "bash",
            "-c",
            "systemctl disable --now code-server 2>/dev/null || true; \
         rm -f /etc/systemd/system/code-server.service; \
         systemctl daemon-reload; \
         rm -rf ~/.local/lib/code-server-* ~/.config/code-server; \
         crontab -l 2>/dev/null | grep -v git-auto-sync | crontab - 2>/dev/null || true; \
         rm -f /usr/local/bin/git-auto-sync.sh; \
         echo 'code-server 제거 완료'",
        ],
    )?;

    // Traefik 라우트 제거
    let route_name = format!("code-{vmid}");
    match common::run("pxi-traefik", &["route-remove", "--name", &route_name]) {
        Ok(_) => println!("Traefik 라우트 제거"),
        Err(_) => println!("  (Traefik 라우트 없음 또는 제거 실패 — 무시)"),
    }

    println!("완료");
    Ok(())
}

fn doctor() {
    println!("=== pxi-code-server doctor ===");
    println!(
        "  pct:          {}",
        if common::command_exists("pct") {
            "OK"
        } else {
            "NOT FOUND"
        }
    );
    println!(
        "  code-server:  {}",
        if common::command_exists("code-server") {
            "OK"
        } else {
            "- (LXC 내부 설치)"
        }
    );
}
