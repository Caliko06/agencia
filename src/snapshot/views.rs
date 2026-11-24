use super::*;

fn first_nonempty(xs: &[String]) -> String {
    xs.iter()
        .find(|x| !x.is_empty())
        .cloned()
        .unwrap_or_default()
}

pub fn agent(aid: &str) -> Option<Value> {
    let (kind, key) = aid.split_once(':')?;
    if !rx(r"^[\w.-]+$").is_match(key) {
        return None;
    }
    let cx = codex_files(10800., false);
    let mut fd;
    let mut job = None;
    let mut oc = None;
    let mut d;
    match kind {
        "w" => {
            let sess = glob(&format!("{}/.claude/sessions/*.json", home()))
                .into_iter()
                .map(|p| jload(&p))
                .find(|v| s(v, "sessionId") == key)?;
            let cwd = s(&sess, "cwd");
            fd = feed(&transcript(key, &cwd), false);
            let account = procs::discover_procs()
                .ok()
                .and_then(|ps| {
                    ps.procs
                        .get(&(n(&sess, "pid") as i32))
                        .and_then(|p| p.env_profile)
                })
                .unwrap_or(1);
            d = json!({"title":first_nonempty(&[s(&sess,"name"),cut(key,8)]),"account":format!("cuenta {account}"),"repo":repo_of(&cwd),"cwd":cwd.replace(&home(),"~"),"status":match s(&sess,"status").as_str(){"busy"=>"working","waiting"=>"waiting",_=>"idle"},"request":first_nonempty(&[fd.last.clone(),fd.first.clone()]),"reply":fd.say});
        }
        "x" | "csub" => {
            let found = cx
                .iter()
                .find(|(p, _)| s(&feed(p, true).meta, "id") == key)
                .cloned()
                .or_else(|| {
                    let primary = std::env::var("AGENCIA_CODEX_NAME")
                        .ok()
                        .filter(|v| !v.is_empty())
                        .unwrap_or_else(|| "Codex".into());
                    agencia_core::jobs::discover_codex_homes()
                        .into_keys()
                        .find_map(|h| {
                            let p = rollout(&h, key);
                            if p.is_empty() {
                                None
                            } else {
                                let label = if h == "default" {
                                    primary.clone()
                                } else {
                                    format!("Codex {h}")
                                };
                                Some((p, label))
                            }
                        })
                })?;
            fd = feed(&found.0, true);
            let cwd = s(&fd.meta, "cwd");
            d = json!({"title":first_nonempty(&[s(&fd.meta,"nick"),base(&cwd),"Codex".into()]),"account":found.1,"repo":repo_of(&cwd),"cwd":cwd.replace(&home(),"~"),"request":first_nonempty(&[fd.first.clone(),fd.last.clone()]),"reply":fd.say});
        }
        "oc" => {
            let x = oc_sessions().into_iter().find(|x| s(x, "id") == key)?;
            let parts = arr(&oc_parts(&oc_msgs(key)));
            let cwd = s(&x["location"], "directory");
            fd = Feed::default();
            d = json!({"title":first_nonempty(&[s(&x,"title"),cut(key,8)]),"account":"opencode","repo":repo_of(&cwd),"cwd":cwd.replace(&home(),"~"),"request":parts.iter().rev().find(|p|s(p,"role")=="user"&&s(p,"kind")=="text").map(|p|s(p,"text")).unwrap_or_default(),"reply":"","model":s(&x["model"],"id"),"status":"waiting"});
            oc = Some(x);
        }
        "agy" => {
            let (rows, asked) = agy_rows();
            let tx = agy_tx(key);
            let tm = mtime(&tx);
            let r = rows
                .into_iter()
                .find(|r| s(r, "id") == key)
                .or_else(|| (tm > 0.).then(|| json!({"id":key,"cwd":home()})))?;
            fd = feed(&tx, false);
            let cwd = s(&r, "cwd");
            let root = s(&r, "parent").is_empty();
            let run =
                s(&r, "status").ends_with("RUNNING") || fd.busy && now() - fd.t1.max(tm) < 300.;
            let title = if root {
                first_nonempty(&[s(&r, "title"), base(&cwd), cut(key, 8)])
            } else {
                first_nonempty(&[s(&r, "role"), s(&r, "agent"), s(&r, "title"), cut(key, 8)])
            };
            d = json!({"title":title,"account":"antigravity","repo":repo_of(&cwd),"cwd":cwd.replace(&home(),"~"),"status":if run{"working"}else if root{"idle"}else{"done"},"request":first_nonempty(&[fd.last.clone(),fd.first.clone(),asked.get(key).cloned().unwrap_or_default()]),"reply":fd.say});
        }
        "sub" => {
            let path = glob(&format!(
                "{}/.claude/projects/*/*/subagents/agent-{key}.jsonl",
                home()
            ))
            .into_iter()
            .next()?;
            let meta = jload(&path.replace(".jsonl", ".meta.json"));
            fd = feed(&path, false);
            d = json!({"title":first_nonempty(&[s(&meta,"description"),s(&meta,"agentType"),"subagente".into()]),"account":"claude","request":fd.first,"reply":fd.say});
        }
        "job" => {
            let j = JOBS.load(key)?;
            fd = job_feed(&j, &cx);
            d = json!({"title":first_nonempty(&[j.brief.clone(),j.text.clone()]),"account":who(&j),"repo":repo_of(&j.repo),"cwd":j.repo.replace(&home(),"~"),"status":j.status,"request":j.text,"reply":j.reply,"model":j.model});
            if fd.path.is_empty() {
                fd.ev = j
                    .acts
                    .iter()
                    .map(|a| Event {
                        t: if j.updated != 0. { j.updated } else { j.ts },
                        raw: a.clone(),
                        desc: String::new(),
                    })
                    .collect();
            }
            job = Some(j);
        }
        _ => return None,
    }
    if let Some(x) = oc {
        d["parts"] = oc_parts(&oc_msgs(key));
        d["usage"] = oc_usage(&x);
    } else if let Some(j) = job.as_ref().filter(|j| j.kind == "bec") {
        d["parts"] = oc_sessions()
            .into_iter()
            .find(|x| s(x, "title") == format!("bec-{}", j.id))
            .map(|x| oc_parts(&oc_msgs(&s(&x, "id"))))
            .unwrap_or(json!([]));
        d["usage"] = bec_usage(j);
    } else {
        d["parts"] = if fd.path.is_empty() {
            json!([])
        } else {
            parts_of(&fd.path, fd.codex)
        };
        d["usage"] = usage(&fd.path, fd.codex);
    }
    let events = fd
        .ev
        .iter()
        .skip(fd.ev.len().saturating_sub(80))
        .flat_map(|e| {
            arr(&steps(std::slice::from_ref(e), 1))
                .into_iter()
                .map(|v| json!({"t":e.t,"ic":v["ic"],"text":v["text"],"raw":e.raw}))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    d["id"] = json!(aid);
    d["model"] = json!(first_nonempty(&[s(&d, "model"), fd.model]));
    d["say"] = json!(cut(&fd.say, 20000));
    d["errs"] = json!(fd.errs);
    d["feed"] = json!(events);
    d["request"] = json!(cut(&s(&d, "request"), 20000));
    d["reply"] = json!(cut(&s(&d, "reply"), 20000));
    Some(d)
}

static REPOS: LazyLock<Mutex<(f64, Value)>> = LazyLock::new(|| Mutex::new((0., json!([]))));
pub fn repos() -> Value {
    let mut cache = REPOS.lock();
    if now() - cache.0 < 60. {
        return cache.1.clone();
    }
    let skip = [
        "node_modules",
        "Library",
        "Applications",
        "Desktop",
        "Documents",
        "Downloads",
        "Movies",
        "Music",
        "Pictures",
        "Public",
    ];
    let entries = |p: &Path| {
        fs::read_dir(p)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                !n.starts_with('.') && !skip.contains(&n.as_str())
            })
            .map(|e| e.path())
            .collect::<Vec<_>>()
    };
    let mut out = vec![];
    for p in entries(Path::new(&home())) {
        if !p.is_dir() {
            continue;
        }
        let mut subs = vec![p.clone()];
        if !p.join(".git").exists() {
            subs.extend(entries(&p));
        }
        for p in subs {
            let g = p.join(".git");
            if g.exists() {
                out.push((
                    mtime(&g.to_string_lossy()),
                    json!({"name":base(&p.to_string_lossy()),"path":p}),
                ));
            }
        }
    }
    out.sort_by(|a, b| b.0.total_cmp(&a.0));
    cache.0 = now();
    cache.1 = json!(out.into_iter().take(40).map(|(_, v)| v).collect::<Vec<_>>());
    cache.1.clone()
}

