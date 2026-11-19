// SPDX-License-Identifier: GPL-3.0-or-later
//! フロー・並列・外部作用の待ち・範囲・問い合わせの検査（Python 版 check.py の Checker の続き）
use super::*;

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

impl<'a> Checker<'a> {
    pub fn check_flows(&mut self) {
        let p = self.p;
        let mut in_flow: IndexMap<String, (String, IndexMap<String, Vec<String>>)> = IndexMap::new();
        for fl in &p.flows {
            if fl.entries.is_empty() {
                self.d(ERROR, "行き先", &format!("フロー {}", fl.name), "流れの中身がありません。`処理: 結果 → 行き先` の行を 1 つ以上書くか、この流れを消してください", 0);
            }
            let mut twice: IndexSet<String> = IndexSet::new();
            for (proc, routes) in &fl.entries {
                let where_ = format!("フロー {} / {}", fl.name, proc);
                if twice.contains(proc) {
                    self.d(ERROR, "行き先", &where_, &format!("処理「{}」の行が、同じ流れに 2 つあります。行き先は 1 行にまとめてください", proc), 0);
                }
                twice.insert(proc.clone());
                if !self.procs.contains_key(proc) {
                    let msg = format!("処理「{}」が未定義です{}", proc, near(proc, self.procs.keys()));
                    self.d(ERROR, "行き先", &where_, &msg, 0);
                    continue;
                }
                let names: Vec<&String> = routes.iter().map(|(v, _)| v).collect();
                let rm = route_map(routes);
                for (v, ds) in &rm {
                    if ds.len() > 1 && ds.iter().any(|d| d == "reply" || d.starts_with("wait ")) {
                        self.d(ERROR, "行き先", &where_, &format!("結果「{}」の行き先が複数あります。応答（reply）と待つ（wait）は、par に入れられません", v), 0);
                    }
                    let mut dups: IndexSet<&String> = IndexSet::new();
                    for d in ds {
                        if ds.iter().filter(|x| *x == d).count() > 1 {
                            dups.insert(d);
                        }
                    }
                    for d in dups {
                        self.d(ERROR, "行き先", &where_, &format!("結果「{}」の行き先「{}」が2回書かれています", v, d), 0);
                    }
                }
                if let Some((fname, prev)) = in_flow.get(proc) {
                    if prev != &rm {
                        let msg = format!("処理 {} は、フロー {} にも違う行き先で書かれています", proc, fname);
                        self.d(ERROR, "行き先", &where_, &msg, 0);
                    }
                }
                in_flow.entry(proc.clone()).or_insert((fl.name.clone(), rm.clone()));
                let rdef = self.results.get(&self.procs[proc].result).copied();
                if let Some(rdef) = rdef {
                    for (v, _) in &rdef.variants {
                        if !names.contains(&v) {
                            self.d(ERROR, "行き先", &where_, &format!("結果「{}」の行き先がありません", v), 0);
                        }
                    }
                    for v in &names {
                        if !rdef.variants.iter().any(|(x, _)| x == *v) {
                            let msg = format!("「{}」は結果 {} にありません{}", v, rdef.name, near_or_all(v, rdef.variants.iter().map(|(x, _)| x.clone())));
                            self.d(ERROR, "名前", &where_, &msg, 0);
                        }
                    }
                }
                for (v, ds) in &rm {
                    if ds.len() > 1 {
                        let branches: Vec<(String, ProcInfo)> = ds.iter().filter(|d| self.infos.contains_key(*d)).map(|d| (d.clone(), self.infos[d].clone())).collect();
                        self.check_par_branches(&where_, v, &branches);
                    }
                }
                for (v, dest) in routes {
                    let base = if dest.starts_with("wait ") { dest.splitn(2, ' ').nth(1).unwrap().to_string() } else { dest.clone() };
                    if dest.starts_with("wait ") {
                        self.check_wait(&where_, proc, &base);
                    } else if base != "reply" && !self.procs.contains_key(&base) {
                        self.d(ERROR, "行き先", &where_, &format!("結果「{}」の行き先「{}」が未定義の処理です", v, base), 0);
                    } else if base != "reply" && rdef.is_some() {
                        let nxt = self.procs[&base];
                        self.check_pass(&where_, rdef.unwrap(), v, nxt);
                    }
                }
            }
        }
        let names: Vec<String> = self.procs.keys().cloned().collect();
        for name in names {
            if !in_flow.contains_key(&name) {
                self.d(WARN, "行き先", &format!("処理 {}", name), "どのフローにも入っていません", 0);
            }
        }
    }

