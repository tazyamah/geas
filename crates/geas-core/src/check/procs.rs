// SPDX-License-Identifier: GPL-3.0-or-later
//! 処理の中身の検査（Python 版 check.py の ProcChecker の移し）。型の推論・値の範囲・単位・null の確かめ。
use super::*;
use crate::parser::parse_chars;
use num_traits::{Signed, Zero};
use std::rc::Rc;

/// 式の同一性（Python の id(式)）：式の置き場所のアドレス
pub fn id_of(e: &Expr) -> usize {
    e as *const Expr as usize
}

/// 型の環境の値。名前 → 型の記号、"#範囲:名前" → 値の範囲、"#元:名前" → 回す元の式
#[derive(Debug, Clone)]
pub enum Ev {
    T(String),
    R(Iv),
    S(String),
}
pub type Env = IndexMap<String, Ev>;

pub fn env_ty<'e>(env: &'e Env, n: &str) -> Option<&'e str> {
    match env.get(n) {
        Some(Ev::T(t)) => Some(t.as_str()),
        _ => None,
    }
}

/// 分かっていること（下限など）の鍵。(束縛, 項目)・(UP, 束縛, 項目)・(NAMED, 名前)・("#不足", データ, 項目, 元)
pub type FKey = Vec<String>;

pub fn fk(parts: &[&str]) -> FKey {
    parts.iter().map(|s| s.to_string()).collect()
}

/// null でないと確かめた (束縛, 項目)。入力は ("#入力", 名前)、成分は ("#成分\u{1}式", 成分)
pub type Pres = (String, String);

pub fn comp_key(base: &str) -> String {
    format!("#成分\u{1}{}", base)
}

pub fn is_name(e: &Expr, n: &str) -> bool {
    matches!(e, Expr::Name { n: m } if m == n)
}

/// 範囲 r の外にはみ出すかもしれない（収まると示せない）
pub fn outside(iv: &Iv, r: &Range) -> bool {
    (r.lo.is_some() && (iv.0.is_none() || iv.0.as_ref().unwrap() < &q(r.lo.unwrap()))) || (r.hi.is_some() && (iv.1.is_none() || iv.1.as_ref().unwrap() > &q(r.hi.unwrap())))
}

pub fn ge(a: &Option<Q>, v: i128) -> bool {
    a.as_ref().map_or(false, |x| x >= &q(v))
}

/// 1 次の式の項（変数 or ある 1 件の項目）
pub fn atom_v(n: &str) -> String {
    format!("v\u{1}{}", n)
}
pub fn atom_f(b: &str, f: &str) -> String {
    format!("f\u{1}{}\u{1}{}", b, f)
}

#[derive(Clone)]
pub struct Snap {
    pub state_guards: IndexSet<(String, String, String)>,
    pub facts: IndexMap<FKey, Q>,
    pub guarded: IndexSet<(String, String)>,
    pub present: IndexSet<Pres>,
    pub cur_ok: IndexMap<(String, String, String), String>,
    pub cur_sets: IndexMap<String, (String, String, String, String)>,
    pub shortage_sets: IndexMap<String, (String, String, String)>,
    pub path_writes: IndexMap<String, usize>,
    pub rel: IndexSet<(Vec<(String, i128)>, i128)>,
}

pub type Shape = (i128, i128, Option<Vec<(u32, u32)>>);

pub struct ProcChecker<'c, 'a> {
    pub ck: &'c mut Checker<'a>,
    pub proc: &'a Process,
    pub info: ProcInfo,
    pub env: Rc<Env>,
    pub where_: String,
    pub exist_sets: IndexMap<String, (String, String)>,
    pub var_text: IndexMap<String, Shape>,
    pub rel: IndexSet<(Vec<(String, i128)>, i128)>,
    pub var_lin: IndexMap<String, Lin>,
    pub bind_names: IndexMap<String, String>,
    pub guarded: IndexSet<(String, String)>,
    pub scoped_ok: IndexSet<usize>,
    pub state_guards: IndexSet<(String, String, String)>,
    pub bindings: IndexMap<String, String>,
    pub binding_data: IndexMap<String, Option<String>>,
    pub facts: IndexMap<FKey, Q>,
    pub var_iv: IndexMap<String, Iv>,
    pub var_unit: IndexMap<String, String>,
    pub binder_src: IndexMap<String, String>,
    pub cur_ok: IndexMap<(String, String, String), String>,
    pub cur_sets: IndexMap<String, (String, String, String, String)>,
    pub loop_deps: IndexSet<String>,
    pub present: IndexSet<Pres>,
    pub writing: bool,
    pub shortage_sets: IndexMap<String, (String, String, String)>,
    pub accessed_vars: IndexSet<String>,
    pub created: IndexSet<String>,
    pub pos: usize,
    pub cur_line: i64,
    pub cur_calc: Option<(String, String)>,
    pub calc_hits: usize,
    pub calc_on: bool,
    pub path_writes: IndexMap<String, usize>,
    pub dup_writes: IndexSet<String>,
    pub raw_convert: bool,
    pub last_exist: Option<(String, String)>,
    pub bind_count: usize,
}

