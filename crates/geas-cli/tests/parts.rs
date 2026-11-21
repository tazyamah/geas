// SPDX-License-Identifier: GPL-3.0-or-later
//! 部品（library）の境界値の検査（geas parts）：宣言を、根の言語で実際に呼んで確かめる。
//! 標準部品（crates/geas-core/assets/std/num.geas）は、全部が組めて、panic せず、範囲に収まり、別に書いた手本（partrun.rs）と合うこと。
//! 元は Python 版の tests/test_parts.py と、test_geas.py の標準部品・library の項目（タグ python-final）。
mod common;
use common::*;
use geas_core::check::{defs::check, ERROR};
use geas_core::ast::Part;
use geas_core::parser::parse;
use geas_core::partcheck::fingerprint;
use geas_core::partrun::{check_parts, reference, V};
use geas_core::spec::build;
use indexmap::IndexMap;
use num_bigint::BigInt;
use serde_json::Value;
use std::collections::BTreeSet;
use std::process::Command;

fn num_path() -> std::path::PathBuf {
    root().join("crates").join("geas-core").join("assets").join("std").join("num.geas")
}

/// 標準部品の束を取り込んだプログラムの部品を、closure に渡す
fn with_std<R>(dir: &std::path::Path, f: impl FnOnce(&[Part]) -> R) -> R {
    let src = format!("use \"{}\"\n", num_path().to_string_lossy());
    let mut prog = parse(&src, Some(&dir.join("all.geas"))).unwrap_or_else(|e| panic!("{}", e));
    let ck = check(&mut prog, false);
    assert!(ck.ok(), "{:?}", ck.diags);
    f(&ck.p.parts)
}

fn refs(parts: &[Part]) -> Vec<&Part> {
    parts.iter().collect()
}

fn with_parts<R>(src: &str, f: impl FnOnce(&[Part]) -> R) -> R {
    let mut prog = parse(src, None).unwrap_or_else(|e| panic!("{}", e));
    let ck = check(&mut prog, false);
    f(&ck.p.parts)
}

#[test]
fn 標準部品は全部が組めて端の値でpanicせず範囲に収まり手本と合う() {
    let t = Tmp::new("stdparts");
    with_std(&t.0, |parts| {
        let rs = refs(parts);
        let rep = check_parts(&rs, &t.0.join("crate"), &reference(&rs)).unwrap();
        assert!(rep.ok(), "{:?}", rep.findings.iter().take(10).map(|f| (f.part.clone(), f.kind.clone(), f.detail.clone())).collect::<Vec<_>>());
        assert!(rep.skipped.is_empty(), "{:?}", rep.skipped);
        // 対応表：標準部品のどれにも手本がある（部品を足して手本を足し忘れると、ここで止まる）
        let names: BTreeSet<String> = parts.iter().map(|p| p.name.clone()).collect();
        assert_eq!(rep.calls.keys().cloned().collect::<BTreeSet<_>>(), names);
        assert_eq!(rep.compared, names);
    });
}

const BAD: &str = "ltype Count = int 0..100
ltype Small = int 0..9

library good(x: Count) -> Count from rust \"std\" \"\" call \"x / 2\" guarantee result <= x trust check \"半分にするだけ\"
example good(4) == 2
library overflow(x: Count) -> Count from rust \"std\" \"\" call \"x * 3\" trust check \"3 倍するだけ\"
example overflow(1) == 3
library too_wide(x: Count) -> Small from rust \"std\" \"\" call \"x / 2\" trust check \"半分にするだけ\"
example too_wide(4) == 2
library broken_promise(x: Count) -> Count from rust \"std\" \"\" call \"x / 2\" guarantee result >= x trust check \"半分にするだけ\"
example broken_promise(0) == 0
library no_such(x: Count) -> Count from rust \"std\" \"\" call \"x.no_such_method()\" trust check \"無い関数\"
example no_such(1) == 1
library by_zero(x: Count, y: Count) -> Count from rust \"std\" \"\" call \"x / y\" trust check \"割るだけ\"
example by_zero(4, 2) == 2
";

