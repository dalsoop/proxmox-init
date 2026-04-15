{
  name = "traefik",
  description = "Traefik 리버스 프록시 + compose 기반 재생성",
  tags = { product = 'infra, layer = 'remote, platform = 'debian },
  requires = ["lxc"],
  provides = ["traefik install", "traefik recreate", "traefik route-add", "traefik route-list"],
}
