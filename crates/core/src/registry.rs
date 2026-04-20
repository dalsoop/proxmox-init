//! 도메인 레지스트리 — ncl/domains.ncl → locale.json → runtime.
//!
//! locale.json 이 유일한 SSOT 소스. 없거나 format_version 미지원이면 **hard-fail**
//! (codex #47 P1): silent degrade 방지. fresh clone 이면 `scripts/install-local.sh`
//! 를 먼저 실행해 locale.json 을 생성.
//!
//! Runtime 에 nickel CLI 의존성 없음.

use serde::Deserialize;
use std::collections::BTreeMap;

/// Registry 가 이해하는 locale.json 포맷 버전. 여기를 벗어나면 load() 가 hard-fail.
/// 새 버전 추가 시 `match` 암(arm) 로 graceful migration 작성.
const SUPPORTED_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
pub struct Registry {
    pub format_version: u32,
    pub domains: BTreeMap<String, Domain>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Domain {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Tags,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub provides: Vec<String>,
    /// SSOT contract 에는 없음. runtime 이 "미구현" 구분에만 사용.
    /// locale.json 에 `"enabled": false` 를 실으면 `planned()` 쪽으로 분류.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Tags {
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default)]
    pub layer: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
}

fn default_true() -> bool { true }

impl Registry {
    pub fn load() -> anyhow::Result<Self> {
        let path = crate::paths::locale_json()?;
        if !path.exists() {
            anyhow::bail!(
                "locale.json 이 없음 ({}). fresh clone 이면 다음을 먼저 실행:\n  \
                 scripts/install-local.sh\n\
                 릴리스 tarball 사용 시 install.sh 가 자동 배치해야 함 — 누락이면 버그.",
                path.display()
            );
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("{} 읽기 실패: {e}", path.display()))?;
        let reg: Registry = serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("{} JSON 파싱 실패: {e}", path.display()))?;
        if reg.format_version != SUPPORTED_FORMAT_VERSION {
            anyhow::bail!(
                "{} format_version={} 은 runtime 이 지원하지 않음 (supported={}). \
                 Nickel SSOT 업그레이드 또는 pxi 바이너리 업그레이드 필요.",
                path.display(),
                reg.format_version,
                SUPPORTED_FORMAT_VERSION
            );
        }
        Ok(reg)
    }

    pub fn available(&self) -> Vec<&Domain> {
        let mut list: Vec<&Domain> = self.domains.values().filter(|d| d.enabled).collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    pub fn planned(&self) -> Vec<&Domain> {
        let mut list: Vec<&Domain> = self.domains.values().filter(|d| !d.enabled).collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }
}

// 레거시 호환 — 기존 호출부(`known_domains`, `binary_name`) 유지.
// 새 코드는 Registry::load() 를 우선 사용.
pub fn known_domains() -> Vec<(&'static str, &'static str)> {
    vec![
        ("code-server", "code-server (VS Code 웹) 설치/제거"),
        ("wordpress", "WordPress LXC 설치/설정/관리"),
    ]
}

pub fn binary_name(domain: &str) -> String {
    crate::brand::domain_bin(domain)
}
