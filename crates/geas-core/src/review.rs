// SPDX-License-Identifier: GPL-3.0-or-later
//! 人が読む仕様書（HTML）を、コードから生成する（Python 版 review.py の移し）。
//! 見た目と構造は specgap のレビュー画面に合わせ、図は外部の読み込みのない自前の SVG で描く。
use crate::ast::*;
use crate::check::{Checker, QUESTION, WARN};
use crate::mcp::tool_defs;
use crate::plan::*;
use crate::pynum::escape;
use crate::spec::{build, level_counts, Spec, ACTOR_NOTE, POST_NOTE, SCOPE_NOTE, STOPS_NOTE};
use crate::svg;
use indexmap::{IndexMap, IndexSet};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

const CSS: &str = include_str!("assets/review_css.txt");
const TAB_JS: &str = include_str!("assets/review_tab_js.txt");
const JS: &str = include_str!("assets/review_js.txt");

/// Python の json.dumps（ensure_ascii=False。区切りは ", " と ": "）
pub fn pydumps(v: &Value) -> String {
    match v {
        Value::Array(a) => format!("[{}]", a.iter().map(pydumps).collect::<Vec<_>>().join(", ")),
        Value::Object(m) => format!("{{{}}}", m.iter().map(|(k, x)| format!("{}: {}", serde_json::to_string(k).unwrap(), pydumps(x))).collect::<Vec<_>>().join(", ")),
        Value::String(s) => serde_json::to_string(s).unwrap(),
        o => o.to_string(),
    }
}

fn inline_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"`([^`]+)`").unwrap())
}

pub fn inline(s: &str) -> String {
    inline_re().replace_all(&escape(s), "<code>$1</code>").into_owned()
}

pub fn table<S: AsRef<str>>(head: &[&str], rows: &[Vec<S>]) -> String {
    table_cls(head, rows, "")
}

pub fn table_cls<S: AsRef<str>>(head: &[&str], rows: &[Vec<S>], cls: &str) -> String {
    let th: String = head.iter().map(|h| format!("<th>{}</th>", escape(h))).collect();
    let body: String = rows.iter().map(|r| format!("<tr>{}</tr>", r.iter().map(|c| format!("<td>{}</td>", inline(c.as_ref()).replace('\n', "<br>"))).collect::<String>())).collect();
    format!("<div class=\"scroll\"><table class=\"grid {}\"><thead><tr>{}</tr></thead><tbody>{}</tbody></table></div>", cls, th, body)
}

/// 仕様書の表（JSON の行の並び）から、文字列の行にする。列を絞るなら upto
fn srows(v: &[Value]) -> Vec<Vec<String>> {
    v.iter().map(|r| r.as_array().map(|a| a.iter().map(|c| c.as_str().map(|s| s.to_string()).unwrap_or_else(|| c.to_string())).collect()).unwrap_or_default()).collect()
}

fn rows<'a>(spec: &'a Spec, k: &str) -> &'a [Value] {
    spec.get(k).and_then(|v| v.as_array()).map(|a| a.as_slice()).unwrap_or(&[])
}
fn cs(v: &Value, i: usize) -> String {
    v[i].as_str().map(|x| x.to_string()).unwrap_or_else(|| v[i].to_string())
}
fn sj(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}
fn jarr(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().map(sj).collect()).unwrap_or_default()
}
fn has(spec: &Spec, k: &str) -> bool {
    !rows(spec, k).is_empty()
}

pub fn md_to_html(md: &str) -> String {
    let (mut out, mut trs, mut items): (Vec<String>, Vec<Vec<String>>, Vec<String>) = (vec![], vec![], vec![]);
    fn flush(out: &mut Vec<String>, trs: &mut Vec<Vec<String>>, items: &mut Vec<String>) {
        if !trs.is_empty() {
            let head: Vec<String> = trs[0].clone();
            let body: Vec<Vec<String>> = trs.iter().skip(2).cloned().collect();
            let h: Vec<&str> = head.iter().map(|s| s.as_str()).collect();
            out.push(table(&h, &body));
            trs.clear();
        }
        if !items.is_empty() {
            out.push(format!("<ul>{}</ul>", items.iter().map(|i| format!("<li>{}</li>", inline(i))).collect::<String>()));
            items.clear();
        }
    }
    for line in md.lines() {
        let s = line.trim();
        if s.starts_with('|') {
            trs.push(s.trim_matches('|').split('|').map(|c| c.trim().to_string()).collect());
            continue;
        }
        if let Some(r) = s.strip_prefix("- ") {
            items.push(r.to_string());
            continue;
        }
        flush(&mut out, &mut trs, &mut items);
        if s.starts_with('#') {
            out.push(format!("<h3>{}</h3>", inline(s.trim_start_matches('#').trim())));
        } else if !s.is_empty() {
            out.push(format!("<p>{}</p>", inline(s)));
        }
    }
    flush(&mut out, &mut trs, &mut items);
    out.join("\n")
}

fn split_h2(md: &str) -> Vec<String> {
    // re.split(r"^## ", md, flags=re.M)
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^## ").unwrap()).split(md).map(|s| s.to_string()).collect()
}

fn partition_nl(s: &str) -> (&str, &str) {
    match s.find('\n') {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, ""),
    }
}

/// 計画書の「0. 概要」と「0.1 利用の形態」の本文
pub fn plan_sections(plan_md: &str) -> (String, String) {
    let (mut overview, mut usage) = (String::new(), String::new());
    for part in split_h2(plan_md) {
        let (head, body) = partition_nl(&part);
        if head.starts_with("0.1") {
            usage = body.to_string();
        } else if head.starts_with("0.2") {
            continue;
        } else if head.starts_with("0.") {
            overview = body.to_string();
        }
    }
    (overview, usage)
}

/// 「0.2 その他の利用イメージ」：前置きの文章と、場面ごとの台本
pub fn plan_others(plan_md: &str) -> (String, Vec<(String, Vec<Row>)>) {
    static R: OnceLock<Regex> = OnceLock::new();
    let h3 = R.get_or_init(|| Regex::new(r"(?m)^### ").unwrap());
    for part in split_h2(plan_md) {
        let (head, body) = partition_nl(&part);
        if !head.starts_with("0.2") {
            continue;
        }
        let mut it = h3.split(body);
        let intro = it.next().unwrap_or("").to_string();
        let mut out = vec![];
        for sc in it {
            let (title, rest) = partition_nl(sc);
            let rs: Vec<Row> = tables(rest).into_iter().flat_map(|(_, rs)| rs).collect();
            out.push((title.trim().to_string(), rs));
        }
        return (intro, out);
    }
    (String::new(), vec![])
}

