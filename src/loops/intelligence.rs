use super::*;
static SAFE: LazyLock<Mutex<Value>> = LazyLock::new(|| {
    Mutex::new(
        load(cache_dir().join("cerrables.json"))
            .or_else(|| load(cache_dir().join("safe.json")))
            .unwrap_or(json!({})),
    )
});
static SAFE_RUNS: LazyLock<Mutex<Vec<f64>>> = LazyLock::new(|| Mutex::new(vec![]));
fn free(c: &Value) -> bool {
    ["idle", "done", "failed"].contains(&s(c, "status"))
}
fn eligible(c: &Value) -> bool {
    ["claude", "opencode", "cdx", "bec", "cla", "sub"].contains(&s(c, "kind"))
        && free(c)
        && c["paused"] != true
        && now() - n(c, "updated") >= 900.
        && !(s(c, "kind") == "claude"
            && (s(c, "title") == "Orquestador"
                || c["children"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(has_busy))))
}
fn has_busy(c: &Value) -> bool {
    !free(c)
        || c["children"]
            .as_array()
            .is_some_and(|a| a.iter().any(has_busy))
}
fn sig(c: &Value) -> String {
    format!("{}|{}", s(c, "id"), n(c, "updated") as i64)
}
pub fn mark_safe(repos: &mut Value) {
    fn walk(c: &mut Value, safe: &Value) {
        if eligible(c) {
            let v = &safe[s(c, "id")];
            if v["ok"] == true && s(v, "sig") == sig(c) {
                c["safe"] = v["why"].clone();
            }
        }
        if let Some(a) = c.get_mut("children").and_then(Value::as_array_mut) {
            for c in a {
                walk(c, safe);
            }
        }
    }
    let safe = SAFE.lock();
    for r in repos.as_array_mut().into_iter().flatten() {
        for c in r["windows"].as_array_mut().into_iter().flatten() {
            walk(c, &safe);
        }
    }
}
pub(super) fn safe_tick() {
    if !awake() {
        return;
    }
    let todo: Vec<_> = crate::snapshot::all_cards()
        .into_iter()
        .filter(|c| eligible(c) && s(&SAFE.lock()[s(c, "id")], "sig") != sig(c))
        .take(25)
        .collect();
    let mut runs = SAFE_RUNS.lock();
    runs.retain(|t| now() - t < 3600.);
    if todo.is_empty() || runs.len() >= 4 {
        return;
    }
    runs.push(now());
    drop(runs);
    let rows = todo
        .iter()
        .map(|c| {
            format!(
                "- {} | {} «{}» | libre hace {} min | pedido: {:?} | lo último que dijo: {:?}",
                s(c, "id"),
                s(c, "kind"),
                sq(s(c, "title"), 60),
                ((now() - n(c, "updated")) / 60.) as i64,
                sq(s(c, "request"), 300),
                sq(s(c, "now"), 300)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        "Eres el Sonnet revisor del Tablero de una agencia de agentes de IA. Cada tarjeta de abajo lleva 15 min o más libre. Marca cerrar=true solo si su pedido ya terminó o se abandonó y no espera nada del OWNER (ni una respuesta, ni una decisión, ni un trabajo a medias que aún importa). Cerrar termina su proceso y no se puede deshacer: ante la duda, false. Responde solo un arreglo JSON: [{{\"id\":\"<id>\",\"cerrar\":true|false,\"why\":\"<una línea en español llano>\"}}]\n\nTARJETAS:\n{rows}"
    );
    if let Ok(tx) = model(
        "sonnet",
        &prompt,
        runway().first().copied().unwrap_or(1),
        "",
        180,
    ) {
        let got = jsonish(&tx);
        let mut safe = SAFE.lock();
        for c in todo {
            if let Some(x) = got
                .as_array()
                .and_then(|a| a.iter().find(|x| s(x, "id") == s(&c, "id")))
            {
                safe[s(&c, "id")] = json!({"sig":sig(&c),"ok":x["cerrar"]==true,"why":sq(s(x,"why"),140),"t":now()});
            }
        }
        if let Some(v) = safe.as_object_mut() {
            v.retain(|_, v| now() - n(v, "t") <= 172800.);
        }
        save(cache_dir().join("cerrables.json"), &safe);
        save(cache_dir().join("safe.json"), &safe);
    }
}
pub(super) fn sweep() {
    let mut old: std::collections::HashMap<String, Vec<String>> = Default::default();
    let mut mid = vec![];
    for (who, path) in agencia_core::jobs::discover_codex_homes() {
        for f in paths(&format!("{}/sessions/*/*/*/rollout-*.jsonl", path.display())) {
            let age = now() - mtime(&f);
            if age < 21600. {
                continue;
            }
            let name = f.file_stem().unwrap_or_default().to_string_lossy();
            let id = name
                .chars()
                .skip(name.chars().count().saturating_sub(36))
                .collect::<String>();
            if age > 259200. {
                old.entry(who.clone()).or_default().push(id);
                continue;
            }
            let tx = tail(&f, 1_500_000);
            let mut first = String::new();
            let mut say = String::new();
            for line in tx.lines() {
                if let Ok(v) = serde_json::from_str::<Value>(line) {
                    if s(&v, "type") == "event_msg" {
                        if s(&v["payload"], "type") == "user_message" && first.is_empty() {
                            first = sq(s(&v["payload"], "message"), 120);
                        }
                        if s(&v["payload"], "type") == "agent_message" {
                            say = sq(s(&v["payload"], "message"), 200);
                        }
                    }
                }
            }
            mid.push(json!({"home":&who,"id":id,"age":age as i64,"first":first,"say":say}));
        }
    }
    mid.sort_by(|a, b| n(b, "age").total_cmp(&n(a, "age")));
    mid.truncate(30);
    let mut pick = vec![];
    if !mid.is_empty() && awake() && used_hour() < HAIKU_PER_HOUR {
        let rows = mid
            .iter()
            .map(|i| {
                format!(
                    "- {} | quieto {} h | pedido: {} | última palabra del agente: {}",
                    s(i, "id"),
                    n(i, "age") as u64 / 3600,
                    s(i, "first"),
                    s(i, "say")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let p = format!(
            "Eres el barrendero de una agencia de agentes de IA. Cada hilo de abajo lleva horas quieto. Marca para ARCHIVAR los que ya terminaron o se abandonaron; deja los que parecen esperar una respuesta del dueño o un trabajo a medias que aún importa. Responde solo un arreglo JSON con los ids a archivar, por ejemplo [\"a\",\"b\"].\n\n{rows}"
        );
        if let Ok(tx) = model("haiku", &p, runway().first().copied().unwrap_or(3), "", 120) {
            pick = jsonish(&tx).as_array().cloned().unwrap_or_default();
        }
    }
    let mut picked = 0;
    for i in &mid {
        if pick.contains(&i["id"]) {
            old.entry(s(i, "home").into())
                .or_default()
                .push(s(i, "id").into());
            picked += 1;
        }
    }
    let archived: usize = old
        .into_iter()
        .map(|(home, ids)| JOBS.archive_threads(&home, &ids, "thread/archive").len())
        .sum();
    save(
        cache_dir().join("janitor.json"),
        &json!({"t":now(),"archived":archived,"judged":mid.len(),"kept":mid.len()-picked}),
    );
}
fn targets() -> Vec<Value> {
    let path = std::env::var_os("AGENCIA_RE_TARGETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home()).join(".config/agencia/re_targets.json"));
    load(path)
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}
fn ghidra_config() -> Value {
    let path = std::env::var_os("AGENCIA_GHIDRA_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home()).join(".config/agencia/ghidra.json"));
    load(path).unwrap_or_else(|| json!({}))
}
fn target(id: &str) -> Option<Value> {
    if let Some(t) = targets().into_iter().find(|t| s(t, "id") == id) {
        return Some(t);
    }
    if !regex::Regex::new(r"^0x[0-9a-f]{5,8}$")
        .unwrap()
        .is_match(id)
    {
        return None;
    }
    let cfg = ghidra_config();
    let proj = s(&cfg, "default_proj");
    let proc_name = s(&cfg, "default_proc");
    if proj.is_empty() || proc_name.is_empty() {
        return None;
    }
    let n = u32::from_str_radix(&id[2..], 16).ok()?;
    let min = cfg.get("addr_min").and_then(Value::as_u64).unwrap_or(0x1000) as u32;
    let max = cfg.get("addr_max").and_then(Value::as_u64).unwrap_or(u32::MAX as u64) as u32;
    if !(min..max).contains(&n) {
        return None;
    }
    Some(json!({
        "id": id,
        "img": s(&cfg, "default_img"),
        "proj": proj,
        "proc": proc_name,
        "addr": id,
        "name": format!("FUN_{n:08x}"),
        "area": "en vivo"
    }))
}
static RE_LOCK: Mutex<()> = Mutex::new(());
pub fn re(id: &str) -> (u16, Value) {
    let Some(t) = target(id) else {
        return (
            200,
            json!({"ok":true,"targets":targets(),"haiku":used_hour(),"cap":HAIKU_PER_HOUR}),
        );
    };
    let _lock = RE_LOCK.lock();
    let dir = cache_dir().join("re");
    let _ = fs::create_dir_all(&dir);
    let out = dir.join(format!("{id}.pseudo.c"));
    let mut code = fs::read_to_string(&out).unwrap_or_default();
    let mut ms = 0u128;
    if code.trim().is_empty() || now() - mtime(&out) >= 86400. {
        let _ = fs::remove_file(&out);
        let cfg = ghidra_config();
        let default = "/opt/homebrew/opt/ghidra/libexec/support/analyzeHeadless";
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(env_bin) = std::env::var_os("AGENCIA_GHIDRA_HEADLESS") {
            candidates.push(PathBuf::from(env_bin));
        }
        let cfg_bin = s(&cfg, "headless");
        if !cfg_bin.is_empty() {
            candidates.push(PathBuf::from(cfg_bin));
        }
        candidates.push(PathBuf::from(default));
        let mut cellar = paths("/opt/homebrew/Cellar/ghidra/*/libexec/support/analyzeHeadless");
        cellar.sort();
        cellar.reverse();
        candidates.extend(cellar);
        let bin = candidates
            .iter()
            .find(|p| p.is_file())
            .cloned()
            .unwrap_or(PathBuf::from(default));
        let proj = std::env::var("AGENCIA_GHIDRA_PROJECT")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| s(&cfg, "project_dir").to_string());
        if proj.is_empty() {
            return (
                409,
                json!({"ok": false, "info": "Configura AGENCIA_GHIDRA_PROJECT o ~/.config/agencia/ghidra.json para decompilar"}),
            );
        }
        let mut c = Command::new(bin);
        let script = crate::shared::root().join("tools/ghidra");
        c.env("JAVA_HOME", "/opt/homebrew/opt/openjdk@21")
            .env(
                "PATH",
                format!(
                    "/opt/homebrew/opt/openjdk@21/bin:{}",
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .current_dir(oneshot())
            .args([
                proj.as_str(),
                s(&t, "proj"),
                "-process",
                s(&t, "proc"),
                "-noanalysis",
                "-readOnly",
                "-scriptPath",
            ])
            .arg(script)
            .arg("-postScript")
            .arg("DecompAt.java")
            .arg(&out)
            .arg(s(&t, "addr"));
        let start = Instant::now();
        let result = command(c, 300);
        ms = start.elapsed().as_millis();
        code = fs::read_to_string(&out).unwrap_or_default();
        if code.trim().is_empty() {
            let info = match result {
                Ok(o) => sq(
                    &String::from_utf8_lossy(if o.stderr.is_empty() {
                        &o.stdout
                    } else {
                        &o.stderr
                    }),
                    220,
                ),
                Err(e) if e.contains("300 s") => {
                    "Ghidra tardó más de 5 minutos y se canceló".into()
                }
                Err(e) => cut(&e, 200),
            };
            return (502, json!({"ok":false,"info":info,"ms":ms as u64}));
        }
    }
    (
        200,
        json!({"ok":true,"target":t,"created":code.lines().next().unwrap_or("").contains("(created here)"),"code":code,"ms":ms as u64}),
    )
}
pub fn re_plain(id: &str) -> (u16, Value) {
    let Some(t) = target(id) else {
        return (404, json!({"ok":false,"info":"función desconocida"}));
    };
    let dir = cache_dir().join("re");
    let code = fs::read_to_string(dir.join(format!("{id}.pseudo.c"))).unwrap_or_default();
    if code.trim().is_empty() {
        return (
            409,
            json!({"ok":false,"info":"primero decompila la función"}),
        );
    }
    let path = dir.join(format!("{id}.plain.md"));
    let mut txt = fs::read_to_string(&path)
        .unwrap_or_default()
        .trim()
        .to_owned();
    if txt.is_empty() && used_hour() < HAIKU_PER_HOUR && loops_enabled() {
        let prompt = format!(
            "Eres el traductor de un binario de PC de 2003 (x86 de 32 bits) para un ingeniero que sabe programar pero no quiere leer doscientas líneas de pseudocódigo para entender una función. Con el NOMBRE y el PSEUDOCODIGO de abajo, escribe de 4 a 6 líneas en español llano, tono humano y directo, sin markdown, sin viñetas, sin comillas y sin numerar. Cada línea es una frase corta de máximo 110 caracteres y cada línea separa una idea: qué hace en una frase; qué recibe; qué devuelve y cómo se nota que cumplió; qué puede fallar; qué parte del juego depende de ella. Usa términos de ingeniería cuando ayuden (puntero, estructura, hilo, buffer, callback) y explica el efecto con palabras normales. Sin adjetivos de relleno, sin repetir el pseudocódigo. Separa cada línea con un salto de línea real.\n\nNOMBRE: {} en {} ({})\n\nPSEUDOCODIGO:\n{}",
            s(&t, "name"),
            s(&t, "proc"),
            s(&t, "addr"),
            cut(&code, 6000)
        );
        txt = thread::spawn(move || {
            model(
                "haiku",
                &prompt,
                runway().first().copied().unwrap_or(3),
                "",
                150,
            )
            .unwrap_or_default()
        })
        .join()
        .unwrap_or_default();
        if !txt.is_empty() {
            let _ = fs::write(path, &txt);
        }
    }
    (
        if txt.is_empty() { 502 } else { 200 },
        json!({"ok":!txt.is_empty(),"plain":txt,"info":if txt.is_empty(){"El traductor no respondió"}else{""}}),
    )
}
static CONSUL: Mutex<()> = Mutex::new(());
pub fn consul_load() -> Value {
    load(cache_dir().join("consultor.json")).unwrap_or(json!([]))
}
fn facts() -> (String, String) {
    fn walk(c: &Value, d: usize, out: &mut Vec<String>) {
        out.push(format!(
            "{}- [{}] {} «{}» {} · hace {} min · {}",
            "  ".repeat(d),
            s(c, "status"),
            s(c, "kind"),
            s(c, "title"),
            s(c, "model"),
            ((now() - n(c, "updated")) / 60.) as i64,
            sq(s(c, "now"), 200)
        ));
        for c in c["children"].as_array().into_iter().flatten() {
            walk(c, d + 1, out);
        }
    }
    let st = crate::snapshot::snapshot();
    let mut rows = vec![];
    for r in st["repos"].as_array().into_iter().flatten() {
        rows.push(format!("Repo {}", s(r, "name")));
        for w in r["windows"].as_array().into_iter().flatten() {
            walk(w, 1, &mut rows);
        }
    }
    let mut done: Vec<_> = JOBS
        .list_jobs()
        .into_iter()
        .filter(|j| ["ok", "fallo", "pregunta"].contains(&j.status.as_str()))
        .collect();
    done.sort_by(|a, b| {
        (if a.ended > 0. { a.ended } else { a.ts })
            .total_cmp(&(if b.ended > 0. { b.ended } else { b.ts }))
    });
    let end = done
        .iter()
        .rev()
        .take(25)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|j| {
            format!(
                "- {} {} «{}»: {}",
                j.kind,
                j.status,
                sq(
                    if j.brief.is_empty() {
                        &j.text
                    } else {
                        &j.brief
                    },
                    90
                ),
                sq(&j.reply, 300)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    (rows.join("\n"), end)
}
pub fn consult(b: &Value) -> (bool, String) {
    if b["reset"] == true {
        save(cache_dir().join("consultor.json"), &json!([]));
        return (true, String::new());
    }
    let text = s(b, "text").trim();
    if text.is_empty() {
        return (false, "Escribe una pregunta".into());
    }
    let Some(_lock) = CONSUL.try_lock() else {
        return (false, "El consultor sigue con la pregunta anterior".into());
    };
    let model_name = if s(b, "model") == "haiku" {
        "haiku"
    } else {
        "sonnet"
    };
    let mut msgs = consul_load().as_array().cloned().unwrap_or_default();
    let (live, done) = facts();
    let talk = msgs[msgs.len().saturating_sub(10)..]
        .iter()
        .map(|m| {
            format!(
                "{}: {}",
                if s(m, "role") == "user" {
                    "OWNER"
                } else {
                    "CONSULTOR"
                },
                cut(s(m, "text"), 1500)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let repo_dir = s(b, "repo").to_string();
    let cwd = if !repo_dir.is_empty() && Path::new(&repo_dir).is_dir() {
        PathBuf::from(repo_dir)
    } else if let Ok(env_repo) = std::env::var("AGENCIA_CONSULTOR_REPO")
        && Path::new(&env_repo).is_dir()
    {
        PathBuf::from(env_repo)
    } else {
        crate::snapshot::repos()
            .as_array()
            .and_then(|a| a.first())
            .and_then(|r| r.get("path"))
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .unwrap_or_else(crate::shared::root)
    };
    let repo_name = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");
    let prompt = format!(
        "Eres el consultor independiente del OWNER sobre una agencia de agentes de IA que trabaja en el repo {repo_name}. No trabajas para el Orquestador ni para ningún agente y no cambias nada: solo lees. Lee GOAL.md o README.md si existen para saber qué se busca. Lo que dicen los agentes son afirmaciones, no hechos: compáralas con los archivos y con git log antes de darlas por buenas, y marca cada conclusión como HECHO, INFERENCIA o DESCONOCIDO. Si el trabajo se repite, va mal o el enfoque no llega a la meta, dilo sin suavizar, aunque contradiga lo que el OWNER espera. Busca patrones. Responde en español neutro de México, de tú, breve y directo, sin guiones largos.\n\nAGENTES AHORA:\n{}\n\nTRABAJOS TERMINADOS RECIENTES:\n{}\n\nCONVERSACIÓN:\n{talk}\n\nPREGUNTA DEL OWNER:\n{}",
        cut(&live, 14000),
        cut(&done, 9000),
        cut(text, 4000)
    );
    let answer = thread::spawn(move || {
        model(
            model_name,
            &format!(
                "Directorio del repo para tus lecturas: {}\n{prompt}",
                cwd.display()
            ),
            runway().first().copied().unwrap_or(3),
            "Read,Glob,Grep,Bash",
            180,
        )
    })
    .join();
    let reply = match answer {
        Ok(Ok(reply)) => reply,
        Ok(Err(e)) => return (false, sq(&e, 200)),
        Err(_) => return (false, "El modelo no contestó".into()),
    };
    msgs.push(json!({"role":"user","text":text,"t":now()}));
    msgs.push(json!({"role":"assistant","text":reply,"t":now(),"model":model_name}));
    if msgs.len() > 60 {
        msgs.drain(..msgs.len() - 60);
    }
    save(cache_dir().join("consultor.json"), &json!(msgs));
    (true, reply)
}
