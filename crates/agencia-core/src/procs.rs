use std::collections::HashMap;
use std::mem::{size_of, zeroed};
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::types::Result;

pub const PROC_PIDFDVNODEPATHINFO: libc::c_int = 2;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct proc_fileinfo {
    pub fi_openflags: u32,
    pub fi_status: u32,
    pub fi_offset: libc::off_t,
    pub fi_type: i32,
    pub fi_guardflags: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct vnode_fdinfowithpath {
    pub pfi: proc_fileinfo,
    pub pvip: libc::vnode_info_path,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcEntry {
    pub pid: i32,
    pub ppid: i32,
    pub uid: u32,
    pub tty: String,
    pub stat: String,
    pub comm: String,
    pub args: String,
    pub cwd: String,
    pub env_profile: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct ProcSnapshot {
    pub procs: HashMap<i32, ProcEntry>,
    pub children: HashMap<i32, Vec<i32>>,
}

fn c_chars_to_string(buf: &[libc::c_char]) -> String {
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), buf.len()) };
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..len]).into_owned()
}

fn vip_path_to_string(vip: &libc::vnode_info_path) -> Option<String> {
    let bytes = unsafe { std::slice::from_raw_parts(vip.vip_path.as_ptr().cast::<u8>(), 1024) };
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    if len == 0 {
        None
    } else {
        Some(String::from_utf8_lossy(&bytes[..len]).into_owned())
    }
}

fn format_tty(e_tdev: u32) -> String {
    if e_tdev == u32::MAX || e_tdev == 0 {
        return "??".to_string();
    }
    let major = (e_tdev >> 24) & 0xff;
    let minor = e_tdev & 0x00ff_ffff;
    if major == 16 {
        format!("s{:03}", minor)
    } else {
        format!("tty{:x}:{:x}", major, minor)
    }
}

fn format_stat(pbi_status: u32) -> &'static str {
    match pbi_status {
        1 => "I",
        2 => "R",
        3 => "S",
        4 => "T",
        5 => "Z",
        _ => "?",
    }
}

fn proc_pidpath_str(pid: i32) -> Option<String> {
    if pid <= 0 {
        return None;
    }
    let mut buf = [0u8; 4096];
    let ret = unsafe {
        libc::proc_pidpath(
            pid,
            buf.as_mut_ptr().cast::<libc::c_void>(),
            buf.len() as u32,
        )
    };
    if ret <= 0 {
        return None;
    }
    let n = (ret as usize).min(buf.len());
    Some(String::from_utf8_lossy(&buf[..n]).into_owned())
}

fn read_procargs2_raw(pid: i32) -> Option<Vec<u8>> {
    if pid <= 0 {
        return None;
    }
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut len: libc::size_t = 0;
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || len == 0 {
        return None;
    }

    let mut buf = vec![0u8; len.saturating_add(1024)];
    for _ in 0..3 {
        let mut actual_len = buf.len();
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                buf.as_mut_ptr().cast::<libc::c_void>(),
                &mut actual_len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc == 0 {
            buf.truncate(actual_len);
            return Some(buf);
        }
        if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOMEM) {
            let next_len = buf.len().saturating_mul(2).min(4 * 1024 * 1024);
            if next_len <= buf.len() {
                return None;
            }
            buf.resize(next_len, 0);
        } else {
            return None;
        }
    }
    None
}

type ProcArgsEnv = (Vec<String>, Vec<(String, String)>);

fn parse_procargs2(buf: &[u8]) -> Option<ProcArgsEnv> {
    if buf.len() < 4 {
        return None;
    }
    let argc = i32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]);
    if argc < 0 {
        return None;
    }

    let mut pos = 4usize;
    while pos < buf.len() && buf[pos] != 0 {
        pos += 1;
    }
    while pos < buf.len() && buf[pos] == 0 {
        pos += 1;
    }

    let mut argv = Vec::with_capacity((argc as usize).min(256));
    for _ in 0..(argc as usize) {
        if pos >= buf.len() {
            break;
        }
        let start = pos;
        while pos < buf.len() && buf[pos] != 0 {
            pos += 1;
        }
        argv.push(String::from_utf8_lossy(&buf[start..pos]).into_owned());
        if pos < buf.len() {
            pos += 1;
        }
    }

    let mut env = Vec::new();
    while pos < buf.len() && buf[pos] != 0 {
        let start = pos;
        while pos < buf.len() && buf[pos] != 0 {
            pos += 1;
        }
        let entry = String::from_utf8_lossy(&buf[start..pos]).into_owned();
        if let Some((k, v)) = entry.split_once('=') {
            env.push((k.to_string(), v.to_string()));
        }
        if pos < buf.len() {
            pos += 1;
        }
    }

    Some((argv, env))
}

