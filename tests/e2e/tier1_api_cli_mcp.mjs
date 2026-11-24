import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import {
  BASE_URL,
  RESULTS_DIR,
  ROOT_DIR,
  SuiteRunner,
  detectMode,
  fetchSseFirstEvent,
  httpReq,
  runCmd,
} from "./lib/common.mjs";

const mode = detectMode(BASE_URL);
const runner = SuiteRunner("tier1_api_cli_mcp", mode);

function resolveMcpCommand() {
  if (process.env.AGENCIA_MCP_CMD) {
    const parts = process.env.AGENCIA_MCP_CMD.trim().split(/\s+/);
    return { cmd: parts[0], args: parts.slice(1), isolatedSupported: true };
  }
  const releaseBin = path.join(ROOT_DIR, "target", "release", "agencia");
  if (mode === "post") {
    const bin = process.env.AGENCIA_BIN || releaseBin;
    return { cmd: bin, args: ["mcp"], isolatedSupported: true };
  }
  return {
    cmd: path.join(ROOT_DIR, "bus_mcp.py"),
    args: [],
    isolatedSupported: false,
  };
}

function encodeFrame(obj, framed) {
  const json = JSON.stringify(obj);
  if (!framed) return Buffer.from(json + "\n", "utf8");
  const body = Buffer.from(json, "utf8");
  const header = Buffer.from(`Content-Length: ${body.length}\r\n\r\n`, "ascii");
  return Buffer.concat([header, body]);
}

function runMcpSequence({ cmd, args, env, framed, messages, expectedResponses }) {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, {
      cwd: ROOT_DIR,
      env: { ...process.env, ...env },
      stdio: ["pipe", "pipe", "pipe"],
    });

    let stdoutBuf = Buffer.alloc(0);
    const stderrChunks = [];
    const responses = [];
    let settled = false;

    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      child.kill("SIGKILL");
      reject(
        new Error(
          `MCP timeout (framed=${framed}); got ${responses.length}/${expectedResponses} responses; stderr=${Buffer.concat(stderrChunks).toString("utf8")}`
        )
      );
    }, 15000);

    function parseMessages() {
      while (true) {
        if (!framed) {
          const nlIdx = stdoutBuf.indexOf(0x0a);
          if (nlIdx === -1) break;
          const line = stdoutBuf.slice(0, nlIdx).toString("utf8").trim();
          stdoutBuf = stdoutBuf.slice(nlIdx + 1);
          if (!line) continue;
          responses.push(JSON.parse(line));
        } else {
          let sepIdx = stdoutBuf.indexOf("\r\n\r\n");
          let sepLen = 4;
          if (sepIdx === -1) {
            sepIdx = stdoutBuf.indexOf("\n\n");
            sepLen = 2;
          }
          if (sepIdx === -1) break;
          const headerStr = stdoutBuf.slice(0, sepIdx).toString("ascii");
          const match = /content-length:\s*(\d+)/i.exec(headerStr);
          if (!match) {
            throw new Error(`Missing Content-Length header in MCP response: ${headerStr}`);
          }
          const len = Number(match[1]);
          const totalNeeded = sepIdx + sepLen + len;
          if (stdoutBuf.length < totalNeeded) break;
          const bodyBuf = stdoutBuf.slice(sepIdx + sepLen, totalNeeded);
          stdoutBuf = stdoutBuf.slice(totalNeeded);
          responses.push(JSON.parse(bodyBuf.toString("utf8")));
        }
      }
    }

    child.stdout.on("data", (chunk) => {
      if (settled) return;
      stdoutBuf = Buffer.concat([stdoutBuf, chunk]);
      try {
        parseMessages();
      } catch (err) {
        settled = true;
        clearTimeout(timer);
        child.kill("SIGKILL");
        reject(err);
        return;
      }
      if (responses.length >= expectedResponses) {
        child.stdin.end();
      }
    });

    child.stderr.on("data", (chunk) => stderrChunks.push(chunk));

    child.on("error", (err) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      reject(err);
    });

    child.on("close", (code) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try {
        parseMessages();
      } catch (err) {
        reject(err);
        return;
      }
      resolve({
        code,
        responses,
        stderr: Buffer.concat(stderrChunks).toString("utf8"),
      });
    });

    for (const msg of messages) {
      child.stdin.write(encodeFrame(msg, framed));
    }
    child.stdin.end();
  });
}

