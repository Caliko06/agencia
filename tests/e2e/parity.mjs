#!/usr/bin/env node
// Parity check between two Agencia servers: shape (status + JSON key structure)
// and latency (median of 5 GETs) for the same routes, plus SSE first-event timing.
// Usage: node tests/e2e/parity.mjs http://127.0.0.1:8800 http://127.0.0.1:8899
// GET only. Always exits 0.

const A = (process.argv[2] || "http://127.0.0.1:8800").replace(/\/+$/, "");
const B = (process.argv[3] || "http://127.0.0.1:8899").replace(/\/+$/, "");

const RUNS = 5;
const FETCH_TIMEOUT = 20000;
const SSE_TIMEOUT = 20000;
const MAX_DEPTH = 4;
const DIFF_PRINT_MAX = 12;

function typeOf(v) {
  if (v === null) return "null";
  if (Array.isArray(v)) return "array";
  return typeof v;
}

// Structural diff of parsed JSON: key presence both ways, type mismatches,
// arrays reduced to their first element, recursion capped at MAX_DEPTH.
function diffShape(a, b, path = "$", depth = 0, out = []) {
  if (depth > MAX_DEPTH) return out;
  const ta = typeOf(a);
  const tb = typeOf(b);
  if (ta !== tb) {
    out.push(`${path}: type A=${ta} B=${tb}`);
    return out;
  }
  if (ta === "array") {
    const ea = a[0];
    const eb = b[0];
    if (ea === undefined || eb === undefined) {
      if (ea === undefined && eb === undefined) return out;
      out.push(`${path}[0]: missing in ${ea === undefined ? "A" : "B"} (len ${a.length} vs ${b.length})`);
      return out;
    }
    diffShape(ea, eb, `${path}[0]`, depth + 1, out);
    return out;
  }
  if (ta === "object") {
    const ka = Object.keys(a);
    const kb = Object.keys(b);
    const sa = new Set(ka);
    const sb = new Set(kb);
    for (const k of ka) if (!sb.has(k)) out.push(`${path}.${k}: only in A`);
    for (const k of kb) if (!sa.has(k)) out.push(`${path}.${k}: only in B`);
    for (const k of ka) if (sb.has(k)) diffShape(a[k], b[k], `${path}.${k}`, depth + 1, out);
  }
  return out;
}

async function once(base, path) {
  const url = base + path;
  const t0 = performance.now();
  const res = await fetch(url, {
    method: "GET",
    redirect: "manual",
    headers: { accept: "*/*" },
    signal: AbortSignal.timeout(FETCH_TIMEOUT),
  });
  const buf = await res.arrayBuffer();
  const ms = performance.now() - t0;
  const text = new TextDecoder("utf-8", { fatal: false }).decode(buf);
  let json = null;
  try {
    json = JSON.parse(text);
  } catch {
    json = null;
  }
  const ctype = res.headers.get("content-type") || "";
  return { status: res.status, bytes: buf.byteLength, ms, text, json, ctype };
}

async function sample(base, path) {
  const runs = [];
  let lastErr = null;
  for (let i = 0; i < RUNS; i++) {
    try {
      runs.push(await once(base, path));
    } catch (e) {
      lastErr = e;
    }
  }
  if (!runs.length) {
    return { status: 0, bytes: 0, ms: NaN, json: null, text: "", ctype: "", error: String(lastErr?.message || lastErr) };
  }
  const ms = runs.map((r) => r.ms).sort((x, y) => x - y);
  const median = ms[Math.floor(ms.length / 2)];
  const last = runs[runs.length - 1];
  return { ...last, ms: median, error: null };
}

// Wait for the first "event: state" line on an SSE endpoint. Abort after SSE_TIMEOUT.
async function sseFirstState(base, path) {
  const path2 = "/api/events";
  const ac = new AbortController();
  const timer = setTimeout(() => ac.abort(new Error("timeout")), SSE_TIMEOUT);
  const t0 = performance.now();
  try {
    const res = await fetch(base + path2, {
      method: "GET",
      headers: { accept: "text/event-stream" },
      signal: ac.signal,
    });
    if (!res.ok || !res.body) {
      clearTimeout(timer);
      return { ok: false, status: res.status, ms: performance.now() - t0, why: `status ${res.status}` };
    }
    const reader = res.body.getReader();
    const dec = new TextDecoder();
    let acc = "";
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      acc += dec.decode(value, { stream: true });
      if (acc.includes("event: state")) {
        const ms = performance.now() - t0;
        clearTimeout(timer);
        try {
          await reader.cancel();
        } catch {}
        return { ok: true, status: res.status, ms, why: `status ${res.status}` };
      }
      if (acc.length > 1 << 20) acc = acc.slice(-65536);
    }
    clearTimeout(timer);
    return { ok: false, status: res.status, ms: performance.now() - t0, why: "stream ended without event: state" };
  } catch (e) {
    clearTimeout(timer);
    return { ok: false, status: 0, ms: performance.now() - t0, why: String(e?.message || e) };
  }
}

