{
  name = "host",
  description = "호스트 시스템 관리 (status/monitor/ssh-keygen/smb-open)",
  tags = { product = 'init, layer = 'platform, platform = 'debian },
  provides = [
    "host status",
    "host monitor",
    "host ssh-keygen",
    "host smb-open",
  ],
}
