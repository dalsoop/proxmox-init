//! prelik-cloudflare — CF API 래퍼
//! - DNS: add/update/list/delete (audience 기반 proxied 자동)
//! - Email Routing: worker-attach (catch-all → Worker)

use clap::{Parser, Subcommand, ValueEnum};
use prelik_core::common;
use serde_json::Value;
use std::fs;

#[derive(Parser)]
#[command(name = "prelik-cloudflare", about = "Cloudflare DNS + Email Routing")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// DNS 레코드 추가 (--audience 기반 proxied 자동)
    DnsAdd {
        #[arg(long)]
        domain: String,
        #[arg(long = "type")]
        record_type: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        content: String,
        #[arg(long, value_enum)]
        audience: Option<Audience>,
        #[arg(long)]
        proxied: Option<bool>,
    },
    /// DNS 레코드 목록
    DnsList {
        #[arg(long)]
        domain: String,
    },
    /// DNS 레코드 수정
    DnsUpdate {
        #[arg(long)]
        domain: String,
        #[arg(long = "type")]
        record_type: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        content: String,
        #[arg(long)]
        proxied: Option<bool>,
    },
    /// DNS 레코드 삭제
    DnsDelete {
        #[arg(long)]
        domain: String,
        #[arg(long = "type")]
        record_type: String,
        #[arg(long)]
        name: String,
    },
    /// 모든 enabled 도메인의 catch-all을 Worker로 전환
    EmailWorkerAttachAll {
        #[arg(long)]
        worker: String,
        /// 실제 변경 없이 대상 목록만 출력
        #[arg(long)]
        dry_run: bool,
    },
    /// SSL 인증서 발급 (Let's Encrypt + CF DNS-01 챌린지, acme.sh 필요)
    SslIssue {
        #[arg(long)]
        domain: String,
        /// 와일드카드 포함 (*.domain)
        #[arg(long)]
        wildcard: bool,
    },
    /// SSL 인증서 갱신
    SslRenew {
        #[arg(long)]
        domain: String,
    },
    /// Cloudflare Pages에 정적 사이트 배포 (wrangler 래퍼)
    PagesDeploy {
        /// 프로젝트 이름
        #[arg(long)]
        project: String,
        /// 배포할 디렉토리
        #[arg(long)]
        directory: String,
    },
    Doctor,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Audience {
    Global,
    Kr,
    Internal,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    // doctor + pages-deploy는 CF API 키 불필요 (wrangler는 자체 인증)
    match &cli.cmd {
        Cmd::Doctor => { doctor(); return Ok(()); }
        Cmd::PagesDeploy { project, directory } => {
            return pages_deploy(project, directory);
        }
        _ => {}
    }
    let (email, key) = creds()?;
    match cli.cmd {
        Cmd::DnsAdd { domain, record_type, name, content, audience, proxied } => {
            let p = match (proxied, audience) {
                (Some(p), _) => p,
                (None, Some(Audience::Global)) => true,
                (None, Some(Audience::Kr)) | (None, Some(Audience::Internal)) => false,
                (None, None) => anyhow::bail!("--proxied 또는 --audience 중 하나 필수"),
            };
            dns_add(&email, &key, &domain, &record_type, &name, &content, p)
        }
        Cmd::DnsList { domain } => dns_list(&email, &key, &domain),
        Cmd::DnsUpdate { domain, record_type, name, content, proxied } => {
            dns_update(&email, &key, &domain, &record_type, &name, &content, proxied)
        }
        Cmd::DnsDelete { domain, record_type, name } => {
            dns_delete(&email, &key, &domain, &record_type, &name)
        }
        Cmd::EmailWorkerAttachAll { worker, dry_run } => worker_attach_all(&email, &key, &worker, dry_run),
        Cmd::SslIssue { domain, wildcard } => ssl_issue(&email, &key, &domain, wildcard),
        Cmd::SslRenew { domain } => ssl_renew(&domain),
        Cmd::Doctor | Cmd::PagesDeploy { .. } => unreachable!("위에서 early return"),
    }
}