impl<'c, 'a> ProcChecker<'c, 'a> {
    pub fn new(ck: &'c mut Checker<'a>, proc: &'a Process) -> Self {
        ProcChecker {
            ck,
            proc,
            info: ProcInfo::new(&proc.name),
            env: Rc::new(Env::new()),
            where_: format!("処理 {}", proc.name),
            exist_sets: IndexMap::new(),
            var_text: IndexMap::new(),
            rel: IndexSet::new(),
            var_lin: IndexMap::new(),
            bind_names: IndexMap::new(),
            guarded: IndexSet::new(),
            scoped_ok: IndexSet::new(),
            state_guards: IndexSet::new(),
            bindings: IndexMap::new(),
            binding_data: IndexMap::new(),
            facts: IndexMap::new(),
            var_iv: IndexMap::new(),
            var_unit: IndexMap::new(),
            binder_src: IndexMap::new(),
            cur_ok: IndexMap::new(),
            cur_sets: IndexMap::new(),
            loop_deps: IndexSet::new(),
            present: IndexSet::new(),
            writing: false,
            shortage_sets: IndexMap::new(),
            accessed_vars: IndexSet::new(),
            created: IndexSet::new(),
            pos: 0,
            cur_line: 0,
            cur_calc: None,
            calc_hits: 0,
            calc_on: false,
            path_writes: IndexMap::new(),
            dup_writes: IndexSet::new(),
            raw_convert: false,
            last_exist: None,
            bind_count: 0,
        }
    }

    pub fn d(&mut self, level: &str, code: &str, msg: &str, line: i64) {
        let ln = if line != 0 {
            line
        } else if self.cur_line != 0 {
            self.cur_line
        } else {
            self.proc.line
        };
        let w = self.where_.clone();
        self.ck.d(level, code, &w, msg, ln);
    }

    pub fn lookup_data(&self, n: &str) -> Option<&String> {
        self.info.lookup_vars.get(n)
    }

    pub fn env_set(&mut self, n: &str, t: &str) {
        Rc::make_mut(&mut self.env).insert(n.to_string(), Ev::T(t.to_string()));
    }

    // ---- 型の推論 ----
    pub fn ty(&mut self, e: &Expr, env: &Rc<Env>) -> String {
        let t = self.ty_inner(e, env);
        self.info.types.insert(id_of(e), t.clone());
        t
    }

    pub fn ty_self(&mut self, e: &Expr) -> String {
        let env = self.env.clone();
        self.ty(e, &env)
    }

    /// 診断を増やさずに式の型を見る（調べるだけ）
    pub fn ty_quiet(&mut self, e: &Expr, env: &Rc<Env>) -> String {
        let n = self.ck.diags.len();
        let saved_types = self.info.types.clone();
        let saved_calc = (self.info.calc.clone(), self.info.calc_trusts.clone(), self.calc_hits);
        let t = self.ty_inner(e, env);
        self.ck.diags.truncate(n);
        self.info.types = saved_types;
        self.info.calc = saved_calc.0;
        self.info.calc_trusts = saved_calc.1;
        self.calc_hits = saved_calc.2;
        t
    }

    pub fn ty_quiet_self(&mut self, e: &Expr) -> String {
        let env = self.env.clone();
        self.ty_quiet(e, &env)
    }

