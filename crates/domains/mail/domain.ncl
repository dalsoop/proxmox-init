{
  name = "mail",
  description = "Maddy 발송 + Mailpit 수신 아카이브 + Postfix relay",
  tags = { product = 'mail, layer = 'remote, platform = 'debian },
  requires = ["lxc"],
  provides = [
    "mail install-mailpit",
    "mail install-maddy",
    "mail postfix-relay",
    "mail status"
  ],
}
