//! 既定の差し込み（アプリ固有の運用の取り決め）。外から埋める値（slot）の取り決めも、最初のデータも持たない。
//! 取り決めが要るアプリは、geas rust --app で、このファイルの代わりに自分の app.rs を渡す。
use crate::db::Db;
use serde_json::{json, Value};

type R<T> = Result<T, String>;

/// 処理が外から受ける参照（slot と新しい ID）を、名前ごとに埋める。slot の取り決めが無いので、slot があればエラー
pub fn fill_ctx(db: &Db, refs: &[String], new_ids: &[String], _input: &Value, _requested: &Value) -> R<Value> {
    let mut ctx = serde_json::Map::new();
    if let Some(r) = refs.first() {
        return Err(format!("参照「{}」を埋める取り決めがありません", r));
    }
    for en in new_ids {
        let n = db.next_counter(&format!("id:{}", en))?;
        ctx.insert(format!("new_{}_id", en), json!(format!("{}-{:04}", en.to_lowercase(), n)));
    }
    Ok(Value::Object(ctx))
}

/// 最初のデータ。既定は何も入れない
pub fn seed_if_empty(_db: &Db, _seed_path: &str, _store: &Value) -> R<()> {
    Ok(())
}