    fn ty_inner(&mut self, e: &Expr, env: &Rc<Env>) -> String {
        let ck_p = self.ck.p;
        let _ = ck_p;
        match e {
            Expr::Num { .. } => "Int".into(),
            Expr::BoolLit { .. } => "Bool".into(),
            Expr::Not { e: inner } => {
                let t = self.ty(inner, env);
                if t != "Bool" && t != "?" {
                    self.d(ERROR, "型", &format!("「{}」: not の後ろは条件（真偽）である必要があります", show(e)), 0);
                }
                "Bool".into()
            }
            Expr::Dur { .. } => "Dur".into(),
            Expr::Null => "Null".into(),
            Expr::Name { n } => {
                if env.contains_key(n) && !self.writing && self.null_input(n, env) && !self.present.contains(&("#入力".to_string(), n.clone())) {
                    self.d(
                        ERROR,
                        "null",
                        &format!("入力「{}」は無いことがあります。使う前に確かめてください（let x = {} else {{ return … }}、または if {} is null {{ return … }}。必ず要るなら入力に required）", n, n, n),
                        0,
                    );
                }
                if let Some(Ev::T(t)) = env.get(n) {
                    return t.clone();
                }
                if let Some(s) = self.ck.variants.get(n) {
                    return format!("State:{}", s);
                }
                let h = name_hint(n);
                let tail = if h.is_empty() { near(n, env.keys().chain(self.ck.variants.keys())) } else { h };
                self.d(ERROR, "名前", &format!("「{}」は定義されていません{}", n, tail), 0);
                "?".into()
            }
            Expr::Index { d, e: ie, scopes } => {
                if !self.ck.data.contains_key(d) {
                    let msg = format!("データ「{}」が未定義です{}", d, near(d, self.ck.data.keys()));
                    self.d(ERROR, "名前", &msg, 0);
                    return "?".into();
                }
                if !scopes.is_empty() && !self.scoped_ok.contains(&id_of(e)) {
                    self.d(ERROR, "範囲", &format!("範囲を通して引けるのは「let 名前 = {}.{}[ID] else {{ return … }}」の形だけです", d, scopes.join(".")), 0);
                }
                if self.ty(ie, env) != "ID" {
                    self.d(ERROR, "型", &format!("{}[ ] の中は ID である必要があります", d), 0);
                }
                format!("Rec:{}", d)
            }
            Expr::Make { .. } => self.make(e, env),
            Expr::Attr { e: base, n } => self.ty_attr(e, base, n, env),
            Expr::Pred { e: inner, kind } => self.ty_pred(e, inner, kind, env),
            Expr::Bin { .. } => self.binop(e, env),
            Expr::Cond { c, a, b } => {
                let (ct, at, bt) = (self.ty(c, env), self.ty(a, env), self.ty(b, env));
                if ct != "Bool" && ct != "?" {
                    self.d(ERROR, "型", &format!("「{}」: if のあとは真偽の式です（{}）", show(e), ct), 0);
                }
                if at == "?" || bt == "?" {
                    return "?".into();
                }
                let both_num = (at == "Int" || at == "Money") && (bt == "Int" || bt == "Money") && (matches!(**a, Expr::Num { .. }) || matches!(**b, Expr::Num { .. }));
                if at != bt && !both_num {
                    self.d(ERROR, "型", &format!("「{}」: then と else の型が違います（{}／{}）", show(e), at, bt), 0);
                    return "?".into();
                }
                if matches!(**a, Expr::Num { .. }) && !matches!(**b, Expr::Num { .. }) {
                    bt
                } else {
                    at
                }
            }
            Expr::StrLit { .. } => {
                self.d(ERROR, "型", "文字列の書き込みは、raw の only( ) の文字種にだけ使えます", 0);
                "?".into()
            }
            Expr::Call { n, args } => self.ty_call(e, n, args, env),
            Expr::SetB { elem, binder, cond } | Expr::SumE { elem, binder, cond } => {
                let env2 = self.bind(binder, env);
                if let Some(c) = cond {
                    self.ty(c, &env2);
                    self.note_exist_set(e);
                }
                let et = self.ty(elem, &env2);
                if matches!(e, Expr::SetB { .. }) {
                    if et != "ID" {
                        self.d(ERROR, "型", "集合の要素は ID である必要があります", 0);
                    }
                    return "Set".into();
                }
                if et != "Int" && et != "Money" {
                    self.d(ERROR, "型", "Σ の要素は数である必要があります", 0);
                }
                et
            }
            Expr::Implies { l, r } => {
                self.ty(l, env);
                self.ty(r, env);
                "Bool".into()
            }
            Expr::Tuple { .. } => "?".into(),
        }
    }

