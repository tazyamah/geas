// SPDX-License-Identifier: GPL-3.0-or-later
//! 生成器が、.geas や計画書の文字列を生成コードに埋めるときに、コードを差し込ませないこと。
//! 人は .geas も生成コードも読まないので、表示名や計画書のセルに書いた文字列が、仕様書に出ないコードとして動いてはいけない。
//! 元は Python 版の tests/test_security.py（タグ python-final）。
mod common;
use common::*;
use geas_core::crategen::write_crate;
use geas_core::examples::runner_source;
use geas_core::hostgen::host_source;
use geas_core::mutate::read_git;
use geas_core::plan::read_plan;
use geas_core::rustgen::{doc_line, generate, rust_str};
use regex::Regex;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

fn shop_dir() -> PathBuf {
    root().join("tests").join("fixtures").join("shop")
}

// 文字列の外へ出ようとする文字の並び：生の文字列の終わり（"####）、行コメント、改行、波かっこ（書式の文字列）、逆斜線
fn nasty() -> Vec<String> {
    ["\"####; pub fn injected() -> i32 { 6 * 7 } //", "\"); panic!(\"x\"); (\"", "a\\", "x\ny", "{}{{", "\\u{41}", "\t\r\0\x7f", "日本語 "].iter().map(|s| s.to_string()).collect()
}

/// s[start] から始まる Rust の文字列リテラル（"…"）を読み、(値, 閉じた " の次の位置（バイト）) を返す
fn literal_value(src: &str, start: usize) -> (String, usize) {
    let b: Vec<char> = src[start..].chars().collect();
    assert_eq!(b[0], '"');
    let (mut out, mut i) = (String::new(), 1usize);
    while b[i] != '"' {
        if b[i] == '\\' {
            if b[i + 1] == 'u' {
                let j = b[i..].iter().position(|&c| c == '}').unwrap() + i;
                let hex: String = b[i + 3..j].iter().collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
                i = j + 1;
                continue;
            }
            out.push(match b[i + 1] {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '\\' => '\\',
                '"' => '"',
                '0' => '\0',
                '\'' => '\'',
                c => panic!("知らない逸脱: {}", c),
            });
            i += 2;
            continue;
        }
        assert_ne!(b[i], '\r', "Rust の文字列リテラルに生の CR は書けない");
        out.push(b[i]);
        i += 1;
    }
    let consumed: usize = b[..=i].iter().map(|c| c.len_utf8()).sum();
    (out, start + consumed)
}

#[test]
fn rust_strはどんな文字列もリテラルの外へ出さずに元の値で読み戻せる() {
    let mut all = nasty();
    all.extend(["", "ふつうの文字列", "\"", "\\\\\""].iter().map(|s| s.to_string()));
    for s in &all {
        let lit = rust_str(s);
        let (value, end) = literal_value(&lit, 0);
        assert!(&value == s && end == lit.len(), "{:?} {}", s, lit);
        assert!(!lit.contains('\n') && !lit.contains('\r'));
    }
    for s in nasty() {
        let d = doc_line(&s);
        assert!(!d.contains('\n') && !d.contains('\r') && !d.contains('\0'), "{:?}", d);
    }
}

fn injected_overseas() -> String {
    // お店の例の、画面の表示名に、生の文字列を抜けてコードを書く文字列を入れたもの
    let src = fixture("overseas_shop/overseas_shop.geas");
    assert!(src.contains("/// 商品一覧の画面\n"));
    src.replacen("/// 商品一覧の画面\n", &format!("/// 商品一覧の画面{}\n", nasty()[0]), 1)
}

#[test]
fn 表示名に生の文字列の終わりを書いてもdefs_jsonは1つの文字列のまま() {
    with_check(&injected_overseas(), true, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        let host = host_source(ck).unwrap();
        let head = "pub const DEFS_JSON: &str = ";
        let at = host.find(head).unwrap() + head.len();
        let (value, end) = literal_value(&host, at);
        assert_eq!(&host[end..end + 2], ";\n"); // リテラルのあとは、文の終わりだけ（差し込まれたコードは無い）
        let defs: Value = serde_json::from_str(&value).unwrap();
        assert_eq!(defs["screens"]["screens"]["ProductList"]["label"], format!("商品一覧の画面{}", nasty()[0]));
        assert!(!format!("{}{}", &host[..at], &host[end..]).contains("pub fn injected")); // 差し込もうとした関数は、リテラルの中の文字としてだけある
    });
}

