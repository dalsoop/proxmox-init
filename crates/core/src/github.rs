//! GitHub Release 바이너리 다운로드 (도메인 installer).

use crate::common;
use std::path::Path;

pub fn latest_tag(repo: &str) -> anyhow::Result<String> {
    let out = common::run(
        "curl",
        &[
            "-sL",
            &format!("https://api.github.com/repos/{repo}/releases/latest"),
        ],
    )?;
    let v: serde_json::Value = serde_json::from_str(&out)?;
    v["tag_name"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("tag_name 필드 없음"))
}

pub fn download_asset(repo: &str, tag: &str, asset: &str, dest: &Path) -> anyhow::Result<()> {
    let url = format!("https://github.com/{repo}/releases/download/{tag}/{asset}");
    common::run(
        "curl",
        &["-sL", "-o", &dest.display().to_string(), &url],
    )?;
    Ok(())
}
