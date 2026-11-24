#!/usr/bin/env bash
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="${AGENCIA_ROOT:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
E2E_DIR="$ROOT_DIR/tests/e2e"
RESULTS_DIR="$E2E_DIR/results"
mkdir -p "$RESULTS_DIR"

BASE_URL="${AGENCIA_URL:-http://127.0.0.1:8800}"
EXPLICIT_AGENCIA_BIN="${AGENCIA_BIN:-}"
AGENCIA_BIN="${AGENCIA_BIN:-$ROOT_DIR/target/release/agencia}"
RAW_MODE="$(printf '%s' "${AGENCIA_MODE:-auto}" | tr '[:upper:]' '[:lower:]')"

if [ "$RAW_MODE" = "pre" ] || [ "$RAW_MODE" = "post" ]; then
  MODE="$RAW_MODE"
elif [ -n "$EXPLICIT_AGENCIA_BIN" ]; then
  if [ -f "$EXPLICIT_AGENCIA_BIN" ]; then
    MODE="post"
  elif [ -f "$ROOT_DIR/agencia.py" ]; then
    MODE="pre"
  elif [ -f "$ROOT_DIR/target/release/agencia" ] && [ ! -f "$ROOT_DIR/agencia.py" ]; then
    MODE="post"
  else
    MODE="pre"
  fi
else
  if [ -f "$ROOT_DIR/target/release/agencia" ] && [ ! -f "$ROOT_DIR/agencia.py" ]; then
    MODE="post"
  else
    MODE="pre"
  fi
fi

if [ -e "$AGENCIA_BIN" ]; then
  AGENCIA_BIN="$(node -e 'const path=require("path"); console.log(path.resolve(process.argv[1]))' "$AGENCIA_BIN" 2>/dev/null || echo "$AGENCIA_BIN")"
fi

PORT="$(node -e '
  const u = new URL(process.argv[1]);
  console.log(u.port || (u.protocol === "https:" ? "443" : "80"));
' "$BASE_URL" 2>/dev/null || echo "8800")"

TMP_WORK="$RESULTS_DIR/.tmp_audit_$$"
mkdir -p "$TMP_WORK"
TESTS_NDJSON="$TMP_WORK/tests.ndjson"
FINDINGS_JSON="$TMP_WORK/findings.json"
: > "$TESTS_NDJSON"

cleanup() {
  if [ -n "${ISO_PID:-}" ] && kill -0 "$ISO_PID" 2>/dev/null; then
    kill "$ISO_PID" 2>/dev/null || true
    wait "$ISO_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP_WORK"
}
trap cleanup EXIT

now_ms() {
  node -e 'console.log(Math.round(performance.timeOrigin + performance.now()))'
}

record_test() {
  local name="$1"
  local status="$2"
  local ms="$3"
  local details="$4"
  node -e '
    const fs = require("fs");
    const rec = {
      name: process.argv[1],
      status: process.argv[2],
      ms: Number(process.argv[3]) || 0,
      details: process.argv[4] || ""
    };
    fs.appendFileSync(process.argv[5], JSON.stringify(rec) + "\n");
  ' "$name" "$status" "$ms" "$details" "$TESTS_NDJSON"
}

START_MS="$(now_ms)"

# ---------------------------------------------------------------------------
# 1. Static check: .py / .zsh files in repo (excluding vendored/build/data dirs)
# ---------------------------------------------------------------------------
t0="$(now_ms)"
FOUND_PY_ZSH="$(find "$ROOT_DIR" \
  \( -path "$ROOT_DIR/app/node_modules" \
  -o -path "$ROOT_DIR/target" \
  -o -path "$ROOT_DIR/.git" \
  -o -path "$ROOT_DIR/cache" \
  -o -path "$ROOT_DIR/jobs" \
  -o -path "$ROOT_DIR/tools/ghidra" \) -prune \
  -o -type f \( -name "*.py" -o -name "*.zsh" \) -print 2>/dev/null | sort)"

CORE_PY_FOUND=""
for core in agencia.py jobs.py bus.py bus_mcp.py procs.py avance.py cartero.py agylink.py; do
  if [ -e "$ROOT_DIR/$core" ]; then
    CORE_PY_FOUND="${CORE_PY_FOUND}${CORE_PY_FOUND:+$'\n'}$ROOT_DIR/$core"
  fi
