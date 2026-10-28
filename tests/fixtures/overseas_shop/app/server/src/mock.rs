//! 外の世界（決済・倉庫・配送）の代わりをする模擬。本物のサービスに替えるときは、ここだけを替える。
//! 外部作用を受けたら、少し待ってから「返事」（reply）をトリガへ返す。冪等キーが同じ請求には、同じ答えを返す。
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct Settings {
    /// 決済の返事を落とす（返事が届かないまま、つながりが戻るのを待つ状況を作る）
    pub drop_payment_reply: bool,
    /// 倉庫が在庫不足を返す
    pub warehouse_short: bool,
    /// 返事を返すまでの秒数
    pub delay_secs: i64,
    /// この金額（最小単位）以上の請求は拒否する
    pub decline_from: i64,
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
    /// 冪等キー → (承認したか, 与信ID)
    charges: HashMap<String, (bool, String)>,
    pub dropped: u32,
}

impl Mock {
    pub fn new() -> Mock {
        Mock {
            settings: Settings { drop_payment_reply: false, warehouse_short: false, delay_secs: 1, decline_from: 500_000 },
            jobs: Vec::new(),
            charges: HashMap::new(),
            dropped: 0,
        }
    }

    /// 外部作用を1件受ける。trigger は、返事が合流するトリガ（無ければ返事はない）
    pub fn handle(&mut self, now: i64, effect: &str, args: &Value, idem_key: &str, trigger: Option<(String, Value)>) {
        let (reply, content) = match effect {
            "Payment.charge" => {
                let amount = args.get("amount").and_then(|x| x.as_i64()).unwrap_or(0);
                let decline_from = self.settings.decline_from;
                let (ok, auth) = self
                    .charges
                    .entry(idem_key.to_string())
                    .or_insert_with(|| (amount < decline_from, format!("auth-{}", idem_key)))
                    .clone();
                if self.settings.drop_payment_reply {
                    self.dropped += 1;
                    return;
                }
                // 拒否は与信ID が無い（空の中身）
                ("PaymentSettled", if ok { json!({ "auth_id": auth }) } else { json!({}) })
            }
            "Warehouse.hold" => ("StockSettled", if self.settings.warehouse_short { json!({ "short": ["out-of-stock"] }) } else { json!({}) }),
            _ => return, // 返金・出荷は返事を待たない
        };
        if let Some((t, targs)) = trigger {
            self.jobs.push(Job { due: now + self.settings.delay_secs, effect: effect.to_string(), trigger: t, trigger_args: targs, reply: reply.to_string(), content });
        }
    }

    pub fn take_due(&mut self, now: i64) -> Vec<Job> {
        let (due, rest): (Vec<Job>, Vec<Job>) = std::mem::take(&mut self.jobs).into_iter().partition(|j| j.due <= now);
        self.jobs = rest;
        due
    }
}
