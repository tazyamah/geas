// SPDX-License-Identifier: GPL-3.0-or-later
//! 検査器。Python 版 geas/check.py の移し。型の推論つき。宣言（読む・書く・遷移・外部・結果）と本体のずれ、
//! 行き先の漏れ、丸めの指定漏れ、冪等キーの漏れなどを見つける。
pub mod num;
pub mod procs;
pub mod procs2;
pub mod procs3;
pub mod procs4;
pub mod defs;
pub mod flows;
pub mod decls;
pub mod screens;

use crate::ast::*;
use crate::difflib::get_close_match;
use crate::foreign::{name_hint, type_hint};
use crate::names::{label_gaps, name_problems};
use indexmap::{IndexMap, IndexSet};
use regex::Regex;
use std::sync::OnceLock;

pub use num::*;

pub const ERROR: &str = "エラー";
pub const WARN: &str = "警告";
pub const QUESTION: &str = "問い";
pub const METHODS: [&str; 3] = ["floor", "ceil", "half_up"];

/// 単位の記号を人向けの言葉に（$名前 は同じ入力・slot の通貨、@… は保存した1件の通貨の項目）
pub fn unit_label(u: &str) -> String {
    match show_ja(u) {
        Some(s) => s.to_string(),
        None => {
            if u.starts_with('$') || u.starts_with('@') {
                "別の通貨".to_string()
            } else {
                u.to_string()
            }
        }
    }
}

pub const CURRENCIES: [&str; 3] = ["JPY", "USD", "EUR"];
pub const RAW_OPS: [&str; 8] = ["len", "only", "text", "positions", "digit", "os_path", "to_raw", "raw_cat"];
pub const NUM_FUNCS: [&str; 3] = ["min", "max", "abs"];

/// 名前 → (変える前, 変えた後)
pub fn conversion(n: &str) -> Option<(&'static str, &'static str)> {
    match n {
        "money" => Some(("Int", "Money")),
        "int" => Some(("Money", "Int")),
        "time" => Some(("Int", "Time")),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct Diag {
    pub level: String,
    pub code: String,
    pub where_: String,
    pub msg: String,
    pub line: i64,
    pub code_where: String,
    pub code_msg: String,
}

impl Diag {
    pub fn plain(level: &str, code: &str, where_: &str, msg: &str, line: i64) -> Diag {
        Diag { level: level.into(), code: code.into(), where_: where_.into(), msg: msg.into(), line, code_where: String::new(), code_msg: String::new() }
    }
    pub fn at(&self) -> String {
        if self.line != 0 {
            format!("（{}行目）", self.line)
        } else {
            String::new()
        }
    }
    /// 名前をコードに書いたとおりに出す（check の出力）
    pub fn for_code(&self) -> String {
        crate::i18n::tr(&format!(
            "[{}] {}{}: {}（{}）",
            self.level,
            if self.code_where.is_empty() { &self.where_ } else { &self.code_where },
            self.at(),
            if self.code_msg.is_empty() { &self.msg } else { &self.code_msg },
            self.code
        ))
    }
}

impl std::fmt::Display for Diag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", crate::i18n::tr(&format!("[{}] {}{}: {}（{}）", self.level, self.where_, self.at(), self.msg, self.code)))
    }
}

/// 綴りの近い名前（打ち間違いの手がかり）。大文字と小文字の違いも拾う。無ければ空
pub fn near<S: AsRef<str>>(name: &str, pool: impl IntoIterator<Item = S>) -> String {
    let mut cands: IndexMap<String, String> = IndexMap::new();
    for x in pool {
        let x = x.as_ref();
        if !x.is_empty() && x != name {
            cands.entry(x.to_lowercase()).or_insert_with(|| x.to_string());
        }
    }
    let keys: Vec<String> = cands.keys().cloned().collect();
    match get_close_match(&name.to_lowercase(), &keys, 0.75) {
        Some(m) => format!("（近い名前：{}）", cands[&m]),
        None => String::new(),
    }
}

/// 綴りの近い名前。無ければ、少ないときは全部を並べる
pub fn near_or_all<S: AsRef<str>>(name: &str, pool: impl IntoIterator<Item = S>) -> String {
    let pool: Vec<String> = pool.into_iter().map(|s| s.as_ref().to_string()).filter(|x| !x.is_empty()).collect();
    let n = near(name, pool.iter());
    if !n.is_empty() {
        return n;
    }
    if !pool.is_empty() && pool.len() <= 12 {
        format!("（あるもの：{}）", pool.join("・"))
    } else {
        String::new()
    }
}

