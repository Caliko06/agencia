use super::*;
/// Same active-turn guard as Python Feed.eat_agy + agy_running.
pub(super) fn transcript_running(key: &str) -> bool {
    let dir = PathBuf::from(crate::shared::home())
        .join(".gemini/antigravity-cli/brain")
        .join(key)
        .join(".system_generated/logs");
    let full = dir.join("transcript_full.jsonl");
    let path = if full.exists() {
        full
    } else {
        dir.join("transcript.jsonl")
    };
    let Ok(mut f) = fs::File::open(&path) else {
        return false;
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(1_500_000);
    let mut busy = false;
    let mut has_user = false;
    let mut t1 = 0.0_f64;
    let user = regex::Regex::new(r"(?s)<USER_REQUEST>\n?(.*?)\n?</USER_REQUEST>").unwrap();
    let system =
        regex::Regex::new(r"(?s)sender=(\S+).*?content=(.*?)(?:\n</SYSTEM_MESSAGE>|$)").unwrap();
    let mut eat = |e: Value| {
        let time = chrono::DateTime::parse_from_rfc3339(s(&e, "created_at"))
            .map(|v| v.timestamp_millis() as f64 / 1000.0)
            .unwrap_or(t1);
        match s(&e, "type") {
            "USER_INPUT" => {
                let c = s(&e, "content");
                let tx = user
                    .captures(c)
                    .and_then(|m| m.get(1))
                    .map(|m| m.as_str())
                    .unwrap_or(c);
                if !tx.trim().is_empty() {
                    has_user = true;
                    t1 = t1.max(time);
                }
                busy = true;
            }
            "SYSTEM_MESSAGE" => {
                if let Some(m) = system.captures(s(&e, "content")) {
                    let sender = &m[1];
                    let tx = m[2].trim();
                    if !tx.is_empty()
                        && !tx.starts_with("[swarm] new board message")
                        && !sender.contains("/task-")
                    {
                        if !has_user {
                            busy = true
                        }
                        t1 = t1.max(time);
                    }
                }
            }
            "PLANNER_RESPONSE" => {
                let tcs = e.get("tool_calls").and_then(Value::as_array);
                let tools = tcs.is_some_and(|a| !a.is_empty());
                busy = tools
                    && !tcs
                        .unwrap()
                        .iter()
                        .any(|tc| matches!(s(tc, "name"), "swarm_wait" | "finish"));
                if tools || !s(&e, "content").trim().is_empty() {
                    t1 = t1.max(time);
                }
            }
            "GENERIC" | "ERROR_MESSAGE" => {
                t1 = t1.max(time);
            }
            _ => {}
        }
    };
    if start > 0 {
        use std::io::{BufRead, BufReader};
        let mut line = String::new();
        let _ = BufReader::new(&mut f).take(2_000_000).read_line(&mut line);
        if let Ok(e) = serde_json::from_str::<Value>(&line) {
            if matches!(s(&e, "type"), "USER_INPUT" | "SYSTEM_MESSAGE") {
                eat(e);
            }
        }
    }
    let _ = f.seek(SeekFrom::Start(start));
    let mut bytes = Vec::new();
    let _ = f.take(8_000_000).read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text.split_inclusive('\n');
    if start > 0 {
        lines.next();
    }
    for line in lines {
        if line.ends_with('\n') && line.len() <= 300_000 {
            if let Ok(e) = serde_json::from_str(line) {
                eat(e);
            }
        }
    }
    busy && crate::shared::now() - t1.max(mtime(&path)) < 300.0
}