    fn ty_attr(&mut self, e: &Expr, base: &Expr, n: &str, env: &Rc<Env>) -> String {
        let bt = self.ty(base, env);
        if let Some(cname) = bt.strip_prefix("Comp:") {
            let ct = self.ck.comps.get(cname).copied();
            let pt = ct.and_then(|c| c.parts.as_ref()).and_then(|ps| ps.iter().find(|(x, _)| x == n).map(|(_, t)| t));
            let pt = match pt {
                Some(t) => t,
                None => {
                    let pool: Vec<String> = ct.and_then(|c| c.parts.as_ref()).map_or(vec![], |ps| ps.iter().map(|(x, _)| x.clone()).collect());
                    self.d(ERROR, "名前", &format!("合成型 {} に成分「{}」がありません{}", cname, n, near(n, pool)), 0);
                    return "?".into();
                }
            };
            if pt.optional && !self.writing && !self.present.contains(&(comp_key(&show(base)), n.to_string())) {
                self.d(
                    ERROR,
                    "null",
                    &format!("「{}」は未指定かもしれない成分です。使う前に確かめてください（if {} is null {{ return … }}）。未指定にアクセスしうる道があるとエラーです", show(e), show(e)),
                    0,
                );
            }
            return tok_of(pt);
        }
        if bt == "?" {
            return "?".into();
        }
        let dname = match bt.strip_prefix("Rec:") {
            Some(d) => d.to_string(),
            None => {
                self.d(ERROR, "型", &format!("「{}」: 「.{}」はデータにしか使えません", show(e), n), 0);
                return "?".into();
            }
        };
        let dt = self.ck.data[&dname];
        let fld = dt.fields.iter().find(|f| f.name == n);
        if fld.is_none() && dt.derived.iter().any(|f| f.name == n) {
            let opened = self.open_derived(e, &dname);
            return self.ty(&opened, env);
        }
        let fld = match fld {
            Some(f) => f,
            None => {
                self.d(ERROR, "名前", &format!("データ {} に項目「{}」がありません{}", dname, n, near(n, dt.fields.iter().map(|f| f.name.clone()))), 0);
                return "?".into();
            }
        };
        let base_present = match base {
            Expr::Name { n: bn } => self.bindings.get(bn).map_or(false, |b| self.present.contains(&(b.clone(), n.to_string()))),
            _ => false,
        };
        if fld.r#type.optional && !self.writing && !base_present {
            self.d(
                ERROR,
                "null",
                &format!("「{}」は無い（null の）ことがあります。使う前に確かめてください（let x = {} else {{ return … }}、または if {} is null {{ return … }}。必ずあるなら項目に required）", show(e), show(e), show(e)),
                0,
            );
        }
        if let Expr::Index { .. } = base {
            if !self.exists_known(base, env) {
                self.d(QUESTION, "存在", &format!("{} が存在しないとき、どうしますか（{}の存在を確かめて返す処理が、使う前にありません）", show(base), dname), 0);
            }
        }
        if let Expr::Name { n: bn } = base {
            self.accessed_vars.insert(bn.clone());
        }
        let created = matches!(base, Expr::Name { n: bn } if self.created.contains(bn));
        if !created {
            self.info.field_reads.insert(format!("{}.{}", dname, n));
        }
        tok_of(&fld.r#type)
    }

    fn ty_pred(&mut self, e: &Expr, inner: &Expr, kind: &str, env: &Rc<Env>) -> String {
        if kind == "null" {
            if let Expr::Name { n } = inner {
                if !self.null_input(n, env) {
                    self.d(ERROR, "型", &format!("「is null」は、無いかもしれない入力（required でない入力）にだけ使えます（{}）", n), 0);
                }
                let saved = self.writing;
                self.writing = true;
                self.ty(inner, env);
                self.writing = saved;
                return "Bool".into();
            }
            if let Expr::Attr { e: base, n } = inner {
                let bt = self.ty_quiet(base, env);
                if let Some(cname) = bt.strip_prefix("Comp:") {
                    let pt = self.ck.comps.get(cname).and_then(|c| c.parts.as_ref()).and_then(|ps| ps.iter().find(|(x, _)| x == n).map(|(_, t)| t.clone()));
                    if pt.map_or(true, |t| !t.optional) {
                        self.d(ERROR, "型", &format!("「is null」は、未指定にできる成分（optional な成分）にだけ使えます（{}）", show(inner)), 0);
                    }
                    let saved = self.writing;
                    self.writing = true;
                    self.ty(inner, env);
                    self.writing = saved;
                    return "Bool".into();
                }
            }
            let ok = match inner {
                Expr::Attr { .. } => self.null_field(inner, env).is_some(),
                _ => false,
            };
            if !ok {
                self.d(ERROR, "型", &format!("「is null」は、無いかもしれない項目（required でない項目）にだけ使えます（{}）", show(inner)), 0);
                return "Bool".into();
            }
            let saved = self.writing;
            self.writing = true;
            self.ty(inner, env);
            self.writing = saved;
            return "Bool".into();
        }
        let _ = e;
        if kind == "ある" || kind == "ない" {
            if let Expr::Index { d, .. } = inner {
                self.ty(inner, env);
                self.info.exist_reads.insert(d.clone());
            } else {
                self.d(ERROR, "型", "「ある／ない」はデータの参照 D[…] にだけ使えます", 0);
            }
        } else {
            let t = self.ty(inner, env);
            if t != "Set" && t != "Str" {
                self.d(ERROR, "型", &format!("「{}」は集合でも文字列でもありません", show(inner)), 0);
            }
        }
        "Bool".into()
    }

