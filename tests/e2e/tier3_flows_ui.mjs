import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import fs from "node:fs";
import { createRequire } from "node:module";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import {
  BASE_URL,
  RESULTS_DIR,
  ROOT_DIR,
  SuiteRunner,
  detectMode,
  httpReq,
} from "./lib/common.mjs";

const require = createRequire(import.meta.url);
const { chromium } = (() => {
  try {
    return require("playwright");
  } catch {
    return require(path.join(os.homedir(), "node_modules", "playwright"));
  }
})();

function resolveChromeBot() {
  const candidates = [
    process.env.CHROME_PATH,
    path.join(os.homedir(), ".local", "bin", "chrome-bot"),
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
  ].filter(Boolean);
  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) return candidate;
  }
  return null;
}

const CHROME_BOT = resolveChromeBot();
const LAUNCH_OPTS = CHROME_BOT ? { executablePath: CHROME_BOT } : {};

const VIEWS = [
  { id: "bandeja", label: "Bandeja de prioridades" },
  { id: "consola", label: "Consola de procesos" },
  { id: "red", label: "Red y topología" },
  { id: "tablero", label: "Tablero de metas" },
  { id: "slices", label: "Slices (Agencia Real)" },
  { id: "lupa", label: "Lupa binaria (RE)" },
];

function createRunner(tierName, mode) {
  try {
    return new SuiteRunner(tierName, mode);
  } catch (err) {
    if (err instanceof TypeError && /not a constructor/i.test(err.message)) {
      return SuiteRunner(tierName, mode);
    }
    throw err;
  }
}

function getFreePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.unref();
    srv.on("error", reject);
    srv.listen(0, "127.0.0.1", () => {
      const addr = srv.address();
      const port = typeof addr === "object" && addr ? addr.port : 0;
      srv.close((closeErr) => (closeErr ? reject(closeErr) : resolve(port)));
    });
  });
}

function collectItemIds(state) {
  const ids = new Map();
  const walkCard = (card) => {
    if (!card || typeof card !== "object") return;
    if (card.id) ids.set(card.id, card);
    if (Array.isArray(card.children)) {
      for (const c of card.children) walkCard(c);
    }
  };
  for (const r of state?.repos || []) {
    for (const w of r?.windows || []) {
      walkCard(w);
    }
  }
  return ids;
}

async function waitForHttpOk(baseUrl, urlPath, timeoutMs = 10000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const res = await httpReq("GET", urlPath, { baseUrl, timeoutMs: 1500 });
      if (res.status === 200 && res.json) return res.json;
    } catch {
      // retry until ready
    }
    await new Promise((r) => setTimeout(r, 150));
  }
  throw new Error(`Timed out waiting for ${baseUrl}${urlPath}`);
}

async function stopChild(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  child.kill("SIGTERM");
  await new Promise((resolve) => {
    const timer = setTimeout(() => {
      if (child.exitCode === null && child.signalCode === null) {
        child.kill("SIGKILL");
      }
      resolve();
    }, 2500);
    child.once("exit", () => {
      clearTimeout(timer);
      resolve();
    });
  });
}

function isHiddenDotFileApiRequest(rawUrl) {
  try {
    const u = new URL(rawUrl);
    if (u.pathname !== "/api/file") return false;
    const p = u.searchParams.get("p") || "";
    return p.split(/[\\/]/).some((seg) => seg.startsWith(".") && seg !== "." && seg !== "..");
  } catch {
    return false;
  }
}

function isBusApiRequest(rawUrl) {
  try {
    return new URL(rawUrl).pathname === "/api/bus";
  } catch {
    return false;
  }
}

