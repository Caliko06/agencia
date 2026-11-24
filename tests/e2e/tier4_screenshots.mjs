import assert from "node:assert/strict";
import fs from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import {
  BASE_URL,
  E2E_DIR,
  RESULTS_DIR,
  detectMode,
  runCmd,
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
const SIPS = "/usr/bin/sips";
const SHOTS_DIR = path.join(E2E_DIR, "screenshots");

const VIEWS = [
  { id: "bandeja", label: "Bandeja de prioridades" },
  { id: "consola", label: "Consola de procesos" },
  { id: "red", label: "Red y topología" },
  { id: "tablero", label: "Tablero de metas" },
  { id: "slices", label: "Slices (Agencia Real)" },
  { id: "lupa", label: "Lupa binaria (RE)" },
];

async function waitForViewReady(page, viewId) {
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
  await page.waitForTimeout(350);
}

async function captureAndResize(page, fileName, records) {
  const outPath = path.join(SHOTS_DIR, fileName);
  await page.screenshot({ path: outPath });
  const resize = await runCmd(SIPS, ["-Z", "1000", outPath], { timeoutMs: 10000 });
  assert.equal(resize.code, 0, `sips -Z 1000 failed for ${fileName}: ${resize.stderr}`);

  const info = await runCmd(SIPS, ["-g", "pixelWidth", "-g", "pixelHeight", outPath], { timeoutMs: 10000 });
  assert.equal(info.code, 0, `sips -g failed for ${fileName}: ${info.stderr}`);

  const wMatch = info.stdout.match(/pixelWidth:\s*(\d+)/);
  const hMatch = info.stdout.match(/pixelHeight:\s*(\d+)/);
  const pixelWidth = wMatch ? Number(wMatch[1]) : 0;
  const pixelHeight = hMatch ? Number(hMatch[1]) : 0;

  assert.ok(pixelWidth > 0 && pixelWidth <= 1000, `${fileName} pixelWidth=${pixelWidth} exceeds 1000`);
  assert.ok(pixelHeight > 0 && pixelHeight <= 1000, `${fileName} pixelHeight=${pixelHeight} exceeds 1000`);
  records.push({ file: fileName, path: outPath, pixelWidth, pixelHeight });
}

async function ensureTheme(page, wantDark) {
  const isDark = await page.evaluate(() => document.documentElement.classList.contains("dark"));
  if (isDark !== wantDark) {
    await page.locator('aside[aria-label="Navegación principal"] button[aria-label^="Cambiar a tema"]').click();
    await page.waitForTimeout(250);
  }
  const nowDark = await page.evaluate(() => document.documentElement.classList.contains("dark"));
  assert.equal(nowDark, wantDark, `Failed to switch theme to ${wantDark ? "dark" : "light"}`);
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

async function openShell(page, mode) {
  await installDisconnectObserver(page);
  await page.goto(BASE_URL, { waitUntil: "domcontentloaded", timeout: 15000 });
  const rail = page.locator('aside[aria-label="Navegación principal"]');
  const emptyCard = page.locator("text=No hay ventanas de agentes activas ahora");
  await Promise.race([
    rail.waitFor({ state: "visible", timeout: 10000 }),
    emptyCard.waitFor({ state: "visible", timeout: 10000 }),
  ]);
  if (mode === "post") {
    const sawDisconnectBanner = await page.evaluate(() => window.__sawDisconnectBanner === true);
    assert.equal(
      sawDisconnectBanner,
      false,
      'Post-rewrite load must not display "Sin conexión con el servidor core" (even transiently)',
    );
    const disconnectVisible = await page.locator("text=Sin conexión con el servidor core (:8800)").isVisible().catch(() => false);
    assert.equal(disconnectVisible, false, 'Post-rewrite load must not display "Sin conexión con el servidor core (:8800)"');
  }
  let usedDemoFallback = false;
  if (await emptyCard.isVisible().catch(() => false)) {
    usedDemoFallback = true;
    if (mode === "post") {
      throw new Error('Post-rewrite live server returned 0 active windows and fell back to "Modo Demo"');
    }
    await page.getByRole("button", { name: "Cargar Modo Demo" }).click();
    await rail.waitFor({ state: "visible", timeout: 10000 });
  }
  return usedDemoFallback;
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

function attachTrackers(page, consoleErrors, pageErrors, httpErrors, mode) {
  let pendingExpected404Console = 0;
  page.on("console", (m) => {
    if (m.type() !== "error") return;
    const text = m.text();
    if (text === "Failed to load resource: the server responded with a status of 404 (Not Found)") {
      const locUrl = m.location()?.url || "";
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
  });
  page.on("pageerror", (e) => pageErrors.push(e?.message || String(e)));
  page.on("response", (r) => {
    if (r.status() >= 400) {
      const url = r.url();
      if (r.status() === 404 && (isHiddenDotFileApiRequest(url) || (mode === "pre" && isBusApiRequest(url)))) {
        pendingExpected404Console += 1;
        return;
      }
      httpErrors.push({ url, status: r.status() });
    }
  });
}

async function main() {
  const t0 = Date.now();
  let mode = (process.env.AGENCIA_MODE || "auto").toLowerCase();
  const consoleErrors = [];
  const pageErrors = [];
  const httpErrors = [];
  const screenshots = [];
  let usedDemoFallback = false;
  let fatalError = null;
  let browser = null;

  try {
    mode = await detectMode(BASE_URL);
    fs.mkdirSync(SHOTS_DIR, { recursive: true });
    fs.mkdirSync(RESULTS_DIR, { recursive: true });

    browser = await chromium.launch({
      ...LAUNCH_OPTS,
      headless: true,
      args: ["--no-sandbox"],
    });

    // Desktop 1440x900 captures (01..15)
    const desktopCtx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    const page = await desktopCtx.newPage();
    attachTrackers(page, consoleErrors, pageErrors, httpErrors, mode);

    usedDemoFallback = await openShell(page, mode);
    const rail = page.locator('aside[aria-label="Navegación principal"]');

    // 01..06 Dark views
    await ensureTheme(page, true);
    for (let i = 0; i < VIEWS.length; i++) {
      const v = VIEWS[i];
      await rail.locator(`button[aria-label="${v.label}"]`).click();
      await waitForViewReady(page, v.id);
      const num = String(i + 1).padStart(2, "0");
      await captureAndResize(page, `${num}_${v.id}_dark.png`, screenshots);
    }

    // 07..12 Light views
    await ensureTheme(page, false);
    for (let i = 0; i < VIEWS.length; i++) {
      const v = VIEWS[i];
      await rail.locator(`button[aria-label="${v.label}"]`).click();
      await waitForViewReady(page, v.id);
      const num = String(i + 7).padStart(2, "0");
      await captureAndResize(page, `${num}_${v.id}_light.png`, screenshots);
    }

    // 13 Inspector dark
    await ensureTheme(page, true);
    await rail.locator('button[aria-label="Consola de procesos"]').click();
    await waitForViewReady(page, "consola");
    const closeInsp = page.locator('button[aria-label="Cerrar inspector"]');
    if (!(await closeInsp.isVisible().catch(() => false))) {
      const firstRow = page.locator('div[role="table"] div[role="row"]').first();
      if ((await firstRow.count()) > 0) {
        await firstRow.click();
        await closeInsp.waitFor({ state: "visible", timeout: 4000 });
      }
    }
    await page.waitForTimeout(300);
    await captureAndResize(page, "13_inspector_dark.png", screenshots);

    // 14 Launch dialog dark
    await rail.locator('button[aria-label="Lanzar agente"]').click();
    await page.locator('textarea[aria-label="Pedido"]').waitFor({ state: "visible", timeout: 4000 });
    await page.waitForTimeout(250);
    await captureAndResize(page, "14_launch_dialog_dark.png", screenshots);
    await page.locator('button[aria-label="Cerrar"]').click();
    await page.locator('textarea[aria-label="Pedido"]').waitFor({ state: "hidden", timeout: 4000 });

    // 15 Activity drawer dark
    await rail.locator('button[aria-label="Actividad de tokens (24h)"]').click();
    await page.getByRole("heading", { name: "Actividad de tokens (24h)" }).waitFor({ state: "visible", timeout: 5000 });
    await page.getByRole("heading", { name: "Por modelo" }).waitFor({ state: "visible", timeout: 8000 });
    await page.waitForTimeout(250);
    await captureAndResize(page, "15_activity_drawer_dark.png", screenshots);
    await page.locator('button[aria-label="Cerrar actividad"]').click();

    await desktopCtx.close();

    // 16 Mobile 390x844 Bandeja dark
    const mobileCtx = await browser.newContext({ viewport: { width: 390, height: 844 } });
    const mPage = await mobileCtx.newPage();
    attachTrackers(mPage, consoleErrors, pageErrors, httpErrors, mode);

    const mobileDemoFallback = await openShell(mPage, mode);
    usedDemoFallback = usedDemoFallback || mobileDemoFallback;
    const mDark = await mPage.evaluate(() => document.documentElement.classList.contains("dark"));
    if (!mDark) {
      await mPage.evaluate(() => document.documentElement.classList.add("dark"));
    }
    await mPage.locator('aside[aria-label="Navegación principal"] button[aria-label="Bandeja de prioridades"]').click();
    await waitForViewReady(mPage, "bandeja");
    await captureAndResize(mPage, "16_mobile_bandeja_dark.png", screenshots);

    await mobileCtx.close();
  } catch (err) {
    fatalError = err?.message || String(err);
    console.error("[tier4_screenshots] Fatal error:", err);
  } finally {
    if (browser) {
      await browser.close().catch(() => {});
    }

    const failedErrors = consoleErrors.length + pageErrors.length + httpErrors.length;
    const allWidthOk = screenshots.every((s) => s.pixelWidth > 0 && s.pixelWidth <= 1000);
    const demoOk = !(mode === "post" && usedDemoFallback);
    const passedCount = screenshots.filter((s) => s.pixelWidth > 0 && s.pixelWidth <= 1000).length;
    const ok = !fatalError && screenshots.length === 16 && allWidthOk && failedErrors === 0 && demoOk;
    const failedCount = ok
      ? 0
      : fatalError
        ? Math.max(1, failedErrors)
        : Math.max(1, failedErrors + (16 - passedCount) + (demoOk ? 0 : 1));

    const summary = {
      tier: "tier4_screenshots",
      mode,
      baseUrl: BASE_URL,
      usedDemoFallback,
      durationMs: Date.now() - t0,
      passed: ok ? 16 : passedCount,
      failed: failedCount,
      skipped: 0,
      ...(fatalError ? { error: fatalError } : {}),
      consoleErrors,
      pageErrors,
      httpErrors,
      screenshots,
    };

    try {
      fs.mkdirSync(RESULTS_DIR, { recursive: true });
      const outJson = path.join(RESULTS_DIR, "tier4_screenshots.json");
      fs.writeFileSync(outJson, JSON.stringify(summary, null, 2) + "\n");
      console.log(
        `[tier4_screenshots] captured=${screenshots.length}/16 mode=${mode} consoleErrors=${consoleErrors.length} pageErrors=${pageErrors.length} httpErrors=${httpErrors.length} -> ${outJson}`,
      );
    } catch (writeErr) {
      console.error("[tier4_screenshots] Failed to write results JSON:", writeErr);
      process.exitCode = 1;
    }

    if (!ok) {
      process.exitCode = 1;
    }
  }
}

main();