fn mcp_summary(t: &Value) -> String {
    let sch = &t["inputSchema"];
    let req: Vec<&str> = sch["required"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
    let ps: Vec<String> = sch["properties"].as_object().map(|m| m.keys().map(|n| format!("{}{}", n, if req.contains(&n.as_str()) { "＊" } else { "" })).collect()).unwrap_or_default();
    if ps.is_empty() {
        "なし".into()
    } else {
        ps.join("、")
    }
}

fn mcp_section(tools: &[Value]) -> String {
    let rows: Vec<Vec<String>> = tools
        .iter()
        .map(|t| {
            vec![
                sj(&t["name"]),
                sj(&t["title"]),
                format!("{} {}", sj(&t["geas"]["method"]), sj(&t["geas"]["path"])),
                mcp_summary(t),
                if t["annotations"]["readOnlyHint"].as_bool() == Some(true) {
                    "読むだけ".into()
                } else if t["annotations"]["destructiveHint"].as_bool() == Some(true) {
                    "消す".into()
                } else {
                    "書く".into()
                },
            ]
        })
        .collect();
    lead("AI（MCP クライアント）は、人や画面と同じ URL・同じ処理を、次のツールとして呼べます。ツールの定義は、URL の定義から機械的に作ったもので、手で足したものはありません。押して動かすことはできません。入力の「＊」は必須です。")
        + &table(&["ツール名", "呼ぶもの", "URL", "入力", "種類"], &rows)
}

/// 台本のツール呼び出し（名前と JSON の入力）が、ツール定義と合っているか。合っていれば空、違えばその理由
fn check_call(call: &str, by_name: &IndexMap<String, &Value>) -> String {
    let c = call.trim();
    let (name, arg) = match c.split_once(' ') {
        Some((a, b)) => (a, b),
        None => (c, ""),
    };
    let name = name.trim_matches('`');
    let t = match by_name.get(name) {
        None => return "定義にないツール".into(),
        Some(t) => t,
    };
    let sch = &t["inputSchema"];
    let a = arg.trim().trim_matches('`');
    let given: Value = match serde_json::from_str(if a.is_empty() { "{}" } else { a }) {
        Ok(v) => v,
        Err(_) => return "入力が JSON ではありません".into(),
    };
    let props: IndexSet<String> = sch["properties"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    let reqd: IndexSet<String> = sch["required"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let keys: IndexSet<String> = match &given {
        Value::Object(m) => m.keys().cloned().collect(),
        _ => IndexSet::new(),
    };
    let mut extra: Vec<&String> = keys.iter().filter(|k| !props.contains(*k)).collect();
    if !extra.is_empty() {
        extra.sort();
        return format!("定義にない入力：{}", extra.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("、"));
    }
    let mut lack: Vec<&String> = reqd.iter().filter(|k| !keys.contains(*k)).collect();
    if !lack.is_empty() {
        lack.sort();
        return format!("足りない入力：{}", lack.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("、"));
    }
    String::new()
}

fn others_view(plan_md: &str, tools: &[Value]) -> String {
    let (intro, scenes) = plan_others(plan_md);
    let by_name: IndexMap<String, &Value> = tools.iter().map(|t| (sj(&t["name"]), t)).collect();
    let mut out: Vec<String> = vec![];
    if !intro.trim().is_empty() {
        out.push(format!("<div class=\"intro\">{}</div>", md_to_html(&intro)));
    }
    for (title, rws) in scenes {
        let mut bs = vec![];
        for r in &rws {
            let g = |k: &str| r.get(k).cloned().unwrap_or_default();
            let (who, say, call) = (g("話し手"), g("発言"), g("呼ぶツール"));
            let cls = match who.as_str() {
                "人" => "me",
                "AI" => "ai",
                _ => "tool",
            };
            let mut tool = String::new();
            if !call.is_empty() {
                let problem = check_call(&call, &by_name);
                tool = format!("<div class=\"call{}\">{}{}</div>", if problem.is_empty() { "" } else { " bad" }, if problem.is_empty() { "ツール：".to_string() } else { format!("⚠ {}：", escape(&problem)) }, inline(&call));
            }
            bs.push(format!("<div class=\"bub {}\"><span class=\"who\">{}</span><div class=\"say\">{}</div>{}</div>", cls, escape(&who), inline(&say), tool));
        }
        out.push(format!("<section><h2>{}</h2><div class=\"chat\">{}</div></section>", escape(&title), bs.concat()));
    }
    out.concat()
}

fn sample_value(ck: &Checker, t: &Type, i: i64, label: &str) -> String {
    let head2: String = label.chars().take(2).collect();
    match t.kind.as_str() {
        "ID" => format!("{}-{}", head2, 1000 + i),
        "文字列" => format!("{}の例{}", label, i),
        "整数" | "割合" => (i * 10).to_string(),
        "金額" => {
            let s = (i * 1200).to_string();
            let mut o = String::new();
            for (k, c) in s.chars().enumerate() {
                if k > 0 && (s.len() - k) % 3 == 0 {
                    o.push(',');
                }
                o.push(c);
            }
            o
        }
        "名前" => match t.target.as_ref().and_then(|x| ck.states.get(x)) {
            Some(st) if !st.initial.is_empty() => ck.p.show_names(&st.initial),
            _ => t.target.clone().unwrap_or_default(),
        },
        "参照" => format!("{}-{}", t.target.clone().unwrap_or_default().chars().take(2).collect::<String>(), 1000 + i),
        _ => "…".into(),
    }
}

/// 問い合わせの結果の見本。実際の値ではなく、列の型から作った例
fn query_sample(ck: &Checker, q: &Query, n: i64) -> Map<String, Value> {
    let show = |s: &str| ck.p.show_names(s);
    let dt = ck.data.get(&q.target).copied();
    let cols: Vec<Column> = if !q.select.is_empty() {
        q.select.clone()
    } else {
        dt.map(|d| d.fields.iter().take(1).map(|f| Column { name: f.name.clone(), kind: "項目".into(), field: f.name.clone(), via: None }).collect()).unwrap_or_default()
    };
    let mut types: Vec<Type> = vec![];
    for c in &cols {
        let mut ty: Option<Type> = None;
        if let Some(dt) = dt {
            if c.kind == "項目" {
                ty = dt.fields.iter().find(|x| x.name == c.field).map(|f| f.r#type.clone());
            } else if c.kind == "参照先" {
                let via = dt.fields.iter().find(|x| Some(&x.name) == c.via.as_ref());
                let tgt = via.filter(|v| v.r#type.kind == "参照").and_then(|v| v.r#type.target.as_ref()).and_then(|t| ck.data.get(t).copied());
                ty = tgt.and_then(|t| t.fields.iter().find(|x| x.name == c.field)).map(|f| f.r#type.clone());
            }
        }
        types.push(ty.unwrap_or_else(|| Type::new("整数")));
    }
    let mut m = Map::new();
    m.insert("cols".into(), json!(cols.iter().map(|c| show(&c.name)).collect::<Vec<_>>()));
    m.insert("rows".into(), json!((0..n).map(|i| cols.iter().zip(types.iter()).map(|(c, t)| sample_value(ck, t, i + 1, &show(&c.name))).collect::<Vec<_>>()).collect::<Vec<_>>()));
    m
}

fn screens_json(ck: &Checker) -> Map<String, Value> {
    let p = ck.p;
    let show = |s: &str| p.show_names(s);
    let mut out = Map::new();
    for sc in &p.screens {
        let mut acts = vec![];
        for (dn, routes) in &sc.actions {
            let proc = match ck.procs.get(dn) {
                Some(p) => *p,
                None => continue,
            };
            acts.push(json!({
                "title": show(dn),
                "inputs": proc.inputs.iter().map(|(n, _)| show(n)).collect::<Vec<_>>(),
                "routes": routes.iter().map(|(v, d)| json!({"result": show(v), "to": match d.as_str() {"stay" => "そのまま".to_string(), "back" => "戻る".to_string(), _ => show(d)}, "kind": if d == "stay" || d == "back" { d.clone() } else { "screen".to_string() }})).collect::<Vec<_>>(),
            }));
        }
        let dg = |k: &str, d: &str| sc.design.get(k).cloned().unwrap_or_else(|| d.to_string());
        let mut shows = vec![];
        for q in &sc.shows {
            if let Some(qd) = ck.queries.get(q) {
                let mut m = Map::new();
                m.insert("title".into(), json!(show(q)));
                for (k, v) in query_sample(ck, qd, 3) {
                    m.insert(k, v);
                }
                shows.push(Value::Object(m));
            }
        }
        out.insert(
            show(&sc.name),
            json!({
                "design": {"layout": dg("layout", "stack"), "accent": dg("accent", "#0b5cad"), "mode": dg("mode", "light")},
                "shows": shows,
                "fields": sc.fields.iter().map(|f| show(f)).collect::<Vec<_>>(),
                "params": sc.params.iter().map(|(n, _)| show(n)).collect::<Vec<_>>(),
                "opens": sc.opens.iter().map(|o| show(o)).collect::<Vec<_>>(),
                "actions": acts,
            }),
        );
    }
    out
}

fn fig(d: &str) -> String {
    format!("<div class=\"scroll figure\">{}</div>", d)
}

pub fn lead(s: &str) -> String {
    format!("<p class=\"lead\">{}</p>", s)
}

/// 外部作用の「出す内容」から、項目名だけを取り出す
fn effect_fields(content: &str) -> String {
    split_top(content, ',').iter().map(|x| x.split(':').next().unwrap_or("").trim().to_string()).collect::<Vec<_>>().join("・")
}

/// ユースケースの手順の処理から、フローと待ちをたどって、触れる外部システムと依頼の名前を集める
fn usecase_externals(ck: &Checker, spec: &Spec, ucs: &IndexMap<String, Vec<Row>>) -> IndexMap<String, IndexMap<String, Vec<String>>> {
    let show = |s: &str| ck.p.show_names(s);
    let procs: IndexMap<String, &Value> = rows(spec, "処理").iter().map(|x| (sj(&x["処理"]), x)).collect();
    let mut nxt: IndexMap<String, Vec<String>> = IndexMap::new();
    for fl in &ck.p.flows {
        for (proc, routes) in &fl.entries {
            for ds in route_map(routes).values() {
                for d in ds {
                    if d != "reply" {
                        nxt.entry(show(proc)).or_default().push(if let Some(r) = d.strip_prefix("wait ") { format!("待つ {}", show(r)) } else { show(d) });
                    }
                }
            }
        }
    }
    for t in rows(spec, "トリガ") {
        nxt.entry(format!("待つ {}", cs(t, 0))).or_default().push(cs(t, 2));
    }
    let reach = |start: &str| -> Vec<String> {
        let (mut seen, mut todo): (Vec<String>, Vec<String>) = (vec![], vec![start.to_string()]);
        while let Some(cur) = todo.pop() {
            if !seen.contains(&cur) {
                seen.push(cur.clone());
                if let Some(n) = nxt.get(&cur) {
                    todo.extend(n.iter().cloned());
                }
            }
        }
        seen
    };
    let mut out = IndexMap::new();
    for (uc, rws) in ucs {
        let mut systems: IndexMap<String, Vec<String>> = IndexMap::new();
        for r in rws {
            for q in reach(r.get("操作").map(|s| s.as_str()).unwrap_or("")) {
                if let Some(pr) = procs.get(&q) {
                    for ext in jarr(&pr["外部"]) {
                        let head = ext.split('(').next().unwrap_or("").to_string();
                        let (system, op) = match head.split_once('.') {
                            Some((a, b)) => (a.to_string(), b.to_string()),
                            None => (head.clone(), String::new()),
                        };
                        let e = systems.entry(system).or_default();
                        if !e.contains(&op) {
                            e.push(op);
                        }
                    }
                }
            }
        }
        out.insert(uc.clone(), systems);
    }
    out
}

fn usecase_diagram(ck: &Checker, ucs: &IndexMap<String, Vec<Row>>, spec: &Spec) -> String {
    let (mut actors, mut cases): (Vec<String>, Vec<(String, String, String)>) = (vec![], vec![]);
    let given = ucs.values().any(|rws| rws[0].get("アクター").map_or(false, |a| !a.is_empty()));
    for (name, rws) in ucs {
        let actor = match rws[0].get("アクター").filter(|a| !a.is_empty()) {
            Some(a) => a.clone(),
            None => format!("{}から始める人", rws[0].get("画面").cloned().unwrap_or_default()),
        };
        if !actors.contains(&actor) {
            actors.push(actor.clone());
        }
        cases.push((actor, name.clone(), svg::SAME.to_string()));
    }
    let ext = usecase_externals(ck, spec, ucs);
    let mut sysmap: IndexMap<String, (Vec<String>, Vec<String>)> = IndexMap::new();
    for (uc, systems) in &ext {
        for (system, ops) in systems {
            let e = sysmap.entry(system.clone()).or_default();
            e.1.push(uc.clone());
            for o in ops {
                if !e.0.contains(o) {
                    e.0.push(o.clone());
                }
            }
        }
    }
    let externals: Vec<(String, String, Vec<String>)> = sysmap.into_iter().map(|(s, (ops, users))| (s, ops.join("・"), users)).collect();
    let trows: Vec<Vec<String>> = cases
        .iter()
        .map(|(a, c, _)| {
            let screens: IndexSet<String> = ucs[c].iter().map(|r| r.get("画面").cloned().unwrap_or_default()).collect();
            let e = ext[c].keys().cloned().collect::<Vec<_>>();
            vec![a.clone(), c.clone(), ucs[c].len().to_string(), screens.into_iter().collect::<Vec<_>>().join("、"), if e.is_empty() { "なし".to_string() } else { e.join("、") }]
        })
        .collect();
    lead(&format!(
        "人と、その人が行うユースケースです。右の四角は、そのユースケースが処理を通じて触れる外部システムです。{}実際の画面での動きは「動作」のタブで確かめます。",
        if given { "" } else { "計画書にアクターの欄が無いので、ユースケースを始める画面ごとに分けています（ユースケースの表に「アクター」列を足すと、その名前で描きます）。" }
    )) + &fig(&svg::usecases(&actors, &cases, "ユースケース図", &externals))
        + &table(&[if given { "アクター" } else { "始める画面" }, "ユースケース", "手順数", "使う画面", "触れる外部システム"], &trows)
}

fn screen_flow(spec: &Spec) -> String {
    let screens = rows(spec, "画面");
    let mut nodes: Vec<String> = screens.iter().map(|x| cs(x, 0)).collect();
    let mut edges: Vec<svg::Edge> = vec![];
    for r in rows(spec, "画面遷移") {
        let (scr, op, res, to) = (cs(r, 0), cs(r, 1), cs(r, 2), cs(r, 3));
        if nodes.contains(&to) && to != scr {
            edges.push(svg::Edge::new(&scr, &if res.is_empty() { op.clone() } else { format!("{}：{}", op, res) }, &to));
        }
    }
    let entries: Vec<String> = screens.iter().enumerate().filter(|(i, x)| *i > 0 && !cs(x, 4).is_empty()).map(|(_, x)| cs(x, 0)).collect();
    let mut outside: Vec<String> = vec![];
    if !entries.is_empty() {
        outside = vec!["外から直接開く".to_string()];
        nodes.push(outside[0].clone());
        for e in &entries {
            edges.push(svg::Edge::new(&outside[0], "入口", e));
        }
    }
    let diagram = svg::flow(&nodes, &edges, &nodes[0], &[], "画面の流れ", &outside);
    lead(&format!("画面と、画面を移る操作です。左端の矢印が最初の画面です。同じ画面にとどまる操作は図に描かず、下の「遷移の表」に載せています。{}", if outside.is_empty() { "" } else { "点線の箱は、別の入口から画面を直接開くことを表します。" })) + &fig(&diagram)
}

fn state_machine(spec: &Spec, dname: &str) -> String {
    let states: Vec<(String, String)> = rows(spec, "状態").iter().filter(|r| cs(r, 0) == dname).map(|r| (cs(r, 1), cs(r, 2))).collect();
    let names: Vec<String> = states.iter().map(|(s, _)| s.clone()).collect();
    let trans: Vec<(String, String, String)> = rows(spec, "遷移").iter().filter(|r| cs(r, 0) == dname).map(|r| (cs(r, 1), cs(r, 2), cs(r, 3))).collect();
    let edges: Vec<svg::Edge> = trans.iter().filter(|(a, _, _)| names.contains(a)).map(|(a, b, p)| svg::Edge::new(a, p, b)).collect();
    let start = states.iter().find(|(_, k)| k == "初期").map(|(s, _)| s.clone()).unwrap_or_default();
    let finals: Vec<String> = states.iter().filter(|(_, k)| k == "終了").map(|(s, _)| s.clone()).collect();
    let diagram = svg::flow(&names, &edges, &start, &finals, &format!("状態の移り変わり {}", dname), &[]);
    let rows_of: IndexSet<String> = trans.iter().map(|(a, _, _)| a.clone()).collect();
    let cols: Vec<&String> = names.iter().filter(|s| trans.iter().any(|(_, b, _)| b == *s)).collect();
    let mut cell: IndexMap<(String, String), Vec<String>> = IndexMap::new();
    for (a, b, p) in &trans {
        cell.entry((a.clone(), b.clone())).or_default().push(p.clone());
    }
    let matrix = format!(
        "<div class=\"scroll\"><table class=\"grid matrix\"><thead><tr><th>元＼先</th>{}</tr></thead><tbody>{}</tbody></table></div>",
        cols.iter().map(|c| format!("<th>{}</th>", escape(c))).collect::<String>(),
        rows_of
            .iter()
            .map(|r| format!("<tr><th scope=\"row\">{}</th>{}</tr>", escape(r), cols.iter().map(|c| format!("<td>{}</td>", escape(&cell.get(&(r.clone(), (*c).clone())).map(|v| v.join(" ・ ")).unwrap_or_default()))).collect::<String>()))
            .collect::<String>()
    );
    let rejects: Vec<Vec<String>> = rows(spec, "拒否").iter().filter(|r| cs(r, 0) == dname).map(|r| vec![cs(r, 1), cs(r, 2), cs(r, 3)]).collect();
    lead(&format!("{}の状態と、状態を移す処理です。左端の矢印が最初の状態、二重枠が終わりの状態です。", escape(dname))) + &fig(&diagram) + &matrix + &if rejects.is_empty() { String::new() } else { format!("<h3>拒否</h3>{}", table(&["元", "先", "理由"], &rejects)) }
}

fn flow_edges(ck: &Checker, fl: &Flow, trig_rows: &[Value]) -> (Vec<String>, Vec<svg::Edge>, String, Vec<(String, String)>) {
    let show = |s: &str| ck.p.show_names(s);
    let mut nodes: Vec<String> = vec![];
    let mut edges: Vec<svg::Edge> = vec![];
    let mut replies: Vec<(String, String)> = vec![];
    fn node(nodes: &mut Vec<String>, n: String) -> String {
        if !nodes.contains(&n) {
            nodes.push(n.clone());
        }
        n
    }
    for (proc, routes) in &fl.entries {
        let src = node(&mut nodes, show(proc));
        for (v, ds) in route_map(routes) {
            for d in &ds {
                if d == "reply" {
                    replies.push((show(proc), show(&v)));
                } else {
                    let dst = node(&mut nodes, if let Some(r) = d.strip_prefix("wait ") { format!("待つ {}", show(r)) } else { show(d) });
                    edges.push(svg::Edge::new(&src, &format!("{}{}", show(&v), if ds.len() > 1 { "（並列）" } else { "" }), &dst));
                }
            }
        }
    }
    for t in trig_rows {
        let (name, why, to) = (cs(t, 0), cs(t, 1), cs(t, 2));
        if nodes.contains(&format!("待つ {}", name)) && nodes.contains(&to) {
            edges.push(svg::Edge::new(&format!("待つ {}", name), why.split_whitespace().next().unwrap_or("トリガ"), &to));
        }
    }
    let start = show(&fl.entries[0].0);
    (nodes, edges, start, replies)
}

fn process_flow(ck: &Checker, spec: &Spec, fl: &Flow) -> String {
    let (nodes, edges, start, replies) = flow_edges(ck, fl, rows(spec, "トリガ"));
    let diagram = svg::flow(&nodes, &edges, &start, &[], &format!("処理の流れ {}", fl.name), &[]);
    let mut grouped: IndexMap<String, Vec<String>> = IndexMap::new();
    for (proc, v) in replies {
        grouped.entry(proc).or_default().push(v);
    }
    let grows: Vec<Vec<String>> = grouped.iter().map(|(k, v)| vec![k.clone(), v.join("・")]).collect();
    lead("処理から処理へのつながりです。矢印の文字は処理の結果の種類で、「並列」は同じ結果から複数の処理を同時に起こします。「待つ」は、返事や期限を待つ点です。応答（呼んだ側へ結果を返して終わる）になる結果は、図に描かず下の表にまとめています。")
        + &fig(&diagram)
        + "<h3>応答で終わる結果</h3>"
        + &table(&["処理", "結果の種類"], &grows)
        + &sequence_of(ck, spec, fl)
}

/// このフローの処理が外へ出す依頼と、返ってくる返事を、時間の順に並べる
fn sequence_of(ck: &Checker, spec: &Spec, fl: &Flow) -> String {
    let me = "このシステム";
    let contents: IndexMap<String, String> = rows(spec, "外部作用").iter().map(|r| (cs(r, 0), cs(r, 1))).collect();
    let mut reply_of: IndexMap<String, String> = IndexMap::new();
    for r in rows(spec, "模擬") {
        reply_of.entry(cs(r, 0)).or_insert(cs(r, 1));
    }
    let join: IndexMap<String, String> = rows(spec, "合流").iter().map(|r| (cs(r, 0), cs(r, 1))).collect();
    let procs: IndexMap<String, &Value> = rows(spec, "処理").iter().map(|x| (sj(&x["処理"]), x)).collect();
    let mut parts: Vec<String> = vec![me.to_string()];
    let mut msgs: Vec<svg::Message> = vec![];
    for (proc, _) in &fl.entries {
        let name = ck.p.show_names(proc);
        let exts: Vec<String> = procs.get(&name).map(|x| jarr(&x["外部"])).unwrap_or_default();
        if msgs.iter().any(|m| m.label == name) || exts.is_empty() {
            continue;
        }
        msgs.push(svg::Message::new(me, me, &name));
        for ext in &exts {
            let eff = ext.split('(').next().unwrap_or("").to_string();
            let (system, op) = match eff.split_once('.') {
                Some((a, b)) => (a.to_string(), b.to_string()),
                None => (eff.clone(), String::new()),
            };
            if !parts.contains(&system) {
                parts.push(system.clone());
            }
            let fields = contents.get(&eff).map(|c| effect_fields(c)).unwrap_or_default();
            msgs.push(svg::Message::new(me, &system, &if fields.is_empty() { op.clone() } else { format!("{}（{}）", op, fields) }));
            let back = format!("{}{}", reply_of.get(&eff).cloned().unwrap_or_else(|| "結果".into()), join.get(&eff).map_or(String::new(), |j| format!(" → {}で待つ", j)));
            msgs.push(svg::Message::dashed(&system, me, &back));
        }
    }
    if msgs.is_empty() {
        return String::new();
    }
    format!("<h3>外部とのやりとりの順序</h3>{}{}", lead("上から順に進みます。実線は外への依頼、破線は返事です。"), fig(&svg::sequence(&parts, &msgs, &format!("外部とのやりとり {}", fl.name))))
}

fn system_context(spec: &Spec) -> String {
    let mut by_system: IndexMap<String, Vec<String>> = IndexMap::new();
    for r in rows(spec, "外部作用") {
        let (name, content) = (cs(r, 0), cs(r, 1));
        let (system, op) = match name.split_once('.') {
            Some((a, b)) => (a.to_string(), b.to_string()),
            None => (name.clone(), String::new()),
        };
        let f = effect_fields(&content);
        by_system.entry(system).or_default().push(if f.is_empty() { op } else { format!("{}（{}）", op, f) });
    }
    let systems: Vec<(String, String, Vec<String>, String)> = by_system.into_iter().map(|(s, lines)| (s, "外部システム".to_string(), lines, svg::SAME.to_string())).collect();
    let mut o = lead("このシステムが外へ依頼する相手と、依頼の中身です。") + &fig(&svg::context("このシステム", &systems, "システムのつながり"));
    o += &format!("<h3>外へ出す内容</h3>{}", table(&["外部作用", "出す内容"], &srows(rows(spec, "外部作用"))));
    if has(spec, "通信") {
        o += &format!("<h3>外へ出る通信（実通信。ここに書いた先にしか出ません）</h3>{}", table(&["外部作用", "通信先", "認証（シークレット）", "本文の項目", "返事の状態 → 結果の種類"], &srows(rows(spec, "通信"))));
    }
    if has(spec, "合流") {
        o += &format!("<h3>返事の合流先</h3>{}", table(&["外部作用", "合流先のトリガ"], &srows(rows(spec, "合流"))));
    }
    if has(spec, "応答") {
        o += &format!("<h3>外から返ってくる内容</h3>{}", table(&["応答", "中身"], &srows(rows(spec, "応答"))));
    }
    if has(spec, "模擬") {
        o += &format!("<h3>開発用の模擬（返事の結果の種類。先頭が平常）</h3>{}", table(&["外部作用", "返ってくる応答", "結果の種類（返事に入る中身）"], &srows(rows(spec, "模擬"))));
    }
    o
}

pub fn card(cls: &str, label: &str, where_: &str, msg: &str) -> String {
    format!("<article class=\"card {cls}\"><div class=\"chips\"><span class=\"chip {cls}\">{label}</span></div><p class=\"ctx\">{}</p><p class=\"q\">{}</p></article>", escape(where_), escape(msg), cls = cls, label = label)
}

const ROUTE_HEAD: [&str; 6] = ["メソッド", "経路", "動作", "呼ぶもの", "結果→状態", "外から"];

fn route_table(spec: &Spec) -> String {
    let rs = srows(rows(spec, "ルート"));
    if has(spec, "役割") {
        let head: Vec<&str> = ROUTE_HEAD.iter().copied().chain(std::iter::once("呼べる人")).collect();
        table(&head, &rs)
    } else {
        let cut: Vec<Vec<String>> = rs.into_iter().map(|r| r.into_iter().take(6).collect()).collect();
        table(&ROUTE_HEAD, &cut)
    }
}

const CHANGE_KEYS: [(&str, &str); 32] = [
    ("アプリ", "アプリ"),
    ("拡張", "ブラウザ拡張"),
    ("データ", "データ"),
    ("状態", "状態"),
    ("遷移", "遷移"),
    ("拒否", "拒否"),
    ("トリガ", "トリガ"),
    ("外部作用", "外部作用"),
    ("通信", "通信"),
    ("常駐", "常駐"),
    ("データ用フォルダ", "データ用フォルダ"),
    ("ファイル", "ファイル"),
    ("流れ", "流れ（処理を書いた順）"),
    ("合流", "合流"),
    ("応答", "応答"),
    ("模擬", "模擬"),
    ("決め方", "slotの決め方"),
    ("出来事", "出来事"),
    ("削除", "削除"),
    ("保存の形", "保存の形の変更"),
    ("保存先", "保存先"),
    ("動かす前提", "動かす前提"),
    ("コンテナと基盤", "コンテナと基盤"),
    ("人の保証", "人の保証"),
    ("保証の強さ", "保証の強さ"),
    ("ルート", "ルート"),
    ("役割", "役割（呼べる人）"),
    ("POSTの問い合わせ", "POST で受ける問い合わせ"),
    ("範囲", "範囲"),
    ("画面", "画面"),
    ("画面遷移", "画面遷移"),
    ("画面デザイン", "画面デザイン"),
];

fn py_repr(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut o = String::from(q);
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            c if c == q => {
                o.push('\\');
                o.push(c);
            }
            c => o.push(c),
        }
    }
    o.push(q);
    o
}

/// Python の str(x)（文字列の組・並びの表示）
fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => format!("({})", a.iter().map(py_item).collect::<Vec<_>>().join(", ")),
        o => o.to_string(),
    }
}
fn py_item(v: &Value) -> String {
    match v {
        Value::String(s) => py_repr(s),
        Value::Array(a) => {
            if a.len() == 1 {
                format!("({},)", py_item(&a[0]))
            } else {
                format!("({})", a.iter().map(py_item).collect::<Vec<_>>().join(", "))
            }
        }
        Value::Null => "None".into(),
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        o => o.to_string(),
    }
}

pub fn row_text(x: &Value) -> String {
    match x {
        Value::Object(m) => {
            let keys = ["処理", "問い合わせ", "入力", "読む", "書く", "結果", "手順", "参照", "外部", "条件", "列", "並び", "ページ"];
            m.iter()
                .filter(|(k, _)| keys.contains(&k.as_str()))
                .map(|(k, v)| format!("{}：{}", k, if let Value::Array(a) = v { a.iter().map(py_str).collect::<Vec<_>>().join("・") } else { py_str(v) }))
                .collect::<Vec<_>>()
                .join("　")
        }
        Value::Array(a) => a.iter().map(py_str).collect::<Vec<_>>().join("　／　"),
        o => py_str(o),
    }
}

/// 前の版との差。行の追加（＋）と削除（−）を、表の種類ごとに並べる
pub fn changes_section(new: &Spec, old: &Spec, label: &str) -> String {
    let mut groups: Vec<String> = vec![];
    let mut keys: Vec<(&str, &str)> = CHANGE_KEYS.to_vec();
    keys.push(("処理", "処理"));
    keys.push(("問い合わせ", "問い合わせ"));
    for (key, title) in keys {
        let a: Vec<String> = rows(old, key).iter().map(row_text).collect();
        let b: Vec<String> = rows(new, key).iter().map(row_text).collect();
        let (mut left, mut added) = (a.clone(), vec![]);
        for x in &b {
            if let Some(i) = left.iter().position(|y| y == x) {
                left.remove(i);
            } else {
                added.push(x.clone());
            }
        }
        let removed = left;
        let moved: Vec<String> = if !added.is_empty() || !removed.is_empty() || a == b { vec![] } else { b.iter().enumerate().filter(|(i, x)| &a[*i] != *x).map(|(i, x)| format!("{} 番目：{}", i + 1, x)).collect() };
        if added.is_empty() && removed.is_empty() && moved.is_empty() {
            continue;
        }
        let mut items: String = removed.iter().map(|x| format!("<li class=\"removed\"><span class=\"sign\">−</span>{}</li>", escape(x))).collect::<String>() + &added.iter().map(|x| format!("<li class=\"added\"><span class=\"sign\">＋</span>{}</li>", escape(x))).collect::<String>();
        if !moved.is_empty() {
            items += "<li class=\"added\"><span class=\"sign\">⇅</span>並びが変わりました。新しい並び（変わった所だけ）</li>";
            items += &moved.iter().map(|x| format!("<li class=\"added\"><span class=\"sign\">⇅</span>{}</li>", escape(x))).collect::<String>();
        }
        groups.push(format!("<div class=\"cgroup\"><h4>{}</h4><ul class=\"clist\">{}</ul></div>", escape(title), items));
    }
    let lbl = if label.is_empty() { "前の版" } else { label };
    if groups.is_empty() {
        return lead(&format!("{}から、仕様書に出る内容の変更はありません。", escape(lbl)));
    }
    lead(&format!("{}と比べて、書き換わった箇所です。意図した変更かどうかを確かめてください。", escape(lbl))) + &format!("<div class=\"cgroups\">{}</div>", groups.concat())
}

/// 食い違い・検査のエラーは「拒否」、検査の問い・警告は「確認」、未規定は「記録」
pub fn collect_questions(ck: &Checker, findings: &[Finding]) -> (Vec<String>, Vec<String>, Vec<String>) {
    let (mut rejects, mut asks, mut records) = (vec![], vec![], vec![]);
    for f in findings {
        let c = card(if f.kind != UNSPEC { "reject" } else { "ask" }, &escape(&f.kind), &f.where_, &f.msg);
        if f.kind == UNSPEC {
            records.push(c);
        } else {
            rejects.push(c);
        }
    }
    for dg in &ck.diags {
        if dg.level == "エラー" {
            rejects.push(card("reject", "拒否", &dg.where_, &dg.msg));
        } else if dg.level == QUESTION || dg.level == WARN {
            asks.push(card("ask", "確認", &dg.where_, &dg.msg));
        }
    }
    (rejects, asks, records)
}

fn page(title: &str, h1: &str, head_extra: &str, body: &str, scripts: &str, fragment: bool) -> String {
    let head = format!(
        "<title>{}</title>\n<meta name=\"claude:viewport-fit-sides\" content=\"cover\">\n<link rel=\"preconnect\" href=\"https://fonts.googleapis.com\">\n<link rel=\"stylesheet\" href=\"https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500&family=IBM+Plex+Sans+JP:wght@400;500;600&family=Zen+Kaku+Gothic+New:wght@700&display=swap\">\n<style>{}</style>\n",
        escape(title),
        CSS
    );
    let content = format!("<div class=\"page\"><header class=\"top\"><h1>{}</h1>{}</header>{}</div>{}", escape(h1), head_extra, body, scripts);
    if fragment {
        return head + &content;
    }
    format!("<!doctype html>\n<html lang=\"{}\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1, viewport-fit=cover\">\n{}</head><body>{}</body></html>\n", if crate::i18n::lang() == crate::i18n::Lang::En { "en" } else { "ja" }, head, content)
}

fn strip_parens(s: &str) -> &str {
    s.trim_matches(|c| c == '（' || c == '）')
}

/// 仕様書とは別の、答えてほしいことと前の版からの変更だけの HTML
pub fn review_changes(ck: &Checker, findings: &[Finding], name: &str, base_ck: Option<&Checker>, base_label: &str, fragment: bool, receipt: Option<&str>) -> String {
    let (rejects, asks, records) = collect_questions(ck, findings);
    let (verdict, vcls) = if rejects.is_empty() { ("受け入れ可", "ok") } else { ("拒否", "reject") };
    let mut meta = vec![format!("<span class=\"chip {}\">{}</span>", vcls, verdict), format!("<span>拒否 {} 件 ・ 確認 {} 件 ・ 記録 {} 件</span>", rejects.len(), asks.len(), records.len())];
    if base_ck.is_some() {
        meta.push(format!("<span>比較元：{}</span>", escape(if base_label.is_empty() { "前の版" } else { base_label })));
    }
    let mut parts: Vec<String> = vec![];
    if !rejects.is_empty() || !asks.is_empty() {
        parts.push(format!(
            "<section id=\"questions\"><h2>答えてほしいこと</h2>{}<div class=\"cards\">{}</div></section>",
            lead(&format!("拒否 {} 件、確認 {} 件です。答えるとコードか計画書が直り、もう一度検査されます。", rejects.len(), asks.len())),
            rejects.iter().chain(asks.iter()).cloned().collect::<String>()
        ));
    }
    if let Some(r) = receipt {
        parts.push(format!("<section id=\"receipt\"><h2>信頼のレシート</h2>{}{}</section>", lead("前の版と比べた、信頼に関わる変化だけです。仕様書の変更は、この下の「前の版からの変更」にあります。"), r));
    }
    if let Some(b) = base_ck {
        parts.push(format!("<section id=\"changes\"><h2>前の版からの変更</h2>{}</section>", changes_section(&build(ck), &build(b), base_label)));
    }
    if !records.is_empty() {
        parts.push(format!(
            "<section id=\"open\"><h2>意図的に決めていないこと</h2>{}<div class=\"cards\">{}</div></section>",
            lead("計画書で「未規定」としたところです。実装が自由に選んでよい箇所で、書き忘れとは区別して記録しています。"),
            records.concat()
        ));
    }
    if parts.is_empty() {
        parts.push(lead("答えてほしいことはありません。前の版も渡されていないので、変更点もありません。"));
    }
    page(format!("確認事項と変更点 {}", name).trim(), format!("確認事項と変更点 {}", strip_parens(name)).trim(), &format!("<div class=\"meta\">{}</div>", meta.concat()), &parts.concat(), "", fragment)
}

fn fix_script(v: &Value) -> String {
    pydumps(v).replace("</", "<\\/")
}

fn svg_steps_tree(tree: &[Value]) -> Vec<svg::Step> {
    tree.iter()
        .map(|n| {
            let a = n.as_array().unwrap();
            let kind = a[0].as_str().unwrap_or("");
            let s = |i: usize| a[i].as_str().unwrap_or("").to_string();
            match kind {
                "結果" => svg::Step::Result(s(1)),
                "する" => svg::Step::Do(s(1)),
                "無ければ" => svg::Step::Absent(s(1), s(2)),
                "各" => svg::Step::Each(s(1), svg_steps_tree(a[2].as_array().unwrap())),
                _ => svg::Step::If(s(1), svg_steps_tree(a[2].as_array().unwrap()), if a[3].is_null() { None } else { Some(svg_steps_tree(a[3].as_array().unwrap())) }),
            }
        })
        .collect()
}

/// 人が読む仕様書
pub fn review(ck: &Checker, plan_md: Option<&str>, findings: &[Finding], name: &str, fragment: bool) -> Result<String, String> {
    let p = ck.p;
    let show = |s: &str| p.show_names(s);
    let plan: Plan = plan_md.map(read_plan).unwrap_or_default();
    let ucs: IndexMap<String, Vec<Row>> = if plan_md.is_some() { usecases(plan.get("ユースケース").map(|v| v.as_slice()).unwrap_or(&[])) } else { IndexMap::new() };
    let spec = build(ck);
    let mut sec: Vec<(String, String, String)> = vec![];
    let mut add = |aid: &str, title: Option<&str>, body: String| {
        let h2 = title.map_or(String::new(), |t| format!("<h2>{}</h2>", escape(t)));
        sec.push((aid.to_string(), title.unwrap_or(aid).to_string(), format!("<section id=\"{}\">{}{}</section>", aid, h2, body)));
    };
    let (_, _, records) = collect_questions(ck, findings);
    let (overview, usage) = plan_md.map(plan_sections).unwrap_or_default();
    if !overview.is_empty() {
        add("overview", Some("概要"), md_to_html(&overview));
    }
    if !usage.is_empty() {
        add("usage", Some("利用の形態"), md_to_html(&usage));
    }
    if !ucs.is_empty() {
        add("usecases", Some("誰が何をするか"), usecase_diagram(ck, &ucs, &spec));
    }
    // 画面
    if !p.screens.is_empty() {
        let sj_ = screens_json(ck);
        let scr_rows: Vec<Vec<String>> = srows(rows(&spec, "画面")).into_iter().map(|r| r.iter().map(|c| show(c)).collect()).collect();
        add(
            "screens",
            Some("画面の流れ"),
            screen_flow(&spec) + "<h3>画面の一覧</h3>" + &table(&["画面", "見せる", "入力欄（←選ぶ元）", "受け取る値", "入口", "選ばせる slot", "URL"], &scr_rows) + "<h3>遷移の表</h3>" + &table(&["画面", "操作", "結果", "行き先"], &srows(rows(&spec, "画面遷移"))),
        );
        let mut wfs: Vec<String> = vec![];
        for (n, s) in &sj_ {
            let d = &s["design"];
            let (layout, accent, mode) = (sj(&d["layout"]), sj(&d["accent"]), sj(&d["mode"]));
            let tiles = match layout.as_str() {
                "grid" => "<div class=\"g\"><div class=\"tile\"></div><div class=\"tile\"></div><div class=\"tile\"></div><div class=\"tile\"></div></div>",
                "list" | "form" => "<div class=\"bar\"></div><div class=\"bar\"></div><div class=\"bar\"></div>",
                "stack" => "<div class=\"tile\"></div><div class=\"bar\"></div>",
                _ => return Err(format!("KeyError: '{}'", layout)),
            };
            let btn = if s["actions"].as_array().map_or(false, |a| !a.is_empty()) { format!("<div class=\"btn\" style=\"background:{}\"></div>", accent) } else { String::new() };
            wfs.push(format!(
                "<div class=\"wf {}\"><header style=\"background:{}\">{}</header><div class=\"body\">{}{}</div><div class=\"sub\" style=\"padding:0 10px 8px\">配置 {}／色 {}／{}</div></div>",
                if mode == "dark" { "dark" } else { "" },
                escape(&accent),
                escape(n),
                tiles,
                btn,
                layout,
                escape(&accent),
                if mode == "dark" { "ダーク" } else { "ライト" }
            ));
        }
        add(
            "design",
            Some("画面デザイン（①との突合には含めない）"),
            lead("この指定は、実際の画面（geas が生成する汎用の画面）の配置・色・明暗にそのまま使われ、最初の画面の色はインストール時のアイコンとテーマ色にもなります。下の絵は、その配置の見取り図です。")
                + &format!("<div class=\"wfs\">{}</div>", wfs.concat())
                + &table(&["画面", "指定", "値"], &srows(rows(&spec, "画面デザイン"))),
        );
        if plan_md.is_some() && has(&spec, "ルート") {
            let (cov, got, total) = coverage(&spec, &plan);
            add("coverage", Some(&format!("内部 URL ごとの確認の網羅（{} / {}）", got, total)), lead("具体例とユースケースが、どの URL のどの結果（HTTP の状態）に当たっているか。「未」は、どちらでも確認していません。") + &table(&COVERAGE_HEAD, &cov));
        }
        let dest_of: IndexMap<(String, String, String), String> = rows(&spec, "画面遷移").iter().map(|x| ((cs(x, 0), cs(x, 1), cs(x, 2)), cs(x, 3))).collect();
        let url_of: IndexMap<String, String> = rows(&spec, "画面").iter().map(|x| (cs(x, 0), cs(x, x.as_array().unwrap().len() - 1))).collect();
        let mut uj: Vec<Value> = vec![];
        for (i, (k, v)) in ucs.iter().enumerate() {
            let (mut steps, mut here): (Vec<Value>, Option<String>) = (vec![], None);
            for r in v {
                let g = |k: &str| r.get(k).cloned().unwrap_or_default();
                let (scr, o, rs) = (g("画面"), g("操作"), g("結果"));
                let to = dest_of.get(&(scr.clone(), o.clone(), rs.clone())).cloned().unwrap_or_default();
                steps.push(json!({"screen": scr, "op": o, "result": rs, "to": to, "url": url_of.get(&scr).cloned().unwrap_or_default()}));
                here = Some(if o == "見る" || to == "そのまま" { scr } else { to });
            }
            let end = match here {
                Some(h) if !h.is_empty() => h,
                _ => v.last().and_then(|r| r.get("画面").cloned()).unwrap_or_default(),
            };
            uj.push(json!({"id": format!("UC{}", i + 1), "name": k, "steps": steps, "end": end}));
        }
        let net_events: Vec<&Value> = rows(&spec, "出来事").iter().filter(|x| cs(x, 0).contains("つながり")).collect();
        let ex: IndexMap<(String, String), &Row> = plan.get("画面例").map(|v| v.iter().map(|r| ((r.get("ユースケース").cloned().unwrap_or_default(), r.get("手順").cloned().unwrap_or_default()), r)).collect()).unwrap_or_default();
        let mut per_uc: Vec<String> = vec![];
        for (i, (k, v)) in ucs.iter().enumerate() {
            let rows_: Vec<Vec<String>> = v
                .iter()
                .map(|r| {
                    let g = |k: &str| r.get(k).cloned().unwrap_or_default();
                    let e = ex.get(&(k.clone(), g("手順")));
                    let eg = |kk: &str| e.and_then(|x| x.get(kk).cloned()).unwrap_or_default();
                    vec![g("手順"), g("画面"), format!("{}{}", g("操作"), if g("結果").is_empty() { String::new() } else { format!("（{}）", g("結果")) }), eg("入れる・選ぶ"), eg("見えること")]
                })
                .collect();
            per_uc.push(format!("<details><summary><b>UC{}</b> {}（{} 手順）</summary>{}</details>", i + 1, escape(k), v.len(), table(&["手順", "画面", "操作（結果）", "入れる・選ぶ", "見えること"], &rows_)));
        }
        let mut env = String::new();
        if has(&spec, "模擬") {
            env += &format!("<fieldset><legend>外部の返事の種類（開発用の模擬）</legend><div class=\"chips\">{}</div></fieldset>", rows(&spec, "模擬").iter().map(|r| format!("<span class=\"envchip\">{} → {}</span>", escape(&cs(r, 0)), escape(&cs(r, 2)))).collect::<String>());
        }
        if has(&spec, "出来事") {
            env += &format!("<fieldset><legend>外の出来事</legend><div class=\"chips\">{}</div></fieldset>", rows(&spec, "出来事").iter().map(|r| format!("<span class=\"envchip\">{}：{}</span>", escape(&cs(r, 0)), escape(&cs(r, 1)))).collect::<String>());
        }
        let timers: Vec<(String, String)> = rows(&spec, "トリガ").iter().filter(|r| cs(r, 1).contains("期限")).map(|r| (cs(r, 0), cs(r, 1))).collect();
        if !timers.is_empty() {
            env += &format!("<fieldset><legend>時間（期限）</legend><div class=\"chips\">{}</div></fieldset>", timers.iter().map(|(n_, w)| format!("<span class=\"envchip\">{}：{}</span>", escape(n_), escape(&w[w.find("期限").unwrap()..]))).collect::<String>());
        }
        let env_html = if env.is_empty() {
            String::new()
        } else {
            format!("<h3>この画面の外にある設定</h3>{}<div class=\"env\">{}</div>", lead("画面の操作では動かせない、環境の側の設定です（オンライン／オフラインは、操作パネルで切り替えられます）。返事の種類と期限は、動作の一覧の「返事を落とす」「時計を進める」の手順で確かめます。"), env)
        };
        let events_script = if net_events.is_empty() {
            String::new()
        } else {
            format!("<script type=\"application/json\" id=\"events\">{}</script>", fix_script(&Value::Array(net_events.iter().map(|x| json!({"name": cs(x, 0), "proc": cs(x, 2)})).collect())))
        };
        let walk = "<div class=\"sc\"><div id=\"mock\"></div></div>".to_string()
            + &env_html
            + "<h3>ユースケースの動作の一覧</h3>"
            + &per_uc.concat()
            + "<div id=\"ngwrap\" hidden><h3>NG にしたもの</h3><ul id=\"ngs\" class=\"plain\"></ul><button id=\"copy\" type=\"button\">コピー</button></div>"
            + &events_script
            + &format!("<script type=\"application/json\" id=\"ucs\">{}</script>", fix_script(&Value::Array(uj)))
            + &format!("<script type=\"application/json\" id=\"screens-data\">{}</script>", fix_script(&Value::Object(sj_.clone())))
            + "<div id=\"pad\" role=\"group\" aria-label=\"レビュー操作パネル\"><div class=\"l\"><span id=\"line\"></span><span id=\"prog\"></span></div><div class=\"r\">"
            + if net_events.is_empty() { "" } else { "<button id=\"bNet\" type=\"button\" aria-label=\"つながりの切り替え\">オンライン</button>" }
            + "<select id=\"opsel\" aria-label=\"確認するユースケース\"></select><button id=\"bReset\" type=\"button\">最初から</button><button id=\"bPrev\" type=\"button\">前へ</button><button id=\"bNext\" class=\"go\" type=\"button\">次へ</button><button id=\"bOk\" class=\"okb\" type=\"button\">OK</button><button id=\"bNg\" class=\"ngb\" type=\"button\">NG</button></div><div class=\"ng\" id=\"ngrow\" hidden><input id=\"why\" placeholder=\"NGの理由（空でもよい）\" aria-label=\"NGの理由\"><button id=\"ngsave\" type=\"button\">保存して次へ</button></div></div>";
        add("walk", None, walk);
    }
    // 動作：MCP
    let tools: Vec<Value> = if has(&spec, "ルート") { tool_defs(ck)? } else { vec![] };
    if !tools.is_empty() {
        add("mcp", None, mcp_section(&tools));
    }
    // 動作：API
    if has(&spec, "ルート") {
        let url_ex: &[Row] = plan.get("URL例").map(|v| v.as_slice()).unwrap_or(&[]);
        let body = if !url_ex.is_empty() {
            let rws: Vec<Vec<String>> = url_ex.iter().map(|r| ["ID", "メソッド", "経路", "入力", "期待する結果", "期待する状態"].iter().map(|k| r.get(*k).cloned().unwrap_or_default()).collect()).collect();
            table(&["ID", "メソッド", "経路", "入力", "期待する結果", "期待する状態"], &rws)
        } else {
            route_table(&spec)
        };
        add("api", None, lead("API は押して動かせません。計画書の URL の具体例を、リクエストと期待する応答の表にしています。") + &body);
    }
    // 決まりごと
    add("data", Some("データ"), table(&["データ", "項目", "型・範囲", "必須", "一意"], &srows(rows(&spec, "データ"))));
    let procs_rows: Vec<Vec<String>> = rows(&spec, "処理")
        .iter()
        .map(|x| {
            let ext = jarr(&x["外部"]).join(", ");
            vec![sj(&x["処理"]), jarr(&x["入力"]).join(", "), jarr(&x["読む"]).join(", "), jarr(&x["書く"]).join(", "), x["結果"].as_array().unwrap().iter().map(|r| format!("{}→{}", cs(r, 0), cs(r, 1))).collect::<Vec<_>>().join(" / "), if ext.is_empty() { "なし".into() } else { ext }]
        })
        .collect();
    let mut rules = format!("<h3>処理</h3>{}", table(&["処理", "入力", "読む", "書く", "結果 → 行き先", "外部作用"], &procs_rows));
    rules += &format!(
        "<h3>処理の中身</h3>{}",
        lead(&format!("上から下へ進みます。四角は「すること」、六角は「分かれ道」、角の丸い太枠は「結果」で、そこで終わりです。ここに出ない動きを、処理は持ちません。{}", STOPS_NOTE))
    );
    for x in rows(&spec, "処理") {
        let refs = x["参照"].as_array().unwrap();
        let tree = svg_steps_tree(x["図"].as_array().unwrap());
        rules += &(format!("<h4>{}</h4>", escape(&sj(&x["処理"])))
            + &if refs.is_empty() { String::new() } else { lead(&format!("外から決まる値（slot）：{}", refs.iter().map(|r| format!("{}: {}", cs(r, 0), cs(r, 1))).collect::<Vec<_>>().join("、"))) }
            + &fig(&svg::steps(&tree, &format!("処理の中身 {}", sj(&x["処理"])))));
    }
    if has(&spec, "問い合わせ") {
        let qrows: Vec<Vec<String>> = rows(&spec, "問い合わせ").iter().map(|x| vec![sj(&x["問い合わせ"]), jarr(&x["入力"]).join(", "), jarr(&x["読む"]).join(", "), jarr(&x["条件"]).join(", "), jarr(&x["列"]).join(", "), sj(&x["並び"]), sj(&x["ページ"])]).collect();
        rules += &format!("<h3>問い合わせ</h3>{}", table(&["問い合わせ", "入力", "読む", "条件", "返す列（この順）", "並び", "ページ"], &qrows));
    }
    if has(&spec, "決め方") {
        rules += &format!("<h3>slot の決め方（サーバーが決める値。上の行から順に、最初に当てはまった値）</h3>{}", table(&["slot", "条件", "値"], &srows(rows(&spec, "決め方"))));
    }
    let inv: Vec<Vec<String>> = ck.invariant_status.iter().map(|(t, rs)| vec![t.clone(), rs.iter().map(|(a, b)| format!("{}: {}", a, b)).collect::<Vec<_>>().join("／")]).collect();
    rules += &format!("<h3>いつでも成り立つこと（不変条件）</h3>{}", table(&["内容", "検査の結果"], &inv));
    add("rules", Some("決まりごと"), rules);
    if has(&spec, "保証の強さ") {
        let counts = level_counts(rows(&spec, "保証の強さ"));
        add(
            "levels",
            Some("保証の強さ"),
            lead("約束ごとに、何が守っているかです。上ほど強く、下の行ほど人が重点的に読む対象です。") + &lead(&format!("件数：{}", counts.iter().map(|(lv, n)| format!("{} {}", lv, n)).collect::<Vec<_>>().join("、"))) + &table(&["強さ", "対象", "内容"], &srows(rows(&spec, "保証の強さ"))),
        );
    }
    let mut seen: IndexSet<String> = IndexSet::new();
    for r in rows(&spec, "遷移") {
        seen.insert(cs(r, 0));
    }
    for dname in seen {
        add(&format!("states-{}", dname), Some(&format!("状態の移り変わり：{}", dname)), state_machine(&spec, &dname));
    }
    if has(&spec, "データ用フォルダ") {
        add("datafolder", Some("データ用フォルダ"), lead("DB・ログ・シークレットを置くフォルダです。OS ごとのパスの書き方で宣言します（相対パスは、exe のあるフォルダが起点）。環境変数が未設定のときや、書き込めないときは、起動時にエラーで止まります。") + &table(&["場合", "パス"], &srows(rows(&spec, "データ用フォルダ"))));
    }
    if has(&spec, "ファイル") {
        add("files", Some("ファイルを触る口"), lead("外部作用が実際にファイルに触ります。触れるのは、ここに書いた操作・場所・ファイルだけです。場所は、実行時に入力されたパス（OS の書式を確かめた値）の下の、書いた相対パスで、一時的に失敗しても、書いていない場所には触りません。書くのは新規作成だけか、置き換えを書いた物だけで、消せるのは、消すと書いた物だけです。") + &table(&["外部作用", "操作", "場所", "結果 → 結果の種類"], &srows(rows(&spec, "ファイル"))));
    }
    if has(&spec, "常駐") {
        add("resident", Some("常駐"), lead("窓を閉じても終了せず、タスクトレイ（macOS はメニューバー）のアイコンに居続けます。アイコンのクリックで窓を開閉し、メニューの項目は入口の画面を開きます。") + &table(&["項目", "内容"], &srows(rows(&spec, "常駐"))));
    }
    if has(&spec, "アプリ") {
        add("app", Some("アプリ"), table(&["項目", "内容"], &srows(rows(&spec, "アプリ"))));
    }
    if has(&spec, "拡張") {
        add("ext", Some("ブラウザ拡張"), lead("ページに出す画面と、ページから値を読む決まりです。読み方と対応表は、ここに書いたものだけです。") + &table(&["画面", "ページ", "見せる問い合わせ", "入力", "ページからの読み方"], &srows(rows(&spec, "拡張"))));
    }
    if has(&spec, "外部作用") {
        add("systems", Some("システムのつながり"), system_context(&spec));
    }
    for fl in p.flows.iter().filter(|f| !f.entries.is_empty()) {
        add(&format!("flow-{}", fl.name), Some(&format!("処理の流れ：{}", show(&fl.name))), process_flow(ck, &spec, fl));
    }
    let mut trig = table(&["トリガ", "要因", "行き先"], &srows(rows(&spec, "トリガ")));
    if has(&spec, "出来事") {
        trig += &format!("<h3>外の出来事</h3>{}", table(&["出来事", "起こし方", "処理"], &srows(rows(&spec, "出来事"))));
    }
    add("triggers", Some("待ちと期限（トリガ）"), trig);
    if has(&spec, "削除") {
        add("deletes", Some("削除"), lead("消してよい状態と、消したときの参照元の扱いです。「削除させない」と書いていないデータは、消されうるものとして扱います。") + &table(&["データ", "消してよい状態", "参照元の扱い"], &srows(rows(&spec, "削除"))));
    }
    if has(&spec, "保存の形") {
        add("migrations", Some("保存の形の変更"), lead("前の版の DB を、起動時に 1 回でまとめて今の形へ書き換える手順です（上から順に当てます）。書き換えた行は今の型の範囲で確かめ、合わない行があれば、何も変えずに止まります。") + &table(&["版", "すること", "項目", "内容"], &srows(rows(&spec, "保存の形"))));
    }
    if has(&spec, "保存先") {
        add("storages", Some("保存先"), lead("entity を置く場所と、その保存先へのリクエストの直列を誰が保証するかです。`store` の行がない entity は標準の保存先に入ります。") + &table(&["データ", "保存先", "直列の担い手"], &srows(rows(&spec, "保存先"))));
    }
    if has(&spec, "動かす前提") {
        add("needs", Some("動かす前提"), lead("アプリが実行環境に要求することです。環境の実際（いま何台か、どのポートが開いているか）は geas に書かず、plan や実環境から読んで照合します。照合は `geas env-verify` で行います（plan の JSON から。読めなかった事実は「確かめられなかった」で、合格にはしません）。由来が「人が書いた」の行は、`needs` に書かれた理由つきの要求です。") + &table(&["番号", "観点", "要求", "由来", "確かめ方"], &srows(rows(&spec, "動かす前提"))));
    }
    if has(&spec, "コンテナと基盤") {
        add("container", Some("コンテナと基盤"), lead("container（基盤に依存しない）と infra（基盤ごと）の宣言です。`geas container` が Dockerfile と compose.yaml を生成します。ホストの OS・Docker・前段のプロキシ・バックアップ・シークレットの値は、geas の外（人の保証）です。") + &crate::containergen::deploy_of(ck.p).map(|d| fig(&svg::deploy(&d, "構成図"))).unwrap_or_default() + &table(&["項目", "内容", "由来"], &srows(rows(&spec, "コンテナと基盤"))));
    }
    if has(&spec, "人の保証") {
        add("trusts", Some("人の保証"), lead("範囲の確認や直列化を、人が引き受けた（または範囲を限った）ものです。入力は、外れると範囲外の値がそのまま入ります。足し引き（+= -=）の saturating は範囲の端で止まり、trust assume は届くと異常で止まります。途中の計算は、収まると示せたものだけが何も書かずに通ります。示せない計算を人が引き受けたものは、ここに「途中の計算」として出ます（check は溢れたら異常で止まり、verify は検証ビルドだけ確かめ、assume は確かめません）。検査器は、宣言した範囲を事実として使っています。") + &table(&["処理", "入力・足し引きする項目", "型・範囲", "実行時の確認", "理由"], &srows(rows(&spec, "人の保証"))));
    }
    if has(&spec, "ルート") {
        let mut b = lead("外から触れるものはすべて URL を持ちます。リソースは名詞で、動作は index／show／create／update／destroy。パスは、どのリソースかを表します。絞り込みの条件があるときだけ、クエリ文字列の q[名前]=値 で渡します（名前の後ろの ? は、省ける条件）。")
            + &lead("「外から」が「内部（流れの途中）」の URL は、返事・期限・前の処理の結果から進む処理を呼ぶものです。外から直接呼ぶと、決済の返事などの確かめを飛ばせてしまうので、サーバーは答えません（テストのために --test で起こしたときだけ答えます）。");
        if rows(&spec, "ルート").iter().any(|r| r[0] == "PUT" || r[0] == "DELETE") {
            b += &lead(&format!("{}。", crate::rest::IF_MATCH_NOTE));
        }
        b += &route_table(&spec);
        if has(&spec, "範囲") {
            b += &format!("<h3>範囲（名前を付けた行の集合）</h3>{}{}", lead(&format!("{}。", SCOPE_NOTE)), table(&["範囲", "データ", "入る行", "使う問い合わせ・処理"], &srows(rows(&spec, "範囲"))));
        }
        if has(&spec, "POSTの問い合わせ") {
            let pr: Vec<Vec<String>> = rows(&spec, "POSTの問い合わせ").iter().map(|r| vec![format!("POST {}", cs(r, 0)), cs(r, 1), cs(r, 2)]).collect();
            b += &format!("<h3>POST で受ける問い合わせ</h3>{}{}", lead(&format!("{}。", POST_NOTE)), table(&["URL", "問い合わせ", "POST で受ける理由"], &pr));
        }
        if has(&spec, "役割") {
            b += &format!("<h3>役割（呼べる人の区別）</h3>{}{}", lead(&format!("{}。「呼べる人」の列に無い役割は、その URL を呼べません。", ACTOR_NOTE)), table(&["役割", "ヘッダに載せる名前", "呼べる URL"], &srows(rows(&spec, "役割"))));
        }
        add("routes", Some("URL（ルート）"), b);
    }
    if !records.is_empty() {
        add("open", Some("意図的に決めていないこと"), lead("計画書で「未規定」としたところです。実装が自由に選んでよい箇所で、書き忘れとは区別して記録しています。") + &format!("<div class=\"cards\">{}</div>", records.concat()));
    }
    let by_id: IndexMap<String, String> = sec.iter().map(|(a, _, h)| (a.clone(), h.clone())).collect();
    let (mut views, mut tabs): (Vec<String>, Vec<String>) = (vec![], vec![]);
    let mut add_view = |gid: &str, label: &str, html: &str| {
        let first = tabs.is_empty();
        tabs.push(format!("<button class=\"tab\" role=\"tab\" id=\"t-{}\" aria-selected=\"{}\" data-view=\"{}\">{}</button>", gid, if first { "true" } else { "false" }, gid, escape(label)));
        views.push(format!("<div class=\"view\" id=\"view-{}\"{}>{}</div>", gid, if first { "" } else { " hidden" }, html));
    };
    if let Some(h) = by_id.get("usecases") {
        add_view("uc", "ユースケース", h);
    }
    let origins: Vec<(&str, &str)> = [("walk", "画面（スマホ・PC）"), ("api", "API"), ("mcp", "MCP（AI から）")].into_iter().filter(|(a, _)| by_id.contains_key(*a)).collect();
    if !origins.is_empty() {
        let devs = if by_id.contains_key("walk") { "<span id=\"devs\" class=\"devs\" role=\"group\" aria-label=\"端末\"><button class=\"tab\" data-dev=\"phone\" aria-pressed=\"true\">スマホ</button><button class=\"tab\" data-dev=\"pc\" aria-pressed=\"false\">PC</button></span>" } else { "" };
        let sel = if origins.len() > 1 { format!("<label for=\"origin\">動作の起点</label><select id=\"origin\">{}</select>", origins.iter().map(|(a, l)| format!("<option value=\"{}\">{}</option>", a, escape(l))).collect::<String>()) } else { String::new() };
        let hint = if by_id.contains_key("walk") { "<span class=\"muted\">画面下部の操作パネルで動作を確認できます</span>" } else { "" };
        let body: String = origins.iter().map(|(a, _)| by_id[*a].clone()).collect();
        add_view("act", "動作", &format!("<div class=\"pick\">{}{}{}</div>{}", sel, devs, hint, body));
    }
    let mut groups: Vec<(&str, &str, Vec<String>)> = vec![
        ("overview", "概要", vec!["app".into(), "overview".into(), "usage".into()]),
        ("screens", "画面（流れ・一覧・デザイン・確認の網羅）", vec!["screens".into(), "design".into(), "coverage".into()]),
        ("data", "データと状態", std::iter::once("data".to_string()).chain(by_id.keys().filter(|a| a.starts_with("states-")).cloned()).collect()),
        ("rules", "決まりごと（処理・問い合わせ・不変条件）", ["rules", "levels", "migrations", "storages", "needs", "container", "trusts"].iter().map(|s| s.to_string()).collect()),
        ("systems", "外部とのつながり・常駐", ["systems", "ext", "resident", "datafolder", "files"].iter().map(|s| s.to_string()).collect()),
        ("flows", "処理の流れ", by_id.keys().filter(|a| a.starts_with("flow-")).cloned().collect()),
        ("triggers", "待ち・期限・出来事・削除", vec!["triggers".into(), "deletes".into()]),
        ("routes", "URL", vec!["routes".into()]),
        ("open", "意図的に決めていないこと", vec!["open".into()]),
    ];
    for g in groups.iter_mut() {
        g.2.retain(|a| by_id.contains_key(a));
    }
    groups.retain(|g| !g.2.is_empty());
    let sub: String = groups.iter().enumerate().map(|(i, (g, _, ids))| format!("<div class=\"sub-view\" data-sub=\"{}\"{}>{}</div>", g, if i == 0 { "" } else { " hidden" }, ids.iter().map(|a| by_id[a].clone()).collect::<String>())).collect();
    let pick = format!("<div class=\"pick\"><label for=\"specsel\">分類</label><select id=\"specsel\">{}</select></div>", groups.iter().map(|(g, l, _)| format!("<option value=\"{}\">{}</option>", g, escape(l))).collect::<String>());
    add_view("spec", "仕様", &(pick + &sub));
    if let Some(md) = plan_md {
        if !plan_others(md).1.is_empty() {
            add_view("etc", "その他", &others_view(md, &tools));
        }
    }
    let nav = format!("<div class=\"tabs\" role=\"tablist\">{}</div>", tabs.concat());
    let body = views.concat();
    let scripts = format!("<script>{}</script>{}", TAB_JS, if p.screens.is_empty() { String::new() } else { format!("<script>{}</script>", JS) });
    Ok(page(format!("仕様書 {}", name).trim(), if strip_parens(name).is_empty() { "仕様書" } else { strip_parens(name) }, &nav, &body, &scripts, fragment))
}
