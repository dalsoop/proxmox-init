//! prelik-account — 범용 리눅스 계정 관리.
//! create/remove/list/status/ssh-key-add. dalsoop "dalroot" 없는 일반화.

use clap::{Parser, Subcommand};
use prelik_core::common;

#[derive(Parser)]
#[command(name = "prelik-account", about = "리눅스 계정 관리")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 사용자 계정 생성
    Create {
        name: String,
        /// sudo 권한 부여 (sudoers.d에 추가)
        #[arg(long)]
        sudo: bool,
        /// SSH 공개키 (파일 경로 또는 직접 문자열, ssh-rsa/ssh-ed25519로 시작)
        #[arg(long)]
        ssh_key: Option<String>,
        /// 셸 (기본: /bin/bash)
        #[arg(long, default_value = "/bin/bash")]
        shell: String,
    },
    /// 사용자 계정 제거
    Remove {
        name: String,
        /// 홈 디렉토리까지 완전 삭제 (기본: 보존)
        #[arg(long)]
        purge: bool,
    },
    /// 사용자 목록 (UID >= 1000만, 시스템 계정 제외)
    List,
    /// 사용자 상태 (홈 디렉토리, sudo, SSH 키)
    Status { name: String },
    /// SSH 공개키 추가 (authorized_keys에 append, 중복 방지)
    SshKeyAdd {
        name: String,
        /// 공개키 (파일 경로 또는 직접 문자열)
        #[arg(long)]
        key: String,
    },
    Doctor,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Create { name, sudo, ssh_key, shell } => {
            create(&name, sudo, ssh_key.as_deref(), &shell)
        }
        Cmd::Remove { name, purge } => remove(&name, purge),
        Cmd::List => {
            list();
            Ok(())
        }
        Cmd::Status { name } => {
            status(&name);
            Ok(())
        }
        Cmd::SshKeyAdd { name, key } => ssh_key_add(&name, &key),
        Cmd::Doctor => {
            doctor();
            Ok(())
        }
    }
}

fn create(name: &str, sudo: bool, ssh_key: Option<&str>, shell: &str) -> anyhow::Result<()> {
    validate_name(name)?;
    println!("=== 계정 생성: {name} ===");

    // 이미 존재하는지
    if common::run("id", &["-u", name]).is_ok() {
        anyhow::bail!("이미 존재: {name}. remove 후 재생성 또는 다른 이름.");
    }

    // useradd
    common::run("sudo", &[
        "useradd", "-m", "-s", shell, name,
    ])?;
    println!("  ✓ 사용자 + 홈 디렉토리");

    // sudo
    if sudo {
        let sudoers_file = format!("/etc/sudoers.d/prelik-{name}");
        let content = format!("{name} ALL=(ALL) NOPASSWD:ALL\n");

        // tempfile 경유 + visudo -cf로 검증 후 설치
        let tmp = common::run("mktemp", &["-t", "prelik.XXXXXXXX"])?.trim().to_string();
        let tmp_path = tmp.clone();
        struct Cleanup(String);
        impl Drop for Cleanup { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }
        let _g = Cleanup(tmp_path);

        std::fs::write(&tmp, content)?;
        // visudo 검증
        common::run("sudo", &["visudo", "-cf", &tmp])?;
        common::run("sudo", &[
            "install", "-m", "440", "-o", "root", "-g", "root", &tmp, &sudoers_file,
        ])?;
        println!("  ✓ sudo 권한: {sudoers_file}");
    }

    // SSH 키
    if let Some(key) = ssh_key {
        ssh_key_add(name, key)?;
    }

    println!("\n✓ {name} 생성 완료");
    status(name);
    Ok(())
}

fn remove(name: &str, purge: bool) -> anyhow::Result<()> {
    validate_name(name)?;
    println!("=== 계정 제거: {name} (purge={purge}) ===");

    // 존재 여부
    if common::run("id", &["-u", name]).is_err() {
        println!("  ⊘ {name} 이미 없음");
        return Ok(());
    }

    // sudoers 제거
    let sudoers_file = format!("/etc/sudoers.d/prelik-{name}");
    if std::path::Path::new(&sudoers_file).exists() {
        common::run("sudo", &["rm", "-f", &sudoers_file])?;
        println!("  ✓ sudoers.d 제거");
    }

    // userdel
    let userdel_args: Vec<&str> = if purge {
        vec!["userdel", "-r", name]
    } else {
        vec!["userdel", name]
    };
    common::run("sudo", &userdel_args)?;
    println!("  ✓ 사용자 삭제");

    if purge {
        println!("  ⚠ 홈 디렉토리 완전 제거됨");
    } else {
        println!("  (홈 디렉토리 보존됨 — --purge로 완전 삭제 가능)");
    }
    Ok(())
}