/// 型の種類を、検査の型トークンにする。知らない種類は None（Python 版は ValueError）
pub fn tok_of(t: &Type) -> String {
    match t.kind.as_str() {
        "ID" | "参照" => "ID".into(),
        "文字列" => "Str".into(),
        "バイト列" => "Bytes".into(),
        "生" => "Raw".into(),
        "日時" => "Time".into(),
        "真偽" => "Bool".into(),
        "整数" => "Int".into(),
        "金額" => "Money".into(),
        "割合" => "Ratio".into(),
        "名前" => format!("State:{}", t.target.clone().unwrap_or_default()),
        "合成" => format!("Comp:{}", t.target.clone().unwrap_or_default()),
        "写像" => format!("Map:{}:{}", t.key.clone().unwrap_or_default(), tok_of(t.value.as_deref().unwrap())),
        "集合" => "Set".into(),
        k => panic!("tok_of: 知らない型の種類 {}", k),
    }
}

/// (名前, 型) の並びから、その型の引数の名前（同じ型の値＝同じオブジェクトで探す）
pub fn pn_of(params: &[(String, Type)], t: &Type) -> String {
    params.iter().find(|(_, x)| std::ptr::eq(x, t)).map(|(n, _)| n.clone()).unwrap_or_else(|| "?".into())
}

/// 範囲から物理数（Rust の型）を自動で選ぶ
pub fn phys(t: &Type) -> &'static str {
    if t.kind == "金額" {
        return "i64";
    }
    if t.kind != "整数" {
        panic!("phys: {}", t.kind);
    }
    let (lo, hi) = match &t.range {
        Some(r) => (r.lo, r.hi),
        None => (None, None),
    };
    if let (Some(lo), Some(hi)) = (lo, hi) {
        if lo >= 0 {
            for (name, bits) in [("u8", 8u32), ("u16", 16), ("u32", 32), ("u64", 64)] {
                if hi < (1i128 << bits) {
                    return name;
                }
            }
        }
        for (name, bits) in [("i8", 8u32), ("i16", 16), ("i32", 32), ("i64", 64)] {
            if -(1i128 << (bits - 1)) <= lo && hi < (1i128 << (bits - 1)) {
                return name;
            }
        }
        return "i128";
    }
    if let Some(lo) = lo {
        if lo >= 0 {
            return "u64";
        }
    }
    "i64"
}

#[derive(Debug, Clone, Default)]
pub struct ProcInfo {
    pub name: String,
    /// id(定義した項目を読む式) → (元の式, 開いた式)
    pub derived: IndexMap<usize, std::rc::Rc<Expr>>,
    pub field_reads: IndexSet<String>,
    pub exist_reads: IndexSet<String>,
    pub writes: IndexSet<String>,
    pub write_ops: IndexMap<String, IndexSet<String>>,
    pub transitions: IndexSet<(String, String, String)>,
    pub effects: Vec<(String, String)>,
    pub returns: IndexSet<String>,
    pub roundings: Vec<(String, String)>,
    pub new_ids: Vec<String>,
    pub types: IndexMap<usize, String>,
    pub lookup_vars: IndexMap<String, String>,
    pub deletes: IndexSet<String>,
    pub scopes: Vec<String>,
    pub caller: bool,
    pub guards: Vec<(String, String, String, String)>,
    pub calc: IndexMap<usize, String>,
    pub calc_trusts: Vec<(String, String, String)>,
}

impl ProcInfo {
    pub fn new(name: &str) -> ProcInfo {
        ProcInfo { name: name.to_string(), ..Default::default() }
    }
    /// 読むもの：項目を読んだデータ（データ.項目）と、存在だけを読んだデータ（項目を読んでいないもの）
    pub fn reads(&self) -> IndexSet<String> {
        let data_with_fields: IndexSet<&str> = self.field_reads.iter().map(|r| r.split('.').next().unwrap_or("")).collect();
        let mut out: IndexSet<String> = self.field_reads.clone();
        for d in &self.exist_reads {
            if !data_with_fields.contains(d.as_str()) {
                out.insert(d.clone());
            }
        }
        out
    }
}

pub const SELECT_KINDS: [&str; 7] = ["ID", "整数", "金額", "日時", "文字列", "名前", "参照"];

/// 不変条件の形
#[derive(Debug, Clone, PartialEq)]
pub enum InvForm {
    /// ("下限", データ, 項目, 下限)
    Lower(String, String, i128),
    /// ("ならば", データ, 状態の項目, 状態, 項目, 数)
    Implies(String, String, String, String, i128),
}

