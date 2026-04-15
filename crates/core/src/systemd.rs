//! systemd unit 생성/제어 헬퍼.

use crate::common;
use std::fs;
use std::path::PathBuf;

pub fn unit_path(name: &str) -> PathBuf {
    if crate::paths::is_root() {
        PathBuf::from(format!("/etc/systemd/system/{name}.service"))
    } else {
        dirs::config_dir()
            .unwrap_or_default()
            .join("systemd/user")
            .join(format!("{name}.service"))
    }
}

pub fn write_unit(name: &str, content: &str) -> anyhow::Result<()> {
    let path = unit_path(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    daemon_reload()?;
    Ok(())
}

pub fn daemon_reload() -> anyhow::Result<()> {
    let user_flag = if crate::paths::is_root() { "" } else { "--user" };
    let args: Vec<&str> = if user_flag.is_empty() {
        vec!["daemon-reload"]
    } else {
        vec!["--user", "daemon-reload"]
    };
    common::run("systemctl", &args)?;
    Ok(())
}

pub fn enable_now(name: &str) -> anyhow::Result<()> {
    let user_flag = if crate::paths::is_root() { "" } else { "--user" };
    let service = format!("{name}.service");
    let args: Vec<&str> = if user_flag.is_empty() {
        vec!["enable", "--now", &service]
    } else {
        vec!["--user", "enable", "--now", &service]
    };
    common::run("systemctl", &args)?;
    Ok(())
}
