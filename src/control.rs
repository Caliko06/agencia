//! Write path: faithful Python control actions, backed by agencia-core.
use agencia_core::{
    agylink,
    bus::Bus,
    cartero,
    jobs::{self, JobManager, JobRecord, LaunchReq},
    procs,
    types::CoreError,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::LazyLock,
    thread,
    time::{Duration, Instant},
};

static JOBS: LazyLock<JobManager> = LazyLock::new(|| JobManager::new(crate::shared::root()));
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
fn cut(v: &str, n: usize) -> String {
    v.chars().take(n).collect()
}
fn fail(v: &str) -> (bool, String) {
    (false, v.into())
}
fn good(v: &str) -> (bool, String) {
    (true, v.into())
}
fn err(e: CoreError) -> String {
    match e {
        CoreError::InvalidInput(s)
        | CoreError::NotFound(s)
        | CoreError::External(s)
        | CoreError::Http(s) => s,
        CoreError::Io(e) => e.to_string(),
        e => e.to_string(),
    }
}
fn response(code: u16, ok: bool, info: impl Into<String>) -> (u16, Value) {
    (code, json!({"ok":ok,"info":info.into()}))
}
fn result(r: (bool, String), bad: u16) -> (u16, Value) {
    response(if r.0 { 200 } else { bad }, r.0, r.1)
}
fn expand(v: &str) -> String {
    if v == "~" {
        crate::shared::home()
    } else if let Some(v) = v.strip_prefix("~/") {
        format!("{}/{}", crate::shared::home(), v)
    } else {
        v.into()
    }
}
fn read_json(p: impl AsRef<Path>) -> Option<Value> {
    serde_json::from_slice(&fs::read(p).ok()?).ok()
}
fn pid(v: &Value) -> i32 {
    v.get("pid").and_then(Value::as_i64).unwrap_or(0) as i32
}
fn session_of(sid: &str) -> Option<Value> {
    let entries = glob::glob(&format!(
        "{}/.claude/sessions/*.json",
        crate::shared::home()
    ))
    .ok()?;
    entries
        .flatten()
        .filter_map(read_json)
        .find(|v| s(v, "sessionId") == sid && procs::pid_alive(pid(v)))
}
fn agy_send(id: &str, text: &str, sender: &str) -> (bool, String) {
    if let Some(inst) = agylink::harvest_agy_instances()
        .into_iter()
        .find(|i| i.conv_id == id)
    {
        return match agylink::send_agy_message(&inst, text, Some(sender)) {
            Ok(()) => good("enviado a Antigravity"),
            Err(e) => (false, cut(&err(e), 200)),
        };
    }
    let live = procs::discover_procs()
        .map(|p| procs::agy_pids(&p).values().any(|v| v == id))
        .unwrap_or(false);
    fail(if live {
        "aún no tengo su enlace: llega la primera vez que esa ventana corre un comando"
    } else {
        "esa ventana de Antigravity ya no está abierta"
    })
}
fn deliver(sid: &str, text: &str) -> (bool, String) {
    if let Some(id) = sid.strip_prefix("agy:") {
        return agy_send(id, text, "Agencia 8800");
    }
    if let Some(v) = session_of(sid).filter(|v| !s(v, "messagingSocketPath").is_empty()) {
        let t = Instant::now();
        return match cartero::send_claude_uds(
            Path::new(s(&v, "messagingSocketPath")),
            text,
            Some("Agencia 8800"),
        ) {
            Ok(()) => good(&format!(
                "sent via Claude socket in {} ms",
                t.elapsed().as_millis()
            )),
            Err(e) => (false, err(e)),
        };
    }
    fail("esa ventana ya no está viva")
}
pub fn send(to: &str, text: &str) -> (bool, String) {
    let text = cut(text.trim(), 4000);
    if let Some(id) = to.strip_prefix("agy:") {
        agy_send(id, &text, "el OWNER, escrito en la Agencia")
    } else {
        deliver(
            to,
            &format!("[Mensaje del OWNER escrito en la Agencia] {text}"),
        )
    }
}
// Fresh kernel read on every signal guard, deliberately never a cached snapshot.
fn ps_cmd(pid: i32) -> String {
    procs::proc_args(pid).unwrap_or_default()
}
fn signal(pid: i32, sig: i32) -> Result<(), String> {
    if pid <= 0 {
        return Err("pid inválido".into());
    }
    if unsafe { libc::kill(pid, sig) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}
fn osa(script: &str) -> Result<Output, String> {
    let mut child = Command::new("osascript")
        .args(["-e", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let t = Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return child.wait_with_output().map_err(|e| e.to_string());
        }
        if t.elapsed() > Duration::from_secs(15) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("osascript excedió 15 s".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}
fn osa_text(script: &str) -> String {
    osa(script)
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}
fn shell_quote(v: &str) -> String {
    format!("'{}'", v.replace('\'', "'\"'\"'"))
}
fn terminal(cwd: &str, env: &[(&str, String)], argv: &[String]) -> (bool, String) {
    let cmd = format!(
        "cd {} && {} {}",
        shell_quote(cwd),
        env.iter()
            .map(|(k, v)| format!("{k}={}", shell_quote(v)))
            .collect::<Vec<_>>()
            .join(" "),
        argv.iter()
            .map(|a| shell_quote(a))
            .collect::<Vec<_>>()
            .join(" ")
    );
    match osa(&format!(
        "tell application \"Terminal\"\nactivate\ndo script \"{}\"\nend tell",
        cmd.replace('\\', "\\\\").replace('"', "\\\"")
    )) {
        Ok(o) if o.status.success() => good("abierto en Terminal"),
        Ok(o) => {
            let e = String::from_utf8_lossy(&o.stderr);
            fail(&cut(
                if e.trim().is_empty() {
                    "osascript falló"
                } else {
                    e.trim()
                },
                200,
            ))
        }
        Err(e) => fail(&cut(&e, 200)),
    }
}
fn clean_window(v: &Value) -> (bool, String) {
    if s(v, "status") != "idle" {
        return fail("solo se limpia una ventana libre: esta trabaja o te espera");
    }
    let tty = procs::discover_procs()
        .ok()
        .and_then(|t| t.procs.get(&pid(v)).map(|p| p.tty.clone()))
        .unwrap_or_default();
    if tty.is_empty() || tty == "??" {
        return fail("su proceso no tiene terminal");
    }
    let tty = format!("/dev/{tty}");
    let script = format!(
        "tell application \"Terminal\"\nrepeat with w in windows\nrepeat with t in tabs of w\nif tty of t is \"{tty}\" then\nset selected of t to true\nset index of w to 1\nactivate\nreturn \"ok\"\nend if\nend repeat\nend repeat\nreturn \"\"\nend tell"
    );
    if osa_text(&script) != "ok" {
        return fail("no encontré su ventana de Terminal");
    }
    thread::sleep(Duration::from_millis(300));
    if osa_text("tell application \"System Events\" to return frontmost of process \"Terminal\"")
        != "true"
        || osa_text("tell application \"Terminal\" to return tty of selected tab of front window")
            != tty
    {
        return fail("su ventana no quedó al frente; no tecleo nada");
    }
    let _ = osa("tell application \"System Events\" to key code 53");
    thread::sleep(Duration::from_millis(300));
    let _ = osa("tell application \"System Events\" to keystroke \"/clear\"");
    thread::sleep(Duration::from_millis(150));
    let _ = osa("tell application \"System Events\" to key code 36");
    good("/clear enviado")
}
fn sub_file(key: &str) -> Option<(PathBuf, String)> {
    // Match literal IDs, never allow a supplied glob to select another subagent.
    let pat = format!(
        "{}/.claude/projects/*/*/subagents/agent-{}.jsonl",
        glob::Pattern::escape(&crate::shared::home()),
        glob::Pattern::escape(key)
    );
    let p = glob::glob(&pat).ok()?.flatten().next()?;
    let sid = p.parent()?.parent()?.file_name()?.to_str()?.to_owned();
    Some((p, sid))
}
fn stopped_sub(p: &Path) -> bool {
    let Ok(mut f) = fs::File::open(p) else {
        return false;
    };
    let len = f.metadata().map(|v| v.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(len.saturating_sub(1500)));
    let mut b = Vec::new();
    let _ = f.read_to_end(&mut b);
    b.windows(b"Request interrupted".len())
        .any(|w| w == b"Request interrupted")
}
fn mtime(p: &Path) -> f64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|t| t.as_secs_f64())
        .unwrap_or(0.0)
}
fn live(j: &JobRecord) -> bool {
    matches!(j.status.as_str(), "q" | "r")
}
fn archive(j: &mut JobRecord) {
    if j.kind == "cdx" && !j.thread.is_empty() && !j.archived {
        j.archived = !JOBS
            .archive_threads(&j.home, &[j.thread.clone()], "thread/archive")
            .is_empty();
        let _ = JOBS.save(j);
    }
}
pub fn stop(id: &str) -> (bool, String) {
    match JOBS.stop(id) {
        Ok(()) => good("detenido"),
        Err(e) => (false, err(e)),
    }
}
pub fn control(id: &str, action: &str) -> (bool, String) {
    if !matches!(action, "pause" | "resume" | "clean" | "close") {
        return fail("acción desconocida");
    }
    let (kind, key) = id.split_once(':').unwrap_or((id, ""));
    let sig = if action == "pause" {
        libc::SIGSTOP
    } else {
        libc::SIGCONT
    };
    match kind {
        "w" => {
            let Some(v) = session_of(key) else {
                return fail("esa ventana ya no está viva");
            };
            let p = pid(&v);
            if !ps_cmd(p).contains("claude") {
                return fail("ese pid ya no es Claude");
            }
            if action == "pause" || action == "resume" {
                return match signal(p, sig) {
                    Ok(()) => good(if action == "pause" {
                        "en pausa"
                    } else {
                        "reanudada"
                    }),
                    Err(e) => (false, e),
                };
            }
            if action == "close" {
                let _ = signal(p, libc::SIGCONT);
                return match signal(p, libc::SIGTERM) {
                    Ok(()) => good("cerrando la CLI"),
                    Err(e) => (false, e),
                };
            }
            if procs::discover_procs()
                .ok()
                .and_then(|t| t.procs.get(&p).map(|v| v.stat.contains('T')))
                .unwrap_or(false)
            {
                return fail("está en pausa: reanúdala primero");
            }
            clean_window(&v)
        }
        "job" => {
            let Some(mut j) = JOBS.load(key) else {
                return fail("ese trabajo ya no existe");
            };
            if action == "close" {
                return stop(key);
            }
            if action == "clean" {
                if live(&j) {
                    return fail("sigue corriendo: ciérralo primero");
                }
                archive(&mut j);
                crate::shared::hide([id.to_owned()]);
                return good(if j.archived {
                    "fuera del tablero, hilo archivado"
                } else {
                    "fuera del tablero"
                });
            }
            if j.kind != "cdx" {
                return fail(
                    "el becario vive en el servicio de OpenCode: se puede cerrar, no pausar",
                );
            }
            let p = j.pid.unwrap_or(0);
            if !live(&j) || p <= 0 || !ps_cmd(p).contains(&j.id) {
                return fail("ya no está corriendo");
            }
            let pg = unsafe { libc::getpgid(p) };
            if pg < 0 {
                return fail(&std::io::Error::last_os_error().to_string());
            }
            let r = if pg != unsafe { libc::getpgrp() } {
                unsafe { libc::killpg(pg, sig) }
            } else {
                unsafe { libc::kill(p, sig) }
            };
            if r != 0 {
                return fail(&std::io::Error::last_os_error().to_string());
            }
            good(if action == "pause" {
                "en pausa"
            } else {
                "reanudado"
            })
        }
        "sub" => {
            let Some((p, sid)) = sub_file(key) else {
                return fail("ese subagente ya no existe");
            };
            let done = read_json(p.with_extension("meta.json"))
                .and_then(|v| v.get("finished").cloned())
                .is_some_and(|v| truthy(&v))
                || crate::shared::now() - mtime(&p) > 180.0
                || stopped_sub(&p);
            if action == "clean" {
                if !done {
                    return fail("sigue trabajando: ciérralo primero");
                }
                crate::shared::hide([id.to_owned()]);
                return good("fuera del tablero");
            }
            if action == "close" {
                if done {
                    return fail("ya terminó: límpialo");
                }
                let r = send(
                    &sid,
                    &format!(
                        "Detén ya el subagente {key} con TaskStop (task_id {key}) y no lo relances. Pedido del OWNER desde la Agencia."
                    ),
                );
                return if r.0 {
                    good("su ventana lo detiene con TaskStop")
                } else {
                    r
                };
            }
            fail("un subagente corre dentro de su ventana: no tiene proceso propio que pausar")
        }
        "agy" => agy_control(id, key, action),
        _ => fail("esa tarjeta no se controla desde aquí"),
    }
}
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}
mod agy;
fn agy_control(id: &str, key: &str, action: &str) -> (bool, String) {
    let p = PathBuf::from(crate::shared::home())
        .join(".gemini/antigravity-cli/conversation_summaries.db");
    let row=rusqlite::Connection::open_with_flags(p,rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok().and_then(|db| db.query_row("SELECT COALESCE(parent_conversation_id,''), COALESCE(agent_name,''), COALESCE(status,'') FROM conversation_summaries WHERE conversation_id=?1",[key],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).ok());
    let Some((parent, agent, status)) = row.filter(|r| !r.0.is_empty()) else {
        return fail("esa ventana vive en tu terminal: contrólala ahí");
    };
    let card = crate::snapshot::find_card(id);
    let run = status.ends_with("RUNNING")
        || agy::transcript_running(key)
        || card.as_ref().is_some_and(|c| s(c, "status") == "working");
    if action == "clean" {
        if run {
            return fail("sigue trabajando: ciérralo primero");
        }
        crate::shared::hide([id.to_owned()]);
        return good("fuera del tablero");
    }
    if action == "close" {
        if !run {
            return fail("ya terminó: límpialo");
        }
        let r = agy_send(
            &parent,
            &format!(
                "Detén ya el subagente {} (conversation_id {key}) y no lo relances. Pedido del OWNER desde la Agencia.",
                if agent.is_empty() { key } else { &agent }
            ),
            "el OWNER, escrito en la Agencia",
        );
        return if r.0 {
            good("su ventana lo detiene")
        } else {
            r
        };
    }
    fail("un subagente corre dentro de su ventana: no tiene proceso propio que pausar")
}
mod cleanup;
pub use cleanup::limpiar;

fn enqueue(b: &Value, kind: &str, home: &str, card: bool) -> (u16, Value) {
    if kind == "cdx" && !jobs::discover_codex_homes().contains_key(home) {
        return response(400, false, "cuenta codex desconocida");
    }
    let req = LaunchReq {
        kind: kind.into(),
        text: s(b, "text").into(),
        home: home.into(),
        repo: b.get("repo").and_then(Value::as_str).map(str::to_owned),
        de: Some("owner".into()),
        effort: Some(s(b, "effort").into()),
        ..Default::default()
    };
    match JOBS.enqueue(req) {
        Ok(j) => {
            if card {
                (
                    200,
                    json!({"ok":true,"id":j.id,"card":format!("job:{}",j.id)}),
                )
            } else {
                (200, json!({"ok":true,"id":j.id}))
            }
        }
        Err(e) => response(400, false, err(e)),
    }
}
pub fn launch(b: &Value) -> (u16, Value) {
    let home = if s(b, "home").is_empty() {
        "default"
    } else {
        s(b, "home")
    };
    if b.get("tui").is_some_and(truthy) && s(b, "kind") == "cdx" {
        let homes = jobs::discover_codex_homes();
        let repo = expand(s(b, "repo"));
        let text = s(b, "text").trim();
        let Some(h) = homes
            .get(home)
            .filter(|_| Path::new(&repo).is_dir() && !text.is_empty())
        else {
            return response(400, false, "falta cuenta, repo o pedido");
        };
        let mut argv = vec!["codex".into()];
        if !s(b, "effort").is_empty() {
            argv.extend([
                "-c".into(),
                format!("model_reasoning_effort={}", s(b, "effort")),
            ]);
        }
        argv.push(text.into());
        return result(
            terminal(
                &repo,
                &[("CODEX_HOME", h.to_string_lossy().into_owned())],
                &argv,
            ),
            502,
        );
    }
    enqueue(b, s(b, "kind"), home, true)
}
pub fn codex_new(b: &Value) -> (u16, Value) {
    let who = if s(b, "who").is_empty() { "default" } else { s(b, "who") };
    if !jobs::discover_codex_homes().contains_key(who) {
        return response(400, false, "cuenta codex desconocida");
    };
    enqueue(b, "cdx", who, false)
}
pub fn codex_open(b: &Value) -> (u16, Value) {
    let Some(mut j) = JOBS
        .load(s(b, "id").strip_prefix("job:").unwrap_or(s(b, "id")))
        .filter(|j| j.kind == "cdx" && !j.thread.is_empty())
    else {
        return response(404, false, "no es un trabajo Codex con hilo");
    };
    if live(&j) {
        return response(
            409,
            false,
            "sigue corriendo en segundo plano; ábrelo cuando termine",
        );
    }
    if j.archived {
        j.archived = JOBS
            .archive_threads(&j.home, &[j.thread.clone()], "thread/unarchive")
            .is_empty();
        let _ = JOBS.save(&j);
    }
    let homes = jobs::discover_codex_homes();
    let Some(home) = homes.get(&j.home) else {
        return response(502, false, "cuenta Codex no disponible");
    };
    result(
        terminal(
            &j.repo,
            &[("CODEX_HOME", home.to_string_lossy().into_owned())],
            &["codex".into(), "resume".into(), j.thread],
        ),
        502,
    )
}
pub fn codex_clear(b: &Value) -> (u16, Value) {
    let Some(mut j) = JOBS
        .load(s(b, "id").strip_prefix("job:").unwrap_or(s(b, "id")))
        .filter(|j| j.kind == "cdx")
    else {
        return response(404, false, "no es un trabajo Codex");
    };
    let mut info = "ya había terminado".to_string();
    if live(&j) {
        let r = stop(&j.id);
        if !r.0 {
            return response(409, false, r.1);
        }
        info = r.1;
        j = JOBS.load(&j.id).unwrap_or(j);
    }
    archive(&mut j);
    info.push_str(if j.archived {
        ", hilo archivado"
    } else if j.thread.is_empty() {
        ", sin hilo que archivar"
    } else {
        ", no se pudo archivar el hilo"
    });
    response(200, true, info)
}
mod network;
pub use network::{bus_reply, bus_send, bus_view};