const fmtB = (n) => (n >= 1048576 ? (n / 1048576).toFixed(1) + "M" : n >= 1024 ? (n / 1024).toFixed(1) + "k" : String(n) + "B");
const fmtMs = (n) => (Number.isFinite(n) ? n.toFixed(n < 10 ? 2 : 0) : "-") + "ms";

const findings = [];
const rows = [];

function addFinding(ep, msg) {
  findings.push({ ep, msg });
}

// --- target agent id from A's /api/state -------------------------------------
let agentId = null;
try {
  const st = await sample(A, "/api/state");
  const repos = st.json?.repos;
  if (Array.isArray(repos)) {
    for (const r of repos) {
      const w = Array.isArray(r?.windows) ? r.windows[0] : null;
      if (w && w.id) {
        agentId = String(w.id);
        break;
      }
    }
  }
} catch {}
if (!agentId) addFinding("/api/state", "no repos[].windows[].id found in A; /api/agent skipped");

const targets = ["/", "/api/state", "/api/repos", "/api/activity", "/api/real", "/api/re", "/api/consultor", "/api/bus"];
if (agentId) targets.push(`/api/agent?id=${encodeURIComponent(agentId)}`);

for (const path of targets) {
  let a;
  let b;
  try {
    [a, b] = await Promise.all([sample(A, path), sample(B, path)]);
  } catch (e) {
    addFinding(path, `fetch failed: ${e?.message || e}`);
    rows.push({ path, a: null, b: null, diffs: 1 });
    continue;
  }
  const diffs = [];
  if (a.error) addFinding(path, `A error: ${a.error}`);
  if (b.error) addFinding(path, `B error: ${b.error}`);
  if (a.status !== b.status) {
    diffs.push(`status A=${a.status} B=${b.status}`);
    addFinding(path, `status A=${a.status} B=${b.status}`);
  }
  if (a.json !== null || b.json !== null) {
    if (a.json === null || b.json === null) {
      diffs.push(`body type: A=${a.json ? "json" : a.text.slice(0, 40).startsWith("<!doctype") || a.ctype.includes("html") ? "html" : "text"} B=${b.json ? "json" : b.ctype || "text"}`);
      addFinding(path, diffs[diffs.length - 1]);
    } else {
      const d = diffShape(a.json, b.json);
      for (const line of d) {
        diffs.push(line);
        addFinding(path, line);
      }
    }
  }
  rows.push({ path, a, b, diffs });
}

// --- SSE latency -------------------------------------------------------------
const [sseA, sseB] = [await sseFirstState(A), await sseFirstState(B)];
if (!sseA.ok) addFinding("/api/events", `A no event: state (${sseA.why})`);
if (!sseB.ok) addFinding("/api/events", `B no event: state (${sseB.why})`);

// --- table -------------------------------------------------------------------
const W = Math.max(24, ...targets.map((t) => t.length)) + 2;
const col = (s, w) => {
  s = String(s);
  return s.length >= w ? s.slice(0, w - 1) + " " : s + " ".repeat(w - s.length);
};
const cell = (r) => (r ? `${r.status} ${fmtB(r.bytes).padStart(6)} ${fmtMs(r.ms).padStart(8)}` : "fetch failed");

console.log(`A = ${A}`);
console.log(`B = ${B}`);
console.log("");
console.log(`${col("ENDPOINT (GET)", W)}${col("A  stat      bytes        ms", 26)}${col("B  stat      bytes        ms", 26)}DIFF`);
console.log("-".repeat(W + 26 + 26 + 6));
for (const r of rows) {
  console.log(`${col(r.path, W)}${col(cell(r.a), 26)}${col(cell(r.b), 26)}${r.diffs.length ? r.diffs.length + " gap(s)" : "ok"}`);
}
console.log(
  `${col("SSE /api/events 1st state", W)}${col(sseA.ok ? `ok  ${fmtMs(sseA.ms)}` : `FAIL  ${sseA.why}`, 26)}${col(
    sseB.ok ? `ok  ${fmtMs(sseB.ms)}` : `FAIL  ${sseB.why}`,
    26
  )}${sseA.ok && sseB.ok ? "ok" : "gap"}`
);
console.log("-".repeat(W + 26 + 26 + 6));

if (findings.length) {
  console.log("");
  console.log("STRUCTURAL DIFFS");
  const byEp = new Map();
  for (const f of findings) {
    if (!byEp.has(f.ep)) byEp.set(f.ep, []);
    byEp.get(f.ep).push(f.msg);
  }
  for (const [ep, msgs] of byEp) {
    console.log(`  ${ep}`);
    for (const m of msgs.slice(0, DIFF_PRINT_MAX)) console.log(`    - ${m}`);
    if (msgs.length > DIFF_PRINT_MAX) console.log(`    ... ${msgs.length - DIFF_PRINT_MAX} more`);
  }
}

console.log("");
console.log(findings.length ? `PARITY GAPS: ${findings.length}` : "PARITY OK");
process.exit(0);
