use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::procs::{ProcSnapshot, agy_lock_conv_ids, agy_pids, discover_procs, proc_env};
use crate::types::{CoreError, Result, home_dir, now_unix_secs, resolve_agencia_root_with};

const KEEP_ENV_KEYS: [&str; 5] = [
    "ANTIGRAVITY_LS_ADDRESS",
    "ANTIGRAVITY_CSRF_TOKEN",
    "ANTIGRAVITY_APP_DATA_DIR",
    "ANTIGRAVITY_LS_VERSION",
    "ANTIGRAVITY_AGENTAPI_EXE",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgyInstance {
    pub pid: i32,
    pub conv_id: String,
    pub ls_address: String,
    pub csrf_token: String,
    pub app_data_dir: String,
    pub ls_version: String,
    pub agentapi_exe: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedAgyFile {
    pid: i32,
    t: f64,
    env: BTreeMap<String, String>,
}

/// Honor `AGENCIA_DIR` first, falling back to `AGENCIA_ROOT` / `agencia_root()`.
pub fn resolve_cache_dir_with(
    agencia_dir: Option<OsString>,
    agencia_root: Option<OsString>,
) -> PathBuf {
    resolve_agencia_root_with(agencia_dir, agencia_root)
        .join("cache")
        .join("agy")
}

/// Honor `AGENCIA_DIR` first, falling back to `AGENCIA_ROOT` / `agencia_root()`.
pub fn default_cache_dir() -> PathBuf {
    resolve_cache_dir_with(
        std::env::var_os("AGENCIA_DIR"),
        std::env::var_os("AGENCIA_ROOT"),
    )
}

fn ensure_cache_dir(cache_dir: &Path) -> Result<()> {
    if !cache_dir.exists() {
        fs::create_dir_all(cache_dir)?;
        let _ = fs::set_permissions(cache_dir, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn instance_from_env(pid: i32, conv_id: String, env: &BTreeMap<String, String>) -> Option<AgyInstance> {
    let ls_address = env.get("ANTIGRAVITY_LS_ADDRESS")?.trim().to_string();
    let csrf_token = env.get("ANTIGRAVITY_CSRF_TOKEN")?.trim().to_string();
    if ls_address.is_empty() || csrf_token.is_empty() {
        return None;
    }
    Some(AgyInstance {
        pid,
        conv_id,
        ls_address,
        csrf_token,
        app_data_dir: env
            .get("ANTIGRAVITY_APP_DATA_DIR")
            .cloned()
            .unwrap_or_default(),
        ls_version: env
            .get("ANTIGRAVITY_LS_VERSION")
            .cloned()
            .unwrap_or_default(),
        agentapi_exe: env
            .get("ANTIGRAVITY_AGENTAPI_EXE")
            .cloned()
            .unwrap_or_default(),
    })
}

fn read_cached_env(cache_dir: &Path, pid: i32) -> Option<BTreeMap<String, String>> {
    let path = cache_dir.join(format!("{pid}.json"));
    let raw = fs::read_to_string(path).ok()?;
    let parsed: CachedAgyFile = serde_json::from_str(&raw).ok()?;
    let ls = parsed.env.get("ANTIGRAVITY_LS_ADDRESS").map(String::as_str).unwrap_or("");
    let csrf = parsed.env.get("ANTIGRAVITY_CSRF_TOKEN").map(String::as_str).unwrap_or("");
    if ls.is_empty() || csrf.is_empty() {
        return None;
    }
    Some(parsed.env)
}

fn write_cached_env_atomic(cache_dir: &Path, pid: i32, env: &BTreeMap<String, String>) -> Result<()> {
    ensure_cache_dir(cache_dir)?;
    let tmp_path = cache_dir.join(format!("{pid}.json.tmp"));
    let final_path = cache_dir.join(format!("{pid}.json"));

    let payload = CachedAgyFile {
        pid,
        t: now_unix_secs(),
        env: env.clone(),
    };
    let bytes = serde_json::to_vec(&payload)?;

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp_path)?;
    file.write_all(&bytes)?;
    file.flush()?;
    drop(file);
    let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));
    fs::rename(&tmp_path, &final_path)?;
    Ok(())
}

fn clean_stale_cache(cache_dir: &Path, live_pids: &HashSet<i32>) {
    let Ok(entries) = fs::read_dir(cache_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Some(stem) = name.strip_suffix(".json")
            && let Ok(pid) = stem.parse::<i32>()
            && !live_pids.contains(&pid)
        {
            let _ = fs::remove_file(&path);
        }
    }
}

fn find_descendant_env(snap: &ProcSnapshot, root_pid: i32) -> Option<BTreeMap<String, String>> {
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    visited.insert(root_pid);

    if let Some(kids) = snap.children.get(&root_pid) {
        for &k in kids {
            if visited.insert(k) {
                queue.push_back(k);
            }
        }
    }

    let mut inspected = 0usize;
    while let Some(desc_pid) = queue.pop_front() {
        if inspected >= 200 {
            break;
        }
        inspected += 1;

        let vars = proc_env(desc_pid);
        if !vars.is_empty() {
            let mut env_map = BTreeMap::new();
            for (k, v) in vars {
                if KEEP_ENV_KEYS.contains(&k.as_str()) {
                    env_map.insert(k, v);
                }
            }
            let has_ls = env_map
                .get("ANTIGRAVITY_LS_ADDRESS")
                .is_some_and(|v| !v.is_empty());
            let has_csrf = env_map
                .get("ANTIGRAVITY_CSRF_TOKEN")
                .is_some_and(|v| !v.is_empty());
            if has_ls && has_csrf {
                return Some(env_map);
            }
        }

        if let Some(kids) = snap.children.get(&desc_pid) {
            for &k in kids {
                if visited.insert(k) {
                    queue.push_back(k);
                }
            }
        }
    }
    None
}

pub fn harvest_agy_instances_in(cache_dir: &Path) -> Vec<AgyInstance> {
    let _ = ensure_cache_dir(cache_dir);
    let Ok(snap) = discover_procs() else {
        return Vec::new();
    };
    let pids = agy_pids(&snap);
    let live_set: HashSet<i32> = pids.keys().copied().collect();
    clean_stale_cache(cache_dir, &live_set);

    let mut out = Vec::new();
    let mut sorted_pids: Vec<(i32, String)> = pids.into_iter().collect();
    sorted_pids.sort_by_key(|(pid, _)| *pid);

    for (pid, mut conv_id) in sorted_pids {
        if conv_id.is_empty()
            && let Some(first) = agy_lock_conv_ids(pid).into_iter().next()
        {
            conv_id = first;
        }

        let env = if let Some(cached) = read_cached_env(cache_dir, pid) {
            Some(cached)
        } else if let Some(found) = find_descendant_env(&snap, pid) {
            let _ = write_cached_env_atomic(cache_dir, pid, &found);
            Some(found)
        } else {
            None
        };

        if let Some(env_map) = env
            && let Some(inst) = instance_from_env(pid, conv_id, &env_map)
        {
            out.push(inst);
        }
    }
    out
}

pub fn harvest_agy_instances() -> Vec<AgyInstance> {
    let dir = default_cache_dir();
    harvest_agy_instances_in(&dir)
}

fn should_invalidate_cache(err_text: &str, status: Option<u16>, is_conn_failed: bool) -> bool {
    if is_conn_failed || matches!(status, Some(401 | 403)) {
        return true;
    }
    let lower = err_text.to_ascii_lowercase();
    lower.contains("csrf") || lower.contains("connection refused")
}

fn invalidate_cache_entry(cache_dir: &Path, pid: i32) {
    if pid > 0 {
        let _ = fs::remove_file(cache_dir.join(format!("{pid}.json")));
    }
}

pub fn send_agy_message_in(
    inst: &AgyInstance,
    text: &str,
    sender: Option<&str>,
    cache_dir: &Path,
) -> Result<()> {
    if inst.ls_address.trim().is_empty() || inst.csrf_token.trim().is_empty() {
        return Err(CoreError::InvalidInput(
            "missing Antigravity LS address or CSRF token".to_string(),
        ));
    }
    if inst.conv_id.trim().is_empty() {
        return Err(CoreError::InvalidInput(
            "missing Antigravity conversation/cascade id".to_string(),
        ));
    }

    let sender_label = sender
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("agencia");
    let formatted_text = format!("[De {sender_label}] {text}");

    let base = if inst.ls_address.starts_with("http://") || inst.ls_address.starts_with("https://") {
        inst.ls_address.trim_end_matches('/').to_string()
    } else {
        format!("http://{}", inst.ls_address.trim_end_matches('/'))
    };
    let url = format!(
        "{base}/exa.language_server_pb.LanguageServerService/SendUserCascadeMessage"
    );

    let mut metadata = serde_json::Map::new();
    metadata.insert("ideName".to_string(), json!("antigravity"));
    if !inst.ls_version.trim().is_empty() {
        metadata.insert("ideVersion".to_string(), json!(inst.ls_version));
    }

    let payload = json!({
        "cascadeId": inst.conv_id,
        "items": [
            {
                "text": formatted_text
            }
        ],
        "metadata": metadata,
    });

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();

    let resp = agent
        .post(&url)
        .set("Content-Type", "application/json")
        .set("Connect-Protocol-Version", "1")
        .set("x-codeium-csrf-token", &inst.csrf_token)
        .send_json(payload);

    match resp {
        Ok(r) => {
            let body = r.into_string().unwrap_or_default();
            if !body.trim().is_empty()
                && let Ok(val) = serde_json::from_str::<serde_json::Value>(&body)
                && let Some(err_val) = val.get("error").filter(|v| !v.is_null())
            {
                let msg = err_val
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| err_val.to_string());
                if should_invalidate_cache(&msg, None, false) {
                    invalidate_cache_entry(cache_dir, inst.pid);
                }
                return Err(CoreError::Http(msg.chars().take(200).collect()));
            }
            Ok(())
        }
        Err(ureq::Error::Status(status, r)) => {
            let body = r.into_string().unwrap_or_default();
            if should_invalidate_cache(&body, Some(status), false) {
                invalidate_cache_entry(cache_dir, inst.pid);
            }
            let summary: String = if body.trim().is_empty() {
                format!("HTTP {status}")
            } else {
                format!("HTTP {status}: {}", body.trim())
            }
            .chars()
            .take(200)
            .collect();
            Err(CoreError::Http(summary))
        }
        Err(ureq::Error::Transport(t)) => {
            let msg = t.to_string();
            let conn_failed = t.kind() == ureq::ErrorKind::ConnectionFailed;
            if should_invalidate_cache(&msg, None, conn_failed) {
                invalidate_cache_entry(cache_dir, inst.pid);
            }
            Err(CoreError::Http(msg.chars().take(200).collect()))
        }
    }
}

pub fn send_agy_message(inst: &AgyInstance, text: &str, sender: Option<&str>) -> Result<()> {
    let dir = default_cache_dir();
    send_agy_message_in(inst, text, sender, &dir)
}

fn load_conversation_titles() -> HashMap<String, (String, String)> {
    let db_path = home_dir()
        .join(".gemini")
        .join("antigravity-cli")
        .join("conversation_summaries.db");
    let mut out = HashMap::new();
    let Ok(conn) = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return out;
    };
    let _ = conn.busy_timeout(Duration::from_secs(2));
    let Ok(mut stmt) = conn.prepare(
        "SELECT conversation_id, COALESCE(title, ''), COALESCE(workspace_uris, '[]') FROM conversation_summaries",
    ) else {
        return out;
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    }) else {
        return out;
    };
    for row in rows.flatten() {
        out.insert(row.0, (row.1, row.2));
    }
    out
}

