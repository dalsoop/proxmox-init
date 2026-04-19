const express = require("express");
const { chromium } = require("playwright");
const fs = require("fs");
const path = require("path");

const ARTIFACTS_ROOT = process.env.ARTIFACTS_ROOT || "/mnt/seo-artifacts";
const PORT = parseInt(process.env.PORT || "3000", 10);

const app = express();
app.use(express.json({ limit: "20mb" }));

let browser;
async function getBrowser() {
  if (!browser || !browser.isConnected()) {
    browser = await chromium.launch({
      headless: true,
      args: ["--no-sandbox", "--disable-dev-shm-usage"],
    });
  }
  return browser;
}

function domainSafe(urlStr) {
  try { return new URL(urlStr).host.replace(/[^a-z0-9.-]/gi, "_"); }
  catch { return "_unknown"; }
}

function ensureDir(p) { fs.mkdirSync(p, { recursive: true }); return p; }

app.get("/health", async (_req, res) => {
  try {
    await getBrowser();
    let artifactsOK = false;
    try { fs.accessSync(ARTIFACTS_ROOT, fs.constants.W_OK); artifactsOK = true; } catch {}
    res.json({ ok: true, artifacts_writable: artifactsOK, artifacts_root: ARTIFACTS_ROOT });
  } catch (e) { res.status(500).json({ ok: false, error: e.message }); }
});

app.post("/navigate", async (req, res) => {
  const { url, actions = [], viewport = { width: 1920, height: 1080 }, persist = true } = req.body || {};
  if (!url) return res.status(400).json({ error: "url required" });

  const started = Date.now();
  const ts = new Date().toISOString().replace(/[:.]/g, "-");
  const vpTag = `${viewport.width}x${viewport.height}`;
  const domain = domainSafe(url);
  const runDir = persist ? ensureDir(path.join(ARTIFACTS_ROOT, domain, "runs", `${ts}_${vpTag}`)) : null;

  let context, page;
  try {
    const b = await getBrowser();
    const ctxOpts = {
      viewport,
      userAgent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/131.0.0.0 Safari/537.36 prelik-seo-monitor-bot",
      ignoreHTTPSErrors: true,
    };
    if (runDir) {
      ctxOpts.recordVideo = { dir: path.join(runDir, "video"), size: viewport };
    }
    context = await b.newContext(ctxOpts);
    if (runDir) await context.tracing.start({ screenshots: true, snapshots: true });

    page = await context.newPage();
    await page.goto(url, { waitUntil: "domcontentloaded", timeout: 30000 });

    const actionResults = [];
    for (const a of actions) {
      if (a.type === "wait") {
        await page.waitForTimeout(a.ms ?? 1000);
        actionResults.push({ type: "wait", ms: a.ms ?? 1000 });
      } else if (a.type === "evaluate") {
        const result = await page.evaluate(a.script);
        actionResults.push({ type: "evaluate", result });
      } else {
        actionResults.push({ type: a.type, skipped: true });
      }
    }

    let screenshot = null;
    if ((viewport.width ?? 1920) >= 1280) {
      const buf = await page.screenshot({ fullPage: false, type: "png" });
      screenshot = buf.toString("base64");
      if (runDir) fs.writeFileSync(path.join(runDir, "screenshot.png"), buf);
    }

    if (runDir) await context.tracing.stop({ path: path.join(runDir, "trace.zip") });

    const payload = {
      url, viewport, actions: actionResults,
      elapsed_ms: Date.now() - started,
      artifact_dir: runDir,
    };
    if (screenshot) payload.screenshot = screenshot;

    if (runDir) {
      const persistable = { ...payload };
      delete persistable.screenshot;
      fs.writeFileSync(path.join(runDir, "response.json"), JSON.stringify(persistable, null, 2));
    }
    res.json(payload);
  } catch (e) {
    console.error("[navigate] error:", e.message);
    res.status(500).json({ error: e.message, elapsed_ms: Date.now() - started, artifact_dir: runDir });
  } finally {
    if (context) await context.close().catch(() => {});
  }
});

app.listen(PORT, "127.0.0.1", () => {
  console.log(`playwright-api listening on 127.0.0.1:${PORT}`);
  console.log(`artifacts root: ${ARTIFACTS_ROOT}`);
});
