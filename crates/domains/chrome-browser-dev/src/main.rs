//! pxi-chrome-browser-dev — Proxmox Chrome 빌더 환경 관리 (Helium chromium 빌드 전용).
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use pxi_core::common;
use pxi_core::types::Vmid;
use std::fs;
use std::process::Command;

const PROVISION_SCRIPT: &str = include_str!("../scripts/provision.sh");
const WRAPPER_SCRIPT: &str = include_str!("../scripts/chromium-browser-dev");

#[derive(Parser)]
#[command(
    name = "pxi-chrome-browser-dev",
    about = "Chromium/Helium 브라우저 빌드 LXC와 캐시 관리"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 환경 점검
    Doctor,
    /// LXC 생성. 기본값: 48 cores, 128G RAM, 32G swap, 800G disk.
    Create {
        #[arg(long)]
        vmid: Vmid,
        #[arg(long, default_value = "chromium-browser-dev")]
        hostname: String,
        #[arg(long)]
        ip: String,
        #[arg(
            long,
            default_value = "local:vztmpl/debian-13-standard_13.1-2_amd64.tar.zst"
        )]
        template: String,
        #[arg(long, default_value = "local-lvm")]
        storage: String,
        #[arg(long, default_value = "800")]
        disk: String,
        #[arg(long, default_value = "48")]
        cores: String,
        #[arg(long, default_value = "131072")]
        memory: String,
        #[arg(long, default_value = "32768")] // LINT_ALLOW: chrome-browser-dev 기본 VMID
        swap: String,
        #[arg(long, default_value = "vmbr1")]
        bridge: String,
        #[arg(long, default_value = "10.0.50.1")] // LINT_ALLOW: PVE 호스트 gateway IP 기본값
        gateway: String,
        #[arg(long)]
        start: bool,
    },
    /// 빌드 의존성, builder 계정, 명시적 chromium-browser-dev wrapper 설치
    Provision {
        #[arg(long)]
        vmid: String,
    },
    /// Helium Linux 레포와 submodule clone
    Clone {
        #[arg(long)]
        vmid: String,
        #[arg(long, default_value = "https://github.com/imputnet/helium-linux.git")]
        repo: String,
        #[arg(long)]
        force: bool,
    },
    /// create/provision/clone을 한 번에 실행
    Setup {
        #[arg(long)]
        vmid: Vmid,
        #[arg(long, default_value = "chromium-browser-dev")]
        hostname: String,
        #[arg(long)]
        ip: String,
        #[arg(
            long,
            default_value = "local:vztmpl/debian-13-standard_13.1-2_amd64.tar.zst"
        )]
        template: String,
        #[arg(long, default_value = "local-lvm")]
        storage: String,
        #[arg(long, default_value = "800")]
        disk: String,
        #[arg(long, default_value = "48")]
        cores: String,
        #[arg(long, default_value = "131072")]
        memory: String,
        #[arg(long, default_value = "32768")] // LINT_ALLOW: chrome-browser-dev 기본 VMID
        swap: String,
        #[arg(long, default_value = "vmbr1")]
        bridge: String,
        #[arg(long, default_value = "10.0.50.1")] // LINT_ALLOW: PVE 호스트 gateway IP 기본값
        gateway: String,
        #[arg(long, default_value = "https://github.com/imputnet/helium-linux.git")]
        repo: String,
    },
    /// Helium setup을 tmux에서 실행. 다운로드 캐시와 out 디렉터리를 재사용한다.
    SourceSetup {
        #[arg(long)]
        vmid: String,
    },
    /// 브라우저 빌드를 tmux에서 시작
    BuildStart {
        #[arg(long)]
        vmid: String,
        #[arg(long, default_value = "helium-dev")]
        profile: String,
    },
    /// 빌드 진행률/자원 상태 출력
    BuildStatus {
        #[arg(long)]
        vmid: String,
    },
    /// 빌드된 Helium 버전 출력
    Version {
        #[arg(long)]
        vmid: String,
    },
    /// 빌드 산출물 경로 출력
    Paths {
        #[arg(long)]
        vmid: String,
    },
    /// 빌드된 Helium을 Xvfb에서 foreground 실행
    Run {
        #[arg(long)]
        vmid: String,
        #[arg(long, default_value = "about:blank")]
        url: String,
        /// 실행 지속 시간. 0이면 종료하지 않고 foreground 유지.
        #[arg(long, default_value_t = 0)]
        timeout_sec: u64,
    },
    /// ChromeDriver 버전 출력
    DriverVersion {
        #[arg(long)]
        vmid: String,
    },
    /// ChromeDriver를 LXC 내부 tmux에서 시작
    DriverStart {
        #[arg(long)]
        vmid: String,
        #[arg(long, default_value_t = 9515)]
        port: u16,
    },
    /// setup/build 로그 출력
    Logs {
        #[arg(long)]
        vmid: String,
        #[arg(long, default_value = "build")]
        kind: String,
        #[arg(long, default_value_t = 120)]
        lines: usize,
    },
    /// 베이스 스냅샷 생성
    SnapshotBase {
        #[arg(long)]
        vmid: String,
        #[arg(long, default_value = "chromium-builder-base")]
        name: String,
    },
    /// 상태 조회
    Status {
        #[arg(long)]
        vmid: String,
    },
    /// LLM/사람 공용 작업공간 형식 생성 (workflow cards + build profiles)
    InitWorkspace {
        #[arg(long)]
        vmid: String,
    },
    /// 새 워크플로우 카드 생성
    WorkflowNew {
        #[arg(long)]
        vmid: String,
        /// 카드 ID. 예: tabs-refactor, captcha-handoff
        #[arg(long)]
        id: String,
        /// 목표 한 문장
        #[arg(long)]
        goal: String,
        /// 작업 영역. 예: helium/core, helium/settings, chromium/net
        #[arg(long, default_value = "helium/core")]
        area: String,
        /// narrow / moderate / broad
        #[arg(long, default_value = "narrow")]
        risk: String,
        /// 영향받는 patch 파일. 여러 번 지정 가능.
        #[arg(long = "patch")]
        patches: Vec<String>,
        /// 영향받는 Chromium 경로. 여러 번 지정 가능.
        #[arg(long = "path")]
        paths: Vec<String>,
        /// 검증 명령/체크. 여러 번 지정 가능.
        #[arg(long = "verify")]
        verifications: Vec<String>,
    },
    /// 워크플로우 카드 목록
    WorkflowList {
        #[arg(long)]
        vmid: String,
    },
    /// 워크플로우 카드 출력
    WorkflowShow {
        #[arg(long)]
        vmid: String,
        #[arg(long)]
        id: String,
    },
    /// 워크플로우 카드의 patch/path dependency가 실제로 존재하는지 검증
    WorkflowCheck {
        #[arg(long)]
        vmid: String,
        #[arg(long)]
        id: String,
    },
    /// 환경 의존/하드코딩 가정을 일반 workflow card로 생성
    CardAssumptions {
        #[arg(long)]
        vmid: String,
    },
    /// workflow card 전체와 환경 가정 상태 점검
    CardCheck {
        #[arg(long)]
        vmid: String,
    },
    /// 빌드 프로파일 출력
    ProfileShow {
        #[arg(long)]
        vmid: String,
        #[arg(long, default_value = "helium-dev")]
        name: String,
    },
    /// 의존성/캐시/패치/빌드 출력 상태 점검
    Check {
        #[arg(long)]
        vmid: String,
    },
    /// GitLab SSH/remote 기본 설정
    GitlabSetup {
        #[arg(long)]
        vmid: String,
        /// GitLab host. 예: gitlab.internal.kr (GITLAB_HOST env 또는 --host 로 override)
        #[arg(long, default_value = "gitlab.internal.kr")] // LINT_ALLOW: 내부 GitLab 기본 호스트, --host 로 override
        host: String,
        /// remote 이름
        #[arg(long, default_value = "gitlab")]
        remote: String,
        /// helium-linux에 추가할 remote URL
        #[arg(long)]
        platform_remote_url: Option<String>,
        /// helium-chromium에 추가할 remote URL
        #[arg(long)]
        main_remote_url: Option<String>,
    },
    /// LXC 내부 X11/Xvfb 시뮬레이션 의존성 점검/설치
    X11Setup {
        #[arg(long)]
        vmid: String,
    },
    /// 빌드된 Helium을 Xvfb에서 실행해 X11 smoke test
    X11Simulate {
        #[arg(long)]
        vmid: String,
        #[arg(long, default_value_t = 30)]
        timeout_sec: u64,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Doctor => doctor(),
        Cmd::Create {
            vmid,
            hostname,
            ip,
            template,
            storage,
            disk,
            cores,
            memory,
            swap,
            bridge,
            gateway,
            start,
        } => create(
            vmid.as_str(),
            &hostname,
            &ip,
            &template,
            &storage,
            &disk,
            &cores,
            &memory,
            &swap,
            &bridge,
            &gateway,
            start,
        ),
        Cmd::Provision { vmid } => provision(&vmid),
        Cmd::Clone { vmid, repo, force } => clone_repo(&vmid, &repo, force),
        Cmd::Setup {
            vmid,
            hostname,
            ip,
            template,
            storage,
            disk,
            cores,
            memory,
            swap,
            bridge,
            gateway,
            repo,
        } => {
            create(
                vmid.as_str(),
                &hostname,
                &ip,
                &template,
                &storage,
                &disk,
                &cores,
                &memory,
                &swap,
                &bridge,
                &gateway,
                true,
            )?;
            provision(vmid.as_str())?;
            clone_repo(vmid.as_str(), &repo, false)
        }
        Cmd::SourceSetup { vmid } => tmux_command(
            &vmid,
            "chromium-setup",
            "chromium-browser-dev setup",
            "chromium-setup.log",
        ),
        Cmd::BuildStart { vmid, profile } => build_start(&vmid, &profile),
        Cmd::BuildStatus { vmid } => build_status(&vmid),
        Cmd::Version { vmid } => browser_version(&vmid),
        Cmd::Paths { vmid } => artifact_paths(&vmid),
        Cmd::Run {
            vmid,
            url,
            timeout_sec,
        } => run_browser(&vmid, &url, timeout_sec),
        Cmd::DriverVersion { vmid } => driver_version(&vmid),
        Cmd::DriverStart { vmid, port } => driver_start(&vmid, port),
        Cmd::Logs { vmid, kind, lines } => logs(&vmid, &kind, lines),
        Cmd::SnapshotBase { vmid, name } => snapshot_base(&vmid, &name),
        Cmd::Status { vmid } => status(&vmid),
        Cmd::InitWorkspace { vmid } => init_workspace(&vmid),
        Cmd::WorkflowNew {
            vmid,
            id,
            goal,
            area,
            risk,
            patches,
            paths,
            verifications,
        } => workflow_new(
            &vmid,
            &id,
            &goal,
            &area,
            &risk,
            &patches,
            &paths,
            &verifications,
        ),
        Cmd::WorkflowList { vmid } => workflow_list(&vmid),
        Cmd::WorkflowShow { vmid, id } => workflow_show(&vmid, &id),
        Cmd::WorkflowCheck { vmid, id } => workflow_check(&vmid, &id),
        Cmd::CardAssumptions { vmid } => card_assumptions(&vmid),
        Cmd::CardCheck { vmid } => card_check(&vmid),
        Cmd::ProfileShow { vmid, name } => profile_show(&vmid, &name),
        Cmd::Check { vmid } => check(&vmid),
        Cmd::GitlabSetup {
            vmid,
            host,
            remote,
            platform_remote_url,
            main_remote_url,
        } => gitlab_setup(
            &vmid,
            &host,
            &remote,
            platform_remote_url.as_deref(),
            main_remote_url.as_deref(),
        ),
        Cmd::X11Setup { vmid } => x11_setup(&vmid),
        Cmd::X11Simulate { vmid, timeout_sec } => x11_simulate(&vmid, timeout_sec),
    }
}

