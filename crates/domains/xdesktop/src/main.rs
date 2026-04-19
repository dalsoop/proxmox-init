//! pxi-xdesktop — X11 원격 데스크톱 LXC (Xpra + 한글 + Helium).
//! Debian 13 기반 LXC에 Xpra HTML5 데스크톱을 설치한다.
//!   - 로케일: ko_KR.UTF-8
//!   - 입력기: fcitx5 + fcitx5-hangul (Chromium/Helium 호환)
//!   - 데스크톱: XFCE4
//!   - 브라우저: Helium (ungoogled-chromium 기반 포크)
//!   - 원격: Xpra start-desktop + HTML5 클라이언트 내장 (bind-tcp, 인증 없음)

use clap::{Parser, Subcommand};
use pxi_core::common;

const INSTALL_SCRIPT: &str = include_str!("../scripts/install-desktop.sh");
const DEV_SCRIPT: &str = include_str!("../scripts/dev-setup.sh");

/// LXC 내부 경로에 문자열 컨텐츠를 기록 (tempfile → pct push).
fn write_to_lxc(vmid: &str, lxc_path: &str, content: &str) -> anyhow::Result<()> {
    let out = common::run_capture("mktemp", &["-t", "pxi-xdesktop.XXXXXXXX"])?;
    let tmp = out.trim().to_string();
    let _guard = TempGuard(tmp.clone());
    common::run("chmod", &["600", &tmp])?;
    std::fs::write(&tmp, content)?;
    common::run("pct", &["push", vmid, &tmp, lxc_path])?;
    Ok(())
}

struct TempGuard(String);
impl Drop for TempGuard {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
}

#[derive(Parser)]
#[command(name = "pxi-xdesktop", about = "X11 원격 데스크톱 LXC 설치 관리 (Xpra + 한글 + Helium)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 전체 배포: LXC 생성 → 데스크톱 설치 → (선택) traefik 라우트 등록
    Setup {
        #[arg(long)] vmid: String,
        #[arg(long)] hostname: String,
        /// IP CIDR (예: 10.0.50.210/16)
        #[arg(long)] ip: String,
        /// traefik 공개 호스트 (예: xdesktop.50.internal.kr). 지정 시 라우트 등록
        #[arg(long)] host: Option<String>,
        #[arg(long, default_value = "4")] cores: String,
        #[arg(long, default_value = "4096")] memory: String,
        #[arg(long, default_value = "20")] disk: String,
        #[arg(long, default_value = "14500")] port: String,
        #[arg(long, default_value = "xuser")] user: String,
        #[arg(long, default_value = "0.11.2.1")] helium_tag: String,
    },
    /// 이미 존재하는 LXC에 데스크톱만 설치 (LXC 재사용)
    Install {
        #[arg(long)] vmid: String,
        #[arg(long, default_value = "14500")] port: String,
        #[arg(long, default_value = "xuser")] user: String,
        #[arg(long, default_value = "0.11.2.1")] helium_tag: String,
    },
    /// traefik 라우트만 등록 (이미 설치된 LXC 대상)
    Expose {
        #[arg(long)] vmid: String,
        #[arg(long)] host: String,
        #[arg(long, default_value = "14500")] port: String,
    },
    /// 상태 조회 (LXC + Xpra systemd + 포트)
    Status {
        #[arg(long)] vmid: String,
    },
    /// LXC 제거 + traefik 라우트 자동 정리
    Destroy {
        #[arg(long)] vmid: String,
        /// 확인 없이 제거
        #[arg(long)] yes: bool,
    },
    /// 공개 URL 스모크 테스트 — HTTP/WebSocket 경로 전수 확인
    Verify {
        #[arg(long)] vmid: String,
        /// 공개 호스트 (생략 시 lxc-ip 직접)
        #[arg(long)] host: Option<String>,
        #[arg(long, default_value = "14500")] port: String,
    },
    /// 개발 도구 설치 — git/gh/node/rust/vscodium + GitHub SSH key + 레포 clone
    Dev {
        #[arg(long)] vmid: String,
        /// 데스크톱 유저 (기본 xuser)
        #[arg(long, default_value = "xuser")] user: String,
        /// GitHub 유저명 (힌트용 — 실제 auth 는 gh login 수동)
        #[arg(long)] github_user: Option<String>,
        /// clone 할 레포 쉼표 구분 (예: "dalsoop/proxmox-init,imputnet/helium-linux")
        #[arg(long)] repos: Option<String>,
        /// VSCodium 설치 스킵
        #[arg(long)] no_vscodium: bool,
    },
    /// 환경 점검 (pct + pxi-lxc 등 존재 확인)
    Doctor,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if !matches!(cli.cmd, Cmd::Doctor) && !common::has_cmd("pct") {
        anyhow::bail!("pct 없음 — Proxmox 호스트에서만 동작");
    }
    match cli.cmd {
        Cmd::Setup { vmid, hostname, ip, host, cores, memory, disk, port, user, helium_tag } => {
            setup(&vmid, &hostname, &ip, host.as_deref(), &cores, &memory, &disk, &port, &user, &helium_tag)
        }
        Cmd::Install { vmid, port, user, helium_tag } => install(&vmid, &port, &user, &helium_tag),
        Cmd::Expose { vmid, host, port } => expose(&vmid, &host, &port),
        Cmd::Status { vmid } => status(&vmid),
        Cmd::Destroy { vmid, yes } => destroy(&vmid, yes),
        Cmd::Verify { vmid, host, port } => verify(&vmid, host.as_deref(), &port),
        Cmd::Dev { vmid, user, github_user, repos, no_vscodium } => {
            dev(&vmid, &user, github_user.as_deref(), repos.as_deref(), !no_vscodium)
        }
        Cmd::Doctor => { doctor(); Ok(()) }
    }
}

