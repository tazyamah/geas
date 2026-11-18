// SPDX-License-Identifier: GPL-3.0-or-later
//! トリガ・応答・外部作用（ファイル・通信）・read・rule・出来事・保存先・常駐・環境の検査（Python 版 check.py の Checker の続き）
use super::*;
use crate::envreq::{derived_instance_limit, effect_namespaces, name_ok, ns_ok};
use crate::rawexpr::{refusal, requires_tree};

pub const MOCK_KINDS: [&str; 5] = ["ID", "参照", "文字列", "集合", "整数"];
pub const HTTP_BODY_KINDS: [&str; 4] = ["ID", "文字列", "整数", "真偽"];

fn rx(pat: &'static str, cell: &'static OnceLock<Regex>) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).unwrap())
}

fn sure_env(os: &str) -> &'static [&'static str] {
    match os {
        "windows" => &[
            "APPDATA", "LOCALAPPDATA", "USERPROFILE", "TEMP", "TMP", "PROGRAMDATA", "PUBLIC", "HOMEDRIVE", "HOMEPATH", "SYSTEMDRIVE", "SYSTEMROOT", "WINDIR", "USERNAME",
            "COMPUTERNAME", "PROGRAMFILES", "ALLUSERSPROFILE",
        ],
        "macos" => &["HOME", "USER", "TMPDIR"],
        _ => &["HOME", "USER"],
    }
}

fn uniq<T: PartialEq + Clone>(xs: &[T]) -> Vec<T> {
    let mut out: Vec<T> = vec![];
    for x in xs {
        if !out.contains(x) {
            out.push(x.clone());
        }
    }
    out
}

impl<'a> Checker<'a> {
    pub fn check_wait(&mut self, where_: &str, proc: &str, trig: &str) {
        let tg = match self.triggers.get(trig) {
            Some(t) => *t,
            None => {
                let msg = format!("wait 先のトリガ「{}」が未定義です{}", trig, near(trig, self.triggers.keys()));
                self.d(ERROR, "トリガ", where_, &msg, 0);
                return;
            }
        };
        let inputs = &self.procs[proc].inputs;
        for (n, _) in &tg.params {
            if !inputs.iter().any(|(m, _)| m == n) {
                self.d(ERROR, "トリガ", where_, &format!("トリガ {} の引数「{}」を、処理 {} は入力にもっていません", trig, n, proc), 0);
            }
        }
        for (n, t) in inputs {
            if t.optional && tg.params.iter().any(|(x, _)| x == n) {
                self.d(ERROR, "トリガ", where_, &format!("トリガ {} の引数「{}」には、required の入力を渡してください（無いかもしれない値では待てません）", trig, n), 0);
            }
        }
        let mine: IndexMap<&String, String> = inputs.iter().map(|(n, t)| (n, show_type(t).unwrap_or_default())).collect();
        for (n, t) in &tg.params {
            if let Some(m) = mine.get(n) {
                if *m != show_type(t).unwrap_or_default() {
                    self.d(ERROR, "トリガ", where_, &format!("トリガ {} の引数「{}」の型が、処理 {} の入力と違います", trig, n, proc), 0);
                }
            }
        }
    }

    /// 要因が成り立ったとき、行き先の処理へ渡るもの：トリガの引数に、起きた応答の中身を足したもの
    pub fn cause_args(&mut self, tg: &Trigger, kind: &str, key: &str, where_: &str) -> Vec<(String, Type)> {
        let p = self.p;
        let mut reps: IndexMap<&String, &ReplyDef> = IndexMap::new();
        for x in &p.replies {
            reps.insert(&x.name, x);
        }
        let names: Vec<String> = if kind == "all" {
            key.split(',').map(|s| s.to_string()).collect()
        } else if kind == "reply" {
            vec![key.to_string()]
        } else {
            vec![]
        };
        let mut args: Vec<(String, Type)> = tg.params.clone();
        for n in &names {
            if let Some(r) = reps.get(n) {
                for (pn, pt) in &r.params {
                    if args.iter().any(|(a, _)| a == pn) {
                        self.d(ERROR, "トリガ", where_, &format!("応答「{}」の中身「{}」が、トリガの引数か、ほかの応答の中身と同じ名前です（行き先の処理の入力として区別できません）", n, pn), 0);
                    } else {
                        args.push((pn.clone(), pt.clone()));
                    }
                }
            }
        }
        args
    }

    /// 応答の結果の種類（outcome）と、外部作用が受ける応答
    pub fn check_replies(&mut self) {
        let p = self.p;
        let mut reps: IndexMap<&String, &ReplyDef> = IndexMap::new();
        for x in &p.replies {
            reps.insert(&x.name, x);
        }
        for rp in &p.replies {
            let where_ = format!("応答 {}", rp.name);
            let ons: Vec<&String> = rp.outcomes.iter().map(|(n, _)| n).collect();
            self.check_unique(&ons, &where_, "結果の種類");
            let have: IndexMap<&String, &Type> = rp.params.iter().map(|(n, t)| (n, t)).collect();
            for (on, gives) in &rp.outcomes {
                for g in gives {
                    match have.get(g) {
                        None => self.d(ERROR, "応答", &where_, &format!("結果の種類「{}」の gives「{}」は、応答の中身にありません", on, g), 0),
                        Some(t) => {
                            if !MOCK_KINDS.contains(&t.kind.as_str()) {
                                self.d(ERROR, "応答", &where_, &format!("中身「{}」は、{} 型なので、模擬が値を作れません（ID・文字列・整数・集合だけ）", g, t.kind), 0);
                            }
                        }
                    }
                }
            }
        }
        for e in &p.effects {
            if let Some(r) = &e.replies {
                if !reps.contains_key(r) {
                    self.d(ERROR, "応答", &format!("外部作用 {}", e.name), &format!("replies「{}」が未定義の応答です", r), 0);
                }
            }
            if e.http.is_some() {
                self.check_http(e, &reps);
            }
            if e.file.is_some() {
                self.check_file(e, &reps);
            }
        }
    }

