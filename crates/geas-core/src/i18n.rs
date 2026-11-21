// SPDX-License-Identifier: GPL-3.0-or-later
//! 日本語・英語モード。
//!
//! 道具が出す文言（検査器の指摘・CLI の表示・生成する仕様書やレビュー画面・生成物の固定文言）は、
//! 日本語で書いてあり、英語モードでは出力の直前に、対訳表（`assets/i18n/en.tsv`）で英語にする。
//! 表に無い文は、日本語のまま出る（落ちはしない）。
//!
//! - 表の 1 行は `日本語<TAB>English`。改行は `\n`、タブは `\t`、`\` は `\\`。
//! - `{}` は引数の場所。英語側は `{}`（順に）か `{1}`、`{2}`（番号）で引数を置く。引数の中の日本語も訳す。
//! - 引数の無い短い語（日本語の字が 3 字未満）は、前後が区切りのときだけ置き換える。
//! - 言語は `set_lang`。既定は日本語（ライブラリとして使うときの出力を変えない）。CLI が `--lang`・`GEAS_LANG` で決める（CLI の既定は英語）。
use regex::{Captures, Regex};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Ja,
    En,
}

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(l: Lang) {
    LANG.store(if l == Lang::En { 1 } else { 0 }, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    if LANG.load(Ordering::Relaxed) == 1 { Lang::En } else { Lang::Ja }
}

pub fn parse_lang(s: &str) -> Option<Lang> {
    match s.trim().to_ascii_lowercase().split(['_', '-', '.']).next().unwrap_or("") {
        "ja" | "jp" | "japanese" => Some(Lang::Ja),
        "en" | "english" => Some(Lang::En),
        _ => None,
    }
}

const TABLE: &str = include_str!("../assets/i18n/en.tsv");

pub fn has_ja(s: &str) -> bool {
    s.chars().any(|c| matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{9fff}' | '\u{ff00}'..='\u{ffef}' | '\u{3000}'..='\u{303f}'))
}

fn has_kana_kanji(s: &str) -> usize {
    s.chars().filter(|c| matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{9fff}')).count()
}

fn unesc(s: &str) -> String {
    let mut o = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => o.push('\n'),
                Some('t') => o.push('\t'),
                Some('\\') => o.push('\\'),
                Some(x) => {
                    o.push('\\');
                    o.push(x)
                }
                None => o.push('\\'),
            }
        } else {
            o.push(c)
        }
    }
    o
}

struct Entry {
    anchor: String,
    re: Regex,
    en: Vec<Piece>,
    /// 生成したコードの中の文字列に入れるときの英語（引用符を増やさない）
    en_code: Vec<Piece>,
    nargs: usize,
    /// 前後の区切り（正規表現の最初と最後のグループ）を持つか
    lead: bool,
    /// 行や表のセルの全体に当てる文（長い行には当てない）
    whole: bool,
}

enum Piece {
    Lit(String),
    Arg(usize),
}

fn parse_en(en: &str) -> Vec<Piece> {
    let mut out = vec![];
    let mut lit = String::new();
    let mut seq = 0;
    let cs: Vec<char> = en.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '{' {
            if let Some(j) = cs[i..].iter().position(|&c| c == '}') {
                let inner: String = cs[i + 1..i + j].iter().collect();
                if inner.is_empty() {
                    if !lit.is_empty() {
                        out.push(Piece::Lit(std::mem::take(&mut lit)));
                    }
                    out.push(Piece::Arg(seq));
                    seq += 1;
                    i += j + 1;
                    continue;
                }
                if let Ok(n) = inner.parse::<usize>() {
                    if n >= 1 {
                        if !lit.is_empty() {
                            out.push(Piece::Lit(std::mem::take(&mut lit)));
                        }
                        out.push(Piece::Arg(n - 1));
                        i += j + 1;
                        continue;
                    }
                }
            }
        }
        lit.push(cs[i]);
        i += 1;
    }
    if !lit.is_empty() {
        out.push(Piece::Lit(lit));
    }
    out
}

