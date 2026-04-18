//! pxi-account — 범용 리눅스 계정 관리.
//! create / remove / list / status / ssh-key-add / teardown / roles-init / roles-apply /
//! roles-status / proxmox-silo.

use clap::{Parser, Subcommand};
use pxi_core::common;
use pxi_core::helpers;

use std::fs;
use std::io::Read;
use std::path::Path;

#[derive(Parser)]
#[command(name = "pxi-account", about = "리눅스 계정 관리")]
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
    /// 전체 계정 삭제 (홈 디렉토리 + sudo + SSH 키)
    Teardown {
        /// 삭제할 계정 이름 (여러 개: 쉼표 구분)
        #[arg(long)]
        names: String,
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
    /// roles.toml 초기화
    RolesInit,
    /// roles.toml 기반 sudoers + Proxmox Pool/ACL 적용
    RolesApply,
    /// 역할 상태 확인
    RolesStatus,
    /// Proxmox user + Pool + ACL + VM 할당을 한 번에 구성 (격리 사일로)
    ProxmoxSilo {
        /// Proxmox 사용자 ID (예: gitlab@pve)
        #[arg(long)]
        userid: String,
        /// 격리용 Pool 이름
        #[arg(long)]
        pool: String,
        /// 포함할 VMID/LXC 목록 (쉼표 구분)
        #[arg(long)]
        vmids: Option<String>,
        /// 이름 prefix로 대상 자동 선택
        #[arg(long)]
        name_prefix: Option<String>,
        /// 태그로 대상 자동 선택 (세미콜론/쉼표 구분)
        #[arg(long)]
        tags: Option<String>,
        /// 부여할 Proxmox 역할
        #[arg(long, default_value = "PVEVMAdmin")]
        role: String,
        /// Pool 및 사용자 설명
        #[arg(long, default_value = "")]
        comment: String,
        /// 초기 비밀번호 (미지정 + 신규 사용자면 자동 생성)
        #[arg(long)]
        password: Option<String>,
        /// 다른 Pool에 있어도 강제로 이동
        #[arg(long, default_value = "false")]
        allow_move: bool,
    },
    /// 의존 도구 점검
    Doctor,
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Create { name, sudo, ssh_key, shell } => create(&name, sudo, ssh_key.as_deref(), &shell),
        Cmd::Remove { name, purge } => remove(&name, purge),
        Cmd::Teardown { names } => teardown(&names),
        Cmd::List => { list(); Ok(()) }
        Cmd::Status { name } => { status(&name); Ok(()) }
        Cmd::SshKeyAdd { name, key } => ssh_key_add(&name, &key),
        Cmd::RolesInit => roles_init(),
        Cmd::RolesApply => roles_apply(),
        Cmd::RolesStatus => { roles_status(); Ok(()) }
        Cmd::ProxmoxSilo { userid, pool, vmids, name_prefix, tags, role, comment, password, allow_move } => {
            proxmox_silo(&userid, &pool, vmids.as_deref(), name_prefix.as_deref(),
                tags.as_deref(), &role, Some(&comment), password.as_deref(), allow_move)
        }
        Cmd::Doctor => { doctor(); Ok(()) }
    }
}

// ---------------------------------------------------------------------------
// create (포트 from phs accounts + 일반화)
// ---------------------------------------------------------------------------

fn create(name: &str, sudo: bool, ssh_key: Option<&str>, shell: &str) -> anyhow::Result<()> {
    validate_name(name)?;
    println!("=== 계정 생성: {name} ===");

    if common::run("id", &["-u", name]).is_ok() {
        anyhow::bail!("이미 존재: {name}. remove 후 재생성 또는 다른 이름.");
    }

    common::run("useradd", &["-m", "-s", shell, name])?;
    println!("  + 사용자 + 홈 디렉토리");

    if sudo {
        setup_sudoers_for(name)?;
    }

    if let Some(key) = ssh_key {
        ssh_key_add(name, key)?;
    }

    println!("\n+ {name} 생성 완료");
    status(name);
    Ok(())
}

fn setup_sudoers_for(name: &str) -> anyhow::Result<()> {
    let sudoers_file = format!("/etc/sudoers.d/pxi-{name}");
    let content = format!("{name} ALL=(ALL) NOPASSWD:ALL\n");

    let (tmp, _guard) = helpers::secure_tempfile()?;
    fs::write(&tmp, content)?;
    // visudo 검증
    let visudo = find_visudo();
    common::run(&visudo, &["-cf", &tmp])?;
    common::run("install", &["-m", "440", "-o", "root", "-g", "root", &tmp, &sudoers_file])?;
    println!("  + sudo 권한: {sudoers_file}");
    Ok(())
}

