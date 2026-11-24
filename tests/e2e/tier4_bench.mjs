import fs from "node:fs";
import path from "node:path";
import net from "node:net";
import { spawn } from "node:child_process";
import {
  ROOT_DIR,
  RESULTS_DIR,
  BASE_URL,
  detectMode,
  httpReq,
  fetchSseFirstEvent,
  runCmd,
  SuiteRunner,
} from "./lib/common.mjs";

const BASELINE = {
  cold_tcp_ms: 115.15,
  cold_state_ms: 596.20,
  live_state_avg_ms: 181.07,
  live_state_max_ms: 1267.96,
  cold_activity_ms: 1405.64,
  sse_first_event_ms: 1106.60,
  agent_avg_ms: 337.48,
  pulse_max_ms: 643.64,
  cli_bec_ls_ms: 152.10,
  cli_sesiones_bus_list_ms: 512.35,
  ui_disconnect_banner_ms: 1309,
};

function round2(n) {
  return Math.round(n * 100) / 100;
}

function summarize(samples) {
  if (!samples || samples.length === 0) return null;
  const sorted = [...samples].sort((a, b) => a - b);
  const sum = sorted.reduce((acc, v) => acc + v, 0);
  const p95Idx = Math.min(sorted.length - 1, Math.ceil(sorted.length * 0.95) - 1);
  return {
    n: sorted.length,
    min: round2(sorted[0]),
    avg: round2(sum / sorted.length),
    p95: round2(sorted[p95Idx]),
    max: round2(sorted[sorted.length - 1]),
  };
}

function speedupStr(baselineMs, measuredMs) {
  if (!baselineMs || !measuredMs || measuredMs <= 0) return "N/A";
  const ratio = baselineMs / measuredMs;
  return `${round2(ratio)}x`;
}

function getFreePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.unref();
    srv.on("error", reject);
    srv.listen(0, "127.0.0.1", () => {
      const addr = srv.address();
      srv.close(() => resolve(addr.port));
    });
  });
}

function waitForTcp(host, port, timeoutMs = 6000) {
  const start = performance.now();
  return new Promise((resolve, reject) => {
    const tryConnect = () => {
      const sock = net.createConnection({ host, port });
      sock.once("connect", () => {
        const ms = performance.now() - start;
        sock.destroy();
        resolve(ms);
      });
      sock.once("error", () => {
        sock.destroy();
        if (performance.now() - start >= timeoutMs) {
          reject(new Error(`TCP listen timeout after ${timeoutMs}ms on ${host}:${port}`));
        } else {
          setTimeout(tryConnect, 5);
        }
      });
    };
    tryConnect();
  });
}

