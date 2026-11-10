//! Geas が生成したアプリの実行部（サーバーと窓版が共有する）。画面からの呼び出しを処理する経路（http::handle）、
//! 処理を動かす層（engine）、保存（db）、外部作用の模擬（mock）。運び方（HTTP の待ち受け・窓の IPC）は持たない。
pub mod app;
pub mod db;
pub mod engine;
pub mod http;
pub mod mock;

use std::sync::{Arc, Mutex};

/// 生成物に埋め込んだ最初のデータ（geas rust --seed）。DB が空のときだけ読んで入れる
pub fn embedded_seed() -> Result<String, String> {
    Ok(include_str!("seed.json").to_string())
}

/// DB を開き、空なら最初のデータを入れ、処理を動かす層を作る。最初のデータは、DB が空のときだけ読む（seed は読む関数）
pub fn open_engine(db_path: &str, seed: impl FnOnce() -> Result<String, String>) -> Result<Arc<Mutex<engine::Engine>>, String> {
    // 保存にある実体の名前は、生成物から取る（空の保存を書き出すと、全部の実体が並ぶ）
    let empty: serde_json::Value = serde_json::from_str(&geas_app::host::write_store(&geas_app::Store::default())).map_err(|_| "保存の形が読めません")?;
    let entities: Vec<String> = empty.as_object().ok_or("保存の形が読めません")?.keys().cloned().collect();
    let db = db::Db::open(db_path, entities).map_err(|e| format!("DB を開けません: {}", e))?;
    app::seed_if_empty(&db, seed).map_err(|e| format!("最初のデータを入れられません: {}", e))?;
    Ok(Arc::new(Mutex::new(engine::Engine::new(db).map_err(|e| format!("定義が読めません: {}", e))?)))
}

/// 時間の経過（模擬の返事・期限）を進めるスレッドを起こす
pub fn spawn_ticker(engine: Arc<Mutex<engine::Engine>>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if let Err(e) = engine::lock(&engine).tick() {
            eprintln!("tick: {}", e);
        }
    });
}
