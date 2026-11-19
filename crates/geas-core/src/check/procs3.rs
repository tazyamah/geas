// SPDX-License-Identifier: GPL-3.0-or-later
//! ProcChecker の続き：分かっていること・文の検査・書き込みの証明
use super::procs::*;
use super::procs2::*;
use super::*;
use num_traits::Signed;
use std::rc::Rc;

pub fn stmt_line(st: &Stmt) -> i64 {
    match st {
        Stmt::Return(x) => x.line,
        Stmt::Assign(x) => x.line,
        Stmt::Create(x) => x.line,
        Stmt::Delete(x) => x.line,
        Stmt::If(x) => x.line,
        Stmt::Update(x) => x.line,
        Stmt::SetField(x) => x.line,
        Stmt::Request(x) => x.line,
        Stmt::ForEach(x) => x.line,
    }
}

fn stmt_calc(st: &Stmt) -> Option<(String, String)> {
    match st {
        Stmt::Return(x) => x.calc.clone(),
        Stmt::Assign(x) => x.calc.clone(),
        Stmt::Create(x) => x.calc.clone(),
        Stmt::SetField(x) => x.calc.clone(),
        _ => None,
    }
}

fn negate(op: &str) -> &'static str {
    match op {
        "<" => ">=",
        "<=" => ">",
        ">" => "<=",
        ">=" => "<",
        "==" => "!=",
        _ => "==",
    }
}

#[derive(Default)]
pub struct WriteOpts<'x> {
    pub bind: Option<&'x str>,
    pub note: Option<String>,
    pub grow: bool,
    pub new_state: Option<&'x IndexMap<String, String>>,
    pub null: bool,
    pub fix: Option<String>,
    pub form: Option<Lin>,
}

impl<'c, 'a> ProcChecker<'c, 'a> {
    pub fn bind_data(&self, b: &str) -> Option<String> {
        if let Some(x) = self.binding_data.get(b) {
            return x.clone();
        }
        b.strip_prefix("#update\u{2}").map(|s| s.to_string())
    }

    pub fn bind_of(&self, var: &str) -> String {
        self.bindings.get(var).cloned().unwrap_or_else(|| var.to_string())
    }

    pub fn add_facts(&self, mut facts: IndexMap<FKey, Q>, new: IndexMap<FKey, Q>) -> IndexMap<FKey, Q> {
        for (k, lo) in new {
            let better = match facts.get(&k) {
                None => true,
                Some(old) => {
                    if k[0] == UP {
                        &lo < old
                    } else {
                        &lo > old
                    }
                }
            };
            if better {
                facts.insert(k, lo);
            }
        }
        facts
    }

