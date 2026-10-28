//! Geas が生成したアプリのサーバー。localhost で動き、DB は SQLite。geas rust --host --runtime が、生成物と一緒に配る。
//! 使い方: geas_server [--port 8787] [--db app.sqlite] [--pwa ../ui] [--dev ../dev] [--seed 別の最初のデータ.json]
mod app;
mod db;
mod engine;
mod http;
mod mock;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn arg(name: &str, default: &str) -> String {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == name).and_then(|i| a.get(i + 1).cloned()).unwrap_or_else(|| default.to_string())
}

fn main() {
    let port = arg("--port", "8787");
    let db_path = arg("--db", "app.sqlite");
    let pwa = PathBuf::from(arg("--pwa", "../ui"));
    let dev = PathBuf::from(arg("--dev", "../dev"));
    // 最初のデータは生成物に埋め込んである（geas rust --seed）。別のものを使うときだけ --seed
    let seed = match arg("--seed", "").as_str() {
        "" => include_str!("seed.json").to_string(),
        path => std::fs::read_to_string(path).expect("最初のデータを読めません"),
    };

    // 保存にある実体の名前は、生成物から取る（空の保存を書き出すと、全部の実体が並ぶ）
    let empty: serde_json::Value = serde_json::from_str(&geas_app::host::write_store(&geas_app::Store::default())).expect("保存の形");
    let entities: Vec<String> = empty.as_object().expect("保存の形").keys().cloned().collect();
    let db = db::Db::open(&db_path, entities).expect("DB を開けません");
    let store = db.load_store().expect("保存を読めません");
    app::seed_if_empty(&db, &seed, &store).expect("最初のデータを入れられません");
    let engine = Arc::new(Mutex::new(engine::Engine::new(db).expect("定義が読めません")));

    // 時間の経過（模擬の返事・期限）
    let ticker = engine.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if let Err(e) = ticker.lock().unwrap().tick() {
            eprintln!("tick: {}", e);
        }
    });

    let addr = format!("127.0.0.1:{}", port);
    let server = Arc::new(tiny_http::Server::http(&addr).expect("ポートを開けません"));
    println!("{}のサーバー: http://{}/  (DB: {})", geas_app::APP_TITLE, addr, db_path);
    let mut workers = Vec::new();
    for _ in 0..4 {
        let server = server.clone();
        let engine = engine.clone();
        let pwa = pwa.clone();
        let dev = dev.clone();
        workers.push(std::thread::spawn(move || {
            for req in server.incoming_requests() {
                http::handle(req, &engine, &pwa, &dev);
            }
        }));
    }
    for w in workers {
        let _ = w.join();
    }
}
