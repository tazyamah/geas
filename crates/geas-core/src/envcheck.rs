// SPDX-License-Identifier: GPL-3.0-or-later
//! 動かす前提の照合：検査項目の一覧（envreq）と、環境の事実を突き合わせる（Python 版 envcheck.py の移し）。
use crate::envreq::EnvItem;
use serde_json::{json, Value};

pub const PASS: &str = "合格";
pub const FAIL: &str = "不合格";
pub const UNKNOWN: &str = "確かめられなかった";
pub const TRUST: &str = "人の保証";

const WEB_PORTS: [&str; 2] = ["80", "443"];

pub struct Res {
    pub id: String,
    pub aspect: String,
    pub text: String,
    pub status: &'static str,
    pub detail: String,
}

fn result(item: &EnvItem, status: &'static str, detail: impl Into<String>) -> Res {
    Res { id: item.id.clone(), aspect: item.aspect.clone(), text: item.text.clone(), status, detail: detail.into() }
}

fn int(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::Number(n)) => n.as_i64(),
        _ => None,
    }
}

fn is_null(v: Option<&Value>) -> bool {
    matches!(v, None | Some(Value::Null))
}

fn dedup(v: Vec<&str>) -> Vec<&str> {
    let mut out: Vec<&str> = vec![];
    for x in v {
        if !out.contains(&x) {
            out.push(x);
        }
    }
    out
}

fn check_instances(item: &EnvItem, f: &Value) -> Res {
    let c = &item.cond;
    let (imax, roll) = (int(f.get("instances_max")), int(f.get("rollout_max")));
    let (mut bad, mut unknown): (Vec<String>, Vec<&str>) = (vec![], vec![]);
    if let Some(n) = c.get("at_most").and_then(|v| v.as_i64()) {
        for (label, v) in [("台数の上限", imax), ("入れ替えのときの同時最大数", roll)] {
            match v {
                None => unknown.push(label),
                Some(v) if v > n => bad.push(format!("{}が {}（{} 以下のはず）", label, v, n)),
                _ => {}
            }
        }
    }
    if c.get("same_host").map_or(false, |v| v.as_bool() == Some(true)) {
        let shared = f.get("shared_host");
        if imax.map_or(false, |m| m <= 1) {
        } else if shared == Some(&Value::Bool(true)) {
        } else if shared == Some(&Value::Bool(false)) && imax.is_some() {
            bad.push(format!("台数の上限が {} で、同じ機械の上で動くことが保証されていない", imax.unwrap()));
        } else {
            unknown.push(if imax.is_none() { "台数の上限" } else { "同じ機械の上で動くか" });
        }
    }
    if !bad.is_empty() {
        return result(item, FAIL, bad.join("。"));
    }
    if !unknown.is_empty() {
        return result(item, UNKNOWN, format!("読めなかった事実：{}", dedup(unknown).join("・")));
    }
    let show = |v: Option<i64>| v.map_or("None".to_string(), |x| x.to_string());
    result(item, PASS, format!("台数の上限 {}、入れ替えのときの同時最大数 {}", show(imax), show(roll)))
}

fn check_durable(item: &EnvItem, f: &Value) -> Res {
    match f.get("storage_persistent") {
        None | Some(Value::Null) => result(item, UNKNOWN, "読めなかった事実：データの領域の種類"),
        Some(v) if truthy(v) => result(item, PASS, "消えない種類の領域が付いている（保存先のパスが、その領域の上にあるかまでは見ていない）"),
        _ => result(item, FAIL, "データの領域が、再起動や入れ替えで消える種類"),
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map_or(true, |f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn check_exposure(item: &EnvItem, f: &Value) -> Res {
    let Some(Value::Array(ports)) = f.get("internet_ingress_ports") else {
        return result(item, UNKNOWN, "読めなかった事実：インターネットから届く口");
    };
    let ports: Vec<String> = ports.iter().map(|x| match x {
        Value::String(s) => s.clone(),
        o => o.to_string(),
    }).collect();
    if item.cond.get("internet").and_then(|v| v.as_str()) == Some("internal") {
        return if ports.is_empty() { result(item, PASS, "インターネットから届く口は無い") } else { result(item, FAIL, format!("インターネットから届く口がある：{}", ports.join("・"))) };
    }
    let extra: Vec<&String> = ports.iter().filter(|x| !WEB_PORTS.contains(&x.as_str())).collect();
    if !extra.is_empty() {
        return result(item, FAIL, format!("公開してよい口（{}）以外が開いている：{}", WEB_PORTS.join("・"), extra.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("・")));
    }
    result(item, PASS, format!("インターネットから届く口：{}", if ports.is_empty() { "なし".to_string() } else { ports.join("・") }))
}

fn check_reach(item: &EnvItem, f: &Value) -> Res {
    match f.get("egress_https") {
        Some(Value::Bool(false)) => result(item, FAIL, "外向きの 443 が許されていない"),
        v if is_null(v) => result(item, UNKNOWN, "読めなかった事実：外向きの通信の許可"),
        _ => result(item, UNKNOWN, "外向きの 443 は許されている。宛先ごとに届くかは、plan からは言えない（実環境で確かめる）"),
    }
}

pub fn verify(items: &[EnvItem], facts: &Value) -> Vec<Res> {
    items
        .iter()
        .map(|it| {
            let key = it.id.split('.').next().unwrap_or("");
            if it.trust {
                return result(it, TRUST, if it.why.is_empty() { "外から確かめられない".to_string() } else { it.why.clone() });
            }
            match key {
                "instances" => check_instances(it, facts),
                "durable" => check_durable(it, facts),
                "exposure" => check_exposure(it, facts),
                "reach" => check_reach(it, facts),
                "secret" => result(it, UNKNOWN, "シークレットの名前を環境の変数名に結ぶ取り決めがまだ無く、plan からは確かめられない"),
                _ => result(it, UNKNOWN, "この項目の照合は、まだ実装されていない"),
            }
        })
        .collect()
}

pub fn counts(results: &[Res]) -> [(&'static str, usize); 4] {
    let n = |s: &str| results.iter().filter(|r| r.status == s).count();
    [(PASS, n(PASS)), (FAIL, n(FAIL)), (UNKNOWN, n(UNKNOWN)), (TRUST, n(TRUST))]
}

pub fn to_text(results: &[Res]) -> String {
    let mut lines: Vec<String> = vec![];
    for r in results {
        lines.push(format!("[{}] {}（{}）{}", r.status, r.id, r.aspect, r.text));
        lines.push(format!("    {}", r.detail));
    }
    lines.push(format!("結果：{}", counts(results).iter().map(|(k, v)| format!("{} {}", k, v)).collect::<Vec<_>>().join("、")));
    lines.join("\n") + "\n"
}

pub fn results_json(results: &[Res]) -> Value {
    Value::Array(results.iter().map(|r| json!({"id": r.id, "観点": r.aspect, "要求": r.text, "判定": r.status, "詳細": r.detail})).collect())
}

pub fn counts_json(results: &[Res]) -> Value {
    let mut m = serde_json::Map::new();
    for (k, v) in counts(results) {
        m.insert(k.to_string(), json!(v));
    }
    Value::Object(m)
}
