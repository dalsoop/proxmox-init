{
  name = "cloudflare",
  description = "CF DNS + Email Routing + Worker attach",
  tags = { product = 'infra, layer = 'tool, platform = 'any },
  provides = ["cf dns-add", "cf dns-list", "cf email-worker-attach-all"],
}
