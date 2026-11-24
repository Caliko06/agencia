//! Port of avance.py: is each repo actually reaching its GOAL.md milestone? One Sonnet auditor per repo.
//! Source of truth: GOAL.md and web/status/board.json (else the live run's board from jobs + plan.json).
//! Contract signatures are fixed. Python is ground truth for behavior and JSON shape.
use agencia_core::jobs::{JobManager, JobRecord, RealRun};
use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::UNIX_EPOCH;

const QUIET: f64 = 60.0;
const PER_HOUR: usize = 6;
const LOOK: f64 = 20.0;

// ---------- persistent state (cache/avance.json: {"audit":..,"owner":..}) ----------
#[derive(Clone, Default, Serialize, Deserialize)]
struct Audit {
    #[serde(default)]
    sig: String,
    #[serde(default)]
    v: String,
    #[serde(default)]
    claim: String,
    #[serde(default)]
    why: String,
    #[serde(default)]
    t: f64,
    #[serde(default)]
    shot: String,
    #[serde(default)]
    redo: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Owner {
    #[serde(default)]
    agree: bool,
    #[serde(default)]
    v: String,
    #[serde(default)]
    why: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    t: f64,
    #[serde(default)]
    at: f64,
}

#[derive(Clone, Default)]
struct St {
    sig: Option<String>,
    seen: f64,
    busy: bool,
    pending: bool,
    err: String,
}

#[derive(Default)]
struct State {
    runs: HashMap<String, Vec<f64>>,
    audit: HashMap<String, HashMap<String, Audit>>,
    owner: HashMap<String, HashMap<String, Owner>>,
    state: HashMap<String, St>,
}

fn store_path() -> PathBuf {
    crate::shared::cache_dir().join("avance.json")
}

fn load_state() -> State {
    let mut st = State::default();
    let Ok(b) = fs::read(store_path()) else {
        return st;
    };
    let Ok(v) = serde_json::from_slice::<Value>(&b) else {
        return st;
    };
    if let Some(a) = v.get("audit")
        && let Ok(m) = serde_json::from_value(a.clone())
    {
        st.audit = m;
    }
    if let Some(o) = v.get("owner")
        && let Ok(m) = serde_json::from_value(o.clone())
    {
        st.owner = m;
    }
    st
}

fn save_state(s: &State) {
    let dir = crate::shared::cache_dir();
    let _ = fs::create_dir_all(&dir);
    let v = json!({"audit": s.audit, "owner": s.owner});
    let Ok(txt) = serde_json::to_vec_pretty(&v) else {
        return;
    };
    let tmp = dir.join("avance.json.tmp");
    let dest = dir.join("avance.json");
    if fs::write(&tmp, txt).is_ok() {
        let _ = fs::rename(&tmp, &dest);
    }
}

static S: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(load_state()));

static JOBS: LazyLock<JobManager> = LazyLock::new(|| JobManager::new(crate::shared::root()));
static JOBS_CACHE: LazyLock<Mutex<(f64, Vec<JobRecord>)>> = LazyLock::new(|| Mutex::new((0.0, Vec::new())));

/// J.jobs() asks `ps` about every running job; the Tablero only needs it fresh to 5 s.
fn jobs_now() -> Vec<JobRecord> {
    let mut c = JOBS_CACHE.lock();
    if crate::shared::now() - c.0 > 5.0 {
        c.1 = JOBS.list_jobs();
        c.0 = crate::shared::now();
    }
    c.1.clone()
}

// ---------- path/number helpers ----------
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn realpath(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    fs::canonicalize(&abs).unwrap_or_else(|_| normalize(&abs))
}

fn realpath_s(p: &str) -> PathBuf {
    realpath(Path::new(p))
}

fn mt(p: &Path) -> f64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn relpath(target: &Path, base: &Path) -> String {
    let t = normalize(target);
    let b = normalize(base);
    let tc: Vec<_> = t.components().collect();
    let bc: Vec<_> = b.components().collect();
    let mut i = 0;
    while i < tc.len() && i < bc.len() && tc[i] == bc[i] {
        i += 1;
    }
    let mut out = PathBuf::new();
    for _ in i..bc.len() {
        out.push("..");
    }
    for c in &tc[i..] {
        out.push(c.as_os_str());
    }
    let s = out.to_string_lossy().into_owned();
    if s.is_empty() { ".".to_string() } else { s }
}

fn numf(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(0.0)
}

/// Python's str() of a JSON number: ints as ints, floats via shortest repr.
fn pystr(v: Option<&Value>) -> String {
    match v {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => if *b { "True".into() } else { "False".into() },
        _ => "".into(),
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|x| x != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn vstr(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or("").to_string()
}

/// A Python list repr, for prompt strings (`['a', 'b']`).
fn py_list(xs: &[String]) -> String {
    let items: Vec<String> = xs
        .iter()
        .map(|x| format!("'{}'", x.replace('\\', "\\\\").replace('\'', "\\'")))
        .collect();
    format!("[{}]", items.join(", "))
}

// ---------- sh / claude ----------
fn sh(cmd: &[&str], cwd: &Path) -> String {
    let out = Command::new(cmd[0])
        .args(&cmd[1..])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => String::new(),
    }
}

fn config_dir(account: u32) -> Option<String> {
    if account == 0 || account == 3 {
        return None;
    }
    let home = crate::shared::home();
    if account == 1 {
        return Some(format!("{home}/.claude-profiles/3"));
    }
    let map: HashMap<String, Value> = fs::read(format!("{home}/.config/claude-acc/accounts.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let val = match map.get(&account.to_string()) {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => account.to_string(),
    };
    Some(format!("{home}/.claude-profiles/{val}"))
}

fn claude(model: &str, prompt: &str, cwd: &Path, account: u32, tools: &str) -> Result<String, String> {
    let mut cmd = Command::new("claude");
    cmd.arg("-p")
        .arg("--model")
        .arg(model)
        .arg("--tools")
        .arg(tools);
    if !tools.is_empty() {
        cmd.arg("--allowedTools")
            .arg("Read Glob Grep Bash(git log:*) Bash(git show:*)");
    }
    cmd.arg("--no-session-persistence")
        .arg("--disable-slash-commands")
        .arg("--setting-sources")
        .arg("")
        .arg("--strict-mcp-config")
        .arg("--permission-mode")
        .arg("dontAsk")
        .arg(prompt);
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, _) in std::env::vars() {
        if k.starts_with("CLAUDE") || k.starts_with("ANTHROPIC_AUTH") {
            cmd.env_remove(&k);
        }
    }
    if let Some(cd) = config_dir(account) {
        cmd.env("CLAUDE_CONFIG_DIR", cd);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = if out.stderr.is_empty() { &out.stdout } else { &out.stderr };
        let s = String::from_utf8_lossy(err);
        let n = s.chars().count();
        return Err(s.chars().skip(n.saturating_sub(200)).collect());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ---------- watched repos ----------
/// Repos the Tablero follows: those with GOAL.md + board.json, and every repo with a live Agencia Real run.
fn watched() -> BTreeMap<String, PathBuf> {
    let mut out: BTreeMap<String, PathBuf> = BTreeMap::new();
    out.insert(
        "mafia-wasm".to_string(),
        PathBuf::from(format!("{}/mafia-wasm", crate::shared::home())),
    );
    for k in agencia_core::jobs::real_runs(true).keys() {
        let p = PathBuf::from(k);
        if p.is_dir()
            && let Some(base) = p.file_name().and_then(|s| s.to_str())
        {
            out.insert(base.to_string(), p);
        }
    }
    out
}

// ---------- board ----------
static REF_RX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\w./-]+\.(?:md|tsv|txt|log|json|c|h|py|rs|ts|tsx|js|mjs|csv|png|jpg)\b").unwrap()
});
static RESUMEN_RX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"RESUMEN:\s*(.+)").unwrap());

