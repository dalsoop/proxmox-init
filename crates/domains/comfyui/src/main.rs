//! pxi-comfyui — ComfyUI LXC 설치 관리.
//! GPU 패스스루 + ComfyUI 클론 + Python 의존성 + systemd.
//! phs의 dalsoop-specific 워크플로우는 제외, 설치 골격만.

use clap::{Parser, Subcommand};
use pxi_core::common;

#[derive(Parser)]
#[command(name = "pxi-comfyui", about = "ComfyUI LXC 설치 관리")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// GPU 패스스루만 설정 (LXC 설정 + NVIDIA device)
    GpuPassthrough {
        #[arg(long)]
        vmid: String,
        /// GPU 인덱스 (예: 0 또는 0,1)
        #[arg(long, default_value = "0")]
        gpu: String,
    },
    /// ComfyUI 설치 (LXC 내부에 clone + 의존성 + systemd)
    Install {
        #[arg(long)]
        vmid: String,
        /// 설치 경로 (LXC 내부)
        #[arg(long, default_value = "/opt/ComfyUI")]
        path: String,
        /// 리스닝 포트
        #[arg(long, default_value = "8188")]
        port: String,
    },
    /// 상태 조회 (systemd + 포트)
    Status {
        #[arg(long)]
        vmid: String,
    },
    Doctor,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if !matches!(cli.cmd, Cmd::Doctor) && !common::has_cmd("pct") {
        anyhow::bail!("pct 없음 — Proxmox 호스트에서만 동작");
    }
    match cli.cmd {
        Cmd::GpuPassthrough { vmid, gpu } => gpu_passthrough(&vmid, &gpu),
        Cmd::Install { vmid, path, port } => install(&vmid, &path, &port),
        Cmd::Status { vmid } => status(&vmid),
        Cmd::Doctor => {
            doctor();
            Ok(())
        }
    }
}

fn gpu_passthrough(vmid: &str, gpu: &str) -> anyhow::Result<()> {
    println!("=== GPU 패스스루: LXC {vmid}, GPU {gpu} ===");

    let config_path = format!("/etc/pve/lxc/{vmid}.conf");
    if !std::path::Path::new(&config_path).exists() {
        anyhow::bail!("LXC {vmid} config 없음 — LXC 먼저 생성");
    }

    // NVIDIA 디바이스 파일 경로
    let gpus: Vec<&str> = gpu.split(',').collect();
    let mut lines_to_add = vec!["lxc.cgroup2.devices.allow: c 195:* rwm".to_string()];

    for g in &gpus {
        lines_to_add.push(format!("dev{}: /dev/nvidia{}", g, g));
    }
    lines_to_add.push("dev-nvidiactl: /dev/nvidiactl".to_string());
    lines_to_add.push("dev-uvm: /dev/nvidia-uvm".to_string());
    lines_to_add.push("dev-uvm-tools: /dev/nvidia-uvm-tools".to_string());

    // 기존 설정 확인 후 추가 (멱등성)
    for line in &lines_to_add {
        let check = std::process::Command::new("sudo")
            .args(["grep", "-qF", line, &config_path])
            .status();
        if check.ok().map(|s| s.success()).unwrap_or(false) {
            continue;
        }
        // append
        let cmd = format!("echo '{}' | sudo tee -a {} >/dev/null", line, config_path);
        common::run_bash(&cmd)?;
    }
    println!("✓ {} 업데이트", config_path);
    println!("  재시작 필요: pct restart {vmid}");
    Ok(())
}

