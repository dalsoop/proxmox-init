//! 브랜드 상수 — 프로젝트 전체 이름/경로의 유일한 원천.
//!
//! 이름을 바꾸고 싶으면 이 파일만 수정하고 `cargo build`.
//! 시스템 경로 마이그레이션은 `pxi rebrand` 커맨드가 처리.

/// CLI 짧은 이름 (바이너리명, 커맨드)
pub const SHORT: &str = "pxi";

/// 프로젝트 전체 이름
pub const FULL: &str = "proxmox-init";

/// GitHub org/repo
pub const REPO: &str = "dalsoop/proxmox-init";

/// 설정 디렉토리 이름 (/etc/{NAME} 또는 ~/.config/{NAME})
pub const CONFIG_DIR_NAME: &str = "pxi";

/// 데이터 디렉토리 이름 (/var/lib/{NAME})
pub const DATA_DIR_NAME: &str = "pxi";

/// 도메인 바이너리 prefix (e.g. "pxi-elk", "pxi-telegram")
pub const BIN_PREFIX: &str = "pxi";

/// 도메인 실행 시 표시명
pub const fn bin_name(domain: &str) -> String {
    // const fn에서 String 못 만들므로 런타임 헬퍼로
    unreachable!()
}

/// 도메인 바이너리 이름 생성
pub fn domain_bin(domain: &str) -> String {
    format!("{}-{}", BIN_PREFIX, domain)
}

/// 시스템 경로들
pub mod paths {
    use super::*;

    pub fn config_root() -> &'static str {
        concat!("/etc/", "pxi")
    }

    pub fn data_root() -> &'static str {
        concat!("/var/lib/", "pxi")
    }
}
