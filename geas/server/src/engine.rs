//! 実行の層。Geas が生成した【処理】（host::call）を呼び、結果の行き先（フロー）・待ち（トリガ）・外部作用を動かす。
//! 生成物の定義（DEFS_JSON）から動くので、この層は「お店」を知らない。アプリに固有なのは app.rs（外から埋める値と最初のデータ）だけ。
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
    /// 保存をメモリに持つ（DB と同じ内容）。None なら、次に使うときに DB から全部を読む。
    /// 1 リクエストごとに全件を読んで JSON で渡し直すと、時間とメモリが件数に比例するため
    store: Option<geas_app::Store>,
    /// store を読んだときの DB の data_version。別の接続が書くと変わるので、そのときだけ読み直す
    store_version: i64,
    /// このトランザクションの中で、メモリ上の保存を書き換えたか（巻き戻すときは、メモリ上の保存も捨てて読み直す）
    dirty: bool,
    /// メモリ上の保存の世代。処理が行を変えるか、DB から読み直すたびに 1 増える。問い合わせの答えは保存と入力だけで決まる
    /// （時計は使わない）ので、同じ世代なら同じ答えになる。問い合わせの応答の ETag に使う（`query_version`）
    generation: u64,
    /// このプロセスの起動を見分ける値。再起動したあとや、同じ DB を使う別のサーバーの版と取り違えない
    boot: String,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

impl Engine {
    pub fn new(db: Db) -> R<Engine> {
        let defs: Value = serde_json::from_str(geas_app::host::DEFS_JSON).map_err(|x| x.to_string())?;
        static STARTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let nth = STARTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let boot = format!("{:x}.{:x}.{:x}", nanos, std::process::id(), nth);
        Ok(Engine { db, defs, mock: Mock::new(), clock_offset: 0, store: None, store_version: 0, dirty: false, generation: 0, boot })
    }

