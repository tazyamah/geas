// SPDX-License-Identifier: GPL-3.0-or-later
//! Checker 側の続き：定義の検査（部品・型・移行）と、検査全体の流れ
use super::procs::*;
use super::*;
use crate::names::{label_gaps, name_problems};
use regex::Regex;
use std::rc::Rc;

#[derive(Clone)]
pub struct Step {
    pub ver: i128,
    pub op: String,
    pub dn: String,
    pub fname: String,
    pub arg: Option<MigVal>,
    pub line: i64,
}

fn to_of(a: &Option<MigVal>) -> String {
    match a {
        Some(MigVal::To(s)) => s.clone(),
        _ => String::new(),
    }
}

fn env_of(items: &[(String, String)]) -> Rc<Env> {
    let mut e = Env::new();
    for (n, t) in items {
        e.insert(n.clone(), Ev::T(t.clone()));
    }
    Rc::new(e)
}

fn plain_type(x: &Type) -> String {
    let mut y = x.clone();
    y.optional = false;
    y.read = None;
    y.trust = None;
    show_type(&y).unwrap_or_default()
}

impl<'a> Checker<'a> {
    pub fn sort_diags(&mut self) {
        let ord = |l: &str| if l == ERROR { 0 } else if l == QUESTION { 1 } else { 2 };
        self.diags.sort_by_key(|x| ord(&x.level));
    }

    pub fn run(&mut self) {
        let p = self.p;
        if self.strict_names {
            let re = Regex::new(r"名前「([^」]+)」").unwrap();
            for (where_, msg) in name_problems(p, &p.ltype_names) {
                let nm = re.captures(&msg).map(|c| c[1].to_string()).unwrap_or_default();
                let line = self.name_line(&where_, &nm);
                self.diags.push(Diag::plain(ERROR, "名前", &where_, &msg, line));
            }
            for (where_, n) in label_gaps(p, &p.ltype_names) {
                let shown = p.origin.get(&n).map(|x| x.1.clone()).unwrap_or(n.clone());
                let line = self.name_line(&where_, &n);
                self.diags.push(Diag::plain(WARN, "表示名", &where_, &format!("「{}」に表示名（直前の /// ）がありません。仕様書には名前のまま出ます", shown), line));
            }
        }
        self.check_defs();
        self.check_derived();
        for proc in &p.processes {
            let info = {
                let mut pc = ProcChecker::new(self, proc);
                pc.run();
                std::mem::replace(&mut pc.info, ProcInfo::new(""))
            };
            self.check_module_deed(proc, &info);
            self.infos.insert(proc.name.clone(), info);
        }
        self.check_flows();
        self.check_queries();
        self.check_triggers();
        self.check_effect_waits();
        self.check_events();
        self.check_screens();
        self.check_resources();
        self.check_deed_callers();
        self.check_deletes();
        self.check_isolations();
        self.check_needs();
        self.check_resident();
        self.check_data_folders();
        self.check_states();
        self.check_invariants();
        self.check_migrations();
        self.sort_diags();
    }

    fn part_checker(&mut self, name: &str, where_: &str) -> ProcChecker<'_, 'a> {
        let proc: &'a Process = Box::leak(Box::new(Process { name: name.to_string(), ..Default::default() }));
        let mut pc = ProcChecker::new(self, proc);
        pc.where_ = where_.to_string();
        pc
    }