    fn ty_call(&mut self, e: &Expr, n: &str, args: &[Expr], env: &Rc<Env>) -> String {
        if RAW_OPS.contains(&n) {
            return self.raw_op(e, n, args, env);
        }
        if let Some((src, dst)) = conversion(n) {
            if n == "money" && args.len() == 2 {
                let ok = matches!(&args[1], Expr::Name { n: u } if CURRENCIES.contains(&u.as_str()));
                if !ok {
                    self.d(ERROR, "型", &format!("money( ) の単位は {} のどれかです", CURRENCIES.join("・")), 0);
                }
            } else if args.len() != 1 {
                self.d(ERROR, "型", &format!("{}( ) の引数は1つです", n), 0);
                return "?".into();
            }
            let x = self.ty(&args[0], env);
            if x != src && x != "?" {
                self.d(ERROR, "型", &format!("{}( ) で変えられるのは{}だけです（{}）", n, if src == "Int" { "整数" } else { "金額" }, x), 0);
            }
            return dst.to_string();
        }
        if let Some(pt) = self.ck.parts.get(n).copied() {
            if pt.params.iter().any(|(_, t)| t.kind == "生") {
                self.d(ERROR, "型", &format!("「{}」: raw から変換する convert は、式の中では呼べません（型に read {} と書いて結びます）", show(e), n), 0);
                return tok_of(&pt.ret);
            }
            let full: Vec<&Expr> = if args.len() < pt.params.len() && pt.params[args.len()..].iter().all(|(pn, _)| pt.defaults.contains_key(pn)) {
                args.iter().chain(pt.params[args.len()..].iter().map(|(pn, _)| &pt.defaults[pn])).collect()
            } else {
                args.iter().collect()
            };
            if full.len() != pt.params.len() {
                let more = if pt.defaults.is_empty() { "" } else { "（デフォルト値のある引数だけ省けます）" };
                self.d(ERROR, "型", &format!("部品 {} の引数は {} 個です{}", n, pt.params.len(), more), 0);
                return tok_of(&pt.ret);
            }
            let sib: IndexMap<&String, &Expr> = pt.params.iter().map(|(pn, _)| pn).zip(full.iter().copied()).collect();
            for (a, (pn, t)) in full.iter().copied().zip(pt.params.iter()) {
                let at = self.ty(a, env);
                if at != tok_of(t) && !(matches!(a, Expr::Num { .. }) && (tok_of(t) == "Int" || tok_of(t) == "Money")) && at != "?" {
                    self.d(ERROR, "型", &format!("部品 {} の引数「{}」の型が違います（{}）。違う型は、変換してから渡してください", n, pn, at), 0);
                }
                self.fit_unit(a, t, &format!("部品 {} の引数「{}」", n, pn), 0, Some(&sib), None);
            }
            if pt.external() {
                self.ck.used_libs.insert(n.to_string());
                for (a, (pn, t)) in full.iter().copied().zip(pt.params.iter()) {
                    if (t.kind == "整数" || t.kind == "金額") && t.range.is_some() {
                        let r = t.range.as_ref().unwrap();
                        let iv = self.interval(a, env);
                        if outside(&iv, r) {
                            self.d(
                                ERROR,
                                "範囲",
                                &format!("部品 {} の引数「{}」に渡す値が、引数の範囲{}に収まると示せません（値の範囲 {}）。範囲を限るか、値を限ってから渡してください", n, pn, show_range(&t.range), iv_show(&iv)),
                                0,
                            );
                        }
                    }
                }
            }
            return tok_of(&pt.ret);
        }
        if NUM_FUNCS.contains(&n) {
            return self.num_func(e, n, args, env);
        }
        if n != "round" || args.len() != 3 {
            self.d(ERROR, "名前", &format!("部品「{}」は呼べません（宣言した部品と、丸め round(値, 方式, 単位)、min( )・max( )・abs( )、型の変換 money( )・int( ) だけ）", n), 0);
            return "?".into();
        }
        let x = self.ty(&args[0], env);
        let (m, u) = (&args[1], &args[2]);
        if x != "Rational" {
            self.d(ERROR, "型", "丸めの対象は端数の出る値（金額 × 割合）である必要があります", 0);
        }
        if !matches!(m, Expr::Name { n: mn } if METHODS.contains(&mn.as_str())) {
            self.d(ERROR, "型", &format!("丸めの方式は {} のどれかです", METHODS.join("／")), 0);
        }
        match u {
            Expr::Name { n: un } => {
                let mname = if let Expr::Name { n: mn } = m { mn.clone() } else { "?".to_string() };
                self.info.roundings.push((mname, un.clone()));
                let au = self.unit_of(&args[0], env);
                let ru = self.cur_name_unit(un, env);
                match ru {
                    None => self.d(ERROR, "型", &format!("丸めの単位「{}」は使えません（{}、または通貨の入力・slot の名前）", un, CURRENCIES.join("・")), 0),
                    Some(ru) => {
                        if let Some(au) = au {
                            if au != ru {
                                self.d(ERROR, "単位", &format!("{}の金額を{}の単位で丸めています", unit_label(&au), unit_label(&ru)), 0);
                            }
                        }
                    }
                }
            }
            _ => self.d(ERROR, "型", "丸めの単位が必要です", 0),
        }
        let inner = &args[0];
        fn has_money(s: &ProcChecker, x: &Expr) -> bool {
            if let Expr::Bin { op, l, r, .. } = x {
                if op == "*" {
                    return has_money(s, l) || has_money(s, r);
                }
            }
            s.info.types.get(&id_of(x)).map_or(false, |t| t == "Money")
        }
        if matches!(inner, Expr::Bin { .. }) && !has_money(self, inner) {
            return "Int".into();
        }
        "Money".into()
    }

