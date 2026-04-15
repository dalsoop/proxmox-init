//! prelik-net — 네트워크 진단 (read-only). Proxmox 브리지/route/DNS/ping.

use clap::{Parser, Subcommand};
use prelik_core::common;
use serde::Serialize;
use std::process::Command;

#[derive(Parser)]
#[command(name = "prelik-net", about = "네트워크 진단")]
struct Cli {
    /// JSON 출력 (자동화/CI 친화)
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 의존성 점검
    Doctor,
    /// 네트워크 인터페이스 요약 (ip -br address)
    Interfaces,
    /// 라우팅 테이블
    Routes,
    /// Proxmox 브리지 점검 (vmbr*)
    Bridges,
    /// DNS 조회 (host/getent)
    Dns { host: String },
    /// ping (3회, count 변경 가능)
    Ping {
        host: String,
        #[arg(long, default_value = "3")]
        count: u32,
    },
}

#[derive(Serialize, Debug, PartialEq)]
struct Iface {
    name: String,
    state: String,
    addresses: Vec<String>,
}

#[derive(Serialize, Debug, PartialEq)]
struct Route {
    destination: String,
    via: Option<String>,
    dev: Option<String>,
    proto: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
struct Bridge {
    name: String,
    interfaces: Vec<String>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Doctor => doctor(),
        Cmd::Interfaces => interfaces(cli.json),
        Cmd::Routes => routes(cli.json),
        Cmd::Bridges => bridges(cli.json),
        Cmd::Dns { host } => dns(&host, cli.json),
        Cmd::Ping { host, count } => ping(&host, count, cli.json),
    }
}

fn doctor() -> anyhow::Result<()> {
    println!("=== prelik-net doctor ===");
    println!("  ip       : {}", mark(common::has_cmd("ip")));
    println!("  ping     : {}", mark(common::has_cmd("ping")));
    println!("  getent   : {}", mark(common::has_cmd("getent")));
    println!("  bridge   : {} (선택, Proxmox 브리지)", mark(common::has_cmd("bridge")));
    println!("  brctl    : {} (선택, legacy)", mark(common::has_cmd("brctl")));
    Ok(())
}

fn mark(b: bool) -> &'static str { if b { "✓" } else { "✗" } }

// ---------- interfaces ----------

fn parse_ip_addr_brief(text: &str) -> Vec<Iface> {
    // `ip -br address` 출력: "name state addr1,addr2,..."
    text.lines().filter_map(|l| {
        let p: Vec<&str> = l.split_whitespace().collect();
        if p.len() < 2 { return None; }
        let addrs: Vec<String> = if p.len() >= 3 {
            p[2..].iter().map(|s| s.to_string()).collect()
        } else {
            vec![]
        };
        Some(Iface { name: p[0].into(), state: p[1].into(), addresses: addrs })
    }).collect()
}

fn interfaces(json: bool) -> anyhow::Result<()> {
    let out = common::run("ip", &["-br", "address"])?;
    if !json {
        println!("=== 인터페이스 ===\n{out}");
        return Ok(());
    }
    let rows = parse_ip_addr_brief(&out);
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}

// ---------- routes ----------

fn parse_ip_route(text: &str) -> Vec<Route> {
    // 라인 예: "default via 192.168.1.1 dev eth0 proto dhcp metric 100"
    //          "10.0.0.0/24 dev eth1 proto kernel scope link src 10.0.0.5"
    text.lines().filter_map(|l| {
        let toks: Vec<&str> = l.split_whitespace().collect();
        if toks.is_empty() { return None; }
        let destination = toks[0].to_string();
        let mut via = None;
        let mut dev = None;
        let mut proto = None;
        let mut i = 1;
        while i + 1 < toks.len() {
            match toks[i] {
                "via" => via = Some(toks[i+1].to_string()),
                "dev" => dev = Some(toks[i+1].to_string()),
                "proto" => proto = Some(toks[i+1].to_string()),
                _ => {}
            }
            i += 1;
        }
        Some(Route { destination, via, dev, proto })
    }).collect()
}

fn routes(json: bool) -> anyhow::Result<()> {
    let out = common::run("ip", &["route"])?;
    if !json {
        println!("=== 라우팅 테이블 ===\n{out}");
        return Ok(());
    }
    println!("{}", serde_json::to_string_pretty(&parse_ip_route(&out))?);
    Ok(())
}

// ---------- bridges ----------

