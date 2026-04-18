//! pxi-telegram — 텔레그램 봇 관리 + 메시지 발송 + 채널 운영.
//!
//! phs telegram 서브커맨드를 pxi-core::common::run() 기반으로 포팅.
//! HTTP 호출은 curl을 통해 수행 (외부 HTTP 라이브러리 의존 없음).

use clap::{Parser, Subcommand};
use pxi_core::{common, paths};
use std::fs;

#[derive(Parser)]
#[command(name = "pxi-telegram", about = "Telegram 봇 관리")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 봇 토큰 등록 (config에 저장)
    Register {
        /// 봇 이름 (내부 식별자)
        name: String,
        /// BotFather에서 받은 토큰
        #[arg(long)]
        token: String,
    },
    /// 메시지 발송
    Send {
        #[arg(long)]
        bot: String,
        #[arg(long)]
        chat: String,
        /// 메시지 텍스트 (생략 시 stdin)
        #[arg(long)]
        text: Option<String>,
        /// Markdown 파싱 모드
        #[arg(long)]
        markdown: bool,
    },
    /// 이미지 파일 전송
    SendPhoto {
        /// 봇 이름
        #[arg(long)]
        bot: String,
        /// 채팅 ID
        #[arg(long)]
        chat: String,
        /// 이미지 파일 경로
        #[arg(long)]
        file: String,
        /// 캡션
        #[arg(long)]
        caption: Option<String>,
    },
    /// 등록된 봇 목록 (토큰은 마스킹)
    List,
    /// 봇 등록 해제
    Remove { name: String },
    /// getMe로 토큰 검증
    Verify { bot: String },
    /// 텔레그램 총괄 상태 (봇 + OpenClaw 채널)
    Status,
    /// 등록된 봇 상세 정보 (username, webhook 등)
    Bots,
    /// webhook 설정/삭제
    Webhook {
        /// 봇 이름
        #[arg(long)]
        bot: String,
        /// webhook URL (delete 또는 off로 삭제)
        #[arg(long)]
        url: String,
    },
    /// OpenClaw 텔레그램 pairing 승인
    PairingApprove {
        /// pairing 코드 (all 이면 전체 승인)
        code: String,
    },
    /// 이미지 생성 + 텔레그램 전송 (ComfyUI)
    Generate {
        /// 봇 이름
        #[arg(long)]
        bot: String,
        /// 채팅 ID
        #[arg(long)]
        chat: String,
        /// 이미지 프롬프트
        #[arg(long)]
        prompt: String,
        /// 모델 (기본: env COMFYUI_DEFAULT_MODEL)
        #[arg(long)]
        model: Option<String>,
    },
    /// 전체 봇 명령어 + 설명 일괄 설정
    SetupAll,
    /// 봇을 Claude Code 또는 OpenClaw에 할당
    Assign {
        /// 봇 이름
        #[arg(long)]
        bot: String,
        /// 대상 (claude 또는 openclaw)
        #[arg(long, alias = "to")]
        target: String,
        /// 채널 이름 (기본: 봇 이름)
        #[arg(long)]
        channel: Option<String>,
    },
    /// Claude Code + OpenClaw 채널 할당 현황
    Channels,
    /// BotFather에서 만든 봇을 .env에 등록
    BotRegister {
        /// 봇 라벨 (예: memo, alert)
        #[arg(long)]
        name: String,
        /// BotFather에서 받은 토큰
        #[arg(long)]
        token: String,
    },
    /// 봇 표시 이름 변경 (Telegram setMyName API)
    BotRename {
        /// 봇 이름
        #[arg(long)]
        bot: String,
        /// 새 표시 이름
        #[arg(long)]
        name: String,
    },
    Doctor,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Register { name, token } => register(&name, &token),
        Cmd::Send { bot, chat, text, markdown } => send(&bot, &chat, text.as_deref(), markdown),
        Cmd::SendPhoto { bot, chat, file, caption } => {
            send_photo(&bot, &chat, &file, caption.as_deref())
        }
        Cmd::List => { list()?; Ok(()) }
        Cmd::Remove { name } => remove(&name),
        Cmd::Verify { bot } => verify(&bot),
        Cmd::Status => { status(); Ok(()) }
        Cmd::Bots => { bots_detail(); Ok(()) }
        Cmd::Webhook { bot, url } => { webhook_set(&bot, &url); Ok(()) }
        Cmd::PairingApprove { code } => { pairing_approve(&code); Ok(()) }
        Cmd::Generate { bot, chat, prompt, model } => {
            generate(&bot, &chat, &prompt, model.as_deref())
        }
        Cmd::SetupAll => { setup_all(); Ok(()) }
        Cmd::Assign { bot, target, channel } => {
            assign(&bot, &target, channel.as_deref());
            Ok(())
        }
        Cmd::Channels => { list_channels(); Ok(()) }
        Cmd::BotRegister { name, token } => { bot_register(&name, &token); Ok(()) }
        Cmd::BotRename { bot, name } => { bot_rename(&bot, &name); Ok(()) }
        Cmd::Doctor => { doctor(); Ok(()) }
    }
}

