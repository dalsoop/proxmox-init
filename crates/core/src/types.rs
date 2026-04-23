//! 강타입 newtype 모음 — Stringly-typed API 를 점진적으로 대체.
//!
//! 현재 포함:
//!   - `Vmid`  : convention 규약 검증이 생성 시점에 강제되는 VMID
//!   - `IpCidr`: "10.0.50.210/16" 같은 CIDR 을 구조화
//!   - `LxcStatus`: "running" 등 문자열 매칭을 enum 으로
//!
//! clap 에서 바로 쓰려면 `#[arg(long)] vmid: Vmid` — FromStr 덕분에 파싱 시점에
//! 검증, invalid 입력은 커맨드 본체에 닿기도 전에 거부.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::Ipv4Addr;
use std::str::FromStr;

// ============================================================================
// Vmid
// ============================================================================

/// 규약(AABCC 5자리, AA ∈ {50,60}, BCC ≤ 255)을 생성 시점에 강제하는 VMID.
///
/// ```text
/// "50210".parse::<Vmid>()?  // Ok(Vmid("50210"))
/// "abc".parse::<Vmid>()?    // Err — clap argparse 단에서 바로 실패
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Vmid(String);

impl Vmid {
    /// 이미 규약 맞는다고 확신하는 경우 (registry 등 내부에서 읽은 값)만.
    /// 외부 입력은 `from_str` / `FromStr::from_str` 사용.
    pub fn new_unchecked(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// convention::canonical_ip(self) 바로가기.
    pub fn canonical_ip(&self) -> anyhow::Result<String> {
        crate::convention::canonical_ip(&self.0)
    }
}

impl fmt::Display for Vmid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Vmid {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        // convention::canonical_ip 가 형식 검증을 겸함. Ok 면 규약 통과.
        crate::convention::canonical_ip(s)?;
        Ok(Self(s.to_string()))
    }
}

impl TryFrom<String> for Vmid {
    type Error = anyhow::Error;
    fn try_from(s: String) -> anyhow::Result<Self> {
        s.parse()
    }
}

impl From<Vmid> for String {
    fn from(v: Vmid) -> String {
        v.0
    }
}

impl AsRef<str> for Vmid {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// ============================================================================
// IpCidr
// ============================================================================

/// "10.0.50.210/16" 같은 IPv4 CIDR 표기.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct IpCidr {
    pub ip: Ipv4Addr,
    pub prefix: u8,
}

impl IpCidr {
    pub fn new(ip: Ipv4Addr, prefix: u8) -> anyhow::Result<Self> {
        if prefix > 32 {
            anyhow::bail!("IPv4 CIDR prefix > 32: {prefix}");
        }
        Ok(Self { ip, prefix })
    }
}

impl fmt::Display for IpCidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.ip, self.prefix)
    }
}

impl FromStr for IpCidr {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        // CIDR prefix 필수. bare IP 허용하려면 config.network.subnet 과 연동돼야
        // 하는데 타입 단독 로는 그 값 알 수 없음 (codex #38: /16 하드코딩 금지).
        // bare IP 가 필요한 호출측은 String 유지 후 pxi-lxc 가 config 기반 확장.
        let (ip_s, prefix_s) = s.split_once('/').ok_or_else(|| {
            anyhow::anyhow!(
                "IpCidr 는 CIDR 형식 필수 (예: 10.0.50.210/16). bare IP 는 호출측에서 \
                 직접 처리 (pxi-lxc 가 config.network.subnet 적용). 받은 값: {s}"
            )
        })?;
        let ip: Ipv4Addr = ip_s
            .parse()
            .map_err(|e| anyhow::anyhow!("IPv4 파싱 실패 '{ip_s}': {e}"))?;
        let prefix: u8 = prefix_s
            .parse()
            .map_err(|e| anyhow::anyhow!("prefix 파싱 실패 '{prefix_s}': {e}"))?;
        Self::new(ip, prefix)
    }
}