    /// over file { … }（外部作用のファイル操作）
    pub fn check_file(&mut self, e: &EffectDecl, reps: &IndexMap<&String, &ReplyDef>) {
        static SEG: OnceLock<Regex> = OnceLock::new();
        let seg = rx("^[^\\\\/:*?\"<>|\\x00-\\x1f]+$", &SEG);
        let f = e.file.as_ref().unwrap();
        let where_ = format!("外部作用 {} / over file", e.name);
        let params: IndexMap<&String, &Type> = e.params.iter().map(|(n, t)| (n, t)).collect();
        if f.op.is_empty() {
            self.d(ERROR, "ファイル", &where_, "write・overwrite（\"{場所の引数}/ファイル名\" from 内容の引数）、read・delete（\"{場所の引数}/ファイル名\"）のどれかの行が要ります", 0);
            return;
        }
        match params.get(&f.dir) {
            None => self.d(ERROR, "ファイル", &where_, &format!("場所の引数「{}」は、外部作用の引数にありません", f.dir), 0),
            Some(t) => {
                if !(t.kind == "文字列" && t.read.as_ref().map_or(false, |r| r.ends_with("_os_path"))) {
                    self.d(ERROR, "ファイル", &where_, &format!("場所の引数「{}」は、実行時に入力されるパスの型（ltype 名前 = path）にしてください。OS の書式を確かめた値だけが、場所になれます", f.dir), 0);
                }
            }
        }
        let bad_seg = f.rest.split('/').any(|s| s.is_empty() || s == "." || s == ".." || !seg.is_match(s));
        if bad_seg || f.rest.chars().count() > 200 {
            self.d(
                ERROR,
                "ファイル",
                &where_,
                &format!("ファイル名「{}」は、/ 区切りの相対パスで書きます（空の区切り・. ・.. ・\\ ・ : * ? \" < > | と制御文字は使えません。200 文字まで）", f.rest),
                0,
            );
        }
        if f.op == "write" || f.op == "overwrite" {
            match params.get(&f.src) {
                None => self.d(ERROR, "ファイル", &where_, &format!("{} の内容「{}」は、外部作用の引数にありません", f.op, f.src), 0),
                Some(s) => {
                    if s.kind != "文字列" {
                        self.d(ERROR, "ファイル", &where_, &format!("{} の内容「{}」は {} 型です（文字列だけ書けます）", f.op, f.src, s.kind), 0);
                    } else if f.src == f.dir {
                        self.d(ERROR, "ファイル", &where_, "場所の引数と内容の引数は別にしてください", 0);
                    }
                }
            }
        }
        let rname = match &e.replies {
            None => {
                self.d(ERROR, "ファイル", &where_, "結果を結果の種類に結ぶので、外部作用に replies 応答名 が要ります", 0);
                return;
            }
            Some(r) => r,
        };
        let rp = match reps.get(rname) {
            Some(r) => *r,
            None => return,
        };
        let mut outcomes: IndexMap<&String, Vec<String>> = rp.outcomes.iter().map(|(on, g)| (on, g.clone())).collect();
        let normal = "Normal".to_string();
        if outcomes.is_empty() {
            outcomes.insert(&normal, rp.params.iter().map(|(n, _)| n.clone()).collect());
        }
        let have: IndexMap<&String, &Type> = rp.params.iter().map(|(n, t)| (n, t)).collect();
        for need in ["ok", "failed"] {
            if !f.rows.contains_key(need) {
                self.d(
                    ERROR,
                    "ファイル",
                    &where_,
                    &format!("{} → 種類 が要ります（{}の結果の種類。決めずに黙って捨てない）", need, if need == "ok" { "成功のとき" } else { "そのほかの失敗のとき（書いていない失敗は、これになる）" }),
                    0,
                );
            }
        }
        for (kind, (o, takes)) in f.rows.iter() {
            if kind == "exists" && f.op != "write" {
                self.d(ERROR, "ファイル", &where_, "exists（すでにある）は write のときだけ書けます", 0);
            }
            let gives = match outcomes.get(o) {
                Some(g) => g.clone(),
                None => {
                    self.d(ERROR, "ファイル", &where_, &format!("結果の種類「{}」は、応答 {} にありません", o, rp.name), 0);
                    continue;
                }
            };
            for (n, src) in takes {
                if kind != "ok" {
                    self.d(ERROR, "ファイル", &where_, &format!("take は、ok だけに書けます（{} → {} take {} from {}）", kind, o, n, src), 0);
                } else if (src != "content" && src != "bytes") || (src == "content" && f.op != "read") || f.op == "delete" {
                    self.d(ERROR, "ファイル", &where_, &format!("take {} from {}：取り出せるのは、content（ファイルの中身。read のとき）と bytes（バイト数。write・overwrite・read のとき）です", n, src), 0);
                } else if !gives.contains(n) {
                    self.d(ERROR, "ファイル", &where_, &format!("take「{}」は、結果の種類「{}」の gives にありません", n, o), 0);
                } else if have.get(n).map_or(true, |t| t.kind != if src == "content" { "文字列" } else { "整数" }) {
                    self.d(ERROR, "ファイル", &where_, &format!("take「{}」は、{} を入れるので、応答の中身は{}型にしてください", n, src, if src == "content" { "文字列" } else { "整数" }), 0);
                }
            }
            let took: IndexSet<&String> = takes.iter().map(|(n, _)| n).collect();
            for g in &gives {
                if have.contains_key(g) && !took.contains(g) {
                    self.d(
                        ERROR,
                        "ファイル",
                        &where_,
                        &format!("結果の種類「{}」は {} を gives で持つので、`take {} from content|bytes` で取り出してください（{} の結果の種類に中身が要らないなら、gives のない種類に結んでください）", o, g, g, kind),
                        0,
                    );
                }
            }
        }
    }