    pub fn check_part(&mut self, pt: &'a Part) {
        let where_ = format!("部品 {}", pt.name);
        if pt.name == "round" || conversion(&pt.name).is_some() {
            self.d(ERROR, "名前", &where_, "組み込みの部品と同じ名前は使えません", 0);
        }
        let names: Vec<String> = pt.params.iter().map(|(n, _)| n.clone()).collect();
        self.check_unique(&names, &where_, "引数");
        if pt.params.iter().any(|(_, t)| t.kind == "生") {
            self.check_raw_convert(pt, &where_);
            return;
        }
        if !pt.requires.is_empty() {
            self.d(ERROR, "型", &where_, "require を書けるのは、raw から変換する convert だけです", 0);
        }
        if names.iter().any(|n| n == "result") {
            self.d(ERROR, "名前", &where_, "引数の名前に result は使えません（戻り値の名前です）", 0);
        }
        let mut all: Vec<(String, &Type)> = pt.params.iter().map(|(n, t)| (n.clone(), t)).collect();
        all.push(("result".to_string(), &pt.ret));
        for (n, t) in &all {
            self.check_type(t, &format!("{} / {}", where_, n), pt.body.is_some() && n != "result", false);
            let ok_kind = ["整数", "金額", "割合", "ID", "文字列", "日時", "真偽"].contains(&t.kind.as_str());
            if !ok_kind && !(pt.body.is_some() && t.kind == "合成" && n != "result") {
                self.d(ERROR, "型", &where_, &format!("部品の引数と戻り値に使えるのは数・割合・ID・文字列・日時・真偽です（{}）", n), 0);
            }
        }
        if let Some(body) = &pt.body {
            let mut pc = self.part_checker(&pt.name, &where_);
            for (n, t) in &pt.params {
                if ["整数", "金額", "割合"].contains(&t.kind.as_str()) {
                    pc.var_iv.insert(n.clone(), iv_of(t));
                }
                if t.fills.as_ref().map_or(false, |f| !f.is_empty()) {
                    pc.ck_fills(&where_, n, t);
                }
            }
            let mut seen_default = false;
            let empty = Rc::new(Env::new());
            for (n, t) in &pt.params {
                let dv = match pt.defaults.get(n) {
                    None => {
                        if seen_default {
                            pc.d(ERROR, "型", &format!("引数「{}」にもデフォルト値が要ります（デフォルト値のある引数より後ろは、すべて省けるようにします）", n), 0);
                        }
                        continue;
                    }
                    Some(d) => d,
                };
                seen_default = true;
                let dt = pc.ty(dv, &empty);
                if dt != tok_of(t) && dt != "?" && !(matches!(dv, Expr::Num { .. }) && (tok_of(t) == "Int" || tok_of(t) == "Money")) {
                    pc.d(ERROR, "型", &format!("引数「{}」のデフォルト値の型（{}）が引数の型（{}）と違います", n, dt, tok_of(t)), 0);
                } else if (t.kind == "整数" || t.kind == "金額") && t.range.is_some() {
                    let iv = pc.interval(dv, &empty);
                    let r = t.range.as_ref().unwrap();
                    if outside(&iv, r) {
                        pc.d(ERROR, "範囲", &format!("引数「{}」のデフォルト値が、引数の範囲{}に収まりません", n, show_range(&t.range)), 0);
                    }
                }
            }
            let penv = env_of(&pt.params.iter().map(|(n, t)| (n.clone(), tok_of(t))).collect::<Vec<_>>());
            let b = pc.ty(body, &penv);
            if b != tok_of(&pt.ret) && b != "?" && !(matches!(body, Expr::Num { .. }) && (tok_of(&pt.ret) == "Int" || tok_of(&pt.ret) == "Money")) {
                pc.d(ERROR, "型", &format!("変換の式の型（{}）が戻り値の型（{}）と違います", b, tok_of(&pt.ret)), 0);
            }
            if ["整数", "金額", "割合"].contains(&pt.ret.kind.as_str()) && pt.ret.range.is_some() {
                let iv = pc.interval(body, &penv);
                if outside(&iv, pt.ret.range.as_ref().unwrap()) {
                    pc.d(
                        ERROR,
                        "範囲",
                        &format!("変換の結果が戻り値の範囲{}に収まると示せません（式の範囲 {}）。引数の型か式を見直してください", show_range(&pt.ret.range), iv_show(&iv)),
                        0,
                    );
                }
            } else if pt.ret.kind == "文字列" && (pt.ret.range.is_some() || pt.ret.chars.is_some()) {
                pc.d(ERROR, "範囲", "長さや文字種が決まっている文字列を返す変換は、まだ静的に示せません", 0);
            }
            let types = std::mem::take(&mut pc.info.types);
            drop(pc);
            self.part_types.extend(types);
            return;
        }
        let (krate, version, expr, _level, _why) = match &pt.lib {
            Some(l) => l.clone(),
            None => return,
        };
        if !Regex::new(r"^[A-Za-z][A-Za-z0-9_-]*$").unwrap().is_match(&krate) {
            self.d(ERROR, "名前", &where_, &format!("クレートの名前「{}」が読めません（半角英数・_・- だけ。標準ライブラリだけなら \"std\"）", krate), 0);
        } else if krate != "std" && !Regex::new(r"^[0-9]+(\.[0-9]+){0,2}([-+][0-9A-Za-z.-]+)?$").unwrap().is_match(&version) {
            self.d(ERROR, "名前", &where_, &format!("クレート {} の版「{}」が読めません（例：\"0.10\"。版は固定します）", krate, version), 0);
        }
        let words: Vec<String> = Regex::new(r"[A-Za-z_]+").unwrap().find_iter(&expr).map(|m| m.as_str().to_string()).collect();
        if words.iter().any(|w| w == "unsafe") {
            self.d(ERROR, "保証", &where_, "call の式に unsafe は書けません（生成物は unsafe を許していません）", 0);
        }
        let mut loops: Vec<String> = ["loop", "while", "for", "fn"].iter().filter(|k| words.iter().any(|w| w == *k)).map(|s| s.to_string()).collect();
        loops.sort();
        let clos = Regex::new(r"(^|[(,=])\s*(move\s+)?\|").unwrap().is_match(&expr);
        if !loops.is_empty() || clos {
            let what = if loops.is_empty() { "クロージャ".to_string() } else { loops.join("・") };
            self.d(
                ERROR,
                "保証",
                &where_,
                &format!("call の式に{}は書けません（部品の包みの側では、引数を渡してライブラリを呼ぶだけにします。繰り返しは、geas の側で上限つきの集まりを回す for や sum で書きます）", what),
                0,
            );
        }
        self.check_examples(pt, &where_);
        let g = match &pt.guarantee {
            None => {
                let r = &pt.ret;
                if ((r.kind == "整数" || r.kind == "金額") && r.range.is_none()) || (r.kind == "文字列" && r.range.is_none() && r.chars.is_none() && r.read.is_none()) {
                    self.d(ERROR, "保証", &where_, "戻り値の範囲（型の範囲・文字種）か guarantee のどちらかで、戻り値を確かめられるようにしてください", 0);
                }
                return;
            }
            Some(g) => g,
        };
        let mut items: Vec<(String, String)> = pt.params.iter().map(|(n, t)| (n.clone(), tok_of(t))).collect();
        items.push(("result".to_string(), tok_of(&pt.ret)));
        let env = env_of(&items);
        let mut pc = self.part_checker(&pt.name, &where_);
        let gt = pc.ty(g, &env);
        if gt != "Bool" && gt != "?" {
            pc.d(ERROR, "型", &format!("保証は真偽の式にしてください（{}）", gt), 0);
        }
        if !names_in(g).contains("result") {
            pc.d(ERROR, "保証", "保証が戻り値（result）を使っていません。何も確かめない保証は書けません", 0);
        }
        let types = std::mem::take(&mut pc.info.types);
        drop(pc);
        self.part_types.extend(types);
    }