async function measureColdBinary(binPath) {
  const port = await getFreePort();
  const tmpDir = path.join(RESULTS_DIR, `.tmp_bench_${process.pid}_${Date.now()}`);
  fs.mkdirSync(path.join(tmpDir, "cache"), { recursive: true });
  fs.mkdirSync(path.join(tmpDir, "jobs"), { recursive: true });

  const child = spawn(binPath, [], {
    cwd: ROOT_DIR,
    env: {
      ...process.env,
      AGENCIA_DIR: tmpDir,
      AGENCIA_ROOT: tmpDir,
      AGENCIA_PORT: String(port),
      AGENCIA_WEB_DIR: process.env.AGENCIA_WEB_DIR || path.join(ROOT_DIR, "web"),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  const t0 = performance.now();
  try {
    await waitForTcp("127.0.0.1", port, 6000);
    const cold_tcp_ms = round2(performance.now() - t0);
    const isoUrl = `http://127.0.0.1:${port}`;
    const st = await httpReq("GET", "/api/state", { baseUrl: isoUrl, timeoutMs: 10000 });
    const cold_state_ms = round2(st.ms);
    const act = await httpReq("GET", "/api/activity", { baseUrl: isoUrl, timeoutMs: 15000 });
    const cold_activity_ms = round2(act.ms);
    return {
      cold_tcp_ms,
      cold_state_ms,
      cold_state_status: st.status,
      cold_activity_ms,
      cold_activity_status: act.status,
    };
  } finally {
    try {
      child.kill("SIGTERM");
    } catch {}
    await new Promise((r) => {
      const timer = setTimeout(() => {
        try { child.kill("SIGKILL"); } catch {}
        r();
      }, 1500);
      child.once("exit", () => {
        clearTimeout(timer);
        r();
      });
    });
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
}

async function benchLoop(count, fn) {
  const samples = [];
  const errors = [];
  for (let i = 0; i < count; i++) {
    const res = await fn(i);
    if (res.ok) {
      samples.push(res.ms);
    } else {
      errors.push(res.err || `status=${res.status}`);
    }
  }
  return { stats: summarize(samples), samples: samples.map(round2), errors };
}

function buildMarkdown(mode, metrics) {
  const rows = [
    ["Cold TCP listen", BASELINE.cold_tcp_ms, metrics.cold?.cold_tcp_ms ? { min: metrics.cold.cold_tcp_ms, avg: metrics.cold.cold_tcp_ms, p95: metrics.cold.cold_tcp_ms, max: metrics.cold.cold_tcp_ms } : null],
    ["Cold GET /api/state", BASELINE.cold_state_ms, metrics.cold?.cold_state_ms ? { min: metrics.cold.cold_state_ms, avg: metrics.cold.cold_state_ms, p95: metrics.cold.cold_state_ms, max: metrics.cold.cold_state_ms } : null],
    ["Cold GET /api/activity", BASELINE.cold_activity_ms, metrics.cold?.cold_activity_ms ? { min: metrics.cold.cold_activity_ms, avg: metrics.cold.cold_activity_ms, p95: metrics.cold.cold_activity_ms, max: metrics.cold.cold_activity_ms } : null],
    ["Live GET / (10x)", null, metrics.live.root?.stats],
    ["Live GET /api/state (10x)", BASELINE.live_state_avg_ms, metrics.live.state?.stats],
    ["Live GET /api/activity (10x)", BASELINE.cold_activity_ms, metrics.live.activity?.stats],
    ["Live GET /api/repos (10x)", null, metrics.live.repos?.stats],
    ["Live GET /api/real (10x)", null, metrics.live.real?.stats],
    ["Live GET /api/re (10x)", null, metrics.live.re?.stats],
    ["Live GET /api/consultor (10x)", null, metrics.live.consultor?.stats],
    ["Live POST /api/pulse (10x)", BASELINE.pulse_max_ms, metrics.live.pulse?.stats],
    ["Live GET /api/events 1st event (10x)", BASELINE.sse_first_event_ms, metrics.live.events?.stats],
    [`Live GET /api/agent${metrics.agentId ? ` (${metrics.agentId})` : ""} (10x)`, BASELINE.agent_avg_ms, metrics.live.agent?.stats],
    ["CLI bin/bec ls (5x)", BASELINE.cli_bec_ls_ms, metrics.cli.bec_ls?.stats],
    ["CLI bin/sesiones-bus list (5x)", BASELINE.cli_sesiones_bus_list_ms, metrics.cli.sesiones_bus_list?.stats],
  ];

  const lines = [
    `# Agencia E2E Latency Benchmark (${mode} mode)`,
    "",
    `- Generated: ${new Date().toISOString()}`,
    `- Target URL: \`${BASE_URL}\``,
    `- Baseline source: \`survey_2\` (pre-rewrite Python server & CLIs)`,
    "",
    "| Metric | Baseline (ms) | Min (ms) | Avg (ms) | P95 (ms) | Max (ms) | Speedup vs Baseline |",
    "|---|---:|---:|---:|---:|---:|---:|",
  ];

  for (const [label, base, st] of rows) {
    const bStr = base != null ? base.toFixed(2) : "—";
    if (!st) {
      lines.push(`| ${label} | ${bStr} | N/A | N/A | N/A | N/A | N/A |`);
    } else {
      const sp = base != null ? speedupStr(base, st.avg) : "—";
      lines.push(`| ${label} | ${bStr} | ${st.min.toFixed(2)} | ${st.avg.toFixed(2)} | ${st.p95.toFixed(2)} | ${st.max.toFixed(2)} | ${sp} |`);
    }
  }
  lines.push("");
  return lines.join("\n");
}

async function main() {
  fs.mkdirSync(RESULTS_DIR, { recursive: true });
  const mode = await detectMode(BASE_URL);
  const runner = new SuiteRunner("tier4_bench", mode);
  const agenciaBin = process.env.AGENCIA_BIN || path.join(ROOT_DIR, "target/release/agencia");

  const metrics = {
    cold: null,
    agentId: null,
    live: {},
    cli: {},
  };

  // Enrich tier4_bench.json with baseline + metrics even if SuiteRunner.finish() calls process.exit()
  process.on("exit", () => {
    const outPath = path.join(RESULTS_DIR, "tier4_bench.json");
    if (fs.existsSync(outPath)) {
      try {
        const cur = JSON.parse(fs.readFileSync(outPath, "utf8"));
        cur.baseline = BASELINE;
        cur.metrics = metrics;
        fs.writeFileSync(outPath, JSON.stringify(cur, null, 2) + "\n");
      } catch {}
    }
  });

  // 1. Cold startup benchmark (when Rust binary exists)
  if (fs.existsSync(agenciaBin)) {
    await runner.test("bench:cold_rust_startup", async () => {
      metrics.cold = await measureColdBinary(agenciaBin);
      if (metrics.cold.cold_state_status !== 200) {
        throw new Error(`Cold /api/state returned ${metrics.cold.cold_state_status}`);
      }
      if (metrics.cold.cold_activity_status !== 200) {
        throw new Error(`Cold /api/activity returned ${metrics.cold.cold_activity_status}`);
      }
      if (mode === "post") {
        if (metrics.cold.cold_tcp_ms >= BASELINE.cold_tcp_ms) {
          throw new Error(`cold_tcp_ms ${metrics.cold.cold_tcp_ms}ms >= baseline ${BASELINE.cold_tcp_ms}ms`);
        }
        if (metrics.cold.cold_state_ms >= BASELINE.cold_state_ms) {
          throw new Error(`cold_state_ms ${metrics.cold.cold_state_ms}ms >= baseline ${BASELINE.cold_state_ms}ms`);
        }
        if (metrics.cold.cold_activity_ms >= BASELINE.cold_activity_ms) {
          throw new Error(`cold_activity_ms ${metrics.cold.cold_activity_ms}ms >= baseline ${BASELINE.cold_activity_ms}ms`);
        }
      }
      return `cold_tcp=${metrics.cold.cold_tcp_ms}ms, cold_state=${metrics.cold.cold_state_ms}ms, cold_activity=${metrics.cold.cold_activity_ms}ms`;
    });
  } else if (mode === "post") {
    await runner.test("bench:cold_rust_startup", async () => {
      throw new Error(`Missing Rust binary at ${agenciaBin} in post mode`);
    });
  } else {
    runner.skip("bench:cold_rust_startup", `pre mode: ${agenciaBin} not built yet`);
  }

  // 2. Discover first agent id from /api/state
  let firstAgentId = null;
  try {
    const stRes = await httpReq("GET", "/api/state", { timeoutMs: 10000 });
    if (stRes.json && Array.isArray(stRes.json.repos)) {
      for (const r of stRes.json.repos) {
        for (const w of r.windows || []) {
          if (w && w.id) {
            firstAgentId = w.id;
            break;
          }
        }
        if (firstAgentId) break;
      }
    }
  } catch {}
  metrics.agentId = firstAgentId;

  // 3. 10x HTTP endpoints
  const getEndpoints = [
    ["root", "/"],
    ["state", "/api/state"],
    ["activity", "/api/activity"],
    ["repos", "/api/repos"],
    ["real", "/api/real"],
    ["re", "/api/re"],
    ["consultor", "/api/consultor"],
  ];

  for (const [key, ep] of getEndpoints) {
    await runner.test(`bench:http_${key}_10x`, async () => {
      const res = await benchLoop(10, async () => {
        const r = await httpReq("GET", ep, { timeoutMs: 15000 });
        return { ok: r.status === 200, ms: r.ms, status: r.status };
      });
      metrics.live[key] = res;
      if (!res.stats || res.errors.length > 0) {
        throw new Error(`GET ${ep} errors: ${res.errors.join(", ")}`);
      }
      if (mode === "post" && key === "state" && res.stats.avg >= 50) {
        throw new Error(`/api/state avg ${res.stats.avg}ms >= 50ms post-rewrite target`);
      }
      return `avg=${res.stats.avg}ms min=${res.stats.min}ms p95=${res.stats.p95}ms max=${res.stats.max}ms`;
    });
  }

  // 10x POST /api/pulse
  await runner.test("bench:http_pulse_10x", async () => {
    const res = await benchLoop(10, async () => {
      const r = await httpReq("POST", "/api/pulse", { body: {}, timeoutMs: 10000 });
      return { ok: r.status === 200, ms: r.ms, status: r.status };
    });
    metrics.live.pulse = res;
    if (!res.stats || res.errors.length > 0) {
      throw new Error(`POST /api/pulse errors: ${res.errors.join(", ")}`);
    }
    if (mode === "post" && res.stats.max >= 50) {
      throw new Error(`/api/pulse max ${res.stats.max}ms >= 50ms post-rewrite target`);
    }
    return `avg=${res.stats.avg}ms max=${res.stats.max}ms (baseline max=${BASELINE.pulse_max_ms}ms)`;
  });

  // 10x SSE /api/events first event
  await runner.test("bench:sse_events_first_event_10x", async () => {
    const res = await benchLoop(10, async () => {
      const r = await fetchSseFirstEvent("/api/events", 15000, BASE_URL);
      return {
        ok: r.status === 200 && r.event === "state",
        ms: r.ms,
        status: r.status,
        err: r.status !== 200 ? `status=${r.status}` : r.event !== "state" ? `event=${r.event}` : null,
      };
    });
    metrics.live.events = res;
    if (!res.stats || res.errors.length > 0) {
      throw new Error(`SSE /api/events errors: ${res.errors.join(", ")}`);
    }
    if (mode === "post" && res.stats.avg >= 100) {
      throw new Error(`/api/events first event avg ${res.stats.avg}ms >= 100ms post-rewrite target`);
    }
    return `avg=${res.stats.avg}ms max=${res.stats.max}ms (baseline=${BASELINE.sse_first_event_ms}ms)`;
  });

  // 10x GET /api/agent?id=...
  if (firstAgentId) {
    await runner.test("bench:http_agent_10x", async () => {
      const q = `/api/agent?id=${encodeURIComponent(firstAgentId)}`;
      const res = await benchLoop(10, async () => {
        const r = await httpReq("GET", q, { timeoutMs: 15000 });
        return { ok: r.status === 200, ms: r.ms, status: r.status };
      });
      metrics.live.agent = res;
      if (!res.stats || res.errors.length > 0) {
        throw new Error(`GET ${q} errors: ${res.errors.join(", ")}`);
      }
      if (mode === "post" && res.stats.avg >= 100) {
        throw new Error(`/api/agent avg ${res.stats.avg}ms >= 100ms post-rewrite target`);
      }
      return `id=${firstAgentId} avg=${res.stats.avg}ms max=${res.stats.max}ms (baseline avg=${BASELINE.agent_avg_ms}ms)`;
    });
  } else {
    runner.skip("bench:http_agent_10x", "No active agent id returned by /api/state");
  }

  // 4. 5x CLI benchmarks: bin/bec ls and bin/sesiones-bus list
  await runner.test("bench:cli_bec_ls_5x", async () => {
    const res = await benchLoop(5, async () => {
      const r = await runCmd(path.join(ROOT_DIR, "bin/bec"), ["ls"], { timeoutMs: 10000 });
      return { ok: r.code === 0, ms: r.ms, status: r.code, err: r.code !== 0 ? r.stderr.trim() : null };
    });
    metrics.cli.bec_ls = res;
    if (!res.stats || res.errors.length > 0) {
      throw new Error(`bin/bec ls errors: ${res.errors.join(", ")}`);
    }
    if (mode === "post" && res.stats.avg >= 50) {
      throw new Error(`bin/bec ls avg ${res.stats.avg}ms >= 50ms post-rewrite target`);
    }
    return `avg=${res.stats.avg}ms max=${res.stats.max}ms (baseline=${BASELINE.cli_bec_ls_ms}ms)`;
  });

  await runner.test("bench:cli_sesiones_bus_list_5x", async () => {
    const res = await benchLoop(5, async () => {
      const r = await runCmd(path.join(ROOT_DIR, "bin/sesiones-bus"), ["list"], { timeoutMs: 10000 });
      return { ok: r.code === 0, ms: r.ms, status: r.code, err: r.code !== 0 ? r.stderr.trim() : null };
    });
    metrics.cli.sesiones_bus_list = res;
    if (!res.stats || res.errors.length > 0) {
      throw new Error(`bin/sesiones-bus list errors: ${res.errors.join(", ")}`);
    }
    if (mode === "post" && res.stats.avg >= 50) {
      throw new Error(`bin/sesiones-bus list avg ${res.stats.avg}ms >= 50ms post-rewrite target`);
    }
    return `avg=${res.stats.avg}ms max=${res.stats.max}ms (baseline=${BASELINE.cli_sesiones_bus_list_ms}ms)`;
  });

  // Write benchmark.md before runner.finish()
  const md = buildMarkdown(mode, metrics);
  fs.writeFileSync(path.join(RESULTS_DIR, "benchmark.md"), md);
  console.log(`\n${md}`);

  await runner.finish();
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