fn activity_row(
    id: String,
    title: String,
    who: String,
    account: String,
    path: &str,
    codex: bool,
    updated: f64,
) -> Value {
    let fd = feed(path, codex);
    let u = usage(path, codex);
    let ms = usage_models(path, codex);
    let model = ms
        .as_object()
        .and_then(|m| {
            m.iter()
                .max_by_key(|(_, v)| {
                    v.as_array()
                        .map(|a| a.iter().filter_map(Value::as_i64).sum::<i64>())
                        .unwrap_or(0)
                })
                .map(|(k, _)| k.clone())
        })
        .unwrap_or(fd.model);
    json!({"id":id,"title":sq(&title,90),"who":who,"account":account,"model":model,"models":ms,"in":u["in"],"out":u["out"],"cache":u["cache"],"updated":updated})
}
static ACT: LazyLock<Mutex<(f64, Value)>> = LazyLock::new(|| Mutex::new((0., Value::Null)));
pub fn activity() -> Value {
    let mut cache = ACT.lock();
    if now() - cache.0 < 10. && !cache.1.is_null() {
        return cache.1.clone();
    }
    let tick = now();
    let mut rows = vec![];
    let jobs = JOBS.list_jobs();
    let mut names = HashMap::new();
    let mut accs = HashMap::new();
    for sf in glob(&format!("{}/.claude/sessions/*.json", home())) {
        let v = jload(&sf);
        names.insert(s(&v, "sessionId"), s(&v, "name"));
    }
    let mut profiles = vec![(format!("{}/.claude", home()), 1)];
    profiles.extend(
        glob(&format!("{}/.claude-profiles/[0-9]*", home()))
            .into_iter()
            .filter_map(|p| base(&p).parse::<u32>().ok().map(|n| (p, n))),
    );
    for (p, n) in profiles {
        if let Ok(es) = fs::read_dir(format!("{p}/session-env")) {
            for e in es.flatten() {
                let sid = e.file_name().to_string_lossy().into_owned();
                if n > 1 || !accs.contains_key(&sid) {
                    accs.insert(sid, format!("cuenta {n}"));
                }
            }
        }
    }
    for p in glob(&format!("{}/.claude/projects/*/*.jsonl", home()))
        .into_iter()
        .chain(glob(&format!(
            "{}/.claude/projects/*/*/subagents/agent-*.jsonl",
            home()
        )))
    {
        let m = mtime(&p);
        if tick - m > 86400. {
            continue;
        }
        let sub = p.contains("/subagents/");
        let fd = feed(&p, false);
        let sid = if sub {
            Path::new(&p)
                .ancestors()
                .nth(2)
                .map(|p| base(&p.to_string_lossy()))
                .unwrap_or_default()
        } else {
            base(&p).trim_end_matches(".jsonl").into()
        };
        let (id, title, who) = if sub {
            let meta = jload(&p.replace(".jsonl", ".meta.json"));
            (
                format!(
                    "sub:{}",
                    base(&p)
                        .trim_start_matches("agent-")
                        .trim_end_matches(".jsonl")
                ),
                first_nonempty(&[
                    s(&meta, "description"),
                    s(&meta, "agentType"),
                    "subagente".into(),
                ]),
                first_nonempty(&[s(&meta, "agentType"), "Subagente".into()]),
            )
        } else {
            (
                format!("w:{sid}"),
                first_nonempty(&[
                    names.get(&sid).cloned().unwrap_or_default(),
                    sq(&fd.first, 60),
                    cut(&sid, 8),
                ]),
                "Claude".into(),
            )
        };
        rows.push(activity_row(
            id,
            title,
            who,
            accs.get(&sid).cloned().unwrap_or("cuenta ?".into()),
            &p,
            false,
            m,
        ));
    }
    for (p, acct) in codex_files(86400., true) {
        let fd = feed(&p, true);
        let mt = &fd.meta;
        let tag = rx(r"^\[agencia cdx (\w+)")
            .captures(&fd.first)
            .map(|c| c[1].to_string());
        let j = tag.and_then(|t| jobs.iter().find(|j| j.id == t));
        let (id, title) = if let Some(j) = j {
            (
                format!("job:{}", j.id),
                first_nonempty(&[j.brief.clone(), j.text.clone()]),
            )
        } else if !s(mt, "parent").is_empty() {
            (
                format!("csub:{}", s(mt, "id")),
                first_nonempty(&[s(mt, "nick"), "subagente".into()]),
            )
        } else {
            (
                format!("x:{}", s(mt, "id")),
                first_nonempty(&[s(mt, "nick"), base(&s(mt, "cwd")), "Codex".into()]),
            )
        };
        rows.push(activity_row(
            id,
            title,
            acct.clone(),
            acct.to_lowercase(),
            &p,
            true,
            mtime(&p),
        ));
    }
    for j in &jobs {
        let t = if j.updated != 0. {
            j.updated
        } else if j.ended != 0. {
            j.ended
        } else {
            j.ts
        };
        if j.kind != "bec" || tick - t > 86400. {
            continue;
        }
        let u = bec_usage(j);
        let model = first_nonempty(&[j.model.clone(), "?".into()]);
        let mut ms = json!({});
        ms[&model] = json!([u["in"], u["out"], u["cache"]]);
        rows.push(json!({"id":format!("job:{}",j.id),"title":sq(&first_nonempty(&[j.brief.clone(),j.text.clone()]),90),"who":"Becario","model":model,"models":ms,"account":"opencode","in":u["in"],"out":u["out"],"cache":u["cache"],"updated":t}));
    }
    let (agy, _) = agy_rows();
    for d in glob(&format!("{}/brain/*/.system_generated/logs", agy_dir())) {
        let cid = Path::new(&d)
            .ancestors()
            .nth(2)
            .map(|p| base(&p.to_string_lossy()))
            .unwrap_or_default();
        let p = agy_tx(&cid);
        let m = mtime(&p);
        if m == 0. || tick - m > 86400. {
            continue;
        }
        let u = usage(&p, false);
        if n(&u, "in") + n(&u, "out") + n(&u, "cache") == 0. {
            continue;
        }
        let fd = feed(&p, false);
        let r = agy
            .iter()
            .find(|r| s(r, "id") == cid)
            .cloned()
            .unwrap_or(Value::Null);
        let title = if !s(&r, "parent").is_empty() {
            first_nonempty(&[
                s(&r, "agent"),
                s(&r, "title"),
                sq(&fd.first, 60),
                cut(&cid, 8),
            ])
        } else {
            first_nonempty(&[
                s(&r, "title"),
                base(&s(&r, "cwd")),
                sq(&fd.first, 60),
                cut(&cid, 8),
            ])
        };
        rows.push(activity_row(
            format!("agy:{cid}"),
            title,
            "Antigravity".into(),
            "antigravity".into(),
            &p,
            false,
            m,
        ));
    }
    rows.sort_by(|a, b| (n(b, "in") + n(b, "out")).total_cmp(&(n(a, "in") + n(a, "out"))));
    let mut bm: BTreeMap<String, Value> = BTreeMap::new();
    let mut ba: BTreeMap<String, Value> = BTreeMap::new();
    for r in &mut rows {
        if let Some(ms) = r
            .as_object_mut()
            .and_then(|r| r.remove("models"))
            .and_then(|m| m.as_object().cloned())
        {
            for (k, v) in ms {
                let g = bm
                    .entry(k.clone())
                    .or_insert_with(|| json!({"model":k,"in":0,"out":0,"cache":0,"agents":0}));
                for (i, k) in ["in", "out", "cache"].iter().enumerate() {
                    g[*k] = json!(g[*k].as_i64().unwrap_or(0) + v[i].as_i64().unwrap_or(0));
                }
                g["agents"] = json!(n(g, "agents") as i64 + 1);
            }
        }
        let k = first_nonempty(&[s(r, "account"), "?".into()]);
        let g = ba
            .entry(k.clone())
            .or_insert_with(|| json!({"account":k,"in":0,"out":0,"agents":0}));
        for k in ["in", "out"] {
            g[k] = json!(g[k].as_i64().unwrap_or(0) + r[k].as_i64().unwrap_or(0));
        }
        g["agents"] = json!(n(g, "agents") as i64 + 1);
    }
    let mut bm = bm.into_values().collect::<Vec<_>>();
    let mut ba = ba.into_values().collect::<Vec<_>>();
    bm.sort_by(|a, b| {
        (n(b, "in") + n(b, "out") + n(b, "cache"))
            .total_cmp(&(n(a, "in") + n(a, "out") + n(a, "cache")))
    });
    ba.sort_by(|a, b| (n(b, "in") + n(b, "out")).total_cmp(&(n(a, "in") + n(a, "out"))));
    cache.0 = now();
    cache.1 = json!({"by_model":bm,"by_account":ba,"agents":rows});
    cache.1.clone()
}
