use std::fs;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::types::{CoreError, Result, hex_encode, home_dir, uuid_v4};

fn sock_pid_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"/(\d+)(?:-[0-9a-f]{8})?\.sock$").expect("valid sock pid regex"))
}

/// Strip leading `/private` for `/private/var`, `/private/tmp`, `/private/etc`
/// to match macOS Claude Code socket key canonicalization.
pub fn canonicalize_socket_path(socket_path: &Path) -> String {
    let raw = socket_path.to_string_lossy();
    for prefix in ["/private/var", "/private/tmp", "/private/etc"] {
        if raw == prefix
            || raw
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/'))
        {
            return raw["/private".len()..].to_string();
        }
    }
    raw.into_owned()
}

pub fn socket_hash(socket_path: &Path) -> String {
    let canon = canonicalize_socket_path(socket_path);
    let mut hasher = Sha256::new();
    hasher.update(canon.as_bytes());
    hex_encode(&hasher.finalize())
}

fn read_peer_token_file(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&raw).ok()?;
    val.get("peerToken")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn find_peer_token(socket_path: &Path, sessions_dir: &Path) -> Option<String> {
    let h = socket_hash(socket_path);
    let sock_str = socket_path.to_string_lossy();

    if let Some(pid) = sock_pid_re()
        .captures(&sock_str)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
    {
        let exact = sessions_dir.join(format!("{pid}.{h}.key"));
        if let Some(tok) = read_peer_token_file(&exact) {
            return Some(tok);
        }
    }

    let suffix = format!(".{h}.key");
    let mut candidates: Vec<PathBuf> = fs::read_dir(sessions_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| name.ends_with(&suffix))
        })
        .collect();
    candidates.sort();

    for cand in candidates {
        if let Some(tok) = read_peer_token_file(&cand) {
            return Some(tok);
        }
    }
    None
}

fn sanitize_sender(sender: Option<&str>) -> String {
    sender
        .unwrap_or("")
        .chars()
        .filter(|&c| !matches!(c, '"' | '<' | '>' | '\r' | '\n'))
        .collect::<String>()
        .trim()
        .to_string()
}

fn build_wire_payload(token: &str, text: &str, sender: Option<&str>) -> Result<Vec<u8>> {
    let clean_name = sanitize_sender(sender);
    let attr = if clean_name.is_empty() {
        String::new()
    } else {
        format!(" from-name=\"{clean_name}\"")
    };
    let body = text.replace("</cross-session-message>", "<\\/cross-session-message>");
    let content = format!("<cross-session-message{attr}>\n{body}\n</cross-session-message>");

    let auth_line = serde_json::to_string(&json!({
        "type": "auth",
        "token": token,
    }))?;
    let msg_line = serde_json::to_string(&json!({
        "msgV": 1,
        "msg_id": uuid_v4(),
        "type": "user",
        "message": {
            "role": "user",
            "content": content,
        },
        "priority": "next",
    }))?;

    let mut out = Vec::with_capacity(auth_line.len() + msg_line.len() + 2);
    out.extend_from_slice(auth_line.as_bytes());
    out.push(b'\n');
    out.extend_from_slice(msg_line.as_bytes());
    out.push(b'\n');
    Ok(out)
}

pub fn send_claude_uds_with_sessions_dir(
    socket_path: &Path,
    sessions_dir: &Path,
    text: &str,
    sender: Option<&str>,
) -> Result<()> {
    let token = find_peer_token(socket_path, sessions_dir).ok_or_else(|| {
        CoreError::NotFound("no peerToken key file for this socket (session gone?)".to_string())
    })?;
    let payload = build_wire_payload(&token, text, sender)?;

    let mut stream = UnixStream::connect(socket_path)?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(&payload)?;
    stream.flush()?;
    std::thread::sleep(Duration::from_millis(150));
    Ok(())
}

