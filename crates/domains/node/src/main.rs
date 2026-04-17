//! prelik-node — Proxmox 클러스터 노드 관리 (read-only + ssh exec).

use clap::{Parser, Subcommand};
use prelik_core::common;
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Parser)]
#[command(name = "prelik-node", about = "Proxmox 클러스터 노드 관리")]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Doctor,
    /// 클러스터 노드 목록 (pvesh get /nodes)
    List,
    /// 단일 노드 정보 (LXC/VM 개수 포함)
    Info { node: String },
    /// 노드에서 명령 실행 (root@<node-ip> ssh)
    Exec {
        node: String,
        /// 실행할 명령 (공백 분리해 단일 문자열로 전달)
        #[arg(trailing_var_arg = true, required = true)]
        cmd: Vec<String>,
    },
    /// 새 노드를 클러스터에 추가 (SSH 부트스트랩 + phs 배포 + 라우트 timer)
    Join {
        node: String,
        /// SSH 접근용 IP (새 노드일 때 필수)
        #[arg(long)]
        ssh_target: Option<String>,
    },
    /// 노드를 클러스터에서 제거 (라우트/타이머 정리)
    Remove {
        node: String,
        #[arg(long)]
        force: bool,
    },
    /// 원격 노드에 phs 바이너리 + config.toml 동기화
    Sync { node: String },
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct NodeRow {
    node: String,
    status: String,
    cpus: u64,
    mem_total_gb: u64,
    uptime_days: u64,
}

#[derive(Serialize)]
struct NodeInfo {
    node: String,
    status: String,
    cpus: u64,
    mem_total_gb: u64,
    uptime_days: u64,
    lxc_total: usize,
    lxc_running: usize,
    vm_total: usize,
    vm_running: usize,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Doctor => doctor(),
        Cmd::List => list(cli.json),
        Cmd::Info { node } => info(&node, cli.json),
        Cmd::Exec { node, cmd } => exec(&node, &cmd),
        Cmd::Join { node, ssh_target } => node_join(&node, ssh_target.as_deref()),
        Cmd::Remove { node, force } => node_remove(&node, force),
        Cmd::Sync { node } => node_sync(&node),
    }
}

fn doctor() -> anyhow::Result<()> {
    println!("=== prelik-node doctor ===");
    println!("  pvesh : {}", mark(common::has_cmd("pvesh")));
    println!("  ssh   : {} (exec 명령용)", mark(common::has_cmd("ssh")));
    Ok(())
}
fn mark(b: bool) -> &'static str { if b { "✓" } else { "✗" } }

// ---------- list ----------

fn parse_pvesh_nodes(json_text: &str) -> anyhow::Result<Vec<NodeRow>> {
    let raw: Vec<serde_json::Value> = serde_json::from_str(json_text)
        .map_err(|e| anyhow::anyhow!("pvesh get /nodes JSON 파싱 실패: {e}"))?;
    let mut rows = Vec::with_capacity(raw.len());
    for n in raw {
        let node = n["node"].as_str()
            .ok_or_else(|| anyhow::anyhow!("node 필드 없음: {n}"))?
            .to_string();
        let status = n["status"].as_str().unwrap_or("unknown").to_string();
        let cpus = n["maxcpu"].as_u64().unwrap_or(0);
        let mem_total_gb = n["maxmem"].as_u64().unwrap_or(0) / 1024 / 1024 / 1024;
        let uptime_days = n["uptime"].as_u64().unwrap_or(0) / 86400;
        rows.push(NodeRow { node, status, cpus, mem_total_gb, uptime_days });
    }
    Ok(rows)
}

fn list(json: bool) -> anyhow::Result<()> {
    if !common::has_cmd("pvesh") {
        anyhow::bail!("pvesh 없음 — Proxmox VE 호스트에서만 동작");
    }
    let out = common::run("pvesh", &["get", "/nodes", "--output-format", "json"])?;
    let rows = parse_pvesh_nodes(&out)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    println!("=== Proxmox 클러스터 노드 ({}) ===\n", rows.len());
    println!("  {:<20} {:<10} {:<6} {:<10} {}", "NODE", "STATUS", "CPUS", "MEM(GB)", "UPTIME");
    for r in &rows {
        println!("  {:<20} {:<10} {:<6} {:<10} {}d",
            r.node, r.status, r.cpus, r.mem_total_gb, r.uptime_days);
    }
    Ok(())
}

// ---------- info ----------