done
for f in "$ROOT_DIR"/bin/*.py "$ROOT_DIR"/bin/*.zsh; do
  if [ -e "$f" ]; then
    CORE_PY_FOUND="${CORE_PY_FOUND}${CORE_PY_FOUND:+$'\n'}$f"
  fi
done

E2E_PY_FOUND="$(find "$E2E_DIR" -type f -name "*.py" 2>/dev/null | sort)"
t1="$(now_ms)"
elapsed=$((t1 - t0))

if [ -n "$E2E_PY_FOUND" ]; then
  record_test "static:zero_python_in_tests_e2e" "failed" "$elapsed" "Found Python files in tests/e2e: $(echo "$E2E_PY_FOUND" | tr '\n' ' ')"
else
  record_test "static:zero_python_in_tests_e2e" "passed" "$elapsed" "No .py files under tests/e2e"
fi

PY_COUNT="$(printf '%s\n' "$FOUND_PY_ZSH" | grep -c . || true)"
CORE_COUNT="$(printf '%s\n' "$CORE_PY_FOUND" | grep -c . || true)"

if [ "$PY_COUNT" -eq 0 ] && [ "$CORE_COUNT" -eq 0 ]; then
  record_test "static:zero_py_zsh_core_files" "passed" "$elapsed" "Zero .py/.zsh files in core repo paths"
elif [ "$MODE" = "pre" ]; then
  record_test "static:zero_py_zsh_core_files" "skipped" "$elapsed" "pre mode: detected $PY_COUNT .py/.zsh files ($CORE_COUNT core files)"
else
  record_test "static:zero_py_zsh_core_files" "failed" "$elapsed" "Found $PY_COUNT .py/.zsh files ($CORE_COUNT core): $(echo "$FOUND_PY_ZSH" | tr '\n' ', ')"
fi

# ---------------------------------------------------------------------------
# 2. Static check: AGENCIA_BIN Mach-O & otool -L (no libpython)
# ---------------------------------------------------------------------------
t0="$(now_ms)"
BIN_FILE_OUT=""
BIN_OTOOL_OUT=""
BIN_OK=0
if [ -x "$AGENCIA_BIN" ]; then
  BIN_FILE_OUT="$(file -b "$AGENCIA_BIN" 2>&1 || true)"
  BIN_OTOOL_OUT="$(otool -L "$AGENCIA_BIN" 2>&1 || true)"
  if echo "$BIN_FILE_OUT" | grep -q "Mach-O" && ! echo "$BIN_OTOOL_OUT" | grep -qi "python"; then
    BIN_OK=1
  fi
fi
t1="$(now_ms)"
elapsed=$((t1 - t0))

if [ "$BIN_OK" -eq 1 ]; then
  record_test "static:rust_binary_macho" "passed" "$elapsed" "$AGENCIA_BIN -> $BIN_FILE_OUT (no Python dylib)"
elif [ "$MODE" = "pre" ] && [ ! -e "$AGENCIA_BIN" ]; then
  record_test "static:rust_binary_macho" "skipped" "$elapsed" "pre mode: $AGENCIA_BIN not built yet"
else
  record_test "static:rust_binary_macho" "failed" "$elapsed" "Invalid or missing Rust binary $AGENCIA_BIN (file='$BIN_FILE_OUT')"
fi

# ---------------------------------------------------------------------------
# 3. Static check: 9 tools in bin/ are compiled Mach-O or symlinks to Rust bin
# ---------------------------------------------------------------------------
t0="$(now_ms)"
BIN_TOOLS="agencia-up bec cdx cla slice real-fin agshot agy-send sesiones-bus"
BIN_REPORT_FILE="$TMP_WORK/bin_tools.ndjson"
: > "$BIN_REPORT_FILE"
BIN_BAD_TOOLS=""

for tool in $BIN_TOOLS; do
  tp="$ROOT_DIR/bin/$tool"
  exists="false"
  is_symlink="false"
  symlink_target=""
  file_info=""
  has_python="false"
  is_rust="false"

  if [ -e "$tp" ] || [ -L "$tp" ]; then
    exists="true"
    if [ -L "$tp" ]; then
      is_symlink="true"
      symlink_target="$(readlink "$tp" 2>/dev/null || true)"
    fi
    file_info="$(file -bL "$tp" 2>&1 || true)"
    if echo "$file_info" | grep -q "Mach-O"; then
      is_rust="true"
      if [ "$is_symlink" = "true" ] && [ -x "$AGENCIA_BIN" ]; then
        resolved_tp="$(node -e 'const fs=require("fs"); try{console.log(fs.realpathSync(process.argv[1]))}catch{}' "$tp")"
        resolved_bin="$(node -e 'const fs=require("fs"); try{console.log(fs.realpathSync(process.argv[1]))}catch{}' "$AGENCIA_BIN")"
        if [ -n "$resolved_tp" ] && [ "$resolved_tp" != "$resolved_bin" ]; then
          is_rust="false"
        fi
      fi
    else
      if grep -qiE "python3?|#!/bin/zsh" "$tp" 2>/dev/null; then
        has_python="true"
      fi
    fi
  fi

  if [ "$is_rust" != "true" ] || [ "$has_python" = "true" ]; then
    BIN_BAD_TOOLS="${BIN_BAD_TOOLS}${BIN_BAD_TOOLS:+, }$tool($file_info)"
  fi

  node -e '
    const fs = require("fs");
    fs.appendFileSync(process.argv[1], JSON.stringify({
      tool: process.argv[2],
      exists: process.argv[3] === "true",
      isSymlink: process.argv[4] === "true",
      symlinkTarget: process.argv[5],
      fileInfo: process.argv[6],
      hasPython: process.argv[7] === "true",
      isRustMachO: process.argv[8] === "true"
    }) + "\n");
  ' "$BIN_REPORT_FILE" "$tool" "$exists" "$is_symlink" "$symlink_target" "$file_info" "$has_python" "$is_rust"
done

t1="$(now_ms)"
elapsed=$((t1 - t0))
if [ -z "$BIN_BAD_TOOLS" ]; then
  record_test "static:bin_tools_rust_macho" "passed" "$elapsed" "All 9 bin/ tools are Mach-O Rust binaries/symlinks with zero python3/zsh"
elif [ "$MODE" = "pre" ]; then
  record_test "static:bin_tools_rust_macho" "skipped" "$elapsed" "pre mode: legacy scripts in bin/: $BIN_BAD_TOOLS"
else
  record_test "static:bin_tools_rust_macho" "failed" "$elapsed" "Non-Rust or Python tools in bin/: $BIN_BAD_TOOLS"
fi

# ---------------------------------------------------------------------------
# 4. Live listener check via lsof on AGENCIA_URL port
# ---------------------------------------------------------------------------
t0="$(now_ms)"
LSOF_OUT="$(lsof -nP -iTCP:"$PORT" -sTCP:LISTEN 2>/dev/null || true)"
LIVE_PID="$(printf '%s\n' "$LSOF_OUT" | awk 'NR==2 {print $2}')"
LIVE_COMM="$(printf '%s\n' "$LSOF_OUT" | awk 'NR==2 {print $1}')"
LIVE_PS_COMM=""
LIVE_PS_ARGS=""
if [ -n "$LIVE_PID" ]; then
  LIVE_PS_COMM="$(ps -p "$LIVE_PID" -o comm= 2>/dev/null || true)"
  LIVE_PS_ARGS="$(ps -p "$LIVE_PID" -o args= 2>/dev/null || true)"
fi
t1="$(now_ms)"
elapsed=$((t1 - t0))

LISTENER_IS_RUST=0
if [ -n "$LIVE_PID" ]; then
  if ! echo "$LIVE_COMM $LIVE_PS_COMM $LIVE_PS_ARGS" | grep -qi "python"; then
    if echo "$LIVE_COMM $LIVE_PS_COMM" | grep -qi "agencia"; then
      LISTENER_IS_RUST=1
    fi
  fi
fi

if [ "$LISTENER_IS_RUST" -eq 1 ]; then
  record_test "live:listener_is_rust" "passed" "$elapsed" "Port $PORT PID=$LIVE_PID comm=$LIVE_COMM ($LIVE_PS_COMM)"
elif [ "$MODE" = "pre" ]; then
  if [ -n "$LIVE_PID" ]; then
    record_test "live:listener_is_rust" "skipped" "$elapsed" "pre mode: port $PORT owned by PID=$LIVE_PID comm=$LIVE_COMM ($LIVE_PS_ARGS)"
  else
    record_test "live:listener_is_rust" "skipped" "$elapsed" "pre mode: no listener on port $PORT"
  fi
else
  record_test "live:listener_is_rust" "failed" "$elapsed" "Expected Rust listener on port $PORT, got PID='${LIVE_PID:-none}' comm='${LIVE_COMM:-none}' ($LIVE_PS_ARGS)"
fi

# ---------------------------------------------------------------------------
# 5. Runtime Non-Sudo Subprocess Audit on live listener (pgrep -P + ps)
# ---------------------------------------------------------------------------
t0="$(now_ms)"
OBSERVED_CHILDREN=""
FORBIDDEN_CHILDREN=""
POLL_SAMPLES=0

if [ -n "$LIVE_PID" ]; then
  CHILD_LOG="$TMP_WORK/live_children.txt"
  : > "$CHILD_LOG"
  CURL_PIDS=""
  for i in 1 2 3; do
    curl -sf --max-time 3 "$BASE_URL/api/state" >/dev/null 2>&1 &
    CURL_PIDS="$CURL_PIDS $!"
    curl -sf --max-time 4 "$BASE_URL/api/activity" >/dev/null 2>&1 &
    CURL_PIDS="$CURL_PIDS $!"
    curl -sf --max-time 2 "$BASE_URL/api/events" >/dev/null 2>&1 &
    CURL_PIDS="$CURL_PIDS $!"
  done
  curl -sf --max-time 3 -X POST -H "Content-Type: application/json" -d '{}' "$BASE_URL/api/pulse" >/dev/null 2>&1 &
  CURL_PIDS="$CURL_PIDS $!"

  while true; do
    ANY_ALIVE=0
    for cp in $CURL_PIDS; do
      if kill -0 "$cp" 2>/dev/null; then
        ANY_ALIVE=1
        break
      fi
    done

    POLL_SAMPLES=$((POLL_SAMPLES + 1))
    CHILDS="$(pgrep -P "$LIVE_PID" 2>/dev/null || true)"
    if [ -n "$CHILDS" ]; then
      PS_COMMS="$(ps -axo pid,ppid,comm 2>/dev/null | awk -v p="$LIVE_PID" '$2 == p {print $3}' || true)"
      if [ -n "$PS_COMMS" ]; then
        printf '%s\n' "$PS_COMMS" >> "$CHILD_LOG"
      else
        for cpid in $CHILDS; do
          echo "child_pid_$cpid" >> "$CHILD_LOG"
        done
      fi
    fi

    if [ "$ANY_ALIVE" -eq 0 ] && [ "$POLL_SAMPLES" -ge 20 ]; then
      break
    fi
    sleep 0.01
  done

  for cp in $CURL_PIDS; do
    wait "$cp" 2>/dev/null || true
  done

  OBSERVED_CHILDREN="$(sort -u "$CHILD_LOG" | tr '\n' ',' | sed 's/,$//')"
  FORBIDDEN_CHILDREN="$(grep -iE '(^|/)(python3?|ps|lsof)$' "$CHILD_LOG" 2>/dev/null | sort -u | tr '\n' ',' | sed 's/,$//' || true)"
fi
t1="$(now_ms)"
elapsed=$((t1 - t0))

if [ -z "$LIVE_PID" ]; then
  if [ "$MODE" = "pre" ]; then
    record_test "runtime:live_no_child_spawns" "skipped" "$elapsed" "pre mode: no live server on port $PORT"
  else
    record_test "runtime:live_no_child_spawns" "failed" "$elapsed" "No live server on port $PORT to poll"
  fi
elif [ -z "$FORBIDDEN_CHILDREN" ] && [ -z "$OBSERVED_CHILDREN" ]; then
  record_test "runtime:live_no_child_spawns" "passed" "$elapsed" "Zero child spawns across $POLL_SAMPLES polls during 10 concurrent requests to PID=$LIVE_PID"
else
  if [ "$MODE" = "pre" ]; then
    record_test "runtime:live_no_child_spawns" "skipped" "$elapsed" "pre mode: observed children=[${OBSERVED_CHILDREN:-$FORBIDDEN_CHILDREN}] (samples=$POLL_SAMPLES)"
  else
    record_test "runtime:live_no_child_spawns" "failed" "$elapsed" "Unexpected child spawn(s) detected under PID=$LIVE_PID: observed=[$OBSERVED_CHILDREN]${FORBIDDEN_CHILDREN:+ forbidden=[$FORBIDDEN_CHILDREN]} (samples=$POLL_SAMPLES)"
  fi
fi

# ---------------------------------------------------------------------------
# 6. Isolated server + static binary strings + DYLD interposer + PATH shim trap
# ---------------------------------------------------------------------------
t0="$(now_ms)"
TRAP_COUNT=0
TRAP_PREVIEW=""
ISO_RAN=0
ISO_FAIL_REASON=""
BIN_FORBIDDEN_STRINGS=""
DYLD_INTERPOSER_ACTIVE=0

if [ -x "$AGENCIA_BIN" ]; then
  ISO_RAN=1
  SHIM_DIR="$TMP_WORK/shim_bin"
  ISO_DIR="$TMP_WORK/isolated_agencia"
  TRAP_LOG="$TMP_WORK/shim_traps.log"
  INTERPOSER_C="$TMP_WORK/interpose.c"
  INTERPOSER_DYLIB="$TMP_WORK/interpose.dylib"
  mkdir -p "$SHIM_DIR" "$ISO_DIR/cache" "$ISO_DIR/jobs"
  : > "$TRAP_LOG"

  if command -v strings >/dev/null 2>&1; then
    BIN_FORBIDDEN_STRINGS="$(strings "$AGENCIA_BIN" 2>/dev/null | grep -E '(^|[^A-Za-z0-9_])(/bin/ps|/usr/sbin/lsof|/usr/bin/python3?|opencode api)([^A-Za-z0-9_]|$)' | head -n 5 | tr '\n' '; ' | sed 's/; $//' || true)"
  fi

  if command -v cc >/dev/null 2>&1; then
    cat > "$INTERPOSER_C" <<'EOF'
#include <fcntl.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

#define DYLD_INTERPOSE(_replacement, _replacee) \
  __attribute__((used)) static struct { const void* replacement; const void* replacee; } _interpose_##_replacee \
  __attribute__((section("__DATA,__interpose"))) = { (const void*)(unsigned long)&_replacement, (const void*)(unsigned long)&_replacee };

static void log_spawn(const char *fn, const char *target) {
  const char *log_path = getenv("AGENCIA_TRAP_LOG");
  if (!log_path || !*log_path) return;
  int fd = open(log_path, O_WRONLY | O_CREAT | O_APPEND, 0644);
  if (fd < 0) return;
  char buf[1024];
  int n = snprintf(buf, sizeof(buf), "interpose %s %s\n", fn ? fn : "?", target ? target : "(null)");
  if (n > 0) {
    size_t len = (size_t)n < sizeof(buf) ? (size_t)n : sizeof(buf) - 1;
    (void)write(fd, buf, len);
  }
  close(fd);
}

int my_posix_spawn(pid_t *pid, const char *path, const posix_spawn_file_actions_t *fa, const posix_spawnattr_t *attr, char *const argv[], char *const envp[]) {
  log_spawn("posix_spawn", path);
  return posix_spawn(pid, path, fa, attr, argv, envp);
}
DYLD_INTERPOSE(my_posix_spawn, posix_spawn)

int my_posix_spawnp(pid_t *pid, const char *file, const posix_spawn_file_actions_t *fa, const posix_spawnattr_t *attr, char *const argv[], char *const envp[]) {
  log_spawn("posix_spawnp", file);
  return posix_spawnp(pid, file, fa, attr, argv, envp);
}
DYLD_INTERPOSE(my_posix_spawnp, posix_spawnp)

int my_execve(const char *path, char *const argv[], char *const envp[]) {
  log_spawn("execve", path);
  return execve(path, argv, envp);
}
DYLD_INTERPOSE(my_execve, execve)

int my_execvp(const char *file, char *const argv[]) {
  log_spawn("execvp", file);
  return execvp(file, argv);
}
DYLD_INTERPOSE(my_execvp, execvp)

int my_system(const char *command) {
  log_spawn("system", command);
  return system(command);
}
DYLD_INTERPOSE(my_system, system)

FILE *my_popen(const char *command, const char *mode) {
  log_spawn("popen", command);
  return popen(command, mode);
}
DYLD_INTERPOSE(my_popen, popen)
EOF
    if cc -shared -fPIC "$INTERPOSER_C" -o "$INTERPOSER_DYLIB" >/dev/null 2>&1; then
      DYLD_INTERPOSER_ACTIVE=1
    fi
  fi

  for cmd in python3 python ps lsof; do
    cat > "$SHIM_DIR/$cmd" <<EOF
#!/usr/bin/env bash
echo "shim $cmd \$*" >> "$TRAP_LOG"
exit 127
EOF
    chmod +x "$SHIM_DIR/$cmd"
  done

  FREE_PORT="$(node -e '
    const net = require("net");
    const s = net.createServer();
    s.listen(0, "127.0.0.1", () => {
      console.log(s.address().port);
      s.close();
    });
  ')"

  ISO_WEB_DIR="${AGENCIA_WEB_DIR:-$ROOT_DIR/web}"
  if [ "$DYLD_INTERPOSER_ACTIVE" -eq 1 ]; then
    (
      cd "$ROOT_DIR" && \
      AGENCIA_WEB_DIR="$ISO_WEB_DIR" \
      AGENCIA_DIR="$ISO_DIR" \
      AGENCIA_ROOT="$ISO_DIR" \
      AGENCIA_PORT="$FREE_PORT" \
      AGENCIA_TRAP_LOG="$TRAP_LOG" \
      DYLD_INSERT_LIBRARIES="$INTERPOSER_DYLIB" \
      PATH="$SHIM_DIR:/usr/bin:/bin:/usr/sbin:/sbin" \
      exec "$AGENCIA_BIN"
    ) >"$TMP_WORK/iso_server.log" 2>&1 &
  else
    (
      cd "$ROOT_DIR" && \
      AGENCIA_WEB_DIR="$ISO_WEB_DIR" \
      AGENCIA_DIR="$ISO_DIR" \
      AGENCIA_ROOT="$ISO_DIR" \
      AGENCIA_PORT="$FREE_PORT" \
      PATH="$SHIM_DIR:/usr/bin:/bin:/usr/sbin:/sbin" \
      exec "$AGENCIA_BIN"
    ) >"$TMP_WORK/iso_server.log" 2>&1 &
  fi
  ISO_PID=$!

  ISO_URL="http://127.0.0.1:$FREE_PORT"
  READY=0
  for _ in $(seq 1 60); do
    if ! kill -0 "$ISO_PID" 2>/dev/null; then
      break
    fi
    if node -e '
      const net = require("net");
      const s = net.createConnection({ host: "127.0.0.1", port: Number(process.argv[1]) });
      s.once("connect", () => { s.destroy(); process.exit(0); });
      s.once("error", () => process.exit(1));
      setTimeout(() => process.exit(1), 200);
    ' "$FREE_PORT" >/dev/null 2>&1; then
      READY=1
      break
    fi
    sleep 0.1
  done

  if [ "$READY" -eq 1 ]; then
    EP_CHECK_ERR="$(node -e '
      const http = require("http");
      const base = process.argv[1];
      function req(method, p, body) {
        return new Promise((resolve, reject) => {
          const u = new URL(p, base);
          const payload = body !== undefined ? JSON.stringify(body) : null;
          const r = http.request(u, {
            method,
            headers: payload ? { "Content-Type": "application/json", "Content-Length": String(Buffer.byteLength(payload)) } : {}
          }, (res) => {
            res.resume();
            res.on("end", () => resolve({ path: p, status: res.statusCode || 0 }));
          });
          r.setTimeout(3000, () => r.destroy(new Error(`timeout ${method} ${p}`)));
          r.on("error", reject);
          if (payload) r.write(payload);
          r.end();
        });
      }
      function sseState() {
        return new Promise((resolve, reject) => {
          const u = new URL("/api/events", base);
          const r = http.request(u, { method: "GET", headers: { Accept: "text/event-stream" } }, (res) => {
            if ((res.statusCode || 0) !== 200) {
              res.resume();
              return reject(new Error(`/api/events status=${res.statusCode}`));
            }
            let buf = "";
            res.setEncoding("utf8");
            res.on("data", (c) => {
              buf += c.replace(/\r\n/g, "\n");
              let idx;
              while ((idx = buf.indexOf("\n\n")) !== -1) {
                const block = buf.slice(0, idx);
                buf = buf.slice(idx + 2);
                let ev = "message";
                let hasData = false;
                for (const line of block.split("\n")) {
                  if (line.startsWith("event:")) ev = line.slice(6).trim();
                  if (line.startsWith("data:")) hasData = true;
                }
                if (hasData) {
                  res.destroy();
                  r.destroy();
                  if (ev !== "state") return reject(new Error(`/api/events expected event=state, got ${ev}`));
                  return resolve();
                }
              }
            });
            res.on("end", () => reject(new Error("/api/events ended without event")));
          });
          r.setTimeout(3000, () => { r.destroy(); reject(new Error("/api/events SSE timeout")); });
          r.on("error", reject);
          r.end();
        });
      }
      (async () => {
        const gets = ["/", "/api/state", "/api/activity", "/api/repos", "/api/real", "/api/re", "/api/consultor", "/api/bus"];
        for (const p of gets) {
          const res = await req("GET", p);
          if (res.status !== 200) throw new Error(`GET ${p} returned HTTP ${res.status}`);
        }
        const pulse = await req("POST", "/api/pulse", {});
        if (pulse.status !== 200) throw new Error(`POST /api/pulse returned HTTP ${pulse.status}`);
        await sseState();
      })().catch((e) => { console.error(e.message || String(e)); process.exit(1); });
    ' "$ISO_URL" 2>&1)" || ISO_FAIL_REASON="$EP_CHECK_ERR"
  else
    ISO_FAIL_REASON="Isolated server failed to become ready on port $FREE_PORT"
  fi

  if kill -0 "$ISO_PID" 2>/dev/null; then
    kill "$ISO_PID" 2>/dev/null || true
    for _ in $(seq 1 20); do
      kill -0 "$ISO_PID" 2>/dev/null || break
      sleep 0.05
    done
    kill -9 "$ISO_PID" 2>/dev/null || true
  fi
  wait "$ISO_PID" 2>/dev/null || true
  ISO_PID=""

  TRAP_COUNT="$(wc -l < "$TRAP_LOG" | tr -d ' ')"
  if [ "$TRAP_COUNT" -gt 0 ]; then
    TRAP_PREVIEW="$(head -n 5 "$TRAP_LOG" | tr '\n' '; ')"
  fi
fi
t1="$(now_ms)"
elapsed=$((t1 - t0))

if [ "$ISO_RAN" -eq 0 ]; then
  if [ "$MODE" = "pre" ]; then
    record_test "runtime:isolated_path_shim_trap" "skipped" "$elapsed" "pre mode: $AGENCIA_BIN not built yet"
  else
    record_test "runtime:isolated_path_shim_trap" "failed" "$elapsed" "Missing $AGENCIA_BIN for isolated PATH shim test"
  fi
elif [ -n "$BIN_FORBIDDEN_STRINGS" ]; then
  record_test "runtime:isolated_path_shim_trap" "failed" "$elapsed" "Forbidden command strings in $AGENCIA_BIN: [$BIN_FORBIDDEN_STRINGS] (trap_count=$TRAP_COUNT)"
elif [ -n "$ISO_FAIL_REASON" ]; then
  record_test "runtime:isolated_path_shim_trap" "failed" "$elapsed" "$ISO_FAIL_REASON (trap_count=$TRAP_COUNT${TRAP_PREVIEW:+: $TRAP_PREVIEW})"
elif [ "$TRAP_COUNT" -eq 0 ]; then
  record_test "runtime:isolated_path_shim_trap" "passed" "$elapsed" "Isolated Rust server returned HTTP 200 on all 9 endpoints + SSE event:state with 0 subprocess spawns (dyld_interpose=$DYLD_INTERPOSER_ACTIVE)"
else
  record_test "runtime:isolated_path_shim_trap" "failed" "$elapsed" "Isolated server invoked subprocess/shim $TRAP_COUNT time(s): $TRAP_PREVIEW"
fi

END_MS="$(now_ms)"
TOTAL_MS=$((END_MS - START_MS))

# ---------------------------------------------------------------------------
# Assemble tests/e2e/results/tier4_audit.json via node (zero Python)
# ---------------------------------------------------------------------------
export AUDIT_MODE="$MODE"
export AUDIT_BASE_URL="$BASE_URL"
export AUDIT_PORT="$PORT"
export AUDIT_BIN="$AGENCIA_BIN"
export AUDIT_BIN_FILE="$BIN_FILE_OUT"
export AUDIT_BIN_OTOOL="$BIN_OTOOL_OUT"
export AUDIT_PY_ZSH="$FOUND_PY_ZSH"
export AUDIT_CORE_PY="$CORE_PY_FOUND"
export AUDIT_E2E_PY="$E2E_PY_FOUND"
export AUDIT_LIVE_PID="$LIVE_PID"
export AUDIT_LIVE_COMM="$LIVE_COMM"
export AUDIT_LIVE_PS_COMM="$LIVE_PS_COMM"
export AUDIT_LIVE_PS_ARGS="$LIVE_PS_ARGS"
export AUDIT_POLL_SAMPLES="$POLL_SAMPLES"
export AUDIT_OBSERVED_CHILDREN="$OBSERVED_CHILDREN"
export AUDIT_FORBIDDEN_CHILDREN="$FORBIDDEN_CHILDREN"
export AUDIT_ISO_RAN="$ISO_RAN"
export AUDIT_TRAP_COUNT="$TRAP_COUNT"
export AUDIT_TRAP_PREVIEW="$TRAP_PREVIEW"
export AUDIT_BIN_FORBIDDEN_STRINGS="$BIN_FORBIDDEN_STRINGS"
export AUDIT_DYLD_INTERPOSER="$DYLD_INTERPOSER_ACTIVE"
export AUDIT_TOTAL_MS="$TOTAL_MS"
export AUDIT_TESTS_NDJSON="$TESTS_NDJSON"
export AUDIT_BIN_REPORT="$BIN_REPORT_FILE"
export AUDIT_OUT_JSON="$RESULTS_DIR/tier4_audit.json"

node -e '
  const fs = require("fs");
  const lines = (s) => (s || "").split("\n").map(x => x.trim()).filter(Boolean);
  const tests = fs.readFileSync(process.env.AUDIT_TESTS_NDJSON, "utf8")
    .trim().split("\n").filter(Boolean).map(l => JSON.parse(l));
  const binTools = fs.readFileSync(process.env.AUDIT_BIN_REPORT, "utf8")
    .trim().split("\n").filter(Boolean).map(l => JSON.parse(l));

  const passed = tests.filter(t => t.status === "passed").length;
  const failed = tests.filter(t => t.status === "failed").length;
  const skipped = tests.filter(t => t.status === "skipped").length;

  const out = {
    tier: "tier4_audit",
    mode: process.env.AUDIT_MODE,
    passed,
    failed,
    skipped,
    durationMs: Number(process.env.AUDIT_TOTAL_MS) || 0,
    tests,
    audit: {
      baseUrl: process.env.AUDIT_BASE_URL,
      port: Number(process.env.AUDIT_PORT),
      agenciaBin: process.env.AUDIT_BIN,
      binFile: process.env.AUDIT_BIN_FILE || null,
      binOtool: lines(process.env.AUDIT_BIN_OTOOL),
      static: {
        pyZshFiles: lines(process.env.AUDIT_PY_ZSH),
        corePyZshFiles: lines(process.env.AUDIT_CORE_PY),
        e2ePyFiles: lines(process.env.AUDIT_E2E_PY),
        binTools
      },
      liveListener: {
        pid: process.env.AUDIT_LIVE_PID ? Number(process.env.AUDIT_LIVE_PID) : null,
        comm: process.env.AUDIT_LIVE_COMM || null,
        psComm: process.env.AUDIT_LIVE_PS_COMM || null,
        psArgs: process.env.AUDIT_LIVE_PS_ARGS || null
      },
      runtimePoll: {
        samples: Number(process.env.AUDIT_POLL_SAMPLES) || 0,
        observedChildren: (process.env.AUDIT_OBSERVED_CHILDREN || "").split(",").filter(Boolean),
        forbiddenChildren: (process.env.AUDIT_FORBIDDEN_CHILDREN || "").split(",").filter(Boolean)
      },
      isolatedShim: {
        ran: process.env.AUDIT_ISO_RAN === "1",
        dyldInterposerActive: process.env.AUDIT_DYLD_INTERPOSER === "1",
        forbiddenBinaryStrings: process.env.AUDIT_BIN_FORBIDDEN_STRINGS || "",
        trapCount: Number(process.env.AUDIT_TRAP_COUNT) || 0,
        trapPreview: process.env.AUDIT_TRAP_PREVIEW || ""
      }
    }
  };
  fs.writeFileSync(process.env.AUDIT_OUT_JSON, JSON.stringify(out, null, 2) + "\n");
  console.log(`[tier4_audit] mode=${out.mode} passed=${passed} failed=${failed} skipped=${skipped} (${out.durationMs}ms)`);
  for (const t of tests) {
    console.log(`  [${t.status.toUpperCase()}] ${t.name}: ${t.details}`);
  }
'

FAIL_COUNT="$(node -e 'const j=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")); console.log(j.failed)' "$RESULTS_DIR/tier4_audit.json")"
if [ "$FAIL_COUNT" -gt 0 ]; then
  exit 1
fi
exit 0
