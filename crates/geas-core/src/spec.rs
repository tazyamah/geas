// SPDX-License-Identifier: GPL-3.0-or-later
//! コードから「検査可能な仕様書」（③）を生成する（Python 版 spec.py の移し）。
//! 仕様書は、Python 版の辞書と同じ形（順序つきの JSON 値）で持つ。突合・レビュー画面もこの形を読む。
use crate::ast::*;
use crate::check::{phys, Checker, VERIFIED_STORES};
use crate::rest::{self, IF_MATCH_NOTE, INTERNAL, OPEN};
use indexmap::{IndexMap, IndexSet};
use serde_json::{json, Map, Value};

pub type Spec = Map<String, Value>;

pub fn ja_dest(d: &str) -> String {
    if d == "reply" {
        "応答".to_string()
    } else if let Some(r) = d.strip_prefix("wait ") {
        format!("待つ {}", r)
    } else {
        d.to_string()
    }
}

pub fn ja_kind(k: &str) -> String {
    match k {
        "reply" => "応答".into(),
        "deadline" => "期限".into(),
        "all" => "全部の応答".into(),
        o => o.into(),
    }
}

pub fn fmt_cause(kind: &str, key: &str) -> String {
    if kind == "all" {
        return key.split(',').collect::<Vec<_>>().join(" ＋ ");
    }
    if kind != "deadline" {
        return key.to_string();
    }
    let s: i128 = key.parse().unwrap_or(0);
    for (unit, sec) in [("日", 86400i128), ("時間", 3600), ("分", 60)] {
        if s % sec == 0 && !(unit == "日" && s < 2 * 86400) {
            return format!("現在+{}{}", s / sec, unit);
        }
    }
    format!("現在+{}秒", s)
}

pub fn fmt_secs(s: i128) -> String {
    for (unit, sec) in [("日", 86400i128), ("時間", 3600), ("分", 60)] {
        if s % sec == 0 && !(unit == "日" && s < 2 * 86400) {
            return format!("{}{}", s / sec, unit);
        }
    }
    format!("{}秒", s)
}

pub fn input_names(inputs: &[(String, Type)]) -> Vec<String> {
    inputs.iter().map(|(n, t)| if t.optional { n.clone() } else { format!("{}（必須）", n) }).collect()
}

fn order_text(ck: &Checker, q: &Query) -> String {
    let (f0, d0) = match &q.order {
        None => return String::new(),
        Some(o) => o.clone(),
    };
    let dt = ck.data.get(&q.target);
    let one = |f: &str, d: &str| -> String {
        let kind = dt.and_then(|dt| dt.fields.iter().find(|x| x.name == f)).map(|x| x.r#type.kind.clone());
        let words = if kind.as_deref() == Some("日時") { ("新しい順", "古い順") } else { ("大きい順", "小さい順") };
        format!("{} {}", f, if d == "降順" { words.0 } else { words.1 })
    };
    let choices = order_choices(q);
    if choices.is_empty() {
        return one(&f0, &d0);
    }
    choices.iter().enumerate().map(|(k, (name, f, d))| format!("{}（q[order]={}{}）", one(f, d), name, if k == 0 { "。選ばないとき" } else { "" })).collect::<Vec<_>>().join("、")
}

fn conds_text(groups: &[Vec<Cond3>]) -> Vec<String> {
    groups.iter().map(|g| g.iter().map(|(f, op, st)| format!("{} {} {}", f, op, st)).collect::<Vec<_>>().join(" または ")).collect()
}

fn query_rows(ck: &Checker, q: &Query) -> Value {
    let mut conds = conds_text(&q.conds);
    let ins: IndexMap<&String, &Type> = q.inputs.iter().map(|(n, t)| (n, t)).collect();
    let absent = |x: &str, what: &str| -> String {
        match ins.get(&x.to_string()) {
            Some(t) if t.optional => format!("（{}が無ければ{}）", x, what),
            _ => String::new(),
        }
    };
    for s in scopes_of(ck.p, q) {
        for g in &s.conds {
            conds.push(format!("{}（範囲 {}）", g.iter().map(|(f, op, st)| format!("{} {} {}", f, op, st)).collect::<Vec<_>>().join(" または "), s.name));
        }
        let args = q.scope_args.get(&s.name).cloned().unwrap_or_default();
        let given: IndexMap<&String, &String> = s.params.iter().map(|(n, _)| n).zip(args.iter()).collect();
        for (f, op, a) in &s.filters {
            conds.push(format!("{} {} {}（範囲 {}）", f, op, given.get(a).map(|x| x.as_str()).unwrap_or(a), s.name));
        }
        if !s.caller.is_empty() {
            conds.push(format!("{} 一致 呼んだ本人のID（範囲 {}）", s.caller, s.name));
        }
    }
    for (f, op, a) in &q.filters {
        if op == "直近" {
            conds.push(format!("{} 直近 {}", f, fmt_secs(a.parse().unwrap_or(0))));
        } else {
            conds.push(format!("{} {} {}{}", f, op, a, absent(a, "絞らない")));
        }
    }
    let page = if let Some(n) = q.page_n.filter(|n| *n != 0) {
        format!("{}件", n)
    } else if let Some(pg) = q.page.as_ref().filter(|s| !s.is_empty()) {
        format!("{}{}", pg, absent(pg, "全件"))
    } else {
        String::new()
    };
    let mut reads: Vec<String> = q.reads.clone();
    reads.sort();
    json!({"問い合わせ": q.name, "入力": input_names(&q.inputs), "読む": reads, "条件": conds, "列": q.select.iter().map(|c| c.name.clone()).collect::<Vec<_>>(), "並び": order_text(ck, q), "ページ": page})
}

/// 仕様書の中の文字列の名前を、表示名（日本語）に置き換える（構造はそのまま）
pub fn show_all(p: &Program, x: &Value) -> Value {
    match x {
        Value::String(s) => Value::String(p.show_names(s)),
        Value::Array(a) => Value::Array(a.iter().map(|v| show_all(p, v)).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), show_all(p, v))).collect()),
        o => o.clone(),
    }
}

pub fn read_text(rd: &Option<ReadSpec>) -> String {
    let rd = match rd {
        None => return String::new(),
        Some(r) => r,
    };
    let mut subj: Vec<String> = rd.urls.iter().map(|u| format!("URL（{}）", u)).collect();
    subj.extend(rd.elements.iter().map(|(sel, m)| format!("要素 {}{}", sel, if m.is_empty() { String::new() } else { format!("（形 {}）", m) })));
    let mut parts = vec![format!("{}を、{}から読む", rd.subject, subj.join("・"))];
    if !rd.value_rows.is_empty() {
        parts.push(format!("値は、{} の行から読む", rd.value_rows.join("・")));
    }
    if !rd.lookup.is_empty() {
        parts.push(format!("読めなければ対応表（{}件：{}）から引く", rd.lookup.len(), rd.lookup.iter().map(|(k, v)| format!("{}→{}", k, v)).collect::<Vec<_>>().join("、")));
    }
    parts.join("。")
}

fn show_in_with(e: &Expr, least: i32, f: &dyn Fn(&Expr) -> String) -> String {
    let s = f(e);
    if prec(e) < least {
        format!("({})", s)
    } else {
        s
    }
}

fn names_text(b: &Binder) -> String {
    if b.names.len() == 1 {
        b.names[0].clone()
    } else {
        format!("({})", b.names.join(", "))
    }
}

fn ja(s: &str) -> String {
    show_ja(s).map(|x| x.to_string()).unwrap_or_else(|| s.to_string())
}