    /// par の枝どうしで、書く項目が重ならないこと
    pub fn check_par_branches(&mut self, where_: &str, v: &str, branches: &[(String, ProcInfo)]) {
        fn data_of(x: &str) -> &str {
            x.split('.').next().unwrap_or("")
        }
        fn touched(info: &ProcInfo) -> IndexSet<String> {
            info.field_reads.iter().chain(info.writes.iter()).chain(info.exist_reads.iter()).map(|x| data_of(x).to_string()).collect()
        }
        for (i, (a, ia)) in branches.iter().enumerate() {
            for (b, ib) in &branches[i + 1..] {
                let mut ws: Vec<&String> = ia.writes.iter().collect();
                ws.sort();
                for w in ws {
                    if w.contains('.') {
                        if ib.writes.contains(w) {
                            let mut ops: IndexSet<&String> = IndexSet::new();
                            if let Some(s) = ia.write_ops.get(w) {
                                ops.extend(s.iter());
                            }
                            if let Some(s) = ib.write_ops.get(w) {
                                ops.extend(s.iter());
                            }
                            if !ops.iter().all(|o| *o == "+=" || *o == "-=") {
                                self.d(ERROR, "並列", where_, &format!("結果「{}」の par の枝「{}」と「{}」が、同じ項目「{}」に書いています（加算・減算の更新だけなら重なってよい）", v, a, b, w), 0);
                            }
                        } else if ib.field_reads.contains(w) || ib.new_ids.iter().any(|x| x == data_of(w)) {
                            self.d(ERROR, "並列", where_, &format!("結果「{}」の par の枝「{}」が書く「{}」を、枝「{}」が読んでいます（読む前か後かで結果が変わります）", v, a, w, b), 0);
                        }
                    } else if touched(ib).contains(data_of(w)) {
                        self.d(ERROR, "並列", where_, &format!("結果「{}」の par の枝「{}」が新規作成する「{}」を、枝「{}」も触っています（採番と読み書きの順が結果に効きます）", v, a, w, b), 0);
                    }
                }
                let mut ws: Vec<&String> = ib.writes.iter().collect();
                ws.sort();
                for w in ws {
                    if w.contains('.') && ia.field_reads.contains(w) && !ia.writes.contains(w) {
                        self.d(ERROR, "並列", where_, &format!("結果「{}」の par の枝「{}」が書く「{}」を、枝「{}」が読んでいます（読む前か後かで結果が変わります）", v, b, w, a), 0);
                    } else if !w.contains('.') && touched(ia).contains(data_of(w)) {
                        self.d(ERROR, "並列", where_, &format!("結果「{}」の par の枝「{}」が新規作成する「{}」を、枝「{}」も触っています（採番と読み書きの順が結果に効きます）", v, b, w, a), 0);
                    }
                }
                for ta in &ia.transitions {
                    for tb in &ib.transitions {
                        if ta.0 == tb.0 && ta.1 != "(新規)" && tb.1 != "(新規)" {
                            self.d(ERROR, "並列", where_, &format!("結果「{}」の par の枝「{}」と「{}」が、どちらも {} の状態を動かします", v, a, b, ta.0), 0);
                        }
                    }
                }
            }
        }
    }

