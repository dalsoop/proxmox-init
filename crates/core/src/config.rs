//! 런타임 설정 로더. ~/.config/pxi/config.toml 또는 /etc/pxi/config.toml.

use crate::paths;
use serde::{Deserialize, Deserializer, Serialize};
use std::fs;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub proxmox: ProxmoxConfig,
    #[serde(default)]
    pub network: NetworkConfig,
    #[serde(default)]
    pub lxc: LxcConfig,
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

/// LXC 생성 기본값. pxi-lxc create 가 default_value 하드코딩 대신 여기서 로드.
/// 도메인별 override 가 필요한 쪽 (xdesktop 무거움, wordpress MariaDB 등) 은 자기 기본값 유지.
///
/// `cores`/`memory`/`disk` 는 pct CLI 가 문자열을 기대해서 String 으로 저장하지만,
/// 관리자가 TOML 에 `cores = 2` (unquoted int) 로 쓸 수 있게 `deserialize_with` 로 int→String 허용 (codex #41 P2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LxcConfig {
    #[serde(default = "default_cores", deserialize_with = "de_str_or_int")]
    pub cores: String,
    #[serde(default = "default_memory", deserialize_with = "de_str_or_int")]
    pub memory: String,
    #[serde(default = "default_disk", deserialize_with = "de_str_or_int")]
    pub disk: String,
    #[serde(default = "default_template")]
    pub template: String,
    #[serde(default = "default_storage")]
    pub storage: String,
    #[serde(default = "default_bridge")]
    pub bridge: String,
}

impl Default for LxcConfig {
    fn default() -> Self {
        Self {
            cores: default_cores(),
            memory: default_memory(),
            disk: default_disk(),
            template: default_template(),
            storage: default_storage(),
            bridge: default_bridge(),
        }
    }
}

fn default_cores() -> String { "2".into() }
fn default_memory() -> String { "2048".into() }
fn default_disk() -> String { "8".into() }
fn default_template() -> String { "debian-13".into() }
fn default_storage() -> String { "local-lvm".into() }
fn default_bridge() -> String { "vmbr1".into() }

/// TOML scalar 가 int 이든 string 이든 String 으로 흡수.
/// `cores = 2` / `cores = "2"` 둘 다 허용.
fn de_str_or_int<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    use serde::de;
    struct V;
    impl<'de> de::Visitor<'de> for V {
        type Value = String;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("integer or string")
        }
        fn visit_str<E: de::Error>(self, s: &str) -> Result<String, E> { Ok(s.to_string()) }
        fn visit_string<E: de::Error>(self, s: String) -> Result<String, E> { Ok(s) }
        fn visit_i64<E: de::Error>(self, n: i64) -> Result<String, E> { Ok(n.to_string()) }
        fn visit_u64<E: de::Error>(self, n: u64) -> Result<String, E> { Ok(n.to_string()) }
    }
    d.deserialize_any(V)
}

impl Config {
    /// 로드 규약 (services 레지스트리와 동일):
    ///   - 파일 없음 → Self::default() (fresh install 안전망)
    ///   - 파일 존재하지만 읽기/파싱 실패 → **bail** (관리자 override 무시되는 silent fallback 방지)
    pub fn load() -> anyhow::Result<Self> {
        let path = paths::config_dir()?.join("config.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("{} 읽기 실패: {e}", path.display()))?;
        let cfg = toml::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("{} TOML 파싱 실패: {e}", path.display()))?;
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lxc_accepts_int_or_string() {
        // unquoted int
        let c: Config = toml::from_str("[lxc]\ncores = 4\nmemory = 8192\ndisk = 32\n").unwrap();
        assert_eq!(c.lxc.cores, "4");
        assert_eq!(c.lxc.memory, "8192");
        assert_eq!(c.lxc.disk, "32");

        // quoted string
        let c: Config = toml::from_str("[lxc]\ncores = \"4\"\nmemory = \"8192\"\ndisk = \"32\"\n").unwrap();
        assert_eq!(c.lxc.cores, "4");
    }

    #[test]
    fn lxc_omitted_uses_defaults() {
        let c: Config = toml::from_str("").unwrap();
        assert_eq!(c.lxc.cores, "2");
        assert_eq!(c.lxc.memory, "2048");
        assert_eq!(c.lxc.disk, "8");
        assert_eq!(c.lxc.template, "debian-13");
    }
}
