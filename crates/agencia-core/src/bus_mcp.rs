use std::io::{self, BufRead, BufReader, BufWriter, Write};

use serde_json::{Value, json};

use crate::bus::{Bus, DEFAULT_MAX_HOPS, PeerInfo};
use crate::types::{CoreError, Result};

const SERVER_NAME: &str = "sesiones-bus";
const SERVER_VERSION: &str = "1.0.0";
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

fn io_err<E: std::fmt::Display>(err: E) -> CoreError {
    io::Error::other(err.to_string()).into()
}

pub fn tools_definition() -> Value {
    json!([
        {
            "name": "bus_list",
            "description": "List all live agent sessions across Claude Code (all accounts), Antigravity (agy), Codex, OpenCode, and registered terminals across all repos.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "provider": {"type": "string", "description": "Optional filter: claude, agy, codex, opencode, terminal"},
                    "repo": {"type": "string", "description": "Optional substring filter on repo name or path"}
                }
            }
        },
        {
            "name": "bus_whoami",
            "description": "Identify and register this session on the universal bus (optionally assigning a friendly name and role).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Optional friendly session name"},
                    "role": {"type": "string", "description": "Optional short role description"}
                }
            }
        },
        {
            "name": "bus_send",
            "description": "Send a message to any live session across providers/terminals/repos, waking it reactively (Claude UDS, Antigravity agentapi, or bus inbox) with a thread_id for autonomous ping-pong.",
            "inputSchema": {
                "type": "object",
                "required": ["to", "body"],
                "properties": {
                    "to": {"type": "string", "description": "Target peer_id, name, conversation id prefix, title substring, or repo (from bus_list)"},
                    "body": {"type": "string", "description": "Message text to deliver"},
                    "topic": {"type": "string", "description": "Optional short thread topic"},
                    "expect_reply": {"type": "boolean", "description": "Whether the receiver should reply autonomously via bus_reply (default true)"},
                    "max_hops": {"type": "integer", "description": "Max ping-pong turns allowed on this thread (default 6, max 30)"}
                }
            }
        },
        {
            "name": "bus_reply",
            "description": "Reply to an existing thread_id, automatically routing and waking up the original sender across providers until done=true or max_hops is reached.",
            "inputSchema": {
                "type": "object",
                "required": ["thread_id", "body"],
                "properties": {
                    "thread_id": {"type": "string", "description": "Thread ID from the received message"},
                    "body": {"type": "string", "description": "Reply text"},
                    "done": {"type": "boolean", "description": "Set true to close the ping-pong thread after delivering this reply"}
                }
            }
        },
        {
            "name": "bus_inbox",
            "description": "Read messages delivered to this session (or a named peer) in bus.db.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "peer": {"type": "string", "description": "Optional peer_id override"},
                    "unread_only": {"type": "boolean", "description": "Only return unread messages (default true)"},
                    "limit": {"type": "integer", "description": "Max messages to return (default 30)"}
                }
            }
        },
        {
            "name": "bus_thread",
            "description": "Inspect full message history, status, and remaining hops for a thread_id.",
            "inputSchema": {
                "type": "object",
                "required": ["thread_id"],
                "properties": {
                    "thread_id": {"type": "string", "description": "Thread ID"}
                }
            }
        },
        {
            "name": "bus_wait",
            "description": "Wait reactively for the next reply on a thread or unread inbox message (useful for Codex/OpenCode/CLI peers without native push sockets).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "thread_id": {"type": "string", "description": "Optional thread_id to wait on"},
                    "after_seq": {"type": "integer", "description": "Wait for a message with seq > after_seq"},
                    "timeout_sec": {"type": "integer", "description": "Max seconds to wait (default 60, max 300)"}
                }
            }
        },
        {
            "name": "bus_close",
            "description": "Close a thread so no further ping-pong turns are sent on it.",
            "inputSchema": {
                "type": "object",
                "required": ["thread_id"],
                "properties": {
                    "thread_id": {"type": "string", "description": "Thread ID to close"},
                    "reason": {"type": "string", "description": "Optional closing note"}
                }
            }
        }
    ])
}

fn ensure_self(bus: &Bus, self_peer: &mut Option<PeerInfo>) -> Result<PeerInfo> {
    if let Some(p) = self_peer {
        return Ok(p.clone());
    }
    let p = bus.register_peer(None, None, None, None)?;
    *self_peer = Some(p.clone());
    Ok(p)
}

fn req_str<'a>(args: &'a Value, key: &str) -> std::result::Result<&'a str, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("Missing required string argument: {key}"))
}

