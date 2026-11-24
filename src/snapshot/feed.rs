use super::*;
#[derive(Clone, Default)]
pub(super) struct Event {
    pub t: f64,
    pub raw: String,
    pub desc: String,
}
#[derive(Clone, Default)]
pub(super) struct Feed {
    pub path: String,
    pub codex: bool,
    pub first: String,
    pub last: String,
    pub say: String,
    pub wait: String,
    pub t1: f64,
    pub model: String,
    pub busy: bool,
    pub meta: Value,
    pub errs: usize,
    pub ev: Vec<Event>,
    pub re: Vec<Value>,
    off: u64,
    started: bool,
    stamp: f64,
    has_user: bool,
}
fn iso(x: &str) -> f64 {
    chrono::NaiveDateTime::parse_from_str(&cut(x, 19), "%Y-%m-%dT%H:%M:%S")
        .map(|t| t.and_utc().timestamp() as f64)
        .unwrap_or_else(|_| now())
}
pub(super) fn tool_arg(name: &str, a: &Value) -> String {
    let lim = if ["Bash", "bash", "shell", "exec_command"].contains(&name) {
        320
    } else {
        160
    };
    if let Some(x) = a.as_str() {
        return sq(x, lim);
    }
    let x = [
        "file_path",
        "path",
        "filePath",
        "notebook_path",
        "command",
        "cmd",
        "pattern",
        "url",
        "query",
        "description",
        "prompt",
        "message",
    ]
    .iter()
    .find_map(|k| a[*k].as_str().filter(|s| !s.is_empty()))
    .or_else(|| {
        a.as_object()
            .and_then(|o| o.values().find_map(Value::as_str))
    })
    .unwrap_or("");
    sq(x, lim)
}
fn tool_line(name: &str, a: &Value) -> String {
    let path = ["file_path", "path", "notebook_path"]
        .iter()
        .find_map(|k| a[*k].as_str().filter(|s| !s.is_empty()));
    let val = match name {
        "Bash" => sq(
            a["description"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or(a["command"].as_str())
                .unwrap_or(""),
            160,
        ),
        _ if path.is_some() => base(path.unwrap()),
        "Grep" | "Glob" => sq(&s(a, "pattern"), 80),
        "Agent" | "Task" => sq(
            a["description"]
                .as_str()
                .or(a["prompt"].as_str())
                .unwrap_or(""),
            120,
        ),
        _ => sq(
            a.as_object()
                .and_then(|a| a.values().find_map(Value::as_str))
                .unwrap_or(""),
            100,
        ),
    };
    if val.is_empty() {
        name.into()
    } else {
        format!("{}: {val}", if name == "Task" { "Agent" } else { name })
    }
}
pub(super) fn re_hit(raw: &str, why: &str) -> Option<Value> {
    let re = rx(
        r"(?i)\bmcp__rea__(\w+)|\b(analyzeHeadless|DecompAt|ghidra|hopper|capstone|objdump|rizin|frida)\b",
    );
    let m = re.captures(raw)?;
    let mut addrs = vec![];
    for a in rx(r"(?i)\bFUN_([0-9a-f]{6,8})\b|\b0x([0-9a-f]{5,8})\b").captures_iter(raw) {
        let a = format!(
            "0x{}",
            a.get(1)
                .or(a.get(2))
                .unwrap()
                .as_str()
                .trim_start_matches('0')
                .to_lowercase()
        );
        if !addrs.contains(&a) && addrs.len() < 4 {
            addrs.push(a)
        }
    }
    Some(
        json!({"tool":m.get(1).or(m.get(2)).unwrap().as_str().to_lowercase().replace('_'," "),"addrs":addrs,"why":sq(if why.is_empty(){raw.split_once(' ').map(|(_,s)|s).unwrap_or(raw)}else{why},110)}),
    )
}
pub(super) fn recent(f: &Feed) -> Value {
    json!(
        f.re.iter()
            .filter(|v| now() - n(v, "t") < 1800.)
            .collect::<Vec<_>>()
    )
}
fn text(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.into();
    }
    arr(v)
        .iter()
        .filter(|v| ["text", "input_text", "output_text"].contains(&s(v, "type").as_str()))
        .map(|v| s(v, "text"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .into()
}
pub(super) fn result_text(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        s.into()
    } else {
        arr(v)
            .iter()
            .map(|b| s(b, "text"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
fn user(e: &Value) -> Option<String> {
    let tx = text(&e["message"]["content"]);
    if let Some(c)=rx(r#"(?s)<cross-session-message from="[^"]*" from-name="([^"]*)"[^>]*>\n?(.*?)\n?</cross-session-message>"#).captures(&tx){return Some(format!("[De {}] {}",&c[1],c[2].trim()))}
    if e["isMeta"] == true
        || rx(r"^<(command-|local-command|system-reminder|task-notification|bash-)").is_match(&tx)
        || tx.is_empty()
    {
        None
    } else {
        Some(tx)
    }
}
pub(super) fn agy_tool(tc: &Value) -> (String, Value) {
    let mut a = tc["args"].clone();
    if let Some(o) = a.as_object_mut() {
        for v in o.values_mut() {
            if let Some(x) = v.as_str() {
                if let Ok(d) = serde_json::from_str::<Value>(x) {
                    *v = d
                }
            }
        }
    }
    let name = s(tc, "name");
    let mapped = match name.as_str() {
        "read_file" | "view_file" => "Read",
        "edit_file" | "replace_file_content" | "multi_replace_file_content" => "Edit",
        "write_file" | "write_to_file" => "Write",
        "run_command" => "Bash",
        "grep_search" => "Grep",
        "find_by_name" | "list_dir" => "Glob",
        "search_web" => "WebSearch",
        "read_url_content" => "WebFetch",
        "invoke_subagent" | "swarm" => "Agent",
        "send_message" | "post_message" => "SendMessage",
        _ => &name,
    };
    let mut i = json!({});
    for (dst, keys) in [
        (
            "file_path",
            vec![
                "Path",
                "AbsolutePath",
                "TargetFile",
                "DirectoryPath",
                "SearchPath",
            ],
        ),
        ("command", vec!["CommandLine"]),
        ("description", vec!["toolSummary"]),
        ("query", vec!["query", "Query"]),
        ("pattern", vec!["Pattern"]),
        ("url", vec!["Url"]),
        ("message", vec!["Message", "text", "summary", "result"]),
        ("prompt", vec!["Prompt", "brief"]),
    ] {
        if let Some(v) = keys
            .iter()
            .find_map(|k| a.get(*k).filter(|v| !v.is_null() && v.as_str() != Some("")))
        {
            i[dst] = v.clone()
        }
    }
    if let Some(p) = i["file_path"].as_str() {
        i["file_path"] = json!(
            rx(&format!(
                r"^{}/worktrees/[^/]+/[^/]+/",
                regex::escape(&agy_dir())
            ))
            .replace(p, "")
            .to_string()
        )
    }
    if name == "invoke_subagent" {
        i["description"] = a
            .get("Role")
            .or(a.get("TypeName"))
            .cloned()
            .unwrap_or(Value::Null)
    }
    if name == "swarm" {
        i["description"] = json!(format!(
            "swarm {} {}",
            s(&a, "action"),
            a["name"].as_str().or(a["title"].as_str()).unwrap_or("")
        ))
    }
    if i.as_object().is_some_and(|o| o.is_empty()) {
        i = a
    }
    (mapped.into(), i)
}
pub(super) fn agy_user(e: &Value) -> String {
    let c = s(e, "content");
    rx(r"(?s)<USER_REQUEST>\n?(.*?)\n?</USER_REQUEST>")
        .captures(&c)
        .map(|m| m[1].trim().into())
        .unwrap_or_else(|| c.trim().into())
}
fn agy_sys(e: &Value) -> (String, String) {
    let c = s(e, "content");
    rx(r"(?s)sender=(\S+).*?content=(.*?)(?:\n</SYSTEM_MESSAGE>|$)")
        .captures(&c)
        .map(|m| {
            let tx = m[2].trim();
            if tx.is_empty() || tx.starts_with("[swarm] new board message") {
                (String::new(), String::new())
            } else {
                (m[1].into(), tx.into())
            }
        })
        .unwrap_or_default()
}
fn agy_err(e: &Value) -> bool {
    ["ERROR", "INVALID"].contains(&s(e, "status").as_str())
        || s(e, "type") == "ERROR_MESSAGE"
        || rx(r"(?:Command|Process) exited with code:? [1-9]").is_match(&cut(&s(e, "content"), 400))
}
fn call(p: &Value) -> (String, String, String) {
    let raw = if s(p, "type") == "custom_tool_call" {
        s(p, "input")
    } else {
        s(p, "arguments")
    };
    let files = rx(r#"\*\*\* (?:Update|Add|Delete) File: ([^\s"'\\]+)"#)
        .captures_iter(&raw)
        .map(|m| m[1].to_string())
        .collect::<Vec<_>>();
    let cmd = rx(r#"cmd\s*:\s*"((?:[^"\\]|\\.)*)"#)
        .captures(&raw)
        .map(|m| m[1].to_string());
    let name = if files.is_empty() {
        s(p, "name")
    } else {
        "apply_patch".into()
    };
    let arg = if !files.is_empty() {
        files.join(", ")
    } else if let Some(c) = cmd {
        c
    } else {
        let v = serde_json::from_str::<Value>(&raw).unwrap_or(json!(raw));
        tool_arg(&name, &v)
    };
    (name, arg, raw)
}
impl Feed {
    fn act(&mut self, t: f64, raw: String, desc: String, source: String) {
        self.ev.push(Event {
            t,
            raw: sq(&raw, 200),
            desc: sq(&desc, 120),
        });
        if self.ev.len() > 80 {
            self.ev.remove(0);
        }
        self.t1 = self.t1.max(t);
        if let Some(mut h) = re_hit(&source, &desc) {
            h["t"] = json!(t);
            self.re.push(h);
            if self.re.len() > 12 {
                self.re.remove(0);
            }
        }
    }
    fn ask(&mut self, tx: String, t: f64) {
        if self.first.is_empty() {
            self.first = tx.clone()
        }
        self.last = tx;
        self.t1 = self.t1.max(t)
    }
    fn eat(&mut self, e: &Value) {
        let ty = s(e, "type");
        let t = e["timestamp"]
            .as_str()
            .or(e["created_at"].as_str())
            .map(iso)
            .unwrap_or(self.t1);
        if self.path.starts_with(&agy_dir()) {
            match ty.as_str() {
                "USER_INPUT" => {
                    let tx = agy_user(e);
                    if !tx.is_empty() {
                        self.has_user = true;
                        self.ask(tx, t)
                    }
                    self.busy = true
                }
                "SYSTEM_MESSAGE" => {
                    let (sender, tx) = agy_sys(e);
                    if !tx.is_empty() && !sender.contains("/task-") {
                        if !self.has_user || sender == "system" {
                            self.ask(tx, t)
                        }
                        if !self.has_user {
                            self.busy = true
                        }
                        self.t1 = self.t1.max(t)
                    }
                }
                "PLANNER_RESPONSE" => {
                    if !s(e, "model").is_empty() {
                        self.model = s(e, "model")
                    }
                    let tcs = arr(&e["tool_calls"]);
                    for tc in &tcs {
                        let (name, i) = agy_tool(tc);
                        self.act(
                            t,
                            tool_line(&name, &i),
                            s(&i, "description"),
                            format!("{name} {}", cut(&i.to_string(), 6000)),
                        );
                        if ["post_message", "send_message", "finish"]
                            .contains(&s(tc, "name").as_str())
                            && !s(&i, "message").is_empty()
                        {
                            self.say = s(&i, "message").trim().into()
                        }
                    }
                    let tx = s(e, "content").trim().to_string();
                    if !tx.is_empty() {
                        if !(tx.len() < 120
                            && self.say.len() > tx.len() * 2
                            && rx("(?i)sent to parent|enviad").is_match(&tx))
                        {
                            self.say = tx.clone()
                        }
                        self.act(t, format!("dice: {tx}"), String::new(), String::new())
                    }
                    self.busy = !tcs.is_empty()
                        && !tcs
                            .iter()
                            .any(|v| ["swarm_wait", "finish"].contains(&s(v, "name").as_str()));
                }
                "GENERIC" | "ERROR_MESSAGE" => {
                    self.errs += usize::from(agy_err(e));
                    self.t1 = self.t1.max(t)
                }
                _ => {}
            }
            return;
        }
        if self.codex {
            let p = &e["payload"];
            match ty.as_str() {
                "session_meta" => {
                    let th = &p["source"]["subagent"]["thread_spawn"];
                    self.meta = json!({"id":s(p,"id"),"cwd":s(p,"cwd"),"parent":s(th,"parent_thread_id"),"nick":s(th,"agent_nickname"),"orig":s(p,"originator")});
                }
                "turn_context" => {
                    if !s(p, "model").is_empty() {
                        self.model = s(p, "model")
                    }
                }
                "event_msg" => {
                    match s(p, "type").as_str() {
                        "task_started" => self.busy = true,
                        "task_complete" => self.busy = false,
                        _ => {}
                    }
                    self.t1 = self.t1.max(t)
                }
                "response_item" => match s(p, "type").as_str() {
                    "custom_tool_call" | "function_call" => {
                        let (_, _, raw) = call(p);
                        let files = rx(r#"\*\*\* (?:Update|Add|Delete) File: ([^\s"'\\]+)"#)
                            .captures_iter(&raw)
                            .map(|m| base(&m[1]))
                            .collect::<Vec<_>>();
                        let cmd = rx(r#"cmd\s*:\s*"((?:[^"\\]|\\.)*)"#)
                            .captures(&raw)
                            .map(|m| m[1].to_string())
                            .unwrap_or_else(|| s(p, "name"));
                        self.act(
                            t,
                            if files.is_empty() {
                                format!("Bash: {cmd}")
                            } else {
                                format!("Edit: {}", files.join(", "))
                            },
                            String::new(),
                            format!("{} {}", s(p, "name"), cut(&raw, 6000)),
                        )
                    }
                    "custom_tool_call_output" | "function_call_output" => {
                        self.errs += usize::from(
                            rx(r"exit code:? [1-9]|Process exited with code [1-9]")
                                .is_match(&cut(&s(p, "output"), 300)),
                        )
                    }
                    "message" => {
                        let tx = text(&p["content"]);
                        if !tx.is_empty() {
                            if s(p, "role") == "assistant" {
                                self.say = tx.clone();
                                self.act(t, format!("dice: {tx}"), String::new(), String::new())
                            } else if s(p, "role") == "user"
                                && !tx.starts_with(['<'])
                                && !tx.starts_with("# AGENTS.md")
                                && !tx.starts_with("#AGENTS")
                            {
                                self.ask(tx, t)
                            }
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
            return;
        }
        let m = &e["message"];
        if ty == "assistant" {
            if !s(m, "model").is_empty() {
                self.model = s(m, "model")
            }
            for b in arr(&m["content"]) {
                if s(&b, "type") == "tool_use" {
                    let name = s(&b, "name");
                    let a = &b["input"];
                    self.wait = if name == "AskUserQuestion"
                        && !s(&a["questions"][0], "question").is_empty()
                    {
                        s(&a["questions"][0], "question")
                    } else {
                        format!("Pide permiso: {}", tool_line(&name, a))
                    };
                    self.act(
                        t,
                        tool_line(&name, a),
                        s(a, "description"),
                        format!("{name} {}", cut(&a.to_string(), 6000)),
                    )
                } else if s(&b, "type") == "text" && !s(&b, "text").trim().is_empty() {
                    self.say = s(&b, "text").trim().into();
                    self.act(
                        t,
                        format!("dice: {}", s(&b, "text")),
                        String::new(),
                        String::new(),
                    )
                }
            }
        } else if ty == "user" {
            for b in arr(&m["content"]) {
                if s(&b, "type") == "tool_result" {
                    self.wait.clear();
                    self.errs += usize::from(b["is_error"] == true)
                }
            }
            if let Some(tx) = user(e) {
                self.ask(tx, t)
            }
        }
    }
}
static FEEDS: LazyLock<Mutex<HashMap<String, Feed>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
pub(super) fn feed(path: &str, codex: bool) -> Feed {
    if path.is_empty() {
        return Feed::default();
    }
    let mut all = FEEDS.lock();
    let f = all.entry(path.into()).or_insert_with(|| Feed {
        path: path.into(),
        codex,
        ..Default::default()
    });
    let Ok(meta) = fs::metadata(path) else {
        return f.clone();
    };
    let sz = meta.len();
    let stamp = mtime(path);
    if f.started && sz == f.off && stamp == f.stamp {
        return f.clone();
    }
    if sz < f.off {
        *f = Feed {
            path: path.into(),
            codex,
            ..Default::default()
        }
    }
    let fresh = !f.started;
    let Ok(mut fh) = fs::File::open(path) else {
        return f.clone();
    };
    if fresh {
        if codex || path.starts_with(&agy_dir()) && sz > 1_500_000 {
            let mut first = String::new();
            use std::io::BufRead;
            let _ = std::io::BufReader::new(&mut fh)
                .take(2_000_000)
                .read_line(&mut first);
            if let Ok(v) = serde_json::from_str::<Value>(&first) {
                f.eat(&v)
            }
        }
        f.off = sz.saturating_sub(1_500_000);
        f.started = true;
    }
    let _ = fh.seek(SeekFrom::Start(f.off));
    let mut data = vec![];
    let _ = fh.take(8_000_000).read_to_end(&mut data);
    let mut start = f.off;
    if fresh && start > 0 {
        let i = data
            .iter()
            .position(|b| *b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(data.len());
        start += i as u64;
        data.drain(..i);
    }
    if let Some(cut) = data.iter().rposition(|b| *b == b'\n') {
        f.off = start + cut as u64 + 1;
        for l in data[..cut].split(|b| *b == b'\n') {
            if l.len() > 300_000
                && !String::from_utf8_lossy(&l[..l.len().min(200)]).contains("\"session_meta\"")
            {
                continue;
            }
            if let Ok(e) = serde_json::from_slice::<Value>(l) {
                f.eat(&e)
            }
        }
    } else {
        f.off = start
    }
    f.stamp = stamp;
    f.clone()
}
#[derive(Default)]
struct Use {
    off: u64,
    ids: HashMap<String, [i64; 3]>,
    i: i64,
    o: i64,
    c: i64,
    turns: usize,
    last: i64,
    models: BTreeMap<String, [i64; 3]>,
    cur: String,
    tot: [i64; 3],
    hist: BTreeMap<i64, [i64; 2]>,
}
static USE: LazyLock<Mutex<HashMap<String, Use>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
pub(super) fn usage(path: &str, codex: bool) -> Value {
    if path.is_empty() {
        return json!({"in":0,"out":0,"cache":0,"turns":0});
    }
    let mut all = USE.lock();
    let u = all.entry(path.into()).or_default();
    let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size < u.off {
        *u = Use::default()
    }
    let agy = path.starts_with(&agy_dir());
    if let Ok(mut file) = fs::File::open(path) {
        while u.off < size {
            let _ = file.seek(SeekFrom::Start(u.off));
            let mut data = vec![];
            let _ = (&mut file).take(16_000_000).read_to_end(&mut data);
            let Some(end) = data.iter().rposition(|b| *b == b'\n') else {
                if data.len() < 16_000_000 {
                    break;
                }
                u.off += data.len() as u64;
                continue;
            };
            u.off += end as u64 + 1;
            for line in data[..end].split(|b| *b == b'\n') {
                let tx = String::from_utf8_lossy(line);
                if !tx.contains(if agy {
                    "\"input_tokens\""
                } else if codex {
                    "\"token_count\""
                } else {
                    "\"usage\""
                }) && !tx.contains("is_error")
                    && !tx.contains("_call_output")
                    && !tx.contains("turn_context")
                    && !(agy
                        && (tx.contains("ERROR")
                            || tx.contains("INVALID")
                            || tx.contains("exited with code")))
                {
                    continue;
                }
                let Ok(e) = serde_json::from_slice::<Value>(line) else {
                    continue;
                };
                let p = &e["payload"];
                if codex && s(&e, "type") == "turn_context" {
                    if !s(p, "model").is_empty() {
                        u.cur = s(p, "model")
                    }
                    continue;
                }
                let slot = e["timestamp"]
                    .as_str()
                    .or(e["created_at"].as_str())
                    .map(|s| (iso(s) / 600.) as i64);
                let was = u.i + u.o;
                let err = if agy {
                    usize::from(agy_err(&e))
                } else if codex {
                    usize::from(
                        rx(r"exit code:? [1-9]|Process exited with code [1-9]")
                            .is_match(&cut(&s(p, "output"), 300)),
                    )
                } else if s(&e, "type") == "user" {
                    arr(&e["message"]["content"])
                        .iter()
                        .filter(|b| s(b, "type") == "tool_result" && b["is_error"] == true)
                        .count()
                } else {
                    0
                };
                if let Some(k) = slot {
                    u.hist.entry(k).or_default()[1] += err as i64
                }
                let (new, model, delta) = if agy {
                    if !["PLANNER_RESPONSE", "CHECKPOINT"].contains(&s(&e, "type").as_str()) {
                        continue;
                    }
                    u.turns += usize::from(s(&e, "type") == "PLANNER_RESPONSE");
                    (
                        [
                            n(&e, "input_tokens") as i64,
                            n(&e, "output_tokens") as i64,
                            n(&e, "cache_read_tokens") as i64,
                        ],
                        s(&e, "model"),
                        true,
                    )
                } else if codex {
                    if s(&e, "type") != "event_msg"
                        || s(p, "type") != "token_count"
                        || !p["info"]["total_token_usage"].is_object()
                    {
                        continue;
                    }
                    let t = &p["info"]["total_token_usage"];
                    let total = n(t, "total_tokens") as i64;
                    if total != u.last {
                        u.turns += 1
                    }
                    u.last = total;
                    (
                        [
                            ((n(t, "input_tokens") - n(t, "cached_input_tokens")) as i64).max(0),
                            n(t, "output_tokens") as i64,
                            n(t, "cached_input_tokens") as i64,
                        ],
                        u.cur.clone(),
                        false,
                    )
                } else {
                    if s(&e, "type") != "assistant" {
                        continue;
                    }
                    let m = &e["message"];
                    let x = &m["usage"];
                    if !x.is_object() {
                        continue;
                    }
                    let new = [
                        (n(x, "input_tokens") + n(x, "cache_creation_input_tokens")) as i64,
                        n(x, "output_tokens") as i64,
                        n(x, "cache_read_input_tokens") as i64,
                    ];
                    let id = m["id"]
                        .as_str()
                        .or(e["uuid"].as_str())
                        .unwrap_or("")
                        .to_string();
                    let old = u.ids.insert(id, new).unwrap_or_default();
                    u.turns = u.ids.len();
                    (
                        [new[0] - old[0], new[1] - old[1], new[2] - old[2]],
                        s(m, "model"),
                        true,
                    )
                };
                let d = if delta {
                    new
                } else {
                    [new[0] - u.tot[0], new[1] - u.tot[1], new[2] - u.tot[2]]
                };
                let mm = u
                    .models
                    .entry(if model.is_empty() { "?".into() } else { model })
                    .or_default();
                for k in 0..3 {
                    mm[k] += d[k]
                }
                if delta {
                    u.i += new[0];
                    u.o += new[1];
                    u.c += new[2]
                } else {
                    u.i = new[0];
                    u.o = new[1];
                    u.c = new[2];
                    u.tot = new
                }
                if let Some(k) = slot {
                    if u.i + u.o > was {
                        u.hist.entry(k).or_default()[0] += u.i + u.o - was
                    }
                }
            }
        }
    }
    let slot = (now() / 600.) as i64;
    u.hist.retain(|k, _| *k > slot - 36);
    let hist = (slot - 35..=slot)
        .map(|k| u.hist.get(&k).copied().unwrap_or_default())
        .collect::<Vec<_>>();
    let mut v = json!({"in":u.i,"out":u.o,"cache":u.c,"turns":u.turns});
    if hist.iter().any(|a| a[0] != 0 || a[1] != 0) {
        v["hist"] = json!(hist)
    }
    v
}
pub(super) fn usage_models(path: &str, codex: bool) -> Value {
    usage(path, codex);
    json!(
        USE.lock()
            .get(path)
            .map(|u| u
                .models
                .iter()
                .filter(|(_, a)| a.iter().any(|n| *n != 0))
                .map(|(k, v)| (k.clone(), *v))
                .collect::<BTreeMap<_, _>>())
            .unwrap_or_default()
    )
}
fn tail(path: &str, size: u64) -> Vec<Value> {
    let Ok(mut f) = fs::File::open(path) else {
        return vec![];
    };
    let off = f
        .metadata()
        .map(|m| m.len().saturating_sub(size))
        .unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(off));
    let mut data = vec![];
    let _ = f.read_to_end(&mut data);
    data.split(|b| *b == b'\n')
        .skip(usize::from(off > 0))
        .filter(|l| l.len() <= 2_000_000)
        .filter_map(|l| serde_json::from_slice(l).ok())
        .collect()
}
pub(super) fn parts_of(path: &str, codex: bool) -> Value {
    let mut out = vec![];
    for size in [6_000_000, 24_000_000, 48_000_000] {
        out = parts(path, codex, size);
        if out.len() >= 80 || size >= fs::metadata(path).map(|m| m.len()).unwrap_or(0) {
            break;
        }
    }
    json!(out)
}
fn parts(path: &str, codex: bool, size: u64) -> Vec<Value> {
    let mut out: Vec<Value> = vec![];
    let mut byid: HashMap<String, usize> = HashMap::new();
    let mut pending = std::collections::VecDeque::new();
    for e in tail(path, size) {
        let ty = s(&e, "type");
        let t = e["timestamp"]
            .as_str()
            .or(e["created_at"].as_str())
            .map(iso)
            .unwrap_or(0.);
        let mut tx = String::new();
        let mut role = "";
        if path.starts_with(&agy_dir()) {
            match ty.as_str() {
                "USER_INPUT" => {
                    tx = agy_user(&e);
                    role = "user"
                }
                "SYSTEM_MESSAGE" => {
                    let (sender, x) = agy_sys(&e);
                    if !x.is_empty() && !sender.contains("/task-") {
                        tx = format!("[Mensaje del equipo] {}", cut(&x, 1400));
                        role = "user"
                    }
                }
                "PLANNER_RESPONSE" => {
                    pending.clear();
                    let mut posted = String::new();
                    for tc in arr(&e["tool_calls"]) {
                        let (name, a) = agy_tool(&tc);
                        pending.push_back(out.len());
                        out.push(json!({"t":t,"role":"assistant","kind":"tool","text":"","name":name,"arg":tool_arg(&name,&a),"ok":true,"out":""}));
                        if ["post_message", "send_message", "finish"]
                            .contains(&s(&tc, "name").as_str())
                        {
                            posted = s(&a, "message")
                        }
                    }
                    tx = s(&e, "content").trim().into();
                    if tx.is_empty() {
                        tx = posted
                    }
                    role = "assistant"
                }
                "GENERIC" | "ERROR_MESSAGE" => {
                    if let Some(i) = pending.pop_front() {
                        out[i]["ok"] = json!(!agy_err(&e));
                        out[i]["out"] = json!(cut(
                            e["error"].as_str().filter(|s| !s.is_empty()).unwrap_or(
                                &rx(r"^(?:(?:Created|Completed) At: \S+\n)+")
                                    .replace(&s(&e, "content"), "")
                            ),
                            400
                        ));
                    }
                }
                _ => {}
            }
        } else if codex {
            if ty != "response_item" {
                continue;
            }
            let p = &e["payload"];
            match s(p, "type").as_str() {
                "message" => {
                    tx = text(&p["content"]);
                    role = if s(p, "role") == "assistant" {
                        "assistant"
                    } else {
                        "user"
                    };
                    if role == "user"
                        && (tx.starts_with('<')
                            || tx.starts_with("# AGENTS.md")
                            || tx.starts_with("#AGENTS"))
                    {
                        tx.clear()
                    }
                }
                "custom_tool_call" | "function_call" => {
                    let (name, arg, _) = call(p);
                    byid.insert(s(p, "call_id"), out.len());
                    out.push(json!({"t":t,"role":"assistant","kind":"tool","text":"","name":name,"arg":sq(&arg,160),"ok":true,"out":""}));
                }
                "custom_tool_call_output" | "function_call_output" => {
                    if let Some(i) = byid.get(&s(p, "call_id")) {
                        let o = result_text(&p["output"]);
                        out[*i]["out"] = json!(cut(&o, 400));
                        out[*i]["ok"]=json!(!rx(r#"exit code:? [1-9]|Process exited with code [1-9]|"exit_code":\s*[1-9]"#).is_match(&cut(&o,600)))
                    }
                }
                _ => {}
            }
        } else {
            let m = &e["message"];
            if ty == "assistant" {
                for b in arr(&m["content"]) {
                    if s(&b, "type") == "text" && !s(&b, "text").trim().is_empty() {
                        out.push(json!({"t":t,"role":"assistant","kind":"text","text":cut(s(&b,"text").trim(),1500)}))
                    } else if s(&b, "type") == "tool_use" {
                        byid.insert(s(&b, "id"), out.len());
                        let name = s(&b, "name");
                        out.push(json!({"t":t,"role":"assistant","kind":"tool","text":"","name":name,"arg":tool_arg(&name,&b["input"]),"ok":true,"out":""}));
                    }
                }
            } else if ty == "user" {
                for b in arr(&m["content"]) {
                    if s(&b, "type") == "tool_result" {
                        if let Some(i) = byid.get(&s(&b, "tool_use_id")) {
                            out[*i]["ok"] = json!(b["is_error"] != true);
                            out[*i]["out"] = json!(cut(&result_text(&b["content"]), 400));
                        }
                    }
                }
                tx = user(&e).unwrap_or_default();
                role = "user"
            }
        }
        if !tx.trim().is_empty() {
            out.push(json!({"t":t,"role":role,"kind":"text","text":cut(tx.trim(),1500)}));
        }
    }
    out.into_iter()
        .rev()
        .take(80)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}
