// SPDX-License-Identifier: GPL-3.0-or-later
//! 「漏れない」の常設の検査：コードがすることは、全部、人が読む仕様書に出る。
//!
//! 見本の `.geas` を 1 か所だけ変えた版をたくさん作る（数を 1 増やす・減らす、比較・加減の演算子を替える、語を替える、
//! 文字列の頭に X を足す、コンマをはさんだ名前を入れ替える、1 行を消す・もう 1 つ足す・次の行と入れ替える）。検査を通って、
//! 生成される Rust 一式が変わったなら、人が読む仕様書（review の HTML）と、更新のたびに見る「前の版からの変更」の両方が
//! 変わらなければならない。変わらないものがあれば、仕様書に出ない動きがある（コードを読まないレビューで見つけられない）ので落とす。
use geas_core::check::{defs::check, ERROR};
use geas_core::lexer::lex;
use geas_core::parser::parse;
use geas_core::review::{changes_section, review};
use geas_core::spec::{build, render};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_geas");

fn swap_op(s: &str) -> Option<&'static str> {
    Some(match s {
        "<" => "<=",
        "<=" => "<",
        ">" => ">=",
        ">=" => ">",
        "==" => "!=",
        "!=" => "==",
        "+" => "-",
        "-" => "+",
        "+=" => "-=",
        "-=" => "+=",
        "*" => "+",
        _ => return None,
    })
}

fn swap_name(s: &str) -> Option<&'static str> {
    Some(match s {
        "and" => "or",
        "or" => "and",
        "asc" => "desc",
        "desc" => "asc",
        "min" => "max",
        "max" => "min",
        "true" => "false",
        "false" => "true",
        _ => return None,
    })
}

fn starts_at(ln: &[char], at: usize, s: &str) -> bool {
    let w: Vec<char> = s.chars().collect();
    ln.len() >= at + w.len() && ln[at..at + w.len()] == w[..]
}

fn s(c: &[char]) -> String {
    c.iter().collect()
}

/// (行, 元の行, 変えた行, 変えた全文) の列
fn mutants(text: &str) -> Vec<(i64, String, String, String)> {
    let lines: Vec<Vec<char>> = text.split('\n').map(|l| l.chars().collect()).collect();
    let join = |ls: &[Vec<char>]| ls.iter().map(|l| s(l)).collect::<Vec<_>>().join("\n");
    let mut out = vec![];
    let toks = lex(text).unwrap();
    for t in &toks {
        if t.kind == "NL" || t.kind == "EOF" || !t.file.is_empty() {
            continue;
        }
        let ln = &lines[t.line as usize - 1];
        let at = t.col as usize - 1;
        let (mut new, mut old): (Option<String>, String) = (None, t.val.clone());
        match t.kind {
            "NUM" => {
                let digits: String = ln[at..].iter().take_while(|c| c.is_ascii_digit()).collect();
                if digits == t.val {
                    if let Ok(n) = t.val.parse::<u128>() {
                        new = Some((n + 1).to_string());
                    }
                }
            }
            "OP" => {
                if let Some(n) = swap_op(&t.val) {
                    if starts_at(ln, at, &t.val) {
                        new = Some(n.into());
                    }
                }
            }
            "NAME" => {
                if let Some(n) = swap_name(&t.val) {
                    if starts_at(ln, at, &t.val) {
                        new = Some(n.into());
                    }
                }
            }
            "STR" => {
                if starts_at(ln, at, "\"") {
                    new = Some("\"X".into());
                    old = "\"".into();
                }
            }
            _ => {}
        }
        let Some(first) = new else { continue };
        let mut news = vec![first];
        if t.kind == "NUM" {
            if let Ok(n) = t.val.parse::<u128>() {
                if n > 0 {
                    news.push((n - 1).to_string());
                }
            }
        }
        for nw in news {
            let mut o = lines.clone();
            let nl: Vec<char> = ln[..at].iter().cloned().chain(nw.chars()).chain(ln[at + old.chars().count()..].iter().cloned()).collect();
            o[t.line as usize - 1] = nl.clone();
            out.push((t.line, s(ln).trim().to_string(), s(&nl).trim().to_string(), join(&o)));
        }
    }
    let toks: Vec<_> = toks.iter().filter(|t| t.file.is_empty()).collect();
    for w in toks.windows(3) {
        let (a, c, b) = (w[0], w[1], w[2]);
        if a.kind == "NAME" && b.kind == "NAME" && c.val == "," && a.line == b.line && a.val != b.val {
            let ln = &lines[a.line as usize - 1];
            let (ac, bc) = (a.col as usize - 1, b.col as usize - 1);
            if starts_at(ln, ac, &a.val) && starts_at(ln, bc, &b.val) {
                let (al, bl) = (a.val.chars().count(), b.val.chars().count());
                let nl: Vec<char> = ln[..ac].iter().cloned().chain(b.val.chars()).chain(ln[ac + al..bc].iter().cloned()).chain(a.val.chars()).chain(ln[bc + bl..].iter().cloned()).collect();
                let mut o = lines.clone();
                o[a.line as usize - 1] = nl.clone();
                out.push((a.line, s(ln).trim().to_string(), s(&nl).trim().to_string(), join(&o)));
            }
        }
    }
    let code_of = |l: &Vec<char>| s(l).split("//").next().unwrap_or("").to_string();
    for (i, ln) in lines.iter().enumerate() {
        let code = code_of(ln);
        if !code.trim().is_empty() && !code.contains('{') && !code.contains('}') {
            let mut o = lines.clone();
            o[i] = vec![];
            out.push((i as i64 + 1, s(ln).trim().to_string(), "（行を消す）".into(), join(&o)));
            let mut o = lines[..=i].to_vec();
            o.extend(lines[i..].iter().cloned());
            out.push((i as i64 + 1, s(ln).trim().to_string(), "（同じ行をもう 1 つ足す）".into(), join(&o)));
            let nxt = if i + 1 < lines.len() { code_of(&lines[i + 1]) } else { String::new() };
            if !nxt.trim().is_empty() && !nxt.contains('{') && !nxt.contains('}') && nxt.trim() != code.trim() {
                let mut o = lines[..i].to_vec();
                o.push(lines[i + 1].clone());
                o.push(lines[i].clone());
                o.extend(lines[i + 2..].iter().cloned());
                out.push((i as i64 + 1, s(ln).trim().to_string(), "（次の行と入れ替える）".into(), join(&o)));
            }
        }
    }
    out
}

