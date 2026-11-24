use super::*;
fn ptable() -> procs::ProcSnapshot {
    procs::discover_procs().unwrap_or_default()
}
fn tree(pid: i32, t: &procs::ProcSnapshot) -> Vec<i32> {
    let mut out = Vec::new();
    let mut stack = vec![pid];
    let mut seen = HashSet::new();
    while let Some(p) = stack.pop() {
        if !seen.insert(p) {
            continue;
        }
        out.push(p);
        if let Some(k) = t.children.get(&p) {
            stack.extend(k);
        }
    }
    out.reverse();
    out
}
fn kill_tree(pids: &[i32]) {
    for &p in pids {
        let _ = signal(p, libc::SIGCONT);
        let _ = signal(p, libc::SIGTERM);
    }
    for _ in 0..30 {
        if !pids.iter().any(|p| procs::pid_alive(*p)) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    for &p in pids {
        let _ = signal(p, libc::SIGKILL);
    }
}
fn login(cmd: &str) -> bool {
    cmd.starts_with('-')
        || cmd == "login"
        || cmd
            .strip_prefix("login")
            .is_some_and(|s| s.starts_with(|c: char| !c.is_alphanumeric() && c != '_'))
}
fn close_tty(pid: i32) -> String {
    let t = ptable();
    let tty = t
        .procs
        .get(&pid)
        .map(|p| p.tty.clone())
        .unwrap_or("??".into());
    let mut top = pid;
    let mut seen = HashSet::new();
    while seen.insert(top) {
        let Some(p) = t.procs.get(&top) else { break };
        let Some(up) = t.procs.get(&p.ppid) else {
            break;
        };
        if p.ppid <= 1 || up.tty != tty || login(&up.args) {
            break;
        }
        top = p.ppid;
    }
    kill_tree(&tree(top, &t));
    if tty.is_empty() || tty == "??" {
        return "sin terminal".into();
    }
    if !t
        .procs
        .get(&top)
        .and_then(|p| t.procs.get(&p.ppid))
        .is_some_and(|p| login(&p.args))
    {
        return "procesos cerrados; su pestaña no tiene shell de login".into();
    }
    let script = format!(
        "tell application \"Terminal\"\nrepeat with w in windows\nrepeat with t in tabs of w\nif tty of t is \"/dev/{tty}\" then\ndo script \"exit\" in t\ndelay 1\ntry\nif (count of tabs of w) is 1 and (tty of tab 1 of w) is \"/dev/{tty}\" then close w\nend try\nreturn \"ok\"\nend if\nend repeat\nend repeat\nreturn \"\"\nend tell"
    );
    if osa_text(&script) == "ok" {
        "pestaña cerrada".into()
    } else {
        "procesos cerrados; no encontré su pestaña de Terminal".into()
    }
}
fn kill_orphans() -> usize {
    let t = ptable();
    let me = unsafe { libc::getuid() };
    let mut hit = Vec::new();
    let mut pad = Vec::new();
    let orphan = regex::Regex::new(
        r"\bmcp\b|chrome-bot|Chrome for Testing|ms-playwright|playwright|shell-snapshots",
    )
    .unwrap();
    let keep = regex::Regex::new(
        r"opencode serve|hermes|tools/serve\.py|agencia\.py|tmux|Traductor|tailscale|dnsmasq|adb ",
    )
    .unwrap();
    for (&p, v) in &t.procs {
        if v.ppid != 1 || v.uid != me || !orphan.is_match(&v.args) || keep.is_match(&v.args) {
            continue;
        }
        if v.args.contains("crashpad_handler") {
            pad.push(p)
        } else {
            hit.extend(tree(p, &t))
        }
    }
    if !t
        .procs
        .iter()
        .any(|(p, v)| !hit.contains(p) && v.args.contains("MacOS/Google Chrome for Testing"))
    {
        hit.extend(pad)
    }
    kill_tree(&hit);
    hit.len()
}
fn close_card(c: &Value) -> (bool, String) {
    let id = s(c, "id");
    let (kind, key) = id.split_once(':').unwrap_or((id, ""));
    match kind {
        "w" => {
            let Some(v) = session_of(key).filter(|v| ps_cmd(pid(v)).contains("claude")) else {
                return fail("ya no está viva");
            };
            if v.get("status")
                .is_some_and(|x| !x.is_null() && x.as_str() != Some("idle"))
            {
                return fail("volvió a trabajar");
            }
            return good(&close_tty(pid(&v)));
        }
        "job" => {
            let Some(mut j) = JOBS.load(key) else {
                return fail("ya no existe");
            };
            if live(&j) {
                let _ = stop(key);
                j = JOBS.load(key).unwrap_or(j)
            }
            if let Some(p) = j
                .pid
                .filter(|p| procs::pid_alive(*p) && ps_cmd(*p).contains(&j.id))
            {
                kill_tree(&tree(p, &ptable()));
            }
            archive(&mut j);
        }
        "oc" => {
            let d = expand(s(c, "cwd"));
            let t = ptable();
            let mine: Vec<i32> = t
                .procs
                .iter()
                .filter_map(|(&p, v)| {
                    let mut args = v.args.split_whitespace();
                    let exe = args.next().unwrap_or("");
                    let next = args.next().unwrap_or("");
                    (v.tty != "??"
                        && Path::new(exe).file_name().and_then(|n| n.to_str()) == Some("opencode")
                        && !matches!(next, "serve" | "run" | "api")
                        && procs::proc_cwd(p).as_deref() == Some(&d))
                    .then_some(p)
                })
                .collect();
            if mine.len() == 1 {
                close_tty(mine[0]);
            }
        }
        _ => {}
    }
    crate::shared::hide([id.to_owned()]);
    good("fuera")
}
fn free(c: &Value, now: f64) -> bool {
    let children_busy = c
        .get("children")
        .and_then(Value::as_array)
        .is_some_and(|a| a.iter().any(busy));
    matches!(
        s(c, "kind"),
        "claude" | "opencode" | "cdx" | "bec" | "cla" | "sub"
    ) && matches!(s(c, "status"), "idle" | "done" | "failed")
        && !c.get("paused").is_some_and(truthy)
        && now - c.get("updated").and_then(Value::as_f64).unwrap_or(now) >= 900.0
        && !(s(c, "kind") == "claude" && (s(c, "title") == "Orquestador" || children_busy))
}
fn busy(c: &Value) -> bool {
    !matches!(s(c, "status"), "idle" | "done" | "failed")
        || c.get("children")
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(busy))
}
pub fn limpiar() -> (bool, String) {
    crate::shared::invalidate();
    let _ = crate::snapshot::state();
    let now = crate::shared::now();
    let mut ok = 0;
    let mut bad = Vec::new();
    for c in crate::snapshot::all_cards()
        .into_iter()
        .filter(|c| free(c, now) && c.get("safe").is_some_and(truthy))
    {
        let r = close_card(&c);
        if r.0 {
            ok += 1
        } else {
            bad.push(format!(
                "{}: {}",
                cut(
                    &s(&c, "title")
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" "),
                    30
                ),
                r.1
            ));
        }
    }
    let n = kill_orphans();
    crate::shared::invalidate();
    let mut msg = format!("{ok} cerradas, {n} procesos huérfanos terminados");
    if let Some(first) = bad.first() {
        msg.push_str(&format!("; no pude con {} ({first})", bad.len()));
    }
    good(&msg)
}
