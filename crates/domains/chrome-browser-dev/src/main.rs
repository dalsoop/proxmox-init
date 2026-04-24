//! pxi-chrome-browser-dev — Proxmox Chrome 빌더 환경 관리 (Helium chromium 빌드 전용).
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use pxi_core::common;
use pxi_core::types::Vmid;
use std::fs;
use std::process::Command;

const PROVISION_SCRIPT: &str = include_str!("../scripts/provision.sh");
const WRAPPER_SCRIPT: &str = include_str!("../scripts/chromium-browser-dev");
const WIN_CROSS_PROVISION_SCRIPT: &str = include_str!("../scripts/win-cross-provision.sh");
const WIN_CROSS_SDK_SCRIPT: &str = include_str!("../scripts/win-cross-sdk");

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
    /// 빌드된 Helium을 tar.xz로 패키징하고 GitLab 릴리즈 생성
    Release {
        #[arg(long)]
        vmid: String,
        /// GitLab project ID (helium-linux)
        #[arg(long, default_value_t = 239)]
        project_id: u32,
        /// GitLab API token (미지정 시 GITLAB_API_TOKEN env 사용)
        #[arg(long)]
        token: Option<String>,
        /// GitLab base URL
        #[arg(long, default_value = "https://gitlab.internal.kr")] // LINT_ALLOW: helium-linux 내부 GitLab 기본값, --gitlab-url로 override
        gitlab_url: String,
    },
    /// Windows 크로스컴파일 의존성 설치 (clang, lld, depot_tools, msvc-wine)
    WinCrossProvision {
        #[arg(long)]
        vmid: String,
    },
    /// Windows SDK 다운로드 (msvc-wine vsdownload.py, tmux, ~30-60분)
    WinCrossSdk {
        #[arg(long)]
        vmid: String,
    },
    /// Windows 크로스컴파일 빌드 시작 (tmux)
    WinCrossBuild {
        #[arg(long)]
        vmid: String,
        /// GN output 디렉터리 이름
        #[arg(long, default_value = "Windows")]
        out_dir: String,
    },
    /// Windows 크로스빌드 진행 상태 조회
    WinCrossStatus {
        #[arg(long)]
        vmid: String,
    },
    /// Windows 빌드 결과물(.exe)을 GitLab 릴리즈로 업로드
    WinCrossRelease {
        #[arg(long)]
        vmid: String,
        #[arg(long, default_value_t = 239)]
        project_id: u32,
        #[arg(long)]
        token: Option<String>,
        #[arg(long, default_value = "https://gitlab.internal.kr")] // LINT_ALLOW: helium-linux 내부 GitLab 기본값, --gitlab-url로 override
        gitlab_url: String,
    },
    /// GitLab 릴리즈에서 Helium을 내려받아 로컬에 설치
    Install {
        /// 설치 디렉터리
        #[arg(long, default_value = "/opt/helium")]
        dir: String,
        /// bin symlink 위치
        #[arg(long, default_value = "/usr/local/bin")]
        bin_dir: String,
        /// GitLab project ID
        #[arg(long, default_value_t = 239)]
        project_id: u32,
        /// GitLab base URL
        #[arg(long, default_value = "https://gitlab.internal.kr")] // LINT_ALLOW: helium-linux 내부 GitLab 기본값, --gitlab-url로 override
        gitlab_url: String,
        /// GitLab API token
        #[arg(long)]
        token: Option<String>,
        /// 버전 태그 (미지정 시 latest)
        #[arg(long)]
        version: Option<String>,
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
        Cmd::WinCrossProvision { vmid } => win_cross_provision(&vmid),
        Cmd::WinCrossSdk { vmid } => win_cross_sdk(&vmid),
        Cmd::WinCrossBuild { vmid, out_dir } => win_cross_build(&vmid, &out_dir),
        Cmd::WinCrossStatus { vmid } => win_cross_status(&vmid),
        Cmd::WinCrossRelease {
            vmid,
            project_id,
            token,
            gitlab_url,
        } => win_cross_release(&vmid, project_id, token.as_deref(), &gitlab_url),
        Cmd::Release {
            vmid,
            project_id,
            token,
            gitlab_url,
        } => release(&vmid, project_id, token.as_deref(), &gitlab_url),
        Cmd::Install {
            dir,
            bin_dir,
            project_id,
            gitlab_url,
            token,
            version,
        } => install_helium(
            &dir,
            &bin_dir,
            project_id,
            &gitlab_url,
            token.as_deref(),
            version.as_deref(),
        ),
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

fn win_cross_provision(vmid: &str) -> Result<()> {
    require_proxmox()?;
    ensure_started(vmid)?;
    push_text(
        vmid,
        "/tmp/win-cross-provision.sh",
        WIN_CROSS_PROVISION_SCRIPT,
        "0755",
    )?;
    common::run_passthrough(
        "pct",
        &["exec", vmid, "--", "bash", "/tmp/win-cross-provision.sh"],
    )
}

fn win_cross_sdk(vmid: &str) -> Result<()> {
    require_proxmox()?;
    ensure_started(vmid)?;
    push_text(
        vmid,
        "/home/builder/win-cross-sdk.sh",
        WIN_CROSS_SDK_SCRIPT,
        "0755",
    )?;
    // SDK 다운로드는 30-60분 걸리므로 tmux에서 실행
    let script = r#"set -euo pipefail
tmux has-session -t win-cross-sdk 2>/dev/null && {
  echo 'tmux session already running: win-cross-sdk'
  exit 0
}
rm -f ~/win-cross-sdk.log
tmux new-session -d -s win-cross-sdk \
  "bash -lc 'set -o pipefail; ~/win-cross-sdk.sh 2>&1 | tee ~/win-cross-sdk.log'"
tmux list-sessions
echo ""
echo "SDK 다운로드 진행 중 — 로그 확인:"
echo "  pxi run chrome-browser-dev win-cross-status --vmid <vmid>"
"#;
    run_builder(vmid, script)
}

fn win_cross_build(vmid: &str, out_dir: &str) -> Result<()> {
    require_proxmox()?;
    ensure_started(vmid)?;

    let script = format!(
        r#"set -euo pipefail
source ~/.win-cross-env

SRC=/home/builder/workspace/helium-linux/build/src
OUT="$SRC/out/{out_dir}"

# SDK 준비 확인
if [ ! -d "$WIN_SDK_DIR/chromium-pkg/$TOOLCHAIN_HASH/win_sdk" ]; then
  echo "ERROR: Windows SDK 없음 — 먼저 win-cross-sdk 실행" >&2
  echo "  pxi run chrome-browser-dev win-cross-sdk --vmid <vmid>" >&2
  exit 1
fi

# GN args 생성
mkdir -p "$OUT"
cat > "$OUT/args.gn" <<GN_ARGS
target_os = "win"
target_cpu = "x64"
is_clang = true
is_debug = false
is_component_build = false
symbol_level = 0
enable_nacl = false
enable_linux_installer = false
GN_ARGS

echo "=== GN 설정 ==="
cd "$SRC"
HASH=$(grep '^TOOLCHAIN_HASH' build/vs_toolchain.py | cut -d"'" -f2)
PATH="$HOME/depot_tools:$PATH" \
DEPOT_TOOLS_WIN_TOOLCHAIN=0 \
DEPOT_TOOLS_WIN_TOOLCHAIN_BASE_URL="$DEPOT_TOOLS_WIN_TOOLCHAIN_BASE_URL" \
  env GYP_MSVS_HASH_$HASH="$HASH" \
  gn gen "out/{out_dir}" 2>&1

echo "=== 빌드 시작 (tmux: win-cross-build) ==="
tmux has-session -t win-cross-build 2>/dev/null && {{
  echo 'tmux session already running: win-cross-build'
  exit 0
}}
rm -f ~/workspace/win-cross-build.log
HASH=$(grep '^TOOLCHAIN_HASH' "$SRC/build/vs_toolchain.py" | cut -d"'" -f2)
tmux new-session -d -s win-cross-build \
  "bash -lc 'set -euo pipefail
source ~/.win-cross-env
cd /home/builder/workspace/helium-linux/build/src
HASH=\$(grep \"^TOOLCHAIN_HASH\" build/vs_toolchain.py | cut -d\"\\x27\" -f2)
PATH=\"\$HOME/depot_tools:\$PATH\" \
DEPOT_TOOLS_WIN_TOOLCHAIN=0 \
DEPOT_TOOLS_WIN_TOOLCHAIN_BASE_URL=\"\$DEPOT_TOOLS_WIN_TOOLCHAIN_BASE_URL\" \
  env GYP_MSVS_HASH_\$HASH=\"\$HASH\" \
  autoninja -C out/{out_dir} mini_installer 2>&1 | tee ~/workspace/win-cross-build.log'"
tmux list-sessions
echo ""
echo "빌드 진행 확인: pxi run chrome-browser-dev win-cross-status --vmid <vmid>"
"#,
        out_dir = out_dir,
    );
    run_builder(vmid, &script)
}

fn win_cross_status(vmid: &str) -> Result<()> {
    require_proxmox()?;
    ensure_started(vmid)?;
    let script = r#"echo "=== tmux sessions ==="
tmux list-sessions 2>/dev/null || echo "(없음)"

echo ""
echo "=== SDK 다운로드 로그 (마지막 5줄) ==="
tail -5 ~/win-cross-sdk.log 2>/dev/null || echo "(없음)"

echo ""
echo "=== 빌드 진행률 ==="
grep -aoE '\[[0-9]+ active/[0-9]+ finished/[0-9]+ total\]' \
  ~/workspace/win-cross-build.log 2>/dev/null | tail -1 || echo "(빌드 로그 없음)"
tail -3 ~/workspace/win-cross-build.log 2>/dev/null || true

echo ""
echo "=== Windows 빌드 결과물 ==="
ls -lh ~/workspace/helium-linux/build/src/out/Windows/mini_installer.exe \
        ~/workspace/helium-linux/build/src/out/Windows/helium.exe \
  2>/dev/null || echo "(아직 없음)"

echo ""
echo "=== 디스크 ==="
df -h / | tail -1
"#;
    common::run_passthrough(
        "pct",
        &["exec", vmid, "--", "runuser", "-l", "builder", "-c", script],
    )
}

fn win_cross_release(
    vmid: &str,
    project_id: u32,
    token: Option<&str>,
    gitlab_url: &str,
) -> Result<()> {
    let api_token = token
        .map(str::to_owned)
        .or_else(|| std::env::var("GITLAB_API_TOKEN").ok())
        .context("GitLab API token 없음 — --token 또는 GITLAB_API_TOKEN 환경변수 필요")?;

    // 버전 조회
    let ver_script = r#"python3 /home/builder/workspace/helium-linux/helium-chromium/utils/helium_version.py \
  --tree /home/builder/workspace/helium-linux/helium-chromium \
  --platform-tree /home/builder/workspace/helium-linux \
  --print"#;
    let version = common::run_capture(
        "pct",
        &[
            "exec", vmid, "--", "runuser", "-l", "builder", "-c", ver_script,
        ],
    )?
    .trim()
    .to_owned();
    if version.is_empty() {
        anyhow::bail!("버전을 가져오지 못함");
    }

    // mini_installer.exe 확인
    let exe_path = "/home/builder/workspace/helium-linux/build/src/out/Windows/mini_installer.exe";
    let check = common::run_capture(
        "pct",
        &[
            "exec", vmid, "--", "bash", "-c",
            &format!("test -f {exe_path} && echo ok || echo missing"),
        ],
    )?;
    if check.trim() != "ok" {
        anyhow::bail!(
            "mini_installer.exe 없음 — 먼저 win-cross-build 완료 후 실행하세요"
        );
    }

    let installer_name = format!("helium-{version}-windows-x64-installer.exe");
    let host_path = format!("/tmp/{installer_name}");

    // 호스트로 복사
    common::run_passthrough("pct", &["pull", vmid, exe_path, &host_path])?;

    // GitLab 업로드
    println!("[win-release] 업로드 중: {installer_name}");
    let upload_out = std::process::Command::new("curl")
        .args([
            "-s",
            "--header", &format!("PRIVATE-TOKEN: {api_token}"),
            "--form", &format!("file=@{host_path}"),
            &format!("{gitlab_url}/api/v4/projects/{project_id}/uploads"),
        ])
        .output()
        .context("curl 실행 실패")?;
    let upload_json = String::from_utf8_lossy(&upload_out.stdout);
    let asset_url = parse_json_field(&upload_json, "full_path")
        .map(|p| format!("{gitlab_url}{p}"))
        .context("업로드 응답에서 full_path 추출 실패")?;
    println!("[win-release] 업로드 완료: {asset_url}");

    // 기존 릴리즈에 Windows 에셋 추가
    let tag = format!("v{version}");
    let link_body = format!(
        r#"{{"name":"{installer_name}","url":"{asset_url}","link_type":"package"}}"#
    );
    let link_out = std::process::Command::new("curl")
        .args([
            "-s", "-X", "POST",
            "--header", &format!("PRIVATE-TOKEN: {api_token}"),
            "--header", "Content-Type: application/json",
            "--data", &link_body,
            &format!("{gitlab_url}/api/v4/projects/{project_id}/releases/{tag}/assets/links"),
        ])
        .output()
        .context("에셋 링크 추가 curl 실패")?;
    let link_resp = String::from_utf8_lossy(&link_out.stdout);
    if let Some(name) = parse_json_field(&link_resp, "name") {
        println!("[win-release] 완료: {name} → {gitlab_url}/root/helium-linux/-/releases/{tag}");
    } else {
        println!("[win-release] 응답: {link_resp}");
    }

    let _ = std::fs::remove_file(&host_path);
    Ok(())
}

fn release(vmid: &str, project_id: u32, token: Option<&str>, gitlab_url: &str) -> Result<()> {
    let api_token = token
        .map(str::to_owned)
        .or_else(|| std::env::var("GITLAB_API_TOKEN").ok())
        .context("GitLab API token 없음 — --token 또는 GITLAB_API_TOKEN 환경변수 필요")?;

    // 버전 조회
    let ver_script = r#"python3 /home/builder/workspace/helium-linux/helium-chromium/utils/helium_version.py \
  --tree /home/builder/workspace/helium-linux/helium-chromium \
  --platform-tree /home/builder/workspace/helium-linux \
  --print"#;
    let version = common::run_capture(
        "pct",
        &[
            "exec", vmid, "--", "runuser", "-l", "builder", "-c", ver_script,
        ],
    )?
    .trim()
    .to_owned();
    if version.is_empty() {
        anyhow::bail!("버전을 가져오지 못함");
    }
    println!("[release] version={version}");

    // 패키징 스크립트 (tar.xz 생성)
    let pkg_script = format!(
        r#"set -euo pipefail
WORKSPACE=/home/builder/workspace/helium-linux
BUILD_DIR=$WORKSPACE/build/src/out/Default
RELEASE_DIR=$WORKSPACE/build/release
VERSION={version}
ARCH=x86_64
NAME=helium-$VERSION-$ARCH
TARBALL_DIR=$RELEASE_DIR/${{NAME}}_linux
TAR_PATH=$RELEASE_DIR/${{NAME}}_linux.tar.xz

mkdir -p "$TARBALL_DIR"

FILES="helium chrome_100_percent.pak chrome_200_percent.pak helium_crashpad_handler \
chromedriver icudtl.dat libEGL.so libGLESv2.so libvk_swiftshader.so libvulkan.so.1 \
locales product_logo_256.png resources.pak v8_context_snapshot.bin \
vk_swiftshader_icd.json xdg-mime xdg-settings"

for f in $FILES; do
  [ -e "$BUILD_DIR/$f" ] && cp -r "$BUILD_DIR/$f" "$TARBALL_DIR/" || echo "SKIP $f"
done

cp "$WORKSPACE/package/helium.desktop" "$TARBALL_DIR/"
cp "$WORKSPACE/package/apparmor.cfg" "$TARBALL_DIR/"
cp "$WORKSPACE/package/helium-wrapper.sh" "$TARBALL_DIR/helium-wrapper"
(cd "$TARBALL_DIR" && ln -sf helium chrome)

find "$TARBALL_DIR" -type f -exec file {{}} + | awk -F: '/ELF/ {{print $1}}' | xargs eu-strip 2>/dev/null || true

SIZE=$(du -sk "$TARBALL_DIR" | cut -f1)
echo "packaging ${{NAME}}_linux (${{SIZE}}k) → tar.xz"
(cd "$RELEASE_DIR" && tar cf - "${{NAME}}_linux" | xz -e -T0 > "$TAR_PATH")
ls -lh "$TAR_PATH"
echo "$TAR_PATH"
"#,
        version = version
    );

    run_builder(vmid, &pkg_script)?;

    // 호스트로 파일 복사
    let tar_name = format!("helium-{version}-x86_64_linux.tar.xz");
    let lxc_path = format!(
        "/home/builder/workspace/helium-linux/build/release/{tar_name}"
    );
    let host_path = format!("/tmp/{tar_name}");
    common::run_passthrough("pct", &["pull", vmid, &lxc_path, &host_path])?;

    // GitLab 파일 업로드
    println!("[release] GitLab 업로드 중…");
    let upload_out = std::process::Command::new("curl")
        .args([
            "-s",
            "--header",
            &format!("PRIVATE-TOKEN: {api_token}"),
            "--form",
            &format!("file=@{host_path}"),
            &format!("{gitlab_url}/api/v4/projects/{project_id}/uploads"),
        ])
        .output()
        .context("curl 실행 실패")?;
    let upload_json = String::from_utf8_lossy(&upload_out.stdout);
    let asset_url = parse_json_field(&upload_json, "full_path")
        .map(|p| format!("{gitlab_url}{p}"))
        .context("업로드 응답에서 full_path 추출 실패")?;
    println!("[release] 업로드 완료: {asset_url}");

    // 태그 생성 (이미 있으면 skip)
    let tag = format!("v{version}");
    let tag_body = format!(
        r#"{{"tag_name":"{tag}","ref":"main","message":"Helium {version}"}}"#
    );
    let tag_out = std::process::Command::new("curl")
        .args([
            "-s", "-X", "POST",
            "--header", &format!("PRIVATE-TOKEN: {api_token}"),
            "--header", "Content-Type: application/json",
            "--data", &tag_body,
            &format!("{gitlab_url}/api/v4/projects/{project_id}/repository/tags"),
        ])
        .output()
        .context("태그 생성 curl 실패")?;
    let tag_resp = String::from_utf8_lossy(&tag_out.stdout);
    if tag_resp.contains("already exists") {
        println!("[release] 태그 {tag} 이미 존재 — skip");
    } else {
        println!("[release] 태그 {tag} 생성");
    }

    // GitLab Release 생성
    let desc = format!(
        "## Helium v{version}\\n\\nChromium 기반 프라이버시 브라우저 — Linux x86_64 빌드.\\n\\n### 설치\\n```bash\\npxi run chrome-browser-dev install\\n```\\n또는 수동:\\n```bash\\ntar xf {tar_name}\\ncd helium-{version}-x86_64_linux\\n./helium-wrapper\\n```"
    );
    let rel_body = format!(
        r#"{{"name":"Helium v{version} (Linux x86_64)","tag_name":"{tag}","description":"{desc}","assets":{{"links":[{{"name":"{tar_name}","url":"{asset_url}","link_type":"package"}}]}}}}"#
    );
    let rel_out = std::process::Command::new("curl")
        .args([
            "-s", "-X", "POST",
            "--header", &format!("PRIVATE-TOKEN: {api_token}"),
            "--header", "Content-Type: application/json",
            "--data", &rel_body,
            &format!("{gitlab_url}/api/v4/projects/{project_id}/releases"),
        ])
        .output()
        .context("릴리즈 생성 curl 실패")?;
    let rel_resp = String::from_utf8_lossy(&rel_out.stdout);
    if let Some(url) = parse_json_field(&rel_resp, "tag_name") {
        println!("[release] 완료: {gitlab_url}/root/helium-linux/-/releases/{url}");
    } else {
        println!("[release] 응답: {rel_resp}");
    }

    let _ = std::fs::remove_file(&host_path);
    Ok(())
}

fn install_helium(
    dir: &str,
    bin_dir: &str,
    project_id: u32,
    gitlab_url: &str,
    token: Option<&str>,
    version: Option<&str>,
) -> Result<()> {
    // 버전 결정
    let tag = if let Some(v) = version {
        if v.starts_with('v') {
            v.to_owned()
        } else {
            format!("v{v}")
        }
    } else {
        let api_token = token
            .map(str::to_owned)
            .or_else(|| std::env::var("GITLAB_API_TOKEN").ok())
            .unwrap_or_default();
        let mut args = vec![
            "-s".to_owned(),
            format!("{gitlab_url}/api/v4/projects/{project_id}/releases?per_page=1"),
        ];
        if !api_token.is_empty() {
            args.insert(0, format!("PRIVATE-TOKEN: {api_token}"));
            args.insert(0, "--header".to_owned());
        }
        let out = std::process::Command::new("curl")
            .args(&args)
            .output()
            .context("릴리즈 목록 조회 실패")?;
        let body = String::from_utf8_lossy(&out.stdout);
        parse_json_field(&body, "tag_name")
            .context("릴리즈 태그를 가져오지 못함 — --version으로 명시하세요")?
    };

    let version_num = tag.trim_start_matches('v');
    let tar_name = format!("helium-{version_num}-x86_64_linux.tar.xz");
    println!("[install] Helium {tag} 설치 시작");
    println!("[install] 설치 경로: {dir}");

    // 아키텍처 확인
    let arch = std::process::Command::new("uname")
        .arg("-m")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    if arch != "x86_64" {
        anyhow::bail!("이 릴리즈는 x86_64 전용입니다 (현재 아키텍처: {arch})");
    }

    // 릴리즈 에셋 URL 조회
    let api_token = token
        .map(str::to_owned)
        .or_else(|| std::env::var("GITLAB_API_TOKEN").ok())
        .unwrap_or_default();
    let rel_url = format!(
        "{gitlab_url}/api/v4/projects/{project_id}/releases/{tag}"
    );
    let mut curl_args = vec!["-s".to_owned(), rel_url.clone()];
    if !api_token.is_empty() {
        curl_args.insert(0, format!("PRIVATE-TOKEN: {api_token}"));
        curl_args.insert(0, "--header".to_owned());
    }
    let rel_out = std::process::Command::new("curl")
        .args(&curl_args)
        .output()
        .context("릴리즈 정보 조회 실패")?;
    let rel_body = String::from_utf8_lossy(&rel_out.stdout);
    let download_url = parse_json_field(&rel_body, "url")
        .context("릴리즈 에셋 URL을 찾지 못함")?;

    // 다운로드
    let tmp_tar = format!("/tmp/{tar_name}");
    println!("[install] 다운로드: {download_url}");
    let mut dl_args = vec![
        "-L".to_owned(), "--fail".to_owned(),
        "-o".to_owned(), tmp_tar.clone(),
        download_url,
    ];
    if !api_token.is_empty() {
        dl_args.insert(0, format!("PRIVATE-TOKEN: {api_token}"));
        dl_args.insert(0, "--header".to_owned());
    }
    let dl_status = std::process::Command::new("curl")
        .args(&dl_args)
        .status()
        .context("다운로드 실패")?;
    if !dl_status.success() {
        anyhow::bail!("다운로드 실패");
    }

    // 설치
    let tmp_dir = format!("/tmp/helium-install-{version_num}");
    std::fs::create_dir_all(&tmp_dir)?;
    common::run_passthrough("tar", &["xf", &tmp_tar, "-C", &tmp_dir])?;

    // 기존 설치 제거 후 이동
    let _ = std::fs::remove_dir_all(dir);
    let extracted = format!("{tmp_dir}/helium-{version_num}-x86_64_linux");
    std::fs::rename(&extracted, dir)
        .or_else(|_| {
            common::run_passthrough("cp", &["-r", &extracted, dir])
        })
        .context(format!("{dir} 설치 실패"))?;

    // wrapper 실행 권한
    let wrapper = format!("{dir}/helium-wrapper");
    common::run_passthrough("chmod", &["+x", &wrapper, &format!("{dir}/helium")])?;

    // symlink
    std::fs::create_dir_all(bin_dir)?;
    let link = format!("{bin_dir}/helium");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&wrapper, &link)
        .context(format!("symlink {link} → {wrapper} 실패"))?;

    // .desktop 파일
    let desktop_src = format!("{dir}/helium.desktop");
    let desktop_dst = "/usr/local/share/applications/helium.desktop";
    if std::path::Path::new(&desktop_src).exists() {
        let _ = std::fs::create_dir_all("/usr/local/share/applications");
        let content = std::fs::read_to_string(&desktop_src)?;
        let content = content.replace("Exec=helium-wrapper", &format!("Exec={wrapper}"));
        std::fs::write(desktop_dst, content)?;
    }

    // 정리
    let _ = std::fs::remove_file(&tmp_tar);
    let _ = std::fs::remove_dir_all(&tmp_dir);

    println!("[install] 완료");
    println!("  바이너리: {dir}/helium");
    println!("  실행:     {link}  (또는 {wrapper})");
    println!("  버전:     Helium {tag}");
    Ok(())
}

fn parse_json_field<'a>(json: &'a str, field: &str) -> Option<String> {
    let key = format!("\"{field}\"");
    let pos = json.find(&key)?;
    let after = json[pos + key.len()..].trim_start();
    let after = after.strip_prefix(':')?.trim_start();
    if after.starts_with('"') {
        let inner = &after[1..];
        let end = inner.find('"')?;
        Some(inner[..end].to_owned())
    } else if after.starts_with('[') {
        // 배열에서 첫 번째 객체의 "url" 필드
        let inner = &after[1..];
        parse_json_field(inner, field)
    } else {
        None
    }
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