pub fn inv_form(inv: &Invariant) -> Option<(String, InvForm)> {
    let c = &inv.cond;
    if let Expr::Bin { op, l, r, .. } = c {
        if op == ">=" || op == ">" {
            if let (Expr::Name { n }, Expr::Num { v }) = (&**l, &**r) {
                return Some(("下限".into(), InvForm::Lower(inv.data.clone(), n.clone(), v + if op == ">" { 1 } else { 0 })));
            }
        }
    }
    if let Expr::Implies { l, r } = c {
        if let (Expr::Bin { op: lop, l: ll, r: lr, .. }, Expr::Bin { op: rop, l: rl, r: rr, .. }) = (&**l, &**r) {
            if lop == "==" && rop == ">" {
                if let (Expr::Name { n: a }, Expr::Name { n: b }, Expr::Name { n: c2 }, Expr::Num { v }) = (&**ll, &**lr, &**rl, &**rr) {
                    return Some(("ならば".into(), InvForm::Implies(inv.data.clone(), a.clone(), b.clone(), c2.clone(), *v)));
                }
            }
        }
    }
    None
}

/// 項目どうしを比べる不変条件と、定義した項目の範囲
#[derive(Debug, Clone)]
pub struct RelInv {
    pub i: Option<usize>,
    pub data: String,
    pub when: Option<(String, String)>,
    pub cons: Vec<Lin>,
    pub text: String,
}

pub struct Checker<'a> {
    pub p: &'a Program,
    pub strict_names: bool,
    pub diags: Vec<Diag>,
    pub data: IndexMap<String, &'a Data>,
    pub states: IndexMap<String, &'a State>,
    pub results: IndexMap<String, &'a ResultDef>,
    pub effects: IndexMap<String, &'a EffectDecl>,
    pub parts: IndexMap<String, &'a Part>,
    pub used_libs: IndexSet<String>,
    pub read_checked: IndexSet<String>,
    pub comps: IndexMap<String, &'a Type>,
    pub part_types: IndexMap<usize, String>,
    pub procs: IndexMap<String, &'a Process>,
    pub triggers: IndexMap<String, &'a Trigger>,
    pub queries: IndexMap<String, &'a Query>,
    pub query_reads: IndexMap<String, IndexSet<String>>,
    pub variants: IndexMap<String, String>,
    pub infos: IndexMap<String, ProcInfo>,
    pub invariant_status: Vec<(String, Vec<(String, String)>)>,
    pub inv_forms: Vec<Option<(String, InvForm)>>,
    pub inv_rows: IndexMap<(bool, usize), IndexMap<String, Vec<(bool, String)>>>,
    pub rel_invs: Vec<RelInv>,
    pub def_lines: Option<IndexMap<(String, String), i64>>,
    pub code_path: Option<String>,
}

fn re_where() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(\S+) ([A-Za-z_][A-Za-z0-9_]*(?: [A-Za-z_][A-Za-z0-9_.]*)?)").unwrap())
}
fn re_where2() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^([A-Za-z_][A-Za-z0-9_]*)\.([A-Za-z_][A-Za-z0-9_]*)").unwrap())
}
fn re_crate() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[A-Za-z][A-Za-z0-9_-]*$").unwrap())
}
fn re_version() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[0-9]+(\.[0-9]+){0,2}([-+][0-9A-Za-z.-]+)?$").unwrap())
}
fn re_words() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[A-Za-z_]+").unwrap())
}
fn re_closure() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(^|[(,=])\s*(move\s+)?\|").unwrap())
}

/// 式に出てくる変数の名前
pub fn names_in(e: &Expr) -> IndexSet<String> {
    let mut out = IndexSet::new();
    names_in_into(e, &mut out);
    out
}

fn names_in_into(e: &Expr, out: &mut IndexSet<String>) {
    match e {
        Expr::Name { n } => {
            out.insert(n.clone());
        }
        Expr::Attr { e, .. } => names_in_into(e, out),
        Expr::Index { e, .. } => names_in_into(e, out),
        Expr::Call { args, .. } => args.iter().for_each(|a| names_in_into(a, out)),
        Expr::Bin { l, r, dflt, .. } => {
            names_in_into(l, out);
            names_in_into(r, out);
            if let Some(d) = dflt {
                names_in_into(d, out);
            }
        }
        Expr::Not { e } => names_in_into(e, out),
        Expr::Tuple { items } => items.iter().for_each(|a| names_in_into(a, out)),
        Expr::Make { fields, .. } => fields.iter().for_each(|(_, x)| names_in_into(x, out)),
        _ => {}
    }
}