function attachTrackers(page, mode) {
  const consoleErrors = [];
  const pageErrors = [];
  const httpErrors = [];
  const ignoredFile404s = [];
  let pendingExpected404Console = 0;

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      const text = msg.text();
      if (text === "Failed to load resource: the server responded with a status of 404 (Not Found)") {
        const locUrl = msg.location()?.url || "";
        if (isHiddenDotFileApiRequest(locUrl) || (mode === "pre" && isBusApiRequest(locUrl))) {
          if (pendingExpected404Console > 0) pendingExpected404Console -= 1;
          return;
        }
        if (!locUrl && pendingExpected404Console > 0) {
          pendingExpected404Console -= 1;
          return;
        }
      }
      consoleErrors.push(text);
    }
  });
  page.on("pageerror", (err) => {
    pageErrors.push(err?.message || String(err));
  });
  page.on("response", (resp) => {
    const status = resp.status();
    if (status >= 400) {
      const url = resp.url();
      if (status === 404 && (isHiddenDotFileApiRequest(url) || (mode === "pre" && isBusApiRequest(url)))) {
        ignoredFile404s.push(url);
        pendingExpected404Console += 1;
        return;
      }
      httpErrors.push({ url, status });
    }
  });

  return { consoleErrors, pageErrors, httpErrors, ignoredFile404s };
}

async function waitForViewLandmark(page, viewId) {
  if (viewId === "bandeja") {
    await page
      .locator("h1")
      .filter({
        hasText: /Todo despejado|decisi(ón|ones) te esperan?|agent(e|es) (falló|fallaron|sin señal)|hilos? del bus requieren? atención/,
      })
      .waitFor({ timeout: 5000 });
  } else if (viewId === "consola") {
    await page.locator('div[role="group"][aria-label="Filtrar procesos"]').waitFor({ timeout: 5000 });
  } else if (viewId === "red") {
    await page.locator(".react-flow").waitFor({ timeout: 5000 });
  } else if (viewId === "tablero") {
    await page.locator("text=/Verificación de metas|Sin tablero de metas en este repositorio/").first().waitFor({ timeout: 5000 });
  } else if (viewId === "slices") {
    await page.locator("text=/Agencia Real está apagada|Comparación de slices|Tiempo usado/").first().waitFor({ timeout: 5000 });
  } else if (viewId === "lupa") {
    await page.locator("text=/Actividad binaria \\(30m\\)|Funciones indexadas/").first().waitFor({ timeout: 5000 });
  }
}

