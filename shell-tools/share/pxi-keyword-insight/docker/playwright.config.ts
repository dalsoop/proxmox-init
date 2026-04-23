import { defineConfig } from "@playwright/test";
export default defineConfig({
  use: {
    ignoreHTTPSErrors: true,
    launchOptions: { args: ["--no-sandbox", "--disable-dev-shm-usage"] },
  },
});
