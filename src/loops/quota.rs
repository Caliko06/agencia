use super::*;
use chrono::{Datelike, TimeZone, Utc};
#[derive(Default)]
struct Accounts {
    t: f64,
    v: Vec<u32>,
    raw: Vec<Value>,
}
static ACCS: LazyLock<Mutex<Accounts>> = LazyLock::new(|| {
    let mut a = Accounts::default();
    if let Some(v) = load(PathBuf::from(home()).join(".cache/claude-acc/tui.json")) {
        if let Some(rows) = v["pool"]["accounts"].as_object() {
            a.raw = rows
                .iter()
                .map(|(k, v)| {
                    let mut v = v.clone();
                    v["n"] = json!(k.parse::<u32>().unwrap_or(0));
                    v
                })
                .collect();
            let mut ok: Vec<_> = a
                .raw
                .iter()
                .filter(|r| {
                    r["logged"] == true
                        && n(&r["usage"]["week"], "resetMs") > 0.
                        && r["usage"]["week"]["pct"].as_f64().unwrap_or(100.) < 90.
                        && r["usage"]["session"]["pct"].as_f64().unwrap_or(100.) < 90.
                })
                .map(|r| (n(&r["usage"]["week"], "resetMs"), n(r, "n") as u32))
                .collect();
            ok.sort_by(|x, y| x.0.total_cmp(&y.0));
            a.v = ok.into_iter().map(|x| x.1).collect();
            a.t = now();
        }
    }
    Mutex::new(a)
});
pub(super) static BAD: LazyLock<Mutex<std::collections::HashMap<u32, f64>>> =
    LazyLock::new(|| Mutex::new(Default::default()));