async function assertViewInvariants(page, contextLabel, { isMobile }) {
  await page.waitForTimeout(300);
  const report = await page.evaluate(({ mobile }) => {
    const issues = [];
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    if (document.documentElement.scrollWidth > vw + 1) {
      issues.push(`documentElement horizontal overflow: scrollWidth=${document.documentElement.scrollWidth} > innerWidth=${vw}`);
    }
    if (document.body.scrollWidth > vw + 1) {
      issues.push(`body horizontal overflow: scrollWidth=${document.body.scrollWidth} > innerWidth=${vw}`);
    }

    const imgs = Array.from(document.querySelectorAll("img"));
    for (const img of imgs) {
      const r = img.getBoundingClientRect();
      const cs = getComputedStyle(img);
      if (r.width > 0 && r.height > 0 && cs.visibility !== "hidden" && cs.display !== "none") {
        if (!img.complete || img.naturalWidth <= 0) {
          issues.push(`broken image: src=${img.getAttribute("src")} complete=${img.complete} naturalWidth=${img.naturalWidth}`);
        }
      }
    }

    const isVisible = (el) => {
      const cs = getComputedStyle(el);
      if (cs.display === "none" || cs.visibility === "hidden" || parseFloat(cs.opacity) === 0) return false;
      const r = el.getBoundingClientRect();
      return r.width > 1 && r.height > 1;
    };

    const inScrollableClip = (el) => {
      let cur = el.parentElement;
      while (cur && cur !== document.body) {
        const cs = getComputedStyle(cur);
        if ((cs.overflowX === "auto" || cs.overflowX === "scroll" || cs.overflowY === "auto" || cs.overflowY === "scroll" || cs.overflow === "hidden") && cur.scrollWidth > cur.clientWidth + 2) {
          return true;
        }
        cur = cur.parentElement;
      }
      return false;
    };

    // Check clipped visible buttons / tabs
    const buttonsAndTabs = Array.from(document.querySelectorAll('button, [role="tab"]')).filter((el) => {
      if (!isVisible(el)) return false;
      if (el.closest(".react-flow__nodes") || el.closest(".react-flow__edges")) return false;
      return true;
    });

    for (const el of buttonsAndTabs) {
      const cs = getComputedStyle(el);
      const hasEllipsis = cs.textOverflow === "ellipsis" || Boolean(el.querySelector(".truncate"));
      const hasFloatingBadge = Boolean(el.querySelector(".absolute"));
      if (!hasEllipsis && !hasFloatingBadge && el.scrollWidth > el.clientWidth + 2) {
        issues.push(`clipped control "${(el.getAttribute("aria-label") || el.textContent || "").trim().slice(0, 40)}": scrollWidth=${el.scrollWidth} > clientWidth=${el.clientWidth}`);
      }
      const r = el.getBoundingClientRect();
      if (!inScrollableClip(el)) {
        if (r.left < -2 || r.right > vw + 2) {
          issues.push(`out-of-bounds control horizontally "${(el.getAttribute("aria-label") || el.textContent || "").trim().slice(0, 40)}": left=${Math.round(r.left)} right=${Math.round(r.right)} vw=${vw}`);
        }
        if (r.top < -2 || r.bottom > vh + 2) {
          // Allow scrolled content inside vertical overflow containers; only check fixed/header/rail chrome
          if (el.closest('aside[aria-label="Navegación principal"], header')) {
            issues.push(`out-of-bounds chrome control vertically "${(el.getAttribute("aria-label") || el.textContent || "").trim().slice(0, 40)}": top=${Math.round(r.top)} bottom=${Math.round(r.bottom)} vh=${vh}`);
          }
        }
      }
    }

    // Check sibling overlap among interactive controls inside the same parent
    const parentGroups = new Map();
    const interactive = Array.from(
      document.querySelectorAll('button, a[href], input, select, textarea, [role="tab"], [role="row"]'),
    ).filter((el) => {
      if (!isVisible(el)) return false;
      if (el.closest(".react-flow__nodes") || el.closest(".react-flow__edges")) return false;
      return true;
    });

    for (const el of interactive) {
      const p = el.parentElement;
      if (!p) continue;
      if (!parentGroups.has(p)) parentGroups.set(p, []);
      parentGroups.get(p).push(el);
    }

    for (const [, group] of parentGroups) {
      if (group.length < 2) continue;
      const rects = group.map((el) => ({ el, r: el.getBoundingClientRect() }));
      for (let i = 0; i < rects.length; i++) {
        for (let j = i + 1; j < rects.length; j++) {
          const a = rects[i];
          const b = rects[j];
          if (a.el.contains(b.el) || b.el.contains(a.el)) continue;
          const ix = Math.max(0, Math.min(a.r.right, b.r.right) - Math.max(a.r.left, b.r.left));
          const iy = Math.max(0, Math.min(a.r.bottom, b.r.bottom) - Math.max(a.r.top, b.r.top));
          if (ix * iy > 2) {
            const nameA = (a.el.getAttribute("aria-label") || a.el.textContent || a.el.tagName).trim().slice(0, 30);
            const nameB = (b.el.getAttribute("aria-label") || b.el.textContent || b.el.tagName).trim().slice(0, 30);
            issues.push(`overlapping sibling controls (${mobile ? "mobile" : "desktop"}): "${nameA}" & "${nameB}" area=${Math.round(ix * iy)}`);
          }
        }
      }
    }

    return { issues, vw, vh };
  }, { mobile: isMobile });

  assert.deepEqual(report.issues, [], `[${contextLabel}] Layout/DOM invariant violations:\n${report.issues.join("\n")}`);
}

async function installDisconnectObserver(page) {
  await page.addInitScript(() => {
    window.__sawDisconnectBanner = false;
    const check = () => {
      try {
        const text = document.body?.innerText || "";
        if (text.includes("Sin conexión con el servidor core")) {
          window.__sawDisconnectBanner = true;
        }
      } catch {
        // ignore transient DOM access errors
      }
    };
    const attach = () => {
      check();
      const target = document.documentElement || document;
      const obs = new MutationObserver(() => check());
      obs.observe(target, { childList: true, subtree: true, characterData: true });
      window.__disconnectObserver = obs;
    };
    if (document.documentElement) {
      attach();
    } else {
      document.addEventListener("DOMContentLoaded", attach, { once: true });
    }
  });
}

