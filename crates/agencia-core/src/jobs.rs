use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex as StdMutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use regex::Regex;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::procs::{pid_alive, proc_args};
use crate::types::{CoreError, Result, agencia_root, hex_encode, home_dir, now_unix_secs, uuid_v4};

pub const BEC_MODEL: &str = "opencode-go/deepseek-v4.1-flash";
pub const BEC_GLM: &str = "opencode-go/glm-5.3-flash";
pub const BEC_SECS: u64 = 3 * 3600;
pub const BEC_STEPS: u32 = 300;
pub const CLA_MODEL: &str = "sonnet";

fn config_string(env_key: &str, json_key: &str) -> Option<String> {
    if let Ok(v) = std::env::var(env_key) {
        let t = v.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    let path = home_dir().join(".config").join("agencia").join("config.json");
    let raw = fs::read_to_string(path).ok()?;
    let val: Value = serde_json::from_str(&raw).ok()?;
    val.get(json_key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn default_bec_model() -> String {
    config_string("AGENCIA_BEC_MODEL", "bec_model").unwrap_or_else(|| BEC_MODEL.to_string())
}

fn default_cla_model() -> String {
    config_string("AGENCIA_CLA_MODEL", "cla_model").unwrap_or_else(|| CLA_MODEL.to_string())
}
pub const CODEX_STOP: f64 = 92.0;
pub const KEEP_DAYS: f64 = 14.0;
pub const REAL_BEC: usize = 60;
pub const LIMIT_CDX: usize = 4;
pub const LIMIT_BEC: usize = 12;
pub const LIMIT_CLA: usize = 4;
pub const TIMEOUT_SECS: u64 = 3 * 3600;
pub const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

pub const REAL_NOTE: &str = "[Agencia Real] Puedes lanzar agentes de cualquier proveedor: `bec \"pedido EN UNA LÍNEA: ...\"`, `cdx \"pedido\"`, `cla \"pedido\"` (un Claude). Lo que lances desde aquí se ve colgado de ti en la Agencia. Si eres Codex, también puedes repartir trabajo entre tus propios subagentes de Codex con tu herramienta nativa de subagentes (spawn_agent), sin pasar por `cdx`; usa `bec` para lecturas amplias y baratas. Trabaja solo en el objetivo que te dieron.\n\n";

const DOC_HELP: &str = r#"cdx, bec and cla: background jobs for Codex, OpenCode interns (DeepSeek Flash) and Claude workers (`claude -p`). One job = one fresh context.
Every job is a file ~/Agencia/jobs/<id>.json that the Agencia page reads; there is no daemon, queue db or tmux.
  cdx [home] [--repo DIR] [--de WHO] [--model M] [--effort E] "request"   -> prints the job id
  bec [--glm] [--repo DIR] [--de WHO] [--linea TEXT] [--secs N] [--steps N] "request ... EN UNA LINEA: what it will do"  -> prints the job id
    --glm: run on GLM 5.3 Flash instead of DeepSeek Flash (same as --model opencode-go/glm-5.3-flash)
  cla [--repo DIR] [--de WHO] [--model M] "request"   -> a Claude worker on this window's account (else the current one), Sonnet unless --model
  Anyone can launch: a Claude, Antigravity or Codex window, or a job itself. A job launched from inside a job hangs below it in the Agencia.
    --secs N: hard deadline, the intern is killed at N s (default 10800, 30..10800); --steps N: OpenCode agent step cap, at the cap it must summarize done and remaining (default 300, 1..1000)
  cdx|bec|cla wait ID [secs] | res ID | ls
  slice NAME [REPO] -> one worktree and branch for a large slice, prints its path | real-fin -> ends the live Agencia Real run of the repo you are in
A Codex job ends with its thread closed and archived; a correction is a new job."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: String,
    pub kind: String,
    pub ts: f64,
    #[serde(default)]
    pub home: String,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub de: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub effort: String,
    pub text: String,
    #[serde(default)]
    pub brief: String,
    #[serde(default)]
    pub secs: u64,
    #[serde(default)]
    pub steps_max: u32,
    pub status: String,
    #[serde(default)]
    pub reply: String,
    #[serde(default)]
    pub started: f64,
    #[serde(default)]
    pub ended: f64,
    #[serde(default)]
    pub updated: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i32>,
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub steps: u32,
    #[serde(default)]
    pub acts: Vec<String>,
    #[serde(default)]
    pub archived: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LaunchReq {
    pub kind: String,
    pub text: String,
    #[serde(default)]
    pub home: String,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub de: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub brief: Option<String>,
    #[serde(default)]
    pub secs: Option<u64>,
    #[serde(default)]
    pub steps: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotRecord {
    pub path: String,
    #[serde(default)]
    pub caption: String,
    pub ts: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealRun {
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub until: f64,
    #[serde(default)]
    pub at: Option<Value>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

pub struct JobManager {
    root: PathBuf,
    cache: Mutex<HashMap<PathBuf, (u64, u64, JobRecord)>>,
}

pub fn resolve_agencia_root() -> PathBuf {
    resolve_agencia_root_with(|k| std::env::var_os(k))
}

fn resolve_agencia_root_with<F>(get_env: F) -> PathBuf
where
    F: Fn(&str) -> Option<std::ffi::OsString>,
{
    for key in ["AGENCIA_DIR", "AGENCIA_ROOT"] {
        if let Some(v) = get_env(key)
            && !v.is_empty()
        {
            return PathBuf::from(v);
        }
    }
    agencia_root()
}

fn is_isolated_with<F>(get_env: F) -> bool
where
    F: Fn(&str) -> Option<std::ffi::OsString>,
{
    ["AGENCIA_DIR", "AGENCIA_ROOT"]
        .iter()
        .any(|k| get_env(k).is_some_and(|v| !v.is_empty()))
}

pub fn discover_codex_homes() -> HashMap<String, PathBuf> {
    discover_codex_homes_in(&home_dir())
}

pub fn discover_codex_homes_in(home: &Path) -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    let Ok(entries) = fs::read_dir(home) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        if !name.starts_with(".codex") {
            continue;
        }
        let valid = path.join("auth.json").exists() || path.join("sessions").is_dir();
        if !valid {
            continue;
        }
        if name == ".codex" {
            out.insert("default".to_string(), path);
        } else if let Some(suffix) = name.strip_prefix(".codex-")
            && !suffix.is_empty()
        {
            out.insert(suffix.to_string(), path);
        }
    }
    out
}

pub fn discover_claude_profiles() -> Vec<(u32, PathBuf)> {
    discover_claude_profiles_in(&home_dir())
}

pub fn discover_claude_profiles_in(home: &Path) -> Vec<(u32, PathBuf)> {
    let mut map: BTreeMap<u32, PathBuf> = BTreeMap::new();
    let default_dir = home.join(".claude");
    if default_dir.is_dir() {
        map.insert(1, default_dir);
    }

    let profiles_dir = home.join(".claude-profiles");
    if let Ok(entries) = fs::read_dir(&profiles_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            if let Some(name) = entry.file_name().to_str()
                && let Ok(n) = name.parse::<u32>()
                && (n >= 2 || !map.contains_key(&n))
            {
                map.insert(n, p);
            }
        }
    }

    let accounts_path = home.join(".config").join("claude-acc").join("accounts.json");
    if let Ok(raw) = fs::read_to_string(&accounts_path)
        && let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&raw)
    {
        let mut logical_keys = HashSet::new();
        let mut phys_targets = HashSet::new();
        let mut mappings: Vec<(u32, String)> = Vec::new();
        for (k, v) in obj {
            let Ok(logical_n) = k.parse::<u32>() else {
                continue;
            };
            let phys_str = match v {
                Value::String(s) => s,
                Value::Number(num) => num.to_string(),
                _ => continue,
            };
            logical_keys.insert(logical_n);
            if let Ok(pn) = phys_str.parse::<u32>() {
                phys_targets.insert(pn);
            }
            mappings.push((logical_n, phys_str));
        }
        for pn in phys_targets {
            if !logical_keys.contains(&pn) {
                map.remove(&pn);
            }
        }
        for (logical_n, phys_str) in mappings {
            let target = profiles_dir.join(&phys_str);
            if target.is_dir() {
                map.insert(logical_n, target);
            }
        }
    }

    let current_path = home.join(".cache").join("claude-acc").join("current");
    if let Ok(cur_raw) = fs::read_to_string(&current_path)
        && let Ok(cur) = cur_raw.trim().parse::<u32>()
        && let std::collections::btree_map::Entry::Vacant(e) = map.entry(cur)
    {
        let candidate = if cur == 1 {
            home.join(".claude")
        } else {
            profiles_dir.join(cur.to_string())
        };
        if candidate.is_dir() {
            e.insert(candidate);
        }
    }

    map.into_iter().collect()
}

pub fn active_claude_config_dir_in(home: &Path) -> Option<PathBuf> {
    let current_path = home.join(".cache").join("claude-acc").join("current");
    let raw = fs::read_to_string(current_path).ok()?;
    let n = raw.trim().parse::<u32>().ok()?;
    // claude-acc / ccp (when installed) swaps logical 1 <-> 3: logical account 3 lives in ~/.claude
    // with CLAUDE_CONFIG_DIR unset so macOS Keychain "Claude Code-credentials" matches.
    let has_claude_acc = home.join(".config/claude-acc/accounts.json").exists()
        || home.join(".local/bin/ccp").exists();
    if has_claude_acc {
        if n == 3 {
            return None;
        }
        if n == 1 {
            let p3 = home.join(".claude-profiles").join("3");
            return p3.is_dir().then_some(p3);
        }
    }
    let profiles = discover_claude_profiles_in(home);
    if let Some((_, dir)) = profiles.into_iter().find(|(id, _)| *id == n) {
        if dir != home.join(".claude") {
            return Some(dir);
        }
        return None;
    }
    let fallback = home.join(".claude-profiles").join(n.to_string());
    if fallback.is_dir() {
        Some(fallback)
    } else {
        None
    }
}

pub fn runner_alive(job: &JobRecord) -> bool {
    if job.id.is_empty() {
        return false;
    }
    let Some(pid) = job.pid else {
        return false;
    };
    if pid <= 0 || !pid_alive(pid) {
        return false;
    }
    proc_args(pid).is_some_and(|args| args.contains(&job.id))
}

fn limit_for(kind: &str) -> usize {
    match kind {
        "cdx" => LIMIT_CDX,
        "bec" => LIMIT_BEC,
        "cla" => LIMIT_CLA,
        _ => 1,
    }
}

fn real_file_in(home: &Path) -> PathBuf {
    home.join(".claude")
        .join("spend-guard")
        .join("agencia-real.json")
}

pub fn real_runs_from(path: &Path, live: bool) -> HashMap<String, RealRun> {
    let Ok(raw) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(val) = serde_json::from_str::<Value>(&raw) else {
        return HashMap::new();
    };
    let Some(obj) = val.as_object() else {
        return HashMap::new();
    };

    let mut entries: HashMap<String, Value> = HashMap::new();
    if obj.contains_key("until") {
        let repo_key = obj
            .get("repo")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        entries.insert(repo_key, val);
    } else {
        for (k, v) in obj {
            entries.insert(k.clone(), v.clone());
        }
    }

    let now = now_unix_secs();
    let mut out = HashMap::new();
    for (k, v) in entries {
        let Ok(mut run) = serde_json::from_value::<RealRun>(v) else {
            continue;
        };
        if run.repo.is_empty() {
            run.repo = k.clone();
        }
        if !live || now < run.until {
            out.insert(k, run);
        }
    }
    out
}

pub fn real_runs(live: bool) -> HashMap<String, RealRun> {
    real_runs_from(&real_file_in(&home_dir()), live)
}

fn real_in(repo: &Path, runs: &HashMap<String, RealRun>) -> Option<RealRun> {
    let p = fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf());
    let p_str = p.to_string_lossy();
    for (k, r) in runs {
        if k.is_empty() {
            continue;
        }
        let k_canon = fs::canonicalize(k).unwrap_or_else(|_| PathBuf::from(k));
        let ks = k_canon.to_string_lossy();
        if p_str == ks.as_ref()
            || p_str.starts_with(&format!("{ks}/"))
            || p_str.starts_with(&format!("{ks}-wt/"))
        {
            return Some(r.clone());
        }
    }
    None
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

fn tail_chars(s: &str, max_chars: usize) -> String {
    let total = s.chars().count();
    if total <= max_chars {
        s.to_string()
    } else {
        s.chars().skip(total - max_chars).collect()
    }
}

fn brief_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?im)^\s*(?:EN UNA L[IÍ]NEA|UNA L[IÍ]NEA|BRIEF)\s*[:：]\s*(.+?)\s*$")
            .expect("valid brief regex")
    })
}