// ─── Config 관리 ────────────────────────────────────────────────────────────

/// 봇 config 경로: config_dir/telegram.json
/// 구조: { "bots": { "name": "token", ... } }
fn bots_path() -> anyhow::Result<std::path::PathBuf> {
    let p = paths::config_dir()?.join("telegram.json");
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(p)
}

fn load_bots() -> anyhow::Result<serde_json::Value> {
    let p = bots_path()?;
    if !p.exists() {
        return Ok(serde_json::json!({"bots": {}}));
    }
    let raw = fs::read_to_string(&p)?;
    Ok(serde_json::from_str(&raw).unwrap_or(serde_json::json!({"bots": {}})))
}

fn save_bots(v: &serde_json::Value) -> anyhow::Result<()> {
    let p = bots_path()?;
    let parent = p.parent()
        .ok_or_else(|| anyhow::anyhow!("부모 디렉토리 없음"))?;
    fs::create_dir_all(parent)?;

    let tmp = parent.join(format!(".telegram.json.tmp.{}", std::process::id()));
    fs::write(&tmp, serde_json::to_string_pretty(v)?)?;
    common::run("chmod", &["600", &tmp.display().to_string()])?;

    if let Err(e) = fs::rename(&tmp, &p) {
        let _ = fs::remove_file(&tmp);
        return Err(anyhow::anyhow!("config 저장 실패: {e}"));
    }
    Ok(())
}

fn mask_token(token: &str) -> String {
    if token.len() < 10 {
        "***".into()
    } else {
        format!("{}...{}", &token[..5], &token[token.len()-4..])
    }
}

/// config에서 봇 토큰을 name으로 조회
fn get_token(bot: &str) -> anyhow::Result<String> {
    let bots = load_bots()?;
    bots["bots"][bot].as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("등록된 봇 없음: {bot} (pxi run telegram register)"))
}

/// config에서 봇 토큰을 name 또는 suffix로 조회 (fuzzy)
fn get_token_fuzzy(bot: &str) -> anyhow::Result<(String, String)> {
    let bots = load_bots()?;
    let map = bots["bots"].as_object()
        .ok_or_else(|| anyhow::anyhow!("config 파싱 실패"))?;

    // 정확히 일치
    if let Some(token) = map.get(bot).and_then(|v| v.as_str()) {
        return Ok((bot.to_string(), token.to_string()));
    }
    // suffix 매칭
    for (name, val) in map {
        if name.ends_with(bot) {
            if let Some(token) = val.as_str() {
                return Ok((name.clone(), token.to_string()));
            }
        }
    }

    let names: Vec<&String> = map.keys().collect();
    anyhow::bail!("봇 '{bot}' 없음. 등록된 봇: {}", names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))
}

/// 등록된 모든 봇 (name, token) 목록
fn all_bots() -> Vec<(String, String)> {
    let bots = load_bots().unwrap_or(serde_json::json!({"bots": {}}));
    let map = match bots["bots"].as_object() {
        Some(m) => m.clone(),
        None => return Vec::new(),
    };
    map.into_iter()
        .filter_map(|(k, v)| v.as_str().map(|t| (k, t.to_string())))
        .collect()
}

// ─── Telegram Bot API (curl 기반) ───────────────────────────────────────────

/// curl로 Telegram Bot API GET 호출, result JSON 반환
fn bot_api_get(token: &str, method: &str) -> Option<serde_json::Value> {
    let url = format!("https://api.telegram.org/bot{token}/{method}");
    let output = std::process::Command::new("curl")
        .args(["-sSL", "--fail", &url])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let resp: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    if resp["ok"].as_bool() == Some(true) {
        Some(resp["result"].clone())
    } else {
        None
    }
}

/// curl로 Telegram Bot API POST 호출 (JSON body), 전체 응답 반환
fn bot_api_post(token: &str, method: &str, body: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let url = format!("https://api.telegram.org/bot{token}/{method}");
    let body_str = body.to_string();
    let output = std::process::Command::new("curl")
        .args([
            "-sSL", "--fail",
            "-X", "POST",
            "-H", "Content-Type: application/json",
            "-d", &body_str,
            &url,
        ])
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "API 호출 실패: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let resp: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    Ok(resp)
}

/// curl로 Telegram Bot API POST (multipart/form-data) — 파일 업로드용
fn bot_api_multipart(token: &str, method: &str, fields: &[(&str, &str)], file_field: Option<(&str, &str)>) -> anyhow::Result<serde_json::Value> {
    let url = format!("https://api.telegram.org/bot{token}/{method}");
    let mut args: Vec<String> = vec!["-sSL".into()];
    for (key, val) in fields {
        args.push("-F".into());
        args.push(format!("{key}={val}"));
    }
    if let Some((key, path)) = file_field {
        args.push("-F".into());
        args.push(format!("{key}=@{path}"));
    }
    args.push(url);

    let output = std::process::Command::new("curl")
        .args(&args)
        .output()?;
    let resp: serde_json::Value = serde_json::from_slice(&output.stdout)
        .unwrap_or(serde_json::json!({"ok": false, "description": String::from_utf8_lossy(&output.stdout).to_string()}));
    Ok(resp)
}

