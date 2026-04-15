{
  name = "backup",
  description = "vzdump 기반 LXC/VM 백업 스케줄 관리",
  tags = { product = 'infra, layer = 'remote, platform = 'proxmox },
  provides = [
    "backup now",
    "backup list",
    "backup schedule-add/remove",
    "backup restore",
  ],
}
