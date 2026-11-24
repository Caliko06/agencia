//! Background intelligence, ported from agencia.py. Request collectors never run a model.
use crate::shared::{HAIKU_PER_HOUR, awake, cache_dir, home, loops_enabled, now};
use agencia_core::jobs::{self, JobManager};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::LazyLock,
    thread,
    time::{Duration, Instant},
};
mod intelligence;
mod narrator;
mod quota;
mod real;
mod reconcile;
pub use narrator::used_hour;
#[allow(dead_code)]
pub fn plain(key: &str, request: &str, acts: &[String], working: bool) -> String {
    narrator::plain(key, request, acts, working)
}
#[allow(dead_code)]
pub fn recap(id: &str) -> Value {
    narrator::recap(id)
}
pub use intelligence::{consul_load, consult, re, re_plain};
pub use quota::{quota, runway};
pub use real::real_view;
#[allow(dead_code)]
pub fn mark_safe(repos: &mut Value) {
    intelligence::mark_safe(repos)
}
static JOBS: LazyLock<JobManager> = LazyLock::new(|| JobManager::new(crate::shared::root()));
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}
fn n(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(0.)
}
fn cut(x: &str, len: usize) -> String {
    x.chars().take(len).collect()
}
fn sq(x: &str, len: usize) -> String {
    cut(&x.split_whitespace().collect::<Vec<_>>().join(" "), len)
}
fn md(x: &str, len: usize) -> String {
    sq(&x.replace(['*', '`', '#', '>', '_'], ""), len)
}
fn load(p: impl AsRef<Path>) -> Option<Value> {
    let b = fs::read(p).ok()?;
    serde_json::Deserializer::from_slice(&b)
        .into_iter()
        .next()?
        .ok()
}
fn save(p: impl AsRef<Path>, v: &Value) {
    let p = p.as_ref();
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    let tmp = p.with_extension(format!("{}.tmp", std::process::id()));
    if fs::write(&tmp, serde_json::to_vec(v).unwrap_or_default()).is_ok() {
        let _ = fs::rename(tmp, p);
    }
}
fn paths(pattern: &str) -> Vec<PathBuf> {
    glob::glob(pattern)
        .map(|g| g.flatten().collect())
        .unwrap_or_default()
}
fn mtime(p: impl AsRef<Path>) -> f64 {
    fs::metadata(p)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.)
}
fn tail(p: impl AsRef<Path>, size: u64) -> String {
    let Ok(mut f) = fs::File::open(p) else {
        return String::new();
    };
    let size0 = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(size0.saturating_sub(size)));
    let mut b = Vec::new();
    let _ = f.take(size).read_to_end(&mut b);
    String::from_utf8_lossy(&b).into_owned()
}
fn oneshot() -> PathBuf {
    let p = cache_dir().join("oneshot");
    let _ = fs::create_dir_all(&p);
    p
}
/// Capture through files so verbose children cannot deadlock on a full pipe. Kill and reap on timeout.
fn command(mut c: Command, timeout: u64) -> Result<Output, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = oneshot();
    let name = format!(
        "call-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let out = dir.join(format!("{name}.out"));
    let err = dir.join(format!("{name}.err"));
    let result = (|| {
        c.stdin(Stdio::null())
            .stdout(fs::File::create(&out).map_err(|e| e.to_string())?)
            .stderr(fs::File::create(&err).map_err(|e| e.to_string())?);
        let mut child = c.spawn().map_err(|e| e.to_string())?;
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Ok(Output {
                        status,
                        stdout: fs::read(&out).unwrap_or_default(),
                        stderr: fs::read(&err).unwrap_or_default(),
                    });
                }
                Ok(None) if start.elapsed().as_secs() < timeout => {
                    thread::sleep(Duration::from_millis(100))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("subproceso excedió {timeout} s"));
                }
            }
        }
    })();
    let _ = fs::remove_file(out);
    let _ = fs::remove_file(err);
    result
}
fn model(
    model: &str,
    prompt: &str,
    account: u32,
    tools: &str,
    timeout: u64,
) -> Result<String, String> {
    if !loops_enabled() {
        return Err("modelos desactivados (AGENCIA_NO_LOOPS)".into());
    }
    let mut c = Command::new("claude");
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("CLAUDE")
            || k.to_string_lossy().starts_with("ANTHROPIC_AUTH")
        {
            c.env_remove(k);
        }
    }
    if account != 0 && account != 3 {
        let mapping =
            load(PathBuf::from(home()).join(".config/claude-acc/accounts.json")).unwrap_or(json!({}));
        let profile = mapping
            .get(account.to_string())
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string())
            })
            .unwrap_or_else(|| account.to_string());
        let config = if account == 1 {
            PathBuf::from(home()).join(".claude-profiles/3")
        } else {
            PathBuf::from(home()).join(format!(".claude-profiles/{profile}"))
        };
        c.env("CLAUDE_CONFIG_DIR", config);
    }
    c.current_dir(oneshot())
        .args([
            "-p",
            "--model",
            model,
            "--tools",
            tools,
            "--no-session-persistence",
            "--disable-slash-commands",
            "--setting-sources",
            "",
            "--strict-mcp-config",
            prompt,
        ]);
    if !tools.is_empty() {
        c.args([
            "--allowedTools",
            "Read Glob Grep Bash(git log:*) Bash(git show:*)",
            "--permission-mode",
            "dontAsk",
        ]);
    }
    let out = command(c, timeout)?;
    if model == "haiku" {
        narrator::record_call();
    }
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().into())
    } else {
        Err(sq(
            &String::from_utf8_lossy(if out.stderr.is_empty() {
                &out.stdout
            } else {
                &out.stderr
            }),
            200,
        ))
    }
}
fn jsonish(x: &str) -> Value {
    for (i, c) in x.char_indices() {
        if c == '[' || c == '{' {
            if let Some(Ok(v)) = serde_json::Deserializer::from_str(&x[i..])
                .into_iter::<Value>()
                .next()
            {
                return v;
            }
        }
    }
    Value::Null
}
fn git(repo: &str, args: &[&str]) -> String {
    let mut c = Command::new("git");
    c.args(["-C", repo]).args(args);
    command(c, 30)
        .map(|o| String::from_utf8_lossy(&o.stdout).trim_end().into())
        .unwrap_or_default()
}
fn periodic(name: &str, delay: u64, every: u64, f: fn()) {
    let _ = thread::Builder::new().name(name.into()).spawn(move || {
        thread::sleep(Duration::from_secs(delay));
        loop {
            f();
            thread::sleep(Duration::from_secs(every));
        }
    });
}
pub fn spawn_all() {
    use std::sync::Once;
    static START: Once = Once::new();
    if !loops_enabled() {
        return;
    }
    START.call_once(|| {
        periodic("narrator-supervisor", 0, 5, narrator::supervise);
        periodic("janitor", 120, 3600, intelligence::sweep);
        periodic("safe-judge", 90, 120, intelligence::safe_tick);
        periodic("real", 15, 30, real::tick);
        periodic("real-views", 8, 15, real::refresh_views);
        if std::env::var_os("AGENCIA_RECONCILE").is_some() {
            periodic("reconcile", 420, 420, reconcile::tick);
        }
        let _ = thread::Builder::new()
            .name("avance".into())
            .spawn(crate::avance::run_loop);
    });
}
