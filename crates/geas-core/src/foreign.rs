// SPDX-License-Identifier: GPL-3.0-or-later
//! 他の言語の書き方（`&&`・`elif`・`string`・`True` など）で書いたときの、直し方の手がかり。Python 版 geas/foreign.py の移し。
use crate::ast::quote_str;
use crate::lexer::{file_name, Tok};
use indexmap::IndexMap;

fn table(rows: &[(&[&str], &'static str)]) -> IndexMap<&'static str, &'static str> {
    let mut m = IndexMap::new();
    for (keys, v) in rows {
        for k in *keys {
            // Python の dict.fromkeys を並べて ** で足す形と同じ：あとの行が勝つ
            let k: &'static str = Box::leak(k.to_string().into_boxed_str());
            m.insert(k, *v);
        }
    }
    m
}

pub fn foreign_types() -> &'static IndexMap<&'static str, &'static str> {
    use std::sync::OnceLock;
    static T: OnceLock<IndexMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| {
        table(&[
            (&["string", "String", "str", "Str", "varchar", "char", "Text"], "文字列は text です（長さと文字種は ltype で text 1..80 のように書きます）"),
            (&["boolean", "Boolean", "Bool"], "真偽は bool です"),
            (
                &["integer", "Integer", "Int", "number", "Number", "long", "short", "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64", "u128", "usize", "isize"],
                "整数は int です（範囲は ltype で int 0..99 のように書き、Rust の型は範囲から選ばれます）",
            ),
            (
                &["float", "double", "f32", "f64", "decimal", "Decimal", "real", "numeric"],
                "小数の型はありません。金額は money(JPY)（最小単位の整数）、割合は ratio、それ以外は単位を決めて int で持ちます",
            ),
            (&["uuid", "UUID", "Uuid", "ID", "Id"], "識別子は id です"),
            (&["datetime", "DateTime", "timestamp", "Timestamp", "date", "Date", "Instant", "Time"], "日時は time です（暦の日付は vtime）"),
            (&["blob", "binary", "Bytes", "bytea"], "バイト列は bytes です"),
            (&["list", "List", "array", "Array", "Vec", "vector", "Set"], "並びは、集合なら ltype に set of id、数を持つなら写像 Product → Quantity を書きます"),
            (&["map", "Map", "dict", "Dict", "HashMap", "object"], "写像は ltype に Product → Quantity の形で書きます"),
            (&["enum", "Enum"], "選択肢の並びは、状態なら state、処理の結果なら result で書きます"),
            (&["Money", "Currency", "Ratio", "Raw", "Vtime"], "組み込みの型の名前は小文字です（money・currency・ratio・raw・vtime）"),
        ])
    })
}

/// 未定義の型名への案内（他の言語の型名・? 付き・Option）。無ければ空
pub fn type_hint(name: &str) -> String {
    let base = name.split('<').next().unwrap_or("");
    if name.ends_with('?') || ["Option", "Optional", "Maybe", "Nullable"].contains(&base) {
        return "（? や Option は書きません。項目と入力は、書かなければ無いかもしれない（null になりうる）扱いで、必ずあるものに required を付けます）".to_string();
    }
    match foreign_types().get(base) {
        Some(h) => format!("（{}）", h),
        None => String::new(),
    }
}

pub fn foreign_names() -> &'static IndexMap<&'static str, &'static str> {
    use std::sync::OnceLock;
    static T: OnceLock<IndexMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| {
        table(&[
            (&["True", "TRUE", "False", "FALSE"], "真偽は true／false と小文字で書きます"),
            (&["None", "nil", "NULL", "Null", "undefined", "nullptr"], "値が無いことは null です（確かめるのは x is null）"),
            (&["this", "self"], "this・self はありません。データは let x = Entity[キー] で引いて、x.項目 で読みます"),
            (&["Now", "today", "current_time", "current_timestamp"], "いまの時刻は now です（期間は 30 min のように書きます）"),
        ])
    })
}

/// 定義されていない名前への案内。無ければ空
pub fn name_hint(name: &str) -> String {
    if name.starts_with('\'') || name.ends_with('\'') {
        return "（文字列は \"…\" で囲みます。' は使えません）".to_string();
    }
    match foreign_names().get(name) {
        Some(h) => format!("（{}）", h),
        None => String::new(),
    }
}

