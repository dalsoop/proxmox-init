//! 도메인 간 공유되는 유틸리티.
//! read_host_env, write_to_lxc, Cleanup 가드.

use crate::common;
use std::fs;
use std::path::{Path, PathBuf};

/// 호스트의 설정 .env에서 값 읽기. 여러 경로 fallback.
pub fn read_host_env(key: &str) -> String {
    let paths = ["/etc/pxi/.env", "/etc/proxmox-host-setup/.env"];
    for p in paths {
        if let Ok(raw) = fs::read_to_string(p) {
            for line in raw.lines() {
                if let Some(v) = line.strip_prefix(&format!("{key}=")) {
                    return v.trim().trim_matches('"').to_string();
                }
            }
        }
    }
    String::new()
}

/// Drop 시 파일 자동 삭제 가드.
pub struct FileCleanup(pub PathBuf);
impl Drop for FileCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// mktemp로 private 파일 만들고 0600 권한 보장 + Drop 가드.
/// 반환: (path 문자열, Cleanup 가드).
pub fn secure_tempfile() -> anyhow::Result<(String, FileCleanup)> {
    let out = common::run("mktemp", &["-t", "pxi.XXXXXXXX"])?;
    let tmp = out.trim().to_string();
    let guard = FileCleanup(PathBuf::from(&tmp));
    common::run("chmod", &["600", &tmp])?;
    Ok((tmp, guard))
}

/// LXC 안의 특정 경로에 파일 내용을 안전하게 배치.
/// mktemp + 0600 + pct push 원자성.
pub fn write_to_lxc(vmid: &str, path: &str, content: &str) -> anyhow::Result<()> {
    let (tmp, _guard) = secure_tempfile()?;
    fs::write(Path::new(&tmp), content)?;
    common::run("pct", &["push", vmid, &tmp, path])?;
    Ok(())
}
