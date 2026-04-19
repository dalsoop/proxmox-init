import { defineConfig } from "@playwright/test";

// SEO 모니터 등록 사이트들에 대한 on-demand / 스케줄 e2e 감사.
// 결과는 Synology NAS (/mnt/seo-artifacts) 에 도메인별로 저장.
// 스펙 내부에서 test.info().outputDir 를 override 하거나, PLAYWRIGHT_DOMAIN env 사용.

const DOMAIN = process.env.PLAYWRIGHT_DOMAIN || "_default";
const ARTIFACTS_ROOT = process.env.ARTIFACTS_ROOT || "/mnt/seo-artifacts";

export default defineConfig({
  testDir: "./specs",
  timeout: 60_000,
  retries: 0,
  workers: 1,
  reporter: [
    ["list"],
    ["json", { outputFile: `${ARTIFACTS_ROOT}/${DOMAIN}/last-report.json` }],
    ["html", { outputFolder: `${ARTIFACTS_ROOT}/${DOMAIN}/html-report`, open: "never" }],
  ],
  use: {
    ignoreHTTPSErrors: true,
    video: "on",
    screenshot: "on",
    trace: "on",
    launchOptions: {
      args: ["--no-sandbox", "--disable-dev-shm-usage"],
    },
  },
  outputDir: `${ARTIFACTS_ROOT}/${DOMAIN}/test-runs`,
});