#[test]
fn 宣言と違う部品を種類ごとに見つける() {
    let t = Tmp::new("badparts");
    with_parts(BAD, |parts| {
        let rs = refs(parts);
        let rep = check_parts(&rs, &t.0, &IndexMap::new()).unwrap();
        let mut kinds: std::collections::HashMap<String, BTreeSet<String>> = Default::default();
        for f in &rep.findings {
            kinds.entry(f.part.clone()).or_default().insert(f.kind.clone());
        }
        let k = |n: &str| kinds.get(n).cloned().unwrap_or_default();
        let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<BTreeSet<String>>();
        assert!(!kinds.contains_key("good") && rep.calls["good"] > 0);
        assert!(k("overflow").contains("panic")); // 100 * 3 は u8 に入らない（デバッグビルドで呼ぶので、あふれが panic になる）
        assert!(k("overflow").contains("範囲外")); // 50 * 3 = 150 は u8 に入るが、宣言の 0..100 を出る
        assert_eq!(k("too_wide"), set(&["範囲外"])); // 100 / 2 = 50 は 0..9 を出る
        assert_eq!(k("broken_promise"), set(&["保証違反"]));
        assert!(k("no_such") == set(&["組めない"]) && !rep.calls.contains_key("no_such"));
        assert_eq!(k("by_zero"), set(&["panic"]));
        let detail = rep.findings.iter().find(|f| f.part == "by_zero").unwrap().detail.clone();
        assert!(detail.ends_with(",0)"), "{}", detail); // どの引数で起きたかが分かる
    });
}

#[test]
fn 標準部品の手本が違えば手本と違うと出る() {
    let t = Tmp::new("badref");
    with_std(&t.0, |parts| {
        let one: Vec<&Part> = parts.iter().filter(|p| p.name == "Num_u8_wrapping_add").collect();
        assert_eq!(one.len(), 1);
        let mut r: IndexMap<String, geas_core::partrun::RefFn> = IndexMap::new();
        // あふれを切り捨てない、誤った手本
        r.insert("Num_u8_wrapping_add".into(), Box::new(|a: &[V]| match (&a[0], &a[1]) {
            (V::I(x), V::I(y)) => V::I(x + y),
            _ => V::I(BigInt::from(0)),
        }));
        let rep = check_parts(&one, &t.0.join("crate"), &r).unwrap();
        let kinds: BTreeSet<String> = rep.findings.iter().map(|f| f.kind.clone()).collect();
        assert_eq!(kinds, ["手本と違う".to_string()].into_iter().collect());
        assert!(rep.findings.iter().any(|f| f.detail.contains("255,1")));
    });
}

fn geas(args: &[&str]) -> (i32, String, String) {
    let r = Command::new(env!("CARGO_BIN_EXE_geas")).env("GEAS_LANG", "ja").args(args).output().unwrap();
    (r.status.code().unwrap_or(-1), String::from_utf8_lossy(&r.stdout).to_string(), String::from_utf8_lossy(&r.stderr).to_string())
}

#[test]
fn partsコマンドは食い違いがあれば終了コード1() {
    let t = Tmp::new("partscmd");
    let src = t.0.join("bad.geas");
    std::fs::write(&src, BAD).unwrap();
    let s = src.to_string_lossy().to_string();
    let (rc, out, err) = geas(&["parts", &s, "--all"]);
    assert_eq!(rc, 1, "{}{}", out, err);
    assert!(out.contains("overflow：panic") && out.contains("食い違う部品が 5 個"), "{}", out);
    let (rc, out, _) = geas(&["parts", &s]);
    assert!(rc == 0 && out.contains("確かめる部品（library）がありません"), "{}", out); // 呼ばれていない部品は、--all を付けたときだけ
}

// ---- 台帳：検査の結果を、仕様書の「人の保証」に出す ----
const APP: &str = "ltype Count = int 0..100

entity Tray {
  tray_id: id unique
  n: Count required
}
result PutResult {
  Done(tray_id: id)
}
library half(x: Count) -> Count from rust \"std\" \"\" call \"x / 2\" guarantee result <= x trust check \"半分にするだけ\"
example half(10) == 5
deed put_tray {
  input n: Count required
  writes Tray
  result PutResult
  effects none

  let t = create Tray { n: half(n) }
  return Done(t.tray_id)
}
nexus Filling {
  put_tray: Done → reply
}
";

