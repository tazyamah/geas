// SPDX-License-Identifier: GPL-3.0-or-later
//! ProcChecker の続き：各文の検査と、書き込みの証明（項目の範囲と不変条件）
use super::procs::*;
use super::procs2::*;
use super::procs3::*;
use super::*;
use num_traits::Signed;
use std::rc::Rc;

fn numeric(k: &str) -> bool {
    k == "整数" || k == "金額" || k == "割合"
}

fn num_ok(vt: &str, a: &Expr, tok: &str) -> bool {
    vt == tok || (matches!(a, Expr::Num { .. }) && (tok == "Int" || tok == "Money"))
}

impl<'c, 'a> ProcChecker<'c, 'a> {
    pub fn stmt_inner(&mut self, st: &Stmt, pos: usize) {
        self.pos = pos;
        match st {
            Stmt::Return(r) => self.check_return(r),
            Stmt::If(i) => self.if_stmt(i, pos),
            Stmt::Assign(a) => self.stmt_assign(a),
            Stmt::Update(u) => self.stmt_update(u),
            Stmt::SetField(s) => self.stmt_setfield(s),
            Stmt::Create(c) => {
                self.no_rebind(&c.name, c.line);
                self.create(c);
            }
            Stmt::Delete(d) => self.delete(d),
            Stmt::Request(r) => self.request(r),
            Stmt::ForEach(f) => {
                let env = self.env.clone();
                let env2 = self.bind(&f.binder, &env);
                let saved = std::mem::replace(&mut self.env, env2);
                self.loop_deps = f.binder.names.iter().take(1).cloned().collect();
                for inner in &f.body {
                    let ok = matches!(inner, Stmt::Request(_)) || matches!(inner, Stmt::Assign(a) if matches!(a.e, Expr::Index { .. }));
                    if ok {
                        self.stmt(inner, pos);
                    } else {
                        self.d(ERROR, "型", "for … { } の中に書けるのは、データを引く文と、要求だけです", f.line);
                    }
                }
                self.env = saved;
                self.loop_deps = IndexSet::new();
            }
        }
    }

    fn stmt_assign(&mut self, st: &Assign) {
        self.no_rebind(&st.name, st.line);
        let env = self.env.clone();
        match self.unit_of(&st.e, &env) {
            Some(u) => {
                self.var_unit.insert(st.name.clone(), u);
            }
            None => {
                self.var_unit.shift_remove(&st.name);
            }
        }
        if let Expr::Index { d, e: ie, scopes } = &st.e {
            if !scopes.is_empty() {
                self.scoped_lookup(st, &st.e);
            }
            let t = self.ty_self(&st.e);
            self.env_set(&st.name, &t);
            self.info.lookup_vars.insert(st.name.clone(), d.clone());
            self.bind_var(&st.name);
            self.var_iv.shift_remove(&st.name);
            self.shortage_sets.shift_remove(&st.name);
            if names_in(ie).iter().any(|n| self.loop_deps.contains(n)) {
                self.loop_deps.insert(st.name.clone());
            }
            self.info.exist_reads.insert(d.clone());
            match &st.absent {
                None => self.d(QUESTION, "存在", &format!("{} が存在しないとき、どうしますか（else {{ return … }} が必要です）", show(&st.e)), st.line),
                Some(a) => {
                    self.check_return(a);
                    self.guarded.insert((d.clone(), show(ie)));
                }
            }
            return;
        }
        let null_name = matches!(&st.e, Expr::Name { n } if self.null_input(n, &env));
        let null_attr = matches!(&st.e, Expr::Attr { .. }) && self.null_field(&st.e, &env).is_some();
        if st.absent.is_some() && null_name {
            self.writing = true;
            let t = self.ty_self(&st.e);
            self.writing = false;
            self.check_return(st.absent.as_ref().unwrap());
            if let Expr::Name { n } = &st.e {
                self.present.insert(("#入力".to_string(), n.clone()));
            }
            self.env_set(&st.name, &t);
            self.info.lookup_vars.shift_remove(&st.name);
            self.bindings.shift_remove(&st.name);
            let iv = self.interval_self(&st.e);
            self.var_iv.insert(st.name.clone(), iv);
            self.shortage_sets.shift_remove(&st.name);
        } else if st.absent.is_some() && null_attr {
            self.writing = true;
            let t = self.ty_self(&st.e);
            self.writing = false;
            self.check_return(st.absent.as_ref().unwrap());
            self.env_set(&st.name, &t);
            self.info.lookup_vars.shift_remove(&st.name);
            self.bindings.shift_remove(&st.name);
            let iv = self.interval_self(&st.e);
            self.var_iv.insert(st.name.clone(), iv);
            self.shortage_sets.shift_remove(&st.name);
            if let Expr::Attr { e: be, n } = &st.e {
                if let Expr::Name { n: bn } = &**be {
                    if let Some(b) = self.bindings.get(bn) {
                        self.present.insert((b.clone(), n.clone()));
                    }
                }
            }
        } else {
            if st.absent.is_some() {
                self.d(ERROR, "型", &format!("let … else は、1件を引くとき（D[k]）か、無いかもしれない項目・入力を取り出すときだけ書けます（{}）", show(&st.e)), st.line);
            }
            self.last_exist = None;
            let t = self.ty_self(&st.e);
            self.env_set(&st.name, &t);
            self.var_lin.shift_remove(&st.name);
            if t == "Int" || t == "Money" {
                if let Some(f) = self.lin_self(&st.e) {
                    self.var_lin.insert(st.name.clone(), f);
                }
            }
            self.var_text.shift_remove(&st.name);
            let env = self.env.clone();
            if t == "Str" && self.decl_type(&st.e, &env).is_none() {
                if let Some(sh) = self.text_shape(&st.e, &env) {
                    self.var_text.insert(st.name.clone(), sh);
                }
            }
            self.info.lookup_vars.shift_remove(&st.name);
            self.bindings.shift_remove(&st.name);
            let iv = self.interval_self(&st.e);
            self.var_iv.insert(st.name.clone(), iv);
            self.shortage_sets.shift_remove(&st.name);
            if let Expr::SetB { .. } = &st.e {
                if let Some(x) = self.last_exist.clone() {
                    self.exist_sets.insert(st.name.clone(), x);
                }
                self.note_shortage_set(&st.name, &st.e);
                self.note_currency_set(&st.name, &st.e);
            }
        }
    }

