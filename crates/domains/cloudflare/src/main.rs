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
    /// 모든 enabled 도메인의 catch-all을 Worker로 전환
    EmailWorkerAttachAll {
        #[arg(long)]
        worker: String,
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
    let (email, key) = creds()?;
    match Cli::parse().cmd {
        Cmd::DnsAdd { domain, record_type, name, content, audience, proxied } => {
            let p = match (proxied, audience) {
                (Some(p), _) => p,
                (None, Some(Audience::Global)) => true,
                (None, Some(Audience::Kr)) | (None, Some(Audience::Internal)) => false,
                (None, None) => anyhow::bail!("--proxied 또는 --audience 중 하나 필수"),
            };
            dns_add(&email, &key, &domain, &record_type, &name, &content, p)
        }
        Cmd::EmailWorkerAttachAll { worker } => worker_attach_all(&email, &key, &worker),
        Cmd::Doctor => { doctor(); Ok(()) }
    }
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

fn worker_attach_all(email: &str, key: &str, worker: &str) -> anyhow::Result<()> {
    println!("=== Email Routing Worker 일괄 연결: {worker} ===");
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
