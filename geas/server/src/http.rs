//! 口の層。画面（PWA）・拡張・どのクライアントからも同じ JSON の口を使う。
//!   GET  /api/queries/{名前}?入力=値     問い合わせ  → {"rows":[…]} か {"ids":[…]}
//!   POST /api/deeds/{名前}  {input, slots}   処理    → {"result","body","trace"}
//!   POST /api/events/{名前} {input}          出来事  → {"event","runs"}
//!   GET/POST /api/test/…                     テスト用の操作（模擬の設定・時計・外部作用の記録）
//!   上の queries・deeds・events・test は、--test で起こしたときだけ開く（resource の宣言を通らずに、どの処理でも名前で呼べるため）。
//!   付けないときに外から呼べるのは、仕様書の URL の表にある口（下の RESTful な URL）と、画面が使う /api/defs・/api/health だけ。
//! RESTful な URL（生成物の resource 宣言＝defs の routes）：GET /products、POST /orders、GET /customers/:customer_id/orders など。
//!   流れの途中の処理を呼ぶ URL（routes の internal。仕様書の URL の表で「内部」）は、--test のときだけ答える。
//!   create の本文は処理の入力（slot は "slots" に）。結果の種類ごとの HTTP の状態は宣言どおり。クエリ文字列は使わない。
//! 画面の URL（screen 宣言から導いた resource の URL）は、Accept: text/html の GET なら画面（殻）を、それ以外は JSON を返す（形式の違い）。
//! 画面のファイルは --pwa のフォルダ（生成した汎用の画面 generated/ui）から配り、manifest は生成物の screens（design）から作る。
//! --dev のフォルダ（開発用の模擬操作）は、--test のときだけ、pwa に無いファイルだけを配る。
//! POST は Content-Type: application/json のものだけを受ける。フォームや text/plain の POST は、別のサイトのページからでも
//! 事前の確認なしに送らせられる（CSRF）。JSON と名乗る要求は、別のサイトからだとブラウザの事前確認（CORS）で止まる。
//! Host が手元の名前（localhost・IP の書き方）か --allow-host で足した名前でなければ断る。悪意のあるページが DNS の付け替え
//! （DNS rebinding）で 127.0.0.1 に届いても、ブラウザは Host にそのページの名前を書くので、同じオリジンとして動かされない。
use crate::engine::Engine;
use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tiny_http::{Header, Method, Request, Response};

/// 画面と API の応答に付ける Content-Security-Policy。スクリプトと通信先は、このサーバーのものだけ。
/// 定義の文字列が HTML に紛れても、インラインのスクリプトやイベントハンドラは動かない（エスケープの漏れの保険）。
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

/// どの応答にも付ける守りのヘッダ：CSP、中身の種類を推測させない（nosniff）、外へ移るときに URL を渡さない
fn guard<R: Read>(r: Response<R>) -> Response<R> {
    r.with_header(Header::from_bytes("Content-Security-Policy", CSP).unwrap())
        .with_header(Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap())
        .with_header(Header::from_bytes("Referrer-Policy", "same-origin").unwrap())
}

fn json_resp(code: u16, v: Value) -> Response<std::io::Cursor<Vec<u8>>> {
    guard(
        Response::from_string(v.to_string())
            .with_status_code(code)
            .with_header(Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap())
            .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap()),
    )
}