impl TryFrom<String> for IpCidr {
    type Error = anyhow::Error;
    fn try_from(s: String) -> anyhow::Result<Self> {
        s.parse()
    }
}

impl From<IpCidr> for String {
    fn from(c: IpCidr) -> String {
        c.to_string()
    }
}

// ============================================================================
// LxcStatus
// ============================================================================

/// `pct status <vmid>` 출력 ("status: running" 등) 을 enum 으로.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LxcStatus {
    Running,
    Stopped,
    NotFound,
    Unknown,
}

impl LxcStatus {
    pub fn is_running(self) -> bool {
        matches!(self, Self::Running)
    }
    pub fn is_stopped(self) -> bool {
        matches!(self, Self::Stopped)
    }
    pub fn exists(self) -> bool {
        !matches!(self, Self::NotFound)
    }
}

impl FromStr for LxcStatus {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // pct status 전형적 출력: "status: running\n" 또는 "status: stopped\n"
        // pct status 존재 안 하면: "Configuration file '...' does not exist"
        let lower = s.to_ascii_lowercase();
        let result = if lower.contains("does not exist") || lower.contains("no such") {
            Self::NotFound
        } else if lower.contains("running") {
            Self::Running
        } else if lower.contains("stopped") {
            Self::Stopped
        } else {
            Self::Unknown
        };
        Ok(result)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vmid_parses_valid() {
        let v: Vmid = "50210".parse().unwrap();
        assert_eq!(v.as_str(), "50210");
        assert_eq!(v.canonical_ip().unwrap(), "10.0.50.210");
    }

    #[test]
    fn vmid_rejects_invalid() {
        assert!("abc".parse::<Vmid>().is_err());
        assert!("70001".parse::<Vmid>().is_err());
        assert!("50256".parse::<Vmid>().is_err());
        assert!("502100".parse::<Vmid>().is_err());
    }

    #[test]
    fn vmid_display_roundtrip() {
        let v: Vmid = "60104".parse().unwrap();
        assert_eq!(v.to_string(), "60104");
        // serde derive 확인 — TryFrom<String> 경유
        let back: Vmid = v.to_string().try_into().unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn ipcidr_parses_valid() {
        let c: IpCidr = "10.0.50.210/16".parse().unwrap();
        assert_eq!(c.ip, Ipv4Addr::new(10, 0, 50, 210));
        assert_eq!(c.prefix, 16);
        assert_eq!(c.to_string(), "10.0.50.210/16");
    }

    #[test]
    fn ipcidr_requires_cidr() {
        // bare IP 거부 (prefix 필수) — config subnet 적용은 호출측 책임
        assert!("10.0.50.210".parse::<IpCidr>().is_err());
        let e = "10.0.50.210".parse::<IpCidr>().unwrap_err().to_string();
        assert!(e.contains("CIDR 형식 필수"));
    }

    #[test]
    fn ipcidr_rejects_malformed() {
        assert!("abc/16".parse::<IpCidr>().is_err()); // bad IP
        assert!("10.0.50.210/99".parse::<IpCidr>().is_err()); // prefix > 32
    }

    #[test]
    fn lxc_status_parsing() {
        assert_eq!(
            "status: running".parse::<LxcStatus>().unwrap(),
            LxcStatus::Running
        );
        assert_eq!(
            "status: stopped".parse::<LxcStatus>().unwrap(),
            LxcStatus::Stopped
        );
        assert_eq!(
            "Configuration file '/etc/pve/lxc/99999.conf' does not exist"
                .parse::<LxcStatus>()
                .unwrap(),
            LxcStatus::NotFound
        );
        assert_eq!("weirdo".parse::<LxcStatus>().unwrap(), LxcStatus::Unknown);
    }

    #[test]
    fn lxc_status_helpers() {
        assert!(LxcStatus::Running.is_running());
        assert!(LxcStatus::Running.exists());
        assert!(!LxcStatus::NotFound.exists());
        assert!(LxcStatus::Stopped.is_stopped());
    }
}