/// 生成したコードの文字列（"…" や '…'）に入る英語は、日本語より引用符を増やすと壊れる。増えるときは、字の形の違う引用符にする
fn code_safe(ja: &str, en: &str) -> String {
    let mut o = en.to_string();
    for (q, alt) in [('"', '\u{201d}'), ('\'', '\u{2019}')] {
        if en.matches(q).count() > ja.matches(q).count() {
            o = o.replace(q, &alt.to_string());
        }
    }
    o
}

pub struct Table {
    exact: HashMap<String, Vec<Piece>>,
    exact_code: HashMap<String, Vec<Piece>>,
    entries: Vec<Entry>,
}

/// 表を読む。(日本語, 英語) の組を返す（検査用にも使う）
pub fn raw_pairs() -> Vec<(String, String)> {
    raw_rows().into_iter().map(|(a, b, _)| (a, b)).collect()
}

/// 3 列目の `u` は、短い文でも前後を問わず置き換える印（生成物の固定文言）
fn raw_rows() -> Vec<(String, String, bool)> {
    let mut v = vec![];
    for l in TABLE.lines().skip(1) {
        if l.is_empty() {
            continue;
        }
        if let Some((a, rest)) = l.split_once('\t') {
            let (b, force) = match rest.split_once('\t') {
                Some((b, f)) => (b, f == "u"),
                None => (rest, false),
            };
            v.push((unesc(a), unesc(b), force));
        }
    }
    v
}

/// 行どうしの対応がとれるときは、行ごとの組に分ける
fn split_lines(ja: &str, en: &str) -> Vec<(String, String)> {
    let a: Vec<&str> = ja.split('\n').collect();
    let b: Vec<&str> = en.split('\n').collect();
    let jl: Vec<&str> = a.iter().copied().filter(|x| has_ja(x)).collect();
    if a.len() > 1 && a.len() == b.len() {
        a.iter().zip(b.iter()).filter(|(x, _)| has_ja(x)).map(|(x, y)| (x.to_string(), y.to_string())).collect()
    } else if a.len() > 1 && jl.len() == b.len() {
        // 英語側が、日本語を含む行だけを順に並べたもの
        jl.iter().zip(b.iter()).map(|(x, y)| (x.to_string(), y.to_string())).collect()
    } else {
        vec![(ja.to_string(), en.to_string())]
    }
}

fn build() -> Table {
    let mut exact = HashMap::new();
    let mut exact_code = HashMap::new();
    let mut entries = vec![];
    for (ja, en, force) in raw_rows() {
        for (ja, en) in split_lines(&ja, &en) {
            let ja = ja.replace("{{", "\u{1}").replace("}}", "\u{2}").replace('\u{1}', "{").replace('\u{2}', "}");
            let en = en.replace("{{", "{").replace("}}", "}");
            // 引数の場所で分ける
            let parts: Vec<&str> = ja.split("{}").collect();
            if parts.len() == 1 {
                exact.insert(ja.clone(), parse_en(&en));
                exact_code.insert(ja.clone(), parse_en(&code_safe(&ja, &en)));
            }
            let anchor = parts.iter().max_by_key(|p| p.chars().count()).map(|s| s.to_string()).unwrap_or_default();
            if anchor.is_empty() || !has_ja(&ja) {
                continue;
            }
            // 日本語の字が少ない文は、利用者の書いた文を壊しやすいので、行か表のセルの全体にだけ当てる
            let nja: usize = parts.iter().map(|p| has_kana_kanji(p)).sum();
            if nja < 2 && parts.len() > 1 && !force {
                continue;
            }
            let short = nja < 5 && !force;
            let body = |any: &str| -> String {
                let mut pat = String::new();
                for (i, p) in parts.iter().enumerate() {
                    pat.push_str(&regex::escape(p));
                    if i + 1 < parts.len() {
                        // 末尾の引数は欲張り、途中は控えめ
                        // 途中の引数は文（。）をまたがない
                        let mid = if any == "." { "[^。]" } else { any };
                        // 文の先頭の引数は名前（空白なし）。前の文や行のコードを飲み込まない
                        let mid = if i == 0 && p.is_empty() && any == "." { r"[^\s;{}()（）<>]" } else { mid };
                        pat.push_str(&if i + 2 == parts.len() && parts[i + 1].is_empty() { format!("({}*)", any) } else { format!("({}*?)", mid) });
                    }
                }
                pat
            };
            let nargs = parts.len() - 1;
            let mut push = |pat: String, lead: bool, whole: bool| {
                if let Ok(re) = Regex::new(&pat) {
                    entries.push(Entry { anchor: anchor.clone(), re, en: parse_en(&en), en_code: parse_en(&code_safe(&ja, &en)), nargs, lead, whole });
                }
            };
            if !short {
                push(body("."), false, false);
            } else if parts.len() == 1 {
                push(format!(r"(^|[\s|>「（(：:,、。/→]){}($|[\s|<」）):：,、。/→])", body(".")), true, false);
            } else {
                push(format!(r"(^\s*(?:[-*] |\d+\. )?){}(\s*$)", body(".")), true, true);
                push(format!(r"(\| ){}( \|)", body("[^|]")), true, true);
                push(format!(r"(>){}(<)", body("[^<]")), true, false);
            }
        }
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.anchor.chars().count()));
    Table { exact, exact_code, entries }
}

