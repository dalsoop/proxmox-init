//! prelik-ai — Claude/Codex CLI + 플러그인 설치.
//! phs의 ai::octopus / superpowers / codex_plugin 이식.

use clap::{Parser, Subcommand};
use prelik_core::common;
use std::fs;
use std::process::Command;

#[derive(Parser)]
#[command(name = "prelik-ai", about = "Claude/Codex CLI + 플러그인 관리")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Claude + Codex CLI 설치 (npm -g)
    Install {
        #[arg(long)]
        vmid: Option<String>,
    },
    /// claude-octopus 플러그인 설치
    OctopusInstall {
        #[arg(long)]
        vmid: Option<String>,
    },
    /// superpowers 스킬 플러그인 설치
    SuperpowersInstall {
        #[arg(long)]
        vmid: Option<String>,
    },
    /// OpenAI Codex Plugin for Claude Code 설치
    CodexPluginInstall {
        #[arg(long)]
        vmid: Option<String>,
        /// 프로그래매틱 호출 가능 fork (훅용)
        #[arg(long)]
        fork: bool,
    },
    /// Claude Stop 훅 — 작업 완료 시 /codex:adversarial-review 자동 실행
    AdversarialReviewHook {
        #[arg(long)]
        disable: bool,
    },
    Doctor,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Install { vmid } => install(vmid.as_deref()),
        Cmd::OctopusInstall { vmid } => octopus_install(vmid.as_deref()),
        Cmd::SuperpowersInstall { vmid } => superpowers_install(vmid.as_deref()),
        Cmd::CodexPluginInstall { vmid, fork } => codex_plugin_install(vmid.as_deref(), fork),
        Cmd::AdversarialReviewHook { disable } => adversarial_review_hook(!disable),
        Cmd::Doctor => { doctor(); Ok(()) }
    }
}

fn run_on(vmid: Option<&str>, script: &str) -> anyhow::Result<String> {
    match vmid {
        Some(v) => common::run("pct", &["exec", v, "--", "bash", "-c", script]),
        None => common::run_bash(script),
    }
}

fn has_on(vmid: Option<&str>, bin: &str) -> bool {
    let script = format!("command -v {bin} >/dev/null 2>&1");
    match vmid {
        Some(v) => Command::new("pct").args(["exec", v, "--", "bash", "-c", &script])
            .status().map(|s| s.success()).unwrap_or(false),
        None => Command::new("bash").args(["-c", &script])
            .status().map(|s| s.success()).unwrap_or(false),
    }
}

fn install(vmid: Option<&str>) -> anyhow::Result<()> {
    println!("=== AI CLI 설치 ===");
    if !has_on(vmid, "npm") {
        anyhow::bail!("npm 없음 — prelik install bootstrap 먼저");
    }
    if !has_on(vmid, "claude") {
        println!("  claude 설치...");
        run_on(vmid, "sudo npm install -g @anthropic-ai/claude-code")?;
    } else {
        println!("  ✓ claude 이미 설치됨");
    }
    if !has_on(vmid, "codex") {
        println!("  codex 설치...");
        run_on(vmid, "sudo npm install -g @openai/codex")?;
    } else {
        println!("  ✓ codex 이미 설치됨");
    }
    println!("✓ AI CLI 설치 완료");
    Ok(())
}

fn octopus_install(vmid: Option<&str>) -> anyhow::Result<()> {
    println!("=== claude-octopus 설치 ===");
    let mut ok = 0;
    if has_on(vmid, "claude") {
        let script = "claude plugin marketplace add https://github.com/nyldn/claude-octopus.git 2>&1 | tail -1; \
                      claude plugin install octo@nyldn-plugins 2>&1 | tail -1";
        match run_on(vmid, script) {
            Ok(out) => { println!("  ✓ claude: {}", out.trim()); ok += 1; }
            Err(e) => println!("  ✗ claude: {e}"),
        }
    }
    if has_on(vmid, "codex") {
        let script = "if [ -d ~/.codex/claude-octopus ]; then \
           cd ~/.codex/claude-octopus && git pull --ff-only 2>&1 | tail -1; \
         else \
           git clone --depth 1 https://github.com/nyldn/claude-octopus.git ~/.codex/claude-octopus 2>&1 | tail -1; \
         fi && \
         mkdir -p ~/.agents/skills && \
         ln -sf ~/.codex/claude-octopus/skills ~/.agents/skills/claude-octopus";
        match run_on(vmid, script) {
            Ok(_) => { println!("  ✓ codex: skills symlink"); ok += 1; }
            Err(e) => println!("  ✗ codex: {e}"),
        }
    }
    if ok == 0 {
        anyhow::bail!("대상에 claude/codex CLI 없음");
    }
    println!("✓ octopus 설치 완료 (다음: /octo:setup)");
    Ok(())
}

