//! HTTP + SSE + static server (axum). Owned by the orchestrator. Every handler runs module code on the blocking pool,
//! so a slow collector never stalls another request. /api/events sends the state on connect, then only changes.
use crate::{avance, control, loops, shared, snapshot};
use axum::body::{Body, Bytes};
use axum::extract::{Query, Request};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::watch;

type Q = Query<HashMap<String, String>>;

/// (version, full body, changed-keys-only body)
type Tick = (u64, Arc<Vec<u8>>, Arc<Vec<u8>>);
static TICK: OnceLock<watch::Sender<Tick>> = OnceLock::new();
fn tick() -> &'static watch::Sender<Tick> {
    TICK.get_or_init(|| watch::channel((0, Arc::new(Vec::new()), Arc::new(Vec::new()))).0)
}

fn bytes(code: u16, ct: &'static str, body: Vec<u8>, cache: &'static str) -> Response {
    let mut r = Response::new(Body::from(body));
    *r.status_mut() = StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, ct.parse().unwrap());
    h.insert(header::CACHE_CONTROL, cache.parse().unwrap());
    h.insert("x-content-type-options", "nosniff".parse().unwrap());
    r
}
fn js(code: u16, v: Value) -> Response {
    bytes(code, "application/json", serde_json::to_vec(&v).unwrap_or_default(), "no-store")
}
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f).await.expect("handler panicked")
}
fn q(q: &HashMap<String, String>, k: &str) -> String {
    q.get(k).cloned().unwrap_or_default()
}
fn okinfo(ok: bool, info: String, bad: u16) -> Response {
    js(if ok { 200 } else { bad }, json!({"ok": ok, "info": info}))
}

pub fn serve() {
    if shared::loops_enabled() {
        loops::spawn_all();
    }
    std::thread::Builder::new()
        .name("warmup".into())
        .spawn(|| {
            let _ = loops::quota();
            let _ = snapshot::state();
        })
        .ok();
    std::thread::Builder::new().name("ticker".into()).spawn(ticker).unwrap();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().max_blocking_threads(64).build().unwrap();
    rt.block_on(async {
        let app = Router::new()
            .route("/api/state", get(|| async { let b = blocking(snapshot::state).await; bytes(200, "application/json", (*b).clone(), "no-store") }))
            .route("/api/events", get(events))
            .route("/api/agent", get(|Query(p): Q| async move {
                match blocking(move || snapshot::agent(&q(&p, "id"))).await {
                    Some(d) => js(200, d),
                    None => js(404, json!({"ok": false, "info": "no encontrado"})),
                }
            }))
            .route("/api/activity", get(|| async { js(200, blocking(snapshot::activity).await) }))
            .route("/api/repos", get(|| async { js(200, blocking(snapshot::repos).await) }))
            .route("/api/shot", get(|Query(p): Q| async move {
                let idx: usize = match q(&p, "index").parse() { Ok(i) => i, Err(_) => return js(404, json!({"ok": false})) };
                match blocking(move || snapshot::shot_png(&q(&p, "id"), idx)).await {
                    Some(b) => bytes(200, "image/png", b, "no-store"),
                    None => js(404, json!({"ok": false})),
                }
            }))
            .route("/api/file", get(|Query(p): Q| async move {
                match blocking(move || snapshot::evidence_file(&q(&p, "p"), &q(&p, "base"))).await {
                    Ok((d, ct)) => {
                        let mut r = bytes(200, "application/octet-stream", d, "no-store");
                        if let Ok(v) = ct.parse() { r.headers_mut().insert(header::CONTENT_TYPE, v); }
                        r
                    }
                    Err(info) => js(404, json!({"ok": false, "info": info})),
                }
            }))
            .route("/api/repofile", get(|Query(p): Q| async move {
                match blocking(move || avance::repofile(&q(&p, "repo"), &q(&p, "p"))).await {
                    Some(b) => bytes(200, "image/png", b, "max-age=86400"),
                    None => js(404, json!({"ok": false})),
                }
            }))
            .route("/api/real", get(|Query(p): Q| async move { js(200, blocking(move || loops::real_view(&q(&p, "repo"))).await) }))
            .route("/api/re", get(|Query(p): Q| async move { let (c, v) = blocking(move || loops::re(&q(&p, "id"))).await; js(c, v) }))
            .route("/api/re/plain", get(|Query(p): Q| async move { let (c, v) = blocking(move || loops::re_plain(&q(&p, "id"))).await; js(c, v) }))
            .route("/api/bus", get(|Query(p): Q| async move { let (c, v) = blocking(move || control::bus_view(&p)).await; js(c, v) }))
            .route("/api/consultor", get(|| async { js(200, json!({"ok": true, "msgs": blocking(loops::consul_load).await})) }).post(post_api))
            .route("/api/pulse", post(post_api))
            .route("/api/send", post(post_api))
            .route("/api/launch", post(post_api))
            .route("/api/stop", post(post_api))
            .route("/api/codex/new", post(post_api))
            .route("/api/codex/open", post(post_api))
            .route("/api/codex/clear", post(post_api))
            .route("/api/control", post(post_api))
            .route("/api/avance/voto", post(post_api))
            .route("/api/bus/send", post(post_api))
            .route("/api/bus/reply", post(post_api))
            .fallback(fallback_handler)
            .layer(axum::extract::DefaultBodyLimit::max(100_000));
        let port = shared::port();
        let l = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.expect("bind");
        println!("Agencia (Rust) en http://127.0.0.1:{port}");
        axum::serve(l, app).await.unwrap();
    });
}