fn spec_row(src: &std::path::Path, needle: &str) -> String {
    let (_, out, _) = geas(&["spec", &src.to_string_lossy()]);
    out.lines().find(|l| l.contains(needle)).unwrap_or_else(|| panic!("{}", out)).to_string()
}

#[test]
fn 仕様書の人の保証に宣言を実際に呼んで確かめたかが出る() {
    let t = Tmp::new("ledger");
    let src = t.0.join("app.geas");
    let s = src.to_string_lossy().to_string();
    std::fs::write(&src, APP).unwrap();
    let row = || spec_row(&src, "ライブラリ half");
    assert!(row().contains("宣言は実際に呼んで確かめていない（geas parts --record）"));
    let (rc, out, err) = geas(&["parts", &s, "--record"]);
    assert!(rc == 0 && t.0.join("app.parts.json").exists(), "{}{}", out, err);
    assert!(row().contains("宣言は端の値で 14 回呼んで確かめた")); // 両端とその隣、0 の周り、真ん中、ビット幅の境目のうち、0..100 に入る 14 個
    // 宣言（範囲・式・guarantee）を変えたら、確かめ直すまで「通過」とは出さない。理由の文章だけの変更は、宣言の変更に数えない
    std::fs::write(&src, APP.replace("半分にするだけ", "2 で割るだけ")).unwrap();
    assert!(row().contains("宣言は端の値で 14 回呼んで確かめた"));
    std::fs::write(&src, APP.replace("call \"x / 2\"", "call \"x / 3\"")).unwrap();
    assert!(row().contains("宣言を変えたあと、確かめ直していない"));
    // 食い違いのある部品は、台帳に載らない
    std::fs::write(&src, APP.replace("guarantee result <= x", "guarantee result > x")).unwrap();
    assert_eq!(geas(&["parts", &s, "--record"]).0, 1);
    assert!(row().contains("宣言は実際に呼んで確かめていない"));
}

const STD_APP: &str = "use \"num.geas\"

ltype Small = int 0..255
ltype Qty = int 0..1000000

entity Item {
  item_id: id unique
  stock: Qty required
}
result PutResult {
  Put(item_id: id)
}
deed put_item {
  input a: Small required, b: Small required
  writes Item
  result PutResult
  effects none

  let item = create Item { stock: Num.u8_wrapping_add(a, b) }
  return Put(item.item_id)
}
nexus Flow {
  put_item: Put → reply
}
";

fn std_app_in(dir: &std::path::Path, src: &str) -> std::path::PathBuf {
    std::fs::copy(num_path(), dir.join("num.geas")).unwrap();
    let f = dir.join("app.geas");
    std::fs::write(&f, src).unwrap();
    f
}

fn std_check<R>(dir: &std::path::Path, src: &str, f: impl FnOnce(&geas_core::check::Checker) -> R) -> R {
    let p = std_app_in(dir, src);
    let mut prog = parse(src, Some(&p)).unwrap_or_else(|e| panic!("{}", e));
    let ck = check(&mut prog, false);
    f(&ck)
}

#[test]
fn 標準部品は同梱の台帳から確かめた内容が仕様書に出る() {
    let t = Tmp::new("stdledger");
    let ledger: Value = serde_json::from_str(&std::fs::read_to_string(root().join("crates/geas-core/assets/std/num.parts.json")).unwrap()).unwrap();
    let ledger = ledger["parts"].as_object().unwrap().clone();
    with_std(&t.0, |parts| {
        // 台帳は、いまの宣言の全部と合っている（num.geas を作り直して台帳を忘れると、ここで止まる）
        let a: std::collections::BTreeMap<String, String> = parts.iter().map(|p| (p.name.clone(), fingerprint(p))).collect();
        let b: std::collections::BTreeMap<String, String> = ledger.iter().map(|(n, r)| (n.clone(), r["fingerprint"].as_str().unwrap().to_string())).collect();
        assert_eq!(a, b);
        assert!(ledger.values().all(|r| r["reference"].as_bool().unwrap_or(false) && r["calls"].as_i64().unwrap_or(0) > 0));
    });
    let d = Tmp::new("stdledger2");
    let f = std_app_in(&d.0, STD_APP);
    let (_, out, _) = geas(&["spec", &f.to_string_lossy()]);
    let row = out.lines().find(|l| l.contains("u8_wrapping_add") && l.contains("ライブラリ")).unwrap_or_else(|| panic!("{}", out)).to_string();
    assert!( row.contains("宣言は全部の入力で証明した（Kani：式どおりで、panic しない）"), "{}", row);
}

