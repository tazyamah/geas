//! 外の世界（決済・倉庫・配送など）の代わりをする模擬。生成物の定義（DEFS_JSON の mock）だけで動き、アプリを知らない。
//! 外部作用を受けたら、宣言された応答（replies）を、結果の種類（outcome）に応じた中身で、少し待ってからトリガへ返す。
//! 冪等キーが同じ要求には、同じ結果の種類を返す。本物のサービスに替えるときは、ここだけを替える。
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct Settings {
    /// 返事を返すまでの秒数
    pub delay_secs: i64,
    /// 応答 → 返す結果の種類（無ければ先頭＝平常）
    pub outcome: HashMap<String, String>,
    /// 応答 → 返事を落とす（届かないまま、つながりが戻るのを待つ状況を作る）
    pub drop: HashMap<String, bool>,
}

pub struct Job {
    pub due: i64,
    pub effect: String,
    pub trigger: String,
    pub trigger_args: Value,
    pub reply: String,
    pub content: Value,
}

pub struct Mock {
    pub settings: Settings,
    pub jobs: Vec<Job>,
    /// 外部作用と冪等キー → 決めた結果の種類
    decided: HashMap<String, String>,
    pub dropped: u32,
}

impl Mock {
    pub fn new() -> Mock {
        Mock { settings: Settings { delay_secs: 1, outcome: HashMap::new(), drop: HashMap::new() }, jobs: Vec::new(), decided: HashMap::new(), dropped: 0 }
    }

    /// 外部作用を1件受ける。trigger は、返事が合流するトリガ（無ければ返事はない）
    pub fn handle(&mut self, defs: &Value, now: i64, effect: &str, args: &Value, idem_key: &str, trigger: Option<(String, Value)>) {
        let _ = args;
        let Some(reply) = defs["mock"]["effects"][effect]["reply"].as_str() else { return };
        let rdef = &defs["mock"]["replies"][reply];
        let outcomes = rdef["outcomes"].as_array().cloned().unwrap_or_default();
        let wanted = self.settings.outcome.get(reply).cloned().or_else(|| outcomes.first().and_then(|o| o["name"].as_str()).map(String::from)).unwrap_or_default();
        let chosen = self.decided.entry(format!("{}|{}", effect, idem_key)).or_insert(wanted).clone();
        if self.settings.drop.get(reply).copied().unwrap_or(false) {
            self.dropped += 1;
            return;
        }
        let gives: Vec<String> = outcomes.iter().find(|o| o["name"] == chosen.as_str()).and_then(|o| o["gives"].as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let mut content = serde_json::Map::new();
        for f in rdef["fields"].as_array().into_iter().flatten() {
            let name = f["name"].as_str().unwrap_or("");
            if !gives.iter().any(|g| g == name) {
                continue; // 入れない中身は「無い」
            }
            content.insert(name.to_string(), match f["kind"].as_str().unwrap_or("text") {
                "int" => json!(1),
                "set" => json!([format!("{}-1", name)]),
                _ => json!(format!("{}-{}", name, idem_key)),
            });
        }
        if let Some((t, targs)) = trigger {
            self.jobs.push(Job { due: now + self.settings.delay_secs, effect: effect.to_string(), trigger: t, trigger_args: targs, reply: reply.to_string(), content: Value::Object(content) });
        }
    }

    pub fn take_due(&mut self, now: i64) -> Vec<Job> {
        let (due, rest): (Vec<Job>, Vec<Job>) = std::mem::take(&mut self.jobs).into_iter().partition(|j| j.due <= now);
        self.jobs = rest;
        due
    }
}
