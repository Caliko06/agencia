//! Reconcile pass: ended/unread, failed, and hung jobs reach their live launcher.
use super::*;
use agencia_core::{agylink, jobs::LaunchReq, procs};
use std::collections::HashMap;
const MARK: &str = "[reconcile-pass]";
const CREW: [(&str, &str, &str); 4] = [
    (
        "Reconciliador",
        "R",
        "For every job in FACTS compare what it claims with what exists: do the files it names exist, does its commit exist on that branch, is its work still uncommitted in that worktree, does its result answer what it was asked.",
    ),
    (
        "Hallazgos",
        "H",
        "Extract what these results teach for the goal: new facts, blockers, and contradictions between jobs. Check each one against its cited source before listing it.",
    ),
    (
        "Auditor",
        "A",
        "Pick the 8 numbers or claims in these results that matter most for the next decision. Re-verify each independently by re-reading the cited line or re-running a read-only grep or count. Mark each CONFIRMED, REFUTED or UNPROVEN and show what you read.",
    ),
    (
        "Cabos sueltos",
        "C",
        "Find loose ends: results nobody integrated, slice branches or worktrees with uncommitted files, duplicated work, and the hung jobs in FACTS (check their process with ps). List the options with their evidence. Do not choose one.",
    ),
];
fn repo(p: &str) -> String {
    let home = home();
    p.strip_prefix(&format!("{home}/"))
        .and_then(|p| p.split('/').next())
        .unwrap_or(p)
        .split("-wt")
        .next()
        .unwrap_or(p)
        .into()
}
fn seen_in(tx: &Path, mark: &str, after: f64) -> bool {
    tail(tx, 1_500_000)
        .lines()
        .filter(|ln| ln.contains(mark))
        .any(|ln| {
            load_line(ln)
                .and_then(|v| chrono::DateTime::parse_from_rfc3339(s(&v, "timestamp")).ok())
                .is_some_and(|t| t.timestamp() as f64 >= after - 2.)
        })
}
fn load_line(s: &str) -> Option<Value> {
    serde_json::from_str(s).ok()
}
fn transcript(sid: &str) -> Option<PathBuf> {
    paths(&format!("{}/.claude/projects/*/{sid}.jsonl", home()))
        .into_iter()
        .next()
}
pub(super) fn tick() {
    if awake() {
        reconcile();
    }
}
fn reconcile() {
    let file = cache_dir().join("reconcile.json");
    let mut seen = load(&file).unwrap_or(json!({}));
    if !seen["ids"].is_object() {
        seen["ids"] = json!({});
    }
    if !seen["pending"].is_object() {
        seen["pending"] = json!({});
    }
    let mut live: HashMap<String, Value> = HashMap::new();
    let mut orq = HashMap::new();
    let mut agy_orq = HashMap::new();
    for f in paths(&format!("{}/.claude/sessions/*.json", home())) {
        if let Some(d) = load(f) {
            if procs::pid_alive(n(&d, "pid") as i32) && !s(&d, "messagingSocketPath").is_empty() {
                let sid = s(&d, "sessionId").to_owned();
                if s(&d, "name") == "Orquestador" {
                    orq.insert(repo(s(&d, "cwd")), sid.clone());
                }
                live.insert(sid, d);
            }
        }
    }
    for inst in agylink::harvest_agy_instances() {
        let cwd = procs::proc_cwd(inst.pid).unwrap_or_else(home);
        let sid = format!("agy:{}", inst.conv_id);
        agy_orq.entry(repo(&cwd)).or_insert(sid.clone());
        live.insert(
            sid.clone(),
            json!({"sessionId":sid,"cwd":cwd,"agy":inst.conv_id}),
        );
    }
    let pending = seen["pending"].as_object().cloned().unwrap_or_default();
    for (sid, msg) in pending {
        if !live.contains_key(&sid) || deliver(&sid, msg.as_str().unwrap_or("")) {
            seen["pending"].as_object_mut().unwrap().remove(&sid);
        }
    }
    let mut boxes: HashMap<String, Vec<(String, jobs::JobRecord)>> = HashMap::new();
    for j in JOBS.list_jobs() {
        if j.text.starts_with(MARK) {
            continue;
        }
        let sid = orq
            .get(&repo(&j.repo))
            .cloned()
            .or_else(|| live.contains_key(&j.de).then(|| j.de.clone()))
            .or_else(|| {
                live.contains_key(&format!("agy:{}", j.de))
                    .then(|| format!("agy:{}", j.de))
            })
            .or_else(|| agy_orq.get(&repo(&j.repo)).cloned());
        let Some(sid) = sid else { continue };
        if j.status == "r" {
            let mut last = if j.updated > 0. { j.updated } else { j.ts };
            if j.kind == "cdx" && !j.thread.is_empty() {
                let base = agencia_core::jobs::discover_codex_homes()
                    .get(&j.home)
                    .cloned()
                    .unwrap_or_else(|| PathBuf::from(home()).join(".codex"));
                for f in paths(&format!(
                    "{}/sessions/*/*/*/*{}.jsonl",
                    base.display(),
                    j.thread
                )) {
                    last = last.max(mtime(f));
                }
            }
            let key = format!("hung:{}", j.id);
            if now() - last > 1200. && seen["ids"].get(&key).is_none() {
                seen["ids"][key] = json!(now());
                boxes.entry(sid).or_default().push(("hung".into(), j));
            }
            continue;
        }
        let ended = if j.ended > 0. {
            j.ended
        } else if j.updated > 0. {
            j.updated
        } else {
            j.ts
        };
        if j.status == "q" || seen["ids"].get(&j.id).is_some() || now() - ended > 10800. {
            continue;
        }
        seen["ids"][&j.id] = json!(now());
        let launcher = live
            .get(&j.de)
            .or_else(|| live.get(&format!("agy:{}", j.de)));
        let tx = if let Some(d) = launcher.filter(|v| v.get("agy").is_some()) {
            paths(&format!(
                "{}/.gemini/antigravity-cli/brain/{}/.system_generated/logs/transcript*.jsonl",
                home(),
                s(d, "agy")
            ))
            .into_iter()
            .next()
        } else {
            transcript(&j.de)
        };
        if tx.is_some_and(|p| seen_in(&p, &j.id, ended)) {
            continue;
        }
        let group = if j.status == "ok" { "done" } else { "bad" };
        boxes.entry(sid).or_default().push((group.into(), j));
    }
    seen["ids"]
        .as_object_mut()
        .unwrap()
        .retain(|_, v| now() - v.as_f64().unwrap_or(0.) < 604800.);
    save(&file, &seen);
    // Each window gets its own pass; workers are the same four read-only interns as Python.
    let handles: Vec<_> = boxes
        .into_iter()
        .map(|(sid, rows)| {
            let cwd = s(&live[&sid], "cwd").to_owned();
            thread::spawn(move || recon_box(sid, rows, cwd))
        })
        .collect();
    for handle in handles {
        if let Ok((sid, Some(msg))) = handle.join() {
            seen["pending"][sid] = json!(msg);
        }
    }
    save(file, &seen);
}
fn recon_box(
    sid: String,
    rows: Vec<(String, jobs::JobRecord)>,
    cwd: String,
) -> (String, Option<String>) {
    let stamp = chrono::Local::now().format("%H:%M").to_string();
    let dir = cache_dir().join("reconcile");
    let _ = fs::create_dir_all(&dir);
    let file = dir.join(format!(
        "{}-{}.md",
        chrono::Local::now().format("%Y%m%d-%H%M"),
        cut(&sid, 8)
    ));
    let mut facts = vec![];
    let mut views = vec![];
    for (group, j) in &rows {
        let v = real::job(j, &j.repo);
        let label = match group.as_str() {
            "done" => "ENDED, NOT READ",
            "bad" => "FAILED OR ASKING",
            _ => "RUNNING WITH NO ACTIVITY FOR 20+ MIN",
        };
        let refs = v["refs"]
            .as_array()
            .map(|a| a.iter().map(|r| s(r, "p")).collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        facts.push(format!("## {label}: {} {}\nworktree: {}\nbrief: {}\nsteps: {}\nfiles it cites: {}\nits own report:\n{}",s(&v,"who"),j.id,j.repo,s(&v,"brief"),j.steps,if refs.is_empty(){"none"}else{&refs},if j.reply.is_empty(){s(&v,"line")}else{s(&v,"reply")}));
        views.push(v);
    }
    let mut body = format!(
        "# Reconcile pass {stamp}, {cwd}\n\n{}\n",
        facts.join("\n\n")
    );
    let _ = fs::write(&file, &body);
    let mut crew = vec![];
    for (who, pfx, role) in CREW {
        let text = format!(
            "{MARK} You are {who}, one of 4 reconcile interns on the {stamp} pass of {cwd}. Read the FACTS file first: {}\nYour job: {role}\nRead-only: never edit, commit, launch agents or message anyone. Data first: every item names its exact job id, file path with line, commit hash or command, and shows what you read or the command output. Anything you could not prove goes under UNPROVEN. Number your items {pfx}-1, {pfx}-2, at most 12, inside HECHO. You are not the final word: the Orquestador decides and will check what it relies on. EN UNA LÍNEA: {who}, pasada {stamp}",
            file.display()
        );
        if let Ok(j) = JOBS.enqueue(LaunchReq {
            kind: "bec".into(),
            text,
            repo: Some(cwd.clone()),
            de: Some(sid.clone()),
            brief: Some(format!("{who} · pasada {stamp}")),
            ..Default::default()
        }) {
            crew.push((who, j.id));
        }
    }
    let end = now() + 360.;
    while now() < end
        && crew.iter().any(|(_, id)| {
            JOBS.load(id)
                .is_some_and(|j| ["q", "r"].contains(&j.status.as_str()))
        })
    {
        thread::sleep(Duration::from_secs(5));
    }
    let mut team = vec![];
    for (who, id) in &crew {
        let j = JOBS.load(id);
        let status = j.as_ref().map(|j| j.status.as_str()).unwrap_or("?");
        let reply = j
            .as_ref()
            .map(|j| j.reply.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("No entregó todavía.");
        body.push_str(&format!("\n# {who} (bec {id}, {status})\n\n{reply}\n"));
        team.push(format!("- {who} (bec {id}, {status}): {}", md(reply, 260)));
    }
    let _ = fs::write(&file, body);
    let count = |g: &str| rows.iter().filter(|r| r.0 == g).count();
    let head = format!(
        "[Agencia reconcile, automatic pass every 7 min, not typed by the OWNER] {} ended unread, {} failed or asking, {} hung.\nThe intern reports below are leads, not truth. You decide and have the last word: verify what you rely on, then pick the next step and keep advancing the goal.\n",
        count("done"),
        count("bad"),
        count("hung")
    );
    let team = team.join("\n");
    let tail = format!("\nFull facts and reports: {}", file.display());
    let room = 3700usize.saturating_sub(head.len() + team.len() + tail.len());
    let mut items = String::new();
    for ((group, j), v) in rows.iter().zip(&views) {
        let line = format!(
            "- {group} {} {} {}: {}\n",
            s(v, "who"),
            j.id,
            s(v, "brief"),
            cut(s(v, "line"), 130)
        );
        if items.len() + line.len() > room {
            items.push_str("- and more, see the file\n");
            break;
        }
        items.push_str(&line);
    }
    let msg = format!("{head}{items}Interns:\n{team}{tail}");
    let sent = deliver(&sid, &msg);
    (sid, if sent { None } else { Some(msg) })
}
fn deliver(sid: &str, text: &str) -> bool {
    if let Some(id) = sid.strip_prefix("agy:") {
        return agylink::harvest_agy_instances()
            .into_iter()
            .find(|i| i.conv_id == id)
            .is_some_and(|i| agylink::send_agy_message(&i, text, Some("Agencia 8800")).is_ok());
    }
    paths(&format!("{}/.claude/sessions/*.json", home()))
        .iter()
        .filter_map(load)
        .find(|d| s(d, "sessionId") == sid && procs::pid_alive(n(d, "pid") as i32))
        .is_some_and(|d| {
            !s(&d, "messagingSocketPath").is_empty()
                && agencia_core::cartero::send_claude_uds(
                    Path::new(s(&d, "messagingSocketPath")),
                    text,
                    Some("Agencia 8800"),
                )
                .is_ok()
        })
}
