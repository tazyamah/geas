// SPDX-License-Identifier: GPL-3.0-or-later
//! plan の仕様書（①）の読み取りと、生成した仕様書（③）との突合（Python 版 plan.py の移し）。
//! ①は固定の見出しと表を持つ Markdown。◆の欄だけを機械が突合し、それ以外（文章・来歴）は読み飛ばす。
use crate::spec::Spec;
use indexmap::{IndexMap, IndexSet};
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct Finding {
    pub kind: String,
    pub where_: String,
    pub msg: String,
}

pub const A_ONLY: &str = "計画にあってコードにない";
pub const B_ONLY: &str = "コードにあって計画にない";
pub const DIFF: &str = "値が違う";
pub const EXAMPLE: &str = "具体例の不一致";
pub const UNSPEC: &str = "未規定";

fn f(kind: &str, where_: String, msg: String) -> Finding {
    Finding { kind: kind.into(), where_, msg }
}

pub type Row = IndexMap<String, String>;
pub type Plan = IndexMap<String, Vec<Row>>;

/// 比べるための正規化：空白を除き、矢印とカンマの書き方をそろえる
pub fn n(s: &str) -> String {
    let s: String = s.chars().filter(|c| !matches!(c, ' ' | '\t' | '　')).collect();
    s.replace("->", "→").replace('、', ",").replace('，', ",").replace('（', "(").replace('）', ")")
}

/// Python の str.strip()（Unicode の空白）
fn strip(s: &str) -> &str {
    s.trim()
}

pub fn split_top(s: &str, sep: char) -> Vec<String> {
    let (mut out, mut depth, mut cur): (Vec<String>, i32, String) = (vec![], 0, String::new());
    for c in s.chars() {
        if "({[".contains(c) {
            depth += 1;
        } else if ")}]".contains(c) {
            depth -= 1;
        }
        if c == sep && depth == 0 {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    if !strip(&cur).is_empty() || !out.is_empty() {
        out.push(cur);
    }
    out.into_iter().map(|x| strip(&x).to_string()).filter(|x| !x.is_empty()).collect()
}

fn sep_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^:?-{2,}:?$").unwrap())
}

/// Markdown の表を (見出し, 行) の列にする。見出しの「◆」は外す
pub fn tables(md: &str) -> Vec<(Vec<String>, Vec<Row>)> {
    let mut out: Vec<(Vec<String>, Vec<Row>)> = vec![];
    let mut cur: Option<(Vec<String>, Vec<Row>)> = None;
    for line in md.lines() {
        if line.trim_start().starts_with('|') {
            let mut cells: Vec<String> = line.trim().trim_matches('|').split('|').map(|c| c.trim().to_string()).collect();
            match &mut cur {
                None => cur = Some((cells.iter().map(|c| c.replace('◆', "").trim().to_string()).collect(), vec![])),
                Some((hd, rows)) => {
                    if cells.iter().filter(|c| !c.is_empty()).all(|c| sep_re().is_match(c)) {
                        continue;
                    }
                    while cells.len() < hd.len() {
                        cells.push(String::new());
                    }
                    let mut r = Row::new();
                    for (h, c) in hd.iter().zip(cells.into_iter()) {
                        r.insert(h.clone(), c);
                    }
                    rows.push(r);
                }
            }
        } else if let Some(c) = cur.take() {
            out.push(c);
        }
    }
    if let Some(c) = cur {
        out.push(c);
    }
    out
}

pub fn classify(headers: &[String]) -> Option<&'static str> {
    let h: IndexSet<&str> = headers.iter().map(|s| s.as_str()).collect();
    let table: [(&str, &[&str]); 20] = [
        ("データ", &["項目", "型・範囲"]),
        ("状態", &["状態", "区分"]),
        ("遷移", &["元", "先", "起こす処理"]),
        ("拒否", &["元", "先", "拒否の理由"]),
        ("問い合わせ", &["問い合わせ", "条件", "並び"]),
        ("処理", &["入力", "読む", "書く"]),
        ("不変条件", &["内容"]),
        ("トリガ", &["トリガ", "要因", "行き先"]),
        ("外部作用", &["外部作用", "出す内容"]),
        ("合流", &["外部作用", "合流先のトリガ"]),
        ("出来事", &["出来事", "起こし方", "処理"]),
        ("ルート", &["メソッド", "経路", "動作", "呼ぶもの"]),
        ("画面", &["画面", "見せる", "入力欄", "入口"]),
        ("画面遷移", &["画面", "操作", "結果", "行き先"]),
        ("ユースケース", &["ユースケース", "手順", "画面", "操作"]),
        ("指定", &["項目", "指定"]),
        ("URL例", &["メソッド", "経路", "期待する結果", "期待する状態"]),
        ("画面例", &["ユースケース", "手順", "入れる・選ぶ", "見えること"]),
        ("デザイン", &["画面", "指定", "値"]),
        ("具体例", &["期待する結果"]),
    ];
    for (kind, need) in table.iter() {
        if need.iter().all(|x| h.contains(x)) {
            return Some(kind);
        }
    }
    None
}

