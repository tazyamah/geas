// SPDX-License-Identifier: GPL-3.0-or-later
//! 削除・リソース・役割・画面・状態・不変条件の検査（Python 版 check.py の Checker の続き）
use super::*;
use crate::rest::{routes, row_entity, screen_url, internal_targets, ANYONE, PATH_INT, PATH_TEXT};

fn count<T: PartialEq>(xs: &[T], x: &T) -> usize {
    xs.iter().filter(|y| *y == x).count()
}

fn states_of(s: &State) -> Vec<&String> {
    std::iter::once(&s.initial).chain(s.middle.iter()).chain(s.final_.iter()).collect()
}

impl<'a> Checker<'a> {
    pub fn check_deletes(&mut self) {
        let p = self.p;
        for dt in &p.data {
            let where_ = format!("データ {}", dt.name);
            if dt.keep && !dt.removable.is_empty() {
                self.d(ERROR, "削除", &where_, "keep（削除させない）と removable（消してよい状態）は、両方は書けません", 0);
            }
            for st in &dt.removable {
                let any = self.states.values().filter(|s| dt.fields.iter().any(|f| f.r#type.kind == "名前" && f.r#type.target.as_ref() == Some(&s.name))).any(|s| states_of(s).contains(&st));
                if !any {
                    self.d(ERROR, "削除", &where_, &format!("removable の「{}」は、{} のどの状態にもありません", st, dt.name), 0);
                }
            }
            for (src, fld, pol) in &dt.on_delete {
                let f = self.data.get(src).and_then(|sd| sd.fields.iter().find(|x| &x.name == fld));
                let is_ref = |f: &Field| (f.r#type.kind == "参照" && f.r#type.target.as_ref() == Some(&dt.name)) || (f.r#type.kind == "写像" && f.r#type.key.as_ref() == Some(&dt.name));
                match f {
                    None => self.d(ERROR, "削除", &where_, &format!("on delete の「{}.{}」は、{} を参照する項目ではありません", src, fld, dt.name), 0),
                    Some(f) if !is_ref(f) => self.d(ERROR, "削除", &where_, &format!("on delete の「{}.{}」は、{} を参照する項目ではありません", src, fld, dt.name), 0),
                    Some(f) => {
                        if pol == "detach" && (f.r#type.kind != "参照" || !f.r#type.optional) {
                            self.d(ERROR, "削除", &where_, &format!("{}.{} は、無いかもしれない（optional）参照ではないので、detach（参照を空にする）できません（leave か cascade にします）", src, fld), 0);
                        } else if pol == "cascade" && f.r#type.kind != "参照" {
                            self.d(ERROR, "削除", &where_, &format!("{}.{} は写像なので、cascade はできません（leave にします）", src, fld), 0);
                        }
                    }
                }
            }
        }
        let deleted: IndexSet<String> = self.infos.values().flat_map(|i| i.deletes.iter().cloned()).collect();
        let mut todo: Vec<String> = deleted.into_iter().collect();
        let mut seen: IndexSet<String> = IndexSet::new();
        while let Some(d) = todo.pop() {
            if seen.contains(&d) {
                continue;
            }
            seen.insert(d.clone());
            let dt = match self.data.get(&d) {
                Some(x) => *x,
                None => continue,
            };
            for rd in &p.data {
                for f in &rd.fields {
                    let r = (f.r#type.kind == "参照" && f.r#type.target.as_ref() == Some(&d)) || (f.r#type.kind == "写像" && f.r#type.key.as_ref() == Some(&d));
                    if !r {
                        continue;
                    }
                    let pol = dt.on_delete.iter().find(|x| x.0 == rd.name && x.1 == f.name).map(|x| x.2.clone());
                    match pol {
                        None => self.d(
                            QUESTION,
                            "削除",
                            &format!("データ {}", d),
                            &format!("{} を消すと、{}.{} の参照が残ります。どう扱うか決めてください（{} の中に on delete {}.{} → detach｜cascade｜leave）", d, rd.name, f.name, d, rd.name, f.name),
                            0,
                        ),
                        Some(pol) => {
                            if pol == "cascade" {
                                todo.push(rd.name.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn check_resources(&mut self) {
        let p = self.p;
        if p.resources.is_empty() {
            return;
        }
        let names: IndexMap<&String, &Resource> = p.resources.iter().map(|r| (&r.name, r)).collect();
        let mut used: IndexMap<String, String> = IndexMap::new();
        let mut seen_routes: IndexMap<(String, String), String> = IndexMap::new();
        for res in &p.resources {
            let where_ = format!("リソース {}", res.name);
            if !res.under.is_empty() && !names.contains_key(&res.under) {
                let msg = format!("親「{}」が未定義です{}", res.under, near(&res.under, p.resources.iter().map(|r| r.name.clone())));
                self.d(ERROR, "リソース", &where_, &msg, 0);
            }
            for a in &res.actions {
                let w = format!("{} / {} {}", where_, a.kind, a.target);
                let is_q = a.kind == "index" || a.kind == "show";
                let (qt, pt): (Option<&Query>, Option<&Process>) = if is_q { (self.queries.get(&a.target).copied(), None) } else { (None, self.procs.get(&a.target).copied()) };
                if qt.is_none() && pt.is_none() && a.kind == "create" && p.events.iter().any(|e| e.name == a.target) {
                    used.insert(a.target.clone(), format!("{}.create", res.name));
                    if !a.statuses.is_empty() {
                        self.d(ERROR, "リソース", &w, "出来事を起こす create は、結果の種類を持たないので、状態の対応は書けません", 0);
                    }
                    continue;
                }
                let inputs: &Vec<(String, Type)> = match (qt, pt) {
                    (Some(q), _) => &q.inputs,
                    (_, Some(x)) => &x.inputs,
                    _ => {
                        let what = if is_q { "問い合わせ" } else { "処理" };
                        let near_s = if is_q { near(&a.target, self.queries.keys()) } else { near(&a.target, self.procs.keys()) };
                        self.d(ERROR, "リソース", &w, &format!("{}「{}」が未定義です（{} が呼べるのは{}です）{}", what, a.target, a.kind, what, near_s), 0);
                        continue;
                    }
                };
                if let Some(u) = used.get(&a.target) {
                    let msg = format!("「{}」はすでに {} に割り当てています（1つの動作に呼ぶものは1つ。振り分けが要るなら別のリソースにします）", a.target, u);
                    self.d(ERROR, "リソース", &w, &msg, 0);
                }
                used.insert(a.target.clone(), format!("{}.{}", res.name, a.kind));
                let required: Vec<&String> = inputs.iter().filter(|(_, t)| !t.optional).map(|(n, _)| n).collect();
                let post = a.post.as_ref().map_or(false, |s| !s.is_empty());
                if is_q {
                    if post && a.kind != "index" {
                        self.d(ERROR, "リソース", &w, "post（POST で受ける問い合わせ）を書けるのは index だけです", a.line);
                    }
                    if post && res.actions.iter().any(|b| b.kind == "create") {
                        self.d(ERROR, "リソース", &w, "post の付いた index は POST /… になるので、同じリソースに create は書けません（検索のためのリソースを別に作ります）", a.line);
                    }
                    let keys: Vec<&String> = if a.kind == "show" { res.keys.iter().collect() } else { vec![] };
                    if a.kind == "show" && keys.is_empty() {
                        self.d(ERROR, "リソース", &w, "show にはパスのキー（key 名前）が要ります", 0);
                    }
                    let missing: Vec<&&String> = keys.iter().filter(|k| !required.contains(k)).collect();
                    for k in &missing {
                        self.d(ERROR, "リソース", &w, &format!("key「{}」が、問い合わせ {} の必須の入力にありません", k, a.target), 0);
                    }
                    if missing.is_empty() {
                        for (n, ty) in inputs {
                            if !PATH_TEXT.contains(&ty.kind.as_str()) && !PATH_INT.contains(&ty.kind.as_str()) {
                                self.d(ERROR, "リソース", &w, &format!("入力「{}」（{}）は、URL に載せられない型です（載せられるのは、ID・文字列・状態の名前・整数・金額・日時）", n, show_type(ty).unwrap_or_default()), 0);
                            }
                        }
                    }
                } else {
                    let proc = pt.unwrap();
                    if (a.kind == "update" || a.kind == "destroy") && res.keys.len() > 1 {
                        self.d(ERROR, "リソース", &w, &format!("{} は 1 つの ID で対象を決めるので、キーが複数のリソース（key {}）には書けません（別のリソースに分けます）", a.kind, res.keys.join(", ")), 0);
                    }
                    if (a.kind == "update" || a.kind == "destroy") && !proc.inputs.iter().any(|(_, t)| t.kind == "ID") {
                        self.d(ERROR, "リソース", &w, &format!("{} は /…/:id の ID で対象を決めるので、処理に ID の入力が要ります", a.kind), 0);
                    } else if (a.kind == "update" || a.kind == "destroy") && row_entity(p, &proc.name).is_empty() {
                        let idn = proc.inputs.iter().find(|(_, t)| t.kind == "ID").map(|(n, _)| n.clone()).unwrap_or_default();
                        self.d(
                            ERROR,
                            "リソース",
                            &w,
                            &format!("{} の処理は、ID の入力で 1 行を引きます（let x = 実体[{}] else {{ … }}）。サーバーは、その行の版を If-Match と比べてから処理を走らせます（どの行を書き換えるのかが、処理から分かる必要があります）", a.kind, idn),
                            0,
                        );
                    }
                    let rdef = self.results.get(&proc.result).copied();
                    let variants: Vec<&String> = rdef.map_or(vec![], |r| r.variants.iter().map(|(v, _)| v).collect());
                    let given: Vec<&String> = a.statuses.iter().map(|(v, _)| v).collect();
                    for v in &variants {
                        if !given.contains(v) {
                            self.d(ERROR, "リソース", &w, &format!("結果「{}」の HTTP の状態がありません", v), 0);
                        }
                    }
                    for (v, code) in &a.statuses {
                        if !variants.contains(&v) {
                            self.d(ERROR, "リソース", &w, &format!("「{}」は処理 {} の結果の種類ではありません", v, a.target), 0);
                        }
                        if count(&given, &v) > 1 {
                            self.d(ERROR, "リソース", &w, &format!("結果「{}」の状態が複数あります", v), 0);
                        }
                        if !(200 <= *code && *code <= 599) {
                            self.d(ERROR, "リソース", &w, &format!("状態 {} は 200〜599 で書きます", code), 0);
                        }
                    }
                }
            }
        }
        self.check_actors();
        for r in routes(p) {
            let key = (r.method.clone(), r.path.clone());
            if let Some(prev) = seen_routes.get(&key) {
                let msg = format!("{} {} が {} と重なります", r.method, r.path, prev);
                self.d(ERROR, "リソース", &format!("リソース {}", r.resource), &msg, 0);
            }
            seen_routes.insert(key, r.resource.clone());
        }
        let mut urls: IndexMap<String, String> = IndexMap::new();
        for sc in &p.screens {
            let where_ = format!("画面 {}", sc.name);
            for qn in &sc.shows {
                if !used.contains_key(qn) {
                    self.d(ERROR, "リソース", &where_, &format!("見せる問い合わせ「{}」が、どのリソースの index／show にも割り当てられていません", qn), 0);
                }
            }
            for (dn, _) in &sc.actions {
                if !used.contains_key(dn) {
                    self.d(ERROR, "リソース", &where_, &format!("操作の処理「{}」が、どのリソースの create／update／destroy にも割り当てられていません", dn), 0);
                }
            }
            let u = screen_url(p, sc);
            if !u.is_empty() {
                if let Some(o) = urls.get(&u) {
                    let msg = format!("URL {} が、画面 {} と重なります（1つの URL に画面は1つ）", u, o);
                    self.d(ERROR, "リソース", &where_, &msg, 0);
                }
                urls.insert(u, sc.name.clone());
            }
        }
        for pr in &p.processes {
            if !used.contains_key(&pr.name) {
                self.d(WARN, "リソース", &format!("処理 {}", pr.name), "どのリソースの動作にも割り当てていません（外から呼べません）", 0);
            }
        }
        for q in &p.queries {
            if !used.contains_key(&q.name) {
                self.d(WARN, "リソース", &format!("問い合わせ {}", q.name), "どのリソースの動作にも割り当てていません（外から読めません）", 0);
            }
        }
        for e in &p.events {
            if !used.contains_key(&e.name) {
                self.d(WARN, "リソース", &format!("出来事 {}", e.name), "どのリソースの create にも割り当てていません（外から起こせません）", 0);
            }
        }
    }

    /// 本人で絞る範囲を通して行を引く処理は、役割のある人が外から呼ぶ口だけで動くこと
    pub fn check_deed_callers(&mut self) {
        let p = self.p;
        let need: Vec<&Process> = p.processes.iter().filter(|x| self.infos.get(&x.name).map_or(false, |i| i.caller)).collect();
        if need.is_empty() {
            return;
        }
        let inner = internal_targets(p);
        let by_event: IndexSet<&String> = p.events.iter().flat_map(|ev| ev.steps.iter().map(|(_, _, pn)| pn)).collect();
        let mut acts: IndexMap<&String, &ResAction> = IndexMap::new();
        for res in &p.resources {
            for a in &res.actions {
                acts.insert(&a.target, a);
            }
        }
        for x in need {
            let w = format!("処理 {}", x.name);
            if inner.contains(&x.name) {
                self.d(ERROR, "範囲", &w, "本人で絞る範囲を通して引くので、流れの途中の処理にはできません（流れの途中には、呼んだ本人がいません）", x.line);
            } else if by_event.contains(&x.name) {
                self.d(ERROR, "範囲", &w, "本人で絞る範囲を通して引くので、出来事からは呼べません（出来事には、呼んだ本人がいません）", x.line);
            } else if acts.get(&x.name).map_or(true, |a| a.actors.as_ref().map_or(true, |ac| ac.iter().any(|y| y == ANYONE))) {
                self.d(ERROR, "範囲", &w, "本人で絞る範囲を通して引くので、呼べるのは役割のある人だけです（resource の動作に `for 役割` を書きます。for anyone にはできません）", x.line);
            }
        }
    }

    pub fn check_actors(&mut self) {
        let p = self.p;
        let names: Vec<&String> = p.actors.iter().map(|(n, _)| n).collect();
        for (n, line) in &p.actors {
            if n == ANYONE {
                self.d(ERROR, "役割", &format!("役割 {}", n), "anyone は「誰でも」を表す語なので、役割の名前にできません", *line);
            }
            if count(&names, &n) > 1 {
                self.d(ERROR, "役割", &format!("役割 {}", n), "同じ名前の役割が2つ以上あります", *line);
            }
        }
        let inner = internal_targets(p);
        let mut used: IndexSet<&String> = IndexSet::new();
        for res in &p.resources {
            for a in &res.actions {
                let w = format!("リソース {} / {} {}", res.name, a.kind, a.target);
                let actors = match &a.actors {
                    None => {
                        if !names.is_empty() && !inner.contains(&a.target) {
                            self.d(ERROR, "役割", &w, &format!("呼べる役割がありません。`{} {} for {}` のように書きます（誰でも呼べる口は `for anyone`）", a.kind, a.target, names[0]), a.line);
                        }
                        continue;
                    }
                    Some(x) => x,
                };
                if names.is_empty() {
                    self.d(ERROR, "役割", &w, "for に書く役割が宣言されていません（`actor 名前` で宣言します）", a.line);
                    continue;
                }
                if inner.contains(&a.target) {
                    self.d(ERROR, "役割", &w, "流れの途中の処理は外から呼べないので、for は書けません", a.line);
                    continue;
                }
                for x in actors {
                    if x != ANYONE && !names.contains(&x) {
                        let msg = format!("役割「{}」が未定義です{}", x, near(x, names.iter().copied()));
                        self.d(ERROR, "役割", &w, &msg, a.line);
                    }
                    if count(actors, x) > 1 {
                        self.d(ERROR, "役割", &w, &format!("役割「{}」を重ねて書いています", x), a.line);
                    }
                }
                if actors.iter().any(|y| y == ANYONE) && actors.len() > 1 {
                    self.d(ERROR, "役割", &w, "for anyone は誰でも呼べる口なので、ほかの役割と並べて書けません", a.line);
                }
                used.extend(actors.iter());
            }
        }
        let scoped: IndexSet<&String> = p.queries.iter().filter(|q| uses_caller(p, q)).map(|q| &q.name).collect();
        for res in &p.resources {
            for a in &res.actions {
                if scoped.contains(&a.target) && a.actors.as_ref().map_or(true, |ac| ac.iter().any(|y| y == ANYONE)) {
                    self.d(
                        ERROR,
                        "範囲",
                        &format!("リソース {} / {} {}", res.name, a.kind, a.target),
                        &format!("問い合わせ {} は、本人で絞る範囲を使うので、呼べるのは役割のある人だけです（`for 役割` を書きます。for anyone にはできません）", a.target),
                        a.line,
                    );
                }
            }
        }
        for (n, line) in &p.actors {
            if !p.resources.is_empty() && !used.contains(n) {
                self.d(WARN, "役割", &format!("役割 {}", n), "どの動作の for にも書かれていません", *line);
            }
        }
        if !names.is_empty() {
            if let Some(r) = &p.resident {
                self.d(ERROR, "役割", "resident", "役割のあるアプリは、常駐（窓つき exe）にできません。窓つき exe には、役割を確かめるプロキシが前にいないためです", r.line);
            }
        }
    }

    /// 画面の検査
    pub fn check_screens(&mut self) {
        static PAGE: OnceLock<Regex> = OnceLock::new();
        static ACCENT: OnceLock<Regex> = OnceLock::new();
        let page_re = PAGE.get_or_init(|| Regex::new(r"^(\*|https?)://[^/]+/.*$").unwrap());
        let accent_re = ACCENT.get_or_init(|| Regex::new(r"^#[0-9a-fA-F]{6}$").unwrap());
        let p = self.p;
        let scr: IndexMap<&String, &Screen> = p.screens.iter().map(|s| (&s.name, s)).collect();
        let mut edges: IndexMap<String, IndexSet<String>> = p.screens.iter().map(|s| (s.name.clone(), IndexSet::new())).collect();
        let inner = internal_targets(p);
        for sc in &p.screens {
            let where_ = format!("画面 {}", sc.name);
            if sc.shows.is_empty() && sc.actions.is_empty() && sc.opens.is_empty() {
                self.d(WARN, "画面", &where_, "見せるものも、操作もありません", 0);
            }
            if !sc.pages.is_empty() {
                let one = sc.shows.len() == 1 && self.queries.get(&sc.shows[0]).map_or(false, |q| q.inputs.len() == 1);
                if !one || !sc.actions.is_empty() || !sc.opens.is_empty() || !sc.params.is_empty() {
                    self.d(ERROR, "画面", &where_, "page を持つ画面（ブラウザ拡張）は、入力を1つだけ持つ問い合わせを1つ見せるだけにしてください（操作・open・受け取る値は持てません）", 0);
                }
                if one {
                    self.check_read(sc, &where_);
                }
                for pg in &sc.pages {
                    if !page_re.is_match(pg) {
                        self.d(ERROR, "画面", &where_, &format!("page「{}」は URL のパターンではありません（例 \"*://www.example.com/*\"）", pg), 0);
                    }
                }
            }
            let mut covered: IndexSet<String> = IndexSet::new();
            let have: IndexSet<&String> = sc.params.iter().map(|(n, _)| n).collect();
            for qn in &sc.shows {
                let q = match self.queries.get(qn).copied() {
                    Some(q) => q,
                    None => {
                        let msg = format!("見せる問い合わせ「{}」が未定義です{}", qn, near(qn, self.queries.keys()));
                        self.d(ERROR, "画面", &where_, &msg, 0);
                        continue;
                    }
                };
                for (n, ty) in &q.inputs {
                    covered.insert(n.clone());
                    if !have.contains(n) && !sc.fields.contains(n) && !ty.optional {
                        self.d(ERROR, "画面", &where_, &format!("問い合わせ {} の入力「{}」を埋めるものがありません（field か画面の受け取る値に書いてください）", qn, n), 0);
                    }
                }
            }
            let mut dup_opens: Vec<&String> = uniq_dups(&sc.opens);
            dup_opens.sort();
            for tn in dup_opens {
                self.d(ERROR, "画面", &where_, &format!("「open {}」が 2 つあります。1 行にしてください", tn), 0);
            }
            for tn in &sc.opens {
                let t = match scr.get(tn) {
                    Some(t) => *t,
                    None => {
                        let msg = format!("開く画面「{}」が未定義です{}", tn, near(tn, p.screens.iter().map(|x| x.name.clone())));
                        self.d(ERROR, "画面", &where_, &msg, 0);
                        continue;
                    }
                };
                edges.get_mut(&sc.name).unwrap().insert(tn.clone());
                if sc.shows.is_empty() {
                    self.d(ERROR, "画面", &where_, &format!("「open {}」は、見せる問い合わせの行を選んだときの行き先なので、show が要ります", tn), 0);
                }
                if t.params.len() > 1 || t.params.iter().any(|(_, ty)| ty.kind != "ID") {
                    self.d(ERROR, "画面", &where_, &format!("画面 {} は、行を選んで開かれるので、受け取る値は ID 1つだけにしてください", tn), 0);
                }
            }
            for (dn, rts) in &sc.actions {
                let proc = match self.procs.get(dn).copied() {
                    Some(x) => x,
                    None => {
                        let msg = format!("操作の処理「{}」が未定義です{}", dn, near(dn, self.procs.keys()));
                        self.d(ERROR, "画面", &where_, &msg, 0);
                        continue;
                    }
                };
                if inner.contains(dn) {
                    self.d(ERROR, "画面", &where_, &format!("操作の処理「{}」は流れの途中の処理（返事・期限・前の処理の結果から進む処理）なので、サーバーは外に出しません。画面からは、流れの最初の処理を呼んでください", dn), 0);
                }
                for (n, ty) in &proc.inputs {
                    covered.insert(n.clone());
                    if !have.contains(n) && !sc.fields.contains(n) && !ty.optional {
                        self.d(ERROR, "画面", &where_, &format!("処理 {} の入力「{}」を埋めるものがありません（field か画面の受け取る値に書いてください）", dn, n), 0);
                    }
                }
                let rdef = self.results.get(&proc.result).copied();
                let variants: IndexMap<&String, &Vec<(String, Type)>> = rdef.map_or(IndexMap::new(), |r| r.variants.iter().map(|(v, ps)| (v, ps)).collect());
                let seen: Vec<&String> = rts.iter().map(|(v, _)| v).collect();
                let w2 = format!("{} / {}", where_, dn);
                for v in variants.keys() {
                    if !seen.contains(v) {
                        self.d(ERROR, "画面", &w2, &format!("結果「{}」の行き先がありません", v), 0);
                    }
                }
                for (v, dest) in rts {
                    if !variants.contains_key(v) {
                        self.d(ERROR, "画面", &w2, &format!("「{}」は処理 {} の結果の種類ではありません", v, dn), 0);
                    }
                    if count(&seen, &v) > 1 {
                        self.d(ERROR, "画面", &w2, &format!("結果「{}」の行き先が複数あります", v), 0);
                    }
                    if dest == "stay" || dest == "back" {
                        continue;
                    }
                    let t = match scr.get(dest) {
                        Some(t) => *t,
                        None => {
                            self.d(ERROR, "画面", &w2, &format!("行き先「{}」が、画面でも stay／back でもありません", dest), 0);
                            continue;
                        }
                    };
                    edges.get_mut(&sc.name).unwrap().insert(dest.clone());
                    let mut avail: IndexSet<&String> = have.clone();
                    avail.extend(proc.inputs.iter().map(|(n, _)| n));
                    if let Some(vs) = variants.get(v) {
                        avail.extend(vs.iter().map(|(n, _)| n));
                    }
                    for (n, _) in &t.params {
                        if !avail.contains(n) {
                            self.d(ERROR, "画面", &w2, &format!("行き先の画面 {} が受け取る「{}」を、この結果から渡せません", dest, n), 0);
                        }
                    }
                }
            }
            let mut types: IndexMap<&String, &Type> = IndexMap::new();
            for (dn, _) in &sc.actions {
                if let Some(x) = self.procs.get(dn) {
                    for (n, ty) in &x.inputs {
                        types.insert(n, ty);
                    }
                }
            }
            for qn in &sc.shows {
                if let Some(q) = self.queries.get(qn) {
                    for (n, ty) in &q.inputs {
                        types.insert(n, ty);
                    }
                }
            }
            for f in &sc.fields {
                if !covered.contains(f) {
                    self.d(ERROR, "画面", &where_, &format!("入力欄「{}」は、どの操作の処理や問い合わせの入力でもありません", f), 0);
                }
            }
            for (f, qn) in sc.field_from.iter() {
                let q = self.queries.get(qn).copied();
                let ty = types.get(f).copied();
                match q {
                    None => {
                        let msg = format!("入力欄「{}」の選ぶ元の問い合わせ「{}」が未定義です{}", f, qn, near(qn, self.queries.keys()));
                        self.d(ERROR, "画面", &where_, &msg, 0);
                    }
                    Some(q) => {
                        if let Some(ty) = ty {
                            let ok = ty.kind == "ID" || ty.kind == "参照" || (ty.kind == "写像" && ty.value.as_ref().map_or(false, |v| v.kind == "整数") && ty.key.as_ref() == Some(&q.target));
                            if !ok {
                                self.d(ERROR, "画面", &where_, &format!("入力欄「{}」（{}）は、問い合わせ {} から選べません（ID か、{} をキーにした数の写像だけです）", f, show_type(ty).unwrap_or_default(), qn, q.target), 0);
                            }
                        }
                    }
                }
            }
            for sl in &sc.slots {
                let mut refs: IndexMap<&String, &Type> = IndexMap::new();
                for (dn, _) in &sc.actions {
                    if let Some(x) = self.procs.get(dn) {
                        for (n, ty) in &x.refs {
                            refs.insert(n, ty);
                        }
                    }
                }
                match refs.get(sl) {
                    None => self.d(ERROR, "画面", &where_, &format!("slot「{}」は、この画面の操作の処理の slot ではありません", sl), 0),
                    Some(ty) => {
                        if ty.kind != "名前" {
                            self.d(ERROR, "画面", &where_, &format!("slot「{}」（{}）は、利用者に選ばせられません（状態や通貨のように、選べる値のものだけです）", sl, show_type(ty).unwrap_or_default()), 0);
                        }
                    }
                }
            }
            for (k, v) in sc.design.iter() {
                let choices: Option<&[&str]> = match k.as_str() {
                    "layout" => Some(&["form", "grid", "list", "stack"]),
                    "mode" => Some(&["dark", "light"]),
                    "accent" => None,
                    _ => {
                        self.d(ERROR, "画面", &where_, &format!("design の「{}」は使えません（layout／mode／accent）", k), 0);
                        continue;
                    }
                };
                match choices {
                    None => {
                        if !accent_re.is_match(v) {
                            self.d(ERROR, "画面", &where_, &format!("design の accent は \"#rrggbb\" の形で書きます（\"{}\"）", v), 0);
                        }
                    }
                    Some(cs) => {
                        if !cs.contains(&v.as_str()) {
                            self.d(ERROR, "画面", &where_, &format!("design の {} は {} のどれかです（{}）", k, cs.join("／"), v), 0);
                        }
                    }
                }
            }
        }
        if !p.screens.is_empty() {
            let mut reach: IndexSet<String> = IndexSet::new();
            let mut todo: Vec<String> = p.screens.iter().enumerate().filter(|(i, x)| *i == 0 || x.entry).map(|(_, x)| x.name.clone()).collect();
            while let Some(x) = todo.pop() {
                if !reach.contains(&x) {
                    reach.insert(x.clone());
                    if let Some(es) = edges.get(&x) {
                        todo.extend(es.iter().cloned());
                    }
                }
            }
            for sc in &p.screens {
                if !reach.contains(&sc.name) {
                    self.d(WARN, "画面", &format!("画面 {}", sc.name), "入口の画面から行けません（別の入口なら entry を付けてください）", 0);
                }
            }
        }
    }

    pub fn check_states(&mut self) {
        let p = self.p;
        for s in &p.states {
            if s.plain {
                continue;
            }
            let mut edges: IndexMap<String, IndexSet<String>> = IndexMap::new();
            for info in self.infos.values() {
                for (dt, a, b) in &info.transitions {
                    if self.data.get(dt).map_or(false, |d| d.fields.iter().any(|f| f.r#type.target.as_ref() == Some(&s.name))) {
                        edges.entry(a.clone()).or_default().insert(b.clone());
                    }
                }
            }
            let reach = |starts: Vec<String>| -> IndexSet<String> {
                let mut seen: IndexSet<String> = IndexSet::new();
                let mut todo = starts;
                while let Some(y) = todo.pop() {
                    if !seen.contains(&y) {
                        seen.insert(y.clone());
                        if let Some(es) = edges.get(&y) {
                            todo.extend(es.iter().cloned());
                        }
                    }
                }
                seen
            };
            let mut starts = vec![s.initial.clone()];
            if let Some(es) = edges.get("(新規)") {
                starts.extend(es.iter().cloned());
            }
            let got = reach(starts);
            let mut msgs: Vec<(&str, String)> = vec![];
            for x in s.middle.iter().chain(s.final_.iter()) {
                if !got.contains(x) {
                    msgs.push(("a", format!("「{}」には、初期からも新規作成からも進む処理が宣言されていません", x)));
                }
            }
            for x in std::iter::once(&s.initial).chain(s.middle.iter()) {
                let seen = reach(vec![x.clone()]);
                if !s.endless && !seen.iter().any(|y| s.final_.contains(y)) {
                    msgs.push(("b", format!("「{}」から終了の状態へ進む処理が宣言されていません", x)));
                }
            }
            for (_, m) in msgs {
                self.d(WARN, "状態", &format!("状態 {}", s.name), &m, 0);
            }
        }
    }

    pub fn check_invariants(&mut self) {
        let p = self.p;
        for (i, inv) in p.invariants.iter().enumerate() {
            let text = format!("{}: {}", inv.data, show(&inv.cond));
            let form = self.inv_forms[i].clone();
            let where_ = format!("不変条件 {}", text);
            if !self.data.contains_key(&inv.data) {
                let msg = format!("データ「{}」が未定義です{}", inv.data, near(&inv.data, self.data.keys()));
                self.d(ERROR, "名前", &where_, &msg, 0);
                continue;
            }
            let rel = self.rel_invs.iter().find(|r| r.i == Some(i)).cloned();
            if form.is_none() {
                if let Some(rel) = rel {
                    let dt = self.data[&inv.data];
                    let bad = self.rel_problem(dt, &rel);
                    if !bad.is_empty() {
                        self.d(ERROR, "不変条件", &where_, &bad, 0);
                        continue;
                    }
                    let rows = self.inv_status_rows((false, i));
                    self.invariant_status.push((text, rows));
                    continue;
                }
                self.d(
                    ERROR,
                    "不変条件",
                    &where_,
                    "この形の不変条件は、まだ検査できません（「項目 >= 数」「状態の項目 == 状態 implies 項目 > 数」と、同じ行の 2 つの数の項目の比較「項目 <= 項目」だけ。足し算・引き算の式は、項目として定義してから比べます：held: Quantity = stock + reserved）。検査できない約束は書けません",
                    0,
                );
                continue;
            }
            let form = form.unwrap().1;
            let fields: IndexMap<&String, &Field> = self.data[&inv.data].fields.iter().map(|f| (&f.name, f)).collect();
            let (names, st_info): (Vec<String>, Option<(String, String)>) = match &form {
                InvForm::Lower(_, f, _) => (vec![f.clone()], None),
                InvForm::Implies(_, sf, st, f, _) => (vec![sf.clone(), f.clone()], Some((sf.clone(), st.clone()))),
            };
            let bad: Vec<&String> = names.iter().filter(|n| !fields.contains_key(n)).collect();
            if let Some(b) = bad.first() {
                let msg = format!("データ {} に項目「{}」がありません{}", inv.data, b, near(b, self.data[&inv.data].fields.iter().map(|f| f.name.clone())));
                self.d(ERROR, "名前", &where_, &msg, 0);
                continue;
            }
            if let Some((sf, st_name)) = st_info {
                let st = fields[&sf].r#type.target.as_ref().and_then(|t| self.states.get(t).copied());
                if st.map_or(true, |s| !states_of(s).contains(&&st_name)) {
                    self.d(ERROR, "名前", &where_, &format!("「{}」は項目 {} の状態ではありません", st_name, sf), 0);
                    continue;
                }
            }
            let rows = self.inv_status_rows((false, i));
            self.invariant_status.push((text, rows));
        }
        for k in 0..self.rel_invs.len() {
            if self.rel_invs[k].i.is_none() {
                let text = self.rel_invs[k].text.clone();
                let rows = self.inv_status_rows((true, k));
                self.invariant_status.push((text, rows));
            }
        }
    }

    pub fn inv_status_rows(&self, key: (bool, usize)) -> Vec<(String, String)> {
        let mut rows = vec![];
        for proc in &self.p.processes {
            let got = self.inv_rows.get(&key).and_then(|m| m.get(&proc.name));
            match got {
                None => rows.push((proc.name.clone(), "対象外".to_string())),
                Some(g) if g.is_empty() => rows.push((proc.name.clone(), "対象外".to_string())),
                Some(g) => {
                    if g.iter().all(|(ok, _)| *ok) {
                        let notes: IndexSet<&String> = g.iter().map(|(_, n)| n).collect();
                        rows.push((proc.name.clone(), format!("証明済み（{}）", notes.into_iter().cloned().collect::<Vec<_>>().join(" ／ "))));
                    } else {
                        rows.push((proc.name.clone(), "証明できない".to_string()));
                    }
                }
            }
        }
        rows
    }

    /// 項目どうしを比べる不変条件に書けない形なら、その理由
    pub fn rel_problem(&self, dt: &Data, rel: &RelInv) -> String {
        let c0 = &self.p.invariants[rel.i.unwrap()].cond;
        let c = if let Expr::Implies { r, .. } = c0 { &**r } else { c0 };
        let ok = match c {
            Expr::Bin { l, r, .. } => matches!(**l, Expr::Name { .. }) && matches!(**r, Expr::Name { .. } | Expr::Num { .. }),
            _ => false,
        };
        if !ok {
            return "不変条件に、足し算・引き算の式は直に書けません。式は項目として定義してから比べます（例：held: Quantity = stock + reserved と定義して、invariant … : held <= cap）".to_string();
        }
        for con in &rel.cons {
            for f in con.0.keys() {
                if let Some(x) = dt.fields.iter().find(|x| &x.name == f) {
                    if (x.r#type.kind != "整数" && x.r#type.kind != "金額") || x.r#type.optional {
                        return format!("項目どうしを比べる不変条件の「{}」は、必須（required）の数の項目にしてください", f);
                    }
                }
            }
        }
        if let Some((sf, sv)) = &rel.when {
            let fld = dt.fields.iter().find(|x| &x.name == sf);
            let st = fld.and_then(|f| f.r#type.target.as_ref()).and_then(|t| self.states.get(t).copied());
            if st.map_or(true, |s| !states_of(s).contains(&sv)) {
                return format!("「{}」は項目 {} の状態ではありません", sv, sf);
            }
        }
        String::new()
    }
}

/// 2 回以上出てくるもの（最初に出た順）
pub fn uniq_dups<T: PartialEq>(xs: &[T]) -> Vec<&T> {
    let mut out: Vec<&T> = vec![];
    for x in xs {
        if xs.iter().filter(|y| *y == x).count() > 1 && !out.contains(&x) {
            out.push(x);
        }
    }
    out
}
