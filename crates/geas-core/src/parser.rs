// SPDX-License-Identifier: GPL-3.0-or-later
//! 構文解析（再帰下降）。Python 版 geas/parser.py の移し。文は1行1つ。ブロックは「{ で行が終わり、} だけの行で閉じる」。
//! キーワードは英語。構文木の中身（型の種類や結果の行き先など）は、仕様書の言葉に合わせた日本語のまま持つ。
use crate::ast::*;
use crate::error::{Error, ParseError};
use crate::foreign::{brace_hint, def_hint, foreign_names, shown, stmt_hint, token_hint, top_keywords};
use crate::lexer::{lex, Tok};
use crate::modules::{expand, statement_starts};
use indexmap::IndexMap;
use std::collections::HashSet;
use std::path::Path;

type PResult<T> = Result<T, ParseError>;

/// Python の int(文字列) に当たる。桁あふれは i128 の端に丸める（構文木の数は i128 で持つ）
pub fn parse_int(s: &str) -> i128 {
    s.parse::<i128>().unwrap_or(i128::MAX)
}

/// 説明コメントの表示名。`@ja 注文` のように言語ごとに書く。タグが無ければ、全体を既定（空の言語名）の表示名とする
pub fn parse_labels(doc: &str) -> Map<String> {
    let mut out: Map<String> = Map::default();
    let mut plain: Vec<&str> = vec![];
    for ln in doc.split('\n') {
        if let Some(rest) = ln.strip_prefix('@') {
            let (tag, text) = match rest.find(' ') {
                Some(k) => (&rest[..k], &rest[k + 1..]),
                None => (rest, ""),
            };
            out.insert(tag.to_string(), text.trim().to_string());
        } else if !ln.is_empty() {
            plain.push(ln);
        }
    }
    let mut res: Map<String> = Map::default();
    if !plain.is_empty() {
        res.insert(String::new(), plain.join(" "));
    }
    for (k, v) in out {
        res.insert(k, v);
    }
    res
}

const BUILTIN_TYPE_NAMES: [&str; 10] = ["id", "set", "text", "time", "bytes", "int", "ratio", "money", "ref", "vtime"];

/// 組み込みの仮想日時：年・月・日・時・分・秒の成分を持つ合成型
pub fn virtual_datetime() -> Type {
    let comp = |lo: i128, hi: i128| Type { range: Some(Range { lo: Some(lo), hi: Some(hi) }), optional: true, ..Type::new("整数") };
    Type {
        target: Some("vtime".into()),
        parts: Some(vec![
            ("year".into(), comp(1, 9999)),
            ("month".into(), comp(1, 12)),
            ("day".into(), comp(1, 31)),
            ("hour".into(), comp(0, 23)),
            ("minute".into(), comp(0, 59)),
            ("second".into(), comp(0, 59)),
        ]),
        ..Type::new("合成")
    }
}

const COMPONENT_KINDS: [&str; 9] = ["ID", "整数", "金額", "割合", "文字列", "日時", "バイト列", "真偽", "合成"];
const CMP: [&str; 6] = ["<", ">", "<=", ">=", "==", "!="];
const CALENDAR_UNITS: [&str; 4] = ["day", "week", "month", "year"];
const MAX_DUR: i128 = 10_000_000_000;
const UNIT_WORDS: &str = "sec・min・hour";

fn unit_secs(u: &str) -> Option<i128> {
    match u {
        "sec" => Some(1),
        "min" => Some(60),
        "hour" => Some(3600),
        _ => None,
    }
}

fn cal_msg(u: &str) -> String {
    format!("「{}」は暦で数え方が変わる（夏時間・月の長さ・タイムゾーン）ので、まだ使えません。ちょうど何時間か何秒かなら hour か sec で書いてください（24 hour など）", u)
}

fn simple_type(tok: &str) -> &'static str {
    match tok {
        "text" => "文字列",
        "time" => "日時",
        "bool" => "真偽",
        "bytes" => "バイト列",
        "int" => "整数",
        "ratio" => "割合",
        _ => "金額",
    }
}

fn pred_word(kw: &str) -> Option<&'static str> {
    match kw {
        "empty" => Some("空"),
        "nonempty" => Some("空でない"),
        "present" => Some("ある"),
        "absent" => Some("ない"),
        "null" => Some("null"),
        _ => None,
    }
}
const PRED_ORDER: &str = "empty・nonempty・present・absent・null";

/// 文字種の書き方 "a-z0-9_" を、小さい順につないだ (下限, 上限) の組にする
pub fn parse_chars(s: &str) -> Result<Vec<(u32, u32)>, String> {
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    let mut out: Vec<(u32, u32)> = vec![];
    fn one(cs: &[char], i: &mut usize) -> Result<u32, String> {
        if cs[*i] == '\\' {
            *i += 1;
            if *i >= cs.len() {
                return Err("文字種の末尾の「\\」のあとに文字がありません".into());
            }
        }
        let c = cs[*i] as u32;
        *i += 1;
        Ok(c)
    }
    while i < cs.len() {
        let a = one(&cs, &mut i)?;
        let mut b = a;
        if i + 1 < cs.len() && cs[i] == '-' {
            i += 1;
            b = one(&cs, &mut i)?;
            if b < a {
                return Err(format!("文字種の範囲の向きが逆です（{}-{}）", char::from_u32(a).unwrap(), char::from_u32(b).unwrap()));
            }
        }
        out.push((a, b));
    }
    if out.is_empty() {
        return Err("文字種が空です".into());
    }
    out.sort();
    let mut merged = vec![out[0]];
    for &(a, b) in &out[1..] {
        let last = merged.last_mut().unwrap();
        if a <= last.1 + 1 {
            last.1 = last.1.max(b);
        } else {
            merged.push((a, b));
        }
    }
    Ok(merged)
}

pub struct Parser {
    t: Vec<Tok>,
    i: usize,
    ltypes: IndexMap<String, Type>,
    ltype_lines: IndexMap<String, i64>,
    auto_parts: Vec<Part>,
    in_ltype: bool,
    composites: HashSet<String>,
    used_builtin: HashSet<String>,
}

fn snake_os_path(name: &str) -> String {
    let mut out = String::new();
    for (k, c) in name.chars().enumerate() {
        if k > 0 && c.is_ascii_uppercase() {
            out.push('_');
        }
        out.push(c);
    }
    out.to_lowercase() + "_os_path"
}

impl Parser {
    pub fn new(toks: Vec<Tok>) -> Parser {
        let mut composites = HashSet::new();
        composites.insert("vtime".to_string());
        Parser { t: toks, i: 0, ltypes: IndexMap::new(), ltype_lines: IndexMap::new(), auto_parts: vec![], in_ltype: false, composites, used_builtin: HashSet::new() }
    }

