//! Read-only Agencia board, transcript inspectors and evidence endpoints.
use agencia_core::{
    jobs::{JobManager, JobRecord, ShotRecord},
    procs,
};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::{Arc, LazyLock},
};
mod feed;
mod sources;
mod views;
use feed::*;
use sources::*;
pub use views::{activity, agent, repos};
fn home() -> String {
    crate::shared::home()
}
fn now() -> f64 {
    crate::shared::now()
}
static JOBS: LazyLock<JobManager> = LazyLock::new(|| JobManager::new(crate::shared::root()));
fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").into()
}
fn n(v: &Value, k: &str) -> f64 {
    v[k].as_f64().unwrap_or(0.)
}
fn arr(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}
fn cut(x: &str, n: usize) -> String {
    x.chars().take(n).collect()
}
fn sq(x: &str, n: usize) -> String {
    cut(&x.split_whitespace().collect::<Vec<_>>().join(" "), n)
}
fn rx(p: &str) -> regex::Regex {
    static R: LazyLock<Mutex<HashMap<String, regex::Regex>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    R.lock()
        .entry(p.into())
        .or_insert_with(|| regex::Regex::new(p).unwrap())
        .clone()
}
fn md(x: &str, n: usize) -> String {
    sq(&rx(r"[*`#>_]+").replace_all(x, ""), n)
}
fn untag(x: &str) -> String {
    rx(r"^\s*(\[[^\]]{1,24}\]\s*)+").replace(x, "").into_owned()
}
fn base(x: &str) -> String {
    Path::new(x)
        .file_name()
        .map(|x| x.to_string_lossy().into_owned())
        .unwrap_or_default()
}
fn glob(p: &str) -> Vec<String> {
    glob::glob(p)
        .into_iter()
        .flatten()
        .flatten()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}