impl<'a> Checker<'a> {
    pub fn new(prog: &'a Program, strict_names: bool) -> Checker<'a> {
        let mut variants = IndexMap::new();
        for s in &prog.states {
            for v in std::iter::once(&s.initial).chain(s.middle.iter()).chain(s.final_.iter()) {
                if !v.is_empty() {
                    variants.insert(v.clone(), s.name.clone());
                }
            }
        }
        let mut ck = Checker {
            p: prog,
            strict_names,
            diags: vec![],
            data: prog.data.iter().map(|d| (d.name.clone(), d)).collect(),
            states: prog.states.iter().map(|s| (s.name.clone(), s)).collect(),
            results: prog.results.iter().map(|r| (r.name.clone(), r)).collect(),
            effects: prog.effects.iter().map(|e| (e.name.clone(), e)).collect(),
            parts: prog.parts.iter().map(|x| (x.name.clone(), x)).collect(),
            used_libs: IndexSet::new(),
            read_checked: IndexSet::new(),
            comps: prog.composites.iter().map(|c| (c.target.clone().unwrap_or_default(), c)).collect(),
            part_types: IndexMap::new(),
            procs: prog.processes.iter().map(|x| (x.name.clone(), x)).collect(),
            triggers: prog.triggers.iter().map(|x| (x.name.clone(), x)).collect(),
            queries: prog.queries.iter().map(|x| (x.name.clone(), x)).collect(),
            query_reads: IndexMap::new(),
            variants,
            infos: IndexMap::new(),
            invariant_status: vec![],
            inv_forms: prog.invariants.iter().map(inv_form).collect(),
            inv_rows: IndexMap::new(),
            rel_invs: vec![],
            def_lines: None,
            code_path: None,
        };
        ck.collect_rel_invs();
        ck
    }

    pub fn d(&mut self, level: &str, code: &str, where_: &str, msg: &str, line: i64) {
        let p = self.p;
        let ln = if line != 0 { line } else { self.line_of(where_) };
        self.diags.push(Diag {
            level: level.into(),
            code: code.into(),
            where_: p.show_names(where_),
            msg: p.show_names(msg),
            line: ln,
            code_where: p.show_code(where_),
            code_msg: p.show_code(msg),
        });
    }

    /// 「データ 名前」の中の項目の名前なら、その項目の行。それ以外は、場所の定義の行
    pub fn name_line(&mut self, where_: &str, name: &str) -> i64 {
        if let Some(rest) = where_.strip_prefix("データ ") {
            self.line_of(where_);
            let key = ("項目".to_string(), format!("{}.{}", rest, name));
            if let Some(&line) = self.def_lines.as_ref().unwrap().get(&key) {
                if line != 0 {
                    return line;
                }
            }
        }
        self.line_of(where_)
    }

    /// 指摘の場所から、定義の行を引く。分からなければ 0
    pub fn line_of(&mut self, where_: &str) -> i64 {
        if self.def_lines.is_none() {
            let p = self.p;
            let mut table: IndexMap<(String, String), i64> = IndexMap::new();
            fn put(table: &mut IndexMap<(String, String), i64>, k: &str, n: &str, line: i64) {
                if line != 0 {
                    table.entry((k.to_string(), n.to_string())).or_insert(line);
                }
            }
            for x in &p.data {
                put(&mut table, "データ", &x.name, x.line);
            }
            for x in &p.states {
                put(&mut table, "状態", &x.name, x.line);
            }
            for x in &p.results {
                put(&mut table, "結果", &x.name, x.line);
            }
            for x in &p.effects {
                put(&mut table, "外部作用", &x.name, x.line);
            }
            for x in &p.parts {
                put(&mut table, "部品", &x.name, x.line);
            }
            for x in &p.processes {
                put(&mut table, "処理", &x.name, x.line);
            }
            for x in &p.flows {
                put(&mut table, "フロー", &x.name, x.line);
            }
            for x in &p.replies {
                put(&mut table, "応答", &x.name, x.line);
            }
            for x in &p.triggers {
                put(&mut table, "トリガ", &x.name, x.line);
            }
            for x in &p.queries {
                put(&mut table, "問い合わせ", &x.name, x.line);
            }
            for x in &p.events {
                put(&mut table, "出来事", &x.name, x.line);
            }
            for x in &p.screens {
                put(&mut table, "画面", &x.name, x.line);
            }
            for x in &p.resources {
                put(&mut table, "リソース", &x.name, x.line);
            }
            for (name, line) in p.ltype_lines.iter() {
                table.entry(("論理型".into(), name.clone())).or_insert(*line);
                table.entry(("合成型".into(), name.clone())).or_insert(*line);
            }
            for d in &p.data {
                for f in &d.fields {
                    put(&mut table, "項目", &format!("{}.{}", d.name, f.name), f.line);
                }
            }
            for r in &p.rules {
                put(&mut table, "rule", &r.slot, r.line);
            }
            for (kind, target, _, _, line) in &p.needs {
                let k = format!("{}{}", kind, if target.is_empty() { String::new() } else { format!(" {}", target) });
                table.entry(("needs".into(), k)).or_insert(*line);
            }
            for (name, (_, _, _, line)) in p.isolations.iter() {
                table.entry(("保存先".into(), name.clone())).or_insert(*line);
            }
            self.def_lines = Some(table);
        }
        let t = self.def_lines.as_ref().unwrap();
        if let Some(m) = re_where().captures(where_) {
            let (kind, rest) = (m[1].to_string(), m[2].to_string());
            if let Some(&l) = t.get(&(kind.clone(), rest.clone())) {
                if l != 0 {
                    return l;
                }
            }
            let first = rest.split(' ').next().unwrap_or("").to_string();
            return t.get(&(kind, first)).copied().unwrap_or(0);
        }
        if let Some(m) = re_where2().captures(where_) {
            if let Some(&l) = t.get(&("項目".to_string(), format!("{}.{}", &m[1], &m[2]))) {
                if l != 0 {
                    return l;
                }
            }
            return t.get(&("データ".to_string(), m[1].to_string())).copied().unwrap_or(0);
        }
        0
    }

    pub fn key_field(&self, dname: &str) -> Option<&'a Field> {
        self.data[dname].fields.iter().find(|f| f.r#type.kind == "ID" && f.unique)
    }

    /// 型として書ける名前（組み込み・論理型・状態・合成型）
    pub fn type_names(&self) -> Vec<String> {
        let mut out: Vec<String> = ["id", "text", "time", "bytes", "int", "ratio", "money", "bool", "currency", "raw", "vtime"].iter().map(|s| s.to_string()).collect();
        out.extend(self.p.ltype_names.iter().cloned());
        out.extend(self.states.keys().cloned());
        out.extend(self.comps.keys().cloned());
        out
    }

    /// 遷移を持つ状態の型か
    pub fn is_lifecycle(&self, t: &Type) -> bool {
        let s = if t.kind == "名前" { t.target.as_ref().and_then(|x| self.states.get(x)) } else { None };
        s.map_or(false, |s| !s.plain)
    }

    pub fn check_module_deed(&mut self, proc: &Process, info: &ProcInfo) {
        if !self.p.origin.contains_key(&proc.name) {
            return;
        }
        let mut used = vec![];
        if !info.reads().is_empty() {
            used.push("データを読む");
        }
        if !(info.writes.is_empty() && info.transitions.is_empty()) {
            used.push("データを書く");
        }
        if !info.new_ids.is_empty() {
            used.push("採番する");
        }
        if !info.effects.is_empty() {
            used.push("外部作用を使う");
        }
        if !used.is_empty() {
            self.d(ERROR, "モジュール", &proc.name, &format!("モジュールの処理は、入力から結果を返すだけです（{}ことはできません）", used.join("・")), 0);
        }
    }

    // ---- 項目どうしを比べる不変条件と、定義した項目 ----
    /// 同じ行の数の項目の、足し算・引き算の式を（項目 → 係数, 定数）にする。定義した項目は、その式に開く。書けない形は None
    pub fn flin(&self, data: &str, e: &Expr) -> Option<Lin> {
        let d = self.data.get(data)?;
        match e {
            Expr::Num { v } => Some((IndexMap::new(), *v)),
            Expr::Name { n } => {
                if d.fields.iter().any(|f| &f.name == n) {
                    let mut m = IndexMap::new();
                    m.insert(n.clone(), 1);
                    return Some((m, 0));
                }
                let dv = d.derived.iter().find(|f| &f.name == n)?;
                self.flin_stored(d, dv.expr.as_ref()?)
            }
            Expr::Bin { op, l, r, .. } if op == "+" || op == "-" => {
                let a = self.flin(data, l)?;
                let b = self.flin(data, r)?;
                Some(lin_add(&a, &b, if op == "+" { 1 } else { -1 }))
            }
            _ => None,
        }
    }

    /// 定義した項目の式：保存する項目と数だけ
    pub fn flin_stored(&self, d: &Data, e: &Expr) -> Option<Lin> {
        match e {
            Expr::Num { v } => Some((IndexMap::new(), *v)),
            Expr::Name { n } => {
                if d.fields.iter().any(|f| &f.name == n) {
                    let mut m = IndexMap::new();
                    m.insert(n.clone(), 1);
                    Some((m, 0))
                } else {
                    None
                }
            }
            Expr::Bin { op, l, r, .. } if op == "+" || op == "-" => {
                let a = self.flin_stored(d, l)?;
                let b = self.flin_stored(d, r)?;
                Some(lin_add(&a, &b, if op == "+" { 1 } else { -1 }))
            }
            _ => None,
        }
    }

    fn collect_rel_invs(&mut self) {
        let p = self.p;
        for (i, inv) in p.invariants.iter().enumerate() {
            if self.inv_forms[i].is_some() || !self.data.contains_key(&inv.data) {
                continue;
            }
            let mut c = &inv.cond;
            let mut when = None;
            if let Expr::Implies { l, r } = c {
                if let Expr::Bin { op, l: ll, r: lr, .. } = &**l {
                    if op == "==" {
                        if let (Expr::Name { n: a }, Expr::Name { n: b }) = (&**ll, &**lr) {
                            when = Some((a.clone(), b.clone()));
                            c = &**r;
                        }
                    }
                }
            }
            let (op, cl, cr) = match c {
                Expr::Bin { op, l, r, .. } if ["<=", "<", ">=", ">", "=="].contains(&op.as_str()) => (op, l, r),
                _ => continue,
            };
            let (a, b) = (self.flin(&inv.data, cl), self.flin(&inv.data, cr));
            let (a, b) = match (a, b) {
                (Some(a), Some(b)) => (a, b),
                _ => continue,
            };
            let le = lin_add(&a, &b, -1);
            let ge = lin_add(&b, &a, -1);
            let cons: Vec<Lin> = match op.as_str() {
                "<=" => vec![le],
                "<" => vec![(le.0.clone(), le.1 + 1)],
                ">=" => vec![ge],
                ">" => vec![(ge.0.clone(), ge.1 + 1)],
                _ => vec![le, ge],
            };
            self.rel_invs.push(RelInv { i: Some(i), data: inv.data.clone(), when, cons, text: format!("{}: {}", inv.data, show(&inv.cond)) });
        }
        for d in &p.data {
            for f in &d.derived {
                let form = match f.expr.as_ref().and_then(|e| self.flin_stored(d, e)) {
                    Some(x) => x,
                    None => continue,
                };
                if f.r#type.kind != "整数" && f.r#type.kind != "金額" {
                    continue;
                }
                let (lo, hi) = if f.r#type.range.is_some() { iv_of(&f.r#type) } else { (None, None) };
                let mut ivs: IndexMap<String, Iv> = IndexMap::new();
                for x in &d.fields {
                    if form.0.contains_key(&x.name) {
                        ivs.insert(x.name.clone(), iv_of(&x.r#type));
                    }
                }
                let (elo, ehi) = lin_bounds(&form, &ivs);
                let mut cons: Vec<Lin> = vec![];
                if let Some(hi) = &hi {
                    if ehi.as_ref().map_or(true, |e| e > hi) {
                        cons.push((form.0.clone(), form.1 - q_to_i128(hi).unwrap_or(0)));
                    }
                }
                if let Some(lo) = &lo {
                    if elo.as_ref().map_or(true, |e| e < lo) {
                        cons.push((form.0.iter().map(|(k, v)| (k.clone(), -v)).collect(), q_to_i128(lo).unwrap_or(0) - form.1));
                    }
                }
                if !cons.is_empty() {
                    self.rel_invs.push(RelInv {
                        i: None,
                        data: d.name.clone(),
                        when: None,
                        cons,
                        text: format!("{}: {}（= {}）が範囲{}に収まる", d.name, f.name, show(f.expr.as_ref().unwrap()), show_range(&f.r#type.range)),
                    });
                }
            }
        }
    }

    /// 定義した項目（名前: 型 = 式）：同じ行の、必須の数の項目の足し算・引き算だけ。定義した項目を別の定義には使えない
    pub fn check_derived(&mut self) {
        let p = self.p;
        for d in &p.data {
            let names: Vec<&String> = d.fields.iter().map(|f| &f.name).collect();
            for f in &d.derived {
                let where_ = format!("データ {}", d.name);
                if names.contains(&&f.name) || d.derived.iter().filter(|x| x.name == f.name).count() > 1 {
                    self.d(ERROR, "名前", &where_, &format!("項目「{}」が2つあります", f.name), f.line);
                }
                if f.unique || f.required {
                    self.d(ERROR, "項目", &where_, &format!("定義した項目「{}」に unique・required は付けられません（保存せず、いつも式から決まります）", f.name), f.line);
                }
                if f.r#type.kind != "整数" && f.r#type.kind != "金額" {
                    self.d(ERROR, "型", &where_, &format!("定義した項目「{}」の型は、整数か金額です", f.name), f.line);
                    continue;
                }
                let expr = f.expr.as_ref().unwrap();
                let used = names_in(expr);
                let mut nested: Vec<&String> = used.iter().filter(|n| d.derived.iter().any(|x| &&x.name == n)).collect();
                nested.sort();
                if !nested.is_empty() {
                    self.d(
                        ERROR,
                        "項目",
                        &where_,
                        &format!("定義した項目「{}」の式に、定義した項目「{}」は使えません（入れ子にはしません。保存する項目で書いてください）", f.name, nested[0]),
                        f.line,
                    );
                    continue;
                }
                if self.flin_stored(d, expr).is_none() {
                    self.d(ERROR, "項目", &where_, &format!("定義した項目「{}」の式に書けるのは、同じ行の数の項目と数の、足し算と引き算だけです（{}）", f.name, show(expr)), f.line);
                    continue;
                }
                let mut used_sorted: Vec<&String> = used.iter().collect();
                used_sorted.sort();
                for n in used_sorted {
                    let x = d.fields.iter().find(|x| &x.name == n).unwrap();
                    if x.r#type.kind != f.r#type.kind || x.r#type.optional || (x.r#type.kind == "金額" && x.r#type.unit != f.r#type.unit) {
                        self.d(
                            ERROR,
                            "型",
                            &where_,
                            &format!(
                                "定義した項目「{}」の式の「{}」は、必須（required）で、同じ種類（{}）の項目にしてください",
                                f.name,
                                n,
                                if f.r#type.kind == "金額" { "金額は同じ通貨" } else { "整数" }
                            ),
                            f.line,
                        );
                    }
                }
            }
        }
    }

    pub fn ok(&self) -> bool {
        !self.diags.iter().any(|x| x.level == ERROR)
    }

    /// 生成してよいか。問いが残っている（仕様が決まっていない）うちは生成しない
    pub fn ready(&self) -> bool {
        !self.diags.iter().any(|x| x.level == ERROR || x.level == QUESTION)
    }

    pub fn currency_field(&self, data: &str, fname: &str) -> bool {
        let dt = self.data.get(data);
        let f = dt.and_then(|d| d.fields.iter().find(|x| x.name == fname));
        f.map_or(false, |f| f.r#type.kind == "名前" && f.r#type.target.as_deref() == Some("Currency"))
    }

    /// money(通貨) の通貨は、同じ並び（入力・slot、項目、引数、中身）にある通貨 currency 型の名前でなければならない
    pub fn check_units(&mut self, where_: &str, items: &[(String, Type)]) {
        let names: IndexSet<&String> = items.iter().filter(|(_, t)| t.kind == "名前" && t.target.as_deref() == Some("Currency")).map(|(n, _)| n).collect();
        for (n, t) in items {
            let u = if t.kind == "金額" { t.unit.as_ref() } else { None };
            let u = match u {
                Some(u) => u,
                None => continue,
            };
            if let Some(rest) = u.strip_prefix('$') {
                if !names.contains(&rest.to_string()) {
                    self.d(ERROR, "単位", &format!("{} / {}", where_, n), &format!("金額の通貨「{}」が、同じ並びにありません（通貨コードの currency 型の名前を書きます）", rest), 0);
                }
            } else if !CURRENCIES.contains(&u.as_str()) {
                self.d(ERROR, "単位", &format!("{} / {}", where_, n), &format!("金額の単位「{}」は使えません（{}）", u, CURRENCIES.join("・")), 0);
            }
        }
    }

    /// 型に出てくる名前（参照先・状態・写像のキー）が定義されているか
    pub fn check_type(&mut self, t: &Type, where_: &str, comp_ok: bool, raw_ok: bool) {
        if t.kind == "生" && !(raw_ok || t.trust.as_ref().map_or(false, |x| x.0 == "dangerous")) {
            self.d(
                ERROR,
                "型",
                where_,
                "raw（未検査の生データ）を受けられるのは、raw から変換する convert の引数と、trust dangerous の入力と、外部作用の引数だけです。変換するなら、型に read 変換名 を結びます",
                0,
            );
        }
        if let Some(r) = &t.read {
            if !self.read_checked.contains(r) {
                self.read_checked.insert(r.clone());
                self.check_type_read(where_, t);
            }
        }
        if t.kind == "合成" && !comp_ok {
            self.d(
                ERROR,
                "型",
                where_,
                &format!("合成型「{}」は、いまは項目・処理の入力と slot・結果の中身・外部作用の引数・応答の中身・トリガの引数にだけ使えます", t.target.clone().unwrap_or_default()),
                0,
            );
        }
        if t.kind == "参照" && !self.data.contains_key(t.target.as_deref().unwrap_or("")) {
            let tg = t.target.clone().unwrap_or_default();
            let msg = format!("参照先のデータ「{}」が未定義です{}", tg, near(&tg, self.data.keys()));
            self.d(ERROR, "名前", where_, &msg, 0);
        }
        if t.kind == "名前" && !self.states.contains_key(t.target.as_deref().unwrap_or("")) {
            let tg = t.target.clone().unwrap_or_default();
            let h = type_hint(&tg);
            let msg = format!("型「{}」が未定義です{}", tg, if h.is_empty() { near(&tg, self.type_names()) } else { h });
            self.d(ERROR, "名前", where_, &msg, 0);
        }
        if t.kind == "写像" && t.key.as_deref() != Some("id") && !self.data.contains_key(t.key.as_deref().unwrap_or("")) {
            let k = t.key.clone().unwrap_or_default();
            let msg = format!("キーのデータ「{}」が未定義です{}", k, near(&k, self.data.keys()));
            self.d(ERROR, "名前", where_, &msg, 0);
        }
        if (t.kind == "写像" || t.kind == "集合") && t.value.is_some() {
            self.check_type(t.value.as_deref().unwrap(), where_, comp_ok, false);
        }
    }

    pub fn check_unique<S: AsRef<str>>(&mut self, names: &[S], where_: &str, what: &str) {
        let mut seen: IndexSet<&str> = IndexSet::new();
        for n in names {
            let n = n.as_ref();
            if names.iter().filter(|x| x.as_ref() == n).count() > 1 {
                seen.insert(n);
            }
        }
        for n in seen {
            self.d(ERROR, "名前", where_, &format!("{}「{}」が2つ以上あります", what, n), 0);
        }
    }

    /// library の具体例：1 つは必須
    pub fn check_examples(&mut self, pt: &Part, where_: &str) {
        if pt.examples.is_empty() {
            self.d(
                ERROR,
                "保証",
                where_,
                &format!("library には具体例が要ります（example {}(値, …) == 期待する値 を 1 行以上。geas parts が実際に呼んで比べ、仕様書の「人の保証」に出ます）", pt.name),
                0,
            );
            return;
        }
        for (args, want, ln) in &pt.examples {
            let call = format!("{}({})", pt.name, args.iter().map(show).collect::<Vec<_>>().join(", "));
            if args.len() != pt.params.len() {
                self.d(ERROR, "保証", where_, &format!("具体例 {} の引数は {} 個ですが、宣言は {} 個です", call, args.len(), pt.params.len()), *ln);
                continue;
            }
            let ret_pair = ("戻り値".to_string(), pt.ret.clone());
            let mut pairs: Vec<(&(String, Type), &Expr)> = pt.params.iter().zip(args.iter()).collect();
            pairs.push((&ret_pair, want));
            for ((n, t), v) in pairs {
                let why = example_mismatch(t, v);
                if !why.is_empty() {
                    self.d(ERROR, "保証", where_, &format!("具体例 {} の {}（{}）が、型 {} に合いません（{}）", call, n, show(v), show_type(t).unwrap_or_default(), why), *ln);
                }
            }
        }
    }
}

/// 具体例に直に書いた値 v が、型 t に合わない理由（合えば空）
pub fn example_mismatch(t: &Type, v: &Expr) -> String {
    match v {
        Expr::BoolLit { .. } => {
            return if t.kind == "真偽" { String::new() } else { "真偽の値ではない型です".into() };
        }
        Expr::Num { v } => {
            if !["整数", "金額", "日時"].contains(&t.kind.as_str()) {
                return "数ではない型です".into();
            }
            if let Some(r) = &t.range {
                if r.lo.map_or(false, |lo| *v < lo) || r.hi.map_or(false, |hi| *v > hi) {
                    return format!("範囲{}の外です", show_range(&t.range));
                }
            }
            return String::new();
        }
        _ => {}
    }
    if !["文字列", "ID", "生"].contains(&t.kind.as_str()) {
        return "文字列ではない型です".into();
    }
    let s = match v {
        Expr::StrLit { s } => s.chars().count() as i128,
        _ => 0,
    };
    if let Some(r) = &t.range {
        if r.lo.map_or(false, |lo| s < lo) || r.hi.map_or(false, |hi| s > hi) {
            return format!("長さ{}の外です", show_range(&t.range));
        }
    }
    String::new()
}
