//! prelik-recovery — LXC config snapshot/restore + audit log.
//! Destructive operation 전에 LXC config (/etc/pve/nodes/<node>/lxc/*.conf) +
//! pvecm 노드 목록을 백업해두고, 사고 시 복원.

use clap::{Parser, Subcommand};
use prelik_core::common;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SNAPSHOT_DIR: &str = "/var/lib/prelik/snapshots";
const AUDIT_LOG: &str = "/var/lib/prelik/audit.log";

#[derive(Parser)]
#[command(name = "prelik-recovery", about = "LXC config 스냅샷 + audit log")]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Doctor,
    /// 스냅샷 생성 (destructive op 전에 호출)
    Create {
        /// 액션 라벨 (예: "lxc-delete-200")
        action: String,
        /// 노드 이름 (생략 시 hostname). pvesh 환경이면 노드 이름 사용.
        #[arg(long)]
        node: Option<String>,
    },
    /// 스냅샷 목록
    List,
    /// 스냅샷 복원 — LXC config만 (실제 LXC 상태는 별도 관리)
    Restore {
        id: String,
        #[arg(long)]
        force: bool,
    },
    /// 스냅샷 삭제
    Delete { id: String },
    /// audit log에 메시지 추가
    AuditLog { message: String },
    /// audit log tail
    AuditShow {
        #[arg(long, default_value = "20")]
        tail: usize,
    },
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Snapshot {
    id: String,
    created_at: u64,
    action: String,
    node: Option<String>,
    /// /etc/pve/nodes/<node>/lxc/*.conf 백업 (filename → content)
    lxc_configs: HashMap<String, String>,
    /// pvecm nodes 출력 (raw)
    cluster_nodes: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Doctor => doctor(),
        Cmd::Create { action, node } => create(&action, node.as_deref(), cli.json),
        Cmd::List => list(cli.json),
        Cmd::Restore { id, force } => restore(&id, force),
        Cmd::Delete { id } => delete(&id),
        Cmd::AuditLog { message } => audit_log(&message),
        Cmd::AuditShow { tail } => audit_show(tail, cli.json),
    }
}

fn doctor() -> anyhow::Result<()> {
    println!("=== prelik-recovery doctor ===");
    println!("  snapshot dir : {} ({})", SNAPSHOT_DIR, mark(Path::new(SNAPSHOT_DIR).exists()));
    println!("  audit log    : {} ({})", AUDIT_LOG, mark(Path::new(AUDIT_LOG).exists()));
    println!("  pvecm        : {}", mark(common::has_cmd("pvecm")));
    Ok(())
}
fn mark(b: bool) -> &'static str { if b { "✓" } else { "✗" } }

fn ensure_dirs() -> anyhow::Result<()> {
    fs::create_dir_all(SNAPSHOT_DIR)?;
    if let Some(parent) = Path::new(AUDIT_LOG).parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn snapshot_path(id: &str) -> PathBuf {
    PathBuf::from(SNAPSHOT_DIR).join(format!("{id}.json"))
}

fn collect_lxc_configs(node: &str) -> anyhow::Result<HashMap<String, String>> {
    let dir = format!("/etc/pve/nodes/{node}/lxc");
    let mut map = HashMap::new();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Ok(map), // Proxmox 아닌 환경에선 빈 맵
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("conf") { continue; }
        if let Ok(content) = fs::read_to_string(&path) {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                map.insert(name.to_string(), content);
            }
        }
    }
    Ok(map)
}

fn create(action: &str, node: Option<&str>, json: bool) -> anyhow::Result<()> {
    ensure_dirs()?;
    let node = match node {
        Some(n) => n.to_string(),
        None => fs::read_to_string("/etc/hostname")?.trim().to_string(),
    };
    let id = now_secs().to_string();
    let lxc_configs = collect_lxc_configs(&node)?;
    let cluster_nodes = if common::has_cmd("pvecm") {
        common::run("pvecm", &["nodes"]).unwrap_or_default()
    } else {
        String::new()
    };
    let snap = Snapshot {
        id: id.clone(),
        created_at: now_secs(),
        action: action.to_string(),
        node: Some(node),
        lxc_configs,
        cluster_nodes,
    };
    fs::write(snapshot_path(&id), serde_json::to_string_pretty(&snap)?)?;
    audit_log_internal(&format!("snapshot-create id={id} action={action}"))?;

    if json {
        println!("{}", serde_json::to_string_pretty(&snap)?);
    } else {
        println!("✓ 스냅샷 생성: id={id} action={action} ({} LXC configs)", snap.lxc_configs.len());
    }
    Ok(())
}

fn list_snapshot_files() -> anyhow::Result<Vec<Snapshot>> {
    let dir = Path::new(SNAPSHOT_DIR);
    if !dir.exists() { return Ok(vec![]); }
    let mut snaps = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") { continue; }
        match fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Snapshot>(&t).ok()) {
            Some(s) => snaps.push(s),
            None => eprintln!("⚠ 스냅샷 파싱 실패: {}", path.display()),
        }
    }
    snaps.sort_by_key(|s| std::cmp::Reverse(s.created_at));
    Ok(snaps)
}

