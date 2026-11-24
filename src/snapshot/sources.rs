use super::*;
use rusqlite::{Connection, OpenFlags};
pub(super) fn codex_files(age: f64, arch: bool) -> Vec<(String, String)> {
    let mut out = vec![];
    let primary_label = std::env::var("AGENCIA_CODEX_NAME")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "Codex".into());
    for (who_key, path) in agencia_core::jobs::discover_codex_homes() {
        let acct = if who_key == "default" {
            primary_label.clone()
        } else {
            format!("Codex {who_key}")
        };
        let base = path.display().to_string();
        for d in 0..2 {
            let day = (chrono::Local::now() - chrono::Duration::days(d))
                .format("%Y/%m/%d")
                .to_string();
            for f in glob(&format!("{base}/sessions/{day}/rollout-*.jsonl")) {
                if now() - mtime(&f) < age {
                    out.push((f, acct.clone()))
                }
            }
        }
        if arch {
            for f in glob(&format!("{base}/archived_sessions/rollout-*.jsonl")) {
                if now() - mtime(&f) < age {
                    out.push((f, acct.clone()))
                }
            }
        }
    }
    out
}
pub(super) fn rollout(home_name: &str, id: &str) -> String {
    if id.is_empty() {
        return String::new();
    }
    let homes = agencia_core::jobs::discover_codex_homes();
    let base = homes
        .get(home_name)
        .cloned()
        .unwrap_or_else(|| std::path::PathBuf::from(home()).join(".codex"));
    glob(&format!(
        "{}/sessions/*/*/*/rollout-*-{id}.jsonl",
        base.display()
    ))
    .into_iter()
    .chain(glob(&format!(
        "{}/archived_sessions/rollout-*-{id}.jsonl",
        base.display()
    )))
    .next()
    .unwrap_or_default()
}
pub(super) fn job_feed(j: &JobRecord, cx: &[(String, String)]) -> Feed {
    if j.kind == "cla" {
        feed(&transcript(&j.thread, &j.repo), false)
    } else if j.kind == "cdx" {
        cx.iter()
            .map(|(f, _)| feed(f, true))
            .find(|f| s(&f.meta, "id") == j.thread)
            .unwrap_or_else(|| feed(&rollout(&j.home, &j.thread), true))
    } else {
        Feed::default()
    }
}
pub(super) fn claude_subs(tx: &str, repo: &str) -> Vec<Value> {
    let mut kids = vec![];
    for path in glob(&format!(
        "{}/subagents/agent-*.jsonl",
        tx.strip_suffix(".jsonl").unwrap_or(tx)
    )) {
        let meta = jload(&path.replace(".jsonl", ".meta.json"));
        let age = now() - mtime(&path);
        let id = format!(
            "sub:{}",
            base(&path)
                .trim_start_matches("agent-")
                .trim_end_matches(".jsonl")
        );
        if crate::shared::hidden(&id) || age > 900. {
            continue;
        }
        let f = feed(&path, false);
        let stopped = fs::File::open(&path)
            .ok()
            .and_then(|mut h| {
                let len = h.metadata().ok()?.len();
                h.seek(SeekFrom::Start(len.saturating_sub(1500))).ok()?;
                let mut b = String::new();
                h.read_to_string(&mut b).ok()?;
                Some(b.contains("Request interrupted"))
            })
            .unwrap_or(false);
        let done = meta["finished"].as_bool().unwrap_or(false) || age > 180. || stopped;
        let title = meta["description"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or(meta["agentType"].as_str())
            .unwrap_or("subagente");
        let mut c = card(
            &id,
            "sub",
            title,
            meta["agentType"].as_str().unwrap_or("Subagente"),
            &f.first,
            if done { "done" } else { "working" },
            &f,
            &if done {
                md(&f.say, 160)
            } else {
                line(&id, &f.first, &f)
            },
            mtime(&path),
        );
        if meta["model"].is_string() {
            c["model"] = meta["model"].clone()
        }
        c["repo"] = json!(repo);
        c["cuenta"] = json!("claude");
        kids.push(c)
    }
    kids
}
pub(super) fn codex_subs(parent: &str, cx: &[(String, String)], depth: usize) -> Vec<Value> {
    if depth > 6 {
        return vec![];
    }
    let mut kids = vec![];
    for (path, acct) in cx {
        let f = feed(path, true);
        if s(&f.meta, "parent") != parent {
            continue;
        }
        let id = format!("csub:{}", s(&f.meta, "id"));
        let done = !f.busy || now() - f.t1 > 300.;
        let nick = s(&f.meta, "nick");
        let mut c = card(
            &id,
            "sub",
            if nick.is_empty() { "subagente" } else { &nick },
            acct,
            &f.first,
            if done { "done" } else { "working" },
            &f,
            &if done {
                md(&f.say, 160)
            } else {
                line(&id, &f.first, &f)
            },
            f.t1,
        );
        c["repo"] = json!(repo_of(&s(&f.meta, "cwd")));
        c["cuenta"] = json!(acct);
        c["children"] = json!(codex_subs(&s(&f.meta, "id"), cx, depth + 1));
        kids.push(c)
    }
    kids
}
pub(super) fn sort_cards(cards: &mut [Value]) {
    cards.sort_by(|a, b| {
        (s(a, "status") != "working")
            .cmp(&(s(b, "status") != "working"))
            .then_with(|| n(b, "updated").total_cmp(&n(a, "updated")))
    })
}
pub(super) fn who(j: &JobRecord) -> String {
    match j.kind.as_str() {
        "bec" => "Becario".into(),
        "cla" => "Claude".into(),
        _ if j.home.is_empty() || j.home == "default" => {
            std::env::var("AGENCIA_CODEX_NAME")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "Codex".into())
        }
        _ => format!("Codex {}", j.home),
    }
}
pub(super) fn job_card(
    j: &JobRecord,
    jobs: &[JobRecord],
    cx: &[(String, String)],
    ps: &procs::ProcSnapshot,
    depth: usize,
) -> Value {
    let mut f = job_feed(j, cx);
    if f.ev.is_empty() {
        f.ev = j
            .acts
            .iter()
            .map(|raw| Event {
                t: if j.updated != 0. { j.updated } else { j.ts },
                raw: raw.clone(),
                desc: String::new(),
            })
            .collect()
    }
    let paused = ["q", "r"].contains(&j.status.as_str())
        && j.kind == "cdx"
        && j.pid
            .and_then(|p| ps.procs.get(&p))
            .is_some_and(|p| p.stat.contains('T'));
    let st = if paused {
        "idle"
    } else {
        match j.status.as_str() {
            "q" => "queued",
            "r" => "working",
            "ok" => "done",
            "fallo" => "failed",
            "pregunta" => "asks",
            _ => "failed",
        }
    };
    let id = format!("job:{}", j.id);
    let l = if paused {
        "En pausa (SIGSTOP)".into()
    } else if st == "queued" {
        "En cola".into()
    } else if st == "working" {
        let l = line(&id, &j.text, &f);
        if l.is_empty() {
            if j.brief.is_empty() {
                "Trabajando".into()
            } else {
                j.brief.clone()
            }
        } else {
            l
        }
    } else {
        rx(r"RESUMEN:\s*(.+)")
            .captures(&j.reply)
            .map(|c| c[1].into())
            .unwrap_or_else(|| j.reply.clone())
    };
    let mut c = card(
        &id,
        &j.kind,
        if j.brief.is_empty() {
            &j.text
        } else {
            &j.brief
        },
        &who(j),
        &j.text,
        st,
        &f,
        &md(&l, 160),
        if j.updated != 0. {
            j.updated
        } else if j.ended != 0. {
            j.ended
        } else {
            j.ts
        },
    );
    if !j.model.is_empty() {
        c["model"] = json!(j.model)
    } else if f.model.is_empty() && j.kind == "cdx" {
        let base = agencia_core::jobs::discover_codex_homes()
            .get(&j.home)
            .cloned()
            .unwrap_or_else(|| std::path::PathBuf::from(home()).join(".codex"));
        let cfg = fs::read_to_string(base.join("config.toml")).unwrap_or_default();
        if let Some(m) = rx(r#"(?m)^model\s*=\s*"([^"]+)""#).captures(&cfg) {
            c["model"] = json!(&m[1])
        }
    }
    if j.kind == "bec" {
        c["usage"] = bec_usage(j)
    }
    c["repo"] = json!(repo_of(&j.repo));
    c["cuenta"] = json!(if j.home.is_empty() {
        if j.kind == "cla" {
            "claude"
        } else {
            "opencode"
        }
    } else {
        &j.home
    });
    c["reply"] = json!(if ["working", "queued"].contains(&st) {
        String::new()
    } else {
        cut(&j.reply, 3000)
    });
    c["pid"] = json!(j.pid);
    c["paused"] = json!(paused);
    c["live"] = json!(["q", "r"].contains(&j.status.as_str()));
    let mut kids = vec![];
    if depth < 12 {
        if j.kind == "cdx" && !j.thread.is_empty() {
            kids.extend(codex_subs(&j.thread, cx, 0))
        }
        if j.kind == "cla" && !j.thread.is_empty() {
            kids.extend(claude_subs(&f.path, &repo_of(&j.repo)))
        }
        for k in jobs {
            if k.de == id && k.id != j.id {
                kids.push(job_card(k, jobs, cx, ps, depth + 1))
            }
        }
    }
    sort_cards(&mut kids);
    c["children"] = json!(kids);
    c
}
static OC: LazyLock<Mutex<(f64, Vec<Value>)>> = LazyLock::new(|| Mutex::new((0., vec![])));
fn oc_db() -> Option<Connection> {
    let c = Connection::open_with_flags(
        format!("{}/.local/share/opencode/opencode.db", home()),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let _ = c.busy_timeout(std::time::Duration::from_millis(200));
    Some(c)
}
pub(super) fn oc_sessions() -> Vec<Value> {
    let mut cache = OC.lock();
    if now() - cache.0 < 10. {
        return cache.1.clone();
    }
    if let Some(c) = oc_db() {
        if let Ok(mut q)=c.prepare("select id,title,directory,model,time_updated,cost,tokens_input,tokens_output,tokens_reasoning,tokens_cache_read,tokens_cache_write from session_v2 order by time_updated desc"){if let Ok(rows)=q.query_map([],|r|{let model:String=r.get::<_,Option<String>>(3)?.unwrap_or_default();Ok(json!({"id":r.get::<_,String>(0)?,"title":r.get::<_,Option<String>>(1)?.unwrap_or_default(),"location":{"directory":r.get::<_,String>(2)?},"model":serde_json::from_str::<Value>(&model).unwrap_or(Value::Null),"time":{"updated":r.get::<_,i64>(4)?},"cost":r.get::<_,f64>(5)?,"tokens":{"input":r.get::<_,i64>(6)?,"output":r.get::<_,i64>(7)?,"reasoning":r.get::<_,i64>(8)?,"cache":{"read":r.get::<_,i64>(9)?,"write":r.get::<_,i64>(10)?}}}))}){cache.1=rows.flatten().collect()}}
    }
    cache.0 = now();
    cache.1.clone()
}
pub(super) fn oc_msgs(id: &str) -> Vec<Value> {
    let Some(c) = oc_db() else { return vec![] };
    let Ok(mut q) =
        c.prepare("select type,data from session_message where session_id=?1 order by seq desc")
    else {
        return vec![];
    };
    q.query_map([id], |r| {
        let data: String = r.get(1)?;
        let mut v: Value = serde_json::from_str(&data).unwrap_or(json!({}));
        v["type"] = json!(r.get::<_, String>(0)?);
        Ok(v)
    })
    .map(|r| r.flatten().collect())
    .unwrap_or_default()
}
pub(super) fn oc_parts(ms: &[Value]) -> Value {
    let mut out = vec![];
    for m in ms.iter().rev() {
        let role = s(m, "type");
        if !["user", "assistant"].contains(&role.as_str()) {
            continue;
        }
        let t = n(&m["time"], "created") / 1000.;
        if role == "user" && !s(m, "text").trim().is_empty() {
            out.push(json!({"t":t,"role":role,"kind":"text","text":cut(s(m,"text").trim(),6000)}))
        }
        for b in arr(&m["content"]) {
            let ty = s(&b, "type");
            if ty == "text" && !s(&b, "text").trim().is_empty() {
                out.push(
                    json!({"t":t,"role":role,"kind":"text","text":cut(s(&b,"text").trim(),1500)}),
                )
            } else if ty == "tool" {
                let st = &b["state"];
                let name = b["name"].as_str().or(b["tool"].as_str()).unwrap_or("?");
                out.push(json!({"t":t,"role":"assistant","kind":"tool","text":"","name":name,"arg":tool_arg(name,&st["input"]),"ok":s(st,"status")!="error","out":cut(&result_text(st.get("content").or(st.get("output")).unwrap_or(&Value::Null)),400)}));
            }
        }
    }
    json!(
        out.into_iter()
            .rev()
            .take(80)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
    )
}
pub(super) fn oc_usage(x: &Value) -> Value {
    let t = &x["tokens"];
    json!({"in":n(t,"input") as i64,"out":(n(t,"output")+n(t,"reasoning")) as i64,"cache":(n(&t["cache"],"read")+n(&t["cache"],"write")) as i64,"turns":0,"usd":(n(x,"cost")*10000.).round()/10000.})
}
pub(super) fn bec_usage(j: &JobRecord) -> Value {
    let x = oc_sessions()
        .into_iter()
        .find(|v| s(v, "title") == format!("bec-{}", j.id))
        .unwrap_or(Value::Null);
    let mut v = oc_usage(&x);
    v["turns"] = json!(j.steps);
    v
}
pub(super) fn oc_windows(ps: &procs::ProcSnapshot) -> Vec<Value> {
    let all = oc_sessions();
    let mut cw: HashMap<String, usize> = HashMap::new();
    for p in ps.procs.values() {
        if p.tty != "??"
            && base(&p.comm) == "opencode"
            && !rx(r"\b(serve|run|api)\b").is_match(&p.args)
        {
            *cw.entry(p.cwd.clone()).or_default() += 1
        }
    }
    let mine = all
        .iter()
        .filter(|x| !s(x, "title").starts_with("bec-"))
        .collect::<Vec<_>>();
    let mut tui = HashSet::new();
    for (dir, n) in cw {
        for x in mine
            .iter()
            .filter(|x| s(&x["location"], "directory") == dir)
            .take(n)
        {
            tui.insert(s(x, "id"));
        }
    }
    let selected = mine
        .iter()
        .filter(|x| tui.contains(&s(x, "id")))
        .copied()
        .collect::<Vec<_>>();
    let mut out = vec![];
    for x in selected {
        let id = format!("oc:{}", s(x, "id"));
        if crate::shared::hidden(&id) {
            continue;
        }
        let ms = oc_msgs(&s(x, "id"));
        let parts = arr(&oc_parts(&ms));
        let tools = parts
            .iter()
            .filter(|p| s(p, "kind") == "tool")
            .collect::<Vec<_>>();
        let us = parts
            .iter()
            .filter(|p| s(p, "role") == "user" && s(p, "kind") == "text")
            .collect::<Vec<_>>();
        let last = ms.first().cloned().unwrap_or(Value::Null);
        let upd = n(&x["time"], "updated") / 1000.;
        let busy = s(&last, "type") == "user"
            || s(&last, "type") == "assistant" && last["time"]["completed"].is_null();
        let st = if busy && now() - upd < 600. {
            "working"
        } else if busy && tui.contains(&s(x, "id")) && now() - upd < 7200. {
            "stalled"
        } else {
            "idle"
        };
        let say = parts
            .iter()
            .rev()
            .find(|p| s(p, "role") == "assistant" && s(p, "kind") == "text")
            .map(|p| s(p, "text"))
            .unwrap_or_default();
        let mut usage = oc_usage(x);
        usage["turns"] = json!(tools.len());
        let ev = tools
            .iter()
            .rev()
            .take(8)
            .rev()
            .map(|p| Event {
                t: n(p, "t"),
                raw: format!("{}: {}", s(p, "name"), s(p, "arg")),
                desc: String::new(),
            })
            .collect::<Vec<_>>();
        let cwd = s(&x["location"], "directory");
        out.push(json!({"id":id,"kind":"opencode","account":"opencode","title":x["title"].as_str().filter(|s|!s.is_empty()).unwrap_or(&cut(&s(x,"id"),8)),"repo":repo_of(&cwd),"cwd":cwd.replace(&home(),"~"),"status":st,"request":us.last().map(|p|cut(&s(p,"text"),6000)).unwrap_or_default(),"now":if !tools.is_empty()&&st=="working"{format!("{}: {}",s(tools[tools.len()-1],"name"),s(tools[tools.len()-1],"arg"))}else if say.is_empty(){"Sin actividad reciente".into()}else{md(&say,160)},"steps":steps(&ev,5),"errs":tools.iter().filter(|p|p["ok"]==false).count(),"updated":upd,"model":s(&x["model"],"id"),"usage":usage,"children":[]}));
    }
    out
}
pub(super) fn agy_dir() -> String {
    format!("{}/.gemini/antigravity-cli", home())
}
pub(super) fn agy_tx(id: &str) -> String {
    let d = format!("{}/brain/{id}/.system_generated/logs", agy_dir());
    let full = format!("{d}/transcript_full.jsonl");
    if Path::new(&full).exists() {
        full
    } else {
        format!("{d}/transcript.jsonl")
    }
}
static AGY: LazyLock<Mutex<(f64, Vec<Value>, HashMap<String, String>)>> =
    LazyLock::new(|| Mutex::new((0., vec![], HashMap::new())));
fn proto_varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let mut val = 0u64;
    let mut shift = 0u32;
    while *i < b.len() && shift < 64 {
        let byte = b[*i];
        *i += 1;
        val |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some(val);
        }
        shift += 7;
    }
    None
}