    /// 条件 c が成り立つ（holds）／成り立たないときに分かる下限
    pub fn cond_facts(&mut self, c: &Expr, holds: bool) -> IndexMap<FKey, Q> {
        if let Expr::Not { e } = c {
            return self.cond_facts(e, !holds);
        }
        if let Expr::Bin { op, l, r, .. } = c {
            if op == if holds { "かつ" } else { "または" } {
                let a = self.cond_facts(l, holds);
                let b = self.cond_facts(r, holds);
                return self.add_facts(a, b);
            }
        }
        let (cop, cl, nval) = match c {
            Expr::Bin { op, l, r, .. } if ["<", "<=", ">", ">=", "==", "!="].contains(&op.as_str()) => match &**r {
                Expr::Num { v } => (op.as_str(), &**l, *v),
                _ => return IndexMap::new(),
            },
            _ => return IndexMap::new(),
        };
        let env = self.env.clone();
        let (b, f): (String, String);
        let mut handled = false;
        let mut bf: (String, String) = (String::new(), String::new());
        if let Expr::Call { n, args } = cl {
            if n == "len" && args.len() == 1 {
                if let Expr::Attr { e: ae, n: an } = &args[0] {
                    if let Expr::Name { n: bn } = &**ae {
                        if let Some(bind) = self.bindings.get(bn).cloned() {
                            let t = self.decl_type(&args[0], &env);
                            if t.map_or(true, |t| t.kind != "集合") {
                                return IndexMap::new();
                            }
                            bf = (bind, format!("{}{}", LEN, an));
                            handled = true;
                        }
                    }
                }
            }
        }
        if !handled {
            if let Expr::Attr { e: ae, n: an } = cl {
                if let Expr::Name { n: bn } = &**ae {
                    if let Some(bind) = self.bindings.get(bn).cloned() {
                        let t = self.decl_type(cl, &env);
                        if t.map_or(true, |t| t.kind != "整数" && t.kind != "金額") {
                            return IndexMap::new();
                        }
                        bf = (bind, an.clone());
                        handled = true;
                    }
                }
            }
        }
        if !handled {
            if let Expr::Name { n } = cl {
                if !self.bindings.contains_key(n) && self.ty_quiet(cl, &env) == "Int" {
                    bf = (NAMED.to_string(), n.clone());
                    handled = true;
                }
            }
        }
        if !handled {
            return IndexMap::new();
        }
        b = bf.0;
        f = bf.1;
        let op = if holds { cop } else { negate(cop) };
        let mut lo: Option<i128> = match op {
            ">" => Some(nval + 1),
            ">=" | "==" => Some(nval),
            _ => None,
        };
        let mut hi: Option<i128> = match op {
            "<" => Some(nval - 1),
            "<=" | "==" => Some(nval),
            _ => None,
        };
        if op == "!=" {
            let known = self.interval(cl, &env);
            if known.0 == Some(q(nval)) {
                lo = Some(nval + 1);
            } else if known.1 == Some(q(nval)) {
                hi = Some(nval - 1);
            }
        }
        let mut out: IndexMap<FKey, Q> = IndexMap::new();
        if let Some(lo) = lo {
            out.insert(vec![b.clone(), f.clone()], q(lo));
        }
        if let Some(hi) = hi {
            out.insert(vec![UP.to_string(), b, f], q(hi));
        }
        out
    }

    /// 足す・引く・掛けるの結果が、計算に使う幅に収まると示す
    pub fn check_calc(&mut self, e: &Expr, env: &Rc<Env>) {
        if !self.calc_on {
            return;
        }
        let (lo, hi) = self.interval(e, env);
        if let (Some(l), Some(h)) = (&lo, &hi) {
            if l >= &q(i128::MIN) && h <= &q(i128::MAX) {
                self.info.calc.entry(id_of(e)).or_insert_with(|| "prove".to_string());
                return;
            }
        }
        let t = self.cur_calc.clone();
        let line = self.cur_line;
        match t {
            None => {}
            Some(ref t) if t.0 == "prove" => {}
            Some(t) => {
                self.calc_hits += 1;
                self.info.calc.insert(id_of(e), t.0.clone());
                let row = (show(e), t.0.clone(), t.1.clone());
                if !self.info.calc_trusts.contains(&row) {
                    self.info.calc_trusts.push(row);
                    if t.0 == "check" {
                        self.d(WARN, "計算", &format!("「{}」: 途中の計算が収まると示せていません（trust check。溢れたら異常で止まります）", show(e)), line);
                    }
                }
                return;
            }
        }
        self.info.calc.insert(id_of(e), "check".to_string());
        self.d(
            ERROR,
            "計算",
            &format!(
                "「{}」: 途中の計算が収まると示せません（値の範囲は {}）。掛ける数・足す数の型に範囲を書くか、先に if で大きさを確かめます。人が引き受けるなら、文の末尾に trust check \"理由\"（警告を出し、溢れたら異常で止める）、trust verify \"理由\"（検証ビルドだけ確かめる）、trust assume \"理由\"（確かめない）のどれかを書きます",
                show(e),
                iv_show(&(lo, hi))
            ),
            line,
        );
    }