    // ---- 部品 ----
    fn peek_at(&self, k: usize) -> &Tok {
        &self.t[(self.i + k).min(self.t.len() - 1)]
    }
    fn peek(&self) -> &Tok {
        self.peek_at(0)
    }
    fn next(&mut self) -> Tok {
        let tok = self.t[self.i].clone();
        if tok.kind != "EOF" {
            self.i += 1;
        }
        tok
    }
    fn is_k(&self, val: &str, k: usize) -> bool {
        let t = self.peek_at(k);
        (t.kind == "NAME" || t.kind == "OP") && t.val == val
    }
    fn is_(&self, val: &str) -> bool {
        self.is_k(val, 0)
    }
    fn accept(&mut self, val: &str) -> bool {
        if self.is_(val) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn err(&self, msg: impl Into<String>, hint: impl Into<String>) -> ParseError {
        let t = self.peek();
        ParseError::at(msg, t.line, t.col, &t.file, hint)
    }
    fn err0(&self, msg: impl Into<String>) -> ParseError {
        self.err(msg, "")
    }
    /// 思っていない字句があった。他の言語の書き方らしければ、直し方を添える
    fn unexpected(&self, msg: impl Into<String>, hint: &str) -> ParseError {
        let h = if hint.is_empty() { token_hint(&self.t, self.i) } else { hint.to_string() };
        self.err(msg, h)
    }
    fn expect(&mut self, val: &str) -> PResult<()> {
        if !self.accept(val) {
            let t = self.peek();
            let hint = if val == "→" && t.val == ":" { "結果の種類と行き先は → か -> でつなぎます（: ではありません）" } else { "" };
            return Err(self.unexpected(format!("「{}」が必要です（「{}」があります）", val, shown(t)), hint));
        }
        Ok(())
    }
    fn name(&mut self) -> PResult<String> {
        let t = self.peek();
        if t.kind != "NAME" {
            return Err(self.unexpected(format!("名前が必要です（「{}」があります）", shown(t)), ""));
        }
        let v = t.val.clone();
        self.i += 1;
        Ok(v)
    }
    fn num(&mut self) -> PResult<i128> {
        let t = self.peek();
        if t.kind != "NUM" {
            return Err(self.err0("数が必要です"));
        }
        let v = parse_int(&t.val);
        self.i += 1;
        Ok(v)
    }
    fn skip_nl(&mut self) {
        while self.peek().kind == "NL" {
            self.i += 1;
        }
    }
    fn end_line(&mut self) -> PResult<()> {
        if self.peek().kind == "NL" {
            self.skip_nl();
        } else if self.peek().kind != "EOF" {
            return Err(self.unexpected(format!("行末のはずが「{}」があります", shown(self.peek())), ""));
        }
        Ok(())
    }
    fn comma_list<T>(&mut self, mut item: impl FnMut(&mut Self) -> PResult<T>) -> PResult<Vec<T>> {
        let mut out = vec![item(self)?];
        while self.accept(",") {
            out.push(item(self)?);
        }
        Ok(out)
    }
    fn names(&mut self) -> PResult<Vec<String>> {
        self.comma_list(|s| s.name())
    }
    fn name_type(&mut self) -> PResult<(String, Type)> {
        let n = self.name()?;
        self.expect(":")?;
        let t = self.type_()?;
        Ok((n, t))
    }
    fn name_expr(&mut self) -> PResult<(String, Expr)> {
        let n = self.name()?;
        self.expect(":")?;
        let e = self.expr()?;
        Ok((n, e))
    }

    // ---- 全体 ----
    pub fn program(&mut self) -> PResult<Program> {
        let mut p = Program::default();
        let mut stores: Vec<(String, String, i64)> = vec![];
        let mut examples: Vec<(String, Vec<Expr>, Expr, i64)> = vec![];
        self.skip_nl();
        while self.peek().kind != "EOF" {
            let line = self.peek().line;
            let kw = self.name()?;
            match kw.as_str() {
                "entity" => p.data.push(self.data(line)?),
                "data" => return Err(ParseError::new("data は entity に改名しました（entity 名 { … } と書きます）", line)),
                "ltype" => self.ltype(line)?,
                "store" => stores.push(self.store_decl(line)?),
                "state" => p.states.push(self.state(line)?),
                "result" => p.results.push(self.result_def(line)?),
                "effect" => p.effects.push(self.effect_decl(line)?),
                "part" => {
                    return Err(ParseError::new(
                        "part は廃止しました。geas の式で書けるなら convert、根の言語（Rust）のライブラリを呼ぶなら library を使います（例：library tax_included(x: Yen) -> Yen from rust \"std\" \"\" call \"x + x / 10\" guarantee result >= x trust check \"整数の計算だけ\"）",
                        line,
                    ))
                }
                "library" => p.parts.push(self.library(line)?),
                "example" => examples.push(self.example(line)?),
                "convert" => p.parts.push(self.convert(line)?),
                "invariant" => p.invariants.push(self.invariant(line)?),
                "deed" => p.processes.push(self.process(line)?),
                "nexus" | "flow" => p.flows.push(self.flow(line)?),
                "reply" => p.replies.push(self.reply_def(line)?),
                "rule" => p.rules.push(self.slot_rule(line)?),
                "migration" => p.migrations.push(self.migration(line)?),
                "trigger" => p.triggers.push(self.trigger(line)?),
                "query" => p.queries.push(self.query(line)?),
                "event" => p.events.push(self.event(line)?),
                "screen" => p.screens.push(self.screen(line)?),
                "resource" => p.resources.push(self.resource(line)?),
                "scope" => p.scopes.push(self.scope_decl(line)?),
                "actor" => {
                    let n = self.name()?;
                    p.actors.push((n, line));
                    self.end_line()?;
                }
                "path" => {
                    let pd = self.path_decl(line)?;
                    if p.paths.iter().any(|x| x.0 == pd.0) {
                        return Err(ParseError::new(format!("パス「{}」が2つ以上あります", pd.0), line));
                    }
                    p.paths.push(pd);
                }
                "data_folder" => {
                    if p.data_folder.is_some() {
                        return Err(ParseError::new("data_folder は 1 つだけ書けます", line));
                    }
                    let n = self.name()?;
                    p.data_folder = Some((n, line));
                    self.end_line()?;
                }
                "resident" => {
                    if p.resident.is_some() {
                        return Err(ParseError::new("resident は 1 つだけ書けます", line));
                    }
                    p.resident = Some(self.resident_decl(line)?);
                }
                "app" => {
                    let t = self.peek();
                    if t.kind != "STR" || t.val.is_empty() {
                        return Err(ParseError::new("app は `app \"アプリの名前\"` と書きます", line));
                    }
                    p.title = t.val.clone();
                    self.i += 1;
                    self.end_line()?;
                }
                "storage" => {
                    let wh = self.name()?;
                    if p.isolations.contains_key(&wh) {
                        return Err(ParseError::new(format!("保存先「{}」の storage が2つ以上あります", wh), line));
                    }
                    if !self.accept("isolation") {
                        return Err(ParseError::new("storage は `storage 保存先 isolation process` か `storage 保存先 isolation store(種類) \"理由\"` と書きます", line));
                    }
                    let who = self.name()?;
                    if who != "process" && who != "store" {
                        return Err(ParseError::new("isolation のあとには process（このアプリのプロセスの中だけ geas が直列にする）か store(種類)（保存先が直列にする）を書きます", line));
                    }
                    let (mut kind, mut why) = (String::new(), String::new());
                    if who == "store" {
                        self.expect("(")?;
                        kind = self.name()?;
                        self.expect(")")?;
                        if self.peek().kind == "STR" {
                            why = self.next().val;
                        }
                    }
                    p.isolations.insert(wh, (who, kind, why, line));
                    self.end_line()?;
                }
                "needs" => p.needs.push(self.needs_decl(line)?),
                _ => {
                    let t = self.t[self.i - 1].clone();
                    let h = def_hint(&kw);
                    let hint = if h.is_empty() { format!("書ける定義は {} です", top_keywords().join("・")) } else { h };
                    return Err(ParseError::at(format!("未知の定義「{}」です", kw), line, t.col, &t.file, hint));
                }
            }
            self.skip_nl();
        }
        p.ltype_names = self.ltypes.keys().cloned().collect();
        let autos = std::mem::take(&mut self.auto_parts);
        p.parts.extend(autos);
        p.ltype_lines = Map(self.ltype_lines.clone());
        p.composites = self.ltypes.values().filter(|t| t.kind == "合成" && t.target.as_ref().map_or(false, |x| self.composites.contains(x))).cloned().collect();
        if self.used_builtin.contains("vtime") {
            p.composites.insert(0, virtual_datetime());
        }
        if self.used_builtin.contains("currency") {
            p.states.push(State { name: "Currency".into(), initial: "JPY".into(), middle: vec!["USD".into(), "EUR".into()], plain: true, ..Default::default() });
        }
        self.apply_stores(&mut p, &stores)?;
        for (name, args, want, ln) in examples {
            match p.parts.iter_mut().rev().find(|pt| pt.name == name && pt.external()) {
                None => {
                    return Err(ParseError::new(
                        format!("example の対象「{}」という library がありません（具体例を書けるのは library だけです。convert は式そのものが定義です）", name),
                        ln,
                    ))
                }
                Some(pt) => pt.examples.push((args, want, ln)),
            }
        }
        self.add_sync_state(&mut p)?;
        if let Some((name, ln)) = p.data_folder.clone() {
            match p.paths.iter().find(|x| x.0 == name) {
                None => {
                    let tail = if p.paths.is_empty() { String::new() } else { format!("。ある物：{}", p.paths.iter().map(|x| x.0.clone()).collect::<Vec<_>>().join("・")) };
                    return Err(ParseError::new(format!("data_folder の「{}」というパス（path）がありません{}", name, tail), ln));
                }
                Some(hit) => p.data_folders = hit.2.clone(),
            }
        }
        Ok(p)
    }

    fn path_decl(&mut self, line: i64) -> PResult<(String, String, Vec<(String, String, i64)>, i64)> {
        let name = self.name()?;
        self.expect("=")?;
        let kind = self.name()?;
        let mut entries = vec![];
        if kind == "absolute" {
            self.block_open()?;
            while !self.accept("}") {
                let ln = self.peek().line;
                let os_name = self.name()?;
                if !["windows", "macos", "linux"].contains(&os_name.as_str()) {
                    return Err(self.err0("absolute の中は、windows・macos・linux のどれかに続けて、その OS の書き方のパスを書きます（例：windows \"%APPDATA%\\\\アプリ名\"）"));
                }
                let t = self.next();
                if t.kind != "STR" || t.val.trim().is_empty() {
                    return Err(self.err0(format!("{} のあとには、パスを \"…\" で書きます", os_name)));
                }
                entries.push((os_name, t.val, ln));
                self.end_line()?;
            }
            self.end_line()?;
        } else if kind == "relative" {
            let t = self.next();
            if t.kind != "STR" || t.val.trim().is_empty() {
                return Err(self.err0("relative のあとには、パスを \"…\" で書きます（exe のあるフォルダが起点）"));
            }
            entries.push((String::new(), t.val, line));
            self.end_line()?;
        } else {
            return Err(ParseError::new("path は `path 名前 = absolute { windows \"…\" macos \"…\" linux \"…\" }` か `path 名前 = relative \"…\"` と書きます", line));
        }
        Ok((name, kind, entries, line))
    }

    fn needs_decl(&mut self, line: i64) -> PResult<(String, String, NeedVal, String, i64)> {
        let usage = "needs は `needs secret 外部.名前 \"理由\"`、`needs exposure public \"理由\"`（または internal）、`needs instances at_most N \"理由\"`、`needs env 環境変数名 \"理由\"` のどれかで書きます";
        let kind = if self.peek().kind == "NAME" { self.name()? } else { String::new() };
        let mut target = String::new();
        let mut value = NeedVal::Str(String::new());
        match kind.as_str() {
            "secret" => target = self.dotted()?,
            "env" => target = self.name()?,
            "exposure" => {
                let v = self.name()?;
                if v != "public" && v != "internal" {
                    return Err(ParseError::new("exposure のあとには public（インターネットに公開してよい）か internal（公開しない）を書きます", line));
                }
                value = NeedVal::Str(v);
            }
            "instances" => {
                if !self.accept("at_most") {
                    return Err(ParseError::new("instances は `needs instances at_most N \"理由\"` と書きます（同時に動くサーバーの数の上限）", line));
                }
                let n = self.peek();
                if n.kind != "NUM" || !n.val.chars().all(|c| c.is_ascii_digit()) || parse_int(&n.val) < 1 {
                    return Err(ParseError::new("at_most のあとには 1 以上の整数を書きます", line));
                }
                value = NeedVal::Int(parse_int(&n.val));
                self.i += 1;
            }
            _ => return Err(ParseError::new(usage, line)),
        }
        let w = self.peek().clone();
        if w.kind != "STR" || w.val.trim().is_empty() {
            return Err(ParseError::new(format!("needs {} には理由が要ります（行末に \"理由\" を書きます）。{}", kind, usage), line));
        }
        self.i += 1;
        self.end_line()?;
        Ok((kind, target, value, w.val, line))
    }

    fn store_decl(&mut self, line: i64) -> PResult<(String, String, i64)> {
        let data = self.name()?;
        self.expect("in")?;
        let wh = self.name()?;
        self.end_line()?;
        let _ = line;
        Ok((data, wh, line))
    }

    fn apply_stores(&self, p: &mut Program, stores: &[(String, String, i64)]) -> PResult<()> {
        for (data, wh, line) in stores {
            match p.data.iter_mut().rev().find(|d| &d.name == data) {
                None => return Err(ParseError::new(format!("store の対象「{}」という entity がありません", data), *line)),
                Some(d) => {
                    if d.store.as_ref().map_or(false, |s| !s.is_empty()) {
                        return Err(ParseError::new(format!("entity「{}」の store が2つ以上あります", data), *line));
                    }
                    d.store = Some(wh.clone());
                }
            }
        }
        Ok(())
    }

    fn add_sync_state(&self, p: &mut Program) -> PResult<()> {
        let ext: Vec<usize> = p.data.iter().enumerate().filter(|(_, d)| d.store.as_ref().map_or(false, |s| !s.is_empty())).map(|(i, _)| i).collect();
        if ext.is_empty() {
            return Ok(());
        }
        for &i in &ext {
            let d = &mut p.data[i];
            if d.fields.iter().any(|f| f.name == "記録") {
                return Err(ParseError::new(format!("データ {} の「記録」は、保存先の宣言から言語が足します。書かないでください", d.name), d.line));
            }
            d.fields.push(Field { name: "記録".into(), r#type: Type { target: Some("記録状態".into()), ..Type::new("名前") }, ..Default::default() });
        }
        let first_line = p.data[ext[0]].line;
        if p.states.iter().any(|s| s.name == "記録状態") {
            return Err(ParseError::new("状態「記録状態」は、保存先の宣言から言語が足します。書かないでください", first_line));
        }
        p.states.push(State { name: "記録状態".into(), initial: "端末保存済".into(), middle: vec!["記録済".into()], endless: true, line: first_line, ..Default::default() });
        Ok(())
    }

    fn block_open(&mut self) -> PResult<()> {
        self.expect("{")?;
        self.end_line()
    }

    // ---- 論理型 ----
    fn ltype(&mut self, line: i64) -> PResult<()> {
        let name = self.name()?;
        if BUILTIN_TYPE_NAMES.contains(&name.as_str()) {
            return Err(ParseError::new(format!("「{}」は組み込みの型の名前なので、論理型の名前には使えません", name), line));
        }
        if self.ltypes.contains_key(&name) {
            return Err(ParseError::new(format!("論理型「{}」が2つ以上あります", name), line));
        }
        if self.is_("{") {
            let t = self.composite(&name, line)?;
            self.ltypes.insert(name.clone(), t);
            self.ltype_lines.insert(name.clone(), line);
            self.composites.insert(name);
            return Ok(());
        }
        self.expect("=")?;
        if self.peek().kind == "NAME" && self.peek().val == "path" {
            self.next();
            self.end_line()?;
            let reader = snake_os_path(&name);
            let mut t = Type { range: Some(Range { lo: Some(1), hi: Some(4096) }), ..Type::new("文字列") };
            t.read = Some(reader.clone());
            self.auto_parts.push(Part {
                name: reader.clone(),
                params: vec![("r".into(), Type::new("生"))],
                ret: Type { range: Some(Range { lo: Some(1), hi: Some(4096) }), read: Some(reader.clone()), ..Type::new("文字列") },
                guarantee: None,
                line,
                body: Some(Expr::Call { n: "os_path".into(), args: vec![Expr::name("r")] }),
                ..Default::default()
            });
            self.ltypes.insert(name.clone(), t);
            self.ltype_lines.insert(name, line);
            return Ok(());
        }
        self.in_ltype = true;
        let t = self.type_();
        self.in_ltype = false;
        let mut t = t?;
        if self.accept("read") {
            if t.kind != "文字列" {
                return Err(ParseError::new("read を結べるのは、いまは文字列（text）の型だけです", line));
            }
            t.read = Some(self.name()?);
        }
        self.end_line()?;
        self.ltypes.insert(name.clone(), t);
        self.ltype_lines.insert(name, line);
        Ok(())
    }

    fn composite(&mut self, name: &str, line: i64) -> PResult<Type> {
        self.expect("{")?;
        let mut parts: Vec<(String, Type)> = vec![];
        let mut one = |s: &mut Parser| -> PResult<()> {
            let cl = s.peek().line;
            let pn = s.name()?;
            s.expect(":")?;
            let mut pt = s.type_()?;
            if s.accept("optional") {
                pt.optional = true;
            }
            if !COMPONENT_KINDS.contains(&pt.kind.as_str()) {
                return Err(ParseError::new(format!("合成型「{}」の成分「{}」に、この型は使えません（使えるのは値の型：{}）", name, pn, COMPONENT_KINDS.join("・")), cl));
            }
            if parts.iter().any(|(x, _)| x == &pn) {
                return Err(ParseError::new(format!("合成型「{}」に成分「{}」が2つ以上あります", name, pn), cl));
            }
            parts.push((pn, pt));
            Ok(())
        };
        if self.peek().kind == "NL" {
            self.end_line()?;
            while !self.accept("}") {
                one(self)?;
                self.end_line()?;
            }
        } else {
            self.comma_list(|s| one(s))?;
            self.expect("}")?;
            self.end_line()?;
        }
        if parts.is_empty() {
            return Err(ParseError::new(format!("合成型「{}」に成分がありません", name), line));
        }
        Ok(Type { target: Some(name.to_string()), parts: Some(parts), ..Type::new("合成") })
    }

    // ---- データ ----
    fn data(&mut self, line: i64) -> PResult<Data> {
        let name = self.name()?;
        if self.is_("store") {
            return Err(self.err0(format!("保存先は entity の中ではなく、別の行で store {} in 保存先 と書きます", name)));
        }
        self.block_open()?;
        let (mut fields, mut derived) = (vec![], vec![]);
        let (mut keep, mut removable, mut on_delete) = (false, vec![], vec![]);
        while !self.accept("}") {
            if self.is_("keep") && !self.is_k(":", 1) {
                self.next();
                keep = true;
                self.end_line()?;
                continue;
            }
            if self.is_("removable") && !self.is_k(":", 1) {
                self.next();
                self.expect("in")?;
                removable.extend(self.names()?);
                self.end_line()?;
                continue;
            }
            if self.is_("on") && self.is_k("delete", 1) && !self.is_k(":", 1) {
                self.next();
                self.next();
                let src = self.name()?;
                self.expect(".")?;
                let fld = self.name()?;
                self.expect("→")?;
                let pol = self.name()?;
                if !["detach", "cascade", "leave"].contains(&pol.as_str()) {
                    return Err(self.err0("on delete の扱いは「detach」（参照を空にする）「cascade」（参照元も消す）「leave」（参照元はそのまま残す）です"));
                }
                on_delete.push((src, fld, pol));
                self.end_line()?;
                continue;
            }
            let fline = self.peek().line;
            let fname = self.name()?;
            self.expect(":")?;
            let t = self.type_()?;
            let mut f = Field { name: fname, r#type: t, line: fline, ..Default::default() };
            while self.peek().kind == "NAME" && ["unique", "optional", "required"].contains(&self.peek().val.as_str()) {
                let m = self.name()?;
                match m.as_str() {
                    "unique" => f.unique = true,
                    "required" => f.required = true,
                    _ => return Err(self.err0("項目は、書かなければ「無いかもしれない」（null になりうる）扱いです。optional は要りません（必須なら required と書きます）")),
                }
            }
            if self.accept("=") {
                f.expr = Some(self.expr()?);
                derived.push(f);
            } else {
                fields.push(f);
            }
            self.end_line()?;
        }
        Ok(Data { name, fields, line, keep, removable, on_delete, derived, store: None })
    }

    fn signed_num(&mut self) -> PResult<i128> {
        if self.accept("-") {
            return Ok(-self.num()?);
        }
        self.num()
    }

    fn range_opt(&mut self) -> PResult<Option<Range>> {
        if self.peek().kind == "NUM" || self.is_("-") {
            let lo = self.signed_num()?;
            self.expect("..")?;
            let hi = if self.peek().kind == "NUM" || self.is_("-") { Some(self.signed_num()?) } else { None };
            return Ok(Some(Range { lo: Some(lo), hi }));
        }
        if self.is_("..") {
            self.next();
            return Ok(Some(Range { lo: None, hi: Some(self.signed_num()?) }));
        }
        Ok(None)
    }

    fn chars_opt(&mut self) -> PResult<Option<Vec<(u32, u32)>>> {
        if !self.accept("of") {
            return Ok(None);
        }
        let t = self.next();
        if t.kind != "STR" || t.val.is_empty() {
            return Err(self.err0("of のあとには、使える文字を \"a-z0-9_\" の形で書きます"));
        }
        match parse_chars(&t.val) {
            Ok(c) => Ok(Some(c)),
            Err(e) => Err(self.err0(e)),
        }
    }

    fn check_opt(&self) -> PResult<()> {
        if self.is_("check") {
            return Err(self.err0("check gtin は無くなりました。検査数字は、標準モジュール Gtin（geas に同梱）で確かめます：use \"gtin.geas\" と書き、型は Gtin.Gtin13（8・12・14 桁は Gtin.Gtin8・Gtin.Gtin12・Gtin.Gtin14）を使います"));
        }
        Ok(())
    }

    fn type_(&mut self) -> PResult<Type> {
        let line = self.peek().line;
        let named = self.ltypes.contains_key(&self.peek().val) && !self.is_k("→", 1);
        let t = self.type_inner()?;
        if !(self.in_ltype || named) && (t.kind == "写像" || t.kind == "集合" || t.range.is_some() || t.chars.is_some()) {
            return Err(ParseError::new("範囲や組み合わせは、ltype で名前を付けてから使ってください（例：ltype 数量 = int 1..99）", line));
        }
        Ok(t)
    }

    fn max_opt(&mut self) -> PResult<Option<i128>> {
        if !self.accept("max") {
            return Ok(None);
        }
        let n = self.num()?;
        if n < 1 {
            return Err(self.err0("max のあとは 1 以上の整数です（例：Product → Quantity max 100）"));
        }
        Ok(Some(n))
    }

    fn type_inner(&mut self) -> PResult<Type> {
        let tok = self.name()?;
        if tok == "id" {
            if self.accept("→") {
                let v = self.type_()?;
                let m = self.max_opt()?;
                return Ok(Type { key: Some("id".into()), value: Some(Box::new(v)), maxn: m, ..Type::new("写像") });
            }
            return Ok(Type::new("ID"));
        }
        if tok == "set" {
            self.expect("of")?;
            self.expect("id")?;
            let m = self.max_opt()?;
            return Ok(Type { value: Some(Box::new(Type::new("ID"))), maxn: m, ..Type::new("集合") });
        }
        if tok == "text" {
            let rng = self.range_opt()?;
            let chars = self.chars_opt()?;
            self.check_opt()?;
            return Ok(Type { range: rng, chars, ..Type::new("文字列") });
        }
        if tok == "time" || tok == "bool" {
            return Ok(Type::new(simple_type(&tok)));
        }
        if tok == "raw" {
            return Ok(Type::new("生"));
        }
        if tok == "bytes" {
            return Ok(Type { range: self.range_opt()?, ..Type::new("バイト列") });
        }
        if tok == "int" || tok == "ratio" {
            return Ok(Type { range: self.range_opt()?, ..Type::new(simple_type(&tok)) });
        }
        if tok == "money" {
            let mut unit = "JPY".to_string();
            if self.accept("(") {
                unit = self.name()?;
                if unit.chars().next().map_or(false, |c| c.is_lowercase()) {
                    unit = format!("${}", unit);
                }
                self.expect(")")?;
            }
            return Ok(Type { range: self.range_opt()?, unit: Some(unit), ..Type::new("金額") });
        }
        if tok == "currency" {
            self.used_builtin.insert(tok);
            return Ok(Type { target: Some("Currency".into()), ..Type::new("名前") });
        }
        if tok == "ref" {
            return Ok(Type { target: Some(self.name()?), ..Type::new("参照") });
        }
        if self.accept("→") {
            let v = self.type_()?;
            let m = self.max_opt()?;
            return Ok(Type { key: Some(tok), value: Some(Box::new(v)), maxn: m, ..Type::new("写像") });
        }
        if tok == "vtime" {
            self.used_builtin.insert(tok);
            return Ok(virtual_datetime());
        }
        if let Some(t) = self.ltypes.get(&tok) {
            return Ok(t.clone());
        }
        Ok(Type { target: Some(tok), ..Type::new("名前") })
    }

    // ---- 状態 ----
    fn state(&mut self, line: i64) -> PResult<State> {
        let mut s = State { name: self.name()?, line, ..Default::default() };
        self.block_open()?;
        while !self.accept("}") {
            let kw = self.name()?;
            match kw.as_str() {
                "initial" => s.initial = self.name()?,
                "middle" => s.middle.extend(self.names()?),
                "final" => s.final_.extend(self.names()?),
                "endless" => s.endless = true,
                "reject" => {
                    let a = self.name()?;
                    self.expect("→")?;
                    let b = self.name()?;
                    self.expect("reason")?;
                    let t = self.next();
                    s.rejects.push((a, b, t.val));
                }
                _ => return Err(self.err0(format!("state の中に「{}」は書けません", kw))),
            }
            self.end_line()?;
        }
        Ok(s)
    }

    fn named_type(&mut self) -> PResult<(String, Type)> {
        let n = self.name()?;
        if !self.accept(":") {
            return Err(self.err0(format!("結果の中身には名前を付けてください（「{}」ではなく「名前: {}」の形）", n, n)));
        }
        Ok((n, self.type_()?))
    }

    // ---- 結果 ----
    fn result_def(&mut self, line: i64) -> PResult<ResultDef> {
        let name = self.name()?;
        self.expect("{")?;
        let mut variants = vec![];
        self.skip_nl();
        while !self.accept("}") {
            let v = self.name()?;
            let mut payload = vec![];
            if self.accept("(") {
                payload = self.comma_list(|s| s.named_type())?;
                self.expect(")")?;
            }
            variants.push((v, payload));
            self.accept(",");
            self.skip_nl();
        }
        self.end_line()?;
        Ok(ResultDef { name, variants, line })
    }

    // ---- 外部作用・部品・不変条件 ----
    fn dotted(&mut self) -> PResult<String> {
        let mut a = self.name()?;
        if self.accept(".") {
            a.push('.');
            a.push_str(&self.name()?);
        }
        Ok(a)
    }

    fn effect_decl(&mut self, line: i64) -> PResult<EffectDecl> {
        let name = self.dotted()?;
        self.expect("(")?;
        let params = self.comma_list(|s| s.name_type())?;
        self.expect(")")?;
        let (mut failures, mut joins, mut replies, mut no_wait) = (vec![], None, None, None);
        let (mut http, mut file) = (None, None);
        while self.peek().kind == "NAME" && ["failures", "joins", "replies", "no_wait", "over"].contains(&self.peek().val.as_str()) {
            let kw = self.name()?;
            if kw == "over" {
                if self.peek().kind == "NAME" && self.peek().val == "file" {
                    self.next();
                    let ln = self.peek().line;
                    file = Some(self.file_decl(ln)?);
                } else {
                    let ln = self.peek().line;
                    http = Some(self.http_decl(ln)?);
                }
                break;
            }
            match kw.as_str() {
                "failures" => failures = self.names()?,
                "joins" => joins = Some(self.name()?),
                "replies" => replies = Some(self.name()?),
                _ => {
                    let t = self.next();
                    if t.kind != "STR" {
                        return Err(self.err0("no_wait の後ろには、理由を \"…\" で書いてください"));
                    }
                    no_wait = Some(t.val);
                }
            }
        }
        if http.is_none() && file.is_none() {
            self.end_line()?;
        }
        Ok(EffectDecl { name, params, line, failures, no_wait, joins, replies, http, file })
    }

    fn resident_decl(&mut self, line: i64) -> PResult<ResidentDecl> {
        let mut r = ResidentDecl { line, ..Default::default() };
        self.block_open()?;
        while !self.accept("}") {
            let ln = self.peek().line;
            let kw = self.name()?;
            match kw.as_str() {
                "tooltip" => {
                    let t = self.next();
                    if t.kind != "STR" {
                        return Err(self.err0("tooltip のあとには、説明を \"…\" で書きます"));
                    }
                    r.tooltip = t.val;
                }
                "start" => {
                    r.start = self.name()?;
                    if r.start != "shown" && r.start != "hidden" {
                        return Err(self.err0("start のあとには shown（窓を開いて始める）か hidden（アイコンだけで始める）を書きます"));
                    }
                }
                "click" => {
                    r.click = self.name()?;
                    if r.click != "toggle" && r.click != "menu" {
                        return Err(self.err0("click のあとには toggle（左クリックで窓を開閉する）か menu（左クリックでもメニューを出す）を書きます"));
                    }
                }
                "separator" => r.menu.push((String::new(), "separator".into(), String::new(), ln)),
                "menu" => {
                    let t = self.next();
                    if t.kind != "STR" {
                        return Err(self.err0("menu のあとには、項目のラベルを \"…\" で書きます（例：menu \"つぶやきを書く\" → TweetList）"));
                    }
                    self.expect("→")?;
                    if self.accept("fire") {
                        let n = self.name()?;
                        r.menu.push((t.val, "event".into(), n, ln));
                    } else {
                        let n = self.name()?;
                        r.menu.push((t.val, "screen".into(), n, ln));
                    }
                }
                _ => return Err(self.err0(format!("resident の中に「{}」は書けません（tooltip・start・click・menu・separator）", kw))),
            }
            self.end_line()?;
        }
        self.end_line()?;
        Ok(r)
    }

    fn file_decl(&mut self, line: i64) -> PResult<FileDecl> {
        let mut f = FileDecl { line, ..Default::default() };
        self.block_open()?;
        while !self.accept("}") {
            let ln = self.peek().line;
            let kw = self.name()?;
            if ["write", "overwrite", "read", "delete"].contains(&kw.as_str()) {
                if !f.op.is_empty() {
                    return Err(ParseError::new("write・overwrite・read・delete は 1 つだけ書けます", ln));
                }
                let t = self.next();
                if t.kind != "STR" {
                    return Err(self.err0(format!("{} のあとには、場所を \"{{引数}}/ファイル名\" の形で書きます", kw)));
                }
                f.op = kw.clone();
                // 場所は "{引数}/相対パス"（引数は [a-z][a-z0-9_]*。相対パスは 1 文字以上で、改行を含まない）
                match split_place(&t.val) {
                    None => {
                        return Err(ParseError::new(
                            format!("{} の場所「{}」は、\"{{引数}}/ファイル名\" の形で書きます（引数は path 型。先頭の {{ }} が場所、あとは相対パス）", kw, t.val),
                            ln,
                        ))
                    }
                    Some((d, r)) => {
                        f.dir = d;
                        f.rest = r;
                    }
                }
                if kw == "write" || kw == "overwrite" {
                    if !self.accept("from") {
                        return Err(self.err0(format!("{} のあとには、from 内容の引数 が要ります", kw)));
                    }
                    f.src = self.name()?;
                }
            } else if ["ok", "exists", "not_found", "denied", "failed"].contains(&kw.as_str()) {
                if f.rows.contains_key(&kw) {
                    return Err(ParseError::new(format!("{} が 2 つあります", kw), ln));
                }
                self.expect("→")?;
                let outcome = self.name()?;
                let mut takes = vec![];
                while self.accept("take") {
                    let n = self.name()?;
                    if !self.accept("from") {
                        return Err(self.err0("take のあとには、take 名前 from content（ファイルの中身。read のとき）か take 名前 from bytes（バイト数）を書きます"));
                    }
                    let src = self.name()?;
                    takes.push((n, src));
                    self.accept(",");
                }
                f.rows.insert(kw, (outcome, takes));
            } else {
                return Err(self.err0(format!("over file の中に「{}」は書けません（write・overwrite・read・delete・ok・exists・not_found・denied・failed）", kw)));
            }
            self.end_line()?;
        }
        self.end_line()?;
        Ok(f)
    }

    fn http_decl(&mut self, line: i64) -> PResult<HttpDecl> {
        if self.name()? != "http" {
            return Err(self.err0("over のあとは http { … } と書きます（実通信の宣言）"));
        }
        let mut h = HttpDecl { line, ..Default::default() };
        self.block_open()?;
        while !self.accept("}") {
            let kw = self.name()?;
            match kw.as_str() {
                "GET" | "POST" | "PUT" | "DELETE" => {
                    let t = self.next();
                    if t.kind != "STR" {
                        return Err(self.err0(format!("{} のあとには、通信先の URL を \"https://…\" で書きます", kw)));
                    }
                    h.method = kw.clone();
                    h.url = t.val;
                }
                "bearer" => h.bearer = Some(self.dotted()?),
                "body" => {
                    h.body = self.comma_list(|s| {
                        if s.peek().kind == "STR" {
                            let key = s.next().val;
                            s.expect("from")?;
                            Ok((key, s.name()?))
                        } else {
                            let n = s.name()?;
                            Ok((n.clone(), n))
                        }
                    })?;
                }
                "status" => {
                    let (mut lo, mut hi): (Option<i128>, i128) = (None, 0);
                    if self.peek().kind == "NAME" && self.peek().val == "other" {
                        self.i += 1;
                    } else {
                        let v = self.num()?;
                        lo = Some(v);
                        hi = v;
                        if self.accept("..") {
                            hi = self.num()?;
                        }
                    }
                    self.expect("→")?;
                    let outcome = self.name()?;
                    let mut takes = vec![];
                    while self.accept("take") {
                        let n = self.name()?;
                        self.expect("from")?;
                        let t = self.next();
                        if t.kind != "STR" {
                            return Err(self.err0("from のあとには、返事の JSON の道すじを \"data.id\" の形で書きます"));
                        }
                        takes.push((n, t.val));
                        self.accept(",");
                    }
                    match lo {
                        None => h.other = Some((outcome, takes)),
                        Some(l) => h.statuses.push((l, hi, outcome, takes)),
                    }
                }
                _ => return Err(self.err0(format!("over http の中に「{}」は書けません（METHOD \"URL\"・bearer・body・status）", kw))),
            }
            self.end_line()?;
        }
        self.end_line()?;
        Ok(h)
    }

    fn library(&mut self, line: i64) -> PResult<Part> {
        let name = self.name()?;
        self.expect("(")?;
        let params = self.comma_list(|s| s.name_type())?;
        self.expect(")")?;
        self.expect("→")?;
        let ret = self.type_()?;
        let usage = format!("library {}(arg: Type) -> 型 from rust \"クレート\" \"版\" call \"Rust の式\" [guarantee 式] trust check \"理由\"", name);
        if !(self.accept("from") && self.accept("rust")) {
            return Err(self.err0(format!("ライブラリの呼び出し元がありません（{}）", usage)));
        }
        let mut strs = vec![];
        for what in ["クレートの名前", "版（std なら空の \"\"）"] {
            let t = self.peek();
            if t.kind != "STR" {
                return Err(self.err0(format!("from rust のあとには、{}を \"…\" で書きます（{}）", what, usage)));
            }
            strs.push(t.val.clone());
            self.i += 1;
        }
        if !self.accept("call") {
            return Err(self.err0(format!("呼び出す Rust の式がありません（call \"式\"。引数の名前を使えます。{}）", usage)));
        }
        let t = self.peek().clone();
        if t.kind != "STR" || t.val.trim().is_empty() {
            return Err(self.err0("call には、Rust の式を \"…\" で書きます"));
        }
        self.i += 1;
        let expr = t.val;
        let g = if self.accept("guarantee") { Some(self.expr()?) } else { None };
        if !self.accept("trust") {
            return Err(self.err0(format!("ライブラリには、人の保証が要ります（trust check|verify|assume \"理由\"）。{}", usage)));
        }
        let level = self.peek().val.clone();
        if !["check", "verify", "assume"].contains(&level.as_str()) {
            return Err(self.err0("trust のあとには check（毎回確かめる）、verify（検証ビルドだけ確かめる）、assume（確かめない）のどれかを書きます"));
        }
        self.i += 1;
        let why = self.peek().clone();
        if why.kind != "STR" || why.val.trim().is_empty() {
            return Err(self.err0("trust には、保証する理由を \"…\" で書いてください（そのライブラリが純粋で、panic・無限ループ・メモリ枯渇をしない根拠など）"));
        }
        self.i += 1;
        self.end_line()?;
        Ok(Part { name, params, ret, guarantee: g, line, lib: Some((strs[0].clone(), strs[1].clone(), expr, level, why.val)), ..Default::default() })
    }

    fn example(&mut self, line: i64) -> PResult<(String, Vec<Expr>, Expr, i64)> {
        let usage = "example tax_included(100) == 110";
        let name = self.name()?;
        self.expect("(")?;
        let args = if self.peek().val == ")" { vec![] } else { self.comma_list(|s| s.example_value())? };
        self.expect(")")?;
        if !self.accept("==") {
            return Err(self.err0(format!("具体例は、呼び出しと期待する値を == でつなぎます（{}）", usage)));
        }
        let want = self.example_value()?;
        self.end_line()?;
        Ok((name, args, want, line))
    }

    fn example_value(&mut self) -> PResult<Expr> {
        let e = self.expr()?;
        if !matches!(e, Expr::Num { .. } | Expr::StrLit { .. } | Expr::BoolLit { .. }) {
            return Err(self.err0("具体例には、直に書いた値（数・\"文字列\"・true・false）だけを書けます"));
        }
        Ok(e)
    }

    fn convert(&mut self, line: i64) -> PResult<Part> {
        let name = self.name()?;
        self.expect("(")?;
        let mut defaults: Map<Expr> = Map::default();
        let params = {
            let mut items = vec![self.convert_param(&mut defaults)?];
            while self.accept(",") {
                items.push(self.convert_param(&mut defaults)?);
            }
            items
        };
        self.expect(")")?;
        self.expect("→")?;
        let ret = self.type_()?;
        if self.is_("{") {
            self.next();
            self.skip_nl();
            let (mut requires, mut body): (Vec<(Expr, String)>, Option<Expr>) = (vec![], None);
            while !self.accept("}") {
                if self.accept("require") {
                    let cond = self.expr()?;
                    let why = self.next();
                    if why.kind != "STR" || why.val.trim().is_empty() {
                        return Err(self.err0("require には、拒否する理由を \"…\" で書いてください"));
                    }
                    requires.push((cond, why.val));
                } else if self.accept("return") {
                    body = Some(self.expr()?);
                } else {
                    return Err(self.err0("この中には require 条件 \"理由\" と return 式 だけ書けます"));
                }
                self.skip_nl();
            }
            if body.is_none() {
                return Err(self.err0("return 式 が必要です"));
            }
            self.end_line()?;
            return Ok(Part { name, params, ret, guarantee: None, line, body, defaults, requires, ..Default::default() });
        }
        self.expect("=")?;
        let body = self.expr()?;
        self.end_line()?;
        Ok(Part { name, params, ret, guarantee: None, line, body: Some(body), defaults, ..Default::default() })
    }

    fn convert_param(&mut self, defaults: &mut Map<Expr>) -> PResult<(String, Type)> {
        let n = self.name()?;
        self.expect(":")?;
        let mut t = self.type_()?;
        if self.accept("fill") {
            self.expect("{")?;
            let fs = self.comma_list(|s| {
                let n = s.name()?;
                s.expect(":")?;
                Ok((n, s.fill_value()?))
            })?;
            self.expect("}")?;
            t.fills = Some(fs);
        }
        if self.accept("=") {
            defaults.insert(n.clone(), self.expr()?);
        }
        Ok((n, t))
    }

    fn fill_value(&mut self) -> PResult<i128> {
        let t = self.next();
        if t.kind != "NUM" {
            return Err(self.err0("rule の値は整数です"));
        }
        Ok(parse_int(&t.val))
    }

    fn invariant(&mut self, line: i64) -> PResult<Invariant> {
        let d = self.name()?;
        self.expect(":")?;
        let mut c = self.expr()?;
        if self.accept("implies") {
            let r = self.expr()?;
            c = Expr::Implies { l: Box::new(c), r: Box::new(r) };
        }
        self.end_line()?;
        Ok(Invariant { data: d, cond: c, line })
    }

    // ---- 処理（deed） ----
    fn process(&mut self, line: i64) -> PResult<Process> {
        let mut p = Process { name: self.name()?, line, ..Default::default() };
        self.block_open()?;
        while !self.accept("}") {
            if self.peek().kind == "NAME" && ["input", "slot", "reads", "writes", "transition", "result", "effects"].contains(&self.peek().val.as_str()) {
                self.decl(&mut p)?;
            } else {
                p.body.push(self.stmt()?);
            }
        }
        Ok(p)
    }

    fn input_item(&mut self) -> PResult<(String, Type)> {
        let n = self.name()?;
        self.expect(":")?;
        let mut t = self.type_()?;
        if self.is_("optional") {
            return Err(self.err0("入力は、書かなければ「無いかもしれない」扱いです。optional は要りません（必ず要るなら required と書きます）"));
        }
        t.optional = !self.accept("required");
        if self.accept("trust") {
            let level = if self.peek().kind == "NAME" { self.peek().val.clone() } else { String::new() };
            if !["verify", "assume", "dangerous"].contains(&level.as_str()) {
                return Err(self.err0("trust のあとには verify（検証ビルドだけ確かめる）、assume（確かめない）、dangerous（確かめず、未検査の raw も受ける）のどれかを書きます"));
            }
            self.i += 1;
            let why = self.next();
            if why.kind != "STR" || why.val.trim().is_empty() {
                return Err(self.err0("trust には、保証する理由を \"…\" で書いてください"));
            }
            t.trust = Some((level, why.val));
        }
        Ok((n, t))
    }

    fn decl(&mut self, p: &mut Process) -> PResult<()> {
        let kw = self.name()?;
        match kw.as_str() {
            "input" => p.inputs.extend(self.comma_list(|s| s.input_item())?),
            "slot" => p.refs.extend(self.comma_list(|s| s.name_type())?),
            "reads" => p.reads.extend(self.comma_list(|s| s.dotted())?),
            "writes" => p.writes.extend(self.comma_list(|s| s.dotted())?),
            "transition" => {
                let d = self.name()?;
                self.expect(":")?;
                let src;
                if self.accept("(") {
                    self.name()?;
                    self.expect(")")?;
                    src = "(新規)".to_string();
                } else {
                    src = self.name()?;
                }
                self.expect("→")?;
                let to = self.name()?;
                p.transitions.push((d, src, to));
            }
            "result" => p.result = self.name()?,
            _ => {
                if !self.accept("none") {
                    p.effects.extend(self.comma_list(|s| s.dotted())?);
                }
            }
        }
        self.end_line()
    }

    // ---- 文 ----
    fn ret(&mut self) -> PResult<Return> {
        let line = self.peek().line;
        self.expect("return")?;
        if self.peek().kind != "NAME" {
            return Err(self.unexpected(
                format!("return のあとに結果の種類が必要です（「{}」があります）", shown(self.peek())),
                "処理は必ず結果の種類を返します（return Done、中身があれば return Received(order_id)）",
            ));
        }
        let v = self.name()?;
        let mut args = vec![];
        if self.accept("(") {
            if self.is_(")") {
                return Err(self.err0(format!("中身の無い結果は return {} と書きます（( ) は付けません）", v)));
            }
            args = self.comma_list(|s| s.expr())?;
            self.expect(")")?;
        }
        Ok(Return { variant: v, args, line, calc: None })
    }

    fn ret_block(&mut self) -> PResult<Return> {
        self.expect("{")?;
        self.skip_nl();
        let r = self.ret()?;
        self.skip_nl();
        self.expect("}")?;
        Ok(r)
    }

    fn end_stmt(&mut self) -> PResult<()> {
        if !self.is_("}") {
            self.end_line()?;
        }
        Ok(())
    }

    fn if_block(&mut self) -> PResult<Vec<Stmt>> {
        self.expect("{")?;
        self.skip_nl();
        let mut body = vec![];
        while !self.accept("}") {
            body.push(self.stmt()?);
            self.skip_nl();
        }
        Ok(body)
    }

    fn if_stmt(&mut self, line: i64) -> PResult<If> {
        let cond = self.expr()?;
        let body = self.if_block()?;
        let mut s = If { cond, body, line, els: None, else_line: 0 };
        if self.is_("else") {
            s.else_line = self.next().line;
            if self.is_("if") {
                let l = self.next().line;
                s.els = Some(vec![Stmt::If(self.if_stmt(l)?)]);
            } else if self.is_("{") {
                s.els = Some(self.if_block()?);
            } else {
                return Err(self.err("else の後ろには { … } か if を書きます", "if 条件 { … } else { … }、または } else if 条件 { … } と続けます"));
            }
        }
        Ok(s)
    }

    fn binder(&mut self) -> PResult<Binder> {
        let names = if self.accept("(") {
            let n = self.names()?;
            self.expect(")")?;
            n
        } else {
            vec![self.name()?]
        };
        self.expect("in")?;
        Ok(Binder { names, src: Box::new(self.postfix()?) })
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        let line = self.peek().line;
        let mut s: Stmt;
        if self.is_("return") {
            s = Stmt::Return(self.ret()?);
        } else if self.accept("if") {
            s = Stmt::If(self.if_stmt(line)?);
        } else if self.accept("update") {
            self.expect("{")?;
            let target = self.postfix()?;
            if !["-=", "+=", "="].contains(&self.peek().val.as_str()) {
                return Err(self.err0("update の演算子（-= += =）が必要です"));
            }
            let op = self.next().val;
            let value = self.expr()?;
            self.expect("|")?;
            let b = self.binder()?;
            let cond = if self.accept(",") { Some(self.expr()?) } else { None };
            self.expect("}")?;
            s = Stmt::Update(Update { target, op, value, binder: b, cond, line });
        } else if self.accept("for") {
            let b = self.binder()?;
            self.block_open()?;
            let mut body = vec![];
            while !self.accept("}") {
                body.push(self.stmt()?);
            }
            s = Stmt::ForEach(ForEach { binder: b, body, line });
        } else if self.accept("request") {
            let eff = self.dotted()?;
            self.expect("(")?;
            let (mut args, mut idem) = (vec![], None);
            loop {
                if self.is_("idempotency_key") && self.is_k("=", 1) {
                    self.next();
                    self.next();
                    idem = Some(self.idem_key()?);
                } else {
                    args.push(self.expr()?);
                }
                if !self.accept(",") {
                    break;
                }
            }
            self.expect(")")?;
            s = Stmt::Request(Request { effect: eff, args, idem, line });
        } else if self.is_("delete") && self.peek_at(1).kind == "NAME" && !self.is_k(".", 1) {
            self.next();
            s = Stmt::Delete(Delete { var: self.name()?, line });
        } else if (self.is_("add") || self.is_("remove")) && !self.is_k(".", 1) {
            let op = self.next().val;
            let word = if op == "add" { "to" } else { "from" };
            let e = self.expr()?;
            if !self.accept(word) {
                return Err(self.err0(format!("{} は「{} 要素 {} 変数.項目」と書きます（例：{} tag_id {} product.tags）", op, op, word, op, word)));
            }
            let n = self.name()?;
            self.expect(".")?;
            let f = self.name()?;
            s = Stmt::SetField(SetField { var: n, f, e, line, op, guard: None, calc: None });
        } else if self.accept("let") {
            if self.is_("mut") && self.peek_at(1).kind == "NAME" {
                return Err(self.err("let に mut は付けません", "let の変数は作り直さず、項目の更新は 名前.項目 = 値 と書きます"));
            }
            let n = self.name()?;
            self.expect("=")?;
            if self.accept("create") {
                let d = self.name()?;
                self.expect("{")?;
                let fields = self.comma_list(|s| s.name_expr())?;
                self.expect("}")?;
                s = Stmt::Create(Create { name: n, data: d, fields, line, calc: None });
            } else {
                let e = self.expr()?;
                let absent = if self.accept("else") { Some(self.ret_block()?) } else { None };
                s = Stmt::Assign(Assign { name: n, e, absent, line, calc: None });
            }
        } else {
            let n = self.name()?;
            if !self.accept(".") {
                let t = self.t[self.i - 1].clone();
                let hint = stmt_hint(&n, "");
                let msg = if !hint.is_empty() { format!("文の始まりが分かりません（「{}」）", n) } else { format!("文の始まりが分かりません（「{}」。変数は let で作り、項目の更新は 名前.項目 = 値 と書きます）", n) };
                return Err(ParseError::at(msg, t.line, t.col, &t.file, hint));
            }
            let f = self.name()?;
            if self.peek().val == "+=" || self.peek().val == "-=" {
                let op = self.next().val;
                let e = self.expr()?;
                let (mut guard, mut calc): (Option<(String, String)>, Option<String>) = (None, None);
                let mut why_val = String::new();
                if self.is_("saturating") || self.is_("trust") {
                    let mut mode = self.next().val;
                    if mode == "trust" {
                        if ["prove", "check", "verify"].contains(&self.peek().val.as_str()) {
                            calc = Some(self.next().val);
                        } else if !self.accept("assume") {
                            return Err(self.err0("trust のあとには prove（示せなければエラー）、check（警告を出し、実行時に確かめる）、verify（検証ビルドだけ確かめる）、assume（確かめない）のどれかを書きます"));
                        }
                        mode = "assume".to_string();
                    }
                    let why = self.next();
                    if why.kind != "STR" || why.val.trim().is_empty() {
                        let head = if mode == "assume" { format!("trust {}", calc.clone().unwrap_or_else(|| "assume".to_string())) } else { mode.clone() };
                        return Err(self.err0(format!("{} には、理由を \"…\" で書いてください", head)));
                    }
                    why_val = why.val.clone();
                    guard = if calc.is_none() { Some((mode, why.val)) } else { None };
                }
                let opc = op[..1].to_string();
                let mut sf = SetField { var: n.clone(), f: f.clone(), e: Expr::bin(&opc, Expr::Attr { e: Box::new(Expr::name(&n)), n: f }, e), line, op, guard, calc: None };
                if let Some(c) = calc {
                    sf.calc = Some((c, why_val));
                }
                s = Stmt::SetField(sf);
            } else {
                if self.is_("<") && self.is_k("<", 1) {
                    return Err(self.err0(format!("集合に足す記号 << はありません。add 要素 to {}.{} と書きます（外すのは remove 要素 from {}.{}）", n, f, n, f)));
                }
                self.expect("=")?;
                let e = self.expr()?;
                s = Stmt::SetField(SetField { var: n, f, e, line, op: "=".into(), guard: None, calc: None });
            }
        }
        if self.is_("trust") {
            let ok = match &s {
                Stmt::Return(r) => r.calc.is_none(),
                Stmt::Assign(a) => a.calc.is_none(),
                Stmt::Create(c) => c.calc.is_none(),
                Stmt::SetField(f) => f.guard.is_none() && f.calc.is_none(),
                _ => false,
            };
            if ok {
                let c = Some(self.calc_trust()?);
                match &mut s {
                    Stmt::Return(r) => r.calc = c,
                    Stmt::Assign(a) => a.calc = c,
                    Stmt::Create(x) => x.calc = c,
                    Stmt::SetField(f) => f.calc = c,
                    _ => {}
                }
            }
        }
        self.end_stmt()?;
        Ok(s)
    }

    fn calc_trust(&mut self) -> PResult<(String, String)> {
        self.expect("trust")?;
        if !["prove", "check", "verify", "assume"].contains(&self.peek().val.as_str()) {
            return Err(self.err0("trust のあとには prove（示せなければエラー）、check（警告を出し、実行時に確かめる）、verify（検証ビルドだけ確かめる）、assume（確かめない）のどれかを書きます"));
        }
        let level = self.next().val;
        let why = self.next();
        if why.kind != "STR" || why.val.trim().is_empty() {
            return Err(self.err0(format!("trust {} には、理由を \"…\" で書いてください", level)));
        }
        Ok((level, why.val))
    }

    // ---- 式 ----
    pub fn expr(&mut self) -> PResult<Expr> {
        let mut l = self.and_expr()?;
        while self.accept("or") {
            let r = self.and_expr()?;
            l = Expr::bin("または", l, r);
        }
        Ok(l)
    }

    fn and_expr(&mut self) -> PResult<Expr> {
        let mut l = self.not_expr()?;
        while self.accept("and") {
            let r = self.not_expr()?;
            l = Expr::bin("かつ", l, r);
        }
        Ok(l)
    }

    fn not_expr(&mut self) -> PResult<Expr> {
        if self.accept("not") {
            return Ok(Expr::Not { e: Box::new(self.not_expr()?) });
        }
        self.cmp_expr()
    }

    fn cmp_expr(&mut self) -> PResult<Expr> {
        let l = self.add()?;
        if self.peek().kind == "OP" && CMP.contains(&self.peek().val.as_str()) {
            let op = self.next().val;
            let r = self.add()?;
            return Ok(Expr::bin(&op, l, r));
        }
        if self.accept("in") {
            let r = self.add()?;
            return Ok(Expr::bin("in", l, r));
        }
        if self.accept("is") {
            let kw = self.name()?;
            match pred_word(&kw) {
                None => {
                    let h = foreign_names().get(kw.as_str()).map(|s| s.to_string()).unwrap_or_default();
                    return Err(self.err(format!("「is」の後ろに「{}」は書けません（{}）", kw, PRED_ORDER), h));
                }
                Some(w) => return Ok(Expr::Pred { e: Box::new(l), kind: w.to_string() }),
            }
        }
        Ok(l)
    }

    fn add(&mut self) -> PResult<Expr> {
        let mut l = self.mul()?;
        while self.peek().kind == "OP" && (self.peek().val == "+" || self.peek().val == "-") {
            let op = self.next().val;
            let r = self.mul()?;
            l = Expr::bin(&op, l, r);
        }
        Ok(l)
    }

    fn unary(&mut self) -> PResult<Expr> {
        if self.peek().kind == "OP" && self.peek().val == "-" {
            self.next();
            let e = self.unary()?;
            return Ok(match e {
                Expr::Num { v } => Expr::Num { v: -v },
                e => Expr::bin("-", Expr::num(0), e),
            });
        }
        self.postfix()
    }

    fn mul(&mut self) -> PResult<Expr> {
        let mut l = self.unary()?;
        loop {
            if self.accept("*") {
                let r = self.unary()?;
                l = Expr::bin("*", l, r);
            } else if self.is_("/") || self.is_("%") {
                let op = self.next().val;
                let t = self.peek().clone();
                let r;
                if t.kind == "NUM" {
                    if parse_int(&t.val) <= 0 {
                        return Err(self.err0(format!("「{}」の右の定数は、正の整数です（0 で割る道を作らないため）", op)));
                    }
                    self.next();
                    r = Expr::num(parse_int(&t.val));
                } else {
                    r = self.postfix()?;
                }
                let dflt = if self.accept("or") { Some(Box::new(self.postfix()?)) } else { None };
                l = Expr::Bin { op, l: Box::new(l), r: Box::new(r), dflt };
            } else {
                return Ok(l);
            }
        }
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            if self.accept(".") {
                let n = self.name()?;
                e = Expr::Attr { e: Box::new(e), n };
            } else if self.is_("[") {
                let mut scopes: Vec<String> = vec![];
                while let Expr::Attr { e: inner, n } = e {
                    scopes.insert(0, n);
                    e = *inner;
                }
                let d = match e {
                    Expr::Name { n } => n,
                    _ => return Err(self.err0("[ ] の前にはデータの名前が必要です（範囲を通すなら「データ.範囲[ID]」）")),
                };
                self.next();
                let k = self.expr()?;
                self.expect("]")?;
                e = Expr::Index { d, e: Box::new(k), scopes };
            } else {
                return Ok(e);
            }
        }
    }

    fn builder(&mut self) -> PResult<(Expr, Binder, Option<Expr>)> {
        self.expect("{")?;
        let elem = self.expr()?;
        self.expect("|")?;
        let b = self.binder()?;
        let cond = if self.accept(",") { Some(self.expr()?) } else { None };
        self.expect("}")?;
        Ok((elem, b, cond))
    }

    fn idem_key(&mut self) -> PResult<Expr> {
        if !self.is_("(") {
            return self.expr();
        }
        self.next();
        let mut items = vec![self.expr()?];
        while self.accept(",") {
            items.push(self.expr()?);
        }
        self.expect(")")?;
        if items.len() == 1 {
            Ok(items.remove(0))
        } else {
            Ok(Expr::Tuple { items })
        }
    }

    fn primary(&mut self) -> PResult<Expr> {
        let t = self.peek().clone();
        if t.kind == "NUM" {
            self.next();
            let nxt = self.peek().clone();
            if nxt.kind == "NAME" && CALENDAR_UNITS.contains(&nxt.val.as_str()) {
                return Err(self.err0(cal_msg(&nxt.val)));
            }
            if nxt.kind == "NAME" {
                if let Some(u) = unit_secs(&nxt.val) {
                    self.next();
                    let secs = parse_int(&t.val).saturating_mul(u);
                    if secs > MAX_DUR {
                        return Err(self.err0(format!("期間が長すぎます（{} {}。{} 秒まで）", t.val, nxt.val, MAX_DUR)));
                    }
                    return Ok(Expr::Dur { secs, text: format!("{} {}", t.val, nxt.val) });
                }
            }
            return Ok(Expr::num(parse_int(&t.val)));
        }
        if t.kind == "STR" {
            self.next();
            return Ok(Expr::StrLit { s: t.val });
        }
        if t.kind == "NAME" && (t.val == "true" || t.val == "false") {
            self.next();
            return Ok(Expr::BoolLit { v: t.val == "true" });
        }
        if self.is_("if") {
            self.next();
            let c = self.expr()?;
            self.expect("then")?;
            let a = self.expr()?;
            self.expect("else")?;
            let b = self.expr()?;
            return Ok(Expr::Cond { c: Box::new(c), a: Box::new(a), b: Box::new(b) });
        }
        if self.accept("(") {
            let e = self.expr()?;
            self.expect(")")?;
            return Ok(e);
        }
        if self.is_("sum") && self.is_k("{", 1) {
            self.next();
            let (elem, binder, cond) = self.builder()?;
            return Ok(Expr::SumE { elem: Box::new(elem), binder, cond: cond.map(Box::new) });
        }
        if self.is_("{") {
            let (elem, binder, cond) = self.builder()?;
            return Ok(Expr::SetB { elem: Box::new(elem), binder, cond: cond.map(Box::new) });
        }
        if t.kind == "NAME" && t.val == "null" {
            self.next();
            return Ok(Expr::Null);
        }
        if t.kind == "NAME" && self.composites.contains(&t.val) && self.is_k("{", 1) && self.peek_at(2).kind == "NAME" && self.is_k(":", 3) {
            self.next();
            self.next();
            let fields = self.comma_list(|s| s.name_expr())?;
            self.expect("}")?;
            return Ok(Expr::Make { n: t.val, fields, line: t.line });
        }
        if t.kind == "NAME" {
            self.next();
            if self.accept("(") {
                let args = self.comma_list(|s| s.expr())?;
                self.expect(")")?;
                return Ok(Expr::Call { n: t.val, args });
            }
            return Ok(Expr::Name { n: t.val });
        }
        Err(self.unexpected(format!("式のはずが「{}」があります", shown(&t)), ""))
    }

    // ---- ネクサス（nexus。flow は別名） ----
    fn flow(&mut self, line: i64) -> PResult<Flow> {
        let name = self.name()?;
        self.block_open()?;
        let mut entries = vec![];
        while !self.accept("}") {
            let proc_ = self.name()?;
            self.expect(":")?;
            let mut routes: Vec<(String, String)> = vec![];
            loop {
                let v = self.name()?;
                self.expect("→")?;
                if routes.iter().any(|(x, _)| x == &v) {
                    return Err(self.err0(format!("結果「{}」の行き先が2つ以上あります。並列に起こすなら par {{ 処理, 処理 }} と書きます", v)));
                }
                let mut d = self.name()?;
                if d == "par" {
                    self.expect("{")?;
                    let ds = self.names()?;
                    self.expect("}")?;
                    if ds.len() < 2 {
                        return Err(self.err0("par には2つ以上の処理を書きます"));
                    }
                    if ds.iter().any(|x| x == "wait") {
                        return Err(self.err0("wait は par に入れられません（待つ行き先は、ほかの行き先と並べられません）"));
                    }
                    for x in ds {
                        routes.push((v.clone(), x));
                    }
                    if !self.accept(",") {
                        break;
                    }
                    self.skip_nl();
                    continue;
                }
                if d == "wait" {
                    d = format!("wait {}", self.name()?);
                }
                routes.push((v, d));
                if !self.accept(",") {
                    break;
                }
                self.skip_nl();
            }
            entries.push((proc_, routes));
            self.end_line()?;
        }
        Ok(Flow { name, entries, line })
    }

    fn unit(&mut self) -> PResult<i128> {
        let unit = self.name()?;
        match unit_secs(&unit) {
            Some(u) => Ok(u),
            None => {
                if CALENDAR_UNITS.contains(&unit.as_str()) {
                    return Err(self.err0(cal_msg(&unit)));
                }
                Err(self.err0(format!("時間の単位「{}」は使えません（{}）", unit, UNIT_WORDS)))
            }
        }
    }

    // ---- 問い合わせ（読むだけの処理） ----
    fn filter_op(op: &str) -> Option<&'static str> {
        match op {
            "at_least" => Some("以上"),
            "at_most" => Some("以下"),
            "equals" => Some("一致"),
            _ => None,
        }
    }
    fn fold_word(w: &str) -> Option<&'static str> {
        match w {
            "ignore_case" => Some("大小無視"),
            "ignore_kana" => Some("かな無視"),
            "ignore_width" => Some("幅無視"),
            _ => None,
        }
    }
    fn order_word_ja(w: &str) -> Option<&'static str> {
        match w {
            "desc" => Some("降順"),
            "asc" => Some("昇順"),
            _ => None,
        }
    }
    const ORDER_HOW: &'static str = "order 項目 desc は大きい順（日時なら新しい順）、order 項目 asc は小さい順（古い順）です";

