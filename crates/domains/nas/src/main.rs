//! pxi-nas — 범용 NAS 관리.
//! SMB/CIFS + NFS 마운트, Synology DSM API, TrueNAS API 지원.

use clap::{Parser, Subcommand, ValueEnum};
use pxi_core::common;
use std::fs;

#[derive(Parser)]
#[command(name = "pxi-nas", about = "NAS 마운트 + Synology/TrueNAS 관리")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    // ---- Mount management ----

    /// NAS 공유 마운트
    Mount {
        /// NAS 서버 주소 (예: 192.168.1.100)
        #[arg(long)]
        host: String,
        /// 공유 이름 (SMB) 또는 export 경로 (NFS)
        #[arg(long)]
        share: String,
        /// 로컬 마운트 포인트 (예: /mnt/nas)
        #[arg(long)]
        target: String,
        /// 프로토콜
        #[arg(long, value_enum, default_value = "smb")]
        protocol: Protocol,
        /// SMB 사용자 (SMB에만 필요)
        #[arg(long)]
        user: Option<String>,
        /// SMB 비밀번호 (SMB에만 필요, 또는 /etc/pxi/.env의 NAS_PASSWORD)
        #[arg(long)]
        password: Option<String>,
        /// /etc/fstab에 영구 등록
        #[arg(long)]
        persist: bool,
    },
    /// 마운트 해제
    Unmount { target: String },
    /// 현재 마운트 목록 (NFS + CIFS만 필터)
    List,

    // ---- Combined NAS status ----

    /// Synology + TrueNAS 통합 상태
    Status,
    /// 전체 공유 목록 (SMB + NFS)
    Shares,
    /// 스토리지 풀/볼륨 목록
    Pools,

    // ---- Share CRUD ----

    /// Synology 공유 폴더 생성
    ShareCreate {
        #[arg(long)]
        name: String,
        #[arg(long)]
        volume: String,
        #[arg(long, default_value = "")]
        desc: String,
    },
    /// Synology 공유 폴더 삭제
    ShareDelete {
        #[arg(long)]
        name: String,
    },

    // ---- Synology-specific ----

    /// Synology에서 공유 폴더 목록 (FileStation API)
    SynologyList,
    /// Synology Active Backup 등 동기화 상태
    SynologySync,
    /// Synology QuickConnect 링크 표시
    SynologyLink,

    Doctor,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Protocol {
    Smb,
    Nfs,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Mount { host, share, target, protocol, user, password, persist } => {
            mount(&host, &share, &target, protocol, user.as_deref(), password.as_deref(), persist)
        }
        Cmd::Unmount { target } => unmount(&target),
        Cmd::List => { list(); Ok(()) }
        Cmd::Status => nas_status(),
        Cmd::Shares => nas_shares(),
        Cmd::Pools => nas_pools(),
        Cmd::ShareCreate { name, volume, desc } => synology_create_share(&name, &volume, &desc),
        Cmd::ShareDelete { name } => synology_delete_share(&name),
        Cmd::SynologyList => synology_list(),
        Cmd::SynologySync => synology_sync(),
        Cmd::SynologyLink => { synology_link(); Ok(()) }
        Cmd::Doctor => { doctor(); Ok(()) }
    }
}

// ============================================================
// Mount management (ported from existing pxi-nas)
// ============================================================

fn mount(
    host: &str, share: &str, target: &str,
    protocol: Protocol,
    user: Option<&str>, password: Option<&str>,
    persist: bool,
) -> anyhow::Result<()> {
    println!("=== NAS 마운트 ({protocol:?}) ===");
    println!("  source: {host}:{share}");
    println!("  target: {target}");

    common::run("sudo", &["mkdir", "-p", target])?;

    match protocol {
        Protocol::Smb => mount_smb(host, share, target, user, password, persist),
        Protocol::Nfs => mount_nfs(host, share, target, persist),
    }
}

