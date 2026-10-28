//! 外から埋める値（slot）と新しい ID を、仕様の定義（defs）から埋める。アプリを知らない汎用のもの。
//! 通貨のように利用者が選ぶ slot は呼び出し元（requested）から、税率・送料のようにサーバーが決める slot は、仕様の fill の表から。
use crate::db::Db;
use serde_json::{json, Value};

type R<T> = Result<T, String>;

/// 道すじ（ship_to.country など）の値を、入力、決まった slot の順に探して、文字列にする
fn lookup(path: &str, input: &Value, ctx: &serde_json::Map<String, Value>) -> Option<String> {
    let mut parts = path.split('.');
    let head = parts.next()?;
    let mut v = input.get(head).or_else(|| ctx.get(head))?;
    for p in parts {
        v = v.get(p)?;
    }
    Some(match v {
        Value::String(x) => x.clone(),
        other => other.to_string(),
    })
}

/// fill の表を上の行から見て、最初に当てはまった行の値
fn decide(rows: &[Value], input: &Value, ctx: &serde_json::Map<String, Value>) -> Option<Value> {
    rows.iter()
        .find(|row| {
            row["when"].as_array().map_or(true, |cs| {
                cs.iter().all(|c| {
                    let (path, want) = (c[0].as_str().unwrap_or(""), c[1].as_str().unwrap_or(""));
                    lookup(path, input, ctx).as_deref() == Some(want)
                })
            })
        })
        .map(|row| row["value"].clone())
}

/// 処理が外から受ける参照（slot と新しい ID）を、名前ごとに埋める。
/// requested は呼び出し元（画面）が渡してよい値で、fill の無い slot（通貨など）だけがここから来る
pub fn fill_ctx(db: &Db, defs: &Value, refs: &[String], new_ids: &[String], input: &Value, requested: &Value) -> R<Value> {
    let mut ctx = serde_json::Map::new();
    for r in refs.iter().filter(|r| defs["fills"].get(r.as_str()).is_none()) {
        let v = requested.get(r).cloned().ok_or(format!("{}（{}）を指定してください", r, r))?;
        ctx.insert(r.clone(), v);
    }
    for r in refs.iter().filter(|r| defs["fills"].get(r.as_str()).is_some()) {
        let rows = defs["fills"][r.as_str()].as_array().cloned().unwrap_or_default();
        let v = decide(&rows, input, &ctx).ok_or(format!("{}を決められません", r))?;
        ctx.insert(r.clone(), v);
    }
    for en in new_ids {
        let n = db.next_counter(&format!("id:{}", en))?;
        ctx.insert(format!("new_{}_id", en), json!(format!("{}-{:04}", en.to_lowercase(), n)));
    }
    Ok(Value::Object(ctx))
}

/// DB が空のときだけ、最初のデータ（seed.json。実体名→キー→行）を入れる
pub fn seed_if_empty(db: &Db, seed: &str, store: &Value) -> R<()> {
    let seed: Value = serde_json::from_str(seed).map_err(|x| format!("seed.json: {}", x))?;
    let rows = seed.as_object().ok_or("seed.json はオブジェクトです")?;
    // 何かの実体にもう行があれば、入れ済み
    if store.as_object().map_or(false, |m| m.values().any(|x| x.as_object().map_or(false, |r| !r.is_empty()))) || rows.is_empty() {
        return Ok(());
    }
    let mut after = store.clone();
    for (en, rows) in rows {
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
