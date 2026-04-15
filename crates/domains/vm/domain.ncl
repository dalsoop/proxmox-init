{
  name = "vm",
  description = "Proxmox QEMU VM 수명관리 (qm 래퍼)",
  tags = { product = 'infra, layer = 'remote, platform = 'proxmox },
  provides = [
    "vm list",
    "vm status",
    "vm start/stop/reboot",
    "vm delete",
    "vm backup",
    "vm resize",
  ],
}
