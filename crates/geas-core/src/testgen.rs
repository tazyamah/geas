// SPDX-License-Identifier: GPL-3.0-or-later
//! 計画書の「URL の具体例」「画面の具体例」から、通しテストを作る（Python 版 testgen.py の移し）。
//! 生成するテストはアプリを知らず、データと汎用の実行部だけでできている。
use crate::ast::*;
use crate::check::Checker;
use crate::crategen::{dumps_indent, ASSETS};
use crate::plan::{route_match, usecases, Plan, Row};
use crate::pyrepr::{repr_opt, repr_str};
use crate::rest::{routes, versioned};
use crate::screengen::screens_def;
use regex::Regex;
use serde_json::{json, Map as JMap, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const HEADER: &str = include_str!("assets/testgen_header.txt");
const SCREEN_TEST: &str = include_str!("assets/testgen_screen_test.txt");

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).unwrap())
}

fn strip_bt(s: &str) -> &str {
    s.trim().trim_matches('`').trim()
}

fn cell(r: &Row, names: &[&str]) -> String {
    for k in names {
        if let Some(v) = r.get(*k) {
            return strip_bt(v).to_string();
        }
    }
    String::new()
}

type UrlRow = (String, String, String, Option<String>, String, i64, Option<String>, Vec<(String, String)>, String);

/// 計画書の URL 例の行を、テストに渡す形にする
pub fn url_rows(ck: &Checker, plan: &Plan) -> Result<Vec<UrlRow>, String> {
    static ID_RE: OnceLock<Regex> = OnceLock::new();
    static KEEP_RE: OnceLock<Regex> = OnceLock::new();
    static VAR_RE: OnceLock<Regex> = OnceLock::new();
    let p = ck.p;
    let rs = routes(p);
    let route_vals: Vec<Value> = rs.iter().map(|r| json!([r.method, r.path])).collect();
    let mut out = vec![];
    let _ = re(&ID_RE, r"x");
    for r in plan.get("URL例").map(|v| v.as_slice()).unwrap_or(&[]) {
        let g = |k: &str| r.get(k).cloned().unwrap_or_default();
        let rid = g("ID").trim().to_string();
        let (method, path) = (g("メソッド"), g("経路"));
        let rt = route_match(&route_vals, &method, &path).ok_or_else(|| format!("URL例 {}: {} {} に当たるルートがありません", rid, method.trim(), path.trim()))?;
        let rt_m = rt[0].as_str().unwrap_or("").to_string();
        let rt_p = rt[1].as_str().unwrap_or("").to_string();
        let names: Vec<String> = rs.iter().filter(|x| x.method == rt_m && x.path == rt_p).flat_map(|x| x.statuses.iter().map(|(v, _)| v.clone())).collect();
        let mut res = if !names.is_empty() { p.resolve(&cell(r, &["期待する結果"]), &names) } else { String::new() };
        if res.is_empty() || res == "－" || res == "-" {
            res = String::new();
        } else if !names.is_empty() && !names.contains(&res) {
            return Err(format!("URL例 {}: 結果「{}」は、ルート {} {} の結果にありません", rid, res, rt_m, rt_p));
        }
        let mut keep = vec![];
        for part in cell(r, &["控える"]).split(';').map(|x| x.trim()).filter(|x| !x.is_empty()) {
            let caps = re(&KEEP_RE, r"^([A-Za-z_]\w*)\s*=\s*(\S+)$").captures(part).ok_or_else(|| format!("URL例 {}: 控え方「{}」は 名前 = 応答の道すじ（例 order_id = body.order_id）で書きます", rid, part))?;
            keep.push((caps[1].to_string(), caps[2].to_string()));
        }
        let body = cell(r, &["入力"]);
        let expect = cell(r, &["期待する本文"]);
        for (label, text) in [("入力", &body), ("期待する本文", &expect)] {
            if !text.is_empty() && text != "－" && text != "-" {
                let t = re(&VAR_RE, r"\{[A-Za-z_]\w*\}").replace_all(text, "x").into_owned();
                if let Err(e) = serde_json::from_str::<Value>(&t) {
                    return Err(format!("URL例 {}: {} が JSON ではありません（{}）", rid, label, e));
                }
            }
        }
        let status: i64 = g("期待する状態").trim().parse().map_err(|_| format!("invalid literal for int() with base 10: {}", repr_str(&g("期待する状態"))))?;
        let ent = rs
            .iter()
            .find(|x| x.method == rt_m && x.path == rt_p && (x.kind == "update" || x.kind == "destroy"))
            .map(|x| versioned(p).get(&x.resource).cloned().unwrap_or_default())
            .unwrap_or_default();
        let none_if = |s: &str| if s.is_empty() || s == "－" || s == "-" { None } else { Some(s.to_string()) };
        out.push((rid, method.trim().to_string(), path.trim().to_string(), none_if(&body), res, status, none_if(&expect), keep, ent));
    }
    Ok(out)
}