pub fn read_plan(md: &str) -> Plan {
    let mut plan: Plan = IndexMap::new();
    for (headers, rows) in tables(md) {
        if let Some(kind) = classify(&headers) {
            plan.entry(kind.to_string()).or_default().extend(rows);
        }
    }
    plan
}

pub fn items(cell: &str) -> BTreeSet<String> {
    split_top(&n(cell).replace('/', ","), ',').into_iter().filter(|x| !x.is_empty() && x != "なし").collect()
}

/// 突合の値：文字列・集合・文字列の並び
#[derive(Clone, Debug, PartialEq)]
pub enum V {
    S(String),
    Set(BTreeSet<String>),
    List(Vec<String>),
}

fn py_repr(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut o = String::new();
    o.push(q);
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            c if c == q => {
                o.push('\\');
                o.push(c)
            }
            c => o.push(c),
        }
    }
    o.push(q);
    o
}

fn fmt(v: &V) -> String {
    match v {
        V::Set(s) => {
            if s.is_empty() {
                "なし".to_string()
            } else {
                format!("{{{}}}", s.iter().cloned().collect::<Vec<_>>().join(", "))
            }
        }
        V::S(s) => {
            if s.is_empty() {
                "（空）".to_string()
            } else {
                s.clone()
            }
        }
        V::List(l) => format!("[{}]", l.iter().map(|x| py_repr(x)).collect::<Vec<_>>().join(", ")),
    }
}

fn s(x: &str) -> V {
    V::S(x.to_string())
}

type Fields = IndexMap<String, V>;

fn cmp_map<K: std::hash::Hash + Eq + Clone>(kind: &str, a: &IndexMap<K, Fields>, b: &IndexMap<K, Fields>, out: &mut Vec<Finding>, label: &dyn Fn(&K) -> String) {
    for k in a.keys() {
        if !b.contains_key(k) {
            out.push(f(A_ONLY, format!("{} {}", kind, label(k)), "計画にあるが、コードから生成した仕様書に無い".into()));
        }
    }
    for k in b.keys() {
        if !a.contains_key(k) {
            out.push(f(B_ONLY, format!("{} {}", kind, label(k)), "コードにあるが、計画に無い（会話で決めていない振る舞いなら、問いにする）".into()));
        }
    }
    for (k, av) in a {
        if let Some(bv) = b.get(k) {
            if !dict_eq(av, bv) {
                for (field, x) in av {
                    if let Some(y) = bv.get(field) {
                        if x != y {
                            out.push(f(DIFF, format!("{} {} の{}", kind, label(k), field), format!("計画: {} ／ コード: {}", fmt(x), fmt(y))));
                        }
                    }
                }
            }
        }
    }
}

fn dict_eq(a: &Fields, b: &Fields) -> bool {
    a.len() == b.len() && a.iter().all(|(k, v)| b.get(k) == Some(v))
}