fn mount_smb(
    host: &str, share: &str, target: &str,
    user: Option<&str>, password: Option<&str>,
    persist: bool,
) -> anyhow::Result<()> {
    let user = user.map(String::from).unwrap_or_else(|| read_env("NAS_USER"));
    let password = password.map(String::from).unwrap_or_else(|| read_env("NAS_PASSWORD"));
    if user.is_empty() {
        anyhow::bail!("SMB 마운트에는 --user 또는 NAS_USER 환경변수 필요");
    }

    let safe_name = format!("{host}_{share}")
        .chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect::<String>();
    let cred_path = format!("/etc/cifs-credentials/{safe_name}");

    common::run("sudo", &["mkdir", "-p", "/etc/cifs-credentials"])?;
    common::run("sudo", &["chmod", "700", "/etc/cifs-credentials"])?;

    let (tmp, _guard) = secure_tempfile()?;
    let content = if password.is_empty() {
        format!("username={user}\n")
    } else {
        format!("username={user}\npassword={password}\n")
    };
    std::fs::write(&tmp, content)?;
    common::run("sudo", &[
        "install", "-m", "600", "-o", "root", "-g", "root",
        &tmp, &cred_path,
    ])?;

    let source = format!("//{host}/{share}");
    let options = format!("credentials={cred_path},vers=3.0,iocharset=utf8,_netdev,nofail");
    common::run("sudo", &[
        "mount", "-t", "cifs", "-o", &options, &source, target,
    ])?;
    println!("✓ 마운트 완료 (credentials: {cred_path}, 0600)");

    if persist {
        let fstab_line = format!("{source} {target} cifs {options} 0 0");
        fstab_add(target, &fstab_line)?;
    }
    Ok(())
}

fn mount_nfs(host: &str, share: &str, target: &str, persist: bool) -> anyhow::Result<()> {
    let source = format!("{host}:{share}");
    common::run("sudo", &["mount", "-t", "nfs", &source, target])?;
    println!("✓ 마운트 완료");

    if persist {
        let fstab_line = format!("{source} {target} nfs _netdev,nofail 0 0");
        fstab_add(target, &fstab_line)?;
    }
    Ok(())
}

fn fstab_add(target: &str, fstab_line: &str) -> anyhow::Result<()> {
    let check = std::process::Command::new("sudo")
        .args(["grep", "-qF", target, "/etc/fstab"])
        .status();
    if check.ok().map(|s| s.success()).unwrap_or(false) {
        println!("  ⊘ /etc/fstab에 이미 등록됨 — 건너뜀");
        return Ok(());
    }

    let last_byte = std::process::Command::new("sudo")
        .args(["sh", "-c", "tail -c1 /etc/fstab 2>/dev/null | od -An -tx1 | tr -d ' \n'"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let needs_leading_nl = !last_byte.is_empty() && last_byte != "0a";
    let prefix = if needs_leading_nl { "\n" } else { "" };
    let line_with_nl = format!("{prefix}{fstab_line}\n");
    let output = std::process::Command::new("sudo")
        .args(["tee", "-a", "/etc/fstab"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(line_with_nl.as_bytes())?;
            }
            child.wait()
        });

    match output {
        Ok(status) if status.success() => {
            println!("  ✓ /etc/fstab 등록 (재부팅 후에도 유지)");
            Ok(())
        }
        Ok(status) => anyhow::bail!("fstab append 실패 (exit: {})", status.code().unwrap_or(-1)),
        Err(e) => anyhow::bail!("sudo tee -a 실행 실패: {e}"),
    }
}

fn secure_tempfile() -> anyhow::Result<(String, TempGuard)> {
    let out = common::run("mktemp", &["-t", "pxi.XXXXXXXX"])?;
    let tmp = out.trim().to_string();
    let guard = TempGuard(tmp.clone());
    common::run("chmod", &["600", &tmp])?;
    Ok((tmp, guard))
}

struct TempGuard(String);
impl Drop for TempGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn unmount(target: &str) -> anyhow::Result<()> {
    println!("=== 마운트 해제: {target} ===");
    common::run("sudo", &["umount", target])?;
    let check = std::process::Command::new("grep")
        .args(["-qF", target, "/etc/fstab"])
        .status();
    if check.ok().map(|s| s.success()).unwrap_or(false) {
        println!("  ⚠ /etc/fstab에 등록 있음. 영구 제거: sudo sed -i \"\\|{target}|d\" /etc/fstab");
    }
    println!("✓ 해제 완료");
    Ok(())
}

fn list() {
    println!("=== NAS 마운트 목록 (cifs + nfs) ===");
    if let Ok(out) = common::run_bash("mount | grep -E 'type (cifs|nfs)'") {
        if out.trim().is_empty() {
            println!("  (없음)");
        } else {
            for line in out.lines() {
                println!("  {line}");
            }
        }
    } else {
        println!("  (없음)");
    }
}

// ============================================================
// Synology DSM API
// ============================================================

struct SynologyConfig {
    url: String,
    user: String,
    password: String,
}

impl SynologyConfig {
    fn load() -> Option<Self> {
        let url = read_env("SYNOLOGY_URL");
        let user = read_env("SYNOLOGY_USER");
        let password = read_env("SYNOLOGY_PASSWORD");
        if url.is_empty() || user.is_empty() {
            return None;
        }
        Some(Self { url, user, password })
    }
}

fn synology_api_call(cfg: &SynologyConfig, sid: &str, api: &str, method: &str, version: u32, extra: &str) -> anyhow::Result<serde_json::Value> {
    let url = format!(
        "{}/webapi/entry.cgi?api={api}&version={version}&method={method}&_sid={sid}{extra}",
        cfg.url
    );
    let output = std::process::Command::new("curl")
        .args(["-sSLk", &url])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("[synology] API 호출 실패");
    }
    let body: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&output.stdout))?;
    if body["success"].as_bool() != Some(true) {
        anyhow::bail!("[synology] API 오류: {}", body);
    }
    Ok(body["data"].clone())
}

