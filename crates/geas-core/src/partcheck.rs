// SPDX-License-Identifier: GPL-3.0-or-later
//! 部品の台帳（geas parts --record の記録）を読む部分（Python 版 partcheck.py の移し）。検査を走らせる部分は段階5で移す。
use crate::ast::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

const STD_LEDGER: &str = include_str!("../assets/std/num.parts.json");

/// Python の json.dumps（ensure_ascii=False、区切りは ", " と ": "）で、文字列の入れ子の配列を書く
fn pyjson(v: &Value) -> String {
    match v {
        Value::Array(xs) => format!("[{}]", xs.iter().map(pyjson).collect::<Vec<_>>().join(", ")),
        Value::String(s) => serde_json::to_string(s).unwrap(),
        other => other.to_string(),
    }
}

fn sha16(text: &str) -> String {
    let h = Sha256::digest(text.as_bytes());
    h.iter().map(|b| format!("{:02x}", b)).collect::<String>()[..16].to_string()
}

pub fn fingerprint(pt: &Part) -> String {
    let (c, v, e) = match &pt.lib {
        Some((c, v, e, _, _)) => (c.clone(), v.clone(), e.clone()),
        None => (String::new(), String::new(), String::new()),
    };
    let types: Vec<Value> = pt.params.iter().map(|(_, t)| Value::String(show_type(t).unwrap_or_default())).collect();
    let names: Vec<Value> = pt.params.iter().map(|(n, _)| Value::String(n.clone())).collect();
    let g = pt.guarantee.as_ref().map(show).unwrap_or_default();
    let all = Value::Array(vec![Value::Array(types), Value::Array(names), Value::String(show_type(&pt.ret).unwrap_or_default()), Value::String(c), Value::String(v), Value::String(e), Value::String(g)]);
    sha16(&pyjson(&all))
}

pub fn examples_fingerprint(pt: &Part) -> String {
    let rows: Vec<Value> = pt
        .examples
        .iter()
        .map(|(args, want, _)| Value::Array(vec![Value::Array(args.iter().map(|a| Value::String(show(a))).collect()), Value::String(show(want))]))
        .collect();
    sha16(&pyjson(&Value::Array(rows)))
}

/// 部品名 → 台帳の行。標準部品の台帳に、アプリの台帳（コードの隣の 名前.parts.json）を重ねる
pub fn load_ledger(code: Option<&str>) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    let mut texts: Vec<String> = vec![STD_LEDGER.to_string()];
    if let Some(c) = code {
        if let Ok(t) = std::fs::read_to_string(Path::new(c).with_extension("parts.json")) {
            texts.push(t);
        }
    }
    for t in texts {
        if let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&t) {
            if let Some(Value::Object(parts)) = m.get("parts") {
                for (k, v) in parts {
                    out.insert(k.clone(), v.clone());
                }
            }
        }
    }
    out
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map_or(false, |x| x != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

fn py_str(v: Option<&Value>) -> String {
    match v {
        None => "0".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(x) => x.to_string(),
    }
}

/// 仕様書の「人の保証」に出す、宣言の確かめの状態
pub fn status_text(pt: &Part, ledger: &serde_json::Map<String, Value>) -> String {
    let row = match ledger.get(&pt.name) {
        None => return "宣言は実際に呼んで確かめていない（geas parts --record）".to_string(),
        Some(r) => r,
    };
    if row.get("fingerprint").and_then(|v| v.as_str()) != Some(fingerprint(pt).as_str()) {
        return "宣言を変えたあと、確かめ直していない（geas parts --record）".to_string();
    }
    let ex = if row.get("examples_fingerprint").and_then(|v| v.as_str()) == Some(examples_fingerprint(pt).as_str()) {
        format!("。具体例 {} 件と一致", py_str(row.get("examples")))
    } else {
        "。具体例は確かめ直していない（geas parts --record）".to_string()
    };
    if truthy(row.get("proved")) {
        return format!("宣言は全部の入力で証明した（Kani：{}）{}", py_str(row.get("proved")), ex);
    }
    if !truthy(row.get("calls")) {
        return format!("宣言は端の値では確かめられない型{}", ex);
    }
    format!("宣言は端の値で {} 回呼んで確かめた{}{}", py_str(row.get("calls")), if truthy(row.get("reference")) { "（手本とも一致）" } else { "" }, ex)
}
