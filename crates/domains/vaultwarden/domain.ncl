let { Domain } = import "../../../ncl/contracts/domain.ncl" in

{
  name = "vaultwarden",
  description = "Vaultwarden (self-hosted Bitwarden)",
  tags = { product = 'tool, layer = 'remote, platform = 'proxmox },
  provides = [
    "vaultwarden status",
    "vaultwarden logs",
    "vaultwarden restart",
    "vaultwarden doctor",
    "vaultwarden domain-set",
    "vaultwarden smtp-mailgun",
    "vaultwarden backup",
    "vaultwarden bw-install",
    "vaultwarden install-systemd",
    "vaultwarden install-backup-timer",
    "vaultwarden install-env",
    "vaultwarden install-binary",
    "vaultwarden install-web-vault",
    "vaultwarden bootstrap",
    "vaultwarden upgrade"
  ],
} | Domain