fn brief_opener_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^Repo \S+?\.?(\s+|$)").expect("valid opener regex"))
}

fn brief_stale_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^Repo \S+?\.?$").expect("valid stale brief regex"))
}

fn heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*#+\s*").expect("valid heading regex"))
}

fn sentence_split_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[.;]\s+").expect("valid split regex"))
}

pub fn brief_of(text: &str) -> String {
    let mut last_match: Option<String> = None;
    for cap in brief_line_re().captures_iter(text) {
        if let Some(m) = cap.get(1) {
            last_match = Some(m.as_str().trim().to_string());
        }
    }
    if let Some(m) = last_match {
        return truncate_chars(&m, 140);
    }

    for line in text.lines() {
        let l = heading_re().replace(line, "");
        let l = l.trim().trim_matches(|c| c == '[' || c == ']').trim();
        let l = brief_opener_re().replace(l, "");
        let l = l.as_ref();
        if l.chars().count() > 8 {
            let first = sentence_split_re().split(l).next().unwrap_or(l);
            return truncate_chars(first, 140);
        }
    }
    String::new()
}

pub fn caller(label: Option<&str>) -> String {
    let ag_job = std::env::var("AGENCIA_JOB")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|id| format!("job:{id}"));
    let claude_sid = std::env::var("CLAUDE_CODE_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty());
    let agy_cid = std::env::var("ANTIGRAVITY_CONVERSATION_ID")
        .ok()
        .filter(|s| !s.is_empty());
    let sid = ag_job.or(claude_sid).or(agy_cid).unwrap_or_default();

    if let Some(lbl) = label
        && !lbl.is_empty()
        && (lbl.starts_with('_') || sid.is_empty())
    {
        return lbl.to_string();
    }
    if sid.is_empty() {
        "owner".to_string()
    } else {
        sid
    }
}

fn local_hhmmss() -> String {
    let now = unsafe { libc::time(std::ptr::null_mut()) } as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&now, &mut tm);
    }
    format!("{:02}{:02}{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}

fn random_hex_4() -> String {
    let mut bytes = [0u8; 2];
    if getrandom::fill(&mut bytes).is_err()
        && let Ok(mut f) = File::open("/dev/urandom")
    {
        let _ = f.read_exact(&mut bytes);
    }
    hex_encode(&bytes)
}

fn expand_tilde(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if s == "~" {
        return home_dir();
    }
    if let Some(rest) = s.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    p.to_path_buf()
}

fn shots_owner_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?:job|claude):[A-Za-z0-9_-]{1,128}$").expect("valid owner regex")
    })
}

fn slice_name_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[a-z0-9][a-z0-9-]{0,40}$").expect("valid slice name regex"))
}

fn rate_limit_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#""primary":\{"used_percent":([\d.]+),"window_minutes":300,"resets_at":(\d+)"#)
            .expect("valid rate limit regex")
    })
}

pub fn codex_5h(home_name: &str) -> f64 {
    let homes = discover_codex_homes();
    let Some(base) = homes.get(home_name) else {
        return 0.0;
    };
    codex_5h_at(base)
}

pub fn codex_5h_at(codex_dir: &Path) -> f64 {
    let mut files: Vec<(u64, PathBuf)> = Vec::new();
    let sessions_dir = codex_dir.join("sessions");
    if sessions_dir.is_dir() {
        for entry in walkdir::WalkDir::new(&sessions_dir)
            .min_depth(4)
            .max_depth(4)
            .into_iter()
            .flatten()
        {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("jsonl")
                && let Ok(meta) = entry.metadata()
            {
                let mtime = (meta.mtime() as u64)
                    .saturating_mul(1_000_000_000)
                    .saturating_add(meta.mtime_nsec() as u64);
                files.push((mtime, p.to_path_buf()));
            }
        }
    }
    let arch_dir = codex_dir.join("archived_sessions");
    if let Ok(entries) = fs::read_dir(&arch_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("jsonl")
                && let Ok(meta) = entry.metadata()
            {
                let mtime = (meta.mtime() as u64)
                    .saturating_mul(1_000_000_000)
                    .saturating_add(meta.mtime_nsec() as u64);
                files.push((mtime, p));
            }
        }
    }

    files.sort_by_key(|(m, _)| *m);
    let newest = files.iter().rev().take(3);
    let now = now_unix_secs();
    let re = rate_limit_re();

    for (_, path) in newest {
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for line in content.lines().rev() {
            if let Some(caps) = re.captures(line) {
                let used = caps
                    .get(1)
                    .and_then(|m| m.as_str().parse::<f64>().ok())
                    .unwrap_or(0.0);
                let resets_at = caps
                    .get(2)
                    .and_then(|m| m.as_str().parse::<f64>().ok())
                    .unwrap_or(0.0);
                return if resets_at <= now { 0.0 } else { used };
            }
        }
    }
    0.0
}

pub fn slice_new(name: &str, repo: Option<&Path>) -> Result<PathBuf> {
    if !slice_name_re().is_match(name) {
        return Err(CoreError::InvalidInput(
            "nombre inválido: minúsculas, números y guiones".to_string(),
        ));
    }
    let cwd = match repo {
        Some(r) => r.to_path_buf(),
        None => std::env::current_dir()?,
    };
    let out = Command::new("git")
        .arg("-C")
        .arg(&cwd)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()?;
    let common = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if common.is_empty() || !out.status.success() {
        return Err(CoreError::InvalidInput("no es un repo git".to_string()));
    }
    let root = Path::new(&common)
        .parent()
        .ok_or_else(|| CoreError::InvalidInput("no es un repo git".to_string()))?;
    let wt_parent = PathBuf::from(format!("{}-wt", root.display()));
    let wt = wt_parent.join(name);
    if wt.exists() {
        return Ok(wt);
    }

    let branch = format!("slice/{name}");
    let add_out = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("worktree")
        .arg("add")
        .arg("-b")
        .arg(&branch)
        .arg(&wt)
        .arg("HEAD")
        .output()?;
    if !add_out.status.success() {
        return Err(CoreError::External(
            String::from_utf8_lossy(&add_out.stderr).trim().to_string(),
        ));
    }

    let ignored = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "-z",
        ])
        .output()?;
    if ignored.status.success() {
        for raw in ignored.stdout.split(|&b| b == 0) {
            let p = String::from_utf8_lossy(raw);
            let p = p.trim_end_matches('/');
            if p.is_empty() {
                continue;
            }
            let base = Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if matches!(
                base,
                "out" | "runs" | "shots" | "__pycache__" | ".DS_Store" | ".unlock"
            ) {
                continue;
            }
            let dst = wt.join(p);
            if let Some(parent) = dst.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let src = root.join(p);
            let _ = Command::new("cp").arg("-cR").arg(&src).arg(&dst).output();
        }
    }

    Ok(wt)
}

pub fn real_fin(cwd: Option<&Path>) -> Result<String> {
    let dir = match cwd {
        Some(c) => c.to_path_buf(),
        None => std::env::current_dir()?,
    };
    let out = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()?;
    let common = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let repo = if !common.is_empty() && out.status.success() {
        Path::new(&common)
            .parent()
            .and_then(|p| fs::canonicalize(p).ok())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        String::new()
    };

    let real_f = real_file_in(&home_dir());
    let mut runs = real_runs_from(&real_f, false);
    if !runs.contains_key(&repo) {
        return Ok("no hay corrida activa en este repo".to_string());
    }
    runs.remove(&repo);
    if runs.is_empty() {
        let _ = fs::remove_file(&real_f);
    } else {
        if let Some(parent) = real_f.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string(&runs)?;
        let tmp = real_f.with_extension(format!("json.{}.tmp", std::process::id()));
        fs::write(&tmp, data)?;
        fs::rename(&tmp, &real_f)?;
    }
    Ok("Agencia Real terminada".to_string())
}

struct FlockGuard {
    _file: File,
}

impl FlockGuard {
    fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self { _file: file })
    }
}