fn is_ref_boundary(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '/' || c == '.' || c == '-'
}

fn refs_of(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for m in REF_RX.find_iter(text) {
        let start = m.start();
        if start > 0 {
            let prev = text[..start].chars().next_back().unwrap_or(' ');
            if is_ref_boundary(prev) {
                continue;
            }
        }
        out.push(m.as_str().to_string());
    }
    out
}

fn cited_files(reply: &str, jr: &Path, root: &Path) -> String {
    let mut out: Vec<String> = Vec::new();
    let rs = format!("{}/", root.display());
    let js = format!("{}/", jr.display());
    for m in refs_of(reply) {
        for base in [jr, root] {
            let f = if m.starts_with('/') {
                realpath_s(&m)
            } else {
                realpath(&base.join(&m))
            };
            let fs_ = f.to_string_lossy();
            if (fs_.starts_with(&rs) || fs_.starts_with(&js)) && f.is_file() {
                let rel = relpath(&f, root);
                if !out.contains(&rel) {
                    out.push(rel);
                }
                break;
            }
        }
        if out.len() >= 4 {
            break;
        }
    }
    out.join(", ")
}

fn real_field<'a>(r: &'a RealRun, k: &str) -> Option<&'a Value> {
    r.extra.get(k)
}

fn at_secs(v: Option<&Value>) -> f64 {
    let Some(Value::String(s)) = v else {
        return 0.0;
    };
    let s = s.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&s.replace('Z', "+00:00")) {
        return dt.timestamp() as f64 + dt.timestamp_subsec_micros() as f64 / 1e6;
    }
    if let Ok(nd) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        && let Some(dt) = nd.and_local_timezone(chrono::Local).single()
    {
        return dt.timestamp() as f64 + dt.timestamp_subsec_micros() as f64 / 1e6;
    }
    0.0
}

fn real_for(root: &Path) -> Option<RealRun> {
    let runs = agencia_core::jobs::real_runs(true);
    let rp = realpath(root);
    let key = rp.to_string_lossy().into_owned();
    if let Some(r) = runs.get(&key) {
        return Some(r.clone());
    }
    for (k, v) in &runs {
        if realpath_s(k) == rp {
            return Some(v.clone());
        }
    }
    None
}

fn state_of(status: &str) -> &'static str {
    match status {
        "ok" => "done",
        "fallo" => "fail",
        "pregunta" => "blocked",
        "r" | "q" => "work",
        _ => "work",
    }
}

fn first_line_of(reply: &str) -> Option<String> {
    RESUMEN_RX
        .captures(reply)
        .and_then(|c| c.get(1).map(|g| g.as_str().trim().to_string()))
}