fn synology_login(cfg: &SynologyConfig) -> anyhow::Result<String> {
    let url = format!("{}/webapi/auth.cgi", cfg.url);
    let device_id = "pxi-nas";
    let params = format!(
        "api=SYNO.API.Auth&version=6&method=login&account={}&passwd={}&format=sid&device_id={device_id}&device_name={device_id}",
        urlenc(&cfg.user),
        urlenc(&cfg.password)
    );

    let output = std::process::Command::new("curl")
        .args(["-sSLk", "-X", "POST",
            "-H", "Content-Type: application/x-www-form-urlencoded",
            "-d", &params, &url])
        .output()?;

    let body: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&output.stdout))?;
    if body["success"].as_bool() == Some(true) {
        body["data"]["sid"].as_str().map(String::from)
            .ok_or_else(|| anyhow::anyhow!("[synology] sid 없음"))
    } else {
        let code = body["error"]["code"].as_i64().unwrap_or(0);
        if code == 403 {
            // Try OTP if available
            if let Ok(otp) = std::env::var("SYNOLOGY_OTP") {
                return synology_login_with_otp(cfg, &otp);
            }
            anyhow::bail!("[synology] 2FA OTP 필요. SYNOLOGY_OTP 환경변수로 전달하세요.");
        }
        anyhow::bail!("[synology] 로그인 실패 (code: {code}): {}", body);
    }
}

fn synology_login_with_otp(cfg: &SynologyConfig, otp: &str) -> anyhow::Result<String> {
    let url = format!("{}/webapi/auth.cgi", cfg.url);
    let device_id = "pxi-nas";
    let params = format!(
        "api=SYNO.API.Auth&version=6&method=login&account={}&passwd={}&format=sid&otp_code={}&device_id={device_id}&device_name={device_id}",
        urlenc(&cfg.user),
        urlenc(&cfg.password),
        otp
    );

    let output = std::process::Command::new("curl")
        .args(["-sSLk", "-X", "POST",
            "-H", "Content-Type: application/x-www-form-urlencoded",
            "-d", &params, &url])
        .output()?;

    let body: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&output.stdout))?;
    if body["success"].as_bool() == Some(true) {
        println!("[synology] OTP 인증 성공, device_id 등록 완료");
        body["data"]["sid"].as_str().map(String::from)
            .ok_or_else(|| anyhow::anyhow!("[synology] sid 없음"))
    } else {
        anyhow::bail!("[synology] OTP 로그인 실패: {}", body);
    }
}