const POSTS: &[&str] = &[
    "/api/pulse", "/api/send", "/api/launch", "/api/stop", "/api/codex/new", "/api/codex/open", "/api/codex/clear",
    "/api/control", "/api/avance/voto", "/api/consultor", "/api/bus/send", "/api/bus/reply",
];

async fn post_api(headers: HeaderMap, req: Request) -> Response {
    let path = req.uri().path().to_string();
    if !POSTS.contains(&path.as_str()) {
        return js(404, json!({"ok": false, "info": "no existe"}));
    }
    let port = shared::port();
    if let Some(o) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        if o != format!("http://127.0.0.1:{port}") && o != format!("http://localhost:{port}") {
            return js(403, json!({"ok": false, "info": "origen no permitido"}));
        }
    }
    let raw: Bytes = match axum::body::to_bytes(req.into_body(), 100_000).await {
        Ok(b) => b,
        Err(_) => return js(400, json!({"ok": false})),
    };
    let b: Value = match serde_json::from_slice(&raw) {
        Ok(v @ Value::Object(_)) => v,
        _ => return js(400, json!({"ok": false, "info": "JSON inválido"})),
    };
    let s = |k: &str| b.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    match path.as_str() {
        "/api/pulse" => { shared::pulse(); js(200, json!({"ok": true})) }
        "/api/consultor" => { let (ok, info) = blocking(move || loops::consult(&b)).await; okinfo(ok, info, 409) }
        "/api/send" => { let (to, text) = (s("to"), s("text")); let (ok, info) = blocking(move || control::send(&to, &text)).await; okinfo(ok, info, 502) }
        "/api/avance/voto" => {
            let r = blocking(move || {
                let s = |k: &str| b.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                avance::vote(&s("repo"), &s("key"), b.get("agree").cloned().unwrap_or(Value::Null), &s("note"))
            }).await;
            shared::invalidate();
            okinfo(r.0, r.1, 409)
        }
        "/api/control" => {
            let (id, action) = (s("id"), s("action"));
            let (ok, info) = blocking(move || if action == "sweep" { control::limpiar() } else { control::control(&id, &action) }).await;
            shared::invalidate();
            okinfo(ok, info, 409)
        }
        "/api/stop" => { let id = s("id"); let (ok, info) = blocking(move || control::stop(&id)).await; shared::invalidate(); okinfo(ok, info, 409) }
        "/api/launch" => { let (c, v) = blocking(move || control::launch(&b)).await; shared::invalidate(); js(c, v) }
        "/api/codex/new" => { let (c, v) = blocking(move || control::codex_new(&b)).await; js(c, v) }
        "/api/codex/open" => { let (c, v) = blocking(move || control::codex_open(&b)).await; js(c, v) }
        "/api/codex/clear" => { let (c, v) = blocking(move || control::codex_clear(&b)).await; shared::invalidate(); js(c, v) }
        "/api/bus/send" => { let (c, v) = blocking(move || control::bus_send(&b)).await; js(c, v) }
        "/api/bus/reply" => { let (c, v) = blocking(move || control::bus_reply(&b)).await; js(c, v) }
        _ => js(404, json!({"ok": false})),
    }
}