fn count_running(json_text: &str) -> (usize, usize) {
    let arr: Vec<serde_json::Value> = serde_json::from_str(json_text).unwrap_or_default();
    let total = arr.len();
    let running = arr.iter().filter(|x| x["status"].as_str() == Some("running")).count();
    (total, running)
}

fn info(node: &str, json: bool) -> anyhow::Result<()> {
    if !common::has_cmd("pvesh") {
        anyhow::bail!("pvesh 없음");
    }
    let nodes_json = common::run("pvesh", &["get", "/nodes", "--output-format", "json"])?;
    let rows = parse_pvesh_nodes(&nodes_json)?;
    let row = rows.into_iter().find(|r| r.node == node)
        .ok_or_else(|| anyhow::anyhow!("노드 '{node}'를 찾지 못함"))?;

    let lxc_json = common::run("pvesh", &["get", &format!("/nodes/{node}/lxc"), "--output-format", "json"])
        .unwrap_or_else(|_| "[]".into());
    let vm_json = common::run("pvesh", &["get", &format!("/nodes/{node}/qemu"), "--output-format", "json"])
        .unwrap_or_else(|_| "[]".into());
    let (lxc_total, lxc_running) = count_running(&lxc_json);
    let (vm_total, vm_running)   = count_running(&vm_json);

    let inf = NodeInfo {
        node: row.node, status: row.status, cpus: row.cpus,
        mem_total_gb: row.mem_total_gb, uptime_days: row.uptime_days,
        lxc_total, lxc_running, vm_total, vm_running,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&inf)?);
        return Ok(());
    }
    println!("=== Node: {} ===", inf.node);
    println!("  status   : {}", inf.status);
    println!("  cpus     : {}", inf.cpus);
    println!("  mem      : {} GB", inf.mem_total_gb);
    println!("  uptime   : {}d", inf.uptime_days);
    println!("  LXC      : {}/{} running", inf.lxc_running, inf.lxc_total);
    println!("  VM       : {}/{} running", inf.vm_running, inf.vm_total);
    Ok(())
}

// ---------- exec ----------

fn cluster_node_ip(node: &str) -> anyhow::Result<String> {
    let raw = common::run("pvesh", &["get", "/cluster/status", "--output-format", "json"])?;
    let arr: Vec<serde_json::Value> = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("pvesh /cluster/status JSON 파싱 실패: {e}"))?;
    arr.iter()
        .find(|v| v["type"].as_str() == Some("node") && v["name"].as_str() == Some(node))
        .and_then(|v| v["ip"].as_str().map(String::from))
        .ok_or_else(|| anyhow::anyhow!(
            "노드 '{node}'는 클러스터 멤버가 아니거나 IP 없음 (prelik-node list 로 확인). \
             임의 호스트 SSH는 차단됩니다."
        ))
}

fn exec(node: &str, cmd: &[String]) -> anyhow::Result<()> {
    if !common::has_cmd("ssh") {
        anyhow::bail!("ssh 없음");
    }
    if !common::has_cmd("pvesh") {
        anyhow::bail!("pvesh 없음 — 노드 멤버십 검증 불가");
    }
    if cmd.is_empty() {
        anyhow::bail!("실행 명령이 비어 있음");
    }
    // 클러스터가 보고한 canonical IP로 직접 접속.
    // 노드 이름만 ssh에 넘기면 ~/.ssh/config의 Host alias가 HostName/ProxyCommand
    // 등으로 우회 가능 — IP 직바인딩 + -F /dev/null로 user config 무시.
    let ip = cluster_node_ip(node)?;
    let target = format!("root@{ip}");
    let joined = cmd.join(" ");
    let status = Command::new("ssh")
        .args([
            "-F", "/dev/null", // user/system ssh_config 무시
            "-o", "ConnectTimeout=10",
            "-o", "StrictHostKeyChecking=yes",
            "-o", "BatchMode=yes",
            "-o", "ProxyCommand=none",
            "-o", "ProxyJump=none",
            &target, &joined,
        ])
        .status()?;
    std::process::exit(status.code().unwrap_or(1));
}

// ---------- ssh helpers ----------

fn ssh_run(node_ip: &str, cmd: &str) -> bool {
    Command::new("ssh")
        .args(["-o", "ConnectTimeout=10", "-o", "StrictHostKeyChecking=no", "-o", "BatchMode=yes",
            &format!("root@{node_ip}"), cmd])
        .status().map_or(false, |s| s.success())
}