pub const ELSE: &str = "else は、if 条件 { … } の閉じる } と同じ行に続けて書きます（} else { … }、} else if 条件 { … }）。let の else も、let x = … else { return … } と同じ行に書きます";
pub const ELIF: &str = "else if 条件 { … } と書きます（elif・elsif はありません）";

/// エラーに出す字句の見せ方
pub fn shown(t: &Tok) -> String {
    match t.kind {
        "NL" => "行末".to_string(),
        "EOF" => "ファイルの終わり".to_string(),
        "STR" => quote_str(&t.val),
        _ => t.val.clone(),
    }
}

/// 位置 i の字句が、他の言語の書き方らしいときの手がかり。無ければ空
pub fn token_hint(toks: &[Tok], i: usize) -> String {
    let t = &toks[i.min(toks.len() - 1)];
    let nxt = if i + 1 < toks.len() { &toks[i + 1] } else { t };
    let v: &str = if t.kind == "NAME" || t.kind == "OP" { &t.val } else { "" };
    let nxt_op = |s: &str| nxt.kind == "OP" && nxt.val == s;
    if v == "&&" || v == "&" {
        return "「かつ」は and と書きます（&& は使えません）".into();
    }
    if (v == "||" || v == "|") && (v == "||" || nxt_op("|")) {
        return "「または」は or と書きます（|| は使えません）".into();
    }
    if v == "!" || (v.starts_with('!') && v.chars().count() > 1) {
        return "否定は not と書きます（! は使えません。等しくないは !=）".into();
    }
    if v == ";" {
        return "文の区切りは改行です（; は書きません）".into();
    }
    if v == "=" && nxt_op(">") {
        return "矢印は → か -> と書きます（=> ではありません）".into();
    }
    if v == "==" && nxt_op("=") {
        return "等しいは == です（=== はありません）".into();
    }
    if (v == "+" || v == "-") && nxt_op(v) {
        return format!("1 だけ{}なら 名前.項目 {}= 1 と書きます（{}{} はありません）", if v == "+" { "足す" } else { "引く" }, v, v, v);
    }
    if v == "/" && nxt_op("*") {
        return "コメントは // から行末です（/* */ はありません）".into();
    }
    if v == "mut" {
        return "let に mut は付けません（let の変数は作り直さず、項目の更新は 名前.項目 = 値 と書きます）".into();
    }
    if v == "else" {
        return ELSE.into();
    }
    if v == "?" || v.ends_with('?') {
        return "? はありません。値を選ぶ式は if 条件 then 式 else 式、無いかもしれない項目は書かなければそう扱われます".into();
    }
    if ["elif", "elsif", "elseif"].contains(&v) {
        return ELIF.into();
    }
    if t.kind == "NAME" && (v.starts_with('\'') || v.ends_with('\'')) {
        return "文字列は \"…\" で囲みます（' は使えません）".into();
    }
    if let Some(h) = foreign_names().get(v) {
        return h.to_string();
    }
    String::new()
}

pub const TOP_KEYWORDS: [&str; 34] = [
    "app", "timezone", "use", "module", "ltype", "entity", "store", "storage", "state", "result", "effect", "reply", "library", "example", "convert", "invariant", "deed", "nexus", "flow",
    "trigger", "rule", "migration", "query", "event", "screen", "resource", "actor", "scope", "needs", "resident", "path", "data_folder", "container", "infra",
];

pub fn top_keywords() -> Vec<&'static str> {
    TOP_KEYWORDS.to_vec()
}

fn def_hints() -> &'static IndexMap<&'static str, &'static str> {
    use std::sync::OnceLock;
    static T: OnceLock<IndexMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| {
        table(&[
            (&["type", "typedef", "alias", "newtype"], "型に名前を付けるのは ltype です（ltype Quantity = int 1..99）"),
            (&["enum"], "選択肢の並びは、状態なら state、処理の結果なら result で書きます"),
            (&["struct", "class", "model", "table", "record", "schema", "interface", "object", "data"], "保存するデータは entity、値の組は ltype 名前 { 成分: 型 } で書きます"),
            (&["fn", "func", "function", "def", "method", "proc", "procedure", "action", "handler", "command", "service"], "データを読み書きする処理は deed、式で書く値の変換は convert です"),
            (&["import", "include", "require", "using", "from", "mod", "package"], "ほかのファイルの取り込みは use \"ファイル.geas\" です"),
            (&["const", "let", "var", "val", "static", "global"], "定義の外に定数や変数は置けません。範囲は ltype、サーバーが決める値は slot と rule で書きます"),
            (&["route", "routes", "endpoint", "api", "controller", "router", "get", "post", "put", "patch", "delete"], "URL は resource で書きます（index・show・create・update・destroy）"),
            (&["view", "page", "component", "form", "layout"], "画面は screen で書きます"),
            (&["test", "tests", "example", "examples", "spec", "describe"], "テストは書きません。計画書の具体例の表から生成します"),
            (&["on", "listener", "subscribe", "cron", "schedule", "job", "timer"], "出来事は event、返事の待ちと期限は trigger で書きます"),
            (&["constraint", "assert", "check", "validate"], "いつでも成り立つ約束は invariant、入力の確かめは ltype の範囲か convert の require で書きます"),
        ])
    })
}