fn claude_profile_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"/\.claude-profiles/(\d+)").expect("valid profile regex"))
}

fn agy_lock_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"presence/([0-9a-fA-F-]+)\.lock$").expect("valid lock regex"))
}

fn extract_env_profile(env: &[(String, String)]) -> Option<u32> {
    for (k, v) in env {
        if k == "CLAUDE_CONFIG_DIR"
            && let Some(caps) = claude_profile_re().captures(v)
            && let Some(m) = caps.get(1)
            && let Ok(num) = m.as_str().parse::<u32>()
        {
            return Some(num);
        }
    }
    None
}

fn list_all_pids() -> Result<Vec<i32>> {
    let estimated = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if estimated < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut cap = (estimated as usize).saturating_add(64).max(128);
    let mut last_pids = Vec::new();
    for _ in 0..3 {
        let mut pids = vec![0i32; cap];
        let buf_bytes = (pids.len() * size_of::<libc::c_int>()) as libc::c_int;
        let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), buf_bytes) };
        if count < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let n = count as usize;
        let valid = n.min(pids.len());
        pids.truncate(valid);
        pids.retain(|&p| p > 0);
        if n < cap {
            return Ok(pids);
        }
        last_pids = pids;
        cap = cap.saturating_mul(2);
    }
    Ok(last_pids)
}

pub fn pid_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid, 0) };
    if rc == 0 {
        return true;
    }
    matches!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EPERM)
    )
}

pub fn proc_cwd(pid: i32) -> Option<String> {
    if pid <= 0 {
        return None;
    }
    let mut vpi: libc::proc_vnodepathinfo = unsafe { zeroed() };
    let sz = size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    let ret = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&mut vpi as *mut libc::proc_vnodepathinfo).cast::<libc::c_void>(),
            sz,
        )
    };
    if ret <= 0 {
        return None;
    }
    vip_path_to_string(&vpi.pvi_cdir)
}

pub fn proc_args(pid: i32) -> Option<String> {
    if let Some(raw) = read_procargs2_raw(pid)
        && let Some((argv, _)) = parse_procargs2(&raw)
        && !argv.is_empty()
    {
        return Some(argv.join(" "));
    }
    proc_pidpath_str(pid)
}

pub fn proc_env(pid: i32) -> Vec<(String, String)> {
    read_procargs2_raw(pid)
        .and_then(|raw| parse_procargs2(&raw))
        .map(|(_, env)| env)
        .unwrap_or_default()
}

pub fn open_lock_fds(pid: i32) -> Vec<String> {
    if pid <= 0 {
        return Vec::new();
    }
    let buf_bytes = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
    if buf_bytes <= 0 {
        return Vec::new();
    }
    let fd_size = size_of::<libc::proc_fdinfo>();
    let cap = (buf_bytes as usize / fd_size).saturating_add(16);
    let mut fds = vec![unsafe { zeroed::<libc::proc_fdinfo>() }; cap];
    let ret = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDLISTFDS,
            0,
            fds.as_mut_ptr().cast::<libc::c_void>(),
            (fds.len() * fd_size) as libc::c_int,
        )
    };
    if ret <= 0 {
        return Vec::new();
    }
    let count = (ret as usize) / fd_size;
    let vfd_sz = size_of::<vnode_fdinfowithpath>() as libc::c_int;
    let mut out = Vec::new();
    for fd in &fds[..count] {
        if fd.proc_fdtype != libc::PROX_FDTYPE_VNODE as u32 {
            continue;
        }
        let mut vfd: vnode_fdinfowithpath = unsafe { zeroed() };
        let r = unsafe {
            libc::proc_pidfdinfo(
                pid,
                fd.proc_fd,
                PROC_PIDFDVNODEPATHINFO,
                (&mut vfd as *mut vnode_fdinfowithpath).cast::<libc::c_void>(),
                vfd_sz,
            )
        };
        if r > 0
            && let Some(path) = vip_path_to_string(&vfd.pvip)
            && path.ends_with(".lock")
        {
            out.push(path);
        }
    }
    out
}

