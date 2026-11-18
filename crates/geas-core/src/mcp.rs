// SPDX-License-Identifier: GPL-3.0-or-later
//! MCP（Model Context Protocol）のツール定義。REST の定義（routes）から機械的に作る（Python 版 mcp.py の移し）。
use crate::ast::*;
use crate::check::Checker;
use crate::rest::{first_by_name, internal_targets, path_inputs, post_queries, q_inputs, q_types, routes as rest_routes, versioned};
use regex::Regex;
use serde_json::{json, Map, Value};

/// 文字の範囲の組（コードポイント）から、正規表現の文字クラス
pub fn class_of(chars: &[(u32, u32)]) -> String {
    let esc = |c: u32| -> String {
        let ch = char::from_u32(c).unwrap_or('\u{fffd}');
        format!("{}{}", if "\\]^-[".contains(ch) { "\\" } else { "" }, ch)
    };
    format!("[{}]", chars.iter().map(|&(lo, hi)| if lo == hi { esc(lo) } else { format!("{}-{}", esc(lo), esc(hi)) }).collect::<String>())
}

pub fn type_schema(ck: &Checker, t: &Type) -> Map<String, Value> {
    let k = t.kind.as_str();
    let r = t.range.as_ref();
    let mut s = Map::new();
    match k {
        "ID" | "参照" => {
            s.insert("type".into(), json!("string"));
        }
        "文字列" => {
            s.insert("type".into(), json!("string"));
            if let Some(lo) = r.and_then(|r| r.lo) {
                s.insert("minLength".into(), json!(lo));
            }
            if let Some(hi) = r.and_then(|r| r.hi) {
                s.insert("maxLength".into(), json!(hi));
            }
            if let Some(c) = t.chars.as_ref().filter(|c| !c.is_empty()) {
                s.insert("pattern".into(), json!(format!("^{}*$", class_of(c))));
            }
        }
        "真偽" => {
            s.insert("type".into(), json!("boolean"));
        }
        "整数" | "金額" | "日時" => {
            s.insert("type".into(), json!("integer"));
            if let Some(lo) = r.and_then(|r| r.lo) {
                s.insert("minimum".into(), json!(lo));
            }
            if let Some(hi) = r.and_then(|r| r.hi) {
                s.insert("maximum".into(), json!(hi));
            }
            if k == "金額" {
                s.insert("description".into(), json!("金額（通貨の最小単位の整数）"));
            }
        }
        "割合" => {
            s.insert("type".into(), json!("string"));
            s.insert("description".into(), json!("割合（小数の文字列。例 0.1）"));
        }
        "バイト列" => {
            s.insert("type".into(), json!("string"));
            s.insert("contentEncoding".into(), json!("base64"));
        }
        "集合" => {
            s.insert("type".into(), json!("array"));
            s.insert("items".into(), json!({"type": "string"}));
            s.insert("uniqueItems".into(), json!(true));
        }
        "名前" => {
            s.insert("type".into(), json!("string"));
            if let Some(st) = t.target.as_ref().and_then(|x| ck.states.get(x)) {
                let mut names = vec![st.initial.clone()];
                names.extend(st.middle.iter().cloned());
                names.extend(st.final_.iter().cloned());
                s.insert("enum".into(), json!(names));
            }
        }
        "合成" => {
            let parts = t.parts.as_deref().unwrap_or(&[]);
            s.insert("type".into(), json!("object"));
            let props: Map<String, Value> = parts.iter().map(|(n, pt)| (n.clone(), Value::Object(type_schema(ck, pt)))).collect();
            s.insert("properties".into(), Value::Object(props));
            s.insert("required".into(), json!(parts.iter().filter(|(_, pt)| !pt.optional).map(|(n, _)| n.clone()).collect::<Vec<_>>()));
        }
        "写像" => {
            s.insert("type".into(), json!("object"));
            s.insert("additionalProperties".into(), Value::Object(type_schema(ck, t.value.as_deref().unwrap())));
        }
        _ => {}
    }
    s
}

pub fn tool_name(kind: &str, path: &str) -> String {
    let words: Vec<&str> = path.trim_matches('/').split('/').filter(|w| !w.starts_with(':')).collect();
    Regex::new(r"\W").unwrap().replace_all(&format!("{}_{}", kind, words.join("_")), "_").into_owned()
}

fn described(ck: &Checker, t: &Type, desc: &str) -> Value {
    let mut m = type_schema(ck, &Type { optional: false, ..t.clone() });
    m.insert("description".into(), json!(desc));
    Value::Object(m)
}