/// A board for a repo with no board.json: the live run's goal is the one milestone.
fn run_board(root: &Path) -> Option<Value> {
    let root = realpath(root);
    let r = real_for(&root)?;
    let t0 = at_secs(r.at.as_ref());
    let goal_txt = real_field(&r, "goal").and_then(Value::as_str).unwrap_or("").to_string();
    let mut plan: Vec<Value> = Vec::new();
    let mut workers: Map<String, Value> = Map::new();
    let mut updated = t0;
    let root_s = root.to_string_lossy().into_owned();
    for j in jobs_now() {
        let jr = realpath_s(&j.repo);
        if j.ts < t0 {
            continue;
        }
        let jrs = jr.to_string_lossy();
        if !(jrs == root_s.as_str()
            || jrs.starts_with(&format!("{root_s}/"))
            || jrs.starts_with(&format!("{root_s}-wt/")))
        {
            continue;
        }
        let reply = &j.reply;
        let mut line = match first_line_of(reply) {
            Some(l) => l.chars().take(240).collect::<String>(),
            None => reply.trim().chars().take(240).collect::<String>(),
        };
        if line.is_empty() {
            line = j.acts.last().cloned().unwrap_or_default().chars().take(240).collect();
        }
        let who = if j.kind == "bec" {
            "Becario".to_string()
        } else if j.home.is_empty() || j.home == "default" {
            "Codex".to_string()
        } else {
            format!("Codex {}", j.home)
        };
        let title = if j.brief.is_empty() {
            j.text.chars().take(90).collect::<String>()
        } else {
            j.brief.clone()
        };
        let id = format!("job:{}", j.id);
        plan.push(json!({
            "id": id,
            "who": who,
            "kind": if j.kind == "bec" { "bec" } else { "codex" },
            "title": title,
            "line": id,
            "milestone": "Corrida",
            "done_when": goal_txt,
        }));
        let t = if j.ended != 0.0 {
            j.ended
        } else if j.updated != 0.0 {
            j.updated
        } else {
            j.ts
        };
        updated = updated.max(t);
        let ev = cited_files(reply, &jr, &root);
        workers.insert(
            id.clone(),
            json!({"lines": [{"state": state_of(&j.status), "line": line, "t": t, "ev": ev}]}),
        );
    }
    let plan_path = root.join("web/plan/plan.json");
    if let Ok(raw) = fs::read(&plan_path)
        && let Ok(pv) = serde_json::from_slice::<Value>(&raw)
    {
        let pm = mt(&plan_path);
        if let Some(steps) = pv.get("steps").and_then(Value::as_array) {
            for st in steps {
                if let Some(checks) = st.get("checks").and_then(Value::as_array) {
                    for (i, c) in checks.iter().enumerate() {
                        let k = format!("plan:{}.{}", pystr(st.get("n")), i);
                        let ev0 = c.get("ev").and_then(Value::as_str).unwrap_or("");
                        let ev = if !ev0.is_empty() && root.join("web/plan").join(ev0).exists() {
                            format!("web/plan/{ev0}")
                        } else {
                            ev0.to_string()
                        };
                        let who = c
                            .get("who")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .unwrap_or("?");
                        let title = format!(
                            "{}: {}",
                            vstr(st.get("name")),
                            vstr(c.get("do"))
                        );
                        plan.push(json!({
                            "id": k, "who": who, "kind": "session", "title": title,
                            "line": k, "milestone": "Corrida", "done_when": goal_txt,
                        }));
                        let state = if truthy(c.get("done").unwrap_or(&Value::Null)) {
                            "pass"
                        } else {
                            "work"
                        };
                        workers.insert(
                            k.clone(),
                            json!({"lines": [{"state": state, "line": vstr(c.get("state")), "t": pm, "ev": ev}]}),
                        );
                    }
                }
            }
            updated = updated.max(pm);
        }
    }
    Some(json!({
        "goal": [{"id": "Corrida", "state": "OPEN", "done_when": goal_txt, "sees": ""}],
        "plan": plan,
        "workers": workers,
        "updated": updated,
    }))
}

fn board(root: &Path) -> Option<Value> {
    if let Ok(b) = fs::read(root.join("web/status/board.json"))
        && let Ok(v) = serde_json::from_slice::<Value>(&b)
    {
        return Some(v);
    }
    run_board(root)
}


/// Compare two audit signatures: serde_json parses floats without `float_roundtrip`, so a
/// timestamp can land one ulp away from Python's; compare the numeric head loosely.
fn sig_eq(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let (ta, ea) = a.split_once('|').unwrap_or((a, ""));
    let (tb, eb) = b.split_once('|').unwrap_or((b, ""));
    if ea != eb {
        return false;
    }
    match (ta.parse::<f64>(), tb.parse::<f64>()) {
        (Ok(x), Ok(y)) => (x - y).abs() < 1e-3,
        _ => false,
    }
}

// ---------- items ----------
const FINAL: [&str; 5] = ["pass", "done", "fail", "fix", "blocked"];

#[derive(Clone, Default)]
struct Item {
    key: String,
    kind: String,
    who: String,
    akind: String,
    title: String,
    line: String,
    ms: String,
    done_when: String,
    done: String,
    lines: Vec<Map<String, Value>>,
    last: Option<Map<String, Value>>,
    t0: f64,
    ready: bool,
    sig: String,
    ev: Option<String>,
}

/// A worker belongs to a plan line when its name is the prefix or the prefix plus a number.
fn mine(name: &str, prefix: &str) -> bool {
    if name == prefix {
        return true;
    }
    if let Some(rest) = name.strip_prefix(prefix) {
        return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
    }
    false
}

fn lines_of(workers: &Map<String, Value>, prefix: &str) -> Vec<Map<String, Value>> {
    let mut out: Vec<Map<String, Value>> = Vec::new();
    for (n, w) in workers {
        if !mine(n, prefix) {
            continue;
        }
        if let Some(lines) = w.get("lines").and_then(Value::as_array) {
            for l in lines {
                if let Some(m) = l.as_object() {
                    let mut mm = m.clone();
                    mm.insert("who".to_string(), json!(n));
                    out.push(mm);
                }
            }
        }
    }
    out.sort_by(|a, b| numf(a.get("t")).total_cmp(&numf(b.get("t"))));
    out
}

