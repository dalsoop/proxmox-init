const express = require("express");
const { chromium } = require("playwright");

const PORT = parseInt(process.env.PORT || "3000", 10);
const PROXY_SERVER = process.env.PROXY_SERVER || "";
const PROXY_USERNAME = process.env.PROXY_USERNAME || "";
const PROXY_PASSWORD = process.env.PROXY_PASSWORD || "";

const app = express();
app.use(express.json({ limit: "20mb" }));

let browser;
async function getBrowser() {
  if (!browser || !browser.isConnected()) {
    const launchOptions = {
      headless: true,
      args: ["--no-sandbox", "--disable-dev-shm-usage"],
    };

    if (PROXY_SERVER) {
      launchOptions.proxy = {
        server: PROXY_SERVER,
        username: PROXY_USERNAME || undefined,
        password: PROXY_PASSWORD || undefined,
      };
    }

    browser = await chromium.launch({
      ...launchOptions,
    });
  }
  return browser;
}

app.get("/health", async (_req, res) => {
  try {
    await getBrowser();
    res.json({ ok: true, proxy_server: PROXY_SERVER || null });
  } catch (e) {
    res.status(500).json({ ok: false, error: e.message });
  }
});

app.post("/navigate", async (req, res) => {
  const { url, actions = [], viewport = { width: 1440, height: 1200 } } = req.body || {};
  if (!url) return res.status(400).json({ error: "url required" });

  let context;
  try {
    const b = await getBrowser();
    context = await b.newContext({
      viewport,
      ignoreHTTPSErrors: true,
      userAgent: "Mozilla/5.0 keyword-insight-playwright",
    });
    const page = await context.newPage();
    await page.goto(url, { waitUntil: "domcontentloaded", timeout: 30000 });
    const finalUrl = page.url();
    const pageTitle = await page.title();

    const actionResults = [];
    for (const action of actions) {
      if (action.type === "wait") {
        await page.waitForTimeout(action.ms ?? 1000);
        actionResults.push({ type: "wait", ms: action.ms ?? 1000 });
      } else if (action.type === "extract_serp") {
        const engine = action.engine ?? "naver";
        if (engine !== "naver") {
          actionResults.push({
            type: "extract_serp",
            engine,
            result: [],
            status: "unsupported",
            message: `Unsupported SERP engine: ${engine}`,
          });
          continue;
        }
        const payload = await extractNaverSerp(page);
        actionResults.push({ type: "extract_serp", engine, result: payload.items, status: payload.status, message: payload.message });
      }
    }

    res.json({
      url,
      final_url: finalUrl,
      page_title: pageTitle,
      actions: actionResults,
      artifact_dir: null,
    });
  } catch (e) {
    res.status(500).json({ error: e.message });
  } finally {
    if (context) await context.close().catch(() => {});
  }
});

async function extractNaverSerp(page) {
  return await page.evaluate(() => {
    const seen = new Set();
    const anchors = Array.from(document.querySelectorAll("a[href]"));

    const looksLikeResultAnchor = (anchor) => {
      const cls = typeof anchor.className === "string" ? anchor.className : "";
      const href = anchor.href || "";
      return (
        /(site|lnk_head|lnk_url|link_desc|news_tit|title|compare)/.test(cls) ||
        href.includes("map.naver.com/p/search/") ||
        href.includes("map.naver.com/p/entry/place/")
      );
    };

    const resolveTarget = (anchor) => {
      const href = anchor.href || "";
      if (
        href.startsWith("https://hotels.naver.com/") ||
        href.startsWith("https://m.naver.com/shorts") ||
        href.startsWith("https://m.blog.naver.com/") ||
        href.startsWith("https://clip.naver.com/@")
      ) {
        return href;
      }

      const onclick = anchor.getAttribute("onclick") || "";
      const match = onclick.match(/urlencode\\(\"([^\"]+)\"\\)/);
      if (match && match[1]) {
        return match[1];
      }

      return href;
    };

    const items = anchors
      .filter((anchor) => looksLikeResultAnchor(anchor))
      .map((anchor) => {
        const title = anchor.textContent?.trim() ?? "";
        const url = resolveTarget(anchor);
        const snippet = anchor.closest("div, li, article")?.innerText?.trim() ?? "";
        return { title, url, snippet };
      })
      .filter((row) => row.title.length >= 4 && row.url.startsWith("http"))
      .filter((row) => !row.url.includes("ader.naver.com"))
      .filter((row) => !row.url.includes("adcr.naver.com"))
      .filter((row) => !row.url.includes("searchad.naver.com"))
      .filter((row) => !row.url.includes("search.naver.com/search.naver"))
      .filter((row) => !row.url.includes("nid.naver.com"))
      .filter((row) => !row.url.includes("help.naver.com"))
      .filter((row) => !row.url.includes("www.naver.com"))
      .filter((row) => !row.url.includes("mail.naver.com"))
      .filter((row) => !row.url.includes("kin.naver.com"))
      .filter((row) => row.title !== "NAVER")
      .filter((row) => !row.title.includes("내 페이포인트"))
      .filter((row) => !row.title.includes("내 블로그"))
      .filter((row) => !row.title.includes("가입한 카페"))
      .map((row) => {
        const match = row.url.match(/map\.naver\.com\/p\/(?:search|entry)\/[^?]*\/place\/([0-9]+)/)
          || row.url.match(/map\.naver\.com\/p\/entry\/place\/([0-9]+)/);
        if (match && match[1]) {
          return { ...row, url: `https://map.naver.com/p/entry/place/${match[1]}` };
        }
        return row;
      })
      .filter((row) => {
        if (seen.has(row.url)) {
          return false;
        }
        seen.add(row.url);
        return true;
      })
      .slice(0, 10);

    return {
      items,
      status: items.length === 0 ? "empty" : "ok",
      message: items.length === 0 ? "No extractable naver results found." : null,
    };
  });
}

app.listen(PORT, "127.0.0.1", () => {
  console.log(`keyword-insight playwright listening on 127.0.0.1:${PORT}`);
});
