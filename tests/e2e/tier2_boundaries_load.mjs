import assert from "node:assert/strict";
import {
  BASE_URL,
  ROOT_DIR,
  SuiteRunner,
  detectMode,
  fetchSseFirstEvent,
  httpReq,
} from "./lib/common.mjs";

const mode = detectMode(BASE_URL);
const runner = SuiteRunner("tier2_boundaries_load", mode);

function assertClientError(res, expectedStatus, label) {
  assert.ok(
    res.status >= 400 && res.status < 500,
    `${label}: expected 4xx, got ${res.status} (${res.text.slice(0, 120)})`
  );
  if (expectedStatus !== undefined) {
    assert.equal(
      res.status,
      expectedStatus,
      `${label}: expected HTTP ${expectedStatus}, got ${res.status}`
    );
  }
}

// ---- 1. /api/file path traversal, dotfiles & disallowed extensions ---------------

const fileRejections = [
  ["relative traversal ../../etc/passwd", `/api/file?p=../../etc/passwd&base=${encodeURIComponent(ROOT_DIR)}`],
  ["absolute outside HOME /etc/passwd", `/api/file?p=${encodeURIComponent("/etc/passwd")}`],
  ["hidden dotdir ~/.ssh/id_rsa", `/api/file?p=${encodeURIComponent("~/.ssh/id_rsa")}`],
  ["hidden .git/config", `/api/file?p=.git/config&base=${encodeURIComponent(ROOT_DIR)}`],
  ["binary sqlite cache/bus.db", `/api/file?p=${encodeURIComponent(`${ROOT_DIR}/cache/bus.db`)}`],
  ["empty p param", `/api/file?p=`],
];

for (const [label, urlPath] of fileRejections) {
  await runner.test(`GET /api/file rejects ${label} with 404 (no 200/5xx)`, async () => {
    const res = await httpReq("GET", urlPath);
    assertClientError(res, 404, label);
    assert.ok(res.json && res.json.ok === false, `${label}: expected {ok:false}`);
  });
}

// ---- 2. /api/repofile traversal, bad repo, bad ext -------------------------------

const repofileRejections = [
  ["traversal ../../etc/passwd", "/api/repofile?repo=agencia&p=../../etc/passwd"],
  ["parent escape ../agencia.py", "/api/repofile?repo=agencia&p=../agencia.py"],
  ["unknown repo __bad__", "/api/repofile?repo=__bad__&p=x.png"],
  ["disallowed ext x.txt", "/api/repofile?repo=agencia&p=x.txt"],
];

for (const [label, urlPath] of repofileRejections) {
  await runner.test(`GET /api/repofile rejects ${label} with 404 (no 5xx)`, async () => {
    const res = await httpReq("GET", urlPath);
    assertClientError(res, 404, label);
  });
}

// ---- 3. /api/shot bad id / index -------------------------------------------------

for (const [label, urlPath] of [
  ["traversal id job:../etc", "/api/shot?id=job:../etc&index=0"],
  ["malformed id=bad", "/api/shot?id=bad&index=0"],
  ["negative index", "/api/shot?id=job:nonexistent123&index=-1"],
]) {
  await runner.test(`GET /api/shot rejects ${label} with 4xx (no 5xx)`, async () => {
    const res = await httpReq("GET", urlPath);
    assertClientError(res, 404, label);
  });
}

// ---- 4. POST CSRF & malformed body -----------------------------------------------

await runner.test("POST CSRF rejects evil Origin header with 403", async () => {
  const res = await httpReq("POST", "/api/pulse", {
    headers: { Origin: "http://evil.example:9999" },
    body: {},
  });
  assert.equal(res.status, 403);
});

await runner.test("POST rejects invalid JSON {bad with 400", async () => {
  const res = await httpReq("POST", "/api/pulse", {
    body: "{bad",
  });
  assert.equal(res.status, 400);
});

await runner.test("POST rejects JSON array [] with 400", async () => {
  const res = await httpReq("POST", "/api/pulse", {
    body: "[]",
  });
  assert.equal(res.status, 400);
});

await runner.test("POST unknown route returns 404", async () => {
  const res = await httpReq("POST", "/api/nonexistent_route_e2e", {
    body: {},
  });
  assert.equal(res.status, 404);
});

// ---- 5. Invalid /api/launch inputs -> 400, ZERO 5xx ------------------------------

const invalidLaunchCases = [
  ["kind=bad", { kind: "bad", text: "valid prompt text", repo: ROOT_DIR }],
  ["kind=cdx, home=unknown_profile", { kind: "cdx", home: "unknown_profile", text: "valid prompt text", repo: ROOT_DIR }],
  ["empty text", { kind: "bec", text: "", repo: ROOT_DIR }],
  ["text > 12000 chars", { kind: "bec", text: "x".repeat(12001), repo: ROOT_DIR }],
  ["repo outside HOME (/etc)", { kind: "bec", text: "valid prompt text", repo: "/etc" }],
  ["invalid effort=ultra", { kind: "cdx", text: "valid prompt text", repo: ROOT_DIR, effort: "ultra" }],
  ["kind=cla empty text", { kind: "cla", text: "", repo: ROOT_DIR }],
  ["kind=cla repo outside HOME (/etc)", { kind: "cla", text: "valid prompt text", repo: "/etc" }],
];

for (const [label, payload] of invalidLaunchCases) {
  await runner.test(`POST /api/launch rejects ${label} with 400 (no 5xx)`, async () => {
    const res = await httpReq("POST", "/api/launch", { body: payload });
    assertClientError(res, 400, label);
    assert.ok(res.json && res.json.ok === false, `${label}: expected {ok:false}`);
  });
}