    /// over http { … }（外部作用の実通信）
    pub fn check_http(&mut self, e: &EffectDecl, reps: &IndexMap<&String, &ReplyDef>) {
        static URL: OnceLock<Regex> = OnceLock::new();
        static HOST: OnceLock<Regex> = OnceLock::new();
        static PATH: OnceLock<Regex> = OnceLock::new();
        let url_re = rx(r"^https://([A-Za-z0-9.-]+)(:\d{1,5})?(/[^\s?#]*)?$", &URL);
        let host_re = rx(r"^([A-Za-z0-9-]+\.)+[A-Za-z]{2,}$", &HOST);
        let path_re = rx(r"^[A-Za-z0-9_]+(\.[A-Za-z0-9_]+)*$", &PATH);
        let h = e.http.as_ref().unwrap();
        let where_ = format!("外部作用 {} / over http", e.name);
        let m = url_re.captures(&h.url);
        if h.method.is_empty() || h.url.is_empty() {
            self.d(ERROR, "通信", &where_, "METHOD \"https://…\" の行が要ります（例：POST \"https://api.example.com/v1/items\"）", 0);
        } else if m.as_ref().map_or(true, |m| !host_re.is_match(&m[1])) {
            self.d(ERROR, "通信", &where_, &format!("通信先「{}」は、https://ホスト名/パス の形で書きます（http や IP の直書き、ユーザー名、? や # は使えません）", h.url), 0);
        }
        let rname = match &e.replies {
            None => {
                self.d(ERROR, "通信", &where_, "返事を結果の種類に結ぶので、外部作用に replies 応答名 が要ります", 0);
                return;
            }
            Some(r) => r,
        };
        let rp = match reps.get(rname) {
            Some(r) => *r,
            None => return,
        };
        let ns = e.name.split('.').next().unwrap().to_string();
        if let Some(b) = &h.bearer {
            let secrets: IndexSet<&String> = self.p.needs.iter().filter(|n| n.0 == "secret").map(|n| &n.1).collect();
            if b.split('.').next().unwrap() != ns {
                self.d(ERROR, "通信", &where_, &format!("bearer「{}」は、この外部作用と同じ外部（{}）のシークレットだけ使えます", b, ns), 0);
            } else if !secrets.contains(b) {
                self.d(ERROR, "通信", &where_, &format!("bearer「{}」は、needs secret {} \"理由\" で宣言していません", b, b), 0);
            }
        }
        let params: IndexMap<&String, &Type> = e.params.iter().map(|(n, t)| (n, t)).collect();
        let keys: Vec<&String> = h.body.iter().map(|(k, _)| k).collect();
        if uniq(&keys).len() != keys.len() {
            self.d(ERROR, "通信", &where_, "body のキーが重なっています", 0);
        }
        for (_, n) in &h.body {
            match params.get(n) {
                None => self.d(ERROR, "通信", &where_, &format!("body「{}」は、外部作用の引数にありません", n), 0),
                Some(t) => {
                    if !HTTP_BODY_KINDS.contains(&t.kind.as_str()) {
                        self.d(ERROR, "通信", &where_, &format!("body「{}」は {} 型で、本文の JSON にできません（ID・文字列・整数・真偽だけ）", n, t.kind), 0);
                    }
                }
            }
        }
        let mut outcomes: IndexMap<&String, Vec<String>> = rp.outcomes.iter().map(|(on, g)| (on, g.clone())).collect();
        let normal = "Normal".to_string();
        if outcomes.is_empty() {
            outcomes.insert(&normal, rp.params.iter().map(|(n, _)| n.clone()).collect());
        }
        let have: IndexSet<&String> = rp.params.iter().map(|(n, _)| n).collect();
        let mut rows: Vec<(Option<(i128, i128)>, &String, &Vec<(String, String)>)> = h.statuses.iter().map(|(lo, hi, o, tk)| (Some((*lo, *hi)), o, tk)).collect();
        if let Some((o, tk)) = &h.other {
            rows.push((None, o, tk));
        }
        if h.other.is_none() {
            self.d(ERROR, "通信", &where_, "status other → 種類 が要ります（書いていない HTTP の状態が来たときの結果の種類。決めずに黙って捨てない）", 0);
        }
        let mut seen: Vec<(i128, i128)> = vec![];
        for (range, o, tk) in rows {
            if let Some((lo, hi)) = range {
                if !(100 <= lo && lo <= hi && hi <= 599) {
                    self.d(ERROR, "通信", &where_, &format!("status {}..{} は、100〜599 の範囲で小さい方を先に書きます", lo, hi), 0);
                }
                for (a, b) in &seen {
                    if lo <= *b && *a <= hi {
                        self.d(ERROR, "通信", &where_, &format!("status {}..{} は、ほかの status と重なっています", lo, hi), 0);
                    }
                }
                seen.push((lo, hi));
            }
            let gives = match outcomes.get(o) {
                Some(g) => g.clone(),
                None => {
                    self.d(ERROR, "通信", &where_, &format!("結果の種類「{}」は、応答 {} にありません", o, rp.name), 0);
                    continue;
                }
            };
            let took: IndexSet<&String> = tk.iter().map(|(n, _)| n).collect();
            for (n, path) in tk {
                if !gives.contains(n) {
                    self.d(ERROR, "通信", &where_, &format!("take「{}」は、結果の種類「{}」の gives にありません", n, o), 0);
                }
                if !path_re.is_match(path) {
                    self.d(ERROR, "通信", &where_, &format!("道すじ「{}」は、data.id のようにキーを . でつなぎます", path), 0);
                }
            }
            for g in &gives {
                if have.contains(g) && !took.contains(g) {
                    self.d(ERROR, "通信", &where_, &format!("結果の種類「{}」は {} を gives で持つので、`take {} from \"道すじ\"` で返事から取り出してください", o, g, g), 0);
                }
            }
        }
    }

