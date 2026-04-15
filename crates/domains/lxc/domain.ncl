{
  name = "lxc",
  description = "LXC 수명 관리 (Proxmox pct 래퍼)",
  tags = { product = 'infra, layer = 'remote, platform = 'proxmox },
  provides = ["lxc create", "lxc delete", "lxc list", "lxc enter", "lxc backup"],
}
