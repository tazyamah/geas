//! 口の層。画面（PWA）・拡張・どのクライアントからも同じ JSON の口を使う。
//!   GET  /api/queries/{名前}?入力=値     問い合わせ  → {"rows":[…]} か {"ids":[…]}
//!   POST /api/deeds/{名前}  {input, slots}   処理    → {"result","body","trace"}
//!   POST /api/events/{名前} {input}          出来事  → {"event","runs"}
//!   GET/POST /api/test/…                     テスト用の操作（模擬の設定・時計・外部作用の記録）
//! RESTful な URL（生成物の resource 宣言＝defs の routes）：GET /products、POST /orders、GET /customers/:customer_id/orders など。
//!   create の本文は処理の入力（slot は "slots" に）。結果の種類ごとの HTTP の状態は宣言どおり。クエリ文字列は使わない。
//! 画面の URL（screen 宣言から導いた resource の URL）は、Accept: text/html の GET なら画面（殻）を、それ以外は JSON を返す（形式の違い）。
//! 画面のファイルは --pwa のフォルダ（生成した汎用の画面 generated/ui）から配り、manifest は生成物の screens（design）から作る。
//! --dev のフォルダ（開発用の模擬操作）は、pwa に無いファイルだけを配る。
use crate::engine::Engine;
use serde_json::{json, Map, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tiny_http::{Header, Method, Request, Response};

/// 画面と API の応答に付ける Content-Security-Policy。スクリプトと通信先は、このサーバーのものだけ。
/// 定義の文字列が HTML に紛れても、インラインのスクリプトやイベントハンドラは動かない（エスケープの漏れの保険）。
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

fn csp() -> Header {
    Header::from_bytes("Content-Security-Policy", CSP).unwrap()
}

fn json_resp(code: u16, v: Value) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(v.to_string())
        .with_status_code(code)
        .with_header(Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap())
        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
        .with_header(csp())
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

/// インストール用の manifest。名前とテーマ色は、Geas の screen 宣言（design）から来る
fn manifest() -> Response<std::io::Cursor<Vec<u8>>> {
    let first = geas_app::screens.first();
    let accent = first.map(|s| s.accent).unwrap_or("#0b5cad");
    let bg = if first.map(|s| s.mode) == Some("dark") { "#151918" } else { "#f6f7f5" };
    let v = json!({
        "name": geas_app::APP_TITLE, "short_name": geas_app::APP_TITLE.chars().take(8).collect::<String>(), "lang": "ja",
        "start_url": "/", "scope": "/", "display": "standalone", "background_color": bg, "theme_color": accent,
        "icons": [
            { "src": "icon-192.png", "sizes": "192x192", "type": "image/png", "purpose": "any" },
            { "src": "icon-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any maskable" },
            { "src": "icon.svg", "sizes": "any", "type": "image/svg+xml" }
        ]
    });
    Response::from_string(v.to_string())
        .with_header(Header::from_bytes("Content-Type", "application/manifest+json; charset=utf-8").unwrap())
        .with_header(Header::from_bytes("Cache-Control", "no-cache").unwrap())
        .with_header(csp())
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
            .with_header(Header::from_bytes("Cache-Control", "no-cache").unwrap())
            .with_header(csp()),
        Err(_) => json_resp(404, json!({ "error": "ファイルがありません" })),
    }
}

/// リクエストの本文の上限（バイト）。長さの既定の上限（TEXT_LIMIT 文字）の文字列を数個含められる大きさ
const MAX_BODY: u64 = 16 * 1024 * 1024;

pub fn handle(mut req: Request, engine: &Mutex<Engine>, pwa: &Path, dev: &Path) {
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let method = req.method().clone();
    // 本文の大きさに上限を置く（上限を超える本文を丸ごとメモリに読み込まない）。文字列の入力は、境界で長さの既定の上限も確かめる
    let mut body = String::new();
    let _ = req.as_reader().take(MAX_BODY + 1).read_to_string(&mut body);
    if body.len() as u64 > MAX_BODY {
        let _ = req.respond(json_resp(413, json!({ "error": "リクエストの本文が大きすぎます" })));
        return;
    }
    let wants_html = req.headers().iter().any(|h| h.field.equiv("Accept") && h.value.as_str().contains("text/html"));
    let resp = if method == Method::Get && wants_html && is_screen_url(path) {
        serve_static(pwa, "/")  // 画面の URL（形式 html）：どの画面でも同じ殻を返し、画面は URL から決まる
    } else if let Some(r) = rest_route(engine, &method, path, &body) {
        r
    } else if let Some(rest) = path.strip_prefix("/api/") {
        let parsed: Value = if body.trim().is_empty() { json!({}) } else { serde_json::from_str(&body).unwrap_or(Value::Null) };
        if parsed.is_null() {
            json_resp(400, json!({ "error": "本文が JSON ではありません" }))
        } else {
            let mut en = crate::engine::lock(engine);
            let r = route(&mut en, &method, rest, query, &parsed);
            match r {
                Ok(v) => json_resp(200, v),
                Err((code, m)) => json_resp(code, json!({ "error": m })),
            }
        }
    } else if method == Method::Get && path == "/manifest.webmanifest" {
        manifest()
    } else if method == Method::Get {
        let r = serve_static(pwa, path);
        if r.status_code().0 == 404 { serve_static(dev, path) } else { r }
    } else {
        json_resp(404, json!({ "error": "ありません" }))
    };
    let _ = req.respond(resp);
}

