#!/usr/bin/env bash
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="${AGENCIA_ROOT:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
E2E_DIR="$ROOT_DIR/tests/e2e"
RESULTS_DIR="$E2E_DIR/results"
mkdir -p "$RESULTS_DIR"

TIER="all"
MODE="${AGENCIA_MODE:-auto}"
URL="${AGENCIA_URL:-}"
BIN="${AGENCIA_BIN:-}"
PORT="${AGENCIA_PORT:-}"
BIN_EXPLICIT=0
PORT_EXPLICIT=0

usage() {
  cat <<'EOF'
Usage: ./tests/e2e/run.sh [--tier 1|2|3|4|all] [--mode auto|pre|post] [--url <url>] [--bin <path>] [--port <port>]
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --tier) TIER="${2:-}"; shift 2 ;;
    --tier=*) TIER="${1#*=}"; shift ;;
    --mode) MODE="${2:-}"; shift 2 ;;
    --mode=*) MODE="${1#*=}"; shift ;;
    --url) URL="${2:-}"; shift 2 ;;
    --url=*) URL="${1#*=}"; shift ;;
    --bin) BIN="${2:-}"; BIN_EXPLICIT=1; shift 2 ;;
    --bin=*) BIN="${1#*=}"; BIN_EXPLICIT=1; shift ;;
    --port) PORT="${2:-}"; PORT_EXPLICIT=1; shift 2 ;;
    --port=*) PORT="${1#*=}"; PORT_EXPLICIT=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

case "$TIER" in
  1|2|3|4|all) ;;
  *) echo "Invalid --tier: $TIER (expected 1|2|3|4|all)" >&2; exit 2 ;;
esac

case "$MODE" in
  auto|pre|post) ;;
  *) echo "Invalid --mode: $MODE (expected auto|pre|post)" >&2; exit 2 ;;
esac

export AGENCIA_MODE="$MODE"
if [ -n "$BIN" ]; then
  export AGENCIA_BIN="$BIN"
  export AGENCIA_BIN_EXPLICIT="1"
fi
if [ -n "$PORT" ]; then
  export AGENCIA_PORT="$PORT"
  if [ -z "$URL" ]; then
    URL="http://127.0.0.1:$PORT"
  fi
fi
if [ -n "$URL" ]; then
  export AGENCIA_URL="$URL"
elif [ -z "${AGENCIA_URL:-}" ]; then
  export AGENCIA_URL="http://127.0.0.1:8800"
fi

RUN_LOG="$RESULTS_DIR/.tmp_run_$$.ndjson"
TMP_SERVER_LOG="$RESULTS_DIR/.tmp_run_server_$$.log"
TMP_AGENCIA_DIR=""
SPAWNED_PID=""
: > "$RUN_LOG"

cleanup() {
  if [ -n "${SPAWNED_PID:-}" ] && kill -0 "$SPAWNED_PID" 2>/dev/null; then
    kill "$SPAWNED_PID" 2>/dev/null || true
    for _ in $(seq 1 20); do
      kill -0 "$SPAWNED_PID" 2>/dev/null || break
      sleep 0.1
    done
    kill -9 "$SPAWNED_PID" 2>/dev/null || true
    wait "$SPAWNED_PID" 2>/dev/null || true
  fi
  rm -f "$RUN_LOG" "$TMP_SERVER_LOG"
  if [ -n "${TMP_AGENCIA_DIR:-}" ]; then
    rm -rf "$TMP_AGENCIA_DIR"
  fi
}
trap cleanup EXIT

port_is_listening() {
  node -e '
    const net = require("net");
    const s = net.createConnection({ host: "127.0.0.1", port: Number(process.argv[1]) });
    s.once("connect", () => { s.destroy(); process.exit(0); });
    s.once("error", () => process.exit(1));
    setTimeout(() => process.exit(1), 250);
  ' "$1" >/dev/null 2>&1
}