fn pages_deploy(project: &str, directory: &str) -> anyhow::Result<()> {
    println!("=== CF Pages 배포: {project} ← {directory} ===");
    if !std::path::Path::new(directory).exists() {
        anyhow::bail!("디렉토리 없음: {directory}");
    }
    if !common::has_cmd("wrangler") {
        anyhow::bail!(
            "wrangler 미설치. 설치: npm install -g wrangler\n\
             또는 npx wrangler pages deploy {directory} --project-name={project}"
        );
    }
    let status = std::process::Command::new("wrangler")
        .args(["pages", "deploy", directory, "--project-name", project])
        .status()?;
    if !status.success() {
        anyhow::bail!("wrangler 배포 실패 (exit: {})", status.code().unwrap_or(-1));
    }
    println!("✓ 배포 완료");
    Ok(())
}

/// acme.sh 실제 경로 찾기.
/// 1) PATH에 있음, 2) ~/.acme.sh/acme.sh (표준 설치),
/// 3) /root/.acme.sh/acme.sh (sudo 설치)
fn find_acme_sh() -> Option<String> {
    if common::has_cmd("acme.sh") {
        return Some("acme.sh".to_string());
    }
    let candidates = [
        dirs::home_dir().map(|h| h.join(".acme.sh/acme.sh")),
        Some(std::path::PathBuf::from("/root/.acme.sh/acme.sh")),
    ];
    for c in candidates.iter().flatten() {
        if c.exists() {
            return Some(c.display().to_string());
        }
    }
    None
}

fn ssl_issue(email: &str, key: &str, domain: &str, wildcard: bool) -> anyhow::Result<()> {
    println!("=== SSL 발급: {domain} (wildcard: {wildcard}) ===");
    let acme = find_acme_sh().ok_or_else(|| anyhow::anyhow!(
        "acme.sh 미설치. 설치: curl https://get.acme.sh | sh\n\
         확인된 경로: PATH, ~/.acme.sh/, /root/.acme.sh/"
    ))?;
    println!("  acme.sh: {acme}");
    // CF credentials env로 acme.sh에 전달
    // wildcard: -d *.domain -d domain 둘 다 필요
    let mut args: Vec<String> = vec!["--issue".into(), "--dns".into(), "dns_cf".into()];
    if wildcard {
        args.push("-d".into());
        args.push(format!("*.{domain}"));
    }
    args.push("-d".into());
    args.push(domain.to_string());

    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let output = std::process::Command::new(&acme)
        .args(&args_ref)
        .env("CF_Email", email)
        .env("CF_Key", key)
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "acme.sh 발급 실패: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    println!("✓ 인증서 발급 완료 — ~/.acme.sh/{domain}/ 확인");
    println!("  설치: acme.sh --install-cert -d {domain} --key-file <path> --fullchain-file <path>");
    Ok(())
}

