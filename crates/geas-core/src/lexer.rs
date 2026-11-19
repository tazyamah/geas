// SPDX-License-Identifier: GPL-3.0-or-later
//! 字句解析。名前は、空白と記号以外の連なり（日本語の名前をそのまま使える）。Python 版 geas/lexer.py の移し。
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct LexError {
    pub msg: String,
    pub line: i64,
    pub col: i64,
    pub file: String,
}

impl LexError {
    pub fn new(msg: impl Into<String>, line: i64, col: i64) -> Self {
        LexError { msg: msg.into(), line, col, file: String::new() }
    }
}

pub fn file_name(p: &str) -> String {
    Path::new(p).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let head = (if self.file.is_empty() { String::new() } else { format!("{} の ", file_name(&self.file)) })
            + &(if self.line != 0 { format!("{}行目", self.line) } else { String::new() });
        if head.is_empty() {
            write!(f, "{}", self.msg)
        } else {
            write!(f, "{}: {}", head, self.msg)
        }
    }
}

/// kind は NAME NUM STR OP NL EOF
#[derive(Debug, Clone, PartialEq)]
pub struct Tok {
    pub kind: &'static str,
    pub val: String,
    pub line: i64,
    pub doc: String,
    pub col: i64,
    pub file: String,
}

impl Tok {
    pub fn new(kind: &'static str, val: impl Into<String>, line: i64) -> Tok {
        Tok { kind, val: val.into(), line, doc: String::new(), col: 0, file: String::new() }
    }
    pub fn at(kind: &'static str, val: impl Into<String>, line: i64, col: i64, file: &str) -> Tok {
        Tok { kind, val: val.into(), line, doc: String::new(), col, file: file.to_string() }
    }
}

const PUNCT: &str = "{}()[],:;.=<>+-*/%|→Σ!";
const TWO: [&str; 8] = ["..", "->", "==", "!=", "<=", ">=", "-=", "+="];
const SPACE: &str = " \t\r　";
const ESC_HOW: &str = "\\ のあとに書けるのは n（改行）・t（タブ）・\"（\" そのもの）・\\（\\ そのもの）と、行の終わり（次の行へ続ける）だけです。正規表現などで \\ そのものを書くときは \\\\ と書きます";
const STR_HOW: &str = "文字列を次の行へ続けるときは行の終わりに \\ を書き（次の行の頭の空白は読み飛ばします）、改行そのものは \\n と書きます";

fn is_punct(c: char) -> bool {
    PUNCT.contains(c)
}
fn is_space(c: char) -> bool {
    SPACE.contains(c)
}

/// Python の str.isdigit() に近いもの（半角の 0〜9 のほか、全角・上付き・丸数字など）
pub fn py_isdigit(c: char) -> bool {
    if c.is_ascii_digit() {
        return true;
    }
    if !c.is_numeric() {
        return false;
    }
    // 分数・ローマ数字などは isdigit ではない（Numeric_Type が Digit／Decimal でないもの）
    let u = c as u32;
    !matches!(u, 0xBC..=0xBE | 0x2150..=0x2189 | 0x3007 | 0x3021..=0x3029 | 0x3038..=0x303A | 0x16EE..=0x16F0 | 0xA6E6..=0xA6EF | 0x10107..=0x10133)
}