    fn query(&mut self, line: i64) -> PResult<Query> {
        let mut q = Query { name: self.name()?, line, ..Default::default() };
        self.block_open()?;
        while !self.accept("}") {
            let kw = self.name()?;
            match kw.as_str() {
                "input" => q.inputs.extend(self.comma_list(|s| s.input_item())?),
                "slot" => q.refs.extend(self.comma_list(|s| s.name_type())?),
                "from" => {
                    q.target = self.name()?;
                    while self.accept(".") {
                        let sn = self.name()?;
                        q.scopes.push(sn.clone());
                        if self.accept("(") {
                            let a = self.names()?;
                            q.scope_args.insert(sn, a);
                            self.expect(")")?;
                        }
                    }
                    if self.is_("in") {
                        return Err(self.err("範囲は in ではなく、データのあとに . で続けます", "from データ.範囲（複数は from データ.範囲1.範囲2。すべてに入る行だけになります）"));
                    }
                }
                "where" => q.conds.push(self.where_group()?),
                "filter" => {
                    let f = self.name()?;
                    if self.peek().kind == "OP" && CMP.contains(&self.peek().val.as_str()) {
                        return Err(self.err(
                            format!("filter に「{}」は書けません", self.peek().val),
                            "filter は 項目 equals|at_least|at_most|contains|within 入力 の形です（== は equals、>= は at_least、<= は at_most）",
                        ));
                    }
                    let op = self.name()?;
                    if op == "within" {
                        let n = self.num()?;
                        let u = self.unit()?;
                        q.filters.push((f, "直近".into(), (n * u).to_string()));
                    } else if op == "contains" {
                        let arg = self.name()?;
                        let mut folds: Vec<&str> = vec![];
                        while self.peek().kind == "NAME" && Self::fold_word(&self.peek().val).is_some() {
                            let w = self.name()?;
                            folds.push(Self::fold_word(&w).unwrap());
                        }
                        q.filters.push((f, format!("含む{}", if folds.is_empty() { String::new() } else { format!("({})", folds.join(",")) }), arg));
                    } else if let Some(j) = Self::filter_op(&op) {
                        let a = self.name()?;
                        q.filters.push((f, j.to_string(), a));
                    } else {
                        return Err(self.err0(format!("filter の種類「{}」は使えません（equals・at_least・at_most・contains・within）", op)));
                    }
                }
                "order" => {
                    let f = self.name()?;
                    if self.peek().kind != "NAME" {
                        return Err(self.err("order の向き（desc か asc）を、項目の後ろに書きます", Self::ORDER_HOW));
                    }
                    let d = self.name()?;
                    match Self::order_word_ja(&d) {
                        None => {
                            let pre = if f == "by" {
                                "by は書きません。"
                            } else if Self::order_word_ja(&d.to_lowercase()).is_some() {
                                "向きは小文字です。"
                            } else {
                                ""
                            };
                            return Err(self.err(format!("order の向きは desc か asc です（「{}」は使えません）", d), format!("{}{}", pre, Self::ORDER_HOW)));
                        }
                        Some(j) => {
                            q.orders.push((f, j.to_string()));
                            q.order = Some(q.orders[0].clone());
                        }
                    }
                }
                "page" => {
                    if self.peek().kind == "NUM" {
                        let n = self.num()?;
                        q.page_n = Some(n);
                        if n < 1 {
                            return Err(self.err0("page の件数は 1 以上にしてください"));
                        }
                    } else {
                        q.page = Some(self.name()?);
                    }
                }
                "limit" => return Err(self.err0("limit は page に替わりました（page 20 か page 入力の名前。order が要ります）")),
                "select" => q.select.extend(self.comma_list(|s| s.column())?),
                "reads" => q.reads.extend(self.comma_list(|s| s.dotted())?),
                _ => return Err(self.err0(format!("query の中に「{}」は書けません", kw))),
            }
            self.end_line()?;
        }
        Ok(q)
    }

