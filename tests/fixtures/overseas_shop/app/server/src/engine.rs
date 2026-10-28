//! 実行の層。Geas が生成した【処理】（host::call）を呼び、結果の行き先（フロー）・待ち（トリガ）・外部作用を動かす。
//! 生成物の定義（DEFS_JSON）から動くので、この層は「お店」を知らない。お店に固有なのは app.rs と mock.rs だけ。
use crate::app;
use crate::db::Db;
use crate::mock::Mock;
use serde_json::{json, Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

type R<T> = Result<T, String>;

pub struct Engine {
    pub db: Db,
    pub defs: Value,
    pub mock: Mock,
    /// テストで時計を進める秒数
    pub clock_offset: i64,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

impl Engine {
    pub fn new(db: Db) -> R<Engine> {
        let defs: Value = serde_json::from_str(geas_app::host::DEFS_JSON).map_err(|x| x.to_string())?;
        Ok(Engine { db, defs, mock: Mock::new(), clock_offset: 0 })
    }

    pub fn now(&self) -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) + self.clock_offset
    }

    /// 生成した host を1回呼ぶ。保存は DB から読み、書込は差分だけ DB へ戻す
    fn call(&self, name: &str, ctx: &Value, input: &Value) -> R<Value> {
        let before = self.db.load_store()?;
        let req = json!({ "name": name, "ctx": ctx, "input": input, "store": before }).to_string();
        let out = geas_app::host::call(&req)?;
        let resp: Value = serde_json::from_str(&out).map_err(|x| format!("host の応答が読めません: {}", x))?;
        if let Some(after) = resp.get("store") {
            self.db.save_diff(&before, after)?;
        }
        Ok(resp)
    }

    fn atomic<T>(&mut self, f: impl FnOnce(&mut Engine) -> R<T>) -> R<T> {
        self.db.begin()?;
        match f(self) {
            Ok(x) => {
                self.db.commit()?;
                Ok(x)
            }
            Err(x) => {
                self.db.rollback();
                Err(x)
            }
        }
    }

    // ---- 問い合わせ ----
    pub fn query(&mut self, name: &str, input: &Value) -> R<Value> {
        let def = self.defs["queries"].get(name).ok_or(format!("問い合わせ「{}」はありません", name))?.clone();
        let refs: Vec<String> = def["refs"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        if !refs.is_empty() {
            return Err(format!("問い合わせ {} は参照 {:?} を要しますが、このサーバーは埋められません", name, refs));
        }
        let resp = self.call(name, &json!({}), input)?;
        if let Some(rows) = resp.get("body").and_then(|b| b.get("rows")) {
            Ok(json!({ "rows": rows }))
        } else {
            Ok(json!({ "ids": resp["args"] }))
        }
    }

    // ---- 処理とフロー ----
    /// 外から呼ばれた処理を、結果の行き先をたどって最後まで動かす。トレース（通った処理と結果）を返す
    pub fn run(&mut self, deed: &str, input: &Value, requested: &Value) -> R<Value> {
        self.atomic(|en| {
            let mut trace = Vec::new();
            let root = en.step(deed, input, requested, &mut trace, 0)?;
            Ok(json!({ "result": root["result"], "body": root["body"], "trace": trace }))
        })
    }

    fn step(&mut self, deed: &str, input: &Value, requested: &Value, trace: &mut Vec<Value>, depth: usize) -> R<Value> {
        if depth > 16 {
            return Err("フローが深すぎます".into());
        }
        let def = self.defs["deeds"].get(deed).ok_or(format!("処理「{}」はありません", deed))?.clone();
        let names = |k: &str| -> Vec<String> { def[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default() };
        let ctx = app::fill_ctx(&self.db, &names("refs"), &names("new_ids"), input, requested)?;
        // 宣言にない入力は渡さない（生成物も無視するが、記録をきれいにする）
        let mut inp = Map::new();
        for k in names("inputs") {
            if let Some(v) = input.get(&k) {
                inp.insert(k, v.clone());
            }
        }
        let inp = Value::Object(inp);
        let resp = self.call(deed, &ctx, &inp)?;
        let result = s(&resp, "result");
        let body = resp.get("body").cloned().unwrap_or(json!({}));
        let route = self.defs["flows"].get(deed).and_then(|f| f.get(&result)).cloned().unwrap_or(json!("reply"));
        let wait_trigger = route.as_str().and_then(|r| r.strip_prefix("wait ")).map(String::from);
        let effects: Vec<Value> = resp.get("effects").and_then(|x| x.as_array()).cloned().unwrap_or_default();
        trace.push(json!({ "deed": deed, "result": result, "body": body, "effects": effects.iter().map(|x| x["name"].clone()).collect::<Vec<_>>() }));
        // 待ちを先に作ってから外部作用を出す（返事が先に着いても取りこぼさない）
        if let Some(t) = &wait_trigger {
            let key = self.trigger_args(t, &inp, &body)?;
            self.register_wait(t, &key)?;
        }
        for ef in &effects {
            self.emit_effect(ef, wait_trigger.as_deref())?;
        }
        match &route {
            Value::String(r) if r == "reply" || r.starts_with("wait ") => {}
            Value::String(next) => {
                self.step(next, &merge(&inp, &body), requested, trace, depth + 1)?;
            }
            Value::Array(nexts) => {
                // 並列：枝の順は意味を持たない（検査器が、書き込みの重なりを止めている）
                for n in nexts.iter().filter_map(|x| x.as_str()) {
                    self.step(n, &merge(&inp, &body), requested, trace, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(json!({ "result": result, "body": body }))
    }

    // ---- トリガと待ち ----
    fn trigger_def(&self, name: &str) -> R<Value> {
        self.defs["triggers"].as_array().and_then(|a| a.iter().find(|t| t["name"] == name)).cloned().ok_or(format!("トリガ「{}」はありません", name))
    }

    /// トリガの引数（名前→値）を、入力か結果の中身から取る
    fn trigger_args(&self, trigger: &str, input: &Value, body: &Value) -> R<Value> {
        let def = self.trigger_def(trigger)?;
        let mut m = Map::new();
        for a in def["args"].as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
            let v = input.get(a).or_else(|| body.get(a)).cloned().ok_or(format!("トリガ {} の引数 {} が、待つ処理の入力にも結果にもありません", trigger, a))?;
            m.insert(a.to_string(), v);
        }
        Ok(Value::Object(m))
    }

    fn register_wait(&mut self, trigger: &str, args: &Value) -> R<()> {
        let def = self.trigger_def(trigger)?;
        let secs = def["causes"].as_array().into_iter().flatten().filter_map(|c| c["secs"].as_i64()).min();
        let deadline = secs.map(|x| self.now() + x).unwrap_or(i64::MAX);
        self.db.add_wait(trigger, args, deadline)
    }

    fn emit_effect(&mut self, ef: &Value, wait_trigger: Option<&str>) -> R<()> {
        let name = s(ef, "name");
        let idem = s(ef, "idem_key");
        let args = ef.get("args").cloned().unwrap_or(json!({}));
        // 返事の行き先：宣言の joins があればそのトリガ、なければ要求した処理が待つトリガ
        let joins = self.defs["effects"].get(&name).and_then(|e| e["joins"].as_str()).map(String::from);
        let target = joins.or_else(|| wait_trigger.map(String::from));
        let routed = match &target {
            Some(t) => Some((t.clone(), self.trigger_args(t, &args, &json!({}))?)),
            None => None,
        };
        let now = self.now();
        self.db.log_effect(now, &name, &args, &idem, target.as_deref())?;
        self.mock.handle(&self.defs, now, &name, &args, &idem, routed);
        Ok(())
    }

    /// 返事を1件、トリガの待ちへ入れる。条件がそろったら行き先の処理を動かす
    pub fn deliver(&mut self, trigger: &str, args: &Value, reply: &str, content: &Value) -> R<Value> {
        self.atomic(|en| en.deliver_inner(trigger, args, reply, content))
    }

    fn deliver_inner(&mut self, trigger: &str, args: &Value, reply: &str, content: &Value) -> R<Value> {
        let Some(w) = self.db.find_wait(trigger, args)? else {
            return Ok(json!({ "ignored": "待っている実体がありません（期限切れか、すでに進んだ）" }));
        };
        let mut collected = w.collected.clone();
        collected.as_object_mut().ok_or("待ちの記録が壊れています")?.insert(reply.to_string(), content.clone());
        let def = self.trigger_def(trigger)?;
        let fired = def["causes"].as_array().into_iter().flatten().find(|c| match c["kind"].as_str() {
            Some("all") => c["replies"].as_array().into_iter().flatten().filter_map(|x| x.as_str()).all(|r| collected.get(r).is_some()),
            Some("reply") => c["key"] == reply,
            _ => false,
        });
        let Some(c) = fired else {
            self.db.set_collected(w.id, &collected)?;
            return Ok(json!({ "waiting": true }));
        };
        let dest = s(c, "dest");
        self.db.remove_wait(w.id)?;
        // 行き先の入力 = トリガの引数 + 揃った応答の中身（空の中身は「無い」）
        let mut input = args.as_object().cloned().unwrap_or_default();
        for r in c["replies"].as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
            if let Some(m) = collected.get(r).and_then(|x| x.as_object()) {
                for (k, v) in m {
                    input.insert(k.clone(), v.clone());
                }
            }
        }
        let mut trace = Vec::new();
        self.step(&dest, &Value::Object(input), &json!({}), &mut trace, 0)?;
        Ok(json!({ "fired": dest, "trace": trace }))
    }

    /// 時間の経過：模擬の返事を返し、期限が来た待ちを動かす
    pub fn tick(&mut self) -> R<Vec<Value>> {
        let now = self.now();
        let mut out = Vec::new();
        for j in self.mock.take_due(now) {
            let r = self.deliver(&j.trigger, &j.trigger_args, &j.reply, &j.content)?;
            out.push(json!({ "reply": j.reply, "trigger": j.trigger, "args": j.trigger_args, "outcome": r }));
        }
        for w in self.db.due_waits(now)? {
            let r = self.atomic(|en| {
                // 期限の要因（deadline）の行き先へ
                let def = en.trigger_def(&w.trigger)?;
                let dest = def["causes"].as_array().into_iter().flatten().find(|c| c["kind"] == "deadline").map(|c| s(c, "dest")).ok_or("期限の要因がありません")?;
                en.db.remove_wait(w.id)?;
                let mut trace = Vec::new();
                en.step(&dest, &w.args, &json!({}), &mut trace, 0)?;
                Ok(json!({ "fired": dest, "trace": trace }))
            })?;
            out.push(json!({ "deadline": w.trigger, "args": w.args, "outcome": r }));
        }
        Ok(out)
    }

    // ---- 出来事 ----
    /// 外の出来事（つながりが戻った など）が起こしたこと。「各」は問い合わせの結果1件ずつ、「外から」は渡された入力で
    pub fn event(&mut self, name: &str, input: &Value) -> R<Value> {
        let steps = self.defs["events"].get(name).and_then(|x| x.as_array()).cloned().ok_or(format!("出来事「{}」はありません", name))?;
        let mut runs = Vec::new();
        for st in steps {
            let deed = s(&st, "deed");
            if st["kind"] == "各" {
                let q = self.query(&s(&st, "query"), &json!({}))?;
                let key = self.defs["deeds"][&deed]["inputs"][0].as_str().ok_or("処理に入力がありません")?.to_string();
                for id in q["ids"].as_array().cloned().unwrap_or_default() {
                    let mut m = Map::new();
                    m.insert(key.clone(), id);
                    runs.push(self.run(&deed, &Value::Object(m), &json!({}))?);
                }
            } else {
                runs.push(self.run(&deed, input, &json!({}))?);
            }
        }
        Ok(json!({ "event": name, "runs": runs }))
    }
}

fn merge(a: &Value, b: &Value) -> Value {
    let mut m = a.as_object().cloned().unwrap_or_default();
    if let Some(bm) = b.as_object() {
        for (k, v) in bm {
            m.insert(k.clone(), v.clone());
        }
    }
    Value::Object(m)
}
