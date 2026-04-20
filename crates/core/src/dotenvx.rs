//! dotenvx 래퍼 (.env.vault 암호화/복호화).
//! 전제: `dotenvx` CLI가 PATH에 있음. `pxi install bootstrap`이 설치.

use crate::{common, paths};
use std::path::Path;

pub fn is_installed() -> bool {
    common::has_cmd("dotenvx")
}

pub fn encrypt(env_path: &Path) -> anyhow::Result<()> {
    common::run("dotenvx", &["encrypt", "-f", &env_path.display().to_string()])?;
    Ok(())
}

pub fn get(key: &str) -> anyhow::Result<String> {
    let env = paths::env_file()?;
    common::run_capture("dotenvx", &["get", key, "-f", &env.display().to_string()])
}