    /// 拡張の画面の read：ページから値を読む取り決め
    pub fn check_read(&mut self, sc: &Screen, where_: &str) {
        let rd = match &sc.read {
            Some(r) => r,
            None => {
                self.d(ERROR, "画面", where_, "page を持つ画面（ブラウザ拡張）には、ページから値を読む read が要ります", 0);
                return;
            }
        };
        if rd.urls.is_empty() && rd.elements.is_empty() {
            self.d(ERROR, "画面", where_, "read に subject（url か element）がありません", 0);
        }
        if rd.value_rows.is_empty() && rd.lookup.is_empty() {
            self.d(ERROR, "画面", where_, "read に value（row か lookup）がありません", 0);
        }
        let mut pats: Vec<(&String, bool)> = rd.urls.iter().map(|u| (u, true)).collect();
        pats.extend(rd.elements.iter().filter(|(_, m)| !m.is_empty()).map(|(_, m)| (m, false)));
        for (pat, need_group) in pats {
            match Regex::new(pat) {
                Err(err) => {
                    let why = py_re_error(pat, &err.to_string());
                    self.d(ERROR, "画面", where_, &format!("read の正規表現「{}」が正しくありません（{}）", pat, why), 0);
                }
                Ok(r) => {
                    if need_group && r.captures_len() < 2 {
                        self.d(ERROR, "画面", where_, &format!("read の url の正規表現「{}」には、値を取り出す ( ) が要ります", pat), 0);
                    }
                }
            }
        }
        let ty = match sc.shows.first().and_then(|s| self.queries.get(s)).and_then(|q| q.inputs.first()) {
            Some((_, t)) => t,
            None => return,
        };
        let part = ty.read.as_ref().and_then(|r| self.parts.get(r).copied());
        let req = if ty.read.is_some() { requires_tree(part) } else { None };
        if let Some(req) = req {
            for (k, v) in rd.lookup.iter() {
                if let Some(why) = refusal(&req, v) {
                    let shown = self.p.show_names(ty.read.as_ref().unwrap());
                    self.d(ERROR, "画面", where_, &format!("read の lookup「{}」の値「{}」は、入力の型の変換 {} が断ります（{}）", k, v, shown, why), 0);
                }
            }
        }
    }

    /// slot の値の決め方（rule）
    pub fn check_slot_rules(&mut self) {
        let p = self.p;
        let mut seen: IndexSet<&String> = IndexSet::new();
        for f in &p.rules {
            let where_ = format!("rule {}", f.slot);
            let users: Vec<&Process> = p.processes.iter().filter(|x| x.refs.iter().any(|(n, _)| *n == f.slot)).collect();
            if seen.contains(&f.slot) {
                self.d(ERROR, "rule", &where_, "同じ slot の rule が2つあります", 0);
            }
            seen.insert(&f.slot);
            if users.is_empty() {
                self.d(ERROR, "rule", &where_, "この名前の slot を持つ処理がありません", 0);
                continue;
            }
            let kinds: IndexSet<&String> = users.iter().flat_map(|x| x.refs.iter().filter(|(n, _)| *n == f.slot).map(|(_, t)| &t.kind)).collect();
            if kinds.len() > 1 {
                self.d(ERROR, "rule", &where_, "この slot は、処理によって型が違うので、1つの表では決められません", 0);
                continue;
            }
            let kind = kinds.iter().next().unwrap().to_string();
            if !["割合", "金額", "整数"].contains(&kind.as_str()) {
                self.d(ERROR, "rule", &where_, &format!("{} 型の slot は、rule で決められません（割合・金額・整数だけです。通貨や状態は、画面が選ばせます）", kind), 0);
            }
            if f.rows.is_empty() || !f.rows.last().unwrap().0.is_empty() {
                self.d(ERROR, "rule", &where_, "最後の行は default にしてください（どれにも当てはまらない場合の値が決まりません）", 0);
            }
            for (i, (conds, val)) in f.rows.iter().enumerate() {
                let sorted = |c: &Vec<(String, String)>| {
                    let mut c = c.clone();
                    c.sort();
                    c
                };
                if !conds.is_empty() && f.rows[..i].iter().any(|(c, _)| sorted(c) == sorted(conds)) {
                    let text = conds.iter().map(|(a, b)| format!("{} = {}", a, b)).collect::<Vec<_>>().join(" かつ ");
                    self.d(ERROR, "rule", &where_, &format!("同じ条件の行が 2 つあります（{}）。上から順に当てはめるので、後ろの行には決して届きません", text), 0);
                }
                if conds.is_empty() && i != f.rows.len() - 1 {
                    self.d(ERROR, "rule", &where_, "default より後ろの行には、決して届きません", 0);
                }
                let is_str = matches!(val, IntOrStr::Str(_));
                if (kind == "割合") != is_str {
                    self.d(ERROR, "rule", &where_, &format!("値が{}の書き方ではありません（割合は 分子/分母、金額と整数は数）", kind), 0);
                } else if let IntOrStr::Str(s) = val {
                    let mut it = s.splitn(2, '/');
                    let num: i128 = it.next().unwrap_or("0").parse().unwrap_or(0);
                    let den: i128 = it.next().unwrap_or("0").parse().unwrap_or(0);
                    if !(0 < den && den <= RATIO_DEN_MAX) {
                        self.d(ERROR, "rule", &where_, &format!("割合の分母は 1〜{} にしてください", RATIO_DEN_MAX), 0);
                    } else if num.abs() > RATIO_VAL_MAX * den {
                        self.d(ERROR, "rule", &where_, &format!("割合の値は ±{} までにしてください", RATIO_VAL_MAX), 0);
                    }
                }
                for (path, _) in conds {
                    let head = path.split('.').next().unwrap();
                    for x in &users {
                        let names: Vec<&String> = x.inputs.iter().chain(x.refs.iter()).map(|(n, _)| n).collect();
                        if head == f.slot || !names.iter().any(|n| n.as_str() == head) {
                            self.d(ERROR, "rule", &where_, &format!("条件の「{}」は、処理 {} の入力か、ほかの slot の名前で始めてください", path, x.name), 0);
                        }
                    }
                }
            }
        }
    }