pub fn table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(build)
}

fn render(en: &[Piece], args: &[String]) -> String {
    let mut o = String::new();
    for p in en {
        match p {
            Piece::Lit(s) => o.push_str(s),
            Piece::Arg(i) => {
                if let Some(a) = args.get(*i) {
                    o.push_str(a)
                }
            }
        }
    }
    o
}

fn tr_line(line: &str, depth: usize, code: bool) -> String {
    if !has_ja(line) || depth > 4 {
        return line.to_string();
    }
    let t = table();
    if let Some(en) = t.exact.get(line) {
        return render(if code { &t.exact_code[line] } else { en }, &[]);
    }
    let mut cur = line.to_string();
    for e in &t.entries {
        if !has_ja(&cur) {
            break;
        }
        if !cur.contains(e.anchor.as_str()) || (e.whole && cur.len() > 400) {
            continue;
        }
        cur = e
            .re
            .replace_all(&cur, |c: &Captures| {
                let off = if e.lead { 2 } else { 1 };
                let args: Vec<String> = (off..off + e.nargs).map(|i| tr_line(c.get(i).map_or("", |m| m.as_str()), depth + 1, code)).collect();
                if e.lead {
                    let pre = c.get(1).map_or("", |m| m.as_str());
                    let post = c.get(e.nargs + 2).map_or("", |m| m.as_str());
                    format!("{}{}{}", pre, render(if code { &e.en_code } else { &e.en }, &args), post)
                } else {
                    render(if code { &e.en_code } else { &e.en }, &args)
                }
            })
            .into_owned();
    }
    cur
}

/// 英語モードなら、文（複数行でもよい）を英語にする。日本語モードと、日本語の無い文は、そのまま
pub fn tr(s: &str) -> String {
    if lang() == Lang::Ja || !has_ja(s) {
        return s.to_string();
    }
    s.split('\n').map(|l| half_width(tr_line(l, 0, false))).collect::<Vec<_>>().join("\n")
}

/// 英語になりきった行の、全角の約物を半角にする（日本語が残る行はそのまま）
fn half_width(l: String) -> String {
    if has_kana_kanji(&l) > 0 || !l.chars().any(|c| matches!(c, '。' | '、' | '（' | '）' | '：' | '，')) {
        return l;
    }
    l.replace('。', ". ").replace('、', ", ").replace('（', " (").replace('）', ")").replace('：', ": ").replace('，', ", ").replace(" . ", ". ").replace(" ,", ",").replace("  ", " ").trim_end().to_string()
}

