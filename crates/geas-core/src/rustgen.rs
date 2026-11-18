// SPDX-License-Identifier: GPL-3.0-or-later
//! Rust の生成（Python 版 rustgen.py の移し）。検査器を通ったコードだけを受け付ける。
//! 【処理】は純粋な関数（文脈, 保存データ, 入力）→（結果, 書込の列, 外部作用の要求の列）として生成し、実行は基盤に任せる。
use crate::ast::*;
use crate::check::procs::id_of;
use crate::check::{iv_of, names_in, phys, tok_of, Checker, ProcInfo};
use crate::parser::parse_chars;
use crate::rest::versioned;
use crate::screengen::rust_screens;
use indexmap::{IndexMap, IndexSet};
use std::collections::{BTreeMap, HashSet};

pub const PRELUDE: &str = include_str!("assets/rust_prelude.txt");
pub const OS_PATH_RS: &str = include_str!("assets/rust_os_path.txt");

pub fn rust_type(t: &Type) -> String {
    let k = t.kind.as_str();
    match k {
        "ID" | "参照" | "文字列" | "生" => "String".into(),
        "日時" => "i64".into(),
        "真偽" => "bool".into(),
        "バイト列" => "Vec<u8>".into(),
        "整数" | "金額" => phys(t).into(),
        "割合" => "Ratio".into(),
        "名前" | "合成" => t.target.clone().unwrap_or_default(),
        "写像" => format!("BTreeMap<String, {}>", rust_type(t.value.as_deref().unwrap())),
        "集合" => "BTreeSet<String>".into(),
        _ => panic!("rust_type: {}", k),
    }
}

pub fn chars_rust(cs: &[(u32, u32)]) -> String {
    format!("&[{}]", cs.iter().map(|(a, b)| format!("('\\u{{{:x}}}', '\\u{{{:x}}}')", a, b)).collect::<Vec<_>>().join(", "))
}

/// 文字列を、Rust の文字列リテラル（"…"）にする。.geas や計画書から来た文字列を生成コードに埋めるときは、必ずここを通す
pub fn rust_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        if c == '\\' || c == '"' {
            out.push('\\');
            out.push(c);
        } else if (c as u32) < 0x20 || c as u32 == 0x7F {
            out.push_str(&format!("\\u{{{:x}}}", c as u32));
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}

/// 文字列を、生成コードの 1 行のコメント（/// …）に入れられる形にする
pub fn doc_line(s: &str) -> String {
    s.chars().map(|c| if (c as u32) < 0x20 || c as u32 == 0x7F || c == '\u{2028}' || c == '\u{2029}' { ' ' } else { c }).collect()
}

pub fn opt<T: std::fmt::Display>(v: Option<T>) -> String {
    match v {
        None => "None".into(),
        Some(v) => format!("Some({})", v),
    }
}

fn no_opt(t: &Type) -> Type {
    Type { optional: false, ..t.clone() }
}

/// 値 x（Rust の場所の式）の、型の範囲の宣言の検査の行。deref なら x は参照（無いかもしれない値の中身）
pub fn value_checks(label: &str, t: &Type, x: &str, deref: bool) -> Vec<String> {
    let mut chk: Vec<String> = vec![];
    let num = if deref { format!("*{}", x) } else { x.to_string() };
    let rf = if deref { x.to_string() } else { format!("&{}", x) };
    let k = t.kind.as_str();
    if k == "文字列" {
        if let Some(rd) = &t.read {
            chk.push(format!("{}(&{}).map(|_| ())?;", rd, x));
        }
    }
    if k == "日時" {
        chk.push(format!("check_range(\"{}\", {} as i128, Some(-TIME_LIMIT), Some(TIME_LIMIT))?;", label, num));
    }
    let r = t.range.as_ref();
    if k == "文字列" && r.map_or(true, |r| r.hi.is_none()) {
        chk.push(format!("check_range(\"{}の長さ\", {}.chars().count() as i128, {}, Some(TEXT_LIMIT))?;", label, x, opt(r.and_then(|r| r.lo))));
    } else if k == "文字列" {
        let r = r.unwrap();
        chk.push(format!("check_range(\"{}の長さ\", {}.chars().count() as i128, {}, {})?;", label, x, opt(r.lo), opt(r.hi)));
    }
    if k == "文字列" {
        if let Some(cs) = &t.chars {
            chk.push(format!("check_chars(\"{}\", &{}, {})?;", label, x, chars_rust(cs)));
        }
    }
    if k == "バイト列" {
        if let Some(r) = r {
            chk.push(format!("check_range(\"{}の長さ\", {}.len() as i128, {}, {})?;", label, x, opt(r.lo), opt(r.hi)));
        }
    }
    if k == "整数" || k == "金額" {
        if let Some(r) = r {
            chk.push(format!("check_range(\"{}\", {} as i128, {}, {})?;", label, num, opt(r.lo), opt(r.hi)));
        }
    }
    if k == "割合" {
        chk.push(format!("check_ratio(\"{}\", {}, {}, {})?;", label, rf, opt(r.and_then(|r| r.lo)), opt(r.and_then(|r| r.hi))));
    }
    if (k == "写像" || k == "集合") && t.maxn.is_some() {
        chk.push(format!("check_range(\"{}の要素数\", {}.len() as i128, Some(0), Some({}))?;", label, x, t.maxn.unwrap()));
    }
    if k == "写像" {
        let v = t.value.as_deref().unwrap();
        if (v.kind == "整数" || v.kind == "金額") && v.range.is_some() {
            let r = v.range.as_ref().unwrap();
            chk.push(format!("for v in {}.values() {{ check_range(\"{}\", *v as i128, {}, {})?; }}", x, label, opt(r.lo), opt(r.hi)));
        }
    }
    if k == "合成" {
        for (pn, pt) in t.parts.as_deref().unwrap_or(&[]) {
            if pt.optional {
                let inner = value_checks(&format!("{}.{}", label, pn), &no_opt(pt), "v", true);
                for c in inner {
                    chk.push(format!("if let Some(v) = &{}.{} {{ {} }}", x, pn, c));
                }
            } else {
                chk.extend(value_checks(&format!("{}.{}", label, pn), pt, &format!("{}.{}", x, pn), false));
            }
        }
    }
    chk
}

/// 外から受け取る値（入力・slot）の、範囲の宣言の検査の行
pub fn range_checks(items: &[(String, Type)]) -> Vec<String> {
    let mut out = vec![];
    for (name, t) in items {
        let opt_ = t.optional;
        let xs = if opt_ { "x".to_string() } else { format!("self.{}", name) };
        let mut chk = value_checks(name, t, &xs, opt_);
        if let Some(trust) = &t.trust {
            if trust.0 == "assume" {
                chk = if t.kind == "割合" { value_checks(name, &Type { range: None, ..t.clone() }, &xs, opt_) } else { vec![] };
            } else {
                chk = chk.into_iter().map(|c| format!("if cfg!(debug_assertions) {{ {} }}", c)).collect();
            }
        }
        for c in chk {
            out.push(if opt_ { format!("        if let Some(x) = &self.{} {{ {} }}", name, c) } else { format!("        {}", c) });
        }
    }
    out
}

/// 項目の Rust の型。無いかもしれない項目は Option
pub fn field_rust(t: &Type) -> String {
    if t.optional {
        format!("Option<{}>", rust_type(t))
    } else {
        rust_type(t)
    }
}

/// 値の表示用の Rust 式（突合のための正規の書式）。null は「null」
pub fn show_expr(t: &Type, x: &str) -> String {
    if t.optional {
        return format!("{}.as_ref().map_or(\"null\".to_string(), |v| format!(\"{{}}\", {}))", x, show_expr(&no_opt(t), "v"));
    }
    match t.kind.as_str() {
        "名前" => format!("{}.name()", x),
        "合成" => format!("format!(\"{{:?}}\", &{})", x),
        "写像" => format!("show_map(&{}, |v| format!(\"{{}}\", {}))", x, show_expr(t.value.as_deref().unwrap(), "v")),
        "集合" => format!("show_set(&{})", x),
        "バイト列" => format!("show_bytes(&{})", x),
        _ => x.to_string(),
    }
}

pub const CONVERSIONS: [&str; 3] = ["money", "int", "time"];

/// 生成物での処理の入力：書いた入力に、本人で絞る範囲の「呼んだ本人の ID」（caller）を足したもの
pub fn deed_inputs_proc(ck: &Checker, proc: &Process) -> Vec<(String, Type)> {
    if ck.infos.get(&proc.name).map_or(false, |i| i.caller) {
        let mut v = proc.inputs.clone();
        v.push((CALLER.to_string(), Type::new("ID")));
        v
    } else {
        proc.inputs.clone()
    }
}

pub fn deed_inputs_query(ck: &Checker, q: &Query) -> Vec<(String, Type)> {
    if ck.infos.get(&q.name).map_or(false, |i| i.caller) {
        let mut v = q.inputs.clone();
        v.push((CALLER.to_string(), Type::new("ID")));
        v
    } else {
        gen_inputs(ck.p, q)
    }
}