#[test]
fn 表示名に生の文字列の終わりを書いてもビルドしたアプリに差し込んだ関数は無い() {
    let t = Tmp::new("injected");
    let main = "fn main() { println!(\"{}\", geas_app::host::DEFS_JSON); }\n";
    with_check(&injected_overseas(), true, |ck| {
        write_crate(ck, &generate(ck), main, &t.0, true).unwrap();
    });
    let r = Command::new("cargo").args(["run", "--quiet", "--offline", "--bin", "plan_examples"]).current_dir(&t.0).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 2500));
    let v: Value = serde_json::from_slice(&r.stdout).unwrap();
    assert!(v["screens"]["screens"]["ProductList"]["label"].as_str().unwrap().ends_with(&nasty()[0]));
    std::fs::write(t.0.join("src").join("bin").join("probe.rs"), "fn main() { println!(\"{}\", geas_app::host::injected()); }\n").unwrap();
    let r = Command::new("cargo").args(["build", "--quiet", "--offline", "--bin", "probe"]).current_dir(&t.0).output().unwrap();
    assert!(!r.status.success() && String::from_utf8_lossy(&r.stderr).contains("injected")); // 差し込んだつもりの関数は、どこにも無い
}

/// 計画書の具体例の値に、Rust の文字列を抜けてファイルを書くコードを入れたもの。ID にも書式の文字（{}）と " を入れる
fn evil_plan() -> String {
    let plan = std::fs::read_to_string(shop_dir().join("shop_plan.md")).unwrap();
    let payload = "C9\".to_string() + &{ std::fs::write(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/PWNED\"), \"x\").ok(); String::new() } + &\"";
    assert!(plan.contains("顧客ID=C9,") && plan.contains("| EX3 |"));
    plan.replacen("顧客ID=C9,", &format!("顧客ID={},", payload), 1).replacen("| EX3 |", "| EX3\"{} |", 1)
}

#[test]
fn 計画書の具体例の値は生成したrustの文字列から出ない() {
    with_check(&std::fs::read_to_string(shop_dir().join("shop.geas")).unwrap(), true, |ck| {
        let rows = read_plan(&evil_plan())["具体例"].clone();
        let (src, _problems) = runner_source(ck, &rows);
        let line = src.lines().find(|x| x.contains("PWNED")).unwrap();
        let m = Regex::new(r#"customer_id: ""#).unwrap().find(line).unwrap();
        let (value, end) = literal_value(line, m.end() - 1);
        assert!(value.starts_with("C9\".to_string()") && line[end..].starts_with(".to_string(), items:"), "{}", line); // 値は文字列のまま、次の項目へ続く
        assert!(src.contains("println!(\"{}\\t") && src.contains("\"EX3\\\"{}\""), "{}", src); // 具体例の ID は、書式の文字列ではなく引数で渡る
    });
}

#[test]
fn 計画書の具体例の値を動かしてもコードとしては動かない() {
    let t = Tmp::new("evilplan");
    let crate_dir = t.0.join("crate");
    with_check(&std::fs::read_to_string(shop_dir().join("shop.geas")).unwrap(), true, |ck| {
        let rows = read_plan(&evil_plan())["具体例"].clone();
        let (src, _) = runner_source(ck, &rows);
        write_crate(ck, &generate(ck), &src, &crate_dir, false).unwrap();
    });
    let r = Command::new("cargo").args(["run", "--quiet", "--offline", "--bin", "plan_examples"]).current_dir(&crate_dir).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 2500));
    assert!(!crate_dir.join("PWNED").exists());
    assert!(String::from_utf8_lossy(&r.stdout).contains("EX3\"{}\tNoSuchCustomer")); // 長い ID の顧客はいない（値として比べられた）
}

#[test]
fn 前の版の指定がハイフンで始まるとgitを呼ばずに断る() {
    let t = Tmp::new("gitrev");
    for rev in ["--output=/tmp/x", "-p", ""] {
        let e = read_git(&t.0.join("a.geas"), rev).err().unwrap_or_else(|| panic!("断るはず: {}", rev));
        assert!(e.contains("前の版の指定"), "{}", e);
    }
}
