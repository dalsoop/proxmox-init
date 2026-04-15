{
  name = "account",
  description = "리눅스 계정 관리 (create/remove/list/status/ssh-key-add)",
  tags = { product = 'init, layer = 'platform, platform = 'debian },
  provides = [
    "account create",
    "account remove",
    "account list",
    "account status",
    "account ssh-key-add",
  ],
}