/// 問い合わせの1列の型（検査済みの前提）
pub fn column_type(ck: &Checker, q: &Query, c: &Column) -> Type {
    let dt = ck.data[&q.target];
    let own = |n: &str| dt.fields.iter().find(|f| f.name == n).unwrap();
    if c.kind == "参照先" {
        let via = own(c.via.as_deref().unwrap());
        let tdt = ck.data[via.r#type.target.as_deref().unwrap()];
        let t = &tdt.fields.iter().find(|x| x.name == c.field).unwrap().r#type;
        return Type { optional: t.optional || via.r#type.optional, ..t.clone() };
    }
    if c.kind == "件数" {
        return Type::new("整数");
    }
    if c.kind == "項目" {
        if let Some(dv) = dt.derived.iter().find(|x| x.name == c.field) {
            return Type { optional: false, ..dv.r#type.clone() };
        }
    }
    let f = own(&c.field);
    if c.kind == "合計" {
        Type::new("整数")
    } else {
        f.r#type.clone()
    }
}

// ---- 全体 ----
pub struct Gen<'a, 'b> {
    pub ck: &'b Checker<'a>,
    pub p: &'a Program,
    pub etag_data: IndexSet<String>,
    pub out: Vec<String>,
    pub need_os_path: bool,
}

impl<'a, 'b> Gen<'a, 'b> {
    pub fn new(ck: &'b Checker<'a>) -> Self {
        Gen { ck, p: ck.p, etag_data: versioned(ck.p).values().cloned().collect(), out: vec![], need_os_path: false }
    }

    pub fn w(&mut self, s: impl Into<String>) {
        self.out.push(s.into());
    }

    pub fn lib(&mut self) -> String {
        self.out = vec![PRELUDE.to_string()];
        self.states();
        self.records();
        self.writes_enum();
        self.effects();
        self.parts();
        self.results();
        for proc in &self.p.processes {
            ProcGen::new(self, proc).emit(self);
        }
        for q in &self.p.queries {
            self.query(q);
        }
        self.flows();
        self.triggers();
        if !self.p.screens.is_empty() {
            let s = rust_screens(self.ck);
            self.w(s);
        }
        self.out.join("\n") + "\n"
    }

    pub fn key(&self, d: &str) -> String {
        self.ck.key_field(d).unwrap().name.clone()
    }

    fn query(&mut self, q: &Query) {
        let n = &q.name;
        let ck = self.ck;
        let dt = ck.data[&q.target];
        let fields: IndexMap<&str, &Field> = dt.fields.iter().map(|f| (f.name.as_str(), f)).collect();
        let key = self.key(&q.target);
        let q_inputs = gen_inputs(ck.p, q);
        let q_filters = gen_filters(ck.p, q);
        let ins: IndexMap<&str, &Type> = q_inputs.iter().map(|(a, t)| (a.as_str(), t)).collect();
        let ty = |t: &Type| -> String {
            let r = rust_type(t);
            if t.optional {
                format!("Option<{}>", r)
            } else {
                r
            }
        };
        self.w(format!("// ---- 問い合わせ {}（読むだけ） ----", n));
        self.w(format!("pub struct {}Input {{", n));
        for (name, t) in &q_inputs {
            self.w(format!("    pub {}: {},", name, ty(t)));
        }
        self.w("}");
        self.w(format!("impl {}Input {{\n    pub fn validate(&self) -> Result<(), String> {{", n));
        for line in range_checks(&q_inputs) {
            self.w(line);
        }
        self.w("        Ok(())\n    }\n}");
        self.w(format!("pub struct {}Ctx {{", n));
        for (name, t) in &q.refs {
            self.w(format!("    pub {}: {},", name, rust_type(t)));
        }
        self.w("}");
        self.w(format!("impl {}Ctx {{\n    /// 外から埋まる値（slot）の、範囲の宣言の検査\n    pub fn validate(&self) -> Result<(), String> {{", n));
        for line in range_checks(&q.refs) {
            self.w(line);
        }
        self.w("        Ok(())\n    }\n}");
        let cols: Vec<(&Column, Type)> = q.select.iter().map(|c| (c, column_type(ck, q, c))).collect();
        if !q.select.is_empty() {
            self.w("#[derive(Clone, Debug, PartialEq)]");
            self.w(format!("pub struct {}Row {{", n));
            for (c, t) in &cols {
                self.w(format!("    pub {}: {},", c.name, if c.kind == "合計" || c.kind == "件数" { "i128".to_string() } else { field_rust(t) }));
            }
            if self.etag_data.contains(&q.target) {
                self.w("    pub row_key_: String,");
            }
            self.w("}");
            let fmt = cols.iter().map(|(c, _)| format!("{}={{}}", c.name)).collect::<Vec<_>>().join("/");
            let vals = cols.iter().map(|(c, t)| show_expr(t, &format!("self.{}", c.name))).collect::<Vec<_>>().join(", ");
            self.w(format!("impl {}Row {{\n    pub fn show(&self) -> String {{ format!(\"{}\", {}) }}\n}}", n, fmt, vals));
        }
        let out_ty = if q.select.is_empty() { "String".to_string() } else { format!("{}Row", n) };
        let pg = paged(q);
        if pg {
            self.w(format!("pub fn {n}(ctx: &{n}Ctx, Store: &Store, Input: &{n}Input) -> Vec<{o}> {{\n    {n}_paged(ctx, Store, Input).0\n}}", n = n, o = out_ty));
            self.w(format!("pub fn {n}_paged(ctx: &{n}Ctx, Store: &Store, Input: &{n}Input) -> (Vec<{o}>, Option<String>) {{", n = n, o = out_ty));
        } else {
            self.w(format!("pub fn {n}(ctx: &{n}Ctx, Store: &Store, Input: &{n}Input) -> Vec<{o}> {{", n = n, o = out_ty));
        }
        self.w(format!("    let mut rows: Vec<&{t}> = Store.{t}.values().filter(|r| {{", t = q.target));
        let atom = |f: &str, op: &str, st: &str| -> String {
            let fl = fields[f];
            if !st.chars().all(|c| c.is_ascii_digit()) || st.is_empty() {
                return format!("r.{} {} {}::{}", f, op, fl.r#type.target.clone().unwrap_or_default(), st);
            }
            if fl.r#type.optional {
                return format!("r.{}.map_or(false, |v| (v as i128) {} {})", f, op, st);
            }
            format!("(r.{} as i128) {} {}", f, op, st)
        };
        for group in gen_conds(ck.p, q) {
            let atoms: Vec<String> = group.iter().map(|(f, op, st)| atom(f, op, st)).collect();
            self.w(format!("        if !({}) {{ return false; }}", atoms.join(" || ")));
        }
        for (f, op, a) in &q_filters {
            let nul = fields[f.as_str()].r#type.optional;
            let v = if nul { "(*fv)".to_string() } else { format!("r.{}", f) };
            let guard = |cond: String| -> String {
                if nul {
                    format!("match &r.{} {{ Some(fv) => {}, None => false }}", f, cond)
                } else {
                    cond
                }
            };
            if op == "直近" {
                self.w(format!("        if !({}) {{ return false; }}", guard(format!("{} >= ctx.current_time - {}", v, a))));
                continue;
            }
            let t = ins[a.as_str()];
            let cond: String;
            if op.starts_with("含む") {
                let folds: Vec<&str> = if op.contains('(') {
                    let inner = &op[op.char_indices().nth(3).map(|x| x.0).unwrap_or(op.len())..op.len() - 1];
                    inner.split(',').collect()
                } else {
                    vec![]
                };
                let flags = ["大小無視", "かな無視", "幅無視"].iter().map(|k| if folds.contains(k) { "true" } else { "false" }).collect::<Vec<_>>().join(", ");
                cond = if !folds.is_empty() { format!("fold_text(&{}, {}).contains(&fold_text(x, {}))", v, flags, flags) } else { format!("{}.contains(x.as_str())", v) };
            } else if op == "一致" {
                let k = fields[f.as_str()].r#type.kind.as_str();
                cond = if k == "文字列" || k == "ID" || k == "参照" { format!("{} == *x", v) } else { format!("({} as i128) == (*x as i128)", v) };
            } else {
                cond = match op.as_str() {
                    "以上" => format!("({} as i128) >= (*x as i128)", v),
                    "以下" => format!("({} as i128) <= (*x as i128)", v),
                    _ => panic!("条件の種類: {}", op),
                };
            }
            let cond = guard(cond);
            if t.optional {
                self.w(format!("        if let Some(x) = &Input.{} {{ if !({}) {{ return false; }} }}", a, cond));
            } else {
                self.w(format!("        {{ let x = &Input.{}; if !({}) {{ return false; }} }}", a, cond));
            }
        }
        self.w("        true\n    }).collect();");
        let tgt = q.target.clone();
        let cmp_of = |f: &str, d: &str| -> String {
            let (a, b) = if d == "降順" { ("b", "a") } else { ("a", "b") };
            let nulls = if fields[f].r#type.optional { format!("a.{f}.is_none().cmp(&b.{f}.is_none()).then(", f = f) } else { "(".to_string() };
            format!("|a, b| {}{}.{}.cmp(&{}.{})).then({}.{}.cmp(&{}.{}))", nulls, a, f, b, f, a, key, b, key)
        };
        let choices = order_choices(q);
        if !choices.is_empty() {
            self.w(format!("    let cmp: fn(&&{t}, &&{t}) -> std::cmp::Ordering = match Input.{o}.as_deref() {{", t = tgt, o = ORDER));
            for (name, f, d) in choices.iter().skip(1) {
                self.w(format!("        Some({}) => {},", rust_str(name), cmp_of(f, d)));
            }
            let (f0, d0) = q.order.as_ref().unwrap();
            self.w(format!("        _ => {},\n    }};", cmp_of(f0, d0)));
        } else if let Some((f0, d0)) = &q.order {
            self.w(format!("    let cmp = |a: &&{t}, b: &&{t}| {};", cmp_of(f0, d0).chars().skip(7).collect::<String>(), t = tgt));
        } else {
            self.w(format!("    let cmp = |a: &&{t}, b: &&{t}| a.{k}.cmp(&b.{k});", t = tgt, k = key));
        }
        self.w("    rows.sort_by(cmp);");
        if q.page.is_some() || q.page_n.is_some() {
            let n_expr = if let Some(pn) = q.page_n {
                pn.to_string()
            } else {
                let pname = q.page.as_ref().unwrap();
                let t = ins[pname.as_str()];
                if t.optional {
                    format!("Input.{}.map_or(rows.len(), |x| x as usize)", pname)
                } else {
                    format!("Input.{} as usize", pname)
                }
            };
            self.w(format!("    if let Some(k) = &Input.{} {{\n        match Store.{}.get(k) {{\n            Some(c) => rows.retain(|r| cmp(r, &c) == std::cmp::Ordering::Greater),\n            None => rows.clear(),\n        }}\n    }}", AFTER, tgt));
            self.w(format!("    let n: usize = {};", n_expr));
            self.w(format!("    let next: Option<String> = if rows.len() > n {{ n.checked_sub(1).and_then(|i| rows.get(i)).map(|r| r.{}.clone()) }} else {{ None }};", key));
            self.w("    rows.truncate(n);  // 1ページ分（並びは上で決めてある）");
        }
        if q.select.is_empty() {
            self.w(format!("    {}rows.into_iter().map(|r| r.{}.clone()).collect(){}\n}}\n", if pg { "(" } else { "" }, key, if pg { ", next)" } else { "" }));
            return;
        }
        self.w(format!("    {}rows.into_iter().filter_map(|r| {{", if pg { "(" } else { "" }));
        let mut vias: IndexSet<&String> = IndexSet::new();
        for c in &q.select {
            if c.kind == "参照先" {
                vias.insert(c.via.as_ref().unwrap());
            }
        }
        for via in vias {
            let vf = fields[via.as_str()];
            let tg = vf.r#type.target.clone().unwrap();
            if vf.r#type.optional {
                self.w(format!("        let j_{via} = match &r.{via} {{ Some(k) => Some(Store.{tg}.get(k)?), None => None }};  // 参照先が無い行は出さない", via = via, tg = tg));
            } else {
                self.w(format!("        let j_{via} = Store.{tg}.get(&r.{via})?;  // 参照先が無い行は出さない", via = via, tg = tg));
            }
        }
        let mut parts: Vec<String> = vec![];
        for (c, _) in &cols {
            if c.kind == "参照先" {
                let via = c.via.as_ref().unwrap();
                let vf = fields[via.as_str()];
                let tf = ck.data[vf.r#type.target.as_deref().unwrap()].fields.iter().find(|x| x.name == c.field).unwrap();
                if vf.r#type.optional {
                    if tf.r#type.optional {
                        parts.push(format!("{}: j_{}.and_then(|j| j.{}.clone())", c.name, via, c.field));
                    } else {
                        parts.push(format!("{}: j_{}.map(|j| j.{}.clone())", c.name, via, c.field));
                    }
                } else {
                    parts.push(format!("{}: j_{}.{}.clone()", c.name, via, c.field));
                }
            } else if c.kind == "合計" {
                if fields[c.field.as_str()].r#type.optional {
                    parts.push(format!("{}: r.{}.iter().flat_map(|m| m.values()).map(|v| *v as i128).sum::<i128>()", c.name, c.field));
                } else {
                    parts.push(format!("{}: r.{}.values().map(|v| *v as i128).sum::<i128>()", c.name, c.field));
                }
            } else if c.kind == "件数" {
                if fields[c.field.as_str()].r#type.optional {
                    parts.push(format!("{}: r.{}.as_ref().map_or(0, |m| m.len()) as i128", c.name, c.field));
                } else {
                    parts.push(format!("{}: r.{}.len() as i128", c.name, c.field));
                }
            } else if let Some(dv) = dt.derived.iter().find(|x| x.name == c.field) {
                fn calc(e: &Expr) -> String {
                    match e {
                        Expr::Num { v } => format!("{}i128", v),
                        Expr::Name { n } => format!("(r.{} as i128)", n),
                        Expr::Bin { op, l, r, .. } => format!("({}).wrapping_{}({})", calc(l), if op == "+" { "add" } else { "sub" }, calc(r)),
                        _ => panic!("定義した項目の式"),
                    }
                }
                parts.push(format!("{}: (({}) as {})", c.name, calc(dv.expr.as_ref().unwrap()), phys(&dv.r#type)));
            } else {
                parts.push(format!("{}: r.{}.clone()", c.name, c.field));
            }
        }
        if self.etag_data.contains(&q.target) {
            parts.push(format!("row_key_: r.{}.clone()", key));
        }
        self.w(format!("        Some({}Row {{ {} }})", n, parts.join(", ")));
        self.w(format!("    }}).collect(){}\n}}\n", if pg { ", next)" } else { "" }));
    }

    fn states(&mut self) {
        for s in &self.p.states {
            let mut vs = vec![s.initial.clone()];
            vs.extend(s.middle.iter().cloned());
            vs.extend(s.final_.iter().cloned());
            self.out.push("#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]".into());
            self.out.push(format!("pub enum {} {{ {} }}", s.name, vs.join(", ")));
            self.out.push(format!("impl {} {{", s.name));
            self.out.push("    pub fn from_name(s: &str) -> Option<Self> {\n        match s {".into());
            for v in &vs {
                self.out.push(format!("            \"{}\" => Some({}::{}),", v, s.name, v));
            }
            self.out.push("            _ => None,\n        }\n    }".into());
            self.out.push("    pub fn name(&self) -> &'static str {\n        match self {".into());
            for v in &vs {
                self.out.push(format!("            {}::{} => \"{}\",", s.name, v, v));
            }
            self.out.push("        }\n    }\n}\n".into());
        }
    }

    fn records(&mut self) {
        let p = self.p;
        for c in &p.composites {
            self.w("#[derive(Clone, Debug, PartialEq)]");
            self.w(format!("pub struct {} {{", c.target.clone().unwrap_or_default()));
            for (pn, pt) in c.parts.as_deref().unwrap_or(&[]) {
                self.w(format!("    pub {}: {},", pn, field_rust(pt)));
            }
            self.w("}\n");
        }
        for d in &p.data {
            self.w("#[derive(Clone, Debug, PartialEq)]");
            self.w(format!("pub struct {} {{", d.name));
            for f in &d.fields {
                self.w(format!("    pub {}: {},", f.name, field_rust(&f.r#type)));
            }
            self.w("}\n");
        }
        self.w("#[derive(Clone, Debug, Default)]\npub struct Store {");
        for d in &p.data {
            self.w(format!("    pub {}: BTreeMap<String, {}>,", d.name, d.name));
        }
        self.w("}\n");
        self.w("/// 書込で変わった行（実体の名前, キー）。サーバーは、保存をメモリに持ったまま、この行だけを DB へ書き戻す\npub type Changed = Vec<(&'static str, String)>;\n");
        self.w("impl Store {");
        self.w("    pub fn apply(&mut self, writes: Vec<Write>) -> Result<(), Fault> {\n        self.apply_tracked(writes, &mut Vec::new())\n    }");
        self.w("    /// 書込を適用し、変わった行を changed に足す（削除の cascade・detach で変わった参照元の行も含む）。\n    /// 途中で止まっても、それまでに変えた行は changed に入っている\n    pub fn apply_tracked(&mut self, writes: Vec<Write>, changed: &mut Changed) -> Result<(), Fault> {\n        for w in writes {\n            match w {");
        for (d, f) in self.write_kinds() {
            match f.as_deref() {
                None => {
                    let k = self.key(&d);
                    self.w(format!("                Write::{d}_New(r) => {{ changed.push((\"{d}\", r.{k}.clone())); self.{d}.insert(r.{k}.clone(), r); }}", d = d, k = k));
                }
                Some("削除") => self.w(format!("                Write::{d}_Del(id) => {{ self.del_{d}(&id, changed); }}", d = d)),
                Some(f) => self.w(format!("                Write::{d}_{f}(id, v) => {{ proven!(self.{d}.get_mut(&id), \"検査で示した書込先が無い: {d}\").{f} = v; changed.push((\"{d}\", id)); }}", d = d, f = f)),
            }
        }
        self.w("            }\n        }\n        Ok(())\n    }");
        self.delete_methods();
        self.w("    pub fn diff(&self, after: &Store) -> Vec<String> {\n        let mut out: Vec<String> = Vec::new();");
        for d in &p.data {
            let key = self.key(&d.name);
            let others: Vec<&Field> = d.fields.iter().filter(|f| f.name != key).collect();
            self.w(format!("        for (id, r) in &after.{n} {{\n            match self.{n}.get(id) {{", n = d.name));
            let fmt = others.iter().map(|f| format!("{}={{}}", f.name)).collect::<Vec<_>>().join(", ");
            let args = others.iter().map(|f| show_expr(&f.r#type, &format!("r.{}", f.name))).collect::<Vec<_>>().join(", ");
            self.w(format!("                None => out.push(format!(\"{}({{}})新規({})\", id{})),", d.name, fmt, if args.is_empty() { String::new() } else { format!(", {}", args) }));
            self.w("                Some(b) => {");
            for f in &others {
                self.w(format!(
                    "                    if b.{f} != r.{f} {{ out.push(format!(\"{d}({{}}).{f}:{{}}→{{}}\", id, {a}, {b})); }}",
                    f = f.name,
                    d = d.name,
                    a = show_expr(&f.r#type, &format!("b.{}", f.name)),
                    b = show_expr(&f.r#type, &format!("r.{}", f.name))
                ));
            }
            self.w("                }\n            }\n        }");
            self.w(format!("        for id in self.{n}.keys() {{ if !after.{n}.contains_key(id) {{ out.push(format!(\"{n}({{}})削除\", id)); }} }}", n = d.name));
        }
        self.w("        out.sort();\n        out\n    }\n}\n");
    }

    fn delete_methods(&mut self) {
        let mut deleted: Vec<String> = self.ck.infos.values().flat_map(|i| i.deletes.iter().cloned()).collect::<IndexSet<_>>().into_iter().collect();
        deleted.sort();
        let mut todo: Vec<String> = deleted;
        let mut seen: Vec<String> = vec![];
        while !todo.is_empty() {
            let d = todo.remove(0);
            if seen.contains(&d) {
                continue;
            }
            seen.push(d.clone());
            let dt = self.ck.data[&d];
            let mut lines = vec![
                format!("    fn del_{}(&mut self, id: &str, changed: &mut Changed) {{", d),
                format!("        if self.{d}.remove(id).is_some() {{ changed.push((\"{d}\", id.to_string())); }}", d = d),
            ];
            for (src, fld, pol) in &dt.on_delete {
                let f = self.ck.data[src.as_str()].fields.iter().find(|x| &x.name == fld).unwrap();
                if pol == "detach" {
                    lines.push(format!("        for (k, r) in self.{src}.iter_mut() {{ if r.{fld}.as_deref() == Some(id) {{ r.{fld} = None; changed.push((\"{src}\", k.clone())); }} }}", src = src, fld = fld));
                } else if pol == "cascade" {
                    let test = if f.r#type.optional { format!("r.{}.as_deref() == Some(id)", fld) } else { format!("r.{} == id", fld) };
                    lines.push(format!("        let gone: Vec<String> = self.{}.iter().filter(|(_, r)| {}).map(|(k, _)| k.clone()).collect();", src, test));
                    lines.push(format!("        for k in gone {{ self.del_{}(&k, changed); }}", src));
                    todo.push(src.clone());
                }
            }
            lines.push("    }".into());
            self.w(lines.join("\n"));
        }
    }

    pub fn write_kinds(&self) -> Vec<(String, Option<String>)> {
        let mut kinds: Vec<(String, Option<String>)> = vec![];
        for proc in &self.p.processes {
            let info = &self.ck.infos[&proc.name];
            let mut ws: Vec<&String> = info.writes.iter().collect();
            ws.sort();
            for w in ws {
                let (d, f) = match w.split_once('.') {
                    Some((a, b)) => (a.to_string(), if b.is_empty() { None } else { Some(b.to_string()) }),
                    None => (w.clone(), None),
                };
                let k = (d, f);
                if !kinds.contains(&k) {
                    kinds.push(k);
                }
            }
            let mut ds: Vec<&String> = info.deletes.iter().collect();
            ds.sort();
            for d in ds {
                let k = (d.clone(), Some("削除".to_string()));
                if !kinds.contains(&k) {
                    kinds.push(k);
                }
            }
        }
        kinds
    }

    fn writes_enum(&mut self) {
        self.w("#[derive(Clone, Debug, PartialEq)]\npub enum Write {");
        for (d, f) in self.write_kinds() {
            match f.as_deref() {
                None => self.w(format!("    {d}_New({d}),", d = d)),
                Some("削除") => self.w(format!("    {}_Del(String),", d)),
                Some(f) => {
                    let t = &self.ck.data[&d].fields.iter().find(|x| x.name == f).unwrap().r#type;
                    self.w(format!("    {}_{}(String, {}),", d, f, field_rust(t)));
                }
            }
        }
        self.w("}\n");
    }

    fn parts(&mut self) {
        let ck = self.ck;
        for pt in &self.p.parts {
            if pt.params.iter().any(|(_, t)| t.kind == "生") {
                self.raw_convert(pt);
            }
        }
        if self.need_os_path {
            self.w(OS_PATH_RS);
        }
        if self.p.parts.is_empty() {
            return;
        }
        let libs: Vec<&Part> = self.p.parts.iter().filter(|pt| pt.external() && ck.used_libs.contains(&pt.name)).collect();
        if libs.is_empty() {
            return;
        }
        self.w("/// 根の言語のライブラリを呼ぶ関数の置き場。ここだけは、人が保証した外のコードを呼ぶので lint の禁止を外す。\n/// 呼ぶ側（生成した処理）が、戻り値を宣言の範囲と保証で確かめる。ライブラリが panic しても Fault にして続ける\n#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects)]\nmod library {\n    #[allow(unused_imports)]\n    use super::*;");
        for pt in libs {
            let (_, _, expr, _, why) = pt.lib.as_ref().unwrap();
            let ps = pt.params.iter().map(|(n, t)| format!("{}: {}", n, rust_type(t))).collect::<Vec<_>>().join(", ");
            self.w(format!(
                "    /// {}\n    pub fn {}({}) -> Result<{}, String> {{\n        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> {} {{ {} }})).map_err(|_| \"ライブラリが panic しました\".to_string())\n    }}",
                doc_line(why),
                pt.name,
                ps,
                rust_type(&pt.ret),
                rust_type(&pt.ret),
                expr
            ));
        }
        self.w("}");
    }

    fn raw_convert(&mut self, pt: &Part) {
        let n = &pt.params[0].0;
        self.w(format!("/// 入力の via で結ぶ変換（入力の選別を通す境界）。拒否は入力不正\npub fn {}({}: &str) -> Result<String, String> {{", pt.name, n));
        for (cond, why) in &pt.requires {
            let c = self.raw_rx(cond, n);
            self.w(format!("    if !({}) {{ return Err({}.to_string()); }}", c, rust_str(why)));
        }
        let b = self.raw_rx(pt.body.as_ref().unwrap(), n);
        self.w(format!("    Ok({})\n}}", b));
    }

    /// raw の変換の中の式。数は i128 で、あふれは入力不正にする
    fn raw_rx(&mut self, e: &Expr, r: &str) -> String {
        match e {
            Expr::Num { v } => format!("{}i128", v),
            Expr::Name { n } => n.clone(),
            Expr::Call { n, args } if n == "len" => format!("({}.chars().count() as i128)", self.raw_rx(&args[0], r)),
            Expr::Call { n, args } if n == "text" => format!("{}.to_string()", self.raw_rx(&args[0], r)),
            Expr::Call { n, args } if n == "os_path" => {
                self.need_os_path = true;
                format!("os_path_expand({})?", self.raw_rx(&args[0], r))
            }
            Expr::Call { n, args } if n == "only" => {
                let s = match &args[1] {
                    Expr::StrLit { s } => s,
                    _ => panic!("only の第2引数"),
                };
                format!("{}.chars().all(|c| ({}).iter().any(|&(a, b)| a <= c && c <= b))", self.raw_rx(&args[0], r), chars_rust(&parse_chars(s).unwrap()))
            }
            Expr::Call { n, args } if n == "digit" => format!(
                "(usize::try_from({}).ok().and_then(|k| {}.chars().nth(k)).and_then(|c| c.to_digit(10)).map_or(0i128, i128::from))",
                self.raw_rx(&args[1], r),
                self.raw_rx(&args[0], r)
            ),
            Expr::Call { n, args } if n == "min" || n == "max" => {
                let mut out = self.raw_rx(&args[0], r);
                for a in &args[1..] {
                    out = format!("({}).{}({})", out, n, self.raw_rx(a, r));
                }
                out
            }
            Expr::Call { n, args } if n == "abs" => format!("({}).checked_abs().ok_or_else(|| \"計算があふれました\".to_string())?", self.raw_rx(&args[0], r)),
            Expr::Cond { c, a, b } => format!("(if {} {{ {} }} else {{ {} }})", self.raw_rx(c, r), self.raw_rx(a, r), self.raw_rx(b, r)),
            Expr::SumE { elem, binder, .. } => {
                let src = match &*binder.src {
                    Expr::Call { args, .. } => &args[0],
                    _ => panic!("sum の束縛"),
                };
                let i = &binder.names[0];
                format!(
                    "{{ let mut acc: i128 = 0; for {} in 0..({}.chars().count() as i128) {{ acc = acc.checked_add({}).ok_or_else(|| \"計算があふれました\".to_string())?; }} acc }}",
                    i,
                    self.raw_rx(src, r),
                    self.raw_rx(elem, r)
                )
            }
            Expr::Bin { op, l, r: rr, dflt } => {
                let ls = self.raw_rx(l, r);
                let rs = self.raw_rx(rr, r);
                if op == "+" || op == "-" || op == "*" {
                    let f = match op.as_str() {
                        "+" => "checked_add",
                        "-" => "checked_sub",
                        _ => "checked_mul",
                    };
                    return format!("({}).{}({}).ok_or_else(|| \"計算があふれました\".to_string())?", ls, f, rs);
                }
                if (op == "/" || op == "%") && matches!(&**rr, Expr::Num { .. }) {
                    return format!("({}).{}({})", ls, if op == "/" { "div_euclid" } else { "rem_euclid" }, rs);
                }
                if op == "/" || op == "%" {
                    let f = if op == "/" { "checked_div_euclid" } else { "checked_rem_euclid" };
                    return match dflt {
                        None => format!("({}).{}({}).ok_or_else(|| \"0 では割れません\".to_string())?", ls, f, rs),
                        Some(d) => format!(
                            "{{ let d_ = {}; if d_ == 0 {{ {} }} else {{ ({}).{}(d_).ok_or_else(|| \"計算があふれました\".to_string())? }} }}",
                            rs,
                            self.raw_rx(d, r),
                            ls,
                            f
                        ),
                    };
                }
                let o = match op.as_str() {
                    "かつ" => "&&",
                    "または" => "||",
                    o => o,
                };
                format!("({} {} {})", ls, o, rs)
            }
            Expr::Not { e } => format!("(!{})", self.raw_rx(e, r)),
            Expr::BoolLit { v } => if *v { "true" } else { "false" }.into(),
            _ => panic!("raw の変換の式に使えません: {}", show(e)),
        }
    }

    fn effects(&mut self) {
        self.w("#[derive(Clone, Debug, PartialEq)]\npub enum Effect {");
        for e in &self.p.effects {
            let fs = e.params.iter().map(|(n, t)| format!("{}: {}", n, rust_type(t))).collect::<Vec<_>>().join(", ");
            self.out.push(format!("    {} {{ {}, idem_key: String }},", e.name.replace('.', "_"), fs));
        }
        self.out.push(format!("}}\nimpl Effect {{\n    pub fn show(&self) -> String {{\n        match {} {{", if self.p.effects.is_empty() { "*self" } else { "self" }));
        for e in &self.p.effects {
            let names: Vec<&str> = e.params.iter().map(|(n, _)| n.as_str()).collect();
            let pat = names.join(", ");
            let fmt = names.iter().map(|_| "{}").collect::<Vec<_>>().join(", ");
            let shown = e.params.iter().map(|(n, t)| show_expr(t, n)).collect::<Vec<_>>().join(", ");
            self.out.push(format!("            Effect::{} {{ {}, idem_key }} => format!(\"{}({}, 冪等キー={{}})\", {}, idem_key),", e.name.replace('.', "_"), pat, e.name, fmt, shown));
        }
        self.out.push("        }\n    }\n}\n".into());
    }

    fn results(&mut self) {
        for r in &self.p.results {
            self.w("#[derive(Clone, Debug, PartialEq)]");
            self.w(format!("pub enum {} {{", r.name));
            let vs: Vec<(&String, Vec<&Type>)> = r.variants.iter().map(|(v, ps)| (v, ps.iter().map(|(_, t)| t).collect())).collect();
            for (v, ts) in &vs {
                if ts.is_empty() {
                    self.w(format!("    {},", v));
                } else {
                    self.w(format!("    {}({}),", v, ts.iter().map(|t| rust_type(t)).collect::<Vec<_>>().join(", ")));
                }
            }
            self.w("}");
            self.w(format!("impl {} {{\n    pub fn show(&self) -> String {{\n        match self {{", r.name));
            for (v, ts) in &vs {
                if !ts.is_empty() {
                    let names: Vec<String> = (0..ts.len()).map(|i| format!("a{}", i)).collect();
                    let shown = ts.iter().zip(names.iter()).map(|(t, n)| if t.kind != "ID" { show_expr(t, n) } else { n.clone() }).collect::<Vec<_>>().join(", ");
                    let fmt = ts.iter().map(|_| "{}").collect::<Vec<_>>().join(", ");
                    self.w(format!("            {}::{}({}) => format!(\"{}({})\", {}),", r.name, v, names.join(", "), v, fmt, shown));
                } else {
                    self.w(format!("            {}::{} => \"{}\".to_string(),", r.name, v, v));
                }
            }
            self.w("        }\n    }");
            self.w("    pub fn name(&self) -> &'static str {\n        match self {");
            for (v, ts) in &vs {
                self.w(format!("            {}::{}{} => \"{}\",", r.name, v, if ts.is_empty() { "" } else { "(..)" }, v));
            }
            self.w("        }\n    }");
            self.w("    pub fn args(&self) -> Vec<String> {\n        match self {");
            for (v, ts) in &vs {
                if !ts.is_empty() {
                    let names: Vec<String> = (0..ts.len()).map(|i| format!("a{}", i)).collect();
                    let shown = ts.iter().zip(names.iter()).map(|(t, n)| format!("format!(\"{{}}\", {})", show_expr(t, n))).collect::<Vec<_>>().join(", ");
                    self.w(format!("            {}::{}({}) => vec![{}],", r.name, v, names.join(", "), shown));
                } else {
                    self.w(format!("            {}::{} => vec![],", r.name, v));
                }
            }
            self.w("        }\n    }\n}\n");
        }
    }

    fn flows(&mut self) {
        self.w("// 【フロー】：結果の種類ごとの行き先。match の網羅は Rust が検査する。");
        for fl in &self.p.flows {
            for (proc, routes) in &fl.entries {
                let rdef = self.ck.results[&self.ck.procs[proc.as_str()].result];
                let m = route_map(routes);
                let many = m.values().any(|ds| ds.len() > 1);
                let one = |dest: &str| -> String {
                    if dest == "reply" {
                        "Dest::Reply".into()
                    } else if let Some(r) = dest.strip_prefix("wait ") {
                        format!("Dest::Wait(\"{}\")", r)
                    } else {
                        format!("Dest::Deed(\"{}\")", dest)
                    }
                };
                self.w(format!("pub fn {}_{}(r: &{}) -> {} {{\n    match r {{", fl.name, proc, rdef.name, if many { "Vec<Dest>" } else { "Dest" }));
                for (v, ts) in &rdef.variants {
                    let pat = format!("{}::{}{}", rdef.name, v, if ts.is_empty() { "" } else { "(..)" });
                    let ds: Vec<String> = m[v].iter().map(|d| one(d)).collect();
                    if many {
                        self.w(format!("        {} => vec![{}],", pat, ds.join(", ")));
                    } else {
                        self.w(format!("        {} => {},", pat, ds[0]));
                    }
                }
                self.w("    }\n}\n");
            }
        }
    }

    fn triggers(&mut self) {
        if self.p.triggers.is_empty() {
            return;
        }
        self.w("// 【トリガ】：待ちの宣言。要因ごとの行き先。作成・発火・期限の監視は基盤の仕事。");
        if !self.p.replies.is_empty() {
            self.w("pub const replies: &[ReplyDef] = &[");
            for rp in &self.p.replies {
                let fs = rp.params.iter().map(|(n, t)| format!("(\"{}\", \"{}\", {})", n, show_type(t).unwrap(), if t.optional { "false" } else { "true" })).collect::<Vec<_>>().join(", ");
                self.w(format!("    ReplyDef {{ name: \"{}\", fields: &[{}] }},", rp.name, fs));
            }
            self.w("];");
        }
        self.w("pub const triggers: &[TriggerDef] = &[");
        for tg in &self.p.triggers {
            let cs = tg.causes.iter().map(|(k, key, d)| format!("(\"{}\", \"{}\", \"{}\")", k, key, d)).collect::<Vec<_>>().join(", ");
            self.w(format!("    TriggerDef {{ name: \"{}\", causes: &[{}] }},", tg.name, cs));
        }
        self.w("];\n");
    }
}

type Reads = Vec<(String, Option<String>)>;

fn reads_in(x: &Expr, derived: &IndexMap<String, IndexSet<String>>, out: &mut Reads) {
    match x {
        Expr::Attr { e, n } if matches!(&**e, Expr::Name { .. }) => {
            let v = match &**e {
                Expr::Name { n } => n.clone(),
                _ => unreachable!(),
            };
            out.push((v.clone(), Some(n.clone())));
            if let Some(ps) = derived.get(n) {
                for part in ps {
                    out.push((v.clone(), Some(part.clone())));
                }
            }
        }
        Expr::Name { n } => out.push((n.clone(), None)),
        Expr::Tuple { items } => items.iter().for_each(|i| reads_in(i, derived, out)),
        Expr::Not { e } | Expr::Attr { e, .. } | Expr::Index { e, .. } | Expr::Pred { e, .. } => reads_in(e, derived, out),
        Expr::Cond { c, a, b } => {
            reads_in(c, derived, out);
            reads_in(a, derived, out);
            reads_in(b, derived, out);
        }
        Expr::Call { args, .. } => args.iter().for_each(|i| reads_in(i, derived, out)),
        Expr::Make { fields, .. } => fields.iter().for_each(|(_, i)| reads_in(i, derived, out)),
        Expr::Bin { l, r, dflt, .. } => {
            reads_in(l, derived, out);
            reads_in(r, derived, out);
            if let Some(d) = dflt {
                reads_in(d, derived, out);
            }
        }
        Expr::SetB { elem, binder, cond } | Expr::SumE { elem, binder, cond } => {
            reads_in(elem, derived, out);
            reads_in(&binder.src, derived, out);
            if let Some(c) = cond {
                reads_in(c, derived, out);
            }
        }
        Expr::Implies { l, r } => {
            reads_in(l, derived, out);
            reads_in(r, derived, out);
        }
        Expr::Num { .. } | Expr::BoolLit { .. } | Expr::StrLit { .. } | Expr::Null | Expr::Dur { .. } => {}
    }
}

fn reads_in_stmt(st: &Stmt, derived: &IndexMap<String, IndexSet<String>>, out: &mut Reads) {
    match st {
        Stmt::Return(r) => r.args.iter().for_each(|a| reads_in(a, derived, out)),
        Stmt::Assign(a) => {
            reads_in(&a.e, derived, out);
            if let Some(r) = &a.absent {
                r.args.iter().for_each(|x| reads_in(x, derived, out));
            }
        }
        Stmt::Create(c) => c.fields.iter().for_each(|(_, x)| reads_in(x, derived, out)),
        Stmt::Delete(_) => {}
        Stmt::If(i) => {
            reads_in(&i.cond, derived, out);
            i.body.iter().for_each(|s| reads_in_stmt(s, derived, out));
            if let Some(e) = &i.els {
                e.iter().for_each(|s| reads_in_stmt(s, derived, out));
            }
        }
        Stmt::Update(u) => {
            reads_in(&u.target, derived, out);
            reads_in(&u.value, derived, out);
            reads_in(&u.binder.src, derived, out);
            if let Some(c) = &u.cond {
                reads_in(c, derived, out);
            }
        }
        Stmt::SetField(s) => reads_in(&s.e, derived, out),
        Stmt::Request(r) => {
            r.args.iter().for_each(|a| reads_in(a, derived, out));
            if let Some(i) = &r.idem {
                reads_in(i, derived, out);
            }
        }
        Stmt::ForEach(f) => {
            reads_in(&f.binder.src, derived, out);
            f.body.iter().for_each(|s| reads_in_stmt(s, derived, out));
        }
    }
}

type Done = HashSet<(String, String)>;

fn hit_of(reads: Reads, done: &Done) -> Done {
    let mut hit = Done::new();
    for (v, f) in reads {
        match f {
            None => {
                for w in done {
                    if w.0 == v {
                        hit.insert(w.clone());
                    }
                }
            }
            Some(f) => {
                if done.contains(&(v.clone(), f.clone())) {
                    hit.insert((v, f));
                }
            }
        }
    }
    hit
}

/// 書いた後に読む (変数, 項目)。done は、そこまでのどれかの道で書いた (変数, 項目)（書き足していく）
fn reads_after_writes(stmts: &[Stmt], done: &mut Done, derived: &IndexMap<String, IndexSet<String>>) -> Done {
    let mut hit = Done::new();
    for st in stmts {
        match st {
            Stmt::If(i) => {
                let mut r = Reads::new();
                reads_in(&i.cond, derived, &mut r);
                hit.extend(hit_of(r, done));
                let mut then_done = done.clone();
                let mut else_done = done.clone();
                hit.extend(reads_after_writes(&i.body, &mut then_done, derived));
                let els: &[Stmt] = i.els.as_deref().unwrap_or(&[]);
                hit.extend(reads_after_writes(els, &mut else_done, derived));
                for (body, d) in [(&i.body[..], then_done), (els, else_done)] {
                    if !always_returns(body) {
                        done.extend(d);
                    }
                }
            }
            Stmt::ForEach(f) => {
                let mut r = Reads::new();
                reads_in(&f.binder.src, derived, &mut r);
                hit.extend(hit_of(r, done));
                hit.extend(reads_after_writes(&f.body, done, derived));
            }
            _ => {
                let mut r = Reads::new();
                reads_in_stmt(st, derived, &mut r);
                hit.extend(hit_of(r, done));
                if let Stmt::SetField(s) = st {
                    done.insert((s.var.clone(), s.f.clone()));
                }
            }
        }
    }
    hit
}

pub struct ProcGen<'a, 'b> {
    ck: &'b Checker<'a>,
    proc: &'a Process,
    info: ProcInfo,
    rdef: &'a ResultDef,
    scope: IndexMap<String, (String, String)>,
    lines: Vec<String>,
    ind: usize,
    null_inputs: IndexSet<String>,
    in_loop: bool,
    local_fields: Done,
    copied: IndexSet<String>,
    owned: IndexSet<String>,
    fills: IndexMap<(String, String), i128>,
    derived_parts: IndexMap<String, IndexSet<String>>,
    key_of: IndexMap<String, String>,
}

impl<'a, 'b> ProcGen<'a, 'b> {
    pub fn new(g: &Gen<'a, 'b>, proc: &'a Process) -> Self {
        let ck = g.ck;
        let mut info = ck.infos[&proc.name].clone();
        let rdef = ck.results[&proc.result];
        for (k, v) in &ck.part_types {
            info.types.insert(*k, v.clone());
        }
        let mut derived_parts: IndexMap<String, IndexSet<String>> = IndexMap::new();
        for d in &ck.p.data {
            for f in &d.derived {
                derived_parts.entry(f.name.clone()).or_default().extend(names_in(f.expr.as_ref().unwrap()));
            }
        }
        let mut done = Done::new();
        let local_fields = reads_after_writes(&proc.body, &mut done, &derived_parts);
        let copied: IndexSet<String> = local_fields.iter().map(|(v, _)| v.clone()).collect::<Vec<_>>().into_iter().collect();
        let key_of = ck.p.data.iter().map(|d| (d.name.clone(), g.key(&d.name))).collect();
        ProcGen {
            ck,
            proc,
            info,
            rdef,
            scope: IndexMap::new(),
            lines: vec![],
            ind: 1,
            null_inputs: IndexSet::new(),
            in_loop: false,
            local_fields,
            copied,
            owned: IndexSet::new(),
            fills: IndexMap::new(),
            derived_parts,
            key_of,
        }
    }

    fn l(&mut self, s: impl AsRef<str>) {
        self.lines.push(format!("{}{}", "    ".repeat(self.ind), s.as_ref()));
    }

    fn tok(&self, e: &Expr) -> String {
        self.info.types.get(&id_of(e)).cloned().unwrap_or_else(|| panic!("型の記録が無い式: {}", show(e)))
    }

    fn wrap(&self, tok: &str, x: &str) -> String {
        if tok == "Int" || tok == "Money" {
            format!("({} as i128)", x)
        } else if tok == "ID" || tok == "Str" || tok == "Bytes" || tok == "Raw" || tok.starts_with("Map:") || tok.starts_with("Comp:") || tok == "Set" {
            format!("{}.clone()", x)
        } else {
            x.to_string()
        }
    }

    fn nullable(&self, base: &Expr, n: &str) -> bool {
        let bt = self.tok(base);
        if let Some(c) = bt.strip_prefix("Comp:") {
            let parts = self.ck.comps[c].parts.as_deref().unwrap_or(&[]);
            return parts.iter().find(|(pn, _)| pn == n).map_or(false, |(_, t)| t.optional);
        }
        if let Some(d) = bt.strip_prefix("Rec:") {
            return self.ck.data[d].fields.iter().find(|f| f.name == n).map_or(false, |f| f.r#type.optional);
        }
        false
    }

    /// 範囲を通す 1 件引き：範囲の外の行は「無い」にする（Option の filter）。範囲が無ければ空
    fn scope_filter(&self, d: &str, scopes: &[String]) -> String {
        if scopes.is_empty() {
            return String::new();
        }
        let fields: IndexMap<&str, &Field> = self.ck.data[d].fields.iter().map(|f| (f.name.as_str(), f)).collect();
        let mut by: IndexMap<&str, &Scope> = IndexMap::new();
        for s in self.ck.p.scopes.iter().rev() {
            by.insert(s.name.as_str(), s);
        }
        let atom = |f: &str, op: &str, st: &str| -> String {
            if !(st.chars().all(|c| c.is_ascii_digit()) && !st.is_empty()) {
                return format!("r.{} {} {}::{}", f, op, fields[f].r#type.target.clone().unwrap_or_default(), st);
            }
            if fields[f].r#type.optional {
                return format!("r.{}.map_or(false, |v| (v as i128) {} {})", f, op, st);
            }
            format!("(r.{} as i128) {} {}", f, op, st)
        };
        let mut conds: Vec<String> = vec![];
        for n in scopes {
            let s = by[n.as_str()];
            for g in &s.conds {
                conds.push(format!("({})", g.iter().map(|(f, op, st)| atom(f, op, st)).collect::<Vec<_>>().join(" || ")));
            }
            if !s.caller.is_empty() {
                if fields[s.caller.as_str()].r#type.optional {
                    conds.push(format!("r.{}.as_deref() == Some(Input.caller.as_str())", s.caller));
                } else {
                    conds.push(format!("r.{} == Input.caller", s.caller));
                }
            }
        }
        if conds.is_empty() {
            String::new()
        } else {
            format!(".filter(|r| {})", conds.join(" && "))
        }
    }

    fn place(&self, e: &Expr) -> String {
        match e {
            Expr::Name { n } => {
                let kind = &self.scope[n].0;
                match kind.as_str() {
                    "input" => format!("Input.{}", n),
                    "ref" => format!("ctx.{}", n),
                    _ => n.clone(),
                }
            }
            Expr::Attr { e, n } => format!("{}.{}", self.place(e), n),
            Expr::Index { d, e, .. } => format!("Store.{}.get(&({})).ok_or(Fault::Unproven(\"検査で示した参照先が無い: {}\"))?", d, self.rx(e), d),
            _ => panic!("place: {}", show(e)),
        }
    }

    /// 部品を呼び、戻り値が型の範囲と保証に合うかをその場で確かめる。違反は保証違反（処理を止める）
    fn part_call(&mut self, name: &str, args: &[Expr]) -> String {
        let pt = self.ck.parts[name];
        let local = |t: &Type| -> String {
            if t.kind == "整数" || t.kind == "金額" {
                "i128".into()
            } else {
                rust_type(t)
            }
        };
        let mut lines: Vec<String> = vec![];
        let mut cargs: Vec<String> = vec![];
        for ((n, t), a) in pt.params.iter().zip(args.iter()) {
            lines.push(format!("let {}: {} = {};", n, local(t), self.rx(a)));
            cargs.push(if t.kind == "整数" || t.kind == "金額" { self.conv(t, n, &format!("{}の{}", name, n), false) } else { format!("{}.clone()", n) });
        }
        let call: String;
        if let Some(body) = &pt.body {
            let saved = self.scope.clone();
            for (n, t) in &pt.params {
                self.scope.insert(n.clone(), ("local".into(), tok_of(t)));
            }
            self.fills = pt.params.iter().flat_map(|(n, t)| t.fills.iter().flatten().map(move |(c, v)| ((n.clone(), c.clone()), *v))).collect();
            call = self.rx(body);
            self.fills = IndexMap::new();
            self.scope = saved;
        } else {
            call = format!("library::{}({}).map_err(|m| Fault::Guarantee(\"{}\", m))?", name, cargs.join(", "), name);
        }
        let ret_num = pt.ret.kind == "整数" || pt.ret.kind == "金額";
        lines.push(format!("let result: {} = {};", local(&pt.ret), if ret_num { format!("{} as i128", call) } else { call }));
        if pt.body.is_some() {
            return format!("{{ {} result }}", lines.join(" "));
        }
        let mut checks: Vec<String> = vec![];
        if ret_num {
            if let Some(r) = &pt.ret.range {
                checks.push(format!("if let Err(m) = check_range(\"result\", result, {}, {}) {{ return Err(Fault::Guarantee(\"{}\", m)) }}", opt(r.lo), opt(r.hi), name));
            }
        }
        if pt.ret.kind == "文字列" {
            if let Some(r) = &pt.ret.range {
                checks.push(format!("if let Err(m) = check_range(\"result\", result.chars().count() as i128, {}, {}) {{ return Err(Fault::Guarantee(\"{}\", m)) }}", opt(r.lo), opt(r.hi), name));
            }
            if let Some(cs) = &pt.ret.chars {
                checks.push(format!("if let Err(m) = check_chars(\"result\", &result, {}) {{ return Err(Fault::Guarantee(\"{}\", m)) }}", chars_rust(cs), name));
            }
            if let Some(rd) = &pt.ret.read {
                checks.push(format!("if let Err(m) = {}(&result) {{ return Err(Fault::Guarantee(\"{}\", m)) }}", rd, name));
            }
        }
        if let Some(gu) = &pt.guarantee {
            let saved = self.scope.clone();
            for (n, t) in pt.params.iter().chain(std::iter::once(&("result".to_string(), pt.ret.clone()))) {
                self.scope.insert(n.clone(), ("local".into(), tok_of(t)));
            }
            let cond = self.rx(gu);
            self.scope = saved;
            checks.push(format!("if !({}) {{ return Err(Fault::Guarantee(\"{}\", {}.to_string())) }}", cond, name, rust_str(&show(gu))));
        }
        let level = &pt.lib.as_ref().map(|l| l.3.clone()).unwrap_or_default();
        if level == "verify" && !checks.is_empty() {
            checks = vec![format!("if cfg!(debug_assertions) {{ {} }}", checks.join(" "))];
        } else if level == "assume" {
            checks = vec![];
        }
        lines.extend(checks);
        format!("{{ {} result }}", lines.join(" "))
    }

    /// 読むだけの場所。無いかもしれない入力は、検査で確かめてあるので中身を借りる
    fn src(&self, e: &Expr) -> String {
        if let Expr::Name { n } = e {
            if self.null_inputs.contains(n) && self.scope.get(n).map_or(false, |s| s.0 == "input") {
                return format!("proven!({}.as_ref(), \"検査で示した入力が無い: {}\")", self.place(e), n);
            }
        }
        self.place(e)
    }

    /// 集合・写像を、借りて読む場所。null になりうる項目は、検査で null でないと確かめてあるので中身を借りる
    fn coll(&self, e: &Expr) -> String {
        if let Expr::Attr { e: base, n } = e {
            if self.nullable(base, n) {
                return format!("proven!({}.as_ref(), \"検査で示した項目が null: {}\")", self.place(e), n);
            }
        }
        self.src(e)
    }

    fn rx(&self, e: &Expr) -> String {
        // part_call は scope と fills を一時的に書き換えるので、可変の経路を通す（内部可変の代わりに、複製した作業用の自分で呼ぶ）
        let mut me = self.snapshot();
        me.rx_mut(e)
    }

    fn snapshot(&self) -> ProcGen<'a, 'b> {
        ProcGen {
            ck: self.ck,
            proc: self.proc,
            info: self.info.clone(),
            rdef: self.rdef,
            scope: self.scope.clone(),
            lines: vec![],
            ind: self.ind,
            null_inputs: self.null_inputs.clone(),
            in_loop: self.in_loop,
            local_fields: self.local_fields.clone(),
            copied: self.copied.clone(),
            owned: self.owned.clone(),
            fills: self.fills.clone(),
            derived_parts: self.derived_parts.clone(),
            key_of: self.key_of.clone(),
        }
    }

    fn rx_mut(&mut self, e: &Expr) -> String {
        match e {
            Expr::Num { v } => return format!("{}i128", v),
            Expr::Dur { secs, .. } => return format!("{}i64", secs),
            _ => {}
        }
        if let Some(op) = self.info.derived.get(&id_of(e)).cloned() {
            return self.rx_mut(&op);
        }
        match e {
            Expr::StrLit { s } => format!("{}.to_string()", rust_str(s)),
            Expr::Name { n } => {
                if let Some((kind, tok)) = self.scope.get(n).cloned() {
                    let x = self.place(e);
                    if kind == "input" && self.null_inputs.contains(n) {
                        return self.wrap(&tok, &format!("proven!({}.clone(), \"検査で示した入力が無い: {}\")", x, n));
                    }
                    if kind == "local" && (tok == "Int" || tok == "Money" || tok == "Rational" || tok == "Ratio") {
                        return x;
                    }
                    if kind == "ref" {
                        return self.wrap(&tok, &x);
                    }
                    if tok.starts_with("Rec:") {
                        return if self.owned.contains(n) { format!("(&{})", x) } else { x };
                    }
                    return self.wrap(&tok, &x);
                }
                format!("{}::{}", self.ck.variants[n], n)
            }
            Expr::Attr { e: base, n } => {
                if let Expr::Name { n: bn } = &**base {
                    if let Some(v) = self.fills.get(&(bn.clone(), n.clone())).copied() {
                        return self.wrap(&self.tok(e), &format!("{}.unwrap_or({})", self.place(e), v));
                    }
                }
                if self.nullable(base, n) {
                    return self.wrap(&self.tok(e), &format!("proven!({}.clone(), \"検査で示した項目が null: {}\")", self.place(e), n));
                }
                self.wrap(&self.tok(e), &self.place(e))
            }
            Expr::Index { .. } => self.place(e),
            Expr::Make { n, fields, .. } => {
                let ct = self.ck.comps[n];
                let cparts = ct.parts.as_deref().unwrap_or(&[]);
                let mut vals: Vec<String> = vec![];
                for (fname, x) in fields {
                    let pt = &cparts.iter().find(|(pn, _)| pn == fname).unwrap().1;
                    let num = pt.kind == "整数" || pt.kind == "金額";
                    if matches!(x, Expr::Null) {
                        vals.push(format!("{}: None", fname));
                        continue;
                    }
                    let xr = self.rx_mut(x);
                    let v = if num { self.conv(pt, &xr, &format!("{}.{}", n, fname), true) } else { xr };
                    vals.push(if pt.optional { format!("{}: Some({})", fname, v) } else { format!("{}: {}", fname, v) });
                }
                for (pn, pt) in cparts {
                    if pt.optional && !fields.iter().any(|(f, _)| f == pn) {
                        vals.push(format!("{}: None", pn));
                    }
                }
                format!("{} {{ {} }}", n, vals.join(", "))
            }
            Expr::Pred { e: inner, kind } if kind == "null" => format!("{}.is_none()", self.place(inner)),
            Expr::Null => "None".into(),
            Expr::BoolLit { v } => if *v { "true" } else { "false" }.into(),
            Expr::Not { e: inner } => format!("(!{})", self.rx_mut(inner)),
            Expr::Cond { c, a, b } => format!("(if {} {{ {} }} else {{ {} }})", self.rx_mut(c), self.rx_mut(a), self.rx_mut(b)),
            Expr::Pred { e: inner, kind } => {
                if kind == "ある" || kind == "ない" {
                    let (d, k) = match &**inner {
                        Expr::Index { d, e, .. } => (d, e),
                        _ => panic!("ある・ない の対象"),
                    };
                    let c = format!("Store.{}.contains_key(&({}))", d, self.rx_mut(k));
                    return if kind == "ある" { c } else { format!("!{}", c) };
                }
                let c = format!("{}.is_empty()", self.src(inner));
                if kind == "空" {
                    c
                } else {
                    format!("!{}", c)
                }
            }
            Expr::Call { n, args } => {
                if n == "len" {
                    let at = self.tok(&args[0]);
                    if at == "Set" || at.starts_with("Map:") {
                        return format!("({}.len() as i128)", self.coll(&args[0]));
                    }
                    return format!("({}.chars().count() as i128)", self.rx_mut(&args[0]));
                }
                if n == "to_raw" {
                    return self.rx_mut(&args[0]);
                }
                if n == "raw_cat" {
                    return format!("format!(\"{{}}{{}}\", {}, {})", self.rx_mut(&args[0]), self.rx_mut(&args[1]));
                }
                if CONVERSIONS.contains(&n.as_str()) {
                    return if n == "time" { format!("(({}) as i64)", self.rx_mut(&args[0])) } else { self.rx_mut(&args[0]) };
                }
                if self.ck.parts.contains_key(n) {
                    return self.part_call(n, args);
                }
                if n == "min" || n == "max" {
                    let mut out = self.rx_mut(&args[0]);
                    for a in &args[1..] {
                        out = format!("({}).{}({})", out, n, self.rx_mut(a));
                    }
                    return out;
                }
                if n == "abs" {
                    return format!("(({}).checked_abs().ok_or(Fault::Overflow({}))?)", self.rx_mut(&args[0]), rust_str(&show(e)));
                }
                let fname = match &args[1] {
                    Expr::Name { n } => n,
                    _ => panic!("関数の呼び出し"),
                };
                format!("{}({})?", fname, self.rx_mut(&args[0]))
            }
            Expr::Bin { op, l, r, dflt } => {
                let (lt, rt) = (self.tok(l), self.tok(r));
                if op == "in" {
                    return format!("{}.{}(&({}))", self.coll(r), if rt == "Set" { "contains" } else { "contains_key" }, self.rx_mut(l));
                }
                let ls = self.rx_mut(l);
                let rs = self.rx_mut(r);
                if op == "+" && lt == "Str" {
                    return format!("format!(\"{{}}{{}}\", {}, {})", ls, rs);
                }
                if (op == "/" || op == "%") && matches!(&**r, Expr::Num { .. }) {
                    return format!("({}).{}({})", ls, if op == "/" { "div_euclid" } else { "rem_euclid" }, rs);
                }
                if op == "/" || op == "%" {
                    let f = if op == "/" { "checked_div_euclid" } else { "checked_rem_euclid" };
                    return match dflt {
                        None => format!("({}).{}({}).ok_or(Fault::Unproven({}))?", ls, f, rs, rust_str(&format!("割る数が 1 以上: {}", show(e)))),
                        Some(d) => format!("{{ let d_ = {}; if d_ == 0 {{ {} }} else {{ ({}).{}(d_).ok_or(Fault::Overflow({}))? }} }}", rs, self.rx_mut(d), ls, f, rust_str(&show(e))),
                    };
                }
                let fr = ["Ratio", "Rational"];
                let (lfr, rfr) = (fr.contains(&lt.as_str()), fr.contains(&rt.as_str()));
                if op == "*" && lfr && rfr {
                    return format!("mul_ratio({}, {})?", ls, rs);
                }
                if op == "*" && (lfr || rfr) {
                    return if lfr { format!("scale({}, {})?", rs, ls) } else { format!("scale({}, {})?", ls, rs) };
                }
                if op == "-" && lt == "Time" && rt == "Time" {
                    return format!("(({} as i128).checked_sub({} as i128).ok_or(Fault::Overflow(\"時刻の差\"))?)", ls, rs);
                }
                if op == "+" || op == "-" || op == "*" {
                    let how = self.info.calc.get(&id_of(e)).cloned().unwrap_or_else(|| "check".into());
                    let f = match op.as_str() {
                        "+" => "add",
                        "-" => "sub",
                        _ => "mul",
                    };
                    if how == "prove" || how == "assume" {
                        return format!("(({}).wrapping_{}({}))", ls, f, rs);
                    }
                    if how == "verify" {
                        return format!(
                            "{{ let (a_, b_): (i128, i128) = ({}, {}); if cfg!(debug_assertions) {{ a_.checked_{f}(b_).ok_or(Fault::Overflow({s}))? }} else {{ a_.wrapping_{f}(b_) }} }}",
                            ls,
                            rs,
                            f = f,
                            s = rust_str(&show(e))
                        );
                    }
                    return format!("(({}).checked_{}({}).ok_or(Fault::Overflow({}))?)", ls, f, rs, rust_str(&show(e)));
                }
                let o = match op.as_str() {
                    "かつ" => "&&",
                    "または" => "||",
                    o => o,
                };
                format!("({} {} {})", ls, o, rs)
            }
            Expr::SetB { .. } | Expr::SumE { .. } => self.builder(e),
            _ => panic!("rx: {}", show(e)),
        }
    }

    /// 束縛の for 文の先頭と、束縛した名前の登録。戻り値は (for 行, 束縛の行, 旧 scope)
    fn bind_lines(&mut self, b: &Binder) -> (String, Vec<String>, IndexMap<String, (String, String)>) {
        let st = self.tok(&b.src);
        let src = self.src(&b.src);
        let saved = self.scope.clone();
        if st.starts_with("Map:") {
            let mut it = st.splitn(3, ':');
            it.next();
            it.next();
            let vt = it.next().unwrap_or("").to_string();
            let (k, v) = (&b.names[0], &b.names[1]);
            let head = format!("for (k_, v_) in {}.iter() {{", src);
            let mut body = vec![format!("let {}: String = k_.clone();", k)];
            if vt == "Int" || vt == "Money" {
                body.push(format!("let {}: i128 = *v_ as i128;", v));
            } else {
                body.push(format!("let {} = v_.clone();", v));
            }
            self.scope.insert(k.clone(), ("local".into(), "ID".into()));
            self.scope.insert(v.clone(), ("local".into(), vt));
            (head, body, saved)
        } else {
            let k = &b.names[0];
            let head = format!("for k_ in {}.iter() {{", src);
            let body = vec![format!("let {}: String = k_.clone();", k)];
            self.scope.insert(k.clone(), ("local".into(), "ID".into()));
            (head, body, saved)
        }
    }

    fn builder(&mut self, e: &Expr) -> String {
        let (elem, binder, cond, is_set) = match e {
            Expr::SetB { elem, binder, cond } => (elem, binder, cond, true),
            Expr::SumE { elem, binder, cond } => (elem, binder, cond, false),
            _ => unreachable!(),
        };
        let (head, body, saved) = self.bind_lines(binder);
        let cond_s = cond.as_ref().map(|c| self.rx_mut(c));
        let elem_s = self.rx_mut(elem);
        self.scope = saved;
        let init = if is_set { "let mut acc: BTreeSet<String> = BTreeSet::new();" } else { "let mut acc: i128 = 0;" };
        let add = if is_set { format!("acc.insert({});", elem_s) } else { format!("acc = acc.checked_add({}).ok_or(Fault::Overflow(\"合計\"))?;", elem_s) };
        let inner = body.join(" ") + &match cond_s {
            Some(c) => format!(" if {} {{ {} }}", c, add),
            None => format!(" {}", add),
        };
        format!("{{ {} {} {} }} acc }}", init, head, inner)
    }

    // ---- 文 ----
    fn ret_expr(&mut self, r: &Return) -> String {
        let ts = &self.rdef.variants.iter().find(|(v, _)| v == &r.variant).unwrap().1;
        let mut parts: Vec<String> = vec![];
        for (a, (n, t)) in r.args.iter().zip(ts.iter()) {
            let x = self.rx_mut(a);
            parts.push(if t.kind == "整数" || t.kind == "金額" { self.conv(t, &x, &format!("{}.{}", r.variant, n), true) } else { x });
        }
        let args = parts.join(", ");
        let v = format!("{}::{}{}", self.rdef.name, r.variant, if r.args.is_empty() { String::new() } else { format!("({})", args) });
        format!("Ok({}Output {{ result: {}, Write: writes, effects: effects }})", self.proc.name, v)
    }

    /// i128 の式を、項目の物理数へ。proven なら、範囲は検査で示してある（実行時には確かめない）
    fn conv(&self, t: &Type, x: &str, name: &str, proven: bool) -> String {
        let r = t.range.as_ref();
        if proven {
            if let Some(r) = r {
                if r.lo.is_some() && r.hi.is_some() {
                    return format!("(({}) as {})", x, phys(t));
                }
            }
            return format!("{}::try_from({}).map_err(|_| Fault::Overflow(\"{}\"))?", phys(t), x, name);
        }
        let lo = r.and_then(|r| r.lo);
        let hi = r.and_then(|r| r.hi);
        format!("{p}::try_from(in_range(\"{n}\", {x}, {lo}, {hi})?).map_err(|_| Fault::Overflow(\"{n}\"))?", p = phys(t), n = name, x = x, lo = opt(lo), hi = opt(hi))
    }

    /// 項目へ書く値。null になりうる項目は Option で、無いかもしれない入力はそのまま渡す
    fn field_value(&mut self, t: &Type, e: &Expr, name: &str) -> String {
        let num = t.kind == "整数" || t.kind == "金額";
        if matches!(e, Expr::Null) {
            return "None".into();
        }
        if t.optional {
            if let Expr::Name { n } = e {
                if self.null_inputs.contains(n) && self.scope[n].0 == "input" {
                    let x = self.place(e);
                    return if num { format!("match {} {{ Some(v) => Some({}), None => None }}", x, self.conv(t, "(v as i128)", name, true)) } else { format!("{}.clone()", x) };
                }
            }
        }
        let xr = self.rx_mut(e);
        let v = if num { self.conv(t, &xr, name, true) } else { xr };
        if t.optional {
            format!("Some({})", v)
        } else {
            v
        }
    }

    fn field_type(&self, d: &str, f: &str) -> Type {
        self.ck.data[d].fields.iter().find(|x| x.name == f).unwrap().r#type.clone()
    }

    fn block(&mut self, stmts: &[Stmt]) {
        self.ind += 1;
        for inner in stmts {
            self.stmt(inner);
        }
        self.ind -= 1;
    }

    fn stmt(&mut self, st: &Stmt) {
        match st {
            Stmt::Return(r) => {
                let x = self.ret_expr(r);
                self.l(format!("return {};", x));
            }
            Stmt::If(i) => {
                let c = self.rx_mut(&i.cond);
                self.l(format!("if {} {{", c));
                self.block(&i.body);
                let mut els: Option<&[Stmt]> = i.els.as_deref();
                while let Some(es) = els {
                    if es.len() == 1 {
                        if let Stmt::If(inner) = &es[0] {
                            let c = self.rx_mut(&inner.cond);
                            self.l(format!("}} else if {} {{", c));
                            self.block(&inner.body);
                            els = inner.els.as_deref();
                            continue;
                        }
                    }
                    self.l("} else {");
                    self.block(es);
                    els = None;
                }
                self.l("}");
            }
            Stmt::Assign(a) => {
                if let Expr::Index { d, e: key, scopes } = &a.e {
                    let mutv = self.copied.contains(&a.name);
                    let k = self.rx_mut(key);
                    let sf = self.scope_filter(d, scopes);
                    self.l(format!("let {}{} = match Store.{}.get(&({})){} {{", if mutv { "mut " } else { "" }, a.name, d, k, sf));
                    self.l(if mutv { "    Some(r) => r.clone()," } else { "    Some(r) => r," });
                    if mutv {
                        self.owned.insert(a.name.clone());
                    }
                    if let Some(ab) = &a.absent {
                        let r = self.ret_expr(ab);
                        if self.in_loop {
                            self.l(format!("    None => {{ effects.truncate(束の始め); return {}; }}", r));
                        } else {
                            self.l(format!("    None => return {},", r));
                        }
                    }
                    self.l("};");
                    self.scope.insert(a.name.clone(), ("local".into(), format!("Rec:{}", d)));
                } else if a.absent.is_some() && matches!(&a.e, Expr::Attr { .. } | Expr::Name { .. }) {
                    let tok = self.tok(&a.e);
                    let ty = match tok.as_str() {
                        "Int" | "Money" => "i128",
                        "Ratio" => "Ratio",
                        "ID" | "Str" => "String",
                        "Time" => "i64",
                        "Set" => "BTreeSet<String>",
                        _ => "_",
                    };
                    let pl = self.place(&a.e);
                    self.l(format!("let {}: {} = match {}.clone() {{", a.name, ty, pl));
                    let some = if tok == "Int" || tok == "Money" { self.wrap(&tok, "v") } else { "v".to_string() };
                    self.l(format!("    Some(v) => {},", some));
                    let r = self.ret_expr(a.absent.as_ref().unwrap());
                    self.l(format!("    None => return {},", r));
                    self.l("};");
                    self.scope.insert(a.name.clone(), ("local".into(), tok));
                } else {
                    let tok = self.tok(&a.e);
                    let ty = match tok.as_str() {
                        "Int" | "Money" => "i128",
                        "Rational" => "Ratio",
                        "ID" | "Str" => "String",
                        "Time" => "i64",
                        "Set" => "BTreeSet<String>",
                        _ => panic!("let の型: {}", tok),
                    };
                    let x = self.rx_mut(&a.e);
                    self.l(format!("let {}: {} = {};", a.name, ty, x));
                    self.scope.insert(a.name.clone(), ("local".into(), tok));
                }
            }
            Stmt::Update(u) => {
                let (head, body, saved) = self.bind_lines(&u.binder);
                let (d, f) = match &u.target {
                    Expr::Attr { e, n } => match &**e {
                        Expr::Index { d, .. } => (d.clone(), n.clone()),
                        _ => panic!("update の対象"),
                    },
                    _ => panic!("update の対象"),
                };
                let tgt_key = match &u.target {
                    Expr::Attr { e, .. } => match &**e {
                        Expr::Index { e, .. } => &**e,
                        _ => unreachable!(),
                    },
                    _ => unreachable!(),
                };
                self.l(head);
                for b in body {
                    self.l(format!("    {}", b));
                }
                let val = self.rx_mut(&u.value);
                let cur = self.rx_mut(&u.target);
                let new = match u.op.as_str() {
                    "-=" => format!("({}).checked_sub({}).ok_or(Fault::Overflow(\"{}.{}\"))?", cur, val, d, f),
                    "+=" => format!("({}).checked_add({}).ok_or(Fault::Overflow(\"{}.{}\"))?", cur, val, d, f),
                    _ => val.clone(),
                };
                let pre = match &u.cond {
                    Some(c) => format!("if {} ", self.rx_mut(c)),
                    None => String::new(),
                };
                let ft = self.field_type(&d, &f);
                self.l(format!("    {}{{", pre));
                let key_s = self.rx_mut(tgt_key);
                if matches!(u.value, Expr::Null) {
                    self.l(format!("        writes.push(Write::{}_{}({}, None));", d, f, key_s));
                } else if ft.kind == "整数" || ft.kind == "金額" {
                    let v = self.conv(&ft, "new_value", &format!("{}.{}", d, f), true);
                    self.l(format!("        let new_value: i128 = {};", new));
                    self.l(format!("        writes.push(Write::{}_{}({}, {}));", d, f, key_s, if ft.optional { format!("Some({})", v) } else { v }));
                } else {
                    self.l(format!("        writes.push(Write::{}_{}({}, {}));", d, f, key_s, if ft.optional { format!("Some({})", val) } else { val.clone() }));
                }
                self.l("    }");
                self.l("}");
                self.scope = saved;
            }
            Stmt::SetField(s) => {
                let d = self.info.lookup_vars[&s.var].clone();
                let key = self.key_of[&d].clone();
                let ft = self.field_type(&d, &s.f);
                let v: String;
                if s.op == "add" || s.op == "remove" {
                    let x = self.rx_mut(&s.e);
                    let change = if s.op == "add" { format!("insert({})", x) } else { format!("remove(&({}))", x) };
                    v = format!("{{ let mut set_ = {}.{}.clone(); set_.{}; set_ }}", s.var, s.f, change);
                } else if matches!(&s.guard, Some(g) if g.0 == "saturating") {
                    let (lo, hi) = iv_of(&ft);
                    let (lo, hi) = (lo.unwrap().to_integer(), hi.unwrap().to_integer());
                    let x = self.rx_mut(&s.e);
                    v = self.conv(&ft, &format!("({}).clamp({}i128, {}i128)", x, lo, hi), &format!("{}.{}", d, s.f), true);
                } else if s.guard.is_some() {
                    let x = self.rx_mut(&s.e);
                    v = self.conv(&ft, &x, &format!("{}.{}", d, s.f), false);
                } else {
                    v = self.field_value(&ft, &s.e, &format!("{}.{}", d, s.f));
                }
                if self.local_fields.contains(&(s.var.clone(), s.f.clone())) {
                    self.l(format!("{{ let 書く値 = {v}; {var}.{f} = 書く値.clone(); writes.push(Write::{d}_{f}({var}.{key}.clone(), 書く値)); }}", v = v, var = s.var, f = s.f, d = d, key = key));
                } else {
                    self.l(format!("writes.push(Write::{}_{}({}.{}.clone(), {}));", d, s.f, s.var, key, v));
                }
            }
            Stmt::Delete(dl) => {
                let d = self.info.lookup_vars[&dl.var].clone();
                let key = self.key_of[&d].clone();
                self.l(format!("writes.push(Write::{}_Del({}.{}.clone()));", d, dl.var, key));
            }
            Stmt::Create(c) => {
                let dt = self.ck.data[&c.data];
                let key = self.ck.key_field(&c.data).unwrap();
                let null_e = Expr::Null;
                self.l(format!("let {}{} = {} {{", if self.copied.contains(&c.name) { "mut " } else { "" }, c.name, c.data));
                for f in &dt.fields {
                    let v = if std::ptr::eq(f, key) {
                        format!("ctx.new_{}_id.clone()", c.data)
                    } else {
                        let given = c.fields.iter().find(|(n, _)| n == &f.name).map(|(_, e)| e).unwrap_or(&null_e);
                        self.field_value(&f.r#type, given, &format!("{}.{}", c.data, f.name))
                    };
                    self.l(format!("    {}: {},", f.name, v));
                }
                self.l("};");
                self.l(format!("writes.push(Write::{}_New({}.clone()));", c.data, c.name));
                self.scope.insert(c.name.clone(), ("local".into(), format!("Rec:{}", c.data)));
                self.owned.insert(c.name.clone());
            }
            Stmt::ForEach(fe) => {
                let (head, body, saved) = self.bind_lines(&fe.binder);
                self.l("let 束の始め = effects.len();");
                self.l(head);
                for b in body {
                    self.l(format!("    {}", b));
                }
                self.in_loop = true;
                for inner in &fe.body {
                    self.stmt(inner);
                }
                self.in_loop = false;
                self.l("}");
                self.scope = saved;
            }
            Stmt::Request(rq) => {
                let eff = self.ck.effects[&rq.effect];
                let mut parts: Vec<String> = vec![];
                for ((n, t), a) in eff.params.iter().zip(rq.args.iter()) {
                    let x = self.rx_mut(a);
                    let v = if t.kind == "整数" || t.kind == "金額" { self.conv(t, &x, n, false) } else { x };
                    parts.push(format!("{}: {}", n, v));
                }
                match rq.idem.as_ref().unwrap() {
                    Expr::Tuple { items } => {
                        let xs: Vec<String> = items.iter().map(|x| self.rx_mut(x)).collect();
                        parts.push(format!("idem_key: join_idem_key(&[{}])", xs.join(", ")));
                    }
                    other => {
                        let x = self.rx_mut(other);
                        parts.push(format!("idem_key: {}", x));
                    }
                }
                self.l(format!("effects.push(Effect::{} {{ {} }});", rq.effect.replace('.', "_"), parts.join(", ")));
            }
        }
    }

    // ---- 関数全体 ----
    pub fn emit(mut self, g: &mut Gen<'a, 'b>) {
        let proc = self.proc;
        let n = &proc.name;
        let info = &self.info;
        g.w(format!("// ---- 処理 {} ----", n));
        g.w(format!("pub struct {}Input {{", n));
        for (name, t) in deed_inputs_proc(g.ck, proc) {
            g.w(format!("    pub {}: {},", name, field_rust(&t)));
        }
        g.w("}");
        g.w(format!("impl {}Input {{\n    /// 外部から取り込むときの、範囲の宣言の検査\n    pub fn validate(&self) -> Result<(), String> {{", n));
        for line in range_checks(&proc.inputs) {
            g.w(line);
        }
        g.w("        Ok(())\n    }\n}");
        g.w(format!("/// 文脈：【参照】（配備時に埋まる）と、採番（実行のたびに基盤が代入）\npub struct {}Ctx {{", n));
        for (name, t) in &proc.refs {
            g.w(format!("    pub {}: {},", name, rust_type(t)));
        }
        let mut ids: Vec<&String> = info.new_ids.iter().collect::<IndexSet<_>>().into_iter().collect();
        ids.sort();
        for d in ids {
            g.w(format!("    pub new_{}_id: String,", d));
        }
        g.w("}");
        g.w(format!("impl {}Ctx {{\n    /// 外から埋まる値（slot）の、範囲の宣言の検査\n    pub fn validate(&self) -> Result<(), String> {{", n));
        for line in range_checks(&proc.refs) {
            g.w(line);
        }
        g.w("        Ok(())\n    }\n}");
        g.w(format!("pub struct {}Output {{\n    pub result: {},\n    pub Write: Vec<Write>,\n    pub effects: Vec<Effect>,\n}}", n, self.rdef.name));
        g.w(format!("pub fn {n}(ctx: &{n}Ctx, Store: &Store, Input: &{n}Input) -> Result<{n}Output, Fault> {{", n = n));
        g.w("    let mut writes: Vec<Write> = Vec::new();\n    let mut effects: Vec<Effect> = Vec::new();");
        self.null_inputs = proc.inputs.iter().filter(|(_, t)| t.optional).map(|(n, _)| n.clone()).collect();
        for (name, t) in &proc.inputs {
            self.scope.insert(name.clone(), ("input".into(), tok_of(t)));
        }
        for (name, t) in &proc.refs {
            self.scope.insert(name.clone(), ("ref".into(), tok_of(t)));
        }
        for st in &proc.body {
            self.stmt(st);
        }
        g.out.extend(self.lines);
        g.w("}\n");
    }
}

pub fn cargo_toml(name: &str, host: bool, deps: &BTreeMap<String, String>) -> String {
    let crate_type = if host { "crate-type = [\"cdylib\", \"rlib\"]\n" } else { "" };
    let optl = if host { "opt-level = \"s\"\n" } else { "" };
    format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n{}\n{}[profile.release]\n{}overflow-checks = true\n",
        name,
        crate_type,
        dependencies(deps),
        optl
    )
}

/// library が呼ぶクレート。版は固定する（=）。std は依存に足さない
pub fn dependencies(deps: &BTreeMap<String, String>) -> String {
    if deps.is_empty() {
        return String::new();
    }
    "[dependencies]\n".to_string() + &deps.iter().map(|(c, v)| format!("{} = \"={}\"\n", c, v)).collect::<String>() + "\n"
}

/// 呼ばれた library のクレートと版。同じクレートの版が宣言どうしで食い違えばエラー
pub fn library_deps(ck: &Checker) -> Result<BTreeMap<String, String>, String> {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for pt in &ck.p.parts {
        if let Some(l) = &pt.lib {
            if l.0 != "std" {
                let prev = seen.entry(l.0.clone()).or_insert_with(|| l.1.clone()).clone();
                if prev != l.1 {
                    return Err(format!("クレート {} の版が2つあります（{} と {}）", l.0, prev, l.1));
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    for pt in &ck.p.parts {
        if let Some(l) = &pt.lib {
            if ck.used_libs.contains(&pt.name) && l.0 != "std" {
                out.insert(l.0.clone(), l.1.clone());
            }
        }
    }
    Ok(out)
}

pub fn generate(ck: &Checker) -> String {
    Gen::new(ck).lib()
}