/// %XX を読み戻す。plus なら + を空白にする（クエリ文字列の書き方）。%XX の XX が 16 進の 2 桁でなければ、そのまま残す
fn percent_decode(s: &str, plus: bool) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1).and_then(|&c| hex(c)), b.get(i + 2).and_then(|&c| hex(c))) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (b'+', _, _) if plus => {
                out.push(b' ');
                i += 1;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// パスの 1 区間（/:id の値など）を読み戻す。パスの + は空白でなく + のまま
fn decode(s: &str) -> String {
    percent_decode(s, false)
}

fn params(q: &str) -> Value {
    let mut m = Map::new();
    for kv in q.split('&').filter(|x| !x.is_empty()) {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        m.insert(percent_decode(k, true), Value::String(percent_decode(v, true)));
    }
    Value::Object(m)
}

/// 要求の本文の形が JSON と名乗っているか（Content-Type: application/json。charset などの付記は問わない）
fn says_json(req: &Request) -> bool {
    req.headers().iter().any(|h| {
        h.field.equiv("Content-Type") && h.value.as_str().split(';').next().map_or(false, |t| t.trim().eq_ignore_ascii_case("application/json"))
    })
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
    guard(
        Response::from_string(v.to_string())
            .with_header(Header::from_bytes("Content-Type", "application/manifest+json; charset=utf-8").unwrap())
            .with_header(Header::from_bytes("Cache-Control", "no-cache").unwrap()),
    )
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
        Ok(bytes) => guard(
            Response::from_data(bytes)
                .with_header(Header::from_bytes("Content-Type", mime(&p)).unwrap())
                .with_header(Header::from_bytes("Cache-Control", "no-cache").unwrap()),
        ),
        Err(_) => json_resp(404, json!({ "error": "ファイルがありません" })),
    }
}

/// リクエストの本文の上限（バイト）。長さの既定の上限（TEXT_LIMIT 文字）の文字列を数個含められる大きさ
const MAX_BODY: u64 = 16 * 1024 * 1024;

/// サーバーの起動の付けもので決まる、口の開け方
pub struct Opts {
    /// テスト用の口と、resource の宣言を通らない口と、開発用の模擬パネル（dev）を開くか（--test）
    pub test_api: bool,
    /// Host として受ける名前（小文字）。localhost・*.localhost・IP の書き方は、ここに無くても受ける（--allow-host）
    pub allow_hosts: Vec<String>,
}

/// Host の値から名前を取り出す（ポートと IPv6 の [ ] を外し、小文字にして、末尾の . を落とす）
fn host_name(v: &str) -> String {
    let v = v.trim();
    let name = match v.strip_prefix('[') {
        Some(r) => r.split(']').next().unwrap_or(""),
        None => match v.rsplit_once(':') {
            Some((n, port)) if port.bytes().all(|c| c.is_ascii_digit()) => n,
            _ => v,
        },
    };
    name.trim_end_matches('.').to_ascii_lowercase()
}

/// この Host で受けてよいか。Host の無い要求（ブラウザ以外）は受ける。
/// IP の書き方と localhost は、DNS を引かないので付け替えられない。それ以外の名前は --allow-host にあるものだけ
fn host_allowed(host: Option<&str>, allow: &[String]) -> bool {
    let Some(h) = host else { return true };
    let name = host_name(h);
    name == "localhost" || name.ends_with(".localhost") || name.parse::<std::net::IpAddr>().is_ok() || allow.iter().any(|a| *a == name)
}