    fn stmt_update(&mut self, st: &Update) {
        let env = self.env.clone();
        let env2 = self.bind(&st.binder, &env);
        let vt = self.ty(&st.value, &env2);
        if let Some(c) = &st.cond {
            self.ty(c, &env2);
        }
        let tgt = &st.target;
        let (ie, tn, td) = match tgt {
            Expr::Attr { e: ie, n } => match &**ie {
                Expr::Index { d, .. } => (&**ie, n, d),
                _ => {
                    self.d(ERROR, "型", "更新の対象は D[k].項目 の形です", st.line);
                    return;
                }
            },
            _ => {
                self.d(ERROR, "型", "更新の対象は D[k].項目 の形です", st.line);
                return;
            }
        };
        let key = format!("{}.{}", td, tn);
        let was_read = self.info.field_reads.contains(&key);
        self.writing = st.op == "=";
        self.ty(tgt, &env2);
        self.writing = false;
        if st.op == "=" && !was_read {
            self.info.field_reads.shift_remove(&key);
        }
        let tf = self.ck.data.get(td).and_then(|d| d.fields.iter().find(|f| &f.name == tn));
        if let Some(tf) = tf {
            if tf.r#type.kind == "名前" {
                self.d(ERROR, "遷移", &format!("update で状態の項目「{}.{}」は変えられません（遷移の確認ができないため。1件ずつ let で引き、元の状態を確かめてから変えてください）", td, tn), st.line);
            }
        }
        if vt == "Null" {
            if let Some(tf) = tf {
                if !(tf.r#type.optional && st.op == "=") {
                    self.d(ERROR, "null", &format!("{}.{} に null は入れられません（必須の項目か、= 以外の更新です）", td, tn), st.line);
                }
            }
        } else if vt == "Rational" {
            self.d(QUESTION, "丸め", "端数が出る値を、丸めずに書いています", st.line);
        } else if tf.map_or(false, |tf| tf.r#type.kind != "名前" && vt != tok_of(&tf.r#type) && vt != "?" && !num_ok(&vt, &st.value, &tok_of(&tf.r#type))) {
            self.d(ERROR, "型", &format!("{}.{} に型の違う値（{}）を入れています", td, tn, vt), st.line);
        } else if tf.map_or(false, |tf| st.op != "=" && !numeric(&tf.r#type.kind)) {
            self.d(ERROR, "型", &format!("{}.{} は数ではないので {} できません", td, tn, st.op), st.line);
        }
        if let Some(tf) = tf {
            if st.op == "=" || st.op == "+=" || st.op == "-=" {
                self.fit_unit(&st.value, &tf.r#type, &key, st.line, None, Some(ie));
            }
        }
        self.note_write(&key);
        self.info.write_ops.entry(key.clone()).or_default().insert(st.op.clone());
        if let Some(tf) = tf {
            if numeric(&tf.r#type.kind) && vt != "Null" {
                self.check_update(st, &env2);
            }
        }
        self.forget(td, tn);
        self.set_null(td, tn, None, false);
    }

    fn stmt_setfield(&mut self, st: &SetField) {
        let data = match self.info.lookup_vars.get(&st.var).cloned() {
            Some(d) => d,
            None => {
                self.d(ERROR, "型", &format!("「{}」は、データを引いた変数ではありません", st.var), st.line);
                return;
            }
        };
        let dt = match self.ck.data.get(&data).copied() {
            Some(d) => d,
            None => return,
        };
        let fld = match dt.fields.iter().find(|f| f.name == st.f) {
            Some(f) => f,
            None => {
                self.d(ERROR, "名前", &format!("データ {} に項目「{}」がありません{}", data, st.f, near(&st.f, dt.fields.iter().map(|f| f.name.clone()))), st.line);
                return;
            }
        };
        if st.op == "add" || st.op == "remove" {
            self.set_change(st, &data, fld);
            return;
        }
        if fld.r#type.kind == "集合" && (st.op == "+=" || st.op == "-=") {
            let word = if st.op == "+=" { "add" } else { "remove" };
            self.d(ERROR, "型", &format!("集合に {} は使えません。{} 要素 {} {}.{} と書きます", st.op, word, if word == "add" { "to" } else { "from" }, st.var, st.f), st.line);
            return;
        }
        let passing = self.pass_null(&st.e, &fld.r#type);
        let saved = std::mem::replace(&mut self.writing, passing);
        let vt = self.ty_self(&st.e);
        self.writing = saved;
        let b = self.bind_of(&st.var);
        let where_ = format!("{}.{}", data, st.f);
        if vt == "Null" {
            if !fld.r#type.optional {
                self.d(ERROR, "null", &format!("{}.{} は必須（required）なので null は入れられません", data, st.f), st.line);
                return;
            }
            if numeric(&fld.r#type.kind) {
                self.check_write(&data, &st.f, (None, None), st.line, WriteOpts { bind: Some(&b), null: true, ..Default::default() });
            }
            self.forget(&data, &st.f);
            self.set_null(&data, &st.f, Some(&b), false);
            self.note_write(&where_);
            self.info.write_ops.entry(where_).or_default().insert(st.op.clone());
            return;
        }
        if vt == "Rational" {
            self.d(QUESTION, "丸め", "端数が出る値を、丸めずに書いています", st.line);
        } else if vt != tok_of(&fld.r#type) && vt != "?" && fld.r#type.kind != "名前" && !num_ok(&vt, &st.e, &tok_of(&fld.r#type)) {
            self.d(ERROR, "型", &format!("{}.{} に型の違う値（{}）を入れています", data, st.f, vt), st.line);
            return;
        }
        if fld.r#type.kind == "集合" {
            self.forget(&data, &format!("{}{}", LEN, st.f));
        }
        let base = Expr::Name { n: st.var.clone() };
        self.fit_unit(&st.e, &fld.r#type, &where_, st.line, None, Some(&base));
        self.check_range_fit(&st.e, &fld.r#type, &where_, st.line);
        self.check_text_fit(&st.e, &fld.r#type, &where_, st.line);
        if fld.r#type.kind == "合成" {
            let path = format!("{}.{}", st.var, st.f);
            self.present.retain(|k| match k.0.strip_prefix("#成分\u{1}") {
                Some(rest) => !rest.starts_with(&path),
                None => true,
            });
        }
        if passing && numeric(&fld.r#type.kind) {
            self.check_write(&data, &st.f, (None, None), st.line, WriteOpts { bind: Some(&b), null: true, ..Default::default() });
        }
        self.set_null(&data, &st.f, Some(&b), !passing);
        if numeric(&fld.r#type.kind) {
            let mut iv = self.interval_self(&st.e);
            if let Some(g) = &st.guard {
                if fld.r#type.kind == "割合" || fld.r#type.optional {
                    self.d(ERROR, "範囲", &format!("{}.{}: {} は、null にならない整数か金額の項目だけに付けられます", data, st.f, g.0), st.line);
                } else {
                    let (lo, hi) = iv_of(&fld.r#type);
                    iv = (
                        Some(match (&iv.0, &lo) {
                            (None, _) => lo.clone().unwrap(),
                            (Some(a), Some(l)) => if a > l { a.clone() } else { l.clone() },
                            (Some(a), None) => a.clone(),
                        }),
                        Some(match (&iv.1, &hi) {
                            (None, _) => hi.clone().unwrap(),
                            (Some(a), Some(h)) => if a < h { a.clone() } else { h.clone() },
                            (Some(a), None) => a.clone(),
                        }),
                    );
                    self.info.guards.push((where_.clone(), st.op.clone(), g.0.clone(), g.1.clone()));
                }
            }
            let fix = if (st.op == "+=" || st.op == "-=") && st.guard.is_none() { Some(self.step_fix(st, fld)) } else { None };
            let form = if st.guard.is_none() { self.lin_self(&st.e) } else { None };
            self.check_write(&data, &st.f, iv.clone(), st.line, WriteOpts { bind: Some(&b), fix, form, ..Default::default() });
            let mut changed: IndexMap<String, Option<Lin>> = IndexMap::new();
            changed.insert(st.f.clone(), self.lin_self(&st.e));
            self.rel_check(&data, &changed, Some(&b), st.line, RelOpts::default());
            self.forget(&data, &st.f);
            if let Some(lo) = &iv.0 {
                self.facts.insert(vec![b.clone(), st.f.clone()], lo.clone());
            }
            if let Some(hi) = &iv.1 {
                self.facts.insert(vec![UP.to_string(), b.clone(), st.f.clone()], hi.clone());
            }
        }
        self.note_write(&where_);
        self.info.write_ops.entry(where_.clone()).or_default().insert(st.op.clone());
        if self.ck.is_lifecycle(&fld.r#type) {
            let name = match &st.e {
                Expr::Name { n } if self.ck.variants.contains_key(n) && !self.env.contains_key(n) => n.clone(),
                _ => {
                    self.d(ERROR, "遷移", &format!("{}.{} は状態名で書いてください（「{}」では、どの遷移か確かめられません）", data, st.f, show(&st.e)), st.line);
                    return;
                }
            };
            self.check_transition(&data, &st.f, &name, st.line, &st.var);
            self.check_state_write(&data, &st.f, &name, &st.var, st.line);
            let bd = self.binding_data.clone();
            self.state_guards.retain(|g| !(bd.get(&g.0).map_or(false, |x| x.as_deref() == Some(data.as_str())) && g.1 == st.f));
            self.state_guards.insert((b, st.f.clone(), name));
        }
    }

    /// x.f += v／x.f -= v が項目の範囲を出うるときの直し方
    pub fn step_fix(&mut self, st: &SetField, fld: &Field) -> String {
        let t = &fld.r#type;
        let (lo, hi) = iv_of(t);
        let (mut vlo, mut vhi) = if let Expr::Bin { r, .. } = &st.e { self.interval_self(r) } else { (None, None) };
        if st.op == "-=" {
            let (a, b) = (vhi.as_ref().map(|x| -x.clone()), vlo.as_ref().map(|x| -x.clone()));
            vlo = a;
            vhi = b;
        }
        let lim = q(1i128 << 62);
        let known = |x: &Option<Q>| x.as_ref().map_or(false, |x| x.is_integer() && x.abs() < lim);
        let plain = (t.kind == "整数" || (t.kind == "金額" && t.unit.as_deref() == Some("JPY"))) && known(&lo) && known(&hi);
        let target = format!("{}.{}", st.var, st.f);
        let mut tests: Vec<Option<String>> = vec![];
        if let Some(h) = &hi {
            if vhi.as_ref().map_or(true, |v| v.is_positive()) {
                let ok = plain && known(&vhi) && vhi.as_ref().unwrap() <= &(h - lo.as_ref().unwrap());
                tests.push(if ok { Some(format!("if {} > {} {{ return … }}", target, q_show(&(h - vhi.as_ref().unwrap())))) } else { None });
            }
        }
        if let Some(l) = &lo {
            if vlo.as_ref().map_or(true, |v| v.is_negative()) {
                let ok = plain && known(&vlo) && &(-vlo.as_ref().unwrap().clone()) <= &(hi.as_ref().unwrap() - l);
                tests.push(if ok { Some(format!("if {} < {} {{ return … }}", target, q_show(&(l - vlo.as_ref().unwrap())))) } else { None });
            }
        }
        let how = if !tests.is_empty() && tests.iter().all(|x| x.is_some()) {
            format!("先に {} のように確かめる", tests.iter().map(|x| format!("`{}`", x.as_ref().unwrap())).collect::<Vec<_>>().join("、"))
        } else {
            format!("項目と{}値の型に ltype で範囲を付けて、先に if で確かめる", if st.op == "+=" { "足す" } else { "引く" })
        };
        if (t.kind == "整数" || t.kind == "金額") && !t.optional {
            return how + "か、人が引き受けるなら文の末尾に saturating \"理由\"（範囲の端で止める）か trust assume \"理由\"（届かないと承知する）を書きます";
        }
        how + "と示せます"
    }

    pub fn inv_row(&mut self, key: (bool, usize), ok: bool, note: &str) {
        let name = self.proc.name.clone();
        self.ck.inv_rows.entry(key).or_default().entry(name).or_default().push((ok, note.to_string()));
    }

    /// 項目へ範囲 iv の値を書くとき、項目の範囲と、その項目にかかる不変条件が守れるか
    pub fn check_write(&mut self, data: &str, fname: &str, iv: Iv, line: i64, o: WriteOpts) {
        let mut iv = iv;
        let fld = match self.ck.data[data].fields.iter().find(|f| f.name == fname) {
            Some(f) => f,
            None => return,
        };
        let (lo, hi) = iv_of(&fld.r#type);
        let env = self.env.clone();
        if let (Some(form), false) = (&o.form, o.null) {
            if let Some(l) = &lo {
                if iv.0.as_ref().map_or(true, |x| x < l) {
                    let t = lin_add(&(IndexMap::new(), q_to_i128(l).unwrap_or(0)), form, -1);
                    if self.prove_le0(&t, &env) {
                        iv = (lo.clone(), iv.1.clone());
                    }
                }
            }
            if let Some(h) = &hi {
                if iv.1.as_ref().map_or(true, |x| x > h) {
                    let t = lin_add(form, &(IndexMap::new(), q_to_i128(h).unwrap_or(0)), -1);
                    if self.prove_le0(&t, &env) {
                        iv = (iv.0.clone(), hi.clone());
                    }
                }
            }
        }
        if !o.null && ((lo.is_some() && iv.0.as_ref().map_or(true, |x| x < lo.as_ref().unwrap())) || (hi.is_some() && iv.1.as_ref().map_or(true, |x| x > hi.as_ref().unwrap()))) {
            let fix = o.fix.clone().unwrap_or_else(|| "上限は ltype に書き、合計は集まりの型に要素数の上限（K → V max N）を書くと示せます".to_string());
            self.d(
                ERROR,
                "範囲",
                &format!("{}.{} に書く値が、項目の範囲 {} に収まると示せません（書く値の範囲 {}。はみ出す例（反例）があります。{}）", data, fname, iv_show(&(lo.clone(), hi.clone())), iv_show(&iv), fix),
                line,
            );
        }
        let forms: Vec<Option<(String, InvForm)>> = self.ck.inv_forms.clone();
        for (i, form) in forms.iter().enumerate() {
            let form = match form {
                Some(f) => &f.1,
                None => continue,
            };
            match form {
                InvForm::Lower(d, f, n) => {
                    if d != data || f != fname {
                        continue;
                    }
                    let ok = ge(&iv.0, *n);
                    self.inv_row((false, i), ok, &o.note.clone().unwrap_or_else(|| format!("書く値は {} 以上", n)));
                    if !ok {
                        let tail = if o.null {
                            "（null は満たしません。必ずあるなら項目に required）".to_string()
                        } else {
                            format!("（反例：書く値が {} になりえます）", match &iv.0 { None => "いくらでも小さく".to_string(), Some(v) => q_show(v) })
                        };
                        self.d(ERROR, "不変条件", &format!("不変条件 {}: {} >= {} を示せません{}", data, fname, n, tail), line);
                    }
                }
                InvForm::Implies(d, sf, s, g, n) => {
                    if d != data || g != fname {
                        continue;
                    }
                    let big = iv.0.as_ref().map_or(false, |x| x > &q(*n));
                    if o.new_state.map_or(false, |ns| ns.get(sf).map(|x| x.as_str()) != Some(s.as_str())) {
                        self.inv_row((false, i), true, &format!("新規作成は{}以外", s));
                    } else if big {
                        self.inv_row((false, i), true, &format!("{} は {} より大きい", g, n));
                    } else if o.grow && o.new_state.is_none() {
                        self.inv_row((false, i), true, "元の値から減らさない");
                    } else if o.bind.map_or(false, |b| self.state_guards.iter().any(|x| x.0 == b && &x.1 == sf && &x.2 != s)) {
                        self.inv_row((false, i), true, &format!("{}でないことを確認している", s));
                    } else {
                        self.inv_row((false, i), false, "");
                        self.d(ERROR, "不変条件", &format!("{}.{} を書くと、{} が {} のとき {} > {} が破れるおそれがあります（書く値の範囲 {}。{}でないことか、{} > {} を確かめてください）", data, g, sf, s, g, n, iv_show(&iv), s, g, n), line);
                    }
                }
            }
        }
    }

    /// update { D[k].f op 値 | … } の書き込み
    pub fn check_update(&mut self, st: &Update, env2: &Rc<Env>) {
        let (ie, fname) = match &st.target {
            Expr::Attr { e, n } => (&**e, n),
            _ => return,
        };
        let (data, ke) = match ie {
            Expr::Index { d, e, .. } => (d, &**e),
            _ => return,
        };
        let b = &st.binder;
        let old = self.field_iv(data, fname);
        let v = self.interval(&st.value, env2);
        let mut note: Option<String> = None;
        let mut grow = false;
        let iv: Iv;
        if st.op == "=" {
            iv = v.clone();
        } else if st.op == "+=" {
            iv = iv_add(&old, &v);
            grow = v.0.as_ref().map_or(false, |x| !x.is_negative());
        } else if b.names.len() == 2 && is_name(ke, &b.names[0]) && is_name(&st.value, &b.names[1]) && self.facts.contains_key(&vec!["#不足".to_string(), data.clone(), fname.clone(), show(&b.src)]) {
            iv = (Some(q(0)), iv_add(&old, &iv_neg(&v)).1);
            note = Some("減算の前に不足を確認している".to_string());
        } else {
            iv = iv_add(&old, &iv_neg(&v));
        }
        self.check_write(data, fname, iv, st.line, WriteOpts { note, grow, ..Default::default() });
        let row = format!("#update\u{2}{}", data);
        let val = self.lin(&st.value, env2);
        let mut cur_m = IndexMap::new();
        cur_m.insert(atom_f(&row, fname), 1i128);
        let cur: Lin = (cur_m, 0);
        let new = match val {
            None => None,
            Some(v) => {
                if st.op == "=" {
                    Some(v)
                } else {
                    Some(lin_add(&cur, &v, if st.op == "+=" { 1 } else { -1 }))
                }
            }
        };
        let mut changed: IndexMap<String, Option<Lin>> = IndexMap::new();
        changed.insert(fname.clone(), new);
        self.rel_check(data, &changed, Some(&row), st.line, RelOpts { env: Some(env2.clone()), ..Default::default() });
    }

    /// x.状態 = 状態名 のとき、「状態 == S implies g > n」が守れるか
    pub fn check_state_write(&mut self, data: &str, sf: &str, dst: &str, var: &str, line: i64) {
        let b = self.bind_of(var);
        self.rel_check(data, &IndexMap::new(), Some(&b), line, RelOpts { force_state: Some((sf.to_string(), dst.to_string())), ..Default::default() });
        let forms: Vec<Option<(String, InvForm)>> = self.ck.inv_forms.clone();
        for (i, form) in forms.iter().enumerate() {
            let (d, fsf, s, g, n) = match form {
                Some((_, InvForm::Implies(d, sf2, s, g, n))) => (d, sf2, s, g, n),
                _ => continue,
            };
            if d != data || fsf != sf {
                continue;
            }
            if dst != s {
                self.inv_row((false, i), true, &format!("{}以外にする", s));
                continue;
            }
            let attr = Expr::Attr { e: Box::new(Expr::Name { n: var.to_string() }), n: g.clone() };
            let mut lo = self.interval_self(&attr).0;
            let fg = self.ck.data[data].fields.iter().find(|f| &f.name == g);
            if fg.map_or(false, |f| f.r#type.optional) && !self.present.contains(&(b.clone(), g.clone())) {
                lo = None;
            }
            let ok = lo.as_ref().map_or(false, |x| x > &q(*n));
            self.inv_row((false, i), ok, &format!("{}にする前に {} > {} を確認している", s, g, n));
            if !ok {
                self.d(ERROR, "不変条件", &format!("{}.{} を {} にする前に、{} > {} の確認がありません（不変条件 {}: {} == {} implies {} > {}）", data, sf, s, g, n, data, sf, s, g, n), line);
            }
        }
    }

    pub fn check_transition_new(&mut self, data: &str, _fname: &str, dst: &str, line: i64) {
        let decl: Vec<&(String, String, String)> = self.proc.transitions.iter().filter(|x| x.0 == data && x.1 == "(新規)" && x.2 == dst).collect();
        if decl.is_empty() {
            self.d(ERROR, "遷移", &format!("{} を新規に「{}」で作る遷移が、処理の宣言（遷移 … (新規) → {}）にありません", data, dst, dst), line);
        } else {
            self.info.transitions.insert(decl[0].clone());
        }
    }

    /// data の 1 件の項目を書くとき、項目どうしを比べる不変条件と、定義した項目の範囲が守れるか
    pub fn rel_check(&mut self, data: &str, changed: &IndexMap<String, Option<Lin>>, bind: Option<&str>, line: i64, o: RelOpts) {
        let rels: Vec<RelInv> = self.ck.rel_invs.clone();
        let env = o.env.clone().unwrap_or_else(|| self.env.clone());
        for (k, r) in rels.iter().enumerate() {
            if r.data != data {
                continue;
            }
            let touched = r.cons.iter().any(|con| con.0.keys().any(|f| changed.contains_key(f)));
            if let Some(fs) = &o.force_state {
                if r.when.as_ref() != Some(fs) {
                    continue;
                }
            } else if !touched {
                continue;
            }
            let key = match r.i {
                Some(i) => (false, i),
                None => (true, k),
            };
            if let (Some((sf, sv)), None) = (&r.when, &o.force_state) {
                if o.creating && o.new_state.as_ref().and_then(|m| m.get(sf)).map(|x| x.as_str()) != Some(sv.as_str()) {
                    self.inv_row(key, true, &format!("新規作成は{}以外", sv));
                    continue;
                }
                if !o.creating && bind.map_or(false, |b| self.state_guards.iter().any(|x| x.0 == b && &x.1 == sf && &x.2 != sv)) {
                    self.inv_row(key, true, &format!("{}でないことを確認している", sv));
                    continue;
                }
            }
            let mut ok = true;
            for (fd, c) in &r.cons {
                let mut t: Lin = (IndexMap::new(), *c);
                for (f, coef) in fd {
                    let v: Option<Lin> = if let Some(v) = changed.get(f) {
                        v.clone()
                    } else if o.creating {
                        None
                    } else {
                        let mut m = IndexMap::new();
                        m.insert(atom_f(bind.unwrap_or(""), f), 1i128);
                        Some((m, 0))
                    };
                    match v {
                        None => {
                            ok = false;
                            break;
                        }
                        Some(v) => t = lin_add(&t, &v, *coef),
                    }
                }
                ok = ok && self.prove_le0(&t, &env);
                if !ok {
                    break;
                }
            }
            self.inv_row(key, ok, "書く前に確かめている");
            if !ok {
                let names: Vec<&String> = changed.keys().collect();
                let what = if names.is_empty() { "状態".to_string() } else { names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("・") };
                self.d(ERROR, "不変条件", &format!("不変条件 {} を示せません（{} の{}を書く前に、守れることを if … {{ return … }} で確かめてください）", r.text, data, what), line);
            }
        }
    }

    /// add 要素 to 変数.項目 ／ remove 要素 from 変数.項目
    pub fn set_change(&mut self, st: &SetField, data: &str, fld: &Field) {
        let where_ = format!("{}.{}", data, st.f);
        let word = if st.op == "add" { "to" } else { "from" };
        if fld.r#type.kind != "集合" {
            self.d(ERROR, "型", &format!("{} … {} の相手は集合の項目です（{} は集合ではありません）", st.op, word, where_), st.line);
            return;
        }
        if fld.r#type.optional {
            self.d(ERROR, "null", &format!("{} は null になりうるので、{} できません（集合の項目に required を付けてください）", where_, st.op), st.line);
            return;
        }
        let t = self.ty_self(&st.e);
        if t != "ID" && t != "?" {
            self.d(ERROR, "型", &format!("集合 {} に{}のは id です（{}）", where_, if st.op == "add" { "足せる" } else { "から外せる" }, show(&st.e)), st.line);
        }
        let b = self.bind_of(&st.var);
        let key = format!("{}{}", LEN, st.f);
        self.info.field_reads.insert(where_.clone());
        let fkey = vec![UP.to_string(), b.clone(), key.clone()];
        if st.op == "add" {
            match fld.r#type.maxn {
                None => self.d(ERROR, "範囲", &format!("{} に要素を足すには、型に件数の上限が要ります（set of id max N）", where_), st.line),
                Some(n) => {
                    let hi = self.facts.get(&fkey).cloned();
                    if hi.as_ref().map_or(true, |h| h >= &q(n)) {
                        self.d(ERROR, "範囲", &format!("{} に足す前に、まだ上限（{} 件）に届いていないと示せません（先に if len({}.{}) >= {} {{ return … }} で確かめてください）", where_, n, st.var, st.f, n), st.line);
                    } else {
                        self.forget(data, &key);
                        self.facts.insert(fkey, hi.unwrap() + q(1));
                    }
                }
            }
        } else {
            let hi = self.facts.get(&fkey).cloned();
            self.forget(data, &key);
            if let Some(h) = hi {
                self.facts.insert(fkey, h);
            }
        }
        self.note_write(&where_);
        self.info.write_ops.entry(where_).or_default().insert(st.op.clone());
    }

    pub fn check_transition(&mut self, data: &str, fname: &str, dst: &str, line: i64, var: &str) {
        let sname = self.ck.data[data].fields.iter().find(|f| f.name == fname).and_then(|f| f.r#type.target.clone()).unwrap_or_default();
        let decl: Vec<(String, String)> = self.proc.transitions.iter().filter(|(d, _, b)| d == data && b == dst).map(|(_, a, b)| (a.clone(), b.clone())).collect();
        if decl.is_empty() {
            self.d(ERROR, "遷移", &format!("{} を「{}」にする遷移が、処理の宣言（遷移）にありません", data, dst), line);
            return;
        }
        let bv = self.bind_of(var);
        for (a, b) in &decl {
            self.info.transitions.insert((data.to_string(), a.clone(), b.clone()));
            if a != "(新規)" && !self.state_guards.contains(&(bv.clone(), fname.to_string(), a.clone())) {
                self.d(ERROR, "遷移", &format!("{} が「{}」であることの確認（if … != {} {{ return … }}、または if … == {} {{ … }} の中）が、遷移の前にありません", data, a, a, a), line);
            }
        }
        if let Some(s) = self.ck.states.get(&sname).copied() {
            for (x, y, why) in &s.rejects {
                if y == dst && decl.iter().any(|(a, _)| a == x) {
                    self.d(ERROR, "遷移", &format!("拒否された遷移 {} → {}（{}）を行っています", x, y, why), line);
                }
            }
        }
    }

    /// 入力をそのまま項目へ入れるとき、入力の範囲が項目の範囲に収まるか
    pub fn check_range_fit(&mut self, e: &Expr, dst: &Type, where_: &str, line: i64) {
        let n = match e {
            Expr::Name { n } => n,
            _ => return,
        };
        let src = match self.proc.inputs.iter().find(|(m, _)| m == n) {
            Some((_, t)) => t,
            None => return,
        };
        if !(src.kind == "写像" && dst.kind == "写像") {
            return;
        }
        let (sv, dv) = match (&src.value, &dst.value) {
            (Some(a), Some(b)) => (a, b),
            _ => return,
        };
        if !numeric(&sv.kind) || sv.kind != dv.kind {
            return;
        }
        let s = sv.range.clone().unwrap_or_default();
        let d = dv.range.clone().unwrap_or_default();
        let lo_ng = d.lo.is_some() && (s.lo.is_none() || s.lo.unwrap() < d.lo.unwrap());
        let hi_ng = d.hi.is_some() && (s.hi.is_none() || s.hi.unwrap() > d.hi.unwrap());
        if lo_ng || hi_ng {
            let a = show_range(&sv.range);
            let a = a.trim();
            self.d(
                ERROR,
                "範囲",
                &format!("{} に、範囲の広い入力「{}」を入れています（入力{} ／ 項目{}）。収まるか確認が要ります", where_, n, if a.is_empty() { "制限なし" } else { a }, show_range(&dv.range).trim()),
                line,
            );
        }
    }

    /// 長さか使える文字が決まっている文字列の項目へ書くとき、書く値の長さと文字が収まると示せるか
    pub fn check_text_fit(&mut self, e: &Expr, dst: &Type, where_: &str, line: i64) {
        if dst.kind != "文字列" || (dst.range.is_none() && dst.chars.is_none() && dst.read.is_none()) || matches!(e, Expr::Null) {
            return;
        }
        let env = self.env.clone();
        let src: Option<Type> = match e {
            Expr::Call { n, .. } if self.ck.parts.contains_key(n) => Some(self.ck.parts[n].ret.clone()),
            _ => self.decl_type(e, &env),
        };
        let subset = |dc: &Vec<(u32, u32)>, sc: &Vec<(u32, u32)>| sc.iter().all(|(a, b)| dc.iter().any(|(c, e2)| c <= a && b <= e2));
        if src.is_none() {
            if let Some((lo, hi, chars)) = self.text_shape(e, &env) {
                let d = dst.range.clone().unwrap_or_default();
                if (d.lo.is_some() && lo < d.lo.unwrap()) || (d.hi.is_some() && hi > d.hi.unwrap()) {
                    self.d(
                        ERROR,
                        "範囲",
                        &format!("{} に書く文字列「{}」の長さ（{}..{}）が、項目の長さの範囲{}に収まると示せません（入れ先の上限を広げるか、つなぐ元の型の上限を狭めます。切り詰める書き方はありません）", where_, show(e), lo, hi, show_range(&dst.range)),
                        line,
                    );
                }
                if let Some(dc) = &dst.chars {
                    if chars.as_ref().map_or(true, |c| !subset(dc, c)) {
                        let have = match &chars {
                            None => "の文字は制限なし".to_string(),
                            Some(c) => format!(" [{}]", show_chars(c)),
                        };
                        self.d(ERROR, "文字種", &format!("{} に書く文字列「{}」の文字が、項目の使える文字に収まると示せません（書く値{} ／ 項目 [{}]）", where_, show(e), have, show_chars(dc)), line);
                    }
                }
                if dst.read.is_some() {
                    self.d(ERROR, "検査", &format!("{} は変換を通った値だけを書けるので、つないだ文字列「{}」は書けません", where_, show(e)), line);
                }
                return;
            }
        }
        let src = match src {
            Some(s) if s.kind == "文字列" => s,
            Some(_) => return,
            None => {
                self.d(ERROR, "範囲", &format!("{} は長さか使える文字が決まっているので、宣言のある入力・項目・slot か部品の戻り値を、そのまま書いてください（「{}」では収まると示せません）", where_, show(e)), line);
                return;
            }
        };
        let s = src.range.clone().unwrap_or_default();
        let d = dst.range.clone().unwrap_or_default();
        if (d.lo.is_some() && d.lo.unwrap() > 0 && (s.lo.is_none() || s.lo.unwrap() < d.lo.unwrap())) || (d.hi.is_some() && (s.hi.is_none() || s.hi.unwrap() > d.hi.unwrap())) {
            let a = show_range(&src.range);
            let b = show_range(&dst.range);
            let (a, b) = (a.trim(), b.trim());
            self.d(
                ERROR,
                "範囲",
                &format!("{} に書く文字列の長さが、項目の長さの範囲に収まると示せません（書く値{} ／ 項目{}）", where_, if a.is_empty() { "長さの制限なし" } else { a }, if b.is_empty() { "長さの制限なし" } else { b }),
                line,
            );
        }
        if let Some(dc) = &dst.chars {
            if src.chars.as_ref().map_or(true, |c| !subset(dc, c)) {
                let have = match &src.chars {
                    None => "の文字は制限なし".to_string(),
                    Some(c) => format!(" [{}]", show_chars(c)),
                };
                self.d(ERROR, "文字種", &format!("{} に書く文字列の文字が、項目の使える文字に収まると示せません（書く値{} ／ 項目 [{}]）", where_, have, show_chars(dc)), line);
            }
        }
        if dst.read.is_some() && src.read != dst.read {
            let have = match &src.read {
                None => "は変換を通っていない".to_string(),
                Some(r) => format!("の変換は {}", r),
            };
            self.d(
                ERROR,
                "検査",
                &format!(
                    "{} に書く文字列が、変換 {} を通った値と示せません（書く値{} ／ 項目は {}）。型に read {} が結ばれた入力・項目・部品の戻り値を、そのまま書いてください",
                    where_,
                    dst.read.as_ref().unwrap(),
                    have,
                    dst.read.as_ref().unwrap(),
                    dst.read.as_ref().unwrap()
                ),
                line,
            );
        }
    }

    /// delete 変数：let で引いた1件を消す
    pub fn delete(&mut self, st: &Delete) {
        let data = match self.info.lookup_vars.get(&st.var).cloned().filter(|d| self.ck.data.contains_key(d)) {
            Some(d) => d,
            None => {
                self.d(ERROR, "削除", &format!("delete は、let で引いた1件の変数に書きます（「{}」は引いた1件ではありません）", st.var), st.line);
                return;
            }
        };
        let dt = self.ck.data[&data];
        if dt.keep {
            self.d(ERROR, "削除", &format!("{} は keep（削除させない）と宣言されているので、消せません", data), st.line);
            return;
        }
        let has_state = dt.fields.iter().any(|f| self.ck.is_lifecycle(&f.r#type));
        if has_state {
            if dt.removable.is_empty() {
                self.d(QUESTION, "削除", &format!("{} は状態を持つので、消してよい状態を決めてください（entity の中に removable in 状態 と書く。消させないなら keep）", data), st.line);
            } else {
                let b = self.bind_of(&st.var);
                if !self.state_guards.iter().any(|g| g.0 == b && dt.removable.contains(&g.2)) {
                    self.d(QUESTION, "削除", &format!("{} を消す前に、状態が {} のどれかだと確かめていません（if で確かめるか、消してよい状態を見直してください）", data, dt.removable.join(", ")), st.line);
                }
            }
        }
        self.info.writes.insert(data.clone());
        self.info.deletes.insert(data);
    }

    pub fn create(&mut self, st: &Create) {
        let dt = match self.ck.data.get(&st.data).copied() {
            Some(d) => d,
            None => {
                let msg = format!("データ「{}」が未定義です{}", st.data, near(&st.data, self.ck.data.keys()));
                self.d(ERROR, "名前", &msg, st.line);
                return;
            }
        };
        let key = self.ck.key_field(&st.data);
        let given: IndexMap<&String, &Expr> = st.fields.iter().map(|(n, e)| (n, e)).collect();
        for f in &dt.fields {
            if key.map_or(false, |k| std::ptr::eq(f, k)) {
                continue;
            }
            if !given.contains_key(&f.name) && !f.r#type.optional {
                self.d(ERROR, "項目", &format!("{} の新規作成に必須の項目「{}」がありません", st.data, f.name), st.line);
            }
        }
        for (n, e) in &st.fields {
            let f = match dt.fields.iter().find(|x| &x.name == n) {
                Some(f) => f,
                None => {
                    let msg = format!("データ {} に項目「{}」がありません{}", st.data, n, near(n, dt.fields.iter().map(|f| f.name.clone())));
                    self.d(ERROR, "名前", &msg, st.line);
                    continue;
                }
            };
            let pn = self.pass_null(e, &f.r#type);
            let saved = std::mem::replace(&mut self.writing, pn);
            let vt = self.ty_self(e);
            self.writing = saved;
            if vt == "Null" {
                if !f.r#type.optional {
                    self.d(ERROR, "null", &format!("{}.{} は必須（required）なので null は入れられません", st.data, n), st.line);
                }
            } else if vt == "Rational" {
                self.d(QUESTION, "丸め", "端数が出る値を、丸めずに書いています", st.line);
            } else if !num_ok(&vt, e, &tok_of(&f.r#type)) && vt != "?" {
                self.d(ERROR, "型", &format!("{}.{} に型の違う値（{}）を入れています", st.data, n, vt), st.line);
            }
            let w = format!("{}.{}", st.data, n);
            self.fit_unit(e, &f.r#type, &w, st.line, Some(&given), None);
            self.check_range_fit(e, &f.r#type, &w, st.line);
            self.check_text_fit(e, &f.r#type, &w, st.line);
            if self.ck.is_lifecycle(&f.r#type) {
                match e {
                    Expr::Name { n: en } if self.ck.variants.contains_key(en) && !self.env.contains_key(en) => self.check_transition_new(&st.data, n, en, st.line),
                    _ => self.d(ERROR, "遷移", &format!("{} の新規作成の「{}」は状態名で書いてください（「{}」では、どの遷移か確かめられません）", st.data, n, show(e)), st.line),
                }
            }
        }
        self.info.writes.insert(st.data.clone());
        self.info.new_ids.push(st.data.clone());
        let mut states: IndexMap<String, String> = IndexMap::new();
        for f in &dt.fields {
            if self.ck.is_lifecycle(&f.r#type) {
                if let Some(Expr::Name { n }) = given.get(&f.name).copied() {
                    states.insert(f.name.clone(), n.clone());
                }
            }
        }
        let mut lows: IndexMap<String, Option<Q>> = IndexMap::new();
        let null_e = Expr::Null;
        for f in &dt.fields {
            let e: &Expr = given.get(&f.name).copied().unwrap_or(&null_e);
            if numeric(&f.r#type.kind) && (given.contains_key(&f.name) || f.r#type.optional) {
                if matches!(e, Expr::Null) {
                    self.check_write(&st.data, &f.name, (None, None), st.line, WriteOpts { new_state: Some(&states), null: true, ..Default::default() });
                    continue;
                }
                if self.pass_null(e, &f.r#type) {
                    self.check_write(&st.data, &f.name, (None, None), st.line, WriteOpts { new_state: Some(&states), null: true, ..Default::default() });
                }
                let iv = self.interval_self(e);
                self.check_write(&st.data, &f.name, iv.clone(), st.line, WriteOpts { new_state: Some(&states), ..Default::default() });
                lows.insert(f.name.clone(), iv.0);
            }
        }
        let mut changed: IndexMap<String, Option<Lin>> = IndexMap::new();
        for (n, e) in &st.fields {
            if dt.fields.iter().any(|f| &f.name == n && (f.r#type.kind == "整数" || f.r#type.kind == "金額")) {
                let l = self.lin_self(e);
                changed.insert(n.clone(), l);
            }
        }
        self.rel_check(&st.data, &changed, None, st.line, RelOpts { new_state: Some(states.clone()), creating: true, ..Default::default() });
        self.created.insert(st.name.clone());
        self.env_set(&st.name, &format!("Rec:{}", st.data));
        self.info.lookup_vars.insert(st.name.clone(), st.data.clone());
        let b = self.bind_var(&st.name);
        for (n, v) in &states {
            self.state_guards.insert((b.clone(), n.clone(), v.clone()));
        }
        for (n, e) in &st.fields {
            if !matches!(e, Expr::Null) {
                self.present.insert((b.clone(), n.clone()));
            }
        }
        for (n, lo) in lows {
            if let Some(lo) = lo {
                self.facts.insert(vec![b.clone(), n], lo);
            }
        }
    }

    /// { k | (k, v) in 元, D[k].通貨 != 通貨 } の形なら、「通貨が違う k の集まり」として覚える
    pub fn note_currency_set(&mut self, name: &str, e: &Expr) {
        self.cur_sets.shift_remove(name);
        let (elem, c, b) = match e {
            Expr::SetB { elem, binder, cond: Some(c) } => (elem, c, binder),
            _ => return,
        };
        let b0 = match b.names.first() {
            Some(x) => x,
            None => return,
        };
        if let Expr::Bin { op, l, r, .. } = &**c {
            if op != "!=" {
                return;
            }
            if let (Expr::Attr { e: le, n: ln }, Expr::Name { n: rn }) = (&**l, &**r) {
                if let Expr::Index { d, e: ke, .. } = &**le {
                    if is_name(ke, b0) && is_name(elem, b0) {
                        let env = self.env.clone();
                        if let Some(unit) = self.cur_name_unit(rn, &env) {
                            let f = self.ck.data.get(d).and_then(|dt| dt.fields.iter().find(|x| &x.name == ln));
                            if f.map_or(false, |f| f.r#type.kind == "名前" && f.r#type.target.as_deref() == Some("Currency")) {
                                self.cur_sets.insert(name.to_string(), (d.clone(), ln.clone(), show(&b.src), unit));
                            }
                        }
                    }
                }
            }
        }
    }

    /// { k | (k, v) in 元, D[k].f < v } の形なら、「減らす量が足りない k の集まり」として覚える
    pub fn note_shortage_set(&mut self, name: &str, e: &Expr) {
        let (elem, c, b) = match e {
            Expr::SetB { elem, binder, cond: Some(c) } => (elem, c, binder),
            _ => return,
        };
        if b.names.len() != 2 {
            return;
        }
        if let Expr::Bin { op, l, r, .. } = &**c {
            if op != "<" {
                return;
            }
            if let Expr::Attr { e: le, n: ln } = &**l {
                if let Expr::Index { d, e: ke, .. } = &**le {
                    if is_name(ke, &b.names[0]) && is_name(r, &b.names[1]) && is_name(elem, &b.names[0]) {
                        self.shortage_sets.insert(name.to_string(), (d.clone(), ln.clone(), show(&b.src)));
                    }
                }
            }
        }
    }

    /// data.fname が書き換わった。分かっていた下限などを消す
    pub fn forget(&mut self, data: &str, fname: &str) {
        let hit = |s: &Self, a: &str| -> bool {
            let p: Vec<&str> = a.split('\u{1}').collect();
            p[0] == "f" && p.len() == 3 && p[2] == fname && s.bind_data(p[1]).as_deref() == Some(data)
        };
        let mut rel = std::mem::take(&mut self.rel);
        rel.retain(|r| !r.0.iter().any(|(a, _)| hit(self, a)));
        self.rel = rel;
        let mut vl = std::mem::take(&mut self.var_lin);
        vl.retain(|_, f| !f.0.keys().any(|a| hit(self, a)));
        self.var_lin = vl;
        let bd = &self.binding_data;
        let is_data = |b: &str| bd.get(b).map_or(false, |x| x.as_deref() == Some(data));
        self.facts.retain(|k, _| {
            let drop = (k[0] == "#不足" && k.len() >= 3 && k[1] == data && k[2] == fname)
                || (k[0] == UP && k.len() >= 3 && is_data(&k[1]) && k[2] == fname)
                || (k[0] != "#不足" && k[0] != UP && k.len() >= 2 && is_data(&k[0]) && k[1] == fname);
            !drop
        });
        let cur_field = self.ck.currency_field(data, fname);
        self.state_guards.retain(|g| !(is_data(&g.0) && g.1 == fname && cur_field));
        self.cur_ok.retain(|k, _| !(k.0 == data && k.1 == fname));
        self.cur_sets.retain(|_, v| !(v.0 == data && v.1 == fname));
        self.shortage_sets.retain(|_, v| !(v.0 == data && v.1 == fname));
    }

    pub fn request(&mut self, st: &Request) {
        let eff = match self.ck.effects.get(&st.effect).copied() {
            Some(e) => e,
            None => {
                self.d(ERROR, "名前", &format!("外部作用「{}」が宣言されていません", st.effect), st.line);
                return;
            }
        };
        if st.args.len() != eff.params.len() {
            self.d(ERROR, "型", &format!("外部作用 {} の引数は {} 個です", st.effect, eff.params.len()), st.line);
        }
        let sib: IndexMap<&String, &Expr> = eff.params.iter().map(|(pn, _)| pn).zip(st.args.iter()).collect();
        for (a, (_, t)) in st.args.iter().zip(eff.params.iter()) {
            let at = self.ty_self(a);
            if !num_ok(&at, a, &tok_of(t)) && at != "?" {
                self.d(ERROR, "型", &format!("外部作用 {} の引数の型が違います（{}）", st.effect, at), st.line);
            }
            self.fit_unit(a, t, &format!("外部作用 {} の引数", st.effect), st.line, Some(&sib), None);
        }
        match &st.idem {
            None => self.d(ERROR, "冪等", &format!("外部作用 {} に冪等キーがありません（idempotency_key = … が必須です）", st.effect), st.line),
            Some(idem) => {
                let parts: Vec<&Expr> = if let Expr::Tuple { items } = idem { items.iter().collect() } else { vec![idem] };
                for x in &parts {
                    let kt = self.ty_self(x);
                    if kt != "ID" && kt != "Str" && kt != "?" {
                        self.d(ERROR, "冪等", &format!("外部作用 {} の冪等キーは ID か文字列（の組）にしてください（{}: {}）", st.effect, show(x), kt), st.line);
                    }
                }
                if !self.loop_deps.is_empty() {
                    let inner = parts.iter().any(|x| names_in(x).iter().any(|n| self.loop_deps.contains(n)));
                    let outer = parts.iter().any(|x| {
                        let ns = names_in(x);
                        !ns.is_empty() && !ns.iter().any(|n| self.loop_deps.contains(n))
                    });
                    if !inner {
                        self.d(ERROR, "冪等", &format!("外部作用 {} の冪等キー「{}」が、for で回す値によりません（どの要求も同じキーになり、2件目からが重複として捨てられます）", st.effect, show(idem)), st.line);
                    } else if !outer {
                        self.d(
                            ERROR,
                            "冪等",
                            &format!("外部作用 {} の冪等キー「{}」が、for で回す値だけです。別の処理で同じ値を回すとキーがぶつかるので、for の外で決まる値との組にしてください（idempotency_key = (注文ID, k) など）", st.effect, show(idem)),
                            st.line,
                        );
                    }
                }
            }
        }
        let e = (st.effect.clone(), st.idem.as_ref().map_or(String::new(), show));
        if !self.info.effects.contains(&e) {
            self.info.effects.push(e);
        }
    }
}

#[derive(Default)]
pub struct RelOpts {
    pub new_state: Option<IndexMap<String, String>>,
    pub creating: bool,
    pub env: Option<Rc<Env>>,
    pub force_state: Option<(String, String)>,
}