    /// 合成型の値を作る式
    pub fn make(&mut self, e: &Expr, env: &Rc<Env>) -> String {
        let (mn, fields, line) = match e {
            Expr::Make { n, fields, line } => (n, fields, *line),
            _ => return "?".into(),
        };
        let ct = self.ck.comps[mn];
        let parts: &Vec<(String, Type)> = ct.parts.as_ref().unwrap();
        let given: IndexMap<&String, &Expr> = fields.iter().map(|(n, x)| (n, x)).collect();
        for (n, _) in fields {
            if fields.iter().filter(|(x, _)| x == n).count() > 1 {
                self.d(ERROR, "名前", &format!("{} の成分「{}」が2つ以上あります", mn, n), line);
            }
        }
        for (pn, pt) in parts {
            if !given.contains_key(pn) && !pt.optional {
                self.d(ERROR, "項目", &format!("{} の成分「{}」がありません（未指定にできる成分（optional）以外は、すべて書きます）", mn, pn), line);
            }
        }
        for (n, x) in fields {
            let pt = match parts.iter().find(|(pn, _)| pn == n) {
                Some((_, t)) => t,
                None => {
                    self.d(ERROR, "名前", &format!("合成型 {} に成分「{}」がありません{}", mn, n, near(n, parts.iter().map(|(pn, _)| pn.clone()))), line);
                    self.ty(x, env);
                    continue;
                }
            };
            let vt = self.ty(x, env);
            if vt == "Null" {
                if !pt.optional {
                    self.d(ERROR, "null", &format!("{}.{} は必須の成分なので null は入れられません", mn, n), line);
                }
            } else if vt == "Rational" {
                self.d(QUESTION, "丸め", "端数が出る値を、丸めずに書いています", line);
            } else if vt != tok_of(pt) && !(matches!(x, Expr::Num { .. }) && (tok_of(pt) == "Int" || tok_of(pt) == "Money")) && vt != "?" {
                self.d(ERROR, "型", &format!("{}.{} に型の違う値（{}）を入れています", mn, n, vt), line);
            } else {
                self.check_component_fit(x, pt, &format!("{}.{}", mn, n), line);
            }
            self.fit_unit(x, pt, &format!("{}.{}", mn, n), line, Some(&given), None);
        }
        format!("Comp:{}", mn)
    }

    /// 成分の範囲・長さ・文字に、書く値が収まると示せるか
    pub fn check_component_fit(&mut self, x: &Expr, pt: &Type, where_: &str, line: i64) {
        if (pt.kind == "整数" || pt.kind == "金額") && pt.range.is_some() {
            let env = self.env.clone();
            let iv = self.interval(x, &env);
            let r = pt.range.as_ref().unwrap();
            if outside(&iv, r) {
                self.d(ERROR, "範囲", &format!("{} に書く値の範囲が、成分の範囲に収まると示せません（書く値 {} ／ 成分 {}）", where_, iv_show(&iv), show_range(&pt.range).trim()), line);
            }
        } else if pt.kind == "文字列" {
            self.check_text_fit(x, pt, where_, line);
        } else if (pt.kind == "割合" || pt.kind == "バイト列") && pt.range.is_some() {
            let env = self.env.clone();
            let src = self.decl_type(x, &env);
            let pr = pt.range.as_ref().unwrap();
            let fits = |s: &Type| -> bool {
                match &s.range {
                    None => false,
                    Some(sr) => (pr.lo.is_none() || (sr.lo.is_some() && sr.lo.unwrap() >= pr.lo.unwrap())) && (pr.hi.is_none() || (sr.hi.is_some() && sr.hi.unwrap() <= pr.hi.unwrap())),
                }
            };
            if src.as_ref().map_or(true, |s| s.kind != pt.kind || !fits(s)) {
                self.d(ERROR, "範囲", &format!("{} は範囲が決まっているので、同じ範囲に収まる宣言のある入力・項目を、そのまま書いてください", where_), line);
            }
        }
    }

