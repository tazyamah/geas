// SPDX-License-Identifier: GPL-3.0-or-later
//! リソース（resource）の URL とルーティング（Python 版 geas/rest.py の移し）。
use crate::ast::*;
use indexmap::{IndexMap, IndexSet};

pub fn method_of(kind: &str) -> &'static str {
    match kind {
        "index" | "show" => "GET",
        "create" => "POST",
        "update" => "PUT",
        _ => "DELETE",
    }
}

pub fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            out.push('_');
        }
        out.push(c);
    }
    out.to_lowercase()
}

pub fn singular(word: &str) -> String {
    if let Some(w) = word.strip_suffix("ies") {
        return format!("{}y", w);
    }
    word.strip_suffix('s').unwrap_or(word).to_string()
}

pub fn first_by_name(p: &Program) -> IndexMap<&str, &Resource> {
    let mut out: IndexMap<&str, &Resource> = IndexMap::new();
    for r in &p.resources {
        out.entry(r.name.as_str()).or_insert(r);
    }
    out
}

pub fn base_path_in(p: &Program, res: &Resource, seen: &[String], by_name: &IndexMap<&str, &Resource>) -> String {
    if !res.under.is_empty() && !seen.contains(&res.under) {
        if let Some(parent) = by_name.get(res.under.as_str()) {
            let ps = singular(&snake(&parent.name));
            let mut own = snake(&res.name);
            if own.starts_with(&format!("{}_", ps)) {
                own = own[ps.len() + 1..].to_string();
            }
            let mut seen2 = seen.to_vec();
            seen2.push(res.name.clone());
            return format!("{}/:{}_id/{}", base_path_in(p, parent, &seen2, by_name), ps, own);
        }
    }
    format!("/{}", snake(&res.name))
}

pub fn base_path(p: &Program, res: &Resource) -> String {
    base_path_in(p, res, &[], &first_by_name(p))
}

pub const OPEN: &str = "呼べる";
pub const INTERNAL: &str = "内部（流れの途中）";

/// 流れの途中の処理
pub fn internal_targets(p: &Program) -> IndexSet<String> {
    let mut out: IndexSet<String> = IndexSet::new();
    for fl in &p.flows {
        for (_, rs) in &fl.entries {
            for (_, d) in rs {
                if d != "reply" && !d.starts_with("wait ") {
                    out.insert(d.clone());
                }
            }
        }
    }
    for t in &p.triggers {
        for (_, _, d) in &t.causes {
            out.insert(d.clone());
        }
    }
    out
}

pub const ANYONE: &str = "anyone";
pub const ACTOR_ID_HEADER: &str = "X-Geas-Actor-Id";
pub const ACTOR_HEADER: &str = "X-Geas-Actor";

/// 呼ぶもの → 呼べる役割の名前の並び
pub fn route_actors(p: &Program) -> IndexMap<String, Vec<String>> {
    let mut out = IndexMap::new();
    for res in &p.resources {
        for a in &res.actions {
            if let Some(ac) = &a.actors {
                out.insert(a.target.clone(), ac.clone());
            }
        }
    }
    out
}

pub fn actors_text(p: &Program, target: &str, inner: &IndexSet<String>) -> String {
    if p.actors.is_empty() {
        return String::new();
    }
    if inner.contains(target) {
        return "（外から呼べない）".to_string();
    }
    let who = match route_actors(p).get(target) {
        Some(w) => w.clone(),
        None => return String::new(),
    };
    let own = if p.queries.iter().any(|q| q.name == target && uses_caller(p, q)) { "（本人の分だけ）" } else { "" };
    if who == vec![ANYONE.to_string()] {
        "誰でも".to_string()
    } else {
        who.iter().map(|x| p.show_names(x)).collect::<Vec<_>>().join("、") + own
    }
}

pub fn path_inputs(p: &Program, res: &Resource, action: &ResAction, q: &Query) -> Vec<String> {
    let keys: Vec<String> = if action.kind == "show" { res.keys.clone() } else { vec![] };
    let parents = base_path(p, res).matches("/:").count();
    let rest: Vec<String> = q.inputs.iter().filter(|(n, t)| !t.optional && !keys.contains(n)).map(|(n, _)| n.clone()).collect();
    let mut out: Vec<String> = rest.into_iter().take(parents).collect();
    out.extend(keys.into_iter().filter(|k| q.inputs.iter().any(|(n, _)| n == k)));
    out
}

pub fn q_inputs(p: &Program, res: &Resource, action: &ResAction, q: &Query) -> Vec<String> {
    let on_path = path_inputs(p, res, action, q);
    let mut out: Vec<String> = q.inputs.iter().filter(|(n, _)| !on_path.contains(n)).map(|(n, _)| n.clone()).collect();
    if !order_choices(q).is_empty() {
        out.push(ORDER.to_string());
    }
    if paged(q) {
        out.push(AFTER.to_string());
    }
    out
}

pub fn q_types(q: &Query) -> IndexMap<String, Type> {
    let mut out: IndexMap<String, Type> = q.inputs.iter().map(|(n, t)| (n.clone(), t.clone())).collect();
    if !order_choices(q).is_empty() {
        out.insert(ORDER.to_string(), Type { optional: true, ..Type::new("文字列") });
    }
    if paged(q) {
        out.insert(AFTER.to_string(), Type { optional: true, ..Type::new("ID") });
    }
    out
}

