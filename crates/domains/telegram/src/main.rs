//! prelik-telegram — 봇 토큰 저장 + 알림 발송 (범용).
//! phs의 dalsoop-specific 봇 8개가 아닌, 임의 봇 N개 관리.

use clap::{Parser, Subcommand};
use prelik_core::{common, paths};
use std::fs;

#[derive(Parser)]
#[command(name = "prelik-telegram", about = "Telegram 봇 관리")]
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
    /// 등록된 봇 목록 (토큰은 마스킹)
    List,
    /// 봇 등록 해제
    Remove { name: String },
    /// getMe로 토큰 검증
    Verify { bot: String },
    Doctor,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Register { name, token } => register(&name, &token),
        Cmd::Send { bot, chat, text, markdown } => send(&bot, &chat, text.as_deref(), markdown),
        Cmd::List => {
            list()?;
            Ok(())
        }
        Cmd::Remove { name } => remove(&name),
        Cmd::Verify { bot } => verify(&bot),
        Cmd::Doctor => {
            doctor();
            Ok(())
        }
    }
}

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
    // 같은 디렉토리에 tempfile 생성 → rename (cross-device 회피, 원자적 교체)
    let parent = p.parent()
        .ok_or_else(|| anyhow::anyhow!("부모 디렉토리 없음"))?;
    fs::create_dir_all(parent)?;

    let tmp = parent.join(format!(".telegram.json.tmp.{}", std::process::id()));
    // 쓰기 + 권한 설정
    fs::write(&tmp, serde_json::to_string_pretty(v)?)?;
    common::run("chmod", &["600", &tmp.display().to_string()])?;

    // 원자적 교체 (같은 FS라서 rename OK)
    if let Err(e) = fs::rename(&tmp, &p) {
        let _ = fs::remove_file(&tmp);
        return Err(anyhow::anyhow!("config 저장 실패: {e}"));
    }
    Ok(())
}

fn register(name: &str, token: &str) -> anyhow::Result<()> {
    // 토큰 형식 검증 — BotFather: <number>:<hash>
    if !token.contains(':') || token.len() < 30 {
        anyhow::bail!("토큰 형식 비정상 (BotFather 토큰 아님?)");
    }

    let mut bots = load_bots()?;
    bots["bots"][name] = serde_json::json!(token);
    save_bots(&bots)?;
    println!("✓ 봇 등록: {name} ({})", mask_token(token));
    Ok(())
}

fn send(bot: &str, chat: &str, text: Option<&str>, markdown: bool) -> anyhow::Result<()> {
    let bots = load_bots()?;
    let token = bots["bots"][bot].as_str()
        .ok_or_else(|| anyhow::anyhow!("등록된 봇 없음: {bot} (prelik run telegram register)"))?;

    // 메시지 — 인자 또는 stdin
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

    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let mut data = serde_json::json!({
        "chat_id": chat,
        "text": message,
    });
    if markdown {
        data["parse_mode"] = serde_json::json!("MarkdownV2");
    }

    // curl 호출 — JSON body로
    let body = data.to_string();
    let output = std::process::Command::new("curl")
        .args([
            "-sSL", "--fail",
            "-X", "POST",
            "-H", "Content-Type: application/json",
            "-d", &body,
            &url,
        ])
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "발송 실패: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let resp: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    if !resp["ok"].as_bool().unwrap_or(false) {
        anyhow::bail!("Telegram API 실패: {}", resp["description"].as_str().unwrap_or("?"));
    }
    println!("✓ 발송 완료 → {bot}:{chat}");
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
    let bots = load_bots()?;
    let token = bots["bots"][bot].as_str()
        .ok_or_else(|| anyhow::anyhow!("등록된 봇 없음: {bot}"))?;

    let url = format!("https://api.telegram.org/bot{token}/getMe");
    let output = std::process::Command::new("curl")
        .args(["-sSL", "--fail", &url])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("API 호출 실패 — 토큰 무효 or 네트워크");
    }
    let resp: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    if !resp["ok"].as_bool().unwrap_or(false) {
        anyhow::bail!("{}", resp["description"].as_str().unwrap_or("?"));
    }
    let r = &resp["result"];
    println!("✓ 봇 유효");
    println!("  id:       {}", r["id"]);
    println!("  username: @{}", r["username"].as_str().unwrap_or("?"));
    println!("  name:     {}", r["first_name"].as_str().unwrap_or("?"));
    Ok(())
}

fn mask_token(token: &str) -> String {
    if token.len() < 10 {
        "***".into()
    } else {
        format!("{}...{}", &token[..5], &token[token.len()-4..])
    }
}

#[allow(dead_code)]
fn secure_tempfile() -> anyhow::Result<(String, TempGuard)> {
    let out = common::run("mktemp", &["-t", "prelik.XXXXXXXX"])?;
    let tmp = out.trim().to_string();
    let guard = TempGuard(tmp.clone());
    common::run("chmod", &["600", &tmp])?;
    Ok((tmp, guard))
}

#[allow(dead_code)]
struct TempGuard(String);
impl Drop for TempGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn doctor() {
    println!("=== prelik-telegram doctor ===");
    println!("  curl:    {}", if common::has_cmd("curl") { "✓" } else { "✗" });
    match bots_path() {
        Ok(p) => println!("  config:  {} ({})", p.display(), if p.exists() { "존재" } else { "없음" }),
        Err(e) => println!("  config:  ✗ {e}"),
    }
    if let Ok(bots) = load_bots() {
        let count = bots["bots"].as_object().map(|o| o.len()).unwrap_or(0);
        println!("  등록된 봇: {count}개");
    }
}

// Write impl for write_all