    pub fn check_triggers(&mut self) {
        self.check_replies();
        self.check_slot_rules();
        let p = self.p;
        let mut used: IndexSet<String> = IndexSet::new();
        for tg in &p.triggers {
            for (k, key, _) in &tg.causes {
                if k == "all" {
                    used.extend(key.split(',').map(|s| s.to_string()));
                } else if k == "reply" {
                    used.insert(key.clone());
                }
            }
        }
        for rp in &p.replies {
            if !used.contains(&rp.name) {
                self.d(WARN, "トリガ", &format!("応答 {}", rp.name), "どのトリガの要因にも使われていません", 0);
            }
        }
        let mut waited: IndexSet<String> = IndexSet::new();
        for fl in &p.flows {
            for (_, routes) in &fl.entries {
                for (_, d) in routes {
                    if d.starts_with("wait ") {
                        waited.insert(d.splitn(2, ' ').nth(1).unwrap().to_string());
                    }
                }
            }
        }
        let names: Vec<&String> = p.triggers.iter().map(|t| &t.name).collect();
        for tg in &p.triggers {
            let where_ = format!("トリガ {}", tg.name);
            if names.iter().filter(|n| **n == &tg.name).count() > 1 {
                self.d(ERROR, "トリガ", &where_, "同じ名前のトリガが複数あります", 0);
            }
            let deadlines: Vec<&String> = tg.causes.iter().filter(|(k, _, _)| k == "deadline").map(|(_, key, _)| key).collect();
            if deadlines.len() > 1 {
                self.d(ERROR, "トリガ", &where_, "期限が複数あります（先に起きた方が勝つので、長い方は起きません）", 0);
            }
            if deadlines.iter().any(|x| x.parse::<i128>().unwrap_or(0) <= 0) {
                self.d(ERROR, "トリガ", &where_, "期限は0より長くしてください", 0);
            }
            if !tg.causes.iter().any(|(k, _, _)| k == "deadline") {
                self.d(ERROR, "トリガ", &where_, "期限がありません（待ち続けないよう、期限と期限切れの行き先が必要です）", 0);
            }
            if !tg.causes.iter().any(|(k, _, _)| k == "reply" || k == "all") {
                self.d(WARN, "トリガ", &where_, "解く応答がありません（期限切れでしか終わらない待ちです）", 0);
            }
            let singles: IndexSet<&String> = tg.causes.iter().filter(|(k, _, _)| k == "reply").map(|(_, key, _)| key).collect();
            let mut in_all: IndexSet<String> = IndexSet::new();
            for (k, key, _) in &tg.causes {
                if k != "all" {
                    continue;
                }
                let ns: Vec<String> = key.split(',').map(|s| s.to_string()).collect();
                if ns.len() < 2 || uniq(&ns).len() != ns.len() {
                    self.d(ERROR, "トリガ", &where_, &format!("all の応答は、重ならない2つ以上を並べてください（{}）。1つだけなら、all を付けない reply の行にします", key), 0);
                }
                for n in &ns {
                    if singles.contains(n) {
                        self.d(ERROR, "トリガ", &where_, &format!("応答「{}」が、all の行と単独の reply の行の両方にあります（先に来た1つで進むのか、全部待つのか、決まりません）", n), 0);
                    } else if in_all.contains(n) {
                        self.d(ERROR, "トリガ", &where_, &format!("応答「{}」が、複数の all の行にあります（どの待ち合わせに数えるのか、決まりません）", n), 0);
                    }
                    in_all.insert(n.clone());
                }
            }
            let mut seen: Vec<(String, String)> = vec![];
            for (k, key, dest) in &tg.causes {
                let skey = if k == "all" {
                    let mut v: Vec<&str> = key.split(',').collect();
                    v.sort();
                    v.join(",")
                } else {
                    key.clone()
                };
                if seen.contains(&(k.clone(), skey.clone())) {
                    self.d(ERROR, "トリガ", &where_, &format!("要因「{} {}」が重複しています", k, skey), 0);
                }
                seen.push((k.clone(), skey));
                match self.procs.get(dest).copied() {
                    None => self.d(ERROR, "トリガ", &where_, &format!("要因「{} {}」の行き先「{}」が未定義の処理です", k, key, dest), 0),
                    Some(proc) => {
                        let want: Vec<(String, String)> = self.cause_args(tg, k, key, &where_).iter().map(|(n, t)| (n.clone(), show_type(t).unwrap_or_default())).collect();
                        let have: Vec<(String, String)> = proc.inputs.iter().map(|(n, t)| (n.clone(), show_type(t).unwrap_or_default())).collect();
                        let bad = if want.len() == tg.params.len() {
                            have != want
                        } else {
                            let (mut a, mut b) = (have.clone(), want.clone());
                            a.sort();
                            b.sort();
                            a != b
                        };
                        if bad {
                            let extra = if want.len() == tg.params.len() { "" } else { "と応答の中身" };
                            self.d(ERROR, "トリガ", &where_, &format!("行き先の処理 {} の入力（名前と型）が、トリガの引数{}と合いません", dest, extra), 0);
                        }
                    }
                }
            }
            if !waited.contains(&tg.name) {
                self.d(WARN, "トリガ", &where_, "どのネクサスからも「wait」で作られていません", 0);
            }
        }
    }