/// 生成したコード・画面の部品の中の文：英語モードなら英語にする（文字列を壊さないよう、引用符を増やさない）
pub fn tr_code(s: &str) -> String {
    if lang() == Lang::Ja || !has_ja(s) {
        return s.to_string();
    }
    s.split('\n').map(|l| tr_line(l, 0, true)).collect::<Vec<_>>().join("\n")
}

/// 表に無くて日本語のまま残る行（網羅の確認用）
pub fn untranslated(s: &str) -> Vec<String> {
    tr_force(s).split('\n').filter(|l| has_ja(l)).map(|l| l.to_string()).collect()
}

fn tr_force(s: &str) -> String {
    s.split('\n').map(|l| tr_line(l, 0, false)).collect::<Vec<_>>().join("\n")
}

/// 言語に関係なく英語にする（検査・テスト用）
pub fn to_en(s: &str) -> String {
    tr_force(s)
}

/// JSON の文字列の値（キーは除く）を、英語モードなら英語にする
pub fn tr_value(v: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    if lang() == Lang::Ja {
        return v;
    }
    match v {
        Value::String(s) => Value::String(tr(&s)),
        Value::Array(a) => Value::Array(a.into_iter().map(tr_value).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, x)| (k, tr_value(x))).collect()),
        x => x,
    }
}

/// 生成物のバイト列：UTF-8 の文章なら英語モードで英語にする（それ以外はそのまま）
pub fn tr_bytes(b: &[u8]) -> Vec<u8> {
    if lang() == Lang::Ja {
        return b.to_vec();
    }
    match std::str::from_utf8(b) {
        Ok(s) if has_ja(s) || s.contains("lang=\"ja\"") || s.contains("\"lang\": \"ja\"") => tr_code(s).replace("lang=\"ja\"", "lang=\"en\"").replace("\"lang\": \"ja\"", "\"lang\": \"en\"").into_bytes(),
        _ => b.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading() {
        let s = "## 画面（入口は最初の画面と entry の画面。入力欄の「←」は選ぶ元の問い合わせ）";
        assert!(!has_ja(&to_en(s)));
    }

    #[test]
    fn 引数の数が合う() {
        // 英語側が使う {N} は、日本語側の {} の数まで。{} の数が日本語より多いのは誤り
        let mut seen = std::collections::HashSet::new();
        for (ja, en) in raw_pairs() {
            assert!(seen.insert(ja.clone()), "対訳表に同じ日本語が 2 つある: {}", ja);
            let nja = ja.replace("{{", "").replace("}}", "").matches("{}").count();
            let en2 = en.replace("{{", "").replace("}}", "");
            let mut seq = 0;
            let mut it = en2.chars().peekable();
            while let Some(c) = it.next() {
                if c == '{' {
                    let mut inner = String::new();
                    for d in it.by_ref() {
                        if d == '}' {
                            break;
                        }
                        inner.push(d);
                    }
                    if inner.is_empty() {
                        seq += 1;
                        assert!(seq <= nja || nja == 0 && ja.contains('{'), "引数が多すぎる: {} ⇒ {}", ja, en);
                    } else if let Ok(n) = inner.parse::<usize>() {
                        assert!(n >= 1 && n <= nja, "引数の番号が範囲外: {} ⇒ {}", ja, en);
                    }
                }
            }
        }
    }

    #[test]
    fn 定型の文を訳す() {
        assert_eq!(to_en("実体「Order」がありません"), "There is no entity \"Order\"");
        assert_eq!(to_en("| 名前 | 表示名 |"), "| Name | Display name |");
        // 利用者の書いた名前は、そのまま
        assert_eq!(to_en("商品の管理の画面"), "商品の管理の画面");
    }

    #[test]
    fn 既定は日本語() {
        assert_eq!(lang(), Lang::Ja);
        assert_eq!(tr("構文エラー"), "構文エラー");
    }

    #[test]
    fn 言語名() {
        assert_eq!(parse_lang("ja_JP.UTF-8"), Some(Lang::Ja));
        assert_eq!(parse_lang("en-US"), Some(Lang::En));
        assert_eq!(parse_lang("fr"), None);
    }
}