fn mtime(p: &str) -> f64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.)
}
fn jload(p: &str) -> Value {
    fs::read(p)
        .ok()
        .and_then(|b| {
            serde_json::Deserializer::from_slice(&b)
                .into_iter::<Value>()
                .next()
                .and_then(Result::ok)
        })
        .unwrap_or(Value::Null)
}
fn repo_of(cwd: &str) -> String {
    let p = cwd.replace(&home(), "");
    let p = p.trim_matches('/');
    let mut it = p.split('/');
    let a = it.next().unwrap_or("");
    let a = if a == "Dashboards" {
        it.next().unwrap_or(a)
    } else {
        a
    };
    let a = a.strip_suffix("-wt").unwrap_or(a);
    if a.is_empty() { "~".into() } else { disk_case(a) }
}
// macOS paths are case-insensitive: ~/tony and ~/Tony are one folder, so name the repo as the disk spells it.
fn disk_case(name: &str) -> String {
    fs::read_dir(home())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.eq_ignore_ascii_case(name))
        .unwrap_or_else(|| name.into())
}
fn transcript(sid: &str, cwd: &str) -> String {
    let p = format!(
        "{}/.claude/projects/{}/{}.jsonl",
        home(),
        rx(r"[^A-Za-z0-9]").replace_all(cwd, "-"),
        sid
    );
    if Path::new(&p).exists() {
        p
    } else {
        glob(&format!("{}/.claude/projects/*/{sid}.jsonl", home()))
            .into_iter()
            .next()
            .unwrap_or_default()
    }
}
fn shots_of(id: &str) -> Vec<ShotRecord> {
    let (k, key) = id.split_once(':').unwrap_or(("", ""));
    match k {
        "job" => JOBS.shots(&format!("job:{key}")),
        "w" | "sub" => JOBS.shots(&format!("claude:{key}")),
        _ => vec![],
    }
}
fn shot_fields(c: &mut Value, id: &str) {
    let shots = shots_of(id);
    c["shot_count"] = json!(shots.len());
    c["shots"]=json!(shots.iter().enumerate().rev().map(|(i,s)|json!({"caption":s.caption,"ts":s.ts,"url":format!("/api/shot?id={}&index={i}",id.replace(':',"%3A"))})).collect::<Vec<_>>());
}
pub fn evidence_file(p: &str, base: &str) -> Result<(Vec<u8>, String), String> {
    if p.is_empty() {
        return Err("sin ruta".into());
    }
    let expand = |x: &str| {
        if x == "~" {
            home()
        } else if let Some(x) = x.strip_prefix("~/") {
            format!("{}/{x}", home())
        } else {
            x.into()
        }
    };
    let mut path = expand(p);
    if !Path::new(&path).is_absolute() {
        path = format!(
            "{}/{}",
            if base.is_empty() {
                home()
            } else {
                expand(base)
            },
            path
        )
    }
    let r = fs::canonicalize(path).map_err(|_| "el archivo ya no existe")?;
    let h = home();
    let rel = r
        .strip_prefix(&h)
        .map_err(|_| "fuera de las carpetas permitidas")?;
    if rel
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
        || rel.as_os_str().is_empty()
    {
        return Err("fuera de las carpetas permitidas".into());
    }
    if !r.is_file() {
        return Err("el archivo ya no existe".into());
    }
    let ext = r
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let ct = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "text/plain; charset=utf-8",
    };
    if ct.starts_with("text")&&!"md txt json jsonl tsv csv log py ts tsx js mjs cjs css html rs c h cpp sh toml yaml yml asm s ndjson svg".split_whitespace().any(|x|x==ext){return Err("tipo de archivo que no se muestra aquí".into())}
    let mut f = fs::File::open(r).map_err(|e| e.to_string())?;
    let mut b = vec![];
    if ct.starts_with("text") {
        f.take(2_000_000).read_to_end(&mut b)
    } else {
        f.read_to_end(&mut b)
    }
    .map_err(|e| e.to_string())?;
    Ok((b, ct.into()))
}
pub fn shot_png(id: &str, index: usize) -> Option<Vec<u8>> {
    use std::os::unix::fs::OpenOptionsExt;
    let shots = shots_of(id);
    let r = shots.get(index)?;
    let mut f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&r.path)
        .ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut b = vec![];
    f.read_to_end(&mut b).ok()?;
    b.starts_with(agencia_core::jobs::PNG_MAGIC).then_some(b)
}
fn human(raw: &str, d: &str) -> (String, String) {
    let (k, v) = raw.split_once(": ").unwrap_or((raw, ""));
    let k = match k {
        "bash" | "shell" | "exec_command" => "Bash",
        "read" => "Read",
        "edit" => "Edit",
        "write" => "Write",
        "grep" => "Grep",
        "glob" => "Glob",
        "webfetch" => "WebFetch",
        "task" => "Task",
        _ => k,
    };
    let (kind, obj) = match k {
        "Bash" => {
            let c = rx(r#"^/bin/(zsh|bash|sh) -l?c ['\"]?"#).replace(v.trim(), "");
            let c = rx(r#"^(cd\s+(\"[^\"]*\"|'[^']*'|\S+)\s*(&&|;)\s*)+"#)
                .replace(&c, "")
                .into_owned();
            let patterns = [
                (r"git (-C \S+ )?commit", "commit"),
                (r"git\b", "git"),
                (r"chrome|webcheck|screenshot|playwright", "browser"),
                (r"curl|wget", "web"),
                (r"(build|make|cargo|npm run|tsc)\b", "build"),
                (
                    r"(cat|sed|head|tail|ls|rg|grep|find|wc|stat|file)\b",
                    "read",
                ),
                (r"sleep\b", "wait"),
            ];
            let hit = if d.is_empty() {
                patterns
                    .iter()
                    .find(|(p, _)| rx(&format!("^({p})")).is_match(&c))
                    .map(|(_, k)| *k)
            } else {
                None
            };
            if let Some(k) = hit {
                (k.to_string(), String::new())
            } else {
                (
                    "run".into(),
                    if !d.is_empty() {
                        cut(d, 70)
                    } else if rx(r"^(python3?|node|bun|deno)\b").is_match(&c) {
                        "corre un script".into()
                    } else if rx(r"^(for|while|until|if)\b").is_match(&c) {
                        "corre un bucle de comandos".into()
                    } else {
                        format!(
                            "corre {}",
                            base(
                                c.split_whitespace()
                                    .find(|w| !w.contains('='))
                                    .unwrap_or("un comando")
                            )
                            .trim_start_matches('(')
                        )
                    },
                )
            }
        }
        "Read" => ("read".into(), base(v)),
        "Edit" | "Write" | "NotebookEdit" | "MultiEdit" => (
            "edit".into(),
            v.split(',')
                .map(|x| base(x.trim()))
                .collect::<Vec<_>>()
                .join(", "),
        ),
        "Grep" | "Glob" => ("search".into(), sq(v, 40)),
        "WebSearch" | "WebFetch" => ("web".into(), sq(v, 50)),
        "Agent" | "Task" => ("agent".into(), sq(&untag(v), 60)),
        "SendMessage" | "CrossSessionMessage" => ("msg".into(), sq(v, 30)),
        "Skill" => ("skill".into(), sq(v, 30)),
        "dice" => ("say".into(), md(&untag(v), 90)),
        _ if k.starts_with("mcp__") => (
            if k.starts_with("mcp__rea") {
                "re"
            } else {
                "tool"
            }
            .into(),
            k.split("__").last().unwrap_or(k).replace('_', " "),
        ),
        _ => ("tool".into(), sq(raw, 60)),
    };
    (kind, obj)
}
fn steps(ev: &[Event], count: usize) -> Value {
    let mut out: Vec<(String, Vec<String>, usize, f64)> = vec![];
    for e in ev {
        let (k, o) = human(&e.raw, &e.desc);
        if let Some(last) = out.last_mut() {
            if last.0 == k
                && (!["say", "run", "agent"].contains(&k.as_str()) || last.1 == vec![o.clone()])
            {
                last.2 += 1;
                last.3 = e.t;
                if !o.is_empty() && !last.1.contains(&o) {
                    last.1.push(o)
                }
                continue;
            }
        }
        out.push((k, if o.is_empty() { vec![] } else { vec![o] }, 1, e.t));
    }
    json!(
        out.iter()
            .skip(out.len().saturating_sub(count))
            .map(|(k, objs, n, t)| {
                let verb = match k.as_str() {
                    "read" => "Leyó",
                    "edit" => "Editó",
                    "search" => "Buscó",
                    "build" => "Compiló",
                    "commit" => "Guardó un commit",
                    "git" => "Revisó el repo",
                    "browser" => "Probó en el navegador",
                    "web" => "Consultó la web",
                    "agent" => "Lanzó un subagente",
                    "msg" => "Reportó a",
                    "skill" => "Usó la habilidad",
                    "re" => "Analizó el binario",
                    "tool" => "Usó",
                    "say" => "Dijo",
                    "wait" => "Esperó",
                    _ => "",
                };
                let mut obj = objs.iter().take(2).cloned().collect::<Vec<_>>().join(", ");
                if objs.len() > 2 {
                    obj += &format!(" +{}", objs.len() - 2)
                }
                let text = if k == "say" {
                    format!("«{obj}»")
                } else {
                    format!("{verb} {obj}").trim().into()
                };
                json!({"ic":k,"n":n,"t":t,"verb":verb,"obj":obj,"text":text})
            })
            .collect::<Vec<_>>()
    )
}
fn line(id: &str, request: &str, f: &Feed) -> String {
    crate::loops::plain(
        id,
        request,
        &f.ev.iter().map(|e| e.raw.clone()).collect::<Vec<_>>(),
        true,
    )
}
fn card(
    id: &str,
    kind: &str,
    title: &str,
    who: &str,
    request: &str,
    status: &str,
    f: &Feed,
    line: &str,
    updated: f64,
) -> Value {
    let mut c = json!({"usage":usage(&f.path,f.codex),"id":id,"kind":kind,"title":sq(title,90),"who":who,"request":cut(request,6000),"status":status,"now":sq(line,160),"steps":steps(&f.ev,3),"errs":f.errs,"updated":updated,"model":f.model,"re":recent(f)});
    shot_fields(&mut c, id);
    c
}
static CACHE: LazyLock<Mutex<(f64, Arc<Vec<u8>>)>> =
    LazyLock::new(|| Mutex::new((0., Arc::new(vec![]))));
pub fn state() -> Arc<Vec<u8>> {
    let mut c = CACHE.lock();
    if crate::shared::take_dirty() || now() - c.0 > 1.5 {
        *c = (
            now(),
            Arc::new(serde_json::to_vec(&snapshot()).unwrap_or_default()),
        );
    }
    c.1.clone()
}
pub fn set_cache(body: Vec<u8>) {
    *CACHE.lock() = (now(), Arc::new(body));
}
fn flatten(c: &Value, out: &mut Vec<Value>) {
    out.push(c.clone());
    for k in arr(&c["children"]) {
        flatten(&k, out)
    }
}
pub fn all_cards() -> Vec<Value> {
    let body = state();
    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let mut out = vec![];
    for r in arr(&v["repos"]) {
        for w in arr(&r["windows"]) {
            flatten(&w, &mut out)
        }
    }
    out
}
pub fn find_card(id: &str) -> Option<Value> {
    all_cards().into_iter().find(|c| s(c, "id") == id)
}
pub fn snapshot() -> Value {
    let tick = now();
    let ps = procs::discover_procs().unwrap_or_default();
    let jobs = JOBS
        .list_jobs()
        .into_iter()
        .filter(|j| {
            ["q", "r"].contains(&j.status.as_str())
                || tick - if j.ended != 0. { j.ended } else { j.ts } < crate::shared::LINGER
                    && !crate::shared::hidden(&format!("job:{}", j.id))
        })
        .collect::<Vec<_>>();
    let cx = codex_files(10800., false);
    let mut cxa = cx.clone();
    for (f, a) in codex_files(900., true) {
        if !cxa.iter().any(|(g, _)| *g == f) {
            cxa.push((f, a))
        }
    }
    for (f, _) in &cxa {
        feed(f, true);
    }
    let used_threads: HashSet<_> = jobs
        .iter()
        .filter(|j| !j.thread.is_empty())
        .map(|j| j.thread.clone())
        .collect();
    let mut used = HashSet::new();
    let mut wins = vec![];
    for sf in glob(&format!("{}/.claude/sessions/*.json", home())) {
        let sess = jload(&sf);
        let pid = n(&sess, "pid") as i32;
        if s(&sess, "kind") != "interactive" || !procs::pid_alive(pid) {
            continue;
        }
        let sid = s(&sess, "sessionId");
        let cwd = s(&sess, "cwd");
        if cwd == crate::shared::cache_dir().join("oneshot").to_string_lossy()
            || cwd.starts_with(&format!("{}/.cache/", home()))
        {
            continue;
        }
        let tx = transcript(&sid, &cwd);
        let f = feed(&tx, false);
        let proc = ps.procs.get(&pid);
        let paused = proc.is_some_and(|p| p.stat.contains('T'));
        let st = if paused {
            "idle"
        } else {
            match s(&sess, "status").as_str() {
                "busy" => "working",
                "waiting" => "waiting",
                _ => "idle",
            }
        };
        let mut kids = claude_subs(&tx, &repo_of(&cwd));
        for j in &jobs {
            if j.de == sid {
                used.insert(j.id.clone());
                kids.push(job_card(j, &jobs, &cxa, &ps, 0))
            }
        }
        sort_cards(&mut kids);
        let request = if f.last.is_empty() { &f.first } else { &f.last };
        let id = format!("w:{sid}");
        let l = if paused {
            "En pausa (SIGSTOP)".into()
        } else if st == "working" {
            line(&id, request, &f)
        } else {
            let l = md(
                if st == "waiting" && !f.wait.is_empty() {
                    &f.wait
                } else {
                    &f.say
                },
                200,
            );
            if l.is_empty() {
                "Sin actividad reciente".into()
            } else {
                l
            }
        };
        let mut c = json!({"id":id,"kind":"claude","account":format!("cuenta {}",proc.and_then(|p|p.env_profile).unwrap_or(1)),"title":sess["name"].as_str().filter(|s|!s.is_empty()).unwrap_or(&cut(&sid,8)),"repo":repo_of(&cwd),"cwd":cwd.replace(&home(),"~"),"status":st,"request":cut(request,6000),"now":l,"steps":steps(&f.ev,5),"errs":f.errs,"updated":(n(&sess,"updatedAt")/1000.).max(f.t1),"model":f.model,"usage":usage(&tx,false),"children":kids,"pid":pid,"paused":paused,"re":recent(&f)});
        shot_fields(&mut c, &id);
        wins.push(c)
    }
    for (path, acct) in &cx {
        let f = feed(path, true);
        let m = &f.meta;
        let id = s(m, "id");
        if id.is_empty()
            || !s(m, "parent").is_empty()
            || used_threads.contains(&id)
            || f.first.contains("[agencia cdx")
        {
            continue;
        }
        let age = tick - f.t1;
        if !f.busy || age > crate::shared::RECENT {
            continue;
        }
        let st = if f.busy && age < 300. {
            "working"
        } else if f.busy && age < 7200. {
            "stalled"
        } else {
            "idle"
        };
        let cwd = s(m, "cwd");
        let aid = format!("x:{id}");
        wins.push(json!({"id":aid,"kind":"codex","account":acct.to_lowercase(),"title":sq(&if s(m,"nick").is_empty(){if base(&cwd).is_empty(){"Codex".into()}else{base(&cwd)}}else{s(m,"nick")},60),"repo":repo_of(&cwd),"cwd":cwd.replace(&home(),"~"),"status":st,"request":cut(if f.first.is_empty(){&f.last}else{&f.first},6000),"now":if st=="working"{line(&aid,&f.first,&f)}else{md(&f.say,160)},"steps":steps(&f.ev,5),"errs":f.errs,"updated":f.t1,"model":f.model,"usage":usage(path,true),"children":codex_subs(&id,&cx,0),"re":recent(&f)}));
    }
    wins.extend(oc_windows(&ps));
    let held = procs::agy_pids(&ps);
    if !held.is_empty() {
        let (rows, asked) = agy_rows();
        for (pid, cid) in held {
            if cid.is_empty() {
                continue;
            }
            let r=rows.iter().find(|r|s(r,"id")==cid).cloned().unwrap_or_else(||json!({"id":cid,"title":"","status":"","t":tick,"cwd":ps.procs.get(&pid).map(|p|p.cwd.clone()).unwrap_or_else(home),"agent":"","parent":""}));
            let mut seen = HashSet::from([cid.clone()]);
            let kids = agy_subs(
                &cid,
                &rows,
                &jobs,
                &mut used,
                &cxa,
                &ps,
                &asked,
                tick,
                0,
                &mut seen,
            );
            wins.push(agy_card(&r, true, kids, Some(pid), &asked));
        }
    }
    let ids = jobs.iter().map(|j| j.id.as_str()).collect::<HashSet<_>>();
    let mut orph: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for j in &jobs {
        if !used.contains(&j.id) && !ids.contains(j.de.strip_prefix("job:").unwrap_or(&j.de)) {
            orph.entry(repo_of(&j.repo))
                .or_default()
                .push(job_card(j, &jobs, &cxa, &ps, 0))
        }
    }
    for (repo, kids) in orph {
        wins.push(json!({"id":format!("o:{repo}"),"kind":"orphan","account":"","title":"Sin ventana","repo":repo,"cwd":"","status":if kids.iter().any(|c|s(c,"status")=="working"){"working"}else{"idle"},"request":"","now":"Trabajos lanzados desde una ventana que ya cerró","steps":[],"errs":0,"updated":kids.iter().map(|c|n(c,"updated")).fold(0.,f64::max),"model":"","usage":usage("",false),"children":kids}));
    }
    let orq = wins
        .iter()
        .filter(|c| s(c, "kind") == "claude" && s(c, "title") == "Orquestador")
        .map(|c| (s(c, "repo"), s(c, "id")))
        .collect::<HashMap<_, _>>();
    let mut nested = vec![];
    wins.retain(|w| {
        if ["claude", "opencode"].contains(&s(w, "kind").as_str())
            && orq.get(&s(w, "repo")).is_some_and(|id| *id != s(w, "id"))
        {
            nested.push(w.clone());
            false
        } else {
            true
        }
    });
    for w in nested {
        if let Some(parent) = wins
            .iter_mut()
            .find(|p| orq.get(&s(&w, "repo")).is_some_and(|id| *id == s(p, "id")))
        {
            parent["children"].as_array_mut().unwrap().insert(0, w)
        }
    }
    fn finish(c: &mut Value) {
        c["now"] = json!(untag(&s(c, "now")));
        c["recap"] = crate::loops::recap(&s(c, "id"));
        if let Some(kids) = c["children"].as_array_mut() {
            for k in kids {
                finish(k)
            }
        }
    }
    let mut grouped: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for mut w in wins {
        finish(&mut w);
        grouped.entry(s(&w, "repo")).or_default().push(w)
    }
    let mut repos = grouped
        .into_iter()
        .map(|(r, mut ws)| {
            sort_cards(&mut ws);
            ws.sort_by_key(|w| s(w, "kind") == "orphan");
            json!({"name":r,"windows":ws})
        })
        .collect::<Vec<_>>();
    repos.sort_by(|a, b| {
        let metric = |r: &Value| {
            let ws = arr(&r["windows"]);
            (
                !ws.iter().any(|w| s(w, "status") == "working"),
                ws.iter().map(|w| n(w, "updated")).fold(0., f64::max),
            )
        };
        let (a1, a2) = metric(a);
        let (b1, b2) = metric(b);
        a1.cmp(&b1).then_with(|| b2.total_cmp(&a2))
    });
    let mut repos = json!(repos);
    crate::loops::mark_safe(&mut repos);
    json!({"t":tick,"repos":repos,"haiku":{"used":crate::loops::used_hour(),"cap":crate::shared::HAIKU_PER_HOUR,"asleep":!crate::shared::awake()},"quota":crate::loops::quota(),"avance":crate::avance::snapshot()})
}
