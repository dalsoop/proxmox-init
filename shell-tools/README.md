# shell-tools

Shell 기반 pxi 확장 도구. Rust 바이너리(`pxi-*` crates) 와 병행 설치.

## 구성

```
bin/
  pxi-laravel         자체 호스팅 Laravel LXC 배포/관리 (nginx + PHP-FPM + MariaDB + Node + Composer + Traefik)
  pxi-svn             게임 협업용 SVN LXC thin wrapper (deploy recipe + service + cloudflare 조합)
  pxi-seo-monitor     Prelik SEO Monitor (LXC 50183) 앱/Playwright 관리 — pxi-laravel 위에 얹는 앱별 CLI
  kubetest-run        k3s testkube 기반 Playwright 런너 헬퍼 (NFS: /volume1/works/e2e/kubetest)

share/
  pxi-seo-monitor/    pxi-seo-monitor 가 LXC 안으로 푸시하는 템플릿 (testkube 미러 2-폴더 구조)
    docker/           /navigate API 런타임 (Express + playwright, systemd playwright-api)
      Dockerfile
      package.json
      playwright.config.ts
      server.js
    tests/            @playwright/test 스펙 (on-demand, 도메인별 NAS 출력)
      package.json
      playwright.config.ts
      specs/seo-audit.spec.ts
    playwright-api.service   systemd 유닛
```

## 설치

`/usr/local/bin` + `/usr/local/share/` 로 복사:

```bash
install -m 0755 shell-tools/bin/pxi-laravel       /usr/local/bin/
install -m 0755 shell-tools/bin/pxi-svn           /usr/local/bin/
install -m 0755 shell-tools/bin/pxi-seo-monitor   /usr/local/bin/
install -m 0755 shell-tools/bin/kubetest-run      /usr/local/bin/

mkdir -p /usr/local/share/pxi-seo-monitor
cp -r shell-tools/share/pxi-seo-monitor/* /usr/local/share/pxi-seo-monitor/
```

호출은 pxi 디스패처로:

```bash
pxi run laravel install --vmid 50183 --hostname prelik-seo-monitor \
  --domain prelik-seo-monitor.50.internal.kr --php 8.4 ...
pxi run svn install --vmid 50123 --hostname svn --domain svn.50.internal.kr
pxi run svn repo-create --vmid 50123 --name prototype
pxi run svn repo-list --vmid 50123
pxi run svn password --vmid 50123 --user admin
pxi run svn user-list --vmid 50123
pxi run seo-monitor playwright install
pxi run seo-monitor sites import-wp
```

## NAS 레이아웃 (참고)

```
/volume1/works/e2e/
  kubetest/            # k3s testkube 결과
  prelik-seo-monitor/playwright/
    {domain}/runs/     # /navigate 호출별 (video+trace+screenshot+response.json)
    {domain}/test-runs/      # @playwright/test 출력
    {domain}/html-report/    # 최신 HTML 리포트
    {domain}/last-report.json
```

LXC 50183 은 `mp1: 10.0.0.5:/volume1/works/e2e/prelik-seo-monitor/playwright,mp=/mnt/seo-artifacts` 바인드마운트 필요.

## Terraform 통합

구조상 짝꿍: `dalsoop/terraform-proxmox-init/stacks/seo-monitor/` — LXC 프로비저닝 + 시드 동일 파일.