    // ---- 道ごとの「分かっていること」 ----
    pub fn snapshot(&self) -> Snap {
        Snap {
            state_guards: self.state_guards.clone(),
            facts: self.facts.clone(),
            guarded: self.guarded.clone(),
            present: self.present.clone(),
            cur_ok: self.cur_ok.clone(),
            cur_sets: self.cur_sets.clone(),
            shortage_sets: self.shortage_sets.clone(),
            path_writes: self.path_writes.clone(),
            rel: self.rel.clone(),
        }
    }

    pub fn restore(&mut self, s: Snap) {
        self.state_guards = s.state_guards;
        self.facts = s.facts;
        self.guarded = s.guarded;
        self.present = s.present;
        self.cur_ok = s.cur_ok;
        self.cur_sets = s.cur_sets;
        self.shortage_sets = s.shortage_sets;
        self.path_writes = s.path_writes;
        self.rel = s.rel;
    }

    /// 2つの道の合流。両方で分かっていることだけが残り、書いた回数は多いほう
    pub fn join(a: &Snap, b: &Snap) -> Snap {
        fn same<K: std::hash::Hash + Eq + Clone, V: PartialEq + Clone>(x: &IndexMap<K, V>, y: &IndexMap<K, V>) -> IndexMap<K, V> {
            x.iter().filter(|(k, v)| y.get(*k).map_or(false, |w| w == *v)).map(|(k, v)| (k.clone(), v.clone())).collect()
        }
        let mut facts: IndexMap<FKey, Q> = IndexMap::new();
        for (k, va) in &a.facts {
            if let Some(vb) = b.facts.get(k) {
                let v = if k[0] == UP { if va > vb { va.clone() } else { vb.clone() } } else if va < vb { va.clone() } else { vb.clone() };
                facts.insert(k.clone(), v);
            }
        }
        let mut pw = a.path_writes.clone();
        for (k, v) in &b.path_writes {
            let e = pw.entry(k.clone()).or_insert(0);
            if *v > *e {
                *e = *v;
            }
        }
        Snap {
            state_guards: a.state_guards.intersection(&b.state_guards).cloned().collect(),
            facts,
            guarded: a.guarded.intersection(&b.guarded).cloned().collect(),
            present: a.present.intersection(&b.present).cloned().collect(),
            cur_ok: same(&a.cur_ok, &b.cur_ok),
            cur_sets: same(&a.cur_sets, &b.cur_sets),
            shortage_sets: same(&a.shortage_sets, &b.shortage_sets),
            path_writes: pw,
            rel: a.rel.intersection(&b.rel).cloned().collect(),
        }
    }

    /// 項目へ書いた。同じ道で同じ項目へ2回書くと、エラー
    pub fn note_write(&mut self, w: &str) {
        self.info.writes.insert(w.to_string());
        if self.path_writes.get(w).copied().unwrap_or(0) > 0 && !self.dup_writes.contains(w) {
            self.dup_writes.insert(w.to_string());
            self.d(ERROR, "書込", &format!("1つの処理で「{}」に2回書いています（要素が重なると結果が順序で変わります。if と else のように、同時には通らない道で書き分けるのはかまいません）", w), 0);
        }
        *self.path_writes.entry(w.to_string()).or_insert(0) += 1;
    }

    /// 文の並び。what は if・else の中
    pub fn block(&mut self, stmts: &[Stmt], pos: usize, what: &str) {
        let before: IndexSet<String> = self.env.keys().cloned().collect();
        for (k, inner) in stmts.iter().enumerate() {
            if k > 0 && always_returns(&stmts[..k]) {
                self.d(ERROR, "結果", "この文には届きません（前の文が、どの道でも return しています）", stmt_line(inner));
                return;
            }
            if !what.is_empty() && matches!(inner, Stmt::Delete(_)) {
                self.d(ERROR, "型", &format!("{} の中に書けるのは、return・let・create・項目の更新・update・request・if・for だけです", what), stmt_line(inner));
                continue;
            }
            self.stmt(inner, pos);
        }
        if !what.is_empty() {
            let gone: IndexSet<String> = self.env.keys().filter(|k| !before.contains(*k)).cloned().collect();
            self.forget_names(&gone);
        }
    }

