//! pxi-license — Keygen CE 라이선스 활성화/상태/해제/체크인/셀프업데이트.
//!
//! Endpoints hit (all at `${API}/v1/accounts/${ACCT}`):
//!   POST /licenses/actions/validate-key
//!   POST /machines
//!   POST /licenses/${id}/actions/check-in
//!   GET  /machines?license=...&limit=100
//!   DELETE /machines/${id}
//!   GET  /products/${code}/releases/actions/upgrade?current=X&channel=stable

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

// ---- Embedded configuration ------------------------------------------------

const KEYGEN_API_URL_DEFAULT: &str = "https://keygen.pxi.com";
const KEYGEN_ACCOUNT_ID_DEFAULT: &str = "f45727f0-dcc6-423f-9d14-0433293a46da";
const KEYGEN_PRODUCT_CODE: &str = "pxi";
const LICENSE_FILE: &str = "/etc/pxi/license.json";
const BYPASS_ENV: &str = "PRELIK_LICENSE_BYPASS";

fn api_url() -> String {
    std::env::var("KEYGEN_API_URL").unwrap_or_else(|_| KEYGEN_API_URL_DEFAULT.to_string())
}

fn account_id() -> String {
    std::env::var("KEYGEN_ACCOUNT_ID").unwrap_or_else(|_| KEYGEN_ACCOUNT_ID_DEFAULT.to_string())
}

fn base() -> String {
    format!(
        "{}/v1/accounts/{}",
        api_url().trim_end_matches('/'),
        account_id()
    )
}

fn product_id() -> String {
    std::env::var("KEYGEN_PRODUCT_CODE").unwrap_or_else(|_| KEYGEN_PRODUCT_CODE.to_string())
}

// ---- CLI -------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "pxi-license", about = "라이선스 관리 (Keygen CE)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 라이선스 키로 이 기기 활성화
    Activate { key: String },
    /// 현재 라이선스 상태
    Status,
    /// 이 기기 활성화 해제
    Deactivate,
    /// 서버에 heartbeat 전송
    CheckIn,
    /// 라이선스 환경 진단
    Doctor,
    /// 바이너리 셀프 업데이트 (SHA-512 검증)
    SelfUpdate {
        /// 릴리스 채널 (stable, beta, ...)
        #[arg(long, default_value = "stable")]
        channel: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Activate { key } => cmd_activate(&key),
        Cmd::Status => cmd_status(),
        Cmd::Deactivate => cmd_deactivate(),
        Cmd::CheckIn => cmd_check_in(),
        Cmd::Doctor => { doctor(); Ok(()) }
        Cmd::SelfUpdate { channel } => cmd_self_update(&channel),
    }
}

// ---- Local storage ---------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone)]
struct StoredLicense {
    key: String,
    license_id: Option<String>,
    machine_id: Option<String>,
    activated_at: Option<String>,
    fingerprint: Option<String>,
}

fn stored_path() -> PathBuf {
    PathBuf::from(LICENSE_FILE)
}

fn load_stored() -> Option<StoredLicense> {
    let p = stored_path();
    if !p.exists() {
        return None;
    }
    let s = fs::read_to_string(&p).ok()?;
    serde_json::from_str(&s).ok()
}

