// SPDX-License-Identifier: GPL-3.0-or-later
//! 名前の書き方の検査。Python 版 geas/names.py の移し。名前は半角英数にして、種類は大文字小文字で見分ける。
use crate::ast::*;
use std::collections::HashSet;

pub const RESERVED: [&str; 90] = ["id", "set", "text", "time", "bytes", "int", "ratio", "money", "ref", "vtime", "round", "floor", "ceil", "half_up", "sum", "now", "null", "absent", "nonempty", "empty", "true", "false", "let", "return", "if", "else", "for", "in", "create", "update", "request", "reply", "wait", "par", "entity", "ltype", "store", "state", "result", "effect", "part", "convert", "invariant", "deed", "nexus", "flow", "trigger", "query", "event", "screen", "resource", "tz_offset", "timezone", "all", "input", "slot", "reads", "writes", "transition", "effects", "initial", "middle", "final", "reject", "required", "unique", "optional", "raw", "require", "read", "trust", "verify", "assume", "then", "len", "only", "os_path", "digit", "positions", "dangerous", "to_raw", "raw_cat", "or", "saturating", "storage", "needs", "isolation", "library", "bool", "not"];

fn is_upper_camel(n: &str) -> bool {
    let mut cs = n.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_uppercase()) && cs.all(|c| c.is_ascii_alphanumeric())
}

fn is_lower_snake(n: &str) -> bool {
    let mut cs = n.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_lowercase()) && cs.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// kind は 'u'（もの）・'l'（動きと値）・'x'（処理の中の変数）
pub fn each_name(p: &Program, ltype_names: &[String], f: &mut dyn FnMut(char, &str, &str)) {
    for n in ltype_names {
        f('u', n, &format!("論理型 {}", n));
    }
    for d in &p.data {
        f('u', &d.name, &format!("データ {}", d.name));
        for fl in &d.fields {
            f('l', &fl.name, &format!("データ {}", d.name));
        }
    }
    for c in &p.composites {
        for (n, _) in c.parts.as_deref().unwrap_or(&[]) {
            f('l', n, &format!("合成型 {}", c.target.clone().unwrap_or_default()));
        }
    }
    for s in &p.states {
        if s.plain {
            continue;
        }
        f('u', &s.name, &format!("状態 {}", s.name));
        for m in std::iter::once(&s.initial).chain(s.middle.iter()).chain(s.final_.iter()) {
            f('u', m, &format!("状態 {}", s.name));
        }
    }
    for r in &p.results {
        f('u', &r.name, &format!("結果 {}", r.name));
        for (v, fields) in &r.variants {
            f('u', v, &format!("結果 {}", r.name));
            for (n, _) in fields {
                f('l', n, &format!("結果 {}", r.name));
            }
        }
    }
    for e in &p.effects {
        let (ns, op) = match e.name.find('.') {
            Some(k) => (&e.name[..k], &e.name[k + 1..]),
            None => (e.name.as_str(), ""),
        };
        f('u', ns, &format!("外部作用 {}", e.name));
        if !op.is_empty() {
            f('l', op, &format!("外部作用 {}", e.name));
        }
        for (n, _) in &e.params {
            f('l', n, &format!("外部作用 {}", e.name));
        }
    }
    for x in &p.parts {
        f('l', &x.name, &format!("部品 {}", x.name));
        for (n, _) in &x.params {
            f('l', n, &format!("部品 {}", x.name));
        }
    }
    for t in &p.triggers {
        f('u', &t.name, &format!("トリガ {}", t.name));
        for (n, _) in &t.params {
            f('l', n, &format!("トリガ {}", t.name));
        }
    }
    for rp in &p.replies {
        f('u', &rp.name, &format!("応答 {}", rp.name));
        for (n, _) in &rp.params {
            f('l', n, &format!("応答 {}", rp.name));
        }
    }
    for fl in &p.flows {
        f('u', &fl.name, &format!("フロー {}", fl.name));
    }
    for ev in &p.events {
        f('u', &ev.name, &format!("出来事 {}", ev.name));
    }
    for sc in &p.screens {
        f('u', &sc.name, &format!("画面 {}", sc.name));
        for (n, _) in &sc.params {
            f('l', n, &format!("画面 {}", sc.name));
        }
    }
    for rs in &p.resources {
        f('u', &rs.name, &format!("リソース {}", rs.name));
    }
    for q in &p.queries {
        f('l', &q.name, &format!("問い合わせ {}", q.name));
        for (n, _) in q.inputs.iter().chain(q.refs.iter()) {
            f('l', n, &format!("問い合わせ {}", q.name));
        }
        for c in &q.select {
            f('l', &c.name, &format!("問い合わせ {}", q.name));
        }
    }
    for x in &p.processes {
        f('l', &x.name, &format!("処理 {}", x.name));
        for (n, _) in x.inputs.iter().chain(x.refs.iter()) {
            f('l', n, &format!("処理 {}", x.name));
        }
        fn walk(stmts: &[Stmt], where_: &str, f: &mut dyn FnMut(char, &str, &str)) {
            for st in stmts {
                match st {
                    Stmt::Assign(a) => f('x', &a.name, where_),
                    Stmt::Create(c) => f('x', &c.name, where_),
                    Stmt::If(i) => {
                        walk(&i.body, where_, f);
                        walk(i.els.as_deref().unwrap_or(&[]), where_, f);
                    }
                    Stmt::ForEach(fe) => walk(&fe.body, where_, f),
                    _ => {}
                }
            }
        }
        walk(&x.body, &format!("処理 {}", x.name), f);
    }
}

fn orig_name<'a>(p: &'a Program, n: &'a str) -> &'a str {
    match p.origin.get(n) {
        Some((_, orig)) => orig.as_str(),
        None => n,
    }
}

/// (場所, 指摘) の列
pub fn name_problems(p: &Program, ltype_names: &[String]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    each_name(p, ltype_names, &mut |kind, n, where_| {
        let n = orig_name(p, n);
        if kind == 'u' {
            if !is_upper_camel(n) {
                out.push((where_.to_string(), format!("名前「{}」は、英大文字で始まる英数字（UpperCamel）で書きます", n)));
            }
        } else if !is_lower_snake(n) {
            out.push((where_.to_string(), format!("名前「{}」は、英小文字で始まる英小文字・数字・_（lower_snake）で書きます", n)));
        } else if RESERVED.contains(&n) {
            out.push((where_.to_string(), format!("名前「{}」は組み込みか予約語なので使えません", n)));
        }
    });
    out
}

/// (場所, 名前) の列：仕様書に出る名前のうち、表示名（///）が無いもの
pub fn label_gaps(p: &Program, ltype_names: &[String]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let ext: HashSet<String> = p.parts.iter().filter(|x| x.external()).map(|x| format!("部品 {}", x.name)).collect();
    each_name(p, ltype_names, &mut |kind, n, where_| {
        if kind == 'x' {
            return;
        }
        if ext.contains(where_) {
            return;
        }
        if let Some(ls) = p.labels.get(n) {
            if ls.get("ja").map_or(false, |s| !s.is_empty()) || ls.get("").map_or(false, |s| !s.is_empty()) {
                return;
            }
        }
        let key = (where_.to_string(), n.to_string());
        if seen.contains(&key) {
            return;
        }
        seen.insert(key);
        out.push((where_.to_string(), n.to_string()));
    });
    out
}
