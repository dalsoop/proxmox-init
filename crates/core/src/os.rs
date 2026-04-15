//! OS/배포판 감지. Linux 전용이지만 Debian/Ubuntu/Alpine/Arch 구분.

use std::fs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distro {
    Debian,
    Ubuntu,
    Alpine,
    Arch,
    Fedora,
    Unknown,
}

impl Distro {
    pub fn detect() -> Self {
        let Ok(content) = fs::read_to_string("/etc/os-release") else {
            return Self::Unknown;
        };
        let id = content
            .lines()
            .find_map(|l| l.strip_prefix("ID="))
            .map(|v| v.trim_matches('"').to_string())
            .unwrap_or_default();
        match id.as_str() {
            "debian" => Self::Debian,
            "ubuntu" => Self::Ubuntu,
            "alpine" => Self::Alpine,
            "arch" => Self::Arch,
            "fedora" => Self::Fedora,
            _ => Self::Unknown,
        }
    }

    pub fn pkg_manager(&self) -> &'static str {
        match self {
            Self::Debian | Self::Ubuntu => "apt-get",
            Self::Alpine => "apk",
            Self::Arch => "pacman",
            Self::Fedora => "dnf",
            Self::Unknown => "apt-get",
        }
    }
}

pub fn is_proxmox() -> bool {
    std::path::Path::new("/usr/sbin/pct").exists()
        || std::path::Path::new("/etc/pve").exists()
}
