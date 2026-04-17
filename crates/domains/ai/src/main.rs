//! prelik-ai — AI agent management for Proxmox (Claude/Codex/Gemini/OpenClaw).
//! Ported from proxmox-host-setup `ai` subcommands.

use clap::{Parser, Subcommand, ValueEnum};
use prelik_core::common;
use std::fs;
use std::path::Path;
use std::process::Command;

// ─── Constants ──────────────────────────────────────────────────────────────

const SOURCE_HOME: &str = "/root";
const ENV_FILE: &str = "/etc/proxmox-host-setup/.env";

// ─── Agent metadata ─────────────────────────────────────────────────────────

#[derive(Clone, Debug, ValueEnum, PartialEq)]
enum AgentFilter {
    Claude,
    Codex,
    Gemini,
    Openclaw,
    All,
}

struct AgentInfo {
    name: &'static str,
    dir: &'static str,
    npm_package: &'static str,
    cli_binary: &'static str,
    sync_files: &'static [&'static str],
    mount_files: &'static [&'static str],
    home_files: &'static [&'static str],
}

const AGENTS: &[AgentInfo] = &[
    AgentInfo {
        name: "claude",
        dir: ".claude",
        npm_package: "@anthropic-ai/claude-code",
        cli_binary: "claude",
        sync_files: &[
            ".claude/.credentials.json",
            ".claude/settings.json",
            ".claude/settings.local.json",
        ],
        mount_files: &[".credentials.json", "settings.json", "settings.local.json"],
        home_files: &[".claude.json"],
    },
    AgentInfo {
        name: "codex",
        dir: ".codex",
        npm_package: "@openai/codex",
        cli_binary: "codex",
        sync_files: &[".codex/auth.json", ".codex/config.toml"],
        mount_files: &["auth.json", "config.toml"],
        home_files: &[],
    },
    AgentInfo {
        name: "gemini",
        dir: ".gemini",
        npm_package: "@google/gemini-cli",
        cli_binary: "gemini",
        sync_files: &[".gemini/config.json", ".gemini/credentials.json"],
        mount_files: &["config.json", "credentials.json"],
        home_files: &[],
    },
    AgentInfo {
        name: "openclaw",
        dir: ".openclaw",
        npm_package: "openclaw",
        cli_binary: "openclaw",
        sync_files: &[],
        mount_files: &[],
        home_files: &[],
    },
];

fn filtered_agents(filter: &AgentFilter) -> Vec<&'static AgentInfo> {
    match filter {
        AgentFilter::All => AGENTS.iter().collect(),
        AgentFilter::Claude => AGENTS.iter().filter(|a| a.name == "claude").collect(),
        AgentFilter::Codex => AGENTS.iter().filter(|a| a.name == "codex").collect(),
        AgentFilter::Gemini => AGENTS.iter().filter(|a| a.name == "gemini").collect(),
        AgentFilter::Openclaw => AGENTS.iter().filter(|a| a.name == "openclaw").collect(),
    }
}

// ─── Infra helpers (shell-based, no crate::infra dependency) ────────────────

fn lxc_exec(vmid: &str, args: &[&str]) -> (bool, String) {
    let mut cmd_args = vec!["exec", vmid, "--"];
    cmd_args.extend_from_slice(args);
    match Command::new("pct").args(&cmd_args).output() {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            let combined = if stderr.is_empty() {
                stdout
            } else {
                format!("{stdout}{stderr}")
            };
            (o.status.success(), combined.trim().to_string())
        }
        Err(e) => (false, format!("pct exec 실행 실패: {e}")),
    }
}

fn lxc_exec_on(node: Option<&str>, vmid: &str, args: &[&str]) -> (bool, String) {
    let local = local_node_name();
    let is_remote = node.is_some() && node != Some(&local);
    if !is_remote {
        return lxc_exec(vmid, args);
    }
    let node = node.unwrap();
    let node_ip = node_ip_from_name(node);
    let inner = args.iter().map(|a| shell_escape(a)).collect::<Vec<_>>().join(" ");
    let ssh_cmd = format!("pct exec {vmid} -- {inner}");
    match Command::new("ssh")
        .args([
            "-o", "ConnectTimeout=10",
            "-o", "StrictHostKeyChecking=no",
            &format!("root@{node_ip}"),
            &ssh_cmd,
        ])
        .output()
    {
        Ok(o) => {
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            (o.status.success(), out.trim().to_string())
        }
        Err(e) => (false, format!("ssh 실행 실패: {e}")),
    }
}

