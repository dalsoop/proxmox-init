use anyhow::{Context, Result};
use std::process::Command;

/// 외부 명령 실행 — stdout 캡처. **기본 호출 시맨틱** (레거시 229 호출처 전부가
/// capture 를 기대). 반환값을 무시하면 interactive 대신 silent 가 됨 — 진행상태를
/// 사용자에게 보여야 하는 경우 `run_passthrough` 사용.
pub fn run(cmd: &str, args: &[&str]) -> Result<String> {
    run_capture(cmd, args)
}

/// stdout/stderr 상속 — 진행상태를 사용자에게 실시간 보여줘야 할 때.
/// (예: `qm start`, `pct enter`). 반환값 없음.
pub fn run_passthrough(cmd: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .with_context(|| format!("{cmd} 실행 실패"))?;
    if !status.success() {
        anyhow::bail!("{cmd} exited with {}", status);
    }
    Ok(())
}

/// 외부 명령 실행 — stdout 캡처. `run` 의 명시적 별칭 (레거시 호환).
pub fn run_capture(cmd: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("{cmd} 실행 실패"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{cmd} 실패: {stderr}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// 명령 존재 여부 확인
pub fn command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `command_exists`의 호환 alias — 레거시 코드 대응
#[inline]
pub fn has_cmd(cmd: &str) -> bool {
    command_exists(cmd)
}

/// bash -lc 스크립트 실행 + stdout 캡처
pub fn run_bash(script: &str) -> Result<String> {
    run_capture("bash", &["-lc", script])
}

/// pct exec 래퍼 — LXC 안에서 명령 실행 (stdout 캡처)
pub fn pct_exec(vmid: &str, cmd_args: &[&str]) -> Result<String> {
    let mut args = vec!["exec", vmid, "--"];
    args.extend_from_slice(cmd_args);
    run_capture("pct", &args)
}

/// pct exec 래퍼 — stdout/stderr 상속 (interactive)
pub fn pct_exec_passthrough(vmid: &str, cmd_args: &[&str]) -> Result<()> {
    let mut args = vec!["exec", vmid, "--"];
    args.extend_from_slice(cmd_args);
    run_passthrough("pct", &args)
}

/// LXC 실행 상태 확인 + 미실행 시 시작
pub fn ensure_lxc_running(vmid: &str) -> Result<()> {
    let status = run_capture("pct", &["status", vmid])?;
    if !status.contains("running") {
        run_passthrough("pct", &["start", vmid])?;
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    Ok(())
}

/// `run_capture`의 간편 별칭 — 레거시 코드가 `common::run(...).trim()` 형태로 사용하던 패턴 복원
#[inline]
pub fn run_str(cmd: &str, args: &[&str]) -> Result<String> {
    run_capture(cmd, args)
}

/// 자격증명·시크릿을 argv 로 넘기는 명령 전용. 실패 시 argv 를 에러 메시지에
/// 포함하지 않음 (유출 방지). stdout 은 캡처하지 않음.
pub fn run_secret(cmd: &str, args: &[&str], context: &str) -> Result<()> {
    let status = Command::new(cmd)
        .args(args)
        .status()
        .map_err(|e| anyhow::anyhow!("{cmd} spawn 실패: {e}"))?;
    if !status.success() {
        anyhow::bail!(
            "{context} 실패 (exit {}). 자격증명 보호를 위해 argv 는 메시지에 포함하지 않음.",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}