fn superpowers_install(vmid: Option<&str>) -> anyhow::Result<()> {
    println!("=== superpowers 설치 ===");
    if !has_on(vmid, "claude") {
        anyhow::bail!("claude CLI 없음");
    }
    let script = "claude plugin install superpowers@claude-plugins-official 2>&1 | tail -1 || ( \
                    claude plugin marketplace add obra/superpowers-marketplace 2>&1 | tail -1 && \
                    claude plugin install superpowers@superpowers-marketplace 2>&1 | tail -1 \
                  )";
    let out = run_on(vmid, script)?;
    println!("  {}", out.trim());
    println!("✓ superpowers 설치 완료 (스킬 자동 활성)");
    Ok(())
}

fn codex_plugin_install(vmid: Option<&str>, fork: bool) -> anyhow::Result<()> {
    println!("=== Codex Plugin for Claude Code 설치 ===");
    if !has_on(vmid, "claude") { anyhow::bail!("claude CLI 없음"); }
    if !has_on(vmid, "codex") { anyhow::bail!("codex CLI 없음 — prelik-ai install 먼저"); }
    let marketplace = if fork { "parthpm/codex-plugin-cc" } else { "openai/codex-plugin-cc" };
    let script = format!(
        "claude plugin marketplace add {marketplace} 2>&1 | tail -1; \
         claude plugin install codex@openai-codex 2>&1 | tail -1"
    );
    let out = run_on(vmid, &script)?;
    println!("  {}", out.trim());
    println!("✓ codex-plugin 설치 완료");
    println!("  다음: claude 세션에서 /codex:setup (OpenAI 인증)");
    if fork {
        println!("  훅 활성화: prelik run ai adversarial-review-hook");
    }
    Ok(())
}

fn adversarial_review_hook(enable: bool) -> anyhow::Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("HOME 미설정"))?;
    let settings_path = home.join(".claude/settings.json");
    println!("=== Adversarial Review Stop 훅 ({}) ===", if enable { "활성화" } else { "비활성화" });

    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = fs::read_to_string(&settings_path).unwrap_or_else(|_| "{}".into());
    let mut v: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::json!({}));
    if !v.is_object() { v = serde_json::json!({}); }
    let obj = v.as_object_mut().unwrap();

    if enable {
        let hook = serde_json::json!({
            "hooks": [{
                "type": "command",
                "command": "F=/tmp/.prelik-adv-review-$(echo $CLAUDE_SESSION_ID | md5sum | cut -c1-8); if [ -f $F ]; then exit 0; fi; touch $F; cat <<EOF\n{\"decision\":\"block\",\"reason\":\"지금까지의 작업을 Codex로 어드버서리얼 리뷰하세요. 실행: /codex:adversarial-review\"}\nEOF"
            }]
        });
        let hooks = obj.entry("hooks".to_string()).or_insert(serde_json::json!({}));
        if let Some(h) = hooks.as_object_mut() {
            h.insert("Stop".into(), serde_json::json!([hook]));
        }
        println!("  Stop 훅 등록 (세션당 1회 가드)");
    } else {
        if let Some(hooks) = obj.get_mut("hooks").and_then(|h| h.as_object_mut()) {
            hooks.remove("Stop");
        }
        println!("  Stop 훅 제거");
    }

    let pretty = serde_json::to_string_pretty(&v)?;
    fs::write(&settings_path, pretty)?;
    println!("✓ {} 업데이트", settings_path.display());
    Ok(())
}

fn doctor() {
    println!("=== prelik-ai doctor ===");
    println!("  npm:    {}", if common::has_cmd("npm") { "✓" } else { "✗" });
    println!("  claude: {}", if common::has_cmd("claude") { "✓" } else { "✗" });
    println!("  codex:  {}", if common::has_cmd("codex") { "✓" } else { "✗" });
    println!("  gemini: {}", if common::has_cmd("gemini") { "✓" } else { "✗" });
}