fn install(vmid: &str, path: &str, port: &str) -> anyhow::Result<()> {
    println!("=== ComfyUI 설치: LXC {vmid} → {path} (포트 {port}) ===");

    // 의존성
    common::run(
        "pct",
        &[
            "exec",
            vmid,
            "--",
            "bash",
            "-c",
            "apt-get update && apt-get install -y python3 python3-venv python3-pip git",
        ],
    )?;

    // 클론 (이미 있으면 pull)
    let clone_cmd = format!(
        "if [ -d {path} ]; then cd {path} && git pull --ff-only; \
         else git clone https://github.com/comfyanonymous/ComfyUI {path}; fi"
    );
    common::run("pct", &["exec", vmid, "--", "bash", "-c", &clone_cmd])?;
    println!("  ✓ ComfyUI 클론");

    // venv + 의존성 (requirements.txt)
    let venv_cmd = format!(
        "cd {path} && python3 -m venv venv && \
         venv/bin/pip install --upgrade pip && \
         venv/bin/pip install -r requirements.txt"
    );
    common::run("pct", &["exec", vmid, "--", "bash", "-c", &venv_cmd])?;
    println!("  ✓ Python 의존성");

    // systemd unit
    let unit = format!(
        "[Unit]
Description=ComfyUI (pxi)
After=network-online.target

[Service]
Type=simple
WorkingDirectory={path}
ExecStart={path}/venv/bin/python main.py --listen 0.0.0.0 --port {port}
Restart=on-failure
RestartSec=10

[Install]
WantedBy=multi-user.target
"
    );

    // tempfile로 로컬 작성 후 pct push
    let (tmp, _g) = secure_tempfile()?;
    std::fs::write(&tmp, unit)?;
    common::run(
        "pct",
        &["push", vmid, &tmp, "/etc/systemd/system/comfyui.service"],
    )?;
    common::run("pct", &["exec", vmid, "--", "systemctl", "daemon-reload"])?;
    common::run(
        "pct",
        &[
            "exec",
            vmid,
            "--",
            "systemctl",
            "enable",
            "--now",
            "comfyui",
        ],
    )?;

    println!("\n✓ 설치 + 시작 완료");
    println!("  Web UI: http://<LXC_IP>:{port}");
    println!("  모델: {path}/models/checkpoints (직접 다운로드 또는 Manager)");
    Ok(())
}

fn status(vmid: &str) -> anyhow::Result<()> {
    println!("=== ComfyUI 상태: LXC {vmid} ===");
    let out = common::run(
        "pct",
        &["exec", vmid, "--", "systemctl", "is-active", "comfyui"],
    );
    match out {
        Ok(s) => println!("  service: {}", s.trim()),
        Err(_) => println!("  service: ✗ (미설치)"),
    }
    if let Ok(ports) = common::run(
        "pct",
        &[
            "exec",
            vmid,
            "--",
            "bash",
            "-c",
            "ss -tlnp 2>/dev/null | grep :8188 || echo '(8188 리스닝 없음)'",
        ],
    ) {
        println!("  포트:    {}", ports.trim());
    }
    Ok(())
}

fn secure_tempfile() -> anyhow::Result<(String, TempGuard)> {
    let out = common::run("mktemp", &["-t", "pxi.XXXXXXXX"])?;
    let tmp = out.trim().to_string();
    let guard = TempGuard(tmp.clone());
    common::run("chmod", &["600", &tmp])?;
    Ok((tmp, guard))
}

struct TempGuard(String);
impl Drop for TempGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn doctor() {
    println!("=== pxi-comfyui doctor ===");
    println!(
        "  pct:       {}",
        if common::has_cmd("pct") { "✓" } else { "✗" }
    );
    // NVIDIA 드라이버 호스트 확인
    if std::path::Path::new("/dev/nvidia0").exists() {
        println!("  /dev/nvidia0: ✓");
    } else {
        println!("  /dev/nvidia0: ✗ (NVIDIA 드라이버 미설치?)");
    }
    if common::run("nvidia-smi", &["-L"]).is_ok() {
        if let Ok(out) = common::run("nvidia-smi", &["-L"]) {
            println!("  GPUs:");
            for line in out.lines() {
                println!("    {line}");
            }
        }
    } else {
        println!("  nvidia-smi: ✗");
    }
}