    /// 外部作用を要求する処理は、結果を待つ経路を持つか、待たない理由を宣言する
    pub fn check_effect_waits(&mut self) {
        let p = self.p;
        let mut waits: IndexMap<String, IndexSet<String>> = IndexMap::new();
        for fl in &p.flows {
            for (proc, routes) in &fl.entries {
                for (_, d) in routes {
                    if d.starts_with("wait ") {
                        waits.entry(proc.clone()).or_default().insert(d.splitn(2, ' ').nth(1).unwrap().to_string());
                    }
                }
            }
        }
        let triggers: IndexMap<&String, &Trigger> = p.triggers.iter().map(|t| (&t.name, t)).collect();
        let mut groups: Vec<Vec<String>> = vec![];
        for fl in &p.flows {
            for (_, routes) in &fl.entries {
                for ds in route_map(routes).values() {
                    if ds.len() > 1 {
                        groups.push(ds.clone());
                    }
                }
            }
        }
        let used: IndexSet<String> = self.infos.values().flat_map(|info| info.effects.iter().map(|(e, _)| e.clone())).collect();
        for e in &p.effects {
            if !used.contains(&e.name) {
                self.d(WARN, "外部作用", &format!("外部作用 {}", e.name), "どの処理からも要求されていません", 0);
            }
            if e.joins.is_some() && e.no_wait.is_some() {
                self.d(ERROR, "合流", &format!("外部作用 {}", e.name), "返事の合流先（joins）を宣言しながら no_wait（待たない）にしています。どちらかにしてください", 0);
            }
            if let Some(j) = &e.joins {
                match triggers.get(j) {
                    None => {
                        let msg = format!("合流先のトリガ「{}」が未定義です{}", j, near(j, self.triggers.keys()));
                        self.d(ERROR, "合流", &format!("外部作用 {}", e.name), &msg, 0);
                    }
                    Some(tg) => {
                        if !tg.causes.iter().any(|(k, _, _)| k == "reply" || k == "all") {
                            self.d(ERROR, "合流", &format!("外部作用 {}", e.name), &format!("合流先のトリガ {} に、応答を受ける行がありません", j), 0);
                        }
                    }
                }
            }
            if !e.failures.is_empty() && e.no_wait.is_some() {
                self.d(ERROR, "失敗", &format!("外部作用 {}", e.name), "失敗の理由を宣言しながら no_wait（待たない）にしています。待たないと、理由ごとの応答を受ける先がありません（どちらかにしてください）", 0);
            }
        }
        let infos: Vec<(String, ProcInfo)> = self.infos.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (proc, info) in infos {
            let mut effs: Vec<String> = info.effects.iter().map(|(e, _)| e.clone()).collect::<IndexSet<_>>().into_iter().collect();
            effs.sort();
            for eff in effs {
                let decl = match self.effects.get(&eff) {
                    Some(d) if d.no_wait.is_none() => *d,
                    _ => continue,
                };
                if let Some(j) = &decl.joins {
                    self.check_join(&proc, decl, triggers.get(j).copied(), &waits, &groups);
                    continue;
                }
                if !waits.contains_key(&proc) {
                    self.d(
                        ERROR,
                        "失敗",
                        &format!("処理 {}", proc),
                        &format!("外部作用 {} の失敗の扱いが決まっていません（ネクサスで「wait」へ進めるか、外部作用の宣言に no_wait \"人が決めた理由\" を書いてください）", eff),
                        0,
                    );
                    continue;
                }
                let mut tnames: Vec<&String> = waits[&proc].iter().collect();
                tnames.sort();
                for tname in tnames {
                    let tg = triggers.get(tname).copied();
                    let answered: IndexSet<&String> = tg.map_or(IndexSet::new(), |tg| tg.causes.iter().filter(|(k, _, _)| k == "reply").map(|(_, key, _)| key).collect());
                    for reason in &decl.failures {
                        if !answered.contains(reason) {
                            self.d(ERROR, "失敗", &format!("処理 {}", proc), &format!("外部作用 {} の失敗の理由「{}」への応答が、トリガ {} にありません", eff, reason, tname), 0);
                        }
                    }
                }
            }
        }
    }