/// 計画書の経路に当たるルートを返す（決まった語の多い経路を優先）。ルートの行は (メソッド, 経路, …)
pub fn route_match<'a>(routes: &'a [Value], method: &str, path: &str) -> Option<&'a Value> {
    let segs: Vec<&str> = path.trim().trim_matches('/').split('/').collect();
    let mut best: Option<&Value> = None;
    for r in routes {
        if r[0].as_str() != Some(strip(method)) {
            continue;
        }
        let rp = r[1].as_str().unwrap_or("");
        let rs: Vec<&str> = rp.trim_matches('/').split('/').collect();
        if rs.len() != segs.len() || rs.iter().zip(segs.iter()).any(|(a, b)| !a.starts_with(':') && a != b) {
            continue;
        }
        let fixed = |v: &[&str]| v.iter().filter(|a| !a.starts_with(':')).count();
        let better = match best {
            None => true,
            Some(bst) => {
                let bp: Vec<&str> = bst[1].as_str().unwrap_or("").trim_matches('/').split('/').collect();
                fixed(&rs) > fixed(&bp)
            }
        };
        if better {
            best = Some(r);
        }
    }
    best
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
fn g(r: &Row, k: &str) -> String {
    r.get(k).cloned().unwrap_or_default()
}
fn gs(r: &Row, k: &str) -> String {
    strip(&g(r, k)).to_string()
}
fn set_of(v: BTreeSet<String>) -> V {
    V::Set(v)
}
fn fl(pairs: Vec<(&str, V)>) -> Fields {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

pub fn compare_static(spec: &Spec, plan: &Plan) -> Vec<Finding> {
    let mut out: Vec<Finding> = vec![];
    let sect = |name: &str| -> &[Row] { plan.get(name).map(|v| v.as_slice()).unwrap_or(&[]) };
    let j = |k: &Vec<String>| k.join(".");
    // データ
    let mut a: IndexMap<Vec<String>, Fields> = IndexMap::new();
    for r in sect("データ") {
        a.insert(vec![g(r, "データ"), g(r, "項目")], fl(vec![("型・範囲", s(&n(&g(r, "型・範囲")))), ("必須", s(&gs(r, "必須"))), ("一意", s(&gs(r, "一意")))]));
    }
    let mut b: IndexMap<Vec<String>, Fields> = IndexMap::new();
    for r in rows(spec, "データ") {
        b.insert(vec![cs(r, 0), cs(r, 1)], fl(vec![("型・範囲", s(&n(&cs(r, 2)))), ("必須", s(&cs(r, 3))), ("一意", s(&cs(r, 4)))]));
    }
    cmp_map("データ", &a, &b, &mut out, &j);
    // 状態
    let mut a: IndexMap<Vec<String>, Fields> = IndexMap::new();
    for r in sect("状態") {
        a.insert(vec![g(r, "データ"), g(r, "状態")], fl(vec![("区分", s(&gs(r, "区分")))]));
    }
    let mut b: IndexMap<Vec<String>, Fields> = IndexMap::new();
    for r in rows(spec, "状態") {
        b.insert(vec![cs(r, 0), cs(r, 1)], fl(vec![("区分", s(&cs(r, 2)))]));
    }
    cmp_map("状態", &a, &b, &mut out, &j);
    // 遷移
    let tl = |k: &Vec<String>| format!("{}: {}→{}", k[0], k[1], k[2]);
    let mut a: IndexMap<Vec<String>, Fields> = IndexMap::new();
    for r in sect("遷移") {
        let procs: Vec<String> = items(&g(r, "起こす処理")).into_iter().collect();
        a.insert(vec![g(r, "データ"), g(r, "元"), g(r, "先")], fl(vec![("起こす処理", s(&procs.join(",")))]));
    }
    let mut bm: IndexMap<Vec<String>, BTreeSet<String>> = IndexMap::new();
    for r in rows(spec, "遷移") {
        bm.entry(vec![cs(r, 0), cs(r, 1), cs(r, 2)]).or_default().insert(cs(r, 3));
    }
    let b: IndexMap<Vec<String>, Fields> = bm.into_iter().map(|(k, v)| (k, fl(vec![("起こす処理", s(&v.into_iter().collect::<Vec<_>>().join(",")))]))).collect();
    cmp_map("遷移", &a, &b, &mut out, &tl);
    let a: IndexMap<Vec<String>, Fields> = sect("拒否").iter().map(|r| (vec![g(r, "データ"), g(r, "元"), g(r, "先")], Fields::new())).collect();
    let b: IndexMap<Vec<String>, Fields> = rows(spec, "拒否").iter().map(|r| (vec![cs(r, 0), cs(r, 1), cs(r, 2)], Fields::new())).collect();
    cmp_map("拒否", &a, &b, &mut out, &tl);
    // 処理
    let mut a: IndexMap<String, Fields> = IndexMap::new();
    for r in sect("処理") {
        let rt = if r.contains_key("結果 → 行き先") { g(r, "結果 → 行き先") } else { g(r, "結果→行き先") };
        a.insert(g(r, "処理"), fl(vec![("入力", set_of(items(&g(r, "入力")))), ("読む", set_of(items(&g(r, "読む")))), ("書く", set_of(items(&g(r, "書く")))), ("結果→行き先", set_of(items(&rt))), ("外部作用", set_of(items(&g(r, "外部作用"))))]));
    }
    let mut b: IndexMap<String, Fields> = IndexMap::new();
    for x in rows(spec, "処理") {
        let res: Vec<String> = x["結果"].as_array().unwrap().iter().map(|r| format!("{}→{}", cs(r, 0), cs(r, 1))).collect();
        b.insert(
            sj(&x["処理"]),
            fl(vec![
                ("入力", set_of(items(&jarr(&x["入力"]).join(",")))),
                ("読む", set_of(items(&jarr(&x["読む"]).join(",")))),
                ("書く", set_of(items(&jarr(&x["書く"]).join(",")))),
                ("結果→行き先", set_of(items(&res.join(",")))),
                ("外部作用", set_of(items(&jarr(&x["外部"]).join(",")))),
            ]),
        );
    }
    cmp_map("処理", &a, &b, &mut out, &|k: &String| k.clone());
    // 問い合わせ
    let mut a: IndexMap<String, Fields> = IndexMap::new();
    for r in sect("問い合わせ") {
        a.insert(g(r, "問い合わせ"), fl(vec![("入力", set_of(items(&g(r, "入力")))), ("読む", set_of(items(&g(r, "読む")))), ("条件", set_of(items(&g(r, "条件")))), ("並び", s(&n(&g(r, "並び")))), ("ページ", s(&n(&g(r, "ページ")))), ("列", set_of(items(&g(r, "列"))))]));
    }
    let mut b: IndexMap<String, Fields> = IndexMap::new();
    for x in rows(spec, "問い合わせ") {
        b.insert(
            sj(&x["問い合わせ"]),
            fl(vec![
                ("入力", set_of(items(&jarr(&x["入力"]).join(",")))),
                ("読む", set_of(items(&jarr(&x["読む"]).join(",")))),
                ("条件", set_of(items(&jarr(&x["条件"]).join(",")))),
                ("並び", s(&n(&sj(&x["並び"])))),
                ("ページ", s(&n(&sj(&x["ページ"])))),
                ("列", set_of(items(&jarr(&x["列"]).join(",")))),
            ]),
        );
    }
    cmp_map("問い合わせ", &a, &b, &mut out, &|k: &String| k.clone());
    // トリガ
    let colon = |k: &Vec<String>| format!("{}: {}", k[0], k[1]);
    let a: IndexMap<Vec<String>, Fields> = sect("トリガ").iter().map(|r| (vec![g(r, "トリガ"), n(&g(r, "要因"))], fl(vec![("行き先", s(&n(&g(r, "行き先"))))]))).collect();
    let b: IndexMap<Vec<String>, Fields> = rows(spec, "トリガ").iter().map(|r| (vec![cs(r, 0), n(&cs(r, 1))], fl(vec![("行き先", s(&n(&cs(r, 2))))]))).collect();
    cmp_map("トリガ", &a, &b, &mut out, &colon);
    // 外部作用
    let a: IndexMap<String, Fields> = sect("外部作用").iter().map(|r| (g(r, "外部作用"), fl(vec![("出す内容", s(&n(&g(r, "出す内容"))))]))).collect();
    let b: IndexMap<String, Fields> = rows(spec, "外部作用").iter().map(|r| (cs(r, 0), fl(vec![("出す内容", s(&n(&cs(r, 1))))]))).collect();
    cmp_map("外部作用", &a, &b, &mut out, &|k: &String| k.clone());
    // 合流
    let a: IndexMap<String, Fields> = sect("合流").iter().map(|r| (g(r, "外部作用"), fl(vec![("合流先", s(&n(&g(r, "合流先のトリガ"))))]))).collect();
    let b: IndexMap<String, Fields> = rows(spec, "合流").iter().map(|r| (cs(r, 0), fl(vec![("合流先", s(&n(&cs(r, 1))))]))).collect();
    cmp_map("合流", &a, &b, &mut out, &|k: &String| k.clone());
    // 出来事
    let a: IndexMap<Vec<String>, Fields> = sect("出来事").iter().map(|r| (vec![g(r, "出来事"), n(&g(r, "起こし方")), n(&g(r, "処理"))], Fields::new())).collect();
    let b: IndexMap<Vec<String>, Fields> = rows(spec, "出来事").iter().map(|r| (vec![cs(r, 0), n(&cs(r, 1)), n(&cs(r, 2))], Fields::new())).collect();
    cmp_map("出来事", &a, &b, &mut out, &|k: &Vec<String>| format!("{}: {} → {}", k[0], k[1], k[2]));
    // 画面
    let mut a: IndexMap<String, Fields> = IndexMap::new();
    for r in sect("画面") {
        a.insert(g(r, "画面"), fl(vec![("見せる", set_of(items(&g(r, "見せる")))), ("入力欄", set_of(items(&g(r, "入力欄")))), ("受け取る値", set_of(items(&g(r, "受け取る値")))), ("入口", s(&gs(r, "入口"))), ("選ばせる slot", set_of(items(&g(r, "選ばせる slot"))))]));
    }
    let mut b: IndexMap<String, Fields> = IndexMap::new();
    for r in rows(spec, "画面") {
        b.insert(cs(r, 0), fl(vec![("見せる", set_of(items(&cs(r, 1)))), ("入力欄", set_of(items(&cs(r, 2)))), ("受け取る値", set_of(items(&cs(r, 3)))), ("入口", s(&cs(r, 4))), ("選ばせる slot", set_of(items(&cs(r, 5))))]));
    }
    if sect("画面").iter().any(|r| r.contains_key("URL")) {
        for r in sect("画面") {
            a.get_mut(&g(r, "画面")).unwrap().insert("URL".into(), s(&n(&g(r, "URL"))));
        }
        for r in rows(spec, "画面") {
            let last = r.as_array().unwrap().len() - 1;
            b.get_mut(&cs(r, 0)).unwrap().insert("URL".into(), s(&n(&cs(r, last))));
        }
    }
    cmp_map("画面", &a, &b, &mut out, &|k: &String| k.clone());
    let a: IndexMap<Vec<String>, Fields> = sect("画面遷移").iter().map(|r| (vec![g(r, "画面"), n(&g(r, "操作")), n(&g(r, "結果"))], fl(vec![("行き先", s(&n(&g(r, "行き先"))))]))).collect();
    let b: IndexMap<Vec<String>, Fields> = rows(spec, "画面遷移").iter().map(|r| (vec![cs(r, 0), n(&cs(r, 1)), n(&cs(r, 2))], fl(vec![("行き先", s(&n(&cs(r, 3))))]))).collect();
    cmp_map("画面遷移", &a, &b, &mut out, &|k: &Vec<String>| format!("{}: {} {}", k[0], k[1], k[2]).trim_end().to_string());
    // URL の具体例
    let routes = rows(spec, "ルート");
    for r in sect("URL例") {
        let where_ = format!("URL例 {}", strip(&g(r, "ID")));
        let rt = match route_match(routes, &g(r, "メソッド"), &g(r, "経路")) {
            None => {
                out.push(f(A_ONLY, where_, format!("{} {} に当たるルートが、コードにありません", strip(&g(r, "メソッド")), strip(&g(r, "経路")))));
                continue;
            }
            Some(x) => x,
        };
        let want: IndexMap<String, String> = cs(rt, 4).split(", ").filter(|x| x.contains('→')).map(|x| {
            let mut it = x.splitn(2, '→');
            (it.next().unwrap().to_string(), it.next().unwrap_or("").to_string())
        }).collect();
        let res = n(&g(r, "期待する結果"));
        let code = gs(r, "期待する状態");
        if res == "－" || res == "-" {
            continue;
        }
        if !want.is_empty() && !want.keys().any(|k| n(k) == res) {
            out.push(f(DIFF, where_, format!("結果「{}」は、ルート {} {} の結果にありません（{}）", strip(&g(r, "期待する結果")), cs(rt, 0), cs(rt, 1), want.keys().cloned().collect::<Vec<_>>().join(", "))));
        } else if !want.is_empty() {
            let w = want.iter().find(|(k, _)| n(k) == res).map(|(_, v)| v.clone()).unwrap();
            if code != w {
                out.push(f(DIFF, where_, format!("計画: {}→{} ／ コード: {}→{}", res, code, res, w)));
            }
        }
    }
    // ルート
    let mut a: IndexMap<Vec<String>, Fields> = IndexMap::new();
    for r in sect("ルート") {
        let mut fs = fl(vec![("動作", s(&gs(r, "動作"))), ("呼ぶもの", s(&n(&g(r, "呼ぶもの")))), ("結果→状態", set_of(items(&g(r, "結果→状態"))))]);
        if r.contains_key("外から") {
            fs.insert("外から".into(), s(&n(&g(r, "外から"))));
        }
        if r.contains_key("呼べる人") {
            let mut l: Vec<String> = g(r, "呼べる人").split('、').filter(|x| !strip(x).is_empty()).map(n).collect();
            l.sort();
            fs.insert("呼べる人".into(), V::List(l));
        }
        a.insert(vec![gs(r, "メソッド"), gs(r, "経路")], fs);
    }
    let mut b: IndexMap<Vec<String>, Fields> = IndexMap::new();
    for r in routes {
        let mut l: Vec<String> = cs(r, 6).split('、').filter(|x| !strip(x).is_empty()).map(n).collect();
        l.sort();
        b.insert(vec![cs(r, 0), cs(r, 1)], fl(vec![("動作", s(&cs(r, 2))), ("呼ぶもの", s(&n(&cs(r, 3)))), ("結果→状態", set_of(items(&cs(r, 4)))), ("外から", s(&n(&cs(r, 5)))), ("呼べる人", V::List(l))]));
    }
    if !a.is_empty() || b.is_empty() {
        cmp_map("ルート", &a, &b, &mut out, &|k: &Vec<String>| format!("{} {}", k[0], k[1]));
    }
    out.extend(check_usecases(spec, sect("ユースケース")));
    // 不変条件
    let a: IndexMap<String, Fields> = sect("不変条件").iter().map(|r| (n(&g(r, "内容")), Fields::new())).collect();
    let b: IndexMap<String, Fields> = rows(spec, "不変条件").iter().map(|t| (n(&sj(t)), Fields::new())).collect();
    cmp_map("不変条件", &a, &b, &mut out, &|k: &String| k.clone());
    // 指定事項
    let mut a: IndexMap<String, Fields> = IndexMap::new();
    for r in sect("指定") {
        if g(r, "指定").contains("未規定") {
            out.push(f(UNSPEC, format!("指定事項 {}", g(r, "項目")), "計画で意図的に未規定（コードは既定に任せる）。問いとして残る".into()));
        } else {
            a.insert(g(r, "項目"), fl(vec![("指定", s(&n(&g(r, "指定"))))]));
        }
    }
    let b: IndexMap<String, Fields> = rows(spec, "指定").iter().map(|r| (cs(r, 0), fl(vec![("指定", s(&n(&cs(r, 1))))]))).collect();
    cmp_map("指定", &a, &b, &mut out, &|k: &String| k.clone());
    out
}

pub const VIEW_OP: &str = "見る";

/// 計画書のユースケースの表を、ユースケースごとの手順の列（手順の番号順）にする
pub fn usecases(rows: &[Row]) -> IndexMap<String, Vec<Row>> {
    let mut out: IndexMap<String, Vec<Row>> = IndexMap::new();
    for r in rows {
        out.entry(g(r, "ユースケース")).or_default().push(r.clone());
    }
    for v in out.values_mut() {
        v.sort_by_key(|r| {
            let t = strip(&g(r, "手順")).to_string();
            if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) {
                t.parse::<i64>().unwrap_or(0)
            } else {
                0
            }
        });
    }
    out
}