    /// 名前と、その名前について分かっていたことを消す
    pub fn forget_names(&mut self, names: &IndexSet<String>) {
        for n in names {
            Rc::make_mut(&mut self.env).shift_remove(n);
            self.info.lookup_vars.shift_remove(n);
            self.bindings.shift_remove(n);
            self.var_iv.shift_remove(n);
            self.var_unit.shift_remove(n);
            self.shortage_sets.shift_remove(n);
            self.exist_sets.shift_remove(n);
            self.cur_sets.shift_remove(n);
            self.var_text.shift_remove(n);
            self.loop_deps.shift_remove(n);
            self.present.shift_remove(&("#入力".to_string(), n.clone()));
        }
        self.facts.retain(|k, _| {
            let l = k.len();
            !(l >= 2 && names.iter().any(|n| k[l - 2] == NAMED && &k[l - 1] == n))
        });
        for n in names {
            self.var_lin.shift_remove(n);
        }
        let gone: IndexSet<String> = names.iter().map(|n| atom_v(n)).collect();
        self.rel.retain(|r| !r.0.iter().any(|(a, _)| gone.contains(a)));
        self.var_lin.retain(|_, f| !f.0.keys().any(|a| gone.contains(a)));
    }

    /// let 名前 = データ.範囲1.範囲2[ID] else { return … }
    pub fn scoped_lookup(&mut self, st: &Assign, ix: &Expr) {
        let (d, scopes) = match ix {
            Expr::Index { d, scopes, .. } => (d, scopes),
            _ => return,
        };
        let p = self.ck.p;
        if st.absent.is_none() {
            self.d(ERROR, "範囲", "範囲を通して引くときは、無いとき（範囲の外のときも含む）の扱い else { return … } が要ります", st.line);
            return;
        }
        self.scoped_ok.insert(id_of(ix));
        let mut by: IndexMap<&String, &Scope> = IndexMap::new();
        for s in p.scopes.iter().rev() {
            by.insert(&s.name, s);
        }
        for n in scopes {
            match by.get(n) {
                None => {
                    let msg = format!("範囲「{}」が未定義です{}", n, near(n, by.keys().copied()));
                    self.d(ERROR, "範囲", &msg, st.line);
                }
                Some(s) => {
                    if &s.data != d {
                        self.d(ERROR, "範囲", &format!("範囲 {} は {} の範囲なので、{} には使えません", n, s.data, d), st.line);
                    } else if scopes.iter().filter(|x| *x == n).count() > 1 {
                        self.d(ERROR, "範囲", &format!("範囲 {} を重ねて書いています", n), st.line);
                    } else if !s.params.is_empty() {
                        self.d(ERROR, "範囲", &format!("引数つきの範囲 {} は、処理の 1 件引きにはまだ使えません", n), st.line);
                    } else {
                        if !self.info.scopes.contains(n) {
                            self.info.scopes.push(n.clone());
                        }
                        self.info.caller = self.info.caller || !s.caller.is_empty();
                        for g in &s.conds {
                            for (f, _, _) in g {
                                self.info.field_reads.insert(format!("{}.{}", d, f));
                            }
                        }
                        if !s.caller.is_empty() {
                            self.info.field_reads.insert(format!("{}.{}", d, s.caller));
                        }
                    }
                }
            }
        }
        if self.info.caller && self.proc.inputs.iter().any(|(n, _)| n == CALLER) {
            self.d(ERROR, "範囲", &format!("入力の名前に {} は使えません（範囲の中で、呼んだ本人の ID を表す語です）", CALLER), st.line);
        }
    }