    pub fn check_join(&mut self, proc: &str, decl: &EffectDecl, tg: Option<&Trigger>, waits: &IndexMap<String, IndexSet<String>>, groups: &[Vec<String>]) {
        let tg = match tg {
            Some(t) => t,
            None => return,
        };
        let mine: IndexMap<&String, &Type> = self.procs[proc].inputs.iter().map(|(n, t)| (n, t)).collect();
        for (n, t) in &tg.params {
            let bad = match mine.get(n) {
                None => true,
                Some(m) => show_type(m).unwrap_or_default() != show_type(t).unwrap_or_default() || m.optional,
            };
            if bad {
                self.d(
                    ERROR,
                    "合流",
                    &format!("処理 {}", proc),
                    &format!("外部作用 {} の返事が合流するトリガ {} の引数「{}」を、処理 {} は同じ型の必須の入力にもっていません（どの待ちか決まりません）", decl.name, tg.name, n, proc),
                    0,
                );
            }
        }
        if waits.get(proc).map_or(false, |w| w.contains(&tg.name)) {
            return;
        }
        for ds in groups {
            if ds.iter().any(|d| d == proc) && ds.iter().any(|d| d != proc && waits.get(d).map_or(false, |w| w.contains(&tg.name))) {
                return;
            }
        }
        self.d(
            ERROR,
            "合流",
            &format!("処理 {}", proc),
            &format!("外部作用 {} の返事はトリガ {} に合流しますが、{} を待つ処理が、この処理自身にも、同じ並列（par）の枝にもありません", decl.name, tg.name, tg.name),
            0,
        );
    }

    /// where の条件 1 つ（項目 比較 状態名か数）の型の検査
    pub fn check_cond(&mut self, where_: &str, f: &Field, fname: &str, op: &str, st: &str, line: i64) {
        if is_digits(st) {
            if !["整数", "金額", "日時"].contains(&f.r#type.kind.as_str()) {
                self.d(ERROR, "型", where_, &format!("条件「{} {} {}」: 数との比較は、整数・金額・日時の項目にだけ使えます", fname, op, st), line);
            }
        } else {
            let ok = f.r#type.kind == "名前"
                && self.p.states.iter().filter(|s| Some(&s.name) == f.r#type.target.as_ref()).any(|s| std::iter::once(&s.initial).chain(s.middle.iter()).chain(s.final_.iter()).any(|x| x == st));
            if !ok {
                self.d(ERROR, "型", where_, &format!("条件「{} {} {}」: 「{}」は項目 {} の状態ではありません", fname, op, st, st, fname), line);
            }
        }
    }

    pub fn check_scopes(&mut self) {
        let p = self.p;
        let mut seen: IndexSet<&String> = IndexSet::new();
        for s in &p.scopes {
            let w = format!("範囲 {}", s.name);
            if seen.contains(&s.name) {
                self.d(ERROR, "範囲", &w, "同じ名前の範囲が2つ以上あります", s.line);
            }
            seen.insert(&s.name);
            let dt = match self.data.get(&s.data) {
                Some(d) => *d,
                None => {
                    let msg = format!("データ「{}」が未定義です{}", s.data, near(&s.data, self.data.keys()));
                    self.d(ERROR, "範囲", &w, &msg, s.line);
                    continue;
                }
            };
            let fields: IndexMap<&String, &Field> = dt.fields.iter().map(|f| (&f.name, f)).collect();
            if fields.contains_key(&s.name) {
                self.d(ERROR, "範囲", &w, &format!("データ {} に同じ名前の項目があります（{}.{} が、項目か範囲か分からなくなります）", s.data, s.data, s.name), s.line);
            }
            for g in &s.conds {
                for (fname, op, st) in g {
                    match fields.get(fname) {
                        None => {
                            let msg = format!("データ {} に項目「{}」がありません{}", s.data, fname, near(fname, fields.keys()));
                            self.d(ERROR, "範囲", &w, &msg, s.line);
                        }
                        Some(f) => self.check_cond(&w, f, fname, op, st, s.line),
                    }
                }
            }
            let names: Vec<&String> = s.params.iter().map(|(n, _)| n).collect();
            for n in &names {
                if names.iter().filter(|x| *x == n).count() > 1 {
                    self.d(ERROR, "範囲", &w, &format!("引数「{}」が重なっています", n), s.line);
                }
                if n.as_str() == CALLER {
                    self.d(ERROR, "範囲", &w, &format!("引数の名前に {} は使えません（呼んだ本人の ID を表す語です）", CALLER), s.line);
                }
                if !s.filters.iter().any(|(_, _, a)| a == *n) {
                    self.d(ERROR, "範囲", &w, &format!("引数「{}」が、どの filter にも使われていません", n), s.line);
                }
            }
            for (fname, op, a) in &s.filters {
                match fields.get(fname) {
                    None => {
                        let msg = format!("データ {} に項目「{}」がありません{}", s.data, fname, near(fname, fields.keys()));
                        self.d(ERROR, "範囲", &w, &msg, s.line);
                    }
                    Some(f) => {
                        if !names.contains(&a) {
                            let msg = format!("filter の相手「{}」が、範囲の引数にありません（比べられるのは、引数か caller です）{}", a, near(a, names.iter().copied()));
                            self.d(ERROR, "範囲", &w, &msg, s.line);
                        } else {
                            let ft = &f.r#type;
                            let t = &s.params.iter().rev().find(|(n, _)| n == a).unwrap().1;
                            if op == "一致" && !((ft.kind == t.kind && ["文字列", "ID", "整数"].contains(&ft.kind.as_str())) || (ft.kind == "参照" && t.kind == "ID")) {
                                self.d(ERROR, "型", &w, &format!("「{} equals {}」は同じ種類の文字列・ID・整数どうし（参照の項目はIDの引数）にだけ使えます", fname, a), s.line);
                            }
                            if (op == "以上" || op == "以下") && !(ft.kind == t.kind && ["日時", "整数", "金額"].contains(&ft.kind.as_str())) {
                                self.d(ERROR, "型", &w, &format!("「{} {} {}」は同じ種類の数・日時どうしにだけ使えます", fname, op, a), s.line);
                            }
                        }
                    }
                }
            }
            if !s.caller.is_empty() {
                match fields.get(&s.caller) {
                    None => {
                        let msg = format!("データ {} に項目「{}」がありません{}", s.data, s.caller, near(&s.caller, fields.keys()));
                        self.d(ERROR, "範囲", &w, &msg, s.line);
                    }
                    Some(f) => {
                        if f.r#type.kind != "ID" && f.r#type.kind != "参照" {
                            self.d(ERROR, "範囲", &w, &format!("「{} equals caller」は、ID か参照（ref）の項目にだけ使えます（呼んだ本人の ID と比べるため）", s.caller), s.line);
                        }
                    }
                }
            }
            if !p.queries.iter().any(|q| q.scopes.contains(&s.name)) {
                self.d(WARN, "範囲", &w, "どこにも使われていません", s.line);
            }
        }
    }