/// 処理の中身に出す式。コードの書き方を、読める言葉にする
pub fn tell(e: &Expr) -> String {
    let t = |x: &Expr| tell(x);
    match e {
        Expr::Call { n, args } if n == "round" && args.len() == 3 => {
            let mode = ja(&show(&args[1]));
            let unit = ja(&show(&args[2]));
            format!("丸め（{}、{} の単位に{}）", t(&args[0]), unit, mode)
        }
        Expr::Call { n, args } if (n == "min" || n == "max") && !args.is_empty() => {
            format!("{}ほう（{}）", if n == "min" { "小さい" } else { "大きい" }, args.iter().map(t).collect::<Vec<_>>().join("、"))
        }
        Expr::Call { n, args } if n == "abs" && args.len() == 1 => format!("絶対値（{}）", t(&args[0])),
        Expr::Call { n, args } if n == "len" && args.len() == 1 => format!("長さ（{}）", t(&args[0])),
        Expr::Call { n, args } => format!("{}({})", n, args.iter().map(t).collect::<Vec<_>>().join(", ")),
        Expr::Not { e } => format!("{} でない", show_in_with(e, 9, &tell)),
        Expr::Cond { c, a, b } => format!("{} なら {}、そうでなければ {}", t(c), t(a), t(b)),
        Expr::Tuple { items } => format!("({})", items.iter().map(t).collect::<Vec<_>>().join(", ")),
        Expr::Attr { e, n } => format!("{}.{}", t(e), n),
        Expr::Index { d, e, scopes } => format!("{}{}[{}]", d, if scopes.is_empty() { String::new() } else { format!("（範囲 {}）", scopes.join("、")) }, t(e)),
        Expr::Make { n, fields, .. } => format!("{} {{ {} }}", n, fields.iter().map(|(k, x)| format!("{}: {}", k, t(x))).collect::<Vec<_>>().join(", ")),
        Expr::Bin { op, l, r, dflt } => {
            let o = match op.as_str() {
                "in" => "が次に入っている：",
                "==" => "=",
                "!=" => "≠",
                "<=" => "≦",
                ">=" => "≧",
                x => x,
            };
            let p = prec(e);
            format!("{} {} {}", show_in_with(l, p, &tell), o, show_in_with(r, p + 1, &tell)) + &dflt.as_ref().map_or(String::new(), |d| format!("（割る数が 0 なら {}）", t(d)))
        }
        Expr::Pred { e, kind } => format!("{} が {}", show_in_with(e, 5, &tell), kind),
        Expr::Implies { l, r } => format!("{} なら {}", t(l), t(r)),
        Expr::SetB { elem, binder, cond } | Expr::SumE { elem, binder, cond } => {
            let only = cond.as_ref().map_or(String::new(), |c| format!(" のうち {} のもの", t(c)));
            let head = if matches!(e, Expr::SumE { .. }) { "合計" } else { "集まり" };
            format!("{}〔{} の各 {}{}について： {}〕", head, t(&binder.src), names_text(binder), only, t(elem))
        }
        _ => show(e),
    }
}

fn ret_text(r: &Return) -> String {
    format!("結果 {}", r.variant) + &if r.args.is_empty() { String::new() } else { format!("（{}）", r.args.iter().map(tell).collect::<Vec<_>>().join(", ")) }
}

/// 処理の中身を、上から順の手順にする（1 行 = 1 文。入れ子は行頭の「　」の数）
pub fn step_lines(body: &[Stmt], depth: usize) -> Vec<String> {
    let pad = "　".repeat(depth);
    let mut out: Vec<String> = vec![];
    for st in body {
        match st {
            Stmt::Return(r) => out.push(format!("{}{}", pad, ret_text(r))),
            Stmt::Assign(a) => out.push(format!("{}{} = {}{}", pad, a.name, tell(&a.e), a.absent.as_ref().map_or(String::new(), |r| format!("。無ければ {}", ret_text(r))))),
            Stmt::Create(c) => out.push(format!("{}{} = {} を新しく作る（{}）", pad, c.name, c.data, c.fields.iter().map(|(n, x)| format!("{}: {}", n, tell(x))).collect::<Vec<_>>().join(", "))),
            Stmt::Delete(d) => out.push(format!("{}{} を消す", pad, d.var)),
            Stmt::If(i) => {
                if i.body.len() == 1 && matches!(i.body[0], Stmt::Return(_)) && i.els.is_none() {
                    if let Stmt::Return(r) = &i.body[0] {
                        out.push(format!("{}{} なら {}", pad, tell(&i.cond), ret_text(r)));
                    }
                } else {
                    out.push(format!("{}{} なら：", pad, tell(&i.cond)));
                    out.extend(step_lines(&i.body, depth + 1));
                    if let Some(els) = &i.els {
                        out.push(format!("{}そうでなければ：", pad));
                        out.extend(step_lines(els, depth + 1));
                    }
                }
            }
            Stmt::SetField(s) => {
                let mut text = if s.op == "add" || s.op == "remove" {
                    format!("{}.{} {}： {}", s.var, s.f, if s.op == "add" { "に 1 つ足す" } else { "から 1 つ外す" }, tell(&s.e))
                } else if (s.op == "+=" || s.op == "-=") && matches!(s.e, Expr::Bin { .. }) {
                    let r = if let Expr::Bin { r, .. } = &s.e { tell(r) } else { String::new() };
                    format!("{}.{} {}： {}", s.var, s.f, if s.op == "+=" { "に足す" } else { "から引く" }, r)
                } else {
                    format!("{}.{} = {}", s.var, s.f, tell(&s.e))
                };
                if let Some(g) = &s.guard {
                    text += if g.0 == "saturating" { "（範囲の端で止める）" } else { "（範囲は確かめない）" };
                }
                out.push(format!("{}{}", pad, text));
            }
            Stmt::Update(u) => {
                let b = &u.binder;
                let op = match u.op.as_str() {
                    "+=" => Some("に足す"),
                    "-=" => Some("から引く"),
                    _ => None,
                };
                let head = format!("{} の各 {} について{}、", tell(&b.src), names_text(b), u.cond.as_ref().map_or(String::new(), |c| format!("（{} のものだけ）", tell(c))));
                out.push(format!(
                    "{}{}{}",
                    pad,
                    head,
                    match op {
                        Some(o) => format!("{} {}： {}", tell(&u.target), o, tell(&u.value)),
                        None => format!("{} {} {}", tell(&u.target), u.op, tell(&u.value)),
                    }
                ));
            }
            Stmt::Request(r) => out.push(format!(
                "{}外部作用 {}（{}）を要求する{}",
                pad,
                r.effect,
                r.args.iter().map(tell).collect::<Vec<_>>().join(", "),
                r.idem.as_ref().map_or(String::new(), |i| format!("（冪等キー = {}）", tell(i)))
            )),
            Stmt::ForEach(f) => {
                out.push(format!("{}{} の各 {} について：", pad, tell(&f.binder.src), names_text(&f.binder)));
                out.extend(step_lines(&f.body, depth + 1));
            }
        }
    }
    out
}

/// 処理の中身を、図（フローチャート）にするための木にする
pub fn step_tree(body: &[Stmt]) -> Vec<Value> {
    let mut out = vec![];
    for st in body {
        let line = step_lines(std::slice::from_ref(st), 0)[0].trim().to_string();
        match st {
            Stmt::Return(_) => out.push(json!(["結果", line.chars().skip(3).collect::<String>()])),
            Stmt::Assign(a) if a.absent.is_some() => {
                let sep = "。無ければ 結果 ";
                let (head, r) = match line.find(sep) {
                    Some(i) => (line[..i].to_string(), line[i + sep.len()..].to_string()),
                    None => (line.clone(), String::new()),
                };
                out.push(json!(["無ければ", head, r]));
            }
            Stmt::If(i) => out.push(json!(["もし", tell(&i.cond), step_tree(&i.body), i.els.as_ref().map(|e| step_tree(e))])),
            Stmt::ForEach(f) => out.push(json!(["各", line.trim_end_matches('：'), step_tree(&f.body)])),
            _ => out.push(json!(["する", line])),
        }
    }
    out
}

pub const ACTOR_NOTE: &str = "役割は、前に置くプロキシが本人を確かめ、ヘッダ X-Geas-Actor に載せて渡します。ヘッダの無い呼び出しは 401、役割が合わない呼び出しは 403 で断ります";

pub const POST_NOTE: &str = "読むだけの問い合わせは GET で受けるのが決まりです。ここにあるものは、条件を URL に載せられない事情があり、POST で受けます。条件は本文の {\"q\": {…}} に書き、保存しません。答えはその場で返します";