    /// { k | (k, v) in 元, D[k] is absent } の形なら、「元のキーがすべて D にあるか」を表す集合として覚える
    pub fn note_exist_set(&mut self, e: &Expr) {
        if let Expr::SetB { elem, binder, cond: Some(c) } = e {
            if let Expr::Pred { e: ie, kind } = &**c {
                if kind == "ない" {
                    if let Expr::Index { d, e: k, .. } = &**ie {
                        if let Some(b0) = binder.names.first() {
                            if is_name(k, b0) && is_name(elem, b0) {
                                self.last_exist = Some((d.clone(), format!("∈{}", show(&binder.src))));
                            }
                        }
                    }
                }
            }
        }
    }

    /// 無いかもしれない入力を、そのまま null になりうる項目へ入れる
    pub fn pass_null(&mut self, e: &Expr, ft: &Type) -> bool {
        match e {
            Expr::Name { n } => {
                let env = self.env.clone();
                ft.optional && self.null_input(n, &env) && !self.present.contains(&("#入力".to_string(), n.clone()))
            }
            _ => false,
        }
    }

    /// n が「無いかもしれない」入力そのものか
    pub fn null_input(&self, n: &str, env: &Env) -> bool {
        if env.contains_key(&format!("#範囲:{}", n)) || self.var_iv.contains_key(n) || self.info.lookup_vars.contains_key(n) {
            return false;
        }
        self.proc.inputs.iter().find(|(m, _)| m == n).map_or(false, |(_, t)| t.optional)
    }

    fn data_of_attr(&self, e: &Expr) -> Option<String> {
        if let Expr::Attr { e: b, .. } = e {
            return match &**b {
                Expr::Name { n } => self.info.lookup_vars.get(n).cloned(),
                Expr::Index { d, .. } => Some(d.clone()),
                _ => None,
            };
        }
        None
    }

