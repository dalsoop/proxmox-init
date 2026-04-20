//! 도메인 레지스트리 — ncl/domains.ncl → locale.json → runtime 2-tier.
//!
//! 로드 우선순위:
//!   1) paths::locale_json() — install-local.sh 또는 release tarball 이 설치한 JSON SSOT
//!   2) 컴파일 타임 embedded fallback — locale.json 없는 fresh 환경 (emergency)
//!
//! Runtime 에 nickel CLI 의존성 없음. ncl/domains.ncl 의 실시간 반영이 필요하면
//! `scripts/install-local.sh` 로 재생성.

use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct Registry {
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
        // Tier 1: locale.json (install-local.sh / release 가 배치)
        if let Ok(path) = crate::paths::locale_json() {
            if path.exists() {
                let raw = std::fs::read_to_string(&path)
                    .map_err(|e| anyhow::anyhow!("{} 읽기 실패: {e}", path.display()))?;
                let domains: BTreeMap<String, Domain> = serde_json::from_str(&raw)
                    .map_err(|e| anyhow::anyhow!("{} JSON 파싱 실패: {e}", path.display()))?;
                return Ok(Registry { domains });
            }
        }
        // Tier 2: fallback (locale.json 없는 fresh 환경 대응).
        // SSOT 아님 — install-local.sh / release 가 locale.json 을 덮어쓰면 이 경로 탈출.
        Ok(Self::fallback())
    }

    fn fallback() -> Self {
        let mut domains = BTreeMap::new();
        let seed: &[(&str, &str)] = &[
            ("bootstrap",   "의존성 설치 (apt/rust/gh/dotenvx/nickel)"),
            ("lxc",         "LXC 수명 관리 (Proxmox pct 래퍼)"),
            ("traefik",     "Traefik 리버스 프록시"),
            ("cloudflare",  "Cloudflare DNS + Email Routing + SSL"),
            ("ai",          "Claude/Codex CLI + 플러그인"),
            ("mail",        "Maddy + Mailpit + Postfix relay"),
            ("connect",     "외부 서비스 연결 관리 (.env + dotenvx)"),
            ("service",     "/opt/services/ 레지스트리"),
        ];
        for (name, desc) in seed {
            domains.insert(
                name.to_string(),
                Domain {
                    name: (*name).into(),
                    description: (*desc).into(),
                    tags: Tags::default(),
                    requires: vec![],
                    provides: vec![],
                    enabled: true,
                },
            );
        }
        Registry { domains }
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