    fn column(&mut self) -> PResult<Column> {
        let first = self.name()?;
        let mut col;
        if (first == "sum" || first == "count") && self.is_("(") {
            self.next();
            let f = self.name()?;
            self.expect(")")?;
            let (suffix, kind) = if first == "sum" { ("合計", "合計") } else { ("件数", "件数") };
            col = Column { name: format!("{}{}", f, suffix), kind: kind.into(), field: f, via: None };
        } else if self.accept(".") {
            let f = self.name()?;
            col = Column { name: format!("{}_{}", first, f), kind: "参照先".into(), field: f, via: Some(first) };
        } else {
            col = Column { name: first.clone(), kind: "項目".into(), field: first, via: None };
        }
        if self.accept("as") {
            col.name = self.name()?;
        }
        Ok(col)
    }

    // ---- 出来事 ----
    fn event(&mut self, line: i64) -> PResult<Event> {
        let mut ev = Event { name: self.name()?, line, ..Default::default() };
        if self.accept("on") {
            ev.on = self.name()?;
            if ev.on != "online" && ev.on != "opened" {
                return Err(self.err0("event の「on」は、online（つながりが戻った）か opened（アプリを開いた）です"));
            }
        }
        self.block_open()?;
        while !self.accept("}") {
            let kw = self.name()?;
            if kw == "each" {
                let q = self.name()?;
                self.expect("→")?;
                let d = self.name()?;
                ev.steps.push(("各".into(), q, d));
            } else if kw == "external" {
                self.expect("→")?;
                let d = self.name()?;
                ev.steps.push(("外から".into(), String::new(), d));
            } else {
                return Err(self.err0("event の中は「each 問い合わせ → 処理」か「external → 処理」です"));
            }
            self.end_line()?;
        }
        Ok(ev)
    }

