// install.prelik.com → github raw 리다이렉트
// URL 패턴:
//   install.prelik.com          → /install.sh (GitHub raw)
//   install.prelik.com/debian/X → GitHub raw scripts/debian/X.sh (향후)
//   install.prelik.com/manifest → 도메인 레지스트리 JSON
export default {
  async fetch(req) {
    const url = new URL(req.url);
    const BASE = "https://raw.githubusercontent.com/dalsoop/prelik-init/main";

    if (url.pathname === "/" || url.pathname === "/install.sh") {
      return Response.redirect(`${BASE}/install.sh`, 302);
    }
    if (url.pathname === "/manifest" || url.pathname === "/manifest.json") {
      // nickel export 결과를 정적으로 제공 (추후 자동 생성)
      return Response.redirect(`${BASE}/ncl/domains.ncl`, 302);
    }
    // /debian/foo.sh 같은 경로는 아직 없음 — 안내 페이지
    return new Response(
      `# prelik-init installer\n\n` +
      `# 메인 설치:\n` +
      `curl -fsSL https://install.prelik.com | bash\n\n` +
      `# 도메인 설치 (prelik 설치 후):\n` +
      `prelik install bootstrap\n` +
      `prelik install lxc traefik mail cloudflare connect ai\n\n` +
      `# 소스: https://github.com/dalsoop/prelik-init\n`,
      { headers: { "Content-Type": "text/plain; charset=utf-8" } }
    );
  },
};