    /// e が無いかもしれない項目なら、その項目
    pub fn null_field(&self, e: &Expr, _env: &Env) -> Option<&'a Field> {
        let data = self.data_of_attr(e)?;
        let dt = self.ck.data.get(&data).copied()?;
        if let Expr::Attr { n, .. } = e {
            let f = dt.fields.iter().find(|x| &x.name == n)?;
            if f.r#type.optional {
                return Some(f);
            }
        }
        None
    }

    /// data.fname に書いた
    pub fn set_null(&mut self, data: &str, fname: &str, bind: Option<&str>, known_present: bool) {
        let bd = &self.binding_data;
        self.present.retain(|p| !(bd.get(&p.0).map_or(false, |x| x.as_deref() == Some(data)) && p.1 == fname));
        if let Some(b) = bind {
            if known_present {
                self.present.insert((b.to_string(), fname.to_string()));
            }
        }
    }

    /// D[k] の存在が確かめ済みか
    pub fn exists_known(&self, ix: &Expr, env: &Env) -> bool {
        if let Expr::Index { d, e, .. } = ix {
            if self.guarded.contains(&(d.clone(), show(e))) {
                return true;
            }
            if let Expr::Name { n } = &**e {
                if let Some(Ev::S(src)) = env.get(&format!("#元:{}", n)) {
                    return self.guarded.contains(&(d.clone(), format!("∈{}", src)));
                }
            }
        }
        false
    }

    pub fn bind_var(&mut self, name: &str) -> String {
        self.bind_count += 1;
        let b = format!("{}#{}", name, self.bind_count);
        self.bindings.insert(name.to_string(), b.clone());
        self.bind_names.insert(b.clone(), name.to_string());
        self.binding_data.insert(b.clone(), self.info.lookup_vars.get(name).cloned());
        b
    }

    /// 通貨コードを表す名前 n の単位。通貨でなければ None
    pub fn cur_name_unit(&self, n: &str, env: &Env) -> Option<String> {
        if CURRENCIES.contains(&n) && !env.contains_key(n) {
            return Some(n.to_string());
        }
        if env_ty(env, n) == Some("State:Currency") {
            return Some(format!("${}", n));
        }
        None
    }

    pub fn cur_expr_unit(&self, e: &Expr, env: &Env) -> Option<String> {
        match e {
            Expr::Name { n } => self.cur_name_unit(n, env),
            Expr::Attr { e: b, n } => Some(self.cur_sym(b, n, env)),
            _ => None,
        }
    }

    /// base の通貨の項目 sib が表す単位
    pub fn cur_sym(&self, base: &Expr, sib: &str, env: &Env) -> String {
        if let Expr::Name { n } = base {
            if let Some(b) = self.bindings.get(n) {
                for (gb, f, x) in &self.state_guards {
                    if gb == b && f == sib {
                        if let Some(u) = self.cur_name_unit(x, env) {
                            return u;
                        }
                    }
                }
            }
        }
        if let Expr::Index { d, e, .. } = base {
            if let Expr::Name { n } = &**e {
                if let Some(src) = self.binder_src.get(n) {
                    if let Some(u) = self.cur_ok.get(&(d.clone(), sib.to_string(), src.clone())) {
                        return u.clone();
                    }
                }
            }
        }
        format!("@{}.{}", show(base), sib)
    }

    /// 金額の式の単位。分からないときは None
    pub fn unit_of(&mut self, e: &Expr, env: &Env) -> Option<String> {
        match e {
            Expr::Name { n } => {
                if let Some(u) = self.var_unit.get(n) {
                    return Some(u.clone());
                }
                let t = self.decl_type(e, env);
                t.filter(|t| t.kind == "金額").and_then(|t| t.unit.clone())
            }
            Expr::Attr { e: b, .. } => {
                let t = self.decl_type(e, env)?;
                if t.kind != "金額" {
                    return None;
                }
                let u = t.unit.clone()?;
                if let Some(rest) = u.strip_prefix('$') {
                    return Some(self.cur_sym(b, rest, env));
                }
                Some(u)
            }
            Expr::Bin { op, l, r, .. } if op == "+" || op == "-" || op == "*" => self.unit_of(l, env).or_else(|| self.unit_of(r, env)),
            Expr::Call { n, args } => {
                if n == "money" {
                    let u = if args.len() == 2 { Some(&args[1]) } else { None };
                    return match u {
                        Some(Expr::Name { n: un }) => Some(self.cur_name_unit(un, env).unwrap_or_else(|| un.clone())),
                        _ => Some("JPY".to_string()),
                    };
                }
                if n == "round" && !args.is_empty() {
                    return self.unit_of(&args[0], env);
                }
                let pt = self.ck.parts.get(n).copied();
                if pt.is_none() && NUM_FUNCS.contains(&n.as_str()) {
                    for a in args {
                        if let Some(u) = self.unit_of(a, env) {
                            return Some(u);
                        }
                    }
                    return None;
                }
                pt.filter(|p| p.ret.kind == "金額").and_then(|p| p.ret.unit.clone())
            }
            Expr::SumE { elem, binder, .. } => {
                let saved = (self.var_unit.clone(), self.binder_src.clone());
                self.note_binder_units(binder, env);
                let u = self.unit_of(elem, env);
                self.var_unit = saved.0;
                self.binder_src = saved.1;
                u
            }
            _ => None,
        }
    }

    /// (k, v) in 写像 で回す v の単位と、k がどの集まりのキーか
    pub fn note_binder_units(&mut self, b: &Binder, env: &Env) {
        let t = if matches!(*b.src, Expr::Name { .. } | Expr::Attr { .. }) { self.decl_type(&b.src, env) } else { None };
        let vt = t.as_ref().filter(|t| t.kind == "写像" && b.names.len() == 2).and_then(|t| t.value.as_ref()).filter(|v| v.kind == "金額");
        if let Some(v) = vt {
            self.var_unit.insert(b.names[1].clone(), v.unit.clone().unwrap_or_default());
        } else {
            for n in &b.names {
                self.var_unit.shift_remove(n);
            }
        }
        for n in &b.names {
            self.binder_src.shift_remove(n);
        }
        if let Some(n0) = b.names.first() {
            self.binder_src.insert(n0.clone(), show(&b.src));
        }
    }

    /// 金額を入れる先の単位と、入れる式の単位が合っているか
    pub fn fit_unit(&mut self, e: &Expr, t: &Type, what: &str, line: i64, siblings: Option<&IndexMap<&String, &Expr>>, base: Option<&Expr>) {
        if t.kind != "金額" {
            return;
        }
        let mut want = t.unit.clone().unwrap_or_default();
        let env = self.env.clone();
        if let Some(sib) = want.strip_prefix('$') {
            let sib = sib.to_string();
            if let Some(x) = siblings.and_then(|s| s.get(&sib)) {
                want = self.cur_expr_unit(x, &env).unwrap_or(want);
            } else if let Some(b) = base {
                want = self.cur_sym(b, &sib, &env);
            }
        }
        let u = self.unit_of(e, &env);
        match u {
            Some(u) if u != want => {
                self.d(
                    ERROR,
                    "単位",
                    &format!("{} は{}の金額ですが、{}の金額（{}）を入れています。換算の部品を通すか、通貨が同じと確かめてから（if … != 通貨 {{ return … }}）使ってください", what, unit_label(&want), unit_label(&u), show(e)),
                    line,
                );
            }
            _ => {
                if let Expr::Num { v } = e {
                    if *v != 0 && want != "JPY" {
                        self.d(ERROR, "単位", &format!("{} は円以外の金額なので、0 以外の数字は直接書けません（money(整数, 通貨) で最小単位の個数から作ります）", what), line);
                    }
                }
            }
        }
    }
}
