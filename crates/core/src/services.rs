//! 서비스 alias → VMID 레지스트리 로더.
//!
//! 로드 우선순위:
//!   1) `/root/control-plane/config/services_registry.toml` (개발/운영 호스트 — 정본)
//!   2) `/etc/pxi/services_registry.toml` (설치 후 시스템 배치)
//!   3) **내장 TOML** (crate `assets/services_registry.toml` — include_str)
//!
//! 3단계 fallback 이라 fresh 설치에서도 hard-fail 하지 않음. 외부 파일이 정본이고
//! 내장 사본은 control-plane 싱크 전까지의 안전망.
//!
//! Nickel contract: `control-plane/nickel/services_registry_contract.ncl`.
//! 런타임 (여기) 에선 TOML 파싱만. contract 검증은 CI 에 위임.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// 빌드 시점에 crate 에 동봉된 레지스트리 사본 (외부 파일 없을 때 fallback).
/// control-plane/config/services_registry.toml 과 sync 필요 — CI 에서 drift 체크 권장.
const EMBEDDED_REGISTRY: &str = include_str!("../assets/services_registry.toml");

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceEntry {
    pub vmid: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ServicesRegistry {
    pub services: HashMap<String, ServiceEntry>,
}

impl ServicesRegistry {
    /// 3단계 fallback:
    ///   1) `/root/control-plane/config/services_registry.toml`
    ///   2) `/etc/pxi/services_registry.toml`
    ///   3) crate 에 embed 된 TOML (최후 안전망)
    ///
    /// 외부 파일이 있으면 embed 된 것은 무시. embed 도 실패하면 빌드가 깨진다
    /// (include_str!). 런타임에서는 항상 Ok 를 반환한다고 사실상 가정 가능.
    pub fn load() -> anyhow::Result<Self> {
        // 규약:
        //   - 파일 없음 (exists=false) → 다음 tier 로 (fresh install 안전망)
        //   - 파일 있음 but 파싱/읽기 실패 → **bail** (stale 사본 쓰지 않음. 운영자 개입 요구)
        // embed 는 모든 외부 tier 가 missing 일 때만.
        let tiers = [
            PathBuf::from("/root/control-plane/config/services_registry.toml"),
            PathBuf::from("/etc/pxi/services_registry.toml"),
        ];
        for path in &tiers {
            if !path.exists() {
                continue;
            }
            let raw = std::fs::read_to_string(path).map_err(|e| {
                anyhow::anyhow!(
                    "{} 읽기 실패: {e}. 수정 후 재실행 (stale embed 로 대체 안 함).",
                    path.display()
                )
            })?;
            let reg: Self = toml::from_str(&raw).map_err(|e| {
                anyhow::anyhow!(
                    "{} TOML 파싱 실패: {e}. 수정 후 재실행 (stale embed 로 대체 안 함).",
                    path.display()
                )
            })?;
            return Ok(reg);
        }
        Ok(toml::from_str(EMBEDDED_REGISTRY)?)
    }

    /// alias 로 VMID 조회. 없으면 Err (도메인은 bail 으로 처리).
    pub fn vmid_for(&self, alias: &str) -> anyhow::Result<&str> {
        self.services
            .get(alias)
            .map(|e| e.vmid.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "services_registry: alias {alias} 미등록. \
                 /root/control-plane/config/services_registry.toml 에 추가 필요."
                )
            })
    }

    /// alias 로 canonical IP 유도 (vmid 조회 후 `convention::canonical_ip`).
    pub fn canonical_ip(&self, alias: &str) -> anyhow::Result<String> {
        let vmid = self.vmid_for(alias)?;
        crate::convention::canonical_ip(vmid)
    }
}

/// 편의 API — 대부분의 도메인은 `vmid_for("mail")` 한 번만 부르면 됨.
pub fn vmid_for(alias: &str) -> anyhow::Result<String> {
    let reg = ServicesRegistry::load()?;
    reg.vmid_for(alias).map(|s| s.to_string())
}

/// 편의 API — alias 로 바로 canonical IP.
pub fn canonical_ip_for(alias: &str) -> anyhow::Result<String> {
    let reg = ServicesRegistry::load()?;
    reg.canonical_ip(alias)
}
