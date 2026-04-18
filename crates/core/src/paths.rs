//! 경로 규약
//! - root: /etc/pxi, /var/lib/pxi, /usr/local/bin
//! - user: $XDG_CONFIG_HOME 또는 $HOME/.config/pxi (절대경로 필수)
//!
//! HOME/XDG 환경변수가 비어있으면 상대경로로 fallback 금지 — 명시적 에러.

use anyhow::{anyhow, Result};
use std::path::PathBuf;

pub fn config_dir() -> Result<PathBuf> {
    if is_root() {
        Ok(PathBuf::from("/etc/pxi"))
    } else {
        dirs::config_dir()
            .map(|p| p.join("pxi"))
            .ok_or_else(|| anyhow!("XDG_CONFIG_HOME/HOME 미설정 — sudo -E 또는 HOME 지정 필요"))
    }
}

pub fn data_dir() -> Result<PathBuf> {
    if is_root() {
        Ok(PathBuf::from("/var/lib/pxi"))
    } else {
        dirs::data_dir()
            .map(|p| p.join("pxi"))
            .ok_or_else(|| anyhow!("XDG_DATA_HOME/HOME 미설정"))
    }
}

pub fn bin_dir() -> Result<PathBuf> {
    if is_root() {
        Ok(PathBuf::from("/usr/local/bin"))
    } else {
        dirs::home_dir()
            .map(|h| h.join(".local/bin"))
            .ok_or_else(|| anyhow!("HOME 미설정"))
    }
}

pub fn domains_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("domains"))
}

pub fn env_file() -> Result<PathBuf> {
    Ok(config_dir()?.join(".env"))
}

pub fn env_vault() -> Result<PathBuf> {
    Ok(config_dir()?.join(".env.vault"))
}

pub fn env_keys() -> Result<PathBuf> {
    Ok(config_dir()?.join(".env.keys"))
}

pub fn is_root() -> bool {
    unsafe { geteuid() == 0 }
}

extern "C" {
    fn geteuid() -> u32;
}