/// Rust 一式（lib・host・画面・拡張・サーバー・窓）を生成して、ファイルごとのハッシュを返す。生成できなければ None
fn crate_of(path: &Path, dest: &Path) -> Option<BTreeMap<String, String>> {
    let _ = std::fs::remove_dir_all(dest);
    let r = Command::new(BIN).arg("rust").arg(path).arg("-o").arg(dest).args(["--host", "--runtime", "--standalone"]).output().unwrap();
    if !r.status.success() {
        return None;
    }
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        for e in std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                out.insert(p.strip_prefix(base).unwrap().to_string_lossy().into_owned(), Sha256::digest(std::fs::read(&p).unwrap()).iter().map(|b| format!("{:02x}", b)).collect());
            }
        }
    }
    let mut m = BTreeMap::new();
    walk(dest, dest, &mut m);
    Some(m)
}

struct Views {
    ok: bool,
    spec: geas_core::spec::Spec,
    html: String,
}

fn views(text: &str, path: &Path, arrow: &Regex) -> Option<Views> {
    let mut prog = parse(text, Some(path)).ok()?;
    let ck = check(&mut prog, true);
    let spec = build(&ck);
    render(&ck, &spec, "");
    let html = arrow.replace_all(&review(&ck, None, &[], "", false).ok()?, "arrowN").into_owned();
    Some(Views { ok: !ck.diags.iter().any(|d| d.level == ERROR) && ck.ready(), spec, html })
}

fn run(name: &str) {
    let arrow = Regex::new(r"arrow\d+").unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let tmp: PathBuf = std::env::temp_dir().join(format!("geas-cover-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let work = tmp.join(format!("{}.geas", name));
    let base = std::fs::read_to_string(root.join("tests").join("fixtures").join(name).join(format!("{}.geas", name))).unwrap();
    std::fs::write(&work, &base).unwrap();
    let v0 = views(&base, &work, &arrow).unwrap();
    let out = tmp.join("out");
    let crate0 = crate_of(&work, &out).expect("見本から生成できません");
    let same = arrow.replace_all(&changes_section(&v0.spec, &v0.spec, "前の版"), "arrowN").into_owned();
    let (mut hidden, mut seen) = (vec![], 0);
    for (line, before, after, text) in mutants(&base) {
        let Some(v) = views(&text, &work, &arrow) else { continue };
        if !v.ok {
            continue;
        }
        std::fs::write(&work, &text).unwrap();
        match crate_of(&work, &out) {
            None => continue,
            Some(c) if c == crate0 => continue,
            _ => {}
        }
        seen += 1;
        let mut miss = vec![];
        if v.html == v0.html {
            miss.push("仕様書");
        }
        if arrow.replace_all(&changes_section(&v.spec, &v0.spec, "前の版"), "arrowN").into_owned() == same {
            miss.push("変更点");
        }
        if !miss.is_empty() {
            hidden.push(format!("{} 行目: {}  →  {}  〔{}に出ない〕", line, before, after, miss.join("・")));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(seen > 0);
    assert!(hidden.is_empty(), "生成物が変わるのに、仕様書か変更点に出ない変更があります:\n{}", hidden.join("\n"));
}

#[test]
fn delete_demo() {
    run("delete_demo");
}
#[test]
fn file_demo() {
    run("file_demo");
}
#[test]
fn overseas_shop() {
    run("overseas_shop");
}
#[test]
fn runtime_demo() {
    run("runtime_demo");
}
#[test]
fn shop() {
    run("shop");
}
#[test]
fn solo_tweet() {
    run("solo_tweet");
}
