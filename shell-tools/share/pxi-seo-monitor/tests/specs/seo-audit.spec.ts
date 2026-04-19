import { test, expect } from "@playwright/test";

// PLAYWRIGHT_TARGET_URL 로 지정된 단일 타겟에 대한 SEO 감사.
// 등록된 사이트를 감사할 때 pxi-seo-monitor tests run <site_id> 가 env 주입.

const TARGET = process.env.PLAYWRIGHT_TARGET_URL || process.env.TARGET_URL;

test.skip(!TARGET, "PLAYWRIGHT_TARGET_URL not set");

test.describe("SEO audit", () => {
  test("homepage loads @smoke", async ({ page }) => {
    const url = TARGET as string;
    const res = await page.goto(url, { waitUntil: "domcontentloaded" });
    expect(res?.status()).toBeLessThan(400);
    await expect(page).toHaveTitle(/.+/);
  });

  test("meta tags present", async ({ page }) => {
    await page.goto(TARGET as string, { waitUntil: "domcontentloaded" });
    const title = await page.title();
    const description = await page
      .locator('meta[name="description"]')
      .getAttribute("content")
      .catch(() => null);
    expect(title.length).toBeGreaterThan(0);
    test.info().annotations.push({ type: "title", description: title });
    if (description) {
      test.info().annotations.push({ type: "description", description });
    }
  });

  test("h1 exists", async ({ page }) => {
    await page.goto(TARGET as string, { waitUntil: "domcontentloaded" });
    const h1Count = await page.locator("h1").count();
    expect(h1Count).toBeGreaterThanOrEqual(1);
  });

  test("no console errors on initial load", async ({ page }) => {
    const errors: string[] = [];
    page.on("console", (msg) => {
      if (msg.type() === "error") errors.push(msg.text());
    });
    page.on("pageerror", (err) => errors.push(String(err)));
    await page.goto(TARGET as string, { waitUntil: "domcontentloaded" });
    await page.waitForTimeout(2000);
    test.info().annotations.push({
      type: "console-errors",
      description: `count=${errors.length}`,
    });
    // soft: just record, don't fail
  });

  test("mobile viewport renders", async ({ browser }) => {
    const context = await browser.newContext({ viewport: { width: 375, height: 812 } });
    const page = await context.newPage();
    const res = await page.goto(TARGET as string, { waitUntil: "domcontentloaded" });
    expect(res?.status()).toBeLessThan(400);
    const hasHScroll = await page.evaluate(
      () => document.documentElement.scrollWidth > window.innerWidth + 2,
    );
    test.info().annotations.push({
      type: "mobile-horizontal-scroll",
      description: String(hasHScroll),
    });
    await context.close();
  });
});
