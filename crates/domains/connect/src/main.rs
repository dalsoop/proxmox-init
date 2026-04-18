//! pxi-connect — 외부 서비스 연결 관리 (.env + dotenvx 암호화)

use clap::{Parser, Subcommand};
use pxi_core::{common, dotenvx, paths};
use std::fs;
use std::io::Write;

#[derive(Parser)]
#[command(name = "pxi-connect", about = "외부 서비스 연결 관리 (.env + dotenvx)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 키/값 추가
    Set { key: String, value: String },
    /// 키 제거
    Remove { key: String },
    /// 전체 목록 (값은 마스킹)
    List,
    /// dotenvx로 .env.vault 암호화
    Encrypt,
    Doctor,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Set { key, value } => set(&key, &value),
        Cmd::Remove { key } => remove(&key),
        Cmd::List => list(),
        Cmd::Encrypt => encrypt(),
        Cmd::Doctor => { doctor(); Ok(()) }
    }
}

fn env_path() -> anyhow::Result<std::path::PathBuf> {
    let p = paths::env_file()?;
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    if !p.exists() {
        fs::write(&p, "")?;
    }
    Ok(p)
}

fn set(key: &str, value: &str) -> anyhow::Result<()> {
    let path = env_path()?;
    let raw = fs::read_to_string(&path).unwrap_or_default();
    let prefix = format!("{key}=");
    let lines: Vec<String> = raw.lines()
        .filter(|l| !l.starts_with(&prefix))
        .map(String::from)
        .collect();
    let mut file = fs::OpenOptions::new().write(true).truncate(true).create(true).open(&path)?;
    for l in &lines {
        writeln!(file, "{l}")?;
    }
    writeln!(file, "{key}={value}")?;
    println!("✓ {key} 저장 ({})", path.display());
    Ok(())
}

fn remove(key: &str) -> anyhow::Result<()> {
    let path = env_path()?;
    let raw = fs::read_to_string(&path)?;
    let prefix = format!("{key}=");
    let lines: Vec<&str> = raw.lines().filter(|l| !l.starts_with(&prefix)).collect();
    fs::write(&path, lines.join("\n") + "\n")?;
    println!("✓ {key} 제거");
    Ok(())
}

fn list() -> anyhow::Result<()> {
    let path = env_path()?;
    let raw = fs::read_to_string(&path)?;
    println!("=== {} ===", path.display());
    for line in raw.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') { continue; }
        if let Some((k, v)) = line.split_once('=') {
            let masked = if v.len() > 6 {
                format!("{}...{}", &v[..3], &v[v.len()-3..])
            } else {
                "***".into()
            };
            println!("  {k} = {masked}");
        }
    }
    Ok(())
}

fn encrypt() -> anyhow::Result<()> {
    if !dotenvx::is_installed() {
        anyhow::bail!("dotenvx 미설치 — pxi install bootstrap");
    }
    let path = env_path()?;
    dotenvx::encrypt(&path)?;
    println!("✓ {} → .env.vault 암호화", path.display());
    Ok(())
}

fn doctor() {
    println!("=== pxi-connect doctor ===");
    println!("  dotenvx:  {}", if dotenvx::is_installed() { "✓" } else { "✗ (pxi install bootstrap)" });
    match paths::env_file() {
        Ok(p) => println!("  env:      {} ({})", p.display(), if p.exists() { "✓" } else { "없음" }),
        Err(e) => println!("  env:      ✗ {e}"),
    }
    println!("  curl:     {}", if common::has_cmd("curl") { "✓" } else { "✗" });
}
