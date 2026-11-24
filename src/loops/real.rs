use super::*;
use agencia_core::jobs::JobRecord;
use std::collections::HashMap;
static V: LazyLock<Mutex<HashMap<String, (f64, Value)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
fn accounts(q: &Value) -> HashMap<String, (String, f64)> {
    let mut out = HashMap::new();
    for c in q["claude"].as_array().into_iter().flatten() {
        if let Some(w) = c["w"].as_f64() {
            out.insert(format!("claude:{}", c["n"]), (s(c, "name").into(), w));
        }
    }
    for c in q["codex"].as_array().into_iter().flatten() {
        if let Some(w) = c["w"].as_f64() {
            let name = s(c, "name");
            let key = if name == "Codex" || name.is_empty() {
                "default"
            } else {
                name
            };
            let label = if name.starts_with("Codex") {
                name.to_string()
            } else {
                format!("Codex {name}")
            };
            out.insert(format!("codex:{key}"), (label, w));
        }
    }
    for g in q["go"].as_array().into_iter().flatten() {
        if s(g, "label") == "semana" {
            out.insert("go".into(), ("DeepSeek".into(), n(g, "pct")));
        }
    }
    out
}
pub(super) fn tick() {
    let runs = jobs::real_runs(true);
    if runs.is_empty() {
        return;
    }
    let q = quota();
    if q.is_null() {
        return;
    }
    let cur = accounts(&q);
    let mut old = load(cache_dir().join("real.json")).unwrap_or(json!({}));
    if old.get("at").is_some() {
        old = Value::Object(
            runs.iter()
                .filter(|(_, r)| r.at.as_ref() == old.get("at"))
                .map(|(repo, _)| (repo.clone(), old.clone()))
                .collect(),
        );
    }
    let mut out = json!({});
    for (repo, r) in runs {
        let at = r.at.unwrap_or(Value::Null);
        let mut c = old
            .get(&repo)
            .filter(|c| c["at"] == at)
            .cloned()
            .unwrap_or(json!({"at":at,"base":{}}));
        let mut over = vec![];
        let pct = r.extra.get("pct").and_then(Value::as_f64).unwrap_or(0.);
        c["now"] = json!({});
        c["names"] = json!({});
        for (k, (name, w)) in &cur {
            let base = c["base"][k].as_f64().unwrap_or(*w).min(*w);
            c["base"][k] = json!(base);
            c["now"][k] = json!(w);
            c["names"][k] = json!(name);
            if w - base >= pct {
                over.push(k.clone());
            }
        }
        over.sort();
        c["over"] = json!(over);
        c["t"] = json!(now());
        out[repo] = c;
    }
    save(cache_dir().join("real.json"), &out);
}
fn files(wt: &str, main: &str) -> (Vec<Value>, i64, i64) {
    let base = git(wt, &["merge-base", main, "HEAD"]);
    let base = if base.is_empty() { main } else { &base };
    let mut nums = HashMap::new();
    for line in git(wt, &["diff", "--numstat", "--no-renames", base]).lines() {
        let a: Vec<_> = line.splitn(3, '\t').collect();
        if a.len() == 3 {
            nums.insert(
                a[2].to_owned(),
                (
                    a[0].parse::<i64>().unwrap_or(0),
                    a[1].parse::<i64>().unwrap_or(0),
                ),
            );
        }
    }
    let new: std::collections::HashSet<_> =
        git(wt, &["ls-files", "--others", "--exclude-standard"])
            .lines()
            .map(str::to_owned)
            .collect();
    let names: std::collections::HashSet<_> =
        nums.keys().cloned().chain(new.iter().cloned()).collect();
    let mut out = vec![];
    for p in names {
        let f = Path::new(wt).join(&p);
        let Ok(st) = fs::metadata(&f) else { continue };
        let (add, rem) = nums.get(&p).copied().unwrap_or((0, 0));
        let ext = f
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        out.push(json!({"p":f,"name":p,"add":add,"rem":rem,"isNew":new.contains(&p),"size":st.len(),"t":mtime(&f),"view":(["md","txt","json","jsonl","tsv","csv","log","py","ts","tsx","js","mjs","cjs","css","html","rs","c","h","cpp","sh","toml","yaml","yml","asm","s","ndjson","svg","png","jpg","jpeg","webp","gif"].contains(&ext.as_str()))}));
    }
    out.sort_by(|a, b| {
        (!s(a, "name").ends_with(".md"))
            .cmp(&(!s(b, "name").ends_with(".md")))
            .then(n(b, "t").total_cmp(&n(a, "t")))
    });
    (
        out,
        nums.values().map(|x| x.0).sum(),
        nums.values().map(|x| x.1).sum(),
    )
}
pub(super) fn job(j: &JobRecord, wt: &str) -> Value {
    let live = j.status == "q" || j.status == "r";
    let res = regex::Regex::new(r"RESUMEN:\s*(.+)")
        .unwrap()
        .captures(&j.reply)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .unwrap_or(&j.reply);
    let mut refs = vec![];
    let rx = regex::Regex::new(r"[\w./-]+\.(?:md|tsv|txt|log|json|c|h|py|rs|ts|csv)\b").unwrap();
    for m in rx.find_iter(&j.reply) {
        let f = Path::new(wt).join(m.as_str());
        if let Ok(f) = fs::canonicalize(f) {
            let fp = f.to_string_lossy();
            if fp.starts_with(&format!("{wt}/"))
                && f.is_file()
                && refs.len() < 6
                && !refs.iter().any(|v: &Value| s(v, "p") == fp)
            {
                refs.push(json!({"p":f,"name":f.strip_prefix(wt).unwrap_or(&f)}));
            }
        }
    }
    let u = job_usage(j);
    let who = match j.kind.as_str() {
        "cdx" => {
            if j.home.is_empty() || j.home == "default" {
                "Codex".into()
            } else {
                format!("Codex {}", j.home)
            }
        }
        "bec" => "Becario".into(),
        "cla" => "Claude".into(),
        _ => j.kind.clone(),
    };
    let line = if live {
        let h = narrator::H.lock();
        let cached = s(&h["lines"][format!("job:{}", j.id)], "line");
        md(
            if cached.is_empty() {
                j.acts.last().map(String::as_str).unwrap_or("")
            } else {
                cached
            },
            170,
        )
    } else {
        md(res, 260)
    };
    json!({"id":format!("job:{}",j.id),"who":who,"brief":if j.brief.is_empty(){sq(&j.text,90)}else{j.brief.clone()},"status":j.status,"model":j.model,"steps":j.steps,"secs":(if j.ended>0.{j.ended}else{now()})-(if j.started>0.{j.started}else{j.ts}),"line":line,"reply":cut(&j.reply,4000),"refs":refs,"tokens":n(&u,"in")+n(&u,"out"),"usd":n(&u,"usd"),"t":if j.updated>0.{j.updated}else{j.ts}})
}
pub fn real_view(want: &str) -> Value {
    let runs = jobs::real_runs(false);
    if runs.is_empty() {
        return json!({"ok":true,"state":"off","runs":[]});
    }
    let mut keys: Vec<_> = runs.keys().cloned().collect();
    keys.sort_by(|a, b| {
        let a = &runs[a];
        let b = &runs[b];
        (now() < b.until)
            .cmp(&(now() < a.until))
            .then(
                b.at.as_ref()
                    .map(Value::to_string)
                    .cmp(&a.at.as_ref().map(Value::to_string)),
            )
            .then(b.until.total_cmp(&a.until))
    });
    let repo = keys
        .iter()
        .find(|k| {
            !want.is_empty()
                && (want == k.as_str() || Path::new(k).file_name().is_some_and(|p| p == want))
        })
        .unwrap_or(&keys[0]);
    let r = &runs[repo];
    let mut listing:Vec<_>=runs.iter().map(|(k,v)|json!({"repo":Path::new(k).file_name().unwrap_or_default().to_string_lossy(),"path":k,"state":if now()<v.until{"on"}else{"done"},"until":v.until,"at":v.at.clone().unwrap_or(json!(""))})).collect();
    listing.sort_by(|a, b| {
        (s(a, "state") != "on")
            .cmp(&(s(b, "state") != "on"))
            .then(n(b, "until").total_cmp(&n(a, "until")))
    });
    if let Some((_t, v)) = V.lock().get(repo) {
        let mut v = v.clone();
        v["runs"] = json!(listing);
        v["haiku"] = json!({"awake":awake(),"used":used_hour(),"cap":HAIKU_PER_HOUR});
        return v;
    }
    if let Some(disk) = load(cache_dir().join("real_views.json")) {
        if let Some(v) = disk.get(repo).filter(|v| v.is_object()) {
            let mut v = v.clone();
            v["runs"] = json!(listing);
            v["haiku"] = json!({"awake":awake(),"used":used_hour(),"cap":HAIKU_PER_HOUR});
            return v;
        }
    }
    let all = load(cache_dir().join("real.json")).unwrap_or(json!({}));
    let c = if all.get("at").is_some() {
        &all
    } else {
        &all[repo]
    };
    let mut acc = vec![];
    if c.get("at") == r.at.as_ref() {
        for (k, w) in c["now"].as_object().into_iter().flatten() {
            let w = w.as_f64().unwrap_or(0.);
            acc.push(json!({"id":k,"name":c["names"].get(k).cloned().unwrap_or(json!(k)),"spent":(w-c["base"][k].as_f64().unwrap_or(w)).round(),"over":c["over"].as_array().is_some_and(|v|v.contains(&json!(k)))}));
        }
    }
    let at = r.at.as_ref().and_then(Value::as_str).unwrap_or("");
    let started = chrono::DateTime::parse_from_rfc3339(at)
        .map(|d| d.timestamp() as f64)
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%dT%H:%M:%S")
                .map(|d| d.and_utc().timestamp() as f64)
        })
        .unwrap_or(now());
    json!({"ok":true,"state":if now()<r.until{"on"}else{"done"},"repo":Path::new(repo).file_name().unwrap_or_default().to_string_lossy(),"goal":r.extra.get("goal").cloned().unwrap_or(json!("")),"started":started,"until":r.until,"pct":r.extra.get("pct").cloned().unwrap_or(Value::Null),"haiku":{"awake":awake(),"used":used_hour(),"cap":HAIKU_PER_HOUR},"accounts":acc,"slices":[],"runs":listing})
}