pub fn handle(mut req: Request, engine: &Mutex<Engine>, pwa: &Path, dev: &Path, opts: &Opts) {
    let test_api = opts.test_api;
    let host = req.headers().iter().find(|h| h.field.equiv("Host")).map(|h| h.value.as_str().to_string());
    if !host_allowed(host.as_deref(), &opts.allow_hosts) {
        let _ = req.respond(json_resp(403, json!({ "error": "この Host の名前では受けていません。その名前で受けるなら、サーバーを --allow-host 名前 で起こしてください" })));
        return;
    }
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let method = req.method().clone();
    // 本文の大きさに上限を置く（上限を超える本文を丸ごとメモリに読み込まない）。文字列の入力は、境界で長さの既定の上限も確かめる
    let mut raw = Vec::new();
    if req.as_reader().take(MAX_BODY + 1).read_to_end(&mut raw).is_err() {
        let _ = req.respond(json_resp(400, json!({ "error": "リクエストの本文を読めません" })));
        return;
    }
    if raw.len() as u64 > MAX_BODY {
        let _ = req.respond(json_resp(413, json!({ "error": "リクエストの本文が大きすぎます" })));
        return;
    }
    // UTF-8 でない本文は、空の本文（{}）とみなさずに断る
    let Ok(body) = String::from_utf8(raw) else {
        let _ = req.respond(json_resp(400, json!({ "error": "本文が UTF-8 ではありません" })));
        return;
    };
    // 状態を変えうる POST は、JSON と名乗るものだけ（別のサイトのフォームや text/plain の POST を通さない。CSRF の対策）
    if method == Method::Post && !says_json(&req) {
        let _ = req.respond(json_resp(415, json!({ "error": "POST は Content-Type: application/json で送ってください" })));
        return;
    }
    let wants_html = req.headers().iter().any(|h| h.field.equiv("Accept") && h.value.as_str().contains("text/html"));
    let resp = if method == Method::Get && wants_html && is_screen_url(path) {
        serve_static(pwa, "/")  // 画面の URL（形式 html）：どの画面でも同じ殻を返し、画面は URL から決まる
    } else if let Some(r) = rest_route(engine, &method, path, &body, test_api) {
        r
    } else if let Some(rest) = path.strip_prefix("/api/") {
        let parsed: Value = if body.trim().is_empty() { json!({}) } else { serde_json::from_str(&body).unwrap_or(Value::Null) };
        if parsed.is_null() {
            json_resp(400, json!({ "error": "本文が JSON ではありません" }))
        } else {
            let mut en = crate::engine::lock(engine);
            let r = route(&mut en, &method, rest, query, &parsed, test_api);
            match r {
                Ok(v) => json_resp(200, v),
                Err((code, m)) => json_resp(code, json!({ "error": m })),
            }
        }
    } else if method == Method::Get && path == "/manifest.webmanifest" {
        manifest()
    } else if method == Method::Get {
        let r = serve_static(pwa, path);
        if r.status_code().0 == 404 && test_api { serve_static(dev, path) } else { r }
    } else {
        json_resp(404, json!({ "error": "ありません" }))
    };
    send(req, resp);
}

/// 応答を 1 回の書き込みで送る。tiny_http の respond は 1KB ずつ書くので、1KB を超える応答では
/// 2 回目の書き込みが Nagle のアルゴリズムと相手の遅延 ACK に挟まれ、毎回 40ms ほど待つ。
/// 中身は respond と同じ（状態行・ヘッダ・本文の作り方は tiny_http のまま）で、書き出す回数だけが違う
fn send(req: Request, resp: Response<std::io::Cursor<Vec<u8>>>) {
    let mut buf = Vec::new();
    let head_only = *req.method() == Method::Head;
    if resp.raw_print(&mut buf, req.http_version().clone(), req.headers(), head_only, None).is_err() {
        return;
    }
    let mut w = req.into_writer();
    let _ = w.write_all(&buf);
    let _ = w.flush();
}