fn synology_logout(cfg: &SynologyConfig, sid: &str) {
    let url = format!(
        "{}/webapi/auth.cgi?api=SYNO.API.Auth&version=6&method=logout&_sid={sid}",
        cfg.url
    );
    let _ = std::process::Command::new("curl")
        .args(["-sSLk", &url])
        .output();
}

fn urlenc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                String::from(b as char)
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

// ============================================================
// TrueNAS API
// ============================================================

struct TrueNasConfig {
    url: String,
    api_key: String,
}

impl TrueNasConfig {
    fn load() -> Option<Self> {
        let url = read_env("TRUENAS_URL");
        let api_key = read_env("TRUENAS_API_KEY");
        if url.is_empty() || api_key.is_empty() {
            return None;
        }
        Some(Self { url, api_key })
    }
}

fn truenas_api_get(cfg: &TrueNasConfig, path: &str) -> anyhow::Result<serde_json::Value> {
    let url = format!("{}/api/v2.0{path}", cfg.url);
    let output = std::process::Command::new("curl")
        .args(["-sSLk",
            "-H", &format!("Authorization: Bearer {}", cfg.api_key),
            &url])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("[truenas] 요청 실패 ({path})");
    }
    let body: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&output.stdout))?;
    Ok(body)
}

// ============================================================
// Combined NAS commands
// ============================================================

fn nas_status() -> anyhow::Result<()> {
    println!("=== NAS 상태 ===\n");

    // Synology
    if let Some(cfg) = SynologyConfig::load() {
        println!("[synology] {} 접속 중...", cfg.url);
        match synology_login(&cfg) {
            Ok(sid) => {
                println!("[synology] 로그인 성공");
                if let Ok(data) = synology_api_call(&cfg, &sid, "SYNO.DSM.Info", "getinfo", 2, "") {
                    let model = data["model"].as_str().unwrap_or("?");
                    let version = data["version_string"].as_str().unwrap_or("?");
                    println!("[synology] 모델: {model}");
                    println!("[synology] DSM: {version}");
                }
                if let Ok(data) = synology_api_call(&cfg, &sid, "SYNO.Storage.CGI.Storage", "load_info", 1, "") {
                    if let Some(volumes) = data["volumes"].as_array() {
                        println!("[synology] 볼륨:");
                        for vol in volumes {
                            let name = vol["display_name"].as_str()
                                .or(vol["vol_path"].as_str())
                                .unwrap_or("?");
                            let total = vol["size"]["total"].as_str().unwrap_or("0");
                            let used = vol["size"]["used"].as_str().unwrap_or("0");
                            let total_gb = total.parse::<u64>().unwrap_or(0) / 1_073_741_824;
                            let used_gb = used.parse::<u64>().unwrap_or(0) / 1_073_741_824;
                            let pct = if total_gb > 0 { used_gb * 100 / total_gb } else { 0 };
                            println!("  {name}: {used_gb}GB / {total_gb}GB ({pct}%)");
                        }
                    }
                }
                synology_logout(&cfg, &sid);
            }
            Err(e) => eprintln!("[synology] {e}"),
        }
    } else {
        println!("[synology] 미설정 (SYNOLOGY_URL / SYNOLOGY_USER 필요)");
    }

    println!();

    // TrueNAS
    if let Some(cfg) = TrueNasConfig::load() {
        println!("[truenas] {} 접속 중...", cfg.url);
        match truenas_api_get(&cfg, "/system/info") {
            Ok(info) => {
                let version = info["version"].as_str().unwrap_or("?");
                let hostname = info["hostname"].as_str().unwrap_or("?");
                let model = info["system_product"].as_str().unwrap_or("?");
                println!("[truenas] 호스트: {hostname}");
                println!("[truenas] 버전: {version}");
                println!("[truenas] 모델: {model}");
            }
            Err(e) => eprintln!("[truenas] {e}"),
        }
        if let Ok(pools) = truenas_api_get(&cfg, "/pool") {
            if let Some(pools) = pools.as_array() {
                println!("[truenas] 풀:");
                for pool in pools {
                    let name = pool["name"].as_str().unwrap_or("?");
                    let status = pool["status"].as_str().unwrap_or("?");
                    let healthy = pool["healthy"].as_bool().unwrap_or(false);
                    let mark = if healthy { "✓" } else { "✗" };
                    println!("  {mark} {name:<20} status:{status}");
                }
            }
        }
    } else {
        println!("[truenas] 미설정 (TRUENAS_URL / TRUENAS_API_KEY 필요)");
    }

    Ok(())
}

