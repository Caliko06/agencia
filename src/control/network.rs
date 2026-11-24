use super::*;
fn message_json(m: agencia_core::bus::MessageRecord) -> Value {
    let mut v = serde_json::to_value(&m).unwrap_or(Value::Null);
    v["expect_reply"] = json!(m.expect_reply != 0);
    v["delivered_ok"] = json!(m.delivered_ok != 0);
    v
}
fn view(bus: &Bus, q: &HashMap<String, String>) -> agencia_core::types::Result<Value> {
    let peers = bus.list_peers(
        q.get("provider").map(String::as_str),
        q.get("repo").map(String::as_str),
    )?;
    let mut threads = bus.list_threads()?;
    if let Some(id) = q.get("thread_id") {
        threads.retain(|t| &t.thread_id == id);
    }
    // Thread detail includes both directions and does not mark messages read.
    let mut messages = Vec::new();
    for t in threads.iter().take(200) {
        if let Some(ms) = bus.thread(&t.thread_id)?.messages {
            messages.extend(ms);
        }
    }
    // Include owner inbox even if a caller narrowed the thread selection.
    if q.get("thread_id").is_none() {
        messages.extend(bus.inbox(Some("owner"), false, 500)?.messages);
    }
    messages.sort_by(|a, b| {
        a.created_at
            .total_cmp(&b.created_at)
            .then(a.seq.cmp(&b.seq))
    });
    let mut seen = HashSet::new();
    messages.retain(|m| seen.insert(m.msg_id.clone()));
    let start = messages.len().saturating_sub(1000);
    let messages: Vec<_> = messages.into_iter().skip(start).map(message_json).collect();
    Ok(json!({"ok":true,"peers":peers,"threads":threads,"messages":messages}))
}
pub fn bus_view(q: &HashMap<String, String>) -> (u16, Value) {
    match Bus::open_default().and_then(|b| view(&b, q)) {
        Ok(v) => (200, v),
        Err(e) => response(500, false, err(e)),
    }
}
fn sent(r: agencia_core::types::Result<agencia_core::bus::SendResult>) -> (u16, Value) {
    match r {
        Ok(r) => {
            let code = if r.ok { 200 } else { 409 };
            let mut v = serde_json::to_value(&r).unwrap_or(Value::Null);
            if let Some(e) = r.error {
                v["info"] = json!(e);
            }
            (code, v)
        }
        Err(e) => response(500, false, err(e)),
    }
}
pub fn bus_send(b: &Value) -> (u16, Value) {
    let to = s(b, "to").trim();
    let body = s(b, "body").trim();
    if to.is_empty() || body.is_empty() {
        return response(400, false, "falta destinatario o mensaje");
    }
    let done = b.get("done").is_some_and(truthy);
    sent(Bus::open_default().and_then(|bus| {
        bus.send(
            to,
            body,
            s(b, "topic").trim(),
            Some("owner"),
            None,
            !done,
            agencia_core::bus::DEFAULT_MAX_HOPS,
            done,
        )
    }))
}
pub fn bus_reply(b: &Value) -> (u16, Value) {
    let id = s(b, "thread_id").trim();
    let body = s(b, "body").trim();
    if id.is_empty() || body.is_empty() {
        return response(400, false, "falta hilo o mensaje");
    }
    sent(
        Bus::open_default()
            .and_then(|bus| bus.reply(id, body, Some("owner"), b.get("done").is_some_and(truthy))),
    )
}