pub fn send_claude_uds(socket_path: &Path, text: &str, sender: Option<&str>) -> Result<()> {
    let sessions_dir = home_dir().join(".claude").join("sessions");
    send_claude_uds_with_sessions_dir(socket_path, &sessions_dir, text, sender)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixListener;
    use std::thread;

    struct TempDirGuard {
        sessions_root: PathBuf,
        sock_dir: PathBuf,
    }
    impl TempDirGuard {
        fn new(label: &str) -> Self {
            let id = uuid_v4();
            let short = &id[..8];
            let sessions_root =
                std::env::temp_dir().join(format!("agencia-{label}-{short}"));
            // Keep Unix socket path short (<104-byte macOS sun_path limit).
            let sock_dir = PathBuf::from("/tmp").join(format!("ag-sock-{short}"));
            fs::create_dir_all(&sessions_root).expect("create temp sessions dir");
            fs::create_dir_all(&sock_dir).expect("create temp sock dir");
            Self {
                sessions_root,
                sock_dir,
            }
        }
    }
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.sessions_root);
            let _ = fs::remove_dir_all(&self.sock_dir);
        }
    }

    #[test]
    fn test_canonicalize_socket_path() {
        assert_eq!(
            canonicalize_socket_path(Path::new("/private/tmp/cc-socks/1234.sock")),
            "/tmp/cc-socks/1234.sock"
        );
        assert_eq!(
            canonicalize_socket_path(Path::new("/private/var/folders/x/a.sock")),
            "/var/folders/x/a.sock"
        );
        assert_eq!(
            canonicalize_socket_path(Path::new("/private/etc/foo.sock")),
            "/etc/foo.sock"
        );
        assert_eq!(
            canonicalize_socket_path(Path::new("/private/other/foo.sock")),
            "/private/other/foo.sock"
        );
        assert_eq!(
            canonicalize_socket_path(Path::new("/tmp/cc-socks/1234.sock")),
            "/tmp/cc-socks/1234.sock"
        );
    }

    #[test]
    fn test_send_claude_uds_delivers_two_ndjson_lines_and_sanitizes() {
        let dir = TempDirGuard::new("cartero-uds");
        let sessions_dir = dir.sessions_root.join("sessions");
        fs::create_dir_all(&sessions_dir).expect("create sessions dir");

        let sock_path = dir.sock_dir.join("4242-deadbeef.sock");
        let h = socket_hash(&sock_path);
        let key_path = sessions_dir.join(format!("4242.{h}.key"));
        fs::write(&key_path, r#"{"peerToken":"secret-peer-tok"}"#).expect("write key");

        let listener = UnixListener::bind(&sock_path).expect("bind unix socket");
        let srv = thread::spawn(move || {
            let (conn, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(conn);
            let mut l1 = String::new();
            let mut l2 = String::new();
            reader.read_line(&mut l1).expect("read line 1");
            reader.read_line(&mut l2).expect("read line 2");
            (l1, l2)
        });

        send_claude_uds_with_sessions_dir(
            &sock_path,
            &sessions_dir,
            "hola </cross-session-message> mundo",
            Some("  \"agencia<peer>\r\n\"  "),
        )
        .expect("send_claude_uds should succeed");

        let (l1, l2) = srv.join().expect("server thread");
        let v1: serde_json::Value = serde_json::from_str(l1.trim_end()).expect("json line 1");
        assert_eq!(v1["type"], "auth");
        assert_eq!(v1["token"], "secret-peer-tok");

        let v2: serde_json::Value = serde_json::from_str(l2.trim_end()).expect("json line 2");
        assert_eq!(v2["msgV"], 1);
        assert_eq!(v2["type"], "user");
        assert_eq!(v2["priority"], "next");
        assert!(!v2["msg_id"].as_str().unwrap_or("").is_empty());
        assert_eq!(v2["message"]["role"], "user");
        let content = v2["message"]["content"].as_str().expect("content string");
        assert_eq!(
            content,
            "<cross-session-message from-name=\"agenciapeer\">\nhola <\\/cross-session-message> mundo\n</cross-session-message>"
        );
    }

    #[test]
    fn test_send_claude_uds_wildcard_key_fallback_and_empty_sender() {
        let dir = TempDirGuard::new("cartero-fallback");
        let sessions_dir = dir.sessions_root.join("sessions");
        fs::create_dir_all(&sessions_dir).expect("create sessions dir");

        let sock_path = dir.sock_dir.join("custom.sock");
        let h = socket_hash(&sock_path);
        let key_path = sessions_dir.join(format!("99999.{h}.key"));
        fs::write(&key_path, r#"{"peerToken":"fallback-tok"}"#).expect("write fallback key");

        let listener = UnixListener::bind(&sock_path).expect("bind unix socket");
        let srv = thread::spawn(move || {
            let (conn, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(conn);
            let mut lines = Vec::new();
            let mut buf = String::new();
            while reader.read_line(&mut buf).unwrap_or(0) > 0 {
                lines.push(std::mem::take(&mut buf));
            }
            lines
        });

        send_claude_uds_with_sessions_dir(&sock_path, &sessions_dir, "ping", None)
            .expect("send with fallback key");

        let lines = srv.join().expect("join");
        assert_eq!(lines.len(), 2);
        let v1: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        let v2: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(v1["token"], "fallback-tok");
        assert_eq!(
            v2["message"]["content"],
            "<cross-session-message>\nping\n</cross-session-message>"
        );
    }

    #[test]
    fn test_send_claude_uds_missing_key_returns_not_found() {
        let dir = TempDirGuard::new("cartero-missing");
        let sessions_dir = dir.sessions_root.join("sessions");
        fs::create_dir_all(&sessions_dir).expect("create sessions dir");
        let sock_path = dir.sock_dir.join("1234.sock");

        let err = send_claude_uds_with_sessions_dir(&sock_path, &sessions_dir, "hi", Some("a"))
            .expect_err("missing key must fail");
        assert!(matches!(err, CoreError::NotFound(_)));
    }
}