async fn events() -> impl IntoResponse {
    let rx = tick().subscribe();
    let first = {
        let cur = rx.borrow();
        if cur.0 > 0 { Some(cur.1.clone()) } else { None }
    };
    let stream = futures_util::stream::unfold((rx, first, true), |(mut rx, first, is_first)| async move {
        if is_first {
            let seen_body = match first { Some(b) => b, None => blocking(snapshot::state).await };
            let ev = Event::default().event("state").data(String::from_utf8_lossy(&seen_body).into_owned());
            return Some((Ok::<_, Infallible>(ev), (rx, None, false)));
        }
        let seen = rx.borrow().0;
        if rx.changed().await.is_err() {
            return None;
        }
        let (v, body, part) = rx.borrow_and_update().clone();
        let data = if seen > 0 && v == seen + 1 { part } else { body };
        Some((Ok(Event::default().event("state").data(String::from_utf8_lossy(&data).into_owned())), (rx, None, false)))
    });
    let mut r = Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("keepalive")).into_response();
    r.headers_mut().insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    r
}

/// One 1 s tick while any /api/events client is connected: publish only when a top-level key (besides `t`) changed.
fn ticker() {
    let mut keys: HashMap<String, String> = HashMap::new();
    loop {
        std::thread::sleep(Duration::from_secs(1));
        if tick().receiver_count() == 0 {
            continue;
        }
        let snap = snapshot::snapshot();
        let Value::Object(map) = &snap else { continue };
        let now: HashMap<String, String> =
            map.iter().filter(|(k, _)| k.as_str() != "t").map(|(k, v)| (k.clone(), v.to_string())).collect();
        let changed: Vec<&String> = now.keys().filter(|k| keys.get(*k) != now.get(*k)).collect();
        if changed.is_empty() {
            continue;
        }
        let body = serde_json::to_vec(&snap).unwrap_or_default();
        let mut part = serde_json::Map::new();
        part.insert("t".into(), map.get("t").cloned().unwrap_or(Value::Null));
        for k in &changed {
            part.insert((*k).clone(), map[k.as_str()].clone());
        }
        let part = serde_json::to_vec(&Value::Object(part)).unwrap_or_default();
        snapshot::set_cache(body.clone());
        keys = now;
        let v = tick().borrow().0 + 1;
        tick().send_replace((v, Arc::new(body), Arc::new(part)));
    }
}

async fn fallback_handler(req: Request) -> Response {
    if req.method() != axum::http::Method::GET && req.method() != axum::http::Method::HEAD {
        return js(404, json!({"ok": false, "info": "no existe"}));
    }
    static_file(req).await
}

async fn static_file(req: Request) -> Response {
    let rel = req.uri().path().trim_start_matches('/').to_string();
    if rel.split('/').any(|c| c == ".." || c.starts_with('.')) {
        return js(404, json!({"ok": false}));
    }
    let web = shared::web_dir();
    let mut p = web.join(&rel);
    if p.is_dir() {
        p = p.join("index.html");
    }
    let ct = match p.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        "css" => "text/css",
        "woff2" => "font/woff2",
        "svg" => "image/svg+xml",
        "jpg" => "image/jpeg",
        "png" => "image/png",
        "json" => "application/json",
        _ => "application/octet-stream",
    };
    let cache = if rel.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-store" };
    match tokio::fs::read(&p).await {
        Ok(b) => bytes(200, ct, b, cache),
        Err(_) => js(404, json!({"ok": false})),
    }
}