struct ChildTimer {
    done: Arc<AtomicBool>,
    pair: Arc<(StdMutex<bool>, Condvar)>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl ChildTimer {
    fn start<F>(secs: u64, on_expire: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        let done = Arc::new(AtomicBool::new(false));
        let pair = Arc::new((StdMutex::new(false), Condvar::new()));
        let done_c = Arc::clone(&done);
        let pair_c = Arc::clone(&pair);
        let handle = std::thread::spawn(move || {
            let (lock, cvar) = &*pair_c;
            let mut finished = lock.lock().unwrap_or_else(|e| e.into_inner());
            let deadline = Instant::now() + Duration::from_secs(secs);
            while !*finished && !done_c.load(Ordering::SeqCst) {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                let (guard, wait_res) = cvar
                    .wait_timeout(finished, deadline - now)
                    .unwrap_or_else(|e| e.into_inner());
                finished = guard;
                if wait_res.timed_out() {
                    break;
                }
            }
            if !*finished && !done_c.load(Ordering::SeqCst) {
                on_expire();
            }
        });
        Self {
            done,
            pair,
            handle: Some(handle),
        }
    }

    fn cancel(mut self) {
        self.done.store(true, Ordering::SeqCst);
        let (lock, cvar) = &*self.pair;
        if let Ok(mut finished) = lock.lock() {
            *finished = true;
            cvar.notify_all();
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for ChildTimer {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
        let (lock, cvar) = &*self.pair;
        if let Ok(mut finished) = lock.lock() {
            *finished = true;
            cvar.notify_all();
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl JobManager {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn jobs_dir(&self) -> PathBuf {
        self.root.join("jobs")
    }

    fn job_path(&self, id: &str) -> PathBuf {
        self.jobs_dir().join(format!("{id}.json"))
    }

    fn load_from_path(path: &Path) -> Option<JobRecord> {
        let raw = fs::read_to_string(path).ok()?;
        let mut job: JobRecord = serde_json::from_str(&raw).ok()?;
        if brief_stale_re().is_match(&job.brief) {
            job.brief = brief_of(&job.text);
        }
        Some(job)
    }

    pub fn load(&self, id: &str) -> Option<JobRecord> {
        let clean = id.strip_prefix("job:").unwrap_or(id);
        Self::load_from_path(&self.job_path(clean))
    }

    pub fn save(&self, job: &JobRecord) -> Result<()> {
        let dir = self.jobs_dir();
        fs::create_dir_all(&dir)?;
        let dest = self.job_path(&job.id);
        let tid = format!("{:?}", std::thread::current().id())
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>();
        let tmp = dir.join(format!("{}.json.{}.{}.tmp", job.id, std::process::id(), tid));
        let json_str = serde_json::to_string_pretty(job)?;
        {
            let mut f = File::create(&tmp)?;
            f.write_all(json_str.as_bytes())?;
            f.flush()?;
        }
        if let Err(e) = fs::rename(&tmp, &dest) {
            let _ = fs::remove_file(&tmp);
            return Err(e.into());
        }
        if let Ok(meta) = fs::metadata(&dest) {
            let mtime_ns = (meta.mtime() as u64)
                .saturating_mul(1_000_000_000)
                .saturating_add(meta.mtime_nsec() as u64);
            self.cache
                .lock()
                .insert(dest, (mtime_ns, meta.size(), job.clone()));
        }
        Ok(())
    }

    pub fn list_jobs(&self) -> Vec<JobRecord> {
        self.list_jobs_by_kind(None)
    }

    pub fn list_jobs_by_kind(&self, kind: Option<&str>) -> Vec<JobRecord> {
        let mut out = Vec::new();
        let dir = self.jobs_dir();
        {
            let mut cache = self.cache.lock();
            let mut seen = HashSet::new();
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let Some(name_str) = name.to_str() else {
                        continue;
                    };
                    if !name_str.ends_with(".json") || name_str.ends_with(".shots.json") {
                        continue;
                    }
                    let Ok(meta) = entry.metadata() else {
                        continue;
                    };
                    let mtime_ns = (meta.mtime() as u64)
                        .saturating_mul(1_000_000_000)
                        .saturating_add(meta.mtime_nsec() as u64);
                    let size = meta.size();
                    let path = entry.path();
                    seen.insert(path.clone());

                    let needs_reload = match cache.get(&path) {
                        Some((cm, cs, _)) => *cm != mtime_ns || *cs != size,
                        None => true,
                    };
                    if needs_reload && let Some(rec) = Self::load_from_path(&path) {
                        cache.insert(path.clone(), (mtime_ns, size, rec));
                    }
                    if let Some((_, _, rec)) = cache.get(&path) {
                        out.push(rec.clone());
                    }
                }
            }
            cache.retain(|k, _| seen.contains(k));
        }

        let now = now_unix_secs();
        let mut res = Vec::with_capacity(out.len());
        for mut j in out {
            if (j.status == "q" || j.status == "r") && j.pid.is_some_and(|p| p > 0) && !runner_alive(&j) {
                j.status = "fallo".to_string();
                j.ended = now;
                j.updated = now;
                if j.reply.is_empty() {
                    j.reply = "(el proceso murió sin terminar: apagón, reinicio o kill)".to_string();
                }
                let _ = self.save(&j);
            }
            if kind.is_none_or(|k| j.kind == k) {
                res.push(j);
            }
        }
        res.sort_by(|a, b| a.ts.total_cmp(&b.ts));
        res
    }

    pub fn sweep(&self) {
        let Ok(entries) = fs::read_dir(self.jobs_dir()) else {
            return;
        };
        let now = now_unix_secs();
        let cutoff = KEEP_DAYS * 86400.0;
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file()
                && let Ok(meta) = entry.metadata()
            {
                let mtime = meta.mtime() as f64;
                if now - mtime > cutoff {
                    let _ = fs::remove_file(p);
                }
            }
        }
    }

    pub fn real(&self, repo: &Path) -> Option<RealRun> {
        let runs = real_runs(true);
        real_in(repo, &runs)
    }

    fn real_cache(&self, r: &RealRun) -> Option<Value> {
        let raw = fs::read_to_string(self.root.join("cache").join("real.json")).ok()?;
        let val: Value = serde_json::from_str(&raw).ok()?;
        let obj = val.as_object()?;
        if let Some(entry) = obj.get(&r.repo) {
            Some(entry.clone())
        } else {
            Some(val)
        }
    }

    fn over(&self, j: &JobRecord, r: &RealRun) -> bool {
        let Some(c) = self.real_cache(r) else {
            return false;
        };
        let same_at = c.get("at") == r.at.as_ref();
        if !same_at {
            return false;
        }
        let key = if j.kind == "cdx" {
            format!("codex:{}", j.home)
        } else {
            "go".to_string()
        };
        c.get("over")
            .and_then(|v| v.as_array())
            .is_some_and(|arr| arr.iter().any(|x| x.as_str() == Some(&key)))
    }

    pub fn enqueue(&self, req: LaunchReq) -> Result<JobRecord> {
        let text = req.text.trim().to_string();
        if !matches!(req.kind.as_str(), "cdx" | "bec" | "cla") {
            return Err(CoreError::InvalidInput("kind: cdx, bec o cla".to_string()));
        }

        let homes = discover_codex_homes();
        let home_val = if req.home.trim().is_empty() {
            "default".to_string()
        } else {
            req.home.trim().to_string()
        };
        if req.kind == "cdx" && !homes.contains_key(&home_val) {
            let mut keys: Vec<&str> = homes.keys().map(|s| s.as_str()).collect();
            keys.sort_unstable();
            let avail = if keys.is_empty() {
                "ninguna".to_string()
            } else {
                keys.join(", ")
            };
            return Err(CoreError::InvalidInput(format!("home: {avail}")));
        }

        let effort = req.effort.unwrap_or_default();
        if !matches!(
            effort.as_str(),
            "" | "minimal" | "low" | "medium" | "high" | "xhigh"
        ) {
            return Err(CoreError::InvalidInput(
                "effort: low, medium, high o xhigh".to_string(),
            ));
        }

        if text.is_empty() || text.chars().count() > 12000 {
            return Err(CoreError::InvalidInput(
                "texto vacío o muy largo".to_string(),
            ));
        }

        let secs = req.secs.filter(|&s| s > 0).unwrap_or(BEC_SECS);
        let steps_max = req.steps.filter(|&s| s > 0).unwrap_or(BEC_STEPS);
        if req.kind == "bec" && (!(30..=TIMEOUT_SECS).contains(&secs) || !(1..=1000).contains(&steps_max))
        {
            return Err(CoreError::InvalidInput(format!(
                "--secs 30..{TIMEOUT_SECS}, --steps 1..1000"
            )));
        }

        let repo_raw = match req.repo.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(r) => PathBuf::from(r),
            None => {
                let out = Command::new("git")
                    .args(["rev-parse", "--show-toplevel"])
                    .output();
                match out {
                    Ok(o) if o.status.success() => {
                        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                        if s.is_empty() {
                            std::env::current_dir()?
                        } else {
                            PathBuf::from(s)
                        }
                    }
                    _ => std::env::current_dir()?,
                }
            }
        };
        let expanded = expand_tilde(&repo_raw);
        let repo_canon = fs::canonicalize(&expanded).map_err(|_| {
            CoreError::InvalidInput("repo no existe o no está en tu carpeta".to_string())
        })?;
        let user_home = home_dir();
        let home_canon = fs::canonicalize(&user_home).unwrap_or(user_home);
        let root_canon = fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone());
        let isolated = is_isolated_with(|k| std::env::var_os(k));
        if !repo_canon.is_dir()
            || (!isolated
                && !repo_canon.starts_with(&home_canon)
                && !repo_canon.starts_with(&root_canon))
        {
            return Err(CoreError::InvalidInput(
                "repo no existe o no está en tu carpeta".to_string(),
            ));
        }

        let jid = format!("{}{}", local_hhmmss(), random_hex_4());
        self.sweep();

        let model = match req.model.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(m) => m.to_string(),
            None if req.kind == "bec" => default_bec_model(),
            None if req.kind == "cla" => default_cla_model(),
            None => String::new(),
        };
        let brief = match req.brief.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(b) => b.to_string(),
            None => brief_of(&text),
        };
        let de = req
            .de
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("owner")
            .to_string();

        let job = JobRecord {
            id: jid.clone(),
            kind: req.kind.clone(),
            ts: now_unix_secs(),
            home: if req.kind == "cdx" {
                home_val
            } else {
                String::new()
            },
            repo: repo_canon.to_string_lossy().into_owned(),
            de,
            model,
            effort,
            text,
            brief,
            secs,
            steps_max,
            status: "q".to_string(),
            reply: String::new(),
            started: 0.0,
            ended: 0.0,
            updated: 0.0,
            pid: None,
            thread: String::new(),
            steps: 0,
            acts: Vec::new(),
            archived: false,
        };
        self.save(&job)?;

        let exe = std::env::current_exe()?;
        let exe = fs::canonicalize(&exe).unwrap_or(exe);
        fs::create_dir_all(self.jobs_dir())?;
        let err_path = self.jobs_dir().join(format!("{jid}.err"));
        let err_file = File::create(err_path)?;

        let mut child = {
            let mut cmd = Command::new(&exe);
            cmd.arg("_run")
                .arg(&jid)
                .current_dir(&repo_canon)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::from(err_file))
                .env("AGENCIA_DIR", &self.root)
                .env("AGENCIA_ROOT", &self.root);
            unsafe {
                cmd.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            cmd.spawn()?
            // `cmd` (and parent's `err_file` handle inside Stdio) is dropped here — zero FD leak.
        };
        std::thread::spawn(move || {
            let _ = child.wait();
        });