fn list() {
    println!("=== 사용자 목록 (UID >= 1000) ===");
    match common::run_bash("awk -F: '$3 >= 1000 && $3 < 60000 { print $1, $3, $6 }' /etc/passwd") {
        Ok(out) => {
            if out.trim().is_empty() {
                println!("  (없음)");
            } else {
                println!("  {:<20} {:<8} {}", "NAME", "UID", "HOME");
                for line in out.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() == 3 {
                        println!("  {:<20} {:<8} {}", parts[0], parts[1], parts[2]);
                    }
                }
            }
        }
        Err(e) => eprintln!("  ✗ {e}"),
    }
}

fn status(name: &str) {
    println!("=== {name} 상태 ===");
    match common::run("id", &[name]) {
        Ok(out) => println!("  id:    {}", out.trim()),
        Err(_) => { println!("  ✗ 존재하지 않음"); return; }
    }
    if let Ok(home) = common::run("getent", &["passwd", name]) {
        if let Some(h) = home.split(':').nth(5) {
            println!("  home:  {} (exists: {})", h, std::path::Path::new(h).exists());
        }
    }
    let sudoers = format!("/etc/sudoers.d/prelik-{name}");
    println!("  sudo:  {}", if std::path::Path::new(&sudoers).exists() { "✓" } else { "✗" });

    if let Ok(home) = common::run_bash(&format!("getent passwd {name} | cut -d: -f6")) {
        let auth_keys = format!("{}/.ssh/authorized_keys", home.trim());
        if std::path::Path::new(&auth_keys).exists() {
            let count = common::run_bash(&format!("sudo wc -l < {auth_keys} 2>/dev/null"))
                .unwrap_or_default();
            println!("  ssh keys: ✓ ({} 줄)", count.trim());
        } else {
            println!("  ssh keys: ✗");
        }
    }
}

fn ssh_key_add(name: &str, key: &str) -> anyhow::Result<()> {
    validate_name(name)?;

    // key가 파일 경로면 읽기, 아니면 문자열 그대로
    let key_content = if std::path::Path::new(key).exists() {
        std::fs::read_to_string(key)?
    } else {
        key.to_string()
    };
    let key_content = key_content.trim();

    // 형식 검증
    if !key_content.starts_with("ssh-") {
        anyhow::bail!("올바른 SSH 공개키 형식 아님 (ssh-rsa/ssh-ed25519/ssh-ecdsa로 시작해야)");
    }

    println!("=== {name}에 SSH 키 추가 ===");

    let home = common::run_bash(&format!("getent passwd {name} | cut -d: -f6"))?
        .trim().to_string();
    if home.is_empty() {
        anyhow::bail!("{name} 홈 디렉토리 조회 실패");
    }

    let ssh_dir = format!("{home}/.ssh");
    let auth_keys = format!("{ssh_dir}/authorized_keys");

    common::run("sudo", &["mkdir", "-p", &ssh_dir])?;
    common::run("sudo", &["chmod", "700", &ssh_dir])?;
    common::run("sudo", &["chown", &format!("{name}:{name}"), &ssh_dir])?;

    // 중복 방지 — fingerprint 비교 어려우니 string 전체로
    let check = std::process::Command::new("sudo")
        .args(["grep", "-qF", key_content, &auth_keys])
        .status();
    if check.ok().map(|s| s.success()).unwrap_or(false) {
        println!("  ⊘ 이미 등록된 키");
        return Ok(());
    }

    // append (fstab과 같은 패턴 — tee -a + EOF \n 체크)
    let last_byte = std::process::Command::new("sudo")
        .args(["sh", "-c", &format!("tail -c1 {auth_keys} 2>/dev/null | od -An -tx1 | tr -d ' \\n'")])
        .output().ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let prefix = if !last_byte.is_empty() && last_byte != "0a" { "\n" } else { "" };
    let line = format!("{prefix}{key_content}\n");

    let output = std::process::Command::new("sudo")
        .args(["tee", "-a", &auth_keys])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(line.as_bytes())?;
            }
            child.wait()
        });

    match output {
        Ok(status) if status.success() => {
            common::run("sudo", &["chmod", "600", &auth_keys])?;
            common::run("sudo", &["chown", &format!("{name}:{name}"), &auth_keys])?;
            println!("  ✓ 키 추가 + 권한 정리");
            Ok(())
        }
        Ok(status) => anyhow::bail!("키 추가 실패 (exit {})", status.code().unwrap_or(-1)),
        Err(e) => anyhow::bail!("sudo tee 실행 실패: {e}"),
    }
}

fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() || name.len() > 32 {
        anyhow::bail!("사용자명 길이 1~32자 필요");
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        anyhow::bail!("사용자명은 영숫자 + -_ 만 허용");
    }
    if !name.chars().next().unwrap().is_ascii_lowercase() && !name.starts_with('_') {
        anyhow::bail!("사용자명은 소문자 또는 _로 시작해야 (POSIX)");
    }
    Ok(())
}

fn doctor() {
    println!("=== prelik-account doctor ===");
    for (name, cmd) in &[
        ("useradd", "useradd"),
        ("userdel", "userdel"),
        ("visudo", "visudo"),
        ("getent", "getent"),
    ] {
        println!("  {} {name}", if common::has_cmd(cmd) { "✓" } else { "✗" });
    }
}