    pub fn if_stmt(&mut self, st: &If, pos: usize) {
        let ct = self.ty_self(&st.cond);
        if ct != "Bool" && ct != "?" {
            self.d(ERROR, "型", &format!("if の条件「{}」は、成り立つか成り立たないかの条件である必要があります", show(&st.cond)), st.line);
        }
        let c = &st.cond;
        let mut fact: Option<(String, String, String)> = None;
        if let Expr::Bin { op, l, r, .. } = c {
            if op == "==" || op == "!=" {
                if let (Expr::Attr { e: ae, n: an }, Expr::Name { n: rn }) = (&**l, &**r) {
                    if let Expr::Name { n: bn } = &**ae {
                        if let Some(b) = self.bindings.get(bn) {
                            fact = Some((b.clone(), an.clone(), rn.clone()));
                        }
                    }
                }
            }
        }
        let state_cmp = fact.is_some();
        if st.body.is_empty() {
            self.d(ERROR, "型", "if の中が空です", st.line);
        }
        if let Some(els) = &st.els {
            if els.is_empty() {
                self.d(ERROR, "型", "else の中が空です（何もしないなら else を書きません）", if st.else_line != 0 { st.else_line } else { st.line });
            }
        }
        let outer = self.snapshot();
        let is_eq = matches!(c, Expr::Bin { op, .. } if op == "==");
        if state_cmp && is_eq {
            self.state_guards.insert(fact.clone().unwrap());
        }
        let f = self.cond_facts(c, true);
        let cur = std::mem::take(&mut self.facts);
        self.facts = self.add_facts(cur, f);
        let r = self.cond_rel(c, true);
        self.rel.extend(r);
        self.block(&st.body, pos, "if");
        let then_end = self.snapshot();
        self.restore(outer);
        let f = self.cond_facts(c, false);
        let cur = std::mem::take(&mut self.facts);
        self.facts = self.add_facts(cur, f);
        let r = self.cond_rel(c, false);
        self.rel.extend(r);
        self.negated_facts(c, state_cmp, fact);
        if let Some(els) = &st.els {
            self.block(els, pos, "else");
        }
        let then_ret = always_returns(&st.body);
        let else_ret = st.els.as_ref().map_or(false, |e| always_returns(e));
        if then_ret && !else_ret {
            return;
        }
        if else_ret && !then_ret {
            self.restore(then_end);
        } else if !then_ret {
            let now = self.snapshot();
            let j = Self::join(&then_end, &now);
            self.restore(j);
        }
    }

    /// 条件 c が成り立たなかった道で分かること
    pub fn negated_facts(&mut self, c: &Expr, state_cmp: bool, fact: Option<(String, String, String)>) {
        let (inner, kind) = match c {
            Expr::Pred { e, kind } => (&**e, kind.as_str()),
            Expr::Bin { op, .. } => {
                if state_cmp && op == "!=" {
                    self.state_guards.insert(fact.unwrap());
                }
                return;
            }
            _ => return,
        };
        if kind == "null" {
            if let Expr::Attr { e: be, n } = inner {
                if let Expr::Name { n: bn } = &**be {
                    if let Some(b) = self.bindings.get(bn) {
                        self.present.insert((b.clone(), n.clone()));
                    }
                }
            }
            if let Expr::Name { n } = inner {
                self.present.insert(("#入力".to_string(), n.clone()));
            }
            if let Expr::Attr { e: be, n } = inner {
                if self.ty_quiet_self(be).starts_with("Comp:") {
                    self.present.insert((comp_key(&show(be)), n.clone()));
                }
            }
        }
        if kind == "空でない" {
            if let Expr::Name { n } = inner {
                if let Some(x) = self.exist_sets.get(n) {
                    self.guarded.insert(x.clone());
                }
                if let Some((d, sib, src, unit)) = self.cur_sets.get(n).cloned() {
                    self.cur_ok.insert((d, sib, src), unit);
                }
                if let Some((a, b, c2)) = self.shortage_sets.get(n).cloned() {
                    self.facts.insert(vec!["#不足".to_string(), a, b, c2], q(0));
                }
            }
        }
        if kind == "ない" {
            if let Expr::Index { d, e, .. } = inner {
                self.guarded.insert((d.clone(), show(e)));
            }
        }
    }

