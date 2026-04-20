//! 브랜드 상수 — 프로젝트 전체 이름/경로 문자열 source of truth.
//!
//! 이름을 바꾸고 싶으면 이 파일 상수만 수정 후 `cargo build`.
//! 시스템 경로 마이그레이션은 `pxi rebrand` 커맨드가 처리.
//!
//! **런타임 경로 헬퍼는 `pxi_core::paths` 로 분리.** (config_dir/data_dir/bin_dir 등)
//! 여기는 문자열 상수만 둔다.

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

/// 도메인 바이너리 이름 생성 (예: "pxi-elk")
pub fn domain_bin(domain: &str) -> String {
    format!("{}-{}", BIN_PREFIX, domain)
}