fn require_proxmox() -> Result<()> {
    if !common::has_cmd("pct") {
        anyhow::bail!("pct 없음 — Proxmox 호스트에서만 동작");
    }
    Ok(())
}

fn doctor() -> Result<()> {
    println!("=== chrome-browser-dev doctor ===");
    println!("pct: {}", common::has_cmd("pct"));
    println!("pveam: {}", common::has_cmd("pveam"));
    println!("pvesm: {}", common::has_cmd("pvesm"));
    if common::has_cmd("pct") {
        let list = common::run_capture("pct", &["list"]).unwrap_or_default();
        println!("lxc count: {}", list.lines().skip(1).count());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn create(
    vmid: &str,
    hostname: &str,
    ip: &str,
    template: &str,
    storage: &str,
    disk: &str,
    cores: &str,
    memory: &str,
    swap: &str,
    bridge: &str,
    gateway: &str,
    start: bool,
) -> Result<()> {
    require_proxmox()?;
    if pct_exists(vmid) {
        println!("[chrome-browser-dev] LXC {vmid} already exists; create skipped");
        if start {
            ensure_started(vmid)?;
        }
        return Ok(());
    }

    ensure_template_available(template)?;
    let net0 = format!("name=eth0,bridge={bridge},ip={ip},gw={gateway}");
    let rootfs = format!("{storage}:{disk}");
    common::run_passthrough(
        "pct",
        &[
            "create",
            vmid,
            template,
            "--hostname",
            hostname,
            "--cores",
            cores,
            "--memory",
            memory,
            "--swap",
            swap,
            "--rootfs",
            &rootfs,
            "--net0",
            &net0,
            "--features",
            "nesting=1,keyctl=1",
            "--unprivileged",
            "1",
            "--ostype",
            "debian",
            "--tags",
            "chrome-browser-dev,chromium,llm",
        ],
    )?;
    if start {
        ensure_started(vmid)?;
    }
    Ok(())
}

fn provision(vmid: &str) -> Result<()> {
    require_proxmox()?;
    ensure_started(vmid)?;
    push_text(
        vmid,
        "/tmp/chrome-browser-dev-provision.sh",
        PROVISION_SCRIPT,
        "0755",
    )?;
    push_text(vmid, "/tmp/chromium-browser-dev", WRAPPER_SCRIPT, "0755")?;
    common::run_passthrough(
        "pct",
        &[
            "exec",
            vmid,
            "--",
            "bash",
            "/tmp/chrome-browser-dev-provision.sh",
        ],
    )?;
    Ok(())
}

fn clone_repo(vmid: &str, repo: &str, force: bool) -> Result<()> {
    require_proxmox()?;
    ensure_started(vmid)?;
    let script = if force {
        format!(
            "set -euo pipefail; rm -rf ~/workspace/helium-linux; cd ~/workspace; git clone --recurse-submodules --depth 1 {} helium-linux",
            shell_quote(repo)
        )
    } else {
        format!(
            "set -euo pipefail; cd ~/workspace; test -d helium-linux/.git || git clone --recurse-submodules --depth 1 {} helium-linux; cd helium-linux; git submodule update --init --depth 1",
            shell_quote(repo)
        )
    };
    common::run_passthrough(
        "pct",
        &[
            "exec", vmid, "--", "runuser", "-l", "builder", "-c", &script,
        ],
    )?;
    Ok(())
}

fn tmux_command(vmid: &str, session: &str, command: &str, log: &str) -> Result<()> {
    require_proxmox()?;
    ensure_started(vmid)?;
    let script = format!(
        "set -euo pipefail; tmux has-session -t {session} 2>/dev/null && {{ echo 'tmux session already running: {session}'; exit 0; }}; rm -f ~/workspace/{log}; tmux new-session -d -s {session} \"bash -lc 'set -o pipefail; {command} 2>&1 | tee ~/workspace/{log}'\"; tmux list-sessions"
    );
    common::run_passthrough(
        "pct",
        &[
            "exec", vmid, "--", "runuser", "-l", "builder", "-c", &script,
        ],
    )?;
    Ok(())
}

fn build_start(vmid: &str, profile: &str) -> Result<()> {
    validate_card_id(profile)?;
    init_workspace(vmid)?;
    let script = format!(
        r#"set -euo pipefail
profile="$HOME/workspace/.pxi/chrome-browser-dev/build-profiles/{profile}.toml"
test -f "$profile" || {{ echo "missing build profile: $profile" >&2; exit 1; }}
grep -q 'build_command = "chromium-browser-dev build"' "$profile" || {{ echo "unsupported build profile command" >&2; exit 1; }}
tmux has-session -t chromium-build 2>/dev/null && {{ echo 'tmux session already running: chromium-build'; exit 0; }}
rm -f "$HOME/workspace/chromium-build.log"
tmux new-session -d -s chromium-build "bash -lc 'set -o pipefail; chromium-browser-dev build 2>&1 | tee ~/workspace/chromium-build.log'"
tmux list-sessions
"#
    );
    run_builder(vmid, &script)
}

fn build_status(vmid: &str) -> Result<()> {
    require_proxmox()?;
    ensure_started(vmid)?;
    let script = r#"echo "=== tmux ==="; tmux list-sessions 2>/dev/null || true
echo "=== progress ==="; grep -aoE '\[[0-9]+ active/[0-9]+ finished/[0-9]+ total\]' ~/workspace/chromium-build.log 2>/dev/null | tail -1 || true
echo "=== binaries ==="; ls -lh ~/workspace/helium-linux/build/src/out/Default/helium ~/workspace/helium-linux/build/src/out/Default/chromedriver 2>/dev/null || true
echo "=== disk ==="; df -h /
echo "=== memory ==="; free -h
echo "=== cache ==="; sccache --show-stats 2>/dev/null | head -25 || true"#;
    common::run_passthrough(
        "pct",
        &["exec", vmid, "--", "runuser", "-l", "builder", "-c", script],
    )?;
    Ok(())
}

fn browser_version(vmid: &str) -> Result<()> {
    let script =
        r#""$HOME/workspace/helium-linux/build/src/out/Default/helium" --version 2>&1 | head -5"#;
    run_builder(vmid, script)
}

fn driver_version(vmid: &str) -> Result<()> {
    let script = r#""$HOME/workspace/helium-linux/build/src/out/Default/chromedriver" --version 2>&1 | head -5"#;
    run_builder(vmid, script)
}

fn artifact_paths(vmid: &str) -> Result<()> {
    let script = r#"set -euo pipefail
echo "helium=$HOME/workspace/helium-linux/build/src/out/Default/helium"
echo "chromedriver=$HOME/workspace/helium-linux/build/src/out/Default/chromedriver"
ls -lh "$HOME/workspace/helium-linux/build/src/out/Default/helium" "$HOME/workspace/helium-linux/build/src/out/Default/chromedriver"
"#;
    run_builder(vmid, script)
}

fn run_browser(vmid: &str, url: &str, timeout_sec: u64) -> Result<()> {
    validate_run_url(url)?;
    x11_setup(vmid)?;
    if timeout_sec == 0 {
        let script = format!(
            r#"set -euo pipefail
bin="$HOME/workspace/helium-linux/build/src/out/Default/helium"
test -x "$bin" || {{ echo "missing helium binary: $bin" >&2; exit 1; }}
rm -rf /tmp/helium-run-profile
exec xvfb-run -a "$bin" --no-sandbox --disable-dev-shm-usage --disable-gpu --no-first-run --user-data-dir=/tmp/helium-run-profile {url}
"#,
            url = shell_quote(url)
        );
        return run_builder(vmid, &script);
    }

    let script = format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
result=/tmp/helium-run.result
rm -f "$result"
bin="$HOME/workspace/helium-linux/build/src/out/Default/helium"
if [ ! -x "$bin" ]; then
  echo "FAIL missing helium binary: $bin" | tee "$result"
  exit 0
fi
rm -rf /tmp/helium-run-profile /tmp/helium-run.log
setsid xvfb-run -a "$bin" --no-sandbox --disable-dev-shm-usage --disable-gpu --no-first-run --user-data-dir=/tmp/helium-run-profile {url} >/tmp/helium-run.log 2>&1 &
pid=$!
sleep {timeout_sec}
if kill -0 "$pid" 2>/dev/null; then
  echo "OK browser ran for {timeout_sec}s under Xvfb" | tee "$result"
  kill -TERM "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  exit 0
fi
set +e
wait "$pid"
code=$?
set -e
if [ "$code" = "0" ]; then
  echo "OK browser exited cleanly" | tee "$result"
  exit 0
fi
{{
  echo "FAIL browser exit=$code"
  tail -n 80 /tmp/helium-run.log || true
}} | tee "$result"
exit 0
"#,
        timeout_sec = timeout_sec,
        url = shell_quote(url)
    );
    push_text(vmid, "/tmp/pxi-browser-run.sh", &script, "0755")?;
    common::run_passthrough(
        "pct",
        &[
            "exec",
            vmid,
            "--",
            "bash",
            "-lc",
            "runuser -l builder -c /tmp/pxi-browser-run.sh; true",
        ],
    )?;
    let result = common::run_capture(
        "pct",
        &["exec", vmid, "--", "cat", "/tmp/helium-run.result"],
    )?;
    println!("{result}");
    if result.starts_with("OK ") {
        Ok(())
    } else {
        anyhow::bail!("{result}")
    }
}

fn driver_start(vmid: &str, port: u16) -> Result<()> {
    let script = format!(
        r#"set -euo pipefail
driver="$HOME/workspace/helium-linux/build/src/out/Default/chromedriver"
test -x "$driver" || {{ echo "missing chromedriver: $driver" >&2; exit 1; }}
tmux has-session -t chromedriver 2>/dev/null && {{ echo "tmux session already running: chromedriver"; exit 0; }}
rm -f "$HOME/workspace/chromedriver.log"
tmux new-session -d -s chromedriver "bash -lc '$driver --port={port} --allowed-ips=0.0.0.0 2>&1 | tee ~/workspace/chromedriver.log'"
tmux list-sessions
"#,
        port = port
    );
    run_builder(vmid, &script)
}

fn logs(vmid: &str, kind: &str, lines: usize) -> Result<()> {
    require_proxmox()?;
    let file = match kind {
        "setup" => "chromium-setup.log",
        "build" => "chromium-build.log",
        other => anyhow::bail!("unknown log kind: {other} (expected setup|build)"),
    };
    let script = format!("tail -n {} ~/workspace/{file}", lines);
    common::run_passthrough(
        "pct",
        &[
            "exec", vmid, "--", "runuser", "-l", "builder", "-c", &script,
        ],
    )?;
    Ok(())
}

fn snapshot_base(vmid: &str, name: &str) -> Result<()> {
    require_proxmox()?;
    let description = "chrome-browser-dev base: deps, wrapper, caches, source/build state";
    common::run_passthrough(
        "pct",
        &["snapshot", vmid, name, "--description", description],
    )?;
    Ok(())
}

fn status(vmid: &str) -> Result<()> {
    require_proxmox()?;
    common::run_passthrough("pct", &["status", vmid])?;
    common::run_passthrough("pct", &["config", vmid])?;
    Ok(())
}

fn pct_exists(vmid: &str) -> bool {
    Command::new("pct")
        .args(["status", vmid])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn ensure_started(vmid: &str) -> Result<()> {
    if !pct_exists(vmid) {
        anyhow::bail!("LXC {vmid} not found");
    }
    let status = common::run_capture("pct", &["status", vmid])?;
    if !status.contains("running") {
        common::run_passthrough("pct", &["start", vmid])?;
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
    Ok(())
}

fn ensure_template_available(template: &str) -> Result<()> {
    if !template.contains(":vztmpl/") {
        return Ok(());
    }
    let (storage, file) = template
        .split_once(":vztmpl/")
        .context("invalid template format")?;
    let list = common::run_capture("pveam", &["list", storage]).unwrap_or_default();
    if list.contains(file) {
        return Ok(());
    }
    println!("[chrome-browser-dev] downloading LXC template {file} to {storage}");
    common::run_passthrough("pveam", &["download", storage, file])?;
    Ok(())
}

fn push_text(vmid: &str, path: &str, content: &str, perms: &str) -> Result<()> {
    let tmp = common::run_capture("mktemp", &["-t", "pxi-chrome-browser-dev.XXXXXXXX"])?;
    fs::write(&tmp, content)?;
    common::run_passthrough("chmod", &[perms, &tmp])?;
    common::run_passthrough("pct", &["push", vmid, &tmp, path, "--perms", perms])?;
    let _ = fs::remove_file(tmp);
    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn init_workspace(vmid: &str) -> Result<()> {
    require_proxmox()?;
    ensure_started(vmid)?;
    let script = r#"set -euo pipefail
root="$HOME/workspace/.pxi/chrome-browser-dev"
mkdir -p "$root/cards" "$root/build-profiles" "$root/state"
if [ -d "$root/workflow-cards" ]; then
  mkdir -p "$root/state/archive"
  mv "$root/workflow-cards" "$root/state/archive/workflow-cards-md.$(date +%s)" || true
fi

cat >"$root/build-profiles/helium-dev.toml" <<'EOF'
# pxi chrome-browser-dev build profile
# Purpose: keep Chromium/Helium builds reproducible and incremental.

[source]
platform_repo = "/home/builder/workspace/helium-linux"
main_repo = "/home/builder/workspace/helium-linux/helium-chromium"
chromium_tree = "/home/builder/workspace/helium-linux/build/src"
output_dir = "/home/builder/workspace/helium-linux/build/src/out/Default"

[cache]
download_cache = "/home/builder/workspace/helium-linux/build/download_cache"
ccache_dir = "/home/builder/.cache/ccache"
sccache_dir = "/home/builder/.cache/sccache"
keep_source_tree = true
keep_output_dir = true

[build]
setup_command = "chromium-browser-dev setup"
build_command = "chromium-browser-dev build"
run_command = "chromium-browser-dev run"
targets = ["chrome", "chromedriver"]
jobs = 48

[dependency_guards]
must_keep = [
  "build/download_cache",
  "build/src/out/Default/args.gn",
  "build/src/out/Default/.ninja_log",
  "patches/series",
  "patches/series.merged",
  "helium-chromium/patches/series",
]
before_edit = [
  "pxi chrome-browser-dev check --vmid <vmid>",
  "pxi chrome-browser-dev workflow-new --vmid <vmid> --id <id> --goal <goal>",
]
after_edit = [
  "chromium-browser-dev patch-push",
  "chromium-browser-dev build",
  "pxi chrome-browser-dev check --vmid <vmid>",
]
EOF

cat >"$root/cards/TEMPLATE.toml" <<'EOF'
schema_version = 1
id = "example-task"
type = "task"
goal = ""
area = "helium/core"
risk = "narrow"
owner = "builder"
created = ""
non_goals = ["visual redesign", "CAPTCHA bypass", "broad engine rewrite"]

[baseline]
platform_repo = "helium-linux"
platform_rev = ""
main_repo = "helium-chromium"
main_rev = ""
build_progress = ""

[dependencies]
patches = []
chromium_paths = []
generated_files = []
build_profile = "helium-dev"

[workflow]
before_edit = [
  "Read affected patch files",
  "Confirm patch order in patches/series or helium-chromium/patches/series",
  "pxi chrome-browser-dev check --vmid <vmid>",
]
change_plan = []

[verification]
commands = []

[completion]
changed_files = []
build_evidence = []
known_gaps = []
EOF


cat >"$root/README.md" <<'EOF'
# chrome-browser-dev workspace

This directory is managed by pxi.

- cards/: one card per implementation task
- build-profiles/: source/cache/build dependency contract
- cards/assumption-*.toml: hardcoded paths, network values, ports, and environment assumptions
- state/: generated state and future machine-readable summaries

Do not delete build/download_cache or build/src/out/Default unless you want a
slow clean rebuild.
EOF

echo "$root"
"#;
    run_builder(vmid, script)
}

fn workflow_new(
    vmid: &str,
    id: &str,
    goal: &str,
    area: &str,
    risk: &str,
    patches: &[String],
    paths: &[String],
    verifications: &[String],
) -> Result<()> {
    validate_card_id(id)?;
    init_workspace(vmid)?;
    let script = format!(
        r#"set -euo pipefail
root="$HOME/workspace/.pxi/chrome-browser-dev"
card="$root/cards/{id}.toml"
if [ -e "$card" ]; then
  echo "card already exists: $card" >&2
  exit 1
fi
created="$(date -Iseconds)"
platform_rev="$(git -C "$HOME/workspace/helium-linux" rev-parse --short HEAD 2>/dev/null || true)"
main_rev="$(git -C "$HOME/workspace/helium-linux/helium-chromium" rev-parse --short HEAD 2>/dev/null || true)"
progress="$(grep -aoE '\[[0-9]+ active/[0-9]+ finished/[0-9]+ total\]' "$HOME/workspace/chromium-build.log" 2>/dev/null | tail -1 || true)"
cat >"$card" <<EOF
schema_version = 1
id = {id_toml}
type = "task"
goal = {goal_toml}
area = {area_toml}
risk = {risk_toml}
owner = "builder"
created = "$created"
non_goals = ["visual redesign", "CAPTCHA bypass", "broad engine rewrite"]

[baseline]
platform_repo = "helium-linux"
platform_rev = "$platform_rev"
main_repo = "helium-chromium"
main_rev = "$main_rev"
build_progress = "$progress"

[dependencies]
patches = {patches_toml}
chromium_paths = {paths_toml}
generated_files = []
build_profile = "helium-dev"

[workflow]
before_edit = [
  "Read affected patch files",
  "Confirm patch order in patches/series or helium-chromium/patches/series",
  "pxi chrome-browser-dev check --vmid <vmid>",
]
change_plan = []

[verification]
commands = {verify_toml}

[completion]
changed_files = []
build_evidence = []
known_gaps = []
EOF
echo "$card"
"#,
        id = id,
        id_toml = toml_string(id),
        goal_toml = toml_string(goal),
        area_toml = toml_string(area),
        risk_toml = toml_string(risk),
        patches_toml = toml_array(patches),
        paths_toml = toml_array(paths),
        verify_toml = toml_array(verifications),
    );
    run_builder(vmid, &script)
}

fn workflow_list(vmid: &str) -> Result<()> {
    init_workspace(vmid)?;
    run_builder(
        vmid,
        r#"find "$HOME/workspace/.pxi/chrome-browser-dev/cards" -maxdepth 1 -type f -name '*.toml' -printf '%f\n' | sort"#,
    )
}

fn workflow_show(vmid: &str, id: &str) -> Result<()> {
    validate_card_id(id)?;
    init_workspace(vmid)?;
    let script = format!(
        r#"cat "$HOME/workspace/.pxi/chrome-browser-dev/cards/{id}.toml""#,
        id = id
    );
    run_builder(vmid, &script)
}

fn workflow_check(vmid: &str, id: &str) -> Result<()> {
    validate_card_id(id)?;
    init_workspace(vmid)?;
    let script = format!(
        r###"set -euo pipefail
card="$HOME/workspace/.pxi/chrome-browser-dev/cards/{id}.toml"
test -f "$card" || {{ echo "missing card: $card" >&2; exit 1; }}
cd "$HOME/workspace/helium-linux"
echo "=== workflow-check: {id} ==="
python3 - "$card" <<'PYCARD'
from pathlib import Path
import sys
try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib

card = Path(sys.argv[1])
data = tomllib.loads(card.read_text())
deps = data.get("dependencies", {{}})
verify = data.get("verification", {{}})
patches = deps.get("patches", [])
paths = deps.get("chromium_paths", [])
verifications = verify.get("commands", [])

print("schema_version=" + str(data.get("schema_version", "missing")))
print("type=" + str(data.get("type", "missing")))
print("patches=" + str(len(patches)))
for rel in patches:
    candidates = [Path(rel), Path("patches") / rel, Path("helium-chromium/patches") / rel]
    ok = any(p.exists() for p in candidates)
    print(("OK   " if ok else "MISS ") + "patch " + rel)

print("paths=" + str(len(paths)))
for rel in paths:
    path = Path("build/src") / rel
    print(("OK   " if path.exists() else "MISS ") + "path " + rel)

print("verifications=" + str(len(verifications)))
for item in verifications:
    print("TODO verify " + item)

if data.get("type") == "task" and not patches and not paths:
    print("WARN no dependency boundaries recorded")
PYCARD
"###,
        id = id
    );
    run_builder(vmid, &script)
}

fn profile_show(vmid: &str, name: &str) -> Result<()> {
    validate_card_id(name)?;
    init_workspace(vmid)?;
    let script = format!(
        r#"cat "$HOME/workspace/.pxi/chrome-browser-dev/build-profiles/{name}.toml""#,
        name = name
    );
    run_builder(vmid, &script)
}

fn check(vmid: &str) -> Result<()> {
    init_workspace(vmid)?;
    let script = r#"set -euo pipefail
echo "=== chrome-browser-dev check ==="
profile="$HOME/workspace/.pxi/chrome-browser-dev/build-profiles/helium-dev.toml"
paths=(
  "$HOME/workspace/helium-linux"
  "$HOME/workspace/helium-linux/helium-chromium"
  "$HOME/workspace/helium-linux/build/download_cache"
  "$HOME/workspace/helium-linux/build/src"
  "$HOME/workspace/helium-linux/build/src/out/Default"
  "$HOME/workspace/helium-linux/build/src/out/Default/args.gn"
  "$HOME/workspace/helium-linux/helium-chromium/patches/series"
  "$profile"
)
for p in "${paths[@]}"; do
  if [ -e "$p" ]; then
    echo "OK   $p"
  else
    echo "MISS $p"
  fi
done
if [ -e "$HOME/workspace/helium-linux/patches/series" ]; then
  echo "OK   platform patches: unmerged series present"
elif [ -e "$HOME/workspace/helium-linux/patches/series.merged" ]; then
  echo "OK   platform patches: merged series present"
else
  echo "MISS platform patches: neither series nor series.merged exists"
fi
echo "=== git ==="
git -C "$HOME/workspace/helium-linux" status --short | sed -n '1,40p' || true
echo "=== build progress ==="
grep -aoE '\[[0-9]+ active/[0-9]+ finished/[0-9]+ total\]' "$HOME/workspace/chromium-build.log" 2>/dev/null | tail -1 || true
echo "=== binaries ==="
ls -lh "$HOME/workspace/helium-linux/build/src/out/Default/helium" "$HOME/workspace/helium-linux/build/src/out/Default/chromedriver" 2>/dev/null || true
echo "=== gitlab ==="
git -C "$HOME/workspace/helium-linux" remote -v | grep -E '^gitlab\s' || true
git -C "$HOME/workspace/helium-linux/helium-chromium" remote -v | grep -E '^gitlab\s' || true
echo "=== x11 ==="
command -v xvfb-run >/dev/null && echo "OK   xvfb-run" || echo "MISS xvfb-run"
test -x "$HOME/workspace/helium-linux/build/src/out/Default/helium" && echo "OK   helium binary" || echo "MISS helium binary"
echo "=== cards ==="
find "$HOME/workspace/.pxi/chrome-browser-dev/cards" -maxdepth 1 -type f -name '*.toml' -printf '%f\n' 2>/dev/null | sort || true
"#;
    run_builder(vmid, script)
}

fn card_assumptions(vmid: &str) -> Result<()> {
    init_workspace(vmid)?;
    let script = r#"set -euo pipefail
root="$HOME/workspace/.pxi/chrome-browser-dev/cards"
mkdir -p "$root"

write_card() {
  id="$1"
  title="$2"
  value="$3"
  owner="$4"
  reason="$5"
  check="$6"
  file="$root/assumption-$id.toml"
  python3 - "$file" "$id" "$title" "$value" "$owner" "$reason" "$check" <<'PYASSUME'
from pathlib import Path
import sys

def q(value: str) -> str:
    return '"' + value.replace('\\', '\\\\').replace('"', '\\"') + '"'

file, id_, title, value, owner, reason, check = sys.argv[1:]
text = """schema_version = 1
id = {id_value}
type = "assumption"
title = {title}
value = {value}
owner = {owner}
reason = {reason}
change_policy = "update this card before changing code or provisioning scripts"

[dependencies]
patches = []
chromium_paths = []
generated_files = []
build_profile = "helium-dev"

[check]
command = {check}

[verification]
commands = [
  "pxi chrome-browser-dev card-check --vmid <vmid>",
  "pxi chrome-browser-dev check --vmid <vmid>",
]

[completion]
changed_files = []
build_evidence = []
known_gaps = []
""".format(
    id_value=q('assumption-' + id_),
    title=q(title),
    value=q(value),
    owner=q(owner),
    reason=q(reason),
    check=q(check),
)
Path(file).write_text(text)
PYASSUME
}

write_card "lxc-network" "Proxmox LXC bridge/gateway/IP convention" "vmbr1 / 10.0.50.1 / 10.0.50.220/16" "pxi host" "local Proxmox lab network convention" "ip addr show eth0; ip route" # LINT_ALLOW: 카드 문서 — 실제 네트워크 토폴로지 기록
write_card "lxc-resources" "Chromium builder resource profile" "48 cores / 128GiB RAM / 32GiB swap / 800G rootfs" "pxi host" "Chromium builds need high parallelism and disk headroom" "nproc; free -h; df -h /"
write_card "builder-user" "Builder account and workspace root" "builder / /home/builder/workspace" "container" "all build caches and cards live under a non-root user" "id builder; ls -ld /home/builder/workspace"
write_card "source-paths" "Helium source and output paths" "/home/builder/workspace/helium-linux + build/src/out/Default" "container" "incremental builds depend on stable paths" "test -d /home/builder/workspace/helium-linux; test -f /home/builder/workspace/helium-linux/build/src/out/Default/args.gn"
write_card "gitlab-remote" "GitLab SSH remotes" "git@10.0.50.63:root/helium*.git" "gitlab" "gitlab.internal.kr DNS currently points elsewhere for SSH" "git -C /home/builder/workspace/helium-linux remote -v; git -C /home/builder/workspace/helium-linux/helium-chromium remote -v" # LINT_ALLOW: 카드 문서 — 실제 GitLab SSH 주소 기록
write_card "chromedriver-port" "ChromeDriver port" "9515" "container" "default WebDriver port for local automation" "ss -ltnp | grep 9515 || true"
write_card "x11-runtime" "X11 simulation runtime" "xvfb-run + /tmp/helium-x11-profile" "container" "LXC has no physical display; simulation uses Xvfb" "command -v xvfb-run; test -x /home/builder/workspace/helium-linux/build/src/out/Default/helium"

# Migrate old markdown cards if present.
if find "$root" -maxdepth 1 -type f -name '*.md' | grep -q .; then
  archive="$HOME/workspace/.pxi/chrome-browser-dev/state/archive/cards-md.$(date +%s)"
  mkdir -p "$archive"
  find "$root" -maxdepth 1 -type f -name '*.md' -exec mv {} "$archive" \;
fi
if [ -d "$HOME/workspace/.pxi/chrome-browser-dev/hardcode-cards" ]; then
  mkdir -p "$HOME/workspace/.pxi/chrome-browser-dev/state/archive"
  mv "$HOME/workspace/.pxi/chrome-browser-dev/hardcode-cards" "$HOME/workspace/.pxi/chrome-browser-dev/state/archive/hardcode-cards.$(date +%s)" || true
fi

find "$root" -maxdepth 1 -type f -name 'assumption-*.toml' -printf '%f\n' | sort
"#;
    run_builder(vmid, script)
}

fn card_check(vmid: &str) -> Result<()> {
    card_assumptions(vmid)?;
    let script = r#"set -euo pipefail
echo "=== card-check ==="
root="$HOME/workspace/.pxi/chrome-browser-dev/cards"
find "$root" -maxdepth 1 -type f -name '*.toml' -printf 'CARD %f\n' | sort
echo "=== assumption checks ==="
echo "network:"; ip -br addr show eth0; ip route | sed -n '1,5p'
echo "resources:"; nproc; free -h | sed -n '1,3p'; df -h / | sed -n '1,2p'
echo "source paths:"; test -d "$HOME/workspace/helium-linux" && echo OK helium-linux || echo MISS helium-linux; test -x "$HOME/workspace/helium-linux/build/src/out/Default/helium" && echo OK helium || echo MISS helium
echo "gitlab remote:"; git -C "$HOME/workspace/helium-linux" remote -v | grep -E '^gitlab\s' || echo MISS platform gitlab remote; git -C "$HOME/workspace/helium-linux/helium-chromium" remote -v | grep -E '^gitlab\s' || echo MISS main gitlab remote
echo "x11/chromedriver:"; command -v xvfb-run; ss -ltnp | grep 9515 || true
"#;
    run_builder(vmid, script)
}

fn gitlab_setup(
    vmid: &str,
    host: &str,
    remote: &str,
    platform_remote_url: Option<&str>,
    main_remote_url: Option<&str>,
) -> Result<()> {
    validate_remote_name(remote)?;
    require_proxmox()?;
    ensure_started(vmid)?;
    let platform_remote = platform_remote_url.unwrap_or("");
    let main_remote = main_remote_url.unwrap_or("");
    let script = format!(
        r#"set -euo pipefail
mkdir -p "$HOME/.ssh"
chmod 700 "$HOME/.ssh"
ssh-keyscan -H {host} >> "$HOME/.ssh/known_hosts" 2>/dev/null || true
sort -u "$HOME/.ssh/known_hosts" -o "$HOME/.ssh/known_hosts" 2>/dev/null || true
chmod 600 "$HOME/.ssh/known_hosts" 2>/dev/null || true

if [ ! -f "$HOME/.ssh/id_ed25519" ]; then
  ssh-keygen -t ed25519 -N '' -f "$HOME/.ssh/id_ed25519" -C "builder@chrome-browser-dev"
fi

git config --global init.defaultBranch main
git config --global pull.rebase true
git config --global --add safe.directory "$HOME/workspace/helium-linux"
git config --global --add safe.directory "$HOME/workspace/helium-linux/helium-chromium"

add_remote() {{
  repo="$1"
  url="$2"
  [ -z "$url" ] && return 0
  if git -C "$repo" remote get-url {remote} >/dev/null 2>&1; then
    git -C "$repo" remote set-url {remote} "$url"
  else
    git -C "$repo" remote add {remote} "$url"
  fi
}}

add_remote "$HOME/workspace/helium-linux" {platform_remote}
add_remote "$HOME/workspace/helium-linux/helium-chromium" {main_remote}

echo "=== GitLab SSH public key ==="
cat "$HOME/.ssh/id_ed25519.pub"
echo "=== remotes ==="
git -C "$HOME/workspace/helium-linux" remote -v
git -C "$HOME/workspace/helium-linux/helium-chromium" remote -v
"#,
        host = shell_quote(host),
        remote = remote,
        platform_remote = shell_quote(platform_remote),
        main_remote = shell_quote(main_remote),
    );
    run_builder(vmid, &script)
}

fn x11_setup(vmid: &str) -> Result<()> {
    require_proxmox()?;
    ensure_started(vmid)?;
    let script = r#"set -euo pipefail
if ! command -v xvfb-run >/dev/null 2>&1 || ! command -v xauth >/dev/null 2>&1; then
  sudo DEBIAN_FRONTEND=noninteractive apt-get update -qq
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y xvfb xauth x11-apps dbus-x11 >/dev/null
fi
mkdir -p "$HOME/workspace/.pxi/chrome-browser-dev/state"
cat > "$HOME/workspace/.pxi/chrome-browser-dev/state/x11.env" <<'EOF'
DISPLAY=auto-via-xvfb-run
HELIUM_BINARY=/home/builder/workspace/helium-linux/build/src/out/Default/helium
USER_DATA_DIR=/tmp/helium-x11-profile
EOF
echo "OK x11 deps"
command -v xvfb-run
command -v xauth
"#;
    run_builder(vmid, script)
}

fn x11_simulate(vmid: &str, timeout_sec: u64) -> Result<()> {
    x11_setup(vmid)?;
    let script = format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
result=/tmp/helium-x11-sim.result
rm -f "$result"
bin="$HOME/workspace/helium-linux/build/src/out/Default/helium"
if [ ! -x "$bin" ]; then
  echo "FAIL missing helium binary: $bin" | tee "$result"
  exit 0
fi
rm -rf /tmp/helium-x11-profile /tmp/helium-x11-smoke.log
setsid xvfb-run -a "$bin" \
  --no-sandbox \
  --disable-dev-shm-usage \
  --disable-gpu \
  --no-first-run \
  --user-data-dir=/tmp/helium-x11-profile \
  about:blank >/tmp/helium-x11-smoke.log 2>&1 &
pid=$!
sleep {timeout_sec}
if kill -0 "$pid" 2>/dev/null; then
  echo "OK x11 simulation: browser stayed alive for {timeout_sec}s under Xvfb" | tee "$result"
  kill -TERM "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  exit 0
fi
set +e
wait "$pid"
code=$?
set -e
if [ "$code" = "0" ]; then
  echo "OK x11 simulation: browser exited cleanly" | tee "$result"
  exit 0
fi
{{
  echo "FAIL x11 simulation exit=$code"
  tail -n 80 /tmp/helium-x11-smoke.log || true
}} | tee "$result"
exit 0
"#
    );
    push_text(vmid, "/tmp/pxi-x11-simulate.sh", &script, "0755")?;
    common::run_passthrough(
        "pct",
        &[
            "exec",
            vmid,
            "--",
            "bash",
            "-lc",
            "runuser -l builder -c /tmp/pxi-x11-simulate.sh; true",
        ],
    )?;
    let result = common::run_capture(
        "pct",
        &["exec", vmid, "--", "cat", "/tmp/helium-x11-sim.result"],
    )?;
    println!("{result}");
    if result.starts_with("OK ") {
        Ok(())
    } else {
        anyhow::bail!("{result}")
    }
}

fn run_builder(vmid: &str, script: &str) -> Result<()> {
    require_proxmox()?;
    ensure_started(vmid)?;
    common::run_passthrough(
        "pct",
        &["exec", vmid, "--", "runuser", "-l", "builder", "-c", script],
    )
}

fn validate_card_id(id: &str) -> Result<()> {
    let ok = id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if id.is_empty() || !ok {
        anyhow::bail!("id/name은 소문자, 숫자, '-', '_'만 허용: {id}");
    }
    Ok(())
}

fn validate_remote_name(name: &str) -> Result<()> {
    let ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if name.is_empty() || !ok {
        anyhow::bail!("remote 이름은 영문/숫자/'-'/'_'만 허용: {name}");
    }
    Ok(())
}

fn validate_run_url(url: &str) -> Result<()> {
    let allowed = ["about:", "http://", "https://", "file://"];
    if allowed.iter().any(|prefix| url.starts_with(prefix)) {
        return Ok(());
    }
    anyhow::bail!("지원하지 않는 URL 형식: {url} (about:/http://https://file://만 허용)")
}

fn toml_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}

fn toml_array(values: &[String]) -> String {
    let items = values
        .iter()
        .map(|value| toml_string(value))
        .collect::<Vec<_>>();
    format!("[{}]", items.join(", "))
}
