// SPDX-License-Identifier: GPL-3.0-or-later
//! 運用のログから、仕様書へ戻る（Python 版 trace.py の移し）。
use crate::check::Checker;
use crate::plan::Row;
use crate::rest::routes;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub fn read_log(text: &str) -> Vec<Value> {
    let mut out = vec![];
    for line in crate::mutate::splitlines(text) {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        if let Ok(d) = serde_json::from_str::<Value>(line) {
            if d.get("log").and_then(|v| v.as_str()) == Some("geas.route") {
                out.push(d);
            }
        }
    }
    out
}

pub struct TRow {
    pub method: String,
    pub route: String,
    pub target: String,
    pub result: String,
    pub count: usize,
    pub status: Vec<i64>,
    pub p50_ms: i64,
    pub max_ms: i64,
    pub usecases: Vec<String>,
    pub screens: Vec<String>,
    pub why: String,
}

pub struct Trace {
    pub rows: Vec<TRow>,
    pub unknown: Vec<TRow>,
}

fn int(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n.as_i64().unwrap_or_else(|| n.as_f64().unwrap_or(0.0) as i64),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        Some(Value::Bool(b)) => *b as i64,
        _ => 0,
    }
}

fn st(e: &Value, k: &str) -> String {
    match e.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(o) => o.to_string(),
    }
}

pub fn trace(ck: &Checker, entries: &[Value], plan_rows: Option<&[Row]>) -> Trace {
    let show = |s: &str| ck.p.show_names(s);
    let spec: BTreeMap<(String, String), (String, String, BTreeMap<String, i128>)> = routes(ck.p).into_iter().map(|r| ((r.method, r.path), (r.kind, r.target, r.statuses.into_iter().collect()))).collect();
    let mut by_op: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for r in plan_rows.unwrap_or(&[]) {
        by_op.entry(r.get("操作").map(|s| s.trim().to_string()).unwrap_or_default()).or_default().push(r);
    }
    let mut groups: BTreeMap<(String, String, String, String), Vec<&Value>> = BTreeMap::new();
    for e in entries {
        groups.entry((st(e, "method"), st(e, "route"), st(e, "target"), st(e, "result"))).or_default().push(e);
    }
    let (mut rows, mut unknown) = (vec![], vec![]);
    for ((m, route, target, result), es) in groups {
        let mut ms: Vec<i64> = es.iter().map(|e| int(e.get("ms"))).collect();
        ms.sort();
        let statuses: Vec<i64> = es.iter().map(|e| int(e.get("status"))).collect::<BTreeSet<_>>().into_iter().collect();
        let mut row = TRow { method: m.clone(), route: route.clone(), target: show(&target), result: show(&result), count: es.len(), status: statuses.clone(), p50_ms: ms[ms.len() / 2], max_ms: *ms.last().unwrap(), usecases: vec![], screens: vec![], why: String::new() };
        let hit = spec.get(&(m, route));
        let Some(hit) = hit.filter(|h| h.1 == target) else {
            row.why = "仕様書のルート表にない経路です".into();
            unknown.push(row);
            continue;
        };
        let want = if !result.is_empty() { hit.2.get(&result).copied() } else { None };
        if !result.is_empty() && want.is_none() {
            row.why = "仕様書にない結果の種類です".into();
            unknown.push(row);
            continue;
        }
        if let Some(w) = want {
            if statuses != vec![w as i64] {
                row.why = format!("HTTP の状態が仕様書（{}）と違います", w);
                unknown.push(row);
                continue;
            }
        }
        let steps: Vec<&&Row> = by_op.get(&show(&target)).map(|v| v.iter().collect()).unwrap_or_default();
        let sr = show(&result);
        row.usecases = steps
            .iter()
            .filter(|s| result.is_empty() || s.get("結果").map_or(true, |x| x.is_empty()) || s["結果"].trim() == sr)
            .map(|s| s.get("ユースケース").cloned().unwrap_or_default())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        row.screens = steps.iter().map(|s| s.get("画面").cloned().unwrap_or_default()).collect::<BTreeSet<_>>().into_iter().collect();
        rows.push(row);
    }
    Trace { rows, unknown }
}

pub fn to_markdown(t: &Trace, name: &str) -> String {
    let sts = |r: &TRow| r.status.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("/");
    let none = |s: &str| if s.is_empty() { "（なし）".to_string() } else { s.to_string() };
    let mut out: Vec<String> = vec![format!("# ログと仕様書の対応（{}）", name), String::new()];
    let ok: usize = t.rows.iter().map(|r| r.count).sum();
    let ng: usize = t.unknown.iter().map(|r| r.count).sum();
    out.push(format!("ログ {} 件。仕様書に結べたもの {} 件、結べなかったもの {} 件。", ok + ng, ok, ng));
    out.push(String::new());
    if !t.unknown.is_empty() {
        out.extend(["## 仕様書に結べなかったもの（最初に見てください）", "", "コードが仕様書と違う動きをした記録か、古い版のログです。", "", "| 動作 | 経路 | 処理 | 結果 | 状態 | 件数 | 理由 |", "|---|---|---|---|---|---|---|"].map(String::from));
        out.extend(t.unknown.iter().map(|r| format!("| {} | {} | {} | {} | {} | {} | {} |", r.method, r.route, r.target, none(&r.result), sts(r), r.count, r.why)));
        out.push(String::new());
    }
    if !t.rows.is_empty() {
        out.extend(["## 仕様書に結べたもの", "", "| 動作 | 経路 | 処理 | 結果 | 状態 | 件数 | 中央値 ms | 最大 ms | ユースケース | 画面 |", "|---|---|---|---|---|---|---|---|---|---|"].map(String::from));
        out.extend(t.rows.iter().map(|r| format!("| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |", r.method, r.route, r.target, none(&r.result), sts(r), r.count, r.p50_ms, r.max_ms, r.usecases.join("、"), r.screens.join("、"))));
    }
    out.join("\n") + "\n"
}