fn list(json: bool) -> anyhow::Result<()> {
    let snaps = list_snapshot_files()?;
    if json {
        // 큰 lxc_configs/cluster_nodes 본문은 제외 — 메타데이터만.
        let summaries: Vec<serde_json::Value> = snaps.iter().map(|s| serde_json::json!({
            "id": s.id,
            "created_at": s.created_at,
            "action": s.action,
            "node": s.node,
            "lxc_config_count": s.lxc_configs.len(),
        })).collect();
        println!("{}", serde_json::to_string_pretty(&summaries)?);
        return Ok(());
    }
    if snaps.is_empty() {
        println!("(스냅샷 없음 — {SNAPSHOT_DIR})");
        return Ok(());
    }
    println!("{:<12} {:<20} {:<10} {}", "ID", "ACTION", "LXC#", "CREATED_AT");
    for s in &snaps {
        println!("{:<12} {:<20} {:<10} {}", s.id, s.action, s.lxc_configs.len(), s.created_at);
    }
    Ok(())
}

fn restore(id: &str, force: bool) -> anyhow::Result<()> {
    let path = snapshot_path(id);
    if !path.exists() {
        anyhow::bail!("스냅샷 {id} 없음");
    }
    let snap: Snapshot = serde_json::from_str(&fs::read_to_string(&path)?)?;
    if !force {
        anyhow::bail!(
            "복원은 --force 필요. 스냅샷 id={} action={} ({} LXC configs)를 \
             /etc/pve/nodes/<node>/lxc 에 덮어씁니다.",
            snap.id, snap.action, snap.lxc_configs.len()
        );
    }
    let node = snap.node.as_deref()
        .ok_or_else(|| anyhow::anyhow!("스냅샷에 노드 이름 없음 — restore 불가"))?;
    let target_dir = format!("/etc/pve/nodes/{node}/lxc");
    if !Path::new(&target_dir).exists() {
        anyhow::bail!("대상 디렉토리 없음 (Proxmox 아닌 환경?): {target_dir}");
    }
    let mut restored = 0;
    for (name, content) in &snap.lxc_configs {
        let p = Path::new(&target_dir).join(name);
        fs::write(&p, content)?;
        restored += 1;
    }
    audit_log_internal(&format!("snapshot-restore id={id} files={restored}"))?;
    println!("✓ 복원 완료: {restored}개 LXC config");
    Ok(())
}

fn delete(id: &str) -> anyhow::Result<()> {
    let path = snapshot_path(id);
    if !path.exists() {
        anyhow::bail!("스냅샷 {id} 없음");
    }
    fs::remove_file(&path)?;
    audit_log_internal(&format!("snapshot-delete id={id}"))?;
    println!("✓ 삭제: {id}");
    Ok(())
}

fn audit_log(message: &str) -> anyhow::Result<()> {
    audit_log_internal(message)?;
    println!("✓ logged");
    Ok(())
}

fn audit_log_internal(message: &str) -> anyhow::Result<()> {
    ensure_dirs()?;
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .create(true).append(true)
        .open(AUDIT_LOG)?;
    writeln!(f, "{}\t{message}", now_secs())?;
    Ok(())
}

fn audit_show(tail: usize, json: bool) -> anyhow::Result<()> {
    if !Path::new(AUDIT_LOG).exists() {
        if json { println!("[]"); } else { println!("(audit log 없음)"); }
        return Ok(());
    }
    let raw = fs::read_to_string(AUDIT_LOG)?;
    let lines: Vec<&str> = raw.lines().collect();
    let start = lines.len().saturating_sub(tail);
    let recent = &lines[start..];
    if json {
        let entries: Vec<serde_json::Value> = recent.iter().filter_map(|l| {
            let (ts, msg) = l.split_once('\t')?;
            Some(serde_json::json!({
                "timestamp": ts.parse::<u64>().ok(),
                "message": msg,
            }))
        }).collect();
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        for l in recent { println!("{l}"); }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn snapshot_roundtrip() {
        let mut configs = HashMap::new();
        configs.insert("100.conf".to_string(), "memory: 2048\n".to_string());
        let s = Snapshot {
            id: "1700000000".into(),
            created_at: 1700000000,
            action: "lxc-delete-100".into(),
            node: Some("pve".into()),
            lxc_configs: configs,
            cluster_nodes: "Membership information\n--------------\n".into(),
        };
        let json_text = serde_json::to_string(&s).unwrap();
        let back: Snapshot = serde_json::from_str(&json_text).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn snapshot_path_basic() {
        let p = snapshot_path("12345");
        assert_eq!(p, PathBuf::from("/var/lib/prelik/snapshots/12345.json"));
    }

    #[test]
    fn audit_format_no_tab_in_msg_safe() {
        // 메시지에 탭이 없으면 split_once('\t')로 안전하게 분리됨
        let line = "1700000000\tsnapshot-create id=1 action=test";
        let (ts, msg) = line.split_once('\t').unwrap();
        assert_eq!(ts, "1700000000");
        assert_eq!(msg, "snapshot-create id=1 action=test");
    }
}
