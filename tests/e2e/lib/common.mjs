import http from "node:http";
import https from "node:https";
import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));

export const ROOT_DIR =
  process.env.AGENCIA_ROOT || path.resolve(HERE, "..", "..", "..");
export const E2E_DIR = path.join(ROOT_DIR, "tests", "e2e");
export const RESULTS_DIR = path.join(E2E_DIR, "results");
export const BASE_URL = process.env.AGENCIA_URL || "http://127.0.0.1:8800";

export function detectMode(_baseUrl = BASE_URL) {
  const envMode = (process.env.AGENCIA_MODE || "auto").toLowerCase();
  if (envMode === "pre" || envMode === "post") {
    return envMode;
  }
  const releaseBin = path.join(ROOT_DIR, "target", "release", "agencia");
  const legacyPy = path.join(ROOT_DIR, "agencia.py");
  if (process.env.AGENCIA_BIN && fs.existsSync(process.env.AGENCIA_BIN)) {
    return "post";
  }
  if (fs.existsSync(releaseBin) && !fs.existsSync(legacyPy)) {
    return "post";
  }
  return "pre";
}

export function httpReq(method, reqPath, options = {}) {
  const {
    body,
    headers = {},
    timeoutMs = 15000,
    baseUrl = BASE_URL,
  } = options;

  const targetUrl = new URL(reqPath, baseUrl);
  const transport = targetUrl.protocol === "https:" ? https : http;
  const reqHeaders = { ...headers };

  let payload = null;
  if (body !== undefined) {
    if (typeof body === "string" || Buffer.isBuffer(body)) {
      payload = body;
      if (!Object.keys(reqHeaders).some((k) => k.toLowerCase() === "content-type")) {
        reqHeaders["Content-Type"] = "application/json";
      }
    } else {
      payload = JSON.stringify(body);
      if (!Object.keys(reqHeaders).some((k) => k.toLowerCase() === "content-type")) {
        reqHeaders["Content-Type"] = "application/json";
      }
    }
    if (!Object.keys(reqHeaders).some((k) => k.toLowerCase() === "content-length")) {
      reqHeaders["Content-Length"] = String(Buffer.byteLength(payload));
    }
  }

  return new Promise((resolve, reject) => {
    const start = performance.now();
    let settled = false;

    const req = transport.request(
      targetUrl,
      { method, headers: reqHeaders },
      (res) => {
        const chunks = [];
        res.on("data", (chunk) => chunks.push(chunk));
        res.on("end", () => {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          const ms = Number((performance.now() - start).toFixed(2));
          const text = Buffer.concat(chunks).toString("utf8");
          let json = null;
          try {
            json = JSON.parse(text);
          } catch {
            json = null;
          }
          resolve({
            status: res.statusCode || 0,
            headers: res.headers,
            text,
            json,
            ms,
          });
        });
        res.on("error", (err) => {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          reject(err);
        });
      }
    );

    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      req.destroy(new Error(`HTTP timeout after ${timeoutMs}ms: ${method} ${targetUrl.href}`));
      reject(new Error(`HTTP timeout after ${timeoutMs}ms: ${method} ${targetUrl.href}`));
    }, timeoutMs);

    req.on("error", (err) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      reject(err);
    });

    if (payload !== null) {
      req.write(payload);
    }
    req.end();
  });
}