fn ssh_capture(node_ip: &str, cmd: &str) -> String {
    Command::new("ssh")
        .args(["-o", "ConnectTimeout=10", "-o", "StrictHostKeyChecking=no", "-o", "BatchMode=yes",
            &format!("root@{node_ip}"), cmd])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

fn cmd_output(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd).args(args).output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn local_node_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .unwrap_or_default().trim().to_string()
}

fn node_ip_from_cluster(node: &str) -> anyhow::Result<String> {
    let raw = common::run("pvesh", &["get", "/cluster/status", "--output-format", "json"])?;
    let arr: Vec<serde_json::Value> = serde_json::from_str(&raw)?;
    arr.iter()
        .find(|v| v["type"].as_str() == Some("node") && v["name"].as_str() == Some(node))
        .and_then(|v| v["ip"].as_str().map(String::from))
        .ok_or_else(|| anyhow::anyhow!("노드 '{node}' IP를 클러스터에서 찾을 수 없음"))
}

// ---------- node-join ----------

fn node_join(node_name: &str, ssh_target: Option<&str>) -> anyhow::Result<()> {
    if !common::has_cmd("pvesh") { anyhow::bail!("pvesh 없음"); }

    println!("=== 노드 추가: {node_name} ===\n");

    // 클러스터에 이미 있는지 확인
    let nodes_json = common::run("pvesh", &["get", "/nodes", "--output-format", "json"])?;
    let exists = serde_json::from_str::<serde_json::Value>(&nodes_json).ok()
        .and_then(|v| v.as_array().map(|arr| arr.iter().any(|n| n["node"].as_str() == Some(node_name))))
        .unwrap_or(false);

    if exists {
        println!("[1/5] 클러스터: 이미 등록됨 — 건너뜀");
    } else if let Some(ssh_ip) = ssh_target {
        println!("[1/5] 클러스터 가입 시도: {node_name} via {ssh_ip}");

        let ssh_ok = ssh_run(ssh_ip, "echo ok");
        if !ssh_ok {
            anyhow::bail!("SSH 접근 불가: root@{ssh_ip}. ssh-copy-id 먼저 실행");
        }

        let local = local_node_name();
        let controller_ip = node_ip_from_cluster(&local)?;

        let add_cmd = format!("echo 'y' | pvecm add {controller_ip} --use_ssh 2>&1");
        let output = Command::new("ssh")
            .args(["-o", "ConnectTimeout=30", "-o", "StrictHostKeyChecking=no", "-o", "BatchMode=yes",
                &format!("root@{ssh_ip}"), &add_cmd])
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("pvecm add 실패: {stderr}");
        }
        println!("     ✓ 클러스터 가입 완료");
        std::thread::sleep(std::time::Duration::from_secs(5));
    } else {
        anyhow::bail!("노드 '{node_name}'가 클러스터에 없고 --ssh-target도 없음");
    }

    // SSH 접근 검증
    let node_ip = if exists {
        node_ip_from_cluster(node_name)?
    } else {
        ssh_target.map(|s| s.to_string()).unwrap_or_default()
    };
    let ssh_ok = ssh_run(&node_ip, "echo ok");
    if !ssh_ok {
        anyhow::bail!("SSH 접근 불가: root@{node_ip}");
    }
    println!("[2/5] ✓ SSH 접근 가능: root@{node_ip}");

    // phs 바이너리 배포
    println!("[3/5] phs 바이너리 배포...");
    let binary_path = "/usr/local/bin/proxmox-host-setup";
    if std::path::Path::new(binary_path).exists() {
        let scp_ok = Command::new("scp")
            .args(["-o", "ConnectTimeout=10", "-o", "StrictHostKeyChecking=no",
                binary_path, &format!("root@{node_ip}:/tmp/phs-new")])
            .status().map_or(false, |s| s.success());
        if scp_ok {
            let mv_ok = ssh_run(&node_ip, "chmod 755 /tmp/phs-new && mv /tmp/phs-new /usr/local/bin/proxmox-host-setup");
            println!("     {}", if mv_ok { "✓ 배포 완료" } else { "⚠ 배포 실패" });
        }
    } else {
        println!("     ⚠ 호스트에 phs 바이너리 없음 — 스킵");
    }

    // route-audit timer
    println!("[4/5] route-audit timer 설치...");
    let timer_ok = ssh_run(&node_ip, "/usr/local/bin/proxmox-host-setup infra route-audit-watch --action install 2>/dev/null || true");
    println!("     {}", if timer_ok { "✓ timer 설치 완료" } else { "⚠ timer 설치 실패" });

    println!("[5/5] 완료");
    println!("\n=== 노드 '{node_name}' 추가 완료 ===");
    Ok(())
}