pub fn check_usecases(spec: &Spec, rows: &[Row]) -> Vec<Finding> {
    if rows.is_empty() {
        return vec![];
    }
    let mut out: Vec<Finding> = vec![];
    let mut trans: IndexMap<(String, String, String), String> = IndexMap::new();
    for r in self::rows(spec, "画面遷移") {
        trans.insert((n(&cs(r, 0)), n(&cs(r, 1)), n(&cs(r, 2))), n(&cs(r, 3)));
    }
    let screens = self::rows(spec, "画面");
    let entries: IndexSet<String> = screens.iter().filter(|x| !cs(x, 4).is_empty()).map(|x| n(&cs(x, 0))).collect();
    let urls: IndexMap<String, String> = screens.iter().map(|x| (n(&cs(x, 0)), n(&cs(x, x.as_array().unwrap().len() - 1)))).collect();
    let mut used: IndexSet<(String, String, String)> = IndexSet::new();
    for (uc, steps) in usecases(rows) {
        let mut here: Option<String> = None;
        for st in &steps {
            let (scr, op, res) = (n(&g(st, "画面")), n(&g(st, "操作")), n(&g(st, "結果")));
            let where_ = format!("ユースケース {} 手順 {}", uc, g(st, "手順"));
            if st.contains_key("URL") {
                if let Some(u) = urls.get(&scr) {
                    if &n(&g(st, "URL")) != u {
                        out.push(f(DIFF, where_.clone(), format!("URL は計画: {} ／ コード: {}（画面「{}」の URL）", strip(&g(st, "URL")), u, g(st, "画面"))));
                    }
                }
            }
            if let Some(h) = &here {
                if &scr != h {
                    out.push(f(DIFF, where_.clone(), format!("前の手順のあとに見えている画面は「{}」ですが、この手順は「{}」です", h, g(st, "画面"))));
                }
            }
            if op == n(VIEW_OP) {
                if here.is_none() && !entries.contains(&scr) {
                    out.push(f(DIFF, where_.clone(), format!("「{}」は画面の入口ではありません", g(st, "画面"))));
                }
                here = Some(scr);
                continue;
            }
            match trans.get(&(scr.clone(), op.clone(), res.clone())) {
                None => {
                    out.push(f(A_ONLY, where_, format!("画面「{}」の操作「{}」（結果「{}」）は、コードの画面遷移にありません", g(st, "画面"), g(st, "操作"), g(st, "結果"))));
                    here = None;
                }
                Some(dest) => {
                    used.insert((scr.clone(), op, res));
                    here = if dest == "そのまま" {
                        Some(scr)
                    } else if dest == "戻る" {
                        None
                    } else {
                        Some(dest.clone())
                    };
                }
            }
        }
    }
    for k in trans.keys() {
        if !used.contains(k) {
            out.push(f(B_ONLY, format!("画面遷移 {}: {} {}", k.0, k.1, k.2).trim_end().to_string(), "どのユースケースの手順にも使われていません（ユースケースが足りないか、不要な遷移）".into()));
        }
    }
    out
}

