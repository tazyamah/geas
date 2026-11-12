//! 外から埋める値（slot）と新しい ID を、仕様の定義（defs）から埋める。アプリを知らない汎用のもの。
//! 通貨のように利用者が選ぶ slot は呼び出し元（requested）から、税率・送料のようにサーバーが決める slot は、仕様の ruleから。
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

/// rule を上の行から見て、最初に当てはまった行の値
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
/// requested は呼び出し元（画面）が渡してよい値で、rule の無い slot（通貨など）だけがここから来る
pub fn fill_ctx(db: &Db, defs: &Value, refs: &[String], new_ids: &[String], input: &Value, requested: &Value) -> R<Value> {
    let mut ctx = serde_json::Map::new();
    for r in refs.iter().filter(|r| defs["rules"].get(r.as_str()).is_none()) {
        let v = requested.get(r).cloned().ok_or(format!("{}（{}）を指定してください", r, r))?;
        ctx.insert(r.clone(), v);
    }
    for r in refs.iter().filter(|r| defs["rules"].get(r.as_str()).is_some()) {
        let rows = defs["rules"][r.as_str()].as_array().cloned().unwrap_or_default();
        let v = decide(&rows, input, &ctx).ok_or(format!("{}を決められません", r))?;
        ctx.insert(r.clone(), v);
    }
    for en in new_ids {
        let n = db.next_counter(&format!("id:{}", en))?;
        ctx.insert(format!("new_{}_id", en), json!(format!("{}-{:04}", en.to_lowercase(), n)));
    }
    Ok(Value::Object(ctx))
}

/// 保存の形の版を合わせる（`.geas` の migration）。DB は自分の版を覚えている（覚えていなければ 1）。
/// 行のある DB が古い版なら、足りない版の手順を順に当て、書き換えた行を今の型の範囲で確かめてから、版を進める。
/// 全部を 1 つのトランザクションで行い、途中で失敗したら何も変えずに止まる。行の無い DB は、今の版として始める
pub fn migrate(db: &Db, defs: &Value) -> R<()> {
    const KEY: &str = "schema_version";
    let current = defs["schema_version"].as_i64().unwrap_or(1);
    let empty = db.is_empty()?;
    let have = match db.kv(KEY)? {
        Some(v) => v,
        None if empty => current,
        None => 1,
    };
    if have > current {
        return Err(format!("DB の保存の形は版 {} で、このアプリ（版 {}）より新しいです。新しい版のアプリで開いてください", have, current));
    }
    if have == current && (db.kv(KEY)?.is_some() || current == 1) {
        return Ok(());
    }
    db.begin()?;
    let r = (|| {
        let mut rows: Vec<(String, String, Value)> = Vec::new();
        db.each_row(|en, k, row| {
            rows.push((en.to_string(), k.to_string(), serde_json::from_str(row).map_err(|x| format!("保存の行 {}「{}」が読めません: {}", en, k, x))?));
            Ok(())
        })?;
        let empty_steps = Vec::new();
        for m in defs["migrations"].as_array().unwrap_or(&empty_steps) {
            if m["version"].as_i64().unwrap_or(0) <= have {
                continue;
            }
            for st in m["steps"].as_array().unwrap_or(&empty_steps) {
                let (en, field) = (st["entity"].as_str().unwrap_or(""), st["field"].as_str().unwrap_or(""));
                for (_, _, row) in rows.iter_mut().filter(|(e, _, _)| e == en) {
                    // 足す：項目の無い行にだけ入れる（もうある行はそのまま）
                    if let Some(obj) = row.as_object_mut() {
                        obj.entry(field.to_string()).or_insert_with(|| st["value"].clone());
                    }
                }
            }
        }
        let mut scratch = geas_app::Store::default();
        for (en, k, row) in &rows {
            let text = row.to_string();
            geas_app::host::load_row(&mut scratch, en, k, &text).map_err(|x| format!("版 {} の形にした {}「{}」が、今の定義に合いません: {}", current, en, k, x))?;
            db.write_row(en, k, Some(&text))?;
        }
        db.set_kv(KEY, current)
    })();
    match r {
        Ok(()) => db.commit(),
        Err(x) => {
            db.rollback();
            Err(x)
        }
    }
}

/// DB が空のときだけ、最初のデータ（seed.json。実体名→キー→行）を入れる。
/// 空かどうかは各表に 1 行でもあるかで見る（保存の全件は読まない）。最初のデータは、空のときだけ読む
/// （大きいと、起動のたびに読んで解析する時間とメモリが、件数に比例してかかるため）
pub fn seed_if_empty(db: &Db, seed: impl FnOnce() -> R<String>) -> R<()> {
    // 何かの実体にもう行があれば、入れ済み
    if !db.is_empty()? {
        return Ok(());
    }
    let seed: Value = serde_json::from_str(&seed()?).map_err(|x| format!("seed.json: {}", x))?;
    let rows = seed.as_object().ok_or("seed.json はオブジェクトです")?;
    if rows.is_empty() {
        return Ok(());
    }
    db.begin()?;
    let mut scratch = geas_app::Store::default();
    let r = rows.iter().try_for_each(|(en, rows)| {
        if !db.has_entity(en) {
            return Err(format!("実体「{}」がありません", en));
        }
        let rows = rows.as_object().ok_or("seed.json の実体は、キー→行のオブジェクトです")?;
        // 入れる前に、行を型の範囲で確かめる（範囲の外の行を DB に入れてしまうと、次の起動からも読めなくなる）
        rows.iter().try_for_each(|(k, v)| {
            let row = v.to_string();
            geas_app::host::load_row(&mut scratch, en, k, &row).map_err(|x| format!("{}「{}」: {}", en, k, x))?;
            db.write_row(en, k, Some(&row))
        })
    });
    match r {
        Ok(()) => db.commit(),
        Err(x) => {
            db.rollback();
            Err(x)
        }
    }
}