        Ok(job)
    }

    fn slot(&self, j: &mut JobRecord) -> Result<()> {
        let slot_path = self.jobs_dir().join(".slot");
        loop {
            {
                let _lock = FlockGuard::acquire(&slot_path)?;
                let now = now_unix_secs();
                let running = self
                    .list_jobs_by_kind(Some(&j.kind))
                    .into_iter()
                    .filter(|x| {
                        x.status == "r"
                            && x.home == j.home
                            && (now - x.started) < (TIMEOUT_SECS as f64)
                    })
                    .count();
                let limit = if j.kind == "bec" && self.real(Path::new(&j.repo)).is_some() {
                    REAL_BEC
                } else {
                    limit_for(&j.kind)
                };
                if running < limit {
                    j.status = "r".to_string();
                    j.started = now;
                    self.save(j)?;
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    }

    fn act(&self, j: &mut JobRecord, text: &str) {
        let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let item = truncate_chars(&clean, 200);
        j.acts.push(item);
        if j.acts.len() > 8 {
            let excess = j.acts.len() - 8;
            j.acts.drain(0..excess);
        }
        j.steps = j.steps.saturating_add(1);
        j.updated = now_unix_secs();
        let _ = self.save(j);
    }

    pub fn archive_threads(&self, home: &str, threads: &[String], method: &str) -> Vec<String> {
        let threads: Vec<&String> = threads.iter().filter(|t| !t.is_empty()).collect();
        if threads.is_empty() {
            return Vec::new();
        }
        let homes = discover_codex_homes();
        let Some(codex_home) = homes.get(home) else {
            return Vec::new();
        };

        let Ok(mut child) = Command::new("codex")
            .arg("app-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env("CODEX_HOME", codex_home)
            .spawn()
        else {
            return Vec::new();
        };

        let Some(mut stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Vec::new();
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Vec::new();
        };

        let (tx, rx) = mpsc::channel::<Value>();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(|l| l.ok()) {
                if let Ok(v) = serde_json::from_str::<Value>(&line)
                    && tx.send(v).is_err()
                {
                    break;
                }
            }
        });

        let rpc = |stdin: &mut std::process::ChildStdin,
                   rx: &mpsc::Receiver<Value>,
                   id: i64,
                   m: &str,
                   prm: Value|
         -> Option<Value> {
            let req = json!({"id": id, "method": m, "params": prm}).to_string() + "\n";
            if stdin.write_all(req.as_bytes()).is_err() || stdin.flush().is_err() {
                return None;
            }
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let rem = deadline.saturating_duration_since(Instant::now());
                if rem.is_zero() {
                    return None;
                }
                match rx.recv_timeout(rem) {
                    Ok(v) => {
                        if v.get("id").and_then(|x| x.as_i64()) == Some(id) {
                            return Some(v);
                        }
                    }
                    Err(_) => return None,
                }
            }
        };

        let mut done = Vec::new();
        if rpc(
            &mut stdin,
            &rx,
            1,
            "initialize",
            json!({"clientInfo": {"name": "agencia", "version": "1"}}),
        )
        .is_some()
        {
            let init_notif = json!({"method": "initialized"}).to_string() + "\n";
            if stdin.write_all(init_notif.as_bytes()).is_ok() && stdin.flush().is_ok() {
                for (i, t) in threads.iter().enumerate() {
                    let id = 2 + i as i64;
                    if let Some(resp) = rpc(&mut stdin, &rx, id, method, json!({"threadId": t}))
                        && resp.get("error").is_none()
                    {
                        done.push((*t).clone());
                    }
                }
            }
        }

        let _ = child.kill();
        let _ = child.wait();
        done
    }

    fn run_cdx(&self, j: &mut JobRecord) -> Result<(Vec<String>, String)> {
        let homes = discover_codex_homes();
        let codex_home = homes.get(&j.home).cloned().ok_or_else(|| {
            CoreError::NotFound(format!("Codex home '{}' no encontrado", j.home))
        })?;
        let live = self.real(Path::new(&j.repo)).is_some();
        let tag = format!("[agencia cdx {} de={}] ", j.id, j.de);
        let prompt = format!("{tag}{}{}", if live { REAL_NOTE } else { "" }, j.text);

        let mut cmd = Command::new("codex");
        cmd.args([
            "exec",
            "--json",
            "--skip-git-repo-check",
            "--dangerously-bypass-approvals-and-sandbox",
        ]);
        if !j.model.is_empty() {
            cmd.arg("-m").arg(&j.model);
        }
        if !j.effort.is_empty() {
            cmd.arg("-c")
                .arg(format!("model_reasoning_effort={}", j.effort));
        }
        cmd.arg(&prompt)
            .current_dir(&j.repo)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("CODEX_HOME", &codex_home)
            .env("AGENCIA_JOB", &j.id);

        let mut child = cmd.spawn()?;
        let child_pid = child.id() as libc::pid_t;
        let _timer = ChildTimer::start(TIMEOUT_SECS, move || unsafe {
            libc::kill(child_pid, libc::SIGKILL);
        });

        let stderr_handle = child.stderr.take().map(|mut err_pipe| {
            std::thread::spawn(move || {
                let mut s = String::new();
                let _ = err_pipe.read_to_string(&mut s);
                s
            })
        });

        let mut reply = Vec::new();
        let mut err = String::new();
        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            let mut checked = Instant::now();
            for line in reader.lines().map_while(|l| l.ok()) {
                if checked.elapsed() > Duration::from_secs(30) {
                    checked = Instant::now();
                    let u = codex_5h_at(&codex_home);
                    if u >= CODEX_STOP {
                        err = format!(
                            "auto-stop: Codex {} 5 h window at {:.0} % (>= {CODEX_STOP:.0})",
                            j.home, u
                        );
                        unsafe {
                            libc::kill(child_pid, libc::SIGTERM);
                        }
                        break;
                    }
                }
                let Ok(e) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let t = e.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let it = e.get("item").and_then(|v| v.as_object());
                if t == "thread.started" {
                    if let Some(tid) = e.get("thread_id").and_then(|v| v.as_str()) {
                        j.thread = tid.to_string();
                        let _ = self.save(j);
                    }
                } else if t == "item.completed" {
                    if let Some(obj) = it
                        && obj.get("type").and_then(|v| v.as_str()) == Some("agent_message")
                    {
                        let txt = obj.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        reply.push(txt.to_string());
                        self.act(j, &format!("dice: {txt}"));
                    }
                } else if t == "item.started" {
                    if let Some(obj) = it {
                        let it_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        if it_type != "agent_message" && it_type != "reasoning" {
                            let desc = if let Some(cmd_s) =
                                obj.get("command").and_then(|v| v.as_str()).filter(|s| !s.is_empty())
                            {
                                format!("Bash: {cmd_s}")
                            } else if let Some(ch) = obj
                                .get("changes")
                                .and_then(|v| v.as_array())
                                .filter(|a| !a.is_empty())
                            {
                                let paths: Vec<&str> = ch
                                    .iter()
                                    .map(|c| c.get("path").and_then(|p| p.as_str()).unwrap_or(""))
                                    .collect();
                                format!("Edit: {}", paths.join(", "))
                            } else if !it_type.is_empty() {
                                it_type.to_string()
                            } else {
                                "tool".to_string()
                            };
                            self.act(j, &desc);
                        }
                    }
                } else if t == "error" || t == "turn.failed" {
                    err = truncate_chars(&line, 400);
                }
            }
        }

        let status = child.wait()?;
        let stderr_str = stderr_handle
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        if err.is_empty() && !status.success() {
            err = tail_chars(&stderr_str, 400);
        }
        Ok((reply, err))
    }

    fn load_becario_md(&self) -> String {
        let candidates = [
            self.root.join("becario.md"),
            agencia_root().join("becario.md"),
        ];
        for path in candidates {
            if path.exists()
                && let Ok(content) = fs::read_to_string(&path)
            {
                return content;
            }
        }
        String::new()
    }

    fn run_bec(&self, j: &mut JobRecord) -> Result<(Vec<String>, String)> {
        let home = home_dir();
        let steps_n = if j.steps_max > 0 { j.steps_max } else { BEC_STEPS };
        let agent_name = Self::ensure_becario_agent(&home, steps_n)?;

        let bec_md = self.load_becario_md();
        let live_note = if self.real(Path::new(&j.repo)).is_some() {
            REAL_NOTE
        } else {
            ""
        };
        let de_desc = if j.de == "owner" {
            "el OWNER".to_string()
        } else {
            format!("el OWNER, a través de {}", j.de)
        };
        let prompt = format!(
            "{live_note}{bec_md}\n\n## PEDIDO (de {de_desc}; repo: {})\n{}\n\n## EN UNA LÍNEA\n{}",
            j.repo, j.text, j.brief
        );

        let secs = if j.secs > 0 { j.secs } else { BEC_SECS };
        let opencode = Self::opencode_bin(&home);
        let model = if j.model.is_empty() {
            BEC_MODEL.to_string()
        } else {
            j.model.clone()
        };

        let mut child = Command::new(&opencode)
            .arg("run")
            .arg("-m")
            .arg(&model)
            .arg("--agent")
            .arg(&agent_name)
            .arg("--title")
            .arg(format!("bec-{}", j.id))
            .arg("--auto")
            .arg("--format")
            .arg("json")
            .arg(&prompt)
            .current_dir(&j.repo)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("AGENCIA_JOB", &j.id)
            .spawn()?;

        let late = Arc::new(AtomicBool::new(false));
        let late_c = Arc::clone(&late);
        let child_pid = child.id() as libc::pid_t;
        let jid_c = j.id.clone();
        let timer = ChildTimer::start(secs, move || {
            late_c.store(true, Ordering::SeqCst);
            unsafe {
                libc::kill(child_pid, libc::SIGKILL);
            }
            Self::interrupt_bec(&jid_c);
        });

        let stderr_handle = child.stderr.take().map(|err_pipe| {
            std::thread::spawn(move || {
                let mut err_tail = String::new();
                let reader = BufReader::new(err_pipe);
                for line in reader.lines().map_while(|l| l.ok()) {
                    let tr = line.trim();
                    if !tr.is_empty() {
                        if !err_tail.is_empty() {
                            err_tail.push(' ');
                        }
                        err_tail.push_str(tr);
                        err_tail = tail_chars(&err_tail, 300);
                    }
                }
                err_tail
            })
        });

        let mut texts = Vec::new();
        let mut err = String::new();
        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(|l| l.ok()) {
                let Ok(ev) = serde_json::from_str::<Value>(&line) else {
                    let tr = line.trim();
                    if !tr.is_empty() {
                        if !err.is_empty() {
                            err.push(' ');
                        }
                        err.push_str(tr);
                        err = tail_chars(&err, 300);
                    }
                    continue;
                };
                let pt = ev.get("part").and_then(|v| v.as_object());
                let ev_type = ev.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if ev_type == "text" {
                    if let Some(txt) = pt
                        .and_then(|o| o.get("text"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        texts.push(txt.to_string());
                    }
                } else if ev_type == "tool_use"
                    && let Some(o) = pt
                {
                    let tool = o.get("tool").and_then(|v| v.as_str()).unwrap_or("");
                    let inp = o
                        .get("state")
                        .and_then(|s| s.get("input"))
                        .and_then(|v| v.as_object());
                    let arg = inp
                        .and_then(|i| {
                            ["path", "filePath", "file_path", "command", "pattern"]
                                .iter()
                                .find_map(|k| {
                                    i.get(*k)
                                        .and_then(|v| v.as_str())
                                        .filter(|s| !s.is_empty())
                                        .map(|s| s.to_string())
                                })
                        })
                        .unwrap_or_default();
                    self.act(j, &format!("{tool}: {arg}"));
                }
            }
        }

        let status = child.wait()?;
        timer.cancel();
        let stderr_tail = stderr_handle
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        if err.is_empty() && !stderr_tail.is_empty() {
            err = stderr_tail;
        }

        if late.load(Ordering::SeqCst) {
            let joined = texts.join("\n").trim().to_string();
            let msg = if joined.is_empty() {
                format!("plazo vencido ({secs} s)")
            } else {
                format!("plazo vencido ({secs} s)\n\n{joined}")
            };
            return Ok((vec![msg], "plazo vencido".to_string()));
        }

        let joined = texts.join("\n").trim().to_string();
        let out_reply = if joined.is_empty() {
            Vec::new()
        } else {
            vec![joined]
        };
        let out_err = if status.success() { String::new() } else { err };
        Ok((out_reply, out_err))
    }

    fn run_cla(&self, j: &mut JobRecord) -> Result<(Vec<String>, String)> {
        j.thread = uuid_v4();
        self.save(j)?;

        let live = self.real(Path::new(&j.repo)).is_some();
        let prompt = format!(
            "[agencia cla {} de={}] {}{}",
            j.id,
            j.de,
            if live { REAL_NOTE } else { "" },
            j.text
        );
        let model = if j.model.is_empty() {
            CLA_MODEL.to_string()
        } else {
            j.model.clone()
        };

        let mut cmd = Command::new("claude");
        cmd.args([
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--session-id",
            &j.thread,
            "--permission-mode",
            "bypassPermissions",
            "--model",
            &model,
            &prompt,
        ])
        .current_dir(&j.repo)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear();

        let home = home_dir();
        let mut has_config_dir = false;
        for (k, v) in std::env::vars() {
            if k.starts_with("CLAUDECODE")
                || k.starts_with("CLAUDE_CODE_")
                || k.starts_with("ANTHROPIC_AUTH")
            {
                continue;
            }
            if k == "CLAUDE_CONFIG_DIR" {
                has_config_dir = true;
            }
            cmd.env(k, v);
        }
        if !has_config_dir
            && let Some(dir) = active_claude_config_dir_in(&home)
        {
            cmd.env("CLAUDE_CONFIG_DIR", dir);
        }
        cmd.env("AGENCIA_JOB", &j.id);

        let mut child = cmd.spawn()?;
        let child_pid = child.id() as libc::pid_t;
        let timer = ChildTimer::start(TIMEOUT_SECS, move || unsafe {
            libc::kill(child_pid, libc::SIGKILL);
        });

        let stderr_handle = child.stderr.take().map(|mut err_pipe| {
            std::thread::spawn(move || {
                let mut s = String::new();
                let _ = err_pipe.read_to_string(&mut s);
                s
            })
        });

        let mut reply = Vec::new();
        let mut err = String::new();
        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(|l| l.ok()) {
                let Ok(e) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let t = e.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if t == "system" && e.get("subtype").and_then(|v| v.as_str()) == Some("init") {
                    if let Some(m) = e.get("model").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                        j.model = m.to_string();
                        let _ = self.save(j);
                    }
                } else if t == "assistant" {
                    if let Some(contents) = e
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_array())
                    {
                        for c in contents {
                            let ctype = c.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            if ctype == "tool_use" {
                                let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                let inp = c.get("input").and_then(|v| v.as_object());
                                let arg = inp
                                    .and_then(|i| {
                                        ["description", "command", "file_path", "pattern", "url"]
                                            .iter()
                                            .find_map(|k| {
                                                i.get(*k)
                                                    .and_then(|v| v.as_str())
                                                    .filter(|s| !s.is_empty())
                                                    .map(|s| s.to_string())
                                            })
                                    })
                                    .unwrap_or_default();
                                self.act(j, &format!("{name}: {arg}"));
                            } else if ctype == "text"
                                && let Some(txt) = c.get("text").and_then(|v| v.as_str())
                                && !txt.trim().is_empty()
                            {
                                self.act(j, &format!("dice: {txt}"));
                            }
                        }
                    }
                } else if t == "result" {
                    let is_err = e.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                    let sub = e.get("subtype").and_then(|v| v.as_str()).unwrap_or("");
                    if is_err || sub != "success" {
                        let msg = e
                            .get("result")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                            .unwrap_or(sub);
                        err = truncate_chars(msg, 400);
                    } else if let Some(res) =
                        e.get("result").and_then(|v| v.as_str()).filter(|s| !s.is_empty())
                    {
                        reply.push(res.to_string());
                    }
                }
            }
        }

        let status = child.wait()?;
        timer.cancel();
        let stderr_str = stderr_handle
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        if err.is_empty() && !status.success() {
            err = tail_chars(&stderr_str, 400);
        }
        Ok((reply, err))
    }

    fn opencode_bin(home: &Path) -> PathBuf {
        let preferred = home.join(".opencode").join("bin").join("opencode");
        if preferred.exists() {
            preferred
        } else {
            PathBuf::from("opencode")
        }
    }

    fn ensure_becario_agent(home: &Path, n: u32) -> Result<String> {
        let agent_dir = home.join(".config").join("opencode").join("agents");
        let name = format!("becario-{n}");
        let file_path = agent_dir.join(format!("{name}.md"));
        if !file_path.exists() {
            fs::create_dir_all(&agent_dir)?;
            let body = format!(
                "---\ndescription: Becario con tope de {n} pasos\nmode: primary\nsteps: {n}\n---\nEres un becario del OWNER. Cumple el pedido; al llegar al tope de pasos, resume lo hecho y lo pendiente.\n"
            );
            fs::write(&file_path, body)?;
        }
        Ok(name)
    }

    fn interrupt_bec(jid: &str) {
        let home = home_dir();
        let title = format!("bec-{jid}");
        let db_path = home
            .join(".local")
            .join("share")
            .join("opencode")
            .join("opencode.db");
        let mut sid: Option<String> = None;

        if db_path.exists()
            && let Ok(conn) = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        {
            let _ = conn.busy_timeout(Duration::from_millis(2000));
            if let Ok(found) = conn.query_row(
                "SELECT id FROM session_v2 WHERE title = ?1 ORDER BY time_created DESC LIMIT 1",
                [&title],
                |row| row.get::<_, String>(0),
            ) {
                sid = Some(found);
            }
        }

        let opencode = Self::opencode_bin(&home);
        if sid.is_none()
            && let Ok(out) = Command::new(&opencode)
                .args(["api", "GET", "/api/session"])
                .output()
            && let Ok(val) = serde_json::from_slice::<Value>(&out.stdout)
            && let Some(arr) = val.get("data").and_then(|d| d.as_array())
        {
            sid = arr.iter().find_map(|x| {
                if x.get("title").and_then(|t| t.as_str()) == Some(&title) {
                    x.get("id").and_then(|i| i.as_str()).map(|s| s.to_string())
                } else {
                    None
                }
            });
        }

        if let Some(session_id) = sid {
            let _ = Command::new(&opencode)
                .args([
                    "api",
                    "POST",
                    &format!("/api/session/{session_id}/interrupt"),
                ])
                .output();
        }
    }

    pub fn run_job(&self, id: &str) -> Result<()> {
        let mut j = self
            .load(id)
            .ok_or_else(|| CoreError::NotFound(format!("job {id} no existe")))?;
        j.pid = Some(std::process::id() as i32);
        self.save(&j)?;

        if let Some(live) = self.real(Path::new(&j.repo))
            && self.over(&j, &live)
        {
            let now = now_unix_secs();
            j.status = "fallo".to_string();
            j.ended = now;
            j.updated = now;
            j.reply = "Agencia Real: esta cuenta ya gastó su tope de la corrida".to_string();
            self.save(&j)?;
            return Ok(());
        }

        self.slot(&mut j)?;

        if j.kind == "cdx" {
            let u = codex_5h(&j.home);
            if u >= CODEX_STOP {
                let now = now_unix_secs();
                j.status = "fallo".to_string();
                j.ended = now;
                j.updated = now;
                j.reply = format!(
                    "auto-stop: Codex {} 5 h window at {:.0} % (>= {CODEX_STOP:.0}); wait for its reset",
                    j.home, u
                );
                self.save(&j)?;
                return Ok(());
            }
        }

        let res = match j.kind.as_str() {
            "cdx" => self.run_cdx(&mut j),
            "bec" => self.run_bec(&mut j),
            "cla" => self.run_cla(&mut j),
            other => Err(CoreError::InvalidInput(format!("kind desconocido: {other}"))),
        };

        let (reply, err) = match res {
            Ok(pair) => pair,
            Err(e) => (Vec::new(), e.to_string()),
        };

        let text = truncate_chars(&reply.join("\n\n"), 20000);
        let mut status = if !text.is_empty() && err.is_empty() {
            "ok".to_string()
        } else {
            "fallo".to_string()
        };
        if text.starts_with("PREGUNTA") {
            status = "pregunta".to_string();
        }

        let mut latest = self.load(id).unwrap_or(j);
        let now = now_unix_secs();
        latest.status = status;
        latest.ended = now;
        latest.updated = now;
        latest.reply = if !text.is_empty() {
            text
        } else if !err.is_empty() {
            err
        } else {
            "(sin respuesta)".to_string()
        };
        self.save(&latest)?;

        if latest.kind == "cdx" && !latest.thread.is_empty() {
            let archived = !self
                .archive_threads(&latest.home, &[latest.thread.clone()], "thread/archive")
                .is_empty();
            latest.archived = archived;
            self.save(&latest)?;
        }

        Ok(())
    }

    pub fn stop(&self, id: &str) -> Result<()> {
        let clean = id.strip_prefix("job:").unwrap_or(id);
        let j = self
            .load(clean)
            .ok_or_else(|| CoreError::NotFound("no existe".to_string()))?;
        if j.status != "q" && j.status != "r" {
            return Err(CoreError::InvalidInput("ya terminó".to_string()));
        }
        let pid = j.pid.filter(|&p| p > 0).ok_or_else(|| {
            CoreError::InvalidInput("trabajo sin pid (lanzado antes de este cambio)".to_string())
        })?;

        if runner_alive(&j) {
            let pg = unsafe { libc::getpgid(pid) };
            let my_pg = unsafe { libc::getpgrp() };
            unsafe {
                if pg <= 0 || pg == my_pg {
                    libc::kill(pid, libc::SIGTERM);
                } else {
                    libc::killpg(pg, libc::SIGTERM);
                }
            }
            for _ in 0..30 {
                if !pid_alive(pid) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if pid_alive(pid) {
                unsafe {
                    if pg <= 0 || pg == my_pg {
                        libc::kill(pid, libc::SIGKILL);
                    } else {
                        libc::killpg(pg, libc::SIGKILL);
                    }
                }
            }
        }

        if j.kind == "bec" {
            Self::interrupt_bec(&j.id);
        }

        let mut latest = self.load(&j.id).unwrap_or(j);
        let now = now_unix_secs();
        latest.status = "fallo".to_string();
        latest.ended = now;
        latest.updated = now;
        latest.reply = "(detenido por el OWNER)".to_string();
        self.save(&latest)?;

        if latest.kind == "cdx" && !latest.thread.is_empty() {
            latest.archived = !self
                .archive_threads(&latest.home, &[latest.thread.clone()], "thread/archive")
                .is_empty();
            self.save(&latest)?;
        }

        Ok(())
    }

    fn shots_path(&self, owner: &str) -> Result<PathBuf> {
        if !shots_owner_re().is_match(owner) {
            return Err(CoreError::InvalidInput("id de agente inválido".to_string()));
        }
        Ok(self
            .jobs_dir()
            .join(format!("{}.shots.json", owner.replace(':', "-"))))
    }

    pub fn shots(&self, owner: &str) -> Vec<ShotRecord> {
        let Ok(path) = self.shots_path(owner) else {
            return Vec::new();
        };
        let Ok(raw) = fs::read_to_string(path) else {
            return Vec::new();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    pub fn add_shot(&self, owner: &str, image: &Path, caption: &str) -> Result<usize> {
        let dest = self.shots_path(owner)?;
        let expanded = expand_tilde(image);
        let canon = fs::canonicalize(&expanded)
            .map_err(|_| CoreError::InvalidInput("la captura no existe".to_string()))?;
        if !canon.is_file() {
            return Err(CoreError::InvalidInput(
                "la captura no existe".to_string(),
            ));
        }

        {
            let mut f = File::open(&canon)?;
            let mut magic = [0u8; 8];
            if f.read_exact(&mut magic).is_err() || &magic != PNG_MAGIC {
                return Err(CoreError::InvalidInput(
                    "la captura debe ser PNG".to_string(),
                ));
            }
        }

        fs::create_dir_all(self.jobs_dir())?;
        let lock_path = PathBuf::from(format!("{}.lock", dest.display()));
        let _lock = FlockGuard::acquire(&lock_path)?;

        let mut records: Vec<ShotRecord> = match fs::read_to_string(&dest) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };

        records.push(ShotRecord {
            path: canon.to_string_lossy().into_owned(),
            caption: caption.to_string(),
            ts: now_unix_secs(),
        });

        let tid = format!("{:?}", std::thread::current().id())
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>();
        let temp = PathBuf::from(format!("{}.{}.{}.tmp", dest.display(), std::process::id(), tid));
        let payload = serde_json::to_string(&records)?;
        let write_res = (|| -> Result<()> {
            let mut f = File::create(&temp)?;
            f.write_all(payload.as_bytes())?;
            f.flush()?;
            f.sync_all()?;
            fs::rename(&temp, &dest)?;
            Ok(())
        })();
        if write_res.is_err() {
            let _ = fs::remove_file(&temp);
        }
        write_res?;

        Ok(records.len())
    }

    fn wait(&self, jid: &str, secs: f64) -> i32 {
        let deadline = Instant::now() + Duration::from_secs_f64(secs.max(0.0));
        while Instant::now() < deadline {
            let Some(j) = self.load(jid) else {
                println!("no existe");
                return 1;
            };
            if matches!(j.status.as_str(), "ok" | "fallo" | "pregunta") {
                let thread_part = if j.thread.is_empty() {
                    String::new()
                } else {
                    format!(" hilo {}", j.thread)
                };
                println!("[{}]{thread_part}\n\n{}", j.status, j.reply);
                return if j.status == "ok" { 0 } else { 1 };
            }
            std::thread::sleep(Duration::from_secs(3));
        }
        println!("sigue corriendo");
        2
    }

    pub fn dispatch_cli(root: PathBuf, argv: &[String]) -> i32 {
        let effective_root = if root.as_os_str().is_empty() {
            resolve_agencia_root()
        } else {
            root
        };
        let mgr = JobManager::new(effective_root);

        let exe_base = argv
            .first()
            .and_then(|s| Path::new(s).file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("");

        let (cmd, args): (&str, &[String]) = if argv.get(1).map(|s| s.as_str()) == Some("_run") {
            ("_run", &argv[2..])
        } else if matches!(
            exe_base,
            "bec" | "cdx" | "cla" | "slice" | "real-fin" | "agshot" | "_run"
        ) {
            (exe_base, if argv.len() > 1 { &argv[1..] } else { &[] })
        } else {
            if argv.len() < 2 {
                eprintln!("{DOC_HELP}");
                return 1;
            }
            (argv[1].as_str(), &argv[2..])
        };

        match cmd {
            "_run" => {
                if args.is_empty() {
                    return 1;
                }
                match mgr.run_job(&args[0]) {
                    Ok(()) => 0,
                    Err(e) => {
                        eprintln!("{e}");
                        1
                    }
                }
            }
            "real-fin" => match real_fin(None) {
                Ok(msg) => {
                    println!("{msg}");
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            },
            "slice" => {
                if args.is_empty() {
                    eprintln!("uso: slice NOMBRE [REPO]");
                    return 1;
                }
                let repo = args.get(1).map(Path::new);
                match slice_new(&args[0], repo) {
                    Ok(wt) => {
                        println!("{}", wt.display());
                        0
                    }
                    Err(e) => {
                        eprintln!("{e}");
                        1
                    }
                }
            }
            "agshot" => {
                if !(1..=2).contains(&args.len()) {
                    eprintln!("Uso: agshot <png path> [caption]");
                    return 1;
                }
                let jid = std::env::var("AGENCIA_JOB").ok().filter(|s| !s.is_empty());
                let sid = std::env::var("CLAUDE_CODE_SESSION_ID")
                    .ok()
                    .filter(|s| !s.is_empty());
                let owner = if let Some(id) = jid {
                    format!("job:{id}")
                } else if let Some(id) = sid {
                    format!("claude:{id}")
                } else {
                    eprintln!("agshot: falta AGENCIA_JOB o CLAUDE_CODE_SESSION_ID");
                    return 1;
                };
                let caption = args.get(1).map(|s| s.as_str()).unwrap_or("");
                match mgr.add_shot(&owner, Path::new(&args[0]), caption) {
                    Ok(count) => {
                        println!("{owner}: {count} captura(s)");
                        0
                    }
                    Err(e) => {
                        eprintln!("agshot: {e}");
                        1
                    }
                }
            }
            "cdx" | "bec" | "cla" => {
                let kind = cmd;
                if args.first().map(|s| s.as_str()) == Some("wait") && args.len() > 1 {
                    let secs = args
                        .get(2)
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(1800.0);
                    return mgr.wait(&args[1], secs);
                }
                if args.first().map(|s| s.as_str()) == Some("res") && args.len() > 1 {
                    match mgr.load(&args[1]) {
                        Some(j) => {
                            if let Ok(pretty) = serde_json::to_string_pretty(&j) {
                                println!("{pretty}");
                            }
                        }
                        None => println!("no existe"),
                    }
                    return 0;
                }
                if args.is_empty() || args.first().map(|s| s.as_str()) == Some("ls") {
                    let jobs = mgr.list_jobs_by_kind(Some(kind));
                    let start = jobs.len().saturating_sub(20);
                    for j in &jobs[start..] {
                        let home_disp = if j.home.is_empty() { "-" } else { &j.home };
                        let thread_tail = if j.thread.is_empty() {
                            "-".to_string()
                        } else {
                            tail_chars(&j.thread, 8)
                        };
                        let repo_base = Path::new(&j.repo)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&j.repo);
                        let preview = truncate_chars(&j.text, 60).replace('\n', " ");
                        println!(
                            "{} {} {} {} {} {}",
                            j.id, home_disp, j.status, thread_tail, repo_base, preview
                        );
                    }
                    return 0;
                }

                let mut repo: Option<String> = None;
                let mut de: Option<String> = None;
                let mut model: Option<String> = None;
                let mut effort: Option<String> = None;
                let mut linea: Option<String> = None;
                let mut secs_s: Option<String> = None;
                let mut steps_s: Option<String> = None;
                let mut home = "default".to_string();
                let mut rest: Vec<String> = Vec::new();

                let homes = discover_codex_homes();
                let mut filtered: Vec<String> = Vec::with_capacity(args.len());
                for arg in args {
                    if kind == "bec" && arg == "--glm" {
                        model = Some(BEC_GLM.to_string());
                    } else {
                        filtered.push(arg.clone());
                    }
                }

                let mut i = 0;
                while i < filtered.len() {
                    let cur = filtered[i].as_str();
                    if matches!(
                        cur,
                        "--repo" | "--de" | "--model" | "--effort" | "--linea" | "--secs" | "--steps"
                    ) && i + 1 < filtered.len()
                    {
                        let val = filtered[i + 1].clone();
                        match cur {
                            "--repo" => repo = Some(val),
                            "--de" => de = Some(val),
                            "--model" => model = Some(val),
                            "--effort" => effort = Some(val),
                            "--linea" => linea = Some(val),
                            "--secs" => secs_s = Some(val),
                            "--steps" => steps_s = Some(val),
                            _ => {}
                        }
                        i += 2;
                    } else if kind == "cdx" && rest.is_empty() && homes.contains_key(cur) {
                        home = cur.to_string();
                        i += 1;
                    } else {
                        rest.push(filtered[i].clone());
                        i += 1;
                    }
                }

                let text = rest.join(" ").trim().to_string();
                if text.is_empty()
                    || text.starts_with('-')
                    || text.eq_ignore_ascii_case("help")
                    || text.eq_ignore_ascii_case("ayuda")
                    || text.chars().count() < 25
                {
                    eprintln!(
                        "{DOC_HELP}\nNo lancé nada: '{text}' no es un pedido (mínimo 25 caracteres, sin empezar con '-')."
                    );
                    return 1;
                }

                let secs = match secs_s {
                    Some(s) => match s.parse::<u64>() {
                        Ok(n) => Some(n),
                        Err(_) => {
                            println!("--secs y --steps son números");
                            return 1;
                        }
                    },
                    None => None,
                };
                let steps = match steps_s {
                    Some(s) => match s.parse::<u32>() {
                        Ok(n) => Some(n),
                        Err(_) => {
                            println!("--secs y --steps son números");
                            return 1;
                        }
                    },
                    None => None,
                };

                let caller_de = caller(de.as_deref());
                let req = LaunchReq {
                    kind: kind.to_string(),
                    text: text.clone(),
                    home,
                    repo,
                    de: Some(caller_de.clone()),
                    model,
                    effort,
                    brief: linea.clone(),
                    secs,
                    steps,
                };

                match mgr.enqueue(req) {
                    Ok(job) => {
                        println!("{}", job.id);
                        if (kind == "bec" || kind == "cla") && !caller_de.starts_with('_') {
                            let has_linea = linea.as_ref().is_some_and(|s| !s.is_empty())
                                || brief_line_re().is_match(&text);
                            let warn = if kind == "cla" || has_linea {
                                ""
                            } else {
                                "FALTA: termina cada pedido con `EN UNA LÍNEA: <qué hará>`. "
                            };
                            eprintln!(
                                "{warn}El OWNER ya lo ve en la Agencia (:8800) colgado de ti. Dile en una línea que lo lanzaste y para qué; trae el resultado con `{kind} wait {}` en background y repórtalo al llegar.",
                                job.id
                            );
                        }
                        0
                    }
                    Err(e) => {
                        println!("{e}");
                        1
                    }
                }
            }
            _ => {
                eprintln!("{DOC_HELP}");
                1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(prefix: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "agencia_jobs_test_{}_{}_{}",
                prefix,
                std::process::id(),
                uuid_v4()
            ));
            fs::create_dir_all(&dir).expect("create temp dir");
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn test_discover_codex_homes_dynamic() {
        let tmp = TempDir::new("codex_homes");
        let h = &tmp.0;

        // Valid primary ~/.codex with auth.json
        fs::create_dir_all(h.join(".codex")).unwrap();
        fs::write(h.join(".codex/auth.json"), "{}").unwrap();

        // Valid extra ~/.codex-gateway with sessions dir
        fs::create_dir_all(h.join(".codex-gateway/sessions")).unwrap();

        // Invalid ~/.codex-empty without auth.json or sessions
        fs::create_dir_all(h.join(".codex-empty")).unwrap();

        let discovered = discover_codex_homes_in(h);
        assert_eq!(discovered.get("default"), Some(&h.join(".codex")));
        assert_eq!(discovered.get("gateway"), Some(&h.join(".codex-gateway")));
        assert!(!discovered.contains_key("empty"));
        assert_eq!(discovered.len(), 2);
    }

    #[test]
    fn test_discover_claude_profiles_unified_accounts_json_and_current() {
        let tmp = TempDir::new("claude_profiles");
        let h = &tmp.0;

        fs::create_dir_all(h.join(".claude")).unwrap();
        fs::create_dir_all(h.join(".claude-profiles/2")).unwrap();
        fs::create_dir_all(h.join(".claude-profiles/3")).unwrap();
        fs::create_dir_all(h.join(".claude-profiles/7")).unwrap();
        fs::create_dir_all(h.join(".claude-profiles/8")).unwrap();
        fs::create_dir_all(h.join(".config/claude-acc")).unwrap();
        fs::create_dir_all(h.join(".cache/claude-acc")).unwrap();

        fs::write(
            h.join(".config/claude-acc/accounts.json"),
            r#"{"6":"7","7":"8"}"#,
        )
        .unwrap();
        fs::write(h.join(".cache/claude-acc/current"), "6\n").unwrap();

        let profiles = discover_claude_profiles_in(h);
        let map: HashMap<u32, PathBuf> = profiles.into_iter().collect();
        assert_eq!(map.get(&1), Some(&h.join(".claude")));
        assert_eq!(map.get(&2), Some(&h.join(".claude-profiles/2")));
        assert_eq!(map.get(&3), Some(&h.join(".claude-profiles/3")));
        assert_eq!(map.get(&6), Some(&h.join(".claude-profiles/7")));
        assert_eq!(map.get(&7), Some(&h.join(".claude-profiles/8")));
        assert!(!map.contains_key(&8));

        let active = active_claude_config_dir_in(h);
        assert_eq!(active, Some(h.join(".claude-profiles/7")));

        // Logical account 3 uses default ~/.claude (CLAUDE_CONFIG_DIR unset for Keychain).
        fs::write(h.join(".cache/claude-acc/current"), "3\n").unwrap();
        assert_eq!(active_claude_config_dir_in(h), None);
    }

    #[test]
    fn test_job_manager_save_load_list_and_mtime_cache_hit() {
        let tmp = TempDir::new("jm_cache");
        let mgr = JobManager::new(tmp.0.clone());

        let j1 = JobRecord {
            id: "120000abcd".to_string(),
            kind: "bec".to_string(),
            ts: 100.0,
            home: String::new(),
            repo: tmp.0.to_string_lossy().into_owned(),
            de: "owner".to_string(),
            model: BEC_MODEL.to_string(),
            effort: String::new(),
            text: "Verificar cache de jobs\nEN UNA LÍNEA: probar cache".to_string(),
            brief: "probar cache".to_string(),
            secs: 300,
            steps_max: 50,
            status: "ok".to_string(),
            reply: "listo".to_string(),
            started: 101.0,
            ended: 102.0,
            updated: 102.0,
            pid: None,
            thread: String::new(),
            steps: 2,
            acts: vec!["read: foo.rs".to_string()],
            archived: false,
        };
        mgr.save(&j1).unwrap();

        let loaded = mgr.load("job:120000abcd").expect("should load job");
        assert_eq!(loaded.id, "120000abcd");
        assert_eq!(loaded.brief, "probar cache");

        // list_jobs populates cache; second call hits cache without re-parsing disk
        let list1 = mgr.list_jobs();
        assert_eq!(list1.len(), 1);
        assert_eq!(mgr.cache.lock().len(), 1);

        // Mutate the cached entry in memory ONLY (without changing disk mtime/size)
        // to prove list_jobs() returns from cache when (mtime_ns, size) match!
        {
            let mut guard = mgr.cache.lock();
            let entry = guard.values_mut().next().unwrap();
            entry.2.brief = "from-cache-hit".to_string();
        }
        let list2 = mgr.list_jobs();
        assert_eq!(list2.len(), 1);
        assert_eq!(list2[0].brief, "from-cache-hit");

        // Now modify the file on disk (changes size & mtime) -> cache misses and reloads disk
        let mut j1_updated = j1.clone();
        j1_updated.brief = "reloaded-from-disk-after-mtime-change".to_string();
        mgr.save(&j1_updated).unwrap();
        let list3 = mgr.list_jobs();
        assert_eq!(list3[0].brief, "reloaded-from-disk-after-mtime-change");
    }

    #[test]
    fn test_job_record_deserializes_legacy_missing_optional_fields() {
        let legacy = r#"{
            "id": "010203cafe",
            "kind": "bec",
            "ts": 1700000000.0,
            "text": "Legacy job without pid or updated",
            "status": "ok"
        }"#;
        let rec: JobRecord = serde_json::from_str(legacy).expect("legacy json should parse");
        assert_eq!(rec.id, "010203cafe");
        assert_eq!(rec.pid, None);
        assert_eq!(rec.updated, 0.0);
        assert_eq!(rec.steps, 0);
        assert!(rec.acts.is_empty());
        assert!(!rec.archived);
    }

    #[test]
    fn test_enqueue_validation_and_persistence() {
        let tmp = TempDir::new("enqueue");
        let repo_dir = tmp.0.join("myrepo");
        fs::create_dir_all(&repo_dir).unwrap();

        let mgr = JobManager::new(tmp.0.clone());

        // Reject invalid kind
        let bad_kind = mgr.enqueue(LaunchReq {
            kind: "unknown".to_string(),
            text: "Este pedido tiene mas de veinticinco caracteres".to_string(),
            repo: Some(repo_dir.to_string_lossy().into_owned()),
            ..Default::default()
        });
        assert!(bad_kind.is_err());

        // Reject out-of-range bec secs
        let bad_secs = mgr.enqueue(LaunchReq {
            kind: "bec".to_string(),
            text: "Este pedido tiene mas de veinticinco caracteres".to_string(),
            repo: Some(repo_dir.to_string_lossy().into_owned()),
            secs: Some(5),
            ..Default::default()
        });
        assert!(bad_secs.is_err());

        // Reject invalid effort
        let bad_effort = mgr.enqueue(LaunchReq {
            kind: "bec".to_string(),
            text: "Este pedido tiene mas de veinticinco caracteres".to_string(),
            repo: Some(repo_dir.to_string_lossy().into_owned()),
            effort: Some("turbo".to_string()),
            ..Default::default()
        });
        assert!(bad_effort.is_err());

        // Valid enqueue
        let job = mgr
            .enqueue(LaunchReq {
                kind: "bec".to_string(),
                text: "Revisar el modulo de jobs en Rust\nEN UNA LÍNEA: probar enqueue"
                    .to_string(),
                repo: Some(repo_dir.to_string_lossy().into_owned()),
                de: Some("owner".to_string()),
                ..Default::default()
            })
            .expect("valid enqueue should succeed");

        assert_eq!(job.id.len(), 10);
        assert_eq!(job.kind, "bec");
        assert_eq!(job.brief, "probar enqueue");
        assert_eq!(job.status, "q");
        assert!(tmp.0.join("jobs").join(format!("{}.json", job.id)).exists());
        assert!(tmp.0.join("jobs").join(format!("{}.err", job.id)).exists());
    }

    #[test]
    fn test_shots_png_magic_validation_owner_and_atomic_write() {
        let tmp = TempDir::new("shots");
        let mgr = JobManager::new(tmp.0.clone());

        let valid_png = tmp.0.join("ok.png");
        let mut png_bytes = PNG_MAGIC.to_vec();
        png_bytes.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R']);
        fs::write(&valid_png, &png_bytes).unwrap();

        let fake_png = tmp.0.join("fake.png");
        fs::write(&fake_png, b"not a png file at all").unwrap();

        // Invalid owner rejected
        assert!(
            mgr.add_shot("bad:../escape", &valid_png, "cap")
                .is_err()
        );
        assert!(mgr.add_shot("other:123", &valid_png, "cap").is_err());

        // Non-PNG rejected
        assert!(mgr.add_shot("job:123456abcd", &fake_png, "cap").is_err());

        // Valid PNG accepted and written atomically
        let count1 = mgr
            .add_shot("job:123456abcd", &valid_png, "primer shot")
            .expect("valid png shot");
        assert_eq!(count1, 1);

        let count2 = mgr
            .add_shot("job:123456abcd", &valid_png, "segundo shot")
            .expect("second png shot");
        assert_eq!(count2, 2);

        let recs = mgr.shots("job:123456abcd");
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].caption, "primer shot");
        assert_eq!(recs[1].caption, "segundo shot");
        assert!(tmp.0.join("jobs/job-123456abcd.shots.json").exists());
    }

    #[test]
    fn test_dispatch_cli_arg_validation_and_agencia_dir() {
        let tmp = TempDir::new("cli");
        let repo_dir = tmp.0.join("repo");
        fs::create_dir_all(&repo_dir).unwrap();

        // Too short (<25 chars) must fail with exit code 1 and create no job
        let rc_short = JobManager::dispatch_cli(
            tmp.0.clone(),
            &["bec".into(), "muy corto".into()],
        );
        assert_eq!(rc_short, 1);
        assert!(JobManager::new(tmp.0.clone()).list_jobs().is_empty());

        // Leading '-' stub must fail
        let rc_flag = JobManager::dispatch_cli(
            tmp.0.clone(),
            &["bec".into(), "--unknown-flag-that-is-over-25-chars".into()],
        );
        assert_eq!(rc_flag, 1);

        // Invalid --secs value returns 1
        let rc_bad_secs = JobManager::dispatch_cli(
            tmp.0.clone(),
            &[
                "bec".into(),
                "--secs".into(),
                "abc".into(),
                "Pedido valido con mas de veinticinco caracteres".into(),
            ],
        );
        assert_eq!(rc_bad_secs, 1);

        // `ls` on empty dir succeeds with 0
        let rc_ls = JobManager::dispatch_cli(tmp.0.clone(), &["bec".into(), "ls".into()]);
        assert_eq!(rc_ls, 0);

        // `agshot` without env vars fails cleanly (when neither AGENCIA_JOB nor CLAUDE_CODE_SESSION_ID set)
        if std::env::var_os("AGENCIA_JOB").is_none()
            && std::env::var_os("CLAUDE_CODE_SESSION_ID").is_none()
        {
            let rc_shot_no_env = JobManager::dispatch_cli(
                tmp.0.clone(),
                &["agshot".into(), "/nonexistent.png".into()],
            );
            assert_eq!(rc_shot_no_env, 1);
        }
    }

    #[test]
    fn test_brief_of_extracts_en_una_linea_and_skips_repo_opener() {
        let t1 = "Algo largo\nEN UNA LÍNEA: resumir el estado real\n";
        assert_eq!(brief_of(t1), "resumir el estado real");

        let t2 = "Repo /Users/dev/agencia.\nImplementar cache de jobs en Rust; luego probar.";
        assert_eq!(brief_of(t2), "Implementar cache de jobs en Rust");
    }

    #[test]
    fn test_resolve_agencia_root_prefers_agencia_dir() {
        let both = resolve_agencia_root_with(|k| match k {
            "AGENCIA_DIR" => Some(std::ffi::OsString::from("/tmp/dir_preferred")),
            "AGENCIA_ROOT" => Some(std::ffi::OsString::from("/tmp/root_fallback")),
            _ => None,
        });
        assert_eq!(both, PathBuf::from("/tmp/dir_preferred"));

        let only_root = resolve_agencia_root_with(|k| match k {
            "AGENCIA_ROOT" => Some(std::ffi::OsString::from("/tmp/root_fallback")),
            _ => None,
        });
        assert_eq!(only_root, PathBuf::from("/tmp/root_fallback"));
    }

    #[test]
    fn test_codex_5h_at_reads_recent_rollout_and_respects_reset() {
        let tmp = TempDir::new("codex_5h");
        let day_dir = tmp.0.join("sessions/2026/11/23");
        fs::create_dir_all(&day_dir).unwrap();

        let future_reset = (now_unix_secs() as u64) + 3600;
        let active_line = format!(
            r#"{{"type":"event_msg","payload":{{"rate_limits":{{"primary":{{"used_percent":84.5,"window_minutes":300,"resets_at":{future_reset}}}}}}}}}"#
        );
        let active_file = day_dir.join("rollout-active.jsonl");
        fs::write(&active_file, format!("{active_line}\n")).unwrap();
        assert!((codex_5h_at(&tmp.0) - 84.5).abs() < 0.01);

        let past_reset = (now_unix_secs() as u64).saturating_sub(60);
        let reset_line = format!(
            r#"{{"type":"event_msg","payload":{{"rate_limits":{{"primary":{{"used_percent":99.0,"window_minutes":300,"resets_at":{past_reset}}}}}}}}}"#
        );
        fs::write(&active_file, format!("{reset_line}\n")).unwrap();
        assert_eq!(codex_5h_at(&tmp.0), 0.0);
    }

    #[test]
    fn test_real_runs_from_flat_and_nested_and_slice_validation() {
        let tmp = TempDir::new("real_slice");
        let repo_dir = tmp.0.join("proj");
        fs::create_dir_all(&repo_dir).unwrap();
        let repo_canon = fs::canonicalize(&repo_dir).unwrap();
        let future = now_unix_secs() + 3600.0;

        let flat_file = tmp.0.join("real_flat.json");
        fs::write(
            &flat_file,
            serde_json::to_string(&json!({
                "repo": repo_canon.to_string_lossy(),
                "until": future,
                "at": 12345
            }))
            .unwrap(),
        )
        .unwrap();

        let runs = real_runs_from(&flat_file, true);
        assert!(real_in(&repo_dir, &runs).is_some());
        assert!(real_in(&repo_canon.join("sub"), &runs).is_some());

        // Invalid slice names rejected
        assert!(slice_new("Bad_Name", Some(&repo_dir)).is_err());
        assert!(slice_new("-leading", Some(&repo_dir)).is_err());
    }

    #[test]
    fn test_runner_alive_checks_pid_and_args_no_false_positive() {
        let me = std::process::id() as i32;
        let job_other = JobRecord {
            id: "ZZZZ999999_NOT_IN_ARGS".to_string(),
            kind: "bec".to_string(),
            ts: 0.0,
            home: String::new(),
            repo: String::new(),
            de: String::new(),
            model: String::new(),
            effort: String::new(),
            text: "x".to_string(),
            brief: String::new(),
            secs: 0,
            steps_max: 0,
            status: "r".to_string(),
            reply: String::new(),
            started: 0.0,
            ended: 0.0,
            updated: 0.0,
            pid: Some(me),
            thread: String::new(),
            steps: 0,
            acts: Vec::new(),
            archived: false,
        };
        assert!(!runner_alive(&job_other));

        let mut job_none = job_other.clone();
        job_none.pid = None;
        assert!(!runner_alive(&job_none));

        let mut job_empty_id = job_other;
        job_empty_id.id = String::new();
        assert!(!runner_alive(&job_empty_id));
    }

    #[test]
    fn test_dispatch_cli_multicall_run_dispatches_before_exe_base() {
        let tmp = TempDir::new("multicall_run");
        let mgr = JobManager::new(tmp.0.clone());

        // Invoking via `bec` symlink with `_run nonexistent_id` must dispatch `_run`
        // (returning 1 from `run_job` NotFound without treating `_run nonexistent_id` as a prompt).
        let rc_missing = JobManager::dispatch_cli(
            tmp.0.clone(),
            &["bec".into(), "_run".into(), "nonexistent_id".into()],
        );
        assert_eq!(rc_missing, 1);
        assert!(mgr.list_jobs().is_empty());

        // Prove `_run` actually executes `run_job` even when `argv[0]` is `"bec"`, `"cdx"`, or `"cla"`
        // by saving a queued job with an unknown kind and verifying `_run` transitions it to `"fallo"`.
        for (idx, exe) in ["bec", "cdx", "cla"].iter().enumerate() {
            let jid = format!("runtest00{idx}");
            let job = JobRecord {
                id: jid.clone(),
                kind: "invalid_test_kind".to_string(),
                ts: 100.0,
                home: String::new(),
                repo: tmp.0.to_string_lossy().into_owned(),
                de: "owner".to_string(),
                model: String::new(),
                effort: String::new(),
                text: "probe multicall _run dispatch".to_string(),
                brief: String::new(),
                secs: 30,
                steps_max: 1,
                status: "q".to_string(),
                reply: String::new(),
                started: 0.0,
                ended: 0.0,
                updated: 0.0,
                pid: None,
                thread: String::new(),
                steps: 0,
                acts: Vec::new(),
                archived: false,
            };
            mgr.save(&job).unwrap();

            let rc = JobManager::dispatch_cli(
                tmp.0.clone(),
                &[(*exe).into(), "_run".into(), jid.clone()],
            );
            assert_eq!(rc, 0, "_run via {exe} symlink must execute run_job");

            let updated = mgr.load(&jid).expect("job should still exist");
            assert_eq!(updated.status, "fallo");
            assert!(updated.reply.contains("kind desconocido"));
        }
    }

    #[test]
    fn test_is_isolated_with_rejects_empty_env_vars() {
        assert!(!is_isolated_with(|_| None));
        assert!(!is_isolated_with(|k| match k {
            "AGENCIA_DIR" | "AGENCIA_ROOT" => Some(std::ffi::OsString::new()),
            _ => None,
        }));
        assert!(is_isolated_with(|k| match k {
            "AGENCIA_DIR" => Some(std::ffi::OsString::from("/tmp/iso")),
            _ => None,
        }));
        assert!(is_isolated_with(|k| match k {
            "AGENCIA_ROOT" => Some(std::ffi::OsString::from("/tmp/iso")),
            _ => None,
        }));
    }

    #[test]
    fn test_load_becario_md_prefers_root_and_falls_back_without_enoent() {
        let tmp = TempDir::new("bec_md");
        let mgr = JobManager::new(tmp.0.clone());

        // Missing in isolated root: must not fail/panic; uses repo fallback or empty string.
        let fallback_content = mgr.load_becario_md();
        if agencia_root().join("becario.md").exists() {
            assert!(!fallback_content.is_empty());
        }

        // Local root becario.md takes precedence.
        fs::write(tmp.0.join("becario.md"), "local becario prompt").unwrap();
        assert_eq!(mgr.load_becario_md(), "local becario prompt");
    }
}