    /// 出来事の検査
    pub fn check_events(&mut self) {
        let p = self.p;
        let names: Vec<&String> = p.events.iter().map(|e| &e.name).collect();
        for ev in &p.events {
            let where_ = format!("出来事 {}", ev.name);
            if names.iter().filter(|n| **n == &ev.name).count() > 1 {
                self.d(ERROR, "名前", &where_, "同じ名前の出来事が複数あります", 0);
            }
            if ev.steps.is_empty() {
                self.d(WARN, "出来事", &where_, "起こす処理がありません", 0);
            }
            for st in uniq(&ev.steps.iter().filter(|x| ev.steps.iter().filter(|y| y == x).count() > 1).cloned().collect::<Vec<_>>()) {
                self.d(ERROR, "出来事", &where_, &format!("同じ行が 2 つあります（処理 {}）。同じ処理が 2 回起こります。1 行にしてください", st.2), 0);
            }
            for (kind, qn, pn) in &ev.steps {
                let proc = match self.procs.get(pn).copied() {
                    Some(x) => x,
                    None => {
                        let msg = format!("起こす処理「{}」が未定義です{}", pn, near(pn, self.procs.keys()));
                        self.d(ERROR, "出来事", &where_, &msg, 0);
                        continue;
                    }
                };
                if kind != "各" {
                    continue;
                }
                let q = match self.queries.get(qn).copied() {
                    Some(q) => q,
                    None => {
                        let msg = format!("問い合わせ「{}」が未定義です{}", qn, near(qn, self.queries.keys()));
                        self.d(ERROR, "出来事", &where_, &msg, 0);
                        continue;
                    }
                };
                if !q.select.is_empty() {
                    self.d(ERROR, "出来事", &where_, &format!("問い合わせ {} は select で列を返すので、出来事の「each」には使えません（IDの列を返す問い合わせにしてください）", qn), 0);
                    continue;
                }
                if uses_caller(p, q) {
                    self.d(ERROR, "出来事", &where_, &format!("問い合わせ {} は、本人で絞る範囲を使うので、出来事からは呼べません（出来事には、呼んだ本人がいません）", qn), 0);
                }
                if q.inputs.iter().any(|(_, t)| !t.optional) {
                    self.d(ERROR, "出来事", &where_, &format!("問い合わせ {} は入力が要るので、出来事からは呼べません（任意の入力だけにしてください）", qn), 0);
                }
                let dt = match self.data.get(&q.target).copied() {
                    Some(d) => d,
                    None => continue,
                };
                let key = match self.key_field(&dt.name) {
                    Some(k) => k,
                    None => continue,
                };
                let have: Vec<(String, String)> = proc.inputs.iter().map(|(n, t)| (n.clone(), show_type(t).unwrap_or_default())).collect();
                if have != vec![(key.name.clone(), "ID".to_string())] {
                    self.d(ERROR, "出来事", &where_, &format!("「each {}」は {} の {} だけを渡すので、処理 {} の入力は「{}: ID」だけにしてください", qn, dt.name, key.name, pn, key.name), 0);
                }
            }
        }
    }

    /// store 名 in 保存先 の保存先は、storage で定義する
    pub fn check_isolations(&mut self) {
        let p = self.p;
        let mut used: IndexMap<&String, &Data> = IndexMap::new();
        for d in &p.data {
            if let Some(s) = &d.store {
                if !s.is_empty() {
                    used.insert(s, d);
                }
            }
        }
        for (where_, d) in &used {
            if !p.isolations.contains_key(*where_) {
                self.diags.push(Diag::plain(
                    ERROR,
                    "直列",
                    &format!("保存先 {}", where_),
                    &format!("保存先「{}」が定義されていません。`storage {} isolation process`（geas のサーバーがプロセスの中で直列にする）か `storage {} isolation store(種類)`（保存先が直列にする）を書いてください", where_, where_, where_),
                    d.line,
                ));
            }
        }
        for (where_, (who, kind, why, line)) in p.isolations.iter() {
            if !used.contains_key(where_) {
                self.diags.push(Diag::plain(ERROR, "直列", &format!("保存先 {}", where_), &format!("storage「{}」は、どの entity の store にも使われていません", where_), *line));
            } else if who == "store" && !VERIFIED_STORES.contains(&kind.as_str()) && why.trim().is_empty() {
                self.diags.push(Diag::plain(
                    ERROR,
                    "直列",
                    &format!("保存先 {}", where_),
                    &format!("保存先の種類「{}」は geas が確かめていないので、人の保証として理由を \"…\" で書いてください（確かめてある種類：{}）", kind, VERIFIED_STORES.join("・")),
                    *line,
                ));
            }
        }
    }

    /// resident（常駐）
    pub fn check_resident(&mut self) {
        let p = self.p;
        let r = match &p.resident {
            Some(r) => r,
            None => return,
        };
        let scr: IndexMap<&String, &Screen> = p.screens.iter().map(|s| (&s.name, s)).collect();
        let evs: IndexMap<&String, &Event> = p.events.iter().map(|e| (&e.name, e)).collect();
        let first = p.screens.first().map(|s| s.name.clone()).unwrap_or_default();
        if p.screens.is_empty() {
            self.d(ERROR, "常駐", "resident", "常駐の窓が開く画面（screen）がありません", 0);
        }
        if r.tooltip.chars().count() > 64 {
            self.d(ERROR, "常駐", "resident / tooltip", "tooltip は 64 文字までです（アイコンに載せる短い説明）", 0);
        }
        let items: Vec<&(String, String, String, i64)> = r.menu.iter().filter(|m| m.1 != "separator").collect();
        let labels: Vec<&String> = items.iter().map(|m| &m.0).collect();
        if uniq(&labels).len() != labels.len() {
            self.d(ERROR, "常駐", "resident / menu", "menu のラベルが重なっています", 0);
        }
        if !r.menu.is_empty() && (r.menu[0].1 == "separator" || r.menu.last().unwrap().1 == "separator") {
            self.d(ERROR, "常駐", "resident / menu", "separator を、メニューの先頭や末尾には置けません（「開く」「終了」の前後にも、言語が区切りを入れます）", 0);
        }
        if r.menu.windows(2).any(|w| w[0].1 == "separator" && w[1].1 == "separator") {
            self.d(ERROR, "常駐", "resident / menu", "separator が続いています", 0);
        }
        for (label, kind, target, line) in items {
            let where_ = format!("resident / menu「{}」", label);
            let n = label.chars().count();
            if label.trim().is_empty() || n > 40 {
                self.d(ERROR, "常駐", &where_, "menu のラベルは 1〜40 文字で書きます", *line);
            }
            if kind == "screen" {
                match scr.get(target) {
                    None => self.d(ERROR, "常駐", &where_, &format!("画面「{}」がありません", target), *line),
                    Some(s) => {
                        if !(s.entry || *target == first) || !s.params.is_empty() {
                            self.d(ERROR, "常駐", &where_, &format!("画面「{}」は開けません（メニューが開けるのは、入口の画面（entry か最初の画面）で、引数を持たないものだけです）", target), *line);
                        }
                    }
                }
            } else {
                match evs.get(target) {
                    None => self.d(ERROR, "常駐", &where_, &format!("出来事「{}」がありません（fire のあとには、event で宣言した名前を書きます）", target), *line),
                    Some(e) => {
                        if e.steps.iter().any(|(k, _, _)| k != "各") {
                            self.d(ERROR, "常駐", &where_, &format!("出来事「{}」は、外から入力を受ける（external）ので、メニューからは起こせません（each だけの出来事を起こせます）", target), *line);
                        }
                    }
                }
            }
        }
        if labels.iter().any(|l| l.trim() == "終了" || l.trim() == "開く") {
            self.d(ERROR, "常駐", "resident / menu", "「開く」（窓を出す）と「終了」の項目は、言語が常に足します。書かないでください", 0);
        }
    }

