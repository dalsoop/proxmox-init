//! GitHub Release 바이너리 다운로드 (도메인 installer).

use crate::common;
use std::path::Path;

pub fn latest_tag(repo: &str) -> anyhow::Result<String> {
    let out = common::run(
        "curl",
        &[
            "-sSL",
            "--fail",
            "-H",
            "Accept: application/vnd.github+json",
            &format!("https://api.github.com/repos/{repo}/releases/latest"),
        ],
    )
    .map_err(|e| anyhow::anyhow!("GitHub API 호출 실패 ({repo}): {e}"))?;
    let v: serde_json::Value = serde_json::from_str(&out)
        .map_err(|e| anyhow::anyhow!("GitHub API 응답 파싱 실패: {e}"))?;
    v["tag_name"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("tag_name 필드 없음 — 릴리스가 존재하지 않을 수 있음"))
}

/// HTTP status 검사 + content-type 확인으로 조용한 실패 방지.
pub fn download_asset(repo: &str, tag: &str, asset: &str, dest: &Path) -> anyhow::Result<()> {
    let url = format!("https://github.com/{repo}/releases/download/{tag}/{asset}");

    // -w 로 status code를 받아서 검증 (-f 옵션과 -o 조합의 모호함 회피)
    let output = std::process::Command::new("curl")
        .args([
            "-sSL",
            "--fail",
            "-o",
            &dest.display().to_string(),
            "-w",
            "%{http_code} %{content_type}",
            &url,
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("curl 실행 실패: {e}"))?;

    if !output.status.success() {
        let _ = std::fs::remove_file(dest);
        anyhow::bail!(
            "다운로드 실패 ({}): curl exit {} — {}",
            asset,
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let info = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = info.split_whitespace().collect();
    let http_code = parts.first().copied().unwrap_or("?");
    let content_type = parts.get(1).copied().unwrap_or("?");

    if http_code != "200" {
        let _ = std::fs::remove_file(dest);
        anyhow::bail!(
            "다운로드 실패 ({}): HTTP {} (GitHub 릴리스 자산 이름이 틀렸을 가능성)",
            asset,
            http_code
        );
    }

    // GitHub는 asset을 application/octet-stream 또는 gzip/x-tar 등으로 반환.
    // HTML이 오면 에러 페이지.
    if content_type.contains("text/html") {
        let _ = std::fs::remove_file(dest);
        anyhow::bail!(
            "다운로드된 파일이 HTML입니다 ({}): 자산이 존재하지 않을 수 있음",
            asset
        );
    }

    // 최종적으로 파일이 비어있지 않은지 확인
    let meta = std::fs::metadata(dest)?;
    if meta.len() < 64 {
        let _ = std::fs::remove_file(dest);
        anyhow::bail!(
            "다운로드된 파일이 너무 작음 ({} bytes) — 자산이 손상됐을 가능성",
            meta.len()
        );
    }

    Ok(())
}