if [ "$BIN_EXPLICIT" -eq 1 ] && [ "$PORT_EXPLICIT" -eq 1 ]; then
  if [ ! -x "$BIN" ]; then
    echo "[run.sh] --bin is not executable: $BIN" >&2
    exit 1
  fi
  if ! port_is_listening "$PORT"; then
    BIN_ABS="$(node -e 'const path=require("path"); console.log(path.resolve(process.argv[1]))' "$BIN")"
    if [ -z "${AGENCIA_DIR:-}" ]; then
      TMP_AGENCIA_DIR="$RESULTS_DIR/.tmp_run_agencia_$$"
      mkdir -p "$TMP_AGENCIA_DIR/cache" "$TMP_AGENCIA_DIR/jobs"
      export AGENCIA_DIR="$TMP_AGENCIA_DIR"
    fi
    echo "[run.sh] Starting $BIN_ABS on 127.0.0.1:$PORT (AGENCIA_DIR=$AGENCIA_DIR)"
    (
      cd "$ROOT_DIR" && \
      AGENCIA_PORT="$PORT" \
      AGENCIA_WEB_DIR="${AGENCIA_WEB_DIR:-$ROOT_DIR/web}" \
      AGENCIA_DIR="$AGENCIA_DIR" \
      exec "$BIN_ABS"
    ) >"$TMP_SERVER_LOG" 2>&1 &
    SPAWNED_PID=$!

    READY=0
    for _ in $(seq 1 60); do
      if ! kill -0 "$SPAWNED_PID" 2>/dev/null; then
        echo "[run.sh] Spawned binary exited prematurely:" >&2
        cat "$TMP_SERVER_LOG" >&2 || true
        exit 1
      fi
      if port_is_listening "$PORT"; then
        READY=1
        break
      fi
      sleep 0.1
    done
    if [ "$READY" -ne 1 ]; then
      echo "[run.sh] Timed out waiting for $BIN_ABS to listen on port $PORT" >&2
      cat "$TMP_SERVER_LOG" >&2 || true
      exit 1
    fi
  fi
fi

OVERALL_RC=0

run_step() {
  local label="$1"
  local runner_kind="$2"
  local script_path="$3"

  rm -f "$RESULTS_DIR/${label}.json"

  if [ ! -f "$script_path" ]; then
    echo "[run.sh] MISSING: $script_path" >&2
    OVERALL_RC=1
    node -e '
      const fs = require("fs");
      fs.appendFileSync(process.argv[1], JSON.stringify({
        step: process.argv[2],
        script: process.argv[3],
        exitCode: 127,
        ms: 0,
        missing: true
      }) + "\n");
    ' "$RUN_LOG" "$label" "$script_path"
    return
  fi

  local t0 t1 ms rc
  t0="$(node -e 'console.log(Date.now())')"
  echo "[run.sh] Running $label ($script_path) mode=$AGENCIA_MODE url=$AGENCIA_URL"
  if [ "$runner_kind" = "bash" ]; then
    bash "$script_path"
    rc=$?
  else
    node "$script_path"
    rc=$?
  fi
  t1="$(node -e 'console.log(Date.now())')"
  ms=$((t1 - t0))

  if [ "$rc" -ne 0 ]; then
    OVERALL_RC=1
  fi

  node -e '
    const fs = require("fs");
    fs.appendFileSync(process.argv[1], JSON.stringify({
      step: process.argv[2],
      script: process.argv[3],
      exitCode: Number(process.argv[4]),
      ms: Number(process.argv[5]),
      missing: false
    }) + "\n");
  ' "$RUN_LOG" "$label" "$script_path" "$rc" "$ms"
}

if [ "$TIER" = "1" ] || [ "$TIER" = "all" ]; then
  run_step "tier1_api_cli_mcp" "node" "$E2E_DIR/tier1_api_cli_mcp.mjs"
fi

if [ "$TIER" = "2" ] || [ "$TIER" = "all" ]; then
  run_step "tier2_boundaries_load" "node" "$E2E_DIR/tier2_boundaries_load.mjs"
fi

if [ "$TIER" = "3" ] || [ "$TIER" = "all" ]; then
  run_step "tier3_flows_ui" "node" "$E2E_DIR/tier3_flows_ui.mjs"
fi

if [ "$TIER" = "4" ] || [ "$TIER" = "all" ]; then
  run_step "tier4_audit" "bash" "$E2E_DIR/tier4_audit.sh"
  run_step "tier4_bench" "node" "$E2E_DIR/tier4_bench.mjs"
  run_step "tier4_screenshots" "node" "$E2E_DIR/tier4_screenshots.mjs"
fi

# Aggregate ONLY executed steps' JSONs into totals (store any other JSONs under historical)
export AGG_TIER="$TIER"
export AGG_MODE="$AGENCIA_MODE"
export AGG_URL="$AGENCIA_URL"
export AGG_RESULTS_DIR="$RESULTS_DIR"
export AGG_RUN_LOG="$RUN_LOG"
export AGG_OVERALL_RC="$OVERALL_RC"