fn nas_shares() -> anyhow::Result<()> {
    println!("=== NAS 공유 목록 ===\n");

    // Synology shares
    if let Some(cfg) = SynologyConfig::load() {
        if let Ok(sid) = synology_login(&cfg) {
            println!("[synology] 공유 폴더:");
            if let Ok(data) = synology_api_call(&cfg, &sid, "SYNO.FileStation.List", "list_share", 2, "") {
                if let Some(shares) = data["shares"].as_array() {
                    for share in shares {
                        let name = share["name"].as_str().unwrap_or("?");
                        let path = share["additional"]["real_path"].as_str()
                            .or(share["path"].as_str())
                            .unwrap_or("?");
                        println!("  {name:<30} {path}");
                    }
                }
            }
            synology_logout(&cfg, &sid);
        }
    }

    // TrueNAS shares
    if let Some(cfg) = TrueNasConfig::load() {
        // NFS
        if let Ok(nfs) = truenas_api_get(&cfg, "/sharing/nfs") {
            if let Some(shares) = nfs.as_array() {
                if !shares.is_empty() {
                    println!("[truenas] NFS:");
                    for share in shares {
                        let path = share["path"].as_str().unwrap_or("?");
                        let enabled = share["enabled"].as_bool().unwrap_or(false);
                        let mark = if enabled { "✓" } else { "✗" };
                        let comment = share["comment"].as_str().unwrap_or("");
                        println!("  {mark} {path:<40} {comment}");
                    }
                }
            }
        }
        // SMB
        if let Ok(smb) = truenas_api_get(&cfg, "/sharing/smb") {
            if let Some(shares) = smb.as_array() {
                if !shares.is_empty() {
                    println!("[truenas] SMB:");
                    for share in shares {
                        let name = share["name"].as_str().unwrap_or("?");
                        let path = share["path"].as_str().unwrap_or("?");
                        let enabled = share["enabled"].as_bool().unwrap_or(false);
                        let mark = if enabled { "✓" } else { "✗" };
                        println!("  {mark} {name:<20} {path}");
                    }
                }
            }
        }
    }

    Ok(())
}