    /// path（パスの値）と data_folder の検査
    pub fn check_data_folders(&mut self) {
        static WIN: OnceLock<Regex> = OnceLock::new();
        static UNIX: OnceLock<Regex> = OnceLock::new();
        static DRIVE: OnceLock<Regex> = OnceLock::new();
        static WINHEAD: OnceLock<Regex> = OnceLock::new();
        static VARNAME: OnceLock<Regex> = OnceLock::new();
        let win_re = rx(r"%([^%]*)%", &WIN);
        let unix_re = rx(r"\$\{([^}]*)\}|\$([A-Za-z_][A-Za-z0-9_]*)", &UNIX);
        let drive_re = rx(r"^[A-Za-z]:[\\/]", &DRIVE);
        let winhead_re = rx(r"^%[^%]+%", &WINHEAD);
        let varname_re = rx(r"^[A-Za-z_][A-Za-z0-9_]*$", &VARNAME);
        let p = self.p;
        let declared: IndexSet<String> = p.needs.iter().filter(|n| n.0 == "env").map(|n| n.1.to_uppercase()).collect();
        let mut flat: Vec<(String, String, String, i64)> = vec![];
        for (pname, pkind, entries, pline) in &p.paths {
            if pkind == "absolute" && entries.is_empty() {
                self.diags.push(Diag::plain(ERROR, "データ用フォルダ", &format!("path {}", pname), "absolute の中に、OS の行がありません", *pline));
            }
            let mut seen: IndexSet<&String> = IndexSet::new();
            for (os_name, path, line) in entries {
                if seen.contains(os_name) {
                    self.diags.push(Diag::plain(ERROR, "データ用フォルダ", &format!("path {}", pname), &format!("{} の行が2つ以上あります", os_name), *line));
                }
                seen.insert(os_name);
                flat.push((pname.clone(), os_name.clone(), path.clone(), *line));
            }
        }
        for (pname, os_name, path, line) in flat {
            let where_ = format!("path {}", pname) + &if os_name.is_empty() { String::new() } else { format!(" / {}", os_name) };
            let push = |s: &mut Self, msg: &str| s.diags.push(Diag::plain(ERROR, "データ用フォルダ", &where_, msg, line));
            if path.trim().is_empty() || path.contains('\n') || path.contains('\0') {
                push(self, "パスが空か、改行・NUL を含んでいます");
                continue;
            }
            let win_var: Vec<String> = win_re.captures_iter(&path).map(|c| c[1].to_string()).collect();
            let unix_var: Vec<(String, String)> = unix_re.captures_iter(&path).map(|c| (c.get(1).map_or("", |m| m.as_str()).to_string(), c.get(2).map_or("", |m| m.as_str()).to_string())).collect();
            let drive = drive_re.is_match(&path);
            if os_name.is_empty() {
                let first = path.chars().next().unwrap();
                let absolute = "/\\~%$".contains(first) || drive;
                if absolute || !win_var.is_empty() || !unix_var.is_empty() {
                    push(self, "relative のパスは、相対パス（exe のあるフォルダが起点）だけです。絶対パスや環境変数は、`absolute { windows \"…\" … }` で、その OS の書き方で書きます");
                } else if path.contains('\\') || path.split('/').any(|s| s == "..") {
                    push(self, "相対パスは `/` 区切りで書きます（`\\` と `..` は使えません。exe のあるフォルダの外へ出られないようにするため）");
                }
                continue;
            }
            let sure = sure_env(&os_name);
            let names: Vec<String>;
            if os_name == "windows" {
                let bad_unix = path.contains("${") || path.starts_with('~') || (path.starts_with('/') && !path.starts_with("//")) || (!unix_var.is_empty() && win_var.is_empty() && path.starts_with('$'));
                if bad_unix {
                    push(self, "windows の行は、Windows のパスの書き方（`%APPDATA%\\名前`、`C:\\…`）で書きます。`$名前`・`~`・`/…` は mac・Linux の書き方です");
                    continue;
                }
                if !(drive || path.starts_with("\\\\") || winhead_re.is_match(&path)) {
                    push(self, "windows の行は絶対パス（`C:\\…`、`\\\\サーバー\\…`、先頭が `%名前%`）で書きます。相対パスは、relative で書きます");
                    continue;
                }
                names = win_var.clone();
            } else {
                if !win_var.is_empty() || drive || path.contains('\\') {
                    push(
                        self,
                        &format!("{} の行は、mac・Linux のパスの書き方（`$名前`、`${{名前}}`、先頭の `~`、`/…`、区切りは `/`）で書きます。`%名前%`・`C:\\`・`\\` は Windows の書き方です", os_name),
                    );
                    continue;
                }
                if !(path.starts_with('/') || path == "~" || path.starts_with("~/") || path.starts_with('$')) {
                    push(self, &format!("{} の行は絶対パス（先頭が `/`、`~`、`$名前`）で書きます。相対パスは、relative で書きます", os_name));
                    continue;
                }
                if path.starts_with('~') && path != "~" && !path.starts_with("~/") {
                    push(self, "`~` は、自分のホームだけです（`~名前` は使えません）");
                    continue;
                }
                names = unix_var.iter().map(|(a, b)| if a.is_empty() { b.clone() } else { a.clone() }).collect();
            }
            if os_name == "windows" && path.matches('%').count() % 2 == 1 {
                push(self, "% が閉じていません（`%名前%` の形で書きます）");
            }
            for nm in &names {
                let key = if os_name == "windows" { nm.to_uppercase() } else { nm.clone() };
                if !varname_re.is_match(nm) {
                    push(self, &format!("環境変数の名前「{}」が書けません（半角英数と _ だけ）", nm));
                } else if !sure.contains(&key.as_str()) && !declared.contains(&key.to_uppercase()) {
                    push(
                        self,
                        &format!("環境変数 {} は、{} で必ず設定されているとは限りません。`needs env {} \"理由\"` で、アプリが要求することとして宣言してください（未設定のときは、起動時にエラーで止まります）", nm, os_name, nm),
                    );
                }
            }
        }
    }

