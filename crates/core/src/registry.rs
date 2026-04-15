//! 도메인 레지스트리 로더.
//! nickel CLI가 PATH에 있으면 `ncl/domains.ncl`을 export 해서 사용.
//! 없으면 컴파일 타임에 내장된 폴백 목록 사용.

use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
pub struct Registry {
    pub domains: BTreeMap<String, Domain>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Domain {
    pub name: String,
    pub description: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub tags: Tags,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub provides: Vec<String>,
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

/// 컴파일 타임에 포함된 ncl 원본 (폴백 export용)
const DOMAINS_NCL: &str = include_str!("../../../ncl/domains.ncl");

impl Registry {
    pub fn load() -> anyhow::Result<Self> {
        // 1) nickel CLI가 있으면 내장 ncl을 JSON으로 export
        if crate::common::has_cmd("nickel") {
            return Self::load_via_nickel();
        }
        // 2) 폴백: 최소 하드코드 (nickel 없어도 동작 보장)
        Ok(Self::fallback())
    }

    fn load_via_nickel() -> anyhow::Result<Self> {
        let tmp = std::env::temp_dir().join("prelik-domains.ncl");
        std::fs::write(&tmp, DOMAINS_NCL)?;
        let json = crate::common::run(
            "nickel",
            &["export", "--format", "json", &tmp.display().to_string()],
        )?;
        let reg: Registry = serde_json::from_str(&json)
            .map_err(|e| anyhow::anyhow!("Nickel export JSON 파싱 실패: {e}"))?;
        Ok(reg)
    }

    fn fallback() -> Self {
        // nickel CLI 없어도 prelik available이 동작하도록 최소 세트
        let mut domains = BTreeMap::new();
        for (name, desc, enabled) in [
            ("bootstrap", "apt/rust/gh/dotenvx 의존성 설치", true),
            ("connect", "외부 서비스 연결 관리 (.env + dotenvx)", true),
            ("manager", "도메인 설치/업데이트 매니저", true),
            ("lxc", "LXC 수명 관리 (Proxmox pct 래퍼)", true),
            ("traefik", "Traefik 리버스 프록시", true),
            ("mail", "Maddy + Mailpit + Postfix relay 번들", true),
            ("cloudflare", "CF DNS/Email Routing/Worker", true),
            ("ai", "Claude/Codex + 플러그인", false),
        ] {
            domains.insert(
                name.to_string(),
                Domain {
                    name: name.into(),
                    description: desc.into(),
                    enabled,
                    tags: Tags::default(),
                    requires: vec![],
                    provides: vec![],
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