// ─── Register / Remove / List / Verify (기존) ───────────────────────────────

fn register(name: &str, token: &str) -> anyhow::Result<()> {
    if !token.contains(':') || token.len() < 30 {
        anyhow::bail!("토큰 형식 비정상 (BotFather 토큰 아님?)");
    }
    let mut bots = load_bots()?;
    bots["bots"][name] = serde_json::json!(token);
    save_bots(&bots)?;
    println!("✓ 봇 등록: {name} ({})", mask_token(token));
    Ok(())
}

fn list() -> anyhow::Result<()> {
    let bots = load_bots()?;
    let map = bots["bots"].as_object();
    println!("=== 등록된 봇 ===");
    match map {
        Some(m) if !m.is_empty() => {
            for (name, token) in m {
                let t = token.as_str().unwrap_or("");
                println!("  {name:<20} {}", mask_token(t));
            }
        }
        _ => println!("  (없음)"),
    }
    Ok(())
}

fn remove(name: &str) -> anyhow::Result<()> {
    let mut bots = load_bots()?;
    let obj = bots["bots"].as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("config 파싱 실패"))?;
    if obj.remove(name).is_some() {
        save_bots(&bots)?;
        println!("✓ 제거: {name}");
    } else {
        println!("⊘ 등록 안 된 이름: {name}");
    }
    Ok(())
}

fn verify(bot: &str) -> anyhow::Result<()> {
    let token = get_token(bot)?;
    let r = bot_api_get(&token, "getMe")
        .ok_or_else(|| anyhow::anyhow!("API 호출 실패 — 토큰 무효 or 네트워크"))?;
    println!("✓ 봇 유효");
    println!("  id:       {}", r["id"]);
    println!("  username: @{}", r["username"].as_str().unwrap_or("?"));
    println!("  name:     {}", r["first_name"].as_str().unwrap_or("?"));
    Ok(())
}

// ─── Send ───────────────────────────────────────────────────────────────────

fn send(bot: &str, chat: &str, text: Option<&str>, markdown: bool) -> anyhow::Result<()> {
    let token = get_token(bot)?;

    let message = match text {
        Some(t) => t.to_string(),
        None => {
            use std::io::Read;
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            s.trim().to_string()
        }
    };
    if message.is_empty() {
        anyhow::bail!("메시지 텍스트 없음 (--text 또는 stdin)");
    }

    let mut data = serde_json::json!({
        "chat_id": chat,
        "text": message,
    });
    if markdown {
        data["parse_mode"] = serde_json::json!("MarkdownV2");
    } else {
        data["parse_mode"] = serde_json::json!("Markdown");
    }

    let resp = bot_api_post(&token, "sendMessage", &data)?;
    if !resp["ok"].as_bool().unwrap_or(false) {
        anyhow::bail!("Telegram API 실패: {}", resp["description"].as_str().unwrap_or("?"));
    }
    println!("✓ 발송 완료 → {bot}:{chat}");
    Ok(())
}

// ─── Send Photo ─────────────────────────────────────────────────────────────

fn send_photo(bot: &str, chat: &str, file_path: &str, caption: Option<&str>) -> anyhow::Result<()> {
    let token = get_token(bot)?;

    if !std::path::Path::new(file_path).exists() {
        anyhow::bail!("파일 없음: {file_path}");
    }

    send_photo_raw(&token, chat, file_path, caption)
}

fn send_photo_raw(token: &str, chat_id: &str, file_path: &str, caption: Option<&str>) -> anyhow::Result<()> {
    let mut fields: Vec<(&str, &str)> = vec![("chat_id", chat_id)];
    if let Some(cap) = caption {
        fields.push(("caption", cap));
    }
    let resp = bot_api_multipart(token, "sendPhoto", &fields, Some(("photo", file_path)))?;
    if resp["ok"].as_bool() == Some(true) {
        println!("✓ 이미지 전송 완료 → {chat_id}");
        Ok(())
    } else {
        anyhow::bail!("이미지 전송 실패: {}", resp.get("description").and_then(|d| d.as_str()).unwrap_or("?"));
    }
}

// ─── Status ─────────────────────────────────────────────────────────────────