fn repr_row(r: &UrlRow) -> String {
    let keep = if r.7.is_empty() { "[]".to_string() } else { format!("[{}]", r.7.iter().map(|(a, b)| format!("({}, {})", repr_str(a), repr_str(b))).collect::<Vec<_>>().join(", ")) };
    format!("({}, {}, {}, {}, {}, {}, {}, {}, {})", repr_str(&r.0), repr_str(&r.1), repr_str(&r.2), repr_opt(&r.3), repr_str(&r.4), r.5, repr_opt(&r.6), keep, repr_str(&r.8))
}

pub fn write_url_tests(ck: &Checker, plan: &Plan, dest: &Path) -> Result<Option<PathBuf>, String> {
    let rows = url_rows(ck, plan)?;
    if rows.is_empty() {
        return Ok(None);
    }
    let out = dest.join("tests");
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let f = out.join("test_url_examples.py");
    let body = format!("[\n{}]", rows.iter().map(|r| format!("    {},\n", repr_row(r))).collect::<String>());
    fs::write(&f, HEADER.replacen("%s", &body, 1)).map_err(|e| e.to_string())?;
    Ok(Some(f))
}

// ---- 画面の具体例 ----
fn kv_pairs(text: &str) -> Vec<(String, String)> {
    static SEP: OnceLock<Regex> = OnceLock::new();
    let sep = re(&SEP, r"\s*[;；]\s*");
    let mut out = vec![];
    for part in sep.split(text.trim().trim_matches('`')) {
        let part = part.trim();
        if part.is_empty() || part == "－" || part == "-" {
            continue;
        }
        match part.find(|c| c == '=' || c == ':' || c == '：') {
            Some(i) => {
                let c = part[i..].chars().next().unwrap();
                out.push((part[..i].trim().to_string(), part[i + c.len_utf8()..].trim().trim_matches('`').to_string()));
            }
            None => out.push((part.trim().to_string(), String::new())),
        }
    }
    out
}

fn js(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

/// 人が書いた名前（表示名でもコードの名前でもよい）で、定義の中から 1 つ探す
fn find<'a>(items: &'a Value, label: &str, key: &str) -> Option<&'a Value> {
    items.as_array()?.iter().find(|it| [it.get(key), it.get("name"), it.get("deed")].iter().any(|v| v.and_then(|x| x.as_str()) == Some(label)))
}

fn field_target(wh: &str, sc: &Value, name: &str, whole: bool) -> Result<JMap<String, Value>, String> {
    let (head, rest) = match name.split_once('.') {
        Some((a, b)) => (a, b),
        None => (name, ""),
    };
    let mut m = JMap::new();
    if let Some(f) = find(&sc["fields"], head, "label") {
        let t = &f["type"];
        let kind = js(t, "kind");
        if kind == "group" {
            let part = find(&t["parts"], rest, "label").ok_or_else(|| format!("{}: 「{}」の「{}」は、入力欄「{}」の部品にありません", wh, name, rest, js(f, "label")))?;
            m.insert("as".into(), json!("field"));
            m.insert("field".into(), json!(format!("{}.{}", js(f, "name"), js(part, "name"))));
            m.insert("map".into(), json!(false));
            return Ok(m);
        }
        if kind == "map" && whole && rest.is_empty() {
            m.insert("as".into(), json!("field"));
            m.insert("field".into(), json!(js(f, "name")));
            return Ok(m);
        }
        if kind == "map" {
            if rest.is_empty() {
                return Err(format!("{}: 「{}」は 明細.行のID の形で個数を書きます", wh, name));
            }
            m.insert("as".into(), json!("count"));
            m.insert("row".into(), json!(rest));
            return Ok(m);
        }
        m.insert("as".into(), json!("field"));
        m.insert("field".into(), json!(js(f, "name")));
        m.insert("map".into(), json!(false));
        return Ok(m);
    }
    if let Some(s) = find(&sc["slots"], head, "label") {
        m.insert("as".into(), json!("slot"));
        m.insert("slot".into(), json!(js(s, "name")));
        return Ok(m);
    }
    Err(format!("{}: 「{}」は、この画面の入力欄にも選ばせる slot にもありません", wh, name))
}