async function openAppAndResolveDemoFallback(page, mode) {
  await installDisconnectObserver(page);
  const t0 = Date.now();
  await page.goto(BASE_URL, { waitUntil: "domcontentloaded", timeout: 15000 });

  const rail = page.locator('aside[aria-label="Navegación principal"]');
  const emptyCard = page.locator("text=No hay ventanas de agentes activas ahora");
  const disconnectCard = page.locator("text=Sin conexión con el servidor core (:8800)");

  await Promise.race([
    rail.waitFor({ state: "visible", timeout: 10000 }),
    emptyCard.waitFor({ state: "visible", timeout: 10000 }),
  ]);
  const timeToShellMs = Date.now() - t0;

  if (mode === "post") {
    const sawDisconnectBanner = await page.evaluate(() => window.__sawDisconnectBanner === true);
    assert.equal(
      sawDisconnectBanner,
      false,
      'Post-rewrite hydration must not show "Sin conexión con el servidor core" (even transiently)',
    );
    const disconnectVisible = await disconnectCard.isVisible().catch(() => false);
    assert.equal(disconnectVisible, false, 'Post-rewrite hydration must not show "Sin conexión con el servidor core (:8800)"');
  }

  let usedDemoFallback = false;
  if (await emptyCard.isVisible().catch(() => false)) {
    usedDemoFallback = true;
    console.log("[tier3] Live server has 0 active windows; activating Modo Demo to exercise all 6 views structurally.");
    await page.getByRole("button", { name: "Cargar Modo Demo" }).click();
    await rail.waitFor({ state: "visible", timeout: 10000 });
  }

  return { timeToShellMs, usedDemoFallback };
}

