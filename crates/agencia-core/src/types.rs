use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("http: {0}")]
    Http(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    External(String),
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

pub fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    if getrandom::fill(&mut b).is_err()
        && let Ok(mut f) = std::fs::File::open("/dev/urandom")
    {
        let _ = std::io::Read::read_exact(&mut f, &mut b);
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex_encode(&b);
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

pub fn now_unix_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub fn home_dir() -> PathBuf {
    if let Some(h) = std::env::var_os("HOME")
        && !h.is_empty()
    {
        return PathBuf::from(h);
    }
    // SAFETY: getpwuid returns a pointer to static passwd storage or null.
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if !pw.is_null() && !(*pw).pw_dir.is_null() {
            let cstr = std::ffi::CStr::from_ptr((*pw).pw_dir);
            if let Ok(s) = cstr.to_str()
                && !s.is_empty()
            {
                return PathBuf::from(s);
            }
        }
    }
    PathBuf::from(".")
}

pub fn resolve_agencia_root_with(
    agencia_dir: Option<OsString>,
    agencia_root: Option<OsString>,
) -> PathBuf {
    for root in [agencia_dir, agencia_root].into_iter().flatten() {
        if !root.is_empty() {
            return PathBuf::from(root);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
        // Binary lives at <repo>/target/release/agencia or <repo>/bin/agencia
        for anc in exe.ancestors().skip(1).take(4) {
            if anc.join("web").is_dir() || anc.join("Cargo.toml").is_file() {
                return anc.to_path_buf();
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir()
        && (cwd.join("web").is_dir() || cwd.join("Cargo.toml").is_file())
    {
        return cwd;
    }
    let home_default = home_dir().join("Agencia");
    if home_default.exists() {
        return home_default;
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn agencia_root() -> PathBuf {
    resolve_agencia_root_with(
        std::env::var_os("AGENCIA_DIR"),
        std::env::var_os("AGENCIA_ROOT"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_encode() {
        assert_eq!(hex_encode(&[]), "");
        assert_eq!(hex_encode(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
    }

    #[test]
    fn test_uuid_v4_format_and_uniqueness() {
        let u1 = uuid_v4();
        let u2 = uuid_v4();
        assert_eq!(u1.len(), 36);
        assert_ne!(u1, u2);
        let bytes = u1.as_bytes();
        assert_eq!(bytes[8], b'-');
        assert_eq!(bytes[13], b'-');
        assert_eq!(bytes[14], b'4');
        assert_eq!(bytes[18], b'-');
        assert!(matches!(bytes[19], b'8' | b'9' | b'a' | b'b'));
        assert_eq!(bytes[23], b'-');
    }

    #[test]
    fn test_now_unix_secs_positive() {
        assert!(now_unix_secs() > 1_700_000_000.0);
    }

    #[test]
    fn test_home_and_root() {
        let home = home_dir();
        assert!(!home.as_os_str().is_empty());
        let root = agencia_root();
        assert!(!root.as_os_str().is_empty());
    }

    #[test]
    fn test_agencia_dir_overrides_agencia_root() {
        assert_eq!(
            resolve_agencia_root_with(
                Some(OsString::from("/tmp/from_agencia_dir")),
                Some(OsString::from("/tmp/from_agencia_root")),
            ),
            PathBuf::from("/tmp/from_agencia_dir")
        );
        assert_eq!(
            resolve_agencia_root_with(None, Some(OsString::from("/tmp/from_agencia_root"))),
            PathBuf::from("/tmp/from_agencia_root")
        );
        assert_eq!(
            resolve_agencia_root_with(
                Some(OsString::from("")),
                Some(OsString::from("/tmp/from_agencia_root")),
            ),
            PathBuf::from("/tmp/from_agencia_root")
        );
        let fallback = resolve_agencia_root_with(None, Some(OsString::from("")));
        assert!(!fallback.as_os_str().is_empty());
    }
}