// ---- 標準部品の束を取り込んで呼ぶ ----
#[test]
fn 標準部品の束を取り込んで呼べ呼んだものだけが人の保証に出る() {
    let t = Tmp::new("stdapp");
    std_check(&t.0, STD_APP, |ck| {
        assert!(!ck.diags.iter().any(|d| d.level == ERROR), "{:?}", ck.diags);
        let spec = build(ck);
        let rows = spec["人の保証"].as_array().unwrap();
        // 束に 200 以上あっても、呼んだ1つだけ
        assert_eq!(rows.iter().map(|r| r[0].as_str().unwrap().to_string()).collect::<Vec<_>>(), vec!["（ライブラリ Num.u8_wrapping_add）"], "{:?}", rows);
        assert!(rows[0][1].as_str().unwrap().contains("std") && rows[0][1].as_str().unwrap().contains("wrapping_add"));
        assert_eq!(spec["部品"].as_array().unwrap().len(), 1);
        assert!(!geas_core::rustgen::generate(ck).contains("u8_wrapping_mul"));
    });
}

#[test]
fn 標準部品はrustの型の範囲を引数に課し戻り値の範囲もrustの型から決まる() {
    let t = Tmp::new("stdrange");
    let wide = STD_APP.replace("ltype Small = int 0..255", "ltype Small = int 0..256");
    std_check(&t.0, &wide, |ck| assert!(ck.diags.iter().any(|d| d.level == ERROR && d.msg.contains("範囲")))); // u8 に収まると示せない値は渡せない
    let big = STD_APP.replace("ltype Qty = int 0..1000000", "ltype Qty = int 0..100");
    std_check(&t.0, &big, |ck| assert!(ck.diags.iter().any(|d| d.level == ERROR && d.msg.contains("収まると示せません")))); // 戻り値 0..255 は 0..100 に収まらない
}