/// Background refresh of git-backed Agencia Real slice views (never runs on the HTTP request thread).
pub(super) fn refresh_views() {
    let runs = jobs::real_runs(false);
    if runs.is_empty() {
        return;
    }
    let mut disk = load(cache_dir().join("real_views.json")).unwrap_or(json!({}));
    if !disk.is_object() {
        disk = json!({});
    }
    for (repo, r) in &runs {
        let v = compute_view(repo, r);
        V.lock().insert(repo.clone(), (now(), v.clone()));
        disk[repo] = v;
    }
    save(cache_dir().join("real_views.json"), &disk);
}

fn compute_view(repo: &String, r: &agencia_core::jobs::RealRun) -> Value {
    let all = load(cache_dir().join("real.json")).unwrap_or(json!({}));
    let c = if all.get("at").is_some() {
        &all
    } else {
        &all[repo]
    };
    let mut acc = vec![];
    if c.get("at") == r.at.as_ref() {
        for (k, w) in c["now"].as_object().into_iter().flatten() {
            let w = w.as_f64().unwrap_or(0.);
            acc.push(json!({"id":k,"name":c["names"].get(k).cloned().unwrap_or(json!(k)),"spent":(w-c["base"][k].as_f64().unwrap_or(w)).round(),"over":c["over"].as_array().is_some_and(|v|v.contains(&json!(k)))}));
        }
    }
    let blocks: Vec<HashMap<String, String>> = git(repo, &["worktree", "list", "--porcelain"])
        .split("\n\n")
        .map(|b| {
            b.lines()
                .map(|ln| {
                    let (k, v) = ln.split_once(' ').unwrap_or((ln, ""));
                    (k.into(), v.into())
                })
                .collect()
        })
        .collect();
    let main = blocks
        .first()
        .and_then(|b| b.get("branch"))
        .map(|b| b.trim_start_matches("refs/heads/"))
        .unwrap_or("main");
    let jobs = JOBS.list_jobs();
    let mut out = vec![];
    let claude: Vec<_> = paths(&format!("{}/.claude/sessions/*.json", home()))
        .iter()
        .filter_map(load)
        .filter(|d| agencia_core::procs::pid_alive(n(d, "pid") as i32))
        .map(|d| s(&d, "cwd").to_owned())
        .collect();
    for b in &blocks {
        let br = b
            .get("branch")
            .map(|s| s.trim_start_matches("refs/heads/"))
            .unwrap_or("");
        let Some(name) = br.strip_prefix("slice/") else {
            continue;
        };
        let wt = b.get("worktree").cloned().unwrap_or_default();
        let mine: Vec<_> = jobs
            .iter()
            .filter(|j| j.repo == wt || j.repo.starts_with(&format!("{wt}/")))
            .collect();
        let count = |status: &str| mine.iter().filter(|j| j.status == status).count();
        let on = |kind: &str| {
            mine.iter()
                .filter(|j| j.kind == kind && ["q", "r"].contains(&j.status.as_str()))
                .count()
        };
        let range = format!("{main}..{br}");
        let commits: Vec<_> = git(&wt, &["log", "-15", "--format=%h%x00%s%x00%ct", &range])
            .lines()
            .filter_map(|ln| {
                let a: Vec<_> = ln.split('\0').collect();
                (a.len() == 3).then(
                    || json!({"hash":a[0],"subject":a[1],"t":a[2].parse::<i64>().unwrap_or(0)}),
                )
            })
            .collect();
        let (files, added, removed) = files(&wt, main);
        let mut rows: Vec<_> = mine.iter().map(|j| job(j, &wt)).collect();
        let order = |x: &Value| match s(x, "status") {
            "r" => 0,
            "pregunta" => 1,
            "q" => 2,
            "fallo" => 3,
            "ok" => 4,
            _ => 5,
        };
        rows.sort_by(|a, b| {
            order(a)
                .cmp(&order(b))
                .then(n(b, "t").total_cmp(&n(a, "t")))
        });
        let at = r.at.as_ref().and_then(Value::as_str).unwrap_or("");
        let key = format!("slice:{at}:{name}");
        let mut ev: Vec<_> = commits
            .iter()
            .map(|c| (n(c, "t"), format!("Commit: {}", s(c, "subject"))))
            .chain(
                files
                    .iter()
                    .filter(|f| f["isNew"] == true && s(f, "name").ends_with(".md"))
                    .map(|f| (n(f, "t"), format!("Escribió {}", s(f, "name")))),
            )
            .chain(
                rows.iter()
                    .filter(|x| ["ok", "fallo", "pregunta"].contains(&s(x, "status")))
                    .map(|x| {
                        (
                            n(x, "t"),
                            format!(
                                "{} {}: {}. Resultado: {}",
                                if s(x, "status") == "fallo" {
                                    "Falló"
                                } else {
                                    "Terminó"
                                },
                                s(x, "who"),
                                s(x, "brief"),
                                s(x, "line")
                            ),
                        )
                    }),
            )
            .collect();
        ev.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut facts: Vec<_> = ev
            .into_iter()
            .rev()
            .take(24)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|x| x.1)
            .collect();
        facts.extend(
            rows.iter()
                .filter(|x| s(x, "status") == "r")
                .take(12)
                .map(|x| format!("Trabajando ahora, {}: {}", s(x, "who"), s(x, "brief"))),
        );
        narrator::story(
            &key,
            r.extra.get("goal").and_then(Value::as_str).unwrap_or(""),
            facts,
        );
        let h = narrator::H.lock();
        out.push(json!({"name":name,"path":wt,"ahead":git(repo,&["rev-list","--count",&range]).parse::<u64>().unwrap_or(0),"dirty":git(&wt,&["status","--short","-uall"]).lines().count(),"last":commits.first().map(|c|s(c,"subject")).unwrap_or(""),"age":commits.first().map(|c|now()-n(c,"t")),"claude":claude.iter().filter(|p|**p==wt||p.starts_with(&format!("{wt}/"))).count(),"codex":on("cdx"),"bec":on("bec"),"running":count("r"),"queued":count("q"),"done":count("ok"),"failed":count("fallo"),"steps":rows.iter().map(|x|n(x,"steps")).sum::<f64>(),"tokens":rows.iter().map(|x|n(x,"tokens")).sum::<f64>(),"usd":(rows.iter().map(|x|n(x,"usd")).sum::<f64>()*100.).round()/100.,"added":added,"removed":removed,"files_n":files.len(),"files":files.into_iter().take(60).collect::<Vec<_>>(),"commits":commits,"jobs":rows,"story":h["story"][&key].as_array().map(|a|a[a.len().saturating_sub(30)..].to_vec()).unwrap_or_default(),"story_err":s(&h["lines"][&key],"err")}));
    }
    let at = r.at.as_ref().and_then(Value::as_str).unwrap_or("");
    let started = chrono::DateTime::parse_from_rfc3339(at)
        .map(|d| d.timestamp() as f64)
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%dT%H:%M:%S")
                .map(|d| d.and_utc().timestamp() as f64)
        })
        .unwrap_or(now());
    let v = json!({"ok":true,"state":if now()<r.until{"on"}else{"done"},"repo":Path::new(repo).file_name().unwrap_or_default().to_string_lossy(),"goal":r.extra.get("goal").cloned().unwrap_or(json!("")),"started":started,"until":r.until,"pct":r.extra.get("pct").cloned().unwrap_or(Value::Null),"haiku":{"awake":awake(),"used":used_hour(),"cap":HAIKU_PER_HOUR},"accounts":acc,"slices":out});
    v
}