fn nas_pools() -> anyhow::Result<()> {
    println!("=== NAS 스토리지 풀 ===\n");

    // Synology volumes
    if let Some(cfg) = SynologyConfig::load() {
        if let Ok(sid) = synology_login(&cfg) {
            println!("[synology] 볼륨/스토리지 풀:");
            if let Ok(data) = synology_api_call(&cfg, &sid, "SYNO.Storage.CGI.Storage", "load_info", 1, "") {
                if let Some(volumes) = data["volumes"].as_array() {
                    for vol in volumes {
                        let name = vol["display_name"].as_str()
                            .or(vol["vol_path"].as_str())
                            .unwrap_or("?");
                        let status = vol["status"].as_str().unwrap_or("?");
                        let fs_type = vol["fs_type"].as_str().unwrap_or("?");
                        println!("  {name:<20} status:{status}  fs:{fs_type}");
                    }
                }
            }
            synology_logout(&cfg, &sid);
        }
    }

    // TrueNAS pools + datasets
    if let Some(cfg) = TrueNasConfig::load() {
        println!("[truenas] 스토리지 풀:");
        if let Ok(pools) = truenas_api_get(&cfg, "/pool") {
            if let Some(pools) = pools.as_array() {
                for pool in pools {
                    let name = pool["name"].as_str().unwrap_or("?");
                    let status = pool["status"].as_str().unwrap_or("?");
                    let vdevs = pool["topology"]["data"].as_array().map(|a| a.len()).unwrap_or(0);
                    println!("  {name:<20} status:{status}  vdevs:{vdevs}");
                }
            }
        }
        if let Ok(datasets) = truenas_api_get(&cfg, "/pool/dataset") {
            if let Some(datasets) = datasets.as_array() {
                println!("\n[truenas] 데이터셋:");
                for ds in datasets {
                    let name = ds["name"].as_str().unwrap_or("?");
                    let used_raw = ds["used"]["rawvalue"].as_str().unwrap_or("0");
                    let avail_raw = ds["available"]["rawvalue"].as_str().unwrap_or("0");
                    let used_gb = used_raw.parse::<u64>().unwrap_or(0) / 1_073_741_824;
                    let avail_gb = avail_raw.parse::<u64>().unwrap_or(0) / 1_073_741_824;
                    println!("  {name:<40} used:{used_gb}GB  avail:{avail_gb}GB");
                }
            }
        }
    }

    Ok(())
}

// ============================================================
// Synology-specific commands
// ============================================================

fn synology_list() -> anyhow::Result<()> {
    let cfg = SynologyConfig::load()
        .ok_or_else(|| anyhow::anyhow!("[synology] 미설정 (SYNOLOGY_URL / SYNOLOGY_USER 필요)"))?;
    let sid = synology_login(&cfg)?;

    println!("=== Synology 공유 폴더 ===\n");
    if let Ok(data) = synology_api_call(&cfg, &sid, "SYNO.FileStation.List", "list_share", 2, "") {
        if let Some(shares) = data["shares"].as_array() {
            for share in shares {
                let name = share["name"].as_str().unwrap_or("?");
                let path = share["additional"]["real_path"].as_str()
                    .or(share["path"].as_str())
                    .unwrap_or("?");
                println!("  {name:<30} {path}");
            }
            println!("\n  총 {}개", shares.len());
        }
    }

    synology_logout(&cfg, &sid);
    Ok(())
}

fn synology_sync() -> anyhow::Result<()> {
    let cfg = SynologyConfig::load()
        .ok_or_else(|| anyhow::anyhow!("[synology] 미설정 (SYNOLOGY_URL / SYNOLOGY_USER 필요)"))?;
    let sid = synology_login(&cfg)?;

    println!("=== Synology 동기화 상태 ===\n");

    // Try Active Backup task list
    match synology_api_call(&cfg, &sid, "SYNO.ActiveBackup.Overview", "list", 1, "") {
        Ok(data) => {
            if let Some(tasks) = data["tasks"].as_array() {
                for task in tasks {
                    let name = task["name"].as_str().unwrap_or("?");
                    let status = task["status"].as_str().unwrap_or("?");
                    let last_run = task["last_run_time"].as_str().unwrap_or("-");
                    println!("  {name:<30} status:{status}  last:{last_run}");
                }
            } else {
                println!("  (Active Backup 작업 없음 또는 미지원)");
            }
        }
        Err(_) => {
            println!("  (Active Backup 조회 실패 — 패키지 미설치이거나 API 미지원)");
        }
    }

    // Hyper Backup (if available)
    match synology_api_call(&cfg, &sid, "SYNO.Backup.Task", "list", 1, "") {
        Ok(data) => {
            if let Some(tasks) = data["task_list"].as_array().or(data["tasks"].as_array()) {
                println!("\n[Hyper Backup]:");
                for task in tasks {
                    let name = task["name"].as_str().unwrap_or("?");
                    let status = task["status"].as_str().unwrap_or("?");
                    println!("  {name:<30} {status}");
                }
            }
        }
        Err(_) => {
            // Hyper Backup not installed or API not supported — silently skip
        }
    }

    synology_logout(&cfg, &sid);
    Ok(())
}