    // ---- 画面 ----
    fn screen(&mut self, line: i64) -> PResult<Screen> {
        let mut sc = Screen { name: self.name()?, line, ..Default::default() };
        if self.accept("(") {
            sc.params = self.comma_list(|s| s.input_item())?;
            self.expect(")")?;
        }
        sc.entry = self.accept("entry");
        self.block_open()?;
        while !self.accept("}") {
            let kw = self.name()?;
            match kw.as_str() {
                "show" => {
                    let n = self.name()?;
                    sc.shows.push(n);
                }
                "open" => {
                    let n = self.name()?;
                    sc.opens.push(n);
                }
                "field" => {
                    self.comma_list(|s| {
                        let f = s.name()?;
                        sc.fields.push(f.clone());
                        if s.accept("from") {
                            let n = s.name()?;
                            sc.field_from.insert(f, n);
                        }
                        Ok(())
                    })?;
                }
                "slot" => sc.slots.extend(self.names()?),
                "action" => {
                    let deed = self.name()?;
                    self.block_open()?;
                    let mut routes = vec![];
                    while !self.accept("}") {
                        let v = self.name()?;
                        self.expect("→")?;
                        let d = self.name()?;
                        routes.push((v, d));
                        self.end_line()?;
                    }
                    sc.actions.push((deed, routes));
                }
                "page" => {
                    self.comma_list(|s| {
                        let t = s.next();
                        if t.kind != "STR" {
                            return Err(ParseError::new("page には、ページの URL のパターン（\"*://www.example.com/*\" のような文字列）を書いてください", t.line));
                        }
                        sc.pages.push(t.val);
                        Ok(())
                    })?;
                }
                "read" => sc.read = Some(self.read_spec()?),
                "design" => {
                    self.block_open()?;
                    while !self.accept("}") {
                        let k = self.name()?;
                        let t = self.next();
                        if !["NAME", "STR", "NUM"].contains(&t.kind) {
                            return Err(ParseError::new(format!("design の「{}」の値が必要です", k), t.line));
                        }
                        sc.design.insert(k, t.val);
                        self.end_line()?;
                    }
                }
                _ => return Err(self.err0("screen の中は「show」「open」「field」「slot」「action」「page」「read」「design」です")),
            }
            self.end_line()?;
        }
        Ok(sc)
    }