fn items(b: &Value) -> Vec<Item> {
    let workers = b
        .get("workers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let goal: Map<String, Value> = b
        .get("goal")
        .and_then(Value::as_array)
        .map(|gs| {
            gs.iter()
                .filter_map(|g| g.get("id").and_then(Value::as_str).map(|id| (id.to_string(), g.clone())))
                .collect()
        })
        .unwrap_or_default();
    let todo: Map<String, Value> = b
        .get("todo")
        .and_then(Value::as_array)
        .map(|ts| {
            ts.iter()
                .filter_map(|t| t.get("id").and_then(Value::as_str).map(|id| (id.to_string(), t.clone())))
                .collect()
        })
        .unwrap_or_default();
    let mut out = Vec::new();
    if let Some(plan) = b.get("plan").and_then(Value::as_array) {
        for p in plan {
            let line_pref = p
                .get("line")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("\0")
                .to_string();
            let ls = lines_of(&workers, &line_pref);
            let last = ls.last().cloned();
            let mut ms = p
                .get("milestone")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if ms.is_empty()
                && let Some(tid) = p.get("todo").and_then(Value::as_str)
                && let Some(t) = todo.get(tid)
            {
                ms = t.get("ms").and_then(Value::as_str).unwrap_or("").to_string();
            }
            let mut done_when = p
                .get("done_when")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if done_when.is_empty()
                && let Some(g) = goal.get(&ms)
            {
                done_when = g.get("done_when").and_then(Value::as_str).unwrap_or("").to_string();
            }
            let t0 = ls.first().map(|l| numf(l.get("t"))).unwrap_or(0.0);
            let ready = last
                .as_ref()
                .and_then(|l| l.get("state"))
                .and_then(Value::as_str)
                .is_some_and(|s| FINAL.contains(&s));
            let sig = match &last {
                Some(l) => format!(
                    "{}|{}",
                    pystr(l.get("t")),
                    l.get("ev").and_then(Value::as_str).unwrap_or("")
                ),
                None => String::new(),
            };
            out.push(Item {
                key: format!("task:{}", vstr(p.get("id"))),
                kind: "task".into(),
                who: p.get("who").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("?").to_string(),
                akind: vstr(p.get("kind")),
                title: vstr(p.get("title")),
                line: vstr(p.get("line")),
                ms,
                done_when,
                done: vstr(p.get("done")),
                lines: ls.iter().rev().take(6).rev().cloned().collect(),
                last,
                t0,
                ready,
                sig,
                ev: None,
            });
        }
    }
    if let Some(gs) = b.get("goal").and_then(Value::as_array) {
        for g in gs {
            if g.get("state").and_then(Value::as_str) != Some("VERIFIED") {
                continue;
            }
            let id = vstr(g.get("id"));
            let state = vstr(g.get("state"));
            let ev = vstr(g.get("ev"));
            out.push(Item {
                key: format!("gate:{id}"),
                kind: "gate".into(),
                who: "Orquestador".into(),
                akind: "session".into(),
                title: id.clone(),
                line: String::new(),
                ms: id,
                done_when: vstr(g.get("done_when")),
                done: vstr(g.get("sees")),
                lines: Vec::new(),
                last: None,
                t0: 0.0,
                ready: true,
                sig: format!("{state}|{ev}"),
                ev: Some(ev),
            });
        }
    }
    out
}

// ---------- evidence ----------
fn evidence(root: &Path, it: &Item) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut raw: Vec<String> = Vec::new();
    for l in &it.lines {
        if let Some(ev) = l.get("ev").and_then(Value::as_str)
            && !ev.is_empty()
        {
            raw.push(ev.to_string());
        }
    }
    if let Some(ev) = &it.ev
        && !ev.is_empty()
    {
        raw.push(ev.clone());
    }
    let mut cited_set: HashSet<String> = HashSet::new();
    for chunk in &raw {
        for p in chunk.split(',') {
            let p = p.trim();
            if !p.is_empty() {
                cited_set.insert(p.to_string());
            }
        }
    }
    let mut cited: Vec<String> = cited_set.into_iter().collect();
    cited.sort();
    if it.kind == "gate" {
        return (cited, Vec::new(), Vec::new());
    }
    let t0 = it.t0 - 300.0;
    let since = format!("--since=@{}", t0 as i64);
    let commits: Vec<String> = sh(&["git", "log", &since, "--format=%h %ad %s", "--date=format:%H:%M"], root)
        .split('\n')
        .map(str::to_string)
        .filter(|l| !l.trim().is_empty())
        .take(8)
        .collect();
    let root_s = root.to_string_lossy().into_owned();
    let mut shot_paths: Vec<PathBuf> = Vec::new();
    for pat in ["web/status/**/*.png", "web/status/**/*.jpg"] {
        let full = format!("{root_s}/{pat}");
        if let Ok(ps) = glob::glob(&full) {
            for p in ps.flatten() {
                if mt(&p) > t0 {
                    shot_paths.push(p);
                }
            }
        }
    }
    let uniq: HashSet<PathBuf> = shot_paths.into_iter().collect();
    let mut shots: Vec<PathBuf> = uniq.into_iter().collect();
    shots.sort_by(|a, b| mt(b).total_cmp(&mt(a)));
    shots.truncate(4);
    let shots_rel = shots.iter().map(|p| relpath(p, root)).collect();
    (cited, commits, shots_rel)
}

// ---------- calibration ----------
fn calibration(repo: &str) -> String {
    let st = S.lock();
    let mut o: Vec<Owner> = st
        .owner
        .get(repo)
        .map(|m| m.values().cloned().collect())
        .unwrap_or_default();
    drop(st);
    o.sort_by(|a, b| a.t.total_cmp(&b.t));
    let no: Vec<&Owner> = o.iter().filter(|x| !x.agree).collect();
    let yes: Vec<&Owner> = o.iter().filter(|x| x.agree).collect();
    let no = &no[no.len().saturating_sub(10)..];
    let yes = &yes[yes.len().saturating_sub(4)..];
    if no.is_empty() && yes.is_empty() {
        return "Todavía no hay votos del OWNER sobre tus veredictos.".into();
    }
    let tx = |x: &Owner| {
        let mut s = format!("- {}: dijiste «{}» ({})", x.title, x.v, x.why);
        if !x.note.is_empty() {
            s.push_str(&format!("; el OWNER: {}", x.note));
        }
        s
    };
    let mut out = String::from(
        "El OWNER revisó veredictos tuyos. Donde dijo que NO estaba de acuerdo, su criterio manda: aplícalo a casos parecidos.\n",
    );
    if !no.is_empty() {
        out.push_str("NO de acuerdo:\n");
        let parts: Vec<String> = no.iter().map(|x| tx(x)).collect();
        out.push_str(&parts.join("\n"));
        out.push('\n');
    }
    if !yes.is_empty() {
        out.push_str("De acuerdo:\n");
        let parts: Vec<String> = yes.iter().map(|x| tx(x)).collect();
        out.push_str(&parts.join("\n"));
    }
    out
}

const RULES: &str = "Reglas del OWNER: nada cuenta sin evidencia que exista y puedas abrir (commit, archivo, captura, log con salida); sirve solo si lo que viste avanza literalmente el 'Done when' de su meta, no algo parecido ni del mismo tema; un commit o captura anterior a la tarea no es evidencia; varios agentes trabajan en el mismo repo, así que algo nuevo puede ser de otro; ante la duda, no sirve.";

fn jsonish(x: &str) -> Vec<Value> {
    static RX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)\[.*\]").unwrap());
    if let Some(m) = RX.find(x) {
        serde_json::from_str::<Vec<Value>>(m.as_str()).unwrap_or_default()
    } else {
        Vec::new()
    }
}

