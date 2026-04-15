//! systemd unit 생성/제어 헬퍼.

use crate::{common, paths};
use std::fs;
use std::path::PathBuf;

pub fn unit_path(name: &str) -> anyhow::Result<PathBuf> {
    if paths::is_root() {
        Ok(PathBuf::from(format!("/etc/systemd/system/{name}.service")))
    } else {
        let base = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("HOME 미설정"))?
            .join("systemd/user");
        Ok(base.join(format!("{name}.service")))
    }
}

pub fn write_unit(name: &str, content: &str) -> anyhow::Result<()> {
    let path = unit_path(name)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    daemon_reload()?;
    Ok(())
}

pub fn daemon_reload() -> anyhow::Result<()> {
    let args: &[&str] = if paths::is_root() {
        &["daemon-reload"]
    } else {
        &["--user", "daemon-reload"]
    };
    common::run("systemctl", args)?;
    Ok(())
}

pub fn enable_now(name: &str) -> anyhow::Result<()> {
    let service = format!("{name}.service");
    let args: Vec<&str> = if paths::is_root() {
        vec!["enable", "--now", &service]
    } else {
        vec!["--user", "enable", "--now", &service]
    };
    common::run("systemctl", &args)?;
    Ok(())
}