    pub fn check_type_read(&mut self, where_: &str, t: &Type) {
        let rd = t.read.clone().unwrap_or_default();
        let pt = self.parts.get(&rd).copied();
        let pt = match pt {
            Some(p) if p.params.iter().any(|(_, x)| x.kind == "生") => p,
            _ => {
                self.d(ERROR, "名前", where_, &format!("read の「{}」は、raw から変換する convert ではありません", rd), 0);
                return;
            }
        };
        if plain_type(&pt.ret) != plain_type(t) {
            self.d(ERROR, "型", where_, &format!("型（{}）と、変換 {} の戻り値の型（{}）が違います", plain_type(t), rd, plain_type(&pt.ret)), 0);
        }
    }

    pub fn check_raw_convert(&mut self, pt: &'a Part, where_: &str) {
        if pt.params.len() != 1 || pt.params[0].1.kind != "生" {
            self.d(ERROR, "型", where_, "raw から変換する convert の引数は、raw 1つだけです", 0);
            return;
        }
        let body = match &pt.body {
            None => {
                self.d(ERROR, "型", where_, "raw から変換する convert は { require … return 式 } の形で書きます", 0);
                return;
            }
            Some(b) => b,
        };
        if pt.ret.kind != "文字列" {
            self.d(ERROR, "型", where_, &format!("raw から変換する convert が返せるのは、いまは文字列（text）だけです（{}）", pt.ret.kind), 0);
        }
        self.check_type(&pt.ret, &format!("{} / 戻り値", where_), false, false);
        let n = pt.params[0].0.clone();
        let env = env_of(&[(n, "Raw".to_string())]);
        let mut pc = self.part_checker(&pt.name, where_);
        pc.raw_convert = true;
        for (cond, _) in &pt.requires {
            let c = pc.ty(cond, &env);
            if c != "Bool" && c != "?" {
                pc.d(ERROR, "型", &format!("require は真偽の式にしてください（{}）", c), 0);
            }
        }
        let b = pc.ty(body, &env);
        if b != tok_of(&pt.ret) && b != "?" {
            pc.d(ERROR, "型", &format!("return の式の型（{}）が戻り値の型（{}）と違います", b, tok_of(&pt.ret)), 0);
        }
        let types = std::mem::take(&mut pc.info.types);
        drop(pc);
        self.part_types.extend(types);
    }