node -e '
  const fs = require("fs");
  const path = require("path");

  const resultsDir = process.env.AGG_RESULTS_DIR;
  const runSteps = fs.readFileSync(process.env.AGG_RUN_LOG, "utf8")
    .trim().split("\n").filter(Boolean).map(l => JSON.parse(l));

  const executedStepSet = new Set(runSteps.map(s => s.step));
  const tiers = [];
  let totalPassed = 0;
  let totalFailed = 0;
  let totalSkipped = 0;
  let totalDurationMs = 0;

  for (const s of runSteps) {
    const f = `${s.step}.json`;
    const fullPath = path.join(resultsDir, f);
    if (s.missing || !fs.existsSync(fullPath)) {
      tiers.push({
        file: f,
        tier: s.step,
        mode: process.env.AGG_MODE,
        error: s.missing ? "Script missing" : "Tier result JSON missing",
        passed: 0,
        failed: 1,
        skipped: 0,
        durationMs: Number(s.ms) || 0,
      });
      totalFailed += 1;
      totalDurationMs += Number(s.ms) || 0;
      continue;
    }
    try {
      const data = JSON.parse(fs.readFileSync(fullPath, "utf8"));
      const passed = Number(data.passed) || 0;
      const failed = Number(data.failed) || 0;
      const skipped = Number(data.skipped) || 0;
      const durationMs = Number(data.durationMs) || 0;
      totalPassed += passed;
      totalFailed += failed;
      totalSkipped += skipped;
      totalDurationMs += durationMs;
      tiers.push({
        file: f,
        tier: data.tier || s.step,
        mode: data.mode || process.env.AGG_MODE,
        passed,
        failed,
        skipped,
        durationMs,
      });
    } catch (err) {
      tiers.push({
        file: f,
        tier: s.step,
        mode: process.env.AGG_MODE,
        error: String(err),
        passed: 0,
        failed: 1,
        skipped: 0,
        durationMs: Number(s.ms) || 0,
      });
      totalFailed += 1;
      totalDurationMs += Number(s.ms) || 0;
    }
  }

  const otherJsonFiles = fs.readdirSync(resultsDir)
    .filter(f => f.endsWith(".json") && f !== "summary.json" && !f.startsWith(".tmp"))
    .filter(f => !executedStepSet.has(f.replace(/\.json$/, "")))
    .sort();

  const historical = [];
  for (const f of otherJsonFiles) {
    try {
      const data = JSON.parse(fs.readFileSync(path.join(resultsDir, f), "utf8"));
      historical.push({
        file: f,
        tier: data.tier || f.replace(/\.json$/, ""),
        mode: data.mode || null,
        passed: Number(data.passed) || 0,
        failed: Number(data.failed) || 0,
        skipped: Number(data.skipped) || 0,
        durationMs: Number(data.durationMs) || 0,
      });
    } catch (err) {
      historical.push({ file: f, tier: f.replace(/\.json$/, ""), error: String(err) });
    }
  }

  const uniqueModes = [...new Set(tiers.map(t => t.mode).filter(Boolean))];
  const effectiveMode = process.env.AGG_MODE === "auto" && uniqueModes.length === 1
    ? uniqueModes[0]
    : process.env.AGG_MODE;

  const allStepsOk = runSteps.every(s => s.exitCode === 0 && !s.missing);
  const selectedFailed = tiers.some(t => t.failed > 0);
  const ok = allStepsOk && !selectedFailed && Number(process.env.AGG_OVERALL_RC) === 0;

  const summary = {
    generatedAt: new Date().toISOString(),
    selectedTier: process.env.AGG_TIER,
    requestedMode: process.env.AGG_MODE,
    mode: effectiveMode,
    url: process.env.AGG_URL,
    ok,
    totals: {
      passed: totalPassed,
      failed: totalFailed,
      skipped: totalSkipped,
      durationMs: totalDurationMs,
    },
    executedSteps: runSteps,
    results: tiers,
    historical,
  };

  fs.writeFileSync(path.join(resultsDir, "summary.json"), JSON.stringify(summary, null, 2) + "\n");

  const md = [
    `# Agencia E2E Summary`,
    "",
    `- Generated: ${summary.generatedAt}`,
    `- Selected Tier: \`${summary.selectedTier}\``,
    `- Mode: \`${summary.mode}\` (requested: \`${summary.requestedMode}\`)`,
    `- URL: \`${summary.url}\``,
    `- Overall: **${ok ? "PASS" : "FAIL"}**`,
    "",
    "## Executed Steps",
    "",
    "| Step | Exit Code | Duration (ms) | Status |",
    "|---|---:|---:|---|",
    ...runSteps.map(s => `| ${s.step} | ${s.exitCode} | ${s.ms} | ${s.missing ? "MISSING" : (s.exitCode === 0 ? "PASS" : "FAIL")} |`),
    "",
    "## Executed Tier JSON Results",
    "",
    "| Tier | Mode | Passed | Failed | Skipped | Duration (ms) |",
    "|---|---|---:|---:|---:|---:|",
    ...tiers.map(t => `| ${t.tier} | ${t.mode || "?"} | ${t.passed} | ${t.failed} | ${t.skipped} | ${t.durationMs} |`),
    `| **TOTAL** | — | **${totalPassed}** | **${totalFailed}** | **${totalSkipped}** | **${totalDurationMs}** |`,
    "",
  ].join("\n");

  fs.writeFileSync(path.join(resultsDir, "summary.md"), md);
  console.log(`\n${md}`);
  process.exit(ok ? 0 : 1);
'
exit $?