pub const SCOPE_NOTE: &str = "データ.範囲1.範囲2 と並べると、すべての範囲に入る行だけになります。「呼んだ本人のID」は、前に置くプロキシが本人を確かめ、ヘッダ X-Geas-Actor-Id に載せて渡します。呼ぶ側は渡せません。ヘッダの無い呼び出しは 401 で断ります。処理が データ.範囲[ID] で 1 件を引くときは、範囲の外の行を「無い」として扱います";

pub fn scope_text(p: &Program, s: &Scope) -> String {
    let mut parts: Vec<String> = s.conds.iter().map(|g| g.iter().map(|(f, op, st)| format!("{} {} {}", p.show_names(f), op, p.show_names(st))).collect::<Vec<_>>().join(" または ")).collect();
    for (f, op, a) in &s.filters {
        let word = match op.as_str() {
            "一致" => "と同じ",
            "以上" => "以上",
            "以下" => "以下",
            o => o,
        };
        parts.push(format!("{} が、引数 {} {}", p.show_names(f), p.show_names(a), word));
    }
    if !s.caller.is_empty() {
        parts.push(format!("{} が、呼んだ本人のID と同じ", p.show_names(&s.caller)));
    }
    parts.join(" かつ ")
}

/// 仕様書の「役割」の表：(役割, ヘッダに載せる名前, 呼べる URL)
pub fn actor_rows(p: &Program) -> Vec<Value> {
    if p.actors.is_empty() {
        return vec![];
    }
    let who = rest::route_actors(p);
    let inner = rest::internal_targets(p);
    let urls: Vec<(String, Option<Vec<String>>)> =
        rest::routes(p).into_iter().filter(|r| !inner.contains(&r.target)).map(|r| (format!("{} {}", r.method, rest::shown_path(p, &r.path, &r.kind, &r.target)), who.get(&r.target).cloned())).collect();
    let mut out: Vec<Value> = vec![];
    for (n, _) in &p.actors {
        let us: Vec<&str> = urls.iter().filter(|(_, w)| w.as_ref().map_or(false, |w| !w.is_empty() && w.contains(n))).map(|(u, _)| u.as_str()).collect();
        out.push(json!([p.show_names(n), n, if us.is_empty() { "なし".to_string() } else { us.join("、") }]));
    }
    let any: Vec<&str> = urls.iter().filter(|(_, w)| w.as_ref() == Some(&vec![rest::ANYONE.to_string()])).map(|(u, _)| u.as_str()).collect();
    out.push(json!(["誰でも", "（要らない）", if any.is_empty() { "なし".to_string() } else { any.join("、") }]));
    out
}

pub const STOPS_NOTE: &str = "処理は必ず止まります。繰り返しは、件数の上限がある集まりを回すものだけです。止まらないことがありうるのは、「人の保証」に出ている根の言語のライブラリ（library）の中だけです。";

pub fn build(ck: &Checker) -> Spec {
    let p = ck.p;
    let raw = build_raw(ck);
    let names = raw.get("名前").cloned().unwrap_or(Value::Null);
    let mut spec: Spec = match show_all(p, &Value::Object(raw)) {
        Value::Object(m) => m,
        _ => unreachable!(),
    };
    let inner = rest::internal_targets(p);
    let rs = rest::routes(p);
    let route_rows: Vec<Value> = rs
        .iter()
        .map(|r| {
            let own = if ck.infos.get(&r.target).map_or(false, |i| i.caller) && !inner.contains(&r.target) { "（本人の分だけ）" } else { "" };
            json!([
                r.method,
                rest::shown_path(p, &r.path, &r.kind, &r.target),
                r.kind,
                p.show_names(&r.target),
                r.statuses.iter().map(|(v, c)| format!("{}→{}", p.show_names(v), c)).collect::<Vec<_>>().join(", "),
                if inner.contains(&r.target) { INTERNAL } else { OPEN },
                rest::actors_text(p, &r.target, &inner) + own
            ])
        })
        .collect();
    spec.insert("ルート".into(), Value::Array(route_rows));
    spec.insert("役割".into(), Value::Array(actor_rows(p)));
    let posts = rest::post_queries(p);
    spec.insert(
        "POSTの問い合わせ".into(),
        Value::Array(rs.iter().filter(|r| r.kind == "index" && posts.contains_key(&r.target)).map(|r| json!([r.path, p.show_names(&r.target), posts[&r.target]])).collect()),
    );
    let show = |s: &str| p.show_names(s);
    let scopes: Vec<Value> = p
        .scopes
        .iter()
        .map(|s| {
            let head = show(&s.name) + &if s.params.is_empty() { String::new() } else { format!("（{}）", s.params.iter().map(|(n, _)| show(n)).collect::<Vec<_>>().join("、")) };
            let mut users: Vec<String> = p.queries.iter().filter(|q| q.scopes.contains(&s.name)).map(|q| show(&q.name)).collect();
            users.extend(p.processes.iter().filter(|x| ck.infos[&x.name].scopes.contains(&s.name)).map(|x| show(&x.name)));
            json!([head, show(&s.data), scope_text(p, s), if users.is_empty() { "なし".to_string() } else { users.join("、") }])
        })
        .collect();
    spec.insert("範囲".into(), Value::Array(scopes));
    let screens: Vec<Value> = spec["画面"]
        .as_array()
        .unwrap()
        .iter()
        .zip(p.screens.iter())
        .map(|(x, sc)| {
            let mut a = x.as_array().unwrap().clone();
            a.pop();
            a.push(Value::String(rest::screen_url(p, sc)));
            Value::Array(a)
        })
        .collect();
    spec.insert("画面".into(), Value::Array(screens));
    spec.insert("名前".into(), names);
    spec
}

fn migration_row(version: i128, s: &crate::check::defs::Step) -> Value {
    let to = |a: &Option<MigVal>| match a {
        Some(MigVal::To(x)) => x.clone(),
        _ => String::new(),
    };
    let v = format!("版 {}", version);
    match s.op.as_str() {
        "rename_entity" => json!([v, "実体の名前を変える", s.dn, format!("行はそのまま、実体 {} に移る", to(&s.arg))]),
        "drop_entity" => json!([v, "実体を消す", s.dn, "行はすべて捨てる"]),
        "rename" => json!([v, "名前を変える", format!("{}.{}", s.dn, s.fname), format!("値はそのまま、項目 {} に移る", to(&s.arg))]),
        "drop" => json!([v, "項目を消す", format!("{}.{}", s.dn, s.fname), "値は捨てる"]),
        _ => {
            let val = match &s.arg {
                Some(MigVal::Null) | None => "空（無い）".to_string(),
                Some(MigVal::Str(x)) => quote_str(x),
                Some(MigVal::Bool(b)) => if *b { "true" } else { "false" }.to_string(),
                Some(MigVal::Int(i)) => i.to_string(),
                Some(MigVal::Name(x)) | Some(MigVal::Ratio(x)) | Some(MigVal::To(x)) => x.clone(),
            };
            json!([v, "項目を足す", format!("{}.{}", s.dn, s.fname), format!("前からある行へ入れる値： {}", val)])
        }
    }
}

fn sorted<T: Ord + Clone>(it: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut v: Vec<T> = it.into_iter().collect();
    v.sort();
    v
}