fn repo_from_workspace_uris(ws_json: &str) -> String {
    let Ok(arr) = serde_json::from_str::<Vec<String>>(ws_json) else {
        return String::new();
    };
    let Some(first) = arr.first() else {
        return String::new();
    };
    let cleaned = first.strip_prefix("file://").unwrap_or(first);
    Path::new(cleaned)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

fn claude_sender_name() -> String {
    let sid = std::env::var("CLAUDE_CODE_SESSION_ID").unwrap_or_default();
    if sid.is_empty() {
        return "una sesión de Claude".to_string();
    }
    let sessions_dir = home_dir().join(".claude").join("sessions");
    if let Ok(entries) = fs::read_dir(sessions_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(raw) = fs::read_to_string(&path)
                && let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw)
                && val.get("sessionId").and_then(|v| v.as_str()) == Some(sid.as_str())
            {
                let name = val
                    .get("name")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| &sid[..sid.len().min(8)]);
                return format!("la sesión de Claude {name}");
            }
        }
    }
    "una sesión de Claude".to_string()
}

pub fn dispatch_cli(argv: &[String]) -> i32 {
    let cache_dir = default_cache_dir();
    let harvested = harvest_agy_instances_in(&cache_dir);
    let inst_by_pid: HashMap<i32, &AgyInstance> =
        harvested.iter().map(|inst| (inst.pid, inst)).collect();

    let Ok(snap) = discover_procs() else {
        eprintln!("agy-send: no se pudo descubrir procesos");
        return 1;
    };
    let pids = agy_pids(&snap);
    let titles = load_conversation_titles();

    let mut wins: Vec<(String, i32, String, String)> = pids
        .into_iter()
        .filter(|(_, conv)| !conv.is_empty())
        .map(|(pid, conv)| {
            let (title, ws) = titles
                .get(&conv)
                .cloned()
                .unwrap_or_else(|| (String::new(), "[]".to_string()));
            (conv, pid, title, ws)
        })
        .collect();
    wins.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    if argv.len() < 3 {
        for (conv, pid, title, ws) in &wins {
            let repo = repo_from_workspace_uris(ws);
            let display_title = if title.is_empty() {
                "(sin título)"
            } else {
                title.as_str()
            };
            let status = if inst_by_pid.contains_key(pid) || read_cached_env(&cache_dir, *pid).is_some()
            {
                "enlazada"
            } else {
                "sin enlace: aún no corre un comando"
            };
            println!("{conv}  {repo:<14} {display_title}  [{status}]");
        }
        return if wins.is_empty() { 1 } else { 0 };
    }

    let query = argv[1].to_lowercase();
    let hits: Vec<&(String, i32, String, String)> = wins
        .iter()
        .filter(|(conv, _, title, _)| {
            conv.to_lowercase().starts_with(&query) || title.to_lowercase().contains(&query)
        })
        .collect();

    if hits.len() != 1 {
        eprintln!(
            "agy-send: {} ventanas coinciden con '{}'; corre agy-send sin argumentos para verlas",
            hits.len(),
            argv[1]
        );
        return 1;
    }

    let (_conv, pid, _, _) = hits[0];
    let Some(inst) = inst_by_pid.get(pid).copied() else {
        println!("aún no tengo su enlace: llega la primera vez que esa ventana corre un comando");
        return 1;
    };

    let body = argv[2..].join(" ");
    let sender = claude_sender_name();
    match send_agy_message_in(inst, &body, Some(&sender), &cache_dir) {
        Ok(()) => {
            println!("enviado a Antigravity");
            0
        }
        Err(e) => {
            println!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write as IoWrite};
    use std::net::TcpListener;
    use std::thread;

    use crate::types::uuid_v4;

    struct TempDirGuard(PathBuf);
    impl TempDirGuard {
        fn new(label: &str) -> Self {
            let base = std::env::temp_dir().join(format!("agencia-{label}-{}", uuid_v4()));
            fs::create_dir_all(&base).expect("create temp test dir");
            Self(base)
        }
    }
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> (String, String) {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        let mut header_end = None;
        let mut content_len = 0usize;

        loop {
            let n = stream.read(&mut tmp).expect("read http req");
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if header_end.is_none()
                && let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n")
            {
                header_end = Some(pos + 4);
                let hdr_str = String::from_utf8_lossy(&buf[..pos + 4]);
                for line in hdr_str.lines() {
                    if let Some((k, v)) = line.split_once(':')
                        && k.trim().eq_ignore_ascii_case("content-length")
                    {
                        content_len = v.trim().parse::<usize>().unwrap_or(0);
                    }
                }
            }
            if let Some(hend) = header_end
                && buf.len() >= hend + content_len
            {
                break;
            }
        }
        let hend = header_end.unwrap_or(buf.len());
        let headers = String::from_utf8_lossy(&buf[..hend]).into_owned();
        let body = String::from_utf8_lossy(&buf[hend..(hend + content_len).min(buf.len())]).into_owned();
        (headers, body)
    }

    #[test]
    fn test_default_cache_dir_honors_agencia_dir_then_agencia_root() {
        assert_eq!(
            resolve_cache_dir_with(
                Some(OsString::from("/tmp/agencia_dir_wins")),
                Some(OsString::from("/tmp/agencia_root_only")),
            ),
            PathBuf::from("/tmp/agencia_dir_wins/cache/agy")
        );
        assert_eq!(
            resolve_cache_dir_with(None, Some(OsString::from("/tmp/agencia_root_only"))),
            PathBuf::from("/tmp/agencia_root_only/cache/agy")
        );
        assert_eq!(
            resolve_cache_dir_with(
                Some(OsString::from("")),
                Some(OsString::from("/tmp/agencia_root_only")),
            ),
            PathBuf::from("/tmp/agencia_root_only/cache/agy")
        );
        let fallback = resolve_cache_dir_with(None, None);
        assert!(fallback.ends_with(Path::new("cache/agy")));
    }

    #[test]
    fn test_cache_write_0600_and_stale_cleanup() {
        let dir = TempDirGuard::new("agylink-cache");
        let cache_dir = dir.0.join("cache/agy");
        ensure_cache_dir(&cache_dir).expect("create cache dir");

        let mut env = BTreeMap::new();
        env.insert("ANTIGRAVITY_LS_ADDRESS".to_string(), "127.0.0.1:12345".to_string());
        env.insert("ANTIGRAVITY_CSRF_TOKEN".to_string(), "tok-1".to_string());
        write_cached_env_atomic(&cache_dir, 999_999_991, &env).expect("write atomic");

        let p = cache_dir.join("999999991.json");
        let mode = fs::metadata(&p).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert!(read_cached_env(&cache_dir, 999_999_991).is_some());

        // Stale PID cleanup should remove nonexistent pid json
        let mut live = HashSet::new();
        live.insert(1234);
        clean_stale_cache(&cache_dir, &live);
        assert!(!p.exists());
    }

    #[test]
    fn test_send_agy_message_http_post_schema_and_headers() {
        let dir = TempDirGuard::new("agylink-http-ok");
        let cache_dir = dir.0.join("cache/agy");
        ensure_cache_dir(&cache_dir).expect("create cache dir");

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind tcp");
        let addr = listener.local_addr().expect("local addr");

        let srv = thread::spawn(move || {
            let (mut conn, _) = listener.accept().expect("accept");
            let (headers, body) = read_http_request(&mut conn);
            let resp_body = r#"{"cascadeId":"conv-abc","executionStatus":"EXECUTION_STATUS_IDLE"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                resp_body.len(),
                resp_body
            );
            conn.write_all(resp.as_bytes()).expect("write resp");
            (headers, body)
        });

        let inst = AgyInstance {
            pid: 54321,
            conv_id: "conv-abc".to_string(),
            ls_address: addr.to_string(),
            csrf_token: "csrf-secret-xyz".to_string(),
            app_data_dir: "/tmp/app".to_string(),
            ls_version: "1.2.3".to_string(),
            agentapi_exe: "/usr/local/bin/agy".to_string(),
        };

        send_agy_message_in(&inst, "hola agy", Some("claude:test"), &cache_dir)
            .expect("send_agy_message_in should succeed");

        let (headers, body) = srv.join().expect("server");
        let first_line = headers.lines().next().unwrap_or("");
        assert!(
            first_line.starts_with(
                "POST /exa.language_server_pb.LanguageServerService/SendUserCascadeMessage HTTP/1."
            ),
            "unexpected request line: {first_line}"
        );
        let lower_headers = headers.to_ascii_lowercase();
        assert!(
            lower_headers.contains("x-codeium-csrf-token: csrf-secret-xyz"),
            "missing csrf header in:\n{headers}"
        );

        let json_body: serde_json::Value = serde_json::from_str(&body).expect("valid json body");
        assert_eq!(json_body["cascadeId"], "conv-abc");
        assert_eq!(json_body["items"][0]["text"], "[De claude:test] hola agy");
        assert_eq!(json_body["metadata"]["ideName"], "antigravity");
        assert_eq!(json_body["metadata"]["ideVersion"], "1.2.3");
    }

    #[test]
    fn test_send_agy_message_invalidates_cache_on_csrf_and_connection_refused() {
        let dir = TempDirGuard::new("agylink-invalidate");
        let cache_dir = dir.0.join("cache/agy");
        ensure_cache_dir(&cache_dir).expect("create cache dir");

        let mut env = BTreeMap::new();
        env.insert("ANTIGRAVITY_LS_ADDRESS".to_string(), "127.0.0.1:1".to_string());
        env.insert("ANTIGRAVITY_CSRF_TOKEN".to_string(), "bad-tok".to_string());

        // Case 1: 403 / CSRF error removes cache/<pid>.json
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind tcp");
        let addr = listener.local_addr().expect("local addr");
        write_cached_env_atomic(&cache_dir, 77701, &env).expect("seed cache 1");
        let cache_file_1 = cache_dir.join("77701.json");
        assert!(cache_file_1.exists());

        let srv = thread::spawn(move || {
            let (mut conn, _) = listener.accept().expect("accept");
            let _ = read_http_request(&mut conn);
            let err_body = r#"{"code":"unauthenticated","message":"invalid csrf token"}"#;
            let resp = format!(
                "HTTP/1.1 403 Forbidden\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                err_body.len(),
                err_body
            );
            conn.write_all(resp.as_bytes()).expect("write 403");
        });

        let inst_csrf = AgyInstance {
            pid: 77701,
            conv_id: "conv-1".to_string(),
            ls_address: addr.to_string(),
            csrf_token: "bad-tok".to_string(),
            app_data_dir: String::new(),
            ls_version: String::new(),
            agentapi_exe: String::new(),
        };
        let err = send_agy_message_in(&inst_csrf, "test", None, &cache_dir)
            .expect_err("403 csrf should fail");
        assert!(matches!(err, CoreError::Http(_)));
        srv.join().expect("join");
        assert!(
            !cache_file_1.exists(),
            "cache file must be invalidated on CSRF/403 error"
        );

        // Case 2: Connection refused removes cache/<pid>.json
        let closed_listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let closed_addr = closed_listener.local_addr().expect("addr");
        drop(closed_listener);

        write_cached_env_atomic(&cache_dir, 77702, &env).expect("seed cache 2");
        let cache_file_2 = cache_dir.join("77702.json");
        assert!(cache_file_2.exists());

        let inst_refused = AgyInstance {
            pid: 77702,
            conv_id: "conv-2".to_string(),
            ls_address: closed_addr.to_string(),
            csrf_token: "tok".to_string(),
            app_data_dir: String::new(),
            ls_version: String::new(),
            agentapi_exe: String::new(),
        };
        let err2 = send_agy_message_in(&inst_refused, "test", None, &cache_dir)
            .expect_err("connection refused should fail");
        assert!(matches!(err2, CoreError::Http(_)));
        assert!(
            !cache_file_2.exists(),
            "cache file must be invalidated on connection refused"
        );
    }

    #[test]
    fn test_harvest_agy_instances_smoke_isolated_dir() {
        let dir = TempDirGuard::new("agylink-smoke");
        let cache_dir = dir.0.join("cache/agy");
        let instances = harvest_agy_instances_in(&cache_dir);
        for inst in &instances {
            assert!(inst.pid > 0);
            assert!(!inst.ls_address.is_empty());
            assert!(!inst.csrf_token.is_empty());
            let cached = cache_dir.join(format!("{}.json", inst.pid));
            assert!(cached.exists());
            let mode = fs::metadata(&cached).expect("stat").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn test_send_agy_message_rejects_missing_fields_and_parses_repo_uri() {
        let dir = TempDirGuard::new("agylink-validate");
        let cache_dir = dir.0.join("cache/agy");

        let missing_conv = AgyInstance {
            pid: 1,
            conv_id: String::new(),
            ls_address: "127.0.0.1:9999".to_string(),
            csrf_token: "tok".to_string(),
            app_data_dir: String::new(),
            ls_version: String::new(),
            agentapi_exe: String::new(),
        };
        assert!(matches!(
            send_agy_message_in(&missing_conv, "hi", None, &cache_dir),
            Err(CoreError::InvalidInput(_))
        ));

        let missing_tok = AgyInstance {
            pid: 1,
            conv_id: "conv".to_string(),
            ls_address: "127.0.0.1:9999".to_string(),
            csrf_token: String::new(),
            app_data_dir: String::new(),
            ls_version: String::new(),
            agentapi_exe: String::new(),
        };
        assert!(matches!(
            send_agy_message_in(&missing_tok, "hi", None, &cache_dir),
            Err(CoreError::InvalidInput(_))
        ));

        assert_eq!(
            repo_from_workspace_uris(r#"["file:///Users/dev/agencia"]"#),
            "agencia"
        );
        assert_eq!(repo_from_workspace_uris("[]"), "");
    }
}
