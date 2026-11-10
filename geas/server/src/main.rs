//! Geas が生成したアプリのサーバー。localhost で動き、DB は SQLite。geas rust --host --runtime が、生成物と一緒に配る。
//! 使い方: geas_server [--port 8787] [--host 127.0.0.1] [--workers 4] [--db app.sqlite] [--pwa ../ui] [--test [--dev ../dev]] [--allow-host 名前] [--seed 別の最初のデータ.json]
//! --test：テスト用の口（/api/test/…：時計を進める・模擬の設定など）と、resource の宣言を通らない口（/api/deeds/… など）、
//!         開発用の模擬パネル（--dev のフォルダ）を開く。付けなければ、外から呼べるのは仕様書の URL の表にある口だけ。公開するサーバーには付けない
//! --allow-host：localhost と IP の書き方のほかに、Host として受ける名前（何度でも書ける。カンマ区切りでもよい）。それ以外の名前の要求は 403
mod app;
mod db;
mod engine;
mod http;
mod mock;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// tiny_http の要求を、運び方によらない形（http::Req）にして処理し、答えを返す。
/// 本文は上限（MAX_BODY）を 1 バイト超えるところまでしか読まない（超える本文を丸ごとメモリに読み込まない。超えた分は処理側が 413 で断る）
fn serve(mut req: tiny_http::Request, engine: &Mutex<engine::Engine>, pwa: &std::path::Path, dev: &std::path::Path, opts: &http::Opts) {
    let mut raw = Vec::new();
    if req.as_reader().take(http::MAX_BODY + 1).read_to_end(&mut raw).is_err() {
        let _ = req.respond(tiny_http::Response::from_string(r#"{"error":"リクエストの本文を読めません"}"#).with_status_code(400));
        return;
    }
    let r = http::Req {
        method: req.method().as_str().to_string(),
        url: req.url().to_string(),
        headers: req.headers().iter().map(|h| (h.field.as_str().as_str().to_string(), h.value.as_str().to_string())).collect(),
        body: raw,
    };
    let out = http::handle(r, engine, pwa, dev, opts);
    send(req, out);
}

/// 応答を 1 回の書き込みで送る。tiny_http の respond は 1KB ずつ書くので、1KB を超える応答では
/// 2 回目の書き込みが Nagle のアルゴリズムと相手の遅延 ACK に挟まれ、毎回 40ms ほど待つ。
/// 中身は respond と同じ（状態行・ヘッダ・本文の作り方は tiny_http のまま）で、書き出す回数だけが違う
fn send(req: tiny_http::Request, out: http::Resp) {
    let mut resp = tiny_http::Response::from_data(out.body).with_status_code(out.status);
    for (k, v) in &out.headers {
        if let Ok(h) = tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()) {
            resp = resp.with_header(h);
        }
    }
    let mut buf = Vec::new();
    let head_only = *req.method() == tiny_http::Method::Head;
    if resp.raw_print(&mut buf, req.http_version().clone(), req.headers(), head_only, None).is_err() {
        return;
    }
    let mut w = req.into_writer();
    let _ = w.write_all(&buf);
    let _ = w.flush();
}

fn arg(name: &str, default: &str) -> String {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == name).and_then(|i| a.get(i + 1).cloned()).unwrap_or_else(|| default.to_string())
}

fn main() {
    let port = arg("--port", "8787");
    let host = arg("--host", "127.0.0.1");
    // リクエストを処理するスレッドの数。遅い本文を送る相手が worker を占めるので、公開するなら nginx などを前に置く（docs/server.md）
    let workers: usize = match arg("--workers", "4").parse() {
        Ok(n) if (1..=256).contains(&n) => n,
        _ => {
            eprintln!("--workers は 1〜256 の整数で指定してください");
            std::process::exit(2);
        }
    };
    let db_path = arg("--db", "app.sqlite");
    let pwa = PathBuf::from(arg("--pwa", "../ui"));
    let dev = PathBuf::from(arg("--dev", "../dev"));
    // テスト用の口は、付けたときだけ開く（時計を進める・模擬の返事を変えるなどは、誰が叩いても全員の注文に効く）
    let test_api = std::env::args().any(|a| a == "--test");
    // Host として受ける名前。名前で書いた --host も受ける（DNS の付け替えで届いたよそのページの名前は断る）
    let args: Vec<String> = std::env::args().collect();
    let mut allow_hosts: Vec<String> = args
        .windows(2)
        .filter(|w| w[0] == "--allow-host")
        .flat_map(|w| w[1].split(',').map(|h| h.trim().trim_end_matches('.').to_ascii_lowercase()).collect::<Vec<_>>())
        .filter(|h| !h.is_empty())
        .collect();
    allow_hosts.push(host.trim_end_matches('.').to_ascii_lowercase());
    let opts = Arc::new(http::Opts { test_api, allow_hosts });
    // 最初のデータは生成物に埋め込んである（geas rust --seed）。別のものを使うときだけ --seed。読むのは DB が空のときだけ
    let seed_path = arg("--seed", "");

    // 保存にある実体の名前は、生成物から取る（空の保存を書き出すと、全部の実体が並ぶ）
    let empty: serde_json::Value = serde_json::from_str(&geas_app::host::write_store(&geas_app::Store::default())).expect("保存の形");
    let entities: Vec<String> = empty.as_object().expect("保存の形").keys().cloned().collect();
    let db = db::Db::open(&db_path, entities).expect("DB を開けません");
    let seed = || match seed_path.as_str() {
        "" => Ok(include_str!("seed.json").to_string()),
        path => std::fs::read_to_string(path).map_err(|x| format!("最初のデータ {} を読めません: {}", path, x)),
    };
    app::seed_if_empty(&db, seed).expect("最初のデータを入れられません");
    let engine = Arc::new(Mutex::new(engine::Engine::new(db).expect("定義が読めません")));

    // 時間の経過（模擬の返事・期限）
    let ticker = engine.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if let Err(e) = engine::lock(&ticker).tick() {
            eprintln!("tick: {}", e);
        }
    });

    let addr = format!("{}:{}", host, port);
    let server = Arc::new(tiny_http::Server::http(&addr).expect("ポートを開けません"));
    println!("{}のサーバー: http://{}/  (DB: {})", geas_app::APP_TITLE, addr, db_path);
    if test_api {
        println!("--test：テスト用の口（/api/test/…）と開発用の模擬パネルを開いています。公開するサーバーでは付けないでください");
    }
    let mut handles = Vec::new();
    for _ in 0..workers {
        let server = server.clone();
        let engine = engine.clone();
        let pwa = pwa.clone();
        let dev = dev.clone();
        let opts = opts.clone();
        handles.push(std::thread::spawn(move || {
            for req in server.incoming_requests() {
                // 1 件の panic でワーカーを失わない（応答は返らず接続が切れるが、次のリクエストは受けられる）
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| serve(req, &engine, &pwa, &dev, &opts)));
            }
        }));
    }
    for w in handles {
        let _ = w.join();
    }
}
