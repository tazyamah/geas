//! 口の層。画面（PWA）・拡張・どのクライアントからも同じ JSON の口を使う。
//!   GET  /api/queries/{名前}?入力=値     問い合わせ  → {"rows":[…]} か {"ids":[…]}
//!   POST /api/deeds/{名前}  {input, slots}   処理    → {"result","body","trace"}
//!   POST /api/events/{名前} {input}          出来事  → {"event","runs"}
//!   GET/POST /api/test/…                     テスト用の操作（模擬の設定・時計・外部作用の記録）
//! 画面のファイルは --pwa のフォルダから配る。
use crate::engine::Engine;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tiny_http::{Header, Method, Request, Response};

fn json_resp(code: u16, v: Value) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(v.to_string())
        .with_status_code(code)
        .with_header(Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap())
        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let h = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
                if let Ok(v) = u8::from_str_radix(h, 16) {
                    out.push(v);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn params(q: &str) -> Value {
    let mut m = Map::new();
    for kv in q.split('&').filter(|x| !x.is_empty()) {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        m.insert(decode(k), Value::String(decode(v)));
    }
    Value::Object(m)
}

fn mime(p: &Path) -> &'static str {
    match p.extension().and_then(|x| x.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "webmanifest" => "application/manifest+json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        _ => "application/octet-stream",
    }
}

fn serve_static(root: &Path, url_path: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let rel = if url_path == "/" { "index.html" } else { url_path.trim_start_matches('/') };
    let mut p = PathBuf::from(root);
    for part in rel.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains('\\') {
            return json_resp(404, json!({ "error": "ファイルがありません" }));
        }
        p.push(part);
    }
    match std::fs::read(&p) {
        Ok(bytes) => Response::from_data(bytes)
            .with_header(Header::from_bytes("Content-Type", mime(&p)).unwrap())
            .with_header(Header::from_bytes("Cache-Control", "no-cache").unwrap()),
        Err(_) => json_resp(404, json!({ "error": "ファイルがありません" })),
    }
}

pub fn handle(mut req: Request, engine: &Mutex<Engine>, pwa: &Path) {
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let method = req.method().clone();
    let mut body = String::new();
    let _ = req.as_reader().read_to_string(&mut body);
    let resp = if let Some(rest) = path.strip_prefix("/api/") {
        let parsed: Value = if body.trim().is_empty() { json!({}) } else { serde_json::from_str(&body).unwrap_or(Value::Null) };
        if parsed.is_null() {
            json_resp(400, json!({ "error": "本文が JSON ではありません" }))
        } else {
            let mut en = engine.lock().unwrap();
            let r = route(&mut en, &method, rest, query, &parsed);
            match r {
                Ok(v) => json_resp(200, v),
                Err((code, m)) => json_resp(code, json!({ "error": m })),
            }
        }
    } else if method == Method::Get {
        serve_static(pwa, path)
    } else {
        json_resp(404, json!({ "error": "ありません" }))
    };
    let _ = req.respond(resp);
}

fn route(en: &mut Engine, method: &Method, rest: &str, query: &str, body: &Value) -> Result<Value, (u16, String)> {
    let bad = |m: String| (400u16, m);
    let parts: Vec<&str> = rest.split('/').collect();
    match (method, parts.as_slice()) {
        (Method::Get, ["health"]) => Ok(json!({ "ok": true })),
        (Method::Get, ["queries", name]) => en.query(name, &params(query)).map_err(bad),
        (Method::Post, ["deeds", name]) => {
            let input = body.get("input").cloned().unwrap_or(json!({}));
            let slots = body.get("slots").cloned().unwrap_or(json!({}));
            en.run(name, &input, &slots).map_err(bad)
        }
        (Method::Post, ["events", name]) => en.event(name, &body.get("input").cloned().unwrap_or(json!({}))).map_err(bad),
        // ---- テスト用 ----
        (Method::Get, ["test", "state"]) => {
            let waits: Vec<Value> = en.db.all_waits().map_err(bad)?.into_iter().map(|w| json!({ "trigger": w.trigger, "args": w.args, "collected": w.collected, "deadline": w.deadline })).collect();
            let jobs: Vec<Value> = en.mock.jobs.iter().map(|j| json!({ "due": j.due, "effect": j.effect, "reply": j.reply, "trigger": j.trigger, "args": j.trigger_args })).collect();
            let s = &en.mock.settings;
            Ok(json!({
                "now": en.now(), "clock_offset": en.clock_offset,
                "settings": { "drop_payment_reply": s.drop_payment_reply, "warehouse_short": s.warehouse_short, "delay_secs": s.delay_secs, "decline_from": s.decline_from },
                "dropped": en.mock.dropped, "waits": waits, "jobs": jobs, "effects": en.db.recent_effects(30).map_err(bad)?
            }))
        }
        (Method::Post, ["test", "settings"]) => {
            let s = &mut en.mock.settings;
            if let Some(x) = body.get("drop_payment_reply").and_then(|x| x.as_bool()) { s.drop_payment_reply = x; }
            if let Some(x) = body.get("warehouse_short").and_then(|x| x.as_bool()) { s.warehouse_short = x; }
            if let Some(x) = body.get("delay_secs").and_then(|x| x.as_i64()) { s.delay_secs = x.clamp(0, 600); }
            if let Some(x) = body.get("decline_from").and_then(|x| x.as_i64()) { s.decline_from = x.max(1); }
            Ok(json!({ "ok": true }))
        }
        (Method::Post, ["test", "advance"]) => {
            en.clock_offset += body.get("secs").and_then(|x| x.as_i64()).unwrap_or(0).clamp(0, 86_400);
            let fired = en.tick().map_err(bad)?;
            Ok(json!({ "fired": fired }))
        }
        (Method::Post, ["test", "tick"]) => Ok(json!({ "fired": en.tick().map_err(bad)? })),
        _ => Err((404, format!("口「{}」はありません", rest))),
    }
}