    /// let で、すでにある名前は付け直せない
    pub fn no_rebind(&mut self, name: &str, line: i64) {
        if self.env.contains_key(name) {
            self.d(ERROR, "名前", &format!("「{}」は、すでにある名前です（入力・slot・前の let）。let で同じ名前は付け直せません。別の名前にしてください", name), line);
        }
    }

    pub fn stmt(&mut self, st: &Stmt, pos: usize) {
        let saved = (self.cur_line, self.cur_calc.clone(), self.calc_hits);
        let ln = stmt_line(st);
        self.cur_line = if ln != 0 { ln } else { saved.0 };
        let calc = stmt_calc(st);
        let guard = if let Stmt::SetField(s) = st { s.guard.clone() } else { None };
        self.cur_calc = if calc.is_some() {
            calc.clone()
        } else if guard.as_ref().map_or(false, |g| g.0 == "assume") {
            guard.clone()
        } else {
            None
        };
        self.calc_hits = 0;
        self.calc_on = true;
        self.stmt_inner(st, pos);
        if let Some(c) = &calc {
            if c.0 != "prove" && self.calc_hits == 0 {
                let line = self.cur_line;
                self.d(ERROR, "計算", &format!("trust {} は要りません。この文の計算は、どれも収まると示せています（trust を消すか、名前を残すなら trust prove と書きます）", c.0), line);
            }
        }
        self.cur_line = saved.0;
        self.cur_calc = saved.1;
        self.calc_hits = saved.2;
    }

    pub fn check_return(&mut self, r: &Return) {
        let rdef = match self.ck.results.get(&self.proc.result).copied() {
            Some(x) => x,
            None => return,
        };
        let v = match rdef.variants.iter().find(|x| x.0 == r.variant) {
            Some(v) => v,
            None => {
                let msg = format!("「{}」は結果 {} にありません{}", r.variant, rdef.name, near_or_all(&r.variant, rdef.variants.iter().map(|(x, _)| x.clone())));
                self.d(ERROR, "結果", &msg, r.line);
                return;
            }
        };
        self.info.returns.insert(r.variant.clone());
        if v.1.len() != r.args.len() {
            self.d(ERROR, "結果", &format!("結果「{}」の引数は {} 個です", r.variant, v.1.len()), r.line);
            return;
        }
        let sib: IndexMap<&String, &Expr> = v.1.iter().map(|(pn, _)| pn).zip(r.args.iter()).collect();
        for ((_, t), a) in v.1.iter().zip(r.args.iter()) {
            let at = self.ty_self(a);
            if at != tok_of(t) && at != "?" && !(matches!(a, Expr::Num { .. }) && (tok_of(t) == "Int" || tok_of(t) == "Money")) {
                self.d(ERROR, "型", &format!("結果「{}」の引数の型が違います（{}）", r.variant, at), r.line);
            }
            self.fit_unit(a, t, &format!("結果「{}」の引数", r.variant), r.line, Some(&sib), None);
            if (t.kind == "整数" || t.kind == "金額") && (at == "Int" || at == "Money") {
                let (lo, hi) = iv_of(t);
                let iv = self.interval_self(a);
                if (lo.is_some() && (iv.0.is_none() || iv.0.as_ref().unwrap() < lo.as_ref().unwrap())) || (hi.is_some() && (iv.1.is_none() || iv.1.as_ref().unwrap() > hi.as_ref().unwrap())) {
                    self.d(
                        ERROR,
                        "範囲",
                        &format!(
                            "結果「{}」の中身「{}」に入れる値が、型の範囲 {} に収まると示せません（値の範囲 {}）。値を限ってから返すか、結果の中身の型の範囲を広げてください",
                            r.variant,
                            pn_of(&v.1, t),
                            iv_show(&(lo, hi)),
                            iv_show(&iv)
                        ),
                        r.line,
                    );
                }
            }
        }
    }