    pub fn now(&self) -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) + self.clock_offset
    }

    /// メモリ上の保存を、DB の今の内容に合わせる。別の接続が書いていなければ、何も読まない
    fn ensure_store(&mut self) -> R<()> {
        if self.db.in_transaction() {
            return self.reload_if_stale();
        }
        // トランザクションの外（問い合わせ）：読み直すなら、全部の表を同じ時点の内容で読む。
        // 読み込みの途中で panic しても、読むだけのトランザクションを開いたままにしない（次のリクエストが始められなくなる）
        self.db.begin_read()?;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.reload_if_stale()))
            .unwrap_or_else(|_| Err("保存を読めません（内部の異常）".into()));
        self.db.rollback();
        r
    }

    fn reload_if_stale(&mut self) -> R<()> {
        let version = self.db.data_version()?;
        if self.store.is_some() && version == self.store_version {
            return Ok(());
        }
        self.store = None;
        let mut s = geas_app::Store::default();
        self.db.each_row(|en, k, row| geas_app::host::load_row(&mut s, en, k, row).map_err(|x| format!("保存の行 {}「{}」が読めません: {}", en, k, x)))?;
        self.store = Some(s);
        self.store_version = version;
        self.generation += 1;
        Ok(())
    }

    /// 問い合わせの答えの版。保存を DB の今の内容に合わせてから返すので、同じ問い合わせ（URL）で、この値も同じなら、
    /// 答えも同じ（サーバーは 304 を返し、問い合わせを実行し直さない）
    pub fn query_version(&mut self) -> R<String> {
        self.ensure_store()?;
        Ok(self.version())
    }

    /// いまの保存の版（起動ごとの値と世代）。問い合わせを実行した直後に呼ぶと、その答えの版になる
    pub fn version(&self) -> String {
        format!("{}-{}", self.boot, self.generation)
    }

    /// 生成した host を1回呼ぶ。保存はメモリ上のものを使い、処理が変えた行だけを DB へ書く
    fn call(&mut self, name: &str, ctx: &Value, input: &Value) -> R<Value> {
        self.ensure_store()?;
        let store = self.store.as_mut().ok_or("保存を読めません")?;
        let req = json!({ "name": name, "ctx": ctx, "input": input }).to_string();
        let mut changed = geas_app::Changed::new();
        let out = geas_app::host::call_store(&req, store, &mut changed);
        // 同じ行が何度変わっても、書くのは最後の内容を 1 回
        changed.sort_unstable();
        changed.dedup();
        let mut written = Ok(());
        if out.is_ok() {
            for (en, k) in &changed {
                written = self.db.write_row(en, k, geas_app::host::row_json(store, en, k).as_deref());
                if written.is_err() {
                    break;
                }
            }
        }
        if !changed.is_empty() {
            self.generation += 1;
            // DB と食い違ったメモリ上の保存は使わない（トランザクションの中なら、巻き戻すときに捨てる）
            self.dirty = true;
            if (out.is_err() || written.is_err()) && !self.db.in_transaction() {
                self.store = None;
            }
        }
        let out = out?;
        written?;
        serde_json::from_str(&out).map_err(|x| format!("host の応答が読めません: {}", x))
    }

    fn atomic<T>(&mut self, f: impl FnOnce(&mut Engine) -> R<T>) -> R<T> {
        self.db.begin()?;
        self.dirty = false;
        // 想定外の panic があっても、書きかけを巻き戻して異常として返す（Mutex を汚さず、以後のリクエストも動く）
        let r = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self))) {
            Ok(Ok(x)) => self.db.commit().map(|()| x),
            Ok(Err(x)) => Err(x),
            Err(_) => {
                self.dirty = true;
                Err("内部の異常でこのリクエストを取り消しました".into())
            }
        };
        if r.is_err() {
            self.db.rollback();
            // メモリ上の保存も巻き戻す（書き換えていたら捨てて、次に使うときに DB から読み直す）
            if self.dirty {
                self.store = None;
            }
        }
        self.dirty = false;
        r
    }

    // ---- 問い合わせ ----
    pub fn query(&mut self, name: &str, input: &Value) -> R<Value> {
        let def = self.defs["queries"].get(name).ok_or(format!("問い合わせ「{}」はありません", name))?.clone();
        let refs: Vec<String> = def["refs"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        if !refs.is_empty() {
            return Err(format!("問い合わせ {} は参照 {:?} を要しますが、このサーバーは埋められません", name, refs));
        }
        let mut resp = self.call(name, &json!({}), input)?;
        // 行は複製せずに取り出す（行の多い一覧で、全部を写し直さない）
        if let Some(rows) = resp.get_mut("body").and_then(|b| b.get_mut("rows")).map(Value::take) {
            Ok(json!({ "rows": rows }))
        } else {
            Ok(json!({ "ids": resp["args"].take() }))
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
        let ctx = app::fill_ctx(&self.db, &self.defs, &names("refs"), &names("new_ids"), input, requested)?;
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

    /// 時間の経過：模擬の返事を返し、期限が来た待ちを動かす。
    /// 1 件の失敗で、ほかの返事や期限を止めない（失敗したものは巻き戻してあり、記録して次へ進む）。
    /// 失敗で打ち切ると、同じ回に届いたほかの返事が失われ、先頭で失敗し続ける待ちのうしろの期限が二度と発火しない
    pub fn tick(&mut self) -> R<Vec<Value>> {
        let now = self.now();
        let mut out = Vec::new();
        for j in self.mock.take_due(now) {
            let head = json!({ "reply": j.reply, "trigger": j.trigger, "args": j.trigger_args });
            out.push(match self.deliver(&j.trigger, &j.trigger_args, &j.reply, &j.content) {
                Ok(r) => merge(&head, &json!({ "outcome": r })),
                Err(e) => tick_failed(&head, e),
            });
        }
        for w in self.db.due_waits(now)? {
            let r = self.atomic(|en| {
                // 期限の要因（deadline）の行き先へ
                let def = en.trigger_def(&w.trigger)?;
                let dest = def["causes"].as_array().into_iter().flatten().find(|c| c["kind"] == "deadline").map(|c| s(c, "dest")).ok_or("期限の要因がありません")?;
                // 期限の一覧は直列化の外で読んでいるので、同じ DB を共有する別のサーバーが先に発火したかもしれない。
                // 待ちを消せた 1 つだけが先へ進む（消せなければ何もしない）
                if !en.db.remove_wait(w.id)? {
                    return Ok(json!({ "skipped": "他のサーバーがすでに発火した" }));
                }
                let mut trace = Vec::new();
                en.step(&dest, &w.args, &json!({}), &mut trace, 0)?;
                Ok(json!({ "fired": dest, "trace": trace }))
            });
            let head = json!({ "deadline": w.trigger, "args": w.args });
            out.push(match r {
                Ok(r) => merge(&head, &json!({ "outcome": r })),
                Err(e) => tick_failed(&head, e),
            });
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

/// 返事・期限の 1 件の失敗を、標準エラーに 1 行の JSON で残し、結果の一覧にも載せる（値は載せず、トリガの名前と失敗の理由だけ）
fn tick_failed(head: &Value, e: String) -> Value {
    let what = head.get("deadline").or_else(|| head.get("trigger")).cloned().unwrap_or(Value::Null);
    eprintln!("{}", json!({ "log": "geas.tick", "failed": what, "kind": if head.get("deadline").is_some() { "deadline" } else { "reply" }, "error": e }));
    merge(head, &json!({ "error": e }))
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

/// Mutex を取る。別のリクエストが panic して汚れていても、続けて使う（書きかけはトランザクションの巻き戻しで消えている）
pub fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::{lock, Engine, R};
    use crate::db::Db;
    use std::sync::{Arc, Mutex};

    fn engine() -> Engine {
        let empty: serde_json::Value = serde_json::from_str(&geas_app::host::write_store(&geas_app::Store::default())).unwrap();
        let entities = empty.as_object().unwrap().keys().cloned().collect();
        Engine::new(Db::open(":memory:", entities).unwrap()).unwrap()
    }

    #[test]
    // 巻き戻したリクエストがメモリ上の保存を書き換えていたら、その保存は捨てて DB から読み直す。書き換えていなければ持ち続ける
    fn rollback_forgets_the_in_memory_store_only_when_it_was_changed() {
        let mut en = engine();
        en.ensure_store().unwrap();
        let r: R<()> = en.atomic(|_| Err("入力が不正".into()));
        assert!(r.is_err() && en.store.is_some());
        let r: R<()> = en.atomic(|en| {
            en.dirty = true; // 前の段が書いたあとで、次の段が失敗した
            Err("次の段で失敗".into())
        });
        assert!(r.is_err() && en.store.is_none());
        en.ensure_store().unwrap();
        let r: R<()> = en.atomic(|_| panic!("想定外の異常"));
        assert!(r.is_err() && en.store.is_none());
        en.ensure_store().unwrap();
        let r: R<()> = en.atomic(|en| {
            en.dirty = true;
            Ok(())
        });
        assert!(r.is_ok() && en.store.is_some() && !en.dirty);
    }

    #[test]
    // 保存の版（問い合わせの ETag のもと）は、保存を読み直したときだけ変わる。同じ版なら、問い合わせの答えも同じ
    fn version_changes_only_when_the_store_is_reloaded_or_written() {
        let mut en = engine();
        let v1 = en.query_version().unwrap();
        assert_eq!(en.query_version().unwrap(), v1); // 何も変わっていなければ、版も同じ（DB を読み直さない）
        en.store = None; // 巻き戻しで捨てた保存は、次に読み直し、版も変わる
        let v2 = en.query_version().unwrap();
        assert_ne!(v2, v1);
        assert_eq!(en.version(), v2);
        assert_ne!(engine().query_version().unwrap(), v1); // 別の起動（別のサーバー）の版とは取り違えない
    }

    #[test]
    // 汚れた Mutex でも続けて取れる
    fn poisoned_mutex_is_still_usable() {
        let m = Arc::new(Mutex::new(1));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("別のリクエストの異常");
        })
        .join();
        assert!(m.lock().is_err());
        *lock(&m) += 1;
        assert_eq!(*lock(&m), 2);
    }
}