// Session attribution follows session-env rather than shared projects/.
#[allow(dead_code)]
pub(super) fn sid_accounts() -> Value {
    static CACHE: LazyLock<Mutex<(f64, Value)>> = LazyLock::new(|| Mutex::new((0., json!({}))));
    let mut cache = CACHE.lock();
    if now() - cache.0 > 60. {
        let mut v = json!({});
        let mut bases = vec![(PathBuf::from(home()).join(".claude"), 1u32)];
        let mut profiles = paths(&format!("{}/.claude-profiles/[0-9]*", home()));
        profiles.sort();
        for p in profiles {
            if let Some(n) = p
                .file_name()
                .and_then(|s| s.to_str())
                .and_then(|s| s.parse::<u32>().ok())
            {
                bases.push((p, n));
            }
        }
        for (base, n) in bases {
            if let Ok(entries) = fs::read_dir(base.join("session-env")) {
                for entry in entries.flatten() {
                    let sid = entry.file_name().to_string_lossy().into_owned();
                    if n > 1 || v.get(&sid).is_none() {
                        v[sid] = json!(format!("cuenta {n}"));
                    }
                }
            }
        }
        *cache = (now(), v);
    }
    cache.1.clone()
}
fn job_usage(j: &JobRecord) -> Value {
    if j.kind == "bec" {
        let Ok(db) = rusqlite::Connection::open_with_flags(
            format!("{}/.local/share/opencode/opencode.db", home()),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ) else {
            return json!({});
        };
        let _ = db.busy_timeout(Duration::from_millis(100));
        return db.query_row("SELECT tokens_input,tokens_output+tokens_reasoning,cost FROM session_v2 WHERE title=?1 ORDER BY time_updated DESC LIMIT 1",[format!("bec-{}",j.id)],|r|Ok(json!({"in":r.get::<_,i64>(0)?,"out":r.get::<_,i64>(1)?,"usd":r.get::<_,f64>(2)?}))).unwrap_or(json!({}));
    }
    if j.thread.is_empty() {
        return json!({});
    }
    let path = if j.kind == "cla" {
        paths(&format!("{}/.claude/projects/*/{}.jsonl", home(), j.thread))
            .into_iter()
            .next()
    } else {
        let base = agencia_core::jobs::discover_codex_homes()
            .get(&j.home)
            .cloned()
            .unwrap_or_else(|| PathBuf::from(home()).join(".codex"));
        let mut p = paths(&format!(
            "{}/sessions/*/*/*/rollout-*-{}.jsonl",
            base.display(),
            j.thread
        ));
        p.extend(paths(&format!(
            "{}/archived_sessions/rollout-*-{}.jsonl",
            base.display(),
            j.thread
        )));
        p.into_iter().next()
    };
    let Some(path) = path else { return json!({}) };
    struct Usage {
        mtime: f64,
        len: u64,
        off: u64,
        ids: HashMap<String, (f64, f64)>,
        input: f64,
        output: f64,
    }
    static CACHE: LazyLock<Mutex<HashMap<PathBuf, Usage>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let Ok(meta) = fs::metadata(&path) else {
        return json!({});
    };
    let mut cache = CACHE.lock();
    let u = cache.entry(path.clone()).or_insert(Usage {
        mtime: 0.,
        len: 0,
        off: 0,
        ids: HashMap::new(),
        input: 0.,
        output: 0.,
    });
    let mt = mtime(&path);
    if meta.len() != u.len || mt != u.mtime {
        if meta.len() < u.off {
            u.off = 0;
            u.ids.clear();
            u.input = 0.;
            u.output = 0.;
        }
        if let Ok(mut file) = fs::File::open(&path) {
            let _ = file.seek(SeekFrom::Start(u.off));
            use std::io::{BufRead, BufReader};
            let mut reader = BufReader::new(file);
            let mut line = String::new();
            loop {
                line.clear();
                let Ok(bytes) = reader.read_line(&mut line) else {
                    break;
                };
                if bytes == 0 || !line.ends_with('\n') {
                    break;
                }
                u.off += bytes as u64;
                if !line.contains(if j.kind == "cla" {
                    "\"usage\""
                } else {
                    "\"token_count\""
                }) {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if j.kind == "cla" && s(&v, "type") == "assistant" {
                    let msg = &v["message"];
                    let x = &msg["usage"];
                    if !x.is_object() {
                        continue;
                    }
                    let id = if s(msg, "id").is_empty() {
                        s(&v, "uuid")
                    } else {
                        s(msg, "id")
                    };
                    let input = n(x, "input_tokens") + n(x, "cache_creation_input_tokens");
                    let output = n(x, "output_tokens");
                    let old = u.ids.insert(id.into(), (input, output)).unwrap_or((0., 0.));
                    u.input += input - old.0;
                    u.output += output - old.1;
                } else if s(&v, "type") == "event_msg" && s(&v["payload"], "type") == "token_count"
                {
                    let t = &v["payload"]["info"]["total_token_usage"];
                    if t.is_object() {
                        u.input = (n(t, "input_tokens") - n(t, "cached_input_tokens")).max(0.);
                        u.output = n(t, "output_tokens");
                    }
                }
            }
        }
        u.mtime = mt;
        u.len = meta.len();
    }
    json!({"in":u.input,"out":u.output})
}