fn dispatch_tool(
    bus: &Bus,
    self_peer: &mut Option<PeerInfo>,
    name: &str,
    args: &Value,
) -> std::result::Result<Value, String> {
    let me = ensure_self(bus, self_peer).map_err(|e| e.to_string())?;
    match name {
        "bus_list" => {
            let provider = args.get("provider").and_then(|v| v.as_str());
            let repo = args.get("repo").and_then(|v| v.as_str());
            let peers = bus.list_peers(provider, repo).map_err(|e| e.to_string())?;
            Ok(json!({
                "self": me.peer_id,
                "count": peers.len(),
                "peers": peers,
            }))
        }
        "bus_whoami" => {
            let nm = args.get("name").and_then(|v| v.as_str());
            let role = args.get("role").and_then(|v| v.as_str());
            let updated = bus
                .register_peer(None, nm, role, None)
                .map_err(|e| e.to_string())?;
            *self_peer = Some(updated.clone());
            serde_json::to_value(updated).map_err(|e| e.to_string())
        }
        "bus_send" => {
            let to = req_str(args, "to")?;
            let body = req_str(args, "body")?;
            let topic = args.get("topic").and_then(|v| v.as_str()).unwrap_or("");
            let expect_reply = args
                .get("expect_reply")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let max_hops = args
                .get("max_hops")
                .and_then(|v| v.as_i64())
                .unwrap_or(DEFAULT_MAX_HOPS);
            let res = bus
                .send(
                    to,
                    body,
                    topic,
                    Some(&me.peer_id),
                    None,
                    expect_reply,
                    max_hops,
                    false,
                )
                .map_err(|e| e.to_string())?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }
        "bus_reply" => {
            let thread_id = req_str(args, "thread_id")?;
            let body = req_str(args, "body")?;
            let done = args.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
            let res = bus
                .reply(thread_id, body, Some(&me.peer_id), done)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }
        "bus_inbox" => {
            let peer = args
                .get("peer")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(&me.peer_id);
            let unread_only = args
                .get("unread_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(30) as usize;
            let res = bus
                .inbox(Some(peer), unread_only, limit)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }
        "bus_thread" => {
            let thread_id = req_str(args, "thread_id")?;
            let res = bus.thread(thread_id).map_err(|e| e.to_string())?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }
        "bus_wait" => {
            let thread_id = args.get("thread_id").and_then(|v| v.as_str());
            let after_seq = args.get("after_seq").and_then(|v| v.as_i64()).unwrap_or(0);
            let timeout_sec = args
                .get("timeout_sec")
                .and_then(|v| v.as_u64())
                .unwrap_or(60);
            let res = bus
                .wait(Some(&me.peer_id), thread_id, after_seq, timeout_sec)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }
        "bus_close" => {
            let thread_id = req_str(args, "thread_id")?;
            let reason = args.get("reason").and_then(|v| v.as_str()).unwrap_or("");
            let res = bus.close(thread_id, reason).map_err(|e| e.to_string())?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }
        other => Err(format!("Unknown tool: {other}")),
    }
}

fn invalid_request_error(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32600,
            "message": "Invalid Request"
        }
    })
}

pub fn handle_request_with_session(
    bus: &Bus,
    self_peer: &mut Option<PeerInfo>,
    req: &Value,
) -> Option<Value> {
    if let Value::Array(items) = req {
        if items.is_empty() {
            return Some(invalid_request_error(Value::Null));
        }
        let responses: Vec<Value> = items
            .iter()
            .filter_map(|item| handle_request_with_session(bus, self_peer, item))
            .collect();
        return if responses.is_empty() {
            None
        } else {
            Some(Value::Array(responses))
        };
    }

    if !req.is_object() {
        return Some(invalid_request_error(Value::Null));
    }

    let rid = req.get("id").cloned();
    let Some(method) = req.get("method").and_then(|v| v.as_str()) else {
        return Some(invalid_request_error(rid.unwrap_or(Value::Null)));
    };
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

    if method == "notifications/initialized" || rid.is_none() || rid.as_ref() == Some(&Value::Null) {
        return None;
    }
    let id = rid.unwrap();

    match method {
        "initialize" => {
            let proto = params
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("2024-11-05");
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": proto,
                    "capabilities": {"tools": {}},
                    "serverInfo": {
                        "name": SERVER_NAME,
                        "version": SERVER_VERSION,
                    }
                }
            }))
        }
        "ping" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {}
        })),
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": tools_definition()
            }
        })),
        "tools/call" => {
            let tname = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let targs = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match dispatch_tool(bus, self_peer, tname, &targs) {
                Ok(res) => {
                    let text = serde_json::to_string_pretty(&res)
                        .unwrap_or_else(|_| "{}".to_string());
                    Some(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{"type": "text", "text": text}],
                            "isError": false
                        }
                    }))
                }
                Err(err) => {
                    let text = json!({"ok": false, "error": err}).to_string();
                    Some(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{"type": "text", "text": text}],
                            "isError": true
                        }
                    }))
                }
            }
        }
        other => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32601,
                "message": format!("Method not found: {other}")
            }
        })),
    }
}