fn bridges(json: bool) -> anyhow::Result<()> {
    // ip -d link show type bridge → name 추출
    if !common::has_cmd("ip") {
        anyhow::bail!("ip 바이너리 없음");
    }
    let names_out = common::run("ip", &["-br", "link", "show", "type", "bridge"])?;
    let bridge_names: Vec<String> = names_out.lines()
        .filter_map(|l| l.split_whitespace().next().map(|s| s.split('@').next().unwrap_or(s).to_string()))
        .collect();

    let mut bs = Vec::new();
    for name in &bridge_names {
        // bridge 멤버: ls /sys/class/net/<bridge>/brif/
        let dir = format!("/sys/class/net/{name}/brif");
        let mut ifaces: Vec<String> = std::fs::read_dir(&dir).ok()
            .map(|rd| rd.flatten().filter_map(|e| e.file_name().into_string().ok()).collect())
            .unwrap_or_default();
        ifaces.sort();
        bs.push(Bridge { name: name.clone(), interfaces: ifaces });
    }

    if !json {
        println!("=== 브리지 ===");
        for b in &bs {
            println!("  {}: {}", b.name, if b.interfaces.is_empty() { "(no members)".into() } else { b.interfaces.join(", ") });
        }
        return Ok(());
    }
    println!("{}", serde_json::to_string_pretty(&bs)?);
    Ok(())
}

// ---------- dns ----------

fn dns(host: &str, json: bool) -> anyhow::Result<()> {
    // getent ahosts <host> → "<addr> <type> <canonical>"
    if !common::has_cmd("getent") {
        anyhow::bail!("getent 바이너리 없음");
    }
    let out = Command::new("getent").args(["ahosts", host]).output()?;
    if !out.status.success() {
        anyhow::bail!("DNS 조회 실패: {host} ({})", String::from_utf8_lossy(&out.stderr));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let addrs: Vec<String> = text.lines()
        .filter_map(|l| l.split_whitespace().next().map(|s| s.to_string()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter().collect();

    if !json {
        println!("=== DNS: {host} ===");
        for a in &addrs { println!("  {a}"); }
        return Ok(());
    }
    let payload = serde_json::json!({ "host": host, "addresses": addrs });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

// ---------- ping ----------

fn ping(host: &str, count: u32, json: bool) -> anyhow::Result<()> {
    if !common::has_cmd("ping") {
        anyhow::bail!("ping 바이너리 없음");
    }
    let count_s = count.to_string();
    let out = Command::new("ping").args(["-c", &count_s, "-W", "2", host]).output()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if !json {
        println!("{text}");
        if !out.status.success() {
            anyhow::bail!("ping {host} 실패");
        }
        return Ok(());
    }
    // 마지막 라인에서 packet loss 추출. 예: "3 packets transmitted, 3 received, 0% packet loss, time 2003ms"
    let summary = text.lines().rev()
        .find(|l| l.contains("packet loss"))
        .unwrap_or("");
    let loss_pct: Option<u32> = summary.split(',')
        .find(|s| s.contains("packet loss"))
        .and_then(|s| s.trim().split('%').next())
        .and_then(|s| s.trim().parse().ok());
    let payload = serde_json::json!({
        "host": host,
        "count": count,
        "success": out.status.success(),
        "loss_pct": loss_pct,
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    if !out.status.success() {
        anyhow::bail!("ping {host} 실패");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iface_basic() {
        let text = "lo               UNKNOWN        127.0.0.1/8 ::1/128\n\
                    eth0             UP             10.0.50.1/16 fe80::1/64\n";
        let rows = parse_ip_addr_brief(text);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "lo");
        assert_eq!(rows[0].state, "UNKNOWN");
        assert_eq!(rows[1].addresses, vec!["10.0.50.1/16".to_string(), "fe80::1/64".into()]);
    }

    #[test]
    fn iface_no_addr() {
        let text = "vmbr2 DOWN\n";
        let rows = parse_ip_addr_brief(text);
        assert_eq!(rows[0].addresses, Vec::<String>::new());
    }

    #[test]
    fn iface_empty() {
        assert_eq!(parse_ip_addr_brief("").len(), 0);
    }

    #[test]
    fn route_default() {
        let text = "default via 192.168.1.1 dev eth0 proto dhcp metric 100\n";
        let rows = parse_ip_route(text);
        assert_eq!(rows[0], Route {
            destination: "default".into(),
            via: Some("192.168.1.1".into()),
            dev: Some("eth0".into()),
            proto: Some("dhcp".into()),
        });
    }

    #[test]
    fn route_link_local() {
        let text = "10.0.0.0/24 dev eth1 proto kernel scope link src 10.0.0.5\n";
        let rows = parse_ip_route(text);
        assert_eq!(rows[0].destination, "10.0.0.0/24");
        assert_eq!(rows[0].via, None);
        assert_eq!(rows[0].dev, Some("eth1".into()));
        assert_eq!(rows[0].proto, Some("kernel".into()));
    }

    #[test]
    fn route_multi_lines() {
        let text = "default via 1.1.1.1 dev eth0\n10.0.0.0/8 dev eth1\n";
        assert_eq!(parse_ip_route(text).len(), 2);
    }
}