    fn resource(&mut self, line: i64) -> PResult<Resource> {
        let mut rs = Resource { name: self.name()?, line, ..Default::default() };
        while self.peek().val != "{" && self.peek().kind != "NL" && self.peek().kind != "EOF" {
            let kw = self.name()?;
            if kw == "under" {
                rs.under = self.name()?;
            } else if kw == "key" {
                let one = "パスのキーは 1 つです（どのリソースかを決める値だけ。絞り込みの条件は、問い合わせの入力に書けば q[名前]=値 で渡ります）";
                if !rs.keys.is_empty() || self.peek().val == "," {
                    return Err(self.err0(one));
                }
                rs.keys.push(self.name()?);
                if self.peek().val == "," {
                    return Err(self.err0(one));
                }
            } else {
                return Err(self.err0("resource の見出しは「under 親」「key 名前」です"));
            }
        }
        self.block_open()?;
        while !self.accept("}") {
            let l2 = self.peek().line;
            let kw = self.name()?;
            if kw == "index" || kw == "show" {
                let target = self.name()?;
                let actors = self.for_actors()?;
                let mut act = ResAction { kind: kw, target, line: l2, actors, ..Default::default() };
                if self.peek().kind == "NAME" && self.peek().val == "post" {
                    self.next();
                    let t = self.next();
                    if t.kind != "STR" || t.val.trim().is_empty() {
                        return Err(ParseError::new("post には、POST で受ける理由を文字列で書きます（例：post \"氏名で探すので、URL とログに残さない\"）", t.line));
                    }
                    act.post = Some(t.val.trim().to_string());
                    if act.actors.is_none() {
                        act.actors = self.for_actors()?;
                    }
                }
                rs.actions.push(act);
            } else if kw == "create" || kw == "update" || kw == "destroy" {
                let target = self.name()?;
                let actors = self.for_actors()?;
                let mut act = ResAction { kind: kw, target, line: l2, actors, ..Default::default() };
                if self.peek().val == "{" {
                    self.block_open()?;
                    while !self.accept("}") {
                        let v = self.name()?;
                        self.expect("→")?;
                        let n = self.num()?;
                        act.statuses.push((v, n));
                        self.end_line()?;
                    }
                }
                rs.actions.push(act);
            } else {
                let hint = match kw.to_lowercase().as_str() {
                    "get" => "GET は index（一覧）か show（1件）です",
                    "post" => "POST は create です",
                    "put" => "PUT は update です",
                    "delete" => "DELETE は destroy です",
                    "patch" => "PATCH はありません。部分を変えるなら、その部分を別の resource に切り出して update します",
                    _ => "",
                };
                return Err(self.err("resource の中は「index」「show」「create」「update」「destroy」です", hint));
            }
            self.end_line()?;
        }
        Ok(rs)
    }

