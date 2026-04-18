//! pxi-net — 네트워크 진단 (read-only). Proxmox 브리지/route/DNS/ping.

use clap::{Parser, Subcommand};
use pxi_core::common;
use serde::Serialize;
use std::process::Command;

#[derive(Parser)]
#[command(name = "pxi-net", about = "네트워크 진단")]
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
    /// 네트워크 상태 (IP forwarding, FORWARD/MASQUERADE 룰, Docker 충돌)
    NetStatus,
    /// 네트워크 감사 (전체 LXC 외부 연결 일괄 테스트)
    NetAudit,
    /// 네트워크 규칙 자동 복구 (FORWARD/MASQUERADE)
    NetFix {
        /// 실제 적용 (없으면 dry-run)
        #[arg(long)]
        apply: bool,
    },
    /// VMID→IP 규칙 위반 전수 점검
    IpAudit,
    /// VMID→IP 규칙 위반 일괄 수정
    IpFix {
        #[arg(long)]
        apply: bool,
    },
    /// 외부 접근 파이프라인 점검 (공인IP → 공유기 → DNAT → Traefik)
    IngressAudit,
    /// 외부 접근 DNAT 룰 자동 복구
    IngressFix {
        #[arg(long)]
        apply: bool,
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
        Cmd::NetStatus => net_status(),
        Cmd::NetAudit => net_audit(),
        Cmd::NetFix { apply } => net_fix(apply),
        Cmd::IpAudit => ip_audit(),
        Cmd::IpFix { apply } => ip_fix(apply),
        Cmd::IngressAudit => ingress_audit(),
        Cmd::IngressFix { apply } => ingress_fix(apply),
    }
}

fn doctor() -> anyhow::Result<()> {
    println!("=== pxi-net doctor ===");
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

// ---------- net-status ----------

fn cmd_output(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd).args(args).output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

const VMBR_INTERNAL: &str = "vmbr1";
const VMBR_EXTERNAL: &str = "vmbr0";
const INTERNAL_CIDR: &str = "10.0.0.0/8";

fn get_forward_policy() -> String {
    let out = cmd_output("iptables", &["-L", "FORWARD", "-n"]);
    out.lines().next()
        .and_then(|l| l.split("policy ").nth(1))
        .and_then(|s| s.split(')').next())
        .unwrap_or("?")
        .to_string()
}

fn has_vmbr_forward_rule() -> bool {
    let out = cmd_output("iptables", &["-L", "FORWARD", "-nv"]);
    out.lines().any(|l| l.contains("ACCEPT") && l.contains(VMBR_INTERNAL) && l.contains(VMBR_EXTERNAL))
}

fn get_masquerade_info() -> String {
    let out = cmd_output("iptables", &["-t", "nat", "-L", "POSTROUTING", "-nv"]);
    for line in out.lines() {
        if line.contains("MASQUERADE") && line.contains(VMBR_EXTERNAL) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 8 {
                return format!("{} -> {}", parts[7], VMBR_EXTERNAL);
            }
        }
    }
    String::new()
}

fn net_status() -> anyhow::Result<()> {
    println!("=== 네트워크 상태 ===\n");

    let fwd = cmd_output("sysctl", &["-n", "net.ipv4.ip_forward"]);
    let fwd_ok = fwd.trim() == "1";
    println!("  IP forwarding: {} ({})", if fwd_ok { "✓" } else { "✗" }, fwd.trim());

    let policy = get_forward_policy();
    let policy_warning = policy == "DROP";
    println!("  FORWARD policy: {} {}", policy,
        if policy_warning { "⚠ (Docker가 DROP으로 변경했을 수 있음)" } else { "" });

    let has_forward = has_vmbr_forward_rule();
    println!("  {VMBR_INTERNAL} FORWARD 룰: {}", if has_forward { "✓" } else { "✗ 누락!" });

    let masq = get_masquerade_info();
    println!("  MASQUERADE: {}", if masq.is_empty() { "✗ 누락!".to_string() } else { format!("✓ ({})", masq) });

    let interfaces = std::fs::read_to_string("/etc/network/interfaces").unwrap_or_default();
    let has_persistent = interfaces.contains("FORWARD") && interfaces.contains(VMBR_INTERNAL) && interfaces.contains(VMBR_EXTERNAL);
    println!("  영구 설정 (interfaces): {}", if has_persistent { "✓" } else { "✗ post-up 규칙 누락" });

    let docker = Command::new("docker").arg("--version").output().map(|o| o.status.success()).unwrap_or(false);
    if docker {
        println!("  Docker: ✓ 설치됨 {}", if policy_warning && !has_forward { "⚠ FORWARD DROP 충돌!" } else { "(충돌 없음)" });
    } else {
        println!("  Docker: 미설치");
    }

    println!();
    if !fwd_ok || (policy_warning && !has_forward) {
        println!("  ⚠ 문제 발견됨 → `pxi-net net-fix --apply`로 복구 가능");
    } else {
        println!("  ✓ 네트워크 정상");
    }
    Ok(())
}

