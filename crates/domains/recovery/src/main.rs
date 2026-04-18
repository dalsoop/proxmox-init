//! pxi-recovery — LXC config snapshot/restore + audit log.
//! Destructive operation 전에 LXC config (/etc/pve/nodes/<node>/lxc/*.conf) +
//! pvecm 노드 목록을 백업해두고, 사고 시 복원.

use clap::{Parser, Subcommand};
use pxi_core::common;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SNAPSHOT_DIR: &str = "/var/lib/pxi/snapshots";
const AUDIT_LOG: &str = "/var/lib/pxi/audit.log";

#[derive(Parser)]
#[command(name = "pxi-recovery", about = "LXC config 스냅샷 + audit log")]
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
    println!("=== pxi-recovery doctor ===");
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

fn snapshot_path(id: &str) -> anyhow::Result<PathBuf> {
    validate_id(id)?;
    Ok(PathBuf::from(SNAPSHOT_DIR).join(format!("{id}.json")))
}

// id 충돌 회피: 같은 초에 여러 스냅샷 생성 시 -1, -2 suffix.
// O_CREAT|O_EXCL로 atomic 예약 — exists() 후 write의 TOCTOU 회피.
// 호출자가 같은 path에 다시 write 가능 (예약된 파일을 덮어씀).
fn reserve_snapshot_id() -> anyhow::Result<String> {
    use std::os::unix::fs::OpenOptionsExt;
    let base = now_secs();
    for n in 0u32..1000 {
        let id = if n == 0 { base.to_string() } else { format!("{base}-{n}") };
        let p = PathBuf::from(SNAPSHOT_DIR).join(format!("{id}.json"));
        // O_EXCL: 이미 존재하면 실패 — 동시 create 경합에서 한 쪽만 성공.
        let res = fs::OpenOptions::new()
            .write(true).create_new(true).mode(0o600)
            .open(&p);
        match res {
            Ok(_) => return Ok(id),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(anyhow::anyhow!("snapshot 예약 실패 {}: {e}", p.display())),
        }
    }
    anyhow::bail!("같은 초에 1000개+ 스냅샷 시도 — 비정상")
}

fn collect_lxc_configs(node: &str) -> anyhow::Result<HashMap<String, String>> {
    validate_node(node)?;
    let dir = format!("/etc/pve/nodes/{node}/lxc");
    let entries = fs::read_dir(&dir)
        .map_err(|e| anyhow::anyhow!("LXC config 디렉토리 읽기 실패 {dir}: {e}"))?;
    let mut map = HashMap::new();
    // 개별 entry/read 실패도 fail-fast — 부분 백업으로 '안전망' 위장 금지.
    for entry in entries {
        let entry = entry
            .map_err(|e| anyhow::anyhow!("디렉토리 entry 읽기 실패 {dir}: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("conf") { continue; }
        let content = fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("config 읽기 실패 {}: {e}", path.display()))?;
        let name = path.file_name().and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("config filename 추출 실패: {}", path.display()))?
            .to_string();
        map.insert(name, content);
    }
    Ok(map)
}

// 노드 이름 검증 — '.'/'..' 와 path separator 차단.
fn validate_node(node: &str) -> anyhow::Result<()> {
    if node.is_empty() { anyhow::bail!("node 이름 비어 있음"); }
    if node == "." || node == ".." {
        anyhow::bail!("node 이름이 디렉토리 참조: {node:?}");
    }
    if !node.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        anyhow::bail!("node 이름은 [A-Za-z0-9_-]만 허용 (.도 차단): {node:?}");
    }
    Ok(())
}

// 노드 이름 자동 감지: pvecm status가 우선 (정확한 PVE 노드명),
// fallback으로 /etc/hostname.
fn detect_node() -> anyhow::Result<String> {
    if common::has_cmd("pvesh") {
        let out = common::run("pvesh", &["get", "/cluster/status", "--output-format", "json"]);
        if let Ok(text) = out {
            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
                if let Some(local) = arr.iter()
                    .find(|v| v["type"].as_str() == Some("node") && v["local"].as_i64() == Some(1))
                    .and_then(|v| v["name"].as_str())
                {
                    return Ok(local.to_string());
                }
            }
        }
    }
    let h = fs::read_to_string("/etc/hostname")?.trim().to_string();
    if h.is_empty() { anyhow::bail!("노드 이름 감지 실패 (/etc/hostname 비어 있음)"); }
    Ok(h)
}

// id는 안전한 문자만 허용 — path traversal/외부 디렉토리 접근 차단.
fn validate_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty() { anyhow::bail!("snapshot id가 비어 있음"); }
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        anyhow::bail!("snapshot id는 [A-Za-z0-9_-]만 허용: {id:?}");
    }
    Ok(())
}

// snapshot 내부 LXC config filename 검증 — Path::file_name과 일치해야 traversal 차단.
fn validate_config_filename(name: &str) -> anyhow::Result<()> {
    if name.is_empty() { anyhow::bail!("config filename 비어 있음"); }
    let p = Path::new(name);
    if p.file_name().and_then(|n| n.to_str()) != Some(name) {
        anyhow::bail!("config filename에 경로 구성요소 포함: {name:?}");
    }
    if !name.ends_with(".conf") {
        anyhow::bail!("config filename이 .conf로 끝나야 함: {name:?}");
    }
    Ok(())
}