fn save_stored(lic: &StoredLicense) -> std::io::Result<()> {
    let p = stored_path();
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut f = fs::File::create(&p)?;
    f.write_all(serde_json::to_string_pretty(lic).unwrap().as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

// ---- Fingerprint -----------------------------------------------------------

fn machine_fingerprint() -> String {
    if let Ok(s) = fs::read_to_string("/etc/machine-id") {
        let t = s.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    if let Ok(s) = fs::read_to_string("/var/lib/dbus/machine-id") {
        let t = s.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn machine_name() -> String {
    Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "pxi-host".to_string())
}

// ---- HTTP helpers ----------------------------------------------------------

fn get_json(url: &str, auth: Option<&str>) -> anyhow::Result<Value> {
    let mut req = ureq::get(url).set("Accept", "application/vnd.api+json");
    if let Some(a) = auth {
        req = req.set("Authorization", a);
    }
    let resp = req.call()?;
    let body: Value = resp.into_json()?;
    Ok(body)
}

fn post_json(url: &str, auth: Option<&str>, body: Value) -> anyhow::Result<Value> {
    let mut req = ureq::post(url)
        .set("Accept", "application/vnd.api+json")
        .set("Content-Type", "application/vnd.api+json");
    if let Some(a) = auth {
        req = req.set("Authorization", a);
    }
    let resp = req.send_json(body)?;
    let j: Value = resp.into_json().unwrap_or(Value::Null);
    Ok(j)
}

fn delete_url(url: &str, auth: Option<&str>) -> anyhow::Result<()> {
    let mut req = ureq::delete(url).set("Accept", "application/vnd.api+json");
    if let Some(a) = auth {
        req = req.set("Authorization", a);
    }
    req.call()?;
    Ok(())
}

// ---- License operations ----------------------------------------------------

fn validate_key(key: &str, fingerprint: Option<&str>) -> anyhow::Result<Value> {
    let url = format!("{}/licenses/actions/validate-key", base());
    let body = if let Some(fp) = fingerprint {
        json!({
            "meta": {
                "key": key,
                "scope": { "fingerprint": fp, "product": product_id() }
            }
        })
    } else {
        json!({ "meta": { "key": key } })
    };
    post_json(&url, None, body)
}

fn cmd_activate(key: &str) -> anyhow::Result<()> {
    let fp = machine_fingerprint();
    let name = machine_name();

    println!("라이선스 키 검증 중...");
    let v = validate_key(key, Some(&fp))?;
    let valid = v["meta"]["valid"].as_bool().unwrap_or(false);
    let detail = v["meta"]["detail"].as_str().unwrap_or("");
    let code = v["meta"]["code"].as_str().unwrap_or("");
    let lic = v["data"].as_object();

    if !valid
        && code != "FINGERPRINT_SCOPE_MISMATCH"
        && code != "NO_MACHINE"
        && code != "NO_MACHINES"
    {
        anyhow::bail!("유효하지 않은 라이선스: {} ({})", detail, code);
    }

    let license_id = lic
        .and_then(|o| o.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("라이선스 id 확인 실패"))?
        .to_string();

    println!(
        "기기 활성화 중... (fingerprint: {}...)",
        &fp[..fp.len().min(16)]
    );
    let url = format!("{}/machines", base());
    let body = json!({
        "data": {
            "type": "machines",
            "attributes": {
                "fingerprint": fp,
                "name": name,
                "platform": std::env::consts::OS
            },
            "relationships": {
                "license": {
                    "data": { "type": "licenses", "id": license_id }
                }
            }
        }
    });
    let auth = format!("License {}", key);

    let resp = match post_json(&url, Some(&auth), body) {
        Ok(r) => r,
        Err(e) => {
            let s = format!("{}", e);
            if s.contains("FINGERPRINT_TAKEN") || s.contains("already") {
                println!("이미 이 기기에 활성화되어 있습니다.");
                let stored = StoredLicense {
                    key: key.to_string(),
                    license_id: Some(license_id),
                    machine_id: None,
                    activated_at: None,
                    fingerprint: Some(fp),
                };
                save_stored(&stored)?;
                return Ok(());
            }
            return Err(e);
        }
    };

    let machine_id = resp["data"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let activated_at = resp["data"]["attributes"]["created"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let stored = StoredLicense {
        key: key.to_string(),
        license_id: Some(license_id),
        machine_id: Some(machine_id.clone()),
        activated_at: Some(activated_at),
        fingerprint: Some(fp),
    };
    save_stored(&stored)?;

    println!("✓ 활성화 완료");
    println!("  저장됨: {}", LICENSE_FILE);
    println!("  기기 id: {}", machine_id);
    Ok(())
}

fn cmd_status() -> anyhow::Result<()> {
    if std::env::var(BYPASS_ENV).is_ok() {
        println!("[{}] 활성 — 라이선스 검증 우회 중", BYPASS_ENV);
        return Ok(());
    }

    // Internal dalsoop hosts: implicit bypass
    if Path::new("/etc/dalsoop-internal").exists() {
        println!("[dalsoop-internal] 내부 호스트 — 라이선스 우회");
        return Ok(());
    }

    let stored = match load_stored() {
        Some(s) => s,
        None => {
            println!(
                "저장된 라이선스 없음. `pxi-license activate <KEY>` 로 활성화하세요."
            );
            return Ok(());
        }
    };

    let fp = machine_fingerprint();
    let v = validate_key(&stored.key, Some(&fp))?;
    let valid = v["meta"]["valid"].as_bool().unwrap_or(false);
    let code = v["meta"]["code"].as_str().unwrap_or("");
    let detail = v["meta"]["detail"].as_str().unwrap_or("");
    let attrs = &v["data"]["attributes"];
    let status = attrs["status"].as_str().unwrap_or("?");
    let expiry = attrs["expiry"].as_str().unwrap_or("(평생)");
    let max_machines = attrs["maxMachines"].as_i64().unwrap_or(-1);
    let uses = attrs["uses"].as_i64().unwrap_or(0);

    println!(
        "라이선스 상태: {}",
        if valid { "✓ 유효" } else { "✗ 검증 실패" }
    );
    println!("  상태: {}", status);
    println!("  만료: {}", expiry);
    println!(
        "  활성 기기: {} / {}",
        uses,
        if max_machines < 0 {
            "무제한".into()
        } else {
            max_machines.to_string()
        }
    );
    if !valid {
        println!("  사유: {} ({})", detail, code);
    }

    // List machines
    if let Some(license_id) = stored.license_id.as_deref() {
        let url = format!("{}/machines?license={}&limit=100", base(), license_id);
        let auth = format!("License {}", stored.key);
        if let Ok(resp) = get_json(&url, Some(&auth)) {
            let machines = resp["data"].as_array().cloned().unwrap_or_default();
            if !machines.is_empty() {
                println!("  기기 목록:");
                for m in &machines {
                    let name = m["attributes"]["name"].as_str().unwrap_or("");
                    let hostname = m["attributes"]["hostname"].as_str().unwrap_or("");
                    let mfp = m["attributes"]["fingerprint"].as_str().unwrap_or("");
                    let last = m["attributes"]["lastHeartbeat"]
                        .as_str()
                        .unwrap_or("-");
                    let platform = m["attributes"]["platform"].as_str().unwrap_or("");
                    let display_name = if name.is_empty() { hostname } else { name };
                    println!(
                        "    · {} ({}...) {} · last heartbeat {}",
                        display_name,
                        &mfp[..mfp.len().min(12)],
                        platform,
                        last
                    );
                }
            }
        }
    }
    Ok(())
}

fn cmd_deactivate() -> anyhow::Result<()> {
    let stored =
        load_stored().ok_or_else(|| anyhow::anyhow!("저장된 라이선스 없음"))?;
    let fp = machine_fingerprint();
    let license_id = stored
        .license_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("license_id 없음, validate 필요"))?;

    // Find machine by fingerprint
    let url = format!("{}/machines?license={}&limit=100", base(), license_id);
    let auth = format!("License {}", stored.key);
    let resp = get_json(&url, Some(&auth))?;
    let machines = resp["data"].as_array().cloned().unwrap_or_default();
    let m = machines
        .iter()
        .find(|m| m["attributes"]["fingerprint"].as_str() == Some(&fp));

    let mid = match m {
        Some(m) => m["data"]
            .as_str()
            .or_else(|| m["id"].as_str())
            .unwrap_or_default()
            .to_string(),
        None => {
            println!("이 기기의 활성화 기록 없음 (이미 해제됨)");
            fs::remove_file(LICENSE_FILE).ok();
            return Ok(());
        }
    };

    let del_url = format!("{}/machines/{}", base(), mid);
    delete_url(&del_url, Some(&auth))?;
    fs::remove_file(LICENSE_FILE).ok();
    println!("✓ 비활성화 완료, 로컬 라이선스 파일 제거");
    Ok(())
}

fn cmd_check_in() -> anyhow::Result<()> {
    let stored =
        load_stored().ok_or_else(|| anyhow::anyhow!("활성화되지 않음"))?;
    let license_id = stored
        .license_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("license_id 없음"))?;
    let url = format!(
        "{}/licenses/{}/actions/check-in",
        base(),
        license_id
    );
    let auth = format!("License {}", stored.key);
    post_json(&url, Some(&auth), json!({}))?;
    println!("✓ check-in");
    Ok(())
}

// ---- Self-update -----------------------------------------------------------

fn cmd_self_update(channel: &str) -> anyhow::Result<()> {
    let stored = load_stored().ok_or_else(|| {
        anyhow::anyhow!("활성화되지 않음 — `pxi-license activate <KEY>` 먼저")
    })?;
    let auth = format!("License {}", stored.key);
    let current = env!("CARGO_PKG_VERSION");

    let url = format!(
        "{}/products/{}/releases/actions/upgrade?current={}&channel={}",
        base(),
        product_id(),
        current,
        channel,
    );
    println!("업그레이드 확인 (현재 v{}, 채널 {})...", current, channel);

    let resp = match get_json(&url, Some(&auth)) {
        Ok(r) => r,
        Err(e) => {
            let s = format!("{}", e);
            if s.contains("204") || s.contains("NO_UPGRADE") {
                println!("✓ 이미 최신 버전입니다.");
                return Ok(());
            }
            return Err(e);
        }
    };

    let next = &resp["data"];
    let next_version = next["attributes"]["version"].as_str().unwrap_or("?");
    let release_id = next["id"].as_str().unwrap_or("");
    println!("새 버전 발견: v{}", next_version);

    // Pick the artifact matching current platform/arch.
    let want_os = std::env::consts::OS;
    let want_arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        a => a,
    };

    let art_url = format!(
        "{}/releases/{}/artifacts?limit=100",
        base(),
        release_id
    );
    let art_resp = get_json(&art_url, Some(&auth))?;
    let arts = art_resp["data"].as_array().cloned().unwrap_or_default();
    let art = arts
        .iter()
        .find(|a| {
            a["attributes"]["platform"].as_str() == Some(want_os)
                && a["attributes"]["arch"].as_str() == Some(want_arch)
        })
        .ok_or_else(|| {
            anyhow::anyhow!("{}-{} 용 아티팩트 없음", want_os, want_arch)
        })?;

    let art_id = art["id"].as_str().unwrap_or_default().to_string();
    let filename = art["attributes"]["filename"]
        .as_str()
        .unwrap_or("pxi")
        .to_string();
    let expected_sha512_b64 = art["attributes"]["checksum"]
        .as_str()
        .map(|s| s.to_string());

    // Download (Keygen 303 -> signed URL). ureq follows redirects by default.
    let dl_url = format!("{}/artifacts/{}", base(), art_id);
    println!("다운로드 중... ({})", filename);
    let resp = ureq::get(&dl_url)
        .set("Authorization", auth.as_str())
        .set("Accept", "application/octet-stream")
        .call()?;
    let mut bytes = Vec::new();
    resp.into_reader().read_to_end(&mut bytes)?;
    println!("  {} 바이트 수신", bytes.len());

    // Verify SHA-512 if provided.
    if let Some(exp) = expected_sha512_b64 {
        let actual = sha512_b64(&bytes);
        if actual != exp {
            anyhow::bail!("체크섬 불일치 — 설치 중단");
        }
        println!("  ✓ SHA-512 검증 통과");
    }

    // Atomic replace of current binary.
    let current_exe = std::env::current_exe()?;
    let tmp = current_exe.with_extension("new");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
        }
    }
    fs::rename(&tmp, &current_exe)?;
    println!(
        "✓ v{} 설치 완료 ({})",
        next_version,
        current_exe.display()
    );
    Ok(())
}

// ---- Minimal SHA-512 -------------------------------------------------------
// openssl-cli via subprocess — keeps the dep tree small.

fn sha512_b64(bytes: &[u8]) -> String {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new("openssl")
        .args(["dgst", "-sha512", "-binary"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("openssl required for checksum verify");
    child.stdin.as_mut().unwrap().write_all(bytes).unwrap();
    let out = child.wait_with_output().expect("openssl failed");
    base64_encode(&out.stdout)
}

fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let b = (bytes[i] as u32) << 16
            | (bytes[i + 1] as u32) << 8
            | bytes[i + 2] as u32;
        out.push(T[((b >> 18) & 63) as usize] as char);
        out.push(T[((b >> 12) & 63) as usize] as char);
        out.push(T[((b >> 6) & 63) as usize] as char);
        out.push(T[(b & 63) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let b = (bytes[i] as u32) << 16;
        out.push(T[((b >> 18) & 63) as usize] as char);
        out.push(T[((b >> 12) & 63) as usize] as char);
        out.push_str("==");
    } else if rem == 2 {
        let b = (bytes[i] as u32) << 16 | (bytes[i + 1] as u32) << 8;
        out.push(T[((b >> 18) & 63) as usize] as char);
        out.push(T[((b >> 12) & 63) as usize] as char);
        out.push(T[((b >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}

// ---- Doctor ----------------------------------------------------------------

fn doctor() {
    println!("=== pxi-license doctor ===\n");

    // License file exists
    let lic_exists = Path::new(LICENSE_FILE).exists();
    println!("  {} {} 존재", if lic_exists { "✓" } else { "✗" }, LICENSE_FILE);

    // Keygen API reachable
    let api = api_url();
    let api_ok = Command::new("curl")
        .args(["-sf", "--max-time", "5", &format!("{}/v1/ping", api)])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    println!("  {} Keygen API ({})", if api_ok { "✓" } else { "✗" }, api);

    // If license exists, validate online
    if lic_exists {
        if let Some(stored) = load_stored() {
            let fp = machine_fingerprint();
            match validate_key(&stored.key, Some(&fp)) {
                Ok(v) => {
                    let valid = v["meta"]["valid"].as_bool().unwrap_or(false);
                    let code = v["meta"]["code"].as_str().unwrap_or("?");
                    if valid {
                        println!("  ✓ 라이선스 온라인 검증 통과");
                    } else {
                        println!("  ✗ 라이선스 검증 실패 ({})", code);
                    }
                }
                Err(e) => println!("  ✗ 라이선스 검증 요청 실패: {}", e),
            }
        } else {
            println!("  ✗ 라이선스 파일 파싱 실패");
        }
    }
}

// ---- Enforcement -----------------------------------------------------------

/// Call from critical subcommand entry points. Cheap local check that a
/// license file exists. Online validation is a separate `pxi-license status`
/// call.
#[allow(dead_code)]
fn require_licensed_or_bypass() -> anyhow::Result<()> {
    if std::env::var(BYPASS_ENV).is_ok() {
        return Ok(());
    }
    if Path::new("/etc/dalsoop-internal").exists() {
        return Ok(());
    }
    if load_stored().is_none() {
        anyhow::bail!(
            "라이선스가 활성화되지 않았습니다. 'pxi-license activate <KEY>' 먼저 실행하세요.\n\
             (dev 환경에서는 환경변수 {}=1 로 우회 가능)",
            BYPASS_ENV
        );
    }
    Ok(())
}