    pub fn check_defs(&mut self) {
        let p = self.p;
        let mut tops: Vec<String> = vec![];
        tops.extend(p.data.iter().map(|x| x.name.clone()));
        tops.extend(p.states.iter().map(|x| x.name.clone()));
        tops.extend(p.results.iter().map(|x| x.name.clone()));
        tops.extend(p.processes.iter().map(|x| x.name.clone()));
        tops.extend(p.queries.iter().map(|x| x.name.clone()));
        tops.extend(p.triggers.iter().map(|x| x.name.clone()));
        tops.extend(p.flows.iter().map(|x| x.name.clone()));
        tops.extend(p.events.iter().map(|x| x.name.clone()));
        tops.extend(p.screens.iter().map(|x| x.name.clone()));
        tops.extend(p.parts.iter().map(|x| x.name.clone()));
        tops.extend(p.composites.iter().map(|c| c.target.clone().unwrap_or_default()));
        self.check_unique(&tops, "全体", "定義の名前");
        let efs: Vec<String> = p.effects.iter().map(|e| e.name.clone()).collect();
        self.check_unique(&efs, "全体", "外部作用");
        let mut sn: Vec<String> = vec![];
        for s in &p.states {
            for v in std::iter::once(&s.initial).chain(s.middle.iter()).chain(s.final_.iter()) {
                if !v.is_empty() {
                    sn.push(v.clone());
                }
            }
        }
        self.check_unique(&sn, "全体", "状態名");
        for r in &p.results {
            let vs: Vec<String> = r.variants.iter().map(|(v, _)| v.clone()).collect();
            self.check_unique(&vs, &format!("結果 {}", r.name), "種類");
            for (v, ps) in &r.variants {
                let ns: Vec<String> = ps.iter().map(|(n, _)| n.clone()).collect();
                self.check_unique(&ns, &format!("結果 {} / {}", r.name, v), "中身の名前");
                for (n, t) in ps {
                    self.check_type(t, &format!("結果 {} / {} / {}", r.name, v, n), true, false);
                }
            }
        }
        for pt in &p.parts {
            self.check_part(pt);
        }
        for e in &p.effects {
            let ns: Vec<String> = e.params.iter().map(|(n, _)| n.clone()).collect();
            self.check_unique(&ns, &format!("外部作用 {}", e.name), "引数");
            for (n, t) in &e.params {
                self.check_type(t, &format!("外部作用 {} / {}", e.name, n), true, true);
            }
        }
        let xs: Vec<(bool, &str, &Vec<(String, Type)>, &Vec<(String, Type)>)> = p
            .processes
            .iter()
            .map(|x| (true, x.name.as_str(), &x.inputs, &x.refs))
            .chain(p.queries.iter().map(|x| (false, x.name.as_str(), &x.inputs, &x.refs)))
            .collect();
        for (is_proc, name, inputs, refs) in &xs {
            let kind = if *is_proc { "処理" } else { "問い合わせ" };
            let ns: Vec<String> = inputs.iter().chain(refs.iter()).map(|(n, _)| n.clone()).collect();
            self.check_unique(&ns, &format!("{} {}", kind, name), "入力・参照の名前");
            for (n, t) in inputs.iter().chain(refs.iter()) {
                self.check_type(t, &format!("{} {} / {}", kind, name, n), *is_proc, false);
            }
        }
        for tg in &p.triggers {
            for (n, t) in &tg.params {
                self.check_type(t, &format!("トリガ {} / {}", tg.name, n), true, false);
            }
            self.check_units(&format!("トリガ {}", tg.name), &tg.params);
        }
        for r in &p.results {
            for (v, ps) in &r.variants {
                self.check_units(&format!("結果 {} / {}", r.name, v), ps);
            }
        }
        for e in &p.effects {
            self.check_units(&format!("外部作用 {}", e.name), &e.params);
        }
        for (is_proc, name, inputs, refs) in &xs {
            let all: Vec<(String, Type)> = inputs.iter().chain(refs.iter()).cloned().collect();
            self.check_units(&format!("{} {}", if *is_proc { "処理" } else { "問い合わせ" }, name), &all);
        }
        for dt in &p.data {
            let fs: Vec<(String, Type)> = dt.fields.iter().map(|f| (f.name.clone(), f.r#type.clone())).collect();
            self.check_units(&format!("データ {}", dt.name), &fs);
        }
        for rp in &p.replies {
            self.check_units(&format!("応答 {}", rp.name), &rp.params);
        }
        let rn: Vec<String> = p.replies.iter().map(|x| x.name.clone()).collect();
        self.check_unique(&rn, "全体", "応答の宣言");
        for rp in &p.replies {
            let ns: Vec<String> = rp.params.iter().map(|(n, _)| n.clone()).collect();
            self.check_unique(&ns, &format!("応答 {}", rp.name), "中身の名前");
            for (n, t) in &rp.params {
                self.check_type(t, &format!("応答 {} / {}", rp.name, n), true, false);
            }
        }
        for dt in &p.data {
            if self.key_field(&dt.name).is_none() {
                self.d(ERROR, "データ", &format!("データ {}", dt.name), "「ID 一意」の項目（キー）がありません", 0);
            }
            let fns: Vec<String> = dt.fields.iter().map(|f| f.name.clone()).collect();
            self.check_unique(&fns, &format!("データ {}", dt.name), "項目");
            for f in &dt.fields {
                self.check_type(&f.r#type, &format!("{}.{}", dt.name, f.name), true, false);
            }
        }
        for s in &p.states {
            if s.plain {
                continue;
            }
            if s.initial.is_empty() || !(!s.final_.is_empty() || s.endless) {
                self.d(ERROR, "状態", &format!("状態 {}", s.name), "初期と終了の状態は必須です（行き来を繰り返す状態は「終了 なし」と書きます）", 0);
            }
            let known: Vec<&String> = std::iter::once(&s.initial).chain(s.middle.iter()).chain(s.final_.iter()).collect();
            for (a, b, _) in &s.rejects {
                for x in [a, b] {
                    if !known.contains(&x) {
                        self.d(ERROR, "名前", &format!("状態 {}", s.name), &format!("拒否に書かれた「{}」は状態にありません", x), 0);
                    }
                }
            }
        }
    }

    // ---- 保存の形の移行 ----
    pub fn schema_version(&self) -> i128 {
        self.p.migrations.iter().map(|m| m.version).max().unwrap_or(1)
    }

    pub fn migration_steps(&self) -> Vec<Step> {
        let mut out = vec![];
        for m in &self.p.migrations {
            for (op, dn, fname, arg, line) in &m.steps {
                out.push(Step { ver: m.version, op: op.clone(), dn: dn.clone(), fname: fname.clone().unwrap_or_default(), arg: arg.clone(), line: *line });
            }
        }
        out
    }

    pub fn migration_final(steps: &[Step], i: usize) -> Option<String> {
        let s = &steps[i];
        let mut name = if s.op == "rename" { to_of(&s.arg) } else { s.fname.clone() };
        for s2 in &steps[i + 1..] {
            if s2.dn != s.dn || s2.fname != name {
                continue;
            }
            if s2.op == "drop" {
                return None;
            }
            if s2.op == "rename" {
                name = to_of(&s2.arg);
            }
        }
        Some(name)
    }

    /// 項目の手順の実体を「最後の名前」に直したもの。実体の手順や、実体ごと消えるものは dn を空にする（None 相当）
    pub fn migration_norm(&self) -> Vec<(Step, Option<String>)> {
        let steps = self.migration_steps();
        steps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let d = if s.op == "rename_entity" || s.op == "drop_entity" { None } else { Self::entity_final(&steps, i as isize, &s.dn) };
                (s.clone(), d)
            })
            .collect()
    }

    /// i 番目の手順の時点での実体の名前 dn の、最後の名前。i = -1 なら全部の手順を見る
    pub fn entity_final(steps: &[Step], i: isize, dn: &str) -> Option<String> {
        let mut dn = dn.to_string();
        for s2 in &steps[(i + 1) as usize..] {
            if s2.dn != dn {
                continue;
            }
            if s2.op == "drop_entity" {
                return None;
            }
            if s2.op == "rename_entity" {
                dn = to_of(&s2.arg);
            }
        }
        Some(dn)
    }

    pub fn check_migrations(&mut self) {
        let p = self.p;
        for (i, m) in p.migrations.iter().enumerate() {
            let where_ = format!("migration {}", m.version);
            if m.version != i as i128 + 2 {
                self.d(ERROR, "移行", &where_, &format!("版は 2 から 1 ずつ増やします（ここは {} のはずです）。前の migration は消さずに残します", i + 2), m.line);
            }
            if m.steps.is_empty() {
                self.d(ERROR, "移行", &where_, "中身がありません（add 実体.項目 = 値、rename 実体.項目 to 新しい名前、drop 実体.項目）", m.line);
            }
        }
        let raw = self.migration_steps();
        let norm = self.migration_norm();
        let mut live: IndexSet<(String, String)> = IndexSet::new();
        for (i, s) in raw.iter().enumerate() {
            let where_ = format!("migration {}", s.ver);
            let (op, dn, fname, line) = (s.op.as_str(), s.dn.clone(), s.fname.clone(), s.line);
            if op == "rename_entity" || op == "drop_entity" {
                let again = raw[i + 1..].iter().any(|s2| s2.op == "rename_entity" && to_of(&s2.arg) == dn);
                if self.data.contains_key(&dn) && !again {
                    let tail = if op == "rename_entity" { format!("名前を変えたなら、実体の定義も「{}」にしてください", to_of(&s.arg)) } else { "消すなら、実体の定義も消してください".to_string() };
                    self.d(ERROR, "移行", &where_, &format!("実体「{}」は、今も定義にあります。{}", dn, tail), line);
                }
                let mut nl: IndexSet<(String, String)> = live.iter().filter(|x| x.0 != dn).cloned().collect();
                if op == "rename_entity" {
                    for (d0, f) in &live {
                        if *d0 == dn {
                            nl.insert((to_of(&s.arg), f.clone()));
                        }
                    }
                }
                live = nl;
                if op == "rename_entity" {
                    if to_of(&s.arg) == dn {
                        self.d(ERROR, "移行", &where_, &format!("実体「{}」の新しい名前が、前と同じです", dn), line);
                        continue;
                    }
                    if let Some(fin) = Self::entity_final(&raw, i as isize, &to_of(&s.arg)) {
                        if !self.data.contains_key(&fin) {
                            let msg = format!("実体「{}」が、今の定義にありません{}", fin, near(&fin, self.data.keys()));
                            self.d(ERROR, "移行", &where_, &msg, line);
                        }
                    }
                }
                continue;
            }
            let dn = match &norm[i].1 {
                None => continue,
                Some(d) => d.clone(),
            };
            let d = match self.data.get(&dn).copied() {
                None => {
                    let msg = format!("実体「{}」がありません{}", dn, near(&dn, self.data.keys()));
                    self.d(ERROR, "移行", &where_, &msg, line);
                    continue;
                }
                Some(d) => d,
            };
            let now: Vec<String> = d.fields.iter().map(|x| x.name.clone()).collect();
            let nsteps: Vec<&Step> = norm.iter().map(|x| &x.0).collect();
            let back = |n: &str| -> bool {
                (i + 1..norm.len()).any(|k| {
                    let (s2, d2) = &norm[k];
                    d2.as_deref() == Some(dn.as_str()) && ((s2.op == "add" && s2.fname == n) || (s2.op == "rename" && to_of(&s2.arg) == n))
                })
            };
            let _ = &nsteps;
            if op == "rename" || op == "drop" {
                live.shift_remove(&(dn.clone(), fname.clone()));
                if now.contains(&fname) && !back(&fname) {
                    let tail = if op == "rename" { format!("名前を変えたなら、実体の項目も「{}」にしてください", to_of(&s.arg)) } else { "消すなら、実体の項目からも消してください".to_string() };
                    self.d(ERROR, "移行", &where_, &format!("「{}.{}」は、今も実体の項目にあります。{}", dn, fname, tail), line);
                }
                if op == "drop" {
                    continue;
                }
                if to_of(&s.arg) == fname {
                    self.d(ERROR, "移行", &where_, &format!("「{}.{}」の新しい名前が、前と同じです", dn, fname), line);
                    continue;
                }
            }
            let made = if op == "rename" { to_of(&s.arg) } else { fname.clone() };
            if live.contains(&(dn.clone(), made.clone())) {
                let extra = if op == "add" { "（add が 2 つあります）" } else { "" };
                self.d(ERROR, "移行", &where_, &format!("「{}.{}」は、前の手順でもう作っています{}", dn, made, extra), line);
            }
            live.insert((dn.clone(), made.clone()));
            // 最後の名前は、実体の名前をそろえた手順で追う
            let normed: Vec<Step> = norm.iter().map(|(s2, d2)| Step { dn: d2.clone().unwrap_or_else(|| s2.dn.clone()), ..s2.clone() }).collect();
            let fin = match Self::migration_final(&normed, i) {
                None => continue,
                Some(f) => f,
            };
            let f = match d.fields.iter().find(|x| x.name == fin) {
                None => {
                    let msg = format!("「{}.{}」は、今の実体の項目にありません{}", dn, fin, near(&fin, now.iter()));
                    self.d(ERROR, "移行", &where_, &msg, line);
                    continue;
                }
                Some(f) => f,
            };
            if op == "add" {
                self.check_migration_value(&where_, &dn, &fname, f, &s.arg, line);
            }
        }
    }

    pub fn check_migration_value(&mut self, where_: &str, dn: &str, fname: &str, f: &Field, value: &Option<MigVal>, line: i64) {
        let t = &f.r#type;
        let key = self.key_field(dn);
        let w = format!("「{}.{}」", dn, fname);
        let in_range = |v: i128| t.range.as_ref().map_or(true, |r| r.lo.map_or(true, |l| l <= v) && r.hi.map_or(true, |h| v <= h));
        let val = value.clone().unwrap_or(MigVal::Null);
        if key.map_or(false, |k| std::ptr::eq(k, f)) {
            self.d(ERROR, "移行", where_, &format!("{}は行のキーなので、あとから足せません", w), line);
        } else if matches!(val, MigVal::Null) {
            if !t.optional {
                self.d(ERROR, "移行", where_, &format!("{}は必ずある項目（required）なので、null にできません。前からある行へ入れる値を書いてください", w), line);
            }
        } else if ["整数", "金額", "日時"].contains(&t.kind.as_str()) {
            match &val {
                MigVal::Int(v) => {
                    if t.range.is_some() && !in_range(*v) {
                        self.d(ERROR, "移行", where_, &format!("{}の値 {} が、型の範囲（{}）の外です", w, v, show_type(t).unwrap_or_default()), line);
                    }
                }
                _ => self.d(ERROR, "移行", where_, &format!("{}は{}なので、値は数です", w, t.kind), line),
            }
        } else if t.kind == "文字列" {
            match &val {
                MigVal::Str(s) => {
                    let n = s.chars().count() as i128;
                    if t.range.is_some() && !in_range(n) {
                        self.d(ERROR, "移行", where_, &format!("{}の値の長さ {} が、型の範囲（{}）の外です", w, n, show_type(t).unwrap_or_default()), line);
                    }
                }
                _ => self.d(ERROR, "移行", where_, &format!("{}は文字列なので、値は \"…\" で書きます", w), line),
            }
        } else if t.kind == "真偽" {
            if !matches!(val, MigVal::Bool(_)) {
                self.d(ERROR, "移行", where_, &format!("{}は真偽なので、値は true か false です", w), line);
            }
        } else if t.kind == "名前" {
            let names: Vec<String> = match t.target.as_ref().and_then(|x| self.states.get(x)) {
                Some(st) => std::iter::once(&st.initial).chain(st.middle.iter()).chain(st.final_.iter()).cloned().collect(),
                None => vec![],
            };
            let target = t.target.clone().unwrap_or_default();
            match &val {
                MigVal::Name(n) if names.contains(n) => {}
                MigVal::Name(n) => {
                    let msg = format!("{}の値は、状態 {} の名前です{}", w, target, near(n, names.iter()));
                    self.d(ERROR, "移行", where_, &msg, line);
                }
                _ => self.d(ERROR, "移行", where_, &format!("{}の値は、状態 {} の名前です", w, target), line),
            }
        } else if t.kind == "割合" {
            if !matches!(val, MigVal::Ratio(_) | MigVal::Int(_)) {
                self.d(ERROR, "移行", where_, &format!("{}は割合なので、値は 分子/分母 です", w), line);
            }
        } else {
            self.d(ERROR, "移行", where_, &format!("{}（{}）には、まだ add で値を書けません（無いかもしれない項目にして = null なら書けます）", w, show_type(t).unwrap_or_default()), line);
        }
    }

    pub fn check_against(&mut self, base: &Checker) {
        let norm = self.migration_norm();
        let steps: Vec<&(Step, Option<String>)> = norm.iter().filter(|s| s.0.ver > base.schema_version()).collect();
        let mut made: IndexSet<(String, String)> = IndexSet::new();
        let mut gone: IndexSet<(String, String)> = IndexSet::new();
        for (s, d) in &steps {
            let dn = d.clone().unwrap_or_default();
            if s.op == "add" || s.op == "rename" {
                made.insert((dn.clone(), if s.op == "rename" { to_of(&s.arg) } else { s.fname.clone() }));
            }
            if s.op == "rename" || s.op == "drop" {
                gone.insert((dn, s.fname.clone()));
            }
        }
        let sv = self.schema_version();
        let nxt = if sv > base.schema_version() { sv } else { base.schema_version() + 1 };
        let newer: Vec<Step> = self.migration_steps().into_iter().filter(|s| s.ver > base.schema_version()).collect();
        let mut now_name: IndexMap<String, Option<String>> = IndexMap::new();
        for dn in base.data.keys() {
            now_name.insert(dn.clone(), Self::entity_final(&newer, -1, dn));
        }
        for dn in base.data.keys() {
            if now_name[dn].as_deref() == Some(dn.as_str()) && !self.data.contains_key(dn) {
                self.d(
                    ERROR,
                    "移行",
                    &format!("データ {}", dn),
                    &format!("前の版にあった実体「{}」がありませんが、migration に書いてありません。前の版の DB に、この実体の行が残ります（migration {} {{ drop entity {} }}。名前を変えたのなら rename entity {} to 新しい名前）", dn, nxt, dn, dn),
                    0,
                );
            }
        }
        let mut before: IndexMap<String, String> = IndexMap::new();
        for (old, new) in &now_name {
            if let Some(n) = new {
                before.insert(n.clone(), old.clone());
            }
        }
        let names: Vec<String> = self.data.keys().cloned().collect();
        for dn in names {
            let d = self.data[&dn];
            let old = match before.get(&dn).and_then(|b| base.data.get(b)) {
                None => continue,
                Some(o) => *o,
            };
            let was: Vec<String> = old.fields.iter().map(|f| f.name.clone()).collect();
            let now: Vec<String> = d.fields.iter().map(|f| f.name.clone()).collect();
            for fname in now.iter().filter(|n| !was.contains(n) && !made.contains(&(dn.clone(), (*n).clone()))) {
                self.d(
                    ERROR,
                    "移行",
                    &format!("データ {}", dn),
                    &format!("前の版に無い項目「{}」が増えましたが、migration に書いてありません。前の版の DB が起動で止まります（migration {} {{ add {}.{} = 値 }}。名前を変えたのなら rename {}.前の名前 to {}）", fname, nxt, dn, fname, dn, fname),
                    0,
                );
            }
            for fname in was.iter().filter(|n| !now.contains(n) && !gone.contains(&(dn.clone(), (*n).clone()))) {
                self.d(
                    ERROR,
                    "移行",
                    &format!("データ {}", dn),
                    &format!("前の版にあった項目「{}」がありませんが、migration に書いてありません。前の版の DB の行に、この項目の値が残ります（migration {} {{ drop {}.{} }}。名前を変えたのなら rename {}.{} to 新しい名前）", fname, nxt, dn, fname, dn, fname),
                    0,
                );
            }
        }
        if sv < base.schema_version() {
            self.d(ERROR, "移行", "migration", &format!("前の版の保存の形は版 {} です。migration は消さずに残します", base.schema_version()), 0);
        }
        self.sort_diags();
    }
}

impl<'c, 'a> ProcChecker<'c, 'a> {
    /// fill：未指定にできる成分に、補う値を示す
    pub fn ck_fills(&mut self, where_: &str, pname: &str, t: &Type) {
        let ct = if t.kind == "合成" { t.target.as_ref().and_then(|x| self.ck.comps.get(x)).copied() } else { None };
        let ct = match ct {
            None => {
                self.ck.d(ERROR, "型", where_, &format!("fill は合成型の引数にだけ書けます（{}）", pname), 0);
                return;
            }
            Some(c) => c,
        };
        for (cn, v) in t.fills.clone().unwrap_or_default() {
            let pt = ct.parts.as_ref().and_then(|ps| ps.iter().find(|(n, _)| *n == cn)).map(|(_, x)| x);
            match pt {
                Some(pt) if pt.optional => {
                    if let Some(r) = &pt.range {
                        if r.lo.map_or(false, |l| v < l) || r.hi.map_or(false, |h| v > h) {
                            self.ck.d(ERROR, "範囲", where_, &format!("fill の「{}」の値 {} が、成分の範囲{}に収まりません", cn, v, show_range(&pt.range)), 0);
                        }
                    }
                    self.present.insert((comp_key(pname), cn.clone()));
                }
                _ => self.ck.d(ERROR, "名前", where_, &format!("fill の「{}」は、{} の未指定にできる成分ではありません", cn, t.target.clone().unwrap_or_default()), 0),
            }
        }
    }
}

pub fn conversion(n: &str) -> Option<(&'static str, &'static str)> {
    match n {
        "money" => Some(("Int", "Money")),
        "int" => Some(("Money", "Int")),
        "time" => Some(("Int", "Time")),
        _ => None,
    }
}

/// 検査の入口
pub fn check(prog: &mut Program, strict_names: bool) -> Checker<'_> {
    prepare(prog);
    let mut ck = Checker::new(prog, strict_names);
    ck.run();
    ck
}

/// 検査の前に構文木へ入れる変更（Python 版の Checker.__init__ が、構文木を直に書き換えていた分）。
/// すべての項目は原則「無いかもしれない」。required と書いたもの・キー・状態の項目だけが必須
pub fn prepare(prog: &mut Program) {
    for d in prog.data.iter_mut() {
        for f in d.fields.iter_mut() {
            f.r#type.optional = !(f.required || (f.r#type.kind == "ID" && f.unique) || f.r#type.kind == "名前");
        }
    }
}
