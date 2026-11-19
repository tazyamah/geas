//! 外部作用の実通信（宣言した `over http`）。送る先は生成物の定義に書いてあるものだけで、リダイレクトにも従わない
//! （宣言していない先へ連れて行かれない）。通信の部品（ureq）は feature `net` を付けたときだけ入る。geas が、宣言に
//! `over http` を持つアプリを生成するときだけ付ける（宣言がなければ、通信のコードは実行ファイルに入らない）。
use serde_json::{Map, Value};

pub struct Request {
    pub method: String,
    pub url: String,
    /// Authorization: Bearer の値（宣言があるとき）。取れなかったら理由（送らない）
    pub bearer: Option<Result<String, String>>,
    pub body: Option<Value>,
}

/// 1 件送って、HTTP の状態と本文（上限 1 MiB）を返す。接続できない・時間切れ・シークレットが無い、は Err
pub fn send(req: &Request) -> Result<(u16, String), String> {
    let token = match &req.bearer {
        Some(Ok(t)) => Some(t.as_str()),
        Some(Err(e)) => return Err(e.clone()),
        None => None,
    };
    transport(req, token)
}

#[cfg(feature = "net")]
fn transport(req: &Request, token: Option<&str>) -> Result<(u16, String), String> {
    use std::io::Read;
    // リダイレクトには従わない（宣言した先だけに出る）。時間切れは 15 秒
    let agent = ureq::AgentBuilder::new().redirects(0).timeout(std::time::Duration::from_secs(15)).build();
    let mut r = agent.request(&req.method, &req.url);
    if let Some(t) = token {
        r = r.set("Authorization", &format!("Bearer {}", t));
    }
    let res = match &req.body {
        Some(b) => r.set("Content-Type", "application/json").send_string(&b.to_string()),
        None => r.call(),
    };
    let res = match res {
        Ok(x) => x,
        Err(ureq::Error::Status(_, x)) => x,  // 4xx・5xx も返事（状態を結果の種類に結ぶ）
        Err(e) => return Err(e.to_string().replace(&req.url, "(通信先)")),
    };
    let status = res.status();
    let mut body = String::new();
    res.into_reader().take(1 << 20).read_to_string(&mut body).map_err(|e| e.to_string())?;
    Ok((status, body))
}

#[cfg(not(feature = "net"))]
fn transport(_req: &Request, _token: Option<&str>) -> Result<(u16, String), String> {
    Err("この実行ファイルには通信の部品が入っていません（`over http` を宣言したアプリを geas で生成し直してください）".into())
}

/// 返事の JSON から、道すじ（data.id）の値を取る
fn pick<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |cur, k| cur.get(k))
}

/// HTTP の状態を結果の種類に結び、返事の本文から応答の中身を取り出す。(結果の種類, 中身)。中身が取れなければ Err
pub fn map_response(http: &Value, fields: &Value, status: u16, body: &str) -> Result<(String, Value), String> {
    let hit = http["statuses"].as_array().into_iter().flatten().find(|r| {
        let (lo, hi) = (r["lo"].as_u64().unwrap_or(1), r["hi"].as_u64().unwrap_or(0));
        (lo..=hi).contains(&(status as u64))
    });
    let row = hit.unwrap_or(&http["other"]);
    let outcome = row["outcome"].as_str().ok_or("結果の種類が決まりません")?.to_string();
    let mut content = Map::new();
    let take = row["take"].as_object();
    if take.map_or(false, |t| !t.is_empty()) {
        let json: Value = serde_json::from_str(body).map_err(|_| format!("返事が JSON ではありません（HTTP {}）", status))?;
        for (name, path) in take.into_iter().flatten() {
            let v = pick(&json, path.as_str().unwrap_or("")).ok_or(format!("返事に {} がありません（HTTP {}）", path, status))?;
            let kind = fields.as_array().into_iter().flatten().find(|f| f["name"] == name.as_str()).and_then(|f| f["kind"].as_str()).unwrap_or("text");
            content.insert(
                name.clone(),
                match (kind, v) {
                    ("int", Value::Number(n)) if n.is_i64() => v.clone(),
                    ("int", _) => return Err(format!("{} は整数のはずです", path)),
                    (_, Value::String(x)) => Value::String(x.clone()),
                    (_, Value::Number(n)) => Value::String(n.to_string()),
                    _ => return Err(format!("{} は文字列のはずです", path)),
                },
            );
        }
    }
    Ok((outcome, Value::Object(content)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn http() -> Value {
        json!({ "statuses": [{ "lo": 201, "hi": 201, "outcome": "Posted", "take": { "post_id": "data.id" } }, { "lo": 400, "hi": 499, "outcome": "Rejected", "take": {} }],
                "other": { "outcome": "Rejected", "take": {} } })
    }

    #[test]
    fn status_maps_to_outcome_and_takes_content() {
        let fields = json!([{ "name": "post_id", "kind": "text" }]);
        let (o, c) = map_response(&http(), &fields, 201, r#"{"data":{"id":"123"}}"#).unwrap();
        assert_eq!((o.as_str(), c["post_id"].as_str()), ("Posted", Some("123")));
        assert_eq!(map_response(&http(), &fields, 403, "x").unwrap().0, "Rejected");
        assert_eq!(map_response(&http(), &fields, 502, "").unwrap().0, "Rejected");
        assert!(map_response(&http(), &fields, 201, r#"{"data":{}}"#).is_err());
        assert!(map_response(&http(), &fields, 201, "not json").is_err());
    }
}
