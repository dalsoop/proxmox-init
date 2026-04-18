{
  name = "xdesktop",
  description = "X11 원격 데스크톱 LXC (Xpra HTML5 + 한글 로케일 + fcitx5-hangul + Helium 브라우저)",
  tags = { product = 'desktop, layer = 'remote, platform = 'proxmox },
  requires = ["lxc"],
  provides = [
    "xdesktop setup",
    "xdesktop install",
    "xdesktop expose",
    "xdesktop status",
    "xdesktop destroy",
    "xdesktop doctor",
  ],
}