/// 拡張の page パターン（*://host/*）から、テストが差し替えるオリジンを作る
fn origin_of(pattern: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    match re(&R, r"^(?:\*|https?)://([^/*]+)/.*$").captures(pattern) {
        Some(c) => format!("https://{}", &c[1]),
        None => String::new(),
    }
}

fn columns_of(defs: &Value, sc: &Value) -> Vec<Value> {
    sc["shows"].as_array().map(|a| a.iter().flat_map(|q| defs["queries"][q.as_str().unwrap_or("")]["columns"].as_array().cloned().unwrap_or_default()).collect()).unwrap_or_default()
}

fn pick_value(sl: &Value, v: &str) -> String {
    sl["values"].as_array().and_then(|a| a.iter().find(|x| js(x, "value") == v || js(x, "label") == v)).map(|x| js(x, "value")).unwrap_or_else(|| v.to_string())
}

/// ユースケースの手順の表と画面の具体例の表から、通しテストの手順（データ）を作る。計画書の食い違いは Err
pub fn screen_cases(ck: &Checker, plan: &Plan) -> Result<Vec<Value>, String> {
    static ROW_RE: OnceLock<Regex> = OnceLock::new();
    static ADV_RE: OnceLock<Regex> = OnceLock::new();
    if plan.get("画面例").map_or(true, |v| v.is_empty()) {
        return Ok(vec![]);
    }
    let defs = screens_def(ck);
    let scr = &defs["screens"];
    let scr_names: Vec<String> = scr.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    let mut design: JMap<String, Value> = JMap::new();
    for r in plan.get("デザイン").map(|v| v.as_slice()).unwrap_or(&[]) {
        let name = ck.p.resolve(r["画面"].trim(), &scr_names);
        let e = design.entry(name).or_insert_with(|| Value::Object(JMap::new()));
        if let Value::Object(m) = e {
            m.insert(r["指定"].trim().to_string(), json!(r["値"].trim()));
        }
    }
    let pages: indexmap::IndexMap<&str, &Vec<String>> = ck.p.screens.iter().map(|s| (s.name.as_str(), &s.pages)).collect();
    let mut examples: indexmap::IndexMap<(String, String), &Row> = indexmap::IndexMap::new();
    for r in &plan["画面例"] {
        examples.insert((r["ユースケース"].trim().to_string(), r["手順"].trim().to_string()), r);
    }
    let ucs = usecases(plan.get("ユースケース").map(|v| v.as_slice()).unwrap_or(&[]));
    let mut cases: Vec<Value> = vec![];
    for (uc, step) in examples.keys() {
        if !ucs.contains_key(uc) || !ucs[uc].iter().any(|s| s["手順"].trim() == step) {
            return Err(format!("画面の具体例 {} 手順 {}: ユースケースの手順の表にありません", uc, step));
        }
    }
    for (uc, steps) in &ucs {
        if !steps.iter().any(|s| examples.contains_key(&(uc.clone(), s["手順"].trim().to_string()))) {
            continue;
        }
        let mut out: Vec<Value> = vec![];
        for (i, st) in steps.iter().enumerate() {
            let no = st["手順"].trim().to_string();
            let wh = format!("画面の具体例 {} 手順 {}", uc, no);
            let name = ck.p.resolve(st["画面"].trim(), &scr_names);
            if !scr_names.contains(&name) {
                return Err(format!("{}: 画面「{}」がありません", wh, st["画面"].trim()));
            }
            let sc = &scr[&name];
            let empty = Row::new();
            let ex: &Row = examples.get(&(uc.clone(), no.clone())).copied().unwrap_or(&empty);
            let op = st["操作"].trim().to_string();
            let pg = pages.get(name.as_str()).copied();
            let has_pages = pg.map_or(false, |p| !p.is_empty());
            let url = {
                let u = st.get("URL").map(|x| x.trim().to_string()).unwrap_or_default();
                if u.is_empty() { js(sc, "url") } else { u }
            };
            let mut step = JMap::new();
            step.insert("no".into(), json!(no));
            step.insert("screen".into(), json!(name));
            step.insert("label".into(), json!(js(sc, "label")));
            step.insert("url".into(), json!(url));
            step.insert("ext".into(), json!(has_pages));
            step.insert("origin".into(), json!(if has_pages { origin_of(&pg.unwrap()[0]) } else { String::new() }));
            step.insert("set".into(), json!([]));
            step.insert("replace".into(), json!([]));
            step.insert("expect".into(), json!([]));
            step.insert("text".into(), json!(format!("{} 手順 {}（{}／{}）", uc, no, js(sc, "label"), op)));
            let step_op: String;
            if op == "見る" {
                step_op = "look".into();
                step.insert("op".into(), json!("look"));
            } else if op == "行を選ぶ" {
                let nxt = if i + 1 < steps.len() { Some(ck.p.resolve(steps[i + 1]["画面"].trim(), &scr_names)) } else { None };
                let opens: Vec<String> = sc["opens"].as_array().map(|a| a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
                match &nxt {
                    Some(n) if opens.contains(n) => {}
                    _ => return Err(format!("{}: 「行を選ぶ」の次の手順の画面が、この画面から開けません", wh)),
                }
                step_op = "open".into();
                step.insert("op".into(), json!("open"));
                step.insert("opens".into(), json!(nxt));
            } else {
                let deeds: Vec<String> = sc["actions"].as_array().map(|a| a.iter().map(|x| js(x, "deed")).collect()).unwrap_or_default();
                let a = find(&sc["actions"], &ck.p.resolve(&op, &deeds), "deed").or_else(|| find(&sc["actions"], &op, "label")).ok_or_else(|| format!("{}: 操作「{}」は、画面「{}」にありません", wh, op, js(sc, "label")))?;
                step_op = "act".into();
                step.insert("op".into(), json!("act"));
                step.insert("deed".into(), json!(js(a, "deed")));
                let res = st.get("結果").map(|x| x.trim().to_string()).unwrap_or_default();
                if !res.is_empty() {
                    let results: Vec<String> = a["routes"].as_array().map(|x| x.iter().map(|r| js(r, "result")).collect()).unwrap_or_default();
                    let rr = find(&a["routes"], &ck.p.resolve(&res, &results), "result").or_else(|| find(&a["routes"], &res, "label")).ok_or_else(|| format!("{}: 結果「{}」は、操作「{}」の結果にありません", wh, res, op))?;
                    step.insert("result".into(), json!(js(rr, "result")));
                    step.insert("to".into(), rr["to"].clone());
                }
            }
            let kvs = kv_pairs(ex.get("入れる・選ぶ").map(|s| s.as_str()).unwrap_or(""));
            for (k, v) in kvs {
                if step_op == "open" {
                    if k != "行" {
                        return Err(format!("{}: 行を選ぶ手順で指せるのは「行 = ID」だけです", wh));
                    }
                    step.insert("row".into(), json!(v));
                    continue;
                }
                if has_pages {
                    if k != "ページ" && k != "ページの表" {
                        return Err(format!("{}: 拡張の画面で指せるのは「ページ = 経路」と「ページの表 = 見出し: 値」です", wh));
                    }
                    if let Some(Value::Array(a)) = step.get_mut("set") {
                        a.push(json!({"as": if k == "ページ" { "page" } else { "table" }, "value": v}));
                    }
                    continue;
                }
                if k == "返事を落とす" {
                    let rnames: Vec<String> = ck.p.replies.iter().map(|r| r.name.clone()).collect();
                    let rn = ck.p.resolve(&v, &rnames);
                    if !rnames.contains(&rn) {
                        return Err(format!("{}: 返事「{}」は、応答の宣言にありません", wh, v));
                    }
                    let e = step.entry("drop").or_insert_with(|| json!([]));
                    if let Value::Array(a) = e {
                        a.push(json!(rn));
                    }
                    continue;
                }
                if k == "時計を進める" {
                    let c = re(&ADV_RE, r"^(\d+)\s*(秒|分|時間)$").captures(&v).ok_or_else(|| format!("{}: 時計を進める長さ「{}」は 30分・90秒・2時間 の形で書きます", wh, v))?;
                    let n: i64 = c[1].parse().unwrap_or(0);
                    let mul = match &c[2] {
                        "秒" => 1,
                        "分" => 60,
                        _ => 3600,
                    };
                    step.insert("advance".into(), json!(n * mul));
                    continue;
                }
                if let Some(rest) = k.strip_prefix("差し替え ") {
                    let mut t = field_target(&wh, sc, rest.trim(), true)?;
                    t.insert("value".into(), json!(v));
                    if let Some(Value::Array(a)) = step.get_mut("replace") {
                        a.push(Value::Object(t));
                    }
                    continue;
                }
                let mut t = field_target(&wh, sc, &k, false)?;
                let as_ = js(&Value::Object(t.clone()), "as");
                let mut val = json!(v);
                if as_ == "count" {
                    val = json!(v.trim().parse::<i64>().map_err(|_| format!("invalid literal for int() with base 10: {}", repr_str(&v)))?);
                }
                if as_ == "slot" {
                    let sl = find(&sc["slots"], &js(&Value::Object(t.clone()), "slot"), "name").unwrap_or(&Value::Null);
                    val = json!(pick_value(sl, &v));
                }
                t.insert("value".into(), val);
                if let Some(Value::Array(a)) = step.get_mut("set") {
                    a.push(Value::Object(t));
                }
            }
            let tsc: &Value = if step_op == "open" { &scr[js(&Value::Object(step.clone()), "opens")] } else { sc };
            let cols = columns_of(&defs, tsc);
            let kvs = kv_pairs(ex.get("見えること").map(|s| s.as_str()).unwrap_or(""));
            for (k, v) in kvs {
                if ["含む", "含まない", "行数", "行", "行なし", "表示なし", "通知"].contains(&k.as_str()) {
                    if let Some(Value::Array(a)) = step.get_mut("expect") {
                        a.push(json!({"as": k, "value": v}));
                    }
                } else if let Some(rest) = k.strip_prefix("選択 ") {
                    let t = field_target(&wh, tsc, rest.trim(), false)?;
                    let slot = t.get("slot").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let sl = find(&tsc["slots"], &slot, "name").ok_or_else(|| format!("{}: 「{}」は slot ではありません", wh, k))?;
                    let val = pick_value(sl, &v);
                    if let Some(Value::Array(a)) = step.get_mut("expect") {
                        a.push(json!({"as": "選択", "slot": slot, "value": val}));
                    }
                } else {
                    let m = re(&ROW_RE, r"^行\s+(\S+)\s+の\s+(\S+)$").captures(&k);
                    let c = m.as_ref().and_then(|m| cols.iter().find(|c| [c.get("label"), c.get("name"), c.get("deed")].iter().any(|x| x.and_then(|y| y.as_str()) == Some(&m[2]))));
                    let (m, c) = match (m, c) {
                        (Some(m), Some(c)) => (m, c),
                        _ => return Err(format!("{}: 見えること「{}」が読めません（含む・含まない・通知・行数・行・行なし・表示なし・選択 slot・行 ID の 列 のどれか）", wh, k)),
                    };
                    if let Some(Value::Array(a)) = step.get_mut("expect") {
                        a.push(json!({"as": "列", "row": &m[1], "col": js(c, "name"), "value": v}));
                    }
                }
            }
            out.push(Value::Object(step));
        }
        for s in out.iter_mut() {
            let sname = js(s, "screen");
            let d = design.get(&sname).cloned().unwrap_or_else(|| json!({}));
            if let Value::Object(m) = s {
                m.insert("design".into(), d);
            }
        }
        cases.push(json!({"name": uc, "steps": out}));
    }
    Ok(cases)
}

pub fn write_screen_tests(ck: &Checker, plan: &Plan, dest: &Path) -> Result<Option<PathBuf>, String> {
    let cases = screen_cases(ck, plan)?;
    if cases.is_empty() {
        return Ok(None);
    }
    let out = dest.join("tests");
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    fs::write(out.join("screen_cases.json"), dumps_indent(&Value::Array(cases), 1) + "\n").map_err(|e| e.to_string())?;
    let runner = ASSETS.get_file("e2e/screen_runner.js").ok_or("e2e/screen_runner.js")?;
    fs::write(out.join("screen_runner.js"), runner.contents()).map_err(|e| e.to_string())?;
    let f = out.join("test_screen_examples.py");
    fs::write(&f, SCREEN_TEST).map_err(|e| e.to_string())?;
    Ok(Some(f))
}
