//! 도메인 레지스트리 — ncl/domains.ncl → locale.json → runtime.
//!
//! 로드 tier (우선순위):
//!   1) `paths::locale_json()` 파일시스템 — install-local.sh / release tarball 이 배치
//!   2) 빌드 시 embed 된 locale.json — build.rs 가 `nickel export` 로 생성
//!   3) hard-fail
//!
//! Tier 1 은 사용자가 수정할 수 있는 경로, tier 2 는 바이너리에 구워진 보증.
//! 둘 다 동일한 `SUPPORTED_FORMAT_VERSION` 검사 대상.
//!
//! Runtime 에 nickel CLI 의존성 없음.

use serde::Deserialize;
use std::collections::BTreeMap;

/// Registry 가 이해하는 locale.json 포맷 버전. 여기를 벗어나면 load() 가 hard-fail.
/// 새 버전 추가 시 `match` 암(arm) 로 graceful migration 작성.
const SUPPORTED_FORMAT_VERSION: u32 = 1;

/// build.rs 가 `nickel export ncl/domains.ncl` 결과를 OUT_DIR/locale.json 으로 기록.
/// nickel 미설치 빌드 환경에서는 empty JSON(`{}`) 이 기록되고, `from_str` 파싱이
/// format_version 검사에서 실패 → tier 3 hard-fail.
const EMBEDDED_LOCALE_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/locale.json"));

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

fn default_true() -> bool {
    true
}

impl Registry {
    pub fn load() -> anyhow::Result<Self> {
        Self::load_from_sources(
            crate::paths::locale_json().ok().filter(|p| p.exists()),
            EMBEDDED_LOCALE_JSON,
        )
    }

    /// 실질 로드 로직 — 테스트에서 path/embedded 직접 주입하기 위한 내부 함수.
    fn load_from_sources(
        fs_path: Option<std::path::PathBuf>,
        embedded: &str,
    ) -> anyhow::Result<Self> {
        // Tier 1: 파일시스템 locale.json. 있으면 여기서 hard-fail 여부 결정 —
        // 파싱·버전 실패가 tier 2 로 silent downgrade 되지 않게 (codex #47 P1).
        if let Some(path) = fs_path {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| anyhow::anyhow!("{} 읽기 실패: {e}", path.display()))?;
            return Self::parse_with_version(&raw, &path.display().to_string()).map_err(|e| {
                anyhow::anyhow!(
                    "{e}\n\
                     복구: locale.json 재생성이 필요하면 'scripts/install-local.sh'. \
                     tier-1 invalid 는 embedded fallback 을 자동 우회하지 않음."
                )
            });
        }
        // Tier 2: 바이너리 embed. 실패 원인을 tier-3 에러에 포함해 디버그 가능 (codex #53 P2).
        match Self::parse_with_version(embedded, "<embedded>") {
            Ok(reg) => Ok(reg),
            Err(embed_err) => anyhow::bail!(
                "locale.json 없음 (fs tier). embedded tier 도 로드 실패:\n  {embed_err}\n\
                 복구:\n  \
                 - fresh clone: 'scripts/install-local.sh' 실행 (nickel 필요)\n  \
                 - release tarball: install.sh 가 자동 배치해야 함 — 누락이면 버그\n  \
                 - 소스 빌드: nickel 설치 후 'cargo build' 재실행"
            ),
        }
    }

    fn parse_with_version(raw: &str, source: &str) -> anyhow::Result<Self> {
        let reg: Registry = serde_json::from_str(raw)
            .map_err(|e| anyhow::anyhow!("{source} JSON 파싱 실패: {e}"))?;
        if reg.format_version != SUPPORTED_FORMAT_VERSION {
            anyhow::bail!(
                "{source} format_version={} 은 runtime 이 지원하지 않음 (supported={}). \
                 Nickel SSOT 업그레이드 또는 pxi 바이너리 업그레이드 필요.",
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const VALID_V1: &str = r#"{
        "format_version": 1,
        "domains": {
            "lxc": {
                "name": "lxc",
                "description": "LXC 관리",
                "tags": {"product": "infra", "layer": "remote", "platform": "proxmox"},
                "provides": ["lxc create"]
            }
        }
    }"#;

    const EMPTY: &str = "{}";
    const WRONG_VERSION: &str = r#"{"format_version": 99, "domains": {}}"#;
    const BROKEN_JSON: &str = r#"not valid json"#;

    fn write_tmp(contents: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pxi-registry-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!(
            "locale-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        path
    }

    #[test]
    fn tier1_valid_hit_returns_fs() {
        let path = write_tmp(VALID_V1);
        let reg = Registry::load_from_sources(Some(path.clone()), EMPTY).unwrap();
        assert_eq!(reg.format_version, 1);
        assert!(reg.domains.contains_key("lxc"));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn tier1_invalid_hard_fails_no_embedded_downgrade() {
        // codex #47 P1: tier 1 가 존재하는데 format_version 틀리면 절대 tier 2 로 내려가선 안 됨.
        let path = write_tmp(WRONG_VERSION);
        let err = Registry::load_from_sources(Some(path.clone()), VALID_V1).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("format_version=99"), "unexpected: {msg}");
        // embedded 가 valid 였어도 tier 2 로 넘어가지 않았음을 확인
        assert!(
            !msg.contains("<embedded>"),
            "should not mention embedded: {msg}"
        );
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn tier1_broken_json_hard_fails() {
        let path = write_tmp(BROKEN_JSON);
        let err = Registry::load_from_sources(Some(path.clone()), VALID_V1).unwrap_err();
        assert!(format!("{err}").contains("JSON 파싱 실패"));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn tier2_embedded_hit_when_fs_absent() {
        let reg = Registry::load_from_sources(None, VALID_V1).unwrap();
        assert_eq!(reg.format_version, 1);
    }

    #[test]
    fn tier3_fails_when_neither() {
        // codex #53 P2: tier 2 실패 원인이 최종 에러에 포함돼야 디버그 가능.
        let err = Registry::load_from_sources(None, EMPTY).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("<embedded>"), "tier-2 원인 누락: {msg}");
    }
}
