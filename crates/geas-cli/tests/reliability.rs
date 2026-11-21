// SPDX-License-Identifier: GPL-3.0-or-later
//! 言語機能ごとの動作テスト（信頼性）。途中の計算の trust の段階と、定義した項目を問い合わせの列に出すことを、生成した Rust で動かす（Python 版 tests/test_reliability.py の移し）。
//! どの機能にどのテストがあるかは docs/language-test-map.md。
mod common;
use common::*;

const TAIL: &str = r####"}
nexus Filling {
  put_tray: Done → reply, Refused → reply
}
"####;

const REL_SRC: &str = r####"ltype Quantity = int 0..1000
ltype Total = int 0..2000

entity Product {{
  product_id: id unique
  stock: Quantity required
  reserved: Quantity required
  cap: Quantity required
{derived}}}
{inv}
result StockResult {{
  Done
  NoProduct
  OverCap
  Short
}}
deed change_stock {{
  input product_id: id required
  input n: Quantity required
  reads {reads}
  writes {writes}
  result StockResult
  effects none

  let p = Product[product_id] else {{ return NoProduct }}
{body}  return Done
}}
nexus Changing {{
  change_stock: Done → reply, NoProduct → reply, OverCap → reply, Short → reply
}}
"####;

const HELD: &str = r####"  held: Total = stock + reserved
"####;

const ALL: &str = r####"Product.stock, Product.reserved, Product.cap"####;

const CALC_HEAD: &str = r####"ltype Wide = int 0..9000000000000000000
ltype Out = int 0..

entity Tray {
  tray_id: id unique
  n: Out required
}
result PutResult {
  Done(tray_id: id)
  Refused
}
deed put_tray {
  input a: Wide required
  input b: Wide required
  input c: Wide required
  writes Tray
  result PutResult
  effects none

"####;

const CALC_MAKE: &str = r####"  let t = create Tray { n: 1 }
  return Done(t.tray_id)
"####;

const HELD_QUERY: &str = r####"
query stock_list {
  from Product
  select product_id, stock, held
  order stock desc
  reads Product.product_id, Product.stock, Product.reserved
}
"####;

/// REL_SRC は Python の format 用（{body} など。波かっこは {{ }}）。同じ置き換えをする
fn rel(body: &str, inv: &str, derived: &str, reads: &str, writes: &str) -> String {
    REL_SRC.replace("{body}", body).replace("{inv}", inv).replace("{derived}", derived).replace("{reads}", reads).replace("{writes}", writes).replace("{{", "{").replace("}}", "}")
}

fn calc(level: &str) -> String {
    format!("{}  let x = a * b * c trust {} \"試験\"\n  if x > 5 {{ return Refused }}\n{}{}", CALC_HEAD, level, CALC_MAKE, TAIL)
}

#[test]
fn 途中の計算は段階どおりに動く() {
    // 9e18 を 3 つ掛けると、計算に使う幅（i128）を超える。check は異常で止まり、assume は確かめずに先へ進む。
    // verify は、検証ビルド（cargo run の既定）では check と同じに止まる
    let req = "r#\"{\"name\":\"put_tray\",\"ctx\":{\"new_Tray_id\":\"T\"},\"input\":{\"a\":9000000000000000000,\"b\":9000000000000000000,\"c\":9000000000000000000},\"store\":{}}\"#";
    let main = format!("use geas_app::host::call;\nfn main() {{\n    match call({}) {{ Ok(s) => println!(\"OK {{}}\", s), Err(e) => println!(\"ERR {{}}\", e) }}\n}}\n", req);
    let mut out = std::collections::HashMap::new();
    let mut dirs = vec![];
    for level in ["check", "verify", "assume"] {
        let t = Tmp::new(level);
        build_crate(&calc(level), &main, &t.0, true);
        out.insert(level, cargo_run(&t.0, false).join("\n"));
        dirs.push((level, t));
    }
    assert!(out["check"].starts_with("ERR") && out["check"].contains("あふれ"), "{:?}", out);
    assert_eq!(out["verify"], out["check"]);
    assert!(out["assume"].starts_with("OK"), "{:?}", out);
    // 通常ビルド（--release）の verify は確かめない：溢れた値（桁が回った値）のまま先へ進み、異常にも panic にもならない。check は通常ビルドでも止まる
    for (level, want) in [("verify", "OK"), ("check", "ERR")] {
        let t = &dirs.iter().find(|(l, _)| *l == level).unwrap().1;
        let got = cargo_run(&t.0, true).join("\n");
        assert!(got.starts_with(want), "{} {}", level, got);
    }
}

fn held_src() -> String {
    format!("{}{}", rel("  if p.held + n > p.cap { return OverCap }\n  p.stock += n\n", "invariant Product: held <= cap", HELD, ALL, "Product.stock"), HELD_QUERY)
}

#[test]
fn 定義した項目は問い合わせの列に出せて読むのは元の項目() {
    assert!(errors(&held_src()).is_empty(), "{:?}", errors(&held_src()));
    // 読む宣言には、定義の式の元の項目（stock と reserved）が要る
    let errs = errors(&held_src().replace(", Product.reserved", ""));
    assert!(errs.iter().any(|m| m.contains("「Product.reserved」を読むのに、宣言（読む）にありません")), "{:?}", errs);
}

#[test]
fn 定義した項目は条件や並びには書けない() {
    let errs = errors(&held_src().replace("order stock desc", "order held desc"));
    assert!(errs.iter().any(|m| m.contains("定義した項目「held」は、問い合わせでは select の列にだけ書けます")), "{:?}", errs);
}

#[test]
fn 問い合わせの答えに定義した項目の値が出る() {
    let store = "{\"Product\":{\"a\":{\"product_id\":\"a\",\"stock\":3,\"reserved\":4,\"cap\":10},\"b\":{\"product_id\":\"b\",\"stock\":600,\"reserved\":300,\"cap\":1000}}}";
    let r = format!("{{\"name\":\"stock_list\",\"ctx\":{{}},\"input\":{{}},\"store\":{}}}", store);
    let main = format!("use geas_app::host::call;\nfn main() {{\n    match call(r#\"{}\"#) {{ Ok(s) => println!(\"{{}}\", s), Err(e) => println!(\"ERR {{}}\", e) }}\n}}\n", r);
    let got = run_app_host(&held_src(), &main);
    let v: serde_json::Value = serde_json::from_str(&got[0]).unwrap_or_else(|e| panic!("{} {}", e, got[0]));
    let want: serde_json::Value = serde_json::json!([{"product_id": "b", "stock": 600, "held": 900}, {"product_id": "a", "stock": 3, "held": 7}]);
    assert_eq!(v["body"]["rows"], want, "{}", got[0]);
}
