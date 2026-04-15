//! prelik-iso — Proxmox ISO 스토리지 + ISO 파일 관리 (pvesm 래퍼).

use clap::{Parser, Subcommand};
use prelik_core::common;

#[derive(Parser)]
#[command(name = "prelik-iso", about = "Proxmox ISO 스토리지 관리")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 의존성 점검 (pvesm)
    Doctor,
    /// 모든 ISO 스토리지 + ISO 파일 목록
    List {
        /// 특정 스토리지만
        #[arg(long)]
        storage: Option<String>,
    },
    /// NFS ISO 스토리지 등록
    StorageAddNfs {
        id: String,
        #[arg(long)]
        server: String,
        #[arg(long)]
        export: String,
        #[arg(long, default_value = "4.2")]
        nfs_version: String,
    },
    /// CIFS/SMB ISO 스토리지 등록
    StorageAddCifs {
        id: String,
        #[arg(long)]
        server: String,
        #[arg(long)]
        share: String,
        #[arg(long)]
        username: String,
        /// 비밀번호 (생략 시 SMB_PASSWORD 환경변수)
        #[arg(long)]
        password: Option<String>,
        #[arg(long, default_value = "3.0")]
        smb_version: String,
    },
    /// ISO 스토리지 삭제 (스토리지 정의만 — 파일은 보존)
    StorageRemove { id: String },
    /// URL에서 ISO 다운로드 (pvesm download-url 사용)
    Download {
        /// 저장할 파일명 (확장자 .iso 포함)
        filename: String,
        #[arg(long)]
        url: String,
        /// 대상 스토리지 ID (기본 local)
        #[arg(long, default_value = "local")]
        storage: String,
        /// SHA-256 체크섬 (선택)
        #[arg(long)]
        checksum: Option<String>,
    },
    /// ISO 파일 삭제
    Remove {
        /// volid 형식: storage:iso/filename.iso
        volid: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Doctor => doctor(),
        Cmd::List { storage } => list(storage.as_deref()),
        Cmd::StorageAddNfs { id, server, export, nfs_version } => {
            storage_add_nfs(&id, &server, &export, &nfs_version)
        }
        Cmd::StorageAddCifs {
            id,
            server,
            share,
            username,
            password,
            smb_version,
        } => storage_add_cifs(&id, &server, &share, &username, password.as_deref(), &smb_version),
        Cmd::StorageRemove { id } => storage_remove(&id),
        Cmd::Download { filename, url, storage, checksum } => {
            download(&filename, &url, &storage, checksum.as_deref())
        }
        Cmd::Remove { volid } => remove(&volid),
    }
}

fn doctor() -> anyhow::Result<()> {
    println!("=== prelik-iso doctor ===");
    let pvesm = which("pvesm");
    println!("  pvesm     : {}", if pvesm { "✓" } else { "✗ (Proxmox 호스트 필요)" });
    if !pvesm {
        println!("\n참고: prelik-iso는 Proxmox VE 호스트에서만 동작합니다.");
    }
    // 다른 도메인의 doctor와 일관성: 누락은 보고만 하고 정상 종료 (CI smoke 호환)
    Ok(())
}

fn which(bin: &str) -> bool {
    // 외부 which 바이너리 의존 회피 — PATH 직접 탐색 (which crate)
    ::which::which(bin).is_ok()
}

fn list(filter_storage: Option<&str>) -> anyhow::Result<()> {
    println!("=== ISO 스토리지 ===");
    common::run("pvesm", &["status", "--content", "iso"])?;
    println!("\n=== ISO 파일 ===");
    let storages = list_iso_storages()?;
    if storages.is_empty() {
        println!("(ISO content 지원하는 스토리지 없음)");
        return Ok(());
    }
    for s in &storages {
        if let Some(f) = filter_storage {
            if f != s {
                continue;
            }
        }
        println!("\n--- {s} ---");
        let _ = common::run("pvesm", &["list", s, "--content", "iso"]);
    }
    Ok(())
}