fn audit(repo: &str, root: &Path, account: u32) -> Result<usize, String> {
    let b = board(root).unwrap_or(Value::Null);
    let seen = {
        let st = S.lock();
        st.audit.get(repo).cloned().unwrap_or_default()
    };
    let all = items(&b);
    let due: Vec<Item> = all
        .into_iter()
        .filter(|it| {
            it.ready
                && match seen.get(&it.key) {
                    Some(a) => !sig_eq(&a.sig, &it.sig) || a.redo,
                    None => true,
                }
        })
        .collect();
    let todo: Vec<Item> = due.iter().take(8).cloned().collect();
    if todo.is_empty() {
        return Ok(0);
    }
    let mut rows: Vec<String> = Vec::new();
    for (i, it) in todo.iter().enumerate() {
        let i = i + 1;
        let (cited, commits, shots) = evidence(root, it);
        let cited_txt = if cited.is_empty() { "ninguna".to_string() } else { py_list(&cited) };
        if it.kind == "gate" {
            rows.push(format!(
                "#{i} [META FIRMADA por el Orquestador] {}: Done when {} · el OWNER ve: {}\n   evidencia citada: {}",
                it.ms, it.done_when, it.done, cited_txt
            ));
        } else {
            let ms_txt = if it.ms.is_empty() { "NINGUNA (desvío)" } else { &it.ms };
            let dw_txt = if it.done_when.is_empty() { "-" } else { &it.done_when };
            let done_txt = if it.done.is_empty() { "-" } else { &it.done };
            let commits_txt = if commits.is_empty() { "ninguno".to_string() } else { py_list(&commits) };
            let shots_txt = if shots.is_empty() { "ninguna".to_string() } else { py_list(&shots) };
            let lasts: Vec<String> = it
                .lines
                .iter()
                .rev()
                .take(3)
                .rev()
                .map(|l| {
                    format!(
                        "[{}] {}",
                        l.get("state").and_then(Value::as_str).unwrap_or(""),
                        l.get("line").and_then(Value::as_str).unwrap_or("")
                    )
                })
                .collect();
            rows.push(format!(
                "#{i} [TAREA] {} · de {} · meta {ms_txt}: Done when {dw_txt} · tarea terminada cuando: {done_txt}\n   evidencia citada: {cited_txt}\n   commits desde que empezó: {commits_txt}\n   capturas nuevas: {shots_txt}\n   LO QUE DICE EL AGENTE (léelo solo después de decidir): {}",
                it.title,
                it.who,
                lasts.join(" / ")
            ));
        }
    }
    let prompt = format!(
        "Eres el auditor del repo {repo}. Trabajas para el OWNER, no para los agentes: dices si de verdad estamos logrando su meta de GOAL.md. No eres amable con nadie y no adivinas. {RULES}\n\nMétodo, en este orden, para cada punto:\n1. Abre la evidencia: git show --stat de cada commit, Read de cada archivo, log y captura (png) citados. Si no existe o no se puede abrir, no cuenta.\n2. Decide solo con lo que viste: v = sirve (avanza el Done when de su meta) | no (hay resultado pero no lo avanza) | sin evidencia (nada que abrir lo prueba).\n3. Después lee lo que dice el agente y compara: claim = coincide | exagera | se queda corto.\nUna meta firmada por el Orquestador: sirve solo si su evidencia muestra el Done when completo; si no, no.\n\n{}\n\nResponde solo un arreglo JSON: [{{\"n\":<número tras #>,\"v\":\"sirve\"|\"no\"|\"sin evidencia\",\"claim\":\"coincide\"|\"exagera\"|\"se queda corto\",\"why\":\"<una línea en español llano: qué abriste y qué viste>\",\"shot\":\"<ruta de la captura que lo prueba, o vacío>\"}}]\n\nPUNTOS:\n{}",
        calibration(repo),
        rows.join("\n")
    );
    let reply = claude("sonnet", &prompt, root, account, "Read,Glob,Grep,Bash")?;
    let mut got: HashMap<i64, Value> = HashMap::new();
    for x in jsonish(&reply) {
        if x.is_object()
            && let Some(n) = x.get("n").and_then(Value::as_i64)
        {
            got.insert(n, x);
        }
    }
    let now = crate::shared::now();
    {
        let mut st = S.lock();
        let o = st.audit.entry(repo.to_string()).or_default();
        for (i, it) in todo.iter().enumerate() {
            let Some(x) = got.get(&((i + 1) as i64)) else {
                continue;
            };
            let v = x.get("v").and_then(Value::as_str).unwrap_or("");
            if !["sirve", "no", "sin evidencia"].contains(&v) {
                continue;
            }
            let claim = if it.kind == "task" {
                x.get("claim").and_then(Value::as_str).unwrap_or("").to_string()
            } else {
                String::new()
            };
            o.insert(
                it.key.clone(),
                Audit {
                    sig: it.sig.clone(),
                    v: v.to_string(),
                    claim,
                    why: x.get("why").map(|w| pystr(Some(w))).unwrap_or_default().chars().take(240).collect(),
                    t: now,
                    shot: x.get("shot").map(|s| pystr(Some(s))).unwrap_or_default(),
                    redo: false,
                },
            );
        }
        save_state(&st);
    }
    Ok(due.len() - todo.len())
}

/// The OWNER's word on one verdict (POST /api/avance/voto).
pub fn vote(repo: &str, key: &str, agree: Value, note: &str) -> (bool, String) {
    let a = {
        let st = S.lock();
        st.audit.get(repo).and_then(|m| m.get(key)).cloned()
    };
    let Some(a) = a else {
        return (false, "no hay veredicto que votar".into());
    };
    let watched_repos = watched();
    let title = watched_repos
        .get(repo)
        .and_then(|root| board(root))
        .map(|b| {
            items(&b)
                .into_iter()
                .find(|it| it.key == key)
                .map(|it| it.title)
                .unwrap_or_else(|| key.to_string())
        })
        .unwrap_or_else(|| key.to_string());
    let agree_b = truthy(&agree);
    let now = crate::shared::now();
    {
        let mut st = S.lock();
        st.owner.entry(repo.to_string()).or_default().insert(
            key.to_string(),
            Owner {
                agree: agree_b,
                v: a.v.clone(),
                why: a.why.clone(),
                title,
                note: note.chars().take(200).collect(),
                t: now,
                at: a.t,
            },
        );
        if !agree_b {
            if let Some(e) = st.audit.get_mut(repo).and_then(|m| m.get_mut(key)) {
                e.redo = true;
            }
            st.state.entry(repo.to_string()).or_default().pending = true;
        }
        save_state(&st);
    }
    if agree_b {
        (true, "anotado: de acuerdo".into())
    } else {
        (true, "anotado: el auditor lo vuelve a ver con tu criterio".into())
    }
}