fn proto_field_bytes<'a>(b: &'a [u8], target_field: u64) -> Option<&'a [u8]> {
    let mut i = 0;
    while i < b.len() {
        let tag = proto_varint(b, &mut i)?;
        let field = tag >> 3;
        let wire = tag & 7;
        match wire {
            0 => {
                proto_varint(b, &mut i)?;
            }
            1 => {
                if i + 8 > b.len() {
                    return None;
                }
                i += 8;
            }
            2 => {
                let len = proto_varint(b, &mut i)? as usize;
                if i + len > b.len() {
                    return None;
                }
                let slice = &b[i..i + len];
                i += len;
                if field == target_field {
                    return Some(slice);
                }
            }
            5 => {
                if i + 4 > b.len() {
                    return None;
                }
                i += 4;
            }
            _ => return None,
        }
    }
    None
}

/// Extracts the human-readable subagent role from Antigravity's `raw_summary` protobuf blob
/// (`metadata` field 17 -> `subagent_info` field 8 -> `role` field 2).
pub(super) fn agy_proto_role(blob: &[u8]) -> String {
    let Some(meta) = proto_field_bytes(blob, 17) else {
        return String::new();
    };
    let Some(info) = proto_field_bytes(meta, 8) else {
        return String::new();
    };
    let Some(role) = proto_field_bytes(info, 2) else {
        return String::new();
    };
    std::str::from_utf8(role)
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

pub(super) fn agy_rows() -> (Vec<Value>, HashMap<String, String>) {
    let mut cache = AGY.lock();
    if now() - cache.0 < 3. {
        return (cache.1.clone(), cache.2.clone());
    }
    if let Ok(c) = Connection::open_with_flags(
        format!("{}/conversation_summaries.db", agy_dir()),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) {
        let _ = c.busy_timeout(std::time::Duration::from_millis(200));
        if let Ok(mut q) = c.prepare(
            "select conversation_id,title,status,step_count,last_modified_time,workspace_uris,agent_name,parent_conversation_id,not_fully_idle,killed,raw_summary from conversation_summaries",
        ) {
            if let Ok(rows) = q.query_map([], |r| {
                let id: String = r.get(0)?;
                let ts: String = r.get::<_, Option<String>>(4)?.unwrap_or_default();
                let dbt = chrono::NaiveDateTime::parse_from_str(&cut(&ts, 19), "%Y-%m-%d %H:%M:%S")
                    .map(|t| t.and_utc().timestamp().max(0) as f64)
                    .unwrap_or(0.);
                let ws: String = r.get::<_, Option<String>>(5)?.unwrap_or_default();
                let ws: Value = serde_json::from_str(&ws).unwrap_or(Value::Null);
                let cwd = ws[0].as_str().unwrap_or("").replace("file://", "");
                let not_fully_idle = r.get::<_, Option<i64>>(8)?.unwrap_or(0) != 0;
                let killed = r.get::<_, Option<i64>>(9)?.unwrap_or(0) != 0;
                let raw: Vec<u8> = r.get::<_, Option<Vec<u8>>>(10)?.unwrap_or_default();
                let role = agy_proto_role(&raw);
                Ok(json!({
                    "id": id,
                    "title": r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    "status": r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    "steps": r.get::<_, i64>(3)?,
                    "t": if dbt == 0. || now() - dbt < 86400. {
                        dbt.max(mtime(&agy_tx(&id)))
                    } else {
                        dbt
                    },
                    "cwd": cwd,
                    "agent": r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                    "parent": r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                    "not_fully_idle": not_fully_idle,
                    "killed": killed,
                    "role": role,
                }))
            }) {
                cache.1 = rows.flatten().collect()
            }
        }
    }
    let by_cwd: HashMap<String, String> =
        cache.1.iter().map(|r| (s(r, "id"), s(r, "cwd"))).collect();
    let by_parent: HashMap<String, String> = cache
        .1
        .iter()
        .map(|r| (s(r, "id"), s(r, "parent")))
        .collect();
    let agy_prefix = format!("{}/", agy_dir());
    for r in &mut cache.1 {
        let mut cur_cwd = s(r, "cwd");
        if cur_cwd.is_empty() || cur_cwd.starts_with(&agy_prefix) {
            let mut p = s(r, "parent");
            for _ in 0..8 {
                if p.is_empty() {
                    break;
                }
                if let Some(pcwd) = by_cwd.get(&p) {
                    if !pcwd.is_empty() && !pcwd.starts_with(&agy_prefix) {
                        cur_cwd = pcwd.clone();
                        break;
                    }
                }
                p = by_parent.get(&p).cloned().unwrap_or_default();
            }
            if !cur_cwd.is_empty() {
                r["cwd"] = json!(cur_cwd);
            }
        }
    }
    cache.2.clear();
    if let Ok(h) = fs::read_to_string(format!("{}/history.jsonl", agy_dir())) {
        for l in h.lines() {
            if let Ok(v) = serde_json::from_str::<Value>(l) {
                if s(&v, "type") != "slash_command" && !s(&v, "conversationId").is_empty() {
                    cache.2.insert(s(&v, "conversationId"), s(&v, "display"));
                }
            }
        }
    }
    cache.0 = now();
    (cache.1.clone(), cache.2.clone())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn agy_subs(
    parent_cid: &str,
    rows: &[Value],
    jobs: &[JobRecord],
    used: &mut HashSet<String>,
    cxa: &[(String, String)],
    ps: &procs::ProcSnapshot,
    asked: &HashMap<String, String>,
    tick: f64,
    depth: usize,
    seen: &mut HashSet<String>,
) -> Vec<Value> {
    let mut kids = vec![];
    if depth >= 10 {
        return kids;
    }
    for k in rows {
        if s(k, "parent") != parent_cid {
            continue;
        }
        let kid_id = s(k, "id");
        if kid_id.is_empty()
            || crate::shared::hidden(&format!("agy:{kid_id}"))
            || !seen.insert(kid_id.clone())
        {
            continue;
        }
        let sub_kids = agy_subs(
            &kid_id,
            rows,
            jobs,
            used,
            cxa,
            ps,
            asked,
            tick,
            depth + 1,
            seen,
        );
        let is_running = s(k, "status").ends_with("RUNNING");
        let not_idle = k["not_fully_idle"].as_bool().unwrap_or(false)
            && !k["killed"].as_bool().unwrap_or(false);
        let recent = tick - n(k, "t").max(mtime(&agy_tx(&kid_id))) < 900.;
        if !sub_kids.is_empty() || is_running || not_idle || recent {
            kids.push(agy_card(k, false, sub_kids, None, asked));
        }
    }
    let prefixed = format!("agy:{parent_cid}");
    for j in jobs {
        if (j.de == parent_cid || j.de == prefixed) && used.insert(j.id.clone()) {
            kids.push(job_card(j, jobs, cxa, ps, 0));
        }
    }
    sort_cards(&mut kids);
    kids
}

pub(super) fn agy_card(
    r: &Value,
    root: bool,
    kids: Vec<Value>,
    pid: Option<i32>,
    asked: &HashMap<String, String>,
) -> Value {
    let tx = agy_tx(&s(r, "id"));
    let tm = mtime(&tx);
    let f = feed(&tx, false);
    let killed = r["killed"].as_bool().unwrap_or(false);
    let live_kids = kids.iter().filter(|k| s(k, "status") == "working").count();
    let self_running =
        !killed && (s(r, "status").ends_with("RUNNING") || f.busy && now() - f.t1.max(tm) < 300.);
    let running = self_running || (!killed && !root && live_kids > 0);
    let st = if running {
        "working"
    } else if root {
        "idle"
    } else {
        "done"
    };
    let request = if !f.last.is_empty() {
        f.last.clone()
    } else if !f.first.is_empty() {
        f.first.clone()
    } else {
        asked.get(&s(r, "id")).cloned().unwrap_or_default()
    };
    let id = format!("agy:{}", s(r, "id"));
    let l = if self_running {
        line(&id, &request, &f)
    } else if running && live_kids > 0 {
        if live_kids == 1 {
            "Coordinando 1 subagente activo".into()
        } else {
            format!("Coordinando {live_kids} subagentes activos")
        }
    } else if !f.say.is_empty() {
        md(&f.say, 200)
    } else {
        f.ev.last()
            .map(|e| md(&e.raw, 200))
            .unwrap_or("Sin actividad reciente".into())
    };
    let upd = n(r, "t").max(f.t1).max(tm);
    let cwd = s(r, "cwd");
    let role = s(r, "role");
    let agent = s(r, "agent");
    let title = if root {
        if !s(r, "title").is_empty() {
            s(r, "title")
        } else if !base(&cwd).is_empty() {
            base(&cwd)
        } else {
            cut(&s(r, "id"), 8)
        }
    } else if !role.is_empty() {
        role
    } else if !agent.is_empty() {
        agent.clone()
    } else if !s(r, "title").is_empty() {
        s(r, "title")
    } else {
        "subagente".into()
    };
    if !root {
        let who = if !agent.is_empty() && title != agent {
            agent
        } else {
            "Antigravity".into()
        };
        let mut c = card(&id, "sub", &title, &who, &request, st, &f, &l, upd);
        c["repo"] = json!(repo_of(&cwd));
        c["cuenta"] = json!("antigravity");
        c["children"] = json!(kids);
        return c;
    }
    json!({"id":id,"kind":"agy","account":"antigravity","title":title,"repo":repo_of(&cwd),"cwd":cwd.replace(&home(),"~"),"status":st,"request":cut(&request,6000),"now":l,"steps":steps(&f.ev,5),"errs":f.errs,"updated":upd,"model":f.model,"usage":usage(&tx,false),"children":kids,"pid":pid,"linked":pid.is_some_and(|p|jload(&crate::shared::cache_dir().join(format!("agy/{p}.json")).to_string_lossy()).is_object()),"re":recent(&f)})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agy_proto_role_extracts_nested_field_17_8_2() {
        fn push_varint(out: &mut Vec<u8>, mut v: u64) {
            loop {
                let mut b = (v & 0x7f) as u8;
                v >>= 7;
                if v != 0 {
                    b |= 0x80;
                }
                out.push(b);
                if v == 0 {
                    break;
                }
            }
        }
        fn encode_len_field(field: u64, payload: &[u8]) -> Vec<u8> {
            let mut out = vec![];
            push_varint(&mut out, (field << 3) | 2);
            push_varint(&mut out, payload.len() as u64);
            out.extend_from_slice(payload);
            out
        }
        let info = [
            encode_len_field(1, b"teamwork_worker"),
            encode_len_field(2, b"M1 host loader and Win32/MSVCRT"),
        ]
        .concat();
        let meta = encode_len_field(8, &info);
        let raw = encode_len_field(17, &meta);
        assert_eq!(agy_proto_role(&raw), "M1 host loader and Win32/MSVCRT");
        assert_eq!(agy_proto_role(&[]), "");
    }
}