pub fn handle_request(bus: &Bus, req: &Value) -> Option<Value> {
    let mut self_peer = None;
    handle_request_with_session(bus, &mut self_peer, req)
}

enum ReadOutcome {
    Eof,
    Message(Value),
    ParseError(String),
}

fn is_mime_header_line(trimmed: &str) -> bool {
    if trimmed.is_empty() || trimmed.starts_with('{') || trimmed.starts_with('[') {
        return false;
    }
    if let Some((hdr, _)) = trimmed.split_once(':') {
        let name = hdr.trim();
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    } else {
        false
    }
}

fn parse_header_line(
    hdr_line: &str,
    content_length: &mut Option<usize>,
    header_err: &mut Option<String>,
) {
    if hdr_line.to_ascii_lowercase().starts_with("content-length:")
        && let Some((_, raw_val)) = hdr_line.split_once(':')
    {
        match raw_val.trim().parse::<usize>() {
            Ok(len) if len > MAX_FRAME_BYTES => {
                header_err.get_or_insert_with(|| {
                    format!(
                        "Content-Length {len} exceeds maximum frame size ({MAX_FRAME_BYTES} bytes)"
                    )
                });
            }
            Ok(len) => {
                *content_length = Some(len);
            }
            Err(e) => {
                header_err.get_or_insert_with(|| format!("invalid Content-Length: {e}"));
            }
        }
    }
}

fn read_next_message<R: BufRead>(reader: &mut R, framed: &mut bool) -> io::Result<ReadOutcome> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(ReadOutcome::Eof);
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_mime_header_line(trimmed) {
            *framed = true;
            let mut content_length: Option<usize> = None;
            let mut header_err: Option<String> = None;
            parse_header_line(trimmed, &mut content_length, &mut header_err);

            loop {
                line.clear();
                let hn = reader.read_line(&mut line)?;
                if hn == 0 {
                    break;
                }
                let hdr_trimmed = line.trim_end_matches(&['\r', '\n'][..]);
                if hdr_trimmed.is_empty() {
                    break;
                }
                parse_header_line(hdr_trimmed, &mut content_length, &mut header_err);
            }

            if let Some(err) = header_err {
                return Ok(ReadOutcome::ParseError(err));
            }
            let Some(len) = content_length else {
                return Ok(ReadOutcome::ParseError(
                    "missing Content-Length header".to_string(),
                ));
            };
            let mut body = vec![0u8; len];
            if let Err(e) = reader.read_exact(&mut body) {
                return Ok(ReadOutcome::ParseError(format!(
                    "incomplete Content-Length body: {e}"
                )));
            }
            return match serde_json::from_slice::<Value>(&body) {
                Ok(v) => Ok(ReadOutcome::Message(v)),
                Err(e) => Ok(ReadOutcome::ParseError(e.to_string())),
            };
        }
        return match serde_json::from_str::<Value>(trimmed) {
            Ok(v) => Ok(ReadOutcome::Message(v)),
            Err(e) => Ok(ReadOutcome::ParseError(e.to_string())),
        };
    }
}

fn write_message<W: Write>(writer: &mut W, obj: &Value, framed: bool) -> io::Result<()> {
    let raw = serde_json::to_vec(obj)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if framed {
        write!(writer, "Content-Length: {}\r\n\r\n", raw.len())?;
        writer.write_all(&raw)?;
    } else {
        writer.write_all(&raw)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()
}

pub fn run_mcp_io<R: BufRead, W: Write>(bus: &Bus, mut reader: R, mut writer: W) -> Result<()> {
    let mut self_peer: Option<PeerInfo> = None;
    let _ = ensure_self(bus, &mut self_peer);
    let mut framed = false;

    loop {
        match read_next_message(&mut reader, &mut framed).map_err(io_err)? {
            ReadOutcome::Eof => break,
            ReadOutcome::ParseError(msg) => {
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {
                        "code": -32700,
                        "message": msg
                    }
                });
                write_message(&mut writer, &err_resp, framed).map_err(io_err)?;
            }
            ReadOutcome::Message(req) => {
                if let Some(resp) = handle_request_with_session(bus, &mut self_peer, &req) {
                    write_message(&mut writer, &resp, framed).map_err(io_err)?;
                }
            }
        }
    }
    Ok(())
}

