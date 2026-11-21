// SPDX-License-Identifier: GPL-3.0-or-later
//! 構文木。Python 版 geas/ast.py の移し。各構造体は、Python 版の dataclass と同じ名前・同じ項目で、
//! serde で JSON にすると Python 版の構文木ダンプ（tools/golden.py の astdump）と同じ形になる。
use indexmap::IndexMap;
use serde::{Serialize, Serializer};
use std::cell::OnceCell;

/// 名前（文字列）をキーにした、挿入順を保つ辞書（Python の dict に当たる）。構文木ダンプでは {"_dict": [[キー, 値], …]} になる
#[derive(Debug, Clone, PartialEq)]
pub struct Map<V>(pub IndexMap<String, V>);

impl<V> Default for Map<V> {
    fn default() -> Self {
        Map(IndexMap::new())
    }
}
impl<V> std::ops::Deref for Map<V> {
    type Target = IndexMap<String, V>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<V> std::ops::DerefMut for Map<V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl<'a, V> IntoIterator for &'a Map<V> {
    type Item = (&'a String, &'a V);
    type IntoIter = indexmap::map::Iter<'a, String, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
impl<V> IntoIterator for Map<V> {
    type Item = (String, V);
    type IntoIter = indexmap::map::IntoIter<String, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
impl<V: Serialize> Serialize for Map<V> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let pairs: Vec<(&String, &V)> = self.0.iter().collect();
        let mut m = s.serialize_map(Some(1))?;
        m.serialize_entry("_dict", &pairs)?;
        m.end()
    }
}

pub fn show_ja(k: &str) -> Option<&'static str> {
    match k {
        "JPY" => Some("円"),
        "USD" => Some("ドル"),
        "EUR" => Some("ユーロ"),
        "floor" => Some("切り捨て"),
        "ceil" => Some("切り上げ"),
        "half_up" => Some("四捨五入"),
        _ => None,
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(tag = "_")]
pub struct Range {
    pub lo: Option<i128>,
    pub hi: Option<i128>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(tag = "_")]
pub struct Type {
    pub kind: String,
    pub range: Option<Range>,
    pub target: Option<String>,
    pub key: Option<String>,
    pub value: Option<Box<Type>>,
    pub unit: Option<String>,
    pub optional: bool,
    pub chars: Option<Vec<(u32, u32)>>,
    pub fills: Option<Vec<(String, i128)>>,
    pub maxn: Option<i128>,
    pub read: Option<String>,
    pub trust: Option<(String, String)>,
    pub parts: Option<Vec<(String, Type)>>,
}

impl Type {
    pub fn new(kind: &str) -> Type {
        Type { kind: kind.to_string(), ..Default::default() }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct Field {
    pub name: String,
    pub r#type: Type,
    pub unique: bool,
    pub required: bool,
    pub line: i64,
    pub expr: Option<Expr>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct Data {
    pub name: String,
    pub fields: Vec<Field>,
    pub line: i64,
    pub store: Option<String>,
    pub keep: bool,
    pub removable: Vec<String>,
    pub on_delete: Vec<(String, String, String)>,
    pub derived: Vec<Field>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct State {
    pub name: String,
    pub initial: String,
    pub middle: Vec<String>,
    #[serde(rename = "final")]
    pub final_: Vec<String>,
    pub rejects: Vec<(String, String, String)>,
    pub endless: bool,
    pub plain: bool,
    pub line: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct ResultDef {
    pub name: String,
    pub variants: Vec<(String, Vec<(String, Type)>)>,
    pub line: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct HttpDecl {
    pub method: String,
    pub url: String,
    pub bearer: Option<String>,
    pub body: Vec<(String, String)>,
    pub statuses: Vec<(i128, i128, String, Vec<(String, String)>)>,
    pub other: Option<(String, Vec<(String, String)>)>,
    pub line: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct FileDecl {
    pub op: String,
    pub dir: String,
    pub rest: String,
    pub src: String,
    pub rows: Map<(String, Vec<(String, String)>)>,
    pub line: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct EffectDecl {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub line: i64,
    pub failures: Vec<String>,
    pub no_wait: Option<String>,
    pub joins: Option<String>,
    pub replies: Option<String>,
    pub http: Option<HttpDecl>,
    pub file: Option<FileDecl>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct Part {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub ret: Type,
    pub guarantee: Option<Expr>,
    pub line: i64,
    pub body: Option<Expr>,
    pub defaults: Map<Expr>,
    pub requires: Vec<(Expr, String)>,
    pub examples: Vec<(Vec<Expr>, Expr, i64)>,
    pub lib: Option<(String, String, String, String, String)>,
}

impl Part {
    /// 根の言語のライブラリを呼ぶ部品（library）
    pub fn external(&self) -> bool {
        self.lib.is_some()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "_")]
pub struct Invariant {
    pub data: String,
    pub cond: Expr,
    pub line: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct Flow {
    pub name: String,
    pub entries: Vec<(String, Vec<(String, String)>)>,
    pub line: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct Trigger {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub causes: Vec<(String, String, String)>,
    pub line: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct ReplyDef {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub line: i64,
    pub outcomes: Vec<(String, Vec<String>)>,
}

/// migration の add／rename などの値：["int", 5]・["str", "x"]・["bool", true]・["null", null]・["name", "状態"]・["ratio", "n/d"]・["to", "新しい名前"]
#[derive(Debug, Clone, PartialEq)]
pub enum MigVal {
    Int(i128),
    Str(String),
    Bool(bool),
    Null,
    Name(String),
    Ratio(String),
    To(String),
}

impl Serialize for MigVal {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeTuple;
        let mut t = s.serialize_tuple(2)?;
        match self {
            MigVal::Int(n) => {
                t.serialize_element("int")?;
                t.serialize_element(n)?;
            }
            MigVal::Str(v) => {
                t.serialize_element("str")?;
                t.serialize_element(v)?;
            }
            MigVal::Bool(b) => {
                t.serialize_element("bool")?;
                t.serialize_element(b)?;
            }
            MigVal::Null => {
                t.serialize_element("null")?;
                t.serialize_element(&Option::<i32>::None)?;
            }
            MigVal::Name(v) => {
                t.serialize_element("name")?;
                t.serialize_element(v)?;
            }
            MigVal::Ratio(v) => {
                t.serialize_element("ratio")?;
                t.serialize_element(v)?;
            }
            MigVal::To(v) => {
                t.serialize_element("to")?;
                t.serialize_element(v)?;
            }
        }
        t.end()
    }
}

/// (すること, 実体, 項目, 中身, 行)
pub type MigStep = (String, String, Option<String>, Option<MigVal>, i64);

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct Migration {
    pub version: i128,
    pub steps: Vec<MigStep>,
    pub line: i64,
}

/// 数か 分子/分母 の文字列
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum IntOrStr {
    Int(i128),
    Str(String),
}

impl Default for IntOrStr {
    fn default() -> Self {
        IntOrStr::Str(String::new())
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct SlotRule {
    pub slot: String,
    pub rows: Vec<(Vec<(String, String)>, IntOrStr)>,
    pub line: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct ReadSpec {
    pub subject: String,
    pub urls: Vec<String>,
    pub elements: Vec<(String, String)>,
    pub value_rows: Vec<String>,
    pub lookup: Map<String>,
    pub line: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct Process {
    pub name: String,
    pub inputs: Vec<(String, Type)>,
    pub refs: Vec<(String, Type)>,
    pub reads: Vec<String>,
    pub writes: Vec<String>,
    pub transitions: Vec<(String, String, String)>,
    pub result: String,
    pub effects: Vec<String>,
    pub body: Vec<Stmt>,
    pub line: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct Column {
    pub name: String,
    pub kind: String,
    pub field: String,
    pub via: Option<String>,
}

pub type Cond3 = (String, String, String);

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct Query {
    pub name: String,
    pub inputs: Vec<(String, Type)>,
    pub refs: Vec<(String, Type)>,
    pub target: String,
    pub conds: Vec<Vec<Cond3>>,
    pub filters: Vec<Cond3>,
    pub order: Option<(String, String)>,
    pub orders: Vec<(String, String)>,
    pub page: Option<String>,
    pub page_n: Option<i128>,
    pub select: Vec<Column>,
    pub reads: Vec<String>,
    pub line: i64,
    pub scopes: Vec<String>,
    pub scope_args: Map<Vec<String>>,
}

pub const CALLER: &str = "caller";
pub const AFTER: &str = "after";
pub const ORDER: &str = "order";

pub fn order_word(d: &str) -> &'static str {
    match d {
        "降順" => "desc",
        _ => "asc",
    }
}

/// 呼ぶ側が選べる並び：(q[order] に書く値, 項目, 降順／昇順)。並びが 1 行以下なら空（選べない）
pub fn order_choices(q: &Query) -> Vec<(String, String, String)> {
    if q.orders.len() > 1 {
        q.orders.iter().map(|(f, d)| (format!("{}.{}", f, order_word(d)), f.clone(), d.clone())).collect()
    } else {
        vec![]
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct Scope {
    pub name: String,
    pub data: String,
    pub conds: Vec<Vec<Cond3>>,
    pub caller: String,
    pub params: Vec<(String, Type)>,
    pub filters: Vec<Cond3>,
    pub line: i64,
}

/// 問い合わせが使う範囲（書いた順。未定義の名前は除く）
pub fn scopes_of<'a>(p: &'a Program, q: &Query) -> Vec<&'a Scope> {
    let mut by: IndexMap<&str, &Scope> = IndexMap::new();
    for s in p.scopes.iter().rev() {
        by.insert(s.name.as_str(), s);
    }
    q.scopes.iter().filter_map(|n| by.get(n.as_str()).copied()).collect()
}

pub fn uses_caller(p: &Program, q: &Query) -> bool {
    scopes_of(p, q).iter().any(|s| !s.caller.is_empty())
}

pub fn paged(q: &Query) -> bool {
    q.page.is_some() || q.page_n.is_some()
}

pub fn gen_inputs(p: &Program, q: &Query) -> Vec<(String, Type)> {
    let mut out: Vec<(String, Type)> = q.inputs.clone();
    if uses_caller(p, q) {
        out.push((CALLER.to_string(), Type::new("ID")));
    }
    if !order_choices(q).is_empty() {
        out.push((ORDER.to_string(), Type { optional: true, ..Type::new("文字列") }));
    }
    if paged(q) {
        out.push((AFTER.to_string(), Type { optional: true, ..Type::new("ID") }));
    }
    out
}

pub fn gen_filters(p: &Program, q: &Query) -> Vec<Cond3> {
    let mut out = q.filters.clone();
    for s in scopes_of(p, q) {
        let args: Vec<String> = q.scope_args.get(&s.name).cloned().unwrap_or_default();
        let given: IndexMap<&str, &str> = s.params.iter().map(|(n, _)| n.as_str()).zip(args.iter().map(|a| a.as_str())).collect();
        for (f, op, a) in &s.filters {
            if let Some(g) = given.get(a.as_str()) {
                out.push((f.clone(), op.clone(), g.to_string()));
            }
        }
        if !s.caller.is_empty() {
            out.push((s.caller.clone(), "一致".to_string(), CALLER.to_string()));
        }
    }
    out
}

pub fn gen_conds(p: &Program, q: &Query) -> Vec<Vec<Cond3>> {
    let mut out = q.conds.clone();
    for s in scopes_of(p, q) {
        out.extend(s.conds.iter().cloned());
    }
    out
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct Event {
    pub name: String,
    pub steps: Vec<(String, String, String)>,
    pub on: String,
    pub line: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct Screen {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub shows: Vec<String>,
    pub opens: Vec<String>,
    pub fields: Vec<String>,
    pub field_from: Map<String>,
    pub slots: Vec<String>,
    pub actions: Vec<(String, Vec<(String, String)>)>,
    pub design: Map<String>,
    pub entry: bool,
    pub read: Option<ReadSpec>,
    pub pages: Vec<String>,
    pub line: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "_")]
pub struct ResidentDecl {
    pub tooltip: String,
    pub start: String,
    pub click: String,
    pub menu: Vec<(String, String, String, i64)>,
    pub line: i64,
}

impl Default for ResidentDecl {
    fn default() -> Self {
        ResidentDecl { tooltip: String::new(), start: "shown".into(), click: "toggle".into(), menu: vec![], line: 0 }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct ResAction {
    pub kind: String,
    pub target: String,
    pub statuses: Vec<(String, i128)>,
    pub line: i64,
    pub actors: Option<Vec<String>>,
    pub post: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct Resource {
    pub name: String,
    pub under: String,
    pub keys: Vec<String>,
    pub actions: Vec<ResAction>,
    pub line: i64,
}

/// コンテナ層の宣言（基盤に依存しない。Dockerfile の元）
#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct ContainerDecl {
    pub port: i64,
    pub data: String,
    pub secrets: Vec<String>,
    pub line: i64,
}

/// 基盤層の宣言（いまは vm_docker だけ）
#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct InfraDecl {
    pub kind: String,
    pub name: String,
    pub host_port: i64,
    pub via: String,
    pub host_name: String,
    pub volume_host: String,
    pub env_file: String,
    pub line: i64,
}

/// needs の値：文字列（secret・exposure は名前か空）か整数（instances の上限）
pub type NeedVal = IntOrStr;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct Program {
    pub data: Vec<Data>,
    pub states: Vec<State>,
    pub results: Vec<ResultDef>,
    pub effects: Vec<EffectDecl>,
    pub parts: Vec<Part>,
    pub invariants: Vec<Invariant>,
    pub processes: Vec<Process>,
    pub flows: Vec<Flow>,
    pub triggers: Vec<Trigger>,
    pub replies: Vec<ReplyDef>,
    pub rules: Vec<SlotRule>,
    pub migrations: Vec<Migration>,
    pub queries: Vec<Query>,
    pub events: Vec<Event>,
    pub screens: Vec<Screen>,
    pub resources: Vec<Resource>,
    pub scopes: Vec<Scope>,
    pub actors: Vec<(String, i64)>,
    pub composites: Vec<Type>,
    pub origin: Map<(String, String)>,
    pub ltype_names: Vec<String>,
    pub ltype_lines: Map<i64>,
    pub isolations: Map<(String, String, String, i64)>,
    pub paths: Vec<(String, String, Vec<(String, String, i64)>, i64)>,
    pub data_folder: Option<(String, i64)>,
    pub data_folders: Vec<(String, String, i64)>,
    pub resident: Option<ResidentDecl>,
    pub needs: Vec<(String, String, NeedVal, String, i64)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<ContainerDecl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub infra: Option<InfraDecl>,
    pub title: String,
    pub tz_offset: i128,
    pub labels: Map<Map<String>>,
    #[serde(skip)]
    pub show_table_cache: OnceCell<IndexMap<String, String>>,
}

impl Program {
    /// 人が書いた名前（表示名でも、コードの名前でもよい）を、候補の中のコードの名前にする。無ければそのまま返す
    pub fn resolve(&self, text: &str, names: &[String]) -> String {
        let text = text.trim();
        if names.iter().any(|n| n == text) {
            return text.to_string();
        }
        for n in names {
            if let Some(ls) = self.labels.get(n) {
                if ls.get("ja").map(|s| s.as_str()) == Some(text) || ls.get("").map(|s| s.as_str()) == Some(text) {
                    return n.clone();
                }
            }
        }
        text.to_string()
    }

    /// AI に見せる文字列：名前はコードの名前のまま。use で取り込んだモジュールの定義だけ、書いたとおりの 名前.定義名 に戻す
    pub fn show_code(&self, text: &str) -> String {
        if self.origin.is_empty() {
            return text.to_string();
        }
        let table: IndexMap<&str, String> = self.origin.iter().map(|(m, (module, n))| (m.as_str(), format!("{}.{}", module, n))).collect();
        name_word_re().replace_all(text, |c: &regex::Captures| table.get(&c[0]).cloned().unwrap_or_else(|| c[0].to_string())).into_owned()
    }

    /// 人に見せる文字列の中の名前を表示名（言語に合うもの、無ければ既定）に置き換える。生成する Rust には使わない
    pub fn show_names(&self, text: &str) -> String {
        if self.labels.is_empty() && self.origin.is_empty() {
            return text.to_string();
        }
        let table = self.show_table();
        name_word_re().replace_all(text, |c: &regex::Captures| table.get(&c[0]).cloned().unwrap_or_else(|| c[0].to_string())).into_owned()
    }

    /// 名前 → 表示名の表
    pub fn show_table(&self) -> &IndexMap<String, String> {
        self.show_table_cache.get_or_init(|| {
            let mut table: IndexMap<String, String> = self.origin.iter().map(|(m, (module, n))| (m.clone(), format!("{}.{}", module, n))).collect();
            for (n, ls) in &self.labels {
                // 英語モードは @en、日本語モードは @ja を先に使い、無ければ既定（タグなし）、最後にもう一方の言語
                let (first, last) = if crate::i18n::lang() == crate::i18n::Lang::En { ("en", "ja") } else { ("ja", "en") };
                let v = ls.get(first).filter(|s| !s.is_empty()).or_else(|| ls.get("").filter(|s| !s.is_empty())).or_else(|| ls.get(last).filter(|s| !s.is_empty()));
                if let Some(v) = v {
                    table.insert(n.clone(), v.clone());
                }
            }
            table
        })
    }
}

pub fn name_word_re() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"[A-Za-z_][A-Za-z0-9_]*").unwrap())
}

// ---- 式 ----
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "_")]
pub enum Expr {
    Num { v: i128 },
    /// 値の組。いまは冪等キーだけに使う
    Tuple { items: Vec<Expr> },
    Not { e: Box<Expr> },
    BoolLit { v: bool },
    StrLit { s: String },
    Cond { c: Box<Expr>, a: Box<Expr>, b: Box<Expr> },
    Null,
    Dur { secs: i128, text: String },
    Name { n: String },
    Attr { e: Box<Expr>, n: String },
    Index { d: String, e: Box<Expr>, scopes: Vec<String> },
    Call { n: String, args: Vec<Expr> },
    Make { n: String, fields: Vec<(String, Expr)>, line: i64 },
    Bin { op: String, l: Box<Expr>, r: Box<Expr>, dflt: Option<Box<Expr>> },
    SetB { elem: Box<Expr>, binder: Binder, cond: Option<Box<Expr>> },
    SumE { elem: Box<Expr>, binder: Binder, cond: Option<Box<Expr>> },
    Pred { e: Box<Expr>, kind: String },
    Implies { l: Box<Expr>, r: Box<Expr> },
}

impl Expr {
    pub fn num(v: i128) -> Expr {
        Expr::Num { v }
    }
    pub fn name(n: &str) -> Expr {
        Expr::Name { n: n.to_string() }
    }
    pub fn bin(op: &str, l: Expr, r: Expr) -> Expr {
        Expr::Bin { op: op.to_string(), l: Box::new(l), r: Box::new(r), dflt: None }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "_")]
pub struct Binder {
    pub names: Vec<String>,
    pub src: Box<Expr>,
}

// ---- 文 ----
#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "_")]
pub struct Return {
    pub variant: String,
    pub args: Vec<Expr>,
    pub line: i64,
    pub calc: Option<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "_")]
pub struct Assign {
    pub name: String,
    pub e: Expr,
    pub absent: Option<Return>,
    pub line: i64,
    pub calc: Option<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "_")]
pub struct Create {
    pub name: String,
    pub data: String,
    pub fields: Vec<(String, Expr)>,
    pub line: i64,
    pub calc: Option<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "_")]
pub struct Delete {
    pub var: String,
    pub line: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "_")]
pub struct If {
    pub cond: Expr,
    pub body: Vec<Stmt>,
    pub line: i64,
    pub els: Option<Vec<Stmt>>,
    pub else_line: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "_")]
pub struct Update {
    pub target: Expr,
    pub op: String,
    pub value: Expr,
    pub binder: Binder,
    pub cond: Option<Expr>,
    pub line: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "_")]
pub struct SetField {
    pub var: String,
    pub f: String,
    pub e: Expr,
    pub line: i64,
    pub op: String,
    pub guard: Option<(String, String)>,
    pub calc: Option<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "_")]
pub struct Request {
    pub effect: String,
    pub args: Vec<Expr>,
    pub idem: Option<Expr>,
    pub line: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "_")]
pub struct ForEach {
    pub binder: Binder,
    pub body: Vec<Stmt>,
    pub line: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Stmt {
    Return(Return),
    Assign(Assign),
    Create(Create),
    Delete(Delete),
    If(If),
    Update(Update),
    SetField(SetField),
    Request(Request),
    ForEach(ForEach),
}

/// 文の並びが、どの道でも return で終わるか
pub fn always_returns(stmts: &[Stmt]) -> bool {
    match stmts.last() {
        None => false,
        Some(Stmt::Return(_)) => true,
        Some(Stmt::If(i)) => i.els.as_ref().map_or(false, |e| always_returns(&i.body) && always_returns(e)),
        _ => false,
    }
}

/// 結果の種類 → 行き先のならび
pub fn route_map(routes: &[(String, String)]) -> IndexMap<String, Vec<String>> {
    let mut m: IndexMap<String, Vec<String>> = IndexMap::new();
    for (v, d) in routes {
        m.entry(v.clone()).or_default().push(d.clone());
    }
    m
}

pub fn show_range(r: &Option<Range>) -> String {
    match r {
        None => String::new(),
        Some(r) => format!(" {}..{}", r.lo.map_or(String::new(), |v| v.to_string()), r.hi.map_or(String::new(), |v| v.to_string())),
    }
}

pub fn show_chars(cs: &[(u32, u32)]) -> String {
    let esc = |c: u32| -> String {
        let ch = char::from_u32(c).unwrap_or('\u{fffd}');
        if "\\-]".contains(ch) {
            format!("\\{}", ch)
        } else {
            ch.to_string()
        }
    };
    cs.iter().map(|&(a, b)| if a == b { esc(a) } else { format!("{}-{}", esc(a), esc(b)) }).collect()
}

/// 型を、仕様書に出す形にする。知らない種類のときは Err（Python 版は ValueError）
pub fn show_type(t: &Type) -> Result<String, String> {
    let k = t.kind.as_str();
    Ok(match k {
        "ID" | "日時" | "真偽" => k.to_string(),
        "文字列" => {
            "文字列".to_string()
                + &show_range(&t.range)
                + &(match &t.chars {
                    None => String::new(),
                    Some(c) => format!(" 文字種 [{}]", show_chars(c)),
                })
                + &(match &t.read {
                    None => String::new(),
                    Some(r) => format!(" 変換 {}", r),
                })
        }
        "バイト列" => "バイト列".to_string() + &show_range(&t.range),
        "生" => "raw（未検査の生データ）".to_string(),
        "整数" => "整数".to_string() + &show_range(&t.range),
        "割合" => "割合".to_string() + &show_range(&t.range),
        "金額" => {
            let u = t.unit.clone().unwrap_or_else(|| "JPY".to_string());
            let head = if let Some(rest) = u.strip_prefix('$') {
                format!("金額(通貨:{})", rest)
            } else {
                format!("金額({})", show_ja(&u).map(|s| s.to_string()).unwrap_or(u.clone()))
            };
            head + &show_range(&t.range)
        }
        "参照" => format!("{} を参照", t.target.clone().unwrap_or_default()),
        "名前" => t.target.clone().unwrap_or_default(),
        "合成" => {
            let mut ps = Vec::new();
            for (n, pt) in t.parts.as_deref().unwrap_or(&[]) {
                ps.push(format!("{}: {}", n, show_type(pt)?));
            }
            format!("{}（{}）", t.target.clone().unwrap_or_default(), ps.join("、"))
        }
        "写像" => {
            format!("{} → {}", t.key.clone().unwrap_or_default(), show_type(t.value.as_deref().unwrap())?)
                + &t.maxn.map_or(String::new(), |n| format!("（最大 {} 件）", n))
        }
        "集合" => format!("{} の集合", show_type(t.value.as_deref().unwrap())?) + &t.maxn.map_or(String::new(), |n| format!("（最大 {} 件）", n)),
        _ => return Err(k.to_string()),
    })
}

pub fn quote_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn op_prec(op: &str) -> i32 {
    match op {
        "または" => 1,
        "かつ" => 2,
        "in" | "==" | "!=" | "<" | "<=" | ">" | ">=" => 4,
        "+" | "-" => 5,
        "*" | "/" | "%" => 6,
        _ => 4,
    }
}

pub fn prec(e: &Expr) -> i32 {
    match e {
        Expr::Cond { .. } | Expr::Implies { .. } => 0,
        Expr::Bin { op, .. } => op_prec(op),
        Expr::Not { .. } => 3,
        Expr::Pred { .. } => 4,
        _ => 9,
    }
}

pub fn show_in(e: &Expr, least: i32) -> String {
    let s = show(e);
    if prec(e) < least {
        format!("({})", s)
    } else {
        s
    }
}

pub fn show(e: &Expr) -> String {
    match e {
        Expr::Num { v } => v.to_string(),
        Expr::Null => "null".to_string(),
        Expr::StrLit { s } => quote_str(s),
        Expr::BoolLit { v } => if *v { "true" } else { "false" }.to_string(),
        Expr::Not { e } => format!("not {}", show_in(e, 3)),
        Expr::Cond { c, a, b } => format!("if {} then {} else {}", show(c), show(a), show(b)),
        Expr::Dur { secs, text } => if text.is_empty() { format!("{} sec", secs) } else { text.clone() },
        Expr::Tuple { items } => format!("({})", items.iter().map(show).collect::<Vec<_>>().join(", ")),
        Expr::Name { n } => n.clone(),
        Expr::Attr { e, n } => format!("{}.{}", show(e), n),
        Expr::Index { d, e, .. } => format!("{}[{}]", d, show(e)),
        Expr::Call { n, args } => format!("{}({})", n, args.iter().map(show).collect::<Vec<_>>().join(", ")),
        Expr::Make { n, fields, .. } => format!("{} {{ {} }}", n, fields.iter().map(|(k, x)| format!("{}: {}", k, show(x))).collect::<Vec<_>>().join(", ")),
        Expr::Bin { op, l, r, dflt } => {
            let p = prec(e);
            let _ = op;
            format!("{} {} {}", show_in(l, p), match e { Expr::Bin { op, .. } => op.as_str(), _ => "" }, show_in(r, p + 1))
                + &dflt.as_ref().map_or(String::new(), |d| format!(" or {}", show(d)))
        }
        Expr::Pred { e, kind } => format!("{} が {}", show_in(e, 5), kind),
        Expr::Implies { l, r } => format!("{} なら {}", show(l), show(r)),
        Expr::SetB { elem, binder, cond } | Expr::SumE { elem, binder, cond } => {
            let names = if binder.names.len() == 1 { binder.names[0].clone() } else { format!("({})", binder.names.join(", ")) };
            let head = if matches!(e, Expr::SumE { .. }) { "Σ " } else { "" };
            let c = cond.as_ref().map_or(String::new(), |c| format!(", {}", show(c)));
            format!("{}{{ {} | {} in {}{} }}", head, show(elem), names, show(&binder.src), c)
        }
    }
}