pub fn lex(src: &str) -> Result<Vec<Tok>, LexError> {
    let s: Vec<char> = src.chars().collect();
    let n = s.len();
    let mut toks: Vec<Tok> = Vec::new();
    let (mut i, mut line) = (0usize, 1i64);
    let mut start = 0usize;
    let mut docs: Vec<String> = Vec::new();
    let mut join_next = false;
    let mut depth: i64 = 0;
    let starts = |i: usize, p: &str| -> bool { p.chars().enumerate().all(|(k, c)| s.get(i + k) == Some(&c)) };

    macro_rules! add {
        ($tok:expr) => {{
            let mut tok: Tok = $tok;
            if !docs.is_empty() && tok.kind != "NL" && tok.kind != "EOF" {
                tok.doc = docs.join("\n");
                docs.clear();
            }
            toks.push(tok);
        }};
    }
    macro_rules! nl {
        () => {{
            if depth == 0 && toks.last().map_or(false, |t| t.kind != "NL") {
                toks.push(Tok::at("NL", "\n", line, i as i64 - start as i64 + 1, ""));
            }
        }};
    }

    while i < n {
        let c = s[i];
        if c == '\n' {
            if join_next {
                join_next = false;
            } else {
                nl!();
            }
            line += 1;
            i += 1;
            start = i;
        } else if is_space(c) {
            i += 1;
        } else if starts(i, "//") || c == '#' {
            let mut j = i;
            while j < n && s[j] != '\n' {
                j += 1;
            }
            if starts(i, "///") {
                let text: String = s[i + 3..j].iter().collect();
                docs.push(text.trim().to_string());
                if toks.last().map_or(false, |t| t.kind != "NL") {
                    join_next = true;
                }
            }
            i = j;
        } else if c == '"' {
            let (sline, scol) = (line, i as i64 - start as i64 + 1);
            let mut buf = String::new();
            let mut j = i + 1;
            loop {
                if j >= n {
                    return Err(LexError::new("文字列が閉じていません（\" で始めた文字列の終わりの \" がありません）", sline, scol));
                }
                let ch = s[j];
                if ch == '"' {
                    break;
                }
                if ch == '\n' {
                    return Err(LexError::new(format!("文字列の途中で行が変わっています（終わりの \" を書き忘れたかもしれません）。{}", STR_HOW), sline, scol));
                }
                if ch != '\\' {
                    buf.push(ch);
                    j += 1;
                    continue;
                }
                let mut k = j + 1;
                while k < n && matches!(s[k], ' ' | '\t' | '\r') {
                    k += 1;
                }
                let nextc = s.get(j + 1).copied();
                let esc = match nextc {
                    Some('n') => Some('\n'),
                    Some('t') => Some('\t'),
                    Some('"') => Some('"'),
                    Some('\\') => Some('\\'),
                    _ => None,
                };
                if let Some(e) = esc {
                    buf.push(e);
                    j += 2;
                } else if k < n && s[k] == '\n' {
                    while k < n && matches!(s[k], ' ' | '\t' | '\r' | '\n') {
                        if s[k] == '\n' {
                            line += 1;
                            start = k + 1;
                        }
                        k += 1;
                    }
                    j = k;
                } else if j + 1 >= n {
                    return Err(LexError::new("文字列が閉じていません（\" で始めた文字列の終わりの \" がありません）", sline, scol));
                } else {
                    return Err(LexError::new(format!("文字列の中に「\\{}」は書けません。{}", s[j + 1], ESC_HOW), line, j as i64 - start as i64 + 1));
                }
            }
            add!(Tok::at("STR", buf, sline, scol, ""));
            i = j + 1;
        } else if i + 1 < n && TWO.contains(&s[i..i + 2].iter().collect::<String>().as_str()) {
            let v: String = s[i..i + 2].iter().collect();
            add!(Tok::at("OP", if v == "->" { "→".to_string() } else { v }, line, i as i64 - start as i64 + 1, ""));
            i += 2;
        } else if is_punct(c) {
            depth = (depth + (c == '(') as i64 - (c == ')') as i64).max(0);
            add!(Tok::at("OP", c.to_string(), line, i as i64 - start as i64 + 1, ""));
            i += 1;
        } else {
            let mut j = i;
            while j < n && !is_space(s[j]) && s[j] != '\n' && !is_punct(s[j]) && s[j] != '"' {
                j += 1;
            }
            let run: String = s[i..j].iter().collect();
            let run_digit = !run.is_empty() && run.chars().all(py_isdigit);
            if run_digit && !run.is_ascii() {
                return Err(LexError::new(format!("数は半角の 0〜9 で書きます（「{}」は半角の数字ではありません）", run), line, i as i64 - start as i64 + 1));
            }
            if run_digit && s.get(j) == Some(&'.') && s.get(j + 1).map_or(false, |c| py_isdigit(*c)) {
                let mut k = j + 1;
                while k < n && py_isdigit(s[k]) {
                    k += 1;
                }
                let lit: String = s[i..k].iter().collect();
                return Err(LexError::new(
                    format!("小数は書けません（「{}」）。割合は rule の値に 分子/分母（10/100 など）で書き、金額は最小単位の整数（ドルならセントの数）で書きます", lit),
                    line,
                    i as i64 - start as i64 + 1,
                ));
            }
            add!(Tok::at(if run_digit { "NUM" } else { "NAME" }, run, line, i as i64 - start as i64 + 1, ""));
            i = j;
        }
    }
    nl!();
    toks.push(Tok::at("EOF", "", line, i as i64 - start as i64 + 1, ""));
    Ok(toks)
}
