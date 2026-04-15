//! 경로 규약
//! - system: /etc/prelik, /var/lib/prelik, /usr/local/bin
//! - user:   ~/.config/prelik, ~/.local/share/prelik, ~/.local/bin

use std::path::PathBuf;

pub fn config_dir() -> PathBuf {
    if is_root() {
        PathBuf::from("/etc/prelik")
    } else {
        dirs::config_dir().unwrap_or_default().join("prelik")
    }
}

pub fn data_dir() -> PathBuf {
    if is_root() {
        PathBuf::from("/var/lib/prelik")
    } else {
        dirs::data_dir().unwrap_or_default().join("prelik")
    }
}

pub fn bin_dir() -> PathBuf {
    if is_root() {
        PathBuf::from("/usr/local/bin")
    } else {
        dirs::home_dir().unwrap_or_default().join(".local/bin")
    }
}

pub fn domains_dir() -> PathBuf {
    data_dir().join("domains")
}

pub fn env_file() -> PathBuf {
    config_dir().join(".env")
}

pub fn env_vault() -> PathBuf {
    config_dir().join(".env.vault")
}

pub fn env_keys() -> PathBuf {
    config_dir().join(".env.keys")
}

pub fn is_root() -> bool {
    unsafe { libc_geteuid() == 0 }
}

unsafe fn libc_geteuid() -> u32 {
    extern "C" {
        fn geteuid() -> u32;
    }
    geteuid()
}