fn ssl_renew(domain: &str) -> anyhow::Result<()> {
    println!("=== SSL 갱신: {domain} ===");
    let acme = find_acme_sh().ok_or_else(|| anyhow::anyhow!("acme.sh 미설치"))?;
    let output = std::process::Command::new(&acme)
        .args(["--renew", "-d", domain, "--force"])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("갱신 실패: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    println!("✓ 갱신 완료");
    Ok(())
}

fn creds() -> anyhow::Result<(String, String)> {
    let email = read_host_env("CLOUDFLARE_EMAIL");
    let key = read_host_env("CLOUDFLARE_API_KEY");
    if email.is_empty() || key.is_empty() {
        anyhow::bail!("/etc/prelik/.env 에 CLOUDFLARE_EMAIL / CLOUDFLARE_API_KEY 필요");
    }
    Ok((email, key))
}

fn cf_api(email: &str, key: &str, method: &str, path: &str, body: Option<&str>) -> anyhow::Result<Value> {
    let url = format!("https://api.cloudflare.com/client/v4{path}");
    let mut args: Vec<String> = vec![
        "-sSL".into(), "--fail".into(),
        "-X".into(), method.into(),
        "-H".into(), format!("X-Auth-Email: {email}"),
        "-H".into(), format!("X-Auth-Key: {key}"),
        "-H".into(), "Content-Type: application/json".into(),
    ];
    if let Some(b) = body {
        args.push("-d".into());
        args.push(b.into());
    }
    args.push(url);
    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let out = common::run("curl", &args_ref)?;
    let v: Value = serde_json::from_str(&out)?;
    if !v["success"].as_bool().unwrap_or(false) {
        anyhow::bail!("CF API 실패: {}", v["errors"]);
    }
    Ok(v["result"].clone())
}

fn zone_id(email: &str, key: &str, domain: &str) -> anyhow::Result<String> {
    let r = cf_api(email, key, "GET", &format!("/zones?name={domain}"), None)?;
    r[0]["id"].as_str().map(String::from)
        .ok_or_else(|| anyhow::anyhow!("zone {domain} 없음"))
}

fn dns_add(email: &str, key: &str, domain: &str, rec_type: &str, name: &str, content: &str, proxied: bool) -> anyhow::Result<()> {
    let zid = zone_id(email, key, domain)?;
    let body = serde_json::json!({
        "type": rec_type, "name": name, "content": content, "proxied": proxied, "ttl": 1
    });
    cf_api(email, key, "POST", &format!("/zones/{zid}/dns_records"), Some(&body.to_string()))?;
    println!("✓ DNS 추가: {rec_type} {name} → {content} (proxied={proxied})");
    Ok(())
}

fn dns_list(email: &str, key: &str, domain: &str) -> anyhow::Result<()> {
    let zid = zone_id(email, key, domain)?;
    let records = cf_api(email, key, "GET", &format!("/zones/{zid}/dns_records?per_page=100"), None)?;
    let Some(arr) = records.as_array() else { anyhow::bail!("dns_records 응답 파싱 실패") };
    println!("=== DNS 레코드: {domain} ({}) ===", arr.len());
    for r in arr {
        let t = r["type"].as_str().unwrap_or("?");
        let n = r["name"].as_str().unwrap_or("?");
        let c = r["content"].as_str().unwrap_or("?");
        let p = r["proxied"].as_bool().unwrap_or(false);
        let icon = if p { "☁" } else { "→" };
        println!("  {t:<8} {n:<40} {icon} {c}");
    }
    Ok(())
}

/// 기존 레코드 찾기 — type + name 매칭
fn find_record(email: &str, key: &str, zid: &str, rec_type: &str, name: &str) -> anyhow::Result<String> {
    let records = cf_api(email, key, "GET", &format!("/zones/{zid}/dns_records?type={rec_type}&name={name}"), None)?;
    let Some(arr) = records.as_array() else { anyhow::bail!("응답 파싱 실패") };
    let Some(first) = arr.first() else {
        anyhow::bail!("레코드 없음: {rec_type} {name}")
    };
    first["id"].as_str().map(String::from)
        .ok_or_else(|| anyhow::anyhow!("record id 없음"))
}

fn dns_update(email: &str, key: &str, domain: &str, rec_type: &str, name: &str, content: &str, proxied: Option<bool>) -> anyhow::Result<()> {
    let zid = zone_id(email, key, domain)?;
    let rid = find_record(email, key, &zid, rec_type, name)?;
    let mut body = serde_json::json!({
        "type": rec_type, "name": name, "content": content
    });
    if let Some(p) = proxied {
        body["proxied"] = serde_json::json!(p);
    }
    cf_api(email, key, "PUT", &format!("/zones/{zid}/dns_records/{rid}"), Some(&body.to_string()))?;
    println!("✓ DNS 수정: {rec_type} {name} → {content}");
    Ok(())
}

fn dns_delete(email: &str, key: &str, domain: &str, rec_type: &str, name: &str) -> anyhow::Result<()> {
    let zid = zone_id(email, key, domain)?;
    let rid = find_record(email, key, &zid, rec_type, name)?;
    cf_api(email, key, "DELETE", &format!("/zones/{zid}/dns_records/{rid}"), None)?;
    println!("✓ DNS 삭제: {rec_type} {name}");
    Ok(())
}

fn worker_attach_all(email: &str, key: &str, worker: &str, dry_run: bool) -> anyhow::Result<()> {
    println!(
        "=== Email Routing Worker 일괄 연결: {worker} {}===",
        if dry_run { "[DRY-RUN] " } else { "" }
    );
    let zones = cf_api(email, key, "GET", "/zones?per_page=50", None)?;
    let Some(arr) = zones.as_array() else { anyhow::bail!("zones 응답 파싱 실패") };

    let mut attached = 0;
    let mut skipped = 0;
    let mut failed: Vec<String> = vec![];

    for z in arr {
        let Some(zid) = z["id"].as_str() else { continue };
        let Some(zname) = z["name"].as_str() else { continue };

        // Email Routing 상태 — 에러와 "비활성화"를 반드시 구분
        match cf_api(email, key, "GET", &format!("/zones/{zid}/email/routing"), None) {
            Ok(r) => {
                if !r["enabled"].as_bool().unwrap_or(false) {
                    println!("  ⊘ {zname}: Email Routing 비활성화");
                    skipped += 1;
                    continue;
                }
            }
            Err(e) => {
                // 401/403/429 등 — 진단 가능하게 노출
                println!("  ⚠ {zname}: routing 상태 조회 실패 ({e})");
                failed.push(format!("{zname}: routing-check {e}"));
                continue;
            }
        }

        if dry_run {
            match cf_api(email, key, "GET", &format!("/zones/{zid}/email/routing/rules/catch_all"), None) {
                Ok(v) => {
                    let current_action = v["actions"].as_array()
                        .map(|a| a.iter().map(|x| x["type"].as_str().unwrap_or("?").to_string()).collect::<Vec<_>>().join(","))
                        .unwrap_or_else(|| "(없음)".into());
                    println!("  ⤳ {zname}: 현재 catch-all=[{current_action}] → worker:{worker}");
                    attached += 1;
                }
                Err(e) => {
                    // read 실패는 명시적으로 노출 (403/429 등 진짜 문제 숨기지 않음)
                    println!("  ⚠ {zname}: 현재 catch-all 조회 실패 ({e}) — 미리보기 신뢰 불가");
                    failed.push(format!("{zname}: dry-run read {e}"));
                }
            }
            continue;
        }

        let body = serde_json::json!({
            "matchers": [{"type": "all"}],
            "actions": [{"type": "worker", "value": [worker]}],
            "enabled": true
        });
        match cf_api(email, key, "PUT", &format!("/zones/{zid}/email/routing/rules/catch_all"), Some(&body.to_string())) {
            Ok(_) => { println!("  ✓ {zname}"); attached += 1; }
            Err(e) => {
                println!("  ✗ {zname}: {e}");
                failed.push(format!("{zname}: attach {e}"));
            }
        }
    }
    println!("\n결과: 연결 {attached}, 건너뜀 {skipped}, 실패 {}", failed.len());
    if !failed.is_empty() {
        for f in &failed {
            eprintln!("  FAIL: {f}");
        }
        anyhow::bail!("일부 도메인 실패 ({}개)", failed.len());
    }
    Ok(())
}

fn doctor() {
    println!("=== prelik-cloudflare doctor ===");
    let email = read_host_env("CLOUDFLARE_EMAIL");
    let key = read_host_env("CLOUDFLARE_API_KEY");
    println!("  CF_EMAIL:   {}", if !email.is_empty() { "✓" } else { "✗" });
    println!("  CF_API_KEY: {}", if !key.is_empty() { "✓" } else { "✗" });
    println!("  curl:       {}", if common::has_cmd("curl") { "✓" } else { "✗" });
}

fn read_host_env(key: &str) -> String {
    for p in ["/etc/prelik/.env", "/etc/proxmox-host-setup/.env"] {
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