// ---------- signature / loop ----------
fn signature(root: &Path) -> String {
    let rp = realpath(root);
    let mut done: Vec<String> = JOBS
        .list_jobs()
        .into_iter()
        .filter(|j| {
            realpath_s(&j.repo) == rp && ["ok", "fallo", "pregunta"].contains(&j.status.as_str())
        })
        .map(|j| j.id)
        .collect();
    done.sort();
    let head = sh(&["git", "rev-parse", "HEAD"], root).trim().to_string();
    format!(
        "{}|{}|{}|{}|{}|{}",
        head,
        mt(&root.join("GOAL.md")),
        mt(&root.join("web/status/board.json")),
        mt(&root.join("web/plan/plan.json")),
        done.len(),
        done.last().cloned().unwrap_or_default()
    )
}

/// avance.loop(runway, awake): the background auditor; loops::spawn_all starts it.
pub fn run_loop() {
    std::thread::sleep(std::time::Duration::from_secs(15));
    loop {
        let repos = watched();
        for (repo, root) in repos {
            let should_go = {
                let mut st = S.lock();
                let sig = signature(&root);
                let stale = st
                    .state
                    .get(&repo)
                    .map(|e| e.sig.as_deref() != Some(sig.as_str()))
                    .unwrap_or(true);
                if stale {
                    let e = st.state.entry(repo.clone()).or_default();
                    e.sig = Some(sig);
                    e.seen = crate::shared::now();
                    e.pending = true;
                    continue;
                }
                let now = crate::shared::now();
                let n = {
                    let runs = st.runs.entry(repo.clone()).or_default();
                    runs.retain(|t| now - *t < 3600.0);
                    runs.len()
                };
                let e = st.state.entry(repo.clone()).or_default();
                e.pending && crate::shared::awake() && now - e.seen > QUIET && n < PER_HOUR && !e.busy
            };
            if should_go {
                {
                    let mut st = S.lock();
                    let e = st.state.entry(repo.clone()).or_default();
                    e.pending = false;
                    e.busy = true;
                    st.runs.entry(repo.clone()).or_default().push(crate::shared::now());
                }
                let account = crate::loops::runway().first().copied().unwrap_or(1);
                let res = audit(&repo, &root, account);
                let mut st = S.lock();
                let e = st.state.entry(repo.clone()).or_default();
                match res {
                    Ok(more) => {
                        e.pending = more > 0;
                        e.err = String::new();
                    }
                    Err(x) => e.err = x.chars().take(160).collect(),
                }
                e.busy = false;
            }
        }
        std::thread::sleep(std::time::Duration::from_secs_f64(LOOK));
    }
}

// ---------- orquestador / claim boards / sweep ----------
fn orquestador(root: &Path) -> String {
    let pat = format!("{}/.claude/sessions/*.json", crate::shared::home());
    let Ok(paths) = glob::glob(&pat) else {
        return String::new();
    };
    let want = realpath(root);
    for p in paths.flatten() {
        let Ok(txt) = fs::read_to_string(&p) else {
            continue;
        };
        let Ok(d) = serde_json::from_str::<Value>(&txt) else {
            continue;
        };
        if d.get("name").and_then(Value::as_str) != Some("Orquestador") {
            continue;
        }
        let cwd = d.get("cwd").and_then(Value::as_str).unwrap_or("");
        if realpath_s(cwd) != want {
            continue;
        }
        if let Some(pid) = d.get("pid").and_then(Value::as_i64) {
            if unsafe { libc::kill(pid as i32, 0) } == 0 {
                return d.get("sessionId").and_then(Value::as_str).unwrap_or("").to_string();
            }
        }
    }
    String::new()
}

static WHY_RX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(Why|Problem|Goal):\s*(.+)").unwrap());
static PATHS_RX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\w.*-]+(?:/[\w.*-]+)+\.\w+").unwrap());
static HEAD_RX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^## ").unwrap());

fn glob_any(pat: PathBuf) -> bool {
    let s = pat.to_string_lossy().into_owned();
    glob::glob(&s).map(|mut it| it.any(|e| e.is_ok())).unwrap_or(false)
}