// ---- 1. HTTP GET -----------------------------------------------------------------

await runner.test("HTTP GET / returns 200 HTML SPA shell", async () => {
  const res = await httpReq("GET", "/");
  assert.equal(res.status, 200);
  assert.match(String(res.headers["content-type"] || ""), /text\/html/i);
  assert.match(res.text, /<!doctype html|<html/i);
  assert.match(res.text, /<div[^>]+id=["']root["']/i);
});

await runner.test("HTTP GET /api/state returns 200 JSON with required schema", async () => {
  const res = await httpReq("GET", "/api/state");
  assert.equal(res.status, 200);
  assert.match(String(res.headers["content-type"] || ""), /application\/json/i);
  assert.ok(res.json && typeof res.json === "object");
  assert.equal(typeof res.json.t, "number");
  assert.ok(Array.isArray(res.json.repos), "repos must be an array");
  assert.ok(res.json.haiku && typeof res.json.haiku === "object", "haiku must be object");
  assert.ok(res.json.quota && typeof res.json.quota === "object", "quota must be object");
  assert.ok(res.json.avance && typeof res.json.avance === "object", "avance must be object");

  if (mode === "post") {
    if (res.json.repos.length > 0) {
      assert.ok(
        Array.isArray(res.json.repos[0].windows),
        "post mode: state.repos[0].windows must be an array"
      );
    }
    let busObj = null;
    if ("bus" in res.json && res.json.bus !== undefined) {
      busObj = res.json.bus;
    } else {
      const busRes = await httpReq("GET", "/api/bus");
      assert.equal(busRes.status, 200, "post mode must expose bus in /api/state or /api/bus");
      busObj = busRes.json;
    }
    assert.ok(busObj && typeof busObj === "object", "post mode bus must be an object");
    assert.notEqual(busObj.ok, false, "post mode bus must not be ok:false stub");
    assert.ok(Array.isArray(busObj.peers), "post mode bus.peers must be an array");
    assert.ok(Array.isArray(busObj.threads), "post mode bus.threads must be an array");
  }
});

await runner.test("HTTP GET /api/events streams text/event-stream state event", async () => {
  const sse = await fetchSseFirstEvent("/api/events", 10000);
  assert.equal(sse.status, 200);
  assert.match(String(sse.headers["content-type"] || ""), /text\/event-stream/i);
  assert.equal(sse.event, "state");
  assert.ok(sse.dataJson && typeof sse.dataJson === "object", "SSE data must parse as JSON");
  assert.equal(typeof sse.dataJson.t, "number");
  assert.ok(Array.isArray(sse.dataJson.repos));
  if (mode === "post") {
    assert.ok(
      sse.ms < 100,
      `Expected post-mode SSE first event < 100 ms, got ${sse.ms} ms`
    );
  }
});

await runner.test("HTTP GET /api/activity returns 200 JSON {by_model, by_account, agents}", async () => {
  const res = await httpReq("GET", "/api/activity");
  assert.equal(res.status, 200);
  assert.ok(res.json && typeof res.json === "object");
  assert.ok(Array.isArray(res.json.by_model), "by_model must be array");
  assert.ok(Array.isArray(res.json.by_account), "by_account must be array");
  assert.ok(Array.isArray(res.json.agents), "agents must be array");
});

await runner.test("HTTP GET /api/repos returns 200 JSON array", async () => {
  const res = await httpReq("GET", "/api/repos");
  assert.equal(res.status, 200);
  assert.ok(Array.isArray(res.json), "/api/repos must return JSON array");
  if (res.json.length > 0) {
    assert.equal(typeof res.json[0].name, "string");
    assert.equal(typeof res.json[0].path, "string");
  }
});

await runner.test("HTTP GET /api/file serves allowed workspace text file", async () => {
  const res = await httpReq(
    "GET",
    `/api/file?p=app/package.json&base=${encodeURIComponent(ROOT_DIR)}`
  );
  assert.equal(res.status, 200);
  assert.match(String(res.headers["content-type"] || ""), /text\/plain/i);
  assert.ok(res.json && typeof res.json === "object", "package.json should parse");
  assert.equal(res.json.name, "app");
});

await runner.test("HTTP GET /api/real returns 200 JSON {ok:true}", async () => {
  const res = await httpReq("GET", "/api/real");
  assert.equal(res.status, 200);
  assert.ok(res.json && res.json.ok === true);
});

await runner.test("HTTP GET /api/re returns 200 JSON {ok:true, targets}", async () => {
  const res = await httpReq("GET", "/api/re");
  assert.equal(res.status, 200);
  assert.ok(res.json && res.json.ok === true);
  assert.ok(Array.isArray(res.json.targets), "targets must be array");
  assert.ok(res.json.targets.length > 0, "targets should not be empty");
});

await runner.test("HTTP GET /api/re/plain?id=__unknown__ returns 404 JSON {ok:false}", async () => {
  const res = await httpReq("GET", "/api/re/plain?id=__unknown__");
  assert.equal(res.status, 404);
  assert.ok(res.json && res.json.ok === false);
});

await runner.test("HTTP GET /api/consultor returns 200 JSON {ok:true, msgs}", async () => {
  const res = await httpReq("GET", "/api/consultor");
  assert.equal(res.status, 200);
  assert.ok(res.json && res.json.ok === true);
  assert.ok(Array.isArray(res.json.msgs), "msgs must be array");
});

if (mode === "pre") {
  const probeBus = await httpReq("GET", "/api/bus");
  if (probeBus.status === 404) {
    runner.skip("HTTP GET /api/bus", "legacy Python (pre mode) returns 404 for /api/bus");
  } else {
    await runner.test("HTTP GET /api/bus returns 200 JSON", async () => {
      assert.equal(probeBus.status, 200);
      assert.ok(probeBus.json !== null);
    });
  }
} else {
  await runner.test("HTTP GET /api/bus returns 200 JSON with peers & threads in post mode", async () => {
    const res = await httpReq("GET", "/api/bus");
    assert.equal(res.status, 200);
    assert.ok(res.json && typeof res.json === "object", "/api/bus must return valid JSON object");
    assert.notEqual(res.json.ok, false, "/api/bus must not return ok:false stub");
    assert.ok(Array.isArray(res.json.peers), "/api/bus.peers must be an array");
    assert.ok(Array.isArray(res.json.threads), "/api/bus.threads must be an array");
  });
}

// ---- 2. HTTP POST (non-destructive) ----------------------------------------------

await runner.test("HTTP POST /api/pulse {} returns 200 {ok:true}", async () => {
  const res = await httpReq("POST", "/api/pulse", { body: {} });
  assert.equal(res.status, 200);
  assert.deepEqual(res.json, { ok: true });
});

await runner.test("HTTP POST /api/consultor {} returns 409 {ok:false}", async () => {
  const res = await httpReq("POST", "/api/consultor", { body: {} });
  assert.equal(res.status, 409);
  assert.ok(res.json && res.json.ok === false);
});

await runner.test("HTTP POST /api/avance/voto unknown verdict returns 409 {ok:false}", async () => {
  const res = await httpReq("POST", "/api/avance/voto", {
    body: { repo: "__none__", key: "none", agree: true },
  });
  assert.equal(res.status, 409);
  assert.ok(res.json && res.json.ok === false);
});

await runner.test("HTTP POST /api/stop unknown id returns 409 {ok:false}", async () => {
  const res = await httpReq("POST", "/api/stop", { body: { id: "__none__" } });
  assert.equal(res.status, 409);
  assert.ok(res.json && res.json.ok === false);
});

await runner.test("HTTP POST /api/control unknown id returns 409 {ok:false}", async () => {
  const res = await httpReq("POST", "/api/control", {
    body: { id: "__none__", action: "pause" },
  });
  assert.equal(res.status, 409);
  assert.ok(res.json && res.json.ok === false);
});

// ---- 3. CLI ----------------------------------------------------------------------

await runner.test("CLI bin/bec ls exits 0", async () => {
  const r = await runCmd(path.join(ROOT_DIR, "bin", "bec"), ["ls"]);
  assert.equal(r.code, 0, `stderr: ${r.stderr}`);
});

await runner.test("CLI bin/cdx ls exits 0", async () => {
  const r = await runCmd(path.join(ROOT_DIR, "bin", "cdx"), ["ls"]);
  assert.equal(r.code, 0, `stderr: ${r.stderr}`);
});

await runner.test("CLI bin/cla ls exits 0", async () => {
  const r = await runCmd(path.join(ROOT_DIR, "bin", "cla"), ["ls"]);
  assert.equal(r.code, 0, `stderr: ${r.stderr}`);
});

await runner.test("CLI bin/sesiones-bus list exits 0 with formatted output", async () => {
  const r = await runCmd(path.join(ROOT_DIR, "bin", "sesiones-bus"), ["list"]);
  assert.equal(r.code, 0, `stderr: ${r.stderr}`);
  assert.ok(r.stdout.trim().length > 0, "expected non-empty list output");
});

await runner.test("CLI bin/sesiones-bus list --json exits 0 with valid JSON array", async () => {
  const r = await runCmd(path.join(ROOT_DIR, "bin", "sesiones-bus"), ["list", "--json"]);
  assert.equal(r.code, 0, `stderr: ${r.stderr}`);
  const parsed = JSON.parse(r.stdout);
  assert.ok(Array.isArray(parsed), "sesiones-bus list --json must return JSON array");
});

await runner.test("CLI bin/sesiones-bus --help exits 0 with usage text", async () => {
  const r = await runCmd(path.join(ROOT_DIR, "bin", "sesiones-bus"), ["--help"]);
  assert.equal(r.code, 0, `stderr: ${r.stderr}`);
  assert.match(r.stdout, /sesiones-bus/i);
  assert.match(r.stdout, /\blist\b/);
});

for (const binName of ["slice", "real-fin", "agshot", "agy-send", "agencia-up"]) {
  await runner.test(`CLI bin/${binName} exists and is executable`, async () => {
    const binPath = path.join(ROOT_DIR, "bin", binName);
    fs.accessSync(binPath, fs.constants.F_OK | fs.constants.X_OK);
  });
}

if (mode === "post") {
  await runner.test(
    "CLI bin/sesiones-bus with isolated AGENCIA_DIR supports whoami & list --json",
    async () => {
      fs.mkdirSync(RESULTS_DIR, { recursive: true });
      const tmpAgenciaDir = fs.mkdtempSync(path.join(RESULTS_DIR, "tmp-bus-cli-"));
      try {
        const who = await runCmd(
          path.join(ROOT_DIR, "bin", "sesiones-bus"),
          ["whoami", "--name", "e2e-cli-peer", "--role", "qa"],
          { env: { AGENCIA_DIR: tmpAgenciaDir } }
        );
        assert.equal(who.code, 0, `whoami stderr: ${who.stderr}`);
        const whoJson = JSON.parse(who.stdout);
        assert.ok(
          typeof whoJson.peer_id === "string" && whoJson.peer_id.length > 0,
          "whoami must return peer_id"
        );
        assert.equal(whoJson.name, "e2e-cli-peer");

        const lst = await runCmd(
          path.join(ROOT_DIR, "bin", "sesiones-bus"),
          ["list", "--json"],
          { env: { AGENCIA_DIR: tmpAgenciaDir } }
        );
        assert.equal(lst.code, 0, `list --json stderr: ${lst.stderr}`);
        const peers = JSON.parse(lst.stdout);
        assert.ok(Array.isArray(peers), "list --json must return JSON array");
        assert.ok(
          peers.some((p) => p.peer_id === whoJson.peer_id),
          "isolated list --json must contain registered peer"
        );
      } finally {
        fs.rmSync(tmpAgenciaDir, { recursive: true, force: true });
      }
    }
  );
}

// ---- 4. MCP stdio ----------------------------------------------------------------

const mcpSpec = resolveMcpCommand();
const canMutateMcpTools =
  mode === "post" ||
  mcpSpec.isolatedSupported ||
  process.env.AGENCIA_MCP_TEST_LIVE === "1";

for (const framed of [false, true]) {
  const label = framed ? "Content-Length" : "newline-JSON";

  await runner.test(
    `MCP stdio (${label}): initialize, notifications/initialized, ping, tools/list${canMutateMcpTools ? ", tools/call(bus_whoami, bus_list)" : ""}`,
    async () => {
      fs.mkdirSync(RESULTS_DIR, { recursive: true });
      const tmpAgenciaDir = fs.mkdtempSync(path.join(RESULTS_DIR, "tmp-mcp-"));
      try {
        const env = canMutateMcpTools ? { AGENCIA_DIR: tmpAgenciaDir } : {};
        const messages = [
          {
            jsonrpc: "2.0",
            id: 1,
            method: "initialize",
            params: {
              protocolVersion: "2024-11-05",
              capabilities: {},
              clientInfo: { name: "agencia-e2e", version: "1.0.0" },
            },
          },
          {
            jsonrpc: "2.0",
            method: "notifications/initialized",
            params: {},
          },
          {
            jsonrpc: "2.0",
            id: 2,
            method: "ping",
            params: {},
          },
          {
            jsonrpc: "2.0",
            id: 3,
            method: "tools/list",
            params: {},
          },
        ];

        if (canMutateMcpTools) {
          messages.push(
            {
              jsonrpc: "2.0",
              id: 4,
              method: "tools/call",
              params: {
                name: "bus_whoami",
                arguments: { name: "e2e-mcp-peer", role: "qa" },
              },
            },
            {
              jsonrpc: "2.0",
              id: 5,
              method: "tools/call",
              params: {
                name: "bus_list",
                arguments: {},
              },
            }
          );
        }

        messages.push({
          jsonrpc: "2.0",
          id: 99,
          method: "agencia/nonexistent_method",
          params: {},
        });

        const expectedResponses = canMutateMcpTools ? 6 : 4;
        const res = await runMcpSequence({
          cmd: mcpSpec.cmd,
          args: mcpSpec.args,
          env,
          framed,
          messages,
          expectedResponses,
        });

        assert.equal(res.code, 0, `MCP exited ${res.code}; stderr=${res.stderr}`);
        assert.equal(
          res.responses.length,
          expectedResponses,
          `Expected ${expectedResponses} JSON-RPC responses (notification must yield none), got ${res.responses.length}`
        );

        const initResp = res.responses.find((r) => r.id === 1);
        assert.ok(initResp && initResp.result, "initialize response missing");
        assert.equal(initResp.result.serverInfo?.name, "sesiones-bus");
        assert.ok(initResp.result.capabilities?.tools !== undefined);

        const pingResp = res.responses.find((r) => r.id === 2);
        assert.ok(pingResp && pingResp.result !== undefined && !pingResp.error, "ping failed");

        const listResp = res.responses.find((r) => r.id === 3);
        assert.ok(listResp && listResp.result && Array.isArray(listResp.result.tools));
        const toolNames = listResp.result.tools.map((t) => t.name);
        assert.equal(toolNames.length, 8, `Expected 8 MCP tools, got ${toolNames.length}`);
        for (const reqTool of [
          "bus_list",
          "bus_whoami",
          "bus_send",
          "bus_reply",
          "bus_inbox",
          "bus_thread",
          "bus_wait",
          "bus_close",
        ]) {
          assert.ok(toolNames.includes(reqTool), `Missing MCP tool: ${reqTool}`);
        }

        const unknownResp = res.responses.find((r) => r.id === 99);
        assert.ok(unknownResp && unknownResp.error, "unknown method must return JSON-RPC error");
        assert.equal(unknownResp.error.code, -32601);

        if (canMutateMcpTools) {
          const whoamiResp = res.responses.find((r) => r.id === 4);
          assert.ok(whoamiResp && whoamiResp.result && whoamiResp.result.isError === false);
          const whoamiJson = JSON.parse(whoamiResp.result.content[0].text);
          assert.ok(typeof whoamiJson.peer_id === "string" && whoamiJson.peer_id.length > 0);
          assert.equal(whoamiJson.name, "e2e-mcp-peer");

          const busListResp = res.responses.find((r) => r.id === 5);
          assert.ok(busListResp && busListResp.result && busListResp.result.isError === false);
          const busListJson = JSON.parse(busListResp.result.content[0].text);
          assert.ok(Array.isArray(busListJson.peers));
          assert.ok(busListJson.count >= 1);
        }
      } finally {
        fs.rmSync(tmpAgenciaDir, { recursive: true, force: true });
      }
    }
  );
}

if (!canMutateMcpTools) {
  runner.skip(
    "MCP stdio tools/call (bus_whoami, bus_list)",
    "pre mode bus_mcp.py lacks AGENCIA_DIR isolation; skipped to avoid polluting live cache/bus.db (set AGENCIA_MCP_TEST_LIVE=1 to force)"
  );
}

runner.finish();