// ---------- net-audit ----------

fn net_audit() -> anyhow::Result<()> {
    println!("=== 네트워크 감사 (전체 LXC 외부 연결 테스트) ===\n");

    if !common::has_cmd("pct") {
        anyhow::bail!("pct 없음 — Proxmox 호스트에서만 동작");
    }

    let pct_list = cmd_output("pct", &["list"]);
    let mut total = 0u32;
    let mut ok = 0u32;
    let mut fail = 0u32;
    let mut failed_list: Vec<(String, String)> = Vec::new();

    for line in pct_list.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 || parts[1] != "running" { continue; }
        let vmid = parts[0];
        let name = parts[2];
        total += 1;

        let ping_ok = Command::new("pct")
            .args(["exec", vmid, "--", "ping", "-c1", "-W2", "8.8.8.8"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status().map(|s| s.success()).unwrap_or(false);

        let dns_ok = Command::new("pct")
            .args(["exec", vmid, "--", "bash", "-c",
                "nslookup google.com >/dev/null 2>&1 || host google.com >/dev/null 2>&1 || getent hosts google.com >/dev/null 2>&1"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status().map(|s| s.success()).unwrap_or(false);

        let config = cmd_output("pct", &["config", vmid]);
        let ip = config.lines().find(|l| l.starts_with("net0:"))
            .and_then(|l| l.split(',').find(|p| p.starts_with("ip=")))
            .map(|p| p.replace("ip=", "")).unwrap_or_else(|| "?".to_string());
        let bridge = config.lines().find(|l| l.starts_with("net0:"))
            .and_then(|l| l.split(',').find(|p| p.starts_with("bridge=")))
            .map(|p| p.replace("bridge=", "")).unwrap_or_else(|| "?".to_string());

        let ping_mark = if ping_ok { "✓" } else { "✗" };
        let dns_mark = if dns_ok { "✓" } else { "✗" };

        if ping_ok && dns_ok {
            ok += 1;
            println!("  ✓ {vmid:<8} {name:<28} {ip:<18} {bridge:<8} ping:{ping_mark} dns:{dns_mark}");
        } else {
            fail += 1;
            failed_list.push((vmid.to_string(), name.to_string()));
            println!("  ✗ {vmid:<8} {name:<28} {ip:<18} {bridge:<8} ping:{ping_mark} dns:{dns_mark}");
        }
    }

    println!("\n{}", "-".repeat(60));
    println!("  총 {total}개 LXC: 정상 {ok}개, 실패 {fail}개");
    if !failed_list.is_empty() {
        println!("\n  실패 목록:");
        for (vmid, name) in &failed_list { println!("    - {vmid} ({name})"); }
        println!("\n  → `pxi-net net-fix --apply`로 FORWARD 규칙 복구 후 재테스트");
    }
    Ok(())
}

// ---------- net-fix ----------

fn net_fix(apply: bool) -> anyhow::Result<()> {
    println!("=== 네트워크 규칙 복구 ===\n");

    let mut issues: Vec<String> = Vec::new();
    let fwd = cmd_output("sysctl", &["-n", "net.ipv4.ip_forward"]);
    if fwd.trim() != "1" { issues.push("IP forwarding 비활성화".to_string()); }
    if !has_vmbr_forward_rule() { issues.push(format!("{VMBR_INTERNAL} -> {VMBR_EXTERNAL} FORWARD 룰 누락")); }
    let masq = get_masquerade_info();
    if masq.is_empty() { issues.push("MASQUERADE 룰 누락".to_string()); }

    if issues.is_empty() {
        println!("  ✓ 네트워크 규칙 정상 — 수정 불필요");
        return Ok(());
    }

    println!("  발견된 문제:");
    for issue in &issues { println!("    ✗ {issue}"); }

    if !apply {
        println!("\n  → `pxi-net net-fix --apply`로 자동 복구");
        return Ok(());
    }

    println!("\n  복구 중...");
    if fwd.trim() != "1" {
        let _ = Command::new("sysctl").args(["-w", "net.ipv4.ip_forward=1"]).status();
        println!("  ✓ IP forwarding 활성화");
    }
    if !has_vmbr_forward_rule() {
        let _ = Command::new("iptables").args(["-I", "FORWARD", "1", "-i", VMBR_INTERNAL, "-o", VMBR_EXTERNAL, "-s", INTERNAL_CIDR, "-j", "ACCEPT"]).status();
        let _ = Command::new("iptables").args(["-I", "FORWARD", "2", "-i", VMBR_EXTERNAL, "-o", VMBR_INTERNAL, "-d", INTERNAL_CIDR, "-j", "ACCEPT"]).status();
        println!("  ✓ FORWARD 룰 추가 ({VMBR_INTERNAL} <-> {VMBR_EXTERNAL})");
    }
    if masq.is_empty() {
        let _ = Command::new("iptables").args(["-t", "nat", "-A", "POSTROUTING", "-s", INTERNAL_CIDR, "-o", VMBR_EXTERNAL, "-j", "MASQUERADE"]).status();
        println!("  ✓ MASQUERADE 룰 추가");
    }

    println!("\n  복구 완료. `pxi-net net-audit`으로 검증하세요.");
    Ok(())
}

// ---------- ip-audit ----------

fn vmid_to_ip_quiet(vmid: &str) -> String {
    let v: u32 = vmid.parse().unwrap_or(0);
    let prefix = v / 1000;
    let host = v % 1000;
    format!("10.0.{prefix}.{host}")
}

fn ip_audit() -> anyhow::Result<()> {
    println!("=== VMID->IP 규칙 점검 ===\n");
    println!("  규칙: VMID XXYYYY -> IP 10.0.XX.YYYY\n");

    if !common::has_cmd("pct") { anyhow::bail!("pct 없음"); }

    let pct_list = cmd_output("pct", &["list"]);
    let mut total = 0u32;
    let mut violations = 0u32;

    for line in pct_list.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 { continue; }
        let vmid = parts[0];
        let status = parts[1];
        let name = parts[2];
        total += 1;
        let expected = vmid_to_ip_quiet(vmid);

        let config_raw = Command::new("pct").args(["config", vmid]).output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        let actual = config_raw.lines().find(|l| l.starts_with("net0:"))
            .and_then(|l| l.split(',').find(|p| p.starts_with("ip=")))
            .map(|p| p.replace("ip=", "").split('/').next().unwrap_or("").to_string())
            .unwrap_or_else(|| "?".to_string());

        if actual == expected {
            println!("  ✓ {vmid:<8} {name:<28} {actual}");
        } else {
            violations += 1;
            println!("  ✗ {vmid:<8} {name:<28} got={actual:<18} expected={expected}  ({status})");
        }
    }

    println!("\n  총 {total}개 LXC, 위반 {violations}개");
    if violations > 0 {
        println!("  → `pxi-net ip-fix --apply`로 IP 일괄 수정");
    }
    Ok(())
}

// ---------- ip-fix ----------

fn ip_fix(apply: bool) -> anyhow::Result<()> {
    println!("=== VMID->IP 규칙 위반 수정 ===\n");

    if !common::has_cmd("pct") { anyhow::bail!("pct 없음"); }

    let pct_list = cmd_output("pct", &["list"]);
    let mut fixes: Vec<(String, String, String, String, String)> = Vec::new();

    for line in pct_list.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 { continue; }
        let vmid = parts[0];
        let status = parts[1];
        let name = parts[2];
        let expected = vmid_to_ip_quiet(vmid);

        let raw = Command::new("pct").args(["config", vmid]).output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        let net0_line = raw.lines().find(|l| l.starts_with("net0:")).unwrap_or("");
        let actual = net0_line.split(',').find(|p| p.starts_with("ip="))
            .map(|p| p.replace("ip=", "").split('/').next().unwrap_or("").to_string())
            .unwrap_or_default();

        if !actual.is_empty() && actual != expected {
            fixes.push((vmid.into(), name.into(), status.into(), actual, expected));
        }
    }

    if fixes.is_empty() {
        println!("  ✓ 위반 없음");
        return Ok(());
    }

    for (vmid, name, status, old_ip, new_ip) in &fixes {
        println!("  {vmid} ({name}) [{status}]: {old_ip} -> {new_ip}");
        if !apply { continue; }

        let was_running = status == "running";
        if was_running {
            println!("    정지 중...");
            let _ = Command::new("pct").args(["stop", vmid]).status();
            for _ in 0..30 {
                let s = cmd_output("pct", &["status", vmid]);
                if s.contains("stopped") { break; }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }

        let raw = Command::new("pct").args(["config", vmid]).output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        let net0_line = raw.lines().find(|l| l.starts_with("net0:")).unwrap_or("");
        if net0_line.is_empty() {
            eprintln!("    ✗ net0 설정을 찾을 수 없음");
            continue;
        }

        let net0_value = net0_line.trim_start_matches("net0: ").trim_start_matches("net0:");
        let new_net0: String = net0_value.split(',').map(|part| {
            if part.starts_with("ip=") { format!("ip={new_ip}/16") }
            else if part.starts_with("bridge=") { format!("bridge={VMBR_INTERNAL}") }
            else { part.to_string() }
        }).collect::<Vec<_>>().join(",");

        match Command::new("pct").args(["set", vmid, "--net0", &new_net0]).output() {
            Ok(o) if o.status.success() => println!("    ✓ IP 변경 완료: {new_ip}"),
            Ok(o) => eprintln!("    ✗ 변경 실패: {}", String::from_utf8_lossy(&o.stderr)),
            Err(e) => eprintln!("    ✗ 실행 실패: {e}"),
        }

        if was_running {
            println!("    시작 중...");
            let _ = Command::new("pct").args(["start", vmid]).status();
        }
    }

    if apply {
        println!("\n  완료. `pxi-net ip-audit`으로 재검증하세요.");
    } else {
        println!("\n  → `pxi-net ip-fix --apply`로 실제 적용");
    }
    Ok(())
}

// ---------- ingress-audit ----------

fn ingress_audit() -> anyhow::Result<()> {
    println!("=== 외부 접근 파이프라인 점검 ===\n");

    let host_ext_ip = std::env::var("HOST_EXTERNAL_IP").unwrap_or_default();
    let traefik_ip = std::env::var("TRAEFIK_INTERNAL_IP").unwrap_or_default();

    if traefik_ip.is_empty() || host_ext_ip.is_empty() {
        println!("  .env에 HOST_EXTERNAL_IP, TRAEFIK_INTERNAL_IP 설정 필요");
        return Ok(());
    }

    let mut issues = 0u32;

    // 공인 IP
    print!("  [1/4] 공인 IP: ");
    let actual_ip = cmd_output("curl", &["-sf", "--max-time", "5", "https://api.ipify.org"]);
    if actual_ip.is_empty() {
        println!("? (확인 불가)");
    } else {
        println!("✓ {actual_ip}");
    }

    // DNAT
    print!("  [2/4] 호스트 DNAT: ");
    let dnat = cmd_output("iptables", &["-t", "nat", "-L", "PREROUTING", "-nv"]);
    let has_http = dnat.contains("dpt:80") && dnat.contains(&traefik_ip);
    let has_https = dnat.contains("dpt:443") && dnat.contains(&traefik_ip);
    if has_http && has_https {
        println!("✓ 80->{traefik_ip}:80, 443->{traefik_ip}:443");
    } else {
        println!("✗ DNAT 룰 누락");
        issues += 1;
    }

    // Traefik 응답
    print!("  [3/4] Traefik: ");
    let traefik_ok = Command::new("curl")
        .args(["-sk", "--max-time", "3", &format!("https://{traefik_ip}:443/"), "-o", "/dev/null", "-w", "%{http_code}"])
        .output()
        .map(|o| { let c = String::from_utf8_lossy(&o.stdout); let c = c.trim(); c == "200" || c == "301" || c == "302" || c == "404" })
        .unwrap_or(false);
    if traefik_ok {
        println!("✓ {traefik_ip}:443 응답");
    } else {
        println!("✗ {traefik_ip}:443 응답 없음");
        issues += 1;
    }

    // DNS
    print!("  [4/4] DNS: ");
    if common::has_cmd("dig") {
        let dns_ip = cmd_output("dig", &["+short", "50.internal.kr", "@1.1.1.1"]);
        if !dns_ip.trim().is_empty() {
            println!("✓ 50.internal.kr -> {}", dns_ip.trim());
        } else {
            println!("✗ DNS 해석 실패");
            issues += 1;
        }
    } else {
        println!("? (dig 없음)");
    }

    println!("\n{}", "-".repeat(60));
    if issues == 0 {
        println!("  ✓ 외부 접근 파이프라인 정상");
    } else {
        println!("  ⚠ {issues}건 문제 발견 → `pxi-net ingress-fix --apply`로 DNAT 복구");
    }
    Ok(())
}

// ---------- ingress-fix ----------

fn ingress_fix(apply: bool) -> anyhow::Result<()> {
    println!("=== 외부 접근 DNAT 복구 ===\n");

    let traefik_ip = std::env::var("TRAEFIK_INTERNAL_IP").unwrap_or_default();
    if traefik_ip.is_empty() {
        anyhow::bail!("TRAEFIK_INTERNAL_IP 미설정 — .env에 추가 후 재실행");
    }

    let dnat = cmd_output("iptables", &["-t", "nat", "-L", "PREROUTING", "-nv"]);
    let has_http = dnat.contains("dpt:80") && dnat.contains(&traefik_ip);
    let has_https = dnat.contains("dpt:443") && dnat.contains(&traefik_ip);

    if has_http && has_https {
        println!("  ✓ DNAT 룰 정상");
        return Ok(());
    }

    if !apply {
        if !has_http { println!("  ✗ HTTP DNAT 누락: vmbr0:80 -> {traefik_ip}:80"); }
        if !has_https { println!("  ✗ HTTPS DNAT 누락: vmbr0:443 -> {traefik_ip}:443"); }
        println!("\n  → `pxi-net ingress-fix --apply`로 적용");
        return Ok(());
    }

    if !has_http {
        let _ = Command::new("iptables").args([
            "-t", "nat", "-A", "PREROUTING", "-i", "vmbr0",
            "-p", "tcp", "--dport", "80",
            "-j", "DNAT", "--to-destination", &format!("{traefik_ip}:80"),
            "-m", "comment", "--comment", "traefik-http",
        ]).status();
        println!("  ✓ HTTP DNAT 추가");
    }
    if !has_https {
        let _ = Command::new("iptables").args([
            "-t", "nat", "-A", "PREROUTING", "-i", "vmbr0",
            "-p", "tcp", "--dport", "443",
            "-j", "DNAT", "--to-destination", &format!("{traefik_ip}:443"),
            "-m", "comment", "--comment", "traefik-https",
        ]).status();
        println!("  ✓ HTTPS DNAT 추가");
    }

    println!("\n  복구 완료. `pxi-net ingress-audit`으로 검증하세요.");
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