// ---------------------------------------------------------------------------
// remove
// ---------------------------------------------------------------------------

fn remove(name: &str, purge: bool) -> anyhow::Result<()> {
    validate_name(name)?;
    println!("=== 계정 제거: {name} (purge={purge}) ===");

    if common::run("id", &["-u", name]).is_err() {
        println!("  = {name} 이미 없음");
        return Ok(());
    }

    // sudoers 제거
    let sudoers_file = format!("/etc/sudoers.d/pxi-{name}");
    if Path::new(&sudoers_file).exists() {
        common::run("rm", &["-f", &sudoers_file])?;
        println!("  + sudoers.d 제거");
    }

    // userdel
    if purge {
        common::run("userdel", &["-r", name])?;
        println!("  + 사용자 삭제 (홈 디렉토리 포함)");
    } else {
        common::run("userdel", &[name])?;
        println!("  + 사용자 삭제 (홈 디렉토리 보존 -- --purge로 완전 삭제 가능)");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// teardown (포트 from phs accounts teardown)
// ---------------------------------------------------------------------------

fn teardown(names_csv: &str) -> anyhow::Result<()> {
    println!("=== 계정 일괄 삭제 ===\n");
    let names: Vec<&str> = names_csv.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();

    for name in &names {
        if common::run("id", &[name]).is_err() {
            println!("[계정] {name} - 존재하지 않음, 스킵");
            continue;
        }
        match common::run("userdel", &["-r", name]) {
            Ok(_) => println!("[계정] {name} - 삭제 완료 (홈 디렉토리 포함)"),
            Err(e) => eprintln!("[계정] {name} 삭제 실패: {e}"),
        }
        // sudoers cleanup
        let sudoers_file = format!("/etc/sudoers.d/pxi-{name}");
        if Path::new(&sudoers_file).exists() {
            let _ = fs::remove_file(&sudoers_file);
        }
    }

    // 공통 sudoers 파일 삭제 (구 phs 호환)
    let old_sudoers = "/etc/sudoers.d/dalroot";
    if Path::new(old_sudoers).exists() {
        fs::remove_file(old_sudoers)?;
        println!("[sudo] {} 파일 삭제 완료", old_sudoers);
    }

    println!("\n=== 삭제 완료 ===");
    Ok(())
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

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
        Err(e) => eprintln!("  - {e}"),
    }
}

// ---------------------------------------------------------------------------
// status
// ---------------------------------------------------------------------------

fn status(name: &str) {
    println!("=== {name} 상태 ===");
    match common::run("id", &[name]) {
        Ok(out) => println!("  id:    {}", out.trim()),
        Err(_) => { println!("  - 존재하지 않음"); return; }
    }
    if let Ok(home) = common::run("getent", &["passwd", name]) {
        if let Some(h) = home.split(':').nth(5) {
            println!("  home:  {} (exists: {})", h, Path::new(h).exists());
        }
    }
    let sudoers = format!("/etc/sudoers.d/pxi-{name}");
    println!("  sudo:  {}", if Path::new(&sudoers).exists() { "+" } else { "-" });

    if let Ok(home) = common::run_bash(&format!("getent passwd {name} | cut -d: -f6")) {
        let auth_keys = format!("{}/.ssh/authorized_keys", home.trim());
        if Path::new(&auth_keys).exists() {
            let count = common::run_bash(&format!("wc -l < {auth_keys} 2>/dev/null"))
                .unwrap_or_default();
            println!("  ssh keys: + ({} 줄)", count.trim());
        } else {
            println!("  ssh keys: -");
        }
    }
}

// ---------------------------------------------------------------------------
// ssh_key_add
// ---------------------------------------------------------------------------

fn ssh_key_add(name: &str, key: &str) -> anyhow::Result<()> {
    validate_name(name)?;

    let key_content = if Path::new(key).exists() {
        fs::read_to_string(key)?
    } else {
        key.to_string()
    };
    let key_content = key_content.trim();

    if !key_content.starts_with("ssh-") {
        anyhow::bail!("올바른 SSH 공개키 형식 아님 (ssh-rsa/ssh-ed25519/ssh-ecdsa로 시작해야)");
    }

    println!("=== {name}에 SSH 키 추가 ===");

    let home = common::run_bash(&format!("getent passwd {name} | cut -d: -f6"))?.trim().to_string();
    if home.is_empty() { anyhow::bail!("{name} 홈 디렉토리 조회 실패"); }

    let ssh_dir = format!("{home}/.ssh");
    let auth_keys = format!("{ssh_dir}/authorized_keys");

    common::run("mkdir", &["-p", &ssh_dir])?;
    common::run("chmod", &["700", &ssh_dir])?;
    common::run("chown", &[&format!("{name}:{name}"), &ssh_dir])?;

    // 중복 확인
    if Path::new(&auth_keys).exists() {
        let existing = fs::read_to_string(&auth_keys).unwrap_or_default();
        if existing.contains(key_content) {
            println!("  = 이미 등록된 키");
            return Ok(());
        }
    }

    // append
    let mut existing = fs::read_to_string(&auth_keys).unwrap_or_default();
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(key_content);
    existing.push('\n');
    fs::write(&auth_keys, &existing)?;
    common::run("chmod", &["600", &auth_keys])?;
    common::run("chown", &[&format!("{name}:{name}"), &auth_keys])?;
    println!("  + 키 추가 + 권한 정리");
    Ok(())
}

// ---------------------------------------------------------------------------
// roles-init (포트 from phs rbac)
// ---------------------------------------------------------------------------

const ROLES_TOML_TEMPLATE: &str = r#"# pxi account roles configuration
# 사용자별 역할 정의. roles-apply로 sudoers + Proxmox ACL에 반영.
#
# [username]
# role = "developer"        # 역할 이름 (자유)
# domains = ["infra", "ai"] # 접근 가능 도메인 (또는 ["*"] 전체)
# vmid_range = [50100, 50199] # 접근 가능 VMID 범위
# pool = "dev-pool"         # Proxmox Pool 이름
# proxmox_role = "PVEVMAdmin" # Proxmox 역할
# description = "개발자"
"#;

fn roles_init() -> anyhow::Result<()> {
    let config_dir = pxi_core::paths::config_dir()?;
    fs::create_dir_all(&config_dir)?;
    let roles_path = config_dir.join("roles.toml");

    if roles_path.exists() {
        println!("[rbac] {} 이미 존재", roles_path.display());
        return Ok(());
    }

    fs::write(&roles_path, ROLES_TOML_TEMPLATE)?;
    common::run("chmod", &["640", &roles_path.display().to_string()])?;
    println!("[rbac] {} 생성 완료", roles_path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// roles-apply (포트 from phs rbac)
// ---------------------------------------------------------------------------

fn roles_apply() -> anyhow::Result<()> {
    println!("=== 역할 적용 ===\n");

    let roles = load_roles()?;
    if roles.is_empty() {
        anyhow::bail!("roles.toml 없음 또는 비어있음. 먼저: pxi run account roles-init");
    }

    // sudoers 생성
    apply_sudoers(&roles)?;

    // Proxmox Pool + ACL (pvesh가 있을 때만)
    if common::has_cmd("pvesh") {
        apply_proxmox_pools(&roles)?;
        apply_proxmox_acl(&roles)?;
    } else {
        println!("[proxmox] pvesh 없음 -- Proxmox ACL 스킵");
    }

    println!("\n=== 역할 적용 완료 ===");
    Ok(())
}

fn apply_sudoers(roles: &std::collections::HashMap<String, RoleConfig>) -> anyhow::Result<()> {
    println!("[sudoers] 역할 기반 sudoers 생성...");
    let sudoers_file = "/etc/sudoers.d/pxi-roles";
    let mut content = String::from("# pxi-account RBAC sudoers (자동 생성)\n# 수동 편집 금지 -- roles-apply로 재생성\n\n");

    for (account, role) in roles {
        if role.domains.contains(&"*".to_string()) {
            content.push_str(&format!("{account}  ALL=(ALL:ALL) NOPASSWD: ALL\n"));
        } else {
            content.push_str(&format!("{account}  ALL=(ALL:ALL) NOPASSWD: ALL\n"));
        }
    }

    let (tmp, _guard) = helpers::secure_tempfile()?;
    fs::write(&tmp, &content)?;
    let visudo = find_visudo();
    match common::run(&visudo, &["-cf", &tmp]) {
        Ok(_) => {
            common::run("install", &["-m", "440", "-o", "root", "-g", "root", &tmp, sudoers_file])?;
            println!("[sudoers] 적용 완료 (검증 통과)");
        }
        Err(e) => {
            eprintln!("[sudoers] 문법 오류! {e}");
        }
    }
    Ok(())
}

fn apply_proxmox_pools(roles: &std::collections::HashMap<String, RoleConfig>) -> anyhow::Result<()> {
    println!("[proxmox] Pool 생성...");
    for (account, role) in roles {
        if role.pool.is_empty() { continue; }
        let exists = common::run("pvesh", &["get", &format!("/pools/{}", role.pool)]).is_ok();
        if exists {
            println!("[proxmox] Pool '{}' 이미 존재", role.pool);
        } else {
            match common::run("pvesh", &["create", "/pools", "--poolid", &role.pool, "--comment", &role.description]) {
                Ok(_) => println!("[proxmox] Pool '{}' 생성 완료 ({})", role.pool, account),
                Err(_) => eprintln!("[proxmox] Pool '{}' 생성 실패", role.pool),
            }
        }
    }
    Ok(())
}

fn apply_proxmox_acl(roles: &std::collections::HashMap<String, RoleConfig>) -> anyhow::Result<()> {
    println!("[proxmox] ACL 적용...");
    for (account, role) in roles {
        if role.pool.is_empty() || role.proxmox_role.is_empty() { continue; }
        let pve_user = format!("{account}@pam");
        // 사용자 생성 (이미 있으면 무시)
        let _ = common::run("pveum", &["user", "add", &pve_user, "--comment", &role.description]);
        let path = format!("/pool/{}", role.pool);
        match common::run("pveum", &["acl", "modify", &path, "--users", &pve_user, "--roles", &role.proxmox_role]) {
            Ok(_) => println!("[proxmox] ACL: {account} -> {} ({})", role.pool, role.proxmox_role),
            Err(_) => eprintln!("[proxmox] ACL 적용 실패: {account}"),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// roles-status (포트 from phs rbac status)
// ---------------------------------------------------------------------------

fn roles_status() {
    println!("=== 역할 상태 ===\n");

    let roles = match load_roles() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[rbac] roles.toml 로드 실패: {e}");
            return;
        }
    };

    if roles.is_empty() {
        println!("[roles.toml] 비어있거나 없음");
        return;
    }

    let caller = std::env::var("SUDO_USER").unwrap_or_else(|_|
        common::run("whoami", &[]).unwrap_or_else(|_| "unknown".to_string())
    );
    println!("[현재 사용자] {caller}\n");

    println!("[역할 정의]");
    for (account, role) in &roles {
        let domains = if role.domains.contains(&"*".to_string()) {
            "전체".to_string()
        } else {
            role.domains.join(", ")
        };
        let vmid = if role.vmid_range.len() == 2 {
            format!("{}-{}", role.vmid_range[0], role.vmid_range[1])
        } else {
            "-".to_string()
        };
        let pool_mark = if !role.pool.is_empty() && common::has_cmd("pvesh") {
            if common::run("pvesh", &["get", &format!("/pools/{}", role.pool)]).is_ok() { "+" } else { "-" }
        } else { "=" };

        println!(
            "  {account:<20} role:{:<14} domains:{domains:<25} vmid:{vmid:<10} pool:{pool_mark} {}",
            role.role, role.description
        );
    }
}

// ---------------------------------------------------------------------------
// proxmox-silo (포트 from phs rbac proxmox_silo)
// ---------------------------------------------------------------------------

fn proxmox_silo(
    userid: &str,
    pool: &str,
    vmids_csv: Option<&str>,
    name_prefix: Option<&str>,
    tags_csv: Option<&str>,
    role: &str,
    comment: Option<&str>,
    password: Option<&str>,
    allow_move: bool,
) -> anyhow::Result<()> {
    println!("=== Proxmox Silo 구성 ===\n");

    if !userid.contains('@') {
        anyhow::bail!("userid는 name@realm 형식이어야 합니다: {}", userid);
    }
    if !common::has_cmd("pvesh") {
        anyhow::bail!("pvesh 없음 -- Proxmox 환경에서만 사용 가능");
    }

    let resources = cluster_vm_resources()?;
    let vmids = resolve_target_vmids(&resources, vmids_csv, name_prefix, tags_csv)?;
    if vmids.is_empty() {
        anyhow::bail!("선택된 대상이 없습니다. --vmids 또는 --name-prefix/--tags를 확인하세요.");
    }

    let comment_value = comment.unwrap_or("");
    let user_exists = proxmox_user_exists(userid);
    let generated_password = if user_exists {
        None
    } else if let Some(pw) = password {
        Some(pw.to_string())
    } else {
        Some(generate_password(24))
    };

    println!("[silo] userid: {}", userid);
    println!("[silo] pool: {}", pool);
    println!("[silo] role: {}", role);
    println!("[silo] vmids: {:?}", vmids);
    if allow_move { println!("[silo] allow-move: true"); }

    // Pool 생성/확인
    ensure_pool(pool, comment_value)?;
    // VM 할당
    assign_pool_members(pool, &vmids, allow_move)?;
    // 사용자 생성/확인
    ensure_proxmox_user(userid, comment_value, generated_password.as_deref())?;
    // ACL 적용
    let acl_path = format!("/pool/{}", pool);
    common::run("pveum", &["acl", "modify", &acl_path, "--users", userid, "--roles", role])?;
    println!("[silo] ACL 적용 완료: {} -> {} ({})", userid, acl_path, role);

    println!("\n=== Proxmox Silo 완료 ===");
    if let Some(pw) = generated_password {
        println!("[silo] 생성된 초기 비밀번호: {}", pw);
        println!("[silo] 로그인 ID: {}", userid);
    } else if user_exists {
        println!("[silo] 기존 사용자 비밀번호는 변경하지 않았습니다.");
    }
    Ok(())
}

fn ensure_pool(pool: &str, comment: &str) -> anyhow::Result<()> {
    let exists = common::run("pvesh", &["get", "/pools", "--poolid", pool]).is_ok();
    if exists {
        println!("[silo] Pool '{}' 이미 존재", pool);
        if !comment.is_empty() {
            let _ = common::run("pvesh", &["set", "/pools", "--poolid", pool, "--comment", comment]);
        }
        return Ok(());
    }
    let mut args = vec!["create", "/pools", "--poolid", pool];
    if !comment.is_empty() { args.extend(["--comment", comment]); }
    common::run("pvesh", &args)?;
    println!("[silo] Pool '{}' 생성 완료", pool);
    Ok(())
}

fn assign_pool_members(pool: &str, vmids: &[u32], allow_move: bool) -> anyhow::Result<()> {
    let existing = current_pool_vmids(pool);
    let to_add: Vec<u32> = vmids.iter().copied().filter(|v| !existing.contains(v)).collect();
    if to_add.is_empty() {
        println!("[silo] Pool '{}' 에 이미 대상 VM이 모두 포함되어 있습니다.", pool);
        return Ok(());
    }
    let vmid_list = to_add.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
    let mut args = vec!["set", "/pools", "--poolid", pool, "--vms", &vmid_list];
    if allow_move { args.extend(["--allow-move", "1"]); }
    common::run("pvesh", &args)?;
    println!("[silo] Pool '{}' 멤버 반영 완료: {}", pool, vmid_list);
    Ok(())
}

fn current_pool_vmids(pool: &str) -> Vec<u32> {
    let out = match common::run("pvesh", &["get", &format!("/pools/{pool}"), "--output-format", "json"]) {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&out) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    parsed.get("members").and_then(|v| v.as_array()).into_iter().flatten()
        .filter_map(|item| item.get("vmid").and_then(|v| v.as_u64()))
        .map(|v| v as u32).collect()
}

fn proxmox_user_exists(userid: &str) -> bool {
    common::run("pveum", &["user", "list"]).map(|o| o.lines().any(|l| l.contains(userid))).unwrap_or(false)
}

fn ensure_proxmox_user(userid: &str, comment: &str, password: Option<&str>) -> anyhow::Result<()> {
    if !proxmox_user_exists(userid) {
        let mut args = vec!["user", "add", userid];
        if !comment.is_empty() { args.extend(["--comment", comment]); }
        if let Some(pw) = password { args.extend(["--password", pw]); }
        common::run("pveum", &args)?;
        println!("[silo] 사용자 '{}' 생성 완료", userid);
    } else {
        if !comment.is_empty() {
            let _ = common::run("pveum", &["user", "modify", userid, "--comment", comment]);
        }
        if let Some(pw) = password {
            common::run("pveum", &["passwd", userid, "--password", pw])?;
        }
        println!("[silo] 사용자 '{}' 이미 존재", userid);
    }
    Ok(())
}

#[derive(serde::Deserialize, Clone, Debug)]
struct ClusterVmResource {
    vmid: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    tags: String,
}

fn cluster_vm_resources() -> anyhow::Result<Vec<ClusterVmResource>> {
    let out = common::run("pvesh", &["get", "/cluster/resources", "--type", "vm", "--output-format", "json"])?;
    let resources: Vec<ClusterVmResource> = serde_json::from_str(&out)?;
    Ok(resources)
}

fn resolve_target_vmids(
    resources: &[ClusterVmResource],
    vmids_csv: Option<&str>,
    name_prefix: Option<&str>,
    tags_csv: Option<&str>,
) -> anyhow::Result<Vec<u32>> {
    let mut selected = Vec::new();

    if let Some(raw) = vmids_csv {
        for chunk in raw.split(',') {
            let trimmed = chunk.trim();
            if trimmed.is_empty() { continue; }
            let vmid: u32 = trimmed.parse().map_err(|_| anyhow::anyhow!("잘못된 VMID: {}", trimmed))?;
            selected.push(vmid);
        }
    }

    if let Some(prefix) = name_prefix {
        let prefix = prefix.trim();
        if !prefix.is_empty() {
            selected.extend(resources.iter().filter(|r| r.name.starts_with(prefix)).map(|r| r.vmid));
        }
    }

    if let Some(tags_raw) = tags_csv {
        let tag_filters: Vec<String> = tags_raw.split([',', ';']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        if !tag_filters.is_empty() {
            selected.extend(resources.iter().filter(|r| {
                let rtags: Vec<&str> = r.tags.split(';').map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
                tag_filters.iter().all(|wanted| rtags.iter().any(|t| *t == wanted))
            }).map(|r| r.vmid));
        }
    }

    selected.sort_unstable();
    selected.dedup();

    // 존재 확인
    let existing_vmids: Vec<u32> = resources.iter().map(|r| r.vmid).collect();
    for vmid in &selected {
        if !existing_vmids.contains(vmid) {
            anyhow::bail!("VMID {} 를 찾을 수 없습니다.", vmid);
        }
    }

    Ok(selected)
}

fn generate_password(len: usize) -> String {
    let alphabet = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789!@#$%^&*";
    let mut file = fs::File::open("/dev/urandom").expect("/dev/urandom 열기 실패");
    let mut bytes = vec![0u8; len];
    file.read_exact(&mut bytes).expect("랜덤 바이트 읽기 실패");
    bytes.into_iter().map(|b| alphabet[(b as usize) % alphabet.len()] as char).collect()
}

// ---------------------------------------------------------------------------
// roles config
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, Clone, Debug)]
struct RoleConfig {
    role: String,
    domains: Vec<String>,
    #[serde(default)]
    vmid_range: Vec<u32>,
    #[serde(default)]
    pool: String,
    #[serde(default)]
    proxmox_role: String,
    #[serde(default)]
    description: String,
}

fn load_roles() -> anyhow::Result<std::collections::HashMap<String, RoleConfig>> {
    let config_dir = pxi_core::paths::config_dir()?;
    let roles_path = config_dir.join("roles.toml");

    // Fallback paths
    let paths = [roles_path.clone(), std::path::PathBuf::from("/etc/pxi/roles.toml"), std::path::PathBuf::from("/etc/proxmox-host-setup/roles.toml")];
    for p in &paths {
        if p.exists() {
            let content = fs::read_to_string(p)?;
            let roles: std::collections::HashMap<String, RoleConfig> = toml::from_str(&content)?;
            return Ok(roles);
        }
    }
    Ok(std::collections::HashMap::new())
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() || name.len() > 32 {
        anyhow::bail!("사용자명 길이 1~32자 필요");
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        anyhow::bail!("사용자명은 영숫자 + -_ 만 허용");
    }
    if let Some(first) = name.chars().next() {
        if !first.is_ascii_lowercase() && first != '_' {
            anyhow::bail!("사용자명은 소문자 또는 _로 시작해야 (POSIX)");
        }
    }
    Ok(())
}

fn find_visudo() -> String {
    for path in ["/usr/sbin/visudo", "/sbin/visudo", "/usr/bin/visudo"] {
        if Path::new(path).exists() { return path.to_string(); }
    }
    if let Ok(out) = common::run("which", &["visudo"]) {
        return out.trim().to_string();
    }
    "visudo".to_string()
}

fn doctor() {
    println!("=== pxi-account doctor ===");
    for (name, cmd) in &[
        ("useradd", "useradd"),
        ("userdel", "userdel"),
        ("visudo", "visudo"),
        ("getent", "getent"),
        ("pvesh (Proxmox)", "pvesh"),
        ("pveum (Proxmox)", "pveum"),
    ] {
        println!("  {} {name}", if common::has_cmd(cmd) { "+" } else { "-" });
    }
}