/// 画面（screen）の URL（geas_app::screens の url。:名前 は任意の 1 区間）に当てはまるか
fn is_screen_url(path: &str) -> bool {
    let segs: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    geas_app::screens.iter().any(|s| {
        let pat: Vec<&str> = s.url.trim_end_matches('/').split('/').collect();
        !s.url.is_empty() && pat.len() == segs.len() && pat.iter().zip(&segs).all(|(p, a)| p.starts_with(':') || p == a)
    })
}

/// resource 宣言の URL に当てはまれば、その動作を実行する。当てはまらなければ None
fn rest_route(engine: &Mutex<Engine>, method: &Method, path: &str, body: &str) -> Option<Response<std::io::Cursor<Vec<u8>>>> {
    let verb = match method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Delete => "DELETE",
        _ => return None,
    };
    let segs: Vec<&str> = path.trim_matches('/').split('/').collect();
    let mut en = crate::engine::lock(engine);
    let routes = en.defs["routes"].as_array().cloned().unwrap_or_default();
    for r in routes {
        if r["method"] != verb {
            continue;
        }
        let pat = r["path"].as_str().unwrap_or("").trim_matches('/').to_string();
        let ps: Vec<&str> = pat.split('/').collect();
        if ps.len() != segs.len() {
            continue;
        }
        let mut caps: Vec<String> = Vec::new();
        if !ps.iter().zip(&segs).all(|(p, s)| if p.starts_with(':') { caps.push(decode(s)); true } else { p == s }) {
            continue;
        }
        let target = r["target"].as_str().unwrap_or("").to_string();
        let parsed: Value = if body.trim().is_empty() { json!({}) } else { serde_json::from_str(body).unwrap_or(Value::Null) };
        if parsed.is_null() {
            return Some(json_resp(400, json!({ "error": "本文が JSON ではありません" })));
        }
        let out: Result<(u16, Value), (u16, String)> = match r["kind"].as_str().unwrap_or("") {
            "index" | "show" => {
                let mut args = Map::new();
                if let (Some(c), Some(name)) = (caps.first(), en.defs["queries"][&target]["inputs"][0].as_str()) {
                    args.insert(name.to_string(), Value::String(c.clone()));
                }
                en.query(&target, &Value::Object(args)).map(|v| (200, v)).map_err(|m| (400, m))
            }
            "create" if en.defs["events"].get(&target).is_some() => en.event(&target, &parsed).map(|v| (201, v)).map_err(|m| (400, m)),
            "create" | "update" | "destroy" => {
                let mut input = parsed.clone();
                if let (Some(c), Some(k)) = (caps.first(), r["id_input"].as_str().filter(|k| !k.is_empty())) {  // /…/:id の ID は、処理の ID の入力になる
                    if let Some(m) = input.as_object_mut() {
                        m.insert(k.to_string(), Value::String(c.clone()));
                    }
                }
                let slots = input.as_object_mut().and_then(|m| m.remove("slots")).unwrap_or(json!({}));
                en.run(&target, &input, &slots).map(|v| {
                    let code = v["result"].as_str().and_then(|k| r["statuses"][k].as_u64()).unwrap_or(200) as u16;
                    (code, v)
                }).map_err(|m| (400, m))
            }
            k => Err((501, format!("動作 {} は実装していません", k))),
        };
        return Some(match out {
            Ok((code, v)) => json_resp(code, v),
            Err((code, m)) => json_resp(code, json!({ "error": m })),
        });
    }
    None
}

fn route(en: &mut Engine, method: &Method, rest: &str, query: &str, body: &Value) -> Result<Value, (u16, String)> {
    let bad = |m: String| (400u16, m);
    let parts: Vec<&str> = rest.split('/').collect();
    match (method, parts.as_slice()) {
        (Method::Get, ["health"]) => Ok(json!({ "ok": true })),
        (Method::Get, ["defs"]) => Ok(en.defs.clone()),
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
            let replies: Map<String, Value> = en.defs["mock"]["replies"].as_object().into_iter().flatten().map(|(name, r)| {
                let first = r["outcomes"][0]["name"].as_str().unwrap_or("");
                (name.clone(), json!({ "outcome": s.outcome.get(name).map(|x| x.as_str()).unwrap_or(first), "drop": s.drop.get(name).copied().unwrap_or(false) }))
            }).collect();
            Ok(json!({
                "now": en.now(), "clock_offset": en.clock_offset,
                "settings": { "delay_secs": s.delay_secs, "replies": replies },
                "dropped": en.mock.dropped, "waits": waits, "jobs": jobs, "effects": en.db.recent_effects(30).map_err(bad)?
            }))
        }
        (Method::Post, ["test", "settings"]) => {
            let known: Vec<String> = en.defs["mock"]["replies"].as_object().into_iter().flatten().map(|(k, _)| k.clone()).collect();
            let s = &mut en.mock.settings;
            if let Some(x) = body.get("delay_secs").and_then(|x| x.as_i64()) { s.delay_secs = x.clamp(0, 600); }
            for (name, v) in body.get("replies").and_then(|x| x.as_object()).into_iter().flatten() {
                if !known.contains(name) { return Err((400, format!("応答「{}」の模擬はありません", name))); }
                if let Some(x) = v.get("outcome").and_then(|x| x.as_str()) { s.outcome.insert(name.clone(), x.to_string()); }
                if let Some(x) = v.get("drop").and_then(|x| x.as_bool()) { s.drop.insert(name.clone(), x); }
            }
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
