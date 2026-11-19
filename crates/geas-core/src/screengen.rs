// SPDX-License-Identifier: GPL-3.0-or-later
//! 画面（screen）から、実行時の画面が使う定義と、Rust の定数を作る（Python 版 screengen.py の移し）。
//! 画面のファイル（HTML・JS）は汎用のもの（ui）で、ここで作る定義だけで動く。
use crate::ast::*;
use crate::check::Checker;
use crate::rest::screen_url;
use crate::rustgen::column_type;
use serde_json::{json, Map as JMap, Value};

/// 通貨ごとの最小単位の桁数（金額は最小単位の整数で持つ）
pub const CURRENCY_MINOR: [(&str, i64); 3] = [("JPY", 0), ("USD", 2), ("EUR", 2)];
pub const DEFAULT_ACCENT: &str = "#0b5cad";

pub fn jint(v: i128) -> Value {
    if let Ok(x) = i64::try_from(v) {
        json!(x)
    } else if let Ok(x) = u64::try_from(v) {
        json!(x)
    } else {
        json!(v as f64)
    }
}
pub fn jopt(v: Option<i128>) -> Value {
    v.map_or(Value::Null, jint)
}

pub fn design_of(sc: &Screen) -> (String, String, String) {
    let g = |k: &str, d: &str| sc.design.get(k).cloned().unwrap_or_else(|| d.to_string());
    (g("layout", "stack"), g("accent", DEFAULT_ACCENT), g("mode", "light"))
}

pub fn design_json(sc: &Screen) -> Value {
    let (l, a, m) = design_of(sc);
    json!({"layout": l, "accent": a, "mode": m})
}

fn merge(mut head: JMap<String, Value>, tail: Value) -> Value {
    if let Value::Object(m) = tail {
        for (k, v) in m {
            head.insert(k, v);
        }
    }
    Value::Object(head)
}

/// 入力・列の型を、画面が欄を作るのに足りる形にする
pub fn type_def(ck: &Checker, t: &Type, show: &dyn Fn(&str) -> String) -> Value {
    match t.kind.as_str() {
        "ID" | "参照" => json!({"kind": "id"}),
        "整数" => json!({"kind": "int", "lo": jopt(t.range.as_ref().and_then(|r| r.lo)), "hi": jopt(t.range.as_ref().and_then(|r| r.hi))}),
        "金額" => {
            let u = t.unit.clone().unwrap_or_default();
            if let Some(rest) = u.strip_prefix('$') {
                json!({"kind": "money", "unit": rest})
            } else {
                json!({"kind": "money", "currency": u})
            }
        }
        "名前" => {
            let st = t.target.as_ref().and_then(|x| ck.states.get(x));
            let mut values: Vec<String> = vec![];
            if let Some(st) = st {
                if !st.initial.is_empty() {
                    values.push(st.initial.clone());
                }
                values.extend(st.middle.iter().cloned());
                values.extend(st.final_.iter().cloned());
            }
            json!({"kind": "enum", "values": values.iter().map(|v| json!({"value": v, "label": show(v)})).collect::<Vec<_>>()})
        }
        "真偽" => json!({"kind": "bool"}),
        "文字列" => json!({"kind": "text", "min": jopt(t.range.as_ref().and_then(|r| r.lo)), "max": jopt(t.range.as_ref().and_then(|r| r.hi))}),
        "合成" => {
            let parts: Vec<Value> = t
                .parts
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .map(|(n, pt)| {
                    let mut m = JMap::new();
                    m.insert("name".into(), json!(n));
                    m.insert("label".into(), json!(show(n)));
                    merge(m, type_def(ck, pt, show))
                })
                .collect();
            json!({"kind": "group", "parts": parts})
        }
        "写像" => json!({"kind": "map", "key": t.key, "value": type_def(ck, t.value.as_deref().unwrap(), show)}),
        _ => json!({"kind": "text"}),
    }
}