    /// needs（アプリが環境に要求すること）の検査
    pub fn check_needs(&mut self) {
        static ENV: OnceLock<Regex> = OnceLock::new();
        let env_re = rx(r"^[A-Za-z_][A-Za-z0-9_]*$", &ENV);
        let p = self.p;
        let spaces = effect_namespaces(p);
        let mut seen: IndexSet<String> = IndexSet::new();
        let mut one: IndexMap<String, IntOrStr> = IndexMap::new();
        let mut lines: IndexMap<String, i64> = IndexMap::new();
        for (kind, target, value, _why, line) in &p.needs {
            let where_ = format!("needs {}", kind) + &if target.is_empty() { String::new() } else { format!(" {}", target) };
            let mut push = |s: &mut Self, msg: String| s.diags.push(Diag::plain(ERROR, "環境", &where_, &msg, *line));
            if kind == "secret" {
                let (ns, nm) = match target.split_once('.') {
                    Some((a, b)) => (a, b),
                    None => (target.as_str(), ""),
                };
                if !(ns_ok(ns) && name_ok(nm)) {
                    push(self, "シークレットの名前は `外部.名前`（外部は UpperCamel、名前は lower_snake の半角英数）で書きます".to_string());
                } else if !spaces.iter().any(|x| x == ns) {
                    let tail = if spaces.is_empty() { String::new() } else { format!("。ある外部：{}", spaces.join("・")) };
                    push(self, format!("「{}」という外部作用の名前空間がありません（`effect {}.…` で宣言した外部だけが、秘密を使えます）{}", ns, ns, tail));
                }
                if seen.contains(target) {
                    push(self, format!("シークレット {} の needs が2つ以上あります", target));
                }
                seen.insert(target.clone());
            } else if kind == "env" {
                if !env_re.is_match(target) {
                    push(self, "環境変数の名前は、半角英数と _ で書きます".to_string());
                }
                if seen.contains(&target.to_uppercase()) {
                    push(self, format!("環境変数 {} の needs が2つ以上あります", target));
                }
                seen.insert(target.to_uppercase());
            } else {
                if one.contains_key(kind) {
                    push(self, format!("needs {} は1つだけ書けます", kind));
                }
                one.insert(kind.clone(), value.clone());
                lines.insert(kind.clone(), *line);
            }
        }
        let limit = derived_instance_limit(p);
        if let (Some(limit), Some(IntOrStr::Int(n))) = (limit, one.get("instances")) {
            if *n > limit {
                let procs: Vec<&String> = p.isolations.iter().filter(|(_, (who, _, _, _))| who == "process").map(|(w, _)| w).collect();
                let procs = procs.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("・");
                self.diags.push(Diag::plain(
                    ERROR,
                    "環境",
                    "needs instances",
                    &format!(
                        "at_most {} は、保存先 {} の isolation process の保証（同時に動くサーバーは {} 台）より緩いので書けません。書けるのは、導出した上限より厳しい値だけです。台数を増やすなら、保存先を `isolation store(種類)` にしてください",
                        n, procs, limit
                    ),
                    *lines.get("instances").unwrap_or(&0),
                ));
            }
        }
        if !p.resources.is_empty() && !one.contains_key("exposure") {
            self.diags.push(Diag::plain(
                WARN,
                "環境",
                "needs exposure",
                "公開範囲が書かれていません。`needs exposure public \"理由\"`（インターネットに公開してよい）か `needs exposure internal \"理由\"`（公開しない）を書くと、動かす前提に出ます",
                0,
            ));
        }
    }
}

/// 正規表現の誤りの文言を、Python の re と同じ言い方にする（わかるものだけ。ほかは regex の文言のまま）
fn py_re_error(pat: &str, rust_msg: &str) -> String {
    let chars: Vec<char> = pat.chars().collect();
    let (mut stack, mut i, mut in_class, mut class_at) = (vec![], 0usize, false, 0usize);
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            i += 2;
            continue;
        }
        if in_class {
            if c == ']' && i > class_at + 1 && !(i == class_at + 2 && chars[class_at + 1] == '^') {
                in_class = false;
            }
        } else if c == '[' {
            in_class = true;
            class_at = i;
        } else if c == '(' {
            stack.push(i);
        } else if c == ')' {
            if stack.pop().is_none() {
                return format!("unbalanced parenthesis at position {}", i);
            }
        }
        i += 1;
    }
    if in_class {
        return format!("unterminated character set at position {}", class_at);
    }
    if let Some(p) = stack.last() {
        return format!("missing ), unterminated subpattern at position {}", p);
    }
    rust_msg.lines().last().unwrap_or("").trim().to_string()
}
