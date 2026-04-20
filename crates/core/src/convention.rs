//! VMID ↔ IP 규약 — 모든 pxi 도메인이 공유.
//!
//! ## 규약
//! VMID 는 5자리 `AABCC` 숫자 (일관 요구).
//!   - `AA` = 서브넷 3번째 옥텟 (50=pve, 60=ranode-3960x)
//!   - `BCC` = 마지막 옥텟 (0-255)
//!
//! 예:
//! - `50210` → `10.0.50.210`
//! - `60104` → `10.0.60.104`
//!
//! ## 강제 이유
//! 레지스트리·DNS·traefik·방화벽 규칙이 모두 VMID→IP 매핑을 가정.
//! 불일치 시 서비스 discovery 깨짐. 예외 허용 안 함 — 미디어 서버 등
//! 특수 케이스도 규약 안에 들어오게 해야 함.

/// 허용된 서브넷 — 늘리려면 인프라 합의 필요.
const ALLOWED_SUBNETS: &[u8] = &[50, 60];

/// VMID 로부터 canonical IP (CIDR 없이) 반환.
///
/// ```text
/// canonical_ip("50211") => Ok("10.0.50.211")
/// canonical_ip("60104") => Ok("10.0.60.104")
/// canonical_ip("70001") => Err (서브넷 70 미지원)
/// canonical_ip("abc")   => Err (숫자 아님)
/// ```
pub fn canonical_ip(vmid: &str) -> anyhow::Result<String> {
    if vmid.len() != 5 || !vmid.chars().all(|c| c.is_ascii_digit()) {
        anyhow::bail!("VMID 규약 위반: 5자리 숫자(AABCC) 필수. 받은 값: {vmid}");
    }
    let subnet: u8 = vmid[..2].parse()
        .map_err(|_| anyhow::anyhow!("VMID {vmid}: 서브넷 파싱 실패"))?;
    let tail: u32 = vmid[2..].parse()
        .map_err(|_| anyhow::anyhow!("VMID {vmid}: tail 파싱 실패"))?;
    if !ALLOWED_SUBNETS.contains(&subnet) {
        anyhow::bail!(
            "VMID {vmid}: 서브넷 {subnet} 미지원. 허용: {:?} (pve=50, ranode=60)",
            ALLOWED_SUBNETS
        );
    }
    if tail > 255 {
        anyhow::bail!(
            "VMID {vmid}: tail({tail}) > 255. 규약상 {subnet}000-{subnet}255 만 유효."
        );
    }
    Ok(format!("10.0.{subnet}.{tail}"))
}

/// 명시적 IP 가 canonical 과 일치하는지 검증. CIDR(`/16`) 는 자동 스트립.
pub fn validate_ip(vmid: &str, ip_cidr: &str) -> anyhow::Result<()> {
    let canonical = canonical_ip(vmid)?;
    let ip_only = ip_cidr.split('/').next().unwrap_or(ip_cidr).trim();
    if ip_only != canonical {
        anyhow::bail!(
            "규약 위반: VMID {vmid} 는 IP {canonical} 이어야 함. 받은 값: {ip_only}. \
             --ip 생략하거나 {canonical}/16 로 맞추기."
        );
    }
    Ok(())
}

/// VMID 로부터 CIDR 포함 canonical IP. prefix 기본 16.
pub fn canonical_cidr(vmid: &str, prefix: u8) -> anyhow::Result<String> {
    Ok(format!("{}/{}", canonical_ip(vmid)?, prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pve_vmid_to_ip() {
        assert_eq!(canonical_ip("50210").unwrap(), "10.0.50.210");
        assert_eq!(canonical_ip("50000").unwrap(), "10.0.50.0");
        assert_eq!(canonical_ip("50255").unwrap(), "10.0.50.255");
    }

    #[test]
    fn ranode_vmid_to_ip() {
        assert_eq!(canonical_ip("60104").unwrap(), "10.0.60.104");
    }

    #[test]
    fn bad_subnet_rejected() {
        assert!(canonical_ip("70001").is_err());
        assert!(canonical_ip("10001").is_err());
    }

    #[test]
    fn bad_vmid_format_rejected() {
        assert!(canonical_ip("5021").is_err());   // 4자리
        assert!(canonical_ip("502100").is_err()); // 6자리
        assert!(canonical_ip("abcde").is_err());  // 비숫자
    }

    #[test]
    fn tail_over_255_rejected() {
        assert!(canonical_ip("50256").is_err());
        assert!(canonical_ip("50999").is_err());
    }

    #[test]
    fn validate_ip_ok() {
        assert!(validate_ip("50210", "10.0.50.210").is_ok());
        assert!(validate_ip("50210", "10.0.50.210/16").is_ok());
    }

    #[test]
    fn validate_ip_mismatch_rejected() {
        assert!(validate_ip("50210", "10.0.50.99/16").is_err());
        assert!(validate_ip("50210", "10.0.60.210/16").is_err());
    }
}
