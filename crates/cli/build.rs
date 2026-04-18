// 빌드 타임에 git tag를 CARGO_PKG_VERSION 대신 PRELIK_GIT_VERSION으로 주입.
// cargo workspace.version은 0.1.0 고정이라 `pxi --version`이 부정확했음.
fn main() {
    let version = std::process::Command::new("git")
        .args(["describe", "--tags", "--dirty=-dev", "--always"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());

    println!("cargo:rustc-env=PRELIK_GIT_VERSION={version}");
    // 재빌드 트리거 — HEAD와 tag 변경 시.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/tags");
}