pub fn query_def(ck: &Checker, q: &Query, show: &dyn Fn(&str) -> String) -> Value {
    let mut cols: Vec<Value> = vec![];
    for c in &q.select {
        let mut m = JMap::new();
        m.insert("name".into(), json!(c.name));
        m.insert("label".into(), json!(show(&c.name)));
        cols.push(merge(m, type_def(ck, &column_type(ck, q, c), show)));
    }
    if cols.is_empty() {
        cols = vec![json!({"name": "id", "label": "ID", "kind": "id"})];
    }
    let inputs: Vec<Value> = q
        .inputs
        .iter()
        .map(|(n, t)| {
            let mut m = JMap::new();
            m.insert("name".into(), json!(n));
            merge(m, type_def(ck, t, show))
        })
        .collect();
    let mut out = JMap::new();
    out.insert("inputs".into(), Value::Array(inputs));
    out.insert("columns".into(), Value::Array(cols));
    out.insert("entity".into(), json!(q.target));
    let choices = order_choices(q);
    if !choices.is_empty() {
        let kinds: Vec<(&str, &str)> = ck.data.get(&q.target).map(|d| d.fields.iter().map(|f| (f.name.as_str(), f.r#type.kind.as_str())).collect()).unwrap_or_default();
        let words = |f: &str, d: &str| -> &'static str {
            let is_time = kinds.iter().any(|(n, k)| *n == f && *k == "日時");
            match (is_time, d != "降順") {
                (true, false) => "新しい順",
                (true, true) => "古い順",
                (false, false) => "大きい順",
                (false, true) => "小さい順",
            }
        };
        out.insert("orders".into(), Value::Array(choices.iter().map(|(name, f, d)| json!({"value": name, "label": format!("{} {}", show(f), words(f, d))})).collect()));
    }
    Value::Object(out)
}

/// 入力欄の型。呼ぶ処理の入力から、無ければ見せる問い合わせの入力から取る
pub fn field_type(ck: &Checker, sc: &Screen, f: &str, show: &dyn Fn(&str) -> String) -> Value {
    for (dn, _) in &sc.actions {
        for (n, t) in &ck.procs[dn.as_str()].inputs {
            if n == f {
                return type_def(ck, t, show);
            }
        }
    }
    for qn in &sc.shows {
        for (n, t) in &ck.queries[qn.as_str()].inputs {
            if n == f {
                return type_def(ck, t, show);
            }
        }
    }
    json!({"kind": "text"})
}

pub fn screens_def(ck: &Checker) -> Value {
    let p = ck.p;
    let show = |s: &str| p.show_names(s);
    let first = p.screens.first().map(|s| s.name.clone()).unwrap_or_default();
    let mut screens = JMap::new();
    for sc in &p.screens {
        let mut actions: Vec<Value> = vec![];
        for (dn, routes) in &sc.actions {
            let proc = ck.procs[dn.as_str()];
            let rdef = ck.results[&proc.result];
            let mut variants: JMap<String, Value> = JMap::new();
            for (v, ps) in &rdef.variants {
                variants.insert(v.clone(), Value::Array(ps.iter().map(|(n, _)| json!({"name": n, "label": show(n)})).collect()));
            }
            let inputs: Vec<Value> = proc
                .inputs
                .iter()
                .map(|(n, t)| {
                    let mut m = JMap::new();
                    m.insert("name".into(), json!(n));
                    m.insert("label".into(), json!(show(n)));
                    merge(m, type_def(ck, t, &show))
                })
                .collect();
            actions.push(json!({
                "deed": dn, "label": show(dn),
                "inputs": inputs,
                "routes": routes.iter().map(|(v, d)| json!({"result": v, "label": show(v), "to": d, "body": variants.get(v).cloned().unwrap_or_else(|| json!([]))})).collect::<Vec<_>>(),
            }));
        }
        let mut slots: Vec<Value> = vec![];
        let mut slot_names: Vec<String> = vec![];
        for (dn, _) in &sc.actions {
            for (n, t) in &ck.procs[dn.as_str()].refs {
                if sc.slots.contains(n) && !slot_names.contains(n) {
                    slot_names.push(n.clone());
                    let mut m = JMap::new();
                    m.insert("name".into(), json!(n));
                    m.insert("label".into(), json!(show(n)));
                    slots.push(merge(m, type_def(ck, t, &show)));
                }
            }
        }
        let fields: Vec<Value> = sc
            .fields
            .iter()
            .map(|f| json!({"name": f, "label": show(f), "from": sc.field_from.get(f), "type": field_type(ck, sc, f, &show)}))
            .collect();
        screens.insert(
            sc.name.clone(),
            json!({
                "label": show(&sc.name), "url": screen_url(p, sc), "entry": sc.entry || sc.name == first,
                "params": sc.params.iter().map(|(n, _)| json!({"name": n, "label": show(n)})).collect::<Vec<_>>(),
                "shows": sc.shows, "opens": sc.opens,
                "fields": fields,
                "slots": slots, "actions": actions, "design": design_json(sc),
            }),
        );
    }
    let mut queries = JMap::new();
    for q in &p.queries {
        let mut m = JMap::new();
        m.insert("label".into(), json!(show(&q.name)));
        queries.insert(q.name.clone(), merge(m, query_def(ck, q, &show)));
    }
    let mut events_on = JMap::new();
    for ev in &p.events {
        if !ev.on.is_empty() {
            events_on.insert(ev.name.clone(), json!(ev.on));
        }
    }
    let mut minor = JMap::new();
    for (k, v) in CURRENCY_MINOR {
        minor.insert(k.into(), json!(v));
    }
    json!({
        "title": if !p.title.is_empty() { p.title.clone() } else if !first.is_empty() { show(&first) } else { String::new() },
        "first": first,
        "order": p.screens.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
        "minor": minor,
        "screens": screens,
        "queries": queries,
        "events_on": events_on,
    })
}

fn esc_q(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// lib.rs に足す定数。サーバーは、ここから manifest のテーマ色や名前を作る
pub fn rust_screens(ck: &Checker) -> String {
    let p = ck.p;
    if p.screens.is_empty() {
        return String::new();
    }
    let first = &p.screens[0].name;
    let title = esc_q(&if !p.title.is_empty() { p.title.clone() } else { p.show_names(first) });
    let mut out: Vec<String> = vec![
        "// 【画面】：screen の宣言。見た目（design）はここに載る。画面のファイルは汎用で、定義（DEFS_JSON の screens）で動く。".into(),
        "pub struct ScreenDef { pub name: &'static str, pub label: &'static str, pub url: &'static str, pub entry: bool, pub layout: &'static str, pub accent: &'static str, pub mode: &'static str }".into(),
        format!("pub const APP_TITLE: &str = \"{}\";", title),
        "pub const screens: &[ScreenDef] = &[".into(),
    ];
    for sc in &p.screens {
        let (l, a, m) = design_of(sc);
        let label = esc_q(&p.show_names(&sc.name));
        let entry = if sc.entry || &sc.name == first { "true" } else { "false" };
        out.push(format!("    ScreenDef {{ name: \"{}\", label: \"{}\", url: \"{}\", entry: {}, layout: \"{}\", accent: \"{}\", mode: \"{}\" }},", sc.name, label, screen_url(p, sc), entry, l, a, m));
    }
    out.push("];\n".into());
    out.join("\n")
}