pub fn tool_defs(ck: &Checker) -> Result<Vec<Value>, String> {
    let p = ck.p;
    let show = |s: &str| p.show_names(s);
    let server_slots: Vec<&String> = p.rules.iter().map(|f| &f.slot).collect();
    let by_res = first_by_name(p);
    let posts = post_queries(p);
    let inner = internal_targets(p);
    let mut out: Vec<Value> = vec![];
    for rt in rest_routes(p) {
        let (method, path, kind, target, res, statuses) = (&rt.method, &rt.path, &rt.kind, &rt.target, &rt.resource, &rt.statuses);
        if inner.contains(target) {
            continue;
        }
        let mut props: Map<String, Value> = Map::new();
        let mut required: Vec<String> = vec![];
        let mut q_names: Vec<String> = vec![];
        let caps: Vec<String> = path.split('/').filter(|s| s.starts_with(':')).map(|s| s[1..].to_string()).collect();
        let proc = ck.procs.get(target).copied();
        let query = p.queries.iter().find(|q| &q.name == target);
        let inputs: Vec<(String, Type)> = match (proc, query) {
            (Some(pr), _) => pr.inputs.clone(),
            (None, Some(q)) => q.inputs.clone(),
            _ => vec![],
        };
        if let Some(pr) = proc {
            let id_in = if kind == "update" || kind == "destroy" { pr.inputs.iter().find(|(_, t)| t.kind == "ID").map(|(n, _)| n.clone()).unwrap_or_default() } else { String::new() };
            for c in &caps {
                props.insert(c.clone(), json!({"type": "string", "description": format!("{}のキー（URL の /:{}）", show(res), c)}));
                required.push(c.clone());
            }
            for (n, t) in &inputs {
                if *n == id_in {
                    continue;
                }
                props.insert(n.clone(), described(ck, t, &show(n)));
                if !t.optional {
                    required.push(n.clone());
                }
            }
            let mut slots = Map::new();
            for (n, t) in &pr.refs {
                if !server_slots.contains(&n) {
                    let mut m = type_schema(ck, t);
                    m.insert("description".into(), json!(show(n)));
                    slots.insert(n.clone(), Value::Object(m));
                }
            }
            if !slots.is_empty() {
                let req: Vec<String> = slots.keys().cloned().collect();
                props.insert("slots".into(), json!({"type": "object", "description": "外から決めて渡す値（サーバーが決めない slot）", "properties": Value::Object(slots), "required": req}));
                required.push("slots".into());
            }
        } else {
            let res_def = by_res[res.as_str()];
            let action = res_def.actions.iter().find(|a| &a.target == target);
            let on_path: Vec<String> = match (query, kind.as_str()) {
                (Some(q), "index") | (Some(q), "show") => path_inputs(p, res_def, action.unwrap(), q),
                _ => inputs.iter().map(|(n, _)| n.clone()).collect(),
            };
            for (c, n) in caps.iter().zip(on_path.iter()) {
                let t = inputs.iter().find(|(x, _)| x == n).map(|(_, t)| t.clone()).unwrap_or_default();
                let mut m = type_schema(ck, &t);
                m.insert("description".into(), json!(show(n)));
                props.insert(c.clone(), Value::Object(m));
                required.push(c.clone());
            }
            if let (Some(q), "index" | "show") = (query, kind.as_str()) {
                let qt = q_types(q);
                for n in q_inputs(p, res_def, action.unwrap(), q) {
                    let t = qt[&n].clone();
                    q_names.push(n.clone());
                    props.insert(n.clone(), described(ck, &t, &show(&n)));
                    if !t.optional {
                        required.push(n.clone());
                    }
                }
            }
            if kind == "create" {
                for (n, t) in &inputs {
                    props.insert(n.clone(), described(ck, t, &show(n)));
                    if !t.optional {
                        required.push(n.clone());
                    }
                }
            }
        }
        let rows = versioned(p);
        if (kind == "update" || kind == "destroy") && rows.contains_key(res) {
            props.insert("if_match".into(), json!({"type": "string", "description": "読んだときの行の版。一覧の行の _etag か、1 件の読み出しの答えの ETag。読んだあとに行が変わっていたら、書き込まずに 412 を返す"}));
            required.push("if_match".into());
        }
        let mut desc = format!("{}（{} {}）", show(target), method, path);
        if !statuses.is_empty() {
            // dict(statuses)：同じ結果の種類は後の状態が残る（位置は最初）
            let mut d: indexmap::IndexMap<&String, &i128> = indexmap::IndexMap::new();
            for (k, v) in statuses {
                d.insert(k, v);
            }
            desc += &format!("。結果：{}", d.iter().map(|(k, v)| format!("{}→{}", show(k), v)).collect::<Vec<_>>().join("、"));
        }
        let body: Vec<String> = if method != "GET" { props.keys().filter(|n| !caps.contains(n) && *n != "if_match").cloned().collect() } else { vec![] };
        let mut geas = Map::new();
        geas.insert("method".into(), json!(method));
        geas.insert("path".into(), json!(path));
        geas.insert("body".into(), json!(body));
        if props.contains_key("if_match") {
            geas.insert("if_match".into(), json!(true));
        }
        if kind == "show" && rows.contains_key(res) {
            geas.insert("etag".into(), json!(true));
        }
        if !q_names.is_empty() {
            geas.insert("q".into(), json!(q_names));
        }
        if kind == "index" && posts.contains_key(target) {
            geas.insert("post_q".into(), json!(true));
        }
        out.push(json!({
            "name": tool_name(kind, path),
            "title": show(target),
            "description": desc,
            "inputSchema": {"type": "object", "properties": Value::Object(props), "required": required},
            "annotations": {"readOnlyHint": method == "GET" || (kind == "index" && posts.contains_key(target)), "destructiveHint": method == "DELETE", "idempotentHint": method == "GET" || method == "PUT" || method == "DELETE"},
            "geas": Value::Object(geas),
        }));
    }
    let names: Vec<String> = out.iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    let mut dup: Vec<String> = names.iter().filter(|n| names.iter().filter(|m| m == n).count() > 1).cloned().collect::<indexmap::IndexSet<_>>().into_iter().collect();
    if !dup.is_empty() {
        dup.sort();
        return Err(format!("MCP のツール名が重なっています: [{}]", dup.iter().map(|x| format!("'{}'", x)).collect::<Vec<_>>().join(", ")));
    }
    Ok(out)
}