export function fetchSseFirstEvent(reqPath, timeoutMs = 10000, baseUrl = BASE_URL) {
  const targetUrl = new URL(reqPath, baseUrl);
  const transport = targetUrl.protocol === "https:" ? https : http;

  return new Promise((resolve, reject) => {
    const start = performance.now();
    let settled = false;
    let buffer = "";

    const req = transport.request(
      targetUrl,
      {
        method: "GET",
        headers: {
          Accept: "text/event-stream",
          "Cache-Control": "no-cache",
        },
      },
      (res) => {
        const status = res.statusCode || 0;
        const headers = res.headers;

        if (status !== 200) {
          settled = true;
          clearTimeout(timer);
          res.resume();
          req.destroy();
          reject(new Error(`SSE non-200 status ${status} for ${targetUrl.href}`));
          return;
        }

        res.setEncoding("utf8");
        res.on("data", (chunk) => {
          if (settled) return;
          buffer += chunk.replace(/\r\n/g, "\n");

          let sepIdx;
          while ((sepIdx = buffer.indexOf("\n\n")) !== -1) {
            const rawBlock = buffer.slice(0, sepIdx);
            buffer = buffer.slice(sepIdx + 2);

            let eventName = "message";
            const dataLines = [];
            for (const line of rawBlock.split("\n")) {
              if (!line || line.startsWith(":")) continue;
              const colonIdx = line.indexOf(":");
              const field = colonIdx === -1 ? line : line.slice(0, colonIdx);
              let value = colonIdx === -1 ? "" : line.slice(colonIdx + 1);
              if (value.startsWith(" ")) value = value.slice(1);
              if (field === "event") eventName = value;
              else if (field === "data") dataLines.push(value);
            }

            if (dataLines.length > 0) {
              settled = true;
              clearTimeout(timer);
              const ms = Number((performance.now() - start).toFixed(2));
              const dataRaw = dataLines.join("\n");
              let dataJson = null;
              try {
                dataJson = JSON.parse(dataRaw);
              } catch {
                dataJson = null;
              }
              res.destroy();
              req.destroy();
              resolve({ status, headers, event: eventName, dataRaw, dataJson, ms });
              return;
            }
          }
        });

        res.on("end", () => {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          reject(new Error(`SSE stream ended before receiving an event: ${targetUrl.href}`));
        });

        res.on("error", (err) => {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          reject(err);
        });
      }
    );

    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      req.destroy();
      reject(new Error(`SSE timeout after ${timeoutMs}ms: ${targetUrl.href}`));
    }, timeoutMs);

    req.on("error", (err) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      reject(err);
    });

    req.end();
  });
}

export function runCmd(cmd, args = [], options = {}) {
  const {
    env,
    cwd = ROOT_DIR,
    stdin,
    timeoutMs = 15000,
  } = options;

  const mergedEnv = { ...process.env };
  if (env) {
    for (const [k, v] of Object.entries(env)) {
      if (v === undefined || v === null) delete mergedEnv[k];
      else mergedEnv[k] = String(v);
    }
  }

  return new Promise((resolve, reject) => {
    const start = performance.now();
    let settled = false;

    const child = spawn(cmd, args, {
      cwd,
      env: mergedEnv,
      stdio: ["pipe", "pipe", "pipe"],
    });

    const outChunks = [];
    const errChunks = [];
    child.stdout.on("data", (d) => outChunks.push(d));
    child.stderr.on("data", (d) => errChunks.push(d));

    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      child.kill("SIGKILL");
      reject(new Error(`Command timeout after ${timeoutMs}ms: ${cmd} ${args.join(" ")}`));
    }, timeoutMs);

    child.on("error", (err) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      reject(err);
    });

    child.on("close", (code, signal) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      const ms = Number((performance.now() - start).toFixed(2));
      resolve({
        code: code !== null ? code : signal ? 128 : 1,
        stdout: Buffer.concat(outChunks).toString("utf8"),
        stderr: Buffer.concat(errChunks).toString("utf8"),
        ms,
      });
    });

    if (stdin !== undefined && stdin !== null) {
      child.stdin.write(stdin);
    }
    child.stdin.end();
  });
}

export function SuiteRunner(tierName, mode = detectMode()) {
  const startedAt = performance.now();
  const tests = [];

  return {
    async test(name, fn) {
      const t0 = performance.now();
      try {
        await fn();
        const ms = Number((performance.now() - t0).toFixed(2));
        tests.push({ name, status: "passed", ms });
        console.log(`  [PASS] ${name} (${ms} ms)`);
      } catch (err) {
        const ms = Number((performance.now() - t0).toFixed(2));
        const error = err && err.stack ? err.message : String(err);
        tests.push({ name, status: "failed", ms, error });
        console.error(`  [FAIL] ${name} (${ms} ms): ${error}`);
      }
    },

    skip(name, reason = "skipped") {
      tests.push({ name, status: "skipped", ms: 0, reason });
      console.log(`  [SKIP] ${name}: ${reason}`);
    },

    finish() {
      const durationMs = Number((performance.now() - startedAt).toFixed(2));
      const passed = tests.filter((t) => t.status === "passed").length;
      const failed = tests.filter((t) => t.status === "failed").length;
      const skipped = tests.filter((t) => t.status === "skipped").length;
      const summary = {
        tier: tierName,
        mode,
        passed,
        failed,
        skipped,
        durationMs,
        tests,
      };

      fs.mkdirSync(RESULTS_DIR, { recursive: true });
      const outPath = path.join(RESULTS_DIR, `${tierName}.json`);
      fs.writeFileSync(outPath, JSON.stringify(summary, null, 2) + "\n", "utf8");

      console.log(
        `\n[${tierName}] mode=${mode} passed=${passed} failed=${failed} skipped=${skipped} (${durationMs} ms) -> ${outPath}`
      );
      process.exit(failed > 0 ? 1 : 0);
    },
  };
}