fn status() {
    println!("=== 텔레그램 총괄 상태 ===\n");

    // API 크리덴셜
    let api_id = std::env::var("TELEGRAM_API_ID").unwrap_or_default();
    let api_hash = std::env::var("TELEGRAM_API_HASH").unwrap_or_default();
    if !api_id.is_empty() && !api_hash.is_empty() {
        println!("API 크리덴셜: api_id={api_id}, api_hash=설정됨");
    } else {
        println!("API 크리덴셜: 미설정 (.env에 TELEGRAM_API_ID/TELEGRAM_API_HASH 추가 필요)");
    }
    println!();

    let bots = all_bots();
    if bots.is_empty() {
        println!("등록된 봇 없음");
        return;
    }

    println!(
        "{:<20} {:<25} {:<15} {}",
        "이름", "봇 username", "토큰 ID", "상태"
    );
    println!("{}", "─".repeat(80));

    for (label, token) in &bots {
        let token_id = token.split(':').next().unwrap_or("?");
        match bot_api_get(token, "getMe") {
            Some(me) => {
                let username = me["username"].as_str().unwrap_or("?");
                let first_name = me["first_name"].as_str().unwrap_or("?");
                println!(
                    "{:<20} @{:<24} {:<15} OK ({})",
                    label, username, token_id, first_name
                );
            }
            None => {
                println!("{:<20} {:<25} {:<15} FAIL", label, "?", token_id);
            }
        }
    }

    // OpenClaw gateway 상태 (LXC 환경에서만)
    if let Ok(vmid) = resolve_openclaw_vmid() {
        println!("\n── OpenClaw Gateway 채널 ──\n");
        if lxc_is_running(&vmid) {
            let out = lxc_exec(&vmid, "export PATH=/usr/local/bin:$PATH && openclaw channels status 2>&1");
            for line in out.lines() {
                if line.contains("Telegram") || line.contains("Gateway") {
                    println!("  {}", line.trim());
                }
            }
        } else {
            println!("  OpenClaw LXC {vmid} 실행 중 아님");
        }
    }
}

// ─── Bots (상세 정보) ───────────────────────────────────────────────────────

fn bots_detail() {
    println!("=== 텔레그램 봇 목록 ===\n");

    let bots = all_bots();
    if bots.is_empty() {
        println!("등록된 봇 없음");
        return;
    }

    for (label, token) in &bots {
        let token_id = token.split(':').next().unwrap_or("?");
        println!("── {label} (토큰 ID: {token_id}) ──");

        if let Some(me) = bot_api_get(token, "getMe") {
            println!("  username : @{}", me["username"].as_str().unwrap_or("?"));
            println!("  이름     : {}", me["first_name"].as_str().unwrap_or("?"));
            println!(
                "  can_join : {}",
                me["can_join_groups"].as_bool().unwrap_or(false)
            );
        } else {
            println!("  상태: API 호출 실패");
        }

        // webhook 상태
        if let Some(wh) = bot_api_get(token, "getWebhookInfo") {
            let url = wh["url"].as_str().unwrap_or("");
            if url.is_empty() {
                println!("  webhook  : 없음 (polling 모드)");
            } else {
                println!("  webhook  : {url}");
                let pending = wh["pending_update_count"].as_u64().unwrap_or(0);
                println!("  pending  : {pending}");
            }
        }
        println!();
    }
}

// ─── Webhook ────────────────────────────────────────────────────────────────

fn webhook_set(bot_name: &str, url: &str) {
    let token = match get_token(bot_name) {
        Ok(t) => t,
        Err(e) => { eprintln!("[telegram] {e}"); return; }
    };

    if url == "delete" || url == "off" || url.is_empty() {
        match bot_api_get(&token, "deleteWebhook?drop_pending_updates=true") {
            Some(_) => println!("[telegram] {bot_name} webhook 삭제 완료"),
            None => eprintln!("[telegram] webhook 삭제 실패"),
        }
    } else {
        let method = format!("setWebhook?url={url}");
        match bot_api_get(&token, &method) {
            Some(_) => println!("[telegram] {bot_name} webhook 설정: {url}"),
            None => eprintln!("[telegram] webhook 설정 실패"),
        }
    }
}

// ─── Pairing Approve ────────────────────────────────────────────────────────

fn pairing_approve(code: &str) {
    let vmid = match resolve_openclaw_vmid() {
        Ok(v) => v,
        Err(e) => { eprintln!("[telegram] {e}"); return; }
    };

    if !lxc_is_running(&vmid) {
        eprintln!("[telegram] OpenClaw LXC {vmid} 실행 중 아님");
        return;
    }

    // pending 목록 먼저 표시
    let list_out = lxc_exec(&vmid, "export PATH=/usr/local/bin:$PATH && openclaw pairing list 2>&1");
    println!("{}", list_out.trim());

    if code == "all" {
        // 8자리 영숫자 코드 추출
        let codes = extract_pairing_codes(&list_out);
        if codes.is_empty() {
            println!("\n[telegram] 승인할 pending 요청 없음");
            return;
        }
        for c in &codes {
            let cmd = format!(
                "export PATH=/usr/local/bin:$PATH && openclaw pairing approve telegram {c} 2>&1"
            );
            let out = lxc_exec(&vmid, &cmd);
            println!("{}", out.trim());
        }
    } else {
        let cmd = format!(
            "export PATH=/usr/local/bin:$PATH && openclaw pairing approve telegram {code} 2>&1"
        );
        let out = lxc_exec(&vmid, &cmd);
        println!("{}", out.trim());
    }
}