pub const COVERAGE_HEAD: [&str; 7] = ["メソッド", "経路", "結果", "状態", "具体例", "ユースケース", "確認"];

/// 内部 URL（ルート）ごと、結果の種類（HTTP の状態）ごとに、計画書の具体例とユースケースが当たっているかを数える
pub fn coverage(spec: &Spec, plan: &Plan) -> (Vec<Vec<String>>, usize, usize) {
    let split_paren = Regex::new(r"[（(]").unwrap();
    let mut ex: IndexMap<(String, String), Vec<String>> = IndexMap::new();
    for r in plan.get("具体例").map(|v| v.as_slice()).unwrap_or(&[]) {
        let res = n(split_paren.split(&g(r, "期待する結果")).next().unwrap_or(""));
        if !g(r, "期待する結果").contains("未規定") {
            ex.entry((n(&g(r, "処理")), res)).or_default().push(strip(&g(r, "ID")).to_string());
        }
    }
    let routes = self::rows(spec, "ルート");
    let mut url_ex: IndexMap<(String, String, String), Vec<String>> = IndexMap::new();
    for r in plan.get("URL例").map(|v| v.as_slice()).unwrap_or(&[]) {
        if let Some(rt) = route_match(routes, &g(r, "メソッド"), &g(r, "経路")) {
            url_ex.entry((cs(rt, 0), cs(rt, 1), n(&g(r, "期待する結果")))).or_default().push(strip(&g(r, "ID")).to_string());
        }
    }
    let mut shows: IndexMap<String, BTreeSet<String>> = IndexMap::new();
    for x in self::rows(spec, "画面") {
        let mut set = items(&cs(x, 1));
        for fd in split_top(&cs(x, 2), ',') {
            if let Some((_, q)) = fd.split_once('←') {
                set.insert(n(q));
            }
        }
        shows.insert(n(&cs(x, 0)), set);
    }
    let mut uc_act: IndexMap<(String, String), BTreeSet<String>> = IndexMap::new();
    let mut uc_get: IndexMap<String, BTreeSet<String>> = IndexMap::new();
    let ucrows: &[Row] = plan.get("ユースケース").map(|v| v.as_slice()).unwrap_or(&[]);
    for (uc, steps) in usecases(ucrows) {
        for st in &steps {
            let (scr, op, res) = (n(&g(st, "画面")), n(&g(st, "操作")), n(&g(st, "結果")));
            if let Some(qs) = shows.get(&scr) {
                for q in qs {
                    uc_get.entry(q.clone()).or_default().insert(uc.clone());
                }
            }
            if op != n(VIEW_OP) {
                uc_act.entry((op, res)).or_default().insert(uc.clone());
            }
        }
    }
    let (mut out, mut got) = (vec![], 0usize);
    for r in routes {
        let (m, path, tgt, st) = (cs(r, 0), cs(r, 1), cs(r, 3), cs(r, 4));
        let t = n(&tgt);
        let mut variants: Vec<(String, String)> = st.split(", ").filter(|x| x.contains('→')).map(|x| {
            let mut it = x.splitn(2, '→');
            (it.next().unwrap().to_string(), it.next().unwrap_or("").to_string())
        }).collect();
        if variants.is_empty() {
            variants.push((String::new(), if m == "GET" { "200".into() } else { "－".into() }));
        }
        for (v, code) in variants {
            let mut e: Vec<String> = if !v.is_empty() { ex.get(&(t.clone(), n(&v))).cloned().unwrap_or_default() } else { vec![] };
            e.extend(url_ex.get(&(m.clone(), path.clone(), n(&v))).cloned().unwrap_or_default());
            if v.is_empty() {
                e.extend(url_ex.get(&(m.clone(), path.clone(), String::new())).cloned().unwrap_or_default());
            }
            let u: Vec<String> = if !v.is_empty() { uc_act.get(&(t.clone(), n(&v))).cloned().unwrap_or_default().into_iter().collect() } else { uc_get.get(&t).cloned().unwrap_or_default().into_iter().collect() };
            let ok = !e.is_empty() || !u.is_empty();
            if ok {
                got += 1;
            }
            out.push(vec![m.clone(), path.clone(), v, code, e.join(", "), u.join("、"), if ok { "○".into() } else { "未".into() }]);
        }
    }
    let total = out.len();
    (out, got, total)
}

pub fn coverage_markdown(spec: &Spec, plan: &Plan) -> Vec<String> {
    let (rows, got, total) = coverage(spec, plan);
    let mut out = vec![
        String::new(),
        format!("## 内部 URL ごとの確認の網羅（確認できた {} / {}、{}%。具体例とユースケースが当たっているか）", got, total, got * 100 / total.max(1)),
        String::new(),
        format!("| {} |", COVERAGE_HEAD.join(" | ")),
        format!("|{}", "---|".repeat(COVERAGE_HEAD.len())),
    ];
    out.extend(rows.iter().map(|r| format!("| {} |", r.join(" | "))));
    out
}