    fn where_group(&mut self) -> PResult<Vec<Cond3>> {
        let mut group: Vec<Cond3> = vec![];
        loop {
            let f = self.name()?;
            if self.accept("in") {
                if !self.accept("[") {
                    return Err(self.err("in のあとには、候補を [ ] で並べます", "where 項目 in [状態, 状態]（候補は、ここに直に書きます。入力は書けません）"));
                }
                let vals = self.comma_list(|s| if s.peek().kind == "NUM" { Ok(s.num()?.to_string()) } else { s.name() })?;
                self.expect("]")?;
                for v in &vals {
                    if vals.iter().filter(|x| *x == v).count() > 1 {
                        return Err(self.err0(format!("in の候補「{}」を重ねて書いています", v)));
                    }
                }
                for v in vals {
                    group.push((f.clone(), "==".into(), v));
                }
                if !self.accept("or") {
                    break;
                }
                continue;
            }
            let op = self.next().val;
            if op == "=" || (op == "==" && self.is_("[")) {
                return Err(self.err("候補のどれかと同じ、は in で書きます", "where 項目 in [状態, 状態]（= や == [ … ] は書けません）"));
            }
            if !CMP.contains(&op.as_str()) {
                return Err(self.err0("where は「項目 == 状態」「項目 != 状態」「項目 >= 数」「項目 in [状態, 状態]」の形です（or でつなげます）"));
            }
            if self.peek().kind == "NUM" {
                let n = self.num()?;
                group.push((f, op, n.to_string()));
            } else if op == "==" || op == "!=" {
                let n = self.name()?;
                group.push((f, op, n));
            } else {
                return Err(self.err0(format!("「{}」は数との比較にだけ使えます", op)));
            }
            if !self.accept("or") {
                break;
            }
        }
        if self.is_("and") || self.is_(",") {
            return Err(self.unexpected(format!("where の1行の中は or でつなぎます（「{}」は書けません）", self.peek().val), "「かつ」は where の行を分けて書きます（行どうしが and です）"));
        }
        Ok(group)
    }

    fn scope_decl(&mut self, line: i64) -> PResult<Scope> {
        let usage = "scope は `scope 名前 on データ { where 項目 == 状態 ／ filter 項目 equals caller }` と書きます（caller は、呼んだ本人の ID）。引数で絞るなら `scope 名前(引数: 型) on データ { filter 項目 equals 引数 }`";
        let name = self.name()?;
        let mut params = vec![];
        if self.accept("(") {
            params = self.comma_list(|s| s.name_type())?;
            self.expect(")")?;
        }
        if !self.accept("on") {
            return Err(self.err("scope の名前のあとには on データ が要ります", usage));
        }
        let data = self.name()?;
        let mut sc = Scope { name, data, line, params, ..Default::default() };
        self.block_open()?;
        while !self.accept("}") {
            let kw = self.name()?;
            if kw == "where" {
                sc.conds.push(self.where_group()?);
            } else if kw == "filter" {
                let f = self.name()?;
                if self.peek().kind == "OP" && CMP.contains(&self.peek().val.as_str()) {
                    return Err(self.err(
                        format!("filter に「{}」は書けません", self.peek().val),
                        "filter は 項目 equals|at_least|at_most 引数 の形です（== は equals、>= は at_least、<= は at_most）",
                    ));
                }
                let op = self.name()?;
                let j = match Self::filter_op(&op) {
                    Some(j) => j,
                    None => return Err(self.err(format!("scope の filter の種類「{}」は使えません（equals・at_least・at_most）", op), usage)),
                };
                let arg = self.name()?;
                if arg == "caller" {
                    if op != "equals" {
                        return Err(self.err("caller（呼んだ本人の ID）と比べられるのは equals だけです", usage));
                    }
                    if !sc.caller.is_empty() {
                        return Err(self.err("scope に書ける filter 項目 equals caller は 1 つです", usage));
                    }
                    sc.caller = f;
                } else {
                    sc.filters.push((f, j.to_string(), arg));
                }
            } else {
                return Err(self.err(format!("scope の中に「{}」は書けません", kw), usage));
            }
            self.end_line()?;
        }
        if sc.conds.is_empty() && sc.caller.is_empty() && sc.filters.is_empty() {
            return Err(ParseError::new(format!("scope の中身がありません。{}", usage), line));
        }
        Ok(sc)
    }

    fn for_actors(&mut self) -> PResult<Option<Vec<String>>> {
        if !self.accept("for") {
            return Ok(None);
        }
        Ok(Some(self.names()?))
    }

    // ---- トリガ ----
    fn reply_def(&mut self, line: i64) -> PResult<ReplyDef> {
        let name = self.name()?;
        let mut params = vec![];
        if self.accept("(") {
            params = self.comma_list(|s| s.input_item())?;
            self.expect(")")?;
        }
        let mut outcomes = vec![];
        if self.peek().val == "{" {
            self.block_open()?;
            while !self.accept("}") {
                let on = self.name()?;
                let gives = if self.accept("gives") { self.names()? } else { vec![] };
                outcomes.push((on, gives));
                self.end_line()?;
            }
        }
        self.end_line()?;
        Ok(ReplyDef { name, params, line, outcomes })
    }

