//! 런타임 설정 로더. ~/.config/prelik/config.toml 또는 /etc/prelik/config.toml.

use crate::paths;
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub proxmox: ProxmoxConfig,
    #[serde(default)]
    pub network: NetworkConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxmoxConfig {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub node: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkConfig {
    #[serde(default)]
    pub bridge: String,
    #[serde(default)]
    pub gateway: String,
    #[serde(default = "default_subnet")]
    pub subnet: u8,
}

fn default_subnet() -> u8 { 16 }

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let path = paths::config_dir()?.join("config.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path)?;
        let cfg = toml::from_str(&raw)?;
        Ok(cfg)
    }
}
