//! Geas が生成したアプリの実行部（サーバーと窓版が共有する）。画面からの呼び出しを処理する経路（http::handle）、
//! 処理を動かす層（engine）、保存（db）、外部作用の模擬（mock）。運び方（HTTP の待ち受け・窓の IPC）は持たない。
pub mod app;
pub mod db;
pub mod engine;
pub mod http;
pub mod mock;
pub mod net;

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
    let mut en = engine::Engine::new(db).map_err(|e| format!("定義が読めません: {}", e))?;
    // 保存の行を、ここで 1 回ぜんぶ読む。.geas の実体の項目を変えたあとに前の版の DB を開くと、行が今の形で読めない。
    // 起動してから全部のリクエストが失敗するより、起動のときに理由を言って止まる
    en.query_version().map_err(|e| format!("DB {} の中身が、このアプリの今の定義と合いません（{}）。.geas の実体の項目を変えたあとは、前の版で作った DB をそのまま使えません。DB を前の版のアプリで開くか、別の DB で起こしてください", db_path, e))?;
    Ok(Arc::new(Mutex::new(en)))
}

/// シークレットのファイルを、持ち主のほかの利用者も読めるなら注意を出す（Unix だけ。起動は止めない）
pub fn warn_secret_perms(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = std::fs::metadata(path) {
            if m.permissions().mode() & 0o077 != 0 {
                eprintln!("注意：シークレットのファイル {} は、ほかの利用者も読める権限です（chmod 600 にしてください）", path.display());
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// 時間の経過（模擬の返事・期限）を進め、宣言した実通信（over http）を送るスレッドを起こす。
/// 通信は 1 件ごとに別のスレッドで送る（遅い相手が、期限の発火や他の処理を止めない）。送るのはコミットのあとで、返事は次の tick が届ける
pub fn spawn_ticker(engine: Arc<Mutex<engine::Engine>>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(300));
        let sends = {
            let mut en = engine::lock(&engine);
            match en.tick() {
                Ok(fired) => {
                    for f in fired {
                        eprintln!("{}", serde_json::json!({ "log": "geas.tick", "fired": f }));  // 届けた返事・発火した期限（入力の値は載せない）
                    }
                }
                Err(e) => eprintln!("tick: {}", e),
            }
            en.take_outbound()
        };
        for (o, req) in sends {
            let engine = engine.clone();
            std::thread::spawn(move || {
                let r = net::send(&req);
                engine::lock(&engine).finish_outbound(&o, r);
            });
        }
    });
}