/// 8자리 영숫자 코드 추출 (regex 없이)
fn extract_pairing_codes(text: &str) -> Vec<String> {
    let mut codes = Vec::new();
    for word in text.split_whitespace() {
        let trimmed = word.trim_matches(|c: char| !c.is_alphanumeric());
        if trimmed.len() == 8 && trimmed.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
            codes.push(trimmed.to_string());
        }
    }
    codes
}

// ─── Generate (ComfyUI 이미지 생성) ────────────────────────────────────────

fn generate(bot: &str, chat: &str, prompt: &str, model: Option<&str>) -> anyhow::Result<()> {
    let token = get_token(bot)?;

    let comfyui_url = std::env::var("COMFYUI_URL")
        .map_err(|_| anyhow::anyhow!("COMFYUI_URL 환경변수 필요"))?;
    let default_model = std::env::var("COMFYUI_DEFAULT_MODEL").unwrap_or_default();
    let ckpt = model.unwrap_or(&default_model);
    if ckpt.is_empty() {
        anyhow::bail!("모델 미지정: --model 또는 COMFYUI_DEFAULT_MODEL 환경변수 필요");
    }

    println!("=== 이미지 생성 ===\n");
    println!("프롬프트 : {prompt}");
    println!("모델     : {ckpt}");
    println!("봇       : {bot}");
    println!();

    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // ComfyUI 워크플로우 제출
    let workflow = serde_json::json!({
        "prompt": {
            "1": {
                "class_type": "CheckpointLoaderSimple",
                "inputs": {"ckpt_name": ckpt}
            },
            "2": {
                "class_type": "CLIPTextEncode",
                "inputs": {"text": prompt, "clip": ["1", 1]}
            },
            "3": {
                "class_type": "CLIPTextEncode",
                "inputs": {"text": "ugly, blurry, low quality, deformed", "clip": ["1", 1]}
            },
            "4": {
                "class_type": "EmptyLatentImage",
                "inputs": {"width": 1024, "height": 1024, "batch_size": 1}
            },
            "5": {
                "class_type": "KSampler",
                "inputs": {
                    "model": ["1", 0],
                    "positive": ["2", 0],
                    "negative": ["3", 0],
                    "latent_image": ["4", 0],
                    "seed": seed,
                    "steps": 25,
                    "cfg": 7.0,
                    "sampler_name": "euler_ancestral",
                    "scheduler": "normal",
                    "denoise": 1.0
                }
            },
            "6": {
                "class_type": "VAEDecode",
                "inputs": {"samples": ["5", 0], "vae": ["1", 2]}
            },
            "7": {
                "class_type": "SaveImage",
                "inputs": {"images": ["6", 0], "filename_prefix": "pxi_gen"}
            }
        }
    });

    let body_str = workflow.to_string();
    let url = format!("{comfyui_url}/prompt");
    let output = std::process::Command::new("curl")
        .args(["-sSL", "-X", "POST", "-H", "Content-Type: application/json", "-d", &body_str, &url])
        .output()?;

    let body: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| anyhow::anyhow!("ComfyUI 응답 파싱 실패"))?;
    let prompt_id = body["prompt_id"].as_str()
        .ok_or_else(|| anyhow::anyhow!("prompt_id 없음: {body}"))?
        .to_string();

    println!("[comfyui] 제출 완료: {prompt_id}");
    println!("[comfyui] 생성 대기 중...");

    // 폴링으로 완료 대기 (최대 120초)
    let mut filename = String::new();
    for _ in 0..24 {
        std::thread::sleep(std::time::Duration::from_secs(5));

        let hist_url = format!("{comfyui_url}/history/{prompt_id}");
        let hist_out = std::process::Command::new("curl")
            .args(["-sSL", &hist_url])
            .output();

        if let Ok(out) = hist_out {
            if let Ok(hist) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                if let Some(entry) = hist.get(&prompt_id) {
                    let status_str = entry["status"]["status_str"].as_str().unwrap_or("");
                    if status_str == "success" {
                        if let Some(outputs) = entry["outputs"].as_object() {
                            'outer: for (_node, out_val) in outputs {
                                if let Some(images) = out_val["images"].as_array() {
                                    for img in images {
                                        if let Some(fn_) = img["filename"].as_str() {
                                            filename = fn_.to_string();
                                            break 'outer;
                                        }
                                    }
                                }
                            }
                        }
                        break;
                    } else if status_str == "error" {
                        anyhow::bail!("ComfyUI 생성 실패");
                    }
                }
            }
        }
    }

    if filename.is_empty() {
        anyhow::bail!("타임아웃 — 이미지 생성 실패");
    }

    println!("[comfyui] 생성 완료: {filename}");

    // 이미지 다운로드
    let tmp_path = format!("/tmp/pxi_gen_{filename}");
    let img_url = format!("{comfyui_url}/view?filename={filename}&type=output");
    common::run("curl", &["-sSL", "-o", &tmp_path, &img_url])?;

    println!("[telegram] 이미지 전송 중...");
    let cap = format!("{prompt}\n{ckpt}");
    send_photo_raw(&token, chat, &tmp_path, Some(&cap))?;

    let _ = fs::remove_file(&tmp_path);
    println!("\n=== 완료 ===");
    Ok(())
}