pub fn def_hint(kw: &str) -> String {
    let h = def_hints();
    if let Some(v) = h.get(kw) {
        return v.to_string();
    }
    h.get(kw.to_lowercase().as_str()).map(|s| s.to_string()).unwrap_or_default()
}

fn stmt_hints() -> &'static IndexMap<&'static str, &'static str> {
    use std::sync::OnceLock;
    static T: OnceLock<IndexMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| {
        table(&[
            (&["var", "const", "val", "mut", "auto"], "変数は let で作ります（let x = 式）"),
            (&["while", "loop", "do", "repeat", "until"], "繰り返しは for 要素 in 集まり { … } だけです（while はありません）"),
            (&["match", "switch", "case", "when"], "分岐は if 条件 { … } else if 条件 { … } else { … } で書きます（match・switch はありません）"),
            (&["throw", "raise", "panic", "error", "fail", "abort", "try", "catch"], "例外はありません。失敗も結果の種類にして return します（return OutOfStock など）"),
            (&["print", "println", "log", "console", "debug", "echo"], "画面や記録への出力は書けません（処理は結果を返すだけです）"),
            (&["else"], ELSE),
            (&["elif", "elsif", "elseif"], ELIF),
            (&["assert", "require", "ensure", "guard"], "処理の中の確かめは if 条件 { return 結果の種類 } と書きます（require は raw から変換する convert の中だけ）"),
            (&["await", "async", "emit", "publish", "send", "call", "fetch"], "外への要求は request 名前空間.操作(…, idempotency_key = …) で書き、返事は trigger で待ちます"),
            (&["set"], "項目の更新は 名前.項目 = 値 と書きます"),
            (&["insert", "new", "create", "add"], "新規作成は let x = create Entity { 項目: 値 } と書きます"),
            (&["remove", "del", "drop", "destroy"], "削除は delete 変数（let で引いた1件）です"),
            (&["break", "continue"], "for の途中で抜ける書き方はありません。条件で絞った集まりを回します"),
        ])
    })
}

/// 文の始まりが分からないときの手がかり。open_def は、閉じていないかもしれない定義
pub fn stmt_hint(kw: &str, open_def: &str) -> String {
    if top_keywords().contains(&kw) {
        let od = if open_def.is_empty() { "直前の定義" } else { open_def };
        return format!("「{}」は定義の始まりです。{}の }} が足りないかもしれません", kw, od);
    }
    stmt_hints().get(kw).map(|s| s.to_string()).unwrap_or_default()
}

/// 字句の場所（use で取り込んだファイルの字句なら、ファイルの名前も）
pub fn place(t: &Tok) -> String {
    (if t.file.is_empty() { String::new() } else { format!("{} の ", file_name(&t.file)) }) + &format!("{}行目", t.line)
}

/// { と } の対応が崩れていれば、その場所。合っていれば空（ファイルごとに見る）
pub fn brace_hint(toks: &[Tok]) -> String {
    let mut stacks: IndexMap<String, Vec<&Tok>> = IndexMap::new();
    for t in toks {
        if t.kind != "OP" || !(t.val == "{" || t.val == "}") {
            continue;
        }
        let st = stacks.entry(t.file.clone()).or_default();
        if t.val == "{" {
            st.push(t);
        } else if st.pop().is_none() {
            return format!("{}の }} に対応する {{ がありません（}} が多いか、{{ を書き忘れています）", place(t));
        }
    }
    for st in stacks.values() {
        if let Some(t) = st.last() {
            return format!("{}の {{ が閉じていません（}} が足りません）", place(t));
        }
    }
    String::new()
}