fn dev(vmid: &str, user: &str, github_user: Option<&str>, repos: Option<&str>, vscodium: bool) -> anyhow::Result<()> {
    println!("=== xdesktop dev 환경 설치: LXC {vmid} ({user}) ===\n");
    common::ensure_lxc_running(vmid)?;

    let script_path = "/root/xdesktop-dev.sh";
    write_to_lxc(vmid, script_path, DEV_SCRIPT)?;
    common::pct_exec(vmid, &["chmod", "+x", script_path])?;

    let env_prefix = format!(
        "XDESKTOP_USER={user} GITHUB_USER={gh} REPOS={repos} INSTALL_VSCODIUM={vs}",
        gh = github_user.unwrap_or(""),
        repos = repos.unwrap_or(""),
        vs = if vscodium { "1" } else { "0" },
    );
    let cmd = format!("{env_prefix} bash {script_path}");
    common::pct_exec_passthrough(vmid, &["bash", "-lc", &cmd])?;
    Ok(())
}

fn setup(
    vmid: &str, hostname: &str, ip: &str, host: Option<&str>,
    cores: &str, memory: &str, disk: &str, port: &str,
    user: &str, helium_tag: &str,
) -> anyhow::Result<()> {
    println!("=== xdesktop setup: LXC {vmid} ({hostname}, {ip}) ===\n");

    // 1. LXC 생성 (이미 있으면 스킵)
    let exists = common::run("pct", &["status", vmid]).is_ok();
    if exists {
        println!("  LXC {vmid} 이미 존재 — 생성 스킵");
    } else {
        println!("[1/3] LXC 생성");
        common::run("pxi-lxc", &[
            "create",
            "--vmid", vmid,
            "--hostname", hostname,
            "--ip", ip,
            "--cores", cores,
            "--memory", memory,
            "--disk", disk,
        ])?;

        // 기동 대기
        print!("  기동 대기");
        for _ in 0..30 {
            if common::pct_exec(vmid, &["true"]).is_ok() { println!(" ✓"); break; }
            print!(".");
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    // 2. 데스크톱 설치
    println!("\n[2/3] 데스크톱 설치 (수 분 소요)");
    install(vmid, port, user, helium_tag)?;

    // 3. traefik 라우트 (선택)
    if let Some(h) = host {
        println!("\n[3/3] traefik 라우트 등록: {h}");
        expose(vmid, h, port)?;
    } else {
        println!("\n[3/3] traefik 라우트 생략 (--host 미지정)");
    }

    // 접속 URL 안내
    let ip_only = ip.split('/').next().unwrap_or(ip);
    println!("\n======================================================================");
    println!(" 완료");
    println!("   VMID:   {vmid}");
    println!("   LXC IP: {ip_only}");
    println!("   Xpra:   http://{ip_only}:{port}/");
    if let Some(h) = host {
        println!("   공개:   https://{h}/");
    }
    println!("   유저:   {user}  (passwordless sudo)");
    println!("======================================================================");
    Ok(())
}

fn install(vmid: &str, port: &str, user: &str, helium_tag: &str) -> anyhow::Result<()> {
    common::ensure_lxc_running(vmid)?;

    // 설치 스크립트를 LXC 내부에 푸시
    let script_path = "/root/xdesktop-install.sh";
    write_to_lxc(vmid, script_path, INSTALL_SCRIPT)?;
    common::pct_exec(vmid, &["chmod", "+x", script_path])?;

    // 환경변수 전달 후 실행 (stdout/stderr 실시간 노출)
    let env_prefix = format!(
        "XDESKTOP_USER={user} XPRA_PORT={port} HELIUM_TAG={helium_tag}"
    );
    let cmd = format!("{env_prefix} bash {script_path}");
    common::pct_exec_passthrough(vmid, &["bash", "-lc", &cmd])?;

    Ok(())
}

fn expose(vmid: &str, host: &str, port: &str) -> anyhow::Result<()> {
    // 기본 레지스트리 등록 — pxi run service list 에 노출 + traefik dynamic/ 기본 yml 생성.
    // nginx HTTP/1.1 bridge 가 LXC 내부에서 Xpra 앞에 있으므로 traefik 특수 설정 불필요.
    let domain = host.splitn(2, '.').nth(1).unwrap_or("50.internal.kr");
    common::run("pxi-service", &[
        "add",
        "--domain", domain,
        "--name", &format!("xdesktop-{vmid}"),
        "--host", host,
        "--ip", &lxc_ip(vmid)?,
        "--port", port,
        "--vmid", vmid,
    ])?;
    println!("  ✓ traefik route 등록 완료");
    Ok(())
}

fn status(vmid: &str) -> anyhow::Result<()> {
    println!("=== xdesktop 상태: LXC {vmid} ===");

    // LXC
    let lxc_status = common::run_capture("pct", &["status", vmid])
        .unwrap_or_else(|_| "unknown".into());
    println!("  LXC:      {}", lxc_status.trim());

    if !lxc_status.contains("running") {
        println!("  (LXC 정지 — 이후 체크 스킵)");
        return Ok(());
    }

    // systemd
    let svc = common::pct_exec(vmid, &["systemctl", "is-active", "xpra-xdesktop"])
        .unwrap_or_else(|_| "inactive\n".into());
    println!("  xpra:     {}", svc.trim());

    // 포트 (nginx 14500 + Xpra 14501)
    let ports = common::pct_exec(vmid, &[
        "bash", "-c",
        "ss -tlnp 2>/dev/null | awk 'NR==1 || /:(14500|14501)\\>/' | head -5"
    ]).unwrap_or_default();
    println!("  포트:");
    for line in ports.lines() {
        println!("    {line}");
    }

    // 패치 상태 (업그레이드 내성 체크)
    let css_ok = common::pct_exec(vmid, &["grep", "-q", "xpra-pxi-overrides", "/usr/share/xpra/www/css/client.css"]).is_ok();
    let js_ok = common::pct_exec(vmid, &["grep", "-q", "server_is_desktop||this.client.server_is_shadow?(this.decorated", "/usr/share/xpra/www/js/Window.js"]).is_ok();
    let divert_ok = common::pct_exec(vmid, &["bash", "-c", "dpkg-divert --list | grep -q /usr/share/xpra/www/js/Window.js"]).is_ok();
    println!("  패치:     css={}  js={}  divert={}",
        if css_ok { "✓" } else { "✗" },
        if js_ok { "✓" } else { "✗" },
        if divert_ok { "✓" } else { "✗" });

    // 버전
    let versions = common::pct_exec(vmid, &[
        "bash", "-c",
        "dpkg-query -W -f='xpra=${Version}\\n' xpra 2>/dev/null; \
         dpkg-query -W -f='xpra-html5=${Version}\\n' xpra-html5 2>/dev/null; \
         dpkg-query -W -f='helium-bin=${Version}\\n' helium-bin 2>/dev/null; \
         dpkg-query -W -f='fcitx5=${Version}\\n' fcitx5 2>/dev/null"
    ]).unwrap_or_default();
    println!("  설치:");
    for line in versions.lines() {
        println!("    {line}");
    }
    Ok(())
}

fn destroy(vmid: &str, yes: bool) -> anyhow::Result<()> {
    if !yes {
        println!("정말 LXC {vmid} 를 제거할까요? --yes 플래그를 추가하세요.");
        anyhow::bail!("확인 필요");
    }
    // traefik 라우트 먼저 정리 (LXC 제거 전에 해야 lxc_ip 등 조회 불필요)
    let svc_name = format!("xdesktop-{vmid}");
    match common::run("pxi-service", &["remove", "--force", &svc_name]) {
        Ok(_) => println!("✓ traefik route 제거: {svc_name}"),
        Err(_) => println!("(traefik route {svc_name} 없음 — 스킵)"),
    }
    common::run("pct", &["stop", vmid]).ok();
    common::run("pct", &["destroy", vmid])?;
    println!("✓ LXC {vmid} 제거 완료");
    Ok(())
}

fn verify(vmid: &str, host: Option<&str>, port: &str) -> anyhow::Result<()> {
    println!("=== xdesktop verify: LXC {vmid} ===\n");
    let mut fails = 0;
    let mut check = |name: &str, ok: bool, detail: &str| {
        let mark = if ok { "✓" } else { fails += 1; "✗" };
        println!("  {mark} {name:<35} {detail}");
    };

    // 1. LXC 기동
    let running = common::run_capture("pct", &["status", vmid])
        .map(|s| s.contains("running")).unwrap_or(false);
    check("LXC running", running, "");
    if !running { anyhow::bail!("LXC 정지 — 이후 체크 생략"); }

    // 2. systemd 서비스
    for svc in ["xpra-xdesktop", "nginx"] {
        let active = common::pct_exec(vmid, &["systemctl", "is-active", svc])
            .map(|s| s.trim() == "active").unwrap_or(false);
        check(&format!("systemd {svc}"), active, "");
    }

    // 3. 포트 리스닝 (nginx + xpra)
    let p_inner: u32 = port.parse::<u32>().map(|p| p + 1).unwrap_or(14501);
    for (name, p) in [("nginx", port.to_string()), ("xpra (127.0.0.1)", p_inner.to_string())] {
        let listening = common::pct_exec(vmid, &[
            "bash", "-c",
            &format!("ss -tlnp 2>/dev/null | grep -q ':{p}\\>'")
        ]).is_ok();
        check(&format!("port {p} ({name})"), listening, "");
    }

    // 4. CSS 패치 적용 상태 — .windowhead hide 실존 여부
    let css_ok = common::pct_exec(vmid, &[
        "grep", "-q", "xpra-pxi-overrides", "/usr/share/xpra/www/css/client.css",
    ]).is_ok();
    check("CSS override applied", css_ok, ".windowhead 숨김 (드래그 offset 해결)");

    // 5. dpkg-divert 보호 — css + js 둘 다
    let divert_css = common::pct_exec(vmid, &[
        "bash", "-c",
        "dpkg-divert --list | grep -q /usr/share/xpra/www/css/client.css",
    ]).is_ok();
    let divert_js = common::pct_exec(vmid, &[
        "bash", "-c",
        "dpkg-divert --list | grep -q /usr/share/xpra/www/js/Window.js",
    ]).is_ok();
    check("dpkg-divert 보호", divert_css && divert_js, "apt upgrade 에도 패치 유지");

    // 6. LXC 내부 HTTP 응답
    let ip = lxc_ip(vmid).unwrap_or_else(|_| "-".into());
    let inner_ok = std::process::Command::new("curl")
        .args(["-sf", "-o", "/dev/null", &format!("http://{ip}:{port}/css/client.css")])
        .status().map(|s| s.success()).unwrap_or(false);
    check(&format!("HTTP {ip}:{port}/css/client.css"), inner_ok, "");

    // 7. 공개 URL (host 지정 시) — HTML + CSS + connect.html 모두 reachable
    if let Some(h) = host {
        for path in ["/", "/connect.html", "/css/client.css"] {
            let url = format!("https://{h}{path}");
            let code = std::process::Command::new("curl")
                .args(["-sk", "-o", "/dev/null", "-w", "%{http_code}", &url])
                .output().ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                .unwrap_or_default();
            let ok = code.trim() == "200";
            check(&format!("HTTPS {h}{path}"), ok, &format!("status={code}"));
        }
        // 공개 URL 에서 실제 CSS 안에 우리 override 포함되는지 (압축본·캐시 스테일 감지)
        let content_ok = std::process::Command::new("curl")
            .args(["-sk", &format!("https://{h}/css/client.css")])
            .output().ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("xpra-pxi-overrides"))
            .unwrap_or(false);
        check("HTTPS CSS contains override", content_ok, "Safari 가 최신 CSS 받음");
    }

    println!();
    if fails == 0 {
        println!("✓ 모든 체크 통과");
    } else {
        anyhow::bail!("{fails}개 체크 실패");
    }
    Ok(())
}

fn lxc_ip(vmid: &str) -> anyhow::Result<String> {
    // pct config 에서 net0 IP 파싱 (ip=10.0.50.210/16 → 10.0.50.210)
    let cfg = common::run_capture("pct", &["config", vmid])?;
    for line in cfg.lines() {
        if let Some(rest) = line.strip_prefix("net0:") {
            for kv in rest.split(',') {
                if let Some(v) = kv.trim().strip_prefix("ip=") {
                    let ip = v.split('/').next().unwrap_or(v).trim();
                    if !ip.is_empty() && ip != "dhcp" {
                        return Ok(ip.to_string());
                    }
                }
            }
        }
    }
    anyhow::bail!("LXC {vmid} 의 IP 조회 실패")
}

fn doctor() {
    println!("=== pxi-xdesktop doctor ===");
    println!("  pct:         {}", if common::has_cmd("pct") { "✓" } else { "✗ (Proxmox 호스트 필요)" });
    println!("  pxi-lxc:     {}", if common::has_cmd("pxi-lxc") { "✓" } else { "✗" });
    println!("  pxi-service: {}", if common::has_cmd("pxi-service") { "✓" } else { "✗ (expose 사용 불가)" });
    println!("  install script: {} bytes", INSTALL_SCRIPT.len());
}
