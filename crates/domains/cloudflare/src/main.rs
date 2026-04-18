//! pxi-cloudflare — CF API 래퍼
//! - DNS: add/update/upsert/list/delete (audience 기반 proxied 자동)
//! - Email Routing: status, forward, forward-all, worker-attach
//! - SSL: install (acme.sh), issue, renew, list, status
//! - Pages: list, create, domain, delete, deploy

use clap::{Parser, Subcommand, ValueEnum};
use pxi_core::common;
use serde_json::Value;
use std::fs;

#[derive(Parser)]
#[command(name = "pxi-cloudflare", about = "Cloudflare DNS + Email Routing + SSL + Pages")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    // ---- DNS ----

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
    /// DNS 레코드 upsert (없으면 생성, 있으면 수정)
    DnsUpsert {
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
        #[arg(long, default_value = "0")]
        ttl: u64,
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

    // ---- Zones ----

    /// Cloudflare 도메인(zone) 목록
    Zones,

    // ---- Email Routing ----

    /// 이메일 라우팅 상태 (전 도메인)
    EmailStatus,
    /// 단일 도메인 이메일 포워딩 (catch-all → destination)
    EmailForward {
        #[arg(long)]
        domain: String,
        #[arg(long)]
        destination: String,
    },
    /// 모든 도메인에 catch-all 이메일 포워딩 설정
    EmailForwardAll {
        #[arg(long)]
        destination: String,
    },
    /// 단일 도메인의 catch-all을 CF Email Worker로 전환
    EmailWorkerAttach {
        #[arg(long)]
        domain: String,
        #[arg(long)]
        worker: String,
    },
    /// 모든 enabled 도메인의 catch-all을 Worker로 전환
    EmailWorkerAttachAll {
        #[arg(long)]
        worker: String,
        /// 실제 변경 없이 대상 목록만 출력
        #[arg(long)]
        dry_run: bool,
    },

    // ---- SSL ----

    /// acme.sh 설치
    SslInstall,
    /// SSL 인증서 발급 (Let's Encrypt + CF DNS-01 챌린지)
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
    /// SSL 인증서 목록
    SslList,
    /// SSL 상태 개요
    SslStatus,

    // ---- Pages ----

    /// Pages 프로젝트 목록
    PagesList,
    /// Pages 프로젝트 생성
    PagesCreate {
        #[arg(long)]
        name: String,
    },
    /// Pages 커스텀 도메인 설정 + DNS CNAME 자동
    PagesDomain {
        #[arg(long)]
        project: String,
        #[arg(long)]
        domain: String,
    },
    /// Pages 프로젝트 삭제
    PagesDelete {
        #[arg(long)]
        project: String,
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

    // ---- Misc ----

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

    // doctor + pages-deploy는 CF API 키 불필요
    match &cli.cmd {
        Cmd::Doctor => {
            doctor();
            return Ok(());
        }
        Cmd::PagesDeploy { project, directory } => {
            return pages_deploy(project, directory);
        }
        Cmd::SslInstall => return ssl_install(),
        Cmd::SslList => {
            ssl_list();
            return Ok(());
        }
        Cmd::SslStatus => {
            ssl_status();
            return Ok(());
        }
        Cmd::SslRenew { domain } => return ssl_renew(domain),
        _ => {}
    }

    let (email, key) = creds()?;

    match cli.cmd {
        // DNS
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
        Cmd::DnsUpsert { domain, record_type, name, content, proxied, ttl } => {
            dns_upsert(&email, &key, &domain, &record_type, &name, &content, proxied, ttl)
        }
        Cmd::DnsDelete { domain, record_type, name } => {
            dns_delete(&email, &key, &domain, &record_type, &name)
        }

        // Zones
        Cmd::Zones => zones(&email, &key),

        // Email
        Cmd::EmailStatus => email_status(&email, &key),
        Cmd::EmailForward { domain, destination } => {
            email_forward(&email, &key, &domain, &destination)
        }
        Cmd::EmailForwardAll { destination } => email_forward_all(&email, &key, &destination),
        Cmd::EmailWorkerAttach { domain, worker } => {
            email_worker_attach(&email, &key, &domain, &worker)
        }
        Cmd::EmailWorkerAttachAll { worker, dry_run } => {
            worker_attach_all(&email, &key, &worker, dry_run)
        }

        // SSL (issue needs CF creds)
        Cmd::SslIssue { domain, wildcard } => ssl_issue(&email, &key, &domain, wildcard),

        // Pages
        Cmd::PagesList => pages_list(&email, &key),
        Cmd::PagesCreate { name } => pages_create(&email, &key, &name),
        Cmd::PagesDomain { project, domain } => pages_domain(&email, &key, &project, &domain),
        Cmd::PagesDelete { project } => pages_delete(&email, &key, &project),

        // early-returned above
        Cmd::Doctor | Cmd::PagesDeploy { .. }
        | Cmd::SslInstall | Cmd::SslList | Cmd::SslStatus | Cmd::SslRenew { .. } => {
            unreachable!("위에서 early return")
        }
    }
}

// ============================================================
// CF API helpers
// ============================================================

fn creds() -> anyhow::Result<(String, String)> {
    let email = read_host_env("CLOUDFLARE_EMAIL");
    let key = read_host_env("CLOUDFLARE_API_KEY");
    if email.is_empty() || key.is_empty() {
        anyhow::bail!("/etc/pxi/.env 에 CLOUDFLARE_EMAIL / CLOUDFLARE_API_KEY 필요");
    }
    Ok((email, key))
}

fn cf_api(email: &str, key: &str, method: &str, path: &str, body: Option<&str>) -> anyhow::Result<Value> {
    let url = format!("https://api.cloudflare.com/client/v4{path}");
    let mut args: Vec<String> = vec![
        "-sSL".into(), "--fail-with-body".into(),
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
    let output = std::process::Command::new("curl").args(&args_ref).output()
        .map_err(|e| anyhow::anyhow!("curl 실행 실패: {e}"))?;
    if !output.status.success() {
        // try to parse body even on failure for better error messages
        let body_str = String::from_utf8_lossy(&output.stdout);
        if let Ok(v) = serde_json::from_str::<Value>(&body_str) {
            if let Some(errors) = v["errors"].as_array() {
                let msgs: Vec<String> = errors.iter()
                    .filter_map(|e| e["message"].as_str().map(String::from))
                    .collect();
                if !msgs.is_empty() {
                    anyhow::bail!("CF API 실패: {}", msgs.join("; "));
                }
            }
        }
        anyhow::bail!(
            "CF API 호출 실패 (exit {}). API 키/네트워크를 확인하세요.",
            output.status.code().unwrap_or(-1)
        );
    }
    let out = String::from_utf8_lossy(&output.stdout).to_string();
    let v: Value = serde_json::from_str(&out)?;
    if !v["success"].as_bool().unwrap_or(false) {
        anyhow::bail!("CF API 실패: {}", v["errors"]);
    }
    Ok(v["result"].clone())
}

fn get_zone_id(email: &str, key: &str, domain: &str) -> anyhow::Result<String> {
    let r = cf_api(email, key, "GET", &format!("/zones?name={domain}"), None)?;
    r[0]["id"].as_str().map(String::from)
        .ok_or_else(|| anyhow::anyhow!("zone {domain} 없음"))
}

fn get_account_id(email: &str, key: &str) -> anyhow::Result<String> {
    let r = cf_api(email, key, "GET", "/accounts?per_page=1", None)?;
    r[0]["id"].as_str().map(String::from)
        .ok_or_else(|| anyhow::anyhow!("account id 조회 실패"))
}

fn find_record(email: &str, key: &str, zid: &str, rec_type: &str, name: &str) -> anyhow::Result<String> {
    let records = cf_api(email, key, "GET", &format!("/zones/{zid}/dns_records?type={rec_type}&name={name}"), None)?;
    let Some(arr) = records.as_array() else { anyhow::bail!("응답 파싱 실패") };
    let Some(first) = arr.first() else {
        anyhow::bail!("레코드 없음: {rec_type} {name}")
    };
    first["id"].as_str().map(String::from)
        .ok_or_else(|| anyhow::anyhow!("record id 없음"))
}

// ============================================================
// Zones
// ============================================================

fn zones(email: &str, key: &str) -> anyhow::Result<()> {
    println!("=== Cloudflare 도메인 목록 ===\n");
    let zones = cf_api(email, key, "GET", "/zones?per_page=50", None)?;
    let Some(arr) = zones.as_array() else { anyhow::bail!("zones 응답 파싱 실패") };
    for zone in arr {
        let name = zone["name"].as_str().unwrap_or("?");
        let status = zone["status"].as_str().unwrap_or("?");
        let plan = zone["plan"]["name"].as_str().unwrap_or("?");
        let mark = if status == "active" { "✓" } else { "✗" };
        println!("  {mark} {name:<30} {status:<10} {plan}");
    }
    println!("\n  총 {}개 도메인", arr.len());
    Ok(())
}

// ============================================================
// DNS
// ============================================================

fn dns_add(email: &str, key: &str, domain: &str, rec_type: &str, name: &str, content: &str, proxied: bool) -> anyhow::Result<()> {
    let zid = get_zone_id(email, key, domain)?;
    let body = serde_json::json!({
        "type": rec_type, "name": name, "content": content, "proxied": proxied, "ttl": 1
    });
    cf_api(email, key, "POST", &format!("/zones/{zid}/dns_records"), Some(&body.to_string()))?;
    println!("✓ DNS 추가: {rec_type} {name} → {content} (proxied={proxied})");
    Ok(())
}

fn dns_list(email: &str, key: &str, domain: &str) -> anyhow::Result<()> {
    let zid = get_zone_id(email, key, domain)?;
    let records = cf_api(email, key, "GET", &format!("/zones/{zid}/dns_records?per_page=100"), None)?;
    let Some(arr) = records.as_array() else { anyhow::bail!("dns_records 응답 파싱 실패") };
    println!("=== DNS 레코드: {domain} ({}) ===\n", arr.len());
    for r in arr {
        let t = r["type"].as_str().unwrap_or("?");
        let n = r["name"].as_str().unwrap_or("?");
        let c = r["content"].as_str().unwrap_or("?");
        let p = r["proxied"].as_bool().unwrap_or(false);
        let ttl = r["ttl"].as_u64().unwrap_or(0);
        let icon = if p { "☁" } else { "→" };
        let ttl_str = if ttl == 1 { "auto".to_string() } else { format!("{ttl}s") };
        println!("  {t:<8} {n:<40} {icon} {c:<30} {ttl_str}");
    }
    println!("\n  총 {}개 레코드", arr.len());
    Ok(())
}

fn dns_update(email: &str, key: &str, domain: &str, rec_type: &str, name: &str, content: &str, proxied: Option<bool>) -> anyhow::Result<()> {
    let zid = get_zone_id(email, key, domain)?;
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

fn dns_upsert(
    email: &str, key: &str, domain: &str,
    rec_type: &str, name: &str, content: &str,
    proxied: Option<bool>, ttl: u64,
) -> anyhow::Result<()> {
    let zid = get_zone_id(email, key, domain)?;

    // Resolve FQDN
    let fqdn = if name == "@" || name.is_empty() {
        domain.to_string()
    } else if name.ends_with(domain) {
        name.to_string()
    } else {
        format!("{name}.{domain}")
    };

    // Check existing
    let path = format!("/zones/{zid}/dns_records?name={fqdn}&type={rec_type}");
    let records = cf_api(email, key, "GET", &path, None)?;
    let existing = records.as_array().and_then(|a| a.first());

    match existing {
        Some(rec) => {
            let current_content = rec["content"].as_str().unwrap_or_default();
            let current_proxied = rec["proxied"].as_bool();
            let current_ttl = rec["ttl"].as_u64().unwrap_or(1);
            let desired_proxied = proxied.unwrap_or(current_proxied.unwrap_or(false));
            let desired_ttl = if ttl == 0 { current_ttl } else { ttl };

            if current_content == content
                && current_proxied == Some(desired_proxied)
                && (ttl == 0 || current_ttl == desired_ttl)
            {
                println!("[dns] 레코드 이미 최신: {rec_type} {fqdn} → {content}");
                return Ok(());
            }

            let rid = rec["id"].as_str().unwrap_or_default();
            let body = serde_json::json!({
                "type": rec_type, "name": fqdn, "content": content,
                "proxied": desired_proxied, "ttl": if desired_ttl == 0 { 1 } else { desired_ttl },
            });
            cf_api(email, key, "PUT", &format!("/zones/{zid}/dns_records/{rid}"), Some(&body.to_string()))?;
            println!("✓ DNS upsert (수정): {rec_type} {fqdn} → {content}");
        }
        None => {
            let p = proxied.unwrap_or(false);
            let body = serde_json::json!({
                "type": rec_type, "name": name, "content": content,
                "proxied": p, "ttl": if ttl == 0 { 1 } else { ttl },
            });
            cf_api(email, key, "POST", &format!("/zones/{zid}/dns_records"), Some(&body.to_string()))?;
            println!("✓ DNS upsert (생성): {rec_type} {name} → {content} (proxied={p})");
        }
    }
    Ok(())
}

fn dns_delete(email: &str, key: &str, domain: &str, rec_type: &str, name: &str) -> anyhow::Result<()> {
    let zid = get_zone_id(email, key, domain)?;
    let rid = find_record(email, key, &zid, rec_type, name)?;
    cf_api(email, key, "DELETE", &format!("/zones/{zid}/dns_records/{rid}"), None)?;
    println!("✓ DNS 삭제: {rec_type} {name}");
    Ok(())
}

// ============================================================
// Email Routing
// ============================================================

fn email_status(email: &str, key: &str) -> anyhow::Result<()> {
    println!("=== Cloudflare Email Routing 상태 ===\n");
    let zones = cf_api(email, key, "GET", "/zones?per_page=50", None)?;
    let Some(arr) = zones.as_array() else { anyhow::bail!("zones 응답 파싱 실패") };

    for zone in arr {
        let name = zone["name"].as_str().unwrap_or("?");
        let zid = zone["id"].as_str().unwrap_or("");
        match cf_api(email, key, "GET", &format!("/zones/{zid}/email/routing"), None) {
            Ok(routing) => {
                let enabled = routing["enabled"].as_bool().unwrap_or(false);
                let status = routing["status"].as_str().unwrap_or("?");
                let mark = if enabled && status == "ready" {
                    "✓"
                } else if enabled {
                    "⚠"
                } else {
                    "✗"
                };
                println!("  {mark} {name:<30} enabled={enabled:<6} status={status}");
            }
            Err(_) => {
                println!("  ? {name:<30} (조회 실패)");
            }
        }
    }
    println!("\n  총 {}개 도메인", arr.len());
    Ok(())
}

fn email_forward(email: &str, key: &str, domain: &str, destination: &str) -> anyhow::Result<()> {
    println!("=== {domain} 이메일 포워딩 → {destination} ===\n");

    let zid = get_zone_id(email, key, domain)?;
    let account_id = get_account_id(email, key)?;

    // Register destination if needed
    let dest_list = cf_api(email, key, "GET",
        &format!("/accounts/{account_id}/email/routing/addresses"), None);
    let exists = dest_list.as_ref().ok()
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(|a| a["email"].as_str() == Some(destination)))
        .unwrap_or(false);

    if !exists {
        let body = serde_json::json!({"email": destination});
        cf_api(email, key, "POST",
            &format!("/accounts/{account_id}/email/routing/addresses"),
            Some(&body.to_string()))?;
        println!("[email] 대상 주소 등록: {destination} (인증 메일 확인 필요)");
    }

    // Enable email routing
    let enable_body = serde_json::json!({"enabled": true});
    let _ = cf_api(email, key, "PUT",
        &format!("/zones/{zid}/email/routing"),
        Some(&enable_body.to_string()));

    // Set catch-all
    let rule_body = serde_json::json!({
        "enabled": true,
        "matchers": [{"type": "all"}],
        "actions": [{"type": "forward", "value": [destination]}]
    });
    cf_api(email, key, "PUT",
        &format!("/zones/{zid}/email/routing/rules/catch_all"),
        Some(&rule_body.to_string()))?;
    println!("[email] {domain} catch-all → {destination} 설정 완료");
    Ok(())
}

fn email_forward_all(email: &str, key: &str, destination: &str) -> anyhow::Result<()> {
    println!("=== 전체 도메인 이메일 포워딩 → {destination} ===\n");

    let account_id = get_account_id(email, key)?;

    // 1. Register destination address
    let dest_list = cf_api(email, key, "GET",
        &format!("/accounts/{account_id}/email/routing/addresses"), None);
    let already_verified = dest_list.as_ref().ok()
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(|a| {
            a["email"].as_str() == Some(destination)
                && (a["verified"].as_str().is_some()
                    || a["status"].as_str() == Some("verified"))
        }))
        .unwrap_or(false);

    let already_exists = dest_list.as_ref().ok()
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(|a| a["email"].as_str() == Some(destination)))
        .unwrap_or(false);

    if already_verified {
        println!("[email] 대상 주소 이미 인증됨: {destination}");
    } else if already_exists {
        println!("[email] 대상 주소 등록됨 (인증 대기 중): {destination}");
        println!("  → {destination} 메일함에서 Cloudflare 인증 메일을 확인하세요");
    } else {
        let body = serde_json::json!({"email": destination});
        cf_api(email, key, "POST",
            &format!("/accounts/{account_id}/email/routing/addresses"),
            Some(&body.to_string()))?;
        println!("[email] 대상 주소 등록 완료: {destination}");
        println!("  → {destination} 메일함에서 Cloudflare 인증 메일을 확인하세요");
    }

    // 2. Set catch-all on each zone
    let zones = cf_api(email, key, "GET", "/zones?per_page=50", None)?;
    let Some(arr) = zones.as_array() else { anyhow::bail!("도메인 목록 조회 실패") };

    let mut ok_count = 0u32;
    let mut skip_count = 0u32;
    let mut err_count = 0u32;

    for zone in arr {
        let name = zone["name"].as_str().unwrap_or("?");
        let zid = zone["id"].as_str().unwrap_or("");

        // Enable email routing
        let enable_body = serde_json::json!({"enabled": true});
        let _ = cf_api(email, key, "PUT",
            &format!("/zones/{zid}/email/routing"),
            Some(&enable_body.to_string()));

        // Check existing catch-all
        let already_forwarding = cf_api(email, key, "GET",
                &format!("/zones/{zid}/email/routing/rules/catch_all"), None)
            .ok()
            .map(|r| {
                r["actions"].as_array()
                    .map(|acts| acts.iter().any(|a| {
                        a["type"].as_str() == Some("forward")
                            && a["value"].as_array()
                                .map(|v| v.iter().any(|e| e.as_str() == Some(destination)))
                                .unwrap_or(false)
                    }))
                    .unwrap_or(false)
            })
            .unwrap_or(false);

        if already_forwarding {
            println!("  ✓ {name:<30} 이미 설정됨");
            skip_count += 1;
            continue;
        }

        let rule_body = serde_json::json!({
            "enabled": true,
            "matchers": [{"type": "all"}],
            "actions": [{"type": "forward", "value": [destination]}]
        });

        match cf_api(email, key, "PUT",
            &format!("/zones/{zid}/email/routing/rules/catch_all"),
            Some(&rule_body.to_string()))
        {
            Ok(_) => {
                println!("  ✓ {name:<30} catch-all → {destination}");
                ok_count += 1;
            }
            Err(_) => {
                eprintln!("  ✗ {name:<30} 설정 실패");
                err_count += 1;
            }
        }
    }

    println!("\n  완료: {ok_count}개 설정, {skip_count}개 스킵, {err_count}개 실패 (총 {}개)", arr.len());

    if !already_verified && !already_exists {
        println!("\n  ⚠ {destination} 인증이 필요합니다. 메일함을 확인하세요.");
    }
    Ok(())
}