pub fn shown_path(p: &Program, path: &str, kind: &str, target: &str) -> String {
    if kind != "index" && kind != "show" {
        return path.to_string();
    }
    let q = p.queries.iter().find(|x| x.name == target);
    let res = p.resources.iter().find(|r| r.actions.iter().any(|a| a.target == target && a.kind == kind));
    let (q, res) = match (q, res) {
        (Some(q), Some(r)) => (q, r),
        _ => return path.to_string(),
    };
    let act = res.actions.iter().find(|a| a.target == target && a.kind == kind).unwrap();
    let opt: IndexMap<String, bool> = q_types(q).into_iter().map(|(n, t)| (n, t.optional)).collect();
    let names = q_inputs(p, res, act, q);
    let mark = |n: &String| if opt.get(n).copied().unwrap_or(false) { "?" } else { "" };
    if act.post.as_ref().map_or(false, |s| !s.is_empty()) {
        return format!("{}（本文 {{\"q\": {{{}}}}}）", path, names.iter().map(|n| format!("{}{}", n, mark(n))).collect::<Vec<_>>().join(", "));
    }
    if names.is_empty() {
        path.to_string()
    } else {
        format!("{}?{}", path, names.iter().map(|n| format!("q[{}{}]=…", n, mark(n))).collect::<Vec<_>>().join("&"))
    }
}

pub const PATH_TEXT: [&str; 4] = ["ID", "参照", "文字列", "名前"];
pub const PATH_INT: [&str; 3] = ["整数", "金額", "日時"];

/// 事情があって POST で受ける問い合わせ：名前 → 理由
pub fn post_queries(p: &Program) -> IndexMap<String, String> {
    let mut out = IndexMap::new();
    for res in &p.resources {
        for a in &res.actions {
            if let Some(w) = &a.post {
                if !w.is_empty() {
                    out.insert(a.target.clone(), w.clone());
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct Route {
    pub method: String,
    pub path: String,
    pub kind: String,
    pub target: String,
    pub resource: String,
    pub statuses: Vec<(String, i128)>,
}

/// ルートの一覧。宣言の順
pub fn routes(p: &Program) -> Vec<Route> {
    let by_name = first_by_name(p);
    let mut out = vec![];
    for res in &p.resources {
        let base = base_path_in(p, res, &[], &by_name);
        for a in &res.actions {
            let mut path = base.clone();
            if a.kind == "show" || a.kind == "update" || a.kind == "destroy" {
                path += &format!("/:{}", res.keys.first().map(|s| s.as_str()).unwrap_or("id"));
            }
            let method = if a.post.as_ref().map_or(false, |s| !s.is_empty()) { "POST" } else { method_of(&a.kind) };
            out.push(Route { method: method.to_string(), path, kind: a.kind.clone(), target: a.target.clone(), resource: res.name.clone(), statuses: a.statuses.clone() });
        }
    }
    out
}

/// 対象 → (経路, 動作)（同じ対象は後に書いたもの）
pub fn target_table(p: &Program) -> IndexMap<String, (String, String)> {
    routes(p).into_iter().map(|r| (r.target, (r.path, r.kind))).collect()
}

pub fn screen_url(p: &Program, sc: &Screen) -> String {
    let by = target_table(p);
    for (dn, _) in &sc.actions {
        if let Some((path, kind)) = by.get(dn) {
            if kind == "create" {
                return format!("{}/new", path);
            }
            if kind == "update" {
                return format!("{}/edit", path);
            }
        }
    }
    for qn in &sc.shows {
        if let Some((path, _)) = by.get(qn) {
            return path.clone();
        }
    }
    String::new()
}

pub const IF_MATCH_NOTE: &str = "PUT と DELETE は、読んだときの行の版をヘッダ If-Match に載せて呼びます（一覧の行の _etag か、1 件の読み出しの ETag）。載せなければ 428、読んだあとに行が変わっていれば 412 で、どちらも処理を走らせません。POST の命令には要りません（状態の遷移の確かめが担います）";

fn find_row(stmts: &[Stmt], idn: &str) -> String {
    for st in stmts {
        if let Stmt::Assign(a) = st {
            if let Expr::Index { d, e, .. } = &a.e {
                if let Expr::Name { n } = &**e {
                    if n == idn {
                        return d.clone();
                    }
                }
            }
        }
        let inner: Vec<&Vec<Stmt>> = match st {
            Stmt::If(i) => {
                let mut v = vec![&i.body];
                if let Some(e) = &i.els {
                    v.push(e);
                }
                v
            }
            Stmt::ForEach(f) => vec![&f.body],
            _ => vec![],
        };
        for b in inner {
            let got = find_row(b, idn);
            if !got.is_empty() {
                return got;
            }
        }
    }
    String::new()
}

/// update・destroy の処理が書き換える 1 行の実体
pub fn row_entity(p: &Program, target: &str) -> String {
    let proc = match p.processes.iter().find(|x| x.name == target) {
        Some(x) => x,
        None => return String::new(),
    };
    match proc.inputs.iter().find(|(_, t)| t.kind == "ID") {
        Some((idn, _)) => find_row(&proc.body, idn),
        None => String::new(),
    }
}

/// リソース名 → update・destroy が書き換える行の実体
pub fn versioned(p: &Program) -> IndexMap<String, String> {
    let mut out: IndexMap<String, String> = IndexMap::new();
    for r in routes(p) {
        if (r.kind == "update" || r.kind == "destroy") && !out.contains_key(&r.resource) {
            let e = row_entity(p, &r.target);
            if !e.is_empty() {
                out.insert(r.resource.clone(), e);
            }
        }
    }
    out
}
