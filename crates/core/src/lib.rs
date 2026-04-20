//! pxi-init 공통 라이브러리.
//! OS 추상화, 경로 규약, dotenvx 래퍼, GitHub Release 다운로더, systemd 헬퍼.

pub mod brand;
pub mod common;
pub mod config;
pub mod convention;
pub mod dotenvx;
pub mod github;
pub mod helpers;
pub mod os;
pub mod paths;
pub mod registry;
pub mod services;
pub mod systemd;
pub mod types;