// ─── Setup All ──────────────────────────────────────────────────────────────

fn setup_all() {
    println!("=== 텔레그램 봇 일괄 설정 ===\n");

    let bots = all_bots();
    if bots.is_empty() {
        println!("등록된 봇 없음");
        return;
    }

    // 봇별 명령어 정의: (label, short_desc, about, commands)
    let bot_commands: &[(&str, &str, &str, &[(&str, &str)])] = &[
        (
            "openclaw-main",
            "Proxmox 50 서버 AI 에이전트",
            "Proxmox 50 호스트 관리 봇. LXC/VM 상태, 인프라 헬스체크, AI 질의 등을 처리합니다.",
            &[
                ("status", "서버 상태 확인"),
                ("help", "사용 가능한 명령어 목록"),
                ("ask", "AI에게 질문"),
                ("lxc", "LXC 컨테이너 목록/상태"),
                ("health", "인프라 헬스체크"),
            ],
        ),
        (
            "openclaw-devops",
            "DevOps 인프라 모니터링 봇",
            "IP 확인, DNS 조회, 서비스 상태 모니터링, 인프라 알림을 처리하는 DevOps 봇입니다.",
            &[
                ("ip", "공인 IP 확인"),
                ("status", "서비스 상태 확인"),
                ("health", "인프라 헬스체크"),
                ("dns", "DNS 레코드 조회"),
                ("alert", "최근 알림 확인"),
            ],
        ),
        (
            "openclaw-synology",
            "Synology NAS 관리 봇",
            "Synology NAS 상태 확인, 공유 폴더 관리, 스토리지 풀 모니터링을 처리합니다.",
            &[
                ("status", "NAS 상태 확인"),
                ("shares", "공유 폴더 목록"),
                ("pools", "스토리지 풀 정보"),
                ("disk", "디스크 사용량"),
                ("help", "사용 가능한 명령어"),
            ],
        ),
        (
            "openclaw-command-install",
            "서비스 설치/배포 봇",
            "LXC/VM에 서비스 설치, 패키지 업데이트, 배포 자동화를 처리하는 봇입니다.",
            &[
                ("install", "패키지/서비스 설치"),
                ("update", "패키지 업데이트"),
                ("list", "설치된 서비스 목록"),
                ("deploy", "서비스 배포"),
                ("help", "사용 가능한 명령어"),
            ],
        ),
        (
            "openclaw-image-gen",
            "AI 이미지 생성 봇",
            "ComfyUI 기반 AI 이미지 생성 봇. 텍스트 프롬프트로 이미지를 생성합니다.",
            &[
                ("generate", "이미지 생성"),
                ("style", "스타일 변경"),
                ("status", "상태 확인"),
                ("help", "사용법"),
            ],
        ),
        (
            "openclaw-gitlab",
            "GitLab CI/CD 관리 봇",
            "GitLab 파이프라인, MR, 이슈, 배포 관리 봇. CI/CD 상태 확인, 로그 조회를 텔레그램에서 처리합니다.",
            &[
                ("status", "GitLab 서비스 상태"),
                ("pipelines", "최근 파이프라인 목록"),
                ("mrs", "오픈 Merge Request 목록"),
                ("issues", "이슈 목록"),
                ("deploy", "배포 상태/트리거"),
                ("logs", "최근 로그 조회"),
                ("help", "사용 가능한 명령어"),
            ],
        ),
        (
            "openclaw-obsidian",
            "Obsidian 노트 관리 봇",
            "Obsidian 볼트 검색, 메모 저장, 노트 관리를 텔레그램에서 처리합니다.",
            &[
                ("search", "노트 검색"),
                ("recent", "최근 수정된 노트"),
                ("memo", "빠른 메모 저장"),
                ("status", "볼트 상태"),
                ("help", "사용법"),
            ],
        ),
        (
            "personal",
            "개인 AI 어시스턴트",
            "개인용 AI 어시스턴트 봇. 질문, 메모, 상태 확인 등을 처리합니다.",
            &[
                ("status", "상태 확인"),
                ("ask", "AI에게 질문"),
                ("memo", "메모 저장/조회"),
                ("help", "사용 가능한 명령어"),
            ],
        ),
    ];

    for (label, short_desc, about, commands) in bot_commands {
        let token = match bots.iter().find(|(l, _)| l == label) {
            Some((_, t)) => t.clone(),
            None => {
                println!("[{label}] 토큰 없음 — 건너뜀");
                continue;
            }
        };

        // setMyCommands
        let cmds_json: Vec<serde_json::Value> = commands
            .iter()
            .map(|(cmd, desc)| serde_json::json!({"command": cmd, "description": desc}))
            .collect();
        let payload = serde_json::json!({"commands": cmds_json});
        let cmd_ok = bot_api_post(&token, "setMyCommands", &payload)
            .ok()
            .and_then(|r| r["ok"].as_bool())
            .unwrap_or(false);

        // setMyShortDescription
        let _ = bot_api_post(&token, "setMyShortDescription", &serde_json::json!({"short_description": short_desc}));

        // setMyDescription
        let _ = bot_api_post(&token, "setMyDescription", &serde_json::json!({"description": about}));

        let status = if cmd_ok { "OK" } else { "FAIL" };
        println!("[{label}] 명령어 {status}, 설명 설정 완료");
    }

    println!("\n=== 일괄 설정 완료 ===");
}