    fn migration(&mut self, line: i64) -> PResult<Migration> {
        let version = self.num()?;
        self.block_open()?;
        let mut steps: Vec<MigStep> = vec![];
        while !self.accept("}") {
            let t = self.peek().clone();
            if self.accept("rename") {
                if self.accept("entity") {
                    let entity = self.name()?;
                    if !self.accept("to") {
                        return Err(self.unexpected("実体の名前の変更は「rename entity 実体 to 新しい名前」です（to が要ります）", ""));
                    }
                    let n = self.name()?;
                    steps.push(("rename_entity".into(), entity, None, Some(MigVal::To(n)), t.line));
                    self.end_line()?;
                    continue;
                }
                let entity = self.name()?;
                if !self.is_(".") {
                    return Err(self.unexpected(format!("項目の名前を変えるなら「rename {}.項目 to 新しい名前」、実体の名前を変えるなら「rename entity {} to 新しい名前」です", entity, entity), ""));
                }
                self.expect(".")?;
                let fname = self.name()?;
                if !self.accept("to") {
                    return Err(self.unexpected("名前の変更は「rename 実体.項目 to 新しい名前」です（to が要ります）", ""));
                }
                let n = self.name()?;
                steps.push(("rename".into(), entity, Some(fname), Some(MigVal::To(n)), t.line));
                self.end_line()?;
                continue;
            }
            if self.accept("drop") {
                if self.accept("entity") {
                    let n = self.name()?;
                    steps.push(("drop_entity".into(), n, None, None, t.line));
                    self.end_line()?;
                    continue;
                }
                let entity = self.name()?;
                if !self.is_(".") {
                    return Err(self.unexpected(format!("項目を消すなら「drop {}.項目」、実体ごと消すなら「drop entity {}」です", entity, entity), ""));
                }
                self.expect(".")?;
                let n = self.name()?;
                steps.push(("drop".into(), entity, Some(n), None, t.line));
                self.end_line()?;
                continue;
            }
            if !self.accept("add") {
                return Err(self.unexpected(
                    format!(
                        "migration の中は「add 実体.項目 = 値」「rename 実体.項目 to 新しい名前」「drop 実体.項目」「rename entity 実体 to 新しい名前」「drop entity 実体」です（「{}」があります）",
                        shown(&t)
                    ),
                    "",
                ));
            }
            let entity = self.name()?;
            self.expect(".")?;
            let fname = self.name()?;
            if !self.accept("=") {
                return Err(self.unexpected("足した項目には、前からある行へ入れる値を書きます（add 実体.項目 = 値。無いかもしれない項目なら = null）", ""));
            }
            let v = self.next();
            let value = if v.kind == "OP" && v.val == "-" && self.peek().kind == "NUM" {
                MigVal::Int(-self.num()?)
            } else if v.kind == "NUM" {
                if self.accept("/") {
                    let d = self.num()?;
                    MigVal::Ratio(format!("{}/{}", v.val, d))
                } else {
                    MigVal::Int(parse_int(&v.val))
                }
            } else if v.kind == "STR" {
                MigVal::Str(v.val)
            } else if v.kind == "NAME" {
                match v.val.as_str() {
                    "true" => MigVal::Bool(true),
                    "false" => MigVal::Bool(false),
                    "null" => MigVal::Null,
                    _ => MigVal::Name(v.val),
                }
            } else {
                return Err(ParseError::new("add の値は、数・文字列・true/false・null・状態の名前・分子/分母 です", v.line));
            };
            steps.push(("add".into(), entity, Some(fname), Some(value), t.line));
            self.end_line()?;
        }
        self.end_line()?;
        Ok(Migration { version, steps, line })
    }

    fn slot_rule(&mut self, line: i64) -> PResult<SlotRule> {
        let slot = self.name()?;
        self.block_open()?;
        let mut rows = vec![];
        while !self.accept("}") {
            let mut conds: Vec<(String, String)> = vec![];
            if !self.accept("default") {
                conds.push(self.rule_cond()?);
                while self.accept("and") {
                    conds.push(self.rule_cond()?);
                }
            }
            self.expect(":")?;
            let t = self.next();
            if t.kind != "NUM" {
                return Err(ParseError::new("rule の値は、数か 分子/分母 です", t.line));
            }
            let mut val = IntOrStr::Int(parse_int(&t.val));
            if self.accept("/") {
                let d = self.num()?;
                val = IntOrStr::Str(format!("{}/{}", parse_int(&t.val), d));
            }
            rows.push((conds, val));
            self.end_line()?;
        }
        self.end_line()?;
        Ok(SlotRule { slot, rows, line })
    }

    fn rule_cond(&mut self) -> PResult<(String, String)> {
        let mut path = vec![self.name()?];
        while self.accept(".") {
            path.push(self.name()?);
        }
        self.expect("==")?;
        let t = self.next();
        if !["NAME", "STR", "NUM"].contains(&t.kind) {
            return Err(ParseError::new("条件の右は、名前・文字列・数です", t.line));
        }
        Ok((path.join("."), t.val))
    }

    fn read_spec(&mut self) -> PResult<ReadSpec> {
        let mut rd = ReadSpec { line: self.peek().line, ..Default::default() };
        fn text(s: &mut Parser, what: &str) -> PResult<String> {
            let t = s.next();
            if t.kind != "STR" {
                return Err(ParseError::new(format!("{}は、\"…\" の文字列で書いてください", what), t.line));
            }
            Ok(t.val)
        }
        self.block_open()?;
        while !self.accept("}") {
            let kw = self.name()?;
            if kw == "subject" {
                let name = self.name()?;
                if !rd.subject.is_empty() && rd.subject != name {
                    return Err(self.err0(format!("subject の名前は1つにしてください（「{}」と「{}」）", rd.subject, name)));
                }
                rd.subject = name;
                let how = self.name()?;
                if how == "url" {
                    rd.urls.push(text(self, "URL の正規表現")?);
                } else if how == "element" {
                    let sel = text(self, "CSS セレクタ")?;
                    let m = if self.accept("matches") { text(self, "matches の正規表現")? } else { String::new() };
                    rd.elements.push((sel, m));
                } else {
                    return Err(self.err0("subject の読み方は「url」か「element」です"));
                }
            } else if kw == "value" {
                let how = self.name()?;
                if how == "row" {
                    rd.value_rows.extend(self.comma_list(|s| text(s, "見出し"))?);
                } else if how == "lookup" {
                    let k = text(self, "lookup のキー")?;
                    self.expect(":")?;
                    let v = text(self, "lookup の値")?;
                    rd.lookup.insert(k, v);
                } else {
                    return Err(self.err0("value の読み方は「row」か「lookup」です"));
                }
            } else {
                return Err(self.err0("read の中は「subject」「value」です"));
            }
            self.end_line()?;
        }
        Ok(rd)
    }

    fn trigger(&mut self, line: i64) -> PResult<Trigger> {
        let name = self.name()?;
        self.expect("(")?;
        let params = self.comma_list(|s| s.name_type())?;
        self.expect(")")?;
        self.block_open()?;
        let mut causes = vec![];
        while !self.accept("}") {
            let mut kind = self.name()?;
            let key;
            if kind == "reply" {
                key = self.name()?;
            } else if kind == "all" {
                if self.name()? != "reply" {
                    return Err(self.err0("all の後ろには、reply と応答の名前を並べます（例：all reply 決済成功, 在庫確保 → 出荷）"));
                }
                key = self.names()?.join(",");
            } else if kind == "deadline" {
                if self.accept("now") {
                    self.expect("+")?;
                }
                let n = self.num()?;
                let u = self.unit()?;
                key = (n * u).to_string();
            } else {
                return Err(self.err0(format!("トリガの要因は「reply」「all reply」「deadline」です（「{}」は使えません）", kind)));
            }
            self.expect("→")?;
            let to = self.name()?;
            causes.push((kind, key, to));
            self.end_line()?;
        }
        Ok(Trigger { name, params, causes, line })
    }
}

/// "{引数}/相対パス" を (引数, 相対パス) に分ける。引数は [a-z][a-z0-9_]*、相対パスは改行を含まない 1 文字以上
fn split_place(s: &str) -> Option<(String, String)> {
    let rest = s.strip_prefix('{')?;
    let close = rest.find('}')?;
    let arg = &rest[..close];
    let mut cs = arg.chars();
    let first = cs.next()?;
    if !first.is_ascii_lowercase() || !cs.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return None;
    }
    let after = &rest[close + 1..];
    let path = after.strip_prefix('/')?;
    if path.is_empty() || path.contains('\n') {
        return None;
    }
    Some((arg.to_string(), path.to_string()))
}

const DECL_KEYWORDS: [&str; 30] = [
    "entity", "ltype", "store", "state", "result", "effect", "part", "library", "example", "convert", "invariant", "deed", "nexus", "flow", "trigger", "query", "event", "screen", "resource", "actor",
    "scope", "let", "initial", "middle", "final", "reply", "deadline", "slot", "input", "",
];

/// 説明コメント（///）の付いた名前の表示名
pub fn collect_labels(toks: &[Tok]) -> Result<Map<Map<String>>, ParseError> {
    let mut labels: Map<Map<String>> = Map::default();
    for (i, t) in toks.iter().enumerate() {
        if t.doc.is_empty() {
            continue;
        }
        let mut target = if t.kind == "NAME" && !t.val.is_empty() && DECL_KEYWORDS.contains(&t.val.as_str()) && i + 1 < toks.len() { &toks[i + 1] } else { t };
        if t.kind == "NAME" && t.val == "all" && i + 2 < toks.len() && toks[i + 1].val == "reply" {
            target = &toks[i + 2];
        }
        if target.kind != "NAME" {
            return Err(ParseError::at("説明コメント（///）は名前の前に書きます", t.line, t.col, &t.file, ""));
        }
        let new = parse_labels(&t.doc);
        match labels.get(&target.val) {
            None => {
                labels.insert(target.val.clone(), new);
            }
            Some(old) => {
                if old != &new {
                    return Err(ParseError::at(format!("名前「{}」に別の表示名が付いています（同じ名前の表示名は1つにします）", target.val), t.line, t.col, &t.file, ""));
                }
            }
        }
    }
    Ok(labels)
}

const MAX_TZ: i128 = 14 * 3600;

/// `timezone +09:00` を取り除き、組み込みの `tz_offset` をその値に置き換える。(字句の列, ずれ（秒）)
pub fn split_timezone(toks: Vec<Tok>) -> Result<(Vec<Tok>, i128), ParseError> {
    let starts: HashSet<usize> = statement_starts(&toks).into_iter().collect();
    let mut keep: Vec<Tok> = vec![];
    let mut found: Option<i128> = None;
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        if starts.contains(&i) && t.kind == "NAME" && t.val == "timezone" {
            if found.is_some() {
                return Err(ParseError::at("timezone は1つだけ宣言できます", t.line, t.col, &t.file, ""));
            }
            let w: &[Tok] = &toks[(i + 1).min(toks.len())..(i + 5).min(toks.len())];
            if w.len() < 4 || w[0].kind != "OP" || !(w[0].val == "+" || w[0].val == "-") || w[1].kind != "NUM" || w[2].val != ":" || w[3].kind != "NUM" {
                return Err(ParseError::at("timezone は `timezone +09:00` のように、UTC からのずれを時:分で書きます", t.line, t.col, &t.file, ""));
            }
            let secs = parse_int(&w[1].val) * 3600 + parse_int(&w[3].val) * 60;
            if parse_int(&w[3].val) > 59 || secs > MAX_TZ {
                return Err(ParseError::at("timezone のずれは、分が 0〜59、全体で ±14:00 までです", t.line, t.col, &t.file, ""));
            }
            found = Some(if w[0].val == "-" { -secs } else { secs });
            i += 5;
            continue;
        }
        keep.push(t.clone());
        i += 1;
    }
    let off = found.unwrap_or(0);
    let mut out = vec![];
    for t in keep {
        if t.kind == "NAME" && t.val == "tz_offset" {
            let mk = |kind: &'static str, val: String| Tok::at(kind, val, t.line, t.col, &t.file);
            if off >= 0 {
                out.push(mk("NUM", off.to_string()));
            } else {
                out.push(mk("OP", "(".into()));
                out.push(mk("NUM", "0".into()));
                out.push(mk("OP", "-".into()));
                out.push(mk("NUM", (-off).to_string()));
                out.push(mk("OP", ")".into()));
            }
        } else {
            out.push(t);
        }
    }
    Ok((out, off))
}

pub fn parse_toks(toks: Vec<Tok>, origin: Map<(String, String)>) -> Result<Program, ParseError> {
    let (toks, tz) = split_timezone(toks)?;
    let mut ps = Parser::new(toks.clone());
    let res = ps.program();
    let mut p = match res {
        Ok(p) => p,
        Err(mut e) => {
            if e.file.is_empty() && e.col == 0 {
                e.file = ps.peek().file.clone();
            }
            let h = brace_hint(&toks);
            if !h.is_empty() && !e.hint.contains(&h) {
                e.hint = if e.hint.is_empty() { h } else { format!("{}。{}", e.hint, h) };
            }
            return Err(e);
        }
    };
    p.tz_offset = tz;
    p.labels = collect_labels(&toks)?;
    p.origin = origin;
    Ok(p)
}

/// path は、use でモジュールを取り込むときの基準になるファイル（文字列から読むときは無し）
pub fn parse(src: &str, path: Option<&Path>) -> Result<Program, Error> {
    let toks = lex(src)?;
    let (toks, origin) = expand(toks, path)?;
    Ok(parse_toks(toks, Map(origin))?)
}