    pub fn run(&mut self) {
        let proc = self.proc;
        if proc.result.is_empty() {
            self.d(ERROR, "名前", "結果の形の宣言がありません（宣言に result 結果の名前 を書き、その結果を result 名前 { 種類, … } で定義します）", 0);
        } else if !self.ck.results.contains_key(&proc.result) {
            let msg = format!("結果「{}」が未定義です{}", proc.result, near(&proc.result, self.ck.results.keys()));
            self.d(ERROR, "名前", &msg, 0);
        }
        for (n, t) in &proc.inputs {
            self.env_set(n, &tok_of(t));
        }
        for (n, t) in &proc.refs {
            self.env_set(n, &tok_of(t));
        }
        self.exist_sets = IndexMap::new();
        for (pos, st) in proc.body.iter().enumerate() {
            if pos > 0 && always_returns(&proc.body[..pos]) {
                self.d(ERROR, "結果", "この文には届きません（前の文が、どの道でも return しています）", stmt_line(st));
                break;
            }
            self.stmt(st, pos);
        }
        self.cur_line = 0;
        if !always_returns(&proc.body) {
            let line = proc.body.last().map_or(0, stmt_line);
            self.d(ERROR, "結果", "処理の最後は return で終えてください（結果を返さずに終わる道があります）", line);
        }
        self.finish();
    }

    pub fn finish(&mut self) {
        let proc = self.proc;
        let reads = self.info.reads();
        let writes = self.info.writes.clone();
        let effs: IndexSet<String> = self.info.effects.iter().map(|(e, _)| e.clone()).collect();
        let mut cmp = |s: &mut Self, kw: &str, does: &str, doesnt: &str, declared: &[String], actual: &IndexSet<String>| {
            let decl: IndexSet<&String> = declared.iter().collect();
            let mut extra: Vec<&String> = actual.iter().filter(|x| !decl.contains(x)).collect();
            extra.sort();
            for x in extra {
                s.d(ERROR, "宣言", &format!("本体は「{}」を{}のに、宣言の {} にありません（{} に足してください）", x, does, kw, kw), 0);
            }
            let mut missing: Vec<&String> = decl.iter().filter(|x| !actual.contains(**x)).cloned().collect();
            missing.sort();
            for x in missing {
                s.d(ERROR, "宣言", &format!("宣言の {} に「{}」がありますが、本体では{}（{} から外してください）", kw, x, doesnt, kw), 0);
            }
        };
        cmp(self, "reads", "読む", "読んでいません", &proc.reads, &reads);
        cmp(self, "writes", "書く", "書いていません", &proc.writes, &writes);
        cmp(self, "effects", "要求する", "要求していません", &proc.effects, &effs);
        let declared_tr: IndexSet<&(String, String, String)> = proc.transitions.iter().collect();
        let mut miss: Vec<&(String, String, String)> = declared_tr.iter().filter(|t| !self.info.transitions.contains(**t)).cloned().collect();
        miss.sort();
        for tr in miss {
            self.d(ERROR, "宣言", &format!("宣言した遷移 {}: {} → {} が、本体にありません（transition から外すか、本体で状態を変えてください）", tr.0, tr.1, tr.2), 0);
        }
        if let Some(rdef) = self.ck.results.get(&proc.result).copied() {
            for (v, _) in &rdef.variants {
                if !self.info.returns.contains(v) {
                    self.d(WARN, "結果", &format!("結果「{}」を返す箇所がありません", v), 0);
                }
            }
        }
    }
}