pub fn agy_lock_conv_ids(pid: i32) -> Vec<String> {
    let re = agy_lock_re();
    let mut out = Vec::new();
    for path in open_lock_fds(pid) {
        if let Some(caps) = re.captures(&path)
            && let Some(m) = caps.get(1)
        {
            let id = m.as_str().to_string();
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

fn is_agy_entry(entry: &ProcEntry) -> bool {
    if Path::new(entry.comm.trim())
        .file_name()
        .and_then(|n| n.to_str())
        == Some("agy")
    {
        return true;
    }

    let args = entry.args.trim();
    if args.is_empty() {
        return false;
    }

    if let Some(quote) = args.chars().next().filter(|&c| c == '"' || c == '\'')
        && let Some(end) = args[1..].find(quote)
    {
        let exe = &args[1..1 + end];
        return Path::new(exe).file_name().and_then(|n| n.to_str()) == Some("agy");
    }

    if let Some(first_tok) = args.split_whitespace().next()
        && Path::new(first_tok).file_name().and_then(|n| n.to_str()) == Some("agy")
    {
        return true;
    }

    let before_flags = args.split(" -").next().unwrap_or(args).trim();
    Path::new(before_flags).file_name().and_then(|n| n.to_str()) == Some("agy")
}

pub fn agy_pids(snap: &ProcSnapshot) -> HashMap<i32, String> {
    let mut out = HashMap::new();
    for (&pid, entry) in &snap.procs {
        // Keep parity with procs.py: only terminal-attached agy processes (skip tty == "??").
        if entry.tty == "??" {
            continue;
        }
        if is_agy_entry(entry) {
            let conv_id = agy_lock_conv_ids(pid).into_iter().next().unwrap_or_default();
            out.insert(pid, conv_id);
        }
    }
    out
}

pub fn discover_procs() -> Result<ProcSnapshot> {
    let pids = list_all_pids()?;
    let my_uid = unsafe { libc::getuid() };
    let mut snap = ProcSnapshot::default();
    let bsd_sz = size_of::<libc::proc_bsdinfo>() as libc::c_int;

    for pid in pids {
        let mut bsd: libc::proc_bsdinfo = unsafe { zeroed() };
        let ret = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut bsd as *mut libc::proc_bsdinfo).cast::<libc::c_void>(),
                bsd_sz,
            )
        };
        if ret <= 0 {
            continue;
        }

        let name = c_chars_to_string(&bsd.pbi_name);
        let short_comm = c_chars_to_string(&bsd.pbi_comm);
        let mut comm = if !name.is_empty() { name } else { short_comm };

        let (args, env_profile) = if bsd.pbi_uid == my_uid {
            if let Some(raw) = read_procargs2_raw(pid) {
                if let Some((argv, env)) = parse_procargs2(&raw) {
                    let args_str = if !argv.is_empty() {
                        if comm.is_empty()
                            && let Some(bn) = Path::new(&argv[0]).file_name().and_then(|s| s.to_str())
                        {
                            comm = bn.to_string();
                        }
                        argv.join(" ")
                    } else {
                        proc_pidpath_str(pid).unwrap_or_else(|| comm.clone())
                    };
                    let prof = extract_env_profile(&env);
                    (args_str, prof)
                } else {
                    (proc_pidpath_str(pid).unwrap_or_else(|| comm.clone()), None)
                }
            } else {
                (proc_pidpath_str(pid).unwrap_or_else(|| comm.clone()), None)
            }
        } else {
            (proc_pidpath_str(pid).unwrap_or_else(|| comm.clone()), None)
        };

        let cwd = if bsd.pbi_uid == my_uid {
            proc_cwd(pid).unwrap_or_default()
        } else {
            String::new()
        };

        let entry = ProcEntry {
            pid,
            ppid: bsd.pbi_ppid as i32,
            uid: bsd.pbi_uid,
            tty: format_tty(bsd.e_tdev),
            stat: format_stat(bsd.pbi_status).to_string(),
            comm,
            args,
            cwd,
            env_profile,
        };

        snap.children.entry(entry.ppid).or_default().push(entry.pid);
        snap.procs.insert(pid, entry);
    }

    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::path::PathBuf;

    #[test]
    fn test_pid_alive_self_and_invalid() {
        let me = std::process::id() as i32;
        assert!(pid_alive(me));
        assert!(!pid_alive(0));
        assert!(!pid_alive(-1));
        assert!(!pid_alive(i32::MAX));
    }

    #[test]
    fn test_discover_procs_contains_self() {
        let me = std::process::id() as i32;
        let snap = discover_procs().expect("discover_procs should succeed");
        assert!(snap.procs.len() > 10, "expected active system processes");
        let entry = snap.procs.get(&me).expect("current pid must be in snapshot");
        assert_eq!(entry.pid, me);
        assert!(entry.ppid > 0);
        assert!(!entry.comm.is_empty());
        assert!(!entry.args.is_empty());
        assert!(!entry.cwd.is_empty());
        let siblings = snap
            .children
            .get(&entry.ppid)
            .expect("parent should list current pid as child");
        assert!(siblings.contains(&me));
    }

    #[test]
    fn test_proc_cwd_matches_current_dir() {
        let me = std::process::id() as i32;
        let cwd = proc_cwd(me).expect("proc_cwd should succeed for self");
        let expected = std::env::current_dir()
            .expect("current_dir")
            .canonicalize()
            .expect("canonicalize");
        let actual = Path::new(&cwd).canonicalize().expect("canonicalize proc_cwd");
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_proc_args_and_proc_env_self() {
        let me = std::process::id() as i32;
        let args = proc_args(me).expect("proc_args should succeed for self");
        assert!(!args.is_empty());
        let env = proc_env(me);
        assert!(!env.is_empty(), "proc_env should return env vars for self");
        assert!(
            env.iter().any(|(k, _)| k == "PATH" || k == "HOME"),
            "expected PATH or HOME in proc_env"
        );
    }

    #[test]
    fn test_open_lock_fds_and_agy_conv_id() {
        struct CleanDir(PathBuf);
        impl Drop for CleanDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let me = std::process::id() as i32;
        let tmp_base = std::env::temp_dir().join(format!(
            "agencia_procs_test_{}_{}",
            me,
            crate::types::uuid_v4()
        ));
        let _guard = CleanDir(tmp_base.clone());
        let pres_dir = tmp_base.join("presence");
        std::fs::create_dir_all(&pres_dir).expect("create temp presence dir");
        let lock_path = pres_dir.join("12345678-9abc-def0-1234-56789abcdef0.lock");
        let _lock_file = File::create(&lock_path).expect("create lock file");

        let fds = open_lock_fds(me);
        let lock_canon = lock_path.canonicalize().expect("canonical lock path");
        assert!(
            fds.iter().any(|p| Path::new(p) == lock_canon),
            "expected open_lock_fds to contain {:?}, got {:?}",
            lock_canon,
            fds
        );

        let convs = agy_lock_conv_ids(me);
        assert!(
            convs.contains(&"12345678-9abc-def0-1234-56789abcdef0".to_string()),
            "expected extracted conv uuid, got {:?}",
            convs
        );
    }

    #[test]
    fn test_extract_env_profile() {
        let env = vec![
            ("HOME".to_string(), "/Users/dev".to_string()),
            (
                "CLAUDE_CONFIG_DIR".to_string(),
                "/Users/dev/.claude-profiles/5".to_string(),
            ),
        ];
        assert_eq!(extract_env_profile(&env), Some(5));
        assert_eq!(extract_env_profile(&[]), None);
    }

    #[test]
    fn test_agy_pids_handles_paths_with_spaces_and_tty_parity() {
        let mut snap = ProcSnapshot::default();
        let mk_entry = |pid: i32, tty: &str, comm: &str, args: &str| ProcEntry {
            pid,
            ppid: 1,
            uid: 501,
            tty: tty.to_string(),
            stat: "S".to_string(),
            comm: comm.to_string(),
            args: args.to_string(),
            cwd: String::new(),
            env_profile: None,
        };

        // Synthetic PIDs above macOS PID_MAX (99999) so open_lock_fds safely returns empty.
        snap.procs.insert(
            999_991,
            mk_entry(999_991, "s001", "", "/Users/foo/My Apps/agy --flag"),
        );
        snap.procs.insert(
            999_992,
            mk_entry(999_992, "s002", "agy", "/Users/foo/My Apps/agy positional"),
        );
        snap.procs.insert(
            999_993,
            mk_entry(
                999_993,
                "s003",
                "",
                "\"/Users/foo/My Apps/agy\" --port 1234",
            ),
        );
        snap.procs
            .insert(999_994, mk_entry(999_994, "??", "agy", "agy"));
        snap.procs.insert(
            999_995,
            mk_entry(
                999_995,
                "s004",
                "other",
                "/Users/foo/My Apps/other --bin /usr/bin/agy",
            ),
        );

        let found = agy_pids(&snap);
        assert!(found.contains_key(&999_991));
        assert!(found.contains_key(&999_992));
        assert!(found.contains_key(&999_993));
        assert!(!found.contains_key(&999_994));
        assert!(!found.contains_key(&999_995));
    }
}