// ---------- node-remove ----------

fn node_remove(node_name: &str, force: bool) -> anyhow::Result<()> {
    if !common::has_cmd("pvesh") { anyhow::bail!("pvesh 없음"); }
    if !common::has_cmd("pvecm") { anyhow::bail!("pvecm 없음"); }

    let local = local_node_name();
    if node_name == local {
        anyhow::bail!("로컬 노드 '{node_name}'는 제거할 수 없습니다");
    }

    println!("=== 노드 제거: {node_name} ===\n");

    // LXC 확인
    let lxc_json = cmd_output("pvesh", &["get", &format!("/nodes/{node_name}/lxc"), "--output-format", "json"]);
    let lxcs: Vec<serde_json::Value> = serde_json::from_str(&lxc_json).unwrap_or_default();
    let running = lxcs.iter().filter(|l| l["status"].as_str() == Some("running")).count();

    if !lxcs.is_empty() && !force {
        anyhow::bail!("LXC {}개 남아있음 (실행 중 {running}). --force 필요", lxcs.len());
    }
    if running > 0 {
        anyhow::bail!("실행 중인 LXC {running}개 — 정지/마이그레이션 후 재시도");
    }

    // 라우트 정리 (SSH 접근 가능하면)
    if let Ok(ip) = node_ip_from_cluster(node_name) {
        println!("[1/3] 원격 정리 ({ip})...");
        let _ = ssh_run(&ip, "systemctl stop phs-route-audit.timer 2>/dev/null; systemctl disable phs-route-audit.timer 2>/dev/null");
        println!("     ✓ timer 정리");
    } else {
        println!("[1/3] 원격 접근 불가 — 스킵");
    }

    // 클러스터에서 제거
    println!("[2/3] 클러스터에서 제거...");
    let result = Command::new("pvecm").args(["delnode", node_name]).output()?;
    if result.status.success() {
        println!("     ✓ pvecm delnode 완료");
    } else {
        let stderr = String::from_utf8_lossy(&result.stderr);
        eprintln!("     ⚠ pvecm delnode: {stderr}");
    }

    // /etc/pve/nodes/<node> 잔여 제거
    println!("[3/3] 잔여 설정 정리...");
    let node_dir = format!("/etc/pve/nodes/{node_name}");
    if std::path::Path::new(&node_dir).exists() {
        let _ = std::fs::remove_dir_all(&node_dir);
        println!("     ✓ {node_dir} 제거");
    }

    println!("\n=== 노드 '{node_name}' 제거 완료 ===");
    Ok(())
}

// ---------- node-sync ----------