fn claim_boards(root: &Path) -> Vec<Value> {
    let root_s = root.to_string_lossy().into_owned();
    let mut files: Vec<String> = Vec::new();
    for pat in [format!("{root_s}/rea/map/*-board.md"), format!("{root_s}/tableros/*-board.md")] {
        if let Ok(ps) = glob::glob(&pat) {
            for p in ps.flatten() {
                files.push(p.to_string_lossy().into_owned());
            }
        }
    }
    files.sort();
    let mut out = Vec::new();
    for f in &files {
        let fpath = Path::new(f);
        let mut title = String::new();
        let mut why = String::new();
        let mut claims: Vec<Value> = Vec::new();
        let mut fronts: Vec<Value> = Vec::new();
        let mut kind = String::new();
        let text = String::from_utf8_lossy(&fs::read(fpath).unwrap_or_default()).into_owned();
        for ln in text.split('\n') {
            if let Some(t) = ln.strip_prefix("# ")
                && title.is_empty()
            {
                title = t.trim().to_string();
                if let Some(r) = title.strip_prefix("Tablero:") {
                    title = r.trim().to_string();
                }
            }
            if why.is_empty()
                && let Some(c) = WHY_RX.captures(ln)
            {
                why = c.get(2).map(|g| g.as_str().trim().to_string()).unwrap_or_default();
            }
            if !ln.starts_with('|') {
                kind.clear();
                continue;
            }
            let trimmed = ln.trim().trim_start_matches('|').trim_end_matches('|');
            let c: Vec<String> = trimmed.split('|').map(|x| x.trim().to_string()).collect();
            if c.iter().flat_map(|x| x.chars()).all(|ch| "-: ".contains(ch)) {
                continue;
            }
            if kind.is_empty() {
                kind = if c.first().map(|x| x.to_lowercase().starts_with("front")).unwrap_or(false) {
                    "front".into()
                } else if c.iter().any(|x| x.to_lowercase().contains("claim")) {
                    "claim".into()
                } else {
                    "-".into()
                };
                continue;
            }
            if kind == "claim" && c.len() >= 3 {
                claims.push(json!({"id": c[0], "claim": c[1], "need": c[2]}));
            } else if kind == "front" && c.len() >= 4 {
                let paths: Vec<String> = PATHS_RX.find_iter(&c[3]).map(|m| m.as_str().to_string()).collect();
                let done = if paths.is_empty() {
                    Value::Null
                } else {
                    Value::Bool(paths.iter().all(|x| glob_any(root.join(x))))
                };
                fronts.push(json!({"id": c[0], "who": c[1], "scope": c[2], "out": c[3], "done": done}));
            }
        }
        let stem = &f[..f.len().saturating_sub(3)];
        let ver_path = format!("{stem}.verdicts.json");
        let ver: Map<String, Value> = fs::read(&ver_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        for c in claims.iter_mut() {
            let id = c.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            let v = ver.get(&id).filter(|x| x.is_object()).cloned().unwrap_or(Value::Null);
            c["verdict"] = v;
        }
        let res_path = format!("{stem}.resumenes.md");
        let mut summaries: Vec<Value> = Vec::new();
        if let Ok(b) = fs::read(&res_path) {
            let txt = String::from_utf8_lossy(&b).into_owned();
            for part in HEAD_RX.split(&txt).skip(1) {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                let mut it = part.splitn(2, '\n');
                let head = it.next().unwrap_or("").trim().to_string();
                let body = it.next().unwrap_or("").trim().chars().take(3000).collect::<String>();
                summaries.push(json!({"head": head, "body": body}));
                if summaries.len() >= 10 {
                    break;
                }
            }
        }
        let updated = mt(fpath).max(mt(Path::new(&ver_path))).max(mt(Path::new(&res_path)));
        let t = if title.is_empty() {
            fpath.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string()
        } else {
            title
        };
        out.push(json!({
            "file": relpath(fpath, root),
            "title": t,
            "why": why,
            "fronts": fronts,
            "claims": claims,
            "summaries": summaries,
            "updated": updated,
        }));
    }
    out
}

fn scene_name(path: &Path) -> String {
    static NUM_RX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+_").unwrap());
    let base = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let stem = base.strip_suffix(".png").unwrap_or(base);
    NUM_RX.replace(stem, "").into_owned()
}

fn png_prefix(path: &Path) -> f64 {
    static PREFIX_RX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d*").unwrap());
    let base = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let m = PREFIX_RX.find(base).map(|m| m.as_str()).unwrap_or("");
    if m.is_empty() {
        1e9
    } else {
        m.parse::<f64>().unwrap_or(1e9)
    }
}

fn is_end_shot(p: &Path) -> bool {
    p.to_string_lossy().ends_with("/end.png")
}

fn run_of(root: &Path, d: &Path) -> Value {
    let r: Value = fs::read(d.join("result.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null);
    let bad = r.get("first_bad").cloned().unwrap_or(Value::Null);
    let bad = if bad.is_object() { bad } else { json!({}) };
    let mut pngs: Vec<PathBuf> = Vec::new();
    let pat = format!("{}/*.png", d.to_string_lossy());
    if let Ok(ps) = glob::glob(&pat) {
        pngs = ps.flatten().collect();
    }
    pngs.sort_by(|a, b| mt(a).total_cmp(&mt(b)));
    let mut by_prefix = pngs.clone();
    by_prefix.sort_by(|a, b| png_prefix(a).total_cmp(&png_prefix(b)));
    let start = r
        .get("start")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| by_prefix.iter().find(|p| !is_end_shot(p)).map(|p| scene_name(p)))
        .unwrap_or_default();
    let ok = r.get("consecutive_ok").and_then(Value::as_i64).unwrap_or(0);
    let of = r.get("expected").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0);
    let bad_scene = bad.get("scene").and_then(Value::as_str).unwrap_or("").to_string();
    let why = bad
        .get("detail")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| bad.get("reason").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();
    let res_mt = mt(&d.join("result.json"));
    let log_mt = mt(&d.join("run.log"));
    let t = res_mt.max(log_mt);
    let live = crate::shared::now() - log_mt < 120.0 && log_mt > res_mt;
    let shot = pngs.last().map(|p| relpath(p, root)).unwrap_or_default();
    let at = pngs.iter().rev().find(|p| !is_end_shot(p)).map(|p| scene_name(p)).unwrap_or_default();
    json!({
        "ok": ok, "of": of, "start": start, "bad": bad_scene, "why": why,
        "t": t, "live": live, "shot": shot, "at": at,
    })
}

fn sweep(root: &Path, total: usize) -> Value {
    let root_s = root.to_string_lossy().into_owned();
    let mut ch: Vec<Value> = Vec::new();
    if let Ok(ps) = glob::glob(&format!("{root_s}/web/status/sweep/*/")) {
        for d in ps.flatten() {
            let mut c = run_of(root, &d);
            let base = d.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let n: i64 = if !base.is_empty() && base.chars().all(|x| x.is_ascii_digit()) {
                base.parse().unwrap_or(0)
            } else {
                0
            };
            if let Some(m) = c.as_object_mut() {
                m.insert("n".into(), json!(n));
            }
            ch.push(c);
        }
    }
    ch.sort_by(|a, b| numf(a.get("n")).total_cmp(&numf(b.get("n"))));
    let mut dirs: Vec<PathBuf> = Vec::new();
    let auto0 = PathBuf::from(format!("{root_s}/web/status/auto/"));
    if auto0.is_dir() {
        dirs.push(auto0);
    }
    if let Ok(ps) = glob::glob(&format!("{root_s}/web/status/auto/r*/")) {
        for d in ps.flatten() {
            if d.is_dir() {
                dirs.push(d);
            }
        }
    }
    let mut auto: Vec<Value> = Vec::new();
    for d in dirs {
        let mut c = run_of(root, &d);
        let dirn = d.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if let Some(m) = c.as_object_mut() {
            m.insert("dir".into(), json!(dirn));
        }
        auto.push(c);
    }
    auto.sort_by(|a, b| numf(b.get("t")).total_cmp(&numf(a.get("t"))));
    let mut runs: Vec<PathBuf> = Vec::new();
    if let Ok(ps) = glob::glob(&format!("{root_s}/census/sweep_runs/*.json")) {
        runs = ps.flatten().collect();
    }
    runs.sort_by(|a, b| mt(a).total_cmp(&mt(b)));
    let census = if !runs.is_empty() || root.join("census/sweep_runs").is_dir() {
        let last = runs
            .last()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .map(|s| s.chars().take(s.chars().count().saturating_sub(5)).collect::<String>())
            .unwrap_or_default();
        json!({
            "runs": runs.len(),
            "last": last,
            "tsv": root.join("census/failures.tsv").exists(),
            "md": root.join("census/failures.md").exists(),
        })
    } else {
        Value::Null
    };
    if ch.is_empty() {
        return Value::Null;
    }
    let mut seen: HashSet<String> = HashSet::new();
    if let Ok(ps) = glob::glob(&format!("{root_s}/web/status/sweep/*/")) {
        for d in ps.flatten() {
            if let Ok(b) = fs::read(d.join("result.json"))
                && let Ok(v) = serde_json::from_slice::<Value>(&b)
                && let Some(arr) = v.get("reached").and_then(Value::as_array)
            {
                for x in arr {
                    if let Some(s) = x.as_str() {
                        seen.insert(s.to_string());
                    }
                }
            }
        }
    }
    json!({
        "ok": seen.len().min(total),
        "total": total,
        "chapters": ch,
        "auto": auto,
        "census": census,
    })
}

// ---------- snapshot ----------
fn view(it: &Item, au: &HashMap<String, Audit>, ow: &HashMap<String, Owner>) -> Value {
    let a = au.get(&it.key).filter(|a| sig_eq(&a.sig, &it.sig));
    let v = ow.get(&it.key);
    let last = match &it.last {
        Some(l) => {
            let mut m = Map::new();
            for k in ["state", "line", "t", "who"] {
                if let Some(x) = l.get(k) {
                    m.insert(k.to_string(), x.clone());
                }
            }
            m.insert(
                "ev".to_string(),
                l.get("ev").cloned().unwrap_or_else(|| json!("")),
            );
            Value::Object(m)
        }
        None => Value::Null,
    };
    let audit_v = match a {
        Some(a) => json!({"v": a.v, "claim": a.claim, "why": a.why, "t": a.t, "shot": a.shot}),
        None => Value::Null,
    };
    let vote_v = match (v, a) {
        (Some(v), Some(a)) if v.at == a.t => json!(v.agree),
        _ => Value::Null,
    };
    json!({
        "key": it.key,
        "who": it.who,
        "kind": it.akind,
        "title": it.title,
        "line": it.line,
        "ms": it.ms,
        "done_when": it.done_when,
        "done": it.done,
        "last": last,
        "audit": audit_v,
        "vote": vote_v,
    })
}

fn rank(kind: &str) -> i64 {
    match kind {
        "sonnet" | "session" | "haiku" => 0,
        "codex" => 1,
        "bec" => 2,
        _ => 1,
    }
}

/// The `avance` field of the snapshot (boards per repo), same shape as avance.snapshot().
pub fn snapshot() -> Value {
    let mut out: Map<String, Value> = Map::new();
    for (repo, root) in watched() {
        let Some(b) = board(&root) else {
            continue;
        };
        let (au, ow, st) = {
            let s = S.lock();
            (
                s.audit.get(&repo).cloned().unwrap_or_default(),
                s.owner.get(&repo).cloned().unwrap_or_default(),
                s.state.get(&repo).cloned().unwrap_or_default(),
            )
        };
        let its = items(&b);
        let mut gates: HashMap<String, Value> = HashMap::new();
        for it in its.iter().filter(|i| i.kind == "gate") {
            gates.insert(it.ms.clone(), view(it, &au, &ow));
        }
        let goal_arr = b.get("goal").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut cur = String::new();
        let mut goal_out: Vec<Value> = Vec::new();
        for g in &goal_arr {
            let id = vstr(g.get("id"));
            let state = vstr(g.get("state"));
            if cur.is_empty() && state != "VERIFIED" {
                cur = id.clone();
            }
            goal_out.push(json!({
                "id": id,
                "state": state,
                "done_when": vstr(g.get("done_when")),
                "sees": vstr(g.get("sees")),
                "ev": vstr(g.get("ev")),
                "gate": gates.get(&vstr(g.get("id"))).cloned().unwrap_or(Value::Null),
            }));
        }
        let mut tasks: Vec<Value> = its
            .iter()
            .filter(|i| i.kind == "task")
            .map(|i| view(i, &au, &ow))
            .collect();
        tasks.sort_by(|a, b| {
            let ka = (
                vstr(a.get("ms")) != cur,
                vstr(a.get("ms")).is_empty(),
                rank(a.get("kind").and_then(Value::as_str).unwrap_or("")),
            );
            let kb = (
                vstr(b.get("ms")) != cur,
                vstr(b.get("ms")).is_empty(),
                rank(b.get("kind").and_then(Value::as_str).unwrap_or("")),
            );
            ka.cmp(&kb)
        });
        let agree = ow.values().filter(|x| x.agree).count();
        let votes = ow.len();
        let now = crate::shared::now();
        let runs = S
            .lock()
            .runs
            .get(&repo)
            .map(|v| v.iter().filter(|t| now - **t < 3600.0).count())
            .unwrap_or(0);
        let scenes = b.get("scenes").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0);
        out.insert(
            repo,
            json!({
                "goal": goal_out,
                "cur": cur,
                "tasks": tasks,
                "agree": agree,
                "votes": votes,
                "updated": b.get("updated").cloned().unwrap_or_else(|| json!(0)),
                "busy": st.busy,
                "pending": st.pending,
                "err": st.err,
                "runs": runs,
                "orq": orquestador(&root),
                "boards": claim_boards(&root),
                "sweep": sweep(&root, scenes),
            }),
        );
    }
    Value::Object(out)
}

/// A PNG inside a watched repo's web/status (the Tablero's shot strip), or None.
pub fn repofile(repo: &str, rel: &str) -> Option<Vec<u8>> {
    let root = watched().get(repo).cloned()?;
    let p = realpath(&root.join(rel));
    let status = realpath(&root.join("web/status"));
    let prefix = format!("{}/", status.to_string_lossy());
    let ps = p.to_string_lossy();
    if !ps.starts_with(&prefix) || !(ps.ends_with(".png") || ps.ends_with(".jpg")) || !p.is_file() {
        return None;
    }
    fs::read(&p).ok()
}