fn list_iso_storages() -> anyhow::Result<Vec<String>> {
    let out = std::process::Command::new("pvesm")
        .args(["status", "--content", "iso"])
        .output()?;
    if !out.status.success() {
        anyhow::bail!("pvesm status 실패");
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut ids = Vec::new();
    for line in text.lines().skip(1) {
        if let Some(id) = line.split_whitespace().next() {
            ids.push(id.to_string());
        }
    }
    Ok(ids)
}

fn storage_add_nfs(id: &str, server: &str, export: &str, nfs_version: &str) -> anyhow::Result<()> {
    println!("=== NFS ISO 스토리지 등록: {id} ===");
    common::run(
        "pvesm",
        &[
            "add", "nfs", id,
            "--server", server,
            "--export", export,
            "--content", "iso",
            "--options", &format!("vers={nfs_version}"),
        ],
    )?;
    println!("✓ {id} 등록 완료 (참조: {id}:iso/<filename>.iso)");
    Ok(())
}

fn storage_add_cifs(
    id: &str,
    server: &str,
    share: &str,
    username: &str,
    password: Option<&str>,
    smb_version: &str,
) -> anyhow::Result<()> {
    println!("=== CIFS ISO 스토리지 등록: {id} ===");
    let pw = match password {
        Some(p) => p.to_string(),
        None => std::env::var("SMB_PASSWORD")
            .map_err(|_| anyhow::anyhow!("--password 또는 SMB_PASSWORD 환경변수 필요"))?,
    };
    // 비밀번호는 argv로 전달 (pvesm 한계). 단, 실패 시 에러 메시지에 평문이 남지 않도록
    // common::run을 우회해서 직접 spawn하고 stdout/stderr만 통과시킨다 (Display 마스킹).
    let status = std::process::Command::new("pvesm")
        .args([
            "add", "cifs", id,
            "--server", server,
            "--share", share,
            "--username", username,
            "--password", &pw,
            "--content", "iso",
            "--smbversion", smb_version,
        ])
        .status()
        .map_err(|e| anyhow::anyhow!("pvesm spawn 실패: {e}"))?;
    if !status.success() {
        // argv 비공개 — 에러는 종료 코드만 노출
        anyhow::bail!(
            "pvesm add cifs {id} 실패 (exit {}). server/share/username/SMB 권한을 확인하세요. \
             (--password 평문 보호를 위해 argv는 메시지에 포함하지 않음)",
            status.code().unwrap_or(-1)
        );
    }
    println!("✓ {id} 등록 완료 (참조: {id}:iso/<filename>.iso)");
    Ok(())
}

fn storage_remove(id: &str) -> anyhow::Result<()> {
    println!("=== 스토리지 정의 삭제: {id} ===");
    println!("(파일은 보존됩니다)");
    common::run("pvesm", &["remove", id])?;
    println!("✓ {id} 정의 삭제됨");
    Ok(())
}

fn download(filename: &str, url: &str, storage: &str, checksum: Option<&str>) -> anyhow::Result<()> {
    println!("=== ISO 다운로드: {filename} → {storage} ===");
    println!("URL: {url}");
    let mut args: Vec<String> = vec![
        "download-url".into(),
        "--content".into(),
        "iso".into(),
    ];
    if let Some(c) = checksum {
        args.push("--checksum".into());
        args.push(c.to_string());
        args.push("--checksum-algorithm".into());
        args.push("sha256".into());
    }
    args.push(storage.to_string());
    args.push(filename.to_string());
    args.push(url.to_string());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    common::run("pvesm", &refs)?;
    println!("✓ 다운로드 완료 — 참조: {storage}:iso/{filename}");
    Ok(())
}

fn remove(volid: &str) -> anyhow::Result<()> {
    if !volid.contains(":iso/") {
        anyhow::bail!("volid는 'storage:iso/filename.iso' 형식이어야 합니다");
    }
    println!("=== ISO 파일 삭제: {volid} ===");
    common::run("pvesm", &["free", volid])?;
    println!("✓ {volid} 삭제됨");
    Ok(())
}