fn create(action: &str, node: Option<&str>, json: bool) -> anyhow::Result<()> {
    ensure_dirs()?;
    let node = match node {
        Some(n) => n.to_string(),
        None => detect_node()?,
    };
    validate_node(&node)?;
    let id = reserve_snapshot_id()?;
    let lxc_configs = collect_lxc_configs(&node)?;
    // 빈 스냅샷 거부 — destructive op 안전망인데 백업 없으면 의미 없음.
    if lxc_configs.is_empty() {
        anyhow::bail!(
            "노드 '{node}'의 LXC config가 0개 — 안전망이 비어 있습니다. \
             노드 이름을 --node로 명시하거나 Proxmox 환경을 확인하세요."
        );
    }
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
    let snap_path = snapshot_path(&id)?;
    fs::write(&snap_path, serde_json::to_string_pretty(&snap)?)?;
    // LXC config에 시크릿(API 토큰, 비밀번호)이 포함될 수 있음 — 0600으로 보호.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&snap_path, fs::Permissions::from_mode(0o600));
    }
    audit_log_internal(&format!("snapshot-create id={id} action={action}"))?;

    if json {
        // LXC config 전문은 시크릿(API 토큰, 비밀번호 등)이 포함될 수 있음.
        // --json 출력에 config 본문 노출 금지 — CI 로그/터미널 scrollback 유출 위험.
        // 메타데이터만 출력 (list와 동일 수준).
        let safe = serde_json::json!({
            "id": snap.id,
            "created_at": snap.created_at,
            "action": snap.action,
            "node": snap.node,
            "lxc_config_count": snap.lxc_configs.len(),
            "snapshot_path": snapshot_path(&id)?.display().to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&safe)?);
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
    let path = snapshot_path(id)?;
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
    validate_node(node)?;
    let target_dir = PathBuf::from(format!("/etc/pve/nodes/{node}/lxc"));
    if !target_dir.exists() {
        anyhow::bail!("대상 디렉토리 없음 (Proxmox 아닌 환경?): {}", target_dir.display());
    }
    let mut restored = 0;
    for (name, content) in &snap.lxc_configs {
        // 각 filename 검증 — 경로 구성요소 차단 (../등)
        validate_config_filename(name)?;
        let p = target_dir.join(name);
        // canonical 검증: 결과 경로가 target_dir 하위인지 확인
        let parent = p.parent().unwrap_or(&target_dir);
        if parent != target_dir {
            anyhow::bail!("복원 경로가 대상 디렉토리 밖: {}", p.display());
        }
        fs::write(&p, content)?;
        restored += 1;
    }
    audit_log_internal(&format!("snapshot-restore id={id} files={restored}"))?;
    println!("✓ 복원 완료: {restored}개 LXC config");
    Ok(())
}

fn delete(id: &str) -> anyhow::Result<()> {
    let path = snapshot_path(id)?;
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
        let p = snapshot_path("12345").unwrap();
        assert_eq!(p, PathBuf::from("/var/lib/pxi/snapshots/12345.json"));
    }

    #[test]
    fn snapshot_path_id_traversal_rejected() {
        assert!(snapshot_path("../etc/passwd").is_err());
        assert!(snapshot_path("foo/bar").is_err());
        assert!(snapshot_path("foo bar").is_err());
        assert!(snapshot_path("").is_err());
        assert!(snapshot_path(".").is_err()); // '.' alone — 영숫자 아님
    }

    #[test]
    fn snapshot_path_id_allowed() {
        assert!(snapshot_path("1700000000").is_ok());
        assert!(snapshot_path("1700000000-1").is_ok());
        assert!(snapshot_path("snap_v2").is_ok());
        assert!(snapshot_path("ABC-123_xyz").is_ok());
    }

    #[test]
    fn config_filename_traversal_rejected() {
        assert!(validate_config_filename("../passwd").is_err());
        assert!(validate_config_filename("foo/bar.conf").is_err());
        assert!(validate_config_filename("noext").is_err());
        assert!(validate_config_filename("").is_err());
    }

    #[test]
    fn config_filename_allowed() {
        assert!(validate_config_filename("100.conf").is_ok());
        assert!(validate_config_filename("abc-test.conf").is_ok());
    }

    #[test]
    fn node_name_traversal_rejected() {
        assert!(validate_node("..").is_err());
        assert!(validate_node(".").is_err());
        assert!(validate_node("a/b").is_err());
        assert!(validate_node("a.b").is_err()); // '.'도 차단 (..로 조립 가능)
        assert!(validate_node("").is_err());
        assert!(validate_node("foo bar").is_err());
    }

    #[test]
    fn node_name_allowed() {
        assert!(validate_node("pve").is_ok());
        assert!(validate_node("ranode-3960x").is_ok());
        assert!(validate_node("node_2").is_ok());
        assert!(validate_node("ABC123").is_ok());
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