fn email_worker_attach(email: &str, key: &str, domain: &str, worker: &str) -> anyhow::Result<()> {
    println!("=== {domain} catch-all → Worker '{worker}' 전환 ===\n");

    let zid = get_zone_id(email, key, domain)?;

    // Enable email routing
    let enable_body = serde_json::json!({"enabled": true});
    let _ = cf_api(email, key, "PUT",
        &format!("/zones/{zid}/email/routing"),
        Some(&enable_body.to_string()));

    // Set catch-all to worker
    let body = serde_json::json!({
        "matchers": [{"type": "all"}],
        "actions": [{"type": "worker", "value": [worker]}],
        "enabled": true
    });
    cf_api(email, key, "PUT",
        &format!("/zones/{zid}/email/routing/rules/catch_all"),
        Some(&body.to_string()))?;
    println!("[email] ✓ {domain} catch-all → worker:{worker}");
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

        match cf_api(email, key, "GET", &format!("/zones/{zid}/email/routing"), None) {
            Ok(r) => {
                if !r["enabled"].as_bool().unwrap_or(false) {
                    println!("  ⊘ {zname}: Email Routing 비활성화");
                    skipped += 1;
                    continue;
                }
            }
            Err(e) => {
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
                    println!("  ⚠ {zname}: 현재 catch-all 조회 실패 ({e})");
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

// ============================================================
// SSL (acme.sh wrapper)
// ============================================================

const ACME_HOME: &str = "/root/.acme.sh";

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

fn ssl_install() -> anyhow::Result<()> {
    println!("=== acme.sh 설치 ===\n");

    if std::path::Path::new(&format!("{ACME_HOME}/acme.sh")).exists() {
        println!("[ssl] acme.sh 이미 설치됨");
        return Ok(());
    }

    let cf_email = read_host_env("CLOUDFLARE_EMAIL");
    if cf_email.is_empty() {
        anyhow::bail!("CLOUDFLARE_EMAIL 미설정");
    }

    println!("[ssl] acme.sh 설치 중...");
    let output = std::process::Command::new("sh")
        .args(["-c", &format!("curl https://get.acme.sh | sh -s email={cf_email}")])
        .output()?;

    if output.status.success() {
        println!("[ssl] acme.sh 설치 완료");
    } else {
        anyhow::bail!("설치 실패: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

fn ssl_issue(cf_email: &str, cf_key: &str, domain: &str, wildcard: bool) -> anyhow::Result<()> {
    println!("=== SSL 발급: {domain} (wildcard: {wildcard}) ===");
    let acme = find_acme_sh().ok_or_else(|| anyhow::anyhow!(
        "acme.sh 미설치. 설치: pxi-cloudflare ssl-install\n\
         확인된 경로: PATH, ~/.acme.sh/, /root/.acme.sh/"
    ))?;
    println!("  acme.sh: {acme}");

    let mut args: Vec<String> = vec!["--issue".into(), "--dns".into(), "dns_cf".into()];
    args.push("-d".into());
    args.push(domain.to_string());
    if wildcard {
        args.push("-d".into());
        args.push(format!("*.{domain}"));
    }

    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let output = std::process::Command::new(&acme)
        .args(&args_ref)
        .env("CF_Email", cf_email)
        .env("CF_Key", cf_key)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        println!("[ssl] 발급 완료");
        for line in stdout.lines().chain(stderr.lines()) {
            if line.contains("Cert success")
                || line.contains("Your cert is in")
                || line.contains("fullchain")
            {
                println!("  {line}");
            }
        }
        let cert_dir = format!("{ACME_HOME}/{domain}_ecc");
        if std::path::Path::new(&cert_dir).exists() {
            println!("\n[ssl] 인증서 위치: {cert_dir}/");
        } else {
            let cert_dir_rsa = format!("{ACME_HOME}/{domain}");
            if std::path::Path::new(&cert_dir_rsa).exists() {
                println!("\n[ssl] 인증서 위치: {cert_dir_rsa}/");
            }
        }
    } else if stderr.contains("already exists") || stdout.contains("Domains not changed") {
        println!("[ssl] 이미 발급된 인증서 존재. 갱신: pxi-cloudflare ssl-renew --domain {domain}");
    } else {
        anyhow::bail!("acme.sh 발급 실패:\n{stdout}\n{stderr}");
    }
    Ok(())
}

fn ssl_renew(domain: &str) -> anyhow::Result<()> {
    println!("=== SSL 갱신: {domain} ===");
    let acme = find_acme_sh().ok_or_else(|| anyhow::anyhow!("acme.sh 미설치"))?;
    let output = std::process::Command::new(&acme)
        .args(["--renew", "-d", domain, "--force"])
        .output()?;
    if output.status.success() {
        println!("✓ 갱신 완료");
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("skip") || stderr.contains("not yet") {
            println!("[ssl] 갱신 불필요 (아직 유효)");
        } else {
            anyhow::bail!("갱신 실패: {}", String::from_utf8_lossy(&output.stdout));
        }
    }
    Ok(())
}

fn ssl_list() {
    println!("=== SSL 인증서 목록 ===\n");
    let Some(acme) = find_acme_sh() else {
        eprintln!("[ssl] acme.sh 미설치");
        return;
    };
    let output = std::process::Command::new(&acme)
        .args(["--list"])
        .output()
        .expect("acme.sh 실행 실패");
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() || stdout.lines().count() <= 1 {
        println!("  (발급된 인증서 없음)");
    } else {
        for line in stdout.lines() {
            println!("  {line}");
        }
    }
}

fn ssl_status() {
    println!("=== SSL 상태 ===\n");
    let installed = std::path::Path::new(&format!("{ACME_HOME}/acme.sh")).exists();
    println!("[acme.sh] {}", if installed { "✓ 설치됨" } else { "✗ 미설치" });

    if installed {
        let output = std::process::Command::new(format!("{ACME_HOME}/acme.sh"))
            .args(["--list"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        let cert_count = output.lines().count().saturating_sub(1);
        println!("[인증서] {}개 발급됨", cert_count);
    }

    let cf_key = read_host_env("CLOUDFLARE_API_KEY");
    let cf_email = read_host_env("CLOUDFLARE_EMAIL");
    println!(
        "[Cloudflare DNS] {}",
        if !cf_key.is_empty() && !cf_email.is_empty() { "✓ 설정됨" } else { "✗ 미설정" }
    );
}

// ============================================================
// Pages
// ============================================================

fn pages_list(email: &str, key: &str) -> anyhow::Result<()> {
    let account_id = get_account_id(email, key)?;
    let body = cf_api(email, key, "GET",
        &format!("/accounts/{account_id}/pages/projects"), None)?;
    println!("\n=== Cloudflare Pages 프로젝트 ===\n");
    if let Some(projects) = body.as_array() {
        for p in projects {
            let name = p["name"].as_str().unwrap_or("?");
            let subdomain = p["subdomain"].as_str().unwrap_or("?");
            let domains = p["domains"].as_array();
            let custom: Vec<&str> = domains
                .map(|d| d.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            let custom_str = if custom.is_empty() {
                String::new()
            } else {
                format!(" => {}", custom.join(", "))
            };
            println!("  {name:<20} https://{subdomain}{custom_str}");
        }
        println!("\n  총 {}개 프로젝트", projects.len());
    }
    Ok(())
}

fn pages_create(email: &str, key: &str, name: &str) -> anyhow::Result<()> {
    let account_id = get_account_id(email, key)?;
    let data = serde_json::json!({
        "name": name,
        "production_branch": "main"
    });
    let result = cf_api(email, key, "POST",
        &format!("/accounts/{account_id}/pages/projects"),
        Some(&data.to_string()))?;
    let subdomain = result["subdomain"].as_str().unwrap_or("?");
    println!("[pages] 프로젝트 생성 완료: {name}");
    println!("[pages] URL: https://{subdomain}");
    Ok(())
}

fn pages_domain(email: &str, key: &str, project: &str, domain_name: &str) -> anyhow::Result<()> {
    let account_id = get_account_id(email, key)?;

    // 1. Add domain to Pages project
    let path = format!("/accounts/{account_id}/pages/projects/{project}/domains");
    let data = serde_json::json!({"name": domain_name});
    cf_api(email, key, "POST", &path, Some(&data.to_string()))?;
    println!("[pages] 도메인 추가: {domain_name}");

    // 2. Auto-configure DNS CNAME
    let parts: Vec<&str> = domain_name.splitn(2, '.').collect();
    if parts.len() != 2 {
        anyhow::bail!("도메인 형식 오류: {domain_name}");
    }
    let zone_domain = parts[1];
    let subdomain = parts[0];

    let zid = get_zone_id(email, key, zone_domain)?;

    // Delete existing records for this FQDN
    let records = cf_api(email, key, "GET",
        &format!("/zones/{zid}/dns_records?name={domain_name}"), None)?;
    if let Some(arr) = records.as_array() {
        for r in arr {
            if let Some(id) = r["id"].as_str() {
                let _ = cf_api(email, key, "DELETE",
                    &format!("/zones/{zid}/dns_records/{id}"), None);
            }
        }
    }

    // Add CNAME
    let cname_data = serde_json::json!({
        "type": "CNAME",
        "name": subdomain,
        "content": format!("{project}.pages.dev"),
        "proxied": true,
        "ttl": 1
    });
    cf_api(email, key, "POST",
        &format!("/zones/{zid}/dns_records"),
        Some(&cname_data.to_string()))?;
    println!("[pages] DNS 설정: {domain_name} => {project}.pages.dev");
    println!("[pages] 완료! https://{domain_name}");
    Ok(())
}

fn pages_delete(email: &str, key: &str, project: &str) -> anyhow::Result<()> {
    let account_id = get_account_id(email, key)?;
    cf_api(email, key, "DELETE",
        &format!("/accounts/{account_id}/pages/projects/{project}"), None)?;
    println!("[pages] 프로젝트 삭제: {project}");
    Ok(())
}

fn pages_deploy(project: &str, directory: &str) -> anyhow::Result<()> {
    println!("=== CF Pages 배포: {project} <- {directory} ===");
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

// ============================================================
// Misc
// ============================================================

fn doctor() {
    println!("=== pxi-cloudflare doctor ===");
    let email = read_host_env("CLOUDFLARE_EMAIL");
    let key = read_host_env("CLOUDFLARE_API_KEY");
    println!("  CF_EMAIL:   {}", if !email.is_empty() { "✓" } else { "✗" });
    println!("  CF_API_KEY: {}", if !key.is_empty() { "✓" } else { "✗" });
    println!("  curl:       {}", if common::has_cmd("curl") { "✓" } else { "✗" });
    println!("  acme.sh:    {}", if find_acme_sh().is_some() { "✓" } else { "✗" });
    println!("  wrangler:   {}", if common::has_cmd("wrangler") { "✓" } else { "✗" });
}

fn read_host_env(key: &str) -> String {
    // Check process env first, then config files
    if let Ok(v) = std::env::var(key) {
        if !v.is_empty() {
            return v;
        }
    }
    for p in ["/etc/pxi/.env", "/etc/proxmox-host-setup/.env"] {
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
