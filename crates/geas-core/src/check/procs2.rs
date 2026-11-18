// SPDX-License-Identifier: GPL-3.0-or-later
//! ProcChecker の続き：値の範囲・演算の型・文字列の形・1 次の式
use super::procs::*;
use super::*;
use num_bigint::BigInt;
use num_traits::Signed;
use std::rc::Rc;

fn qmax(a: &Q, b: &Q) -> Q {
    if a > b { a.clone() } else { b.clone() }
}
fn qmin(a: &Q, b: &Q) -> Q {
    if a < b { a.clone() } else { b.clone() }
}
fn opt_floor(x: &Q) -> Q {
    qi(q_floor(x))
}
fn opt_ceil(x: &Q) -> Q {
    qi(q_ceil(x))
}

pub fn call_name(e: &Expr) -> &str {
    match e {
        Expr::Name { n } => n,
        _ => "",
    }
}

impl<'c, 'a> ProcChecker<'c, 'a> {
    /// 入力・参照・項目として宣言された型（範囲つき）。分からなければ None
    pub fn decl_type(&self, e: &Expr, env: &Env) -> Option<Type> {
        match e {
            Expr::Name { n } => {
                if env.contains_key(n) && self.var_iv.contains_key(n) {
                    return None;
                }
                self.proc.inputs.iter().chain(self.proc.refs.iter()).find(|(m, _)| m == n).map(|(_, t)| t.clone())
            }
            Expr::Attr { e: b, n } => {
                let data = match &**b {
                    Expr::Name { n: bn } => self.info.lookup_vars.get(bn).cloned(),
                    Expr::Index { d, .. } => Some(d.clone()),
                    _ => None,
                };
                if let Some(dt) = data.as_ref().and_then(|d| self.ck.data.get(d)) {
                    return dt.fields.iter().find(|f| &f.name == n).map(|f| f.r#type.clone());
                }
                let bt = if matches!(**b, Expr::Name { .. } | Expr::Attr { .. }) { self.decl_type(b, env) } else { None };
                if let Some(bt) = bt {
                    if bt.kind == "合成" {
                        return bt.parts.as_ref().and_then(|ps| ps.iter().find(|(x, _)| x == n).map(|(_, t)| t.clone()));
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// (k, v) in 写像 の v に、写像の値の範囲を持たせる
    pub fn range_env(&self, b: &Binder, env: &Env) -> Env {
        let mut env2 = env.clone();
        for n in &b.names {
            env2.insert(format!("#範囲:{}", n), Ev::R((None, None)));
        }
        let t = self.decl_type(&b.src, env);
        if let Some(t) = t {
            if t.kind == "写像" && b.names.len() == 2 {
                if let Some(v) = &t.value {
                    env2.insert(format!("#範囲:{}", b.names[1]), Ev::R(iv_of(v)));
                }
            }
        }
        env2
    }

    pub fn field_iv(&self, data: &str, fname: &str) -> Iv {
        let dt = match self.ck.data.get(data) {
            Some(d) => *d,
            None => return (None, None),
        };
        let fld = match dt.fields.iter().find(|f| f.name == fname) {
            Some(f) => f,
            None => return (None, None),
        };
        let (mut lo, hi) = iv_of(&fld.r#type);
        for form in self.ck.inv_forms.iter().flatten() {
            if let (name, InvForm::Lower(d, f, n)) = form {
                if name == "下限" && d == data && f == fname && (lo.is_none() || &q(*n) > lo.as_ref().unwrap()) {
                    lo = Some(q(*n));
                }
            }
        }
        (lo, hi)
    }

    /// 式の値の範囲。分かる形だけを見て、分からなければ限りなし
    pub fn interval(&mut self, e: &Expr, env: &Rc<Env>) -> Iv {
        match e {
            Expr::Num { v } => (Some(q(*v)), Some(q(*v))),
            Expr::Name { n } => {
                if let Some(Ev::R(iv)) = env.get(&format!("#範囲:{}", n)) {
                    return iv.clone();
                }
                let (mut lo, mut hi) = if let Some(iv) = self.var_iv.get(n) {
                    iv.clone()
                } else {
                    match self.decl_type(e, env) {
                        None => (None, None),
                        Some(t) => iv_of(&t),
                    }
                };
                if let Some(known) = self.facts.get(&fk(&[NAMED, n])) {
                    if lo.is_none() || known > lo.as_ref().unwrap() {
                        lo = Some(known.clone());
                    }
                }
                if let Some(upper) = self.facts.get(&fk(&[UP, NAMED, n])) {
                    if hi.is_none() || upper < hi.as_ref().unwrap() {
                        hi = Some(upper.clone());
                    }
                }
                (lo, hi)
            }
            Expr::Attr { e: b, n } => {
                let data = match &**b {
                    Expr::Name { n: bn } => self.info.lookup_vars.get(bn).cloned(),
                    Expr::Index { d, .. } => Some(d.clone()),
                    _ => None,
                };
                let data = match data.filter(|d| self.ck.data.contains_key(d)) {
                    Some(d) => d,
                    None => {
                        return match self.decl_type(e, env) {
                            None => (None, None),
                            Some(t) => iv_of(&t),
                        };
                    }
                };
                let dt = self.ck.data[&data];
                if let Some(dv) = dt.derived.iter().find(|f| &f.name == n) {
                    let opened = self.open_derived(e, &data);
                    let (lo, hi) = self.interval(&opened, env);
                    let (tlo, thi) = if dv.r#type.range.is_some() { iv_of(&dv.r#type) } else { (None, None) };
                    let lo2 = match (&lo, &tlo) {
                        (None, _) => tlo.clone(),
                        (_, None) => lo.clone(),
                        (Some(a), Some(b)) => Some(qmax(a, b)),
                    };
                    let hi2 = match (&hi, &thi) {
                        (None, _) => thi.clone(),
                        (_, None) => hi.clone(),
                        (Some(a), Some(b)) => Some(qmin(a, b)),
                    };
                    return (lo2, hi2);
                }
                let (mut lo, mut hi) = self.field_iv(&data, n);
                if let Expr::Name { n: bn } = &**b {
                    if let Some(bind) = self.bindings.get(bn).cloned() {
                        if let Some(known) = self.facts.get(&fk(&[&bind, n])) {
                            if lo.is_none() || known > lo.as_ref().unwrap() {
                                lo = Some(known.clone());
                            }
                        }
                        if let Some(upper) = self.facts.get(&fk(&[UP, &bind, n])) {
                            if hi.is_none() || upper < hi.as_ref().unwrap() {
                                hi = Some(upper.clone());
                            }
                        }
                    }
                }
                (lo, hi)
            }
            Expr::Call { n, args } if n == "len" => {
                let t = if !args.is_empty() && matches!(args[0], Expr::Name { .. } | Expr::Attr { .. }) { self.decl_type(&args[0], env) } else { None };
                let big = q(i64::MAX as i128);
                if let Some(t) = &t {
                    if t.kind == "文字列" && t.range.is_some() {
                        let r = t.range.as_ref().unwrap();
                        return (Some(q(r.lo.unwrap_or(0))), Some(r.hi.map_or(big, q)));
                    }
                    if (t.kind == "写像" || t.kind == "集合") && t.maxn.is_some() {
                        return (Some(q(0)), Some(q(t.maxn.unwrap())));
                    }
                }
                (Some(q(0)), Some(big))
            }
            Expr::Call { n, args } if NUM_FUNCS.contains(&n.as_str()) && !self.ck.parts.contains_key(n) && !args.is_empty() => {
                let ivs: Vec<Iv> = args.iter().map(|a| self.interval(a, env)).collect();
                let los: Vec<&Option<Q>> = ivs.iter().map(|x| &x.0).collect();
                let his: Vec<&Option<Q>> = ivs.iter().map(|x| &x.1).collect();
                if n == "min" {
                    let lo = if los.iter().any(|x| x.is_none()) { None } else { los.iter().map(|x| (**x).clone().unwrap()).min() };
                    let hi = his.iter().filter_map(|x| (**x).clone()).min();
                    return (lo, hi);
                }
                if n == "max" {
                    let lo = los.iter().filter_map(|x| (**x).clone()).max();
                    let hi = if his.iter().any(|x| x.is_none()) { None } else { his.iter().map(|x| (**x).clone().unwrap()).max() };
                    return (lo, hi);
                }
                let (lo, hi) = ivs[0].clone();
                if let Some(l) = &lo {
                    if !l.is_negative() {
                        return (lo, hi);
                    }
                }
                if let Some(h) = &hi {
                    if !h.is_positive() {
                        return (Some(-h.clone()), lo.map(|l| -l));
                    }
                }
                let top = match (&lo, &hi) {
                    (Some(l), Some(h)) => Some(qmax(&-l.clone(), h)),
                    _ => None,
                };
                (Some(q(0)), top)
            }
            Expr::Cond { c, a, b } => {
                let saved = self.facts.clone();
                let f1 = self.cond_facts(c, true);
                self.facts = self.add_facts(saved.clone(), f1);
                let (al, ah) = self.interval(a, env);
                let f2 = self.cond_facts(c, false);
                self.facts = self.add_facts(saved.clone(), f2);
                let (bl, bh) = self.interval(b, env);
                self.facts = saved;
                (
                    match (&al, &bl) {
                        (Some(x), Some(y)) => Some(qmin(x, y)),
                        _ => None,
                    },
                    match (&ah, &bh) {
                        (Some(x), Some(y)) => Some(qmax(x, y)),
                        _ => None,
                    },
                )
            }
            Expr::Bin { op, .. } if op == "/" || op == "%" => self.div_interval(e, env),
            Expr::Bin { op, l, r, .. } if op == "-" && self.ty_quiet(l, env) == "Time" && self.ty_quiet(r, env) == "Time" => (Some(q(-2 * TIME_LIMIT)), Some(q(2 * TIME_LIMIT))),
            Expr::Bin { op, l, r, .. } if op == "+" || op == "-" || op == "*" => {
                let a = self.interval(l, env);
                let b = self.interval(r, env);
                if op == "+" {
                    iv_add(&a, &b)
                } else if op == "-" {
                    iv_add(&a, &iv_neg(&b))
                } else {
                    iv_mul(&a, &b)
                }
            }
            Expr::Call { n, args } if conversion(n).is_some() && args.len() == 1 => self.interval(&args[0], env),
            Expr::Call { n, .. } if self.ck.parts.contains_key(n) => iv_of(&self.ck.parts[n].ret),
            Expr::Call { n, args } if n == "round" && args.len() == 3 => {
                let (lo, hi) = self.interval(&args[0], env);
                let how = call_name(&args[1]).to_string();
                if call_name(&args[2]) == "JPY" {
                    return (
                        lo.as_ref().map(|l| if how != "ceil" { opt_floor(l) } else { opt_ceil(l) }),
                        hi.as_ref().map(|h| if how != "floor" { opt_ceil(h) } else { opt_floor(h) }),
                    );
                }
                let nonneg = lo.as_ref().map_or(false, |l| !l.is_negative());
                (if nonneg { Some(q(0)) } else { None }, if how == "floor" && nonneg && hi.is_some() { Some(opt_ceil(hi.as_ref().unwrap())) } else { None })
            }
            Expr::SumE { elem, binder, .. } => {
                let renv = Rc::new(self.range_env(binder, env));
                let (lo, hi) = self.interval(elem, &renv);
                let src = self.decl_type(&binder.src, env);
                let n = src.filter(|s| s.kind == "写像").and_then(|s| s.maxn);
                let lo_out = if lo.as_ref().map_or(false, |l| !l.is_negative()) {
                    Some(q(0))
                } else if n.is_none() || lo.is_none() {
                    None
                } else {
                    Some(q(n.unwrap()) * lo.as_ref().unwrap())
                };
                let hi_out = if n.is_none() || hi.is_none() { None } else { Some(qmax(&q(0), &(q(n.unwrap()) * hi.as_ref().unwrap()))) };
                (lo_out, hi_out)
            }
            _ => (None, None),
        }
    }

    pub fn interval_self(&mut self, e: &Expr) -> Iv {
        let env = self.env.clone();
        self.interval(e, &env)
    }

    /// 割り算（負の無限大へ切り捨て）・余り（0 以上）の値の範囲
    pub fn div_interval(&mut self, e: &Expr, env: &Rc<Env>) -> Iv {
        let (op, l, r, dflt) = match e {
            Expr::Bin { op, l, r, dflt } => (op, l, r, dflt),
            _ => return (None, None),
        };
        let (nl, nh) = self.interval(l, env);
        let (mut dl, mut dh) = self.interval(r, env);
        if let Expr::Num { v } = &**r {
            dl = Some(q(*v));
            dh = Some(q(*v));
        }
        if dflt.is_some() && dl.as_ref().map_or(false, |d| !d.is_negative()) {
            let d1 = qmax(dl.as_ref().unwrap(), &q(1));
            dl = Some(d1.clone());
            dh = dh.map(|h| qmax(&h, &d1));
        }
        let out: Iv;
        if dl.as_ref().map_or(false, |d| d >= &q(1)) {
            let dlv = dl.clone().unwrap();
            let fdiv = |n: &Q, d: &Option<Q>| -> Q {
                match d {
                    None => {
                        if n.is_negative() {
                            q(-1)
                        } else {
                            q(0)
                        }
                    }
                    Some(d) => qi(q_floor(&(n / d))),
                }
            };
            if op == "%" {
                let mut hi = dh.as_ref().map(|h| h - q(1));
                if nl.as_ref().map_or(false, |l| !l.is_negative()) && nh.is_some() {
                    hi = Some(match &hi {
                        None => nh.clone().unwrap(),
                        Some(h) => qmin(h, nh.as_ref().unwrap()),
                    });
                }
                out = (Some(q(0)), hi);
            } else {
                let lo = nl.as_ref().map(|n| qmin(&fdiv(n, &Some(dlv.clone())), &fdiv(n, &dh)));
                let hi = nh.as_ref().map(|n| qmax(&fdiv(n, &Some(dlv.clone())), &fdiv(n, &dh)));
                out = (lo, hi);
            }
        } else {
            let m = match (&nl, &nh) {
                (Some(l), Some(h)) => Some(qmax(&l.abs(), &h.abs())),
                _ => None,
            };
            let dm = match (&dl, &dh) {
                (Some(l), Some(h)) => Some(qmax(&l.abs(), &h.abs())),
                _ => None,
            };
            out = if op == "%" { (Some(q(0)), dm.map(|d| d - q(1))) } else { (m.as_ref().map(|m| -m.clone()), m) };
        }
        if let Some(d) = dflt {
            let di = self.interval(d, env);
            return iv_join_one(&out, &di);
        }
        out
    }

    /// 変数で割る割り算・余り。割る数が 1 以上と示せるか、0 のときの値（or）が書いてあるかを確かめる
    pub fn check_divisor(&mut self, e: &Expr, b: &str, env: &Rc<Env>) {
        let (r, dflt) = match e {
            Expr::Bin { r, dflt, .. } => (r, dflt),
            _ => return,
        };
        if b != "Int" {
            self.d(ERROR, "型", &format!("「{}」: 割る数は整数です（{}）", show(e), b), 0);
            return;
        }
        if let Some(df) = dflt {
            let t = self.ty(df, env);
            if t != "Int" && t != "?" {
                self.d(ERROR, "型", &format!("「{}」: 0 のときの値（or）は整数です", show(e)), 0);
            }
            return;
        }
        let iv = self.interval(r, env);
        if iv.0.as_ref().map_or(true, |l| l < &q(1)) {
            self.d(
                ERROR,
                "割り算",
                &format!(
                    "「{}」: 割る数「{}」が 0 になる道があります（範囲は{}）。割る数の範囲を 1 以上にするか、割る前に「if {} == 0 {{ return … }}」で止めるか、「{} or 0」のように 0 のときの値を書いてください",
                    show(e),
                    show(r),
                    iv_show(&iv),
                    show(r),
                    show(e)
                ),
                0,
            );
        }
    }

    pub fn bind(&mut self, b: &Binder, env: &Rc<Env>) -> Rc<Env> {
        let st = self.ty(&b.src, env);
        self.note_binder_units(b, env);
        let mut env2 = self.range_env(b, env);
        if st.starts_with("Map:") && b.names.len() == 2 {
            let vt = st.splitn(3, ':').nth(2).unwrap_or("").to_string();
            env2.insert(b.names[0].clone(), Ev::T("ID".into()));
            env2.insert(b.names[1].clone(), Ev::T(vt));
            env2.insert(format!("#元:{}", b.names[0]), Ev::S(show(&b.src)));
        } else if st == "Positions" && b.names.len() == 1 {
            env2.insert(b.names[0].clone(), Ev::T("Int".into()));
        } else if st == "Set" && b.names.len() == 1 {
            env2.insert(b.names[0].clone(), Ev::T("ID".into()));
            env2.insert(format!("#元:{}", b.names[0]), Ev::S(show(&b.src)));
        } else {
            self.d(ERROR, "型", &format!("「{}」を (k, v) in … で回せません", show(&b.src)), 0);
            for n in &b.names {
                env2.insert(n.clone(), Ev::T("?".into()));
            }
        }
        Rc::new(env2)
    }

    /// 端数の出る式の、途中の分子の絶対値の上限と、分母の上限。分からなければ None
    pub fn frac_bound(&mut self, e: &Expr, env: &Rc<Env>) -> Option<(Q, Q)> {
        let t = self.ty_quiet(e, env);
        if let Expr::Bin { op, l, r, .. } = e {
            if op == "*" && t == "Rational" {
                let a = self.frac_bound(l, env);
                let b = self.frac_bound(r, env);
                return match (a, b) {
                    (Some(a), Some(b)) => Some((a.0 * b.0, a.1 * b.1)),
                    _ => None,
                };
            }
        }
        let (lo, hi) = self.interval(e, env);
        let (lo, hi) = (lo?, hi?);
        let m = qmax(&lo.abs(), &hi.abs());
        if t == "Ratio" || t == "Rational" {
            return Some((qi(q_ceil(&(m * q(RATIO_DEN_MAX)))), q(RATIO_DEN_MAX)));
        }
        Some((m, q(1)))
    }

    /// 割合を掛けた途中の分子・分母が i128 に収まると示す
    pub fn check_frac_fit(&mut self, e: &Expr, env: &Rc<Env>) {
        let (l, r) = match e {
            Expr::Bin { l, r, .. } => (l, r),
            _ => return,
        };
        let a = self.frac_bound(l, env);
        let b = self.frac_bound(r, env);
        match (a, b) {
            (Some(a), Some(b)) => {
                let lim = ratio_num_limit();
                let num = &a.0 * &b.0;
                let den = &a.1 * &b.1;
                if num > lim || den > lim {
                    self.d(
                        ERROR,
                        "範囲",
                        &format!("「{}」: 割合を掛けた途中の分子が最大 約 {} になり、収まる上限 約 {} を超えるかもしれません。金額や割合の範囲を狭めてください", show(e), sci(&num), sci(&lim)),
                        0,
                    );
                }
            }
            _ => self.d(ERROR, "範囲", &format!("「{}」: 掛ける数の範囲が分からないので、途中の計算が収まると示せません（範囲を書いてください）", show(e)), 0),
        }
    }

    /// raw の操作：len・only・text・positions・digit・os_path・to_raw・raw_cat
    pub fn raw_op(&mut self, _e: &Expr, n: &str, args: &[Expr], env: &Rc<Env>) -> String {
        let want = match n {
            "len" | "text" | "positions" | "os_path" | "to_raw" => 1,
            _ => 2,
        };
        if ["only", "text", "positions", "digit", "os_path"].contains(&n) && !self.raw_convert {
            self.d(ERROR, "型", &format!("{}( ) は、raw から変換する convert の中でだけ使えます（処理の中で raw に使えるのは len・to_raw・raw_cat です）", n), 0);
            return "?".into();
        }
        if args.len() != want {
            self.d(ERROR, "型", &format!("{}( ) の引数は {} 個です", n, want), 0);
            return "?".into();
        }
        if n == "to_raw" {
            if self.ty(&args[0], env) != "Str" {
                self.d(ERROR, "型", "to_raw( ) の対象は文字列です", 0);
                return "?".into();
            }
            return "Raw".into();
        }
        if n == "raw_cat" {
            let ts: Vec<String> = args.iter().map(|a| self.ty(a, env)).collect();
            if !ts.iter().all(|t| t == "Raw") {
                self.d(ERROR, "型", "raw_cat( ) の引数は raw 2つです（文字列は to_raw( ) で raw にします）", 0);
                return "?".into();
            }
            return "Raw".into();
        }
        let at = self.ty(&args[0], env);
        if n == "len" && (at == "Str" || at == "Set" || at == "?" || at.starts_with("Map:")) {
            return if at != "?" { "Int".into() } else { "?".into() };
        }
        if at != "Raw" {
            let m = if n == "len" { "len( ) の対象は、文字列（文字数）・集合・写像（件数）・raw です".to_string() } else { format!("{}( ) の対象は raw です", n) };
            self.d(ERROR, "型", &m, 0);
            return "?".into();
        }
        if n == "only" {
            let c = &args[1];
            match c {
                Expr::StrLit { s } => {
                    if let Err(ex) = crate::parser::parse_chars(s) {
                        self.d(ERROR, "型", &format!("only( ) の文字種が正しくありません（{}）", ex), 0);
                    }
                    return "Bool".into();
                }
                _ => {
                    self.d(ERROR, "型", "only( ) の文字種は、\"a-z0-9\" のように文字列で書きます", 0);
                    return "?".into();
                }
            }
        }
        if n == "digit" {
            if self.ty(&args[1], env) != "Int" {
                self.d(ERROR, "型", "digit( ) の位置は整数です", 0);
            }
            return "Int".into();
        }
        if n == "positions" {
            return "Positions".into();
        }
        if n == "len" { "Int".into() } else { "Str".into() }
    }

    /// min(a, b, …)・max(a, b, …)・abs(x)
    pub fn num_func(&mut self, e: &Expr, n: &str, args: &[Expr], env: &Rc<Env>) -> String {
        let want = if n == "abs" { "1つ" } else { "2つ以上" };
        let bad = if n == "abs" { args.len() != 1 } else { args.len() < 2 };
        if bad {
            self.d(ERROR, "型", &format!("{}( ) の引数は{}です（{}）", n, want, show(e)), 0);
            for a in args {
                self.ty(a, env);
            }
            return "?".into();
        }
        let ts: Vec<String> = args.iter().map(|a| self.ty(a, env)).collect();
        if ts.iter().any(|t| t == "?") {
            return "?".into();
        }
        let mut kinds: IndexSet<String> = args.iter().zip(ts.iter()).filter(|(a, _)| !matches!(a, Expr::Num { .. })).map(|(_, t)| t.clone()).collect();
        if kinds.is_empty() {
            kinds.insert("Int".into());
        }
        if kinds.len() != 1 || !kinds.iter().all(|k| k == "Int" || k == "Money") {
            self.d(ERROR, "型", &format!("「{}」: {}( ) に渡せるのは、整数どうしか金額どうしです（{}）", show(e), n, ts.join("／")), 0);
            return "?".into();
        }
        let mut units: IndexSet<String> = IndexSet::new();
        for a in args {
            if let Some(u) = self.unit_of(a, env) {
                if !u.is_empty() {
                    units.insert(u);
                }
            }
        }
        if units.len() > 1 {
            let mut v: Vec<String> = units.iter().map(|u| unit_label(u)).collect();
            v.sort();
            self.d(ERROR, "単位", &format!("「{}」: 単位の違う金額は、そのままでは比べられません（{}）", show(e), v.join("・")), 0);
        }
        kinds.into_iter().next().unwrap()
    }

    pub fn binop(&mut self, e: &Expr, env: &Rc<Env>) -> String {
        let (op, l, r, _dflt) = match e {
            Expr::Bin { op, l, r, dflt } => (op.as_str(), &**l, &**r, dflt),
            _ => return "?".into(),
        };
        if matches!(l, Expr::StrLit { .. }) || matches!(r, Expr::StrLit { .. }) {
            return self.text_binop(e, env);
        }
        let mut a = self.ty(l, env);
        let mut b = self.ty(r, env);
        if op == "+" && a == "Str" && b == "Str" {
            return "Str".into();
        }
        if a == "?" || b == "?" {
            return "?".into();
        }
        let num = |t: &str| t == "Int" || t == "Money";
        if num(&a) && num(&b) && a != b && (matches!(l, Expr::Num { .. }) || matches!(r, Expr::Num { .. })) {
            a = "Money".into();
            b = "Money".into();
        }
        let mixed = num(&a) && num(&b) && a != b;
        if ["+", "-", "<", ">", "<=", ">=", "==", "!="].contains(&op) && (a == "Money" || b == "Money") {
            let ul = self.unit_of(l, env);
            let ur = self.unit_of(r, env);
            match (&ul, &ur) {
                (Some(x), Some(y)) if x != y => {
                    self.d(ERROR, "単位", &format!("「{}」: {}の金額と{}の金額は、そのままでは足し引きも比較もできません。換算の部品を通すか、通貨が同じと確かめてから使ってください", show(e), unit_label(x), unit_label(y)), 0);
                }
                _ => {
                    let num_side = if let Expr::Num { v } = l {
                        Some(*v)
                    } else if let Expr::Num { v } = r {
                        Some(*v)
                    } else {
                        None
                    };
                    let u = ul.or(ur);
                    if let Some(v) = num_side {
                        if v != 0 && u.as_deref().map_or(false, |u| u != "JPY") {
                            self.d(ERROR, "単位", &format!("「{}」: {}の金額と比べたり足したりする 0 以外の数字は直接書けません（money(整数, 通貨) で作ります）", show(e), unit_label(u.as_ref().unwrap())), 0);
                        }
                    }
                }
            }
        }
        if op == "かつ" || op == "または" {
            if a != "Bool" || b != "Bool" {
                self.d(ERROR, "型", &format!("「{}」: {} の両側は条件（真偽）である必要があります", show(e), if op == "かつ" { "and" } else { "or" }), 0);
            }
            return "Bool".into();
        }
        if op == "in" {
            if !(b == "Set" || b.starts_with("Map:")) {
                self.d(ERROR, "型", &format!("「{}」: in の右は、集合か写像です（{}）", show(e), b), 0);
            } else if a != "ID" {
                self.d(ERROR, "型", &format!("「{}」: in の左は、id（集合の要素・写像のキー）です（{}）", show(e), a), 0);
            }
            return "Bool".into();
        }
        if ["<", ">", "<=", ">="].contains(&op) {
            if !((num(&a) && num(&b) && !mixed) || (a == "Time" && b == "Time")) {
                let tail = if mixed { "（整数と金額は比べられません）" } else { "" };
                self.d(ERROR, "型", &format!("「{}」: 同じ種類の数どうし（または日時どうし）で比べてください{}", show(e), tail), 0);
            }
            return "Bool".into();
        }
        if op == "==" || op == "!=" {
            if a != b {
                let null_cmp = a == "Null" || b == "Null";
                let tail = if null_cmp {
                    let x = show(if b == "Null" { l } else { r });
                    if op == "==" {
                        format!("（null かどうかは is null で確かめます：{} is null）", x)
                    } else {
                        format!("（null でないことは not ({} is null) で確かめます）", x)
                    }
                } else {
                    String::new()
                };
                self.d(ERROR, "型", &format!("「{}」: 違う型を比べています{}", show(e), tail), 0);
            }
            return "Bool".into();
        }
        if (op == "+" || op == "-") && (a == "Time" || b == "Time") && !(op == "-" && a == "Dur") {
            if (a == "Time" && b == "Dur") || (op == "+" && a == "Dur" && b == "Time") {
                return "Time".into();
            }
            if a == "Time" && b == "Time" && op == "-" {
                return "Int".into();
            }
            self.d(ERROR, "型", &format!("「{}」: 時刻に足し引きできるのは期間（30 min など）だけです。時刻どうしは引き算（差は秒数）だけできます", show(e)), 0);
            return "?".into();
        }
        if a == "Dur" || b == "Dur" {
            self.d(ERROR, "型", &format!("「{}」: 期間は、時刻に足し引きするときだけ使えます", show(e)), 0);
            return "?".into();
        }
        if op == "+" || op == "-" {
            if a == "Rational" || b == "Rational" {
                self.d(QUESTION, "丸め", &format!("「{}」: 端数が出る計算の丸め（切り捨て・切り上げ・四捨五入）と単位が指定されていません。端数をどう丸めますか", show(e)), 0);
                return "Money".into();
            }
            if num(&a) && num(&b) && !mixed {
                self.check_calc(e, env);
                return a;
            }
            let tail = if mixed { "（整数と金額は、そのままでは足し引きできません）" } else { "" };
            self.d(ERROR, "型", &format!("「{}」: 同じ種類の数どうしで計算してください{}", show(e), tail), 0);
            return "?".into();
        }
        if op == "/" || op == "%" {
            if a != "Int" {
                self.d(ERROR, "型", &format!("「{}」: 割り算できるのは整数だけです（{}）", show(e), a), 0);
                return "?".into();
            }
            if !matches!(r, Expr::Num { .. }) {
                self.check_divisor(e, &b, env);
            }
            return "Int".into();
        }
        if op == "*" {
            if a == "Money" && b == "Money" {
                self.d(ERROR, "型", &format!("「{}」: 金額どうしは掛けられません（金額 × 整数 か 金額 × 割合 にしてください）", show(e)), 0);
                return "?".into();
            }
            if num(&a) && num(&b) {
                self.check_calc(e, env);
                return if a == "Money" || b == "Money" { "Money".into() } else { "Int".into() };
            }
            let frac = |t: &str| t == "Ratio" || t == "Rational";
            let inset = |t: &str| ["Ratio", "Rational", "Int", "Money"].contains(&t);
            if inset(&a) && inset(&b) && (frac(&a) || frac(&b)) {
                self.check_frac_fit(e, env);
                return "Rational".into();
            }
            self.d(ERROR, "型", &format!("「{}」: 掛けられない組み合わせです", show(e)), 0);
            return "?".into();
        }
        "?".into()
    }

    /// 片側か両側が "…" と直に書いた文字列の式
    pub fn text_binop(&mut self, e: &Expr, env: &Rc<Env>) -> String {
        let (op, l, r) = match e {
            Expr::Bin { op, l, r, .. } => (op.as_str(), &**l, &**r),
            _ => return "?".into(),
        };
        let mut side = |s: &mut Self, x: &Expr| -> String {
            if let Expr::StrLit { .. } = x {
                s.info.types.insert(id_of(x), "Str".to_string());
                "Str".to_string()
            } else {
                s.ty(x, env)
            }
        };
        let a = side(self, l);
        let b = side(self, r);
        if a == "?" || b == "?" {
            return "?".into();
        }
        if !["==", "!=", "+"].contains(&op) {
            self.d(ERROR, "型", &format!("「{}」: 直に書いた文字列は、文字列との == ・!= と、文字列をつなぐ + にだけ使えます（大小は比べられません）", show(e)), 0);
            return "?".into();
        }
        if a != "Str" || b != "Str" {
            self.d(
                ERROR,
                "型",
                &format!("「{}」: 直に書いた文字列と{}られるのは文字列だけです（{}）。数を文字にするには convert を通します", show(e), if op != "+" { "比べ" } else { "つなげ" }, if a == "Str" { &b } else { &a }),
                0,
            );
            return "?".into();
        }
        if op == "+" {
            return "Str".into();
        }
        let (lit, other) = if matches!(l, Expr::StrLit { .. }) { (l, r) } else { (r, l) };
        if matches!(other, Expr::StrLit { .. }) {
            self.d(ERROR, "型", &format!("「{}」: 直に書いた文字列どうしの比較は、いつも同じ結果です", show(e)), 0);
            return "Bool".into();
        }
        let lits = if let Expr::StrLit { s } = lit { s.clone() } else { String::new() };
        if let Some((lo, hi, chars)) = self.text_shape(other, env) {
            let n = lits.chars().count() as i128;
            if !(lo <= n && n <= hi) {
                self.d(ERROR, "範囲", &format!("「{}」: {} の長さは {}..{} なので、{} 文字の {} と同じになることはありません", show(e), show(other), lo, hi, n, show(lit)), 0);
            } else if let Some(chars) = chars {
                let mut bad: Vec<char> = lits.chars().filter(|c| !chars.iter().any(|(x, y)| *x <= *c as u32 && (*c as u32) <= *y)).collect();
                bad.sort();
                bad.dedup();
                if !bad.is_empty() {
                    self.d(
                        ERROR,
                        "文字種",
                        &format!("「{}」: {} に使える文字は [{}] なので、「{}」を含む {} と同じになることはありません", show(e), show(other), show_chars(&chars), bad.iter().collect::<String>(), show(lit)),
                        0,
                    );
                }
            }
        }
        "Bool".into()
    }

    /// 文字列の式の (最短の長さ, 最長の長さ, 使える文字)。形が分からなければ None
    pub fn text_shape(&self, e: &Expr, env: &Env) -> Option<Shape> {
        match e {
            Expr::StrLit { s } => {
                let n = s.chars().count() as i128;
                let mut cs: Vec<(u32, u32)> = s.chars().map(|c| (c as u32, c as u32)).collect();
                cs.sort();
                cs.dedup();
                return Some((n, n, Some(cs)));
            }
            Expr::Bin { op, l, r, .. } if op == "+" => {
                let a = self.text_shape(l, env)?;
                let b = self.text_shape(r, env)?;
                let chars = match (&a.2, &b.2) {
                    (Some(x), Some(y)) => {
                        let mut v: Vec<(u32, u32)> = x.iter().chain(y.iter()).cloned().collect();
                        v.sort();
                        v.dedup();
                        Some(v)
                    }
                    _ => None,
                };
                return Some((a.0 + b.0, a.1 + b.1, chars));
            }
            Expr::Name { n } => {
                if let Some(s) = self.var_text.get(n) {
                    return Some(s.clone());
                }
            }
            _ => {}
        }
        let t = match e {
            Expr::Call { n, .. } if self.ck.parts.contains_key(n) => Some(self.ck.parts[n].ret.clone()),
            _ => self.decl_type(e, env),
        };
        let t = t?;
        if t.kind != "文字列" {
            return None;
        }
        let r = t.range.clone().unwrap_or_default();
        Some((r.lo.unwrap_or(0), r.hi.unwrap_or(TEXT_LIMIT), t.chars.clone()))
    }

    /// 定義した項目を読む式 x.held を、定義の式 x.stock + x.reserved に開く
    pub fn open_derived(&mut self, e: &Expr, dname: &str) -> Rc<Expr> {
        if let Some(x) = self.info.derived.get(&id_of(e)) {
            return x.clone();
        }
        let (base, n) = match e {
            Expr::Attr { e: b, n } => (b, n),
            _ => unreachable!(),
        };
        let dv = self.ck.data[dname].derived.iter().find(|f| &f.name == n).unwrap();
        fn sub(x: &Expr, base: &Expr) -> Expr {
            match x {
                Expr::Name { n } => Expr::Attr { e: Box::new(base.clone()), n: n.clone() },
                Expr::Bin { op, l, r, .. } => Expr::Bin { op: op.clone(), l: Box::new(sub(l, base)), r: Box::new(sub(r, base)), dflt: None },
                other => other.clone(),
            }
        }
        let opened = Rc::new(sub(dv.expr.as_ref().unwrap(), base));
        self.info.derived.insert(id_of(e), opened.clone());
        opened
    }

    /// 数の式を、項目（その 1 件の今の値）と変数の 1 次の式にする。表せなければ None
    pub fn lin(&mut self, e: &Expr, env: &Rc<Env>) -> Option<Lin> {
        match e {
            Expr::Num { v } => Some((IndexMap::new(), *v)),
            Expr::Name { n } => {
                if let Some(f) = self.var_lin.get(n) {
                    if Rc::ptr_eq(env, &self.env) {
                        return Some(f.clone());
                    }
                }
                let t = self.ty_quiet(e, env);
                if t == "Int" || t == "Money" {
                    let mut m = IndexMap::new();
                    m.insert(atom_v(n), 1);
                    Some((m, 0))
                } else {
                    None
                }
            }
            Expr::Attr { e: b, n } if matches!(&**b, Expr::Name { n: bn } if self.bindings.contains_key(bn)) => {
                let bn = if let Expr::Name { n } = &**b { n } else { unreachable!() };
                let data = self.info.lookup_vars.get(bn).cloned();
                let dt = data.as_ref().and_then(|d| self.ck.data.get(d)).copied()?;
                if dt.derived.iter().any(|f| &f.name == n) {
                    let opened = self.open_derived(e, data.as_ref().unwrap());
                    return self.lin(&opened, env);
                }
                let fld = dt.fields.iter().find(|f| &f.name == n)?;
                if (fld.r#type.kind != "整数" && fld.r#type.kind != "金額") || fld.r#type.optional {
                    return None;
                }
                let mut m = IndexMap::new();
                m.insert(atom_f(&self.bindings[bn], n), 1);
                Some((m, 0))
            }
            Expr::Bin { op, l, r, .. } if op == "+" || op == "-" => {
                let a = self.lin(l, env)?;
                let b = self.lin(r, env)?;
                Some(lin_add(&a, &b, if op == "+" { 1 } else { -1 }))
            }
            Expr::Bin { op, l, r, .. } if op == "*" && (matches!(**l, Expr::Num { .. }) || matches!(**r, Expr::Num { .. })) => {
                let (k, x) = if let Expr::Num { v } = &**l { (*v, &**r) } else if let Expr::Num { v } = &**r { (*v, &**l) } else { unreachable!() };
                let a = self.lin(x, env)?;
                Some((a.0.iter().map(|(n, c)| (n.clone(), c * k)).filter(|(_, c)| *c != 0).collect(), a.1 * k))
            }
            Expr::Call { n, args } if conversion(n).is_some() && args.len() == 1 => self.lin(&args[0], env),
            _ => None,
        }
    }

    pub fn lin_self(&mut self, e: &Expr) -> Option<Lin> {
        let env = self.env.clone();
        self.lin(e, &env)
    }

    /// 1 次の式の項（変数か、ある 1 件の項目）の値の範囲
    pub fn atom_iv(&mut self, a: &str, env: &Rc<Env>) -> Iv {
        let parts: Vec<&str> = a.split('\u{1}').collect();
        if parts[0] == "v" {
            return self.interval(&Expr::Name { n: parts[1].to_string() }, env);
        }
        let (b, f) = (parts[1], parts[2]);
        if let Some(var) = self.bind_names.get(b).cloned() {
            if self.bindings.get(&var).map(|s| s.as_str()) == Some(b) {
                let e = Expr::Attr { e: Box::new(Expr::Name { n: var }), n: f.to_string() };
                return self.interval_self(&e);
            }
        }
        let data = self.bind_data(b);
        match data {
            Some(d) => self.field_iv(&d, f),
            None => (None, None),
        }
    }

    /// 条件 c が成り立つ／成り立たないときに分かる、数どうしの関係（式 <= 0 の集まり）
    pub fn cond_rel(&mut self, c: &Expr, holds: bool) -> IndexSet<(Vec<(String, i128)>, i128)> {
        if let Expr::Not { e } = c {
            return self.cond_rel(e, !holds);
        }
        if let Expr::Bin { op, l, r, .. } = c {
            if op == if holds { "かつ" } else { "または" } {
                let mut a = self.cond_rel(l, holds);
                a.extend(self.cond_rel(r, holds));
                return a;
            }
        }
        let (op, l, r) = match c {
            Expr::Bin { op, l, r, .. } if ["<", "<=", ">", ">=", "==", "!="].contains(&op.as_str()) => (op.as_str(), &**l, &**r),
            _ => return IndexSet::new(),
        };
        let env = self.env.clone();
        let (tl, tr) = (self.ty_quiet(l, &env), self.ty_quiet(r, &env));
        if !(tl == "Int" || tl == "Money") || !(tr == "Int" || tr == "Money") {
            return IndexSet::new();
        }
        let (a, b) = (self.lin(l, &env), self.lin(r, &env));
        let (a, b) = match (a, b) {
            (Some(a), Some(b)) => (a, b),
            _ => return IndexSet::new(),
        };
        let op2 = if holds {
            op
        } else {
            match op {
                "<" => ">=",
                "<=" => ">",
                ">" => "<=",
                ">=" => "<",
                "==" => "!=",
                _ => "==",
            }
        };
        let le = lin_add(&a, &b, -1);
        let ge = lin_add(&b, &a, -1);
        let bump = |f: &Lin| (f.0.clone(), f.1 + 1);
        let forms: Vec<Lin> = match op2 {
            "<=" => vec![le],
            "<" => vec![bump(&le)],
            ">=" => vec![ge],
            ">" => vec![bump(&ge)],
            "==" => vec![le, ge],
            _ => vec![],
        };
        forms.into_iter().map(|f| (canon(&f.0), f.1)).collect()
    }

    /// 1 次の式 t が 0 以下と示せるか
    pub fn prove_le0(&mut self, t: &Lin, env: &Rc<Env>) -> bool {
        let mut known: Vec<Lin> = self.rel.iter().map(|(items, c)| (items.iter().cloned().collect(), *c)).collect();
        let mut bs: IndexSet<String> = IndexSet::new();
        for a in t.0.keys() {
            let p: Vec<&str> = a.split('\u{1}').collect();
            if p[0] == "f" {
                bs.insert(p[1].to_string());
            }
        }
        for b in &bs {
            let data = self.bind_data(b);
            for r in &self.ck.rel_invs {
                if Some(&r.data) == data.as_ref() && r.when.as_ref().map_or(true, |(sf, sv)| self.state_guards.contains(&(b.clone(), sf.clone(), sv.clone()))) {
                    for con in &r.cons {
                        known.push((con.0.iter().map(|(f, c)| (atom_f(b, f), *c)).collect(), con.1));
                    }
                }
            }
        }
        let mut cands: Vec<Lin> = vec![(IndexMap::new(), 0)];
        cands.extend(known.iter().cloned());
        for (i, x) in known.iter().enumerate() {
            for y in &known[i + 1..] {
                cands.push(lin_add(x, y, 1));
            }
        }
        for k in cands {
            let form = lin_add(t, &k, -1);
            let mut ivs: IndexMap<String, Iv> = IndexMap::new();
            for a in form.0.keys() {
                let iv = self.atom_iv(a, env);
                ivs.insert(a.clone(), iv);
            }
            let hi = lin_bounds(&form, &ivs).1;
            if hi.map_or(false, |h| !h.is_positive()) {
                return true;
            }
        }
        false
    }
}

/// 1 次の式の項を、並びによらない形にする（Python の frozenset(items) に当たる）
pub fn canon(m: &IndexMap<String, i128>) -> Vec<(String, i128)> {
    let mut v: Vec<(String, i128)> = m.iter().map(|(k, c)| (k.clone(), *c)).collect();
    v.sort();
    v
}

/// Python の `{x:.2e}` に当たる表示
pub fn sci(x: &Q) -> String {
    use num_traits::ToPrimitive;
    let f = x.numer().to_f64().unwrap_or(f64::INFINITY) / x.denom().to_f64().unwrap_or(1.0);
    let s = format!("{:.2e}", f);
    // Rust は 1.23e38、Python は 1.23e+38
    match s.split_once('e') {
        Some((m, e)) => {
            let (sign, digits) = if let Some(d) = e.strip_prefix('-') { ('-', d) } else { ('+', e) };
            format!("{}e{}{:0>2}", m, sign, digits)
        }
        None => s,
    }
}

#[allow(dead_code)]
fn _unused(_: BigInt) {}