pub fn build_raw(ck: &Checker) -> Spec {
    let p = ck.p;
    let mut spec: Spec = Map::new();
    spec.insert("アプリ".into(), if p.title.is_empty() { json!([]) } else { json!([["アプリの名前", p.title]]) });
    let mut data: Vec<Value> = vec![];
    for d in &p.data {
        for f in &d.fields {
            data.push(json!([d.name, f.name, show_type(&f.r#type).unwrap_or_default(), if f.r#type.optional { "" } else { "○" }, if f.unique { "○" } else { "" }]));
        }
    }
    for d in &p.data {
        for f in &d.derived {
            data.push(json!([d.name, f.name, format!("{}（保存しない。= {}）", show_type(&f.r#type).unwrap_or_default(), f.expr.as_ref().map(show).unwrap_or_default()), "○", ""]));
        }
    }
    spec.insert("データ".into(), Value::Array(data));
    let (mut states, mut trans, mut rejects): (Vec<Value>, Vec<Value>, Vec<Value>) = (vec![], vec![], vec![]);
    for d in &p.data {
        for f in &d.fields {
            if f.r#type.kind == "名前" {
                if let Some(s) = f.r#type.target.as_ref().and_then(|t| ck.states.get(t)) {
                    if s.plain {
                        continue;
                    }
                    states.push(json!([d.name, s.initial, "初期"]));
                    states.extend(s.middle.iter().map(|x| json!([d.name, x, "中間"])));
                    states.extend(s.final_.iter().map(|x| json!([d.name, x, "終了"])));
                    rejects.extend(s.rejects.iter().map(|(a, b, why)| json!([d.name, a, b, why])));
                }
            }
        }
    }
    for proc in &p.processes {
        for (dt, a, b) in sorted(ck.infos[&proc.name].transitions.iter().cloned()) {
            trans.push(json!([dt, a, b, proc.name]));
        }
    }
    spec.insert("状態".into(), Value::Array(states));
    spec.insert("遷移".into(), Value::Array(trans));
    spec.insert("拒否".into(), Value::Array(rejects));

    let mut routes: IndexMap<String, IndexMap<String, String>> = IndexMap::new();
    for fl in &p.flows {
        for (proc, rs) in &fl.entries {
            let e = routes.entry(proc.clone()).or_default();
            for (v, ds) in route_map(rs) {
                let t = if ds.len() == 1 { ja_dest(&ds[0]) } else { format!("並列［{}］", ds.iter().map(|x| ja_dest(x)).collect::<Vec<_>>().join(" ＋ ")) };
                e.insert(v, t);
            }
        }
    }
    spec.insert("流れ".into(), Value::Array(p.flows.iter().flat_map(|fl| fl.entries.iter().map(move |(proc, _)| json!([fl.name, proc]))).collect()));
    let mut procs: Vec<Value> = vec![];
    let mut roundings_all: Vec<(String, String, String)> = vec![];
    for proc in &p.processes {
        let info = &ck.infos[&proc.name];
        let rdef = ck.results.get(&proc.result);
        let rs: Vec<Value> = rdef
            .map(|r| r.variants.iter().map(|(v, _)| json!([v, routes.get(&proc.name).and_then(|m| m.get(v)).cloned().unwrap_or_else(|| "（未定義）".to_string())])).collect())
            .unwrap_or_default();
        let mut phys_rows: Vec<Value> = proc.inputs.iter().filter(|(_, t)| t.kind == "整数").map(|(n, t)| json!([n, phys(t)])).collect();
        phys_rows.extend(proc.inputs.iter().filter(|(_, t)| t.kind == "写像" && t.value.as_ref().map_or(false, |v| v.kind == "整数" || v.kind == "金額")).map(|(n, t)| json!([format!("{}.値", n), phys(t.value.as_ref().unwrap())])));
        let roundings = sorted(info.roundings.iter().cloned().collect::<IndexSet<_>>());
        for (m, u) in &roundings {
            roundings_all.push((proc.name.clone(), m.clone(), u.clone()));
        }
        procs.push(json!({
            "処理": proc.name,
            "入力": input_names(&proc.inputs),
            "読む": sorted(info.reads()),
            "書く": sorted(info.writes.iter().cloned()),
            "結果": rs,
            "手順": step_lines(&proc.body, 0),
            "図": step_tree(&proc.body),
            "外部": sorted(info.effects.iter().map(|(e, k)| format!("{}(冪等キー={})", e, k))),
            "参照": proc.refs.iter().map(|(n, t)| json!([n, show_type(t).unwrap_or_default()])).collect::<Vec<_>>(),
            "採番": sorted(info.new_ids.iter().cloned().collect::<IndexSet<_>>()),
            "物理数": phys_rows,
            "丸め": roundings.iter().map(|(m, u)| json!([m, u])).collect::<Vec<_>>(),
        }));
    }
    spec.insert("処理".into(), Value::Array(procs));
    spec.insert("問い合わせ".into(), Value::Array(p.queries.iter().map(|q| query_rows(ck, q)).collect()));
    spec.insert("トリガ".into(), Value::Array(p.triggers.iter().flat_map(|tg| tg.causes.iter().map(move |(k, key, d)| json!([tg.name, format!("{} {}", ja_kind(k), fmt_cause(k, key)), d]))).collect()));
    spec.insert("応答".into(), Value::Array(p.replies.iter().map(|rp| json!([rp.name, input_names(&rp.params).join(", ")])).collect()));
    spec.insert("合流".into(), Value::Array(p.effects.iter().filter_map(|e| e.joins.as_ref().filter(|j| !j.is_empty()).map(|j| json!([e.name, j]))).collect()));
    let reps: IndexMap<&String, &ReplyDef> = p.replies.iter().map(|r| (&r.name, r)).collect();
    let mut mock: Vec<Value> = vec![];
    for e in &p.effects {
        if let Some(rn) = e.replies.as_ref().filter(|s| !s.is_empty()) {
            if let Some(rp) = reps.get(rn) {
                let outs: Vec<(String, Vec<String>)> = if rp.outcomes.is_empty() { vec![("平常".to_string(), rp.params.iter().map(|(n, _)| n.clone()).collect())] } else { rp.outcomes.clone() };
                let t = outs.iter().map(|(on, g)| format!("{}{}", on, if g.is_empty() { String::new() } else { format!("（{}）", g.join(", ")) })).collect::<Vec<_>>().join(" ／ ");
                mock.push(json!([e.name, rn, t]));
            }
        }
    }
    spec.insert("模擬".into(), Value::Array(mock));
    let mut rules: Vec<Value> = vec![];
    for f in &p.rules {
        for (conds, val) in &f.rows {
            let c = conds.iter().map(|(pa, v)| format!("{} = {}", pa, v)).collect::<Vec<_>>().join(" かつ ");
            let vs = match val {
                IntOrStr::Int(i) => i.to_string(),
                IntOrStr::Str(s) => s.clone(),
            };
            rules.push(json!([f.slot, if c.is_empty() { "上のどれでもない".to_string() } else { c }, vs]));
        }
    }
    spec.insert("決め方".into(), Value::Array(rules));
    spec.insert("外部作用".into(), Value::Array(p.effects.iter().map(|e| json!([e.name, e.params.iter().map(|(n, t)| format!("{}: {}", n, show_type(t).unwrap_or_default())).collect::<Vec<_>>().join(", ")])).collect()));
    let tk_text = |tk: &Vec<(String, String)>| tk.iter().map(|(n, path)| format!("（{}←{}）", n, path)).collect::<String>();
    let mut comm: Vec<Value> = vec![];
    for e in &p.effects {
        if let Some(h) = &e.http {
            let body = h.body.iter().map(|(k, n)| if k == n { k.clone() } else { format!("{}←{}", k, n) }).collect::<Vec<_>>().join("、");
            let mut st = h.statuses.iter().map(|(lo, hi, o, tk)| format!("{} → {}{}", if lo == hi { lo.to_string() } else { format!("{}〜{}", lo, hi) }, o, tk_text(tk))).collect::<Vec<_>>().join(" ／ ");
            if let Some((o, tk)) = &h.other {
                st += &format!(" ／ その他 → {}{}", o, tk_text(tk));
            }
            comm.push(json!([e.name, format!("{} {}", h.method, h.url), h.bearer.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "なし".to_string()), if body.is_empty() { "なし".to_string() } else { body }, st]));
        }
    }
    spec.insert("通信".into(), Value::Array(comm));
    let mut folders: Vec<Value> = vec![];
    if let Some((path, _)) = &p.data_folder {
        folders.push(json!(["データ用フォルダ", format!("パス {}", path)]));
    }
    for (n, _, es, _) in &p.paths {
        for (os_name, path, _) in es {
            folders.push(json!([format!("パス {}（{}）", n, if os_name.is_empty() { "相対：exe のあるフォルダが起点" } else { os_name }), path]));
        }
    }
    spec.insert("データ用フォルダ".into(), Value::Array(folders));
    let mut files: Vec<Value> = vec![];
    for e in &p.effects {
        if let Some(f) = &e.file {
            let op = match f.op.as_str() {
                "write" => "新規に書く（すでにあれば書かない）",
                "overwrite" => "書く（あれば置き換える）",
                "read" => "読む",
                _ => "消す",
            };
            let rows = f.rows.iter().map(|(k, (o, tk))| format!("{} → {}{}", k, o, tk.iter().map(|(n, s)| format!("（{}←{}）", n, s)).collect::<String>())).collect::<Vec<_>>().join(" ／ ");
            files.push(json!([e.name, op, format!("引数 {} の場所の下の {}", f.dir, f.rest), rows]));
        }
    }
    spec.insert("ファイル".into(), Value::Array(files));
    let mut res: Vec<Value> = vec![];
    if let Some(r) = &p.resident {
        res.push(json!(["アイコンの説明", if r.tooltip.is_empty() { "アプリ名" } else { r.tooltip.as_str() }]));
        res.push(json!(["始めかた", if r.start == "shown" { "窓を開いて始める" } else { "アイコンだけで始める（窓は隠れている）" }]));
        res.push(json!(["左クリック", if r.click == "toggle" { "窓を開閉する" } else { "右クリックと同じメニューを出す" }]));
        res.push(json!(["右クリックのメニュー「開く」", "窓を出す（言語が常に足す）"]));
        for (label, kind, target, _) in &r.menu {
            if kind == "separator" {
                res.push(json!(["　――――", "区切り線"]));
            } else {
                res.push(json!([format!("右クリックのメニュー「{}」", label), if kind == "screen" { format!("画面 {} を開く", target) } else { format!("出来事 {} を起こす", target) }]));
            }
        }
        res.push(json!(["右クリックのメニュー「終了」", "アプリを終える（言語が常に足す）"]));
    }
    spec.insert("常駐".into(), Value::Array(res));
    let mut evs: Vec<Value> = vec![];
    for ev in &p.events {
        for (k, q, d) in &ev.steps {
            let on = match ev.on.as_str() {
                "online" => "つながりが戻ったとき: ",
                "opened" => "アプリを開いたとき: ",
                _ => "",
            };
            evs.push(json!([ev.name, format!("{}{}", on, if k == "各" { format!("各 {}", q) } else { "外から".to_string() }), d]));
        }
    }
    spec.insert("出来事".into(), Value::Array(evs));
    spec.insert(
        "画面".into(),
        Value::Array(
            p.screens
                .iter()
                .enumerate()
                .map(|(i, sc)| {
                    json!([
                        sc.name,
                        sc.shows.join(", "),
                        sc.fields.iter().map(|f| format!("{}{}", f, sc.field_from.get(f).map_or(String::new(), |x| format!("←{}", x)))).collect::<Vec<_>>().join(", "),
                        sc.params.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>().join(", "),
                        if i == 0 || sc.entry { "○" } else { "" },
                        sc.slots.join(", "),
                        rest::screen_url(p, sc)
                    ])
                })
                .collect(),
        ),
    );
    let mut trs: Vec<Value> = vec![];
    for sc in &p.screens {
        for t in &sc.opens {
            trs.push(json!([sc.name, "行を選ぶ", "", t]));
        }
    }
    for sc in &p.screens {
        for (dn, rs) in &sc.actions {
            for (v, dest) in rs {
                let d = match dest.as_str() {
                    "stay" => "そのまま",
                    "back" => "戻る",
                    o => o,
                };
                trs.push(json!([sc.name, dn, v, d]));
            }
        }
    }
    spec.insert("画面遷移".into(), Value::Array(trs));
    let deleted: IndexSet<&String> = ck.infos.values().flat_map(|i| i.deletes.iter()).collect();
    let mut dels: Vec<Value> = vec![];
    for d in &p.data {
        if d.keep || !d.removable.is_empty() || !d.on_delete.is_empty() || deleted.contains(&d.name) {
            let a = if d.keep {
                "削除させない".to_string()
            } else if !d.removable.is_empty() {
                d.removable.join(", ")
            } else if deleted.contains(&d.name) {
                "消せる".to_string()
            } else {
                String::new()
            };
            let b = d
                .on_delete
                .iter()
                .map(|(s, f, pol)| {
                    format!(
                        "{}.{}→{}",
                        s,
                        f,
                        match pol.as_str() {
                            "detach" => "参照を空に",
                            "cascade" => "参照元も消す",
                            _ => "そのまま残す",
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            dels.push(json!([d.name, a, b]));
        }
    }
    spec.insert("削除".into(), Value::Array(dels));
    let mut guar: Vec<[String; 5]> = vec![];
    for (n, t, name) in p.processes.iter().flat_map(|x| x.inputs.iter().map(move |(n, t)| (n, t, &x.name))).chain(p.queries.iter().flat_map(|x| x.inputs.iter().map(move |(n, t)| (n, t, &x.name)))) {
        if let Some((lv, why)) = &t.trust {
            let c = match lv.as_str() {
                "verify" => "検証ビルドだけ確かめる",
                "assume" => "確かめない",
                _ => "確かめない（危険：未検査の raw も受ける）",
            };
            guar.push([name.clone(), n.clone(), show_type(t).unwrap_or_default(), c.to_string(), why.clone()]);
        }
    }
    // 処理 → 問い合わせの順を保つ（上の chain は 処理 の入力、問い合わせの入力の順）
    spec.insert("保存の形".into(), Value::Array(ck.migration_steps().iter().map(|s| migration_row(s.ver, s)).collect()));
    spec.insert(
        "保存先".into(),
        Value::Array(
            p.data
                .iter()
                .filter_map(|d| {
                    let st = d.store.as_ref().filter(|s| !s.is_empty())?;
                    let iso = p.isolations.get(st)?;
                    let who = if iso.0 == "process" { "このアプリのプロセスの中だけ".to_string() } else { "保存先が直列にする".to_string() };
                    Some(json!([d.name, st, who + &if iso.0 == "store" { format!("（{}）", iso.1) } else { String::new() }]))
                })
                .collect(),
        ),
    );
    spec.insert("動かす前提".into(), Value::Array(crate::envreq::rows(p).iter().map(|r| json!(r)).collect()));
    for (where_, (who, kind, why, _)) in p.isolations.iter() {
        if who == "process" {
            guar.push([format!("（保存先 {}）", where_), "リクエストの直列化".into(), "このアプリのプロセスの中だけ".into(), "geas のサーバーが直列にする（別プロセスとの関係は約束しない）".into(), "プロセスの外は保証の対象外".into()]);
        } else if !VERIFIED_STORES.contains(&kind.as_str()) {
            guar.push([format!("（保存先 {}）", where_), "リクエストの直列化".into(), format!("保存先 {} に任せる", kind), "保存先が、1 リクエストを直列に並べた場合と同じ結果にする".into(), why.clone()]);
        }
    }
    for x in &p.processes {
        let info = &ck.infos[&x.name];
        for (target, op, mode, why) in &info.guards {
            let (d, f) = target.split_once('.').unwrap_or((target, ""));
            let ft = ck.data[d].fields.iter().find(|y| y.name == f).map(|y| show_type(&y.r#type).unwrap_or_default()).unwrap_or_default();
            guar.push([x.name.clone(), format!("{} {}", target, op), ft, if mode == "saturating" { "範囲の端で止まる".into() } else { "確かめない（届くと異常で止まる）".into() }, why.clone()]);
        }
        for (expr, level, why) in &info.calc_trusts {
            let c = match level.as_str() {
                "check" => "毎回確かめる（溢れたら異常で止まる）",
                "verify" => "検証ビルドだけ確かめる（通常ビルドでは溢れた値がそのまま使われる）",
                _ => "確かめない（溢れた値がそのまま使われる）",
            };
            guar.push([x.name.clone(), format!("途中の計算 {}", expr), "収まると示せていない".into(), c.into(), why.clone()]);
        }
    }
    spec.insert(
        "拡張".into(),
        Value::Array(
            p.screens
                .iter()
                .filter(|sc| !sc.pages.is_empty() && !sc.shows.is_empty() && ck.queries.contains_key(&sc.shows[0]))
                .map(|sc| json!([sc.name, sc.pages.join(", "), sc.shows.join(", "), ck.queries[&sc.shows[0]].inputs.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>().join(", "), read_text(&sc.read)]))
                .collect(),
        ),
    );
    spec.insert("画面デザイン".into(), Value::Array(p.screens.iter().flat_map(|sc| sc.design.iter().map(move |(k, v)| json!([sc.name, k, v]))).collect()));
    spec.insert("不変条件".into(), Value::Array(ck.invariant_status.iter().map(|(t, _)| json!(t)).collect()));
    let mut parts: Vec<Value> = vec![];
    for pt in &p.parts {
        if !pt.external() || ck.used_libs.contains(&pt.name) {
            let g = match &pt.body {
                Some(b) => format!("{}= {}", pt.requires.iter().map(|(c, why)| format!("{} でなければ断る（{}）。 ", show(c), why)).collect::<String>(), show(b)),
                None => pt.guarantee.as_ref().map(show).unwrap_or_else(|| "戻り値の範囲".to_string()),
            };
            parts.push(json!([pt.name, pt.params.iter().map(|(n, t)| format!("{}: {}", n, show_type(t).unwrap_or_default())).collect::<Vec<_>>().join(", "), show_type(&pt.ret).unwrap_or_default(), g]));
        }
    }
    let ledger = crate::partcheck::load_ledger(ck.code_path.as_deref());
    for pt in &p.parts {
        if pt.external() && ck.used_libs.contains(&pt.name) {
            let (krate, version, expr, level, why) = pt.lib.clone().unwrap();
            let where_ = if krate == "std" { krate.clone() } else { format!("{} {}", krate, version) };
            let ex = pt.examples.iter().map(|(args, want, _)| format!("{}({}) == {}", pt.name, args.iter().map(show).collect::<Vec<_>>().join(", "), show(want))).collect::<Vec<_>>().join("、");
            let lv = match level.as_str() {
                "check" => "戻り値を毎回確かめる",
                "verify" => "戻り値を検証ビルドだけ確かめる",
                _ => "戻り値を確かめない",
            };
            guar.push([
                format!("（ライブラリ {}）", pt.name),
                format!("Rust の {} を呼ぶ: {}。具体例: {}", where_, expr, ex),
                show_type(&pt.ret).unwrap_or_default(),
                format!("{}（panic は異常で止まる。無限ループ・メモリ枯渇は防げない）。{}", lv, crate::partcheck::status_text(pt, &ledger)),
                why,
            ]);
        }
    }
    spec.insert("人の保証".into(), Value::Array(guar.iter().map(|r| json!(r)).collect()));
    let levels = guarantee_levels(ck, &guar);
    spec.insert("保証の強さ".into(), Value::Array(levels.iter().map(|r| json!(r)).collect()));
    spec.insert("部品".into(), Value::Array(parts));
    let mut spec2: Spec = Map::new();
    // 項目の並びは Python 版の挿入順に合わせる（部品 → 保証の強さ の順にしない）
    for k in ["アプリ", "データ", "状態", "遷移", "拒否", "流れ", "処理", "問い合わせ", "トリガ", "応答", "合流", "模擬", "決め方", "外部作用", "通信", "データ用フォルダ", "ファイル", "常駐", "出来事", "画面", "画面遷移", "削除", "人の保証", "保存の形", "保存先", "動かす前提", "拡張", "画面デザイン", "不変条件", "部品", "保証の強さ"] {
        if let Some(v) = spec.get(k) {
            spec2.insert(k.to_string(), v.clone());
        }
    }
    let mut spec = spec2;
    spec.insert("指定".into(), Value::Array(roundings_all.iter().map(|(x, m, u)| json!([format!("{}.丸め", x), format!("{}({})", ja(m), ja(u))])).collect()));
    spec.insert("名前".into(), Value::Array(p.labels.iter().map(|(n, ls)| json!([n, ls.iter().map(|(k, v)| format!("{}: {}", if k.is_empty() { "既定" } else { k }, v)).collect::<Vec<_>>().join(" / ")])).collect()));
    spec.insert(
        "実現".into(),
        Value::Array(p.data.iter().flat_map(|d| d.fields.iter().filter(|f| f.r#type.kind == "整数" || f.r#type.kind == "金額").map(move |f| json!([format!("{}.{}", d.name, f.name), phys(&f.r#type)]))).collect()),
    );
    spec
}

pub const LEVEL_PROVEN: &str = "検査器が示した";
pub const LEVEL_RUNTIME: &str = "実行時に毎回確かめる";
pub const LEVEL_VERIFY: &str = "検証ビルドだけ確かめる";
pub const LEVEL_STOP: &str = "範囲の端で止まる（人の保証）";
pub const LEVEL_TRUST: &str = "確かめない（人の保証）";
pub const LEVEL_NOT: &str = "示せていない";
pub const LEVELS: [&str; 6] = [LEVEL_PROVEN, LEVEL_RUNTIME, LEVEL_VERIFY, LEVEL_STOP, LEVEL_TRUST, LEVEL_NOT];

pub fn level_of_check(text: &str) -> &'static str {
    if text.contains("範囲の端で止まる") {
        LEVEL_STOP
    } else if text.contains("毎回確かめる") {
        LEVEL_RUNTIME
    } else if text.contains("検証ビルドだけ") {
        LEVEL_VERIFY
    } else {
        LEVEL_TRUST
    }
}

/// (強さ, 対象, 内容)
pub fn guarantee_levels(ck: &Checker, guar: &[[String; 5]]) -> Vec<[String; 3]> {
    let mut rows: Vec<[String; 3]> = vec![];
    for (text, st) in &ck.invariant_status {
        let ok = st.iter().all(|(_, s)| s.starts_with("証明済み") || s == "対象外");
        rows.push([(if ok { LEVEL_PROVEN } else { LEVEL_NOT }).to_string(), "不変条件".to_string(), text.clone()]);
    }
    for [who, target, typ, check, why] in guar {
        rows.push([level_of_check(check).to_string(), format!("{} {}", who, target), format!("{}：{}（理由：{}）", typ, check, why)]);
    }
    let rank = |x: &str| LEVELS.iter().position(|l| *l == x).unwrap_or(0);
    rows.sort_by_key(|r| rank(&r[0]));
    rows
}

pub fn level_counts(rows: &[Value]) -> Vec<(String, usize)> {
    LEVELS.iter().filter_map(|lv| {
        let n = rows.iter().filter(|r| r[0].as_str() == Some(*lv)).count();
        if n > 0 { Some((lv.to_string(), n)) } else { None }
    }).collect()
}

pub fn flow_diagram(fl: &Flow) -> Vec<String> {
    let mut ids: IndexMap<String, String> = IndexMap::new();
    let mut out: Vec<String> = vec!["flowchart LR".to_string()];
    fn node(ids: &mut IndexMap<String, String>, out: &mut Vec<String>, key: &str, label: &str, shape: &str) -> String {
        if !ids.contains_key(key) {
            let id = format!("n{}", ids.len());
            ids.insert(key.to_string(), id.clone());
            out.push(format!("    {}{}", id, shape.replace("{}", &format!("\"{}\"", label))));
        }
        ids[key].clone()
    }
    fn dest(ids: &mut IndexMap<String, String>, out: &mut Vec<String>, d: &str) -> String {
        if d == "reply" {
            node(ids, out, "reply", "応答（reply）", "([{}])")
        } else if let Some(r) = d.strip_prefix("wait ") {
            node(ids, out, d, &format!("待つ {}", r), "[[{}]]")
        } else {
            node(ids, out, &format!("処理:{}", d), d, "[{}]")
        }
    }
    for (proc, routes) in &fl.entries {
        let src = node(&mut ids, &mut out, &format!("処理:{}", proc), proc, "[{}]");
        let rm = route_map(routes);
        let gw = if rm.len() > 1 {
            let g = node(&mut ids, &mut out, &format!("×:{}", proc), "×", "(({}))");
            out.push(format!("    {} --> {}", src, g));
            Some(g)
        } else {
            None
        };
        for (v, ds) in &rm {
            let a = gw.clone().unwrap_or_else(|| src.clone());
            if ds.len() > 1 {
                let par = node(&mut ids, &mut out, &format!("＋:{}:{}", proc, v), "＋", "(({}))");
                out.push(format!("    {} -- \"{}\" --> {}", a, v, par));
                // Python 版は、リスト内包表記の中で dest（箱の宣言を足す）を呼び終えてから、辺の行をまとめて足す
                let targets: Vec<String> = ds.iter().map(|d| dest(&mut ids, &mut out, d)).collect();
                for t in targets {
                    out.push(format!("    {} --> {}", par, t));
                }
            } else {
                let t = dest(&mut ids, &mut out, &ds[0]);
                out.push(format!("    {} -- \"{}\" --> {}", a, v, t));
            }
        }
    }
    out
}

fn rows<'a>(spec: &'a Spec, k: &str) -> &'a Vec<Value> {
    static EMPTY: Vec<Value> = Vec::new();
    spec.get(k).and_then(|v| v.as_array()).unwrap_or(&EMPTY)
}

fn cell(v: &Value, i: usize) -> String {
    match &v[i] {
        Value::String(s) => s.clone(),
        o => o.to_string(),
    }
}

fn join_strs(v: &Value) -> String {
    v.as_array().map(|a| a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect::<Vec<_>>().join(", ")).unwrap_or_default()
}

fn table(out: &mut Vec<String>, spec: &Spec, key: &str, n: usize) {
    for r in rows(spec, key) {
        out.push(format!("| {} |", (0..n).map(|i| cell(r, i)).collect::<Vec<_>>().join(" | ")));
    }
}

fn push_head(out: &mut Vec<String>, title: &str, cols: &str, n: usize, lead_blank: bool) {
    if lead_blank {
        out.push(String::new());
    }
    out.push(title.to_string());
    out.push(String::new());
    out.push(cols.to_string());
    out.push(format!("|{}", "---|".repeat(n)));
}

fn has(spec: &Spec, k: &str) -> bool {
    !rows(spec, k).is_empty()
}

pub fn render(ck: &Checker, spec: &Spec, name: &str) -> String {
    let p = ck.p;
    let mut out: Vec<String> = vec![format!("# 生成した仕様書（③）{}", name), String::new(), "コードから生成。人が直さない。◆の欄は plan の仕様書（①）と機械で突合する。".to_string(), String::new()];
    let app = rows(spec, "アプリ");
    if !app.is_empty() {
        out.push(format!("アプリの名前：{}", cell(&app[0], 1)));
        out.push(String::new());
    }
    push_head(&mut out, "## データ", "| データ | 項目 | ◆型・範囲 | ◆必須 | ◆一意 |", 5, false);
    table(&mut out, spec, "データ", 5);
    push_head(&mut out, "## 状態と遷移", "| データ | ◆状態 | ◆区分 |", 3, true);
    table(&mut out, spec, "状態", 3);
    out.push(String::new());
    out.push("| データ | ◆元 | ◆先 | ◆起こす処理 |".into());
    out.push("|---|---|---|---|".into());
    table(&mut out, spec, "遷移", 4);
    out.push(String::new());
    out.push("| データ | ◆元 | ◆先 | 拒否の理由 |".into());
    out.push("|---|---|---|---|".into());
    table(&mut out, spec, "拒否", 4);
    push_head(&mut out, "## 処理", "| 処理 | ◆入力 | ◆読む | ◆書く | ◆結果 → 行き先 | ◆外部作用 |", 6, true);
    for x in rows(spec, "処理") {
        let rs = x["結果"].as_array().unwrap().iter().map(|r| format!("{}→{}", cell(r, 0), cell(r, 1))).collect::<Vec<_>>().join(" / ");
        let ext = join_strs(&x["外部"]);
        out.push(format!("| {} | {} | {} | {} | {} | {} |", x["処理"].as_str().unwrap(), join_strs(&x["入力"]), join_strs(&x["読む"]), join_strs(&x["書く"]), rs, if ext.is_empty() { "なし".to_string() } else { ext }));
    }
    out.push(String::new());
    out.push("## 処理の中身（上から順に進む。「結果」に着いたら、そこで終わる）".into());
    out.push(String::new());
    out.push(STOPS_NOTE.into());
    for x in rows(spec, "処理") {
        out.push(String::new());
        out.push(format!("### {}", x["処理"].as_str().unwrap()));
        out.push(String::new());
        let refs = x["参照"].as_array().unwrap();
        if !refs.is_empty() {
            out.push(format!("外から決まる値（slot）：{}", refs.iter().map(|r| format!("{}: {}", cell(r, 0), cell(r, 1))).collect::<Vec<_>>().join("、")));
            out.push(String::new());
        }
        for (i, s) in x["手順"].as_array().unwrap().iter().enumerate() {
            out.push(format!("{}. {}", i + 1, s.as_str().unwrap()));
        }
    }
    for fl in &p.flows {
        out.push(String::new());
        out.push(format!("## フロー図 {}（×＝結果で行き先が分かれる、＋＝並列に起こす）", fl.name));
        out.push(String::new());
        out.push("```mermaid".into());
        out.extend(flow_diagram(fl).iter().map(|l| p.show_names(l)));
        out.push("```".into());
    }
    if has(spec, "問い合わせ") {
        push_head(&mut out, "## 問い合わせ（読むだけの処理）", "| 問い合わせ | ◆入力 | ◆読む | ◆条件 | ◆並び | ◆ページ |", 6, true);
        for x in rows(spec, "問い合わせ") {
            out.push(format!("| {} | {} | {} | {} | {} | {} |", x["問い合わせ"].as_str().unwrap(), join_strs(&x["入力"]), join_strs(&x["読む"]), join_strs(&x["条件"]), x["並び"].as_str().unwrap(), x["ページ"].as_str().unwrap()));
        }
    }
    push_head(&mut out, "## トリガ", "| トリガ | ◆要因 | ◆行き先 |", 3, true);
    table(&mut out, spec, "トリガ", 3);
    let sections: [(&str, &str, &str, usize); 0] = [];
    let _ = sections;
    if has(spec, "応答") {
        push_head(&mut out, "## 応答の中身（外から返ってくる内容。入出力型）", "| 応答 | ◆中身 |", 2, true);
        table(&mut out, spec, "応答", 2);
    }
    if has(spec, "決め方") {
        push_head(&mut out, "## slot の決め方（サーバーが決める値。上の行から順に、最初に当てはまった値になる）", "| slot | 条件 | 値 |", 3, true);
        table(&mut out, spec, "決め方", 3);
    }
    if has(spec, "模擬") {
        push_head(&mut out, "## 開発用の模擬（外部作用の返事。先頭が平常。開発用の操作パネルで切り替える）", "| 外部作用 | 返ってくる応答 | 結果の種類（返事に入る中身） |", 3, true);
        table(&mut out, spec, "模擬", 3);
    }
    if has(spec, "合流") {
        push_head(&mut out, "## 返事の合流先（外部作用の返事が入るトリガ）", "| 外部作用 | ◆合流先のトリガ |", 2, true);
        table(&mut out, spec, "合流", 2);
    }
    if has(spec, "データ用フォルダ") {
        push_head(&mut out, "## データ用フォルダ（DB・ログ・シークレットの置き場。OS ごとの書き方。実際の環境で書き込めるかは、起動時に確かめる）", "| 場合 | ◆パス |", 2, true);
        table(&mut out, spec, "データ用フォルダ", 2);
    }
    if has(spec, "ファイル") {
        push_head(&mut out, "## ファイルを触る口（実際にファイルに触る。ここに書いた操作・場所・ファイルにしか触りません）", "| 外部作用 | ◆操作 | ◆場所 | ◆結果 → 結果の種類 |", 4, true);
        table(&mut out, spec, "ファイル", 4);
    }
    if has(spec, "常駐") {
        push_head(&mut out, "## 常駐（窓を閉じても終了せず、タスクトレイ・メニューバーのアイコンに居続ける。左クリックの動きと右クリックのメニューを宣言できる）", "| 項目 | ◆内容 |", 2, true);
        table(&mut out, spec, "常駐", 2);
    }
    if has(spec, "通信") {
        push_head(&mut out, "## 外へ出る通信（実通信。ここに書いた先にしか出ません。リダイレクトにも従いません）", "| 外部作用 | ◆通信先 | ◆認証（シークレット） | ◆本文の項目 | ◆返事の状態 → 結果の種類 |", 5, true);
        table(&mut out, spec, "通信", 5);
    }
    if has(spec, "外部作用") {
        push_head(&mut out, "## 外部作用（外へ出す内容。入出力型）", "| 外部作用 | ◆出す内容 |", 2, true);
        table(&mut out, spec, "外部作用", 2);
    }
    if has(spec, "出来事") {
        push_head(&mut out, "## 出来事（外の出来事をきっかけに起こす処理）", "| 出来事 | ◆起こし方 | ◆処理 |", 3, true);
        table(&mut out, spec, "出来事", 3);
    }
    if has(spec, "画面") {
        push_head(&mut out, "## 画面（入口は最初の画面と entry の画面。入力欄の「←」は選ぶ元の問い合わせ）", "| 画面 | ◆見せる | ◆入力欄 | ◆受け取る値 | ◆入口 | ◆選ばせる slot | ◆URL |", 7, true);
        table(&mut out, spec, "画面", 7);
        push_head(&mut out, "## 画面遷移", "| 画面 | ◆操作 | ◆結果 | ◆行き先 |", 4, true);
        table(&mut out, spec, "画面遷移", 4);
        if has(spec, "拡張") {
            push_head(&mut out, "## ブラウザ拡張（ページに出す画面。ページからの読み方も宣言から生成）", "| 画面 | ページ | 見せる問い合わせ | ページから読む値 | 読み方 |", 5, true);
            table(&mut out, spec, "拡張", 5);
        }
        push_head(&mut out, "## 画面デザイン（突合しない。配置と色）", "| 画面 | 指定 | 値 |", 3, true);
        table(&mut out, spec, "画面デザイン", 3);
    }
    if has(spec, "人の保証") {
        push_head(&mut out, "## 人の保証（範囲の確認を、人が危険を承知で省いたもの。検査器は宣言した範囲を事実として使う）", "| 処理 | 入力 | 型・範囲 | 実行時の確認 | 理由 |", 5, true);
        table(&mut out, spec, "人の保証", 5);
    }
    if has(spec, "保存の形") {
        push_head(&mut out, &format!("## 保存の形の変更（今は版 {}。前の版の DB は、起動時に 1 回でまとめて今の形にする）", ck.schema_version()), "| 版 | すること | 項目 | 内容 |", 4, true);
        table(&mut out, spec, "保存の形", 4);
    }
    if has(spec, "保存先") {
        push_head(&mut out, "## 保存先（entity を置く場所と、リクエストの直列を誰が保証するか）", "| データ | 保存先 | 直列の担い手 |", 3, true);
        table(&mut out, spec, "保存先", 3);
    }
    if has(spec, "動かす前提") {
        push_head(&mut out, "## 動かす前提（アプリが実行環境に要求すること。環境の実際は書かず、plan や実環境から読んで照合する。照合は geas env-verify（plan の JSON から。読めなかった事実は「確かめられなかった」で、合格にはしない））", "| 番号 | 観点 | 要求 | 由来 | 確かめ方 |", 5, true);
        // Python 版は見出しと表の間に空行を 1 つ入れ、表の見出しは同じ要領で続ける
        table(&mut out, spec, "動かす前提", 5);
    }
    if has(spec, "削除") {
        push_head(&mut out, "## 削除（消してよい状態と、消したときの参照元の扱い）", "| データ | 消してよい状態 | 参照元の扱い |", 3, true);
        table(&mut out, spec, "削除", 3);
    }
    if has(spec, "ルート") {
        let with_actor = has(spec, "役割");
        out.push(String::new());
        out.push("## ルート（URL。結果の種類ごとの HTTP の状態つき。「内部」は流れの途中の処理で、サーバーは外に出さない）".into());
        out.push(String::new());
        out.push(format!("| メソッド | 経路 | 動作 | 呼ぶもの | 結果→状態 | 外から |{}", if with_actor { " 呼べる人 |" } else { "" }));
        out.push(format!("|---|---|---|---|---|---|{}", if with_actor { "---|" } else { "" }));
        for r in rows(spec, "ルート") {
            out.push(format!("| {} | {} | {} | {} | {} | {} |{}", cell(r, 0), cell(r, 1), cell(r, 2), cell(r, 3), cell(r, 4), cell(r, 5), if with_actor { format!(" {} |", cell(r, 6)) } else { String::new() }));
        }
        if rows(spec, "ルート").iter().any(|r| r[0] == "PUT" || r[0] == "DELETE") {
            out.push(String::new());
            out.push(format!("{}。", IF_MATCH_NOTE));
        }
    }
    if has(spec, "POSTの問い合わせ") {
        push_head(&mut out, &format!("## POST で受ける問い合わせ（{}）", POST_NOTE), "| URL | 問い合わせ | POST で受ける理由 |", 3, true);
        for r in rows(spec, "POSTの問い合わせ") {
            out.push(format!("| POST {} | {} | {} |", cell(r, 0), cell(r, 1), cell(r, 2)));
        }
    }
    if has(spec, "範囲") {
        push_head(&mut out, &format!("## 範囲（名前を付けた行の集合。{}）", SCOPE_NOTE), "| 範囲 | データ | 入る行 | 使う問い合わせ・処理 |", 4, true);
        table(&mut out, spec, "範囲", 4);
    }
    if has(spec, "役割") {
        push_head(&mut out, &format!("## 役割（呼べる人の区別。{}）", ACTOR_NOTE), "| 役割 | ヘッダに載せる名前 | 呼べる URL |", 3, true);
        table(&mut out, spec, "役割", 3);
    }
    push_head(&mut out, "## 不変条件", "| ◆内容 | 検査の結果 |", 2, true);
    for (text, rows_) in &ck.invariant_status {
        out.push(format!("| {} | {} |", text, rows_.iter().map(|(p_, s)| format!("{}: {}", p_, s)).collect::<Vec<_>>().join("／")));
    }
    if has(spec, "保証の強さ") {
        out.push(String::new());
        out.push("## 保証の強さ（約束ごとに、何が守っているか。上ほど強く、下の行ほど人が重点的に読む）".into());
        out.push(String::new());
        out.push(format!("件数：{}", level_counts(rows(spec, "保証の強さ")).iter().map(|(lv, n)| format!("{} {}", lv, n)).collect::<Vec<_>>().join("、")));
        out.push(String::new());
        out.push("| 強さ | 対象 | 内容 |".into());
        out.push("|---|---|---|".into());
        table(&mut out, spec, "保証の強さ", 3);
    }
    if has(spec, "部品") {
        push_head(&mut out, "## 部品（外から足す。呼ぶたびに、戻り値が保証に合うかを確かめる）", "| 部品 | ◆引数 | ◆戻り値 | ◆保証 |", 4, true);
        table(&mut out, spec, "部品", 4);
    }
    push_head(&mut out, "## 指定事項（コードに書かれた、既定でない指定）", "| 項目 | ◆指定 |", 2, true);
    table(&mut out, spec, "指定", 2);
    if has(spec, "名前") {
        push_head(&mut out, "## 名前と表示名（説明コメント /// から）", "| 名前 | 表示名 |", 2, true);
        table(&mut out, spec, "名前", 2);
    }
    out.push(String::new());
    out.push("## 実現の報告（振る舞いの仕様には含めない）".into());
    out.push(String::new());
    for r in rows(spec, "実現") {
        out.push(format!("- 物理数 {}: {}", cell(r, 0), cell(r, 1)));
    }
    for x in rows(spec, "処理") {
        let refs = x["参照"].as_array().unwrap();
        let ids = x["採番"].as_array().unwrap();
        if !refs.is_empty() || !ids.is_empty() {
            let mut parts: Vec<String> = refs.iter().map(|r| format!("参照 {}: {}（配備時に埋まる）", cell(r, 0), cell(r, 1))).collect();
            parts.extend(ids.iter().map(|d| format!("採番 {}（実行のたびに基盤が代入）", d.as_str().unwrap())));
            out.push(format!("- 処理 {} の文脈: {}", x["処理"].as_str().unwrap(), parts.join("、")));
        }
    }
    out.push(String::new());
    out.push("## 検査の結果".into());
    out.push(String::new());
    if ck.diags.is_empty() {
        out.push("- 指摘なし".into());
    } else {
        out.extend(ck.diags.iter().map(|d| format!("- {}", d)));
    }
    let lines: Vec<String> = out.into_iter().map(|x| if x.starts_with('|') { x.replace('\n', "<br>") } else { x }).collect();
    lines.join("\n") + "\n"
}
