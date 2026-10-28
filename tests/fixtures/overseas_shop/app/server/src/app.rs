//! このお店の設定。Geas の約束の外にある、サーバーの運用の取り決め：
//! 外から埋める枠（slot）の値、新しい ID の付け方、最初に入れておく商品と顧客。
use crate::db::Db;
use serde_json::{json, Value};

type R<T> = Result<T, String>;

/// 税率（最小単位でなく割合）。国内向け（円）は10%、外貨は輸出免税として0%
fn tax_rate(currency: &str) -> &'static str {
    if currency == "JPY" { "10/100" } else { "0/1" }
}

/// 送料（その通貨の最小単位）。円は国内500円・海外2500円、ドルとユーロは1500・3000（セント）
fn shipping(currency: &str, country: &str) -> i64 {
    let domestic = country == "JP";
    match (currency, domestic) {
        ("JPY", true) => 500,
        ("JPY", false) => 2500,
        (_, true) => 1500,
        (_, false) => 3000,
    }
}

/// 処理が外から受ける参照（slot と新しい ID）を、名前ごとに埋める。
/// requested は呼び出し元（画面）が渡してよい値で、通貨だけ。税率と送料は、サーバーが決める
pub fn fill_ctx(db: &Db, refs: &[String], new_ids: &[String], input: &Value, requested: &Value) -> R<Value> {
    let mut ctx = serde_json::Map::new();
    for r in refs {
        let v = match r.as_str() {
            "currency" => requested.get("currency").cloned().ok_or("通貨（currency）を指定してください")?,
            "tax_rate" => json!(tax_rate(requested.get("currency").and_then(|x| x.as_str()).ok_or("通貨（currency）を指定してください")?)),
            "shipping" => {
                let cur = requested.get("currency").and_then(|x| x.as_str()).ok_or("通貨（currency）を指定してください")?;
                let country = input.get("ship_to").and_then(|x| x.get("country")).and_then(|x| x.as_str()).unwrap_or("");
                json!(shipping(cur, country))
            }
            other => return Err(format!("参照「{}」を埋める取り決めがありません", other)),
        };
        ctx.insert(r.clone(), v);
    }
    for en in new_ids {
        let n = db.next_counter(&format!("id:{}", en))?;
        ctx.insert(format!("new_{}_id", en), json!(format!("{}-{:04}", en.to_lowercase(), n)));
    }
    Ok(Value::Object(ctx))
}

/// DB が空のときだけ、最初の商品と顧客を入れる（商品と顧客を作る処理は、この題材の Geas には無い）
pub fn seed_if_empty(db: &Db, seed_path: &str, store: &Value) -> R<()> {
    let has = store.get("Product").and_then(|x| x.as_object()).map_or(false, |m| !m.is_empty());
    if has {
        return Ok(());
    }
    let text = std::fs::read_to_string(seed_path).map_err(|x| format!("{}: {}", seed_path, x))?;
    let seed: Value = serde_json::from_str(&text).map_err(|x| format!("{}: {}", seed_path, x))?;
    let mut after = store.clone();
    for (en, rows) in seed.as_object().ok_or("seed.json はオブジェクトです")? {
        let target = after.get_mut(en).and_then(|x| x.as_object_mut()).ok_or(format!("実体「{}」がありません", en))?;
        for (k, v) in rows.as_object().ok_or("seed.json の実体は、キー→行のオブジェクトです")? {
            target.insert(k.clone(), v.clone());
        }
    }
    db.begin()?;
    let r = db.save_diff(store, &after);
    match r {
        Ok(()) => db.commit(),
        Err(x) => {
            db.rollback();
            Err(x)
        }
    }
}