pub fn run_mcp_stdio(bus: &Bus) -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let reader = BufReader::new(stdin.lock());
    let writer = BufWriter::new(stdout.lock());
    run_mcp_io(bus, reader, writer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::uuid_v4;
    use std::fs;
    use std::io::Cursor;
    use std::path::PathBuf;

    struct CleanDir(PathBuf);
    impl CleanDir {
        fn new(name: &str) -> Self {
            let base = std::env::temp_dir().join(format!("agencia-mcp-{name}-{}", uuid_v4()));
            fs::create_dir_all(&base).unwrap();
            Self(base)
        }
        fn path(&self) -> &PathBuf {
            &self.0
        }
    }
    impl Drop for CleanDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn handle_request_initialize_ping_tools_list_and_unknown() {
        let dir = CleanDir::new("basic");
        let bus = Bus::open(dir.path().join("bus.db")).unwrap();

        let init = handle_request(
            &bus,
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}}),
        )
        .unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "sesiones-bus");

        let notif = handle_request(
            &bus,
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        );
        assert!(notif.is_none());

        let ping = handle_request(&bus, &json!({"jsonrpc": "2.0", "id": 2, "method": "ping"})).unwrap();
        assert_eq!(ping["result"], json!({}));

        let tools = handle_request(&bus, &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"})).unwrap();
        let arr = tools["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = arr.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec![
                "bus_list",
                "bus_whoami",
                "bus_send",
                "bus_reply",
                "bus_inbox",
                "bus_thread",
                "bus_wait",
                "bus_close"
            ]
        );

        let unk = handle_request(&bus, &json!({"jsonrpc": "2.0", "id": 4, "method": "foo/bar"})).unwrap();
        assert_eq!(unk["error"]["code"], -32601);
    }

    #[test]
    fn run_mcp_io_ndjson_roundtrip_all_tools() {
        let dir = CleanDir::new("ndjson");
        let bus = Bus::open(dir.path().join("bus.db")).unwrap();
        bus.register_peer(Some("codex:target"), Some("target"), None, Some("codex"))
            .unwrap();

        let input = [
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "bus_whoami", "arguments": {"name": "tester", "role": "qa"}}}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "bus_list", "arguments": {}}}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "bus_send", "arguments": {"to": "codex:target", "body": "ping from mcp", "topic": "mcp"}}}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "bus_inbox", "arguments": {"peer": "codex:target"}}}),
            json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "unknown_tool", "arguments": {}}}),
        ]
        .iter()
        .map(|v| serde_json::to_string(v).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
            + "\n";

        let mut out = Vec::new();
        run_mcp_io(&bus, Cursor::new(input.as_bytes()), &mut out).unwrap();

        let out_str = String::from_utf8(out).unwrap();
        let lines: Vec<Value> = out_str
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 5);

        assert_eq!(lines[0]["result"]["isError"], false);
        assert_eq!(lines[1]["result"]["isError"], false);

        let send_text = lines[2]["result"]["content"][0]["text"].as_str().unwrap();
        let send_val: Value = serde_json::from_str(send_text).unwrap();
        assert_eq!(send_val["ok"], true);
        let tid = send_val["thread_id"].as_str().unwrap().to_string();

        let inbox_text = lines[3]["result"]["content"][0]["text"].as_str().unwrap();
        let inbox_val: Value = serde_json::from_str(inbox_text).unwrap();
        assert_eq!(inbox_val["count"], 1);
        assert_eq!(inbox_val["messages"][0]["body"], "ping from mcp");

        assert_eq!(lines[4]["result"]["isError"], true);

        // Now test reply, thread, wait, close via handle_request
        let r_reply = handle_request(
            &bus,
            &json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {"name": "bus_reply", "arguments": {"thread_id": tid, "body": "pong", "done": false}}}),
        )
        .unwrap();
        assert_eq!(r_reply["result"]["isError"], false);

        let r_thread = handle_request(
            &bus,
            &json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": "bus_thread", "arguments": {"thread_id": tid}}}),
        )
        .unwrap();
        assert_eq!(r_thread["result"]["isError"], false);

        let r_wait = handle_request(
            &bus,
            &json!({"jsonrpc": "2.0", "id": 8, "method": "tools/call", "params": {"name": "bus_wait", "arguments": {"thread_id": tid, "after_seq": 99, "timeout_sec": 1}}}),
        )
        .unwrap();
        let wait_val: Value = serde_json::from_str(
            r_wait["result"]["content"][0]["text"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(wait_val["timed_out"], true);

        let r_close = handle_request(
            &bus,
            &json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": {"name": "bus_close", "arguments": {"thread_id": tid, "reason": "done"}}}),
        )
        .unwrap();
        assert_eq!(r_close["result"]["isError"], false);
    }

    #[test]
    fn run_mcp_io_content_length_framing() {
        let dir = CleanDir::new("framed");
        let bus = Bus::open(dir.path().join("bus.db")).unwrap();

        let p1 = serde_json::to_string(&json!({"jsonrpc": "2.0", "id": 10, "method": "initialize"})).unwrap();
        let p2 = serde_json::to_string(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).unwrap();
        let p3 = serde_json::to_string(&json!({"jsonrpc": "2.0", "id": 11, "method": "ping"})).unwrap();

        let input = format!(
            "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
            p1.len(),
            p1,
            p2.len(),
            p2,
            p3.len(),
            p3
        );
        let mut out = Vec::new();
        run_mcp_io(&bus, Cursor::new(input.as_bytes()), &mut out).unwrap();

        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.starts_with("Content-Length: "));
        assert!(out_str.contains("\"id\":10"));
        assert!(out_str.contains("\"id\":11"));
    }

    #[test]
    fn run_mcp_io_reports_parse_error() {
        let dir = CleanDir::new("parse-err");
        let bus = Bus::open(dir.path().join("bus.db")).unwrap();
        let input = "{not valid json\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n";
        let mut out = Vec::new();
        run_mcp_io(&bus, Cursor::new(input.as_bytes()), &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        let lines: Vec<Value> = s.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["error"]["code"], -32700);
        assert_eq!(lines[1]["id"], 1);
    }

    #[test]
    fn run_mcp_io_content_type_before_content_length_has_no_parse_error() {
        let dir = CleanDir::new("ct-before-cl");
        let bus = Bus::open(dir.path().join("bus.db")).unwrap();

        let p1 = serde_json::to_string(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}))
            .unwrap();
        let p2 = serde_json::to_string(&json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}))
            .unwrap();

        let input = format!(
            "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}X-Extra: foo\r\ncontent-length: {}\r\n\r\n{}",
            p1.len(),
            p1,
            p2.len(),
            p2
        );
        let mut out = Vec::new();
        run_mcp_io(&bus, Cursor::new(input.as_bytes()), &mut out).unwrap();

        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.starts_with("Content-Length: "));
        assert!(!out_str.contains("-32700"));

        // Parse every Content-Length frame out of out_str to verify exact framing.
        let mut rest = out_str.as_str();
        let mut ids = Vec::new();
        while !rest.is_empty() {
            let (hdr, after_hdr) = rest.split_once("\r\n\r\n").expect("valid frame header");
            let len: usize = hdr
                .strip_prefix("Content-Length: ")
                .expect("Content-Length prefix")
                .parse()
                .expect("integer length");
            let (body, tail) = after_hdr.split_at(len);
            let val: Value = serde_json::from_str(body).expect("valid framed JSON");
            assert!(val.get("error").is_none());
            ids.push(val["id"].as_i64().unwrap());
            rest = tail;
        }
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn run_mcp_io_non_object_json_returns_invalid_request_32600() {
        let dir = CleanDir::new("non-obj");
        let bus = Bus::open(dir.path().join("bus.db")).unwrap();

        let input = "123\n\"hi\"\ntrue\nnull\n[]\n";
        let mut out = Vec::new();
        run_mcp_io(&bus, Cursor::new(input.as_bytes()), &mut out).unwrap();

        let s = String::from_utf8(out).unwrap();
        let lines: Vec<Value> = s.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 5);
        for line in lines {
            assert_eq!(line["jsonrpc"], "2.0");
            assert_eq!(line["id"], Value::Null);
            assert_eq!(line["error"]["code"], -32600);
        }
    }

    #[test]
    fn run_mcp_io_oversized_content_length_returns_parse_error() {
        let dir = CleanDir::new("oversized");
        let bus = Bus::open(dir.path().join("bus.db")).unwrap();

        let oversized = MAX_FRAME_BYTES + 1;
        let input = format!("Content-Length: {oversized}\r\n\r\n");
        let mut out = Vec::new();
        run_mcp_io(&bus, Cursor::new(input.as_bytes()), &mut out).unwrap();

        let s = String::from_utf8(out).unwrap();
        assert!(s.starts_with("Content-Length: "));
        let (_, body) = s.split_once("\r\n\r\n").unwrap();
        let val: Value = serde_json::from_str(body).unwrap();
        assert_eq!(val["id"], Value::Null);
        assert_eq!(val["error"]["code"], -32700);
    }
}