// ─── Assign ─────────────────────────────────────────────────────────────────

fn assign(bot_name: &str, target: &str, channel: Option<&str>) {
    let token = match get_token(bot_name) {
        Ok(t) => t,
        Err(e) => { eprintln!("[telegram] {e}"); return; }
    };

    let username = bot_api_get(&token, "getMe")
        .and_then(|me| me["username"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    match target {
        "claude" | "claude-code" => {
            let ch_name = channel.unwrap_or(bot_name);
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
            let ch_dir = format!("{home}/.claude/channels/{ch_name}");
            fs::create_dir_all(&ch_dir).ok();

            let env_path = format!("{ch_dir}/.env");
            let env_content = format!(
                "TELEGRAM_BOT_TOKEN={token}\n# @{username} — assigned by: pxi-telegram assign\n"
            );
            fs::write(&env_path, &env_content).ok();
            let _ = common::run("chmod", &["600", &env_path]);

            println!("[telegram] {bot_name} (@{username}) → Claude Code 채널 '{ch_name}'");
            println!("  경로: {env_path}");
            println!("  시작: claude --channel {ch_name}");
        }
        "openclaw" => {
            let vmid = match resolve_openclaw_vmid() {
                Ok(v) => v,
                Err(e) => { eprintln!("[telegram] {e}"); return; }
            };

            if !lxc_is_running(&vmid) {
                eprintln!("[telegram] OpenClaw LXC {vmid} 실행 중 아님");
                return;
            }

            let account = channel.unwrap_or(bot_name);
            let cmd = format!(
                "export PATH=/usr/local/bin:$PATH && openclaw channels add --channel telegram --account {account} --token {token} 2>&1"
            );
            let out = lxc_exec(&vmid, &cmd);
            println!("{}", out.trim());
            if out.contains("error") || out.contains("Error") {
                eprintln!("[telegram] OpenClaw 채널 등록 실패");
            } else {
                println!(
                    "\n[telegram] {bot_name} (@{username}) → OpenClaw 채널 '{account}' 등록 완료"
                );
            }
        }
        _ => {
            eprintln!("[telegram] 대상은 'claude' 또는 'openclaw' 중 선택하세요");
        }
    }
}

// ─── Channels ───────────────────────────────────────────────────────────────

fn list_channels() {
    println!("=== 텔레그램 채널 할당 현황 ===\n");

    // Claude Code 채널
    println!("── Claude Code 채널 ──\n");
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let ch_dir = format!("{home}/.claude/channels");
    if let Ok(entries) = fs::read_dir(&ch_dir) {
        let mut found = false;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let env_path = format!("{ch_dir}/{name}/.env");
            if let Ok(content) = fs::read_to_string(&env_path) {
                let token_line = content.lines().find(|l| l.starts_with("TELEGRAM_BOT_TOKEN="));
                if let Some(line) = token_line {
                    let token = line.trim_start_matches("TELEGRAM_BOT_TOKEN=");
                    let token_id = token.split(':').next().unwrap_or("?");
                    let username = bot_api_get(token, "getMe")
                        .and_then(|me| me["username"].as_str().map(|s| format!("@{s}")))
                        .unwrap_or_else(|| "?".to_string());
                    println!("  {name:<25} {username:<30} token_id={token_id}");
                    found = true;
                }
            }
        }
        if !found {
            println!("  (없음)");
        }
    } else {
        println!("  (채널 디렉토리 없음)");
    }

    // OpenClaw 채널
    println!("\n── OpenClaw 채널 ──\n");
    if let Ok(vmid) = resolve_openclaw_vmid() {
        if lxc_is_running(&vmid) {
            let out = lxc_exec(&vmid, "export PATH=/usr/local/bin:$PATH && openclaw channels list 2>&1");
            for line in out.lines() {
                if line.contains("Telegram") {
                    println!("  {}", line.trim().trim_start_matches("- "));
                }
            }
        } else {
            println!("  OpenClaw LXC {vmid} 실행 중 아님");
        }
    } else {
        println!("  OPENCLAW_VMID 환경변수 미설정");
    }
}

// ─── Bot Register (.env에 등록) ─────────────────────────────────────────────

fn bot_register(label: &str, token: &str) {
    // 토큰 검증
    let username = match bot_api_get(token, "getMe") {
        Some(me) => me["username"].as_str().unwrap_or("?").to_string(),
        None => {
            eprintln!("[telegram] 토큰이 유효하지 않습니다");
            return;
        }
    };

    let token_id = token.split(':').next().unwrap_or("?");

    // pxi config telegram.json 에도 등록
    let mut bots = load_bots().unwrap_or(serde_json::json!({"bots": {}}));
    let bot_label = if label.starts_with("openclaw-") { label.to_string() } else { format!("openclaw-{label}") };
    bots["bots"][&bot_label] = serde_json::json!(token);
    if let Err(e) = save_bots(&bots) {
        eprintln!("[telegram] config 저장 실패: {e}");
        return;
    }

    // .env에도 추가 (호환용)
    if let Ok(env_path) = paths::env_file() {
        let env_key = format!(
            "OPENCLAW_TELEGRAM_TOKEN_{}",
            label.to_uppercase().replace('-', "_")
        );
        let content = fs::read_to_string(&env_path).unwrap_or_default();
        if content.contains(&env_key) {
            let new_content: String = content.lines()
                .map(|l| {
                    if l.starts_with(&format!("{env_key}=")) {
                        format!("{env_key}={token}")
                    } else {
                        l.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            let _ = fs::write(&env_path, format!("{new_content}\n"));
        } else {
            use std::io::Write;
            if let Ok(mut f) = fs::OpenOptions::new().append(true).create(true).open(&env_path) {
                let _ = writeln!(f, "{env_key}={token}");
            }
        }
        println!("ENV 변수 : {env_key}");
    }

    println!("=== 봇 등록 완료 ===\n");
    println!("라벨     : {bot_label}");
    println!("봇       : @{username} (토큰 ID: {token_id})");
    println!("\n다음 단계:");
    println!("  pxi run telegram setup-all                                               # 명령어/설명 설정");
    println!("  pxi run telegram assign --bot {bot_label} --target claude --channel {label}  # Claude Code 할당");
}

// ─── Bot Rename ─────────────────────────────────────────────────────────────

fn bot_rename(bot_name: &str, display_name: &str) {
    let (label, token) = match get_token_fuzzy(bot_name) {
        Ok(t) => t,
        Err(e) => { eprintln!("[telegram] {e}"); return; }
    };

    let body = serde_json::json!({"name": display_name});
    match bot_api_post(&token, "setMyName", &body) {
        Ok(resp) if resp["ok"].as_bool() == Some(true) => {
            let username = bot_api_get(&token, "getMe")
                .and_then(|me| me["username"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "?".to_string());
            println!("[telegram] @{username} ({label}) → \"{display_name}\" 변경 완료");
        }
        Ok(resp) => {
            eprintln!("[telegram] 이름 변경 실패: {}", resp.get("description").and_then(|d| d.as_str()).unwrap_or("?"));
        }
        Err(e) => {
            eprintln!("[telegram] 이름 변경 실패: {e}");
        }
    }
}

// ─── Doctor ─────────────────────────────────────────────────────────────────

fn doctor() {
    println!("=== pxi-telegram doctor ===");
    println!("  curl:    {}", if common::has_cmd("curl") { "✓" } else { "✗" });
    match bots_path() {
        Ok(p) => println!("  config:  {} ({})", p.display(), if p.exists() { "존재" } else { "없음" }),
        Err(e) => println!("  config:  ✗ {e}"),
    }
    if let Ok(bots) = load_bots() {
        let count = bots["bots"].as_object().map(|o| o.len()).unwrap_or(0);
        println!("  등록된 봇: {count}개");
    }
    // LXC 환경 검사
    let pct = common::has_cmd("pct");
    println!("  pct:     {}", if pct { "✓ (Proxmox LXC 지원)" } else { "✗ (LXC 비지원 — assign/pairing/status 일부 제한)" });
    if let Ok(vmid) = resolve_openclaw_vmid() {
        let running = lxc_is_running(&vmid);
        println!("  openclaw: LXC {vmid} ({})", if running { "running" } else { "stopped" });
    } else {
        println!("  openclaw: OPENCLAW_VMID 미설정");
    }
}

// ─── LXC 헬퍼 ──────────────────────────────────────────────────────────────

fn resolve_openclaw_vmid() -> anyhow::Result<String> {
    let raw = std::env::var("OPENCLAW_VMID")
        .map_err(|_| anyhow::anyhow!("OPENCLAW_VMID 환경변수 미설정"))?;
    if raw.is_empty() {
        anyhow::bail!("OPENCLAW_VMID 비어있음");
    }
    if raw.len() >= 5 { Ok(raw) } else { Ok(format!("50{raw}")) }
}

fn lxc_is_running(vmid: &str) -> bool {
    common::run("pct", &["status", vmid])
        .map(|s| s.contains("running"))
        .unwrap_or(false)
}

fn lxc_exec(vmid: &str, script: &str) -> String {
    common::run("pct", &["exec", vmid, "--", "bash", "-c", script])
        .unwrap_or_default()
}

