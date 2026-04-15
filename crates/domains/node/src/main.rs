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
    // 권한 경계: 실제 Proxmox 클러스터 멤버만 허용. 임의 호스트 SSH 프리미티브로 변질 금지.
    let nodes_json = common::run("pvesh", &["get", "/nodes", "--output-format", "json"])?;
    let rows = parse_pvesh_nodes(&nodes_json)?;
    if !rows.iter().any(|r| r.node == node) {
        anyhow::bail!(
            "노드 '{node}'는 클러스터 멤버가 아님 (prelik-node list 로 확인). \
             임의 호스트 SSH는 차단됩니다."
        );
    }

    let target = format!("root@{node}");
    let joined = cmd.join(" ");
    // StrictHostKeyChecking=yes — 첫 연결 자동 신뢰 금지.
    // 사전에 known_hosts에 등록되어 있어야 함 (Proxmox 클러스터는 보통 가입 시 등록됨).
    let status = Command::new("ssh")
        .args([
            "-o", "ConnectTimeout=10",
            "-o", "StrictHostKeyChecking=yes",
            "-o", "BatchMode=yes",
            &target, &joined,
        ])
        .status()?;
    std::process::exit(status.code().unwrap_or(1));
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
