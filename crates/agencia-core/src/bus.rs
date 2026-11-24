use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agylink::{AgyInstance, harvest_agy_instances, send_agy_message};
use crate::cartero::send_claude_uds;
use crate::procs::{agy_pids, discover_procs, pid_alive};
use crate::types::{CoreError, Result, agencia_root, home_dir, now_unix_secs, uuid_v4};

pub const DEFAULT_MAX_HOPS: i64 = 6;
pub const STALE_PEER_SECS: f64 = 600.0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerInfo {
    pub peer_id: String,
    pub provider: String,
    pub pid: i32,
    pub name: String,
    pub repo: String,
    pub branch: String,
    pub cwd: String,
    pub transport: String,
    pub endpoint: String,
    pub title: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub push_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThreadRecord {
    pub thread_id: String,
    pub topic: String,
    pub created_by: String,
    pub created_at: f64,
    pub hops_left: i64,
    pub max_hops: i64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageRecord {
    pub msg_id: String,
    pub thread_id: String,
    pub seq: i64,
    pub from_peer: String,
    pub to_peer: String,
    pub body: String,
    pub expect_reply: i64,
    pub delivered_via: String,
    pub delivered_ok: i64,
    pub delivered_info: String,
    pub created_at: f64,
    pub read_at: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerCandidate {
    pub peer_id: String,
    pub provider: String,
    pub repo: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SendResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates: Option<Vec<PeerCandidate>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered_via: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered_info: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hops_left: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InboxResult {
    pub peer: String,
    pub count: usize,
    pub messages: Vec<MessageRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThreadDetail {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<ThreadRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<MessageRecord>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaitResult {
    pub ok: bool,
    pub timed_out: bool,
    pub message: Option<MessageRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloseResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Bus {
    db_path: PathBuf,
}

fn io_err<E: std::fmt::Display>(err: E) -> CoreError {
    io::Error::other(err.to_string()).into()
}

fn now_f64() -> f64 {
    now_unix_secs()
}

fn short_id(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

fn hex_id(len: usize) -> String {
    uuid_v4().replace('-', "").chars().take(len).collect()
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

fn parent_pid() -> i32 {
    #[cfg(unix)]
    {
        std::os::unix::process::parent_id() as i32
    }
    #[cfg(not(unix))]
    {
        0
    }
}

fn ensure_dir_0700(dir: &Path) -> Result<()> {
    if dir.as_os_str().is_empty() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        if !dir.exists() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

fn chmod_0600(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
}

/// Resolve `bus.db` path honoring `SESIONES_BUS_DB` -> `AGENCIA_DIR` -> `AGENCIA_ROOT` -> `agencia_root()`.
pub fn resolve_db_path(
    sesiones_bus_db: Option<&str>,
    agencia_dir: Option<&str>,
    agencia_root_env: Option<&str>,
) -> PathBuf {
    if let Some(p) = sesiones_bus_db.map(str::trim).filter(|s| !s.is_empty()) {
        return PathBuf::from(p);
    }
    if let Some(dir) = agencia_dir.map(str::trim).filter(|s| !s.is_empty()) {
        return PathBuf::from(dir).join("cache").join("bus.db");
    }
    if let Some(root) = agencia_root_env.map(str::trim).filter(|s| !s.is_empty()) {
        return PathBuf::from(root).join("cache").join("bus.db");
    }
    agencia_root().join("cache").join("bus.db")
}

/// Resolve the default `bus.db` path honoring:
/// 1. `SESIONES_BUS_DB`
/// 2. `AGENCIA_DIR` -> `<AGENCIA_DIR>/cache/bus.db`
/// 3. `AGENCIA_ROOT` -> `<AGENCIA_ROOT>/cache/bus.db`
/// 4. `crate::types::agencia_root()` -> `<root>/cache/bus.db`
pub fn default_db_path() -> PathBuf {
    resolve_db_path(
        std::env::var("SESIONES_BUS_DB").ok().as_deref(),
        std::env::var("AGENCIA_DIR").ok().as_deref(),
        std::env::var("AGENCIA_ROOT").ok().as_deref(),
    )
}

/// Detect `(repo_name, git_branch)` by reading `.git/HEAD` (including `.git` file worktrees)
/// without spawning `git` subprocesses.
pub fn git_info(cwd: &Path) -> (String, String) {
    let raw = cwd.to_string_lossy();
    let trimmed = raw.trim_end_matches('/');
    if trimmed.is_empty() || !cwd.is_dir() {
        return (String::new(), String::new());
    }
    let repo = Path::new(trimmed)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut git_dir: Option<PathBuf> = None;
    for ancestor in cwd.ancestors() {
        let dot_git = ancestor.join(".git");
        if dot_git.is_dir() {
            git_dir = Some(dot_git);
            break;
        }
        if dot_git.is_file() {
            if let Ok(content) = fs::read_to_string(&dot_git) {
                for line in content.lines() {
                    let line = line.trim();
                    if let Some(rest) = line.strip_prefix("gitdir:") {
                        let target = PathBuf::from(rest.trim());
                        let resolved = if target.is_absolute() {
                            target
                        } else {
                            ancestor.join(target)
                        };
                        git_dir = Some(resolved);
                        break;
                    }
                }
            }
            break;
        }
    }

    let branch = match git_dir {
        Some(gdir) => match fs::read_to_string(gdir.join("HEAD")) {
            Ok(head) => {
                let h = head.trim();
                if let Some(ref_name) = h.strip_prefix("ref:") {
                    let ref_name = ref_name.trim();
                    ref_name
                        .strip_prefix("refs/heads/")
                        .unwrap_or(ref_name)
                        .to_string()
                } else if !h.is_empty() {
                    "HEAD".to_string()
                } else {
                    String::new()
                }
            }
            Err(_) => String::new(),
        },
        None => String::new(),
    };

    (repo, branch)
}

#[derive(Debug, Clone)]
struct ClaudeSessionFile {
    pid: i32,
    session_id: String,
    cwd: String,
    messaging_socket_path: String,
    name: String,
    status: String,
}

fn read_claude_sessions_in(dir: &Path) -> Vec<ClaudeSessionFile> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let val = match serde_json::Deserializer::from_str(&content)
            .into_iter::<Value>()
            .next()
        {
            Some(Ok(v)) => v,
            _ => continue,
        };
        let pid = val.get("pid").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let session_id = val
            .get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let cwd = val
            .get("cwd")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let messaging_socket_path = val
            .get("messagingSocketPath")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let name = val
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let status = val
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        out.push(ClaudeSessionFile {
            pid,
            session_id,
            cwd,
            messaging_socket_path,
            name,
            status,
        });
    }
    out
}

fn claude_sessions_dir() -> PathBuf {
    home_dir().join(".claude").join("sessions")
}

fn agy_summaries_db_path() -> PathBuf {
    home_dir()
        .join(".gemini")
        .join("antigravity-cli")
        .join("conversation_summaries.db")
}

fn read_agy_summaries_at(db_file: &Path) -> HashMap<String, (String, String)> {
    let mut map = HashMap::new();
    if !db_file.exists() {
        return map;
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = match Connection::open_with_flags(db_file, flags) {
        Ok(c) => c,
        Err(_) => return map,
    };
    let _ = conn.busy_timeout(Duration::from_millis(2000));
    let mut stmt = match conn.prepare(
        "SELECT conversation_id, title, workspace_uris FROM conversation_summaries",
    ) {
        Ok(s) => s,
        Err(_) => return map,
    };
    let rows = stmt.query_map([], |row| {
        let cid: String = row.get(0)?;
        let title: Option<String> = row.get(1)?;
        let ws: Option<String> = row.get(2)?;
        Ok((cid, title.unwrap_or_default(), ws.unwrap_or_else(|| "[]".to_string())))
    });
    if let Ok(iter) = rows {
        for r in iter.flatten() {
            map.insert(r.0, (r.1, r.2));
        }
    }
    map
}

fn extract_first_workspace(ws_json: &str) -> String {
    if ws_json.trim().is_empty() {
        return String::new();
    }
    if let Ok(arr) = serde_json::from_str::<Vec<String>>(ws_json)
        && let Some(first) = arr.first()
    {
        return first.strip_prefix("file://").unwrap_or(first).to_string();
    }
    String::new()
}

fn map_message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRecord> {
    Ok(MessageRecord {
        msg_id: row.get(0)?,
        thread_id: row.get(1)?,
        seq: row.get(2)?,
        from_peer: row.get(3)?,
        to_peer: row.get(4)?,
        body: row.get(5)?,
        expect_reply: row.get(6)?,
        delivered_via: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        delivered_ok: row.get(8)?,
        delivered_info: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
        created_at: row.get(10)?,
        read_at: row.get(11)?,
    })
}

fn map_thread_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ThreadRecord> {
    Ok(ThreadRecord {
        thread_id: row.get(0)?,
        topic: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        created_by: row.get(2)?,
        created_at: row.get(3)?,
        hops_left: row.get(4)?,
        max_hops: row.get(5)?,
        status: row.get(6)?,
    })
}

impl Bus {
    pub fn open(db_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            ensure_dir_0700(parent)?;
        }
        let conn = Connection::open(&db_path).map_err(io_err)?;
        conn.busy_timeout(Duration::from_millis(5000))
            .map_err(io_err)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            PRAGMA busy_timeout=5000;
            CREATE TABLE IF NOT EXISTS peers (
                peer_id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                pid INTEGER DEFAULT 0,
                name TEXT DEFAULT '',
                repo TEXT DEFAULT '',
                branch TEXT DEFAULT '',
                cwd TEXT DEFAULT '',
                transport TEXT NOT NULL,
                endpoint TEXT DEFAULT '',
                title TEXT DEFAULT '',
                last_seen REAL NOT NULL,
                meta_json TEXT DEFAULT '{}'
            );
            CREATE TABLE IF NOT EXISTS threads (
                thread_id TEXT PRIMARY KEY,
                topic TEXT DEFAULT '',
                created_by TEXT NOT NULL,
                created_at REAL NOT NULL,
                hops_left INTEGER NOT NULL,
                max_hops INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'open'
            );
            CREATE TABLE IF NOT EXISTS messages (
                msg_id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL,
                seq INTEGER NOT NULL,
                from_peer TEXT NOT NULL,
                to_peer TEXT NOT NULL,
                body TEXT NOT NULL,
                expect_reply INTEGER NOT NULL DEFAULT 1,
                delivered_via TEXT DEFAULT '',
                delivered_ok INTEGER NOT NULL DEFAULT 0,
                delivered_info TEXT DEFAULT '',
                created_at REAL NOT NULL,
                read_at REAL
            );
            CREATE INDEX IF NOT EXISTS idx_msg_thread ON messages(thread_id, seq);
            CREATE INDEX IF NOT EXISTS idx_msg_to ON messages(to_peer, read_at, created_at);
            ",
        )
        .map_err(io_err)?;
        chmod_0600(&db_path);
        Ok(Self { db_path })
    }

    pub fn open_default() -> Result<Self> {
        Self::open(default_db_path())
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    fn conn(&self) -> Result<Connection> {
        let conn = Connection::open(&self.db_path).map_err(io_err)?;
        conn.busy_timeout(Duration::from_millis(5000))
            .map_err(io_err)?;
        Ok(conn)
    }

    pub fn detect_self_identity(&self) -> PeerInfo {
        self.detect_self_identity_with_env(
            std::env::var("SESIONES_PEER_ID").ok().as_deref(),
            std::env::var("SESIONES_PROVIDER").ok().as_deref(),
            std::env::var("ANTIGRAVITY_CONVERSATION_ID").ok().as_deref(),
            std::env::var("CLAUDE_CODE_SESSION_ID").ok().as_deref(),
        )
    }

    fn detect_self_identity_with_env(
        &self,
        sesiones_peer_id: Option<&str>,
        sesiones_provider: Option<&str>,
        agy_conv_id: Option<&str>,
        claude_session_id: Option<&str>,
    ) -> PeerInfo {
        let cwd_path = std::env::current_dir().unwrap_or_default();
        let cwd = cwd_path.to_string_lossy().into_owned();
        let (repo, branch) = git_info(&cwd_path);

        // 1. Explicit override
        if let Some(peer_id) = sesiones_peer_id.filter(|s| !s.is_empty()) {
            let provider = sesiones_provider
                .filter(|s| !s.is_empty())
                .unwrap_or("terminal")
                .to_string();
            return PeerInfo {
                peer_id: peer_id.to_string(),
                provider,
                pid: parent_pid(),
                name: peer_id.to_string(),
                repo,
                branch,
                cwd,
                transport: "bus-inbox".to_string(),
                endpoint: String::new(),
                title: String::new(),
                status: "live".to_string(),
                push_ready: false,
            };
        }

        // 2. Antigravity (agy)
        if let Some(conv) = agy_conv_id.filter(|s| !s.is_empty()) {
            let summaries = read_agy_summaries_at(&agy_summaries_db_path());
            let title = summaries
                .get(conv)
                .map(|(t, _)| t.clone())
                .unwrap_or_default();
            let short = short_id(conv, 8);
            return PeerInfo {
                peer_id: format!("agy:{short}"),
                provider: "agy".to_string(),
                pid: parent_pid(),
                name: if title.is_empty() { short } else { title.clone() },
                repo,
                branch,
                cwd,
                transport: "agy-agentapi".to_string(),
                endpoint: conv.to_string(),
                title,
                status: "live".to_string(),
                push_ready: true,
            };
        }

        // 3. Claude Code via CLAUDE_CODE_SESSION_ID
        if let Some(csid) = claude_session_id.filter(|s| !s.is_empty()) {
            let sessions = read_claude_sessions_in(&claude_sessions_dir());
            for s in sessions {
                if s.session_id == csid {
                    let nm = if s.name.is_empty() {
                        short_id(csid, 8)
                    } else {
                        s.name.clone()
                    };
                    let c_cwd = if s.cwd.is_empty() {
                        cwd.clone()
                    } else {
                        s.cwd.clone()
                    };
                    let (c_repo, c_br) = git_info(Path::new(&c_cwd));
                    let push_ready = !s.messaging_socket_path.is_empty()
                        && Path::new(&s.messaging_socket_path).exists();
                    return PeerInfo {
                        peer_id: format!("claude:{nm}"),
                        provider: "claude".to_string(),
                        pid: if s.pid > 0 { s.pid } else { parent_pid() },
                        name: nm.clone(),
                        repo: c_repo,
                        branch: c_br,
                        cwd: c_cwd,
                        transport: "claude-uds".to_string(),
                        endpoint: s.messaging_socket_path,
                        title: nm,
                        status: "live".to_string(),
                        push_ready,
                    };
                }
            }
        }

        // 4. Walk ancestor processes via native libproc/sysctl (ZERO ps subprocesses!)
        if let Ok(snap) = discover_procs() {
            let mut cur = std::process::id() as i32;
            let mut visited = HashSet::new();
            let sessions = read_claude_sessions_in(&claude_sessions_dir());
            while cur > 1 && visited.insert(cur) {
                let Some(entry) = snap.procs.get(&cur) else {
                    break;
                };
                let args_low = entry.args.to_lowercase();
                let comm_low = entry.comm.to_lowercase();
                let first_tok = entry.args.split_whitespace().next().unwrap_or("");
                let base = Path::new(first_tok)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&entry.comm)
                    .to_lowercase();

                if base == "claude" || comm_low == "claude" || args_low.contains("/claude") {
                    for s in &sessions {
                        if s.pid == cur {
                            let nm = if s.name.is_empty() {
                                format!("pid-{cur}")
                            } else {
                                s.name.clone()
                            };
                            let c_cwd = if s.cwd.is_empty() {
                                cwd.clone()
                            } else {
                                s.cwd.clone()
                            };
                            let (c_repo, c_br) = git_info(Path::new(&c_cwd));
                            let push_ready = !s.messaging_socket_path.is_empty()
                                && Path::new(&s.messaging_socket_path).exists();
                            return PeerInfo {
                                peer_id: format!("claude:{nm}"),
                                provider: "claude".to_string(),
                                pid: cur,
                                name: nm.clone(),
                                repo: c_repo,
                                branch: c_br,
                                cwd: c_cwd,
                                transport: "claude-uds".to_string(),
                                endpoint: s.messaging_socket_path.clone(),
                                title: nm,
                                status: "live".to_string(),
                                push_ready,
                            };
                        }
                    }
                }

                if base == "agy" || comm_low == "agy" || args_low.contains("antigravity") {
                    let instances = harvest_agy_instances();
                    let conv_id = instances
                        .into_iter()
                        .find(|i| i.pid == cur)
                        .map(|i| i.conv_id)
                        .or_else(|| agy_pids(&snap).remove(&cur))
                        .unwrap_or_default();
                    if !conv_id.is_empty() {
                        let summaries = read_agy_summaries_at(&agy_summaries_db_path());
                        let title = summaries
                            .get(&conv_id)
                            .map(|(t, _)| t.clone())
                            .unwrap_or_default();
                        let short = short_id(&conv_id, 8);
                        return PeerInfo {
                            peer_id: format!("agy:{short}"),
                            provider: "agy".to_string(),
                            pid: cur,
                            name: if title.is_empty() { short } else { title.clone() },
                            repo,
                            branch,
                            cwd,
                            transport: "agy-agentapi".to_string(),
                            endpoint: conv_id,
                            title,
                            status: "live".to_string(),
                            push_ready: true,
                        };
                    }
                }

                if base == "codex" || base.contains("codex") || comm_low.contains("codex") {
                    let tag = if repo.is_empty() { "term" } else { &repo };
                    let name = if repo.is_empty() {
                        format!("codex-{cur}")
                    } else {
                        format!("codex-{repo}")
                    };
                    return PeerInfo {
                        peer_id: format!("codex:{tag}-{cur}"),
                        provider: "codex".to_string(),
                        pid: cur,
                        name,
                        repo: repo.clone(),
                        branch,
                        cwd,
                        transport: "bus-inbox".to_string(),
                        endpoint: String::new(),
                        title: format!("codex ({repo})"),
                        status: "live".to_string(),
                        push_ready: false,
                    };
                }

                if base == "opencode" || base.contains("opencode") || comm_low.contains("opencode") {
                    let tag = if repo.is_empty() { "term" } else { &repo };
                    let name = if repo.is_empty() {
                        format!("opencode-{cur}")
                    } else {
                        format!("opencode-{repo}")
                    };
                    return PeerInfo {
                        peer_id: format!("opencode:{tag}-{cur}"),
                        provider: "opencode".to_string(),
                        pid: cur,
                        name,
                        repo: repo.clone(),
                        branch,
                        cwd,
                        transport: "bus-inbox".to_string(),
                        endpoint: String::new(),
                        title: format!("opencode ({repo})"),
                        status: "live".to_string(),
                        push_ready: false,
                    };
                }

                cur = entry.ppid;
            }
        }

        // 5. Terminal fallback
        let ppid = parent_pid();
        let tag = if repo.is_empty() { "shell" } else { &repo };
        PeerInfo {
            peer_id: format!("term:{tag}-{ppid}"),
            provider: "terminal".to_string(),
            pid: ppid,
            name: format!("{tag}-{ppid}"),
            repo,
            branch,
            cwd: cwd.clone(),
            transport: "bus-inbox".to_string(),
            endpoint: String::new(),
            title: cwd,
            status: "live".to_string(),
            push_ready: false,
        }
    }

    pub fn register_peer(
        &self,
        peer_id: Option<&str>,
        name: Option<&str>,
        role: Option<&str>,
        provider: Option<&str>,
    ) -> Result<PeerInfo> {
        let mut ident = self.detect_self_identity();
        if let Some(pid_override) = peer_id
            && !pid_override.is_empty()
        {
            ident.peer_id = pid_override.to_string();
        }
        if let Some(prov) = provider
            && !prov.is_empty()
        {
            if prov != ident.provider && !matches!(prov, "claude" | "agy") {
                ident.transport = "bus-inbox".to_string();
                ident.endpoint.clear();
                ident.push_ready = false;
            }
            ident.provider = prov.to_string();
        }
        if let Some(nm) = name
            && !nm.is_empty()
        {
            ident.name = nm.to_string();
            if matches!(ident.provider.as_str(), "codex" | "opencode" | "terminal")
                && peer_id.map(|s| s.is_empty()).unwrap_or(true)
            {
                ident.peer_id = format!("{}:{nm}", ident.provider);
            }
        }
        let meta = serde_json::json!({ "role": role.unwrap_or("") }).to_string();
        let now = now_f64();
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO peers (peer_id, provider, pid, name, repo, branch, cwd, transport, endpoint, title, last_seen, meta_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(peer_id) DO UPDATE SET
               provider=excluded.provider, pid=excluded.pid, name=excluded.name,
               repo=excluded.repo, branch=excluded.branch, cwd=excluded.cwd,
               transport=excluded.transport, endpoint=excluded.endpoint,
               title=excluded.title, last_seen=excluded.last_seen, meta_json=excluded.meta_json",
            params![
                ident.peer_id,
                ident.provider,
                ident.pid,
                ident.name,
                ident.repo,
                ident.branch,
                ident.cwd,
                ident.transport,
                ident.endpoint,
                ident.title,
                now,
                meta,
            ],
        )
        .map_err(io_err)?;
        Ok(ident)
    }

    pub fn list_peers(&self, provider: Option<&str>, repo: Option<&str>) -> Result<Vec<PeerInfo>> {
        let mut found: HashMap<String, PeerInfo> = HashMap::new();
        let now = now_f64();

        // 1. Live Claude Code sessions
        for s in read_claude_sessions_in(&claude_sessions_dir()) {
            if s.pid <= 0 || !pid_alive(s.pid) {
                continue;
            }
            let nm = if s.name.is_empty() {
                format!("pid-{}", s.pid)
            } else {
                s.name
            };
            let (r_name, br) = git_info(Path::new(&s.cwd));
            let push_ready = !s.messaging_socket_path.is_empty()
                && Path::new(&s.messaging_socket_path).exists();
            let key = format!("claude:{nm}");
            found.insert(
                key.clone(),
                PeerInfo {
                    peer_id: key,
                    provider: "claude".to_string(),
                    pid: s.pid,
                    name: nm.clone(),
                    repo: r_name,
                    branch: br,
                    cwd: s.cwd,
                    transport: "claude-uds".to_string(),
                    endpoint: s.messaging_socket_path,
                    title: nm,
                    status: if s.status.is_empty() {
                        "live".to_string()
                    } else {
                        s.status
                    },
                    push_ready,
                },
            );
        }

        // 2. Live Antigravity (agy) windows
        let instances = harvest_agy_instances();
        let linked_pids: HashSet<i32> = instances.iter().map(|i| i.pid).collect();
        let mut agy_windows: HashMap<i32, String> = instances
            .into_iter()
            .filter(|i| !i.conv_id.is_empty())
            .map(|i| (i.pid, i.conv_id))
            .collect();
        if let Ok(snap) = discover_procs() {
            for (pid, conv) in agy_pids(&snap) {
                if !conv.is_empty() {
                    agy_windows.entry(pid).or_insert(conv);
                }
            }
        }
        if !agy_windows.is_empty() {
            let summaries = read_agy_summaries_at(&agy_summaries_db_path());
            for (pid, conv_id) in agy_windows {
                if conv_id.is_empty() || pid <= 0 || !pid_alive(pid) {
                    continue;
                }
                let (title, ws_json) = summaries
                    .get(&conv_id)
                    .cloned()
                    .unwrap_or_else(|| (String::new(), "[]".to_string()));
                let ws = extract_first_workspace(&ws_json);
                let (r_name, br) = git_info(Path::new(&ws));
                let short = short_id(&conv_id, 8);
                let key = format!("agy:{short}");
                let linked = linked_pids.contains(&pid);
                found.insert(
                    key.clone(),
                    PeerInfo {
                        peer_id: key,
                        provider: "agy".to_string(),
                        pid,
                        name: if title.is_empty() {
                            short
                        } else {
                            title.clone()
                        },
                        repo: r_name,
                        branch: br,
                        cwd: ws,
                        transport: "agy-agentapi".to_string(),
                        endpoint: conv_id,
                        title: if title.is_empty() {
                            "(sin título)".to_string()
                        } else {
                            title
                        },
                        status: if linked {
                            "linked".to_string()
                        } else {
                            "unlinked".to_string()
                        },
                        push_ready: linked,
                    },
                );
            }
        }

        // 3. Registered peers in bus.db + upsert discovered live peers
        let mut conn = self.conn()?;
        let db_rows: Vec<(PeerInfo, f64)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT peer_id, provider, pid, name, repo, branch, cwd, transport, endpoint, title, last_seen
                     FROM peers",
                )
                .map_err(io_err)?;
            let mapped = stmt
                .query_map([], |row| {
                    let transport: String = row.get(7)?;
                    let push_ready =
                        transport == "claude-uds" || transport == "agy-agentapi";
                    Ok((
                        PeerInfo {
                            peer_id: row.get(0)?,
                            provider: row.get(1)?,
                            pid: row.get::<_, Option<i32>>(2)?.unwrap_or(0),
                            name: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                            repo: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                            branch: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                            cwd: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                            transport,
                            endpoint: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
                            title: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
                            status: "registered".to_string(),
                            push_ready,
                        },
                        row.get::<_, f64>(10)?,
                    ))
                })
                .map_err(io_err)?;
            mapped.filter_map(|r| r.ok()).collect()
        };

        for (p, last_seen) in db_rows {
            let alive = if p.pid > 0 {
                pid_alive(p.pid)
            } else {
                now - last_seen < STALE_PEER_SECS
            };
            if !alive {
                continue;
            }
            if let Some(existing) = found.get_mut(&p.peer_id) {
                if !p.name.is_empty() && p.name != existing.name {
                    existing.name = p.name;
                }
            } else {
                found.insert(p.peer_id.clone(), p);
            }
        }

        if !found.is_empty() {
            let tx = conn.transaction().map_err(io_err)?;
            {
                let mut upsert = tx
                    .prepare(
                        "INSERT INTO peers (peer_id, provider, pid, name, repo, branch, cwd, transport, endpoint, title, last_seen, meta_json)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, '{}')
                         ON CONFLICT(peer_id) DO UPDATE SET
                           pid=excluded.pid, name=excluded.name, repo=excluded.repo, branch=excluded.branch,
                           cwd=excluded.cwd, transport=excluded.transport, endpoint=excluded.endpoint,
                           title=excluded.title, last_seen=excluded.last_seen",
                    )
                    .map_err(io_err)?;
                for p in found.values() {
                    upsert
                        .execute(params![
                            p.peer_id,
                            p.provider,
                            p.pid,
                            p.name,
                            p.repo,
                            p.branch,
                            p.cwd,
                            p.transport,
                            p.endpoint,
                            p.title,
                            now,
                        ])
                        .map_err(io_err)?;
                }
            }
            tx.commit().map_err(io_err)?;
        }

        let mut peers: Vec<PeerInfo> = found.into_values().collect();
        if let Some(prov) = provider {
            let pl = prov.trim().to_lowercase();
            if !pl.is_empty() {
                peers.retain(|p| p.provider.to_lowercase() == pl);
            }
        }
        if let Some(rp) = repo {
            let rl = rp.trim().to_lowercase();
            if !rl.is_empty() {
                peers.retain(|p| {
                    p.repo.to_lowercase().contains(&rl) || p.cwd.to_lowercase().contains(&rl)
                });
            }
        }
        peers.sort_by(|a, b| {
            (&a.provider, &a.repo, &a.peer_id).cmp(&(&b.provider, &b.repo, &b.peer_id))
        });
        Ok(peers)
    }

    pub fn resolve_peer(&self, target: &str) -> Result<(Option<PeerInfo>, Vec<PeerInfo>)> {
        let peers = self.list_peers(None, None)?;
        let q = target.trim();
        if q.is_empty() {
            return Ok((None, peers));
        }
        let ql = q.to_lowercase();

        // 1. Exact peer_id or endpoint match
        for p in &peers {
            if p.peer_id.to_lowercase() == ql
                || (!p.endpoint.is_empty() && p.endpoint.to_lowercase() == ql)
            {
                return Ok((Some(p.clone()), vec![p.clone()]));
            }
        }

        // 2. Exact name match
        let exact_name: Vec<PeerInfo> = peers
            .iter()
            .filter(|p| p.name.to_lowercase() == ql)
            .cloned()
            .collect();
        if exact_name.len() == 1 {
            return Ok((Some(exact_name[0].clone()), exact_name));
        }

        // 3. Substring across peer_id, name, title, endpoint, repo
        let hits: Vec<PeerInfo> = peers
            .into_iter()
            .filter(|p| {
                p.peer_id.to_lowercase().contains(&ql)
                    || p.name.to_lowercase().contains(&ql)
                    || p.title.to_lowercase().contains(&ql)
                    || p.endpoint.to_lowercase().contains(&ql)
                    || p.repo.to_lowercase().contains(&ql)
            })
            .collect();
        if hits.len() == 1 {
            return Ok((Some(hits[0].clone()), hits));
        }
        Ok((None, hits))
    }

    pub fn list_threads(&self) -> Result<Vec<ThreadRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT thread_id, topic, created_by, created_at, hops_left, max_hops, status
                 FROM threads ORDER BY created_at DESC LIMIT 100",
            )
            .map_err(io_err)?;
        let rows = stmt.query_map([], map_thread_row).map_err(io_err)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn deliver_wakeup(
        peer: &PeerInfo,
        from_peer: &str,
        thread_id: &str,
        hop_used: i64,
        max_hops: i64,
        body: &str,
        expect_reply: bool,
    ) -> (bool, String, String) {
        let reply_hint = if expect_reply {
            format!(
                "Responde autonomamente con MCP `bus_reply(thread_id=\"{thread_id}\", body=\"...\", done=false|true)` (o CLI `sesiones-bus reply {thread_id} \"...\"`)."
            )
        } else {
            format!("(Hilo {thread_id} cerrado / solo informativo; no requiere respuesta.)")
        };
        let envelope = format!(
            "[sesiones-bus thread={thread_id} from={from_peer} hop={hop_used}/{max_hops}]\n{body}\n---\n{reply_hint}"
        );

        match peer.transport.as_str() {
            "claude-uds" => {
                if peer.endpoint.is_empty() {
                    return (
                        false,
                        "claude-uds".to_string(),
                        "Claude session has no messagingSocketPath".to_string(),
                    );
                }
                match send_claude_uds(Path::new(&peer.endpoint), &envelope, Some(from_peer)) {
                    Ok(()) => (true, "claude-uds".to_string(), "sent via claude-uds".to_string()),
                    Err(e) => (false, "claude-uds".to_string(), e.to_string()),
                }
            }
            "agy-agentapi" => {
                if peer.endpoint.is_empty() {
                    return (
                        false,
                        "agy-agentapi".to_string(),
                        "Antigravity window has no conversation_id".to_string(),
                    );
                }
                let instances = harvest_agy_instances();
                let inst: Option<AgyInstance> =
                    instances.into_iter().find(|i| i.conv_id == peer.endpoint);
                match inst {
                    Some(inst) => match send_agy_message(&inst, &envelope, Some(from_peer)) {
                        Ok(()) => (
                            true,
                            "agy-agentapi".to_string(),
                            "enviado a Antigravity".to_string(),
                        ),
                        Err(e) => (false, "agy-agentapi".to_string(), e.to_string()),
                    },
                    None => (
                        false,
                        "agy-agentapi".to_string(),
                        "esa ventana de Antigravity ya no está abierta".to_string(),
                    ),
                }
            }
            _ => (
                true,
                "bus-inbox".to_string(),
                "stored in bus.db inbox (peer reads via bus_inbox/bus_wait)".to_string(),
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn send(
        &self,
        to: &str,
        body: &str,
        topic: &str,
        from_peer: Option<&str>,
        thread_id: Option<&str>,
        expect_reply: bool,
        max_hops: i64,
        done: bool,
    ) -> Result<SendResult> {
        self.send_with_wakeup(
            to,
            body,
            topic,
            from_peer,
            thread_id,
            expect_reply,
            max_hops,
            done,
            Self::deliver_wakeup,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn send_with_wakeup<F>(
        &self,
        to: &str,
        body: &str,
        topic: &str,
        from_peer: Option<&str>,
        thread_id: Option<&str>,
        expect_reply: bool,
        max_hops: i64,
        done: bool,
        wakeup: F,
    ) -> Result<SendResult>
    where
        F: FnOnce(&PeerInfo, &str, &str, i64, i64, &str, bool) -> (bool, String, String),
    {
        let sender = match from_peer {
            Some(f) if !f.is_empty() => f.to_string(),
            _ => self.detect_self_identity().peer_id,
        };

        let (peer_opt, cands) = self.resolve_peer(to)?;
        let Some(peer) = peer_opt else {
            if cands.is_empty() {
                return Ok(SendResult {
                    ok: false,
                    error: Some(format!(
                        "No live peer matched {to:?}. Run bus_list() to see active windows."
                    )),
                    candidates: None,
                    msg_id: None,
                    thread_id: None,
                    seq: None,
                    from: None,
                    to: None,
                    to_provider: None,
                    delivered_via: None,
                    delivered_info: None,
                    hops_left: None,
                    done: None,
                });
            }
            return Ok(SendResult {
                ok: false,
                error: Some(format!(
                    "Ambiguous target {to:?} ({} matches). Pick an exact peer_id:",
                    cands.len()
                )),
                candidates: Some(
                    cands
                        .into_iter()
                        .map(|c| PeerCandidate {
                            peer_id: c.peer_id,
                            provider: c.provider,
                            repo: c.repo,
                            title: c.title,
                        })
                        .collect(),
                ),
                msg_id: None,
                thread_id: None,
                seq: None,
                from: None,
                to: None,
                to_provider: None,
                delivered_via: None,
                delivered_info: None,
                hops_left: None,
                done: None,
            });
        };

        let now = now_f64();
        let msg_id = hex_id(12);
        let want_reply = expect_reply && !done;

        // Transaction 1 (short Immediate): validate/update thread, allocate seq,
        // insert message with delivered_ok=0, and commit before any wakeup I/O.
        let (tid, seq, hop_used, max_h, hops_left) = {
            let mut conn = self.conn()?;
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(io_err)?;

            let (tid, seq, hop_used, max_h, hops_left) = match thread_id.filter(|s| !s.is_empty()) {
                None => {
                    let tid = hex_id(10);
                    let base_hops = if max_hops <= 0 {
                        DEFAULT_MAX_HOPS
                    } else {
                        max_hops
                    };
                    let mh = base_hops.clamp(1, 30);
                    let hops_left = if done { 0 } else { mh - 1 };
                    let status = if done { "done" } else { "open" };
                    let topic_val = if topic.is_empty() {
                        truncate_chars(body, 80)
                    } else {
                        topic.to_string()
                    };
                    tx.execute(
                        "INSERT INTO threads (thread_id, topic, created_by, created_at, hops_left, max_hops, status)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![tid, topic_val, sender, now, hops_left, mh, status],
                    )
                    .map_err(io_err)?;
                    (tid, 1_i64, 1_i64, mh, hops_left)
                }
                Some(tid) => {
                    let tr: Option<ThreadRecord> = tx
                        .query_row(
                            "SELECT thread_id, topic, created_by, created_at, hops_left, max_hops, status
                             FROM threads WHERE thread_id=?1",
                            params![tid],
                            map_thread_row,
                        )
                        .optional()
                        .map_err(io_err)?;
                    let Some(tr) = tr else {
                        return Ok(SendResult {
                            ok: false,
                            error: Some(format!("Thread {tid:?} not found")),
                            candidates: None,
                            msg_id: None,
                            thread_id: None,
                            seq: None,
                            from: None,
                            to: None,
                            to_provider: None,
                            delivered_via: None,
                            delivered_info: None,
                            hops_left: None,
                            done: None,
                        });
                    };
                    if tr.status != "open" {
                        return Ok(SendResult {
                            ok: false,
                            error: Some(format!("Thread {tid:?} is already {}", tr.status)),
                            candidates: None,
                            msg_id: None,
                            thread_id: None,
                            seq: None,
                            from: None,
                            to: None,
                            to_provider: None,
                            delivered_via: None,
                            delivered_info: None,
                            hops_left: None,
                            done: None,
                        });
                    }
                    if tr.hops_left <= 0 && !done {
                        tx.execute(
                            "UPDATE threads SET status='exhausted' WHERE thread_id=?1",
                            params![tid],
                        )
                        .map_err(io_err)?;
                        tx.commit().map_err(io_err)?;
                        return Ok(SendResult {
                            ok: false,
                            error: Some(format!(
                                "Thread {tid:?} exhausted its {} hops (anti-loop guard)",
                                tr.max_hops
                            )),
                            candidates: None,
                            msg_id: None,
                            thread_id: None,
                            seq: None,
                            from: None,
                            to: None,
                            to_provider: None,
                            delivered_via: None,
                            delivered_info: None,
                            hops_left: None,
                            done: None,
                        });
                    }
                    let max_seq: i64 = tx
                        .query_row(
                            "SELECT COALESCE(MAX(seq), 0) FROM messages WHERE thread_id=?1",
                            params![tid],
                            |r| r.get(0),
                        )
                        .map_err(io_err)?;
                    let seq = max_seq + 1;
                    let max_h = tr.max_hops;
                    let hop_used = max_h - tr.hops_left + 1;
                    let new_hops = if done { 0 } else { (tr.hops_left - 1).max(0) };
                    let new_status = if done {
                        "done"
                    } else if new_hops == 0 {
                        "exhausted"
                    } else {
                        "open"
                    };
                    tx.execute(
                        "UPDATE threads SET hops_left=?1, status=?2 WHERE thread_id=?3",
                        params![new_hops, new_status, tid],
                    )
                    .map_err(io_err)?;
                    (tid.to_string(), seq, hop_used, max_h, new_hops)
                }
            };

            tx.execute(
                "INSERT INTO messages (msg_id, thread_id, seq, from_peer, to_peer, body, expect_reply, delivered_via, delivered_ok, delivered_info, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '', 0, '', ?8)",
                params![
                    msg_id,
                    tid,
                    seq,
                    sender,
                    peer.peer_id,
                    body,
                    i64::from(want_reply),
                    now,
                ],
            )
            .map_err(io_err)?;
            tx.commit().map_err(io_err)?;
            (tid, seq, hop_used, max_h, hops_left)
        };

        // Blocking wakeup I/O runs outside any DB transaction or connection lock.
        let (ok, via, info) =
            wakeup(&peer, &sender, &tid, hop_used, max_h, body, want_reply);

        // Transaction 2 (short Immediate): record delivery outcome.
        {
            let mut conn = self.conn()?;
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(io_err)?;
            tx.execute(
                "UPDATE messages SET delivered_via = ?1, delivered_ok = ?2, delivered_info = ?3 WHERE msg_id = ?4",
                params![via, i64::from(ok), info, msg_id],
            )
            .map_err(io_err)?;
            tx.commit().map_err(io_err)?;
        }

        Ok(SendResult {
            ok,
            error: None,
            candidates: None,
            msg_id: Some(msg_id),
            thread_id: Some(tid),
            seq: Some(seq),
            from: Some(sender),
            to: Some(peer.peer_id),
            to_provider: Some(peer.provider),
            delivered_via: Some(via),
            delivered_info: Some(info),
            hops_left: Some(hops_left),
            done: Some(done),
        })
    }

    pub fn reply(
        &self,
        thread_id: &str,
        body: &str,
        from_peer: Option<&str>,
        done: bool,
    ) -> Result<SendResult> {
        let sender = match from_peer {
            Some(f) if !f.is_empty() => f.to_string(),
            _ => self.detect_self_identity().peer_id,
        };
        let rows: Vec<MessageRecord> = {
            let conn = self.conn()?;
            let mut stmt = conn
                .prepare(
                    "SELECT msg_id, thread_id, seq, from_peer, to_peer, body, expect_reply,
                            delivered_via, delivered_ok, delivered_info, created_at, read_at
                     FROM messages WHERE thread_id=?1 ORDER BY seq DESC",
                )
                .map_err(io_err)?;
            let mapped = stmt
                .query_map(params![thread_id], map_message_row)
                .map_err(io_err)?;
            mapped.filter_map(|r| r.ok()).collect()
        };
        if rows.is_empty() {
            return Ok(SendResult {
                ok: false,
                error: Some(format!("Thread {thread_id:?} not found or empty")),
                candidates: None,
                msg_id: None,
                thread_id: None,
                seq: None,
                from: None,
                to: None,
                to_provider: None,
                delivered_via: None,
                delivered_info: None,
                hops_left: None,
                done: None,
            });
        }
        let target = rows
            .iter()
            .find(|r| r.from_peer != sender)
            .map(|r| r.from_peer.clone())
            .unwrap_or_else(|| {
                if rows[0].from_peer == sender {
                    rows[0].to_peer.clone()
                } else {
                    rows[0].from_peer.clone()
                }
            });

        self.send(
            &target,
            body,
            "",
            Some(&sender),
            Some(thread_id),
            !done,
            DEFAULT_MAX_HOPS,
            done,
        )
    }

    pub fn inbox(
        &self,
        peer: Option<&str>,
        unread_only: bool,
        limit: usize,
    ) -> Result<InboxResult> {
        let me = match peer {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => self.detect_self_identity().peer_id,
        };
        let lim = limit.max(1) as i64;
        let now = now_f64();
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(io_err)?;

        let sql = if unread_only {
            "SELECT msg_id, thread_id, seq, from_peer, to_peer, body, expect_reply,
                    delivered_via, delivered_ok, delivered_info, created_at, read_at
             FROM messages WHERE to_peer=?1 AND read_at IS NULL
             ORDER BY created_at DESC LIMIT ?2"
        } else {
            "SELECT msg_id, thread_id, seq, from_peer, to_peer, body, expect_reply,
                    delivered_via, delivered_ok, delivered_info, created_at, read_at
             FROM messages WHERE to_peer=?1
             ORDER BY created_at DESC LIMIT ?2"
        };

        let mut rows: Vec<MessageRecord> = {
            let mut stmt = tx.prepare(sql).map_err(io_err)?;
            let mapped = stmt
                .query_map(params![me, lim], map_message_row)
                .map_err(io_err)?;
            mapped.filter_map(|r| r.ok()).collect()
        };

        if unread_only && !rows.is_empty() {
            let mut upd = tx
                .prepare("UPDATE messages SET read_at=?1 WHERE msg_id=?2")
                .map_err(io_err)?;
            for r in &rows {
                upd.execute(params![now, r.msg_id]).map_err(io_err)?;
            }
        }
        tx.commit().map_err(io_err)?;

        rows.reverse();
        let count = rows.len();
        Ok(InboxResult {
            peer: me,
            count,
            messages: rows,
        })
    }

    pub fn thread(&self, thread_id: &str) -> Result<ThreadDetail> {
        let conn = self.conn()?;
        let tr: Option<ThreadRecord> = conn
            .query_row(
                "SELECT thread_id, topic, created_by, created_at, hops_left, max_hops, status
                 FROM threads WHERE thread_id=?1",
                params![thread_id],
                map_thread_row,
            )
            .optional()
            .map_err(io_err)?;
        let Some(tr) = tr else {
            return Ok(ThreadDetail {
                ok: false,
                error: Some(format!("Thread {thread_id:?} not found")),
                thread: None,
                messages: None,
            });
        };
        let mut stmt = conn
            .prepare(
                "SELECT msg_id, thread_id, seq, from_peer, to_peer, body, expect_reply,
                        delivered_via, delivered_ok, delivered_info, created_at, read_at
                 FROM messages WHERE thread_id=?1 ORDER BY seq ASC",
            )
            .map_err(io_err)?;
        let msgs: Vec<MessageRecord> = stmt
            .query_map(params![thread_id], map_message_row)
            .map_err(io_err)?
            .filter_map(|r| r.ok())
            .collect();
        Ok(ThreadDetail {
            ok: true,
            error: None,
            thread: Some(tr),
            messages: Some(msgs),
        })
    }

    pub fn close(&self, thread_id: &str, reason: &str) -> Result<CloseResult> {
        let conn = self.conn()?;
        let changed = conn
            .execute(
                "UPDATE threads SET status='done', hops_left=0 WHERE thread_id=?1",
                params![thread_id],
            )
            .map_err(io_err)?;
        if changed == 0 {
            return Ok(CloseResult {
                ok: false,
                error: Some(format!("Thread {thread_id:?} not found")),
                thread_id: None,
                status: None,
                reason: None,
            });
        }
        Ok(CloseResult {
            ok: true,
            error: None,
            thread_id: Some(thread_id.to_string()),
            status: Some("done".to_string()),
            reason: Some(reason.to_string()),
        })
    }

    pub fn wait(
        &self,
        peer: Option<&str>,
        thread_id: Option<&str>,
        after_seq: i64,
        timeout_sec: u64,
    ) -> Result<WaitResult> {
        let me = match peer {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => self.detect_self_identity().peer_id,
        };
        let secs = if timeout_sec == 0 { 60 } else { timeout_sec.clamp(1, 300) };
        let deadline = Instant::now() + Duration::from_secs(secs);

        loop {
            {
                let mut conn = self.conn()?;
                let tx = conn
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(io_err)?;
                let found: Option<MessageRecord> = if let Some(tid) = thread_id.filter(|s| !s.is_empty()) {
                    tx.query_row(
                        "SELECT msg_id, thread_id, seq, from_peer, to_peer, body, expect_reply,
                                delivered_via, delivered_ok, delivered_info, created_at, read_at
                         FROM messages WHERE thread_id=?1 AND seq>?2 AND from_peer!=?3
                         ORDER BY seq ASC LIMIT 1",
                        params![tid, after_seq, me],
                        map_message_row,
                    )
                    .optional()
                    .map_err(io_err)?
                } else {
                    tx.query_row(
                        "SELECT msg_id, thread_id, seq, from_peer, to_peer, body, expect_reply,
                                delivered_via, delivered_ok, delivered_info, created_at, read_at
                         FROM messages WHERE to_peer=?1 AND read_at IS NULL
                         ORDER BY created_at ASC LIMIT 1",
                        params![me],
                        map_message_row,
                    )
                    .optional()
                    .map_err(io_err)?
                };
                if let Some(msg) = found {
                    tx.execute(
                        "UPDATE messages SET read_at=?1 WHERE msg_id=?2",
                        params![now_f64(), msg.msg_id],
                    )
                    .map_err(io_err)?;
                    tx.commit().map_err(io_err)?;
                    return Ok(WaitResult {
                        ok: true,
                        timed_out: false,
                        message: Some(msg),
                    });
                }
                tx.commit().map_err(io_err)?;
            }

            if Instant::now() >= deadline {
                return Ok(WaitResult {
                    ok: true,
                    timed_out: true,
                    message: None,
                });
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn is_subcommand(s: &str) -> bool {
    matches!(
        s,
        "list" | "whoami" | "send" | "reply" | "inbox" | "thread" | "wait" | "close" | "-h" | "--help"
    )
}

const CLI_USAGE: &str = "Usage:
  sesiones-bus list [--provider P] [--repo R] [--json]
  sesiones-bus whoami [--name N] [--role R]
  sesiones-bus send <target> <message> [--topic T] [--hops N] [--no-reply] [--from ID]
  sesiones-bus reply <thread_id> <message> [--done] [--from ID]
  sesiones-bus inbox [--peer ID] [--all]
  sesiones-bus thread <thread_id>
  sesiones-bus wait [--thread ID] [--after N] [--timeout S] [--peer ID]
  sesiones-bus close <thread_id> [reason]";

fn print_cli_usage() {
    eprintln!("{CLI_USAGE}");
}

fn next_arg(args: &[String], idx: &mut usize, flag: &str) -> std::result::Result<String, String> {
    if *idx + 1 >= args.len() {
        return Err(format!("missing value for {flag}"));
    }
    *idx += 1;
    Ok(args[*idx].clone())
}

pub fn dispatch_cli(bus: &Bus, argv: &[String]) -> i32 {
    if argv.is_empty() {
        print_cli_usage();
        return 2;
    }
    let slice: &[String] = if is_subcommand(&argv[0]) {
        argv
    } else if argv.len() > 1 {
        &argv[1..]
    } else {
        print_cli_usage();
        return 2;
    };

    let cmd = slice[0].as_str();
    let rest = &slice[1..];
    match cmd {
        "-h" | "--help" => {
            println!("{CLI_USAGE}");
            0
        }
        "list" => {
            let mut provider: Option<String> = None;
            let mut repo: Option<String> = None;
            let mut as_json = false;
            let mut i = 0;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--provider" => match next_arg(rest, &mut i, "--provider") {
                        Ok(v) => provider = Some(v),
                        Err(e) => {
                            eprintln!("{e}");
                            return 2;
                        }
                    },
                    "--repo" => match next_arg(rest, &mut i, "--repo") {
                        Ok(v) => repo = Some(v),
                        Err(e) => {
                            eprintln!("{e}");
                            return 2;
                        }
                    },
                    "--json" => as_json = true,
                    other => {
                        eprintln!("unknown argument: {other}");
                        return 2;
                    }
                }
                i += 1;
            }
            match bus.list_peers(provider.as_deref(), repo.as_deref()) {
                Ok(peers) => {
                    if as_json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&peers).unwrap_or_else(|_| "[]".into())
                        );
                    } else if peers.is_empty() {
                        println!("(ninguna sesión activa)");
                    } else {
                        for x in peers {
                            let push = if x.push_ready { "push" } else { "inbox" };
                            let rp = if x.repo.is_empty() { "-" } else { &x.repo };
                            let br = if x.branch.is_empty() { "-" } else { &x.branch };
                            println!(
                                "{:<30} {:<9} {:<16} {:<18} [{}/{}] {}",
                                x.peer_id, x.provider, rp, br, x.status, push, x.title
                            );
                        }
                    }
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        "whoami" => {
            let mut name: Option<String> = None;
            let mut role: Option<String> = None;
            let mut provider: Option<String> = None;
            let mut i = 0;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--name" => match next_arg(rest, &mut i, "--name") {
                        Ok(v) => name = Some(v),
                        Err(e) => {
                            eprintln!("{e}");
                            return 2;
                        }
                    },
                    "--role" => match next_arg(rest, &mut i, "--role") {
                        Ok(v) => role = Some(v),
                        Err(e) => {
                            eprintln!("{e}");
                            return 2;
                        }
                    },
                    "--provider" => match next_arg(rest, &mut i, "--provider") {
                        Ok(v) => provider = Some(v),
                        Err(e) => {
                            eprintln!("{e}");
                            return 2;
                        }
                    },
                    other => {
                        eprintln!("unknown argument: {other}");
                        return 2;
                    }
                }
                i += 1;
            }
            match bus.register_peer(None, name.as_deref(), role.as_deref(), provider.as_deref()) {
                Ok(p) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&p).unwrap_or_else(|_| "{}".into())
                    );
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        "send" => {
            let mut topic = String::new();
            let mut hops = DEFAULT_MAX_HOPS;
            let mut no_reply = false;
            let mut from_peer: Option<String> = None;
            let mut pos: Vec<String> = Vec::new();
            let mut i = 0;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--topic" => match next_arg(rest, &mut i, "--topic") {
                        Ok(v) => topic = v,
                        Err(e) => {
                            eprintln!("{e}");
                            return 2;
                        }
                    },
                    "--hops" => match next_arg(rest, &mut i, "--hops") {
                        Ok(v) => match v.parse::<i64>() {
                            Ok(n) => hops = n,
                            Err(_) => {
                                eprintln!("invalid --hops: {v}");
                                return 2;
                            }
                        },
                        Err(e) => {
                            eprintln!("{e}");
                            return 2;
                        }
                    },
                    "--no-reply" => no_reply = true,
                    "--from" => match next_arg(rest, &mut i, "--from") {
                        Ok(v) => from_peer = Some(v),
                        Err(e) => {
                            eprintln!("{e}");
                            return 2;
                        }
                    },
                    other => pos.push(other.to_string()),
                }
                i += 1;
            }
            if pos.len() < 2 {
                eprintln!("usage: sesiones-bus send <target> <message...>");
                return 2;
            }
            let to = &pos[0];
            let body = pos[1..].join(" ");
            match bus.send(
                to,
                &body,
                &topic,
                from_peer.as_deref(),
                None,
                !no_reply,
                hops,
                false,
            ) {
                Ok(res) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&res).unwrap_or_else(|_| "{}".into())
                    );
                    if res.ok { 0 } else { 1 }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        "reply" => {
            let mut done = false;
            let mut from_peer: Option<String> = None;
            let mut pos: Vec<String> = Vec::new();
            let mut i = 0;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--done" => done = true,
                    "--from" => match next_arg(rest, &mut i, "--from") {
                        Ok(v) => from_peer = Some(v),
                        Err(e) => {
                            eprintln!("{e}");
                            return 2;
                        }
                    },
                    other => pos.push(other.to_string()),
                }
                i += 1;
            }
            if pos.len() < 2 {
                eprintln!("usage: sesiones-bus reply <thread_id> <message...>");
                return 2;
            }
            let tid = &pos[0];
            let body = pos[1..].join(" ");
            match bus.reply(tid, &body, from_peer.as_deref(), done) {
                Ok(res) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&res).unwrap_or_else(|_| "{}".into())
                    );
                    if res.ok { 0 } else { 1 }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        "inbox" => {
            let mut peer: Option<String> = None;
            let mut all = false;
            let mut i = 0;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--peer" => match next_arg(rest, &mut i, "--peer") {
                        Ok(v) => peer = Some(v),
                        Err(e) => {
                            eprintln!("{e}");
                            return 2;
                        }
                    },
                    "--all" => all = true,
                    other => {
                        eprintln!("unknown argument: {other}");
                        return 2;
                    }
                }
                i += 1;
            }
            match bus.inbox(peer.as_deref(), !all, 30) {
                Ok(res) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&res).unwrap_or_else(|_| "{}".into())
                    );
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        "thread" => {
            if rest.is_empty() {
                eprintln!("usage: sesiones-bus thread <thread_id>");
                return 2;
            }
            match bus.thread(&rest[0]) {
                Ok(res) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&res).unwrap_or_else(|_| "{}".into())
                    );
                    if res.ok { 0 } else { 1 }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        "wait" => {
            let mut thread: Option<String> = None;
            let mut after: i64 = 0;
            let mut timeout: u64 = 60;
            let mut peer: Option<String> = None;
            let mut i = 0;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--thread" => match next_arg(rest, &mut i, "--thread") {
                        Ok(v) => thread = Some(v),
                        Err(e) => {
                            eprintln!("{e}");
                            return 2;
                        }
                    },
                    "--after" => match next_arg(rest, &mut i, "--after") {
                        Ok(v) => match v.parse::<i64>() {
                            Ok(n) => after = n,
                            Err(_) => {
                                eprintln!("invalid --after: {v}");
                                return 2;
                            }
                        },
                        Err(e) => {
                            eprintln!("{e}");
                            return 2;
                        }
                    },
                    "--timeout" => match next_arg(rest, &mut i, "--timeout") {
                        Ok(v) => match v.parse::<u64>() {
                            Ok(n) => timeout = n,
                            Err(_) => {
                                eprintln!("invalid --timeout: {v}");
                                return 2;
                            }
                        },
                        Err(e) => {
                            eprintln!("{e}");
                            return 2;
                        }
                    },
                    "--peer" => match next_arg(rest, &mut i, "--peer") {
                        Ok(v) => peer = Some(v),
                        Err(e) => {
                            eprintln!("{e}");
                            return 2;
                        }
                    },
                    other => {
                        eprintln!("unknown argument: {other}");
                        return 2;
                    }
                }
                i += 1;
            }
            match bus.wait(peer.as_deref(), thread.as_deref(), after, timeout) {
                Ok(res) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&res).unwrap_or_else(|_| "{}".into())
                    );
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        "close" => {
            if rest.is_empty() {
                eprintln!("usage: sesiones-bus close <thread_id> [reason...]");
                return 2;
            }
            let tid = &rest[0];
            let reason = if rest.len() > 1 {
                rest[1..].join(" ")
            } else {
                String::new()
            };
            match bus.close(tid, &reason) {
                Ok(res) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&res).unwrap_or_else(|_| "{}".into())
                    );
                    if res.ok { 0 } else { 1 }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        other => {
            eprintln!("unknown command: {other}");
            print_cli_usage();
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CleanDir(PathBuf);
    impl CleanDir {
        fn new(name: &str) -> Self {
            let base = std::env::temp_dir().join(format!("agencia-bus-{name}-{}", uuid_v4()));
            fs::create_dir_all(&base).unwrap();
            Self(base)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for CleanDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn open_creates_wal_schema_and_0600_perms() {
        let dir = CleanDir::new("open");
        let db = dir.path().join("cache").join("bus.db");
        let bus = Bus::open(db.clone()).unwrap();
        assert_eq!(bus.db_path(), db.as_path());
        assert!(db.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&db).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        let conn = Connection::open(&db).unwrap();
        let jm: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(jm.to_lowercase(), "wal");

        let mut tables = Vec::new();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap();
        for r in stmt.query_map([], |row| row.get::<_, String>(0)).unwrap() {
            tables.push(r.unwrap());
        }
        assert!(tables.contains(&"peers".to_string()));
        assert!(tables.contains(&"threads".to_string()));
        assert!(tables.contains(&"messages".to_string()));

        let mut idxs = Vec::new();
        let mut istmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index'")
            .unwrap();
        for r in istmt.query_map([], |row| row.get::<_, String>(0)).unwrap() {
            idxs.push(r.unwrap());
        }
        assert!(idxs.contains(&"idx_msg_thread".to_string()));
        assert!(idxs.contains(&"idx_msg_to".to_string()));
    }

    #[test]
    fn default_db_path_honors_env_precedence() {
        assert_eq!(
            resolve_db_path(None, None, Some("/tmp/root-one")),
            PathBuf::from("/tmp/root-one/cache/bus.db")
        );
        assert_eq!(
            resolve_db_path(None, Some("/tmp/dir-two"), Some("/tmp/root-one")),
            PathBuf::from("/tmp/dir-two/cache/bus.db")
        );
        assert_eq!(
            resolve_db_path(
                Some("/tmp/explicit-bus.db"),
                Some("/tmp/dir-two"),
                Some("/tmp/root-one")
            ),
            PathBuf::from("/tmp/explicit-bus.db")
        );
    }

    #[test]
    fn git_info_reads_standard_and_worktree_head() {
        let dir = CleanDir::new("git");
        let repo_a = dir.path().join("alpha-repo");
        fs::create_dir_all(repo_a.join(".git")).unwrap();
        fs::write(repo_a.join(".git/HEAD"), "ref: refs/heads/feat/rust-bus\n").unwrap();
        let sub = repo_a.join("crates/sub");
        fs::create_dir_all(&sub).unwrap();
        let (r, b) = git_info(&sub);
        assert_eq!(r, "sub");
        assert_eq!(b, "feat/rust-bus");

        let (r2, b2) = git_info(&repo_a);
        assert_eq!(r2, "alpha-repo");
        assert_eq!(b2, "feat/rust-bus");

        // Worktree .git file
        let wt = dir.path().join("wt-repo");
        let actual_git = dir.path().join("actual-git");
        fs::create_dir_all(&wt).unwrap();
        fs::create_dir_all(&actual_git).unwrap();
        fs::write(wt.join(".git"), "gitdir: ../actual-git\n").unwrap();
        fs::write(actual_git.join("HEAD"), "ref: refs/heads/wt-branch\n").unwrap();
        let (wr, wb) = git_info(&wt);
        assert_eq!(wr, "wt-repo");
        assert_eq!(wb, "wt-branch");

        // Detached HEAD
        fs::write(
            actual_git.join("HEAD"),
            "0123456789abcdef0123456789abcdef01234567\n",
        )
        .unwrap();
        let (_, db) = git_info(&wt);
        assert_eq!(db, "HEAD");
    }

    #[test]
    fn claude_sessions_and_agy_summaries_helpers() {
        let dir = CleanDir::new("helpers");
        let sess_dir = dir.path().join("sessions");
        fs::create_dir_all(&sess_dir).unwrap();
        fs::write(
            sess_dir.join("1234.json"),
            r#"{"pid":1234,"sessionId":"abc-123","cwd":"/tmp","messagingSocketPath":"/tmp/cc.sock","name":"win-a","status":"idle"} trailing"#,
        )
        .unwrap();
        let sessions = read_claude_sessions_in(&sess_dir);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].pid, 1234);
        assert_eq!(sessions[0].name, "win-a");
        assert_eq!(sessions[0].messaging_socket_path, "/tmp/cc.sock");

        let agy_db = dir.path().join("conversation_summaries.db");
        let conn = Connection::open(&agy_db).unwrap();
        conn.execute(
            "CREATE TABLE conversation_summaries (conversation_id TEXT PRIMARY KEY, title TEXT, workspace_uris TEXT)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversation_summaries VALUES ('conv-1234-5678', 'My Title', '[\"file:///Users/test/repo\"]')",
            [],
        )
        .unwrap();
        drop(conn);

        let sums = read_agy_summaries_at(&agy_db);
        let (title, ws_json) = sums.get("conv-1234-5678").unwrap();
        assert_eq!(title, "My Title");
        assert_eq!(extract_first_workspace(ws_json), "/Users/test/repo");
    }

    #[test]
    fn self_identity_honors_sesiones_peer_id() {
        let dir = CleanDir::new("ident");
        let bus = Bus::open(dir.path().join("bus.db")).unwrap();
        let ident = bus.detect_self_identity_with_env(
            Some("custom:peer-99"),
            Some("codex"),
            Some("ignored-agy-conv"),
            Some("ignored-claude-sid"),
        );
        assert_eq!(ident.peer_id, "custom:peer-99");
        assert_eq!(ident.provider, "codex");
        assert_eq!(ident.transport, "bus-inbox");
    }

    #[test]
    fn register_send_reply_inbox_wait_close_flow() {
        let dir = CleanDir::new("flow");
        let db = dir.path().join("bus.db");
        let bus = Bus::open(db).unwrap();

        let p1 = bus
            .register_peer(Some("codex:alpha"), Some("alpha"), Some("worker"), Some("codex"))
            .unwrap();
        assert_eq!(p1.peer_id, "codex:alpha");
        let p2 = bus
            .register_peer(Some("opencode:beta"), Some("beta"), Some("reviewer"), Some("opencode"))
            .unwrap();
        assert_eq!(p2.peer_id, "opencode:beta");
        let _g1 = bus
            .register_peer(Some("opencode:gamma-one"), Some("gamma-one"), None, Some("opencode"))
            .unwrap();
        let _g2 = bus
            .register_peer(Some("opencode:gamma-two"), Some("gamma-two"), None, Some("opencode"))
            .unwrap();

        // Ambiguous substring target returns candidates
        let amb = bus
            .send("gamma", "ambiguous", "", Some("codex:alpha"), None, true, 3, false)
            .unwrap();
        assert!(!amb.ok);
        assert_eq!(amb.candidates.as_ref().unwrap().len(), 2);

        // Exact name match resolves uniquely
        let exact = bus
            .send("beta", "hola beta", "test-topic", Some("codex:alpha"), None, true, 3, false)
            .unwrap();
        assert!(exact.ok);
        let tid = exact.thread_id.clone().unwrap();
        assert_eq!(exact.seq, Some(1));
        assert_eq!(exact.hops_left, Some(2));
        assert_eq!(exact.delivered_via.as_deref(), Some("bus-inbox"));

        let peers = bus.list_peers(Some("codex"), None).unwrap();
        assert!(peers.iter().any(|p| p.peer_id == "codex:alpha"));
        assert!(!peers.iter().any(|p| p.peer_id == "opencode:beta"));

        // Beta reads inbox (marks read)
        let inb = bus.inbox(Some("opencode:beta"), true, 10).unwrap();
        assert_eq!(inb.count, 1);
        assert_eq!(inb.messages[0].body, "hola beta");
        let inb_empty = bus.inbox(Some("opencode:beta"), true, 10).unwrap();
        assert_eq!(inb_empty.count, 0);

        // Unread_only = false still sees historical messages
        let inb_all = bus.inbox(Some("opencode:beta"), false, 10).unwrap();
        assert_eq!(inb_all.count, 1);

        // Beta replies -> alpha
        let r1 = bus
            .reply(&tid, "recibido alpha", Some("opencode:beta"), false)
            .unwrap();
        assert!(r1.ok);
        assert_eq!(r1.seq, Some(2));
        assert_eq!(r1.to.as_deref(), Some("codex:alpha"));
        assert_eq!(r1.hops_left, Some(1));

        // Alpha waits for reply after seq 1
        let w = bus
            .wait(Some("codex:alpha"), Some(&tid), 1, 1)
            .unwrap();
        assert!(w.ok);
        assert!(!w.timed_out);
        assert_eq!(w.message.unwrap().body, "recibido alpha");

        // Wait with no new messages times out cleanly
        let w_to = bus
            .wait(Some("codex:alpha"), Some(&tid), 2, 1)
            .unwrap();
        assert!(w_to.ok);
        assert!(w_to.timed_out);
        assert!(w_to.message.is_none());

        // Exhaustion anti-loop guard (1 hop left -> seq 3 exhausts)
        let r2 = bus
            .reply(&tid, "ultimo hop", Some("codex:alpha"), false)
            .unwrap();
        assert!(r2.ok);
        assert_eq!(r2.hops_left, Some(0));

        let r3 = bus
            .reply(&tid, "sobre limite", Some("opencode:beta"), false)
            .unwrap();
        assert!(!r3.ok);

        // Inspect thread & list_threads
        let detail = bus.thread(&tid).unwrap();
        assert!(detail.ok);
        assert_eq!(detail.messages.unwrap().len(), 3);
        let threads = bus.list_threads().unwrap();
        assert!(threads.iter().any(|t| t.thread_id == tid));

        // Close a new thread
        let s2 = bus
            .send(
                "opencode:beta",
                "nuevo hilo",
                "",
                Some("codex:alpha"),
                None,
                true,
                6,
                false,
            )
            .unwrap();
        let tid2 = s2.thread_id.unwrap();
        let cl = bus.close(&tid2, "listo").unwrap();
        assert!(cl.ok);
        assert_eq!(cl.status.as_deref(), Some("done"));

        let after_close = bus
            .reply(&tid2, "despues de cerrar", Some("opencode:beta"), false)
            .unwrap();
        assert!(!after_close.ok);
    }

    #[test]
    fn cli_dispatch_handles_commands() {
        let dir = CleanDir::new("cli");
        let db = dir.path().join("bus.db");
        let bus = Bus::open(db).unwrap();
        bus.register_peer(Some("term:cli-1"), Some("cli1"), None, Some("terminal"))
            .unwrap();
        bus.register_peer(Some("term:cli-2"), Some("cli2"), None, Some("terminal"))
            .unwrap();

        let args = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(dispatch_cli(&bus, &args(&["sesiones-bus", "list", "--json"])), 0);
        assert_eq!(
            dispatch_cli(
                &bus,
                &args(&["sesiones-bus", "send", "term:cli-2", "hola", "mundo", "--from", "term:cli-1"])
            ),
            0
        );
        assert_eq!(
            dispatch_cli(&bus, &args(&["inbox", "--peer", "term:cli-2"])),
            0
        );
        assert_eq!(
            dispatch_cli(&bus, &args(&["sesiones-bus", "thread", "nonexistent"])),
            1
        );
    }

    #[test]
    fn send_does_not_hold_sqlite_write_lock_during_wakeup() {
        use std::sync::mpsc;

        let dir = CleanDir::new("concurrent-send");
        let db = dir.path().join("bus.db");
        let bus_a = Bus::open(db.clone()).unwrap();
        let bus_b = Bus::open(db.clone()).unwrap();

        bus_a
            .register_peer(Some("codex:alpha"), Some("alpha"), None, Some("codex"))
            .unwrap();
        bus_a
            .register_peer(Some("claude:gamma"), Some("gamma"), None, Some("claude"))
            .unwrap();
        bus_a
            .register_peer(Some("opencode:beta"), Some("beta"), None, Some("opencode"))
            .unwrap();

        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let bus_a_clone = bus_a.clone();
        let handle = std::thread::spawn(move || {
            bus_a_clone.send_with_wakeup(
                "claude:gamma",
                "message during blocking wakeup",
                "lock-test",
                Some("codex:alpha"),
                None,
                true,
                4,
                false,
                |_peer, _from, _tid, _hop, _max_h, _body, _want_reply| {
                    entered_tx.send(()).expect("signal entered wakeup");
                    release_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("wakeup release timeout — write lock may be held");
                    (
                        true,
                        "claude-uds".to_string(),
                        "sent via blocking wakeup".to_string(),
                    )
                },
            )
        });

        // Wait until Thread A is actively inside wakeup I/O (between Transaction 1 and Transaction 2).
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("thread A never entered wakeup");

        // Another Bus::open handle must be able to write concurrently without SQLITE_BUSY.
        let writer = bus_b
            .register_peer(Some("term:writer"), Some("writer"), None, Some("terminal"))
            .expect("concurrent register_peer should not hit SQLITE_BUSY");
        assert_eq!(writer.peer_id, "term:writer");

        let concurrent_send = bus_b
            .send(
                "opencode:beta",
                "concurrent write while wakeup blocked",
                "concurrent",
                Some("term:writer"),
                None,
                true,
                3,
                false,
            )
            .expect("concurrent send should not hit SQLITE_BUSY");
        assert!(concurrent_send.ok);

        // Transaction 1 already committed the message with delivered_ok=0 before wakeup began.
        let mid_inbox = bus_b.inbox(Some("claude:gamma"), false, 10).unwrap();
        assert_eq!(mid_inbox.count, 1);
        assert_eq!(mid_inbox.messages[0].delivered_ok, 0);

        // Unblock Thread A's wakeup so Transaction 2 commits delivery status.
        release_tx.send(()).unwrap();
        let send_res = handle.join().unwrap().unwrap();
        assert!(send_res.ok);
        assert_eq!(send_res.delivered_via.as_deref(), Some("claude-uds"));
        assert_eq!(
            send_res.delivered_info.as_deref(),
            Some("sent via blocking wakeup")
        );

        let tid = send_res.thread_id.unwrap();
        let detail = bus_b.thread(&tid).unwrap();
        let msgs = detail.messages.unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].delivered_ok, 1);
        assert_eq!(msgs[0].delivered_via, "claude-uds");
        assert_eq!(msgs[0].delivered_info, "sent via blocking wakeup");
    }
}
