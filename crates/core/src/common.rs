//! 공통 헬퍼
use std::process::Command;

pub fn run(cmd: &str, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new(cmd).args(args).output()?;
    if !output.status.success() {
        anyhow::bail!(
            "{} {:?} 실패: {}",
            cmd, args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn run_bash(script: &str) -> anyhow::Result<String> {
    run("bash", &["-c", script])
}

pub fn has_cmd(name: &str) -> bool {
    which::which(name).is_ok()
}