// ---- 6. Unknown IDs / missing params ---------------------------------------------

await runner.test("GET /api/agent?id=nonexistent:123 returns 404", async () => {
  const res = await httpReq("GET", "/api/agent?id=nonexistent:123");
  assertClientError(res, 404, "agent nonexistent");
});

await runner.test("GET /api/agent?id=../../etc returns 404", async () => {
  const res = await httpReq("GET", "/api/agent?id=../../etc");
  assertClientError(res, 404, "agent traversal");
});

await runner.test("GET /api/agent without id returns 404", async () => {
  const res = await httpReq("GET", "/api/agent");
  assertClientError(res, 404, "agent missing id");
});

await runner.test("POST /api/codex/open {id:'bad'} returns 404 (no 5xx)", async () => {
  const res = await httpReq("POST", "/api/codex/open", { body: { id: "bad" } });
  assertClientError(res, 404, "codex open bad");
  assert.ok(res.json && res.json.ok === false);
});

await runner.test("POST /api/codex/clear {id:'bad'} returns 404 (no 5xx)", async () => {
  const res = await httpReq("POST", "/api/codex/clear", { body: { id: "bad" } });
  assertClientError(res, 404, "codex clear bad");
  assert.ok(res.json && res.json.ok === false);
});

await runner.test("POST /api/send missing/bad target returns 4xx or 502 (zero 500)", async () => {
  const res = await httpReq("POST", "/api/send", {
    body: { to: "__nonexistent__:bad", text: "ping" },
  });
  assert.ok(
    (res.status >= 400 && res.status < 500) || res.status === 502,
    `send bad target: expected 4xx or 502, got ${res.status} (${res.text.slice(0, 120)})`
  );
  assert.notEqual(res.status, 500, "send bad target must never return HTTP 500");
  assert.ok(res.json && res.json.ok === false, "send bad target expected {ok:false}");
});

// ---- 7. Concurrency burst --------------------------------------------------------

await runner.test("Concurrency burst: 20 GET /api/state + 5 GET /api/events + 10 POST /api/pulse + 5 GET /api/activity + 5 GET /api/re + 5 GET /api/agent(nonexistent) (zero 5xx)", async () => {
  const stateTasks = Array.from({ length: 20 }, () => httpReq("GET", "/api/state", { timeoutMs: 25000 }));
  const sseTasks = Array.from({ length: 5 }, () => fetchSseFirstEvent("/api/events", 15000));
  const pulseTasks = Array.from({ length: 10 }, () => httpReq("POST", "/api/pulse", { body: {}, timeoutMs: 25000 }));
  const activityTasks = Array.from({ length: 5 }, () => httpReq("GET", "/api/activity", { timeoutMs: 25000 }));
  const reTasks = Array.from({ length: 5 }, () => httpReq("GET", "/api/re", { timeoutMs: 25000 }));
  const agentMissTasks = Array.from({ length: 5 }, () => httpReq("GET", "/api/agent?id=__nonexistent__:123", { timeoutMs: 25000 }));

  const [states, sses, pulses, activities, resList, agentMisses] = await Promise.all([
    Promise.all(stateTasks),
    Promise.all(sseTasks),
    Promise.all(pulseTasks),
    Promise.all(activityTasks),
    Promise.all(reTasks),
    Promise.all(agentMissTasks),
  ]);

  for (let i = 0; i < states.length; i++) {
    assert.ok(states[i].status < 500, `state[${i}] 5xx: ${states[i].status}`);
    assert.equal(states[i].status, 200, `state[${i}] status`);
    assert.ok(states[i].json && typeof states[i].json.t === "number", `state[${i}] valid json`);
  }
  for (let i = 0; i < sses.length; i++) {
    assert.equal(sses[i].status, 200, `sse[${i}] status`);
    assert.equal(sses[i].event, "state", `sse[${i}] event`);
    assert.ok(sses[i].dataJson && typeof sses[i].dataJson.t === "number", `sse[${i}] json`);
  }
  for (let i = 0; i < pulses.length; i++) {
    assert.ok(pulses[i].status < 500, `pulse[${i}] 5xx: ${pulses[i].status}`);
    assert.equal(pulses[i].status, 200, `pulse[${i}] status`);
    assert.deepEqual(pulses[i].json, { ok: true }, `pulse[${i}] body`);
  }
  for (let i = 0; i < activities.length; i++) {
    assert.ok(activities[i].status < 500, `activity[${i}] 5xx: ${activities[i].status}`);
    assert.equal(activities[i].status, 200, `activity[${i}] status`);
    assert.ok(activities[i].json && Array.isArray(activities[i].json.agents), `activity[${i}] valid json`);
  }
  for (let i = 0; i < resList.length; i++) {
    assert.ok(resList[i].status < 500, `re[${i}] 5xx: ${resList[i].status}`);
    assert.equal(resList[i].status, 200, `re[${i}] status`);
    assert.ok(resList[i].json && resList[i].json.ok === true && Array.isArray(resList[i].json.targets), `re[${i}] valid json`);
  }
  for (let i = 0; i < agentMisses.length; i++) {
    assert.ok(agentMisses[i].status < 500, `agentMiss[${i}] 5xx: ${agentMisses[i].status}`);
    assert.equal(agentMisses[i].status, 404, `agentMiss[${i}] status`);
  }
});

runner.finish();