#[test]
fn 標準部品は生成物の中で実際に動く() {
    let t = Tmp::new("stdrun");
    let main = r##"use geas_app::host::call;
fn main() { println!("{:?}", call(r#"{"name":"put_item","ctx":{"new_Item_id":"A"},"input":{"a":200,"b":100},"store":{}}"#)); }
"##;
    let p = std_app_in(&t.0, STD_APP);
    let mut prog = parse(STD_APP, Some(&p)).unwrap();
    let ck = check(&mut prog, false);
    let crate_dir = t.0.join("crate");
    geas_core::crategen::write_crate(&ck, &geas_core::rustgen::generate(&ck), main, &crate_dir, true).unwrap();
    let out = cargo_run(&crate_dir, false);
    let joined = out.join("\n");
    assert!(joined.starts_with("Ok(") && joined.contains("\\\"stock\\\":44"), "{}", joined); // 200u8 の wrapping_add(100) は 44
}

// ---- 具体例（example）と、call の式に書けないもの ----
fn lib(call: &str) -> String {
    format!("ltype Count = int 0..100\nlibrary half(x: Count) -> Count from rust \"std\" \"\" call \"{}\" trust check \"半分にするだけ\"\n", call)
}

fn any(v: &[String], needle: &str) -> bool {
    v.iter().any(|m| m.contains(needle))
}

#[test]
fn libraryには具体例が要る() {
    assert!(any(&errors_loose(&lib("x / 2")), "library には具体例が要ります"));
    assert!(errors_loose(&(lib("x / 2") + "example half(10) == 5\n")).is_empty());
}

#[test]
fn 具体例の値は宣言の型に合わせる() {
    let base = lib("x / 2");
    assert!(any(&errors_loose(&(base.clone() + "example half(10, 2) == 5\n")), "引数は 2 個ですが、宣言は 1 個"));
    assert!(errors_loose(&(base.clone() + "example half(200) == 5\n")).iter().any(|m| m.contains("範囲") && m.contains("の外")));
    assert!(errors_loose(&(base.clone() + "example half(10) == \"5\"\n")).iter().any(|m| m.contains("数ではない型") || m.contains("文字列ではない型")));
    assert!(parse_err(&(base.clone() + "example nothing(1) == 1\n")).unwrap().contains("library がありません"));
    assert!(parse_err(&(base + "example half(5 + 5) == 5\n")).unwrap().contains("直に書いた値"));
}

#[test]
fn callの式には繰り返しを書けない() {
    for (call, word) in [
        ("{ let mut n = x; loop { n += 1; } }", "loop"),
        ("{ let mut n = x; while n > 0 { n -= 1; } n }", "while"),
        ("{ let mut n = 0; for i in 0..x { n += i; } n }", "for"),
        ("(0..x).map(|i| i).sum()", "クロージャ"),
        ("{ fn f(n: u8) -> u8 { f(n) } f(x) }", "fn"),
    ] {
        let msgs = errors_loose(&(lib(call) + "example half(10) == 5\n"));
        assert!(msgs.iter().any(|m| m.contains("call の式に") && m.contains(word) && m.contains("書けません")), "{}: {:?}", call, msgs);
    }
}

#[test]
fn ビットの演算の縦棒はクロージャと取り違えない() {
    assert!(errors_loose(&(lib("(x | 1) & 100") + "example half(10) == 11\n")).is_empty());
}

#[test]
fn 具体例と違う答えはpartsが見つける() {
    let t = Tmp::new("exdiff");
    let src = lib("x / 2") + "example half(10) == 5\nexample half(10) == 6\n";
    with_parts(&src, |parts| {
        let rs = refs(parts);
        let rep = check_parts(&rs, &t.0, &IndexMap::new()).unwrap();
        assert_eq!(rep.findings.iter().map(|f| (f.part.clone(), f.kind.clone())).collect::<Vec<_>>(), vec![("half".to_string(), "具体例と違う".to_string())]);
        assert_eq!(rep.examples.get("half"), Some(&1));
        assert!(rep.findings[0].detail.contains("half(10) = 5（具体例は 6）"), "{}", rep.findings[0].detail);
    });
}

#[test]
fn 文字列の部品は具体例だけを確かめる() {
    let t = Tmp::new("strparts");
    let src = "ltype Word = text 0..10\nlibrary shout(x: Word) -> Word from rust \"std\" \"\" call \"x.to_uppercase()\" trust check \"大文字にするだけ\"\nexample shout(\"abc\") == \"ABC\"\nexample shout(\"\") == \"\"\n";
    assert!(errors_loose(src).is_empty());
    with_parts(src, |parts| {
        let rs = refs(parts);
        let rep = check_parts(&rs, &t.0, &IndexMap::new()).unwrap();
        assert!(rep.ok() && rep.examples.get("shout") == Some(&2) && rep.skipped.contains_key("shout"), "{:?}", rep.findings.len());
    });
}

#[test]
fn 台帳は具体例を変えたら確かめ直しを求める() {
    let t = Tmp::new("exledger");
    let src = t.0.join("app.geas");
    let s = src.to_string_lossy().to_string();
    std::fs::write(&src, APP).unwrap();
    let row = || spec_row(&src, "ライブラリ half");
    assert!(row().contains("具体例: half(10) == 5"));
    assert_eq!(geas(&["parts", &s, "--record"]).0, 0);
    assert!(row().contains("具体例 1 件と一致"));
    std::fs::write(&src, APP.replace("example half(10) == 5", "example half(10) == 5\nexample half(8) == 4")).unwrap();
    let r = row();
    assert!(r.contains("具体例は確かめ直していない") && r.contains("宣言は端の値で 14 回呼んで確かめた"), "{}", r);
}