async function main() {
  const mode = await detectMode(BASE_URL);
  const runner = createRunner("tier3_flows_ui", mode);

  await runner.test("isolated_job_lifecycle_or_pre_non_destructive_flow", async () => {
    const binPath = process.env.AGENCIA_BIN || (mode === "post" ? path.join(ROOT_DIR, "target/release/agencia") : "");
    if (binPath) {
      assert.ok(fs.existsSync(binPath), `AGENCIA_BIN does not exist: ${binPath}`);
      const port = await getFreePort();
      const isoBase = `http://127.0.0.1:${port}`;
      fs.mkdirSync(RESULTS_DIR, { recursive: true });
      const tmpRoot = fs.mkdtempSync(path.join(RESULTS_DIR, "tmp-tier3-"));
      const tmpBin = path.join(tmpRoot, "bin");
      const tmpDir = path.join(tmpRoot, "agencia_dir");
      let child = null;

      try {
        fs.mkdirSync(tmpBin, { recursive: true });
        fs.mkdirSync(path.join(tmpDir, "jobs"), { recursive: true });
        fs.mkdirSync(path.join(tmpDir, "cache"), { recursive: true });
        const webDir = path.join(ROOT_DIR, "web");
        if (fs.existsSync(webDir)) {
          fs.symlinkSync(webDir, path.join(tmpDir, "web"));
        }

        for (const cmdName of ["bec", "opencode", "codex", "claude"]) {
          const p = path.join(tmpBin, cmdName);
          fs.writeFileSync(p, "#!/bin/sh\nsleep 30\n", { mode: 0o755 });
        }

        child = spawn(binPath, [], {
          cwd: ROOT_DIR,
          env: {
            ...process.env,
            AGENCIA_DIR: tmpDir,
            AGENCIA_ROOT: tmpDir,
            AGENCIA_PORT: String(port),
            AGENCIA_WEB_DIR: webDir,
            PATH: `${tmpBin}:${process.env.PATH || "/usr/bin:/bin"}`,
          },
          stdio: ["ignore", "pipe", "pipe"],
        });

        await waitForHttpOk(isoBase, "/api/state", 12000);

        const launchRes = await httpReq("POST", "/api/launch", {
          baseUrl: isoBase,
          body: {
            kind: "bec",
            tui: false,
            home: "",
            repo: ROOT_DIR,
            text: "E2E isolated smoke job\nEN UNA LÍNEA: E2E isolated smoke job",
          },
          timeoutMs: 5000,
        });
        assert.equal(launchRes.status, 200, `POST /api/launch failed: ${launchRes.text}`);
        assert.equal(launchRes.json?.ok, true, `POST /api/launch ok!==true: ${launchRes.text}`);
        const jobId = String(launchRes.json?.id || "");
        assert.ok(jobId.length > 0, `Missing job id from POST /api/launch: ${launchRes.text}`);

        const cardId = launchRes.json?.card || `job:${jobId}`;
        let found = null;
        const findDeadline = Date.now() + 6000;
        while (Date.now() < findDeadline) {
          const stateRes = await httpReq("GET", "/api/state", { baseUrl: isoBase, timeoutMs: 3000 });
          assert.equal(stateRes.status, 200);
          const map = collectItemIds(stateRes.json);
          found = map.get(cardId) || map.get(jobId) || map.get(`job:${jobId}`) || null;
          if (found) break;
          await new Promise((r) => setTimeout(r, 200));
        }
        assert.ok(found, `Enqueued job ${cardId} did not appear in GET /api/state`);

        const stopRes = await httpReq("POST", "/api/stop", {
          baseUrl: isoBase,
          body: { id: jobId },
          timeoutMs: 5000,
        });
        assert.equal(stopRes.status, 200, `POST /api/stop failed: ${stopRes.text}`);
        assert.equal(stopRes.json?.ok, true, `POST /api/stop ok!==true: ${stopRes.text}`);

        // Verify job is stopped (either absent/done/failed/live===false)
        let stoppedOk = false;
        const stopDeadline = Date.now() + 5000;
        while (Date.now() < stopDeadline) {
          const stateRes = await httpReq("GET", "/api/state", { baseUrl: isoBase, timeoutMs: 3000 });
          const map = collectItemIds(stateRes.json);
          const item = map.get(cardId) || map.get(jobId) || map.get(`job:${jobId}`) || null;
          if (!item || (item.live !== true && item.status !== "working" && item.status !== "queued")) {
            stoppedOk = true;
            break;
          }
          await new Promise((r) => setTimeout(r, 200));
        }
        assert.equal(stoppedOk, true, `Job ${cardId} remained working/queued after POST /api/stop`);
      } finally {
        await stopChild(child);
        fs.rmSync(tmpRoot, { recursive: true, force: true });
      }
    } else {
      // Non-destructive pre-mode API flow against live server
      const pulse = await httpReq("POST", "/api/pulse", { body: {}, timeoutMs: 5000 });
      assert.equal(pulse.status, 200, `POST /api/pulse status=${pulse.status}`);
      assert.equal(pulse.json?.ok, true);

      const consultor = await httpReq("GET", "/api/consultor", { timeoutMs: 5000 });
      assert.equal(consultor.status, 200, `GET /api/consultor status=${consultor.status}`);
      assert.equal(consultor.json?.ok, true);
      assert.ok(Array.isArray(consultor.json?.msgs), "GET /api/consultor msgs should be array");

      const negVote = await httpReq("POST", "/api/avance/voto", {
        body: {
          repo: "__e2e_nonexistent_repo__",
          key: "__e2e_nonexistent_key__",
          agree: true,
          note: "",
        },
        timeoutMs: 5000,
      });
      assert.ok(negVote.status >= 400 && negVote.status < 500, `Expected 4xx on nonexistent avance voto, got ${negVote.status}`);
      assert.equal(negVote.json?.ok, false);
    }
  });

  await runner.test("desktop_1440x900_views_inspector_overlays_theme", async () => {
    const browser = await chromium.launch({
      ...LAUNCH_OPTS,
      headless: true,
      args: ["--no-sandbox"],
    });
    try {
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
      const page = await context.newPage();
      const trackers = attachTrackers(page, mode);

      const { timeToShellMs } = await openAppAndResolveDemoFallback(page, mode);
      console.log(`[tier3] desktop timeToShellMs=${timeToShellMs}`);
      if (mode === "post") {
        assert.ok(timeToShellMs < 2000, `Expected fast shell hydration <2000ms in post mode, got ${timeToShellMs}ms`);
      }

      const rail = page.locator('aside[aria-label="Navegación principal"]');

      // 1. Navigate all 6 views and verify layout invariants
      for (const v of VIEWS) {
        const btn = rail.locator(`button[aria-label="${v.label}"]`);
        await btn.click();
        await assert.doesNotReject(() => btn.waitFor({ state: "visible" }));
        const pressed = await btn.getAttribute("aria-pressed");
        assert.equal(pressed, "true", `NavRail button for ${v.id} should have aria-pressed="true"`);
        await waitForViewLandmark(page, v.id);
        await assertViewInvariants(page, `desktop:${v.id}`, { isMobile: false });
      }

      // 2. AgentInspector flow in Consola
      await rail.locator('button[aria-label="Consola de procesos"]').click();
      await waitForViewLandmark(page, "consola");
      const closeInspBtn = page.locator('button[aria-label="Cerrar inspector"]');
      if (!(await closeInspBtn.isVisible().catch(() => false))) {
        const firstRow = page.locator('div[role="table"] div[role="row"]').first();
        if ((await firstRow.count()) > 0) {
          await firstRow.click();
          await closeInspBtn.waitFor({ state: "visible", timeout: 4000 });
        }
      }
      if (await closeInspBtn.isVisible().catch(() => false)) {
        const tablist = page.locator('div[role="tablist"][aria-label="Vista del agente"]');
        await tablist.waitFor({ state: "visible", timeout: 4000 });
        const terminalTab = tablist.locator('button[role="tab"]', { hasText: /Terminal/ });
        const informeTab = tablist.locator('button[role="tab"]', { hasText: /Informe/ });
        await terminalTab.click();
        assert.equal(await terminalTab.getAttribute("aria-selected"), "true");
        await informeTab.click();
        assert.equal(await informeTab.getAttribute("aria-selected"), "true");
        await closeInspBtn.click();
        await closeInspBtn.waitFor({ state: "hidden", timeout: 4000 });
      }

      // 3. LaunchDialog
      await rail.locator('button[aria-label="Lanzar agente"]').click();
      const pedidoInput = page.locator('textarea[aria-label="Pedido"]');
      await pedidoInput.waitFor({ state: "visible", timeout: 4000 });
      await page.locator('select[aria-label="Repositorio destino"]').waitFor({ state: "visible", timeout: 4000 });
      if (mode === "post") {
        const dialogText = await page.locator('[role="dialog"]').innerText();
        assert.equal(/codex/i.test(dialogText), true, "LaunchDialog must expose Codex runtime option in post mode");
        assert.equal(/claude/i.test(dialogText), true, "LaunchDialog must expose Claude (cla) runtime option in post mode");
      }
      await page.locator('button[aria-label="Cerrar"]').click();
      await pedidoInput.waitFor({ state: "hidden", timeout: 4000 });

      // 4. ActivityDrawer
      await rail.locator('button[aria-label="Actividad de tokens (24h)"]').click();
      const activityHeading = page.getByRole("heading", { name: "Actividad de tokens (24h)" });
      await activityHeading.waitFor({ state: "visible", timeout: 5000 });
      await page.getByRole("heading", { name: "Por modelo" }).waitFor({ state: "visible", timeout: 8000 });
      await page.locator('button[aria-label="Cerrar actividad"]').click();
      await activityHeading.waitFor({ state: "hidden", timeout: 4000 });

      // 5. ConsultorDrawer
      await rail.locator('button[aria-label="Consultor independiente"]').click();
      const consultorHeading = page.getByRole("heading", { name: "Consultor independiente" });
      await consultorHeading.waitFor({ state: "visible", timeout: 4000 });
      await page.locator('textarea[aria-label="Pregunta"]').waitFor({ state: "visible", timeout: 4000 });
      await page.locator('button[aria-label="Cerrar consultor"]').click();
      await consultorHeading.waitFor({ state: "hidden", timeout: 4000 });

      // 6. CommandPalette (Control+k -> Escape)
      await page.keyboard.press("Control+k");
      const paletteInput = page.locator('input[placeholder="Saltar a ventana, agente o acción…"]');
      if (!(await paletteInput.isVisible().catch(() => false))) {
        await rail.locator('button[aria-label="Paleta de comandos"]').click();
      }
      await paletteInput.waitFor({ state: "visible", timeout: 4000 });
      await page.keyboard.press("Escape");
      await paletteInput.waitFor({ state: "hidden", timeout: 4000 });

      // 7. Theme Toggle
      const beforeTheme = await page.evaluate(() => ({
        dark: document.documentElement.classList.contains("dark"),
        bg: getComputedStyle(document.body).backgroundColor,
      }));
      await rail.locator('button[aria-label^="Cambiar a tema"]').click();
      await page.waitForTimeout(150);
      const afterTheme = await page.evaluate(() => ({
        dark: document.documentElement.classList.contains("dark"),
        bg: getComputedStyle(document.body).backgroundColor,
      }));
      assert.notEqual(afterTheme.dark, beforeTheme.dark, "Theme toggle must flip html.dark class");
      assert.notEqual(afterTheme.bg, beforeTheme.bg, "Theme toggle must change computed body backgroundColor");
      // Toggle back to restore initial theme
      await rail.locator('button[aria-label^="Cambiar a tema"]').click();

      assert.deepEqual(trackers.consoleErrors, [], `Console errors: ${JSON.stringify(trackers.consoleErrors)}`);
      assert.deepEqual(trackers.pageErrors, [], `Page errors: ${JSON.stringify(trackers.pageErrors)}`);
      assert.deepEqual(trackers.httpErrors, [], `HTTP >= 400 errors: ${JSON.stringify(trackers.httpErrors)}`);
      await context.close();
    } finally {
      await browser.close();
    }
  });

  await runner.test("mobile_390x844_views_and_layout_invariants", async () => {
    const browser = await chromium.launch({
      ...LAUNCH_OPTS,
      headless: true,
      args: ["--no-sandbox"],
    });
    try {
      const context = await browser.newContext({ viewport: { width: 390, height: 844 } });
      const page = await context.newPage();
      const trackers = attachTrackers(page, mode);

      await openAppAndResolveDemoFallback(page, mode);
      const rail = page.locator('aside[aria-label="Navegación principal"]');

      for (const v of VIEWS) {
        const btn = rail.locator(`button[aria-label="${v.label}"]`);
        await btn.click();
        assert.equal(await btn.getAttribute("aria-pressed"), "true");
        await waitForViewLandmark(page, v.id);
        await assertViewInvariants(page, `mobile:${v.id}`, { isMobile: true });
      }

      // Open & close mobile inspector sheet in Consola if rows exist
      await rail.locator('button[aria-label="Consola de procesos"]').click();
      await waitForViewLandmark(page, "consola");
      const firstRow = page.locator('div[role="table"] div[role="row"]').first();
      if ((await firstRow.count()) > 0) {
        await firstRow.click();
        const closeBtn = page.locator('button[aria-label="Cerrar inspector"]');
        await closeBtn.waitFor({ state: "visible", timeout: 4000 });
        await closeBtn.click();
        await closeBtn.waitFor({ state: "hidden", timeout: 4000 });
      }

      assert.deepEqual(trackers.consoleErrors, [], `Mobile console errors: ${JSON.stringify(trackers.consoleErrors)}`);
      assert.deepEqual(trackers.pageErrors, [], `Mobile page errors: ${JSON.stringify(trackers.pageErrors)}`);
      assert.deepEqual(trackers.httpErrors, [], `Mobile HTTP >= 400 errors: ${JSON.stringify(trackers.httpErrors)}`);
      await context.close();
    } finally {
      await browser.close();
    }
  });

  await runner.finish();
}

main().catch((err) => {
  console.error("[tier3_flows_ui] Fatal error:", err);
  process.exit(1);
});
