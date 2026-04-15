//! prelik-iso — Proxmox ISO 스토리지 + ISO 파일 관리 (pvesm 래퍼).

use clap::{Parser, Subcommand};
use prelik_core::common;
use serde::Serialize;

#[derive(Parser)]
#[command(name = "prelik-iso", about = "Proxmox ISO 스토리지 관리")]
struct Cli {
    /// list를 JSON으로 출력 (자동화/CI 친화)
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Serialize, Debug, PartialEq)]
struct StorageRow {
    name: String,
    storage_type: String,
    status: String,
}

#[derive(Serialize, Debug, PartialEq)]
struct IsoFile {
    volid: String,
    format: String,
    size: u64,
}

#[derive(Serialize)]
struct ListSnap {
    storages: Vec<StorageRow>,
    files: Vec<IsoFileGroup>,
}

#[derive(Serialize)]
struct IsoFileGroup {
    storage: String,
    files: Vec<IsoFile>,
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
    let json = cli.json;
    match cli.cmd {
        Cmd::Doctor => doctor(),
        Cmd::List { storage } => list(storage.as_deref(), json),
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
    common::has_cmd(bin)
}

// 순수 파서 — pvesm status 출력 (Name Type Status Total Used Avail %)
fn parse_pvesm_status(text: &str) -> anyhow::Result<Vec<StorageRow>> {
    let mut rows = Vec::new();
    for line in text.lines().skip(1) {
        if line.trim().is_empty() { continue; }
        let p: Vec<&str> = line.split_whitespace().collect();
        if p.len() < 3 {
            anyhow::bail!("pvesm status 라인 파싱 실패 (컬럼 {}개): {line:?}", p.len());
        }
        rows.push(StorageRow {
            name: p[0].into(),
            storage_type: p[1].into(),
            status: p[2].into(),
        });
    }
    Ok(rows)
}

// 순수 파서 — pvesm list 출력 (Volid Format Type Size [VMID])
fn parse_pvesm_list(text: &str) -> anyhow::Result<Vec<IsoFile>> {
    let mut rows = Vec::new();
    for line in text.lines().skip(1) {
        if line.trim().is_empty() { continue; }
        let p: Vec<&str> = line.split_whitespace().collect();
        if p.len() < 4 {
            anyhow::bail!("pvesm list 라인 파싱 실패 (컬럼 {}개): {line:?}", p.len());
        }
        let size: u64 = p[3].parse()
            .map_err(|_| anyhow::anyhow!("pvesm list size 컬럼이 숫자 아님: {line:?}"))?;
        rows.push(IsoFile {
            volid: p[0].into(),
            format: p[1].into(),
            size,
        });
    }
    Ok(rows)
}

fn list(filter_storage: Option<&str>, json: bool) -> anyhow::Result<()> {
    if !json {
        println!("=== ISO 스토리지 ===");
        common::run("pvesm", &["status", "--content", "iso"])?;
        println!("\n=== ISO 파일 ===");
        let storages = list_iso_storage_ids()?;
        if storages.is_empty() {
            println!("(ISO content 지원하는 스토리지 없음)");
            return Ok(());
        }
        for s in &storages {
            if filter_storage.is_some_and(|f| f != s) { continue; }
            println!("\n--- {s} ---");
            let _ = common::run("pvesm", &["list", s, "--content", "iso"]);
        }
        return Ok(());
    }

    // JSON: 통합 스냅샷
    let status_out = run_pvesm(&["status", "--content", "iso"])?;
    let storages = parse_pvesm_status(&status_out)?;
    let mut files = Vec::new();
    for s in &storages {
        if filter_storage.is_some_and(|f| f != s.name) { continue; }
        let list_out = run_pvesm(&["list", &s.name, "--content", "iso"])?;
        let entries = parse_pvesm_list(&list_out)?;
        files.push(IsoFileGroup { storage: s.name.clone(), files: entries });
    }
    let snap = ListSnap { storages, files };
    println!("{}", serde_json::to_string_pretty(&snap)?);
    Ok(())
}

fn run_pvesm(args: &[&str]) -> anyhow::Result<String> {
    let out = std::process::Command::new("pvesm").args(args).output()?;
    if !out.status.success() {
        anyhow::bail!("pvesm {args:?} 실패: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn list_iso_storage_ids() -> anyhow::Result<Vec<String>> {
    let text = run_pvesm(&["status", "--content", "iso"])?;
    Ok(parse_pvesm_status(&text)?.into_iter().map(|r| r.name).collect())
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
    // 비밀번호는 argv로 전달 (pvesm 한계). run_secret으로 실패 시 argv 비노출.
    common::run_secret(
        "pvesm",
        &[
            "add", "cifs", id,
            "--server", server,
            "--share", share,
            "--username", username,
            "--password", &pw,
            "--content", "iso",
            "--smbversion", smb_version,
        ],
        &format!("pvesm add cifs {id}"),
    )?;
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

#[cfg(test)]
mod tests {
    use super::*;

    // ----- parse_pvesm_status -----

    #[test]
    fn status_basic() {
        let text = "Name               Type     Status     Total      Used   Avail        %\n\
                    local               dir     active     98497780   71976  21471940    73.07%\n\
                    truenas-iso         nfs     active     4022270    14919  4020779     0.04%\n";
        let rows = parse_pvesm_status(text).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], StorageRow {
            name: "local".into(), storage_type: "dir".into(), status: "active".into(),
        });
        assert_eq!(rows[1].name, "truenas-iso");
        assert_eq!(rows[1].storage_type, "nfs");
    }

    #[test]
    fn status_skips_empty_lines() {
        let text = "Name Type Status\n\
                    local dir active\n\
                    \n\
                    other nfs inactive\n";
        assert_eq!(parse_pvesm_status(text).unwrap().len(), 2);
    }

    #[test]
    fn status_fails_on_short_line() {
        let text = "Name Type Status\n\
                    local dir\n";
        assert!(parse_pvesm_status(text).is_err());
    }

    #[test]
    fn status_only_header() {
        assert!(parse_pvesm_status("Name Type Status").unwrap().is_empty());
    }

    // ----- parse_pvesm_list -----

    #[test]
    fn list_basic() {
        let text = "Volid                                       Format  Type            Size VMID\n\
                    local:iso/linuxmint-22.iso                  iso     iso       3091660800\n";
        let rows = parse_pvesm_list(text).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], IsoFile {
            volid: "local:iso/linuxmint-22.iso".into(),
            format: "iso".into(),
            size: 3091660800,
        });
    }

    #[test]
    fn list_with_vmid_column() {
        let text = "Volid Format Type Size VMID\n\
                    local:iso/x.iso iso iso 1024 100\n";
        // 5컬럼이지만 첫 4개만 사용
        let rows = parse_pvesm_list(text).unwrap();
        assert_eq!(rows[0].size, 1024);
    }

    #[test]
    fn list_fails_on_bad_size() {
        let text = "Volid Format Type Size\n\
                    local:iso/x.iso iso iso notanumber\n";
        assert!(parse_pvesm_list(text).is_err());
    }

    #[test]
    fn list_fails_on_short_line() {
        let text = "Volid Format Type Size\n\
                    local:iso/x.iso iso\n";
        assert!(parse_pvesm_list(text).is_err());
    }

    #[test]
    fn list_skips_empty_lines() {
        let text = "Volid Format Type Size\n\
                    a iso iso 100\n\
                    \n\
                    b iso iso 200\n";
        assert_eq!(parse_pvesm_list(text).unwrap().len(), 2);
    }

    #[test]
    fn list_only_header_returns_empty() {
        assert!(parse_pvesm_list("Volid Format Type Size").unwrap().is_empty());
    }
}