fn synology_link() {
    println!("=== Synology QuickConnect ===\n");
    let qc_id = read_env("SYNOLOGY_QUICKCONNECT_ID");
    if qc_id.is_empty() {
        let url = read_env("SYNOLOGY_URL");
        if url.is_empty() {
            println!("  SYNOLOGY_URL / SYNOLOGY_QUICKCONNECT_ID 미설정");
        } else {
            println!("  직접 접속: {url}");
            println!("  (QuickConnect ID는 SYNOLOGY_QUICKCONNECT_ID로 설정)");
        }
    } else {
        println!("  QuickConnect: https://quickconnect.to/{qc_id}");
        let url = read_env("SYNOLOGY_URL");
        if !url.is_empty() {
            println!("  직접 접속:    {url}");
        }
    }
}

fn synology_create_share(name: &str, volume: &str, desc: &str) -> anyhow::Result<()> {
    let cfg = SynologyConfig::load()
        .ok_or_else(|| anyhow::anyhow!("[synology] 미설정"))?;
    let sid = synology_login(&cfg)?;

    println!("[synology] 공유 폴더 생성: {name} (볼륨: {volume})");
    let extra = format!(
        "&name={}&share_info=%7B%22vol_path%22%3A%22{}%22%2C%22name%22%3A%22{}%22%2C%22desc%22%3A%22{}%22%7D",
        urlenc(name), urlenc(volume), urlenc(name), urlenc(desc)
    );
    synology_api_call(&cfg, &sid, "SYNO.Core.Share", "create", 1, &extra)?;
    println!("[synology] 생성 완료: {name}");
    synology_logout(&cfg, &sid);
    Ok(())
}

fn synology_delete_share(name: &str) -> anyhow::Result<()> {
    let cfg = SynologyConfig::load()
        .ok_or_else(|| anyhow::anyhow!("[synology] 미설정"))?;
    let sid = synology_login(&cfg)?;

    // Check existence first
    let data = synology_api_call(&cfg, &sid, "SYNO.Core.Share", "list", 1, "")?;
    let exists = data["shares"].as_array()
        .map(|shares| shares.iter().any(|s| s["name"].as_str() == Some(name)))
        .unwrap_or(false);

    if !exists {
        synology_logout(&cfg, &sid);
        anyhow::bail!("[synology] 공유 폴더 '{name}' 없음");
    }

    println!("[synology] 공유 폴더 삭제: {name}");
    let extra = format!("&name={}", urlenc(name));
    synology_api_call(&cfg, &sid, "SYNO.Core.Share", "delete", 1, &extra)?;
    println!("[synology] 삭제 완료: {name}");
    synology_logout(&cfg, &sid);
    Ok(())
}

// ============================================================
// Misc
// ============================================================

fn read_env(key: &str) -> String {
    if let Ok(v) = std::env::var(key) {
        if !v.is_empty() {
            return v;
        }
    }
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

fn doctor() {
    println!("=== pxi-nas doctor ===");
    for (name, cmd) in &[
        ("mount", "mount"),
        ("mount.cifs (cifs-utils)", "mount.cifs"),
        ("mount.nfs (nfs-common)", "mount.nfs"),
        ("curl", "curl"),
    ] {
        println!("  {} {name}", if common::has_cmd(cmd) { "✓" } else { "✗" });
    }

    println!();
    let syn = SynologyConfig::load();
    println!("  Synology: {}", if syn.is_some() { "✓ 설정됨" } else { "✗ 미설정" });
    let tnas = TrueNasConfig::load();
    println!("  TrueNAS:  {}", if tnas.is_some() { "✓ 설정됨" } else { "✗ 미설정" });

    println!("\n필요시 설치: sudo apt install -y cifs-utils nfs-common");
}