fn ensure_lxc_running(vmid: &str) {
    let out = cmd_output("pct", &["status", vmid]);
    if !out.contains("running") {
        eprintln!("[prelik-ai] LXC {vmid} 실행 중 아님. 시작 중...");
        let _ = Command::new("pct").args(["start", vmid]).status();
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
}

fn lxc_pid(vmid: &str) -> String {
    cmd_output("pct", &["status", vmid])
        .lines()
        .find_map(|l| {
            if l.contains("pid:") {
                l.split_whitespace().last().map(|s| s.to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| {
            // fallback: lxc-info
            let out = cmd_output("lxc-info", &["-n", vmid, "-p"]);
            out.split_whitespace().last().unwrap_or("0").to_string()
        })
}

fn cmd_output(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|o| {
            let s = String::from_utf8_lossy(&o.stdout).to_string();
            s.trim().to_string()
        })
        .unwrap_or_default()
}

fn local_node_name() -> String {
    fs::read_to_string("/etc/hostname")
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn node_ip_from_name(node: &str) -> String {
    // Try pvesh first, then /etc/hosts
    let json = cmd_output(
        "pvesh",
        &["get", &format!("/nodes/{node}/network"), "--output-format", "json"],
    );
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json) {
        if let Some(arr) = parsed.as_array() {
            for iface in arr {
                let name = iface["iface"].as_str().unwrap_or("");
                if name == "vmbr1" || name == "vmbr0" {
                    if let Some(addr) = iface["address"].as_str() {
                        if !addr.is_empty() {
                            return addr.to_string();
                        }
                    }
                }
            }
        }
    }
    // Fallback: /etc/hosts
    if let Ok(hosts) = fs::read_to_string("/etc/hosts") {
        for line in hosts.lines() {
            if line.contains(node) {
                if let Some(ip) = line.split_whitespace().next() {
                    return ip.to_string();
                }
            }
        }
    }
    node.to_string()
}

fn running_lxc_list() -> Vec<(String, String)> {
    let output = cmd_output("pct", &["list"]);
    let mut result = Vec::new();
    for line in output.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() >= 3 && cols[1] == "running" {
            result.push((cols[0].to_string(), cols[2].to_string()));
        }
    }
    result
}

fn shell_escape(s: &str) -> String {
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
}

fn set_permissions(path: &str, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
}

fn base64_encode(input: &str) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if i + 1 < bytes.len() {
            out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < bytes.len() {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

// ─── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "prelik-ai", about = "AI agent management for Proxmox")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    // ── existing (keep) ──
    /// Claude + Codex CLI 설치 (npm -g)
    Install {
        #[arg(long)]
        vmid: Option<String>,
    },
    /// claude-octopus 플러그인 설치
    OctopusInstall {
        #[arg(long)]
        vmid: Option<String>,
    },
    /// superpowers 스킬 플러그인 설치
    SuperpowersInstall {
        #[arg(long)]
        vmid: Option<String>,
    },
    /// OpenAI Codex Plugin for Claude Code 설치
    CodexPluginInstall {
        #[arg(long)]
        vmid: Option<String>,
        #[arg(long)]
        fork: bool,
    },
    /// Claude Stop 훅 — /codex:adversarial-review 자동 실행
    AdversarialReviewHook {
        #[arg(long)]
        disable: bool,
    },
    /// 환경 진단
    Doctor,

    // ── ported commands ──

    /// 크리덴셜 + settings 동기화 (호스트 계정 간)
    Sync {
        #[arg(long, value_enum, default_value = "all")]
        agent: AgentFilter,
    },
    /// AI 에이전트 세션을 LXC에 배포
    Mount {
        #[arg(long)]
        vmid: String,
        #[arg(long, value_enum, default_value = "all")]
        agent: AgentFilter,
        #[arg(long)]
        node: Option<String>,
    },
    /// 실행 중인 모든 LXC에 AI 에이전트 일괄 배포
    MountAll {
        #[arg(long, value_enum, default_value = "all")]
        agent: AgentFilter,
    },
    /// LXC에서 AI 에이전트 제거
    Unmount {
        #[arg(long)]
        vmid: String,
        #[arg(long, value_enum, default_value = "all")]
        agent: AgentFilter,
    },
    /// LXC 컨테이너 접속 (인터랙티브)
    Enter {
        #[arg(long)]
        vmid: String,
    },
    /// LXC별 AI 에이전트 배포 현황
    List,
    /// LXC 내 AI CLI 업데이트
    Update {
        #[arg(long)]
        vmid: String,
        #[arg(long, value_enum, default_value = "all")]
        agent: AgentFilter,
    },
    /// AI 에이전트 전체 상태
    Status,
    /// Claude Code 권한 최대 부여
    PermMax {
        #[arg(long)]
        vmid: Option<String>,
    },
    /// LXC에 호스트 크리덴셜 복사 (OAuth 토큰 만료 복구)
    CredentialSync {
        #[arg(long)]
        vmid: String,
        #[arg(long, default_value = "root")]
        user: String,
        #[arg(long)]
        home: Option<String>,
        #[arg(long, value_enum, default_value = "claude")]
        agent: AgentFilter,
        #[arg(long)]
        node: Option<String>,
    },
    /// LXC 내 AI 에이전트 복구 (error -> active + 서비스 재시작)
    AgentRecovery {
        #[arg(long)]
        vmid: String,
        #[arg(long)]
        db: String,
        #[arg(long)]
        service: String,
    },

    // ── ComfyUI ──

    /// ComfyUI VFX 환경 전체 세팅
    ComfyuiSetup {
        #[arg(long)]
        node: String,
        #[arg(long)]
        vmid: Option<String>,
        #[arg(long, default_value = "0,1,2,3")]
        gpu: String,
    },
    /// ComfyUI 자동 동기화 timer
    ComfyuiSyncWatch {
        #[arg(long)]
        node: String,
        #[arg(long, value_parser = ["install", "uninstall", "status"], default_value = "install")]
        action: String,
    },
    /// ComfyUI MCP를 LXC의 Claude Code에 등록
    ComfyuiMcpSetup {
        #[arg(long)]
        node: String,
        #[arg(long)]
        vmid: Option<String>,
    },
    /// ComfyUI 라이브 테스트
    ComfyuiTest {
        #[arg(long)]
        node: String,
        #[arg(long)]
        vmid: Option<String>,
    },
    /// ComfyUI 종합 점검
    ComfyuiStatus {
        #[arg(long)]
        node: String,
        #[arg(long)]
        vmid: Option<String>,
    },
    /// ComfyUI 미사용 모델 정리
    ComfyuiCleanup {
        #[arg(long)]
        node: String,
        #[arg(long)]
        vmid: Option<String>,
        #[arg(long)]
        apply: bool,
    },

    // ── OpenClaw ──

    /// OpenClaw LXC 전체 세팅
    OpenclawSetup {
        #[arg(long)]
        vmid: String,
    },
    /// OpenClaw 텔레그램 봇 등록
    OpenclawTelegram {
        #[arg(long)]
        vmid: String,
        #[arg(long)]
        token: String,
        #[arg(long, default_value = "default")]
        account: String,
    },
    /// OpenClaw 관리 대상 서버 추가
    OpenclawServer {
        #[arg(long)]
        vmid: String,
        #[arg(long)]
        ip: String,
        #[arg(long)]
        name: String,
    },
    /// OpenClaw gateway 제어
    OpenclawGateway {
        #[arg(long)]
        vmid: String,
        #[arg(long)]
        action: String,
    },
    /// 실행 중인 모든 LXC에 claude-octopus 일괄 설치 (AI CLI 감지된 LXC만)
    OctopusInstallAll {
        #[arg(long)]
        force: bool,
    },
    /// 실행 중인 모든 LXC에 superpowers 일괄 설치
    SuperpowersInstallAll {
        #[arg(long)]
        force: bool,
    },
    /// 호스트 openclaw 명령을 OpenClaw LXC로 자동 포워딩하는 wrapper 설치
    OpenclawWrap {
        /// OpenClaw LXC VMID (생략 시 hostname=openclaw인 LXC 자동 탐색)
        #[arg(long)]
        vmid: Option<String>,
    },
    /// OpenClaw LLM 프리셋 전환 (local-gemma4, cloud-codex, cloud-claude)
    OpenclawLlmSwitch {
        #[arg(long)]
        vmid: Option<String>,
        /// 프리셋 이름
        #[arg(long, default_value = "")]
        preset: String,
        /// 사용 가능한 프리셋 목록 표시
        #[arg(long)]
        list: bool,
    },
    /// OpenClaw 기본 모델 정책 적용
    OpenclawModelDefaults {
        #[arg(long)]
        vmid: Option<String>,
        #[arg(long, default_value = "openai-codex/gpt-5.4")]
        model: String,
        #[arg(long, default_value = "high")]
        thinking: String,
        #[arg(long, default_value = "on")]
        reasoning: String,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        fast_mode: bool,
        #[arg(long)]
        fallback: Option<String>,
    },

    // ── Cluster files ──

    /// 클러스터 통합 파일 브라우저 설치
    ClusterFilesSetup,
    /// cluster-files 마운트 동기화
    ClusterFilesSync,
    /// cluster-files 마운트 전체 리셋
    ClusterFilesReset,
    /// pve SSH 공개키를 모든 클러스터 LXC에 배포
    ClusterSshDeploy,
    /// ComfyUI 폴더 웹 GUI (filebrowser)
    FilebrowserSetup {
        #[arg(long)]
        node: String,
        #[arg(long)]
        vmid: Option<String>,
    },

    // ── VM ──

    /// VM에 AI 에이전트 SSH 배포
    VmMount {
        #[arg(long)]
        vmid: String,
        #[arg(long)]
        ip: String,
        #[arg(long, default_value = "root")]
        user: String,
        #[arg(long, value_enum, default_value = "all")]
        agent: AgentFilter,
    },
    /// VM에서 AI 에이전트 제거
    VmUnmount {
        #[arg(long)]
        vmid: String,
        #[arg(long)]
        ip: String,
        #[arg(long, default_value = "root")]
        user: String,
        #[arg(long, value_enum, default_value = "all")]
        agent: AgentFilter,
    },
    /// VM SSH 접속
    VmEnter {
        #[arg(long)]
        vmid: String,
        #[arg(long)]
        ip: String,
        #[arg(long, default_value = "root")]
        user: String,
    },
    /// VM 내 AI CLI 업데이트
    VmUpdate {
        #[arg(long)]
        vmid: String,
        #[arg(long)]
        ip: String,
        #[arg(long, default_value = "root")]
        user: String,
        #[arg(long, value_enum, default_value = "all")]
        agent: AgentFilter,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        // existing
        Cmd::Install { vmid } => install_cli(vmid.as_deref()),
        Cmd::OctopusInstall { vmid } => octopus_install(vmid.as_deref()),
        Cmd::SuperpowersInstall { vmid } => superpowers_install(vmid.as_deref()),
        Cmd::CodexPluginInstall { vmid, fork } => codex_plugin_install(vmid.as_deref(), fork),
        Cmd::AdversarialReviewHook { disable } => adversarial_review_hook(!disable),
        Cmd::Doctor => {
            doctor();
            Ok(())
        }
        // ported
        Cmd::Sync { agent } => {
            sync(&agent);
            Ok(())
        }
        Cmd::Mount { vmid, agent, node } => {
            mount_on(node.as_deref(), &vmid, &agent);
            Ok(())
        }
        Cmd::MountAll { agent } => {
            mount_all(&agent);
            Ok(())
        }
        Cmd::Unmount { vmid, agent } => {
            unmount(&vmid, &agent);
            Ok(())
        }
        Cmd::Enter { vmid } => {
            enter(&vmid);
            Ok(())
        }
        Cmd::List => {
            list();
            Ok(())
        }
        Cmd::Update { vmid, agent } => {
            update(&vmid, &agent);
            Ok(())
        }
        Cmd::Status => {
            status();
            Ok(())
        }
        Cmd::PermMax { vmid } => {
            perm_max(vmid.as_deref());
            Ok(())
        }
        Cmd::CredentialSync {
            vmid,
            user,
            home,
            agent,
            node,
        } => {
            credential_sync_on(node.as_deref(), &vmid, &user, home.as_deref(), &agent);
            Ok(())
        }
        Cmd::AgentRecovery { vmid, db, service } => {
            agent_recovery(&vmid, &db, &service);
            Ok(())
        }
        // ComfyUI
        Cmd::ComfyuiSetup { node, vmid, gpu } => {
            comfyui_setup(&node, vmid.as_deref(), &gpu);
            Ok(())
        }
        Cmd::ComfyuiSyncWatch { node, action } => {
            comfyui_sync_watch(&node, &action);
            Ok(())
        }
        Cmd::ComfyuiMcpSetup { node, vmid } => {
            comfyui_mcp_setup(&node, vmid.as_deref());
            Ok(())
        }
        Cmd::ComfyuiTest { node, vmid } => {
            comfyui_test(&node, vmid.as_deref());
            Ok(())
        }
        Cmd::ComfyuiStatus { node, vmid } => {
            comfyui_status_cmd(&node, vmid.as_deref());
            Ok(())
        }
        Cmd::ComfyuiCleanup { node, vmid, apply } => {
            comfyui_cleanup(&node, vmid.as_deref(), apply);
            Ok(())
        }
        // OpenClaw
        Cmd::OpenclawSetup { vmid } => {
            openclaw_setup(&vmid);
            Ok(())
        }
        Cmd::OpenclawTelegram {
            vmid,
            token,
            account,
        } => {
            openclaw_telegram(&vmid, &token, &account);
            Ok(())
        }
        Cmd::OpenclawServer { vmid, ip, name } => {
            openclaw_server(&vmid, &ip, &name);
            Ok(())
        }
        Cmd::OpenclawGateway { vmid, action } => {
            openclaw_gateway(&vmid, &action);
            Ok(())
        }
        Cmd::OctopusInstallAll { force } => {
            octopus_install_all(force);
            Ok(())
        }
        Cmd::SuperpowersInstallAll { force } => {
            superpowers_install_all(force);
            Ok(())
        }
        Cmd::OpenclawWrap { vmid } => {
            openclaw_wrap(vmid.as_deref());
            Ok(())
        }
        Cmd::OpenclawLlmSwitch { vmid, preset, list } => {
            openclaw_llm_switch(vmid.as_deref(), &preset, list);
            Ok(())
        }
        Cmd::OpenclawModelDefaults {
            vmid,
            model,
            thinking,
            reasoning,
            fast_mode,
            fallback,
        } => {
            openclaw_model_defaults(
                vmid.as_deref(),
                &model,
                fallback.as_deref(),
                &thinking,
                &reasoning,
                fast_mode,
            );
            Ok(())
        }
        // Cluster
        Cmd::ClusterFilesSetup => {
            cluster_files_setup();
            Ok(())
        }
        Cmd::ClusterFilesSync => {
            cluster_files_sync();
            Ok(())
        }
        Cmd::ClusterFilesReset => {
            cluster_files_reset();
            Ok(())
        }
        Cmd::ClusterSshDeploy => {
            cluster_ssh_deploy();
            Ok(())
        }
        Cmd::FilebrowserSetup { node, vmid } => {
            filebrowser_setup(&node, vmid.as_deref());
            Ok(())
        }
        // VM
        Cmd::VmMount {
            vmid,
            ip,
            user,
            agent,
        } => {
            vm_mount(&vmid, &ip, &user, &agent);
            Ok(())
        }
        Cmd::VmUnmount {
            vmid,
            ip,
            user,
            agent,
        } => {
            vm_unmount(&vmid, &ip, &user, &agent);
            Ok(())
        }
        Cmd::VmEnter { vmid: _, ip, user } => {
            vm_enter(&ip, &user);
            Ok(())
        }
        Cmd::VmUpdate {
            vmid,
            ip,
            user,
            agent,
        } => {
            vm_update(&vmid, &ip, &user, &agent);
            Ok(())
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Existing commands (kept from original prelik-ai)
// ═══════════════════════════════════════════════════════════════════════════════

fn run_on(vmid: Option<&str>, script: &str) -> anyhow::Result<String> {
    match vmid {
        Some(v) => common::run("pct", &["exec", v, "--", "bash", "-c", script]),
        None => common::run_bash(script),
    }
}

fn has_on(vmid: Option<&str>, bin: &str) -> bool {
    let script = format!("command -v {bin} >/dev/null 2>&1");
    match vmid {
        Some(v) => Command::new("pct")
            .args(["exec", v, "--", "bash", "-c", &script])
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
        None => Command::new("bash")
            .args(["-c", &script])
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
    }
}

fn install_cli(vmid: Option<&str>) -> anyhow::Result<()> {
    println!("=== AI CLI 설치 ===");
    if !has_on(vmid, "npm") {
        anyhow::bail!("npm 없음 — prelik install bootstrap 먼저");
    }
    let needs_sudo = vmid.is_none() && unsafe { libc_geteuid() } != 0;
    let sudo = if needs_sudo { "sudo " } else { "" };

    if !has_on(vmid, "claude") {
        println!("  claude 설치...");
        run_on(vmid, &format!("{sudo}npm install -g @anthropic-ai/claude-code"))?;
    } else {
        println!("  ✓ claude 이미 설치됨");
    }
    if !has_on(vmid, "codex") {
        println!("  codex 설치...");
        run_on(vmid, &format!("{sudo}npm install -g @openai/codex"))?;
    } else {
        println!("  ✓ codex 이미 설치됨");
    }
    println!("✓ AI CLI 설치 완료");
    Ok(())
}

unsafe fn libc_geteuid() -> u32 {
    extern "C" {
        fn geteuid() -> u32;
    }
    geteuid()
}

fn octopus_install(vmid: Option<&str>) -> anyhow::Result<()> {
    println!("=== claude-octopus 설치 ===");
    let mut ok = 0;
    if has_on(vmid, "claude") {
        let script = "claude plugin marketplace add https://github.com/nyldn/claude-octopus.git 2>&1 | tail -1; \
                      claude plugin install octo@nyldn-plugins 2>&1 | tail -1";
        match run_on(vmid, script) {
            Ok(out) => {
                println!("  ✓ claude: {}", out.trim());
                ok += 1;
            }
            Err(e) => println!("  ✗ claude: {e}"),
        }
    }
    if has_on(vmid, "codex") {
        let script = "if [ -d ~/.codex/claude-octopus ]; then \
           cd ~/.codex/claude-octopus && git pull --ff-only 2>&1 | tail -1; \
         else \
           git clone --depth 1 https://github.com/nyldn/claude-octopus.git ~/.codex/claude-octopus 2>&1 | tail -1; \
         fi && \
         mkdir -p ~/.agents/skills && \
         ln -sf ~/.codex/claude-octopus/skills ~/.agents/skills/claude-octopus";
        match run_on(vmid, script) {
            Ok(_) => {
                println!("  ✓ codex: skills symlink");
                ok += 1;
            }
            Err(e) => println!("  ✗ codex: {e}"),
        }
    }
    if ok == 0 {
        anyhow::bail!("대상에 claude/codex CLI 없음");
    }
    println!("✓ octopus 설치 완료 (다음: /octo:setup)");
    Ok(())
}

fn superpowers_install(vmid: Option<&str>) -> anyhow::Result<()> {
    println!("=== superpowers 설치 ===");
    if !has_on(vmid, "claude") {
        anyhow::bail!("claude CLI 없음");
    }
    let script = "claude plugin install superpowers@claude-plugins-official 2>&1 | tail -1 || ( \
                    claude plugin marketplace add obra/superpowers-marketplace 2>&1 | tail -1 && \
                    claude plugin install superpowers@superpowers-marketplace 2>&1 | tail -1 \
                  )";
    let out = run_on(vmid, script)?;
    println!("  {}", out.trim());
    println!("✓ superpowers 설치 완료 (스킬 자동 활성)");
    Ok(())
}

fn codex_plugin_install(vmid: Option<&str>, fork: bool) -> anyhow::Result<()> {
    println!("=== Codex Plugin for Claude Code 설치 ===");
    if !has_on(vmid, "claude") {
        anyhow::bail!("claude CLI 없음");
    }
    if !has_on(vmid, "codex") {
        anyhow::bail!("codex CLI 없음 — prelik-ai install 먼저");
    }
    let marketplace = if fork {
        "parthpm/codex-plugin-cc"
    } else {
        "openai/codex-plugin-cc"
    };
    let script = format!(
        "claude plugin marketplace add {marketplace} 2>&1 | tail -1; \
         claude plugin install codex@openai-codex 2>&1 | tail -1"
    );
    let out = run_on(vmid, &script)?;
    println!("  {}", out.trim());
    println!("✓ codex-plugin 설치 완료");
    println!("  다음: claude 세션에서 /codex:setup (OpenAI 인증)");
    if fork {
        println!("  훅 활성화: prelik run ai adversarial-review-hook");
    }
    Ok(())
}

fn adversarial_review_hook(enable: bool) -> anyhow::Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("HOME 미설정"))?;
    let settings_path = home.join(".claude/settings.json");
    println!(
        "=== Adversarial Review Stop 훅 ({}) ===",
        if enable { "활성화" } else { "비활성화" }
    );

    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = fs::read_to_string(&settings_path).unwrap_or_else(|_| "{}".into());
    let mut v: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::json!({}));
    if !v.is_object() {
        v = serde_json::json!({});
    }
    let obj = v.as_object_mut().unwrap();

    const PRELIK_MARKER: &str = "__PRELIK_AI_ADV_REVIEW_HOOK__";
    const LEGACY_MARKER: &str = "prelik-adv-review-";

    let hook_cmd = format!(
        "F=/tmp/.{PRELIK_MARKER}$(echo $CLAUDE_SESSION_ID | md5sum | cut -c1-8); if [ -f $F ]; then exit 0; fi; touch $F; cat <<EOF\n{{\"decision\":\"block\",\"reason\":\"지금까지의 작업을 Codex로 어드버서리얼 리뷰하세요. 실행: /codex:adversarial-review\"}}\nEOF"
    );

    let hooks_root = obj
        .entry("hooks".to_string())
        .or_insert(serde_json::json!({}));
    let hooks_root = hooks_root.as_object_mut().unwrap();

    let mut stop_arr: Vec<serde_json::Value> = hooks_root
        .get("Stop")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    stop_arr.retain(|entry| {
        let inner = entry.get("hooks").and_then(|h| h.as_array());
        !inner
            .map(|arr| {
                arr.iter().any(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .map(|s| s.contains(PRELIK_MARKER) || s.contains(LEGACY_MARKER))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    });

    if enable {
        let prelik_entry = serde_json::json!({
            "hooks": [{
                "type": "command",
                "command": hook_cmd,
            }]
        });
        stop_arr.push(prelik_entry);
        println!("  Stop 훅 등록 (기존 훅 {} 개 유지)", stop_arr.len() - 1);
    } else {
        println!("  prelik Stop 훅만 제거 (기존 {} 개 유지)", stop_arr.len());
    }

    if stop_arr.is_empty() {
        hooks_root.remove("Stop");
    } else {
        hooks_root.insert("Stop".into(), serde_json::json!(stop_arr));
    }

    let pretty = serde_json::to_string_pretty(&v)?;
    fs::write(&settings_path, pretty)?;
    println!("✓ {} 업데이트", settings_path.display());
    Ok(())
}

fn doctor() {
    println!("=== prelik-ai doctor ===");
    println!(
        "  npm:    {}",
        if common::has_cmd("npm") { "✓" } else { "✗" }
    );
    println!(
        "  claude: {}",
        if common::has_cmd("claude") { "✓" } else { "✗" }
    );
    println!(
        "  codex:  {}",
        if common::has_cmd("codex") { "✓" } else { "✗" }
    );
    println!(
        "  gemini: {}",
        if common::has_cmd("gemini") { "✓" } else { "✗" }
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// sync — credential sync to host user accounts
// ═══════════════════════════════════════════════════════════════════════════════

fn sync(filter: &AgentFilter) {
    let agents = filtered_agents(filter);
    let names: Vec<&str> = agents.iter().map(|a| a.name).collect();
    println!("=== AI 에이전트 설정 동기화: {} ===\n", names.join(", "));

    let home = SOURCE_HOME;

    let any_source = agents.iter().any(|a| {
        a.sync_files
            .first()
            .map(|f| Path::new(&format!("{home}/{f}")).exists())
            .unwrap_or(false)
    });
    if !any_source {
        eprintln!("[ai-sync] 선택된 에이전트의 소스 크리덴셜이 없습니다.");
        std::process::exit(1);
    }

    // Discover target users (non-system users with home dirs in /home)
    let target_users = discover_target_users();

    for user in &target_users {
        let target_home = format!("/home/{user}");
        if !Path::new(&target_home).exists() {
            continue;
        }

        let mut synced = 0;
        for agent in &agents {
            let source_dir = format!("{home}/{}", agent.dir);
            if !Path::new(&source_dir).exists() {
                continue;
            }

            let target_dir = format!("{target_home}/{}", agent.dir);
            fs::create_dir_all(&target_dir).unwrap_or_default();

            for file in agent.sync_files {
                let src = format!("{home}/{file}");
                let dst = format!("{target_home}/{file}");
                if !Path::new(&src).exists() {
                    continue;
                }

                let output = Command::new("rsync")
                    .args(["-a", &src, &dst])
                    .output();
                if let Ok(o) = output {
                    if o.status.success() {
                        set_permissions(&dst, 0o600);
                        synced += 1;
                    }
                }
            }

            set_permissions(&target_dir, 0o700);
            let _ = Command::new("chown")
                .args(["-R", &format!("{user}:{user}"), &target_dir])
                .status();

            for file in agent.home_files {
                let src = format!("{home}/{file}");
                let dst = format!("{target_home}/{file}");
                if !Path::new(&src).exists() {
                    continue;
                }
                let output = Command::new("rsync").args(["-a", &src, &dst]).output();
                if let Ok(o) = output {
                    if o.status.success() {
                        set_permissions(&dst, 0o600);
                        let _ = Command::new("chown")
                            .args([&format!("{user}:{user}"), &dst])
                            .status();
                        synced += 1;
                    }
                }
            }
        }

        println!("[ai-sync] {user} - {synced}개 파일 동기화 완료");
    }

    println!("\n=== 동기화 완료 ===");
}

fn discover_target_users() -> Vec<String> {
    let mut users = Vec::new();
    if let Ok(entries) = fs::read_dir("/home") {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    users.push(name.to_string());
                }
            }
        }
    }
    users
}

// ═══════════════════════════════════════════════════════════════════════════════
// mount / mount-all / unmount — deploy AI agent sessions to LXC
// ═══════════════════════════════════════════════════════════════════════════════

fn mount_on(node: Option<&str>, vmid: &str, filter: &AgentFilter) {
    let agents = filtered_agents(filter);
    let names: Vec<&str> = agents.iter().map(|a| a.name).collect();
    let node_label = node.unwrap_or("(local)");
    println!(
        "=== AI 에이전트 -> LXC {vmid} (node: {node_label}) 배포: {} ===\n",
        names.join(", ")
    );

    let local = local_node_name();
    let is_remote = node.is_some() && node != Some(&local);

    if is_remote {
        mount_remote(node.unwrap(), vmid, &agents);
        return;
    }

    let status = cmd_output("pct", &["status", vmid]);
    if !status.contains("running") {
        eprintln!("[ai-mount] LXC {vmid} 이 실행 중이 아닙니다: {status}");
        return;
    }

    let home = SOURCE_HOME;
    let pid = lxc_pid(vmid);
    let rootfs = format!("/proc/{pid}/root");
    let mut synced = 0;

    for agent in &agents {
        let source_dir = format!("{home}/{}", agent.dir);
        if !Path::new(&source_dir).exists() {
            println!("[ai-mount] {} - 호스트에 없음, 스킵", agent.dir);
            continue;
        }

        let dest_dir = format!("{rootfs}/root/{}", agent.dir);
        fs::create_dir_all(&dest_dir).unwrap_or_default();

        let mut copied = 0;
        for file in agent.mount_files {
            let src = format!("{source_dir}/{file}");
            let dst = format!("{dest_dir}/{file}");
            if !Path::new(&src).exists() {
                continue;
            }
            if Command::new("cp")
                .args([&src, &dst])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                copied += 1;
            }
        }

        if copied > 0 {
            println!("[ai-mount] {} - {copied}개 파일 복사 완료", agent.dir);
            synced += 1;
        }

        for file in agent.home_files {
            let src = format!("{home}/{file}");
            let dest = format!("{rootfs}/root/{file}");
            if !Path::new(&src).exists() {
                continue;
            }
            if Command::new("cp")
                .args([&src, &dest])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                if *file == ".claude.json" {
                    patch_claude_json_for_npm(&dest);
                }
                println!("[ai-mount] {file} 복사 완료");
            }
        }
    }

    // Claude plugins directory
    if agents.iter().any(|a| a.name == "claude") {
        let plugin_src = format!("{home}/.claude/plugins");
        if Path::new(&plugin_src).exists() {
            let plugin_dest = format!("{rootfs}/root/.claude/plugins");
            let _ = Command::new("cp")
                .args(["-r", &plugin_src, &plugin_dest])
                .output();
            println!("[ai-mount] .claude/plugins/ 복사 완료");
        }
    }

    if synced > 0 {
        println!("\n[ai-mount] {synced}개 에이전트 배포 완료");
    } else {
        println!("\n[ai-mount] 배포할 에이전트 없음");
    }

    // Install CLIs inside LXC
    install_agents_in_lxc(vmid, filter);
}

fn mount_remote(node: &str, vmid: &str, agents: &[&AgentInfo]) {
    let node_ip = node_ip_from_name(node);
    let home = SOURCE_HOME;

    for agent in agents {
        let source_dir = format!("{home}/{}", agent.dir);
        if !Path::new(&source_dir).exists() {
            println!("[ai-mount] {} - 호스트에 없음, 스킵", agent.dir);
            continue;
        }

        let archive = format!(
            "/tmp/prelik-aimount-{}-{}.tar.gz",
            agent.dir.replace('.', ""),
            std::process::id()
        );
        let tar_ok = Command::new("tar")
            .args(["czf", &archive, "-C", home, agent.dir])
            .status()
            .map_or(false, |s| s.success());
        if !tar_ok {
            eprintln!("[ai-mount] {} - tar 실패", agent.dir);
            continue;
        }

        let remote_archive = format!(
            "/tmp/{}",
            Path::new(&archive)
                .file_name()
                .unwrap()
                .to_string_lossy()
        );
        let _ = Command::new("scp")
            .args([
                "-o",
                "ConnectTimeout=10",
                "-o",
                "StrictHostKeyChecking=no",
                &archive,
                &format!("root@{node_ip}:{remote_archive}"),
            ])
            .status();
        let _ = fs::remove_file(&archive);

        let push_cmd = format!(
            "pct push {vmid} {remote_archive} {remote_archive} && \
             pct exec {vmid} -- bash -c 'mkdir -p /root/{} && tar xzf {remote_archive} -C /root/ && rm {remote_archive}' && \
             rm {remote_archive}",
            agent.dir
        );
        let push_ok = Command::new("ssh")
            .args([
                "-o",
                "ConnectTimeout=10",
                "-o",
                "StrictHostKeyChecking=no",
                &format!("root@{node_ip}"),
                &push_cmd,
            ])
            .status()
            .map_or(false, |s| s.success());
        if push_ok {
            println!("[ai-mount] {} - 원격 배포 완료", agent.dir);
        } else {
            eprintln!("[ai-mount] {} - 원격 배포 실패", agent.dir);
        }
    }

    println!("\n[ai-mount] 원격 LXC {vmid} (node:{node}) 배포 완료");
}

fn install_agents_in_lxc(vmid: &str, filter: &AgentFilter) {
    let agents = filtered_agents(filter);
    let names: Vec<&str> = agents.iter().map(|a| a.name).collect();
    println!(
        "\n=== LXC {vmid} 에이전트 CLI 설치: {} ===\n",
        names.join(", ")
    );

    // Ensure node.js
    let (node_ok, _) = lxc_exec(
        vmid,
        &[
            "bash",
            "-c",
            "export PATH=/usr/local/bin:$PATH && which node",
        ],
    );
    if !node_ok {
        println!("[lxc-install] Node.js 설치 중...");
        let _ = lxc_exec(
            vmid,
            &[
                "bash",
                "-c",
                "apt-get update -qq && apt-get install -y -qq nodejs npm",
            ],
        );
    }

    for agent in &agents {
        if !Path::new(&format!("{}/{}", SOURCE_HOME, agent.dir)).exists() {
            continue;
        }

        let (ok, _) = lxc_exec(
            vmid,
            &[
                "bash",
                "-c",
                &format!(
                    "export PATH=/usr/local/bin:$PATH && which {}",
                    agent.cli_binary
                ),
            ],
        );
        if !ok {
            println!("[lxc-install] {} 설치 중...", agent.name);
            let (ok, out) = lxc_exec(
                vmid,
                &[
                    "bash",
                    "-c",
                    &format!(
                        "export PATH=/usr/local/bin:$PATH && npm install -g {}",
                        agent.npm_package
                    ),
                ],
            );
            if ok {
                println!("[lxc-install] {} 설치 완료", agent.name);
            } else {
                eprintln!("[lxc-install] {} 설치 실패: {out}", agent.name);
            }
        } else {
            println!("[lxc-install] {} 이미 설치됨", agent.name);
        }
    }
}

fn mount_all(filter: &AgentFilter) {
    let agents = filtered_agents(filter);
    let names: Vec<&str> = agents.iter().map(|a| a.name).collect();
    println!(
        "=== AI 에이전트 -> 전체 LXC 일괄 배포: {} ===\n",
        names.join(", ")
    );

    let containers = running_lxc_list();
    if containers.is_empty() {
        println!("[mount-all] 실행 중인 LXC 없음");
        return;
    }

    for (vmid, name) in &containers {
        println!("{}", "-".repeat(50));
        println!("[mount-all] {vmid} ({name})");
        mount_on(None, vmid, filter);
        println!();
    }

    println!("=== 일괄 배포 완료 ===");
}

fn unmount(vmid: &str, filter: &AgentFilter) {
    let agents = filtered_agents(filter);
    let names: Vec<&str> = agents.iter().map(|a| a.name).collect();
    println!(
        "=== AI 에이전트 <- LXC {vmid} 제거: {} ===\n",
        names.join(", ")
    );

    let status = cmd_output("pct", &["status", vmid]);
    if !status.contains("running") {
        eprintln!("[ai-unmount] LXC {vmid} 이 실행 중이 아닙니다.");
        return;
    }

    let mut removed = 0;
    for agent in &agents {
        let (exists, _) = lxc_exec(
            vmid,
            &["bash", "-c", &format!("test -d /root/{}", agent.dir)],
        );
        if exists {
            let (ok, _) = lxc_exec(
                vmid,
                &["bash", "-c", &format!("rm -rf /root/{}", agent.dir)],
            );
            if ok {
                println!("[ai-unmount] {} 삭제 완료", agent.dir);
                removed += 1;
            }
        }

        for file in agent.home_files {
            let _ = lxc_exec(vmid, &["bash", "-c", &format!("rm -f /root/{file}")]);
        }
    }

    if removed > 0 {
        println!("\n[ai-unmount] {removed}개 에이전트 제거 완료");
    } else {
        println!("\n[ai-unmount] 제거할 에이전트 없음");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// enter — interactive LXC shell
// ═══════════════════════════════════════════════════════════════════════════════

fn enter(vmid: &str) {
    let _ = Command::new("pct")
        .args(["enter", vmid])
        .status();
}

// ═══════════════════════════════════════════════════════════════════════════════
// list / status / update
// ═══════════════════════════════════════════════════════════════════════════════

fn list() {
    println!("=== AI 에이전트 LXC 배포 현황 ===\n");

    let containers = running_lxc_list();
    if containers.is_empty() {
        println!("실행 중인 LXC 없음");
        return;
    }

    println!(
        "{:<6} {:<16} {:<10} {:<10} {:<10} {:<12}",
        "VMID", "HOSTNAME", ".claude", ".codex", ".gemini", "claude-cli"
    );
    println!("{}", "-".repeat(70));

    for (vmid, name) in &containers {
        let claude = check_agent_in_lxc(vmid, ".claude");
        let codex = check_agent_in_lxc(vmid, ".codex");
        let gemini = check_agent_in_lxc(vmid, ".gemini");
        let cli_ver = get_cli_version_in_lxc(vmid, "claude");

        println!(
            "{:<6} {:<16} {:<10} {:<10} {:<10} {:<12}",
            vmid, name, claude, codex, gemini, cli_ver
        );
    }
}

fn check_agent_in_lxc(vmid: &str, agent: &str) -> String {
    let (ok, _) = lxc_exec(vmid, &["bash", "-c", &format!("test -d /root/{agent}")]);
    if ok { "Y".to_string() } else { "N".to_string() }
}

fn get_cli_version_in_lxc(vmid: &str, cli: &str) -> String {
    let (ok, out) = lxc_exec(
        vmid,
        &[
            "bash",
            "-c",
            &format!("export PATH=/usr/local/bin:$PATH && {cli} --version 2>/dev/null"),
        ],
    );
    if ok {
        out.lines().next().unwrap_or("?").trim().to_string()
    } else {
        "N".to_string()
    }
}

fn status() {
    println!("=== AI 에이전트 상태 ===\n");

    println!("[호스트 CLI]");
    for agent in AGENTS {
        let ok = Command::new(agent.cli_binary)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        println!("  {:<10} {}", agent.name, if ok { "Y" } else { "N" });
    }

    println!("\n[소스 파일]");
    for agent in AGENTS {
        for file in agent.sync_files {
            let path = format!("{SOURCE_HOME}/{file}");
            let mark = if Path::new(&path).exists() { "Y" } else { "N" };
            println!("  {mark} {file}");
        }
    }

    let containers = running_lxc_list();
    if !containers.is_empty() {
        println!("\n[LXC 배포]");
        for (vmid, name) in &containers {
            let mut marks = Vec::new();
            for agent in AGENTS {
                let mark = check_agent_in_lxc(vmid, agent.dir);
                marks.push(format!("{}:{mark}", agent.name));
            }
            let cli_ver = get_cli_version_in_lxc(vmid, "claude");
            println!("  {vmid} {name:<16} {}  cli:{cli_ver}", marks.join("  "));
        }
    }
}

fn update(vmid: &str, filter: &AgentFilter) {
    let agents = filtered_agents(filter);
    let names: Vec<&str> = agents.iter().map(|a| a.name).collect();
    println!("=== LXC {vmid} AI CLI 업데이트: {} ===\n", names.join(", "));

    let st = cmd_output("pct", &["status", vmid]);
    if !st.contains("running") {
        eprintln!("[ai-update] LXC {vmid} 이 실행 중이 아닙니다.");
        return;
    }

    for agent in &agents {
        let (exists, _) = lxc_exec(
            vmid,
            &[
                "bash",
                "-c",
                &format!(
                    "export PATH=/usr/local/bin:$PATH && which {}",
                    agent.cli_binary
                ),
            ],
        );
        if !exists {
            continue;
        }

        let (_, before) = lxc_exec(
            vmid,
            &[
                "bash",
                "-c",
                &format!(
                    "export PATH=/usr/local/bin:$PATH && {} --version 2>/dev/null",
                    agent.cli_binary
                ),
            ],
        );
        println!("[ai-update] {} 업데이트 중... (현재: {before})", agent.name);

        let (ok, out) = lxc_exec(
            vmid,
            &[
                "bash",
                "-c",
                &format!(
                    "export PATH=/usr/local/bin:$PATH && npm install -g {}@latest",
                    agent.npm_package
                ),
            ],
        );
        if ok {
            let (_, after) = lxc_exec(
                vmid,
                &[
                    "bash",
                    "-c",
                    &format!(
                        "export PATH=/usr/local/bin:$PATH && {} --version 2>/dev/null",
                        agent.cli_binary
                    ),
                ],
            );
            println!(
                "[ai-update] {} 업데이트 완료: {before} -> {after}",
                agent.name
            );
        } else {
            eprintln!("[ai-update] {} 업데이트 실패: {out}", agent.name);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// perm-max — set maximum Claude Code permissions
// ═══════════════════════════════════════════════════════════════════════════════

fn perm_max(vmid: Option<&str>) {
    let max_permissions = serde_json::json!({
        "permissions": {
            "allow": [
                "Bash(*)",
                "Edit(*)",
                "Write(*)",
                "Read(*)",
                "Glob(*)",
                "Grep(*)",
                "WebSearch(*)",
                "WebFetch(*)",
                "Agent(*)",
                "NotebookEdit(*)",
                "mcp__*"
            ]
        }
    });
    let content = serde_json::to_string_pretty(&max_permissions).unwrap();

    let home = SOURCE_HOME;
    let host_path = format!("{home}/.claude/settings.local.json");
    fs::create_dir_all(format!("{home}/.claude")).ok();
    fs::write(&host_path, &content).ok();
    set_permissions(&host_path, 0o600);
    println!("[perm-max] 호스트 적용 완료: {host_path}");

    if let Some(vmid) = vmid {
        ensure_lxc_running(vmid);
        let pid = lxc_pid(vmid);
        let rootfs = format!("/proc/{pid}/root");
        let lxc_dir = format!("{rootfs}/root/.claude");
        let lxc_path = format!("{lxc_dir}/settings.local.json");
        fs::create_dir_all(&lxc_dir).ok();
        fs::write(&lxc_path, &content).ok();
        set_permissions(&lxc_path, 0o600);
        println!("[perm-max] LXC {vmid} 적용 완료: {lxc_path}");
    }

    println!("\n=== Claude Code 권한 최대 부여 완료 ===");
}

// ═══════════════════════════════════════════════════════════════════════════════
// credential-sync — copy host credentials to LXC
// ═══════════════════════════════════════════════════════════════════════════════

fn credential_sync_on(
    node: Option<&str>,
    vmid: &str,
    user: &str,
    home: Option<&str>,
    filter: &AgentFilter,
) {
    let local = local_node_name();
    let is_remote = node.is_some() && node != Some(&local);

    if !is_remote {
        ensure_lxc_running(vmid);
    }

    let target_home = home.unwrap_or_else(|| {
        if user == "root" {
            "/root"
        } else {
            Box::leak(format!("/home/{user}").into_boxed_str())
        }
    });

    let agents = filtered_agents(filter);
    let names: Vec<&str> = agents.iter().map(|a| a.name).collect();
    println!(
        "=== credential-sync: LXC {vmid} (user={user}, home={target_home}) -- {} ===\n",
        names.join(", ")
    );

    let src_home = SOURCE_HOME;
    let mut synced = 0u32;

    for agent in &agents {
        for file in agent.sync_files {
            let src = format!("{src_home}/{file}");
            if !Path::new(&src).exists() {
                println!("[cred-sync] 소스 없음, 건너뜀: {src}");
                continue;
            }

            let dst = format!("{target_home}/{file}");
            let dst_dir = dst.rsplit_once('/').map(|(d, _)| d).unwrap_or(target_home);

            lxc_exec_on(node, vmid, &["bash", "-c", &format!("mkdir -p {dst_dir}")]);

            let content = fs::read_to_string(&src).unwrap_or_else(|e| {
                eprintln!("[cred-sync] 파일 읽기 실패 {src}: {e}");
                std::process::exit(1);
            });

            let (ok, _) = lxc_exec_on(
                node,
                vmid,
                &[
                    "bash",
                    "-c",
                    &format!("cat > {dst} << 'CREDSYNCEOF'\n{content}\nCREDSYNCEOF"),
                ],
            );

            if ok {
                lxc_exec_on(node, vmid, &["chown", &format!("{user}:{user}"), &dst]);
                lxc_exec_on(node, vmid, &["chmod", "600", &dst]);
                println!("[cred-sync] Y {file}");
                synced += 1;
            } else {
                eprintln!("[cred-sync] N {file} 복사 실패");
            }
        }
    }

    println!("\n[cred-sync] 완료: {synced}개 파일 동기화됨");
}

// ═══════════════════════════════════════════════════════════════════════════════
// agent-recovery — repair error-state agents in LXC
// ═══════════════════════════════════════════════════════════════════════════════

fn agent_recovery(vmid: &str, db: &str, service: &str) {
    ensure_lxc_running(vmid);

    println!("=== agent-recovery: LXC {vmid} (db={db}, service={service}) ===\n");

    println!("[recovery] 에이전트 상태 조회...");
    let (ok, out) = lxc_exec(
        vmid,
        &[
            "bash",
            "-c",
            &format!(
                "sudo -u postgres psql -d {db} -t -A -c \
                 \"SELECT name || ':' || status FROM agents WHERE status NOT IN ('terminated');\""
            ),
        ],
    );
    if !ok {
        eprintln!("[recovery] DB 조회 실패: {out}");
        std::process::exit(1);
    }

    let mut error_count = 0u32;
    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, ':').collect();
        if parts.len() == 2 {
            let (name, status) = (parts[0], parts[1]);
            if status == "error" {
                error_count += 1;
            }
            let marker = if status == "error" { "N" } else { "Y" };
            println!("[recovery]   {marker} {name}: {status}");
        }
    }

    if error_count == 0 {
        println!("\n[recovery] error 상태 에이전트 없음.");
        return;
    }

    println!("\n[recovery] {error_count}개 에이전트 error -> active 전환...");
    let (ok, out) = lxc_exec(
        vmid,
        &[
            "bash",
            "-c",
            &format!(
                "sudo -u postgres psql -d {db} -c \
                 \"UPDATE agents SET status = 'active' WHERE status = 'error' RETURNING name, status;\""
            ),
        ],
    );
    if ok {
        println!("[recovery] {out}");
    } else {
        eprintln!("[recovery] 상태 업데이트 실패: {out}");
        std::process::exit(1);
    }

    println!("[recovery] 런타임 에러 초기화...");
    let _ = lxc_exec(
        vmid,
        &[
            "bash",
            "-c",
            &format!(
                "sudo -u postgres psql -d {db} -c \
                 \"UPDATE agent_runtime_state SET last_run_status = NULL, last_error = NULL \
                  WHERE last_run_status = 'failed';\""
            ),
        ],
    );

    println!("\n[recovery] {service} 서비스 재시작...");
    let (ok, _) = lxc_exec(vmid, &["systemctl", "restart", service]);
    if !ok {
        eprintln!("[recovery] 서비스 재시작 실패");
        std::process::exit(1);
    }

    std::thread::sleep(std::time::Duration::from_secs(3));
    let (ok, out) = lxc_exec(vmid, &["systemctl", "is-active", service]);
    if ok && out.trim() == "active" {
        println!("[recovery] {service} 서비스 정상 가동");
    } else {
        eprintln!("[recovery] {service} 서비스 상태 이상: {out}");
    }

    println!("\n[recovery] 완료");
}

// ═══════════════════════════════════════════════════════════════════════════════
// OpenClaw commands
// ═══════════════════════════════════════════════════════════════════════════════

fn openclaw_setup(vmid: &str) {
    println!("=== OpenClaw 세팅: LXC {vmid} ===\n");
    ensure_lxc_running(vmid);

    println!("-- 1/5 Node.js 22 설치 --\n");
    let (_, ver) = lxc_exec(
        vmid,
        &["bash", "-c", "node --version 2>/dev/null || echo none"],
    );
    let ver = ver.trim();
    if ver.starts_with("v22") || ver.starts_with("v24") {
        println!("[openclaw] Node.js {ver} 이미 설치됨");
    } else {
        println!("[openclaw] Node.js 업그레이드 중...");
        lxc_exec(
            vmid,
            &[
                "bash",
                "-c",
                "curl -fsSL https://deb.nodesource.com/setup_22.x | bash -",
            ],
        );
        lxc_exec(vmid, &["bash", "-c", "apt-get install -y nodejs"]);
    }

    println!("\n-- 2/5 OpenClaw CLI 설치 --\n");
    let (ok, _) = lxc_exec(
        vmid,
        &[
            "bash",
            "-c",
            "which openclaw >/dev/null 2>&1 && openclaw --version",
        ],
    );
    if !ok {
        lxc_exec(vmid, &["bash", "-c", "npm install -g openclaw"]);
        let (link_ok, _) = lxc_exec(vmid, &["bash", "-c", "which openclaw"]);
        if !link_ok {
            lxc_exec(
                vmid,
                &[
                    "bash",
                    "-c",
                    "ln -sf /usr/local/lib/node_modules/openclaw/openclaw.mjs /usr/local/bin/openclaw",
                ],
            );
        }
    }

    println!("\n-- 3/5 크리덴셜 복사 --\n");
    let home = SOURCE_HOME;
    lxc_exec(
        vmid,
        &["bash", "-c", "mkdir -p /root/.openclaw/agents/main/agent"],
    );
    let agent_src = format!("{home}/.openclaw/agents/main/agent");
    for file in &["auth-profiles.json", "models.json"] {
        let src = format!("{agent_src}/{file}");
        if Path::new(&src).exists() {
            let content = fs::read_to_string(&src).unwrap_or_default();
            let dst = format!("/root/.openclaw/agents/main/agent/{file}");
            let (ok, _) = lxc_exec(
                vmid,
                &[
                    "bash",
                    "-c",
                    &format!("cat > {dst} << 'CREDSYNCEOF'\n{content}\nCREDSYNCEOF"),
                ],
            );
            if ok {
                lxc_exec(vmid, &["chmod", "600", &dst]);
                println!("[openclaw] {file} 복사 완료");
            }
        }
    }

    let cfg_src = format!("{home}/.openclaw/openclaw.json");
    if Path::new(&cfg_src).exists() {
        let content = fs::read_to_string(&cfg_src).unwrap_or_default();
        let (ok, _) = lxc_exec(
            vmid,
            &[
                "bash",
                "-c",
                &format!(
                    "cat > /root/.openclaw/openclaw.json << 'CREDSYNCEOF'\n{content}\nCREDSYNCEOF"
                ),
            ],
        );
        if ok {
            println!("[openclaw] openclaw.json 복사 완료");
        }
    }

    println!("\n-- 4/5 gateway 설정 --\n");
    lxc_exec(
        vmid,
        &[
            "bash",
            "-c",
            "export PATH=/usr/local/bin:$PATH && openclaw config set gateway.mode local",
        ],
    );

    println!("\n-- 5/5 SSH 키 생성 --\n");
    let (has_key, _) = lxc_exec(
        vmid,
        &["bash", "-c", "test -f /root/.ssh/id_ed25519 && echo yes"],
    );
    if !has_key {
        lxc_exec(
            vmid,
            &[
                "bash",
                "-c",
                "ssh-keygen -t ed25519 -f /root/.ssh/id_ed25519 -N '' -q",
            ],
        );
    }

    println!("\n=== OpenClaw 세팅 완료 (LXC {vmid}) ===");
}

fn openclaw_telegram(vmid: &str, token: &str, account: &str) {
    println!("=== OpenClaw 텔레그램 봇 등록: LXC {vmid} ===\n");
    ensure_lxc_running(vmid);

    let cmd = format!(
        "export PATH=/usr/local/bin:$PATH && openclaw channels add --channel telegram --token {} --account {}",
        token, account
    );
    let (ok, out) = lxc_exec(vmid, &["bash", "-c", &cmd]);
    println!("{}", out.trim());

    if ok {
        let env_key = if account == "default" {
            "OPENCLAW_TELEGRAM_TOKEN".to_string()
        } else {
            format!(
                "OPENCLAW_TELEGRAM_TOKEN_{}",
                account.to_uppercase().replace('-', "_")
            )
        };
        let content = fs::read_to_string(ENV_FILE).unwrap_or_default();
        let key_prefix = format!("{env_key}=");
        if content.contains(&key_prefix) {
            let updated: String = content
                .lines()
                .map(|l| {
                    if l.starts_with(&key_prefix) {
                        format!("{env_key}={token}")
                    } else {
                        l.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(ENV_FILE, updated).ok();
        } else {
            if let Ok(mut f) = fs::OpenOptions::new().append(true).open(ENV_FILE) {
                use std::io::Write;
                writeln!(f, "\n{env_key}={token}").ok();
            }
        }
        println!("\n[openclaw] 텔레그램 봇 등록 완료");
    }
}

fn openclaw_server(vmid: &str, ip: &str, name: &str) {
    println!("=== OpenClaw 서버 추가: {name} ({ip}) -> LXC {vmid} ===\n");
    ensure_lxc_running(vmid);

    let (ok, pubkey) = lxc_exec(vmid, &["bash", "-c", "cat /root/.ssh/id_ed25519.pub"]);
    if !ok || pubkey.trim().is_empty() {
        eprintln!("[openclaw] SSH 공개키 없음. openclaw-setup 먼저 실행.");
        return;
    }
    let pubkey = pubkey.trim();

    println!("-- SSH 키 배포 --\n");
    let ssh_cmd = format!(
        "ssh -o StrictHostKeyChecking=no root@{ip} 'mkdir -p ~/.ssh && grep -qF \"{pubkey}\" ~/.ssh/authorized_keys 2>/dev/null || echo \"{pubkey}\" >> ~/.ssh/authorized_keys'",
    );
    let output = Command::new("bash").args(["-c", &ssh_cmd]).output();
    match output {
        Ok(o) if o.status.success() => println!("[openclaw] {ip} SSH 키 등록 완료"),
        _ => {
            eprintln!("[openclaw] {ip} SSH 키 등록 실패");
            return;
        }
    }

    let test_cmd = format!(
        "export PATH=/usr/local/bin:$PATH && ssh -o StrictHostKeyChecking=no -o ConnectTimeout=5 root@{ip} hostname"
    );
    let (ok, hostname) = lxc_exec(vmid, &["bash", "-c", &test_cmd]);
    if ok {
        println!("[openclaw] SSH 연결 확인: {} ({})", hostname.trim(), ip);
    } else {
        eprintln!("[openclaw] SSH 연결 실패: {ip}");
        return;
    }

    println!("\n-- CLAUDE.md 업데이트 --\n");
    let pid = lxc_pid(vmid);
    let rootfs = format!("/proc/{pid}/root");
    let workspace_dir = format!("{rootfs}/root/.openclaw/workspace");
    fs::create_dir_all(&workspace_dir).ok();
    let claude_md_path = format!("{workspace_dir}/CLAUDE.md");

    let existing = fs::read_to_string(&claude_md_path).unwrap_or_default();

    if existing.contains(ip) {
        println!("[openclaw] {ip} 이미 CLAUDE.md에 등록됨");
    } else if existing.is_empty() {
        let content = format!(
            "# Proxmox 관리 봇\n\n## 서버 목록\n\n| 이름 | IP | 접속 |\n|---|---|---|\n| {name} | {ip} | `ssh root@{ip}` |\n"
        );
        fs::write(&claude_md_path, content).ok();
        println!("[openclaw] CLAUDE.md 생성 -- {name} ({ip}) 등록");
    } else {
        let new_row = format!("| {name} | {ip} | `ssh root@{ip}` |");
        let updated = format!("{existing}\n{new_row}\n");
        fs::write(&claude_md_path, updated).ok();
        println!("[openclaw] CLAUDE.md에 {name} ({ip}) 추가");
    }

    println!("\n=== 서버 추가 완료: {name} ({ip}) ===");
}

fn openclaw_gateway(vmid: &str, action: &str) {
    ensure_lxc_running(vmid);

    match action {
        "start" => {
            println!("=== OpenClaw gateway 시작: LXC {vmid} ===\n");

            // Kill existing gateway
            let (_, pids) = lxc_exec(
                vmid,
                &[
                    "bash",
                    "-c",
                    "pgrep -f 'openclaw gateway' 2>/dev/null || true",
                ],
            );
            if !pids.trim().is_empty() {
                println!("[openclaw] 기존 gateway 프로세스 종료...");
                lxc_exec(
                    vmid,
                    &[
                        "bash",
                        "-c",
                        "pkill -9 -f 'openclaw gateway' 2>/dev/null || true",
                    ],
                );
                std::thread::sleep(std::time::Duration::from_secs(3));
            }

            // Ensure gateway.mode
            let (_, mode) = lxc_exec(
                vmid,
                &[
                    "bash",
                    "-c",
                    "export PATH=/usr/local/bin:$PATH && openclaw config get gateway.mode 2>&1 || true",
                ],
            );
            if mode.contains("not found") || mode.contains("unset") || mode.trim().is_empty() {
                lxc_exec(
                    vmid,
                    &[
                        "bash",
                        "-c",
                        "export PATH=/usr/local/bin:$PATH && openclaw config set gateway.mode local",
                    ],
                );
            }

            // Spawn gateway
            lxc_exec(
                vmid,
                &[
                    "bash",
                    "-c",
                    "export PATH=/usr/local/bin:$PATH && nohup openclaw gateway run > /var/log/openclaw-gateway.log 2>&1 &",
                ],
            );
            std::thread::sleep(std::time::Duration::from_secs(8));

            let (_, out) = lxc_exec(
                vmid,
                &[
                    "bash",
                    "-c",
                    "export PATH=/usr/local/bin:$PATH && openclaw channels status 2>&1",
                ],
            );
            println!("{}", out.trim());
            println!("\n[openclaw] gateway 시작 완료");
        }
        "stop" => {
            println!("=== OpenClaw gateway 중지: LXC {vmid} ===\n");
            let (_, out) = lxc_exec(
                vmid,
                &[
                    "bash",
                    "-c",
                    "pkill -9 -f 'openclaw gateway' 2>/dev/null && echo '중지 완료' || echo '실행 중인 gateway 없음'",
                ],
            );
            println!("[openclaw] {}", out.trim());
        }
        "status" => {
            println!("=== OpenClaw gateway 상태: LXC {vmid} ===\n");
            let (_, out) = lxc_exec(
                vmid,
                &[
                    "bash",
                    "-c",
                    "export PATH=/usr/local/bin:$PATH && openclaw channels status 2>&1 && echo '---' && openclaw models status 2>&1",
                ],
            );
            println!("{}", out.trim());
        }
        _ => {
            eprintln!("[openclaw] 알 수 없는 action: {action} (start/stop/status)");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ComfyUI commands — thin wrappers that delegate to shell scripts
// ═══════════════════════════════════════════════════════════════════════════════

fn find_comfyui_vmid(node: &str) -> String {
    let output = cmd_output(
        "pvesh",
        &[
            "get",
            &format!("/nodes/{node}/lxc"),
            "--output-format",
            "json",
        ],
    );
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&output) {
        if let Some(lxcs) = parsed.as_array() {
            for lxc in lxcs {
                let tags = lxc["tags"].as_str().unwrap_or("");
                let name = lxc["name"].as_str().unwrap_or("");
                if tags.contains("comfyui") || name.contains("comfyui") || name.contains("ai-img")
                {
                    return lxc["vmid"].as_u64().unwrap_or(0).to_string();
                }
            }
        }
    }
    eprintln!("[comfyui] 노드 '{node}'에 ComfyUI LXC를 찾을 수 없습니다. --vmid 지정 필요");
    std::process::exit(1);
}

fn get_remote_lxc_ip(node: &str, vmid: &str) -> String {
    let conf_path = format!("/etc/pve/nodes/{node}/lxc/{vmid}.conf");
    if let Ok(content) = fs::read_to_string(&conf_path) {
        for line in content.lines() {
            if line.starts_with("net0:") {
                if let Some(ip) = line.split(',').find_map(|p| p.strip_prefix("ip=")) {
                    return ip.split('/').next().unwrap_or(ip).to_string();
                }
            }
        }
    }
    "unknown".to_string()
}

fn comfyui_setup(node: &str, vmid: Option<&str>, gpu_ids: &str) {
    println!("=== ComfyUI VFX 세팅 ===\n");

    let target_vmid = vmid
        .map(|v| v.to_string())
        .unwrap_or_else(|| find_comfyui_vmid(node));
    println!("[comfyui] 노드: {node}, VMID: {target_vmid}, GPU: {gpu_ids}");

    // Check LXC exists and is running
    let lxc_json = cmd_output(
        "pvesh",
        &[
            "get",
            &format!("/nodes/{node}/lxc/{target_vmid}/status/current"),
            "--output-format",
            "json",
        ],
    );
    if lxc_json.is_empty() || lxc_json.contains("does not exist") {
        eprintln!("[comfyui] LXC {target_vmid} 가 노드 '{node}' 에 존재하지 않습니다.");
        std::process::exit(1);
    }

    // Install ComfyUI if missing
    let (_, check) = lxc_exec_on(
        Some(node),
        &target_vmid,
        &[
            "bash",
            "-lc",
            "ls /opt/comfyui/main.py 2>/dev/null && echo EXISTS",
        ],
    );
    if !check.contains("EXISTS") {
        println!("  ComfyUI 미설치 -- 설치 중...");
        let cmds = [
            "apt-get update -qq && apt-get install -y -qq python3 python3-venv python3-pip git wget ffmpeg libgl1 libglib2.0-0 libgomp1 libsm6 libxext6",
            "python3 -m venv /opt/comfyui-venv",
            "git clone https://github.com/comfyanonymous/ComfyUI.git /opt/comfyui",
            "/opt/comfyui-venv/bin/pip install -q torch torchvision torchaudio --index-url https://download.pytorch.org/whl/cu124",
            "cd /opt/comfyui && /opt/comfyui-venv/bin/pip install -q -r requirements.txt",
        ];
        for cmd in &cmds {
            let (ok, out) = lxc_exec_on(Some(node), &target_vmid, &["bash", "-lc", cmd]);
            if !ok {
                eprintln!("  실패: {}", out.lines().last().unwrap_or(""));
            }
        }
    } else {
        println!("  ComfyUI 이미 설치됨");
    }

    // Setup systemd service
    let cuda_devices = gpu_ids;
    let service_script = format!(
        "printf '[Unit]\\nDescription=ComfyUI\\nAfter=network.target\\n\\n[Service]\\nType=simple\\nUser=root\\nWorkingDirectory=/opt/comfyui\\nExecStart=/opt/comfyui-venv/bin/python3 main.py --listen 0.0.0.0 --port 8188\\nRestart=on-failure\\nRestartSec=10\\nEnvironment=CUDA_VISIBLE_DEVICES={cuda_devices}\\n\\n[Install]\\nWantedBy=multi-user.target\\n' > /etc/systemd/system/comfyui.service && systemctl daemon-reload && systemctl enable comfyui && systemctl restart comfyui"
    );
    let (ok, _) = lxc_exec_on(
        Some(node),
        &target_vmid,
        &["bash", "-lc", &service_script],
    );
    if ok {
        println!("  comfyui.service 설정 완료");
    }

    println!("\n=== ComfyUI VFX 세팅 완료 ===");
}

fn comfyui_sync_watch(node: &str, action: &str) {
    match action {
        "install" => {
            let svc = format!(
                "[Unit]\nDescription=prelik ComfyUI sync ({node})\nAfter=pve-cluster.service\n\n\
[Service]\nType=oneshot\nExecStart=/usr/local/bin/proxmox-host-setup ai comfyui-setup --node {node}\n"
            );
            let timer = "[Unit]\nDescription=prelik ComfyUI sync timer\n\n\
[Timer]\nOnBootSec=5min\nOnUnitActiveSec=1h\nUnit=phs-comfyui-sync.service\n\n\
[Install]\nWantedBy=timers.target\n";
            fs::write(
                "/etc/systemd/system/phs-comfyui-sync.service",
                svc,
            )
            .ok();
            fs::write(
                "/etc/systemd/system/phs-comfyui-sync.timer",
                timer,
            )
            .ok();
            for cmd in [
                vec!["daemon-reload"],
                vec!["enable", "phs-comfyui-sync.timer"],
                vec!["start", "phs-comfyui-sync.timer"],
            ] {
                let _ = Command::new("systemctl").args(&cmd).status();
            }
            println!("[comfyui-sync-watch] timer 설치 완료");
        }
        "uninstall" => {
            let _ = Command::new("systemctl")
                .args(["stop", "phs-comfyui-sync.timer"])
                .status();
            let _ = Command::new("systemctl")
                .args(["disable", "phs-comfyui-sync.timer"])
                .status();
            let _ = fs::remove_file("/etc/systemd/system/phs-comfyui-sync.service");
            let _ = fs::remove_file("/etc/systemd/system/phs-comfyui-sync.timer");
            let _ = Command::new("systemctl").args(["daemon-reload"]).status();
            println!("[comfyui-sync-watch] timer 제거 완료");
        }
        "status" => {
            let active = cmd_output("systemctl", &["is-active", "phs-comfyui-sync.timer"]);
            println!("  active: {active}");
            let next = cmd_output(
                "systemctl",
                &[
                    "show",
                    "phs-comfyui-sync.timer",
                    "--property=NextElapseUSecRealtime",
                    "--value",
                ],
            );
            println!("  next: {next}");
        }
        _ => eprintln!("[comfyui-sync-watch] action: install / uninstall / status"),
    }
}

fn comfyui_mcp_setup(node: &str, vmid: Option<&str>) {
    println!("=== ComfyUI MCP -> Claude Code 등록 ===\n");
    let target_vmid = vmid
        .map(|v| v.to_string())
        .unwrap_or_else(|| find_comfyui_vmid(node));

    let (ok, _) = lxc_exec_on(
        Some(node),
        &target_vmid,
        &["bash", "-c", "test -x /usr/local/bin/claude && echo OK"],
    );
    if !ok {
        eprintln!("  Claude Code 미설치. 먼저: prelik-ai mount --vmid {target_vmid}");
        std::process::exit(1);
    }

    let _ = lxc_exec_on(
        Some(node),
        &target_vmid,
        &[
            "bash",
            "-c",
            "export PATH=/usr/local/bin:$PATH && claude mcp remove comfyui --scope user 2>&1 | head -3",
        ],
    );

    let add_cmd = "export PATH=/usr/local/bin:$PATH && \
                   claude mcp add comfyui --scope user \
                     --env COMFYUI_PATH=/opt/comfyui \
                     --env COMFYUI_HOST=http://127.0.0.1:8188 \
                     -- npx -y comfyui-mcp 2>&1";
    let (add_ok, add_out) = lxc_exec_on(Some(node), &target_vmid, &["bash", "-c", add_cmd]);
    if !add_ok {
        eprintln!("  MCP 등록 실패: {add_out}");
        std::process::exit(1);
    }
    println!("  comfyui MCP 등록 완료");
}

fn comfyui_test(node: &str, vmid: Option<&str>) {
    println!("=== ComfyUI Live Test (node: {node}) ===\n");

    let target_vmid = vmid
        .map(|v| v.to_string())
        .unwrap_or_else(|| find_comfyui_vmid(node));
    let lxc_ip = get_remote_lxc_ip(node, &target_vmid);
    let api_url = format!("http://{lxc_ip}:8188");

    println!("[1/3] API 응답 확인...");
    let api_check = Command::new("curl")
        .args([
            "-s",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "--connect-timeout",
            "5",
            &format!("{api_url}/"),
        ])
        .output();
    match api_check {
        Ok(out) => {
            let code = String::from_utf8_lossy(&out.stdout);
            if code == "200" {
                println!("  Y {api_url} -> 200");
            } else {
                eprintln!("  N {api_url} -> {code}");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("  N curl 실패: {e}");
            std::process::exit(1);
        }
    }

    println!("\n[2/3] 시스템 정보...");
    let stats = Command::new("curl")
        .args(["-s", "--max-time", "5", &format!("{api_url}/system_stats")])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&stats) {
        let version = parsed["system"]["comfyui_version"].as_str().unwrap_or("?");
        println!("  ComfyUI: {version}");
    }

    println!("\n[3/3] 서비스 상태...");
    let (_, svc) = lxc_exec_on(
        Some(node),
        &target_vmid,
        &["bash", "-lc", "systemctl is-active comfyui"],
    );
    println!("  comfyui.service: {}", svc.trim());

    println!("\n=== Live Test 완료 ===");
}

fn comfyui_status_cmd(node: &str, vmid: Option<&str>) {
    println!("=== ComfyUI Status (node: {node}) ===\n");

    let target_vmid = vmid
        .map(|v| v.to_string())
        .unwrap_or_else(|| find_comfyui_vmid(node));
    let lxc_ip = get_remote_lxc_ip(node, &target_vmid);

    let (_, svc) = lxc_exec_on(
        Some(node),
        &target_vmid,
        &["bash", "-lc", "systemctl is-active comfyui"],
    );
    println!("[서비스] comfyui.service: {}", svc.trim());

    let api_url = format!("http://{lxc_ip}:8188");
    let stats = Command::new("curl")
        .args(["-s", "--max-time", "5", &format!("{api_url}/system_stats")])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&stats) {
        let version = parsed["system"]["comfyui_version"]
            .as_str()
            .unwrap_or("?");
        let pytorch = parsed["system"]["pytorch_version"]
            .as_str()
            .unwrap_or("?");
        println!("[API] ComfyUI: {version}, PyTorch: {pytorch}");
    } else {
        println!("[API] 응답 없음");
    }

    let (_, gpus) = lxc_exec_on(
        Some(node),
        &target_vmid,
        &[
            "bash",
            "-lc",
            "nvidia-smi --query-gpu=index,memory.used,memory.total --format=csv,noheader 2>/dev/null",
        ],
    );
    if !gpus.is_empty() {
        println!("[GPU] {gpus}");
    }

    let (_, disk) = lxc_exec_on(
        Some(node),
        &target_vmid,
        &[
            "bash",
            "-lc",
            "df -h / | tail -1 | awk '{print $3, \"used /\", $2, \"(\", $5, \")\"}'",
        ],
    );
    println!("[디스크] {disk}");
}

fn comfyui_cleanup(node: &str, vmid: Option<&str>, apply: bool) {
    println!("=== ComfyUI Cleanup (node: {node}) ===\n");

    let target_vmid = vmid
        .map(|v| v.to_string())
        .unwrap_or_else(|| find_comfyui_vmid(node));

    let scan_cmd = "find /opt/comfyui/models -type f -size +100M 2>/dev/null | xargs -I{} sh -c 'echo $(stat -c %s {}) {}' | sort -rn";
    let (_, out) = lxc_exec_on(Some(node), &target_vmid, &["bash", "-lc", scan_cmd]);

    let mut total: u64 = 0;
    let mut count = 0u32;
    for line in out.lines() {
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        if parts.len() != 2 {
            continue;
        }
        let size: u64 = parts[0].parse().unwrap_or(0);
        let path = parts[1];
        let size_mb = size / 1024 / 1024;
        println!("  {path} ({size_mb} MB)");
        total += size;
        count += 1;
    }

    println!("\n  {count}개 파일, {:.1} GB", total as f64 / 1e9);

    if !apply {
        println!("\n  실제 삭제: prelik-ai comfyui-cleanup --node {node} --apply");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Cluster files / SSH deploy
// ═══════════════════════════════════════════════════════════════════════════════

const CLUSTER_FILES_ROOT: &str = "/mnt/files";

fn cluster_files_setup() {
    println!("=== Cluster Files (pve 호스트) ===\n");

    let _ = fs::create_dir_all(CLUSTER_FILES_ROOT);

    // Install filebrowser
    let _ = Command::new("bash")
        .arg("-c")
        .arg(
            "test -x /usr/local/bin/filebrowser || \
             (cd /tmp && wget -q https://github.com/filebrowser/filebrowser/releases/download/v2.63.1/linux-amd64-filebrowser.tar.gz -O fb.tar.gz && \
              tar xzf fb.tar.gz -C /usr/local/bin/ filebrowser && chmod 755 /usr/local/bin/filebrowser && rm fb.tar.gz)"
        )
        .status();
    println!("  filebrowser 설치됨");

    // Sync timer
    let sync_svc = "[Unit]\nDescription=prelik cluster-files sync\n\n\
[Service]\nType=oneshot\nExecStart=/usr/local/bin/proxmox-host-setup ai cluster-files-sync\n";
    let _ = fs::write(
        "/etc/systemd/system/cluster-files-sync.service",
        sync_svc,
    );

    let sync_timer = "[Unit]\nDescription=cluster-files sync timer\n\n\
[Timer]\nOnBootSec=30s\nOnUnitActiveSec=2min\nUnit=cluster-files-sync.service\n\n\
[Install]\nWantedBy=timers.target\n";
    let _ = fs::write(
        "/etc/systemd/system/cluster-files-sync.timer",
        sync_timer,
    );

    for cmd in [
        vec!["daemon-reload"],
        vec!["enable", "cluster-files-sync.timer"],
        vec!["start", "cluster-files-sync.timer"],
    ] {
        let _ = Command::new("systemctl").args(&cmd).status();
    }

    cluster_files_sync();

    println!("\n=== 완료 ===");
    println!("  루트: {CLUSTER_FILES_ROOT}");
    println!("  자동: 2분마다 새 LXC 발견/제거");
}

fn cluster_files_sync() {
    let _ = fs::create_dir_all(CLUSTER_FILES_ROOT);

    // NAS symlink
    let nas_link = format!("{CLUSTER_FILES_ROOT}/nas");
    if Path::new("/mnt/synology").exists() {
        let lp = Path::new(&nas_link);
        if !lp.exists() {
            let _ = std::os::unix::fs::symlink("/mnt/synology", &nas_link);
        }
    }

    let nodes_json = cmd_output("pvesh", &["get", "/nodes", "--output-format", "json"]);
    let nodes: Vec<serde_json::Value> =
        serde_json::from_str(&nodes_json).unwrap_or_default();
    let local_node = local_node_name();

    let mut succeeded: u32 = 0;
    let mut already: u32 = 0;
    let mut failed: u32 = 0;

    for node in &nodes {
        let node_name = node["node"].as_str().unwrap_or("");
        if node_name.is_empty() {
            continue;
        }

        let lxc_json = cmd_output(
            "pvesh",
            &[
                "get",
                &format!("/nodes/{node_name}/lxc"),
                "--output-format",
                "json",
            ],
        );
        let lxcs: Vec<serde_json::Value> =
            serde_json::from_str(&lxc_json).unwrap_or_default();

        for lxc in &lxcs {
            let vmid = lxc["vmid"].as_u64().unwrap_or(0);
            let name = lxc["name"].as_str().unwrap_or("?");
            let status = lxc["status"].as_str().unwrap_or("");
            if vmid == 0 || status != "running" {
                continue;
            }

            let mount_dir =
                format!("{CLUSTER_FILES_ROOT}/{node_name}/{vmid}-{name}");
            let _ = fs::create_dir_all(&mount_dir);

            let mounted = cmd_output("findmnt", &["-n", "--target", &mount_dir]);
            if mounted
                .lines()
                .any(|l| l.split_whitespace().next() == Some(mount_dir.as_str()))
            {
                already += 1;
                continue;
            }

            // Get LXC IP for sshfs
            let conf_path =
                format!("/etc/pve/nodes/{node_name}/lxc/{vmid}.conf");
            let lxc_ip = fs::read_to_string(&conf_path)
                .ok()
                .and_then(|c| {
                    c.lines()
                        .find(|l| l.starts_with("net0:"))
                        .and_then(|l| l.split(',').find_map(|p| p.strip_prefix("ip=")))
                        .map(|ip| ip.split('/').next().unwrap_or(ip).to_string())
                })
                .unwrap_or_default();
            if lxc_ip.is_empty() {
                failed += 1;
                continue;
            }

            let _ = Command::new("sshfs")
                .args([
                    "-o",
                    "allow_other,default_permissions,reconnect,StrictHostKeyChecking=no,UserKnownHostsFile=/dev/null,ServerAliveInterval=15,ConnectTimeout=5,BatchMode=yes,ro",
                    &format!("root@{lxc_ip}:/"),
                    &mount_dir,
                ])
                .output();

            // Verify mount
            let post = cmd_output("findmnt", &["-n", "--target", &mount_dir]);
            if post
                .lines()
                .any(|l| l.split_whitespace().next() == Some(mount_dir.as_str()))
            {
                succeeded += 1;
            } else {
                failed += 1;
            }
            let _ = &local_node; // silence unused
        }
    }

    println!(
        "[cluster-files] mounted={succeeded} already={already} failed={failed}"
    );
}

fn cluster_files_reset() {
    println!("=== cluster-files reset ===");
    if !Path::new(CLUSTER_FILES_ROOT).exists() {
        println!("  {CLUSTER_FILES_ROOT} 없음");
        return;
    }

    let raw = cmd_output("findmnt", &["-rn", "-o", "TARGET"]);
    let mut targets: Vec<String> = raw
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|t| t.starts_with(&format!("{CLUSTER_FILES_ROOT}/")))
        .collect();
    targets.sort_by_key(|t| std::cmp::Reverse(t.matches('/').count()));
    let total = targets.len();
    println!("  발견된 마운트: {total}");

    let mut umounted = 0u32;
    for t in &targets {
        let st = Command::new("umount").args(["-l", t]).status();
        if st.map(|s| s.success()).unwrap_or(false) {
            umounted += 1;
        }
    }
    println!("  umount 성공: {umounted}/{total}");

    // Clean empty dirs
    let mut removed = 0u32;
    if let Ok(node_dirs) = fs::read_dir(CLUSTER_FILES_ROOT) {
        for nd in node_dirs.flatten() {
            let p = nd.path();
            let is_symlink = fs::symlink_metadata(&p)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            if is_symlink || !p.is_dir() {
                continue;
            }
            if let Ok(lxc_dirs) = fs::read_dir(&p) {
                for ld in lxc_dirs.flatten() {
                    let lp = ld.path();
                    if fs::read_dir(&lp)
                        .map(|mut it| it.next().is_none())
                        .unwrap_or(false)
                    {
                        if fs::remove_dir(&lp).is_ok() {
                            removed += 1;
                        }
                    }
                }
            }
            if fs::read_dir(&p)
                .map(|mut it| it.next().is_none())
                .unwrap_or(false)
            {
                let _ = fs::remove_dir(&p);
            }
        }
    }
    println!("  빈 디렉토리 제거: {removed}");
}

fn cluster_ssh_deploy() {
    println!("=== Cluster SSH Key Deploy ===\n");

    let pubkey = ["/root/.ssh/id_ed25519.pub", "/root/.ssh/id_rsa.pub"]
        .iter()
        .find_map(|p| {
            fs::read_to_string(p)
                .ok()
                .map(|c| (p.to_string(), c.trim().to_string()))
        })
        .filter(|(_, k)| !k.is_empty());
    let (key_path, pubkey) = match pubkey {
        Some(v) => v,
        None => {
            eprintln!("SSH 공개키 없음");
            std::process::exit(1);
        }
    };
    println!("[1/3] 공개키: {key_path}");

    let nodes_json = cmd_output("pvesh", &["get", "/nodes", "--output-format", "json"]);
    let nodes: Vec<serde_json::Value> =
        serde_json::from_str(&nodes_json).unwrap_or_default();
    let local_node = local_node_name();

    let key_b64 = base64_encode(&pubkey);

    println!("\n[2/3] LXC 키 배포...");
    let mut added = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;

    for node in &nodes {
        let node_name = node["node"].as_str().unwrap_or("");
        if node_name.is_empty() {
            continue;
        }
        let is_local = node_name == local_node;

        let lxc_json = cmd_output(
            "pvesh",
            &[
                "get",
                &format!("/nodes/{node_name}/lxc"),
                "--output-format",
                "json",
            ],
        );
        let lxcs: Vec<serde_json::Value> =
            serde_json::from_str(&lxc_json).unwrap_or_default();

        for lxc in &lxcs {
            let vmid = lxc["vmid"].as_u64().unwrap_or(0);
            let name = lxc["name"].as_str().unwrap_or("?");
            let status = lxc["status"].as_str().unwrap_or("");
            if vmid == 0 || status != "running" {
                continue;
            }

            let inner_cmd = format!(
                "mkdir -p /root/.ssh && chmod 700 /root/.ssh && \
                 KEY=$(echo {key_b64} | base64 -d) && \
                 if grep -qF \"$KEY\" /root/.ssh/authorized_keys 2>/dev/null; then \
                   echo SKIP; \
                 else \
                   echo \"$KEY\" >> /root/.ssh/authorized_keys && chmod 600 /root/.ssh/authorized_keys && echo ADDED; \
                 fi"
            );

            let result = if is_local {
                Command::new("pct")
                    .args([
                        "exec",
                        &vmid.to_string(),
                        "--",
                        "bash",
                        "-c",
                        &inner_cmd,
                    ])
                    .output()
            } else {
                let node_ip_str = node_ip_from_name(node_name);
                Command::new("ssh")
                    .args([
                        "-o",
                        "ConnectTimeout=5",
                        "-o",
                        "StrictHostKeyChecking=no",
                        "-o",
                        "BatchMode=yes",
                        &format!("root@{node_ip_str}"),
                        &format!(
                            "pct exec {vmid} -- bash -c {}",
                            shell_escape(&inner_cmd)
                        ),
                    ])
                    .output()
            };

            match result {
                Ok(out) if out.status.success() => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if stdout.contains("ADDED") {
                        println!("  Y {node_name} {vmid} {name}");
                        added += 1;
                    } else if stdout.contains("SKIP") {
                        skipped += 1;
                    }
                }
                _ => {
                    eprintln!("  N {node_name} {vmid} {name}");
                    failed += 1;
                }
            }
        }
    }

    println!("\n[3/3] 결과: added={added}, skipped={skipped}, failed={failed}");
}

fn filebrowser_setup(node: &str, vmid: Option<&str>) {
    println!("=== File Browser 설치 ===\n");

    let target_vmid = vmid
        .map(|v| v.to_string())
        .unwrap_or_else(|| find_comfyui_vmid(node));
    let lxc_ip = get_remote_lxc_ip(node, &target_vmid);

    // Install filebrowser binary
    let install_cmd = "test -x /usr/local/bin/filebrowser || \
         (cd /tmp && rm -f filebrowser.tar.gz && \
          wget -q https://github.com/filebrowser/filebrowser/releases/download/v2.63.1/linux-amd64-filebrowser.tar.gz -O filebrowser.tar.gz && \
          tar xzf filebrowser.tar.gz -C /usr/local/bin/ filebrowser && \
          chmod 755 /usr/local/bin/filebrowser && rm -f filebrowser.tar.gz)";
    let (ok, _) = lxc_exec_on(Some(node), &target_vmid, &["bash", "-lc", install_cmd]);
    if !ok {
        eprintln!("  filebrowser 설치 실패");
        std::process::exit(1);
    }

    // Initialize DB
    let init_cmd = "mkdir -p /var/lib/filebrowser && \
         (test -f /var/lib/filebrowser/filebrowser.db || \
          (/usr/local/bin/filebrowser config init -d /var/lib/filebrowser/filebrowser.db && \
           /usr/local/bin/filebrowser config set --address 0.0.0.0 --port 8080 --root /opt/comfyui --auth.method=json -d /var/lib/filebrowser/filebrowser.db))";
    let _ = lxc_exec_on(Some(node), &target_vmid, &["bash", "-lc", init_cmd]);

    // systemd service
    let svc = "[Unit]\nDescription=File Browser\nAfter=network.target\n\n\
[Service]\nType=simple\nExecStart=/usr/local/bin/filebrowser -d /var/lib/filebrowser/filebrowser.db\nRestart=on-failure\nRestartSec=5\n\n\
[Install]\nWantedBy=multi-user.target\n";
    let svc_b64 = base64_encode(svc);
    let svc_cmd = format!(
        "echo '{svc_b64}' | base64 -d > /etc/systemd/system/filebrowser.service && \
         systemctl daemon-reload && systemctl enable filebrowser && systemctl restart filebrowser"
    );
    let _ = lxc_exec_on(Some(node), &target_vmid, &["bash", "-lc", &svc_cmd]);

    println!("=== File Browser 설치 완료 ===");
    println!("  LXC: {target_vmid}");
    println!("  직접: http://{lxc_ip}:8080");
}

// ═══════════════════════════════════════════════════════════════════════════════
// VM commands
// ═══════════════════════════════════════════════════════════════════════════════

fn vm_exec(ip: &str, user: &str, args: &[&str]) -> (bool, String) {
    let joined = if args.len() > 1 {
        args.join(" ")
    } else {
        args.first().unwrap_or(&"").to_string()
    };
    let target = format!("{user}@{ip}");
    match Command::new("ssh")
        .args([
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "ConnectTimeout=10",
            &target,
            &joined,
        ])
        .output()
    {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).trim().to_string();
            (o.status.success(), stdout)
        }
        Err(e) => (false, format!("ssh 실패: {e}")),
    }
}

fn vm_scp(ip: &str, user: &str, src: &str, dst: &str) -> bool {
    Command::new("scp")
        .args([
            "-o",
            "StrictHostKeyChecking=no",
            src,
            &format!("{user}@{ip}:{dst}"),
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn vm_mount(vmid: &str, ip: &str, user: &str, filter: &AgentFilter) {
    let agents = filtered_agents(filter);
    let names: Vec<&str> = agents.iter().map(|a| a.name).collect();
    println!(
        "=== AI 에이전트 -> VM {vmid} 배포 ({ip}): {} ===\n",
        names.join(", ")
    );

    // SSH test
    let (ok, _) = vm_exec(ip, user, &["echo", "OK"]);
    if !ok {
        eprintln!("[ai-vm-mount] SSH 접속 실패: {user}@{ip}");
        return;
    }

    let home = SOURCE_HOME;
    let remote_home = if user == "root" {
        "/root".to_string()
    } else {
        format!("/home/{user}")
    };
    let mut synced = 0;

    for agent in &agents {
        let source_dir = format!("{home}/{}", agent.dir);
        if !Path::new(&source_dir).exists() {
            println!("[ai-vm-mount] {} - 호스트에 없음, 스킵", agent.dir);
            continue;
        }

        let remote_dir = format!("{remote_home}/{}", agent.dir);
        let _ = vm_exec(ip, user, &["mkdir", "-p", &remote_dir]);

        let mut copied = 0;
        for file in agent.mount_files {
            let src = format!("{source_dir}/{file}");
            if !Path::new(&src).exists() {
                continue;
            }
            let dst = format!("{remote_dir}/{file}");
            if vm_scp(ip, user, &src, &dst) {
                let _ = vm_exec(ip, user, &["chmod", "600", &dst]);
                copied += 1;
            }
        }

        if copied > 0 {
            let _ = vm_exec(ip, user, &["chmod", "700", &remote_dir]);
            println!("[ai-vm-mount] {} - {copied}개 파일 복사 완료", agent.dir);
            synced += 1;
        }

        for file in agent.home_files {
            let src = format!("{home}/{file}");
            if !Path::new(&src).exists() {
                continue;
            }
            let dst = format!("{remote_home}/{file}");
            if vm_scp(ip, user, &src, &dst) {
                if *file == ".claude.json" {
                    let tmp = format!("/tmp/.prelik-vm-{vmid}-claude.json");
                    let _ = fs::copy(&src, &tmp);
                    patch_claude_json_for_npm(&tmp);
                    let _ = vm_scp(ip, user, &tmp, &dst);
                    let _ = fs::remove_file(&tmp);
                }
                let _ = vm_exec(ip, user, &["chmod", "600", &dst]);
                println!("[ai-vm-mount] {file} 복사 완료");
            }
        }
    }

    // Plugins
    if agents.iter().any(|a| a.name == "claude") {
        let plugin_src = format!("{home}/.claude/plugins");
        if Path::new(&plugin_src).exists() {
            let remote_plugin = format!("{remote_home}/.claude/plugins");
            let _ = vm_exec(ip, user, &["mkdir", "-p", &remote_plugin]);
            let _ = Command::new("scp")
                .args([
                    "-r",
                    "-o",
                    "StrictHostKeyChecking=no",
                    &format!("{plugin_src}/"),
                    &format!("{user}@{ip}:{remote_plugin}/"),
                ])
                .output();
        }
    }

    if synced > 0 {
        println!("\n[ai-vm-mount] {synced}개 에이전트 배포 완료");
    }

    // Install CLIs in VM
    install_agents_in_vm(vmid, ip, user, filter);
}

fn install_agents_in_vm(_vmid: &str, ip: &str, user: &str, filter: &AgentFilter) {
    let agents = filtered_agents(filter);
    let sudo = if user == "root" { "" } else { "sudo " };

    // Ensure Node.js
    let (node_ok, _) = vm_exec(
        ip,
        user,
        &["bash -c 'export PATH=/usr/local/bin:$PATH && which node'"],
    );
    if !node_ok {
        let _ = vm_exec(
            ip,
            user,
            &[&format!(
                "bash -c '{sudo}apt-get update -qq && {sudo}apt-get install -y -qq nodejs npm'"
            )],
        );
    }

    for agent in &agents {
        if !Path::new(&format!("{}/{}", SOURCE_HOME, agent.dir)).exists() {
            continue;
        }
        let (ok, _) = vm_exec(
            ip,
            user,
            &[&format!(
                "bash -c 'export PATH=/usr/local/bin:$PATH && which {}'",
                agent.cli_binary
            )],
        );
        if !ok {
            println!("[vm-install] {} 설치 중...", agent.name);
            let (ok, out) = vm_exec(
                ip,
                user,
                &[&format!(
                    "bash -c 'export PATH=/usr/local/bin:$PATH && {sudo}npm install -g {}'",
                    agent.npm_package
                )],
            );
            if ok {
                println!("[vm-install] {} 설치 완료", agent.name);
            } else {
                eprintln!("[vm-install] {} 설치 실패: {out}", agent.name);
            }
        } else {
            println!("[vm-install] {} 이미 설치됨", agent.name);
        }
    }
}

fn vm_unmount(vmid: &str, ip: &str, user: &str, filter: &AgentFilter) {
    let agents = filtered_agents(filter);
    let names: Vec<&str> = agents.iter().map(|a| a.name).collect();
    println!(
        "=== AI 에이전트 <- VM {vmid} 제거 ({ip}): {} ===\n",
        names.join(", ")
    );

    let remote_home = if user == "root" {
        "/root".to_string()
    } else {
        format!("/home/{user}")
    };
    let mut removed = 0;

    for agent in &agents {
        let remote_dir = format!("{remote_home}/{}", agent.dir);
        let (exists, _) = vm_exec(ip, user, &["test", "-d", &remote_dir]);
        if exists {
            let (ok, _) = vm_exec(ip, user, &["rm", "-rf", &remote_dir]);
            if ok {
                println!("[ai-vm-unmount] {} 삭제 완료", agent.dir);
                removed += 1;
            }
        }

        for file in agent.home_files {
            let _ = vm_exec(ip, user, &["rm", "-f", &format!("{remote_home}/{file}")]);
        }
    }

    if removed > 0 {
        println!("\n[ai-vm-unmount] {removed}개 에이전트 제거 완료");
    }
}

fn vm_enter(ip: &str, user: &str) {
    let _ = Command::new("ssh")
        .args([
            "-o",
            "StrictHostKeyChecking=no",
            &format!("{user}@{ip}"),
        ])
        .status();
}

fn vm_update(vmid: &str, ip: &str, user: &str, filter: &AgentFilter) {
    let agents = filtered_agents(filter);
    let names: Vec<&str> = agents.iter().map(|a| a.name).collect();
    println!(
        "=== VM {vmid} AI CLI 업데이트 ({ip}): {} ===\n",
        names.join(", ")
    );

    let sudo = if user == "root" { "" } else { "sudo " };

    for agent in &agents {
        let (exists, _) = vm_exec(
            ip,
            user,
            &[&format!(
                "bash -c 'export PATH=/usr/local/bin:$PATH && which {}'",
                agent.cli_binary
            )],
        );
        if !exists {
            continue;
        }

        let (_, before) = vm_exec(
            ip,
            user,
            &[&format!(
                "bash -c 'export PATH=/usr/local/bin:$PATH && {} --version 2>/dev/null'",
                agent.cli_binary
            )],
        );
        println!(
            "[ai-vm-update] {} 업데이트 중... (현재: {before})",
            agent.name
        );

        let (ok, out) = vm_exec(
            ip,
            user,
            &[&format!(
                "bash -c 'export PATH=/usr/local/bin:$PATH && {sudo}npm install -g {}@latest'",
                agent.npm_package
            )],
        );
        if ok {
            let (_, after) = vm_exec(
                ip,
                user,
                &[&format!(
                    "bash -c 'export PATH=/usr/local/bin:$PATH && {} --version 2>/dev/null'",
                    agent.cli_binary
                )],
            );
            println!(
                "[ai-vm-update] {} 완료: {before} -> {after}",
                agent.name
            );
        } else {
            eprintln!("[ai-vm-update] {} 실패: {out}", agent.name);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn patch_claude_json_for_npm(path: &str) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    let Ok(mut json): Result<serde_json::Value, _> = serde_json::from_str(&content) else {
        return;
    };

    if let Some(obj) = json.as_object_mut() {
        obj.insert(
            "installMethod".into(),
            serde_json::Value::String("npm".into()),
        );
        obj.remove("autoUpdatesProtectedForNative");
        if let Some(features) = obj
            .get_mut("cachedGrowthBookFeatures")
            .and_then(|v| v.as_object_mut())
        {
            features.insert(
                "auto_migrate_to_native".into(),
                serde_json::Value::Bool(false),
            );
            features.insert(
                "tengu_native_installation".into(),
                serde_json::Value::Bool(false),
            );
        }
    }

    if let Ok(patched) = serde_json::to_string_pretty(&json) {
        let _ = fs::write(path, patched);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// New: octopus-install-all / superpowers-install-all / openclaw-wrap /
//      openclaw-llm-switch / openclaw-model-defaults
// ═══════════════════════════════════════════════════════════════════════════════

fn octopus_install_all(force: bool) {
    println!("=== claude-octopus 전체 LXC 일괄 설치 ===\n");

    let vmids = running_lxc_list();
    if vmids.is_empty() {
        eprintln!("[octopus] 실행 중인 LXC 없음");
        return;
    }
    println!("[octopus] 대상 LXC: {}개", vmids.len());

    let mut succeeded = 0;
    let mut skipped = 0;

    for (vmid, hostname) in &vmids {
        let has_claude = has_on(Some(vmid), "claude");
        let has_codex = has_on(Some(vmid), "codex");
        if !has_claude && !has_codex {
            println!("  - {vmid} ({hostname}): AI CLI 없음 -- 건너뜀");
            skipped += 1;
            continue;
        }
        let mut clis = Vec::new();
        if has_claude { clis.push("claude"); }
        if has_codex { clis.push("codex"); }
        println!("\n--- LXC {vmid} ({hostname}) [{}] ---", clis.join(","));

        if has_claude {
            let script = format!(
                "claude plugin marketplace add https://github.com/nyldn/claude-octopus.git 2>&1 | tail -1; \
                 claude plugin install octo@nyldn-plugins 2>&1 | tail -1"
            );
            match run_on(Some(vmid), &script) {
                Ok(_) => { println!("  ✓ claude"); succeeded += 1; }
                Err(e) => println!("  ✗ claude: {e}"),
            }
        }

        if has_codex {
            let rm = if force { "rm -rf ~/.codex/claude-octopus;" } else { "" };
            let script = format!(
                "{rm} if [ -d ~/.codex/claude-octopus ]; then \
                   cd ~/.codex/claude-octopus && git pull --ff-only 2>&1 | tail -1; \
                 else \
                   git clone --depth 1 https://github.com/nyldn/claude-octopus.git ~/.codex/claude-octopus 2>&1 | tail -1; \
                 fi && \
                 mkdir -p ~/.agents/skills && \
                 ln -sf ~/.codex/claude-octopus/skills ~/.agents/skills/claude-octopus"
            );
            match run_on(Some(vmid), &script) {
                Ok(_) => { println!("  ✓ codex"); succeeded += 1; }
                Err(e) => println!("  ✗ codex: {e}"),
            }
        }
    }

    println!("\n=== 결과: 설치 {succeeded}, 건너뜀 {skipped} ===");
}

fn superpowers_install_all(force: bool) {
    println!("=== superpowers 전체 LXC 일괄 설치 ===\n");

    let vmids = running_lxc_list();
    if vmids.is_empty() {
        eprintln!("[superpowers] 실행 중인 LXC 없음");
        return;
    }
    println!("[superpowers] 대상 LXC: {}개", vmids.len());

    let mut succeeded = 0;
    let mut skipped = 0;

    for (vmid, hostname) in &vmids {
        let has_claude = has_on(Some(vmid), "claude");
        let has_gemini = has_on(Some(vmid), "gemini");
        if !has_claude && !has_gemini {
            println!("  - {vmid} ({hostname}): 지원 CLI 없음 -- 건너뜀");
            skipped += 1;
            continue;
        }
        let mut clis = Vec::new();
        if has_claude { clis.push("claude"); }
        if has_gemini { clis.push("gemini"); }
        println!("\n--- LXC {vmid} ({hostname}) [{}] ---", clis.join(","));

        if has_claude {
            let _force_flag = force; // reserved for future use
            let script = "claude plugin install superpowers@claude-plugins-official 2>&1 | tail -1 || ( \
                            claude plugin marketplace add obra/superpowers-marketplace 2>&1 | tail -1 && \
                            claude plugin install superpowers@superpowers-marketplace 2>&1 | tail -1 \
                          )";
            match run_on(Some(vmid), script) {
                Ok(_) => { println!("  ✓ claude"); succeeded += 1; }
                Err(e) => println!("  ✗ claude: {e}"),
            }
        }

        if has_gemini {
            let script = "gemini extensions install https://github.com/obra/superpowers 2>&1 | tail -1";
            match run_on(Some(vmid), script) {
                Ok(_) => { println!("  ✓ gemini"); succeeded += 1; }
                Err(e) => println!("  ✗ gemini: {e}"),
            }
        }
    }

    println!("\n=== 결과: 설치 {succeeded}, 건너뜀 {skipped} ===");
}

fn openclaw_wrap(vmid_arg: Option<&str>) {
    let vmid = match vmid_arg {
        Some(v) => v.to_string(),
        None => {
            let list_out = cmd_output("pct", &["list"]);
            let mut found = String::new();
            for line in list_out.lines() {
                let cols: Vec<&str> = line.split_whitespace().collect();
                if cols.len() >= 3 && cols[2] == "openclaw" {
                    found = cols[0].to_string();
                    break;
                }
            }
            if found.is_empty() {
                eprintln!(
                    "[openclaw-wrap] hostname=openclaw 인 LXC를 찾을 수 없습니다. --vmid 옵션을 지정하세요."
                );
                std::process::exit(1);
            }
            found
        }
    };

    ensure_lxc_running(&vmid);

    let wrapper = format!(
        r#"#!/bin/bash
# Auto-generated by: prelik-ai openclaw-wrap
# 호스트에서 openclaw 명령 실행 시 LXC {vmid}로 자동 포워딩
VMID={vmid}

STATUS=$(pct status "$VMID" 2>/dev/null | awk '{{print $2}}')
if [ "$STATUS" != "running" ]; then
    echo "[openclaw] LXC $VMID 이 실행 중이 아닙니다 (현재: $STATUS)" >&2
    exit 1
fi

ARGS=""
for arg in "$@"; do
    ARGS="$ARGS $(printf '%q' "$arg")"
done

exec pct exec "$VMID" -- bash -c "export PATH=/usr/local/bin:\$PATH && openclaw $ARGS"
"#
    );

    let path = "/usr/local/bin/openclaw";

    if Path::new(path).exists() {
        let is_script = fs::read_to_string(path)
            .map(|c| c.starts_with("#!"))
            .unwrap_or(false);
        if !is_script {
            let backup = "/usr/local/bin/openclaw.real";
            if !Path::new(backup).exists() {
                fs::copy(path, backup).ok();
                println!("[openclaw-wrap] 기존 바이너리 백업: {backup}");
            }
        }
    }

    fs::write(path, &wrapper).ok();
    set_permissions(path, 0o755);
    println!("[openclaw-wrap] wrapper 설치 완료: {path} -> LXC {vmid}");
    println!("[openclaw-wrap] 이제 호스트에서 openclaw 명령이 자동으로 LXC {vmid} 안에서 실행됩니다.");
}

// ── LLM 프리셋 ──

struct LlmPreset {
    name: &'static str,
    description: &'static str,
    model_id: &'static str,
    thinking: &'static str,
    reasoning: &'static str,
    fast_mode: bool,
    fallback: Option<&'static str>,
}

const LLM_PRESETS: &[LlmPreset] = &[
    LlmPreset {
        name: "cloud-codex",
        description: "OpenAI Codex (GPT-5.4, 클라우드)",
        model_id: "openai-codex/gpt-5.4",
        thinking: "high",
        reasoning: "on",
        fast_mode: true,
        fallback: Some("anthropic/claude-opus-4-6"),
    },
    LlmPreset {
        name: "cloud-claude",
        description: "Anthropic Claude Opus 4.6 (클라우드)",
        model_id: "anthropic/claude-opus-4-6",
        thinking: "high",
        reasoning: "on",
        fast_mode: true,
        fallback: Some("openai-codex/gpt-5.4"),
    },
    LlmPreset {
        name: "local-gemma4",
        description: "Gemma 4 26B-A4B Q8_0 (ranode-3960x 로컬)",
        model_id: "local-llama/gemma-4-26B-A4B-it-Q8_0.gguf",
        thinking: "off",
        reasoning: "off",
        fast_mode: true,
        fallback: Some("openai-codex/gpt-5.4"),
    },
];

fn openclaw_llm_switch(vmid: Option<&str>, preset_name: &str, list: bool) {
    if list {
        println!("[openclaw] 사용 가능한 LLM 프리셋:\n");
        for p in LLM_PRESETS {
            println!("  {:<16} {} [{}]", p.name, p.description, p.model_id);
        }
        return;
    }

    if preset_name.is_empty() {
        eprintln!("[openclaw] --preset 필요. 사용 가능: {}", LLM_PRESETS.iter().map(|p| p.name).collect::<Vec<_>>().join(", "));
        eprintln!("  목록 보기: prelik run ai openclaw-llm-switch --list");
        std::process::exit(1);
    }

    let preset = match LLM_PRESETS.iter().find(|p| p.name == preset_name) {
        Some(p) => p,
        None => {
            eprintln!("[openclaw] 알 수 없는 프리셋: {preset_name}");
            eprintln!("사용 가능: {}", LLM_PRESETS.iter().map(|p| p.name).collect::<Vec<_>>().join(", "));
            std::process::exit(1);
        }
    };

    println!("[openclaw] LLM 프리셋 전환: {}", preset.name);
    println!("  설명: {}", preset.description);
    println!("  model: {}", preset.model_id);
    println!("  thinking: {}, reasoning: {}, fast: {}", preset.thinking, preset.reasoning, preset.fast_mode);
    if let Some(fb) = preset.fallback {
        println!("  fallback: {fb}");
    }
    println!();

    // Apply model defaults to host openclaw.json
    let config_path = format!("{SOURCE_HOME}/.openclaw/openclaw.json");
    match apply_model_defaults_to_file(
        &config_path, preset.model_id, preset.fallback, preset.thinking, preset.reasoning, preset.fast_mode,
    ) {
        Ok(()) => println!("[openclaw] 호스트 프리셋 적용 완료"),
        Err(e) => {
            eprintln!("[openclaw] 호스트 프리셋 적용 실패: {e}");
            std::process::exit(1);
        }
    }

    if let Some(vmid) = vmid {
        // Apply in LXC via pct exec
        println!("\n[openclaw] LXC {vmid} 프리셋 적용 중...");
        apply_model_defaults_in_lxc(
            vmid, preset.model_id, preset.fallback, preset.thinking, preset.reasoning, preset.fast_mode,
        );
        // Restart gateway
        println!("[openclaw] gateway 재시작...");
        openclaw_gateway(vmid, "start");
    }
}

fn openclaw_model_defaults(
    vmid: Option<&str>,
    model: &str,
    fallback: Option<&str>,
    thinking: &str,
    reasoning: &str,
    fast_mode: bool,
) {
    if let Some(vmid) = vmid {
        apply_model_defaults_in_lxc(vmid, model, fallback, thinking, reasoning, fast_mode);
        println!("[openclaw] LXC {vmid} 기본 정책 적용 완료");
        println!("  model: {model}");
        println!("  fallback: {}", fallback.unwrap_or("(none)"));
        println!("  thinking: {thinking}, reasoning: {reasoning}, fast: {fast_mode}");
    } else {
        let config_path = format!("{SOURCE_HOME}/.openclaw/openclaw.json");
        match apply_model_defaults_to_file(
            &config_path, model, fallback, thinking, reasoning, fast_mode,
        ) {
            Ok(()) => {
                println!("[openclaw] 기본 정책 적용 완료");
                println!("  config: {config_path}");
                println!("  model: {model}");
                println!("  fallback: {}", fallback.unwrap_or("(none)"));
                println!("  thinking: {thinking}, reasoning: {reasoning}, fast: {fast_mode}");
            }
            Err(e) => {
                eprintln!("[openclaw] 기본 정책 적용 실패: {e}");
                std::process::exit(1);
            }
        }
    }
}

/// Apply model defaults to a local openclaw.json file
fn apply_model_defaults_to_file(
    config_path: &str,
    model: &str,
    fallback: Option<&str>,
    thinking: &str,
    reasoning: &str,
    fast_mode: bool,
) -> anyhow::Result<()> {
    if let Some(parent) = Path::new(config_path).parent() {
        fs::create_dir_all(parent)?;
    }

    let mut cfg: serde_json::Value = fs::read_to_string(config_path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let obj = cfg.as_object_mut().ok_or_else(|| anyhow::anyhow!("config 루트가 object가 아님"))?;
    obj.insert("defaultModel".into(), serde_json::json!(model));
    obj.insert("defaultThinking".into(), serde_json::json!(thinking));
    obj.insert("defaultReasoning".into(), serde_json::json!(reasoning));
    obj.insert("defaultFastMode".into(), serde_json::json!(fast_mode));
    if let Some(fb) = fallback {
        obj.insert("defaultFallback".into(), serde_json::json!(fb));
    } else {
        obj.remove("defaultFallback");
    }

    let pretty = serde_json::to_string_pretty(&cfg)?;
    fs::write(config_path, format!("{pretty}\n"))?;
    Ok(())
}

/// Apply model defaults inside an LXC via pct exec + node one-liner
fn apply_model_defaults_in_lxc(
    vmid: &str,
    model: &str,
    fallback: Option<&str>,
    thinking: &str,
    reasoning: &str,
    fast_mode: bool,
) {
    ensure_lxc_running(vmid);

    let fb_json = match fallback {
        Some(fb) => format!("\"{}\"", fb),
        None => "null".to_string(),
    };
    let script = format!(
        r#"
const fs = require("fs");
const p = "/root/.openclaw/openclaw.json";
let cfg = {{}};
try {{ cfg = JSON.parse(fs.readFileSync(p, "utf8")); }} catch {{}}
cfg.defaultModel = "{model}";
cfg.defaultThinking = "{thinking}";
cfg.defaultReasoning = "{reasoning}";
cfg.defaultFastMode = {fast_mode};
const fb = {fb_json};
if (fb) cfg.defaultFallback = fb; else delete cfg.defaultFallback;
fs.writeFileSync(p, JSON.stringify(cfg, null, 2) + "\n");
console.log("ok");
"#
    );

    let (ok, out) = lxc_exec(vmid, &["node", "-e", &script]);
    if !ok {
        eprintln!("[openclaw] LXC {vmid} 모델 기본값 적용 실패: {out}");
    }
}