fn node_sync(node_name: &str) -> anyhow::Result<()> {
    if !common::has_cmd("pvesh") { anyhow::bail!("pvesh 없음"); }

    let local = local_node_name();
    if node_name == local {
        anyhow::bail!("로컬 노드 '{node_name}'는 동기화 대상이 아닙니다");
    }

    let node_ip = node_ip_from_cluster(node_name)?;
    println!("=== 노드 동기화: {node_name} ({node_ip}) ===\n");

    // 바이너리 동기화
    const LOCAL_BIN: &str = "/usr/local/bin/proxmox-host-setup";
    println!("[node-sync] 바이너리 버전 확인 중...");
    let local_hash = cmd_output("sha256sum", &[LOCAL_BIN]);
    let local_hash = local_hash.split_whitespace().next().unwrap_or("").to_string();
    let remote_hash = ssh_capture(&node_ip, &format!("sha256sum {LOCAL_BIN} 2>/dev/null || true"));
    let remote_hash = remote_hash.split_whitespace().next().unwrap_or("").to_string();

    if local_hash == remote_hash && !local_hash.is_empty() {
        println!("[node-sync] 바이너리 동일 — 스킵");
    } else {
        println!("[node-sync] 바이너리 배포 중...");
        let remote_tmp = format!("{LOCAL_BIN}.new");
        let scp_ok = Command::new("scp")
            .args(["-o", "ConnectTimeout=10", "-o", "StrictHostKeyChecking=no",
                LOCAL_BIN, &format!("root@{node_ip}:{remote_tmp}")])
            .status().map_or(false, |s| s.success());
        if scp_ok && ssh_run(&node_ip, &format!("mv {remote_tmp} {LOCAL_BIN}")) {
            println!("[node-sync] 바이너리 배포 완료");
        } else {
            eprintln!("[node-sync] 바이너리 배포 실패");
        }
    }

    // config.toml 동기화
    const CONFIG_FILE: &str = "/etc/proxmox-host-setup/config.toml";
    println!("\n[node-sync] config.toml 확인 중...");
    let exists = ssh_capture(&node_ip, &format!("test -f {CONFIG_FILE} && echo yes || echo no"));
    if exists.trim() == "yes" {
        println!("[node-sync] config.toml 이미 존재 — 스킵");
    } else {
        println!("[node-sync] config.toml 없음 — 자동 생성");
        // LXC net0에서 gw/subnet 추출
        let list = ssh_capture(&node_ip, "pct list 2>/dev/null | awk 'NR>1 {print $1}' | head -10");
        let mut gateway = String::new();
        let mut subnet = String::from("16");
        for vmid in list.lines() {
            let vmid = vmid.trim();
            if vmid.is_empty() { continue; }
            let cfg = ssh_capture(&node_ip, &format!("pct config {vmid} 2>/dev/null | grep '^net0:' | head -1"));
            if !cfg.contains("bridge=vmbr1") { continue; }
            for field in cfg.split(',') {
                let field = field.trim();
                if let Some(v) = field.strip_prefix("gw=") { gateway = v.to_string(); }
                if let Some(v) = field.strip_prefix("ip=") {
                    if let Some((_, m)) = v.split_once('/') { subnet = m.to_string(); }
                }
            }
            if !gateway.is_empty() { break; }
        }
        // fallback: vmbr1 IP
        if gateway.is_empty() {
            let cidr = ssh_capture(&node_ip, "ip -4 addr show vmbr1 2>/dev/null | awk '/inet / {print $2; exit}'");
            let cidr = cidr.trim();
            if let Some((ip, mask)) = cidr.split_once('/') {
                gateway = ip.to_string();
                subnet = mask.to_string();
            }
        }
        if !gateway.is_empty() {
            let config_body = format!("[network]\ngateway = \"{gateway}\"\nbridge = \"vmbr1\"\nsubnet = \"{subnet}\"\n");
            let write_cmd = format!("mkdir -p /etc/proxmox-host-setup && cat > {CONFIG_FILE} <<'EOF'\n{config_body}EOF");
            if ssh_run(&node_ip, &write_cmd) {
                println!("[node-sync] config.toml 생성 완료");
            } else {
                eprintln!("[node-sync] config.toml 생성 실패");
            }
        } else {
            eprintln!("[node-sync] 네트워크 감지 실패 — config.toml 수동 작성 필요");
        }
    }

    println!("\n=== 노드 '{node_name}' 동기화 완료 ===");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let json_text = r#"[
          {"node":"pve","status":"online","maxcpu":64,"maxmem":549755813888,"uptime":864000},
          {"node":"pve2","status":"offline","maxcpu":32,"maxmem":274877906944,"uptime":0}
        ]"#;
        let rows = parse_pvesh_nodes(json_text).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], NodeRow {
            node: "pve".into(), status: "online".into(), cpus: 64,
            mem_total_gb: 512, uptime_days: 10,
        });
        assert_eq!(rows[1].status, "offline");
    }

    #[test]
    fn parse_missing_optional_fields() {
        let json_text = r#"[{"node":"x"}]"#;
        let rows = parse_pvesh_nodes(json_text).unwrap();
        assert_eq!(rows[0], NodeRow {
            node: "x".into(), status: "unknown".into(),
            cpus: 0, mem_total_gb: 0, uptime_days: 0,
        });
    }

    #[test]
    fn parse_fails_on_missing_node() {
        let json_text = r#"[{"status":"online"}]"#;
        assert!(parse_pvesh_nodes(json_text).is_err());
    }

    #[test]
    fn parse_fails_on_invalid_json() {
        assert!(parse_pvesh_nodes("not-json").is_err());
    }

    #[test]
    fn parse_empty_array() {
        assert_eq!(parse_pvesh_nodes("[]").unwrap().len(), 0);
    }

    #[test]
    fn count_running_basic() {
        let j = r#"[{"status":"running"},{"status":"stopped"},{"status":"running"}]"#;
        assert_eq!(count_running(j), (3, 2));
    }

    #[test]
    fn count_running_invalid_returns_zero() {
        assert_eq!(count_running("not-json"), (0, 0));
    }

    #[test]
    fn count_running_empty() {
        assert_eq!(count_running("[]"), (0, 0));
    }
}