/// 画面（screen）の URL（geas_app::screens の url。:名前 は任意の 1 区間）に当てはまるか
fn is_screen_url(path: &str) -> bool {
    let segs: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    geas_app::screens.iter().any(|s| {
        let pat: Vec<&str> = s.url.trim_end_matches('/').split('/').collect();
        !s.url.is_empty() && pat.len() == segs.len() && pat.iter().zip(&segs).all(|(p, a)| p.starts_with(':') || p == a)
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// 定義の routes（resource 宣言の URL）。生成物に埋め込まれていて、動いている間は変わらないので、最初に 1 回だけ読む
fn routes() -> &'static [Value] {
    static ROUTES: std::sync::OnceLock<Vec<Value>> = std::sync::OnceLock::new();
    ROUTES.get_or_init(|| {
        let defs: Value = serde_json::from_str(geas_app::host::DEFS_JSON).unwrap_or(Value::Null);
        defs["routes"].as_array().cloned().unwrap_or_default()
    })
}

/// resource 宣言の URL に当てはまれば、その動作を実行する。当てはまらなければ None。
/// 流れの途中の処理を呼ぶ URL（定義の internal。仕様書の URL の表で「内部」）は、test_api（--test）のときだけ当てはめる。
/// 外から直接呼ぶと、決済の返事などの確かめを飛ばして先へ進められるため
fn rest_route(engine: &Mutex<Engine>, method: &Method, path: &str, body: &str, test_api: bool) -> Option<Response<std::io::Cursor<Vec<u8>>>> {
    let started = std::time::Instant::now();
    let verb = match method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Delete => "DELETE",
        _ => return None,
    };
    let segs: Vec<&str> = path.trim_matches('/').split('/').collect();
    // 当てはまるルートを上から探す。処理の Mutex は、当てはまってから取る（画面のファイルの配信などは、処理中のリクエストを待たない）
    let matched = routes().iter().find_map(|r| {
        if r["method"] != verb || (!test_api && r["internal"] == true) {
            return None;
        }
        let ps: Vec<&str> = r["path"].as_str().unwrap_or("").trim_matches('/').split('/').collect();
        if ps.len() != segs.len() {
            return None;
        }
        let mut caps: Vec<String> = Vec::new();
        if !ps.iter().zip(&segs).all(|(p, s)| if p.starts_with(':') { caps.push(decode(s)); true } else { p == s }) {
            return None;
        }
        Some((r, caps))
    });
    if let Some((r, caps)) = matched {
        let mut en = crate::engine::lock(engine);
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
        let (code, v) = match out {
            Ok((code, v)) => (code, v),
            Err((code, m)) => (code, json!({ "error": m })),
        };
        // 構造化ログ：仕様書のルート表の行（動作・経路・呼ぶもの）と結果の種類に戻れる 1 行。本文と入力の値は書かない（`geas trace` が読む）
        eprintln!("{}", json!({
            "log": "geas.route", "ts_ms": now_ms(), "method": verb, "route": r["path"], "target": target,
            "result": v["result"], "status": code, "ms": started.elapsed().as_millis() as u64,
        }));
        return Some(json_resp(code, v));
    }
    None
}

fn route(en: &mut Engine, method: &Method, rest: &str, query: &str, body: &Value, test_api: bool) -> Result<Value, (u16, String)> {
    let bad = |m: String| (400u16, m);
    let parts: Vec<&str> = rest.split('/').collect();
    match (method, parts.as_slice()) {
        (Method::Get, ["health"]) => Ok(json!({ "ok": true })),
        (Method::Get, ["defs"]) => Ok(en.defs.clone()),
        // ここから下は --test のときだけ。名前でどの処理・問い合わせでも呼べ（仕様書の URL の表を通らない）、時計や模擬も変えられる
        _ if !test_api => Err((404, format!("口「{}」はありません", rest))),
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

#[cfg(test)]
mod tests {
    use super::{decode, host_allowed, params};

    #[test]
    // DNS の付け替えで届いたページの名前は断り、手元の名前・IP の書き方・足した名前は受ける
    fn only_local_names_ip_literals_and_allowed_hosts_are_accepted() {
        let allow = vec!["shop.example".to_string()];
        for ok in ["127.0.0.1:8787", "localhost:8787", "LOCALHOST", "app.localhost:1", "[::1]:8787", "192.168.0.5", "shop.example:443", "Shop.Example."] {
            assert!(host_allowed(Some(ok), &allow), "{}", ok);
        }
        for ng in ["evil.example:8787", "127.0.0.1.evil.example", "localhost.evil.example", "shop.example.evil", "", "0x7f.0.0.1:8787"] {
            assert!(!host_allowed(Some(ng), &allow), "{}", ng);
        }
        assert!(host_allowed(None, &[]));
    }

    #[test]
    // パスの区間では + は + のまま（空白にするのはクエリ文字列だけ）。16 進の 2 桁でない % は、そのまま残す
    fn path_keeps_plus_and_only_valid_percent_escapes_are_decoded() {
        assert_eq!(decode("a+b%20c"), "a+b c");
        assert_eq!(decode("%E3%81%82"), "あ");
        assert_eq!(decode("%+5%zz%4"), "%+5%zz%4");
        assert_eq!(params("q=a+b%2Bc")["q"], "a b+c");
    }
}