    /// 問い合わせ（読むだけの処理）の検査
    pub fn check_queries(&mut self) {
        self.check_scopes();
        let p = self.p;
        for q in &p.queries {
            let where_ = format!("問い合わせ {}", q.name);
            if self.procs.contains_key(&q.name) {
                self.d(ERROR, "名前", &where_, "同じ名前の処理があります", 0);
            }
            let dt = match self.data.get(&q.target) {
                Some(d) => *d,
                None => {
                    let msg = format!("対象のデータ「{}」が未定義です{}", q.target, near(&q.target, self.data.keys()));
                    self.d(ERROR, "名前", &where_, &msg, 0);
                    continue;
                }
            };
            let fields: IndexMap<&String, &Field> = dt.fields.iter().map(|f| (&f.name, f)).collect();
            let mut used: IndexSet<String> = IndexSet::new();
            let ins: IndexMap<&String, &Type> = q.inputs.iter().map(|(n, t)| (n, t)).collect();
            if ins.contains_key(&CALLER.to_string()) {
                self.d(ERROR, "範囲", &where_, &format!("入力の名前に {} は使えません（範囲の中で、呼んだ本人の ID を表す語です）", CALLER), q.line);
            }
            if ins.contains_key(&AFTER.to_string()) && paged(q) {
                self.d(ERROR, "並び", &where_, &format!("page のある問い合わせでは、入力の名前に {} は使えません（続きのページを読む印 q[{}] に使う語です）", AFTER, AFTER), q.line);
            }
            let known: IndexMap<&String, &Scope> = p.scopes.iter().map(|s| (&s.name, s)).collect();
            for sn in &q.scopes {
                match known.get(sn) {
                    None => {
                        let msg = format!("範囲「{}」が未定義です{}", sn, near(sn, known.keys().copied()));
                        self.d(ERROR, "範囲", &where_, &msg, q.line);
                    }
                    Some(sc) => {
                        if sc.data != q.target {
                            self.d(ERROR, "範囲", &where_, &format!("範囲 {} は {} のものなので、from {} には使えません", sn, sc.data, q.target), q.line);
                        }
                    }
                }
                if q.scopes.iter().filter(|x| *x == sn).count() > 1 {
                    self.d(ERROR, "範囲", &where_, &format!("範囲「{}」を重ねて書いています", sn), q.line);
                }
            }
            let mine: Vec<&Scope> = scopes_of(p, q).into_iter().filter(|s| s.data == q.target).collect();
            for s in &mine {
                for g in &s.conds {
                    for (fname, _, _) in g {
                        if fields.contains_key(fname) {
                            used.insert(format!("{}.{}", dt.name, fname));
                        }
                    }
                }
                for (fname, _, _) in &s.filters {
                    if fields.contains_key(fname) {
                        used.insert(format!("{}.{}", dt.name, fname));
                    }
                }
                if fields.contains_key(&s.caller) {
                    used.insert(format!("{}.{}", dt.name, s.caller));
                }
            }
            // field_of：項目を引く。無ければ指摘して None
            macro_rules! field_of {
                ($fname:expr) => {{
                    let fname: &String = $fname;
                    if !fields.contains_key(fname) {
                        if dt.derived.iter().any(|x| &x.name == fname) {
                            self.d(ERROR, "項目", &where_, &format!("定義した項目「{}」は、問い合わせでは select の列にだけ書けます（条件・絞り込み・並び・集計には、保存する項目を使います）", fname), 0);
                        } else {
                            let msg = format!("データ {} に項目「{}」がありません{}", dt.name, fname, near(fname, dt.fields.iter().map(|f| f.name.clone())));
                            self.d(ERROR, "名前", &where_, &msg, 0);
                        }
                        None
                    } else {
                        used.insert(format!("{}.{}", dt.name, fname));
                        Some(fields[fname])
                    }
                }};
            }
            for group in &q.conds {
                for (fname, op, st) in group {
                    if let Some(f) = field_of!(fname) {
                        self.check_cond(&where_, f, fname, op, st, 0);
                    }
                }
            }
            let mut arg_filters: Vec<(String, String, String)> = vec![];
            for s in &mine {
                let given: Vec<String> = q.scope_args.get(&s.name).cloned().unwrap_or_default();
                if given.len() != s.params.len() {
                    let names = s.params.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>().join(", ");
                    self.d(
                        ERROR,
                        "範囲",
                        &where_,
                        &format!("範囲 {} の引数は {} 個です（{}）。渡しているのは {} 個です", s.name, s.params.len(), if names.is_empty() { "なし".to_string() } else { names }, given.len()),
                        q.line,
                    );
                    continue;
                }
                for ((pn, pt), a) in s.params.iter().zip(given.iter()) {
                    match ins.get(a) {
                        None => self.d(ERROR, "範囲", &where_, &format!("範囲 {} に渡す「{}」が、入力にありません（渡せるのは、問い合わせの入力です）", s.name, a), q.line),
                        Some(t) => {
                            if t.optional {
                                self.d(ERROR, "範囲", &where_, &format!("範囲 {} に渡す入力「{}」は required にしてください（無いときに、範囲が絞られなくなるのを防ぎます）", s.name, a), q.line);
                            } else if t.kind != pt.kind {
                                self.d(ERROR, "範囲", &where_, &format!("範囲 {} の引数 {} は {} ですが、渡している入力「{}」は {} です", s.name, pn, pt.kind, a, t.kind), q.line);
                            }
                        }
                    }
                }
                let by: IndexMap<&String, &String> = s.params.iter().map(|(n, _)| n).zip(given.iter()).collect();
                for (f, op, a) in &s.filters {
                    if let Some(g) = by.get(a) {
                        if ins.contains_key(*g) {
                            arg_filters.push((f.clone(), op.clone(), (*g).clone()));
                        }
                    }
                }
            }
            let all_filters: Vec<(String, String, String)> = q.filters.iter().cloned().chain(arg_filters.into_iter()).collect();
            for (fname, op, arg) in &all_filters {
                let f = match field_of!(fname) {
                    Some(f) => f,
                    None => continue,
                };
                if op == "直近" {
                    if f.r#type.kind != "日時" {
                        self.d(ERROR, "型", &where_, &format!("「{} 直近」は日時の項目にだけ使えます", fname), 0);
                    }
                    if !q.refs.iter().any(|(n, _)| n == "current_time") {
                        self.d(ERROR, "名前", &where_, "「within」を使うには、時刻の slot current_time が必要です", 0);
                    }
                    continue;
                }
                let t = match ins.get(arg) {
                    Some(t) => *t,
                    None => {
                        self.d(ERROR, "名前", &where_, &format!("絞り込みの入力「{}」が、入力にありません", arg), 0);
                        continue;
                    }
                };
                if op.starts_with("含む") && !(f.r#type.kind == "文字列" && t.kind == "文字列") {
                    self.d(ERROR, "型", &where_, &format!("「{} 含む {}」は文字列どうしにだけ使えます", fname, arg), 0);
                }
                if op == "一致" && !((f.r#type.kind == t.kind && ["文字列", "ID", "整数"].contains(&f.r#type.kind.as_str())) || (f.r#type.kind == "参照" && t.kind == "ID")) {
                    self.d(ERROR, "型", &where_, &format!("「{} equals {}」は同じ種類の文字列・ID・整数どうし（参照の項目はIDの入力）にだけ使えます", fname, arg), 0);
                }
                if (op == "以上" || op == "以下") && !(f.r#type.kind == t.kind && ["日時", "整数", "金額"].contains(&f.r#type.kind.as_str())) {
                    self.d(ERROR, "型", &where_, &format!("「{} {} {}」は同じ種類の数・日時どうしにだけ使えます", fname, op, arg), 0);
                }
            }
            let mut seen_cols: IndexSet<&String> = IndexSet::new();
            for c in &q.select {
                if seen_cols.contains(&c.name) {
                    self.d(ERROR, "名前", &where_, &format!("select の列名「{}」が重なっています（as で別の名前にしてください）", c.name), 0);
                }
                seen_cols.insert(&c.name);
                if c.kind == "参照先" {
                    let via = c.via.clone().unwrap_or_default();
                    let rf = match field_of!(&via) {
                        Some(f) => f,
                        None => continue,
                    };
                    let tdt = match rf.r#type.target.as_ref().filter(|_| rf.r#type.kind == "参照").and_then(|t| self.data.get(t)) {
                        Some(d) => *d,
                        None => {
                            self.d(ERROR, "型", &where_, &format!("「{}.{}」: {} は他のデータへの参照（ref）ではありません", via, c.field, via), 0);
                            continue;
                        }
                    };
                    let tf = match tdt.fields.iter().find(|x| x.name == c.field) {
                        Some(t) => t,
                        None => {
                            let msg = format!("データ {} に項目「{}」がありません{}", tdt.name, c.field, near(&c.field, tdt.fields.iter().map(|f| f.name.clone())));
                            self.d(ERROR, "名前", &where_, &msg, 0);
                            continue;
                        }
                    };
                    used.insert(format!("{}.{}", tdt.name, c.field));
                    if !SELECT_KINDS.contains(&tf.r#type.kind.as_str()) {
                        self.d(ERROR, "型", &where_, &format!("「{}.{}」は select に書けない型です（{}）", via, c.field, tf.r#type.kind), 0);
                    }
                    continue;
                }
                let dv = if c.kind == "項目" { dt.derived.iter().find(|x| x.name == c.field) } else { None };
                if let Some(dv) = dv {
                    for n in names_in(dv.expr.as_ref().unwrap()) {
                        used.insert(format!("{}.{}", dt.name, n));
                    }
                    continue;
                }
                let f = match field_of!(&c.field) {
                    Some(f) => f,
                    None => continue,
                };
                if c.kind == "項目" && !SELECT_KINDS.contains(&f.r#type.kind.as_str()) {
                    self.d(ERROR, "型", &where_, &format!("「{}」は select に書けない型です（{}）。集まりは sum( ) か count( ) にしてください", c.field, f.r#type.kind), 0);
                }
                if (c.kind == "合計" || c.kind == "件数") && f.r#type.kind != "写像" {
                    self.d(ERROR, "型", &where_, &format!("「{}」は集計できません（sum と count は、キーつきの集まり 〈データ → 値〉 の項目にだけ使えます）", c.field), 0);
                }
                if c.kind == "合計" && f.r#type.kind == "写像" && !["整数", "金額"].contains(&f.r#type.value.as_ref().unwrap().kind.as_str()) {
                    self.d(ERROR, "型", &where_, &format!("sum({}) は、値が整数か金額の集まりにだけ使えます", c.field), 0);
                }
            }
            for (of, _od) in &q.orders {
                if let Some(f) = field_of!(of) {
                    if !["日時", "整数", "金額"].contains(&f.r#type.kind.as_str()) {
                        self.d(ERROR, "型", &where_, &format!("並びの項目「{}」は数か日時である必要があります", of), 0);
                    }
                }
            }
            let mut dups: Vec<&(String, String)> = q.orders.iter().filter(|x| q.orders.iter().filter(|y| y == x).count() > 1).collect();
            dups.sort();
            dups.dedup();
            for dup in dups {
                self.d(ERROR, "並び", &where_, &format!("同じ並び「order {} {}」を 2 回書いています", dup.0, order_word(&dup.1)), q.line);
            }
            if !order_choices(q).is_empty() && ins.contains_key(&ORDER.to_string()) {
                self.d(ERROR, "並び", &where_, &format!("並びを 2 行以上書いた問い合わせでは、入力の名前に {} は使えません（並びを選ぶ q[{}] に使う語です）", ORDER, ORDER), q.line);
            }
            if let Some(pg) = &q.page {
                let t = ins.get(pg);
                if t.map_or(true, |t| t.kind != "整数") {
                    self.d(ERROR, "型", &where_, &format!("1ページの件数「{}」は、整数の入力である必要があります", pg), 0);
                }
            }
            if (q.page.is_some() || q.page_n.is_some()) && q.order.is_none() {
                self.d(ERROR, "並び", &where_, "page には order が要ります（並びが無いと、どの1ページかが決まらず、実行のたびに変わりえます）", 0);
            }
            for (n, t) in &q.inputs {
                if !t.optional && !q.filters.iter().any(|(_, _, a)| a == n) && q.page.as_ref() != Some(n) && !q.scope_args.values().any(|xs| xs.contains(n)) {
                    self.d(WARN, "宣言", &where_, &format!("入力「{}」はどこにも使われていません", n), 0);
                }
            }
            let declared: IndexSet<String> = q.reads.iter().cloned().collect();
            let actual = used;
            let mut miss: Vec<&String> = actual.iter().filter(|x| !declared.contains(*x)).collect();
            miss.sort();
            for x in miss {
                self.d(ERROR, "宣言", &where_, &format!("問い合わせは「{}」を読むのに、宣言（読む）にありません", x), 0);
            }
            let mut extra: Vec<&String> = declared.iter().filter(|x| !actual.contains(*x)).collect();
            extra.sort();
            for x in extra {
                self.d(ERROR, "宣言", &where_, &format!("宣言（読む）に「{}」がありますが、使っていません", x), 0);
            }
            self.query_reads.insert(q.name.clone(), actual);
        }
    }

    /// 結果の中身を、次の処理の入力へ名前と型で渡せるか（過不足も型違いも許さない）
    pub fn check_pass(&mut self, where_: &str, rdef: &ResultDef, v: &str, nxt: &Process) {
        let ps = match rdef.variants.iter().find(|(x, _)| x == v) {
            Some((_, ps)) => ps,
            None => return,
        };
        let have: IndexMap<&String, String> = ps.iter().map(|(n, t)| (n, show_type(t).unwrap_or_default())).collect();
        let want: IndexMap<&String, String> = nxt.inputs.iter().map(|(n, t)| (n, show_type(t).unwrap_or_default())).collect();
        for n in want.keys() {
            match have.get(n) {
                None => self.d(ERROR, "行き先", where_, &format!("処理 {} の入力「{}」が、結果「{}」の中身にありません", nxt.name, n, v), 0),
                Some(h) => {
                    if h != &want[n] {
                        self.d(ERROR, "行き先", where_, &format!("結果「{}」の中身「{}」の型（{}）が、処理 {} の入力（{}）と違います", v, n, h, nxt.name, want[n]), 0);
                    }
                }
            }
        }
        for n in have.keys() {
            if !want.contains_key(n) {
                self.d(ERROR, "行き先", where_, &format!("結果「{}」の中身「{}」を、処理 {} は入力にもっていません", v, n, nxt.name), 0);
            }
        }
    }
}