pub fn runway() -> Vec<u32> {
    let mut a = ACCS.lock();
    if now() - a.t > 180. && loops_enabled() {
        a.t = now();
        thread::spawn(|| {
            let mut c = Command::new("claude-acc");
            c.args(["usage", "--json"]);
            if let Ok(o) = command(c, 180) {
                if let Ok(Value::Array(rows)) = serde_json::from_slice(&o.stdout) {
                    let mut ok: Vec<_> = rows
                        .iter()
                        .filter(|r| {
                            r["logged"] == true
                                && n(&r["usage"]["week"], "resetMs") > 0.
                                && r["usage"]["week"]["pct"].as_f64().unwrap_or(100.) < 90.
                                && r["usage"]["session"]["pct"].as_f64().unwrap_or(100.) < 90.
                        })
                        .map(|r| (n(&r["usage"]["week"], "resetMs"), n(r, "n") as u32))
                        .collect();
                    ok.sort_by(|a, b| a.0.total_cmp(&b.0));
                    let mut a = ACCS.lock();
                    a.v = ok.into_iter().map(|x| x.1).collect();
                    a.raw = rows;
                }
            }
        });
    }
    let v = if a.v.is_empty() { vec![3] } else { a.v.clone() };
    let bad = BAD.lock();
    v.into_iter()
        .filter(|n| bad.get(n).copied().unwrap_or(0.) < now())
        .collect()
}
#[derive(Default)]
struct Quota {
    t: f64,
    run: bool,
    v: Value,
}
static Q: LazyLock<Mutex<Quota>> = LazyLock::new(|| {
    let initial = load(cache_dir().join("quota.json"))
        .filter(|v| v.is_object())
        .unwrap_or_else(|| json!({"claude": [], "codex": [], "go": []}));
    Mutex::new(Quota {
        t: 0.0,
        run: false,
        v: initial,
    })
});
pub fn quota() -> Value {
    let mut q = Q.lock();
    if now() - q.t > 60. && !q.run {
        q.t = now();
        q.run = true;
        thread::spawn(|| {
            let v = read();
            save(cache_dir().join("quota.json"), &v);
            let mut q = Q.lock();
            if v["claude"].as_array().is_none_or(|a| a.is_empty()) {
                q.t -= 55.;
            }
            q.v = v;
            q.run = false;
        });
    }
    q.v.clone()
}
fn since() -> [i64; 3] {
    let dt = Utc::now();
    let day = dt.date_naive();
    let wk = day - chrono::Duration::days(dt.weekday().num_days_from_monday() as i64);
    let (y, m) = if dt.day() >= 30
        || (dt.month() == 2 && dt.day() >= 28 && day.succ_opt().is_some_and(|d| d.month() != 2))
    {
        (dt.year(), dt.month())
    } else if dt.month() == 1 {
        (dt.year() - 1, 12)
    } else {
        (dt.year(), dt.month() - 1)
    };
    let mut d = 30;
    while chrono::NaiveDate::from_ymd_opt(y, m, d).is_none() {
        d -= 1;
    }
    [
        (now() * 1000.) as i64 - 18_000_000,
        Utc.from_utc_datetime(&wk.and_hms_opt(0, 0, 0).unwrap())
            .timestamp_millis(),
        Utc.with_ymd_and_hms(y, m, d, 0, 0, 0)
            .unwrap()
            .timestamp_millis(),
    ]
}
fn read() -> Value {
    runway();
    let cl:Vec<_>=ACCS.lock().raw.iter().filter(|r|r["logged"]==true && r["usage"].is_object()).map(|r|json!({"n":r["n"],"name":format!("Cuenta {}",r["n"]),"s":r["usage"]["session"]["pct"],"w":r["usage"]["week"]["pct"],"sr":r["usage"]["session"].get("reset").cloned().unwrap_or(json!("")),"wr":r["usage"]["week"].get("reset").cloned().unwrap_or(json!(""))})).collect();
    let mut cx = vec![];
    for (who_key, base) in agencia_core::jobs::discover_codex_homes() {
        let name = if who_key == "default" {
            std::env::var("AGENCIA_CODEX_NAME")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "Codex".into())
        } else {
            who_key.clone()
        };
        let mut files = paths(&format!("{}/sessions/*/*/*/rollout-*.jsonl", base.display()));
        files.extend(paths(&format!(
            "{}/archived_sessions/rollout-*.jsonl",
            base.display()
        )));
        files.sort_by(|a, b| mtime(b).total_cmp(&mtime(a)));
        for f in files.iter().take(3) {
            let tx = tail(f, 400_000);
            let Some(i) = tx.rfind("\"rate_limits\":{") else {
                continue;
            };
            let Some(Ok(rl)) = serde_json::Deserializer::from_str(&tx[i + 14..])
                .into_iter::<Value>()
                .next()
            else {
                continue;
            };
            cx.push(json!({"name":name,"s":if n(&rl["primary"],"resets_at")>now(){rl["primary"]["used_percent"].clone()}else{json!(0)},"w":if n(&rl["secondary"],"resets_at")>now(){rl["secondary"]["used_percent"].clone()}else{json!(0)},"sr":rl["primary"]["resets_at"],"wr":rl["secondary"]["resets_at"]}));
            break;
        }
    }
    let mut go = vec![];
    if let Ok(db) = rusqlite::Connection::open_with_flags(
        format!("{}/.local/share/opencode/opencode.db", home()),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        let _ = db.busy_timeout(Duration::from_millis(100));
        // Bound indexed time range; the query is off the snapshot path and interrupted after 2 seconds.
        let interrupt = db.get_interrupt_handle();
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        thread::spawn(move || {
            if rx.recv_timeout(Duration::from_secs(2)).is_err() {
                interrupt.interrupt();
            }
        });
        let ms = since();
        let low = *ms.iter().min().unwrap();
        let sql = "SELECT coalesce(sum(case when time_created>?1 then json_extract(data,'$.cost') end),0),coalesce(sum(case when time_created>?2 then json_extract(data,'$.cost') end),0),coalesce(sum(case when time_created>?3 then json_extract(data,'$.cost') end),0) FROM session_message WHERE time_created>?4 AND type='assistant' AND json_extract(data,'$.model.providerID')='opencode-go' AND json_extract(data,'$.model.id') LIKE 'deepseek%'";
        if let Ok(row) = db.query_row(sql, rusqlite::params![ms[0], ms[1], ms[2], low], |r| {
            Ok([
                r.get::<_, f64>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, f64>(2)?,
            ])
        }) {
            for ((label, cap), usd) in [("5 h", 20.), ("semana", 50.), ("mes", 100.)]
                .into_iter()
                .zip(row)
            {
                go.push(json!({"label":label,"usd":(usd*100.).round()/100.,"cap":cap as i32,"pct":(100.*usd/cap).round()}));
            }
        }
        let _ = tx.send(());
    }
    json!({"claude":cl,"codex":cx,"go":go})
}
